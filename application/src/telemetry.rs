//! Command telemetry with the semantics of `application/telemetry/observe.mjs`: one
//! trace per invocation, operations nested by explicit parent, local batches under the
//! shared store lock. Remote export is returned to the caller; until bounded HTTP moves
//! to Rust, the JS forwarder posts those events.
use craft_engine::{contracts::js_length, telemetry, timestamp};
use craft_platform::telemetry as store;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

const PACKAGE: &str = include_str!("../../package.json");
const EXPORT_EVENTS: usize = 32;

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as i64)
}

/// The installed pack version, which `package.json` owns; `None` when unreadable.
pub fn pack_version() -> Option<String> {
    let version = serde_json::from_str::<Value>(PACKAGE).ok()?["version"]
        .as_str()?
        .to_string();
    let parts: Vec<_> = version.split('.').collect();
    (parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())))
    .then_some(version)
}

pub fn valid_session(value: &str) -> bool {
    (1..=512).contains(&js_length(value))
}

/// Accepted remote endpoints: HTTPS, or HTTP to a loopback name, without credentials,
/// query or fragment. Stricter than WHATWG parsing; the forwarder normalizes the URL.
fn endpoint(text: &str) -> Option<String> {
    let (scheme, rest) = text.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    let (authority, path) = rest.find('/').map_or((rest, ""), |at| rest.split_at(at));
    let (host, port) = match authority.strip_prefix('[') {
        Some(inner) => {
            let (host, after) = inner.split_once(']')?;
            (format!("[{host}]"), after.strip_prefix(':'))
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
    let port_ok = port.is_none_or(|p| p.is_empty() || p.parse::<u16>().is_ok());
    let path_ok = path
        .bytes()
        .all(|b| b.is_ascii_graphic() && !b"\\?#".contains(&b));
    let local = ["localhost", "127.0.0.1", "[::1]"].contains(&host.as_str());
    (host_ok
        && port_ok
        && path_ok
        && !authority.contains('@')
        && (scheme == "https" || scheme == "http" && local))
        .then(|| text.to_string())
}

/// `telemetryOptions`: `{local, remote}` where remote is the accepted endpoint, if enabled.
fn options(config: &Value) -> Result<(bool, Option<String>), ()> {
    let map = config.as_object().ok_or(())?;
    if map.keys().any(|k| k != "local" && k != "remote")
        || map.get("local").is_some_and(|l| !l.is_boolean())
    {
        return Err(());
    }
    let remote = match map.get("remote") {
        None => None,
        Some(remote) => {
            let remote = remote.as_object().ok_or(())?;
            if remote.keys().any(|k| k != "enabled" && k != "endpoint") {
                return Err(());
            }
            match remote.get("enabled") {
                Some(Value::Bool(true)) => Some(
                    endpoint(remote.get("endpoint").and_then(Value::as_str).ok_or(())?).ok_or(())?,
                ),
                Some(Value::Bool(false)) => None,
                _ => return Err(()),
            }
        }
    };
    Ok((map.get("local") != Some(&Value::Bool(false)), remote))
}

pub struct Operation {
    id: String,
    name: &'static str,
    parent: Option<String>,
    started: i64,
}

pub struct Recorder {
    enabled: bool,
    root: PathBuf,
    local: bool,
    remote: Option<String>,
    session: Option<String>,
    host: Option<String>,
    /// The capability package a host hook belongs to, recorded on every event.
    package: Option<String>,
    version: Option<String>,
    trace: String,
    session_id: Option<String>,
    identity_unavailable: bool,
    /// Startup diagnostics, reported once against the first event.
    pending: Vec<&'static str>,
    batch: Vec<Value>,
    export: Vec<Value>,
    local_drops: u64,
    queue_drops: u64,
}

fn compact_uuid() -> String {
    store::uuid().unwrap_or_default().replace('-', "")
}

impl Recorder {
    pub fn open(root: &Path, session: Option<&str>, host: Option<&str>) -> Self {
        let root = store::root(root);
        let (local, remote, invalid) = match store::configuration(&root) {
            Ok(config) => match options(&config) {
                Ok((local, remote)) => (local, remote, false),
                Err(()) => (config.get("local") != Some(&Value::Bool(false)), None, true),
            },
            Err(_) => (true, None, true),
        };
        let version = pack_version();
        let mut pending = Vec::new();
        if invalid {
            pending.push("TELEMETRY_CONFIG_INVALID");
        }
        if version.is_none() {
            pending.push("TELEMETRY_METADATA_UNAVAILABLE");
        }
        Self {
            enabled: local || remote.is_some(),
            root,
            local,
            remote,
            session: session.filter(|s| valid_session(s)).map(str::to_string),
            host: host.map(str::to_string),
            package: None,
            version,
            trace: compact_uuid(),
            session_id: None,
            identity_unavailable: false,
            pending,
            batch: Vec::new(),
            export: Vec::new(),
            local_drops: 0,
            queue_drops: 0,
        }
    }

    /// Adopt an identity the command learns from its input: a session when none is valid
    /// yet (later events correlate to it) and a host when none was given.
    pub fn enrich(&mut self, session: Option<&str>, host: Option<&str>) {
        if self.session.is_none()
            && let Some(session) = session.filter(|s| valid_session(s))
        {
            self.session = Some(session.to_string());
            self.session_id = None;
            self.identity_unavailable = false;
            self.pending
                .retain(|reason| *reason != "TELEMETRY_IDENTITY_UNAVAILABLE");
        }
        if self.host.is_none() {
            self.host = host.map(str::to_string);
        }
    }

    /// Attribute events to a capability package; the event envelope keeps only known names.
    pub fn package(&mut self, name: Option<&str>) {
        self.package = name.map(str::to_string);
    }

    pub fn start(&self, name: &'static str, parent: Option<&Operation>) -> Operation {
        Operation {
            id: store::uuid().unwrap_or_default(),
            name,
            parent: parent.map(|p| p.id.clone()),
            started: now_ms(),
        }
    }

    fn correlation(&mut self) -> String {
        if let Some(id) = &self.session_id {
            return id.clone();
        }
        let subject = self
            .session
            .clone()
            .unwrap_or_else(|| store::uuid().unwrap_or_default());
        let id = store::identity(&self.root, &subject).unwrap_or_else(|_| {
            self.identity_unavailable = true;
            self.pending.push("TELEMETRY_IDENTITY_UNAVAILABLE");
            compact_uuid()
        });
        self.session_id = Some(id.clone());
        id
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        &mut self,
        name: &str,
        mut attributes: Map<String, Value>,
        outcome: Option<&str>,
        duration: Option<i64>,
        operation_id: String,
        parent: Option<String>,
        reason: Option<&str>,
        operation: bool,
        skills: &[&str],
    ) -> Result<Value, String> {
        let session_id = self.correlation();
        let scope = if self.session.is_some() && !self.identity_unavailable {
            "session"
        } else {
            "invocation"
        };
        if let Some(package) = &self.package {
            attributes.insert("package".into(), json!(package));
        }
        attributes.insert("identityScope".into(), json!(scope));
        telemetry::envelope(
            &telemetry::Event {
                name,
                timestamp: timestamp::format(now_ms()),
                session_id: &session_id,
                trace_id: &self.trace,
                operation_id,
                parent_operation_id: parent,
                pack_version: self.version.as_deref(),
                host: self.host.as_deref(),
                attributes,
                outcome,
                duration_ms: duration,
                reason_code: reason,
                operation,
            },
            skills,
        )
    }

    /// Diagnostics bypass the batch and export queue and never diagnose their own failure.
    fn diagnostic(
        &mut self,
        reason: &'static str,
        attributes: Map<String, Value>,
        parent: Option<String>,
    ) -> bool {
        if !self.local {
            return false;
        }
        let mut attributes = attributes;
        attributes.insert("area".into(), json!("telemetry"));
        let id = store::uuid().unwrap_or_default();
        match self.build(
            "diagnostic",
            attributes,
            Some("failure"),
            None,
            id,
            parent,
            Some(reason),
            false,
            &[],
        ) {
            Ok(event) => store::append(&self.root, &[event]),
            Err(_) => false,
        }
    }

    pub fn complete(
        &mut self,
        operation: Operation,
        attributes: Map<String, Value>,
        failure: Option<&'static str>,
    ) {
        if !self.enabled {
            return;
        }
        let duration = (now_ms() - operation.started).max(0);
        let outcome = if failure.is_some() {
            "failure"
        } else {
            "success"
        };
        let event = self.build(
            operation.name,
            attributes,
            Some(outcome),
            Some(duration),
            operation.id.clone(),
            operation.parent,
            failure,
            true,
            &[],
        );
        self.record(event, operation.id);
    }

    /// A domain diagnostic under `parent`, stored and exported like operation events.
    pub fn report(
        &mut self,
        area: &'static str,
        reason: &'static str,
        mut attributes: Map<String, Value>,
        parent: &Operation,
    ) {
        if !self.enabled {
            return;
        }
        attributes.insert("area".into(), json!(area));
        let id = store::uuid().unwrap_or_default();
        let event = self.build(
            "diagnostic",
            attributes,
            Some("failure"),
            None,
            id.clone(),
            Some(parent.id.clone()),
            Some(reason),
            false,
            &[],
        );
        self.record(event, id);
    }

    /// An observation under `parent`; a `skill` attribute is kept only when `skills` lists it.
    pub fn observe(
        &mut self,
        name: &'static str,
        attributes: Map<String, Value>,
        parent: &Operation,
        skills: &[&str],
    ) {
        if !self.enabled {
            return;
        }
        let id = store::uuid().unwrap_or_default();
        let event = self.build(
            name,
            attributes,
            Some("success"),
            None,
            id.clone(),
            Some(parent.id.clone()),
            None,
            false,
            skills,
        );
        self.record(event, id);
    }

    /// An observation under `parent` that carries its own outcome and duration.
    pub fn observed(
        &mut self,
        name: &'static str,
        attributes: Map<String, Value>,
        parent: &Operation,
        outcome: &'static str,
        duration: i64,
    ) {
        if !self.enabled {
            return;
        }
        let id = store::uuid().unwrap_or_default();
        let event = self.build(
            name,
            attributes,
            Some(outcome),
            Some(duration.max(0)),
            id.clone(),
            Some(parent.id.clone()),
            None,
            false,
            &[],
        );
        self.record(event, id);
    }

    fn record(&mut self, event: Result<Value, String>, id: String) {
        for reason in std::mem::take(&mut self.pending) {
            self.diagnostic(reason, Map::new(), Some(id.clone()));
        }
        let Ok(event) = event else {
            self.diagnostic("TELEMETRY_EVENT_INVALID", Map::new(), Some(id));
            return;
        };
        if self.remote.is_some() {
            if self.export.len() < EXPORT_EVENTS {
                self.export.push(event.clone());
            } else {
                self.queue_drops += 1;
            }
        }
        if self.local {
            self.batch.push(event);
            if self.batch.len() >= store::BATCH_EVENTS {
                self.flush();
            }
        }
    }

    fn flush(&mut self) {
        if self.batch.is_empty() {
            return;
        }
        let events: Vec<_> = self
            .batch
            .drain(..self.batch.len().min(store::BATCH_EVENTS))
            .collect();
        if !store::append(&self.root, &events) {
            self.local_drops += events.len() as u64;
        }
    }

    /// Store remaining events and return the remote export, if one is configured.
    pub fn finish(mut self) -> Option<Value> {
        if !self.enabled {
            return None;
        }
        while !self.batch.is_empty() {
            self.flush();
        }
        for (reason, count) in [
            ("TELEMETRY_EVENTS_DROPPED", self.local_drops),
            ("EXPORT_QUEUE_FULL", self.queue_drops),
        ] {
            if count > 0 {
                let attributes = Map::from_iter([(
                    "droppedEvents".to_string(),
                    json!(count.min(1_000_000_000)),
                )]);
                self.diagnostic(reason, attributes, None);
            }
        }
        let endpoint = self.remote.take()?;
        (!self.export.is_empty()).then(|| {
            json!({
                "endpoint": endpoint,
                "root": self.root.to_string_lossy(),
                "session": self.session,
                "host": self.host,
                "package": self.package,
                "events": self.export,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_endpoints_are_https_or_loopback_http_without_credentials() {
        for text in [
            "https://collector.example/v1/traces",
            "HTTPS://Collector.Example:4318",
            "http://localhost:4318/v1/traces",
            "http://127.0.0.1",
            "http://[::1]:4318/",
        ] {
            assert_eq!(endpoint(text).as_deref(), Some(text), "{text}");
        }
        for text in [
            "http://collector.example",
            "https://user@collector.example",
            "https://collector.example/?a",
            "https://collector.example/#x",
            "ftp://collector.example",
            "https://",
            "https://collector.example:99999",
            "https://exa mple",
            "collector.example",
        ] {
            assert_eq!(endpoint(text), None, "{text}");
        }
        assert_eq!(options(&json!({})), Ok((true, None)));
        assert_eq!(
            options(&json!({"local": false, "remote": {"enabled": false}})),
            Ok((false, None))
        );
        assert_eq!(
            options(&json!({"remote": {"enabled": true, "endpoint": "https://c.example"}})),
            Ok((true, Some("https://c.example".into())))
        );
        for config in [
            json!([]),
            json!({"local": "no"}),
            json!({"other": 1}),
            json!({"remote": {"enabled": "yes"}}),
            json!({"remote": {"enabled": true}}),
            json!({"remote": {"enabled": true, "endpoint": "http://c.example"}}),
        ] {
            assert_eq!(options(&config), Err(()), "{config}");
        }
        assert!(pack_version().is_some());
    }
}
