use super::*;
use crate::contracts::{Documents, effective_settings, effective_settings_with};
use crate::routing::{Request, route};

const REVIEW: &str = "craft-review:review-change";
const SKILL: &str = "---\nname: team-review\ndescription: Our review.\n---\nBody\n";

fn project(skills: Value) -> Value {
    json!({"formatVersion": 1, "modules": [{"name": "api", "path": "src/api"}], "skills": skills})
}

/// Settings with every declared SKILL.md given `text` (None = not readable); no mirrors.
fn settings(skills: Value, text: Option<&str>) -> std::result::Result<Value, String> {
    let declared: Vec<String> = skills["declared"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|skill| skill["path"].as_str().map(|p| format!("{p}/SKILL.md")))
        .collect();
    with_files(skills, |file| {
        declared
            .contains(&file.to_string())
            .then(|| text.map(str::to_string))
            .flatten()
    })
}

/// Settings where each SKILL.md Craft asks for reads as `read(path)`.
fn with_files(
    skills: Value,
    read: impl Fn(&str) -> Option<String>,
) -> std::result::Result<Value, String> {
    let project = project(skills);
    let documents = Documents {
        skills: declared_files(&project)
            .into_iter()
            .map(|path| {
                let text = read(&path);
                (path, text)
            })
            .collect(),
        ..Documents::default()
    };
    Ok(effective_settings_with(&project, &json!({}), &json!({}), &documents)?["settings"].take())
}

fn routed(settings: &Value, intent: &str, files: &[&str], host: &str) -> Value {
    let files: Vec<Value> = files.iter().map(|file| json!(file)).collect();
    route(
        settings,
        &Request {
            intent: Some(intent),
            files: &files,
            risks: &[],
            host,
            delegate: false,
            task: None,
            verification_mode: None,
            artifact: None,
            packages: None,
        },
    )
    .unwrap()
}

fn replacing(path: &str) -> Value {
    json!({"declared": [{"name": "team-review", "path": path, "replaces": [REVIEW]}]})
}

#[test]
fn a_declared_replacement_takes_the_package_skills_place_and_says_so() {
    let settings = settings(replacing(".claude/skills/team-review"), Some(SKILL)).unwrap();
    let plan = routed(&settings, "review", &[], "claude");
    let skills = plan["skills"].as_array().unwrap();
    assert_eq!(skills[0], "project:team-review");
    assert!(!skills.contains(&json!(REVIEW)));
    assert_eq!(
        plan["skillSources"],
        json!([{"skill": "project:team-review", "source": "project", "path": ".claude/skills/team-review", "replaces": [REVIEW]}])
    );
    // Other work keeps its route: nothing replaced there, nothing attributed.
    let plan = routed(&settings, "implement", &[], "claude");
    assert!(plan.get("skillSources").is_none());
}

#[test]
fn a_mirror_in_a_folder_the_host_reads_serves_that_host() {
    // One declaration for Codex (`.agents/skills`) and its copy for Claude (`.claude/skills`).
    let mirrored = |name: &'static str| {
        move |file: &str| match file {
            ".agents/skills/team-review/SKILL.md" => Some(SKILL.to_string()),
            ".claude/skills/team-review/SKILL.md" => Some(name.to_string()),
            _ => None,
        }
    };
    let settings = with_files(replacing(".agents/skills/team-review"), mirrored(SKILL)).unwrap();
    for host in ["claude", "codex"] {
        let plan = routed(&settings, "review", &[], host);
        assert_eq!(plan["skills"][0], "project:team-review", "{host}: {plan}");
        assert_eq!(plan["missing"], json!([]), "{host}");
        // The source names the copy this host reads.
        let read = if host == "claude" {
            ".claude"
        } else {
            ".agents"
        };
        assert_eq!(
            plan["skillSources"][0]["path"],
            format!("{read}/skills/team-review")
        );
    }
    // A copy under another name is not the declared skill.
    let other = "---\nname: other\n---\n";
    let settings = with_files(replacing(".agents/skills/team-review"), mirrored(other)).unwrap();
    let plan = routed(&settings, "review", &[], "claude");
    let missing = plan["missing"][0].as_str().unwrap();
    assert!(
        missing.contains("mirror the skill to .claude/skills/team-review"),
        "the advice names the exact folder: {missing}"
    );
}

#[test]
fn a_replacement_the_host_cannot_read_keeps_the_package_skill_and_reports() {
    let settings = settings(replacing(".agents/skills/team-review"), Some(SKILL)).unwrap();
    let plan = routed(&settings, "review", &[], "claude");
    assert!(plan["skills"].as_array().unwrap().contains(&json!(REVIEW)));
    assert!(
        !plan["skills"]
            .as_array()
            .unwrap()
            .contains(&json!("project:team-review"))
    );
    let missing = plan["missing"][0].as_str().unwrap();
    assert!(
        missing.contains("claude does not read .agents/skills"),
        "{missing}"
    );
    assert!(
        missing.contains(&format!("routed {REVIEW} instead")),
        "{missing}"
    );
    assert_eq!(plan["skillSources"][0]["status"], "unavailable");
    // Codex reads .agents/skills: the replacement applies there.
    let plan = routed(&settings, "review", &[], "codex");
    assert_eq!(plan["skills"][0], "project:team-review");
}

#[test]
fn a_missing_or_misnamed_skill_file_is_unavailable_not_routed() {
    let missing = settings(replacing(".claude/skills/team-review"), None).unwrap();
    let plan = routed(&missing, "review", &[], "claude");
    assert!(plan["skills"].as_array().unwrap().contains(&json!(REVIEW)));
    assert!(
        plan["missing"][0]
            .as_str()
            .unwrap()
            .contains("SKILL.md not found")
    );
    let other = settings(
        replacing(".claude/skills/team-review"),
        Some("---\nname: other\n---\n"),
    )
    .unwrap();
    let plan = routed(&other, "review", &[], "claude");
    assert!(
        plan["missing"][0]
            .as_str()
            .unwrap()
            .contains("names the skill other")
    );
    let bare = settings(
        replacing(".claude/skills/team-review"),
        Some("# no front matter\n"),
    )
    .unwrap();
    let plan = routed(&bare, "review", &[], "claude");
    assert!(
        plan["missing"][0]
            .as_str()
            .unwrap()
            .contains("no front matter name")
    );
}

#[test]
fn a_scoped_skill_joins_only_the_work_its_scope_covers() {
    let skills = json!({"declared": [{"name": "team-review", "path": ".claude/skills/team-review",
        "scope": {"modules": ["api"], "intents": ["implement"]}}]});
    let settings = settings(skills, Some(SKILL)).unwrap();
    let inside = routed(&settings, "implement", &["src/api/a.rs"], "claude");
    assert!(
        inside["skills"]
            .as_array()
            .unwrap()
            .contains(&json!("project:team-review"))
    );
    let outside = routed(&settings, "implement", &["docs/a.md"], "claude");
    assert!(
        !outside["skills"]
            .as_array()
            .unwrap()
            .contains(&json!("project:team-review"))
    );
    let other_work = routed(&settings, "review", &["src/api/a.rs"], "claude");
    assert!(
        !other_work["skills"]
            .as_array()
            .unwrap()
            .contains(&json!("project:team-review"))
    );
}

#[test]
fn a_scoped_replacement_replaces_only_inside_its_scope() {
    let skills = json!({"declared": [{"name": "team-review", "path": ".claude/skills/team-review",
        "replaces": [REVIEW], "scope": {"modules": ["api"]}}]});
    let settings = settings(skills, Some(SKILL)).unwrap();
    let inside = routed(&settings, "review", &["src/api/a.rs"], "claude");
    assert_eq!(inside["skills"][0], "project:team-review");
    let outside = routed(&settings, "review", &["docs/a.md"], "claude");
    assert!(
        outside["skills"]
            .as_array()
            .unwrap()
            .contains(&json!(REVIEW))
    );
    assert!(outside.get("skillSources").is_none());
}

#[test]
fn a_disabled_package_skill_leaves_the_route_and_stays_attributed() {
    let skills = json!({"disables": ["craft-engineering:testing-engineering"]});
    let settings =
        effective_settings(&project(skills), &json!({}), &json!({})).unwrap()["settings"].take();
    let plan = routed(&settings, "implement", &[], "claude");
    assert!(
        !plan["skills"]
            .as_array()
            .unwrap()
            .contains(&json!("craft-engineering:testing-engineering"))
    );
    assert_eq!(
        plan["skillSources"],
        json!([{"skill": "craft-engineering:testing-engineering", "source": "package", "status": "disabled", "host": STILL_LISTED}])
    );
}

#[test]
fn declarations_that_conflict_or_could_never_route_are_refused() {
    let refused = |skills: Value, needle: &str| {
        let error = settings(skills, Some(SKILL)).unwrap_err();
        assert!(error.contains(needle), "{error}");
    };
    let entry = |name: &str| json!({"name": name, "path": format!(".claude/skills/{name}"), "replaces": [REVIEW]});
    refused(
        json!({"declared": [entry("a"), entry("b")]}),
        "both replace",
    );
    refused(
        json!({"declared": [entry("a")], "disables": [REVIEW]}),
        "skills.disables also names",
    );
    refused(
        json!({"declared": [{"name": "a", "path": ".claude/skills/a"}]}),
        "needs replaces or scope",
    );
    refused(
        json!({"declared": [{"name": "a", "path": "skills/a", "replaces": [REVIEW]}]}),
        "path must be",
    );
    refused(
        json!({"declared": [{"name": "a", "path": ".claude/skills/a/b", "replaces": [REVIEW]}]}),
        "path must be",
    );
    refused(
        json!({"declared": [{"name": "a", "path": ".claude/skills/a", "replaces": ["craft-x:none"]}]}),
        "unknown package skill",
    );
    refused(
        json!({"disables": ["project:mine"]}),
        "unknown package skill",
    );
    refused(
        json!({"declared": [{"name": "a", "path": ".claude/skills/a", "scope": {"modules": ["web"]}}]}),
        "unknown module web",
    );
    refused(
        json!({"declared": [entry("a"), entry("a")]}),
        "duplicate skill name",
    );
    refused(json!({"other": []}), "unknown skills field");
}

#[test]
fn hosts_list_consumer_skills_by_their_bare_name() {
    assert_eq!(host_name("project:team-review", true), "team-review");
    assert_eq!(host_name("project:team-review", false), "team-review");
    assert_eq!(host_name(REVIEW, true), REVIEW);
    assert_eq!(host_name(REVIEW, false), "craft-review-review-change");
    assert!(consumer("project:x") && !consumer("projects:x") && !consumer(REVIEW));
}

#[test]
fn front_matter_name_reads_only_the_header() {
    assert_eq!(front_matter_name(SKILL).as_deref(), Some("team-review"));
    assert_eq!(
        front_matter_name("---\nname: \"q\"\n---\n").as_deref(),
        Some("q")
    );
    assert_eq!(
        front_matter_name("---\ndescription: x\n---\nname: late\n"),
        None
    );
    assert_eq!(front_matter_name("name: x\n"), None);
}

#[test]
fn every_host_names_its_project_skill_folders() {
    for host in [
        "claude",
        "codex",
        "cursor",
        "copilot",
        "gemini",
        "gemini-extension",
        "jetbrains",
        "agents",
    ] {
        assert!(!folders(host).is_empty(), "{host}");
    }
    assert_eq!(folders("claude"), [".claude/skills"]);
    assert!(!folders("claude").contains(&".agents/skills"));
}

#[test]
fn a_route_keeps_only_skills_of_packages_the_host_installed() {
    let settings = settings(
        json!({"declared": [{"name": "team-stories", "path": ".agents/skills/team-stories",
            "replaces": ["craft-stories:ground-spec"]}]}),
        Some("---\nname: team-stories\ndescription: Our stories.\n---\nBody\n"),
    )
    .unwrap();
    let request = |packages| Request {
        intent: Some("story"),
        files: &[],
        risks: &[],
        host: "codex",
        delegate: false,
        task: None,
        verification_mode: None,
        artifact: None,
        packages,
    };
    // Unobserved: the route is the catalog's, and says it did not verify availability.
    let unverified = route(&settings, &request(None)).unwrap();
    assert_eq!(unverified["skillAvailability"], "not-verified");
    assert_eq!(
        unverified["skills"],
        json!([
            "craft-stories:workflow-prepare-stories",
            "project:team-stories"
        ])
    );
    // Observed without the stories package: its skill leaves and is attributed; the
    // project's own replacement stays, since the project serves it.
    let installed = [
        "craft-delivery".to_string(),
        "craft-engineering".to_string(),
    ];
    let plan = route(&settings, &request(Some(&installed))).unwrap();
    assert_eq!(plan["skillAvailability"], "installed-packages");
    assert_eq!(plan["skills"], json!(["project:team-stories"]));
    let left_out: Vec<&Value> = plan["skillSources"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|source| source["status"] == "not-installed")
        .collect();
    assert_eq!(
        left_out,
        [
            &json!({"skill": "craft-stories:workflow-prepare-stories", "source": "package", "status": "not-installed"})
        ]
    );
    // With the package installed nothing changes but the verification.
    let all = ["craft-delivery".to_string(), "craft-stories".to_string()];
    let plan = route(&settings, &request(Some(&all))).unwrap();
    assert_eq!(plan["skills"], unverified["skills"]);
}
