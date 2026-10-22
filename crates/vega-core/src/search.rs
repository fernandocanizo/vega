//! Search over a schema's operations (root fields) and named types.

use serde::Serialize;

use crate::schema::{Root, Schema};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HitKind {
    Query,
    Mutation,
    Subscription,
    Type,
}

impl HitKind {
    fn rank(self) -> u8 {
        match self {
            HitKind::Query => 0,
            HitKind::Mutation => 1,
            HitKind::Subscription => 2,
            HitKind::Type => 3,
        }
    }
}

fn kind_of(root: Root) -> HitKind {
    match root {
        Root::Query => HitKind::Query,
        Root::Mutation => HitKind::Mutation,
        Root::Subscription => HitKind::Subscription,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hit {
    pub kind: HitKind,
    /// `Query.userRead` for operations, `User` for types. Feed it to `describe::detail`.
    pub path: String,
    /// Field signature for operations, type kind (e.g. `OBJECT`) for types.
    pub summary: String,
}

type Ranked = Vec<((bool, u8, String), Hit)>;

pub fn search(schema: &Schema, text: &str) -> Vec<Hit> {
    let needle = text.trim().to_lowercase();
    let mut ranked: Ranked = Vec::new();

    for root in Root::ALL {
        let Some(ty) = schema.root_type(root) else {
            continue;
        };
        for field in ty.fields.iter().flatten() {
            let path = format!("{}.{}", root.label(), field.name);
            push_if_match(
                &mut ranked,
                &needle,
                kind_of(root),
                &field.name,
                path,
                field.signature(),
            );
        }
    }

    for ty in &schema.types {
        let Some(name) = ty.name.as_deref() else {
            continue;
        };
        if name.starts_with("__") || schema.is_root_type(name) {
            continue;
        }
        push_if_match(
            &mut ranked,
            &needle,
            HitKind::Type,
            name,
            name.to_string(),
            ty.kind.as_str().to_string(),
        );
    }

    ranked.sort_by(|a, b| a.0.cmp(&b.0));
    ranked.into_iter().map(|(_, hit)| hit).collect()
}

fn push_if_match(
    ranked: &mut Ranked,
    needle: &str,
    kind: HitKind,
    bare_name: &str,
    path: String,
    summary: String,
) {
    let bare_lower = bare_name.to_lowercase();
    let haystack = if needle.contains('.') {
        path.to_lowercase()
    } else {
        bare_lower.clone()
    };
    if !haystack.contains(needle) {
        return;
    }
    let is_prefix = haystack.starts_with(needle);
    ranked.push((
        (!is_prefix, kind.rank(), bare_lower),
        Hit {
            kind,
            path,
            summary,
        },
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::fixture_schema;

    fn paths(hits: &[Hit]) -> Vec<&str> {
        hits.iter().map(|h| h.path.as_str()).collect()
    }

    #[test]
    fn empty_text_lists_everything_except_roots_and_introspection_types() {
        let hits = search(&fixture_schema(), "");
        assert_eq!(hits.len(), 10);
        assert!(!paths(&hits).contains(&"QueryType"));
        assert!(!paths(&hits).contains(&"__Type"));
    }

    #[test]
    fn matches_are_case_insensitive_and_operations_come_first() {
        let hits = search(&fixture_schema(), "USER");
        assert_eq!(
            paths(&hits),
            [
                "Query.userRead",
                "Query.userSearch",
                "Mutation.userCreate",
                "User",
                "UserInput"
            ]
        );
    }

    #[test]
    fn prefix_matches_rank_before_inner_matches() {
        let hits = search(&fixture_schema(), "search");
        assert_eq!(paths(&hits), ["SearchResult", "Query.userSearch"]);
    }

    #[test]
    fn dotted_text_matches_the_full_path() {
        let hits = search(&fixture_schema(), "query.user");
        assert_eq!(paths(&hits), ["Query.userRead", "Query.userSearch"]);
    }

    #[test]
    fn hit_summaries_and_kinds() {
        let hits = search(&fixture_schema(), "userread");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].kind, HitKind::Query);
        assert_eq!(hits[0].summary, "userRead(id: ID!): User");

        let hits = search(&fixture_schema(), "role");
        assert_eq!(paths(&hits), ["Role"]);
        assert_eq!(hits[0].kind, HitKind::Type);
        assert_eq!(hits[0].summary, "ENUM");
    }

    #[test]
    fn no_match_returns_empty() {
        assert!(search(&fixture_schema(), "zzz").is_empty());
    }

    #[test]
    fn hits_serialize_with_lowercase_kind() {
        let hits = search(&fixture_schema(), "userread");
        let json = serde_json::to_value(&hits).unwrap();
        assert_eq!(json[0]["kind"], "query");
        assert_eq!(json[0]["path"], "Query.userRead");
    }
}
