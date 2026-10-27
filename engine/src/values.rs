//! JSON value predicates with the JavaScript semantics that state formats were written under.
use serde_json::{Map, Value, json};

pub const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

/// JavaScript truthiness: absent, `null`, `false`, `0` and `""` are false; arrays and objects
/// are true.
pub fn truthy<'a>(value: impl Into<Option<&'a Value>>) -> bool {
    match value.into().unwrap_or(&Value::Null) {
        Value::Null | Value::Bool(false) => false,
        Value::String(s) => !s.is_empty(),
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        _ => true,
    }
}

/// `Number.isSafeInteger` over a JSON value; integral floats such as `1.0` qualify.
pub fn safe_integer(value: &Value) -> Option<i64> {
    let number = value.as_number()?;
    let integer = match (number.as_i64(), number.as_f64()) {
        (Some(integer), _) => integer,
        (None, Some(float)) if float.fract() == 0.0 && float.abs() <= MAX_SAFE_INTEGER as f64 => {
            float as i64
        }
        _ => return None,
    };
    (integer.abs() <= MAX_SAFE_INTEGER).then_some(integer)
}

/// Copy JSON-integral floats as integers, so `1.0` renders as `JSON.stringify` does.
pub fn normalize_numbers(value: &mut Value) {
    match value {
        Value::Number(number) if number.is_f64() => {
            if let Some(integer) = safe_integer(value) {
                *value = json!(integer);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(normalize_numbers),
        Value::Object(map) => map.values_mut().for_each(normalize_numbers),
        _ => {}
    }
}

/// An object whose keys are all `allowed`; diagnostics name the first unknown field.
pub fn object<'a>(
    value: &'a Value,
    allowed: &[&str],
    label: &str,
) -> Result<&'a Map<String, Value>, String> {
    let map = value
        .as_object()
        .ok_or_else(|| format!("{label} must be an object"))?;
    match map.keys().find(|key| !allowed.contains(&key.as_str())) {
        Some(key) => Err(format!("unknown {label} field: {key}")),
        None => Ok(map),
    }
}

/// JavaScript string length, which bounds identifiers in UTF-16 code units.
pub fn js_length(text: &str) -> usize {
    text.encode_utf16().count()
}

/// JavaScript whitespace (`\s`, `trim`), which includes U+FEFF but not U+0085.
pub fn js_space(c: char) -> bool {
    c == '\u{feff}' || (c.is_whitespace() && c != '\u{85}')
}

/// Whether `text.trim()` is empty in JavaScript.
pub fn js_blank(text: &str) -> bool {
    text.chars().all(js_space)
}

/// At most `max` UTF-16 units and no C0 control or DEL; emptiness is the caller's rule.
pub fn line_text(text: &str, max: usize) -> bool {
    js_length(text) <= max && !text.chars().any(|c| c <= '\u{1f}' || c == '\u{7f}')
}

/// A bounded single-line identifier: 1..512 UTF-16 units without control characters.
pub fn text(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|s| !s.is_empty() && line_text(s, 512))
}

/// Exactly `length` lowercase hex digits, as digests and object IDs are written.
pub fn lower_hex(text: &str, length: usize) -> bool {
    text.len() == length && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A SHA-256 digest in lowercase hex.
pub fn sha256(value: &Value) -> bool {
    value.as_str().is_some_and(|s| lower_hex(s, 64))
}

/// `String(number)` in JavaScript.
pub fn number_text(number: &serde_json::Number) -> String {
    if number.is_i64() || number.is_u64() {
        return number.to_string();
    }
    let float = number.as_f64().unwrap_or_default();
    let scientific = format!("{float:e}");
    let (mantissa, exponent) = scientific.split_once('e').expect("exponent form");
    let exponent: i32 = exponent.parse().expect("integer exponent");
    if (-6..21).contains(&exponent) {
        format!("{float}")
    } else {
        format!(
            "{mantissa}e{}{exponent}",
            if exponent > 0 { "+" } else { "" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truthiness_follows_javascript() {
        for falsy in [
            json!(null),
            json!(false),
            json!(0),
            json!(-0.0),
            json!(0.0),
            json!(""),
        ] {
            assert!(!truthy(&falsy), "{falsy}");
        }
        for value in [
            json!(true),
            json!(1),
            json!(-1.5),
            json!(" "),
            json!([]),
            json!({}),
        ] {
            assert!(truthy(&value), "{value}");
        }
    }

    #[test]
    fn hex_is_lowercase_and_exact() {
        let digest = "a".repeat(64);
        assert!(sha256(&json!(digest)));
        assert!(!sha256(&json!("A".repeat(64))));
        assert!(!sha256(&json!("a".repeat(63))));
        assert!(!sha256(&json!(1)));
        assert!(lower_hex("0123456789abcdef", 16) && !lower_hex("0123456789abcdeg", 16));
    }

    #[test]
    fn line_text_bounds_units_and_controls() {
        assert!(line_text("", 1) && line_text("😀", 2) && !line_text("😀", 1));
        assert!(!line_text("a\nb", 8) && !line_text("a\u{7f}", 8) && line_text("a\u{85}", 8));
    }
}
