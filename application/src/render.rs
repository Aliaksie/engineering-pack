//! Text shared by compact command views.
use craft_engine::values::number_text;
use serde_json::Value;

/// `String(value)` for a status field; absent and `null` read as unknown.
pub(crate) fn shown(value: &Value) -> String {
    match value {
        Value::Null => "unknown".into(),
        Value::String(text) => text.clone(),
        Value::Number(number) => number_text(number),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn fields_read_as_javascript_strings() {
        for (value, text) in [
            (json!(null), "unknown"),
            (json!("text"), "text"),
            (json!(42), "42"),
            (json!(42.0), "42"),
            (json!(2.5), "2.5"),
            (json!(1e20), "100000000000000000000"),
            (json!(1e21), "1e+21"),
            (json!(0.000001), "0.000001"),
            (json!(true), "true"),
        ] {
            assert_eq!(shown(&value), text, "{value}");
        }
    }
}
