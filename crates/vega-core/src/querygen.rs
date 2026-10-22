//! Turns a selection of leaf fields into GraphQL query text plus the variables the
//! query needs. Required arguments are always emitted (as variables), optional ones only when
//! opted in; everything is
//! generated in schema order so the same selection always gives the same text.

use std::collections::HashSet;

use serde::Serialize;

use crate::schema::{Field, FullType, InputValue, Root, Schema, TypeKind, TypeRef};
use crate::selection::{OperationRef, Path, Seg, Selection};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VariableDef {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    /// Non-null with no default: the server cannot do without a value.
    pub required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// Where the argument sits in the tree; keys the value the user typed for it.
    #[serde(skip)]
    pub path: Path,
    /// The argument's full type, for choosing an editor and validating values.
    #[serde(skip)]
    pub type_ref: TypeRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GeneratedQuery {
    #[serde(rename = "operationName")]
    pub operation_name: String,
    #[serde(rename = "query")]
    pub text: String,
    pub variables: Vec<VariableDef>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum GenerateError {
    #[error("unknown operation `{0}`")]
    UnknownOperation(String),
    #[error("nothing selected: select at least one field")]
    EmptySelection,
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn is_composite(kind: TypeKind) -> bool {
    matches!(
        kind,
        TypeKind::Object | TypeKind::Interface | TypeKind::Union
    )
}

fn arg_path(field_path: &[Seg], name: &str) -> Path {
    let mut path = field_path.to_vec();
    path.push(Seg::Args);
    path.push(Seg::Arg(name.to_string()));
    path
}

struct Builder<'a> {
    schema: &'a Schema,
    selection: &'a Selection,
    variables: Vec<VariableDef>,
    used: HashSet<String>,
}

impl Builder<'_> {
    /// `name` or `name(arg: $arg, ...)`, registering a variable per included argument:
    /// every required one, plus the optional ones the user opted into.
    fn call(&mut self, field: &Field, field_path: &[Seg]) -> String {
        let included: Vec<&InputValue> = field
            .args
            .iter()
            .filter(|arg| {
                arg.is_required()
                    || self
                        .selection
                        .arg_included(&arg_path(field_path, &arg.name))
            })
            .collect();
        let args: Vec<String> = included
            .into_iter()
            .map(|arg| {
                let variable = self.variable_for(&field.name, field_path, arg);
                format!("{}: ${variable}", arg.name)
            })
            .collect();
        if args.is_empty() {
            field.name.clone()
        } else {
            format!("{}({})", field.name, args.join(", "))
        }
    }

    /// The argument's own name if free, else `<field><Arg>`, then `<name>2`, `<name>3`...
    fn variable_for(&mut self, field_name: &str, field_path: &[Seg], arg: &InputValue) -> String {
        let mut name = arg.name.clone();
        if self.used.contains(&name) {
            name = format!("{field_name}{}", capitalize(&arg.name));
        }
        let base = name.clone();
        let mut counter = 2;
        while self.used.contains(&name) {
            name = format!("{base}{counter}");
            counter += 1;
        }
        self.used.insert(name.clone());
        self.variables.push(VariableDef {
            name: name.clone(),
            ty: arg.ty.render(),
            required: arg.is_required(),
            default: arg.default_value.clone(),
            path: arg_path(field_path, &arg.name),
            type_ref: arg.ty.clone(),
        });
        name
    }

    /// The lines of the selection set of `ty`, indented at `depth`; empty when no
    /// selected leaf lies below `path`.
    fn selection_set(&mut self, ty: &FullType, path: &mut Path, depth: usize) -> Vec<String> {
        let schema = self.schema;
        let indent = "  ".repeat(depth);
        let mut lines = Vec::new();

        for field in ty.fields.iter().flatten() {
            path.push(Seg::Field(field.name.clone()));
            if self.selection.has_under(&path[..]) {
                let call = self.call(field, &path[..]);
                let target = field.ty.named().and_then(|name| schema.type_by_name(name));
                match target {
                    Some(inner) if is_composite(inner.kind) => {
                        lines.push(format!("{indent}{call} {{"));
                        lines.extend(self.selection_set(inner, path, depth + 1));
                        lines.push(format!("{indent}}}"));
                    }
                    _ => lines.push(format!("{indent}{call}")),
                }
            }
            path.pop();
        }

        for possible in ty.possible_types.iter().flatten() {
            let Some(name) = possible.named() else {
                continue;
            };
            path.push(Seg::Branch(name.to_string()));
            if self.selection.has_under(&path[..]) {
                if let Some(inner) = schema.type_by_name(name) {
                    lines.push(format!("{indent}... on {name} {{"));
                    lines.extend(self.selection_set(inner, path, depth + 1));
                    lines.push(format!("{indent}}}"));
                }
            }
            path.pop();
        }

        if !lines.is_empty() && matches!(ty.kind, TypeKind::Union | TypeKind::Interface) {
            lines.insert(0, format!("{indent}__typename"));
        }
        lines
    }
}

pub fn generate(
    schema: &Schema,
    operation: &OperationRef,
    selection: &Selection,
) -> Result<GeneratedQuery, GenerateError> {
    let field = operation
        .field(schema)
        .ok_or_else(|| GenerateError::UnknownOperation(operation.label()))?;
    let mut builder = Builder {
        schema,
        selection,
        variables: Vec::new(),
        used: HashSet::new(),
    };
    let call = builder.call(field, &[]);

    let mut body = Vec::new();
    let target = field.ty.named().and_then(|name| schema.type_by_name(name));
    match target {
        Some(ty) if is_composite(ty.kind) => {
            let inner = builder.selection_set(ty, &mut Path::new(), 2);
            if inner.is_empty() {
                return Err(GenerateError::EmptySelection);
            }
            body.push(format!("  {call} {{"));
            body.extend(inner);
            body.push("  }".to_string());
        }
        _ => body.push(format!("  {call}")),
    }

    let keyword = match operation.root {
        Root::Query => "query",
        Root::Mutation => "mutation",
        Root::Subscription => "subscription",
    };
    let name = capitalize(&field.name);
    let header = if builder.variables.is_empty() {
        format!("{keyword} {name} {{")
    } else {
        let defs: Vec<String> = builder
            .variables
            .iter()
            .map(|v| match &v.default {
                Some(default) => format!("${}: {} = {default}", v.name, v.ty),
                None => format!("${}: {}", v.name, v.ty),
            })
            .collect();
        format!("{keyword} {name}({}) {{", defs.join(", "))
    };
    let text = format!("{header}\n{}\n}}", body.join("\n"));

    Ok(GeneratedQuery {
        operation_name: name,
        text,
        variables: builder.variables,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::parse_select;
    use crate::testutil::design_schema;

    fn gen(operation: &str, select: &str) -> GeneratedQuery {
        let schema = design_schema();
        let op = OperationRef::parse(&schema, operation).unwrap();
        let selection = if select.is_empty() {
            Selection::default()
        } else {
            parse_select(&schema, &op, select).unwrap()
        };
        generate(&schema, &op, &selection).unwrap()
    }

    fn pairs(q: &GeneratedQuery) -> Vec<(&str, &str)> {
        q.variables
            .iter()
            .map(|v| (v.name.as_str(), v.ty.as_str()))
            .collect()
    }

    #[test]
    fn selects_leaves_in_schema_order_whatever_the_selection_order() {
        let expected = "query Me {\n  me {\n    id\n    name\n  }\n}";
        let a = gen("Query.me", "id,name");
        let b = gen("Query.me", "name,id");
        assert_eq!(a.text, expected);
        assert_eq!(b.text, expected);
        assert_eq!(a.operation_name, "Me");
        assert!(a.variables.is_empty());
    }

    #[test]
    fn nested_objects_appear_only_with_a_selected_leaf_below_them() {
        let q = gen("Query.me", "id,friends.name");
        assert_eq!(
            q.text,
            "query Me {\n  me {\n    id\n    friends {\n      name\n    }\n  }\n}"
        );
    }

    #[test]
    fn required_arguments_become_variables() {
        let q = gen("Query.me", "posts.title");
        assert_eq!(
            q.text,
            "query Me($first: Int!) {\n  me {\n    posts(first: $first) {\n      title\n    }\n  }\n}"
        );
        assert_eq!(pairs(&q), [("first", "Int!")]);
    }

    #[test]
    fn optional_arguments_are_not_emitted() {
        let q = gen("Query.search", "on:User.name");
        assert_eq!(
            q.text,
            "query Search($term: String!) {\n  search(term: $term) {\n    __typename\n    ... on User {\n      name\n    }\n  }\n}"
        );
        assert_eq!(pairs(&q), [("term", "String!")]);
    }

    #[test]
    fn interfaces_emit_typename_shared_fields_then_fragments() {
        let q = gen("Query.node", "id,on:User.name,on:Post.title");
        assert_eq!(
            q.text,
            "query Node($id: ID!) {\n  node(id: $id) {\n    __typename\n    id\n    ... on User {\n      name\n    }\n    ... on Post {\n      title\n    }\n  }\n}"
        );
    }

    #[test]
    fn colliding_variable_names_are_prefixed_with_the_field() {
        let q = gen("Query.node", "on:User.post.title");
        assert_eq!(
            q.text,
            "query Node($id: ID!, $postId: ID!) {\n  node(id: $id) {\n    __typename\n    ... on User {\n      post(id: $postId) {\n        title\n      }\n    }\n  }\n}"
        );
        assert_eq!(pairs(&q), [("id", "ID!"), ("postId", "ID!")]);
    }

    #[test]
    fn mutations_use_their_keyword_and_input_types() {
        let q = gen("Mutation.postCreate", "id");
        assert_eq!(
            q.text,
            "mutation PostCreate($input: PostInput!) {\n  postCreate(input: $input) {\n    id\n  }\n}"
        );
        assert_eq!(pairs(&q), [("input", "PostInput!")]);
    }

    #[test]
    fn scalar_operations_need_no_selection() {
        let q = gen("Query.version", "");
        assert_eq!(q.text, "query Version {\n  version\n}");
    }

    #[test]
    fn an_empty_selection_on_an_object_is_an_error() {
        let schema = design_schema();
        let op = OperationRef::parse(&schema, "Query.me").unwrap();
        assert_eq!(
            generate(&schema, &op, &Selection::default()),
            Err(GenerateError::EmptySelection)
        );
    }

    #[test]
    fn serializes_for_the_command_line() {
        let q = gen("Query.node", "id");
        let json = serde_json::to_value(&q).unwrap();
        assert_eq!(json["operationName"], "Node");
        assert!(json["query"].as_str().unwrap().starts_with("query Node("));
        assert_eq!(json["variables"][0]["name"], "id");
        assert_eq!(json["variables"][0]["type"], "ID!");
        assert_eq!(json["variables"][0]["required"], true);
        assert!(json["variables"][0].get("path").is_none());
    }

    #[test]
    fn opted_in_optional_arguments_become_variables_in_schema_order() {
        let q = gen("Query.search", "on:User.name,arg:first");
        assert_eq!(
            q.text,
            "query Search($term: String!, $first: Int) {\n  search(term: $term, first: $first) {\n    __typename\n    ... on User {\n      name\n    }\n  }\n}"
        );
        assert_eq!(pairs(&q), [("term", "String!"), ("first", "Int")]);
        assert!(q.variables[0].required);
        assert!(!q.variables[1].required);
    }

    #[test]
    fn an_optional_argument_alone_does_not_make_a_valid_selection() {
        let schema = design_schema();
        let op = OperationRef::parse(&schema, "Query.users").unwrap();
        let selection = parse_select(&schema, &op, "arg:role").unwrap();
        assert_eq!(
            generate(&schema, &op, &selection),
            Err(GenerateError::EmptySelection)
        );
    }

    #[test]
    fn defaulted_arguments_keep_their_default_in_the_definition() {
        let q = gen("Query.users", "id,arg:limit");
        assert_eq!(
            q.text,
            "query Users($limit: Int! = 10) {\n  users(limit: $limit) {\n    id\n  }\n}"
        );
        assert_eq!(q.variables[0].default.as_deref(), Some("10"));
        assert!(!q.variables[0].required);
    }

    #[test]
    fn scalar_operations_can_take_opted_in_arguments() {
        let q = gen("Query.postCount", "arg:since");
        assert_eq!(
            q.text,
            "query PostCount($since: String) {\n  postCount(since: $since)\n}"
        );
    }

    #[test]
    fn variables_remember_where_their_argument_sits() {
        let q = gen("Query.me", "posts.title");
        assert_eq!(
            q.variables[0].path,
            vec![
                Seg::Field("posts".to_string()),
                Seg::Args,
                Seg::Arg("first".to_string())
            ]
        );
        assert_eq!(q.variables[0].type_ref.render(), "Int!");
    }
}
