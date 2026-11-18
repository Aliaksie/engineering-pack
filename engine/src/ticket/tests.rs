use super::*;

#[test]
fn a_prompt_names_its_ticket_by_url_or_by_a_key_it_calls_a_ticket() {
    for (prompt, expected) in [
        (
            "let's implement https://github.com/acme/shop/issues/145",
            Some("https://github.com/acme/shop/issues/145"),
        ),
        (
            "fix (https://gitlab.com/acme/shop/-/issues/7).",
            Some("https://gitlab.com/acme/shop/-/issues/7"),
        ),
        (
            "work on https://acme.atlassian.net/browse/SHOP-12 please",
            Some("https://acme.atlassian.net/browse/SHOP-12"),
        ),
        (
            "see https://linear.app/acme/issue/ENG-3/refunds",
            Some("https://linear.app/acme/issue/ENG-3/refunds"),
        ),
        ("start ticket SHOP-12", Some("SHOP-12")),
        ("Issue #42: the parser drops quotes", Some("#42")),
        ("please do story 7.", Some("7")),
        // Not a ticket: a pull request, a commit, an encoding name, a bare number.
        ("review https://github.com/acme/shop/pull/9", None),
        ("convert the files to UTF-8 and SHA-256", None),
        ("bump the limit to 145", None),
        ("the issues page is slow", None),
    ] {
        assert_eq!(reference(prompt).as_deref(), expected, "{prompt}");
    }
}

#[test]
fn a_path_holds_a_contract_by_its_kind() {
    for (path, expected) in [
        ("schemas/project.schema.json", Some("schema")),
        ("api/openapi.yaml", Some("schema")),
        ("proto/order.proto", Some("schema")),
        ("db/migrations/0004_refunds.sql", Some("migration")),
        ("hosts/hooks.json", Some("hooks")),
        (".claude/settings.json", Some("hooks")),
        (".github/workflows/ci.yml", Some("ci")),
        ("src/schema.rs", None),
        ("engine/src/contracts.rs", None),
        ("docs/ROUTING.md", None),
    ] {
        assert_eq!(contract(path), expected, "{path}");
    }
}

fn decided(criteria: &str, questions: usize, modules: &[&str], contracts: &[&str]) -> Value {
    let owned = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    decide(&Facts {
        criteria,
        questions,
        labels: &[],
        modules: &owned(modules),
        contracts: &owned(contracts),
    })
}

/// One module, no contract and clear criteria is direct work; several modules, a contract or
/// unclear criteria plan first; open questions are asked before either.
#[test]
fn the_mode_follows_the_tickets_facts() {
    let direct = decided("clear", 0, &["application"], &[]);
    assert_eq!(direct["mode"], "direct");
    assert_eq!(
        line(&direct),
        "Craft: direct delivery (1 module, no contract change, clear acceptance criteria)"
    );

    let plan = decided("clear", 0, &["engine", "application", "docs"], &["schema"]);
    assert_eq!(plan["mode"], "plan");
    assert_eq!(
        line(&plan),
        "Craft suggests: plan → implement → review (3 modules, schema change)"
    );
    assert_eq!(decided("unclear", 0, &["engine"], &[])["mode"], "plan");
    assert_eq!(
        decided("missing", 0, &[], &["hooks", "hooks"])["reasons"],
        json!(["hooks change", "no acceptance criteria"])
    );

    let ask = decided("clear", 2, &["engine"], &[]);
    assert_eq!(ask["mode"], "ask");
    assert_eq!(
        line(&ask),
        "Craft suggests: answer the ticket's questions first (2 open questions in the ticket)"
    );
}

#[test]
fn labels_that_ask_for_discussion_or_design_decide_too() {
    let labelled = |label: &str| {
        decide(&Facts {
            criteria: "clear",
            questions: 0,
            labels: &[label.to_owned()],
            modules: &["engine".to_owned()],
            contracts: &[],
        })
    };
    assert_eq!(labelled("Needs-Info")["mode"], "ask");
    assert_eq!(labelled("epic")["reasons"], json!(["labelled epic"]));
    assert_eq!(labelled("bug")["mode"], "direct");
}

#[test]
fn the_offer_is_on_until_the_project_turns_it_off() {
    assert!(enabled(&json!({"routing": {}})));
    assert!(!enabled(&json!({"routing": {"ticketOffer": false}})));
}
