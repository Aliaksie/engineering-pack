//! Bounded literal navigation requests and UTF-16 line/snippet coordinates.
use crate::values::{js_length, safe_integer};
use serde_json::{Value, json};

pub mod planning;
pub mod question;

pub const SNIPPET: usize = 240;
pub const MAX_FILE_BYTES: usize = 262_144;

pub struct Search {
    pub terms: Vec<String>,
    pub scopes: Vec<String>,
    pub max_files: usize,
    pub max_matches: usize,
    pub max_bytes: usize,
}

pub fn scope(value: &Value) -> Result<String, String> {
    let text = value
        .as_str()
        .ok_or("paths must be relative scopes without traversal")?;
    let drive = text.as_bytes().get(1) == Some(&b':') && text.as_bytes()[0].is_ascii_alphabetic();
    if text.is_empty()
        || text.starts_with('/')
        || drive
        || text.contains('\\')
        || text.split('/').any(|p| p == "..")
    {
        return Err("paths must be relative scopes without traversal".into());
    }
    let name = text
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect::<Vec<_>>()
        .join("/");
    Ok(if name.is_empty() { ".".into() } else { name })
}

fn bound(input: &Value, field: &str, default: usize, ceiling: usize) -> Result<usize, String> {
    match input.get(field) {
        None => Ok(default),
        Some(value) => safe_integer(value)
            .filter(|n| *n >= 1 && *n <= ceiling as i64)
            .map(|n| n as usize)
            .ok_or_else(|| format!("{field} must be an integer from 1 to {ceiling}")),
    }
}

pub fn request(input: &Value) -> Result<Search, String> {
    let literals = input["terms"]
        .as_array()
        .filter(|terms| !terms.is_empty() && terms.len() <= 32)
        .ok_or(
            "terms must contain 1 to 32 nonempty single-line literals of at most 256 characters",
        )?;
    let mut terms = Vec::new();
    for term in literals {
        let term = term.as_str().filter(|term| !term.is_empty() && js_length(term) <= 256 && !term.contains(['\r', '\n', '\0']))
            .ok_or("terms must contain 1 to 32 nonempty single-line literals of at most 256 characters")?;
        if !terms.iter().any(|t| t == term) {
            terms.push(term.to_string());
        }
    }
    let mut scopes = Vec::new();
    if let Some(paths) = input.get("paths") {
        for path in paths.as_array().ok_or("paths must be an array")? {
            let path = scope(path)?;
            if !scopes.contains(&path) {
                scopes.push(path);
            }
        }
    }
    Ok(Search {
        terms,
        scopes,
        max_files: bound(input, "maxFiles", 500, 10_000)?,
        max_matches: bound(input, "maxMatches", 80, 1000)?,
        max_bytes: bound(input, "maxBytes", 2_097_152, 16_777_216)?,
    })
}

impl Search {
    #[cfg(test)]
    fn value(&self) -> Value {
        json!({"terms":self.terms,"scopes":self.scopes,"maxFiles":self.max_files,"maxMatches":self.max_matches,"maxBytes":self.max_bytes,"maxFileBytes":MAX_FILE_BYTES,"maxSnippetCharacters":SNIPPET})
    }
    pub fn bounds(&self) -> Value {
        json!({"maxFiles":self.max_files,"maxMatches":self.max_matches,"maxBytes":self.max_bytes,"maxFileBytes":MAX_FILE_BYTES,"maxSnippetCharacters":SNIPPET})
    }
}

pub fn matches(content: &str, file: &str, terms: &[String], max_matches: usize) -> Vec<Value> {
    let mut result = Vec::new();
    let newline_count = content.bytes().filter(|b| *b == b'\n').count();
    for (index, line) in content.split('\n').enumerate() {
        // Only a CR immediately preceding a newline belongs to the separator.
        let line = if index < newline_count {
            line.strip_suffix('\r').unwrap_or(line)
        } else {
            line
        };
        let found: Vec<_> = terms
            .iter()
            .enumerate()
            .filter_map(|(i, term)| {
                line.find(term)
                    .map(|offset| (i, js_length(&line[..offset])))
            })
            .collect();
        if found.is_empty() {
            continue;
        }
        let mut offset = found
            .iter()
            .map(|(_, offset)| *offset)
            .min()
            .unwrap_or(0)
            .saturating_sub(40);
        let utf16: Vec<_> = line.encode_utf16().collect();
        let mut end = (offset + SNIPPET).min(utf16.len());
        // Keep supplementary characters intact at either edge of a snippet.
        if offset > 0 && (0xDC00..=0xDFFF).contains(&utf16[offset]) {
            offset += 1;
        }
        if end > offset && (0xD800..=0xDBFF).contains(&utf16[end - 1]) {
            end -= 1;
        }
        let text = String::from_utf16(&utf16[offset..end]).expect("whole Unicode scalars");
        result.push(json!({"path":file,"line":index+1,"text":text,"termIndexes":found.iter().map(|(i,_)| *i).collect::<Vec<_>>(),"snippetTruncated":offset>0 || end<utf16.len()}));
        if result.len() >= max_matches {
            break;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snippets_keep_supplementary_characters_whole_at_both_edges() {
        let terms = vec!["needle".into()];
        let at_end = matches(&format!("needle{}😀tail", "x".repeat(233)), "a", &terms, 1);
        assert_eq!(at_end[0]["text"], format!("needle{}", "x".repeat(233)));
        let at_start = matches(&format!("😀{}needle", "x".repeat(39)), "a", &terms, 1);
        assert_eq!(at_start[0]["text"], format!("{}needle", "x".repeat(39)));
        assert_eq!(at_start[0]["snippetTruncated"], true);
    }
    #[test]
    fn frozen_literal_search_contracts() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/runtime/locate-contracts.json"
        ))
        .unwrap();
        for case in corpus["cases"].as_array().unwrap() {
            let input = &case["request"]["input"];
            let actual = match case["request"]["operation"].as_str().unwrap() {
                "locate-scope" => scope(&input["value"]).map(|s| json!(s)),
                "locate-request" => request(&input["input"]).map(|s| s.value()),
                "locate-match" => {
                    match (
                        input["terms"].as_array(),
                        input["content"].as_str(),
                        input["file"].as_str(),
                        input["maxMatches"].as_u64(),
                    ) {
                        (Some(terms), Some(content), Some(file), Some(max)) => {
                            let terms: Vec<_> = terms
                                .iter()
                                .map(|v| v.as_str().unwrap().to_string())
                                .collect();
                            Ok(json!(matches(content, file, &terms, max as usize)))
                        }
                        _ => Err("invalid match input".into()),
                    }
                }
                other => panic!("{other}"),
            };
            if case["response"]["ok"] == true {
                assert_eq!(
                    actual.unwrap(),
                    case["response"]["result"],
                    "{}",
                    case["id"]
                );
            } else {
                assert!(actual.is_err(), "{}", case["id"]);
            }
        }
    }
}
