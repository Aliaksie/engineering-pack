use super::*;

fn rules(found: &[Value]) -> Vec<&str> {
    found.iter().map(|f| f["rule"].as_str().unwrap()).collect()
}

#[test]
fn changes_number_added_lines_in_the_new_file() {
    let diff = "diff --git a/x.rs b/x.rs\n--- a/x.rs\n+++ b/x.rs\n@@ -3 +3,2 @@ fn a()\n-    old();\n+    new();\n+    more();\n@@ -10,0 +12 @@\n+last\n\\ No newline at end of file\n";
    let (added, removed) = changes(diff);
    assert_eq!(
        added,
        vec![
            (3, "    new();".to_owned()),
            (4, "    more();".to_owned()),
            (12, "last".to_owned())
        ]
    );
    assert_eq!(removed, vec!["    old();".to_owned()]);
    // File headers before the first hunk are not lines.
    assert_eq!(changes("+++ b/x.rs\n--- a/x.rs\n"), (vec![], vec![]));
}

#[test]
fn markers_report_only_what_the_change_adds() {
    let added = |lines: &[&str]| -> Vec<(usize, String)> {
        lines
            .iter()
            .enumerate()
            .map(|(i, l)| (i + 1, (*l).to_owned()))
            .collect()
    };
    let found = markers(
        &added(&[
            "    // TODO: handle the empty case",
            "# FIXME later",
            "    let todo = TODO_LIST; // ordinary code",
            "  // eslint-disable-next-line no-console",
            "#[allow(dead_code)]",
            "it.skip('flaky', () => {})",
            "    let rest = items.iter().skip(1);",
            "@Disabled(\"slow\")",
        ]),
        &[],
    );
    assert_eq!(
        rules(&found),
        vec![
            "todo-marker",
            "todo-marker",
            "suppression",
            "suppression",
            "skipped-test",
            "skipped-test"
        ]
    );
    assert_eq!(found[0]["line"], 1);
    assert!(
        found[0]["message"]
            .as_str()
            .unwrap()
            .starts_with("adds a TODO comment")
    );
    // An identifier or a word inside one is not a marker; a marker outside a comment neither.
    assert!(
        markers(
            &added(&["let TODOS = 1;", "call(XXX_LIMIT)", "TODO()"]),
            &[]
        )
        .is_empty()
    );
}

#[test]
fn fewer_assertions_than_removed_is_a_finding() {
    let removed = vec![
        "    assert_eq!(a, 1);".to_owned(),
        "    expect(b).toBe(2);".to_owned(),
        "    // assert later".to_owned(),
    ];
    let found = markers(&[(5, "    assert!(a > 0);".to_owned())], &removed);
    assert_eq!(rules(&found), vec!["removed-assertion"]);
    assert_eq!(
        found[0]["message"],
        "removes 2 assertion line(s) and adds 1"
    );
    // Rewriting an assertion keeps the count.
    let rewritten = markers(&[(5, "    assert_eq!(a, 2);".to_owned())], &removed[..1]);
    assert!(rewritten.is_empty());
}

#[test]
fn stop_speaks_once_per_finding_and_review_item() {
    let finding = json!({"file": "a.rs", "rule": "todo-marker", "line": 3, "message": "adds a TODO comment: // TODO"});
    let review = json!({"id": "api-review", "level": "review", "checklist": ["versioned"]});
    let first = stop(
        std::slice::from_ref(&finding),
        std::slice::from_ref(&review),
        &[],
    )
    .unwrap();
    assert_eq!(first.summary["findings"], json!([finding]));
    assert_eq!(first.summary["review"], json!([review]));
    assert_eq!(first.keys.len(), 2);
    let told = delivered(&[], &first.keys);
    // Same finding on another line, review already shown: quiet.
    let moved = json!({"file": "a.rs", "rule": "todo-marker", "line": 9, "message": "adds a TODO comment: // TODO"});
    assert_eq!(
        stop(
            std::slice::from_ref(&moved),
            std::slice::from_ref(&review),
            &told
        ),
        None
    );
    // A new finding brings back every current one, but not the review item.
    let other = json!({"file": "b.rs", "rule": "suppression", "line": 1, "message": "adds a #[allow( suppression"});
    let second = stop(&[moved.clone(), other.clone()], &[review], &told).unwrap();
    assert_eq!(second.summary["findings"], json!([moved, other]));
    assert!(second.summary.get("review").is_none());
    assert_eq!(stop(&[], &[], &[]), None);
}

#[test]
fn delivered_keys_stay_bounded_newest_last() {
    let earlier: Vec<String> = (0..MAX_DELIVERED).map(|i| i.to_string()).collect();
    let keys = delivered(&earlier, &["0".to_owned(), "new".to_owned()]);
    assert_eq!(keys.len(), MAX_DELIVERED);
    assert_eq!(keys.last().unwrap(), "new");
    assert_eq!(keys[keys.len() - 2], "0");
    assert!(!keys.contains(&"1".to_owned()));
}
