use super::*;
use serde_json::json;

fn project() -> Value {
    json!({"modules": [
        {"name": "core", "path": "src/core", "languages": ["rust"]},
        {"name": "app", "path": "src/app"}
    ]})
}

fn added(lines: &[&str]) -> Vec<(usize, String)> {
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| (index + 1, (*line).to_owned()))
        .collect()
}

fn facts_of(file: &str, add: &[&str], remove: &[&str]) -> Vec<&'static str> {
    let added = added(add);
    let removed: Vec<String> = remove.iter().map(|line| (*line).to_owned()).collect();
    facts(
        &project(),
        &Change {
            file,
            lines: Some((&added, &removed)),
            created: false,
            new_folder: false,
            imports: &[],
            committed: false,
        },
    )
}

#[test]
fn tests_are_known_by_their_folder_or_name() {
    for test in [
        "tests/a.rs",
        "engine/src/routing/tests.rs",
        "src/__tests__/a.ts",
        "a.test.ts",
        "a.spec.js",
        "pkg/a_test.go",
        "test_a.py",
        "conftest.py",
        "src/OrderTest.java",
        "src/OrderTests.cs",
    ] {
        assert!(test_file(test), "{test}");
    }
    for source in ["src/a.rs", "src/contest.py", "Test.java", "src/latest.ts"] {
        assert!(!test_file(source), "{source}");
    }
}

#[test]
fn lines_tell_behavior_from_prose() {
    assert_eq!(
        facts_of("src/app/a.ts", &["const a = 1"], &[]),
        vec!["behavior"]
    );
    assert_eq!(
        facts_of("src/app/a.ts", &[], &["const a = 1"]),
        vec!["behavior"]
    );
    assert_eq!(
        facts_of("src/app/a.ts", &["// why", "  "], &[]),
        vec!["prose"]
    );
    assert!(facts_of("src/app/a.test.ts", &["expect(a)"], &["expect(b)"]).is_empty());
    assert_eq!(
        facts_of("src/app/a.test.ts", &["// why"], &[]),
        vec!["prose"],
        "a comment in a test is still prose"
    );
    // In a document a heading is content, not a comment.
    assert_eq!(
        facts_of("docs/a.md", &["# Title"], &[]),
        vec!["behavior", "prose"]
    );
    assert!(facts_of("src/app/a.ts", &[], &["// gone"]).is_empty());
}

#[test]
fn unknown_lines_and_session_commits_count_as_holding() {
    let change = |lines, committed| Change {
        file: "src/app/a.ts",
        lines,
        created: false,
        new_folder: false,
        imports: &[],
        committed,
    };
    assert_eq!(
        facts(&project(), &change(None, false)),
        vec!["behavior", "prose"]
    );
    assert_eq!(
        facts(&project(), &change(Some((&[], &[])), true)),
        vec!["prose"]
    );
}

#[test]
fn structure_is_a_new_folder_a_cross_module_import_or_a_foreign_language() {
    let structure = |file: &str, created: bool, new_folder: bool, imports: &[Import]| {
        facts(
            &project(),
            &Change {
                file,
                lines: Some((&[], &[])),
                created,
                new_folder,
                imports,
                committed: false,
            },
        )
        .contains(&"structure")
    };
    let import = |resolved: &str| Import {
        line: 1,
        target: "x".into(),
        resolved: Some(resolved.into()),
    };
    assert!(structure(
        "src/app/a.ts",
        false,
        false,
        &[import("src/core/lib.rs")]
    ));
    assert!(!structure(
        "src/app/a.ts",
        false,
        false,
        &[import("src/app/b.ts")]
    ));
    assert!(
        !structure("tests/a.ts", false, false, &[import("src/core/lib.rs")]),
        "a file outside the modules crosses no boundary"
    );
    assert!(structure("tools/a.ts", true, true, &[]));
    assert!(
        structure("src/core/a.ts", true, false, &[]),
        "TypeScript in a Rust module"
    );
    assert!(!structure("src/core/a.rs", true, false, &[]));
    assert!(
        !structure("src/core/a.ts", false, false, &[]),
        "already there"
    );
}
