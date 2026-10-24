use serde_json::json;
use std::io::{self, Read};

fn main() {
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
