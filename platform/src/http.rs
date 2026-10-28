//! One bounded JSON POST to an accepted telemetry endpoint: HTTPS through rustls with the
//! operating system verifying each certificate chain, or plain HTTP to a loopback name. Transport only; the
//! caller interprets the observation.
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

pub const MAX_REQUEST: usize = 262_144;
pub const MAX_RESPONSE: usize = 16_384;
/// Status line and headers are bounded separately from the body.
const MAX_HEAD: usize = 16_384;

#[derive(Debug, PartialEq)]
enum Failure {
    Transport,
    Timeout,
    TooLarge,
}

struct Target {
    tls: bool,
    host: String,
    port: u16,
    authority: String,
    path: String,
}

/// The one accepted telemetry endpoint form: HTTPS, or HTTP to a loopback name, with an
/// optional port and path, and no credentials, query or fragment. Scheme and host are
/// case-insensitive; an empty port means the scheme default.
fn target(endpoint: &str) -> Option<Target> {
    let (scheme, rest) = endpoint.split_once("://")?;
    let tls = match scheme.to_ascii_lowercase().as_str() {
        "https" => true,
        "http" => false,
        _ => return None,
    };
    let (authority, path) = rest.find('/').map_or((rest, ""), |at| rest.split_at(at));
    let (host, port) = match authority.strip_prefix('[') {
        Some(inner) => {
            let (host, tail) = inner.split_once(']')?;
            (format!("[{host}]"), tail.strip_prefix(':'))
        }
        None => match authority.rsplit_once(':') {
            Some((host, port)) => (host.to_string(), Some(port)),
            None => (authority.to_string(), None),
        },
    };
    let host = host.to_ascii_lowercase();
    let host_ok = host == "[::1]"
        || (!host.is_empty()
            && host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-'));
    let port = match port.filter(|port| !port.is_empty()) {
        Some(port) if port.bytes().all(|b| b.is_ascii_digit()) => Some(port.parse::<u16>().ok()?),
        Some(_) => return None,
        None => None,
    };
    let path_ok = path
        .bytes()
        .all(|b| b.is_ascii_graphic() && !b"\\?#".contains(&b));
    let loopback = ["localhost", "127.0.0.1", "[::1]"].contains(&host.as_str());
    if !host_ok || !path_ok || authority.contains('@') || !(tls || loopback) {
        return None;
    }
    let authority = match port {
        Some(port) => format!("{host}:{port}"),
        None => host.clone(),
    };
    Some(Target {
        tls,
        port: port.unwrap_or(if tls { 443 } else { 80 }),
        host: host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_string(),
        authority,
        path: if path.is_empty() { "/" } else { path }.to_string(),
    })
}

/// `text` normalized as transport posts to it (lower-case scheme and host, explicit port
/// only when given, `/` for an empty path), or `None` when it is not an accepted endpoint.
/// Configuration keeps this value, so every endpoint it accepts is one transport can use.
pub fn endpoint(text: &str) -> Option<String> {
    let target = target(text)?;
    let scheme = if target.tls { "https" } else { "http" };
    Some(format!("{scheme}://{}{}", target.authority, target.path))
}

fn connect(target: &Target, deadline: Instant) -> Result<TcpStream, Failure> {
    let addresses = (target.host.as_str(), target.port)
        .to_socket_addrs()
        .map_err(|_| Failure::Transport)?;
    for address in addresses {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(Failure::Timeout);
        }
        if let Ok(stream) = TcpStream::connect_timeout(&address, left) {
            return Ok(stream);
        }
    }
    Err(Failure::Transport)
}

/// The platform verifier asks the OS per chain instead of loading its whole store.
fn tls_config() -> Result<Arc<rustls::ClientConfig>, Failure> {
    use rustls_platform_verifier::BuilderVerifierExt;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|_| Failure::Transport)?
        .with_platform_verifier()
        .map_err(|_| Failure::Transport)?
        .with_no_client_auth();
    Ok(Arc::new(config))
}

/// Read until the peer closes or the bounded head and body are complete.
fn exchange(stream: &mut dyn ReadWrite, request: &[u8]) -> Result<(u16, Vec<u8>), Failure> {
    stream.write_all(request).map_err(io)?;
    stream.flush().map_err(io)?;
    let mut buffer = Vec::new();
    let mut chunk = [0; 4096];
    loop {
        if let Some((status, body)) = response(&buffer)? {
            return Ok((status, body));
        }
        let read = match stream.read(&mut chunk) {
            Ok(read) => read,
            // A peer that closes without TLS close_notify still delivered what it sent.
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => 0,
            Err(error) => return Err(io(error)),
        };
        if read == 0 {
            return complete(&buffer);
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() > MAX_HEAD + MAX_RESPONSE + 4096 {
            return Err(Failure::TooLarge);
        }
    }
}

trait ReadWrite: Read + Write {}
impl<T: Read + Write> ReadWrite for T {}

fn io(error: std::io::Error) -> Failure {
    match error.kind() {
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => Failure::Timeout,
        _ => Failure::Transport,
    }
}

struct Head {
    status: u16,
    length: Option<usize>,
    chunked: bool,
    body: usize,
}

fn head(buffer: &[u8]) -> Result<Option<Head>, Failure> {
    let Some(end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") else {
        return if buffer.len() > MAX_HEAD {
            Err(Failure::Transport)
        } else {
            Ok(None)
        };
    };
    let text = std::str::from_utf8(&buffer[..end]).map_err(|_| Failure::Transport)?;
    let mut lines = text.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.strip_prefix("HTTP/1."))
        .and_then(|line| line.get(2..5))
        .and_then(|code| code.parse().ok())
        .ok_or(Failure::Transport)?;
    let mut length = None;
    let mut chunked = false;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            length = Some(value.parse().map_err(|_| Failure::Transport)?);
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            chunked = value.to_ascii_lowercase().contains("chunked");
        }
    }
    Ok(Some(Head {
        status,
        length,
        chunked,
        body: end + 4,
    }))
}

/// A finished response, if the buffer already holds one.
fn response(buffer: &[u8]) -> Result<Option<(u16, Vec<u8>)>, Failure> {
    let Some(head) = head(buffer)? else {
        return Ok(None);
    };
    let body = &buffer[head.body..];
    if head.chunked {
        return Ok(dechunk(body)?.map(|body| (head.status, body)));
    }
    match head.length {
        Some(length) if length > MAX_RESPONSE => Err(Failure::TooLarge),
        Some(length) if body.len() >= length => Ok(Some((head.status, body[..length].to_vec()))),
        _ if body.len() > MAX_RESPONSE => Err(Failure::TooLarge),
        _ => Ok(None),
    }
}

/// The response at end of stream: a body without a length runs to the close.
fn complete(buffer: &[u8]) -> Result<(u16, Vec<u8>), Failure> {
    let head = head(buffer)?.ok_or(Failure::Transport)?;
    if head.chunked || head.length.is_some() {
        return Err(Failure::Transport);
    }
    Ok((head.status, buffer[head.body..].to_vec()))
}

/// The decoded body once the terminating chunk arrived.
fn dechunk(mut body: &[u8]) -> Result<Option<Vec<u8>>, Failure> {
    let mut decoded = Vec::new();
    loop {
        let Some(line) = body.windows(2).position(|w| w == b"\r\n") else {
            return Ok(None);
        };
        let size = std::str::from_utf8(&body[..line])
            .ok()
            .and_then(|text| usize::from_str_radix(text.split(';').next()?.trim(), 16).ok())
            .ok_or(Failure::Transport)?;
        if size == 0 {
            return Ok(Some(decoded));
        }
        if decoded.len() + size > MAX_RESPONSE {
            return Err(Failure::TooLarge);
        }
        let start = line + 2;
        if body.len() < start + size + 2 {
            return Ok(None);
        }
        decoded.extend_from_slice(&body[start..start + size]);
        body = &body[start + size + 2..];
    }
}

fn send(endpoint: &str, body: &[u8], timeout: Duration) -> Result<(u16, Vec<u8>), Failure> {
    let deadline = Instant::now() + timeout;
    let target = target(endpoint).ok_or(Failure::Transport)?;
    let mut stream = connect(&target, deadline)?;
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(Failure::Timeout);
    }
    stream.set_read_timeout(Some(left)).map_err(io)?;
    stream.set_write_timeout(Some(left)).map_err(io)?;
    let mut request = format!(
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        target.path,
        target.authority,
        body.len()
    )
    .into_bytes();
    request.extend_from_slice(body);
    if !target.tls {
        return exchange(&mut stream, &request);
    }
    let name = rustls::pki_types::ServerName::try_from(target.host.clone())
        .map_err(|_| Failure::Transport)?;
    let connection =
        rustls::ClientConnection::new(tls_config()?, name).map_err(|_| Failure::Transport)?;
    let mut tls = rustls::StreamOwned::new(connection, stream);
    exchange(&mut tls, &request)
}

/// POST `body` and describe what happened, as the transport observation
/// `{ok, statusCode?, json?, failure?}`: `ok` is a 200 response with a JSON body.
/// Name resolution has no timeout of its own, so the whole exchange runs on a thread
/// that is abandoned at the deadline.
pub fn post(endpoint: &str, body: &[u8], timeout: Duration) -> Value {
    if body.len() > MAX_REQUEST {
        return json!({"ok": false, "failure": "request-too-large"});
    }
    let (sender, receiver) = mpsc::channel();
    let endpoint = endpoint.to_string();
    let body = body.to_vec();
    std::thread::spawn(move || {
        let _ = sender.send(send(&endpoint, &body, timeout));
    });
    match receiver.recv_timeout(timeout) {
        Ok(Ok((status, body))) => match serde_json::from_slice::<Value>(&body) {
            Ok(value) => json!({"ok": status == 200, "statusCode": status, "json": value}),
            Err(_) => json!({"ok": false, "statusCode": status, "failure": "invalid-response"}),
        },
        Ok(Err(Failure::TooLarge)) => json!({"ok": false, "failure": "response-too-large"}),
        Ok(Err(Failure::Timeout)) | Err(_) => json!({"ok": false, "failure": "timeout"}),
        Ok(Err(Failure::Transport)) => json!({"ok": false, "failure": "transport"}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// A one-shot loopback server that answers with `reply` and returns the request.
    fn serve(reply: &'static [u8], delay: Duration) -> (String, std::thread::JoinHandle<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 4096];
            while let Ok(read) = stream.read(&mut chunk) {
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);
                if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let text = String::from_utf8_lossy(&request[..end]).to_lowercase();
                    let length: usize = text
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length: "))
                        .and_then(|v| v.trim().parse().ok())
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            std::thread::sleep(delay);
            let _ = stream.write_all(reply);
            request
        });
        (format!("http://127.0.0.1:{port}/v1/traces"), handle)
    }

    #[test]
    fn a_json_response_reports_its_status_and_body() {
        let (url, server) = serve(
            b"HTTP/1.1 200 OK\r\nContent-Length: 18\r\n\r\n{\"partialSuccess\":1",
            Duration::ZERO,
        );
        // The declared length cuts the body before the closing brace: invalid JSON.
        assert_eq!(
            post(&url, b"{}", Duration::from_secs(2)),
            json!({"ok": false, "statusCode": 200, "failure": "invalid-response"})
        );
        let request = String::from_utf8(server.join().unwrap()).unwrap();
        assert!(
            request.starts_with("POST /v1/traces HTTP/1.1\r\n"),
            "{request}"
        );
        assert!(request.contains("Content-Type: application/json\r\n"));
        assert!(request.ends_with("\r\n\r\n{}"));
        let (url, _server) = serve(
            b"HTTP/1.1 202 Accepted\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{}\r\n0\r\n\r\n",
            Duration::ZERO,
        );
        assert_eq!(
            post(&url, b"{}", Duration::from_secs(2)),
            json!({"ok": false, "statusCode": 202, "json": {}})
        );
        let (url, _server) = serve(b"HTTP/1.1 200 OK\r\n\r\n{\"a\":1}", Duration::ZERO);
        assert_eq!(
            post(&url, b"{}", Duration::from_secs(2)),
            json!({"ok": true, "statusCode": 200, "json": {"a": 1}})
        );
    }

    #[test]
    fn bounds_and_failures_are_reported_without_a_response() {
        assert_eq!(
            post(
                "http://127.0.0.1:9/",
                &vec![b' '; MAX_REQUEST + 1],
                Duration::from_secs(1)
            ),
            json!({"ok": false, "failure": "request-too-large"})
        );
        let (url, _server) = serve(
            b"HTTP/1.1 200 OK\r\nContent-Length: 20000\r\n\r\n",
            Duration::ZERO,
        );
        assert_eq!(
            post(&url, b"{}", Duration::from_secs(2)),
            json!({"ok": false, "failure": "response-too-large"})
        );
        let (url, _server) = serve(b"HTTP/1.1 200 OK\r\n\r\n{}", Duration::from_millis(600));
        assert_eq!(
            post(&url, b"{}", Duration::from_millis(200)),
            json!({"ok": false, "failure": "timeout"})
        );
        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}/", closed.local_addr().unwrap().port());
        drop(closed);
        // Windows retries a refused loopback connection for about two seconds.
        assert_eq!(
            post(&url, b"{}", Duration::from_secs(5)),
            json!({"ok": false, "failure": "transport"})
        );
    }

    /// Network-dependent: a public HTTPS server verified by the operating system.
    #[test]
    #[ignore = "requires outbound HTTPS"]
    fn https_is_verified_by_the_operating_system() {
        let result = post("https://example.com/", b"{}", Duration::from_secs(5));
        assert!(result["statusCode"].is_u64(), "{result}");
        let result = post(
            "https://self-signed.badssl.com/",
            b"{}",
            Duration::from_secs(5),
        );
        assert_eq!(result, json!({"ok": false, "failure": "transport"}));
    }

    #[test]
    fn endpoints_resolve_to_host_port_and_path() {
        let parsed = target("https://collector.example/v1/traces").unwrap();
        assert_eq!(
            (
                parsed.tls,
                parsed.host.as_str(),
                parsed.port,
                parsed.path.as_str()
            ),
            (true, "collector.example", 443, "/v1/traces")
        );
        let parsed = target("http://[::1]:4318").unwrap();
        assert_eq!(
            (
                parsed.tls,
                parsed.host.as_str(),
                parsed.port,
                parsed.authority.as_str(),
                parsed.path.as_str()
            ),
            (false, "::1", 4318, "[::1]:4318", "/")
        );
        assert!(target("ftp://x").is_none());
        assert!(target("http://:80/").is_none());
    }

    #[test]
    fn every_accepted_endpoint_is_one_transport_can_post_to() {
        for (text, normalized) in [
            (
                "https://collector.example/v1/traces",
                "https://collector.example/v1/traces",
            ),
            (
                "HTTPS://Collector.Example:4318",
                "https://collector.example:4318/",
            ),
            ("https://collector.example:", "https://collector.example/"),
            (
                "https://collector.example:0443/v1",
                "https://collector.example:443/v1",
            ),
            (
                "http://localhost:4318/v1/traces",
                "http://localhost:4318/v1/traces",
            ),
            ("HTTP://LocalHost", "http://localhost/"),
            ("http://127.0.0.1", "http://127.0.0.1/"),
            ("http://[::1]:4318/", "http://[::1]:4318/"),
        ] {
            assert_eq!(endpoint(text).as_deref(), Some(normalized), "{text}");
            assert!(target(normalized).is_some(), "{normalized}");
            assert_eq!(
                endpoint(normalized).as_deref(),
                Some(normalized),
                "{normalized}"
            );
        }
        for text in [
            "http://collector.example",
            "https://user@collector.example",
            "https://collector.example/?a",
            "https://collector.example/#x",
            "ftp://collector.example",
            "https://",
            "https://collector.example:99999",
            "https://collector.example:+443",
            "https://exa mple",
            "https://[2001:db8::1]/",
            "collector.example",
        ] {
            assert_eq!(endpoint(text), None, "{text}");
        }
    }
}
