//! Replay of the frozen JS engine corpus (`tests/fixtures/runtime/engine-contracts.json`)
//! through the Rust engine. Error codes are the contract; error prose is not. Each case is
//! replayed here, replayed by the corpus named in `REPLAYED_ELSEWHERE`, or listed in `RETIRED`.
mod support;

use craft_engine::session::status::{self, Observations};
use craft_engine::{
    context, contracts, digest, models, routing, routing::catalog, services, session, workers,
};
use serde_json::{Value, json};

/// Cases whose operation is replayed by another frozen corpus with an equivalent input.
const REPLAYED_ELSEWHERE: &[(&str, &str)] = &[
    // The views take the documents directly; `view-contracts.json` includes these cases.
    ("product-projection", "view-contracts.json"),
    ("architecture-projection", "view-contracts.json"),
    ("qa-projection", "view-contracts.json"),
    // Outcome records with an explicit source and time; `feature-contracts.json` includes them.
    ("feature-record", "feature-contracts.json"),
    ("feature-report", "feature-contracts.json"),
];

/// Cases with no Rust counterpart, by reason.
const RETIRED: &[(&str, &[&str])] = &[
    // The JS dispatcher envelope: Rust requests are typed values, and `digest`/`hash` of an
    // absent value hashed the empty text that `undefined` became.
    (
        "envelope",
        &[
            "envelope:0",
            "envelope:1",
            "envelope:2",
            "envelope:3",
            "digest:empty",
            "digest:omitted",
            "hash:empty",
        ],
    ),
    // JS failed reading an argument the dispatcher left `undefined`, or reported `undefined`
    // fields as absent; every Rust caller passes a value (`null` included).
    (
        "absent argument",
        &[
            "task-binding:empty",
            "task-context:empty",
            "services:empty",
            "session-advice:empty",
            "session-status:empty",
            "session-observation-plan:empty",
            "context-requests:empty",
            "checkpoint-compare:empty",
        ],
    ),
    // The routing catalog was a request input; it is embedded and validated on use now.
    // `route-contracts.json` and `session-contracts.json` replay these over the build catalog.
    (
        "catalog input",
        &[
            "session:observation-scoped",
            "session:observation-unmatched-task",
        ],
    ),
    // The engine decision without the built-in CLI mappings the use case added;
    // `services-contracts.json` replays the use case.
    (
        "built-in mappings",
        &[
            "services:default-unverified",
            "services:observed-inventories",
        ],
    ),
];

/// JS validators returned their input; Rust validators return nothing, so only the outcome
/// is compared.
const VALIDATORS: &[&str] = &[
    "engineering-contract-validate",
    "acceptance-evidence-validate",
    "task-context-validate",
    "checkpoint-validate",
    "checkpoint-metadata",
    "activity-validate",
];

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect()
}

fn text(value: &Value) -> Option<String> {
    value.as_str().map(str::to_string)
}

/// The JS operations took a project; the Rust engine takes the effective settings.
fn settings(project: &Value) -> Result<Value, String> {
    let project = if project.is_null() {
        &json!({"formatVersion": 1})
    } else {
        project
    };
    contracts::validate_project(project)?;
    Ok(contracts::effective_settings(project, &json!({}), &json!({}))?["settings"].clone())
}

fn run(operation: &str, input: &Value) -> Result<Value, String> {
    let empty = json!({});
    let defaulted = |name: &str| input.get(name).unwrap_or(&empty);
    match operation {
        "digest" => Ok(json!(digest(&input["value"]))),
        "validate-project" => {
            contracts::validate_project(&input["project"]).map(|()| input["project"].clone())
        }
        "effective-settings" => contracts::effective_settings(
            &input["project"],
            defaulted("preferences"),
            defaulted("overrides"),
        ),
        "worker-budget" => {
            let options = defaulted("options");
            contracts::validate_project(&input["project"])?;
            workers::budget(
                &input["project"],
                defaulted("facts"),
                options["session"].as_str(),
                options["now"].as_i64().unwrap_or(i64::MIN),
            )
        }
        "validate-catalog" => {
            catalog::validate(&input["catalog"]).map(|()| input["catalog"].clone())
        }
        "route" => {
            let settings = settings(&input["project"])?;
            let files = input["files"].as_array().cloned().unwrap_or_default();
            routing::route(
                &settings,
                &routing::Request {
                    intent: input["intent"].as_str(),
                    files: &files,
                    risks: &strings(&input["risks"]),
                    host: input["host"].as_str().unwrap_or("agents"),
                    delegate: input["delegate"] == true,
                    task: input["task"].as_str(),
                    verification_mode: input["verificationMode"].as_str(),
                },
            )
        }
        "capability" => match (input["operation"].as_str(), input["risks"].as_array()) {
            (Some(operation), Some(_)) => {
                Ok(routing::capability(operation, &strings(&input["risks"])))
            }
            _ => Err("capability requires an operation and risks".into()),
        },
        "operation" => services::plan(
            &settings(&input["project"])?,
            input["operation"].as_str().unwrap_or_default(),
            input.get("params").unwrap_or(&empty),
        ),
        "services" => {
            let options = defaulted("options");
            services::prepare(
                &settings(&input["project"])?,
                options.get("availableCommands"),
                options.get("availableMcpTools"),
            )
        }
        "model-advice" => models::advise(&settings(&input["project"])?, &input["input"]),
        "attempt-status" => match (input["records"].as_array(), input["operation"].as_str()) {
            (Some(records), operation) => Ok(session::attempt_status(
                records,
                operation.unwrap_or_default(),
            )),
            (None, _) => Err("attempt status requires records".into()),
        },
        "context-requests" => context::requests(&input["plan"]).map(|requests| {
            json!(
                requests
                    .iter()
                    .map(|r| json!({"target": r.target, "selectedBy": r.selected_by}))
                    .collect::<Vec<_>>()
            )
        }),
        "context-summary" => {
            let budget = match input.get("budgetBytes") {
                None | Some(Value::Null) => None,
                Some(value) => Some(context::budget(value)?),
            };
            match (input["entries"].as_array(), input["missing"].as_array()) {
                (Some(entries), Some(missing)) => {
                    Ok(context::summary(entries.clone(), missing.clone(), budget))
                }
                _ => Err("context summary requires entries and missing".into()),
            }
        }
        "session-advice" => {
            let now = input["options"]["now"].as_i64().unwrap_or(0);
            status::advice(defaulted("input"), &settings(&input["project"])?, now)
        }
        "session-observation-plan" => {
            let needs = status::needs(
                defaulted("input"),
                &settings(&input["project"])?,
                input.get("state"),
                input.get("saved").unwrap_or(&Value::Null),
            );
            let mut result = json!({});
            match needs.plan {
                Some(Ok(plan)) => result["plan"] = plan,
                Some(Err(error)) => result["packError"] = json!(error),
                None => {}
            }
            result["recover"] = json!(needs.recover);
            result["repository"] = json!(needs.repository);
            Ok(result)
        }
        "session-status" => {
            let options = defaulted("options");
            let observed = &options["observations"];
            let observations = Observations {
                state: observed.get("state").cloned(),
                state_error: text(&observed["stateError"]),
                saved: observed.get("saved").cloned().unwrap_or(Value::Null),
                recovered: observed.get("recovered").cloned(),
                checkpoint_error: text(&observed["checkpointError"]),
                repository: observed.get("repository").cloned(),
                pack: observed.get("pack").cloned(),
                plan: observed.get("plan").cloned(),
                pack_error: text(&observed["packError"]),
            };
            status::status(status::Request {
                input: defaulted("input"),
                settings: &settings(&input["project"])?,
                session: options["session"].as_str(),
                task: options["task"].as_str(),
                now: options["now"].as_i64().unwrap_or(0),
                observations: &observations,
            })
        }
        other => support::run(other, input),
    }
}

/// Where a result first departs from the captured response, with a little context.
fn divergence(expected: &Value, actual: &Result<Value, String>) -> String {
    let (expected, actual) = match actual {
        Ok(result) => (expected["result"].to_string(), result.to_string()),
        Err(message) => (expected.to_string(), format!("error {message}")),
    };
    let at = expected
        .chars()
        .zip(actual.chars())
        .take_while(|(a, b)| a == b)
        .count();
    let window =
        |text: &str| -> String { text.chars().skip(at.saturating_sub(60)).take(160).collect() };
    format!(
        "\n  expected …{}\n  actual   …{}",
        window(&expected),
        window(&actual)
    )
}

#[test]
fn frozen_engine_contracts() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/runtime/engine-contracts.json"
    ))
    .unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    let (mut failures, mut replayed, mut elsewhere, mut retired) = (Vec::new(), 0, 0, 0);
    for case in cases {
        let id = case["id"].as_str().unwrap();
        if RETIRED.iter().any(|(_, ids)| ids.contains(&id)) {
            retired += 1;
            continue;
        }
        let operation = case["request"]["operation"].as_str().unwrap();
        if REPLAYED_ELSEWHERE.iter().any(|(op, _)| *op == operation) {
            elsewhere += 1;
            continue;
        }
        let mut input = case["request"]["input"].clone();
        contracts::normalize_numbers(&mut input);
        let actual = run(operation, &input);
        let expected = &case["response"];
        let matches = match &actual {
            Ok(_) if VALIDATORS.contains(&operation) => expected["ok"] == true,
            Ok(result) => {
                expected["ok"] == true
                    && expected.get("result").is_none_or(|e| {
                        serde_json::to_string(e).unwrap() == serde_json::to_string(result).unwrap()
                    })
            }
            Err(_) => expected["ok"] == false && expected["error"]["code"] == "INVALID_INPUT",
        };
        if !matches {
            failures.push(format!("{id}: {}", divergence(expected, &actual)));
        }
        replayed += 1;
    }
    assert!(
        failures.is_empty(),
        "{} of {replayed} cases differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
    let listed: usize = RETIRED.iter().map(|(_, ids)| ids.len()).sum();
    assert_eq!(retired, listed, "every retired case exists");
    assert_eq!((replayed, elsewhere, retired), (102, 12, 19));
}
