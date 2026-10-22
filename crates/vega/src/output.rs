//! Plain-text rendering for CLI output.

use vega_core::querygen::GeneratedQuery;
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
}
