//! A navigable view of one type or one field, shared by the CLI and the TUI.

use serde::Serialize;

use crate::schema::{FullType, Schema};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NavItem {
    pub label: String,
    /// A path accepted by [`detail`], or `None` when the item cannot be followed.
    pub target: Option<String>,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Detail {
    pub title: String,
    pub body: String,
    pub nav: Vec<NavItem>,
}

#[derive(Debug, thiserror::Error)]
pub enum DescribeError {
    #[error("type `{0}` not found")]
    TypeNotFound(String),
    #[error("field `{field}` not found on type `{type_name}`")]
    FieldNotFound { type_name: String, field: String },
}

/// Describe `Type` or `Type.field`.
pub fn detail(schema: &Schema, target: &str) -> Result<Detail, DescribeError> {
    let (type_part, field_part) = match target.split_once('.') {
        Some((t, f)) => (t, Some(f)),
        None => (target, None),
    };
    let ty = schema
        .resolve_type(type_part)
        .ok_or_else(|| DescribeError::TypeNotFound(type_part.to_string()))?;
    match field_part {
        None => Ok(type_detail(ty)),
        Some(field) => field_detail(type_part, ty, field),
    }
}

fn type_detail(ty: &FullType) -> Detail {
    let name = ty.name.clone().unwrap_or_default();
    let mut nav = Vec::new();

    for field in ty.fields.iter().flatten() {
        nav.push(NavItem {
            label: field.signature(),
            target: field.ty.named().map(str::to_string),
            description: field.description.clone().unwrap_or_default(),
        });
    }
    for input in ty.input_fields.iter().flatten() {
        nav.push(NavItem {
            label: input.render(),
            target: input.ty.named().map(str::to_string),
            description: input.description.clone().unwrap_or_default(),
        });
    }
    for value in ty.enum_values.iter().flatten() {
        nav.push(NavItem {
            label: value.name.clone(),
            target: None,
            description: value.description.clone().unwrap_or_default(),
        });
    }
    for possible in ty.possible_types.iter().flatten() {
        nav.push(NavItem {
            label: possible.render(),
            target: possible.named().map(str::to_string),
            description: String::new(),
        });
    }

    Detail {
        title: format!("{name} ({})", ty.kind.as_str()),
        body: ty.description.clone().unwrap_or_default(),
        nav,
    }
}

fn field_detail(
    type_label: &str,
    ty: &FullType,
    field_name: &str,
) -> Result<Detail, DescribeError> {
    let field = ty
        .fields
        .iter()
        .flatten()
        .find(|f| f.name == field_name)
        .ok_or_else(|| DescribeError::FieldNotFound {
            type_name: type_label.to_string(),
            field: field_name.to_string(),
        })?;

    let mut body = field.description.clone().unwrap_or_default();
    if field.is_deprecated {
        let reason = field
            .deprecation_reason
            .as_deref()
            .unwrap_or("no reason given");
        if !body.is_empty() {
            body.push_str("\n\n");
        }
        body.push_str(&format!("Deprecated: {reason}"));
    }

    let mut nav = vec![NavItem {
        label: format!("returns {}", field.ty.render()),
        target: field.ty.named().map(str::to_string),
        description: String::new(),
    }];
    for arg in &field.args {
        nav.push(NavItem {
            label: arg.render(),
            target: arg.ty.named().map(str::to_string),
            description: arg.description.clone().unwrap_or_default(),
        });
    }

    Ok(Detail {
        title: format!("{type_label}.{}", field.signature()),
        body,
        nav,
    })
}

/// Plain-text rendering used by `vega --describe`.
pub fn render_text(detail: &Detail) -> String {
    let mut out = String::new();
    out.push_str(&detail.title);
    out.push('\n');
    if !detail.body.is_empty() {
        out.push('\n');
        out.push_str(&detail.body);
        out.push('\n');
    }
    if !detail.nav.is_empty() {
        out.push('\n');
    }
    for item in &detail.nav {
        out.push_str("  ");
        out.push_str(&item.label);
        out.push('\n');
        for line in item.description.lines() {
            out.push_str(format!("      {line}").trim_end());
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::fixture_schema;

    fn labels(d: &Detail) -> Vec<&str> {
        d.nav.iter().map(|n| n.label.as_str()).collect()
    }

    #[test]
    fn field_detail_shows_signature_docs_return_type_and_args() {
        let d = detail(&fixture_schema(), "Query.userRead").unwrap();
        assert_eq!(d.title, "Query.userRead(id: ID!): User");
        assert!(d.body.contains("Requires USER_READ"));
        assert_eq!(labels(&d), ["returns User", "id: ID!"]);
        assert_eq!(d.nav[0].target.as_deref(), Some("User"));
        assert_eq!(d.nav[1].target.as_deref(), Some("ID"));
        assert_eq!(d.nav[1].description, "The user id.");
    }

    #[test]
    fn object_type_detail_lists_fields_that_jump_to_their_types() {
        let d = detail(&fixture_schema(), "User").unwrap();
        assert_eq!(d.title, "User (OBJECT)");
        assert_eq!(d.body, "A person with an account.");
        assert_eq!(labels(&d), ["id: ID!", "name: String", "role: Role"]);
        assert_eq!(d.nav[2].target.as_deref(), Some("Role"));
        assert_eq!(d.nav[2].description, "Access role.");
    }

    #[test]
    fn enum_values_are_not_navigable() {
        let d = detail(&fixture_schema(), "Role").unwrap();
        assert_eq!(labels(&d), ["ADMIN", "VIEWER"]);
        assert!(d.nav.iter().all(|n| n.target.is_none()));
        assert_eq!(d.nav[0].description, "Full access.");
    }

    #[test]
    fn input_object_and_union_detail() {
        let d = detail(&fixture_schema(), "UserInput").unwrap();
        assert_eq!(labels(&d), ["name: String!", "role: Role"]);
        assert_eq!(d.nav[0].target.as_deref(), Some("String"));

        let d = detail(&fixture_schema(), "SearchResult").unwrap();
        assert_eq!(labels(&d), ["User"]);
        assert_eq!(d.nav[0].target.as_deref(), Some("User"));
    }

    #[test]
    fn root_alias_resolves_to_the_server_root_type() {
        let d = detail(&fixture_schema(), "Query").unwrap();
        assert_eq!(d.title, "QueryType (OBJECT)");
        assert_eq!(d.nav.len(), 2);
    }

    #[test]
    fn unknown_targets_are_errors() {
        let schema = fixture_schema();
        let err = detail(&schema, "Nope").unwrap_err();
        assert_eq!(err.to_string(), "type `Nope` not found");
        let err = detail(&schema, "User.nope").unwrap_err();
        assert_eq!(err.to_string(), "field `nope` not found on type `User`");
    }

    #[test]
    fn render_text_prints_title_body_and_indented_nav() {
        let text = render_text(&detail(&fixture_schema(), "Query.userRead").unwrap());
        let expected = "\
Query.userRead(id: ID!): User

Fetch one user.

Requires USER_READ

  returns User
  id: ID!
      The user id.
";
        assert_eq!(text, expected);
    }

    #[test]
    fn detail_serializes_to_json() {
        let d = detail(&fixture_schema(), "Role").unwrap();
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["title"], "Role (ENUM)");
        assert_eq!(json["nav"][0]["label"], "ADMIN");
        assert!(json["nav"][0]["target"].is_null());
    }
}
