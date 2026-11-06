use super::*;
use serde_json::json;

fn names(tier: Tier) -> Vec<&'static str> {
    anchors()
        .unwrap()
        .iter()
        .filter(|anchor| anchor.tier == tier)
        .map(|anchor| anchor.id.as_str())
        .collect()
}

#[test]
fn the_overlay_offers_the_eight_crisp_names_with_their_defaults() {
    assert_eq!(
        names(Tier::Structure),
        [
            "conventional-commits",
            "keep-a-changelog",
            "semantic-versioning",
            "adr-according-to-nygard",
            "madr",
            "gherkin",
            "ears-requirements",
            "connextra-user-story"
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
    assert_eq!(find("madr").unwrap().input(), Some(Input::File));
    assert_eq!(
        find("conventional-commits").unwrap().input(),
        Some(Input::CommitMessage)
    );
    assert!(
        find("arc42").is_none(),
        "a catalog-only name is not Craft's"
    );
    for (alias, id) in [
        ("semver", "semantic-versioning"),
        ("nygard-adr", "adr-according-to-nygard"),
        ("ears", "ears-requirements"),
        ("connextra", "connextra-user-story"),
    ] {
        assert_eq!(canonical(alias), id);
        assert_eq!(find(alias).unwrap().id, id);
    }
    assert_eq!(canonical("arc42"), "arc42");
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
        if anchor.tier != Tier::Refused {
            let entry =
                entry(&anchor.id).unwrap_or_else(|| panic!("{}: no catalog entry", anchor.id));
            assert!(entry["definition"].as_str().is_some_and(|d| !d.is_empty()));
        }
        for intent in &anchor.intents {
            assert!(
                intents.contains(&intent.as_str()),
                "{}: {intent}",
                anchor.id
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
        "id": "demo",
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
    assert!(broken("summary", Value::Null).contains("need a summary"));
    assert!(broken("evidence", Value::Null).contains("evidence is required"));
    assert!(broken("aliases", json!(["Semver"])).contains("kebab-case"));
    assert!(broken("extra", json!(1)).contains("unknown anchor field: extra"));
    assert!(
        parse_overlay(&overlay(vec![base.clone(), base.clone()]))
            .unwrap_err()
            .contains("duplicate anchor demo")
    );
    let alias = json!({"id": "other", "aliases": ["demo"], "tier": "structure", "check": "gherkin", "summary": "x", "evidence": "e"});
    assert!(
        parse_overlay(&overlay(vec![base.clone(), alias]))
            .unwrap_err()
            .contains("duplicate anchor demo"),
        "an alias may not reuse a name"
    );
    let refused = json!({"id": "vague", "tier": "refused", "summary": "x", "evidence": "e"});
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

const INTENTS: &[&str] = &["plan", "story", "implement"];

#[test]
fn project_catalogs_load_in_order_with_their_source() {
    let first = json!({"anchors": [
        {"id": "our-adr", "title": "ACME ADR", "checklist": ["Owner named"], "intents": ["plan"]},
        {"id": "semver", "summary": "Calendar versions"}
    ]});
    let second = json!({"anchors": [{"id": "our-adr", "summary": "ACME ADR v2"}]});
    let loaded = load(
        &[("a.json".into(), first), ("b.json".into(), second)],
        INTENTS,
    )
    .unwrap();
    assert_eq!(
        loaded,
        json!([
            {"id": "semantic-versioning", "summary": "Calendar versions", "source": "a.json"},
            {"id": "our-adr", "summary": "ACME ADR v2", "source": "b.json"}
        ]),
        "aliases become ids; a later file replaces an entry"
    );
    let settings = json!({"anchors": {"loaded": loaded}});
    let ours = resolve(&settings, "semver").unwrap();
    assert_eq!(ours.source, Source::Project("a.json".into()));
    assert!(ours.check.is_none());
    assert_eq!(
        resolve(&settings, "keep-a-changelog").unwrap().source,
        Source::Craft
    );
    let arc42 = resolve(&settings, "arc42").unwrap();
    assert_eq!(
        (arc42.source, arc42.tier),
        (Source::Catalog, Tier::Vocabulary)
    );
    assert!(resolve(&settings, "no-such-anchor").is_none());
}

#[test]
fn a_malformed_project_entry_names_its_file() {
    let fails = |entry: Value| {
        load(
            &[("ours.json".into(), json!({"anchors": [entry]}))],
            INTENTS,
        )
        .unwrap_err()
    };
    assert_eq!(
        fails(json!({"id": "x"})),
        "anchor catalog ours.json: anchor x: needs a summary or a title"
    );
    assert!(
        fails(json!({"id": "x", "summary": "s", "check": "lint"})).contains("no check named lint")
    );
    assert!(
        fails(json!({"id": "x", "summary": "s", "intents": ["deploy"]}))
            .contains("unknown intent deploy")
    );
    assert!(
        fails(json!({"id": "x", "summary": "s", "source": "y"}))
            .contains("unknown anchor field: source")
    );
    assert!(
        fails(json!({"id": "x", "summary": "s", "tier": "structure"}))
            .contains("unknown anchor field: tier")
    );
    let twice = json!({"anchors": [{"id": "x", "title": "X"}, {"id": "x", "title": "Y"}]});
    assert!(
        load(&[("ours.json".into(), twice)], INTENTS)
            .unwrap_err()
            .contains("duplicate anchor x")
    );
    let by_anchor = load(
        &[(
            "ours.json".into(),
            json!({"anchors": [{"id": "ours", "title": "T", "check": "nygard-adr"}]}),
        )],
        INTENTS,
    )
    .unwrap();
    let anchor = resolve(&json!({"anchors": {"loaded": by_anchor}}), "ours").unwrap();
    assert_eq!(
        anchor.check.as_deref(),
        Some("nygard-adr"),
        "a Craft anchor name reuses its check"
    );
}

#[test]
fn listing_shows_project_and_craft_names_and_search_reaches_the_catalog() {
    let settings = json!({"anchors": {"loaded": [{"id": "our-adr", "title": "ACME ADR", "source": "a.json"}]}});
    let all = list(&settings, None);
    let ids: Vec<&str> = all["anchors"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|a| a["id"].as_str())
        .collect();
    assert_eq!(ids[0], "our-adr");
    assert!(ids.contains(&"clean-code") && !ids.contains(&"arc42"));
    assert_eq!(all["catalog"]["entries"], catalog_entries().len());
    let found = list(&settings, Some("ARC42"));
    assert_eq!(found["anchors"][0]["id"], "arc42");
    assert_eq!(found["anchors"][0]["source"], "catalog");
    assert!(
        found["anchors"][0]["link"]
            .as_str()
            .unwrap()
            .ends_with("/docs/anchors/arc42.adoc")
    );
    let semver = list(&settings, Some("semver"));
    assert_eq!(semver["anchors"][0]["aliases"], json!(["semver"]));
}
