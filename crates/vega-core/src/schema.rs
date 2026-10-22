//! Introspection response model. Field names follow the GraphQL spec so the
//! structs round-trip through serde unchanged.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IntrospectionResponse {
    pub data: IntrospectionData,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IntrospectionData {
    #[serde(rename = "__schema")]
    pub schema: Schema,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Schema {
    #[serde(rename = "queryType")]
    pub query_type: Option<TypeName>,
    #[serde(rename = "mutationType")]
    pub mutation_type: Option<TypeName>,
    #[serde(rename = "subscriptionType")]
    pub subscription_type: Option<TypeName>,
    pub types: Vec<FullType>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TypeName {
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TypeKind {
    Scalar,
    Object,
    Interface,
    Union,
    Enum,
    InputObject,
    List,
    NonNull,
}

impl TypeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TypeKind::Scalar => "SCALAR",
            TypeKind::Object => "OBJECT",
            TypeKind::Interface => "INTERFACE",
            TypeKind::Union => "UNION",
            TypeKind::Enum => "ENUM",
            TypeKind::InputObject => "INPUT_OBJECT",
            TypeKind::List => "LIST",
            TypeKind::NonNull => "NON_NULL",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FullType {
    pub kind: TypeKind,
    pub name: Option<String>,
    pub description: Option<String>,
    pub fields: Option<Vec<Field>>,
    #[serde(rename = "inputFields")]
    pub input_fields: Option<Vec<InputValue>>,
    #[serde(rename = "enumValues")]
    pub enum_values: Option<Vec<EnumValue>>,
    #[serde(rename = "possibleTypes")]
    pub possible_types: Option<Vec<TypeRef>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Field {
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub args: Vec<InputValue>,
    #[serde(rename = "type")]
    pub ty: TypeRef,
    #[serde(rename = "isDeprecated", default)]
    pub is_deprecated: bool,
    #[serde(rename = "deprecationReason")]
    pub deprecation_reason: Option<String>,
}

impl Field {
    /// `name(arg: Type, ...): ReturnType`, or `name: ReturnType` without arguments.
    pub fn signature(&self) -> String {
        let args = if self.args.is_empty() {
            String::new()
        } else {
            let rendered: Vec<String> = self.args.iter().map(InputValue::render).collect();
            format!("({})", rendered.join(", "))
        };
        format!("{}{}: {}", self.name, args, self.ty.render())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InputValue {
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "type")]
    pub ty: TypeRef,
    #[serde(rename = "defaultValue")]
    pub default_value: Option<String>,
}

impl InputValue {
    /// `name: Type`, with ` = default` when the server reports one.
    pub fn render(&self) -> String {
        match &self.default_value {
            Some(default) => format!("{}: {} = {}", self.name, self.ty.render(), default),
            None => format!("{}: {}", self.name, self.ty.render()),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EnumValue {
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "isDeprecated", default)]
    pub is_deprecated: bool,
    #[serde(rename = "deprecationReason")]
    pub deprecation_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TypeRef {
    pub kind: TypeKind,
    pub name: Option<String>,
    #[serde(rename = "ofType")]
    pub of_type: Option<Box<TypeRef>>,
}

impl TypeRef {
    /// GraphQL notation, e.g. `[User!]!`.
    pub fn render(&self) -> String {
        let inner = || {
            self.of_type
                .as_deref()
                .map(TypeRef::render)
                .unwrap_or_default()
        };
        match self.kind {
            TypeKind::NonNull => format!("{}!", inner()),
            TypeKind::List => format!("[{}]", inner()),
            _ => self.name.clone().unwrap_or_default(),
        }
    }

    /// The innermost named type, ignoring list and non-null wrappers.
    pub fn named(&self) -> Option<&str> {
        match &self.of_type {
            Some(inner) => inner.named(),
            None => self.name.as_deref(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Root {
    Query,
    Mutation,
    Subscription,
}

impl Root {
    pub const ALL: [Root; 3] = [Root::Query, Root::Mutation, Root::Subscription];

    pub fn label(self) -> &'static str {
        match self {
            Root::Query => "Query",
            Root::Mutation => "Mutation",
            Root::Subscription => "Subscription",
        }
    }
}

impl Schema {
    pub fn root_type_name(&self, root: Root) -> Option<&str> {
        let name = match root {
            Root::Query => &self.query_type,
            Root::Mutation => &self.mutation_type,
            Root::Subscription => &self.subscription_type,
        };
        name.as_ref().map(|t| t.name.as_str())
    }

    pub fn root_type(&self, root: Root) -> Option<&FullType> {
        self.type_by_name(self.root_type_name(root)?)
    }

    pub fn type_by_name(&self, name: &str) -> Option<&FullType> {
        self.types.iter().find(|t| t.name.as_deref() == Some(name))
    }

    /// Like `type_by_name`, but `Query`, `Mutation` and `Subscription` also
    /// resolve to the server's root types even when they are named differently.
    pub fn resolve_type(&self, name: &str) -> Option<&FullType> {
        if let Some(found) = self.type_by_name(name) {
            return Some(found);
        }
        let root = Root::ALL.into_iter().find(|r| r.label() == name)?;
        self.root_type(root)
    }

    pub fn is_root_type(&self, name: &str) -> bool {
        Root::ALL
            .into_iter()
            .any(|r| self.root_type_name(r) == Some(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::fixture_schema;

    #[test]
    fn parses_fixture_roots_and_types() {
        let schema = fixture_schema();
        assert_eq!(schema.root_type_name(Root::Query), Some("QueryType"));
        assert_eq!(schema.root_type_name(Root::Mutation), Some("MutationType"));
        assert_eq!(schema.root_type_name(Root::Subscription), None);
        assert_eq!(schema.types.len(), 10);
    }

    #[test]
    fn type_ref_renders_and_unwraps() {
        let schema = fixture_schema();
        let field = &schema
            .root_type(Root::Query)
            .unwrap()
            .fields
            .as_ref()
            .unwrap()[1];
        assert_eq!(field.name, "userSearch");
        assert_eq!(field.ty.render(), "[User!]!");
        assert_eq!(field.ty.named(), Some("User"));
    }

    #[test]
    fn field_signature_includes_arguments() {
        let schema = fixture_schema();
        let field = &schema
            .root_type(Root::Query)
            .unwrap()
            .fields
            .as_ref()
            .unwrap()[0];
        assert_eq!(field.signature(), "userRead(id: ID!): User");
        let user = schema.type_by_name("User").unwrap();
        assert_eq!(user.fields.as_ref().unwrap()[0].signature(), "id: ID!");
    }

    #[test]
    fn resolve_type_accepts_root_aliases() {
        let schema = fixture_schema();
        assert_eq!(
            schema.resolve_type("Query").and_then(|t| t.name.as_deref()),
            Some("QueryType")
        );
        assert_eq!(
            schema.resolve_type("User").and_then(|t| t.name.as_deref()),
            Some("User")
        );
        assert!(schema.resolve_type("Nope").is_none());
        assert!(schema.is_root_type("QueryType"));
        assert!(!schema.is_root_type("User"));
    }
}
