//! Check a tool call's arguments against the tool's own schema before it runs.
//!
//! A model that leaves out a required field, or sends the wrong kind of value,
//! gets one error naming every problem and can fix the call in one go, instead
//! of the tool failing in whatever way it happens to on bad input.
//!
//! Deliberately forgiving where models are routinely loose: a number may come
//! as numeric text, a boolean as "true"/"false", extra properties are ignored,
//! and a tool with no schema here (MCP, custom tools) is not checked at all.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde_json::Value;

/// Problems with `args` against a JSON-schema `schema` of type object; empty when they fit.
pub fn check(schema: &Value, args: &Value) -> Vec<String> {
    let mut problems = vec![];
    let Some(obj) = args.as_object() else {
        return vec![format!("the arguments must be an object, not {}", kind(args))];
    };
    let props = schema.get("properties").and_then(Value::as_object);
    for name in schema.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
        if obj.get(name).is_none_or(Value::is_null) {
            problems.push(format!("`{name}` is required"));
        }
    }
    for (name, value) in obj {
        let Some(prop) = props.and_then(|p| p.get(name)) else { continue };
        if value.is_null() {
            continue;
        }
        if let Some(p) = check_value(prop, value, name) {
            problems.push(p);
        }
    }
    problems
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

fn check_value(prop: &Value, value: &Value, name: &str) -> Option<String> {
    let ok = match prop.get("type").and_then(Value::as_str) {
        Some("string") => value.is_string(),
        Some("number") => value.is_number() || value.as_str().is_some_and(|s| s.trim().parse::<f64>().is_ok()),
        Some("integer") => value.is_i64() || value.is_u64() || value.as_f64().is_some_and(|f| f.fract() == 0.0) || value.as_str().is_some_and(|s| s.trim().parse::<i64>().is_ok()),
        Some("boolean") => value.is_boolean() || matches!(value.as_str(), Some("true" | "false")),
        Some("array") => value.is_array(),
        Some("object") => value.is_object(),
        _ => true,
    };
    if !ok {
        let want = prop.get("type").and_then(Value::as_str).unwrap_or("a different type");
        return Some(format!("`{name}` must be {want}, not {}", kind(value)));
    }
    if let (Some(options), Some(text)) = (prop.get("enum").and_then(Value::as_array), value.as_str()) {
        if !options.iter().any(|o| o.as_str() == Some(text)) {
            let list: Vec<&str> = options.iter().filter_map(Value::as_str).collect();
            return Some(format!("`{name}` must be one of: {}", list.join(", ")));
        }
    }
    None
}

/// Each built-in tool's parameter schema, by name.
fn schemas() -> &'static HashMap<String, Value> {
    static S: OnceLock<HashMap<String, Value>> = OnceLock::new();
    S.get_or_init(|| {
        crate::agent::tools::schemas::static_tool_definitions()
            .into_iter()
            .filter_map(|d| Some((d.get("name")?.as_str()?.to_string(), d.get("parameters")?.clone())))
            .collect()
    })
}

/// The message to send back when a built-in tool's arguments don't fit its schema.
pub fn rejection(tool: &str, args: &Value) -> Option<String> {
    let schema = schemas().get(tool)?;
    let problems = check(schema, args);
    (!problems.is_empty()).then(|| format!("The `{tool}` call was not run because its arguments are invalid: {}. Fix these and call it again.", problems.join("; ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema() -> Value {
        json!({ "type": "object", "properties": {
            "path": { "type": "string" },
            "limit": { "type": "number" },
            "ocr": { "type": "boolean" },
            "mode": { "type": "string", "enum": ["a", "b"] },
            "items": { "type": "array" }
        }, "required": ["path"] })
    }

    #[test]
    fn good_arguments_pass_and_extra_properties_are_ignored() {
        assert!(check(&schema(), &json!({ "path": "a", "limit": 5, "ocr": true, "mode": "a", "items": [], "extra": 1 })).is_empty());
    }

    #[test]
    fn every_problem_is_named_at_once() {
        let p = check(&schema(), &json!({ "limit": [], "mode": "z", "ocr": 3 }));
        assert_eq!(p.len(), 4, "{p:?}");
        assert!(p[0].contains("`path` is required"));
        assert!(p.iter().any(|x| x.contains("`limit` must be number, not an array")));
        assert!(p.iter().any(|x| x.contains("`mode` must be one of: a, b")));
        assert!(p.iter().any(|x| x.contains("`ocr` must be boolean")));
    }

    #[test]
    fn a_null_counts_as_missing_for_required_and_is_skipped_otherwise() {
        assert_eq!(check(&schema(), &json!({ "path": null })), ["`path` is required"]);
        assert!(check(&schema(), &json!({ "path": "a", "limit": null })).is_empty());
    }

    #[test]
    fn loose_but_common_values_are_accepted() {
        assert!(check(&schema(), &json!({ "path": "a", "limit": "20", "ocr": "true" })).is_empty());
        assert!(!check(&schema(), &json!({ "path": "a", "limit": "lots" })).is_empty());
    }

    #[test]
    fn arguments_that_are_not_an_object_are_refused() {
        assert_eq!(check(&schema(), &json!("oops")), ["the arguments must be an object, not a string"]);
    }

    #[test]
    fn built_in_tools_are_checked_against_their_real_schema_and_unknown_tools_are_not() {
        let msg = rejection("read", &json!({})).expect("read needs a path");
        assert!(msg.contains("`read` call was not run") && msg.contains("`path` is required"), "{msg}");
        assert!(rejection("read", &json!({ "path": "a.txt" })).is_none());
        assert!(rejection("some_mcp_tool", &json!({})).is_none());
    }
}
