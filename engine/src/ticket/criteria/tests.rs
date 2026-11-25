use super::*;
use crate::session::acceptance::validate_contract;

fn criteria(body: &str) -> Vec<String> {
    stated(body).criteria
}

#[test]
fn the_list_under_an_acceptance_heading_is_stated() {
    let body = "Problem text.\n\nAcceptance:\n- First holds.\n- `second` holds\n  across two lines.\n\nFound in production.\n- not a criterion";
    assert_eq!(
        criteria(body),
        ["First holds.", "`second` holds across two lines."]
    );
}

#[test]
fn headings_checkboxes_numbers_and_sub_items_are_read() {
    let body = "## Acceptance Criteria\r\n\r\n1. [ ] One\r\n2) [x] Two\r\n   - detail of two\r\n\t- tab detail\r\n* [X] Three\r\n\r\n## Notes\r\n- no";
    assert_eq!(
        criteria(body),
        ["One", "Two detail of two tab detail", "Three"]
    );
    assert_eq!(
        criteria("**Acceptance criteria**\n+ Plus item"),
        ["Plus item"]
    );
    assert_eq!(
        criteria("### acceptance:\nThe change is done when:\n- It works"),
        ["It works"]
    );
}

#[test]
fn a_body_without_an_acceptance_list_states_none() {
    assert_eq!(stated(""), Stated::default());
    assert!(criteria("- a list\n- with no heading").is_empty());
    assert!(criteria("Acceptance criteria are below in the PR.\n- no").is_empty());
    assert!(criteria("Acceptance:\n\n## Next\n- other").is_empty());
}

#[test]
fn criteria_a_contract_cannot_hold_are_counted_not_cut() {
    let long = "x".repeat(LENGTH + 1);
    let mut body = format!("Acceptance:\n- {long}\n");
    for n in 0..MAX + 2 {
        body.push_str(&format!("- item {n}\n"));
    }
    let stated = stated(&body);
    assert_eq!(stated.criteria.len(), MAX);
    assert_eq!(stated.criteria[0], "item 0");
    assert_eq!(stated.omitted, 3);
}

fn linked() -> Value {
    json!({"reference": "https://github.com/o/r/issues/7", "url": "https://github.com/o/r/issues/7",
        "title": "Ship it", "criteria": ["One holds", "Two holds"], "linkedAt": 1_764_072_000_000_i64})
}

#[test]
fn a_linked_issue_derives_a_valid_required_contract() {
    let contract = contract(&linked()).unwrap();
    validate_contract(&contract).unwrap();
    assert_eq!(contract["intent"], "Ship it");
    let ids: Vec<&Value> = contract["criteria"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| &c["id"])
        .collect();
    assert_eq!(ids, [&json!("ac-1"), &json!("ac-2")]);
    assert!(
        contract["criteria"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["required"] == true && c["checkIds"] == json!([CHECK]))
    );
    let source = &contract["context"]["sources"][0];
    assert_eq!(source["reference"], "https://github.com/o/r/issues/7");
    assert_eq!(source["observedAt"], "2025-11-25T12:00:00.000Z");
    // The same linked issue derives the same contract.
    assert_eq!(contract, super::contract(&linked()).unwrap());
}

#[test]
fn an_issue_without_criteria_or_link_time_derives_none() {
    let mut issue = linked();
    issue["criteria"] = json!([]);
    assert!(contract(&issue).is_none());
    let mut issue = linked();
    issue.as_object_mut().unwrap().remove("linkedAt");
    assert!(contract(&issue).is_none());
    // An untitled issue is named by its reference.
    let mut issue = linked();
    issue["title"] = Value::Null;
    issue.as_object_mut().unwrap().remove("url");
    let contract = contract(&issue).unwrap();
    validate_contract(&contract).unwrap();
    assert_eq!(
        contract["intent"],
        "The acceptance criteria of https://github.com/o/r/issues/7"
    );
}

#[test]
fn criteria_past_the_contract_size_are_left_out_and_named() {
    let long: String = "ж".repeat(LENGTH);
    let mut issue = linked();
    issue["criteria"] = json!(vec![long; MAX]);
    issue["criteriaOmitted"] = json!(2);
    let contract = contract(&issue).unwrap();
    validate_contract(&contract).unwrap();
    let kept = contract["criteria"].as_array().unwrap().len();
    assert!(kept > 0 && kept < MAX, "{kept}");
    let left = MAX - kept + 2;
    assert_eq!(
        contract["context"]["questions"],
        json!([format!(
            "{left} acceptance criteria the issue lists are not in this contract; read them in the issue"
        )])
    );
    assert_eq!(contract["context"]["coverage"], "incomplete");
    // Criteria the issue listed past those kept at linking are named too.
    let mut issue = linked();
    issue["criteriaOmitted"] = json!(1);
    let contract = super::contract(&issue).unwrap();
    validate_contract(&contract).unwrap();
    assert_eq!(contract["criteria"].as_array().unwrap().len(), 2);
    assert_eq!(
        contract["context"]["questions"].as_array().unwrap().len(),
        1
    );
}
