//! Service setup discovery: the `services` binding that facts already on the machine support,
//! with the fact behind each choice. Facts are the Git remote, the issue, project or Jira URLs
//! the project's prompts and plans named, the CLIs found on `PATH` and the MCP tools a host
//! reports. Without a supporting fact nothing is proposed, and a configured binding is kept.
use super::setups;
use serde_json::{Map, Value, json};

/// Tracker identities a project keeps, oldest dropped first.
pub const KEPT: usize = 16;

/// URLs read from one prompt.
const PER_TEXT: usize = 8;

const SERVICES: [&str; 2] = ["codehost", "tasks"];

/// A path segment of a repository: no empty, `.` or `..` part, nothing a shell or URL reads.
fn segment(part: &str) -> bool {
    !part.is_empty()
        && part != "."
        && part != ".."
        && part
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

/// A web host: a name with a dot, optionally a port.
fn web_host(host: &str) -> bool {
    crate::contracts::host_name(host) && host.split(':').next().is_some_and(|n| n.contains('.'))
}

fn positive(text: &str) -> Option<u64> {
    (!text.is_empty() && text.len() <= 9 && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
        .filter(|n| *n > 0)
}

fn login(text: &str) -> bool {
    !text.is_empty()
        && !text.starts_with('-')
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Host and repository path a remote URL names, for `https://`, `ssh://` and scp-like
/// `user@host:path` forms. Credentials are dropped, and an SSH port is not the web host's.
pub fn remote(url: &str) -> Option<(String, String)> {
    let url = url.trim();
    let (host, path) = match url.split_once("://") {
        Some((scheme, rest)) => {
            let ssh = match scheme.to_ascii_lowercase().as_str() {
                "https" | "http" | "git" => false,
                "ssh" | "git+ssh" | "ssh+git" => true,
                _ => return None,
            };
            let (authority, path) = rest.split_once('/')?;
            let host = authority
                .rsplit_once('@')
                .map_or(authority, |(_, host)| host);
            (if ssh { host.split(':').next()? } else { host }, path)
        }
        None => {
            let (authority, path) = url.split_once(':')?;
            if authority.contains('/') || path.starts_with('/') {
                return None;
            }
            (
                authority
                    .rsplit_once('@')
                    .map_or(authority, |(_, host)| host),
                path,
            )
        }
    };
    let host = host.to_ascii_lowercase();
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let parts: Vec<&str> = path.split('/').collect();
    (web_host(&host) && parts.len() >= 2 && parts.iter().all(|p| segment(p)))
        .then(|| (host, parts.join("/")))
}

/// The tracker a URL names: a GitHub issue on github.com, a GitHub Project, a GitLab issue or
/// work item, or a Jira Cloud work item (`*.atlassian.net`). Other hosts use the same path
/// shapes for other things, so they name nothing. Only identity is kept: provider, host,
/// repository, project or site.
pub fn tracker(url: &str) -> Option<Value> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let rest = rest.split(['?', '#']).next()?;
    let (host, path) = rest.split_once('/')?;
    let host = host.to_ascii_lowercase();
    if !web_host(&host) {
        return None;
    }
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    if host == "github.com"
        && parts.len() >= 4
        && matches!(parts[0], "users" | "orgs")
        && login(parts[1])
        && parts[2] == "projects"
        && let Some(number) = positive(parts[3])
    {
        return Some(json!({"provider": "github-project", "owner": parts[1], "number": number}));
    }
    if let Some(at) = parts.iter().position(|p| *p == "-")
        && at >= 2
        && matches!(parts.get(at + 1), Some(&("issues" | "work_items")))
        && parts.get(at + 2).and_then(|n| positive(n)).is_some()
        && parts[..at].iter().all(|p| segment(p))
    {
        return Some(
            json!({"provider": "gitlab", "host": host, "repository": parts[..at].join("/")}),
        );
    }
    if host == "github.com"
        && parts.len() >= 4
        && parts[2] == "issues"
        && positive(parts[3]).is_some()
        && segment(parts[0])
        && segment(parts[1])
    {
        let repository = format!("{}/{}", parts[0], parts[1]);
        return Some(json!({"provider": "github", "host": host, "repository": repository}));
    }
    if host.ends_with(".atlassian.net")
        && parts.len() >= 2
        && parts[0] == "browse"
        && crate::ticket::key(parts[1])
    {
        return Some(json!({"provider": "jira", "site": host}));
    }
    None
}

/// The trackers the URLs in `text` name, each once, in order.
pub fn trackers(text: &str) -> Vec<Value> {
    let mut found: Vec<Value> = Vec::new();
    for word in text.split_whitespace() {
        let word = word.trim_matches(|c: char| "<>()[]{}\"'`,.;:!".contains(c));
        let Some(start) = word.find("http") else {
            continue;
        };
        if let Some(identity) = tracker(&word[start..])
            && !found.contains(&identity)
        {
            found.push(identity);
            if found.len() == PER_TEXT {
                break;
            }
        }
    }
    found
}

/// The trackers a kept record holds; none from a record this build does not know.
pub fn kept(record: &Value) -> Vec<Value> {
    record["trackers"]
        .as_array()
        .filter(|_| record["formatVersion"] == 1)
        .cloned()
        .unwrap_or_default()
}

/// The kept record with `found` added after its trackers, or `None` when every one is already
/// kept, or the record is one this build does not know: a newer build's record is left as it
/// is. The record holds identities only, never the text that named them.
pub fn keep(record: &Value, found: &[Value]) -> Option<Value> {
    if !record.is_null() && record["formatVersion"] != 1 {
        return None;
    }
    let mut trackers = kept(record);
    let mut added = false;
    for identity in found {
        if !trackers.contains(identity) {
            trackers.push(identity.clone());
            added = true;
        }
    }
    if !added {
        return None;
    }
    let extra = trackers.len().saturating_sub(KEPT);
    trackers.drain(..extra);
    Some(json!({"formatVersion": 1, "trackers": trackers}))
}

fn names<'a>(facts: &'a Value, field: &str) -> Vec<&'a str> {
    facts[field]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

/// `github` or `gitlab` for a code host, and the fact that says so.
fn host_kind(host: &str, trackers: &[Value]) -> Option<(&'static str, String)> {
    match host {
        "github.com" => return Some(("github", format!("the remote host is {host}"))),
        "gitlab.com" => return Some(("gitlab", format!("the remote host is {host}"))),
        _ => {}
    }
    // GitHub issue URLs are read on github.com only, so another host shows only GitLab.
    if trackers
        .iter()
        .any(|t| t["provider"] == "gitlab" && t["host"] == host)
    {
        return Some((
            "gitlab",
            format!("a GitLab issue URL on {host} was named in this project"),
        ));
    }
    None
}

/// The code host binding the remote supports: `origin`, or the one repository every remote
/// names.
fn codehost(facts: &Value, trackers: &[Value]) -> Result<(Value, Vec<String>), String> {
    let remotes = facts["remotes"].as_array().cloned().unwrap_or_default();
    if remotes.is_empty() {
        return Err("no Git remote is configured".into());
    }
    let parsed: Vec<(String, String, String)> = remotes
        .iter()
        .filter_map(|r| {
            let (host, path) = remote(r["url"].as_str()?)?;
            Some((r["name"].as_str().unwrap_or("?").to_string(), host, path))
        })
        .collect();
    let chosen = parsed
        .iter()
        .find(|(name, ..)| name == "origin")
        .or_else(|| {
            parsed
                .iter()
                .all(|(_, h, p)| (h, p) == (&parsed[0].1, &parsed[0].2))
                .then(|| parsed.first())
                .flatten()
        });
    let Some((name, host, path)) = chosen else {
        return Err(if parsed.is_empty() {
            "no remote URL names a host and repository".into()
        } else {
            "several remotes name different repositories and none is origin".into()
        });
    };
    let (setup, why) = host_kind(host, trackers)
        .ok_or_else(|| format!("remote {name} host {host} is not known as GitHub or GitLab"))?;
    if setup == "github" && path.split('/').count() != 2 {
        return Err(format!(
            "remote {name} path {path} is not a GitHub owner/name"
        ));
    }
    let mut binding = json!({"setup": setup, "repository": path});
    if !matches!(host.as_str(), "github.com" | "gitlab.com") {
        binding["host"] = json!(host);
    }
    Ok((
        binding,
        vec![format!("remote {name} is {host}/{path}"), why],
    ))
}

fn label(tracker: &Value) -> String {
    match tracker["provider"].as_str() {
        Some("github-project") => format!(
            "GitHub Project {}/{}",
            tracker["owner"].as_str().unwrap_or("?"),
            tracker["number"]
        ),
        Some("jira") => format!("Jira {}", tracker["site"].as_str().unwrap_or("?")),
        Some(provider) => format!(
            "{} {}/{}",
            if provider == "gitlab" {
                "GitLab"
            } else {
                "GitHub"
            },
            tracker["host"].as_str().unwrap_or("?"),
            tracker["repository"].as_str().unwrap_or("?")
        ),
        None => "?".into(),
    }
}

/// This project's code host as a tracker identity compares: provider, host and repository.
fn own_repository(codehost: Option<&Value>) -> Option<(&str, &str, &str)> {
    let binding = codehost?;
    let provider = match binding["setup"].as_str().or(binding["provider"].as_str())? {
        "github" => "github",
        "gitlab" => "gitlab",
        _ => return None,
    };
    let host = binding["host"].as_str().unwrap_or(if provider == "github" {
        "github.com"
    } else {
        "gitlab.com"
    });
    Some((provider, host, binding["repository"].as_str()?))
}

fn several(named: &[&Value]) -> String {
    let named: Vec<String> = named.iter().map(|t| label(t)).collect();
    format!(
        "several trackers were named: {}; choose one with configure-service",
        named.join(", ")
    )
}

/// The tasks binding the named trackers support. Issues count only when they are this
/// project's: the code host's repository. A GitHub Project counts when its owner owns that
/// repository, and it is the tracker of the repository's issues. Trackers of different kinds,
/// or several of one kind, leave the choice to the consumer.
fn tasks(trackers: &[Value], codehost: Option<&Value>) -> Result<(Value, Vec<String>), String> {
    if trackers.is_empty() {
        return Err(
            "no issue, GitHub Project or Jira URL has been named in this project's prompts or plans"
                .into(),
        );
    }
    let own = own_repository(codehost);
    let mine = |t: &&Value| {
        own.is_some_and(|(provider, host, repository)| {
            t["provider"] == provider && t["host"] == host && t["repository"] == repository
        })
    };
    let issues: Vec<&Value> = trackers
        .iter()
        .filter(|t| matches!(t["provider"].as_str(), Some("github" | "gitlab")))
        .collect();
    let projects: Vec<&Value> = trackers
        .iter()
        .filter(|t| t["provider"] == "github-project")
        .collect();
    let sites: Vec<&Value> = trackers
        .iter()
        .filter(|t| t["provider"] == "jira")
        .collect();
    let foreign: Vec<&Value> = issues.iter().copied().filter(|t| !mine(t)).collect();
    let github = own.filter(|(provider, host, _)| *provider == "github" && *host == "github.com");
    let owned: Vec<&Value> = projects
        .iter()
        .copied()
        .filter(|p| {
            github.is_some_and(|(_, _, repository)| {
                repository.split('/').next() == p["owner"].as_str()
            })
        })
        .collect();
    let mut candidates: Vec<&Value> = if owned.is_empty() {
        issues.iter().copied().filter(mine).take(1).collect()
    } else {
        // The project holds the repository's issues: they do not compete with it.
        owned.clone()
    };
    candidates.extend(&sites);
    match candidates.as_slice() {
        [] => {
            let mut reasons = Vec::new();
            if !projects.is_empty() {
                reasons.push(match github {
                    None => "a GitHub Project was named, but no GitHub repository on github.com is this project's code host".to_string(),
                    Some((_, _, repository)) => format!(
                        "{} was named, but its owner does not own {repository}",
                        projects.iter().map(|p| label(p)).collect::<Vec<_>>().join(", ")
                    ),
                });
            }
            if !foreign.is_empty() {
                let named: Vec<String> = foreign.iter().map(|t| label(t)).collect();
                reasons.push(match own {
                    Some(_) => format!(
                        "{} named, but not this project's repository",
                        named.join(", ")
                    ),
                    None => format!(
                        "{} named, but no code host shows it is this project's",
                        named.join(", ")
                    ),
                });
            }
            Err(reasons.join("; "))
        }
        [chosen] => {
            let provider = chosen["provider"].as_str().unwrap_or_default();
            let (owner_repository, binding) = match provider {
                "github-project" => {
                    let repository = github.map(|(_, _, r)| r).unwrap_or_default();
                    (
                        Some(repository),
                        json!({"setup": "github-project", "repository": repository,
                            "project": {"owner": chosen["owner"], "number": chosen["number"]}}),
                    )
                }
                "jira" => (None, json!({"setup": "jira", "site": chosen["site"]})),
                _ => {
                    let mut binding =
                        json!({"setup": provider, "repository": chosen["repository"]});
                    if !matches!(chosen["host"].as_str(), Some("github.com" | "gitlab.com")) {
                        binding["host"] = chosen["host"].clone();
                    }
                    (None, binding)
                }
            };
            let mut why = vec![format!("{} was named in this project", label(chosen))];
            if let Some(repository) = owner_repository {
                why.push(format!(
                    "its owner owns the code host repository {repository}"
                ));
            }
            Ok((binding, why))
        }
        several_named => Err(several(several_named)),
    }
}

/// The transport the machine supports: the setup's CLI on `PATH`, otherwise its MCP server
/// when the host reports that server's read tool. With neither, the CLI default stays and
/// the proposal says what is missing.
fn transport(service: &str, binding: &mut Value, facts: &Value, why: &mut Vec<String>) -> Value {
    let setup = binding["setup"].as_str().unwrap_or_default();
    let Some((command, server)) = setups::transports(setup) else {
        return Value::Null;
    };
    if names(facts, "commands").contains(&command) {
        why.push(format!("{command} is on PATH"));
        return Value::Null;
    }
    let provider = if setup == "github-project" {
        "github"
    } else {
        setup
    };
    let operation = if service == "tasks" {
        "task.read"
    } else {
        "change.read"
    };
    let tool = setups::tool(
        &json!({"transport": "mcp", "provider": provider}),
        operation,
    );
    if !server.is_empty()
        && let Some(tool) = tool
        && let Some(seen) = names(facts, "tools")
            .into_iter()
            .find(|seen| setups::observed(seen, server, tool))
    {
        binding["transport"] = json!("mcp");
        why.push(format!("the host reports MCP tool {seen}"));
        return Value::Null;
    }
    json!(format!(
        "{command} was not found on PATH: install it{} before reads work",
        if server.is_empty() {
            ""
        } else {
            ", or connect the MCP server,"
        }
    ))
}

/// The proposal for `settings` over `facts` (`{remotes: [{name, url}], trackers, commands,
/// tools}`): per service `configured`, `proposed` with its binding and facts, or `none` with
/// the reason; `merge` holds the proposed bindings only.
pub fn propose(settings: &Value, facts: &Value) -> Value {
    let trackers: Vec<Value> = facts["trackers"].as_array().cloned().unwrap_or_default();
    let configured = |service: &str| settings.pointer(&format!("/services/{service}")).cloned();
    let mut services = Map::new();
    let mut merge = Map::new();
    let mut known_codehost = configured("codehost").map(|b| setups::expand(&b));
    for service in SERVICES {
        if configured(service).is_some() {
            services.insert(service.into(), json!({"status": "configured"}));
            continue;
        }
        let found = match service {
            "codehost" => codehost(facts, &trackers),
            _ => tasks(&trackers, known_codehost.as_ref()),
        };
        let entry = match found {
            Ok((mut binding, mut why)) => {
                let missing = transport(service, &mut binding, facts, &mut why);
                match setups::validate(service, &binding) {
                    Ok(_) => {
                        if service == "codehost" {
                            known_codehost = Some(binding.clone());
                        }
                        merge.insert(service.into(), binding.clone());
                        let mut entry =
                            json!({"status": "proposed", "binding": binding, "facts": why});
                        if !missing.is_null() {
                            entry["missing"] = missing;
                        }
                        entry
                    }
                    Err(reason) => json!({"status": "none", "reason": reason}),
                }
            }
            Err(reason) => json!({"status": "none", "reason": reason}),
        };
        services.insert(service.into(), entry);
    }
    let mut proposal = json!({"formatVersion": 1, "services": services});
    if !merge.is_empty() {
        proposal["merge"] = json!({"services": merge});
    }
    proposal
}

/// `project` with the proposed bindings added under `services`; a binding already there is
/// never replaced. `None` when there is nothing to add.
pub fn apply(project: &Value, proposal: &Value) -> Option<Value> {
    let proposed = proposal.pointer("/merge/services")?.as_object()?;
    let mut project = project.clone();
    if !project.is_object() {
        return None;
    }
    if !project["services"].is_object() {
        if project.get("services").is_some() {
            return None;
        }
        project["services"] = json!({});
    }
    let services = project["services"].as_object_mut()?;
    let mut added = false;
    for (service, binding) in proposed {
        if !services.contains_key(service) {
            services.insert(service.clone(), binding.clone());
            added = true;
        }
    }
    added.then_some(project)
}

/// A short description of each proposed binding: `codehost github acme/shop`.
fn summary(proposal: &Value) -> Vec<String> {
    let Some(merge) = proposal
        .pointer("/merge/services")
        .and_then(Value::as_object)
    else {
        return Vec::new();
    };
    merge
        .iter()
        .map(|(service, binding)| {
            let target = match binding["setup"].as_str() {
                Some("github-project") => format!(
                    "{}/{}",
                    binding["project"]["owner"].as_str().unwrap_or("?"),
                    binding["project"]["number"]
                ),
                Some("jira") => binding["site"].as_str().unwrap_or("?").to_string(),
                _ => binding["repository"].as_str().unwrap_or("?").to_string(),
            };
            format!(
                "{service} {} {target}",
                binding["setup"].as_str().unwrap_or("?")
            )
        })
        .collect()
}

/// The line a session start and `doctor` say about a proposal, `None` when none is made.
pub fn line(proposal: &Value) -> Option<String> {
    let proposed = summary(proposal);
    (!proposed.is_empty()).then(|| {
        format!(
            "Craft: facts on this machine support a service setup this project has not configured ({}). `craft service-setup` shows the facts; `craft service-setup --apply` adds it to .craft/project.json.",
            proposed.join("; ")
        )
    })
}

/// Whether a session start says `proposal`: once per distinct proposal, recorded in `shown`.
/// Returns the record to keep when it is said.
pub fn offer(proposal: &Value, shown: &Value) -> Option<Value> {
    if !shown.is_null() && shown["formatVersion"] != 1 {
        return None;
    }
    let merge = proposal.get("merge")?;
    let digest = crate::hash(merge.to_string().as_bytes());
    (shown["digest"] != digest.as_str()).then(|| json!({"formatVersion": 1, "digest": digest}))
}

#[cfg(test)]
mod tests;
