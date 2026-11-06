//! Anchors: established standards a project names in one word instead of describing them.
//! Two embedded files hold them. `catalog.json` is generated from the Semantic Anchors catalog
//! (see `anchors/NOTICE`) and says what a name means; `overlay.json` is Craft's own and says
//! which names Craft offers, at which tier, with which check, defaults and evidence. A name is
//! offered only when the overlay lists it; the catalog alone grants nothing.
use crate::contracts::kebab;
use crate::values::object;
use serde_json::Value;
use std::sync::OnceLock;

pub mod checks;

pub use checks::Finding;

type Result<T = ()> = std::result::Result<T, String>;

const OVERLAY: &str = include_str!("../anchors/overlay.json");
const CATALOG: &str = include_str!("../anchors/catalog.json");
const MAX_SUMMARY: usize = 200;

/// What Craft can say about work that names an anchor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// Required elements are checkable from the text.
    Structure,
    /// Checkable from facts Craft holds about the project.
    Facts,
    /// Shared meaning only; never reported as verified.
    Vocabulary,
    /// Not offered: models do not agree on what the name requires.
    Refused,
}

impl Tier {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "structure" => Self::Structure,
            "facts" => Self::Facts,
            "vocabulary" => Self::Vocabulary,
            "refused" => Self::Refused,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Structure => "structure",
            Self::Facts => "facts",
            Self::Vocabulary => "vocabulary",
            Self::Refused => "refused",
        }
    }
}

/// What a check reads: a commit message, or a file written in the standard's scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    CommitMessage,
    File,
}

#[derive(Debug)]
pub struct Anchor {
    pub name: String,
    /// The catalog id this name stands for; refused names may have none.
    pub catalog: Option<String>,
    pub tier: Tier,
    /// The deterministic check, for structure anchors.
    pub check: Option<String>,
    pub input: Input,
    /// Route intents the standard applies to when the consumer declares no scope.intents; empty = any.
    pub intents: Vec<String>,
    /// Files the check reads when the consumer declares no scope.paths: a file name ("CHANGELOG.md") or an extension (".feature"); empty = consumer must declare paths (commit messages need none).
    pub targets: Vec<String>,
    /// One line the model is shown instead of the standard's text; refused names have none.
    pub summary: Option<String>,
    /// Why the name has its tier.
    pub evidence: String,
}

fn strings(entry: &serde_json::Map<String, Value>, field: &str, name: &str) -> Result<Vec<String>> {
    let Some(value) = entry.get(field) else {
        return Ok(Vec::new());
    };
    value
        .as_array()
        .ok_or_else(|| format!("anchor {name}: {field} must be an array"))?
        .iter()
        .map(|item| {
            item.as_str()
                .filter(|text| !text.trim().is_empty())
                .map(str::to_owned)
                .ok_or_else(|| format!("anchor {name}: {field} entries must be text"))
        })
        .collect()
}

fn text(entry: &serde_json::Map<String, Value>, field: &str) -> Option<String> {
    entry.get(field).and_then(Value::as_str).map(str::to_owned)
}

fn parse_anchor(value: &Value) -> Result<Anchor> {
    let entry = object(
        value,
        &[
            "name", "catalog", "tier", "check", "input", "intents", "targets", "summary",
            "evidence",
        ],
        "anchor",
    )?;
    let name = text(entry, "name")
        .filter(|name| kebab(name))
        .ok_or("anchor name must be kebab-case")?;
    let tier = entry["tier"]
        .as_str()
        .and_then(Tier::parse)
        .ok_or_else(|| format!("anchor {name}: unknown tier"))?;
    let evidence = text(entry, "evidence")
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| format!("anchor {name}: evidence is required"))?;
    let summary = text(entry, "summary");
    if summary.as_ref().is_some_and(|line| {
        line.trim().is_empty() || line.contains('\n') || line.chars().count() > MAX_SUMMARY
    }) {
        return Err(format!(
            "anchor {name}: summary must be one line of at most {MAX_SUMMARY} characters"
        ));
    }
    let check = text(entry, "check");
    if let Some(id) = &check
        && !checks::CHECKS.contains(&id.as_str())
    {
        return Err(format!("anchor {name}: no check named {id}"));
    }
    let input = match entry.get("input").and_then(Value::as_str) {
        Some("commit-message") => Input::CommitMessage,
        Some("file") | None => Input::File,
        Some(other) => return Err(format!("anchor {name}: unknown input {other}")),
    };
    let intents = strings(entry, "intents", &name)?;
    if let Some(intent) = intents.iter().find(|intent| !kebab(intent)) {
        return Err(format!("anchor {name}: intent {intent} must be kebab-case"));
    }
    let anchor = Anchor {
        catalog: text(entry, "catalog"),
        targets: strings(entry, "targets", &name)?,
        name,
        tier,
        check,
        input,
        intents,
        summary,
        evidence,
    };
    let offered = anchor.tier != Tier::Refused;
    if offered && (anchor.summary.is_none() || anchor.catalog.is_none()) {
        return Err(format!(
            "anchor {}: offered names need a catalog id and a summary",
            anchor.name
        ));
    }
    if (anchor.tier == Tier::Structure) != anchor.check.is_some() {
        return Err(format!(
            "anchor {}: structure anchors, and only they, name a check",
            anchor.name
        ));
    }
    if !offered
        && (anchor.summary.is_some() || !anchor.intents.is_empty() || !anchor.targets.is_empty())
    {
        return Err(format!(
            "anchor {}: a refused name carries only its evidence",
            anchor.name
        ));
    }
    Ok(anchor)
}

fn parse_overlay(source: &str) -> Result<Vec<Anchor>> {
    let overlay: Value = serde_json::from_str(source).map_err(|error| error.to_string())?;
    let overlay = object(&overlay, &["tiers", "anchors"], "anchor overlay")?;
    let list = overlay
        .get("anchors")
        .and_then(Value::as_array)
        .ok_or("anchor overlay needs an anchors array")?;
    let mut anchors: Vec<Anchor> = Vec::with_capacity(list.len());
    for value in list {
        let anchor = parse_anchor(value)?;
        if anchors.iter().any(|known| known.name == anchor.name) {
            return Err(format!("duplicate anchor {}", anchor.name));
        }
        anchors.push(anchor);
    }
    Ok(anchors)
}

/// Every anchor Craft knows, offered or refused, validated once per process.
pub fn anchors() -> Result<&'static [Anchor]> {
    static ANCHORS: OnceLock<Result<Vec<Anchor>>> = OnceLock::new();
    ANCHORS
        .get_or_init(|| parse_overlay(OVERLAY))
        .as_ref()
        .map(Vec::as_slice)
        .map_err(Clone::clone)
}

/// The anchor named `name`, offered or refused.
pub fn find(name: &str) -> Option<&'static Anchor> {
    anchors().ok()?.iter().find(|anchor| anchor.name == name)
}

/// Findings for `text` under the anchor named `name`; empty means it passes. A name without
/// a check (unknown, refused or not structural) is itself a finding, never a pass.
pub fn check(name: &str, text: &str, path: Option<&str>) -> Vec<Finding> {
    match find(name).and_then(|anchor| anchor.check.as_deref()) {
        Some(id) => checks::check(id, text, path),
        None => vec![checks::finding(
            "unknown-anchor",
            None,
            format!("no anchor check named {name}"),
        )],
    }
}

/// The generated catalog; parsed only when something reads it.
pub fn catalog() -> Result<&'static Value> {
    static CATALOG_VALUE: OnceLock<Result<Value>> = OnceLock::new();
    CATALOG_VALUE
        .get_or_init(|| serde_json::from_str(CATALOG).map_err(|error| error.to_string()))
        .as_ref()
        .map_err(Clone::clone)
}

/// The catalog entry for `id`: title, definition, categories, roles, related.
pub fn entry(id: &str) -> Option<&'static Value> {
    catalog()
        .ok()?
        .get("anchors")?
        .as_array()?
        .iter()
        .find(|entry| entry["id"] == id)
}

#[cfg(test)]
mod tests;
