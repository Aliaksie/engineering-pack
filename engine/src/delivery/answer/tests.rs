use super::*;

#[test]
fn only_a_complete_bounded_result_is_worth_a_further_lookup() {
    let row = json!({"path": "src/a.rs", "line": 3, "excerpt": "a()"});
    let result = |complete: bool, rows: Vec<Value>| json!({"route": {"answeredBy": "intelligence", "complete": complete}, "results": rows, "continuation": null});
    assert!(answerable(&result(true, vec![row.clone()])));
    assert!(!answerable(&result(false, vec![row.clone()])));
    assert!(!answerable(&result(true, vec![])));
    assert!(!answerable(&result(true, vec![row; ROWS + 1])));
}

#[test]
fn a_lookup_prompt_names_one_question_and_one_code_name() {
    let eval = "A teammate wants to change the signature of `DiscountPolicy.applyDiscount` in this\nrepository and asked which methods call it directly.\n\nWrite the answer to `ANSWER.md` at the repository root, one caller per line, as\n`<path from the repository root>#<method name>`, for example\n`src/main/java/com/acme/billing/Example.java#run`. Do not change any other file.";
    assert_eq!(
        question(eval),
        Some(("callers", "DiscountPolicy.applyDiscount".into()))
    );
    for (prompt, expected) in [
        (
            "Who calls prompt::request?",
            Some(("callers", "prompt::request")),
        ),
        (
            "who calls `apply_discount()`",
            Some(("callers", "apply_discount")),
        ),
        (
            "Where is applyDiscount defined?",
            Some(("declaration", "applyDiscount")),
        ),
        (
            "Which files use `AdrSlug`, apart from the file that declares it?",
            None,
        ),
        ("Where is `Checkout` defined and where is it used?", None),
        (
            "Where is `Checkout` declared?",
            Some(("declaration", "Checkout")),
        ),
        (
            "Which methods call `Policy.apply`?",
            Some(("callers", "Policy.apply")),
        ),
        ("Who calls Order#total?", Some(("callers", "Order#total"))),
        ("Who calls the code in `orders.py`?", None),
        // A word in a sentence is not a symbol; neither is a file or a version.
        ("Who calls Checkout?", None),
        ("Who calls the code in src/order.js?", None),
        ("Where is v2.3 defined?", None),
        // Both questions, neither, or two names ask nothing.
        ("Where is `run` defined and who calls it?", None),
        ("Where is `run` used?", None),
        ("Who calls `run` or `start`?", None),
    ] {
        assert_eq!(
            question(prompt),
            expected.map(|(ask, name)| (ask, name.to_string())),
            "{prompt}"
        );
    }
}

#[test]
fn decision_key_and_mention_prompts_name_their_question() {
    for (prompt, expected) in [
        (
            "Which architecture decision record is the current decision on how an ADR is uniquely identified?",
            Some(("decisions", "*")),
        ),
        (
            "Which ADRs govern the React code?",
            Some(("decisions", "*")),
        ),
        ("What did we decide about caching?", None),
        (
            "Which ADR decides the cache policy?",
            Some(("decisions", "*")),
        ),
        (
            "Which file models an ADR's status in the core package?",
            None,
        ),
        (
            "Where is `compilerOptions.strict` set?",
            Some(("key", "compilerOptions.strict")),
        ),
        (
            "Which file configures jobs.build.runs-on?",
            Some(("key", "jobs.build.runs-on")),
        ),
        ("Where is config.json set?", None),
        (
            "Which docs mention `AdrSlug`?",
            Some(("mentions", "AdrSlug")),
        ),
        (
            "Which documents link to src/hono.ts?",
            Some(("mentions", "src/hono.ts")),
        ),
        // A document question without a name, or with two, asks nothing.
        (
            "Which documentation page explains how to configure connection pool limits?",
            None,
        ),
        ("Which docs mention `a_b` and `c_d`?", None),
        // A symbol question keeps its meaning beside document words.
        (
            "Where is `parseArgs` defined in the docs?",
            Some(("declaration", "parseArgs")),
        ),
    ] {
        assert_eq!(
            question(prompt),
            expected.map(|(ask, name)| (ask, name.to_string())),
            "{prompt}"
        );
    }
}

#[test]
fn decisions_and_keys_answer_with_status_and_value() {
    let decisions: Vec<Value> = (0..12)
        .map(|n| json!({"path": format!("docs/adr/{n}.md"), "line": 1, "kind": "md:decision", "name": format!("Use {n}"), "target": "accepted", "excerpt": "# Use"}))
        .collect();
    let answer = answered("decisions", "*", &result(decisions.clone(), None), None).unwrap();
    assert_eq!(answer["rows"][0], "docs/adr/0.md:1: Use 0 [accepted]");
    assert_eq!(answer["rows"].as_array().unwrap().len(), 12);
    let many: Vec<Value> = decisions
        .iter()
        .cycle()
        .take(DECISION_ROWS + 1)
        .cloned()
        .collect();
    assert!(answered("decisions", "*", &result(many, None), None).is_none());
    // Records under test or fixture directories are test data: left out, and counted.
    let fixtures: Vec<Value> = (0..DECISION_ROWS)
        .map(|n| {
            let directory = ["packages/core/integration-tests/p/docs/adr", "tests/fixtures/adr", "src/__tests__/adr"][n % 3];
            json!({"path": format!("{directory}/{n}.md"), "line": 1, "kind": "md:decision", "name": format!("Fixture {n}"), "target": "accepted"})
        })
        .chain(decisions.iter().cloned())
        .collect();
    let answer = answered("decisions", "*", &result(fixtures, None), None).unwrap();
    assert_eq!(answer["rows"].as_array().unwrap().len(), 12);
    assert_eq!(
        answer["leftOut"],
        "24 records under test or fixture directories"
    );
    assert!(
        answered("decisions", "*", &result(decisions.clone(), None), None)
            .unwrap()
            .get("leftOut")
            .is_none()
    );
    assert!(!under_tests("docs/adr/testing-strategy.md"));
    assert!(!under_tests("tests.md"));
    assert!(under_tests("Tests/adr/x.md"));
    let key = json!({"path": "tsconfig.json", "line": 8, "kind": "config:key", "name": "compilerOptions.strict", "target": "true", "excerpt": "\"strict\": true,", "container": {"name": "compilerOptions"}});
    assert_eq!(
        answered("key", "strict", &result(vec![key], None), None).unwrap()["rows"][0],
        "tsconfig.json:8 in compilerOptions: compilerOptions.strict: \"strict\": true,"
    );
    // Symbol answers keep their row bound.
    let rows = vec![row("docs/a.md", 1, Some("Use")); ROWS + 1];
    assert!(answered("mentions", "x", &result(rows, None), None).is_none());
}

#[test]
fn pasted_material_does_not_name_the_lookup() {
    let prompt = "Who calls `apply`?\n\nlog: `retry_once` failed, see `Worker.run`";
    assert_eq!(question(prompt), Some(("callers", "apply".into())));
}

fn row(path: &str, line: u64, container: Option<&str>) -> Value {
    let mut row = json!({"path": path, "line": line, "excerpt": "  x = policy.apply(1);  "});
    if let Some(name) = container {
        row["container"] = json!({"name": name});
    }
    row
}

fn result(rows: Vec<Value>, qualifier: Option<&str>) -> Value {
    let mut route = json!({"answeredBy": "intelligence", "complete": true, "scopes": ["."]});
    if let Some(qualifier) = qualifier {
        route["qualifier"] = json!(qualifier);
    }
    json!({"results": rows, "continuation": null, "route": route})
}

#[test]
fn callers_answer_only_for_one_declaration_named_by_the_qualifier() {
    let callers = result(
        vec![row("src/Checkout.java", 14, Some("finish"))],
        Some("Policy"),
    );
    let one = result(vec![row("src/Policy.java", 5, None)], Some("Policy"));
    let answer = answered("callers", "Policy.apply", &callers, Some(&one)).unwrap();
    assert_eq!(
        answer["rows"],
        json!(["src/Checkout.java:14 in finish: x = policy.apply(1);"])
    );
    assert_eq!(answer["question"], "callers Policy.apply");
    // Two declarations: the rows may call either one.
    let two = result(
        vec![
            row("src/Policy.java", 5, None),
            row("src/Other.java", 3, None),
        ],
        Some("Policy"),
    );
    assert_eq!(
        answered("callers", "Policy.apply", &callers, Some(&two)),
        None
    );
    // The one declaration lives elsewhere: the qualifier may name an outside type.
    let elsewhere = result(vec![row("src/Other.java", 3, None)], Some("Policy"));
    assert_eq!(
        answered("callers", "Policy.apply", &callers, Some(&elsewhere)),
        None
    );
    assert_eq!(answered("callers", "Policy.apply", &callers, None), None);
    // A Rust module path names its file or its folder's mod.rs.
    let unqualified = result(
        vec![row("a/b.rs", 1, None)],
        Some("engine::delivery::prompt"),
    );
    let module = result(vec![row("src/prompt/mod.rs", 9, None)], None);
    assert!(answered("callers", "prompt::request", &unqualified, Some(&module)).is_some());
}

#[test]
fn answers_state_what_their_rows_rest_on() {
    let kind = |mut row: Value, kind: &str| {
        row["kind"] = json!(kind);
        row
    };
    let declaration = result(
        vec![kind(
            row("src/Policy.java", 5, None),
            "java:declares-method",
        )],
        None,
    );
    let callers = result(
        vec![kind(
            row("src/A.java", 1, Some("run")),
            "java:invokes-syntax",
        )],
        None,
    );
    let parsed = answered("declaration", "apply", &declaration, None).unwrap();
    assert_eq!(
        (&parsed["basis"], &parsed["use"]),
        (&json!("parsed"), &json!(USE))
    );
    let named = usages("apply", &callers, &declaration).unwrap();
    assert_eq!(named["basis"], "name");
    assert!(named["use"].as_str().unwrap().ends_with(BY_NAME));
    assert_eq!(
        named["rows"],
        json!(["src/A.java:1 in run: x = policy.apply(1);"])
    );
    // Only a mixed answer marks its name rows; a row of unknown kind is one of them.
    let mentions = result(
        vec![
            kind(row("docs/a.md", 2, None), "md:link"),
            kind(row("docs/a.md", 4, None), "md:mention"),
            row("docs/b.md", 1, None),
        ],
        None,
    );
    let mixed = answered("mentions", "apply", &mentions, None).unwrap();
    assert_eq!(mixed["basis"], "mixed");
    assert_eq!(
        mixed["rows"],
        json!([
            "docs/a.md:2: x = policy.apply(1);",
            "docs/a.md:4: x = policy.apply(1); [by name]",
            "docs/b.md:1: x = policy.apply(1); [by name]"
        ])
    );
}

#[test]
fn incomplete_or_oversized_answers_abstain() {
    let one = result(vec![row("src/Policy.java", 5, None)], None);
    let callers = result(vec![row("src/A.java", 1, None)], None);
    assert!(answered("callers", "apply", &callers, Some(&one)).is_some());
    let mut partial = callers.clone();
    partial["route"]["complete"] = json!(false);
    let mut literal = callers.clone();
    literal["route"]["answeredBy"] = json!("locate");
    let mut gaps = callers.clone();
    gaps["route"]["answeredBy"] = json!("intelligence+locate");
    gaps["gapSearch"] = json!({"matches": [{"path": "web/a.js", "line": 2}]});
    let mut paged = callers.clone();
    paged["continuation"] = json!({"after": "0".repeat(64)});
    let empty = result(vec![], None);
    let many = result(
        (0..=ROWS as u64)
            .map(|n| row("src/A.java", n + 1, None))
            .collect(),
        None,
    );
    for (name, case) in [
        ("partial", &partial),
        ("literal", &literal),
        ("gaps", &gaps),
        ("paged", &paged),
        ("empty", &empty),
        ("many", &many),
    ] {
        assert_eq!(
            answered("callers", "apply", case, Some(&one)),
            None,
            "{name}"
        );
    }
    let mut clean_gaps = gaps.clone();
    clean_gaps["gapSearch"] = json!({"matches": []});
    assert!(answered("callers", "apply", &clean_gaps, Some(&one)).is_some());
    let mut long = row("src/A.java", 1, Some("run"));
    long["excerpt"] = json!("y".repeat(400));
    let rows = (0..ROWS).map(|_| long.clone()).collect();
    assert_eq!(
        answered("callers", "apply", &result(rows, None), Some(&one)),
        None
    );
}

#[test]
fn declarations_keep_the_rows_the_qualifier_names() {
    let both = result(
        vec![
            row("src/Policy.java", 5, None),
            row("src/Other.java", 3, None),
        ],
        Some("Policy"),
    );
    let answer = answered("declaration", "Policy.apply", &both, None).unwrap();
    assert_eq!(answer["rows"].as_array().unwrap().len(), 1);
    let none = result(vec![row("src/Other.java", 3, None)], Some("Policy"));
    assert_eq!(answered("declaration", "Policy.apply", &none, None), None);
    assert!(
        answered(
            "declaration",
            "apply",
            &result(vec![row("a.rs", 1, None)], None),
            None
        )
        .is_some()
    );
}

#[test]
fn a_search_names_one_literal_identifier() {
    let grep = |pattern| searched(Some(pattern), None);
    assert_eq!(grep("applyDiscount"), Some("applyDiscount".into()));
    assert_eq!(grep("\\bapply_discount\\b"), Some("apply_discount".into()));
    assert_eq!(grep("applyDiscount\\("), Some("applyDiscount".into()));
    assert_eq!(grep("Policy.apply"), Some("Policy.apply".into()));
    assert_eq!(grep("crate::route"), Some("crate::route".into()));
    for regex in [
        "apply.*Discount",
        "foo|bar",
        "ab",
        "config.json",
        "x y",
        "^main",
    ] {
        assert_eq!(grep(regex), None, "{regex}");
    }
    let shell = |command| searched(None, Some(command));
    assert_eq!(
        shell("rg -n applyDiscount src"),
        Some("applyDiscount".into())
    );
    assert_eq!(
        shell("cd app && rg -n 'applyDiscount' -g '*.java' | head -20"),
        Some("applyDiscount".into())
    );
    assert_eq!(
        shell("grep -rn --include=*.py apply_rate ."),
        Some("apply_rate".into())
    );
    assert_eq!(
        shell("git grep -n -e TaxRate -- src"),
        Some("TaxRate".into())
    );
    assert_eq!(
        shell("rg -A 3 -t java applyDiscount"),
        Some("applyDiscount".into())
    );
    assert_eq!(
        shell("grep -r applyDiscount src"),
        Some("applyDiscount".into()),
        "grep -r takes no value"
    );
    for none in [
        "rg -v applyDiscount",
        "grep -rv applyDiscount .",
        "rg -e one -e two",
        "rg -f patterns.txt",
        "rg 'apply.*'",
        "rg applyDiscount && grep total src",
        "cat a.txt",
        "rg 'unterminated",
        "ls | wc -l",
        "copilot --help | grep -iA3 \"model\"",
        "cargo tree | rg serde_json",
        "git log --oneline | grep -e applyDiscount",
    ] {
        assert_eq!(shell(none), None, "{none}");
    }
    assert_eq!(
        shell("git ls-files | rg applyDiscount src"),
        Some("applyDiscount".into()),
        "a piped search with a file operand still reads the repository"
    );
    assert_eq!(
        shell("false || rg applyDiscount"),
        Some("applyDiscount".into()),
        "`||` does not feed output"
    );
    assert_eq!(
        searched(Some(""), Some("rg -n applyDiscount")),
        Some("applyDiscount".into())
    );
    assert_eq!(searched(None, None), None);
}

#[test]
fn double_quotes_keep_a_backslash_the_way_bash_does() {
    let word = |command: &str| words(command).and_then(|words| words.into_iter().nth(1));
    assert_eq!(word(r#"grep "STANDARDS\b""#), Some(r"STANDARDS\b".into()));
    assert_eq!(word(r#"grep "one\|two""#), Some(r"one\|two".into()));
    assert_eq!(word(r#"grep "\$HOME""#), Some("$HOME".into()));
    assert_eq!(word(r#"grep "a\"b\\c\`d""#), Some(r#"a"b\c`d"#.into()));
    assert_eq!(word("grep \"one\\\ntwo\""), Some("onetwo".into()));
    assert_eq!(word(r"grep STANDARDS\b"), Some("STANDARDSb".into()));
    assert_eq!(word(r"grep 'STANDARDS\b'"), Some(r"STANDARDS\b".into()));
    assert_eq!(
        searched(None, Some(r#"grep -n "STANDARDS\b" AGENTS.md"#)),
        Some("STANDARDS".into())
    );
    assert_eq!(searched(None, Some(r#"grep -n "one\|two" src"#)), None);
}

#[test]
fn a_search_answer_names_the_declaration_and_its_callers() {
    let callers = result(
        vec![row("src/Checkout.java", 14, Some("finish"))],
        Some("Policy"),
    );
    let one = result(vec![row("src/Policy.java", 5, None)], Some("Policy"));
    let answer = usages("Policy.apply", &callers, &one).unwrap();
    assert!(
        answer["declaration"]
            .as_str()
            .unwrap()
            .starts_with("src/Policy.java:5")
    );
    assert_eq!(answer["rows"].as_array().unwrap().len(), 1);
    assert_eq!(answer["complete"], true);
    let two = result(
        vec![
            row("src/Policy.java", 5, None),
            row("src/Rule.java", 2, None),
        ],
        None,
    );
    assert_eq!(usages("apply", &callers, &two), None);
    let open = json!({"results": [row("a.rs", 1, None)], "continuation": null, "route": {"answeredBy": "locate", "complete": true}});
    assert_eq!(usages("apply", &open, &one), None);
}
