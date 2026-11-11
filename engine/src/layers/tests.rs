use super::*;
use crate::contracts::{Documents, effective_settings, layered_settings};

fn layer<'a>(origin: &'static str, document: &'a Value) -> Layer<'a> {
    Layer {
        origin,
        label: format!("{origin} layer /managed/{origin}.json"),
        document,
    }
}

fn resolved(organization: &Value, user: &Value, project: &Value) -> Result<Value> {
    let layers = [layer("organization", organization), layer("user", user)];
    layered_settings(project, &layers, &json!({}), &Documents::default(), true)
}

fn row<'a>(rows: &'a [Value], field: &str) -> &'a Value {
    rows.iter()
        .find(|row| row["field"] == field)
        .unwrap_or_else(|| panic!("no row for {field}"))
}

#[test]
fn a_later_layer_wins_field_by_field_and_names_what_it_hid() {
    let organization = json!({"formatVersion": 1,
        "routing": {"debtMarkers": true, "stopReview": false},
        "efficiency": {"context": {"compactAt": 0.7}},
        "safety": {"enabled": true, "disables": ["force-push", "discard-changes"]}});
    let user = json!({"routing": {"stopReview": true}});
    let project = json!({"formatVersion": 1, "routing": {"debtMarkers": false},
        "safety": {"disables": ["force-push"]}});
    let result = resolved(&organization, &user, &project).unwrap();
    let settings = &result["settings"];
    assert_eq!(settings["routing"]["debtMarkers"], false);
    assert_eq!(settings["routing"]["stopReview"], true);
    assert_eq!(settings["efficiency"]["context"]["compactAt"], 0.7);
    // An object merges key by key; a plain list is replaced whole.
    assert_eq!(settings["safety"]["enabled"], true);
    assert_eq!(settings["safety"]["disables"], json!(["force-push"]));
    let origins = &result["origins"];
    assert_eq!(origins["routing.debtMarkers"], "project");
    assert_eq!(origins["routing.stopReview"], "user");
    assert_eq!(origins["efficiency.context.compactAt"], "organization");
    assert_eq!(origins["routing.editHints"], "bundled");
    let rows = explanation(result["history"].as_object().unwrap());
    let markers = row(&rows, "routing.debtMarkers");
    assert_eq!(markers["origin"], "project");
    assert_eq!(
        markers["overrides"],
        json!([{"origin": "organization", "value": true}, {"origin": "bundled", "value": false}])
    );
    // The project restates the bundled value over the organization's: not redundant.
    assert!(markers.get("redundant").is_none());
    let review = row(&rows, "routing.stopReview");
    assert_eq!(review["overrides"][0]["origin"], "organization");
    assert!(row(&rows, "routing.editHints").get("overrides").is_none());
}

#[test]
fn identified_entries_merge_by_id_and_a_disabled_entry_removes_one() {
    let organization = json!({
        "standards": [
            {"id": "commits", "anchor": "conventional-commits"},
            {"id": "review", "checklist": ["Tests cover the change"]},
            {"id": "docs", "checklist": ["Update the docs"]}],
        "safety": {"enabled": true, "deny": [
            {"id": "no-destroy", "command": ["terraform", "destroy"]},
            {"id": "no-drop", "command": ["dropdb"]}]}});
    let project = json!({"formatVersion": 1,
        "standards": [
            {"id": "review", "checklist": ["Tests and a changelog line"]},
            {"id": "docs", "disabled": true},
            {"id": "ghost", "disabled": true},
            {"id": "api", "checklist": ["Document the endpoint"]}],
        "safety": {"deny": [{"id": "no-drop", "disabled": true}]}});
    let result = resolved(&organization, &json!({}), &project).unwrap();
    let standards = result["settings"]["standards"].as_array().unwrap();
    let ids: Vec<_> = standards
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    // Replaced in place, removed, appended.
    assert_eq!(ids, ["commits", "review", "api"]);
    assert_eq!(standards[1]["checklist"][0], "Tests and a changelog line");
    assert_eq!(
        result["settings"]["safety"]["deny"],
        json!([{"id": "no-destroy", "command": ["terraform", "destroy"]}])
    );
    assert_eq!(
        result["entries"],
        json!({
            "standards": {"commits": "organization", "review": "project", "api": "project"},
            "safety.deny": {"no-destroy": "organization"}})
    );
    let listing = crate::safety::layered_listing(&result);
    assert_eq!(listing["origin"], "organization");
    let rule = listing["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|rule| rule["id"] == "no-destroy")
        .unwrap();
    assert_eq!(rule["source"], "organization");
    let rows = explanation(result["history"].as_object().unwrap());
    let docs = row(&rows, "standards[docs]");
    assert_eq!(
        (&docs["origin"], &docs["disabled"]),
        (&json!("project"), &json!(true))
    );
    assert_eq!(docs["overrides"][0]["origin"], "organization");
    assert!(docs.get("disablesNothing").is_none());
    // An override that changes nothing is said, not silent.
    assert_eq!(row(&rows, "standards[ghost]")["disablesNothing"], true);
    assert_eq!(
        row(&rows, "standards[review]")["overrides"][0]["value"]["checklist"][0],
        "Tests cover the change"
    );
    assert_eq!(
        contributed(result["history"].as_object().unwrap(), "organization"),
        6
    );
}

#[test]
fn a_value_repeating_the_one_beneath_it_is_marked_redundant() {
    let organization = json!({"routing": {"debtMarkers": true}});
    let project = json!({"formatVersion": 1, "routing": {"debtMarkers": true}});
    let result = resolved(&organization, &json!({}), &project).unwrap();
    let rows = explanation(result["history"].as_object().unwrap());
    assert_eq!(row(&rows, "routing.debtMarkers")["redundant"], true);
    let shown = text(&json!({"order": ["bundled", "project"], "layers": [], "overrides": rows}));
    assert!(shown.contains(
        "routing.debtMarkers = true (project) over true (organization), false (bundled) — repeats"
    ), "{shown}");
}

#[test]
fn a_layer_is_refused_with_its_label_for_what_belongs_to_one_project() {
    let refused = |document: Value, expected: &str| {
        let error = resolved(&document, &json!({}), &json!({"formatVersion": 1})).unwrap_err();
        assert!(
            error.starts_with("organization layer /managed/organization.json: ")
                && error.contains(expected),
            "{error}"
        );
    };
    refused(json!({"modules": []}), "unknown layer field: modules");
    refused(
        json!({"permissions": {"writeOperations": ["git.push"]}}),
        "unknown layer field",
    );
    refused(json!({"formatVersion": 2}), "unsupported formatVersion");
    refused(json!([]), "layer must be an object");
    refused(
        json!({"routing": {"debtMarkers": "yes"}}),
        "must be boolean",
    );
    refused(
        json!({"routing": {"unknown": true}}),
        "unknown settings.routing field",
    );
    refused(
        json!({"standards": [{"id": "api", "file": "docs/api.md"}]}),
        "file names a project file",
    );
    refused(
        json!({"standards": [{"id": "api", "checklist": ["x"], "scope": {"modules": ["api"]}}]}),
        "scope.modules names project modules",
    );
    refused(
        json!({"anchors": {"catalogs": ["a.json"]}}),
        "anchors.catalogs",
    );
    refused(
        json!({"skills": {"declared": [{"name": "a", "path": ".agents/skills/a"}]}}),
        "skills.declared",
    );
    refused(
        json!({"standards": [{"id": "a"}]}),
        "needs a file, folder, anchor, run or checklist",
    );
    refused(
        json!({"standards": [{"id": "a", "disabled": true, "checklist": ["x"]}]}),
        "a disabled entry takes only its id",
    );
    refused(
        json!({"safety": {"deny": [{"id": "a", "disabled": "no"}]}}),
        "disabled must be boolean",
    );
    let user = json!({"execution": {"roles": {"reviewer": {"mode": "walk"}}}});
    let error = resolved(&json!({}), &user, &json!({"formatVersion": 1})).unwrap_err();
    assert!(
        error.starts_with("user layer /managed/user.json: "),
        "{error}"
    );
}

#[test]
fn layers_valid_alone_are_checked_again_once_merged() {
    // An anchor no layer can resolve is found once the layers are merged.
    let organization = json!({"safety": {"deny": [{"id": "a", "command": ["x"]}]},
        "standards": [{"id": "sem", "anchor": "no-such-anchor"}]});
    let error = resolved(&organization, &json!({}), &json!({"formatVersion": 1})).unwrap_err();
    assert!(error.contains("no-such-anchor"), "{error}");
    let many: Vec<Value> = (0..40)
        .map(|n| json!({"id": format!("org-{n}"), "checklist": ["x"]}))
        .collect();
    let more: Vec<Value> = (0..40)
        .map(|n| json!({"id": format!("own-{n}"), "checklist": ["x"]}))
        .collect();
    let merged = resolved(
        &json!({"standards": many}),
        &json!({}),
        &json!({"formatVersion": 1, "standards": more}),
    );
    match merged {
        Err(error) => assert!(
            error.starts_with("merged layers: standards must be"),
            "{error}"
        ),
        Ok(result) => assert_eq!(
            result["settings"]["standards"].as_array().unwrap().len(),
            80
        ),
    }
}

#[test]
fn without_a_baseline_layer_the_result_is_the_project_alone() {
    let project = json!({"formatVersion": 1,
        "standards": [{"id": "commits", "anchor": "conventional-commits"}],
        "safety": {"enabled": true, "deny": [{"id": "no-drop", "command": ["dropdb"]}]}});
    let plain = effective_settings(&project, &json!({}), &json!({})).unwrap();
    assert!(plain.get("entries").is_none() && plain.get("history").is_none());
    assert_eq!(plain["origins"]["standards"], "project");
    let switched = effective_settings(
        &project,
        &json!({"routing": {"editHints": false}}),
        &json!({}),
    )
    .unwrap();
    assert!(switched.get("entries").is_none());
    assert_eq!(switched["origins"]["routing.editHints"], "user");
    // A project may switch off its own entry.
    let own = json!({"formatVersion": 1, "standards": [
        {"id": "commits", "anchor": "conventional-commits"}, {"id": "commits2", "disabled": true}]});
    let result = effective_settings(&own, &json!({}), &json!({})).unwrap();
    assert_eq!(result["settings"]["standards"].as_array().unwrap().len(), 1);
}

#[test]
fn a_field_filter_keeps_the_field_and_what_lies_beneath_it() {
    assert!(beneath("routing", "routing"));
    assert!(beneath("routing.debtMarkers", "routing"));
    assert!(beneath("standards[api]", "standards"));
    assert!(!beneath("routingx", "routing"));
    assert!(!beneath("routing", "routing.debtMarkers"));
}
