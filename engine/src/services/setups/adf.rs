//! A Jira rich-text description (Atlassian Document Format) as the text a task body holds:
//! headings, paragraphs and lists in the Markdown a GitHub or GitLab body uses, so the same
//! readers find the same acceptance list whatever the tracker stored.
use crate::timestamp;
use serde_json::Value;

/// How deep blocks may nest; anything deeper is left out rather than read without bound.
const DEPTH: usize = 32;

/// The text of an ADF `doc`, `None` for any other value and for a document with nothing to show.
pub fn text(doc: &Value) -> Option<String> {
    if doc["type"] != "doc" {
        return None;
    }
    let text = blocks(&doc["content"], 0).join("\n\n");
    let text = text.trim_end();
    (!text.trim().is_empty()).then(|| text.to_owned())
}

/// The non-empty blocks among `content`, in order.
fn blocks(content: &Value, depth: usize) -> Vec<String> {
    let Some(nodes) = content.as_array().filter(|_| depth < DEPTH) else {
        return Vec::new();
    };
    nodes
        .iter()
        .filter_map(|node| block(node, depth + 1))
        .filter(|text| !text.trim().is_empty())
        .collect()
}

fn block(node: &Value, depth: usize) -> Option<String> {
    let attrs = &node["attrs"];
    Some(match node["type"].as_str()? {
        "paragraph" => inline(&node["content"], depth),
        "heading" => {
            let level = attrs["level"].as_u64().unwrap_or(1).clamp(1, 6) as usize;
            format!(
                "{} {}",
                "#".repeat(level),
                inline(&node["content"], depth).trim()
            )
        }
        "bulletList" | "decisionList" => list(node, depth, |_, _| "- ".to_owned()),
        "orderedList" => {
            let start = attrs["order"].as_u64().unwrap_or(1);
            list(node, depth, |index, _| {
                format!("{}. ", start.saturating_add(index as u64))
            })
        }
        "taskList" => list(node, depth, |_, item| {
            let done = item["attrs"]["state"] == "DONE";
            format!("- [{}] ", if done { 'x' } else { ' ' })
        }),
        "codeBlock" => {
            let language = plain(attrs["language"].as_str().unwrap_or_default());
            let code: String = children(&node["content"])
                .filter_map(|n| n["text"].as_str())
                .collect();
            format!("```{language}\n{}\n```", code.trim_end_matches('\n'))
        }
        "blockquote" => blocks(&node["content"], depth)
            .join("\n\n")
            .lines()
            .map(|line| format!("> {line}").trim_end().to_owned())
            .collect::<Vec<_>>()
            .join("\n"),
        "rule" => "---".into(),
        "table" => children(&node["content"])
            .map(|row| {
                let cells: Vec<String> = children(&row["content"])
                    .map(|cell| blocks(&cell["content"], depth).join(" ").replace('\n', " "))
                    .collect();
                format!("| {} |", cells.join(" | "))
            })
            .collect::<Vec<_>>()
            .join("\n"),
        "blockCard" | "embedCard" => plain(attrs["url"].as_str()?),
        "media" | "mediaSingle" | "mediaGroup" | "extension" | "bodiedExtension" => {
            return None;
        }
        // A container such as a panel, an expand or a layout: its title, then its blocks.
        _ if node["content"].is_array() && !holds_inline(&node["content"]) => {
            let mut parts: Vec<String> = attrs["title"].as_str().map(plain).into_iter().collect();
            parts.extend(blocks(&node["content"], depth));
            parts.join("\n\n")
        }
        _ => inline_node(node, depth),
    })
}

/// A list's items, each starting with its `marker` and every further line of the item
/// indented under it, so wrapped text and nested lists stay with their item.
fn list(node: &Value, depth: usize, marker: impl Fn(usize, &Value) -> String) -> String {
    children(&node["content"])
        .enumerate()
        .filter_map(|(index, item)| {
            let parts = if holds_inline(&item["content"]) {
                // A task or decision item holds its text inline, and may hold nested lists.
                let mut text = String::new();
                let mut parts = Vec::new();
                for node in children(&item["content"]) {
                    if is_inline(node) {
                        text += &inline_node(node, depth + 1);
                    } else if depth < DEPTH {
                        parts.extend(block(node, depth + 1));
                    }
                }
                parts.insert(0, text);
                parts
            } else {
                blocks(&item["content"], depth)
            };
            let body = parts
                .into_iter()
                .filter(|part| !part.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            if body.trim().is_empty() {
                return None;
            }
            let marker = marker(index, item);
            let indent = " ".repeat(marker.chars().count());
            let mut lines = body.lines();
            let first = format!("{marker}{}", lines.next().unwrap_or_default().trim_start());
            let rest = lines.map(|line| {
                if line.trim().is_empty() {
                    String::new()
                } else {
                    format!("{indent}{line}")
                }
            });
            Some(std::iter::once(first).chain(rest).collect::<Vec<_>>())
        })
        .flatten()
        .collect::<Vec<_>>()
        .join("\n")
}

/// The text of inline nodes; a hard break is the only new line they hold.
fn inline(content: &Value, depth: usize) -> String {
    if depth >= DEPTH {
        return String::new();
    }
    children(content)
        .map(|node| inline_node(node, depth + 1))
        .collect()
}

fn inline_node(node: &Value, depth: usize) -> String {
    let attrs = &node["attrs"];
    let attr = |name: &str| plain(attrs[name].as_str().unwrap_or_default());
    match node["type"].as_str().unwrap_or_default() {
        "text" => plain(node["text"].as_str().unwrap_or_default()),
        "hardBreak" => "\n".into(),
        "mention" | "status" => attr("text"),
        "emoji" => plain(
            attrs["text"]
                .as_str()
                .or_else(|| attrs["shortName"].as_str())
                .unwrap_or_default(),
        ),
        "inlineCard" => attr("url"),
        "date" => attrs["timestamp"]
            .as_str()
            .and_then(|ms| ms.parse::<i64>().ok())
            .map(|ms| timestamp::format(ms).chars().take(10).collect())
            .unwrap_or_default(),
        _ => inline(&node["content"], depth),
    }
}

/// Text from the document on one line: a control character, such as a line break inside a
/// mention, is a space, so it cannot end the list or the line that holds it.
fn plain(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

fn children(content: &Value) -> impl Iterator<Item = &Value> {
    content.as_array().into_iter().flatten()
}

/// Whether `content` holds inline nodes rather than blocks.
fn holds_inline(content: &Value) -> bool {
    children(content).any(is_inline)
}

fn is_inline(node: &Value) -> bool {
    matches!(
        node["type"].as_str(),
        Some(
            "text"
                | "hardBreak"
                | "mention"
                | "emoji"
                | "status"
                | "inlineCard"
                | "date"
                | "placeholder"
                | "mediaInline"
        )
    )
}

#[cfg(test)]
mod tests;
