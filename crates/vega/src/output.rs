//! Plain-text rendering for CLI output.

use vega_core::search::Hit;

/// One `path  summary` line per hit.
pub fn hits_text(hits: &[Hit]) -> String {
    hits.iter()
        .map(|hit| format!("{}  {}\n", hit.path, hit.summary))
        .collect()
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
}
