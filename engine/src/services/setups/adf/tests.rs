use super::text;
use crate::ticket::criteria::stated;
use serde_json::{Value, json};

fn doc(content: Value) -> Value {
    json!({"type": "doc", "version": 1, "content": content})
}

fn t(text: &str) -> Value {
    json!({"type": "text", "text": text})
}

fn para(content: Value) -> Value {
    json!({"type": "paragraph", "content": content})
}

fn heading(level: u64, text: &str) -> Value {
    json!({"type": "heading", "attrs": {"level": level}, "content": [t(text)]})
}

fn item(content: Value) -> Value {
    json!({"type": "listItem", "content": content})
}

fn list(kind: &str, items: Value) -> Value {
    json!({"type": kind, "content": items})
}

#[test]
fn a_description_reads_as_the_markdown_a_body_holds() {
    let description = doc(json!([
        heading(2, "Context"),
        para(
            json!([t("Checkout "), {"type": "text", "text": "fails", "marks": [{"type": "strong"}]},
                    t(" on retry."), {"type": "hardBreak"}, t("Seen by "),
                    {"type": "mention", "attrs": {"id": "1", "text": "@Ana"}}])
        ),
        heading(3, "Acceptance criteria"),
        list(
            "orderedList",
            json!([
                item(json!([para(json!([t("A retried payment charges once.")]))])),
                item(json!([
                    para(json!([t("The receipt names")])),
                    para(json!([t("the retried order.")])),
                    list(
                        "bulletList",
                        json!([item(json!([para(json!([t("including its id")]))]))])
                    ),
                ])),
            ])
        ),
        para(json!([t("Notes after the list.")])),
    ]));
    assert_eq!(
        text(&description).unwrap(),
        "## Context\n\nCheckout fails on retry.\nSeen by @Ana\n\n### Acceptance criteria\n\n\
         1. A retried payment charges once.\n2. The receipt names\n   the retried order.\n   \
         - including its id\n\nNotes after the list."
    );
}

#[test]
fn criteria_read_from_a_description_match_its_markdown_twin() {
    let description = doc(json!([
        para(json!([t("Acceptance:")])),
        list(
            "bulletList",
            json!([
                item(json!([para(json!([t("First")]))])),
                item(json!([para(
                    json!([t("Second "), {"type": "hardBreak"}, t("continued")])
                )])),
                item(json!([para(json!([t("Third")]))])),
            ])
        ),
        para(json!([t("Out of scope: refunds")])),
    ]));
    let read = stated(&text(&description).unwrap());
    assert_eq!(read.criteria, ["First", "Second continued", "Third"]);
    assert_eq!(
        read,
        stated("Acceptance:\n- First\n- Second\n  continued\n- Third\n\nOut of scope: refunds")
    );
}

#[test]
fn ordered_and_task_lists_keep_their_numbering_and_state() {
    let description = doc(json!([
        {"type": "orderedList", "attrs": {"order": 3}, "content": [
            item(json!([para(json!([t("three")]))])),
            item(json!([para(json!([]))])),
            item(json!([para(json!([t("five")]))])),
        ]},
        {"type": "taskList", "attrs": {"localId": "l"}, "content": [
            {"type": "taskItem", "attrs": {"localId": "a", "state": "DONE"}, "content": [t("done")]},
            {"type": "taskItem", "attrs": {"localId": "b", "state": "TODO"}, "content": [
                t("open"),
                {"type": "taskList", "content": [
                    {"type": "taskItem", "attrs": {"state": "TODO"}, "content": [t("nested")]}]}]},
        ]},
    ]));
    // An empty item is left out; the others keep the position the tracker gave them.
    assert_eq!(
        text(&description).unwrap(),
        "3. three\n5. five\n\n- [x] done\n- [ ] open\n      - [ ] nested"
    );
}

#[test]
fn other_blocks_keep_their_text_and_media_is_left_out() {
    let description = doc(json!([
        {"type": "panel", "attrs": {"panelType": "info"}, "content": [para(json!([t("In a panel")]))]},
        {"type": "expand", "attrs": {"title": "Details"}, "content": [para(json!([t("Hidden")]))]},
        {"type": "codeBlock", "attrs": {"language": "sh"}, "content": [t("craft doctor\nexit 0\n")]},
        {"type": "blockquote", "content": [para(json!([t("Quoted")]))]},
        {"type": "rule"},
        {"type": "table", "content": [
            {"type": "tableRow", "content": [
                {"type": "tableHeader", "content": [para(json!([t("Case")]))]},
                {"type": "tableHeader", "content": [para(json!([t("Result")]))]}]},
            {"type": "tableRow", "content": [
                {"type": "tableCell", "content": [para(json!([t("retry")]))]},
                {"type": "tableCell", "content": [para(json!([
                    {"type": "status", "attrs": {"text": "DONE"}}, t(" "),
                    {"type": "emoji", "attrs": {"shortName": ":tick:"}}, t(" "),
                    {"type": "date", "attrs": {"timestamp": "1764201600000"}}]))]}]}]},
        {"type": "mediaSingle", "content": [{"type": "media", "attrs": {"id": "x", "type": "file"}}]},
        para(json!([{"type": "inlineCard", "attrs": {"url": "https://acme.test/1"}}])),
    ]));
    assert_eq!(
        text(&description).unwrap(),
        "In a panel\n\nDetails\n\nHidden\n\n```sh\ncraft doctor\nexit 0\n```\n\n> Quoted\n\n---\n\n\
         | Case | Result |\n| retry | DONE :tick: 2025-11-27 |\n\nhttps://acme.test/1"
    );
}

#[test]
fn only_a_document_with_text_has_a_body() {
    for empty in [
        doc(json!([])),
        doc(json!([para(json!([])), para(json!([t("  ")]))])),
        json!({"type": "paragraph", "content": [t("not a doc")]}),
        json!(null),
        json!(["Acceptance:"]),
        json!(12),
    ] {
        assert_eq!(text(&empty), None, "{empty}");
    }
}

#[test]
fn nesting_past_the_bound_is_left_out_without_failing() {
    let mut deep = para(json!([t("deepest")]));
    for _ in 0..200 {
        deep = list("bulletList", json!([item(json!([deep]))]));
    }
    let after = list(
        "bulletList",
        json!([item(json!([para(json!([t("after")]))]))]),
    );
    let description = doc(json!([para(json!([t("Top")])), deep, after]));
    // A list whose only text sits past the bound has nothing left to show; the rest reads on.
    assert_eq!(text(&description).unwrap(), "Top\n\n- after");
}

#[test]
fn values_from_the_document_cannot_break_a_list_or_overflow_its_numbering() {
    let description = doc(json!([
        para(json!([t("Acceptance:")])),
        {"type": "orderedList", "attrs": {"order": u64::MAX}, "content": [
            item(json!([para(json!([t("first")]))])),
            item(json!([para(json!([
                t("second for "),
                {"type": "mention", "attrs": {"text": "@Ana\nBo"}},
                {"type": "status", "attrs": {"text": "\rDONE"}},
                {"type": "inlineCard", "attrs": {"url": "https://acme.test/\n1"}}]))])),
        ]},
        {"type": "expand", "attrs": {"title": "Details\n- not an item"}, "content": [
            para(json!([t("Hidden")]))]},
    ]));
    let max = u64::MAX;
    assert_eq!(
        text(&description).unwrap(),
        format!(
            "Acceptance:\n\n{max}. first\n{max}. second for @Ana Bo DONEhttps://acme.test/ 1\n\n\
             Details - not an item\n\nHidden"
        )
    );
}
