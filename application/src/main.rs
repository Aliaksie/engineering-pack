use serde_json::json;
use std::io::{self, IsTerminal, Read, Write};

fn main() {
    // `craft <command> [options]` for people; no arguments reads one JSON envelope on stdin.
    let argv: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    // A host hook reads the host's own payload on stdin and writes its wire response.
    if argv.first().is_some_and(|command| command == "hook") {
        std::process::exit(craft_application::hook::main(&argv[1..]));
    }
    // A host status line reads the host's own payload on stdin and writes one line.
    if argv.first().is_some_and(|command| command == "statusline") {
        std::process::exit(craft_application::statusline::main(&argv[1..]));
    }
    // A person who runs it without a command gets the usage, not a wait for a request.
    if !argv.is_empty() || io::stdin().is_terminal() {
        std::process::exit(craft_application::cli::main(&argv));
    }
    let mut input = Vec::new();
    let mut ok = false;
    let mut write = |response: &serde_json::Value| {
        println!("{response}");
        let _ = io::stdout().flush();
        ok = response["ok"] == true;
    };
    match io::stdin()
        .take(3 * 1024 * 1024 + 1)
        .read_to_end(&mut input)
    {
        Ok(_) if input.len() <= 3 * 1024 * 1024 => match serde_json::from_slice(&input) {
            Ok(request) => craft_application::serve(request, write),
            Err(error) => write(
                &json!({"formatVersion":1,"ok":false,"error":{"code":"INVALID_INPUT","message":error.to_string()}}),
            ),
        },
        _ => write(
            &json!({"formatVersion":1,"ok":false,"error":{"code":"INVALID_INPUT","message":"request exceeds input bound or could not be read"}}),
        ),
    }
    if !ok {
        std::process::exit(1);
    }
}
