//! A lazy tree over an operation's return type (or over a type), plus the set of
//! selected leaf fields. Pure data: no terminal or network code.

use std::collections::BTreeSet;

use crate::querygen::is_composite;
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
    /// The `args` group of a field.
    Args,
    /// One argument inside an `args` group.
    Arg(String),
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
    /// A required argument: always part of the query, cannot be toggled.
    Required,
    /// An optional argument: part of the query only when selected.
    Optional,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub path: Path,
    pub label: String,
    pub kind: NodeKind,
    pub description: String,
    /// The field behind this node takes arguments, so it has an `args` group.
    pub has_args: bool,
}

impl Node {
    /// Composite nodes expand, and so do leaves that have arguments.
    pub fn expandable(&self) -> bool {
        match self.kind {
            NodeKind::Composite => true,
            NodeKind::Leaf => self.has_args,
            _ => false,
        }
    }
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
                has_args: !field.args.is_empty(),
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
                has_args: false,
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
            Seg::Args | Seg::Arg(_) => return None,
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

/// The field the node at `path` stands for: the operation at the root, or a field of
/// the type above it. Branches, arguments and type roots stand for no field.
fn field_at<'a>(schema: &'a Schema, root: &TreeRoot, path: &[Seg]) -> Option<&'a Field> {
    match path.split_last() {
        None => root.operation()?.field(schema),
        Some((Seg::Field(name), parent)) => {
            let (ty, _) = resolve(schema, root, parent)?;
            ty.fields.iter().flatten().find(|f| f.name == *name)
        }
        Some(_) => None,
    }
}

/// The arguments of the field that owns the `args` group at `group`.
fn arg_nodes(schema: &Schema, root: &TreeRoot, group: &[Seg]) -> Vec<Node> {
    let owner = &group[..group.len() - 1];
    let Some(field) = field_at(schema, root, owner) else {
        return Vec::new();
    };
    field
        .args
        .iter()
        .map(|arg| Node {
            path: join(group, Seg::Arg(arg.name.clone())),
            label: arg.render(),
            kind: if arg.is_required() {
                NodeKind::Required
            } else {
                NodeKind::Optional
            },
            description: arg.description.clone().unwrap_or_default(),
            has_args: false,
        })
        .collect()
}

/// The children of the node at `path`: fields, input fields, enum values, then
/// union or interface branches, all in schema order.
pub fn children(schema: &Schema, root: &TreeRoot, path: &[Seg]) -> Vec<Node> {
    if path.last() == Some(&Seg::Args) {
        return arg_nodes(schema, root, path);
    }
    let Some((ty, names)) = resolve(schema, root, path) else {
        return Vec::new();
    };
    let mut nodes = Vec::new();
    if field_at(schema, root, path).is_some_and(|field| !field.args.is_empty()) {
        nodes.push(Node {
            path: join(path, Seg::Args),
            label: "args".to_string(),
            kind: NodeKind::Composite,
            description: "Arguments of this field".to_string(),
            has_args: false,
        });
    }
    for field in ty.fields.iter().flatten() {
        nodes.push(Node {
            path: join(path, Seg::Field(field.name.clone())),
            label: field.listing_label(),
            kind: node_kind(schema, field.ty.named(), &names),
            description: field.description.clone().unwrap_or_default(),
            has_args: !field.args.is_empty(),
        });
    }
    for input in ty.input_fields.iter().flatten() {
        nodes.push(Node {
            path: join(path, Seg::Field(input.name.clone())),
            label: input.render(),
            kind: node_kind(schema, input.ty.named(), &names),
            description: input.description.clone().unwrap_or_default(),
            has_args: false,
        });
    }
    for value in ty.enum_values.iter().flatten() {
        nodes.push(Node {
            path: join(path, Seg::Field(value.name.clone())),
            label: value.name.clone(),
            kind: NodeKind::Leaf,
            description: value.description.clone().unwrap_or_default(),
            has_args: false,
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
            has_args: false,
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
    let is_expanded = node.expandable() && expanded.contains(&node.path);
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

/// The set of selected leaf fields and opted-in optional arguments. Objects are never
/// stored: they are implied by the leaves below them.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Selection {
    leaves: BTreeSet<Path>,
    args: BTreeSet<Path>,
}

impl Selection {
    /// True when neither a leaf nor an optional argument is selected.
    pub fn is_empty(&self) -> bool {
        self.leaves.is_empty() && self.args.is_empty()
    }

    pub fn contains(&self, path: &[Seg]) -> bool {
        self.leaves.contains(path)
    }

    /// True when an optional argument at `path` was opted in.
    pub fn arg_included(&self, path: &[Seg]) -> bool {
        self.args.contains(path)
    }

    /// True when a selected leaf equals `prefix` or lies below it. Arguments do not
    /// count: a field with only an opted-in argument still has nothing to select.
    pub fn has_under(&self, prefix: &[Seg]) -> bool {
        self.leaves
            .range(prefix.to_vec()..)
            .next()
            .is_some_and(|path| path.starts_with(prefix))
    }

    pub fn insert(&mut self, path: Path) {
        self.leaves.insert(path);
    }

    pub fn insert_arg(&mut self, path: Path) {
        self.args.insert(path);
    }

    /// Toggles a leaf or an optional argument. Returns whether the selection changed;
    /// objects, groups, required arguments and recursive nodes are ignored.
    pub fn toggle_node(&mut self, node: &Node) -> bool {
        let set = match node.kind {
            NodeKind::Leaf => &mut self.leaves,
            NodeKind::Optional => &mut self.args,
            _ => return false,
        };
        if !set.remove(&node.path) {
            set.insert(node.path.clone());
        }
        true
    }

    pub fn leaves(&self) -> impl Iterator<Item = &Path> {
        self.leaves.iter()
    }

    /// True when every leaf and opted-in argument of `other` is selected here too.
    pub fn has_all_of(&self, other: &Selection) -> bool {
        other.leaves.is_subset(&self.leaves) && other.args.is_subset(&self.args)
    }

    /// Selects everything `other` selects, keeping what was selected.
    pub fn extend_with(&mut self, other: &Selection) {
        self.leaves.extend(other.leaves.iter().cloned());
        self.args.extend(other.args.iter().cloned());
    }

    /// Unselects everything `other` selects, keeping the rest.
    pub fn remove_all_of(&mut self, other: &Selection) {
        self.leaves.retain(|path| !other.leaves.contains(path));
        self.args.retain(|path| !other.args.contains(path));
    }

    pub fn leaf_count(&self) -> usize {
        self.leaves.len()
    }

    /// The optional arguments that were opted in.
    pub fn optional_args(&self) -> impl Iterator<Item = &Path> {
        self.args.iter()
    }
}

/// Most leaves `select_all` selects; densely linked schemas have more paths than anyone
/// could send in one query.
pub const SELECT_ALL_LIMIT: usize = 1_000;

/// How many levels of fields `select_all` goes down: the operation's own fields are the
/// first level.
pub const SELECT_ALL_DEPTH: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllLeaves {
    pub selection: Selection,
    /// The limit was reached: shallower fields come first, deeper ones were left out.
    pub capped: bool,
}

/// Every leaf of an operation, `SELECT_ALL_DEPTH` levels down, that can be reached without
/// entering a type already on the path from the root (a field of `User` that leads back to
/// `User` is skipped, siblings of the same type are not). Fields that take a required
/// argument need a variable: they are skipped, with everything below them, unless
/// `with_variables`, which also opts in every optional argument of the fields it selects.
/// The operation's own required arguments stay either way. Breadth first, so the limit
/// cuts the deepest fields. Type trees and scalar operations have no fields to select.
pub fn select_all(schema: &Schema, root: &TreeRoot, with_variables: bool) -> AllLeaves {
    select_all_up_to(
        schema,
        root,
        with_variables,
        SELECT_ALL_LIMIT,
        SELECT_ALL_DEPTH,
    )
}

/// Where `select_all` stands on a type: the path to it, the names of the types from the
/// root down to it and how many fields deep it is (branches are not a level).
struct Frame<'a> {
    path: Path,
    ty: &'a FullType,
    names: Vec<&'a str>,
    depth: usize,
}

/// Queues `inner` below `frame` unless its type is already on the way down.
fn queue<'a>(next: &mut Vec<Frame<'a>>, frame: &Frame<'a>, seg: Seg, inner: &'a FullType) {
    let Some(name) = inner.name.as_deref() else {
        return;
    };
    if frame.names.contains(&name) {
        return;
    }
    let level = usize::from(matches!(seg, Seg::Field(_)));
    next.push(Frame {
        path: join(&frame.path, seg),
        ty: inner,
        names: [&frame.names[..], &[name]].concat(),
        depth: frame.depth + level,
    });
}

fn select_all_up_to(
    schema: &Schema,
    root: &TreeRoot,
    with_variables: bool,
    limit: usize,
    depth: usize,
) -> AllLeaves {
    let mut out = AllLeaves {
        selection: Selection::default(),
        capped: false,
    };
    let (TreeRoot::Operation(_), Some(start)) = (root, root.start_type(schema)) else {
        return out;
    };
    let Some(start_name) = start.name.as_deref() else {
        return out;
    };
    // Optional arguments to opt in, with the field they belong to.
    let mut optional: Vec<(Path, Path)> = Vec::new();
    if with_variables {
        if let Some(field) = root.operation().and_then(|op| op.field(schema)) {
            optional_args(&mut optional, &Path::new(), field);
        }
    }
    let mut level = vec![Frame {
        path: Path::new(),
        ty: start,
        names: vec![start_name],
        depth: 0,
    }];
    'levels: while !level.is_empty() {
        let mut next = Vec::new();
        for frame in &level {
            for field in frame.ty.fields.iter().flatten() {
                if !with_variables && field.args.iter().any(|arg| arg.is_required()) {
                    continue;
                }
                let target = field.ty.named().and_then(|name| schema.type_by_name(name));
                match target {
                    Some(inner) if is_composite(inner.kind) => {
                        if frame.depth + 1 < depth {
                            queue(&mut next, frame, Seg::Field(field.name.clone()), inner);
                        }
                    }
                    _ => {
                        if out.selection.leaf_count() >= limit {
                            out.capped = true;
                            break 'levels;
                        }
                        out.selection
                            .insert(join(&frame.path, Seg::Field(field.name.clone())));
                    }
                }
                if with_variables {
                    let path = join(&frame.path, Seg::Field(field.name.clone()));
                    optional_args(&mut optional, &path, field);
                }
            }
            for possible in frame.ty.possible_types.iter().flatten() {
                let inner = possible.named().and_then(|name| schema.type_by_name(name));
                if let (Some(name), Some(inner)) = (possible.named(), inner) {
                    queue(&mut next, frame, Seg::Branch(name.to_string()), inner);
                }
            }
        }
        level = next;
    }
    // The arguments of a field that ended up with nothing selected go with it.
    let composite_root = is_composite(start.kind);
    for (field, arg) in optional {
        if out.selection.has_under(&field) || (field.is_empty() && !composite_root) {
            out.selection.insert_arg(arg);
        }
    }
    out
}

/// Notes the optional arguments of `field`, which sits at `path`.
fn optional_args(found: &mut Vec<(Path, Path)>, path: &Path, field: &Field) {
    for arg in field.args.iter().filter(|arg| !arg.is_required()) {
        found.push((
            path.clone(),
            join(&join(path, Seg::Args), Seg::Arg(arg.name.clone())),
        ));
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SelectError {
    #[error("no field selected: pass at least one path, for example `id,name`")]
    Empty,
    #[error("`{0}`: no such field or branch")]
    Unknown(String),
    #[error("`{0}` has subfields: select one of them, for example `{0}.id`")]
    NotALeaf(String),
    #[error("`{0}` is a required argument: it is always included")]
    RequiredArgument(String),
}

/// Parses `id,friends.name,on:User.name` into a selection. Every path must end at a
/// leaf. Unlike the tree view, this allows any depth, including through recursive types.
pub fn parse_select(
    schema: &Schema,
    operation: &OperationRef,
    text: &str,
) -> Result<Selection, SelectError> {
    let root = TreeRoot::Operation(operation.clone());
    let mut selection = Selection::default();
    for item in text.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let mut path: Path = Vec::new();
        let mut last: Option<Node> = None;
        for segment in item.split('.') {
            let (parent, wanted) = match segment.strip_prefix("arg:") {
                Some(name) => {
                    let group = join(&path, Seg::Args);
                    let wanted = join(&group, Seg::Arg(name.to_string()));
                    (group, wanted)
                }
                None => {
                    let seg = match segment.strip_prefix("on:") {
                        Some(type_name) => Seg::Branch(type_name.to_string()),
                        None => Seg::Field(segment.to_string()),
                    };
                    (path.clone(), join(&path, seg))
                }
            };
            let found = children(schema, &root, &parent)
                .into_iter()
                .find(|node| node.path == wanted)
                .ok_or_else(|| SelectError::Unknown(item.to_string()))?;
            path = wanted;
            last = Some(found);
        }
        let node = last.ok_or_else(|| SelectError::Unknown(item.to_string()))?;
        match node.kind {
            NodeKind::Leaf => selection.insert(path),
            NodeKind::Optional => selection.insert_arg(path),
            NodeKind::Required => return Err(SelectError::RequiredArgument(item.to_string())),
            NodeKind::Composite | NodeKind::Recursive => {
                return Err(SelectError::NotALeaf(item.to_string()))
            }
        }
    }
    if selection.is_empty() {
        Err(SelectError::Empty)
    } else {
        Ok(selection)
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
    fn a_deprecated_field_is_marked_in_the_tree() {
        let mut schema = design_schema();
        let user = schema
            .types
            .iter_mut()
            .find(|ty| ty.name.as_deref() == Some("User"))
            .unwrap();
        for field in user.fields.iter_mut().flatten() {
            field.is_deprecated = field.name == "name";
        }
        let kids = children(&schema, &root(&schema, "Query.me"), &[]);
        assert_eq!(kids[0].label, "id: ID!");
        assert_eq!(kids[1].label, "name: String [deprecated]");
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
        assert_eq!(labels(&kids), ["args", "id: ID!", "on User", "on Post"]);
        assert_eq!(
            kinds(&kids),
            [
                NodeKind::Composite,
                NodeKind::Leaf,
                NodeKind::Composite,
                NodeKind::Composite
            ]
        );
        assert_eq!(kids[2].path, vec![branch("User")]);

        let user = children(&schema, &r, &[branch("User")]);
        assert_eq!(user.len(), 6);
        assert_eq!(user[0].label, "id: ID!");
        assert_eq!(user[0].path, vec![branch("User"), field("id")]);
    }

    #[test]
    fn unions_list_only_branches() {
        let schema = design_schema();
        let kids = children(&schema, &root(&schema, "Query.search"), &[]);
        assert_eq!(labels(&kids), ["args", "on User", "on Post"]);
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

    #[test]
    fn toggling_selects_and_unselects_leaves_only() {
        let schema = design_schema();
        let r = root(&schema, "Query.me");
        let kids = children(&schema, &r, &[]);
        let mut selection = Selection::default();
        assert!(selection.is_empty());

        assert!(selection.toggle_node(&kids[0]));
        assert!(selection.contains(&kids[0].path));
        assert!(!selection.is_empty());

        // `friends` is an object: ignored.
        assert!(!selection.toggle_node(&kids[3]));
        assert!(!selection.contains(&kids[3].path));

        assert!(selection.toggle_node(&kids[0]));
        assert!(selection.is_empty());
    }

    #[test]
    fn has_under_finds_selected_leaves_at_or_below_a_prefix() {
        let mut selection = Selection::default();
        selection.insert(vec![field("friends"), field("name")]);
        selection.insert(vec![field("id")]);
        assert!(selection.has_under(&[]));
        assert!(selection.has_under(&[field("friends")]));
        assert!(selection.has_under(&[field("friends"), field("name")]));
        assert!(selection.has_under(&[field("id")]));
        assert!(!selection.has_under(&[field("friends"), field("id")]));
        assert!(!selection.has_under(&[field("posts")]));
        assert!(!Selection::default().has_under(&[]));
        assert_eq!(selection.leaves().count(), 2);
    }

    #[test]
    fn parse_select_accepts_plain_nested_and_branch_paths() {
        let schema = design_schema();
        let me = OperationRef::parse(&schema, "Query.me").unwrap();
        let selection = parse_select(&schema, &me, "id, friends.name").unwrap();
        assert!(selection.contains(&[field("id")]));
        assert!(selection.contains(&[field("friends"), field("name")]));
        assert_eq!(selection.leaves().count(), 2);

        let search = OperationRef::parse(&schema, "Query.search").unwrap();
        let selection = parse_select(&schema, &search, "on:User.name,on:Post.title").unwrap();
        assert!(selection.contains(&[branch("User"), field("name")]));
        assert!(selection.contains(&[branch("Post"), field("title")]));
    }

    #[test]
    fn parse_select_allows_any_depth_through_recursive_types() {
        let schema = design_schema();
        let me = OperationRef::parse(&schema, "Query.me").unwrap();
        let selection = parse_select(&schema, &me, "friends.friends.friends.name").unwrap();
        assert_eq!(selection.leaves().count(), 1);
    }

    #[test]
    fn parse_select_rejects_bad_input() {
        let schema = design_schema();
        let me = OperationRef::parse(&schema, "Query.me").unwrap();
        assert_eq!(parse_select(&schema, &me, ""), Err(SelectError::Empty));
        assert_eq!(parse_select(&schema, &me, " , "), Err(SelectError::Empty));
        assert_eq!(
            parse_select(&schema, &me, "nope"),
            Err(SelectError::Unknown("nope".to_string()))
        );
        assert_eq!(
            parse_select(&schema, &me, "id.deeper"),
            Err(SelectError::Unknown("id.deeper".to_string()))
        );
        assert_eq!(
            parse_select(&schema, &me, "friends"),
            Err(SelectError::NotALeaf("friends".to_string()))
        );
        let search = OperationRef::parse(&schema, "Query.search").unwrap();
        assert_eq!(
            parse_select(&schema, &search, "on:Nope.name"),
            Err(SelectError::Unknown("on:Nope.name".to_string()))
        );
    }

    fn arg(name: &str) -> Seg {
        Seg::Arg(name.to_string())
    }

    #[test]
    fn a_field_with_arguments_lists_an_args_group_first() {
        let schema = design_schema();
        let r = root(&schema, "Query.node");
        let kids = children(&schema, &r, &[]);
        assert_eq!(labels(&kids), ["args", "id: ID!", "on User", "on Post"]);
        assert_eq!(kids[0].kind, NodeKind::Composite);
        assert_eq!(kids[0].path, vec![Seg::Args]);

        let args = children(&schema, &r, &[Seg::Args]);
        assert_eq!(labels(&args), ["id: ID!"]);
        assert_eq!(kinds(&args), [NodeKind::Required]);
        assert_eq!(args[0].path, vec![Seg::Args, arg("id")]);
    }

    #[test]
    fn optional_and_defaulted_arguments_are_optional_nodes() {
        let schema = design_schema();
        let args = children(&schema, &root(&schema, "Query.users"), &[Seg::Args]);
        assert_eq!(
            labels(&args),
            [
                "role: Role",
                "active: Boolean",
                "floor: Float",
                "ids: [ID!]",
                "filter: PostInput",
                "limit: Int! = 10"
            ]
        );
        assert!(kinds(&args).iter().all(|k| *k == NodeKind::Optional));
    }

    #[test]
    fn nested_fields_with_arguments_get_their_own_group() {
        let schema = design_schema();
        let r = root(&schema, "Query.me");
        let kids = children(&schema, &r, &[field("posts")]);
        assert_eq!(kids[0].label, "args");
        assert_eq!(kids[0].path, vec![field("posts"), Seg::Args]);
        let args = children(&schema, &r, &[field("posts"), Seg::Args]);
        assert_eq!(args[0].path, vec![field("posts"), Seg::Args, arg("first")]);
        assert_eq!(args[0].kind, NodeKind::Required);
        // A field without arguments has no group.
        assert_ne!(children(&schema, &r, &[])[0].label, "args");
        // Arguments are leaves: they have no children of their own.
        assert!(children(&schema, &r, &[field("posts"), Seg::Args, arg("first")]).is_empty());
    }

    #[test]
    fn scalar_operations_with_arguments_can_still_be_expanded() {
        let schema = design_schema();
        let r = root(&schema, "Query.postCount");
        let node = root_node(&schema, &r).unwrap();
        assert_eq!(node.kind, NodeKind::Leaf);
        assert!(node.expandable());
        let mut expanded = BTreeSet::new();
        expanded.insert(Vec::new());
        let shown = rows(&schema, &r, &expanded);
        assert_eq!(shown.len(), 2);
        assert_eq!(shown[1].node.label, "args");

        let version = root_node(&schema, &root(&schema, "Query.version")).unwrap();
        assert!(!version.expandable());
    }

    #[test]
    fn toggling_an_optional_argument_selects_it_apart_from_the_leaves() {
        let schema = design_schema();
        let r = root(&schema, "Query.search");
        let args = children(&schema, &r, &[Seg::Args]);
        let mut selection = Selection::default();
        assert!(!selection.toggle_node(&args[0])); // term: required
        assert!(selection.is_empty());
        assert!(selection.toggle_node(&args[1])); // first: optional
        assert!(selection.arg_included(&args[1].path));
        assert!(!selection.has_under(&[]));
        assert!(!selection.is_empty());
        assert!(selection.toggle_node(&args[1]));
        assert!(selection.is_empty());
    }

    #[test]
    fn parse_select_accepts_optional_arguments() {
        let schema = design_schema();
        let search = OperationRef::parse(&schema, "Query.search").unwrap();
        let selection = parse_select(&schema, &search, "on:User.name,arg:first").unwrap();
        assert!(selection.arg_included(&[Seg::Args, arg("first")]));
        assert!(selection.contains(&[branch("User"), field("name")]));

        let users = OperationRef::parse(&schema, "Query.users").unwrap();
        let selection = parse_select(&schema, &users, "id,arg:role,arg:limit").unwrap();
        assert!(selection.arg_included(&[Seg::Args, arg("role")]));
        assert!(selection.arg_included(&[Seg::Args, arg("limit")]));
        assert_eq!(selection.leaves().count(), 1);

        // An argument alone is a valid selection for a scalar operation.
        let count = OperationRef::parse(&schema, "Query.postCount").unwrap();
        assert!(parse_select(&schema, &count, "arg:since").is_ok());
    }

    #[test]
    fn parse_select_rejects_required_and_unknown_arguments() {
        let schema = design_schema();
        let search = OperationRef::parse(&schema, "Query.search").unwrap();
        assert_eq!(
            parse_select(&schema, &search, "arg:term"),
            Err(SelectError::RequiredArgument("arg:term".to_string()))
        );
        assert_eq!(
            parse_select(&schema, &search, "arg:nope"),
            Err(SelectError::Unknown("arg:nope".to_string()))
        );
        assert_eq!(
            parse_select(&schema, &search, "arg:first.x"),
            Err(SelectError::Unknown("arg:first.x".to_string()))
        );
    }

    fn all(operation: &str) -> Vec<String> {
        paths(operation, true)
    }

    fn paths(operation: &str, with_variables: bool) -> Vec<String> {
        let schema = design_schema();
        let root = root(&schema, operation);
        let mut found: Vec<String> = select_all(&schema, &root, with_variables)
            .selection
            .leaves()
            .map(|path| {
                path.iter()
                    .map(|seg| match seg {
                        Seg::Field(name) => name.clone(),
                        Seg::Branch(name) => format!("on:{name}"),
                        other => format!("{other:?}"),
                    })
                    .collect::<Vec<_>>()
                    .join(".")
            })
            .collect();
        found.sort();
        found
    }

    #[test]
    fn select_all_skips_a_type_that_is_already_on_the_path() {
        // `friends` and `author` lead back to User, which is already above them.
        assert_eq!(
            all("Query.me"),
            [
                "id",
                "name",
                "post.id",
                "post.title",
                "posts.id",
                "posts.title",
                "role"
            ]
        );
    }

    #[test]
    fn select_all_keeps_siblings_of_the_same_type() {
        let found = all("Query.me");
        assert!(found.contains(&"post.title".to_string()));
        assert!(found.contains(&"posts.title".to_string()));
    }

    #[test]
    fn select_all_goes_through_every_branch_of_a_union() {
        let found = all("Query.search");
        assert!(found.contains(&"on:User.name".to_string()));
        assert!(found.contains(&"on:User.posts.title".to_string()));
        assert!(found.contains(&"on:Post.title".to_string()));
        assert!(found.contains(&"on:Post.author.name".to_string()));
        // Post is on the path already, so User's posts do not appear below its author.
        assert!(!found.iter().any(|p| p.starts_with("on:Post.author.post")));
    }

    #[test]
    fn select_all_has_nothing_for_scalar_operations_and_type_trees() {
        assert!(all("Query.version").is_empty());
        let schema = design_schema();
        let found = select_all(&schema, &TreeRoot::Type("User".to_string()), true);
        assert!(found.selection.is_empty());
        assert!(!found.capped);
    }

    #[test]
    fn select_all_generates_a_valid_query_without_optional_arguments() {
        let schema = design_schema();
        let root = root(&schema, "Query.me");
        let all = select_all(&schema, &root, true);
        let op = root.operation().unwrap();
        let query = crate::querygen::generate(&schema, op, &all.selection).unwrap();
        // Only the required arguments: `posts(first)` and `post(id)`.
        assert_eq!(query.variables.len(), 2);
        assert!(query.text.contains("posts(first: $first)"));
        assert!(query.text.contains("post(id: $id)"));
    }

    #[test]
    fn a_limit_keeps_the_shallowest_fields_and_says_it_was_reached() {
        let schema = design_schema();
        let root = root(&schema, "Query.me");
        let found = select_all_up_to(&schema, &root, true, 3, 3);
        assert!(found.capped);
        assert_eq!(found.selection.leaf_count(), 3);
        assert!(found.selection.leaves().all(|path| path.len() == 1));
        assert!(!select_all_up_to(&schema, &root, true, 7, 3).capped);
    }

    #[test]
    fn select_all_without_variables_skips_fields_with_required_arguments_and_what_is_below() {
        // `posts(first: Int!)` and `post(id: ID!)` need variables.
        assert_eq!(paths("Query.me", false), ["id", "name", "role"]);
    }

    #[test]
    fn the_operations_own_arguments_stay_without_variables_too() {
        let found = paths("Query.node", false);
        assert!(found.contains(&"id".to_string()));
        assert!(found.contains(&"on:User.name".to_string()));
        assert!(!found.contains(&"on:User.posts.title".to_string()));
    }

    #[test]
    fn select_all_stops_at_the_depth_and_branches_are_not_a_level() {
        let schema = design_schema();
        let me = root(&schema, "Query.me");
        let names = |found: AllLeaves| found.selection.leaf_count();
        assert_eq!(names(select_all_up_to(&schema, &me, true, 1_000, 1)), 3);
        assert_eq!(names(select_all_up_to(&schema, &me, true, 1_000, 2)), 7);

        // search { ... on User { name posts { id } } }: `posts.id` is two levels, however
        // many branches lie on the way.
        let search = root(&schema, "Query.search");
        let one = select_all_up_to(&schema, &search, true, 1_000, 1);
        assert!(one.selection.leaves().all(|path| path.len() == 2));
        let two = select_all_up_to(&schema, &search, true, 1_000, 2);
        assert!(two.selection.leaves().any(|path| path.len() == 3));
    }

    fn arg_names(operation: &str, with_variables: bool) -> Vec<String> {
        let schema = design_schema();
        let root = root(&schema, operation);
        select_all(&schema, &root, with_variables)
            .selection
            .optional_args()
            .map(|path| match path.last() {
                Some(Seg::Arg(name)) => name.clone(),
                other => panic!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn only_select_all_with_variables_opts_in_the_optional_arguments() {
        assert!(arg_names("Query.users", false).is_empty());
        let mut with = arg_names("Query.users", true);
        with.sort();
        assert_eq!(with, ["active", "filter", "floor", "ids", "limit", "role"]);
    }

    #[test]
    fn optional_arguments_of_a_scalar_operation_are_opted_in_without_any_field() {
        assert_eq!(arg_names("Query.postCount", true), ["since"]);
        assert!(arg_names("Query.postCount", false).is_empty());
    }

    #[test]
    fn required_arguments_are_never_opted_in_and_they_make_variables_anyway() {
        let schema = design_schema();
        let root = root(&schema, "Query.search");
        let found = select_all(&schema, &root, true);
        assert_eq!(arg_names("Query.search", true), ["first"]);
        let query = crate::querygen::generate(&schema, root.operation().unwrap(), &found.selection)
            .unwrap();
        assert!(
            query.text.contains("search(term: $term, first: $first)"),
            "{}",
            query.text
        );
    }

    #[test]
    fn a_selection_that_has_everything_tells_apart_fields_from_arguments() {
        let schema = design_schema();
        let root = root(&schema, "Query.users");
        let safe = select_all(&schema, &root, false).selection;
        let full = select_all(&schema, &root, true).selection;
        assert!(full.has_all_of(&safe));
        assert!(!safe.has_all_of(&full));
        let mut mixed = safe.clone();
        mixed.extend_with(&full);
        assert!(mixed.has_all_of(&full));
        mixed.remove_all_of(&full);
        assert!(mixed.is_empty());
    }
}
