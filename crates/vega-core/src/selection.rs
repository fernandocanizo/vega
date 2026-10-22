//! A lazy tree over an operation's return type (or over a type), plus the set of
//! selected leaf fields. Pure data: no terminal or network code.

use std::collections::BTreeSet;

use crate::schema::{Field, FullType, Root, Schema, TypeKind};

/// A type that has already appeared this many times on the way down is shown as
/// recursive and cannot be expanded again, which keeps self-referencing schemas finite.
const MAX_REPEATS: usize = 2;

/// One step from a node down to a child.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Seg {
    /// A field (or input field, or enum value) of the current type.
    Field(String),
    /// A union or interface branch: `... on Type`.
    Branch(String),
}

/// Steps from the tree root down to a node. The root itself is the empty path.
pub type Path = Vec<Seg>;

fn join(path: &[Seg], seg: Seg) -> Path {
    let mut joined = path.to_vec();
    joined.push(seg);
    joined
}

/// A root operation field such as `Query.userRead`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationRef {
    pub root: Root,
    pub field: String,
}

impl OperationRef {
    /// Parses `Query.userRead`; the field must exist on that root type.
    pub fn parse(schema: &Schema, text: &str) -> Option<Self> {
        let (label, field) = text.split_once('.')?;
        let root = Root::ALL.into_iter().find(|r| r.label() == label)?;
        let operation = Self {
            root,
            field: field.to_string(),
        };
        operation.field(schema)?;
        Some(operation)
    }

    pub fn label(&self) -> String {
        format!("{}.{}", self.root.label(), self.field)
    }

    pub fn field<'a>(&self, schema: &'a Schema) -> Option<&'a Field> {
        schema
            .root_type(self.root)?
            .fields
            .iter()
            .flatten()
            .find(|f| f.name == self.field)
    }
}

/// What a tree is rooted at: an operation, or a plain type (read-only exploration).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeRoot {
    Operation(OperationRef),
    Type(String),
}

impl TreeRoot {
    /// `Query.userRead` is an operation, `User` is a type (a search hit path).
    pub fn parse(schema: &Schema, hit_path: &str) -> Option<Self> {
        if hit_path.contains('.') {
            return OperationRef::parse(schema, hit_path).map(Self::Operation);
        }
        schema.resolve_type(hit_path)?;
        Some(Self::Type(hit_path.to_string()))
    }

    /// Stable key for storing per-root state.
    pub fn key(&self) -> String {
        match self {
            Self::Operation(op) => op.label(),
            Self::Type(name) => name.clone(),
        }
    }

    pub fn operation(&self) -> Option<&OperationRef> {
        match self {
            Self::Operation(op) => Some(op),
            Self::Type(_) => None,
        }
    }

    fn start_type<'a>(&self, schema: &'a Schema) -> Option<&'a FullType> {
        match self {
            Self::Operation(op) => schema.type_by_name(op.field(schema)?.ty.named()?),
            Self::Type(name) => schema.resolve_type(name),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    /// A scalar or enum: can be selected.
    Leaf,
    /// Has children and can be expanded.
    Composite,
    /// Its type already appeared on the path (see `MAX_REPEATS`): cannot be expanded.
    Recursive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub path: Path,
    pub label: String,
    pub kind: NodeKind,
    pub description: String,
}

/// One visible line of the flattened tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub node: Node,
    pub depth: usize,
    pub expanded: bool,
}

pub fn root_node(schema: &Schema, root: &TreeRoot) -> Option<Node> {
    match root {
        TreeRoot::Operation(op) => {
            let field = op.field(schema)?;
            let kind = match root.start_type(schema).map(|t| t.kind) {
                Some(TypeKind::Object | TypeKind::Interface | TypeKind::Union) => {
                    NodeKind::Composite
                }
                _ => NodeKind::Leaf,
            };
            Some(Node {
                path: Vec::new(),
                label: field.signature(),
                kind,
                description: field.description.clone().unwrap_or_default(),
            })
        }
        TreeRoot::Type(_) => {
            let ty = root.start_type(schema)?;
            let kind = if ty.kind == TypeKind::Scalar {
                NodeKind::Leaf
            } else {
                NodeKind::Composite
            };
            Some(Node {
                path: Vec::new(),
                label: format!(
                    "{} ({})",
                    ty.name.as_deref().unwrap_or_default(),
                    ty.kind.as_str()
                ),
                kind,
                description: ty.description.clone().unwrap_or_default(),
            })
        }
    }
}

fn field_type_name<'a>(ty: &'a FullType, name: &str) -> Option<&'a str> {
    let from_fields = ty
        .fields
        .iter()
        .flatten()
        .find(|f| f.name == name)
        .and_then(|f| f.ty.named());
    from_fields.or_else(|| {
        ty.input_fields
            .iter()
            .flatten()
            .find(|f| f.name == name)
            .and_then(|f| f.ty.named())
    })
}

/// The type at `path` and the names of the types from the root down to it (inclusive).
fn resolve<'a>(
    schema: &'a Schema,
    root: &TreeRoot,
    path: &[Seg],
) -> Option<(&'a FullType, Vec<&'a str>)> {
    let mut ty = root.start_type(schema)?;
    let mut names = vec![ty.name.as_deref()?];
    for seg in path {
        ty = match seg {
            Seg::Field(name) => schema.type_by_name(field_type_name(ty, name)?)?,
            Seg::Branch(name) => schema.type_by_name(name)?,
        };
        names.push(ty.name.as_deref()?);
    }
    Some((ty, names))
}

fn node_kind(schema: &Schema, type_name: Option<&str>, names: &[&str]) -> NodeKind {
    let Some(name) = type_name else {
        return NodeKind::Leaf;
    };
    match schema.type_by_name(name).map(|t| t.kind) {
        Some(TypeKind::Object | TypeKind::Interface | TypeKind::Union | TypeKind::InputObject) => {
            if names.iter().filter(|n| **n == name).count() >= MAX_REPEATS {
                NodeKind::Recursive
            } else {
                NodeKind::Composite
            }
        }
        _ => NodeKind::Leaf,
    }
}

/// The children of the node at `path`: fields, input fields, enum values, then
/// union or interface branches, all in schema order.
pub fn children(schema: &Schema, root: &TreeRoot, path: &[Seg]) -> Vec<Node> {
    let Some((ty, names)) = resolve(schema, root, path) else {
        return Vec::new();
    };
    let mut nodes = Vec::new();
    for field in ty.fields.iter().flatten() {
        nodes.push(Node {
            path: join(path, Seg::Field(field.name.clone())),
            label: field.signature(),
            kind: node_kind(schema, field.ty.named(), &names),
            description: field.description.clone().unwrap_or_default(),
        });
    }
    for input in ty.input_fields.iter().flatten() {
        nodes.push(Node {
            path: join(path, Seg::Field(input.name.clone())),
            label: input.render(),
            kind: node_kind(schema, input.ty.named(), &names),
            description: input.description.clone().unwrap_or_default(),
        });
    }
    for value in ty.enum_values.iter().flatten() {
        nodes.push(Node {
            path: join(path, Seg::Field(value.name.clone())),
            label: value.name.clone(),
            kind: NodeKind::Leaf,
            description: value.description.clone().unwrap_or_default(),
        });
    }
    for possible in ty.possible_types.iter().flatten() {
        let Some(name) = possible.named() else {
            continue;
        };
        nodes.push(Node {
            path: join(path, Seg::Branch(name.to_string())),
            label: format!("on {name}"),
            kind: node_kind(schema, Some(name), &names),
            description: String::new(),
        });
    }
    nodes
}

/// The visible lines of the tree: the root, plus the children of every expanded node.
pub fn rows(schema: &Schema, root: &TreeRoot, expanded: &BTreeSet<Path>) -> Vec<Row> {
    let mut out = Vec::new();
    if let Some(node) = root_node(schema, root) {
        push_rows(&mut out, schema, root, expanded, node, 0);
    }
    out
}

fn push_rows(
    out: &mut Vec<Row>,
    schema: &Schema,
    root: &TreeRoot,
    expanded: &BTreeSet<Path>,
    node: Node,
    depth: usize,
) {
    let is_expanded = node.kind == NodeKind::Composite && expanded.contains(&node.path);
    let path = node.path.clone();
    out.push(Row {
        node,
        depth,
        expanded: is_expanded,
    });
    if is_expanded {
        for child in children(schema, root, &path) {
            push_rows(out, schema, root, expanded, child, depth + 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::design_schema;

    fn root(schema: &Schema, text: &str) -> TreeRoot {
        TreeRoot::parse(schema, text).unwrap()
    }

    fn labels(nodes: &[Node]) -> Vec<&str> {
        nodes.iter().map(|n| n.label.as_str()).collect()
    }

    fn kinds(nodes: &[Node]) -> Vec<NodeKind> {
        nodes.iter().map(|n| n.kind).collect()
    }

    fn field(name: &str) -> Seg {
        Seg::Field(name.to_string())
    }

    fn branch(name: &str) -> Seg {
        Seg::Branch(name.to_string())
    }

    #[test]
    fn operation_refs_parse_only_existing_fields() {
        let schema = design_schema();
        let op = OperationRef::parse(&schema, "Query.me").unwrap();
        assert_eq!(op.label(), "Query.me");
        assert_eq!(op.field(&schema).unwrap().name, "me");
        assert!(OperationRef::parse(&schema, "Query.nope").is_none());
        assert!(OperationRef::parse(&schema, "Subscription.me").is_none());
        assert!(OperationRef::parse(&schema, "Nope.me").is_none());
        assert!(OperationRef::parse(&schema, "me").is_none());
    }

    #[test]
    fn tree_roots_distinguish_operations_from_types() {
        let schema = design_schema();
        assert!(matches!(
            TreeRoot::parse(&schema, "Query.me"),
            Some(TreeRoot::Operation(_))
        ));
        assert_eq!(
            TreeRoot::parse(&schema, "User"),
            Some(TreeRoot::Type("User".to_string()))
        );
        assert!(TreeRoot::parse(&schema, "Nope").is_none());
        assert!(TreeRoot::parse(&schema, "Query.nope").is_none());
        assert_eq!(root(&schema, "Query.me").key(), "Query.me");
        assert_eq!(root(&schema, "User").key(), "User");
        assert!(root(&schema, "Query.me").operation().is_some());
        assert!(root(&schema, "User").operation().is_none());
    }

    #[test]
    fn root_nodes_describe_the_operation_or_the_type() {
        let schema = design_schema();
        let me = root_node(&schema, &root(&schema, "Query.me")).unwrap();
        assert_eq!(me.label, "me: User");
        assert_eq!(me.kind, NodeKind::Composite);
        assert_eq!(me.description, "The current user.");
        assert!(me.path.is_empty());

        let version = root_node(&schema, &root(&schema, "Query.version")).unwrap();
        assert_eq!(version.kind, NodeKind::Leaf);

        let user = root_node(&schema, &root(&schema, "User")).unwrap();
        assert_eq!(user.label, "User (OBJECT)");
        assert_eq!(user.kind, NodeKind::Composite);
        let id = root_node(&schema, &root(&schema, "ID")).unwrap();
        assert_eq!(id.kind, NodeKind::Leaf);
    }

    #[test]
    fn children_of_an_object_are_its_fields_in_schema_order() {
        let schema = design_schema();
        let kids = children(&schema, &root(&schema, "Query.me"), &[]);
        assert_eq!(
            labels(&kids),
            [
                "id: ID!",
                "name: String",
                "role: Role",
                "friends: [User!]!",
                "posts(first: Int!): [Post!]!",
                "post(id: ID!): Post"
            ]
        );
        assert_eq!(
            kinds(&kids),
            [
                NodeKind::Leaf,
                NodeKind::Leaf,
                NodeKind::Leaf,
                NodeKind::Composite,
                NodeKind::Composite,
                NodeKind::Composite
            ]
        );
        assert_eq!(kids[0].path, vec![field("id")]);
        assert_eq!(kids[2].description, "Access role.");
    }

    #[test]
    fn a_type_that_appeared_twice_on_the_path_is_recursive() {
        let schema = design_schema();
        let r = root(&schema, "Query.me");
        // me: User, so friends: User is the second User and is still expandable.
        let kids = children(&schema, &r, &[]);
        assert_eq!(kids[3].kind, NodeKind::Composite);
        // Inside friends, the next friends would be the third User.
        let inner = children(&schema, &r, &[field("friends")]);
        assert_eq!(inner[3].label, "friends: [User!]!");
        assert_eq!(inner[3].kind, NodeKind::Recursive);
        assert_eq!(inner[4].kind, NodeKind::Composite);
    }

    #[test]
    fn interfaces_list_shared_fields_then_branches() {
        let schema = design_schema();
        let r = root(&schema, "Query.node");
        let kids = children(&schema, &r, &[]);
        assert_eq!(labels(&kids), ["id: ID!", "on User", "on Post"]);
        assert_eq!(
            kinds(&kids),
            [NodeKind::Leaf, NodeKind::Composite, NodeKind::Composite]
        );
        assert_eq!(kids[1].path, vec![branch("User")]);

        let user = children(&schema, &r, &[branch("User")]);
        assert_eq!(user.len(), 6);
        assert_eq!(user[0].label, "id: ID!");
        assert_eq!(user[0].path, vec![branch("User"), field("id")]);
    }

    #[test]
    fn unions_list_only_branches() {
        let schema = design_schema();
        let kids = children(&schema, &root(&schema, "Query.search"), &[]);
        assert_eq!(labels(&kids), ["on User", "on Post"]);
    }

    #[test]
    fn input_objects_and_enums_expand_as_type_roots() {
        let schema = design_schema();
        let input = children(&schema, &root(&schema, "PostInput"), &[]);
        assert_eq!(labels(&input), ["title: String!"]);
        assert_eq!(input[0].kind, NodeKind::Leaf);

        let role = children(&schema, &root(&schema, "Role"), &[]);
        assert_eq!(labels(&role), ["ADMIN", "VIEWER"]);
        assert_eq!(role[0].description, "Full access.");
    }

    #[test]
    fn unknown_paths_have_no_children() {
        let schema = design_schema();
        let r = root(&schema, "Query.me");
        assert!(children(&schema, &r, &[field("nope")]).is_empty());
        assert!(children(&schema, &r, &[field("id")]).is_empty());
    }

    #[test]
    fn rows_follow_the_expanded_set() {
        let schema = design_schema();
        let r = root(&schema, "Query.me");

        let collapsed = rows(&schema, &r, &BTreeSet::new());
        assert_eq!(collapsed.len(), 1);
        assert!(!collapsed[0].expanded);

        let mut expanded = BTreeSet::new();
        expanded.insert(Vec::new());
        let one_level = rows(&schema, &r, &expanded);
        assert_eq!(one_level.len(), 7);
        assert!(one_level[0].expanded);
        assert_eq!(one_level[1].depth, 1);

        expanded.insert(vec![field("friends")]);
        let two_levels = rows(&schema, &r, &expanded);
        assert_eq!(two_levels.len(), 13);
        assert_eq!(two_levels[4].node.label, "friends: [User!]!");
        assert!(two_levels[4].expanded);
        assert_eq!(two_levels[5].depth, 2);
        assert_eq!(two_levels[8].node.kind, NodeKind::Recursive);
        assert!(!two_levels[8].expanded);
    }

    #[test]
    fn leaves_are_never_expanded_even_when_listed() {
        let schema = design_schema();
        let r = root(&schema, "Query.me");
        let mut expanded = BTreeSet::new();
        expanded.insert(Vec::new());
        expanded.insert(vec![field("id")]);
        let rows = rows(&schema, &r, &expanded);
        assert_eq!(rows.len(), 7);
        assert!(!rows[1].expanded);
    }
}
