use super::*;
use serde_json::json;

fn names(tier: Tier) -> Vec<&'static str> {
    anchors()
        .unwrap()
        .iter()
        .filter(|anchor| anchor.tier == tier)
        .map(|anchor| anchor.name.as_str())
        .collect()
}

#[test]
fn the_overlay_offers_the_eight_crisp_names_with_their_defaults() {
    assert_eq!(
        names(Tier::Structure),
        [
            "conventional-commits",
            "keep-a-changelog",
            "semver",
            "nygard-adr",
            "madr",
            "gherkin",
            "ears",
            "connextra"
        ]
    );
    assert_eq!(names(Tier::Refused), ["clean-code"]);
    let intents = |name| find(name).unwrap().intents.clone();
    let targets = |name| find(name).unwrap().targets.clone();
    assert_eq!(intents("connextra"), ["story"]);
    assert_eq!(intents("ears"), ["story"]);
    assert_eq!(intents("gherkin"), ["story", "verify"]);
    assert_eq!(intents("nygard-adr"), ["plan"]);
    assert_eq!(intents("madr"), ["plan"]);
    assert!(intents("conventional-commits").is_empty());
    assert_eq!(targets("keep-a-changelog"), ["CHANGELOG.md"]);
    assert_eq!(targets("gherkin"), [".feature"]);
    assert_eq!(
        targets("semver"),
        [
            "CHANGELOG.md",
            "package.json",
            "Cargo.toml",
            "pyproject.toml"
        ]
    );
    assert!(targets("nygard-adr").is_empty() && targets("ears").is_empty());
    assert_eq!(find("madr").unwrap().input, Input::File);
    assert_eq!(
        find("conventional-commits").unwrap().input,
        Input::CommitMessage
    );
    assert!(find("arc42").is_none());
}

#[test]
fn every_offered_name_stands_for_a_catalog_entry_and_known_intents() {
    let catalog = catalog().unwrap();
    assert_eq!(catalog["source"]["license"], "Apache-2.0");
    assert_eq!(catalog["source"]["commit"].as_str().unwrap().len(), 40);
    let routing = crate::routing::catalog::catalog().unwrap();
    let intents: Vec<&str> = routing["intents"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for anchor in anchors().unwrap() {
        if let Some(id) = &anchor.catalog {
            let entry =
                entry(id).unwrap_or_else(|| panic!("{}: no catalog entry {id}", anchor.name));
            assert!(entry["definition"].as_str().is_some_and(|d| !d.is_empty()));
        }
        for intent in &anchor.intents {
            assert!(
                intents.contains(&intent.as_str()),
                "{}: {intent}",
                anchor.name
            );
        }
    }
    assert_eq!(
        entry("adr-according-to-nygard").unwrap()["title"],
        "ADR according to Nygard"
    );
    assert!(entry("no-such-anchor").is_none());
}

#[test]
fn the_notice_names_the_commit_the_catalog_was_generated_from() {
    let commit = catalog().unwrap()["source"]["commit"].as_str().unwrap();
    let notice = include_str!("../../anchors/NOTICE");
    assert!(notice.contains(commit) && notice.contains("Apache License, Version 2.0"));
}

#[test]
fn a_malformed_overlay_entry_is_rejected_with_its_name() {
    let base = json!({
        "name": "demo",
        "catalog": "demo",
        "tier": "structure",
        "check": "gherkin",
        "summary": "One line.",
        "evidence": "Measured."
    });
    let overlay = |entries: Vec<Value>| json!({"anchors": entries}).to_string();
    assert_eq!(
        parse_overlay(&overlay(vec![base.clone()])).unwrap().len(),
        1
    );
    let broken = |field: &str, value: Value| {
        let mut entry = base.clone();
        if value.is_null() {
            entry.as_object_mut().unwrap().remove(field);
        } else {
            entry[field] = value;
        }
        parse_overlay(&overlay(vec![entry])).unwrap_err()
    };
    assert!(broken("check", json!("lint")).contains("no check named lint"));
    assert!(broken("check", Value::Null).contains("name a check"));
    assert!(broken("tier", json!("vocabulary")).contains("name a check"));
    assert!(broken("tier", json!("strict")).contains("unknown tier"));
    assert!(broken("summary", json!("two\nlines")).contains("one line"));
    assert!(broken("summary", Value::Null).contains("need a catalog id and a summary"));
    assert!(broken("evidence", Value::Null).contains("evidence is required"));
    assert!(broken("input", json!("stdin")).contains("unknown input"));
    assert!(broken("extra", json!(1)).contains("unknown anchor field: extra"));
    assert!(
        parse_overlay(&overlay(vec![base.clone(), base.clone()]))
            .unwrap_err()
            .contains("duplicate anchor demo")
    );
    let refused = json!({"name": "vague", "tier": "refused", "summary": "x", "evidence": "e"});
    assert!(
        parse_overlay(&overlay(vec![refused]))
            .unwrap_err()
            .contains("only its evidence")
    );
}

#[test]
fn a_name_without_a_check_is_a_finding_never_a_pass() {
    let rules = |name| {
        check(name, "anything", None)
            .into_iter()
            .map(|finding| finding.rule)
            .collect::<Vec<_>>()
    };
    assert_eq!(rules("clean-code"), ["unknown-anchor"]);
    assert_eq!(rules("arc42"), ["unknown-anchor"]);
    assert!(check("conventional-commits", "fix: trim input", None).is_empty());
    assert_eq!(rules("conventional-commits")[0], "header");
}
