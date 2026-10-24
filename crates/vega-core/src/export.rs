//! The text `Ctrl-C` puts on the clipboard: the JSON body of a GraphQL request, ready
//! for `curl -d @-`.

use serde_json::{json, Map, Value};

use crate::querygen::GeneratedQuery;
use crate::schema::Schema;
use crate::variables::{self, State, Values};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Export {
    /// `{"query": ..., "variables": {...}}`, pretty-printed.
    pub text: String,
    /// Variables left out because they are unset or invalid.
    pub missing: usize,
}

/// The request body for `query` with the values typed so far. Unlike a real run it never
/// fails: variables without a usable value are left out and counted.
pub fn request_json(schema: &Schema, query: &GeneratedQuery, values: &Values) -> Export {
    let mut variables = Map::new();
    let mut missing = 0;
    for def in &query.variables {
        match variables::state(schema, def, values.get(&def.path).map(String::as_str)) {
            State::Value(value) => {
                variables.insert(def.name.clone(), value);
            }
            State::Unset => {}
            State::Invalid(_) => missing += 1,
        }
    }
    let mut body = json!({ "query": query.text });
    if !variables.is_empty() {
        body["variables"] = Value::Object(variables);
    }
    let text = serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string());
    Export { text, missing }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::querygen::generate;
    use crate::selection::{parse_select, OperationRef};
    use crate::testutil::design_schema;

    fn export(operation: &str, select: &str, typed: &[(&str, &str)]) -> Export {
        let schema = design_schema();
        let op = OperationRef::parse(&schema, operation).unwrap();
        let selection = parse_select(&schema, &op, select).unwrap();
        let query = generate(&schema, &op, &selection).unwrap();
        let values: Values = query
            .variables
            .iter()
            .filter_map(|def| {
                typed
                    .iter()
                    .find(|(name, _)| *name == def.name)
                    .map(|(_, raw)| (def.path.clone(), raw.to_string()))
            })
            .collect();
        request_json(&schema, &query, &values)
    }

    #[test]
    fn the_body_carries_the_query_and_the_variables_that_are_set() {
        let out = export("Query.me", "posts.title", &[("first", "3")]);
        let body: Value = serde_json::from_str(&out.text).unwrap();
        assert_eq!(
            body["query"],
            "query Me($first: Int!) {\n  me {\n    posts(first: $first) {\n      title\n    }\n  }\n}"
        );
        assert_eq!(body["variables"], json!({ "first": 3 }));
        assert_eq!(out.missing, 0);
        assert!(out.text.contains("\n  \"query\""), "{}", out.text);
    }

    #[test]
    fn a_query_without_variables_has_no_variables_key() {
        let out = export("Query.me", "id", &[]);
        let body: Value = serde_json::from_str(&out.text).unwrap();
        assert_eq!(
            body,
            json!({ "query": "query Me {\n  me {\n    id\n  }\n}" })
        );
        assert_eq!(out.missing, 0);
    }

    #[test]
    fn an_unset_required_variable_is_left_out_and_counted() {
        let out = export("Query.me", "posts.title", &[]);
        let body: Value = serde_json::from_str(&out.text).unwrap();
        assert!(body.get("variables").is_none());
        assert_eq!(out.missing, 1);
    }

    #[test]
    fn an_invalid_value_is_left_out_and_counted() {
        let out = export("Query.me", "posts.title", &[("first", "many")]);
        let body: Value = serde_json::from_str(&out.text).unwrap();
        assert!(body.get("variables").is_none());
        assert_eq!(out.missing, 1);
    }
}
