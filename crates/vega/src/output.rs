//! Plain-text rendering for CLI output.

use vega_core::client::Response;
use vega_core::querygen::GeneratedQuery;
use vega_core::response::ResponseView;
use vega_core::search::Hit;
use vega_core::variables::{self, State};

/// One `path  summary` line per hit.
pub fn hits_text(hits: &[Hit]) -> String {
    hits.iter()
        .map(|hit| format!("{}  {}\n", hit.path, hit.summary))
        .collect()
}

/// The query text, then the variables it needs (if any), one per line, each with its
/// value when a valid one was given.
pub fn query_text(query: &GeneratedQuery, states: &[State]) -> String {
    let mut out = query.text.clone();
    out.push('\n');
    if !query.variables.is_empty() {
        out.push_str("\nVariables:\n");
        for (index, variable) in query.variables.iter().enumerate() {
            out.push_str(&format!("  ${}: {}", variable.name, variable.ty));
            if let Some(State::Value(value)) = states.get(index) {
                out.push_str(&format!(" = {}", variables::display(value)));
            }
            out.push('\n');
        }
    }
    out
}

/// The generated query as JSON; each variable that has a valid value also gets `value`.
pub fn query_json(query: &GeneratedQuery, states: &[State]) -> anyhow::Result<serde_json::Value> {
    let mut json = serde_json::to_value(query)?;
    if let Some(entries) = json["variables"].as_array_mut() {
        for (entry, state) in entries.iter_mut().zip(states) {
            if let (State::Value(value), Some(object)) = (state, entry.as_object_mut()) {
                object.insert("value".to_string(), value.clone());
            }
        }
    }
    Ok(json)
}

/// What `--run --json` prints: the response parsed when it is JSON, a string otherwise.
pub fn run_json(query: &GeneratedQuery, response: &Response) -> serde_json::Value {
    let body = serde_json::from_str::<serde_json::Value>(&response.body)
        .unwrap_or_else(|_| serde_json::Value::String(response.body.clone()));
    serde_json::json!({
        "operationName": query.operation_name,
        "status": response.status,
        "elapsedMs": u64::try_from(response.elapsed.as_millis()).unwrap_or(u64::MAX),
        "response": body,
    })
}

/// One line saying why a run did not succeed.
pub fn run_failure(view: &ResponseView) -> String {
    if !(200..300).contains(&view.status) {
        format!("server answered HTTP {}", view.status)
    } else if view.not_json {
        "the response is not valid JSON".to_string()
    } else {
        format!("GraphQL errors: {}", view.errors.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vega_core::search::HitKind;

    #[test]
    fn hits_text_prints_one_line_per_hit() {
        let hits = vec![
            Hit {
                kind: HitKind::Query,
                path: "Query.userRead".into(),
                summary: "userRead(id: ID!): User".into(),
            },
            Hit {
                kind: HitKind::Type,
                path: "User".into(),
                summary: "OBJECT".into(),
            },
        ];
        assert_eq!(
            hits_text(&hits),
            "Query.userRead  userRead(id: ID!): User\nUser  OBJECT\n"
        );
    }

    #[test]
    fn no_hits_prints_nothing() {
        assert_eq!(hits_text(&[]), "");
    }

    fn id_variable() -> vega_core::querygen::VariableDef {
        use vega_core::schema::{TypeKind, TypeRef};
        vega_core::querygen::VariableDef {
            name: "id".into(),
            ty: "ID!".into(),
            required: true,
            default: None,
            path: Vec::new(),
            type_ref: TypeRef {
                kind: TypeKind::Scalar,
                name: Some("ID".into()),
                of_type: None,
            },
        }
    }

    #[test]
    fn query_text_lists_the_variables_after_the_query() {
        let query = GeneratedQuery {
            operation_name: "UserRead".into(),
            text: "query UserRead($id: ID!) {\n  userRead(id: $id) {\n    id\n  }\n}".into(),
            variables: vec![id_variable()],
        };
        assert_eq!(
            query_text(&query, &[State::Unset]),
            "query UserRead($id: ID!) {\n  userRead(id: $id) {\n    id\n  }\n}\n\nVariables:\n  $id: ID!\n"
        );
    }

    #[test]
    fn query_text_shows_the_value_of_a_variable_that_has_one() {
        let query = GeneratedQuery {
            operation_name: "UserRead".into(),
            text: "query UserRead($id: ID!) {\n  userRead(id: $id) {\n    id\n  }\n}".into(),
            variables: vec![id_variable()],
        };
        let states = [State::Value(serde_json::json!("42"))];
        assert!(query_text(&query, &states).ends_with("Variables:\n  $id: ID! = 42\n"));
    }

    #[test]
    fn query_text_without_variables_is_just_the_query() {
        let query = GeneratedQuery {
            operation_name: "Version".into(),
            text: "query Version {\n  version\n}".into(),
            variables: Vec::new(),
        };
        assert_eq!(query_text(&query, &[]), "query Version {\n  version\n}\n");
    }

    #[test]
    fn query_json_adds_the_value_to_variables_that_have_one() {
        let query = GeneratedQuery {
            operation_name: "UserRead".into(),
            text: "query UserRead($id: ID!) {\n  userRead(id: $id) {\n    id\n  }\n}".into(),
            variables: vec![id_variable()],
        };
        let json = query_json(&query, &[State::Value(serde_json::json!("42"))]).unwrap();
        assert_eq!(json["variables"][0]["value"], "42");
        let json = query_json(&query, &[State::Unset]).unwrap();
        assert!(json["variables"][0].get("value").is_none());
    }

    #[test]
    fn run_json_wraps_the_parsed_body_in_an_envelope() {
        use std::time::Duration;
        use vega_core::client::Response;
        let query = GeneratedQuery {
            operation_name: "Me".into(),
            text: "query Me { me { id } }".into(),
            variables: Vec::new(),
        };
        let response = Response {
            status: 200,
            elapsed: Duration::from_millis(7),
            body: r#"{"data":{"me":null}}"#.into(),
        };
        let json = run_json(&query, &response);
        assert_eq!(json["operationName"], "Me");
        assert_eq!(json["status"], 200);
        assert_eq!(json["elapsedMs"], 7);
        assert_eq!(json["response"]["data"]["me"], serde_json::Value::Null);

        let raw = Response {
            body: "<html>".into(),
            ..response
        };
        assert_eq!(run_json(&query, &raw)["response"], "<html>");
    }

    #[test]
    fn run_failure_names_the_reason() {
        use vega_core::response::ResponseView;
        let view = |status, not_json, errors: Vec<&str>| ResponseView {
            status,
            elapsed_ms: 1,
            errors: errors.into_iter().map(str::to_string).collect(),
            not_json,
            lines: Vec::new(),
            total_lines: 0,
        };
        assert_eq!(
            run_failure(&view(500, false, vec![])),
            "server answered HTTP 500"
        );
        assert_eq!(
            run_failure(&view(200, true, vec![])),
            "the response is not valid JSON"
        );
        assert_eq!(
            run_failure(&view(200, false, vec!["a", "b"])),
            "GraphQL errors: a; b"
        );
    }
}
