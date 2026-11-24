//! Named service setups: one `setup` name plus its target fields stands for a complete binding
//! with built-in read operations, so switching a project's tracker changes configuration only.
//! Plans also say where the shared task fields sit in each provider's response; nothing here
//! runs a command or decodes a response.
use super::change_id;
use crate::values::{object, safe_integer};
use serde_json::{Map, Value, json};

type Result<T> = std::result::Result<T, String>;

struct Setup {
    name: &'static str,
    provider: &'static str,
    services: &'static [&'static str],
    command: &'static str,
    /// The MCP server whose tool schemas the built-in MCP mappings follow; empty when none.
    server: &'static str,
}

const SETUPS: &[Setup] = &[
    Setup {
        name: "github",
        provider: "github",
        services: &["codehost", "tasks"],
        command: "gh",
        server: "github",
    },
    Setup {
        name: "github-project",
        provider: "github",
        services: &["tasks"],
        command: "gh",
        server: "",
    },
    Setup {
        name: "gitlab",
        provider: "gitlab",
        services: &["codehost", "tasks"],
        command: "glab",
        server: "",
    },
    Setup {
        name: "jira",
        provider: "jira",
        services: &["tasks"],
        command: "acli",
        server: "atlassian",
    },
];

pub const NAMES: [&str; 4] = ["github", "github-project", "gitlab", "jira"];

/// Search results returned by one built-in search.
const SEARCH_LIMIT: &str = "50";

/// Project field names read for the shared task fields when a project does not rename them.
const PROJECT_FIELDS: [(&str, &str); 3] = [
    ("status", "Status"),
    ("priority", "Priority"),
    ("release", "Release"),
];

fn setup(binding: &Value) -> Option<&'static Setup> {
    let name = binding.get("setup")?.as_str()?;
    SETUPS.iter().find(|setup| setup.name == name)
}

fn text(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
}

fn server_name(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn login(text: &str) -> bool {
    !text.is_empty()
        && !text.starts_with('-')
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Setup-specific fields of a binding that names a setup; returns its transport. Host,
/// repository identity and operation mappings are checked with every other binding.
pub fn validate(service: &str, binding: &Value) -> Result<&'static str> {
    let map = object(
        binding,
        &[
            "setup",
            "transport",
            "command",
            "server",
            "host",
            "repository",
            "project",
            "site",
            "operations",
        ],
        "setup binding",
    )?;
    let name = map["setup"].as_str().unwrap_or_default();
    let setup = setup(binding).ok_or_else(|| format!("unknown service setup: {name}"))?;
    if !setup.services.contains(&service) {
        return Err(format!("setup {name} does not serve {service}"));
    }
    let transport = match map.get("transport").map(Value::as_str) {
        None | Some(Some("cli")) => "cli",
        Some(Some("mcp")) if !setup.server.is_empty() => "mcp",
        Some(Some("mcp")) => {
            return Err(format!(
                "setup {name} has no built-in MCP mapping; use an explicit MCP binding"
            ));
        }
        _ => return Err("invalid service binding".into()),
    };
    if map.contains_key("command") && text(map.get("command")).is_none() {
        return Err("binding command required".into());
    }
    if map
        .get("server")
        .is_some_and(|server| !server.as_str().is_some_and(server_name))
    {
        return Err("invalid binding server".into());
    }
    let repository = map.get("repository").and_then(Value::as_str);
    match setup.provider {
        "github" if !repository.is_some_and(|r| r.split('/').count() == 2) => {
            return Err(format!("setup {name} requires repository owner/name"));
        }
        "gitlab" if repository.is_none() => {
            return Err(format!("setup {name} requires repository"));
        }
        _ => {}
    }
    if name == "github-project" && map.contains_key("host") {
        return Err(
            "setup github-project reads github.com only; gh project commands take no host".into(),
        );
    }
    match (name, map.get("project")) {
        ("github-project", Some(project)) => validate_project(project)?,
        ("github-project", None) => {
            return Err("setup github-project requires project owner and number".into());
        }
        (_, Some(_)) => return Err(format!("setup {name} takes no project")),
        _ => {}
    }
    match (name, transport, map.get("site")) {
        ("jira", "mcp", None) => return Err("setup jira over MCP requires site".into()),
        ("jira", _, Some(site)) if !site.as_str().is_some_and(crate::contracts::host_name) => {
            return Err("invalid binding site".into());
        }
        ("jira", _, _) => {}
        (_, _, Some(_)) => return Err(format!("setup {name} takes no site")),
        _ => {}
    }
    Ok(transport)
}

fn validate_project(project: &Value) -> Result<()> {
    let project = object(project, &["owner", "number", "fields"], "project")?;
    if !text(project.get("owner")).is_some_and(login) {
        return Err("invalid project owner".into());
    }
    if !project
        .get("number")
        .and_then(safe_integer)
        .is_some_and(|number| number > 0)
    {
        return Err("project number must be a positive integer".into());
    }
    if let Some(fields) = project.get("fields") {
        let names: Vec<&str> = PROJECT_FIELDS.iter().map(|(field, _)| *field).collect();
        for value in object(fields, &names, "project fields")?.values() {
            if text(Some(value)).is_none() {
                return Err("project field names must be non-empty strings".into());
            }
        }
    }
    Ok(())
}

/// The binding a setup stands for: provider, transport and command or server filled in;
/// a command or server written in the binding wins over the setup's default.
pub fn expand(binding: &Value) -> Value {
    let Some(setup) = setup(binding) else {
        return binding.clone();
    };
    let mut expanded = binding.clone();
    expanded["provider"] = json!(setup.provider);
    let mcp = binding["transport"] == "mcp";
    expanded["transport"] = json!(if mcp { "mcp" } else { "cli" });
    if mcp {
        if binding.get("server").is_none() {
            expanded["server"] = json!(setup.server);
        }
    } else if binding.get("command").is_none() {
        expanded["command"] = json!(setup.command);
    }
    expanded
}

/// Whether a setup supplies `operation` for this binding. CLI code-host reads stay with the
/// GitHub and GitLab built-ins every binding of those providers already has.
pub fn supplies(binding: &Value, operation: &str) -> bool {
    let Some(setup) = setup(binding) else {
        return false;
    };
    let mcp = binding["transport"] == "mcp";
    match operation {
        "task.read" | "task.search" => setup.services.contains(&"tasks"),
        "change.read" | "change.diff" => mcp && setup.services.contains(&"codehost"),
        _ => false,
    }
}

/// Parameter names a supplied operation needs.
pub fn required(operation: &str) -> &'static [&'static str] {
    match operation {
        "task.search" => &["query"],
        _ => &["id"],
    }
}

/// The MCP tool a supplied operation calls, for session metadata.
pub fn tool(binding: &Value, operation: &str) -> Option<&'static str> {
    if binding["transport"] != "mcp" {
        return None;
    }
    Some(match (binding["provider"].as_str()?, operation) {
        ("github", "task.read") => "issue_read",
        ("github", "task.search") => "search_issues",
        ("github", _) => "pull_request_read",
        ("jira", "task.read") => "getJiraIssue",
        ("jira", _) => "searchJiraIssuesUsingJql",
        _ => return None,
    })
}

/// Whether a host inventory name is `tool` on `server`: the bare name, or the host's
/// qualified form such as `mcp__github__issue_read` or `github.issue_read`.
pub fn observed(seen: &str, server: &str, tool: &str) -> bool {
    if seen == tool {
        return true;
    }
    seen.strip_suffix(tool)
        .filter(|prefix| prefix.ends_with(['_', '.', ':', '/']))
        .map(|prefix| prefix.trim_end_matches(['_', '.', ':', '/']))
        .is_some_and(|prefix| {
            prefix
                .strip_suffix(server)
                .is_some_and(|rest| rest.is_empty() || rest.ends_with(['_', '.', ':', '/']))
        })
}

pub enum Invocation {
    Cli(Vec<String>),
    Mcp {
        tool: &'static str,
        arguments: Value,
    },
}

fn query(params: &Map<String, Value>) -> Result<String> {
    params
        .get("query")
        .and_then(Value::as_str)
        .filter(|query| !query.trim().is_empty() && !query.chars().any(char::is_control))
        .map(str::to_string)
        .ok_or_else(|| "query must be a non-empty single-line string".into())
}

/// A Jira work item key such as `TEAM-123`.
fn issue_key(params: &Map<String, Value>) -> Result<String> {
    params
        .get("id")
        .and_then(Value::as_str)
        .filter(|key| {
            key.split_once('-').is_some_and(|(project, number)| {
                project
                    .bytes()
                    .next()
                    .is_some_and(|b| b.is_ascii_uppercase())
                    && project
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
                    && number
                        .bytes()
                        .next()
                        .is_some_and(|b| (b'1'..=b'9').contains(&b))
                    && number.bytes().all(|b| b.is_ascii_digit())
            })
        })
        .map(str::to_string)
        .ok_or_else(|| "Jira key must look like TEAM-123".into())
}

fn owner_repo(binding: &Value) -> (String, String) {
    let repository = binding["repository"].as_str().unwrap_or_default();
    let (owner, name) = repository.split_once('/').unwrap_or_default();
    (owner.into(), name.into())
}

fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| item.to_string()).collect()
}

/// The arguments for one supplied operation; parameter values are validated, never re-read
/// as placeholders, and searches pass their query as `--flag=value` so it is never an option.
pub fn invocation(
    binding: &Value,
    operation: &str,
    params: &Map<String, Value>,
) -> Result<Invocation> {
    let setup = setup(binding).ok_or("binding names no setup")?;
    let mcp = binding["transport"] == "mcp";
    let (owner, name) = owner_repo(binding);
    Ok(match (setup.name, mcp, operation) {
        ("github", false, "task.read") => {
            let mut items = args(&["issue", "view"]);
            items.extend([issue_number(params)?, "--repo".into(), github_repo(binding)]);
            items.extend(args(&["--json", GITHUB_ISSUE_JSON]));
            Invocation::Cli(items)
        }
        ("github", false, _) => {
            let mut items = args(&["issue", "list", "--repo"]);
            items.extend([github_repo(binding), format!("--search={}", query(params)?)]);
            items.extend(args(&[
                "--json",
                GITHUB_ISSUE_JSON,
                "--limit",
                SEARCH_LIMIT,
            ]));
            Invocation::Cli(items)
        }
        ("github", true, "task.read") => Invocation::Mcp {
            tool: "issue_read",
            arguments: json!({"method": "get", "owner": owner, "repo": name,
                              "issue_number": number(issue_number(params)?)?}),
        },
        ("github", true, "task.search") => Invocation::Mcp {
            tool: "search_issues",
            arguments: json!({"query": format!("repo:{owner}/{name} {}", query(params)?),
                              "perPage": 50}),
        },
        ("github", true, _) => Invocation::Mcp {
            tool: "pull_request_read",
            arguments: json!({
                "method": if operation == "change.diff" { "get_diff" } else { "get" },
                "owner": owner, "repo": name, "pullNumber": number(change_id(params.get("id"))?)?,
            }),
        },
        ("github-project", _, "task.read") => {
            let id = issue_number(params)?;
            let mut items = args(&["api", "graphql"]);
            items.extend([
                "-f".into(),
                format!("query={PROJECT_ISSUE_QUERY}"),
                "-F".into(),
                format!("owner={owner}"),
                "-F".into(),
                format!("repo={name}"),
                "-F".into(),
                format!("number={id}"),
                "--jq".into(),
                project_issue_filter(binding),
            ]);
            Invocation::Cli(items)
        }
        ("github-project", _, _) => {
            let project = &binding["project"];
            let mut items = args(&["project", "item-list"]);
            items.extend([
                project_number(project),
                "--owner".into(),
                project["owner"].as_str().unwrap_or_default().into(),
                "--format".into(),
                "json".into(),
                "--limit".into(),
                SEARCH_LIMIT.into(),
                format!("--query={}", query(params)?),
                "--jq".into(),
                project_items_filter(binding),
            ]);
            Invocation::Cli(items)
        }
        ("gitlab", _, "task.read") => {
            let mut items = args(&["issue", "view"]);
            items.extend([issue_number(params)?, "--repo".into(), gitlab_repo(binding)]);
            items.extend(args(&["--output", "json"]));
            Invocation::Cli(items)
        }
        ("gitlab", _, _) => {
            let mut items = args(&["issue", "list", "--repo"]);
            items.extend([gitlab_repo(binding), format!("--search={}", query(params)?)]);
            items.extend(args(&["--output", "json", "--per-page", SEARCH_LIMIT]));
            Invocation::Cli(items)
        }
        ("jira", false, "task.read") => {
            let mut items = args(&["jira", "workitem", "view"]);
            items.extend([issue_key(params)?, format!("--fields={JIRA_FIELDS}")]);
            items.push("--json".into());
            Invocation::Cli(items)
        }
        ("jira", false, _) => {
            let mut items = args(&["jira", "workitem", "search"]);
            items.extend([
                format!("--jql={}", query(params)?),
                format!("--fields={JIRA_FIELDS}"),
                "--limit".into(),
                SEARCH_LIMIT.into(),
            ]);
            items.push("--json".into());
            Invocation::Cli(items)
        }
        ("jira", true, "task.read") => Invocation::Mcp {
            tool: "getJiraIssue",
            arguments: json!({"cloudId": binding["site"], "issueIdOrKey": issue_key(params)?}),
        },
        ("jira", true, _) => Invocation::Mcp {
            tool: "searchJiraIssuesUsingJql",
            arguments: json!({"cloudId": binding["site"], "jql": query(params)?,
                              "fields": JIRA_FIELDS.split(',').collect::<Vec<_>>(),
                              "maxResults": 50}),
        },
        _ => {
            return Err(format!(
                "setup {} has no mapping for {operation}",
                setup.name
            ));
        }
    })
}

/// The issue number `task.read` takes, from `id`.
fn issue_number(params: &Map<String, Value>) -> Result<String> {
    change_id(params.get("id")).map_err(|_| {
        "task.read takes an issue number as its id: a positive integer such as 145".to_string()
    })
}

/// A validated positive id as a JSON number, for MCP tools whose schemas take integers.
fn number(id: String) -> Result<Value> {
    Ok(json!(id.parse::<u64>().map_err(|e| e.to_string())?))
}

fn project_number(project: &Value) -> String {
    project["number"]
        .as_f64()
        .map(|number| (number as u64).to_string())
        .unwrap_or_default()
}

fn github_repo(binding: &Value) -> String {
    let repository = binding["repository"].as_str().unwrap_or_default();
    match binding["host"].as_str() {
        Some(host) => format!("{host}/{repository}"),
        None => repository.into(),
    }
}

fn gitlab_repo(binding: &Value) -> String {
    let repository = binding["repository"].as_str().unwrap_or_default();
    match binding["host"].as_str() {
        Some(host) => format!("https://{host}/{repository}"),
        None => repository.into(),
    }
}

const GITHUB_ISSUE_JSON: &str = "number,title,body,state,labels,milestone,url";
const JIRA_FIELDS: &str = "summary,description,status,priority,fixVersions,labels";

const PROJECT_ISSUE_QUERY: &str = "query($owner:String!,$repo:String!,$number:Int!){repository(owner:$owner,name:$repo){issue(number:$number){number title body state url labels(first:50){nodes{name}} projectItems(first:20){nodes{project{number owner{... on User{login} ... on Organization{login}}} fieldValues(first:50){nodes{... on ProjectV2ItemFieldSingleSelectValue{name field{... on ProjectV2FieldCommon{name}}} ... on ProjectV2ItemFieldTextValue{text field{... on ProjectV2FieldCommon{name}}} ... on ProjectV2ItemFieldIterationValue{title field{... on ProjectV2FieldCommon{name}}}}}}}}}}";

/// Configured Project field names as jq string literals: a JSON string is a valid jq literal
/// that cannot start an interpolation, so names never become program text.
fn project_fields(binding: &Value) -> Vec<(&'static str, String)> {
    PROJECT_FIELDS
        .iter()
        .map(|(field, default)| {
            let name = binding
                .pointer(&format!("/project/fields/{field}"))
                .and_then(Value::as_str)
                .unwrap_or(default);
            (*field, Value::String(name.into()).to_string())
        })
        .collect()
}

/// One issue with its labels and the configured Project's status, priority and release.
fn project_issue_filter(binding: &Value) -> String {
    let project = &binding["project"];
    let owner = Value::String(project["owner"].as_str().unwrap_or_default().into());
    let picks = project_fields(binding)
        .iter()
        .map(|(field, name)| format!("{field}: $f[{name}]"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        ".data.repository.issue | ([.projectItems.nodes[] | select(.project.number == {number} \
         and .project.owner.login == {owner}) | .fieldValues.nodes[] | select(.field) \
         | {{key: .field.name, value: (.name // .text // .title)}}] | from_entries) as $f \
         | {{number, title, body, state, url, labels: [.labels.nodes[].name], {picks}}}",
        number = project_number(project),
    )
}

/// Project items in the shape `task.read` returns; gh names item fields in lower case. Draft
/// items have no issue to read or link and are left out.
fn project_items_filter(binding: &Value) -> String {
    let picks = project_fields(binding)
        .iter()
        .map(|(field, name)| format!("{field}: .[{name} | ascii_downcase]"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "[.items[] | select(.content.type != \"DraftIssue\") | {{number: .content.number, \
         title, body: .content.body, url: .content.url, type: .content.type, labels, {picks}}}]"
    )
}

/// The value at a dotted `path` of `value`; paths with `[]` name lists and are not read here.
fn at<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .try_fold(value, |value, step| value.get(step))
}

/// Single-line text of at most `limit` characters, `None` when blank.
fn line(value: Option<&Value>, limit: usize) -> Option<String> {
    let text: String = value?
        .as_str()?
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(limit)
        .collect();
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// The identity of the one issue a `task.read` response holds: the setup, the tracker's id,
/// title, status and an `https` URL, where the response or the binding's site gives one.
pub fn identity(binding: &Value, response: &Value) -> Result<Value> {
    let setup = setup(binding).ok_or("binding names no setup")?;
    let located = fields(binding, "task.read").ok_or("setup has no task.read")?;
    let field = |name: &str| {
        located["fields"][name]
            .as_str()
            .and_then(|path| at(response, path))
    };
    let id = match field("id") {
        Some(Value::Number(number)) => safe_integer(&Value::Number(number.clone()))
            .filter(|n| *n > 0)
            .map(|n| n.to_string()),
        other => line(other, 64),
    }
    .ok_or("the response holds no issue id; pass the JSON task.read returned for one issue")?;
    let url = line(field("url"), 2048)
        .or_else(|| {
            binding["site"]
                .as_str()
                .map(|site| format!("https://{site}/browse/{id}"))
        })
        .filter(|url| url.starts_with("https://"));
    Ok(json!({
        "service": setup.name,
        "id": id,
        "title": line(field("title"), 256),
        "status": line(field("status"), 64),
        "url": url,
    }))
}

/// Where the shared task fields sit in the response: dotted paths, `[]` for each element,
/// and `list` for the array of results a search returns. Absent fields are not provided.
pub fn fields(binding: &Value, operation: &str) -> Option<Value> {
    if !operation.starts_with("task.") {
        return None;
    }
    let setup = setup(binding)?;
    let mcp = binding["transport"] == "mcp";
    let search = operation == "task.search";
    let (list, fields) = match (setup.name, mcp) {
        ("github", false) => (
            "[]",
            json!({"id": "number", "title": "title", "body": "body", "status": "state",
                   "labels": "labels[].name", "release": "milestone.title", "url": "url"}),
        ),
        ("github", true) => (
            "items[]",
            json!({"id": "number", "title": "title", "body": "body", "status": "state",
                   "labels": "labels[].name", "release": "milestone.title", "url": "html_url"}),
        ),
        ("github-project", _) => (
            "[]",
            json!({"id": "number", "title": "title", "body": "body", "status": "status",
                   "priority": "priority", "release": "release", "labels": "labels[]",
                   "url": "url"}),
        ),
        ("gitlab", _) => (
            "[]",
            json!({"id": "iid", "title": "title", "body": "description", "status": "state",
                   "labels": "labels[]", "release": "milestone.title", "url": "web_url"}),
        ),
        _ => (
            if mcp { "issues[]" } else { "[]" },
            json!({"id": "key", "title": "fields.summary", "body": "fields.description",
                   "status": "fields.status.name", "priority": "fields.priority.name",
                   "release": "fields.fixVersions[].name", "labels": "fields.labels[]"}),
        ),
    };
    let mut response = json!({ "fields": fields });
    if search {
        response["list"] = json!(list);
    }
    Some(response)
}

#[cfg(test)]
mod tests;
