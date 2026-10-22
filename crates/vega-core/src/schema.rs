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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InputValue {
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "type")]
    pub ty: TypeRef,
    #[serde(rename = "defaultValue")]
    pub default_value: Option<String>,
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
}
