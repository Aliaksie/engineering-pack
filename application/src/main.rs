use serde_json::json;
use std::io::{self, Read};

fn main() {
    // `craft <command> [options]` for people; no arguments reads one JSON envelope on stdin.
    let argv: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    if !argv.is_empty() {
        let outcome = craft_application::cli::run(&argv);
        print!("{}", outcome.stdout);
        eprint!("{}", outcome.stderr);
        std::process::exit(outcome.exit_code);
    }
    let mut input = Vec::new();
    let response = match io::stdin()
        .take(3 * 1024 * 1024 + 1)
        .read_to_end(&mut input)
    {
        Ok(_) if input.len() <= 3 * 1024 * 1024 => match serde_json::from_slice(&input) {
            Ok(request) => craft_application::evaluate(request),
            Err(error) => {
                json!({"formatVersion":1,"ok":false,"error":{"code":"INVALID_INPUT","message":error.to_string()}})
            }
        },
        _ => {
            json!({"formatVersion":1,"ok":false,"error":{"code":"INVALID_INPUT","message":"request exceeds input bound or could not be read"}})
        }
    };
    println!("{response}");
    if response["ok"] != true {
        std::process::exit(1);
    }
}
