//! The model-facing text `craft hook` renders, authored in `application/text/` and embedded.
//! See [`craft_engine::text`] for the template rules and identities.

use craft_engine::text::Template;

macro_rules! hook_template {
    ($name:ident, $id:literal, [$($slot:literal),*]) => {
        pub(crate) const $name: Template = Template {
            id: concat!("hook.", $id),
            slots: &[$($slot),*],
            source: include_str!(concat!("../../text/hook/", $id, ".txt")),
        };
    };
}

hook_template!(SESSION, "session", ["identity"]);
hook_template!(HANDOFF, "handoff", ["handoff"]);
hook_template!(PLANNING, "planning", ["planning"]);
hook_template!(PLANS, "plans", ["plans"]);
hook_template!(ROUTING, "routing", ["hint"]);
hook_template!(ANSWER, "answer", ["answer"]);
hook_template!(RESUME, "resume", ["in-force", "records"]);
hook_template!(CHECKPOINT, "checkpoint", ["in-force", "checkpoint"]);
hook_template!(IN_FORCE, "in-force", []);
hook_template!(SERVICES, "services", ["services"]);
hook_template!(PRESERVE, "preserve", []);
hook_template!(UNAVAILABLE, "unavailable", ["reason"]);
hook_template!(STANDARDS, "standards", ["found"]);
hook_template!(CONVENTIONS, "conventions", ["offer"]);
hook_template!(HOOK_GAP, "hook-gap", ["notice"]);
hook_template!(TESTS, "tests", ["summary"]);
hook_template!(DELEGATION, "delegation", ["delegation"]);
hook_template!(STOP, "stop", ["count", "items", "lines"]);
hook_template!(TICKET, "ticket", ["reference", "command"]);
hook_template!(WORK, "work", ["command"]);

/// Every model-facing template `craft` embeds, the engine's included.
pub fn templates() -> Vec<&'static Template> {
    let mut all = craft_engine::text::templates();
    all.extend([
        &SESSION,
        &HANDOFF,
        &PLANNING,
        &PLANS,
        &ROUTING,
        &ANSWER,
        &RESUME,
        &CHECKPOINT,
        &IN_FORCE,
        &SERVICES,
        &PRESERVE,
        &UNAVAILABLE,
        &STANDARDS,
        &CONVENTIONS,
        &HOOK_GAP,
        &TESTS,
        &DELEGATION,
        &STOP,
        &TICKET,
        &WORK,
    ]);
    all
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_template_is_well_formed_with_a_unique_id() {
        assert_eq!(
            craft_engine::text::check(&super::templates()),
            Vec::<String>::new()
        );
    }
}
