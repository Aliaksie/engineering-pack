//! Work that writes documents, not code: its prompt hint leaves the run and commit gates to
//! the edit, and the first edit of each file it writes lists the standards covering that file.
mod support;
use serde_json::{Value, json};
use support::Repo;

const PROJECT: &str = r#"{"formatVersion":1,
  "standards":[{"id":"commits","anchor":"conventional-commits"},
    {"id":"fast-gate","run":{"command":"git","args":["--version"],"stage":"pr"}},
    {"id":"stories","file":"docs/story-rules.md","scope":{"paths":["docs/stories"],"intents":["story"]}}]}"#;

/// What the hook added to the model's context for `lifecycle`, empty when nothing.
fn said(repo: &Repo, lifecycle: &str, extra: Value) -> String {
    let mut payload = json!({"session_id": "S1", "cwd": repo.path()});
    for (key, value) in extra.as_object().unwrap() {
        payload[key] = value.clone();
    }
    let args: Vec<String> = [
        "claude",
        lifecycle,
        "--package",
        "craft-delivery",
        "--handler",
        "deliver",
    ]
    .iter()
    .map(|a| a.to_string())
    .collect();
    let (wire, _) = craft_application::hook::run(&args, &mut payload.to_string().as_bytes());
    assert_eq!(wire.exit_code, 0, "{lifecycle}: {}", wire.stderr);
    let output: Value = serde_json::from_str(&wire.stdout).unwrap_or_default();
    output["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

fn edit(repo: &Repo, file: &str) -> String {
    let path = std::path::Path::new(repo.path()).join(file);
    let call = json!({"tool_name": "Write",
        "tool_input": {"file_path": path, "content": "As a shopper…\n"}});
    said(repo, "before-tool", call)
}

#[test]
fn story_work_hears_its_gates_at_the_edit_of_the_document() {
    let repo = Repo::new(
        "document-work",
        &[
            (".craft/project.json", PROJECT),
            ("docs/story-rules.md", "Stories follow INVEST.\n"),
        ],
    );
    said(&repo, "session-start", json!({"source": "startup"}));
    let prompt = said(
        &repo,
        "before-prompt",
        json!({"prompt": "Write user stories for the checkout page"}),
    );
    assert!(prompt.contains("stories"), "{prompt}");
    assert!(!prompt.contains("commits"), "{prompt}");
    assert!(!prompt.contains("fast-gate"), "{prompt}");

    let first = edit(&repo, "docs/stories/checkout.md");
    for standard in ["commits", "fast-gate", "stories"] {
        assert!(first.contains(standard), "{standard}: {first}");
    }
    assert_eq!(
        edit(&repo, "docs/stories/checkout.md"),
        "",
        "routed once per file"
    );

    // A review, which may end in a commit, lists its gates at the prompt as before.
    let review = said(
        &repo,
        "before-prompt",
        json!({"prompt": "Review the checkout change before we commit it"}),
    );
    assert!(review.contains("commits"), "{review}");
    assert!(review.contains("fast-gate"), "{review}");
}
