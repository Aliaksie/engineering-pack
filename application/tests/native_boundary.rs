use serde_json::{Value, json};
use std::io::Write;
use std::process::{Command, Stdio};

fn invoke(binary: &str, input: &[u8]) -> (bool, Value) {
    let mut child = Command::new(binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    // Oversized input may close the pipe as soon as the bound is reached.
    let _ = child.stdin.take().unwrap().write_all(input);
    let output = child.wait_with_output().unwrap();
    (
        output.status.success(),
        serde_json::from_slice(&output.stdout).unwrap(),
    )
}

#[test]
fn primary_and_compatibility_executables_share_the_versioned_boundary() {
    for binary in [
        env!("CARGO_BIN_EXE_craft"),
        env!("CARGO_BIN_EXE_craft-project-evidence"),
    ] {
        for (input, code) in [
            (b"{".to_vec(), "INVALID_INPUT"),
            (vec![b' '; 3 * 1024 * 1024 + 1], "INVALID_INPUT"),
            (
                br#"{"formatVersion":2,"operation":"version"}"#.to_vec(),
                "UNSUPPORTED_VERSION",
            ),
            (
                br#"{"formatVersion":1,"operation":"missing"}"#.to_vec(),
                "UNKNOWN_OPERATION",
            ),
            (b"null".to_vec(), "UNSUPPORTED_VERSION"),
        ] {
            let (success, response) = invoke(binary, &input);
            assert!(!success);
            assert_eq!(response["formatVersion"], 1);
            assert_eq!(response["error"]["code"], code);
        }
        let input = json!({"formatVersion":1,"operation":"extract","input":{
            "path":"sample.js","source":"export function greet() { return 1; }"
        }});
        let (success, response) = invoke(binary, &serde_json::to_vec(&input).unwrap());
        assert!(success, "{response}");
        assert_eq!(response["ok"], true);
        assert!(response["result"].to_string().contains("greet"));
    }
}
