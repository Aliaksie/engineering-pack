//! Anchors: established standards a project names in one word instead of describing them.
//! Three sources define a name, and the most specific wins on the same id:
//! - `catalog.json`, generated from the Semantic Anchors catalog (see `anchors/NOTICE`), says
//!   what every catalogued name means. A catalog-only name is vocabulary: shown by its title,
//!   never checked.
//! - `overlay.json`, Craft's own, gives the names Craft has evaluated a tier, a deterministic
//!   check, defaults and the evidence; refused names are listed with the reason.
//! - The project's catalog files (`anchors.catalogs`), read by the application and passed in
//!   as `settings.anchors.loaded`; an entry replaces the built-in entry with its id.
//!
//! Names are catalog ids; an alias (`semver`) stands for its canonical id everywhere.
use crate::contracts::kebab;
use crate::values::object;
use serde_json::{Map, Value, json};
use std::sync::OnceLock;

pub mod checks;

pub use checks::Finding;

type Result<T = ()> = std::result::Result<T, String>;

const OVERLAY: &str = include_str!("../anchors/overlay.json");
const CATALOG: &str = include_str!("../anchors/catalog.json");
const MAX_SUMMARY: usize = 200;
const MAX_TITLE: usize = 120;
const MAX_CHECKLIST: usize = 16;
/// How many catalog files a project lists, and how many anchors they hold together.
pub const MAX_CATALOGS: usize = 8;
const MAX_PROJECT_ANCHORS: usize = 256;
/// How many entries `list` returns for a search.
const MAX_LISTED: usize = 50;

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

/// Where an anchor's definition came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// Craft's overlay: evaluated, with tier and evidence.
    Craft,
    /// The imported catalog only: a recognised name, not evaluated by Craft.
    Catalog,
    /// A project catalog file, by its project-relative path.
    Project(String),
}

impl Source {
    pub fn label(&self) -> &str {
        match self {
            Self::Craft => "craft",
            Self::Catalog => "catalog",
            Self::Project(path) => path,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Anchor {
    /// The canonical name: the catalog id for catalogued names.
    pub id: String,
    /// Other names that stand for `id`.
    pub aliases: Vec<String>,
    pub source: Source,
    pub tier: Tier,
    /// The deterministic check, for structure anchors.
    pub check: Option<String>,
    /// Route intents the standard applies to when the consumer declares no scope.intents; empty = any.
    pub intents: Vec<String>,
    /// Route intents Craft adds this anchor for, at `advise`, when the project turns
    /// `anchors.defaults` on; Craft's own entries only.
    pub defaults: Vec<String>,
    /// Files the check reads when the consumer declares no scope.paths: a file name ("CHANGELOG.md") or an extension (".feature"); empty = consumer must declare paths (commit messages need none).
    pub targets: Vec<String>,
    /// One line the model is shown instead of the standard's text; refused names have none.
    pub summary: Option<String>,
    /// Review items shown with the route.
    pub checklist: Vec<String>,
    /// Why the name has its tier; Craft's own entries only.
    pub evidence: Option<String>,
}

impl Anchor {
    /// What the check reads; anchors without a check read nothing.
    pub fn input(&self) -> Option<Input> {
        self.check.as_deref().map(checks::input)
    }
}

fn strings(entry: &Map<String, Value>, field: &str, name: &str) -> Result<Vec<String>> {
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

fn text(entry: &Map<String, Value>, field: &str) -> Option<String> {
    entry.get(field).and_then(Value::as_str).map(str::to_owned)
}

fn line(entry: &Map<String, Value>, field: &str, max: usize, name: &str) -> Result<Option<String>> {
    match entry.get(field) {
        None => Ok(None),
        Some(Value::String(line))
            if !line.trim().is_empty() && !line.contains('\n') && line.chars().count() <= max =>
        {
            Ok(Some(line.clone()))
        }
        Some(_) => Err(format!(
            "anchor {name}: {field} must be one line of at most {max} characters"
        )),
    }
}

fn identifier(entry: &Map<String, Value>) -> Result<String> {
    text(entry, "id")
        .filter(|id| kebab(id) && id.len() <= 64)
        .ok_or_else(|| "anchor id must be kebab-case".to_owned())
}

fn parse_overlay_entry(value: &Value) -> Result<Anchor> {
    let entry = object(
        value,
        &[
            "id", "aliases", "tier", "check", "intents", "defaults", "targets", "summary",
            "evidence",
        ],
        "anchor",
    )?;
    let id = identifier(entry)?;
    let tier = entry["tier"]
        .as_str()
        .and_then(Tier::parse)
        .ok_or_else(|| format!("anchor {id}: unknown tier"))?;
    let evidence = text(entry, "evidence")
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| format!("anchor {id}: evidence is required"))?;
    let check = text(entry, "check");
    if let Some(check) = &check
        && !checks::CHECKS.contains(&check.as_str())
    {
        return Err(format!("anchor {id}: no check named {check}"));
    }
    let intents = strings(entry, "intents", &id)?;
    if let Some(intent) = intents.iter().find(|intent| !kebab(intent)) {
        return Err(format!("anchor {id}: intent {intent} must be kebab-case"));
    }
    let defaults = strings(entry, "defaults", &id)?;
    if let Some(intent) = defaults.iter().find(|intent| !kebab(intent)) {
        return Err(format!(
            "anchor {id}: default intent {intent} must be kebab-case"
        ));
    }
    let aliases = strings(entry, "aliases", &id)?;
    if let Some(alias) = aliases.iter().find(|alias| !kebab(alias)) {
        return Err(format!("anchor {id}: alias {alias} must be kebab-case"));
    }
    let anchor = Anchor {
        summary: line(entry, "summary", MAX_SUMMARY, &id)?,
        targets: strings(entry, "targets", &id)?,
        id,
        aliases,
        source: Source::Craft,
        tier,
        check,
        intents,
        defaults,
        checklist: Vec::new(),
        evidence: Some(evidence),
    };
    let offered = anchor.tier != Tier::Refused;
    if offered && anchor.summary.is_none() {
        return Err(format!(
            "anchor {}: offered names need a summary",
            anchor.id
        ));
    }
    if (anchor.tier == Tier::Structure) != anchor.check.is_some() {
        return Err(format!(
            "anchor {}: structure anchors, and only they, name a check",
            anchor.id
        ));
    }
    if !offered
        && (anchor.summary.is_some()
            || !anchor.intents.is_empty()
            || !anchor.defaults.is_empty()
            || !anchor.targets.is_empty()
            || !anchor.aliases.is_empty())
    {
        return Err(format!(
            "anchor {}: a refused name carries only its evidence",
            anchor.id
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
        let anchor = parse_overlay_entry(value)?;
        let names = || std::iter::once(&anchor.id).chain(&anchor.aliases);
        if let Some(name) = names().find(|name| {
            anchors
                .iter()
                .any(|known| known.id == **name || known.aliases.contains(name))
        }) {
            return Err(format!("duplicate anchor {name}"));
        }
        anchors.push(anchor);
    }
    Ok(anchors)
}

/// Craft's own anchors, offered or refused, validated once per process.
pub fn anchors() -> Result<&'static [Anchor]> {
    static ANCHORS: OnceLock<Result<Vec<Anchor>>> = OnceLock::new();
    ANCHORS
        .get_or_init(|| parse_overlay(OVERLAY))
        .as_ref()
        .map(Vec::as_slice)
        .map_err(Clone::clone)
}

/// Craft's own anchor named `name` (id or alias), offered or refused.
pub fn find(name: &str) -> Option<&'static Anchor> {
    anchors()
        .ok()?
        .iter()
        .find(|anchor| anchor.id == name || anchor.aliases.iter().any(|alias| alias == name))
}

/// The canonical id `name` stands for: an alias becomes its id, any other name stays.
pub fn canonical(name: &str) -> &str {
    find(name).map_or(name, |anchor| anchor.id.as_str())
}

/// Findings for `text` under Craft's anchor named `name`; empty means it passes. A name
/// without a check (unknown, refused or not structural) is itself a finding, never a pass.
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

fn catalog_entries() -> &'static [Value] {
    catalog()
        .ok()
        .and_then(|catalog| catalog["anchors"].as_array())
        .map_or(&[], Vec::as_slice)
}

/// The catalog entry for `id`: title, definition, categories, roles, related.
pub fn entry(id: &str) -> Option<&'static Value> {
    catalog_entries().iter().find(|entry| entry["id"] == id)
}

/// Where a person reads the catalog entry for `id`, at the pinned commit.
pub fn link(id: &str) -> Option<String> {
    let source = &catalog().ok()?["source"];
    entry(id)?;
    Some(format!(
        "{}/blob/{}/{}/{id}.adoc",
        source["repository"].as_str()?,
        source["commit"].as_str()?,
        source["path"].as_str()?
    ))
}

/// A catalog-only name: vocabulary, shown by its title.
fn from_catalog(entry: &Value) -> Option<Anchor> {
    Some(Anchor {
        id: entry["id"].as_str()?.to_owned(),
        aliases: Vec::new(),
        source: Source::Catalog,
        tier: Tier::Vocabulary,
        check: None,
        intents: Vec::new(),
        defaults: Vec::new(),
        targets: Vec::new(),
        summary: entry["title"].as_str().map(str::to_owned),
        checklist: Vec::new(),
        evidence: None,
    })
}

const PROJECT_FIELDS: [&str; 7] = [
    "id",
    "title",
    "summary",
    "checklist",
    "check",
    "intents",
    "targets",
];

/// A project catalog entry. `check` names a Craft check or a Craft anchor that has one;
/// without `targets` the check brings the files Craft's anchor for it reads.
/// `stored` entries come from `settings.anchors.loaded` and carry the `source` Craft added.
fn parse_project_entry(
    value: &Value,
    source: &str,
    intents: &[&str],
    stored: bool,
) -> Result<Anchor> {
    let mut fields = PROJECT_FIELDS.to_vec();
    if stored {
        fields.push("source");
    }
    let entry = object(value, &fields, "anchor")?;
    let id = canonical(&identifier(entry)?).to_owned();
    let title = line(entry, "title", MAX_TITLE, &id)?;
    let summary = line(entry, "summary", MAX_SUMMARY, &id)?.or(title);
    if summary.is_none() {
        return Err(format!("anchor {id}: needs a summary or a title"));
    }
    let checklist = strings(entry, "checklist", &id)?;
    if checklist.len() > MAX_CHECKLIST
        || checklist
            .iter()
            .any(|item| item.contains('\n') || item.chars().count() > MAX_SUMMARY)
    {
        return Err(format!(
            "anchor {id}: checklist takes at most {MAX_CHECKLIST} single-line items of at most {MAX_SUMMARY} characters"
        ));
    }
    let check = match text(entry, "check") {
        None if entry.contains_key("check") => {
            return Err(format!("anchor {id}: check must be text"));
        }
        None => None,
        Some(name) if checks::CHECKS.contains(&name.as_str()) => Some(name),
        Some(name) => Some(
            find(&name)
                .and_then(|anchor| anchor.check.clone())
                .ok_or_else(|| format!("anchor {id}: no check named {name}"))?,
        ),
    };
    let declared = strings(entry, "intents", &id)?;
    if let Some(intent) = declared
        .iter()
        .find(|intent| !intents.contains(&intent.as_str()))
    {
        return Err(format!("anchor {id}: unknown intent {intent}"));
    }
    let mut targets = strings(entry, "targets", &id)?;
    if targets.is_empty()
        && let Some(check) = &check
    {
        targets = anchors()?
            .iter()
            .find(|anchor| anchor.check.as_ref() == Some(check))
            .map(|anchor| anchor.targets.clone())
            .unwrap_or_default();
    }
    Ok(Anchor {
        id,
        aliases: Vec::new(),
        source: Source::Project(source.to_owned()),
        tier: if check.is_some() {
            Tier::Structure
        } else {
            Tier::Vocabulary
        },
        check,
        intents: declared,
        defaults: Vec::new(),
        targets,
        summary,
        checklist,
        evidence: None,
    })
}

/// Validate the project's catalog documents, each `(path, document)` in declaration order,
/// and return their entries as `settings.anchors.loaded`: canonical ids, each with its
/// `source`, a later file replacing an earlier entry with the same id.
pub fn load(catalogs: &[(String, Value)], intents: &[&str]) -> Result<Value> {
    if catalogs.len() > MAX_CATALOGS {
        return Err(format!(
            "anchors.catalogs lists at most {MAX_CATALOGS} files"
        ));
    }
    let mut loaded: Vec<Value> = Vec::new();
    for (path, document) in catalogs {
        let fail = |error: String| format!("anchor catalog {path}: {error}");
        let list = object(document, &["anchors"], "anchor catalog")
            .map_err(fail)?
            .get("anchors")
            .and_then(Value::as_array)
            .ok_or_else(|| fail("needs an anchors array".into()))?;
        let mut ids: Vec<String> = Vec::new();
        for value in list {
            let anchor = parse_project_entry(value, path, intents, false).map_err(fail)?;
            if ids.contains(&anchor.id) {
                return Err(fail(format!("duplicate anchor {}", anchor.id)));
            }
            ids.push(anchor.id.clone());
            let mut entry = value.clone();
            entry["id"] = json!(anchor.id);
            entry["source"] = json!(path);
            loaded.retain(|known| known["id"] != entry["id"]);
            loaded.push(entry);
        }
    }
    if loaded.len() > MAX_PROJECT_ANCHORS {
        return Err(format!(
            "anchor catalogs hold at most {MAX_PROJECT_ANCHORS} anchors together"
        ));
    }
    Ok(Value::Array(loaded))
}

fn loaded(settings: &Value) -> &[Value] {
    settings["anchors"]["loaded"]
        .as_array()
        .map_or(&[], Vec::as_slice)
}

fn project_anchor(entry: &Value) -> Option<Anchor> {
    // Entries were validated when the settings were loaded; intents were checked then.
    let any: Vec<&str> = entry["intents"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    parse_project_entry(entry, entry["source"].as_str()?, &any, true).ok()
}

/// The anchor `name` stands for in this project: the project's entry, else Craft's, else
/// the catalog's. Aliases resolve to their canonical id first.
pub fn resolve(settings: &Value, name: &str) -> Option<Anchor> {
    let id = canonical(name);
    if let Some(entry) = loaded(settings).iter().find(|entry| entry["id"] == id) {
        return project_anchor(entry);
    }
    if let Some(anchor) = find(id) {
        return Some(anchor.clone());
    }
    entry(id).and_then(from_catalog)
}

fn shown(anchor: &Anchor) -> Value {
    let mut item = json!({
        "id": anchor.id,
        "source": anchor.source.label(),
        "tier": anchor.tier.name(),
    });
    let title = entry(&anchor.id).and_then(|entry| entry["title"].as_str());
    for (field, value) in [
        ("title", title.map(Value::from)),
        ("summary", anchor.summary.as_deref().map(Value::from)),
        ("check", anchor.check.as_deref().map(Value::from)),
        ("link", link(&anchor.id).map(Value::from)),
        ("evidence", anchor.evidence.as_deref().map(Value::from)),
    ] {
        if let Some(value) = value {
            item[field] = value;
        }
    }
    if !anchor.aliases.is_empty() {
        item["aliases"] = json!(anchor.aliases);
    }
    if !anchor.checklist.is_empty() {
        item["checklist"] = json!(anchor.checklist);
    }
    if !anchor.defaults.is_empty() {
        item["defaults"] = json!(anchor.defaults);
    }
    item
}

fn found(anchor: &Anchor, query: &str) -> bool {
    let catalog = entry(&anchor.id);
    let mut texts = vec![anchor.id.as_str()];
    texts.extend(anchor.aliases.iter().map(String::as_str));
    texts.extend(anchor.summary.as_deref());
    if let Some(catalog) = catalog {
        texts.extend(catalog["title"].as_str());
        for field in ["categories", "roles"] {
            texts.extend(
                catalog[field]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str),
            );
        }
    }
    texts.iter().any(|text| text.to_lowercase().contains(query))
}

/// The anchors a project can name. Without a query: the project's and Craft's own. With
/// one: every source, matched case-insensitively on id, alias, title, summary, category or
/// role, at most `MAX_LISTED`. Each id appears once, as it resolves for the project.
pub fn list(settings: &Value, query: Option<&str>) -> Value {
    let mut known: Vec<Anchor> = loaded(settings).iter().filter_map(project_anchor).collect();
    let add = |anchor: Anchor, known: &mut Vec<Anchor>| {
        if !known.iter().any(|entry| entry.id == anchor.id) {
            known.push(anchor);
        }
    };
    for anchor in anchors().unwrap_or_default() {
        add(anchor.clone(), &mut known);
    }
    let query = query.map(|query| query.trim().to_lowercase());
    if query.is_some() {
        for anchor in catalog_entries().iter().filter_map(from_catalog) {
            add(anchor, &mut known);
        }
    }
    let matched: Vec<Value> = known
        .iter()
        .filter(|anchor| query.as_deref().is_none_or(|query| found(anchor, query)))
        .map(shown)
        .collect();
    let source = &catalog().map_or(Value::Null, |catalog| catalog["source"].clone());
    json!({
        "matched": matched.len(),
        "anchors": matched.into_iter().take(MAX_LISTED).collect::<Vec<_>>(),
        "catalog": {
            "repository": source["repository"],
            "commit": source["commit"],
            "license": source["license"],
            "entries": catalog_entries().len(),
        },
    })
}

#[cfg(test)]
mod tests;
