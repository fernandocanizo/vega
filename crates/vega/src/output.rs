//! Plain-text rendering for CLI output.

use vega_core::querygen::GeneratedQuery;
use vega_core::search::Hit;

/// One `path  summary` line per hit.
pub fn hits_text(hits: &[Hit]) -> String {
    hits.iter()
        .map(|hit| format!("{}  {}\n", hit.path, hit.summary))
        .collect()
}

/// The query text, then the variables it needs (if any), one per line.
pub fn query_text(query: &GeneratedQuery) -> String {
    let mut out = query.text.clone();
    out.push('\n');
    if !query.variables.is_empty() {
        out.push_str("\nVariables:\n");
        for variable in &query.variables {
            out.push_str(&format!("  ${}: {}\n", variable.name, variable.ty));
        }
    }
    out
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

    #[test]
    fn query_text_lists_the_variables_after_the_query() {
        use vega_core::querygen::{GeneratedQuery, VariableDef};
        let query = GeneratedQuery {
            operation_name: "UserRead".into(),
            text: "query UserRead($id: ID!) {\n  userRead(id: $id) {\n    id\n  }\n}".into(),
            variables: vec![VariableDef {
                name: "id".into(),
                ty: "ID!".into(),
            }],
        };
        assert_eq!(
            query_text(&query),
            "query UserRead($id: ID!) {\n  userRead(id: $id) {\n    id\n  }\n}\n\nVariables:\n  $id: ID!\n"
        );
    }

    #[test]
    fn query_text_without_variables_is_just_the_query() {
        use vega_core::querygen::GeneratedQuery;
        let query = GeneratedQuery {
            operation_name: "Version".into(),
            text: "query Version {\n  version\n}".into(),
            variables: Vec::new(),
        };
        assert_eq!(query_text(&query), "query Version {\n  version\n}\n");
    }
}
