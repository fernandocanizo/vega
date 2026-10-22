# Vega Phase 2, Step 1: Design Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Browse the schema as an expandable tree, select leaf fields with SPACE, see the generated query and its variables live, and print the same query from the command line with `--query` and `--select`. Variables are listed but not yet editable, and nothing is sent to the server (those are steps 2 and 3).

**Architecture:** `vega-core` gains three pure modules: `selection` (lazy tree over an operation's return type or over a type, plus the set of selected leaves), `querygen` (selection to query text and variable definitions) and test helpers. The TUI replaces its detail-view history with a `design` state (Browse and Design modes) that renders the tree, a Description pane, and, for operations, Query and Variables panes. Spec: `docs/superpowers/specs/2026-10-22-vega-phase-2-design.md`.

**Tech Stack:** Rust (stable, edition 2021), `serde`, `thiserror`, `clap`, `anyhow`, `ratatui` 0.30. No new dependencies.

## Global Constraints

- The screen shows at most two main panes side by side, with shorter panes below them.
- Details pane title is always `Details` (never `Preview`). Results show only the path (`Query.userRead`).
- Design layout for an operation: left column Details (tree) over Description; right column Query (read-only) over Variables. For a type (not an operation): the tree and Description only, read-only, no Query or Variables panes and no selection.
- SPACE toggles leaf fields only; on an object it does nothing. Objects are never selected directly. The query is derived from the set of selected leaf paths and contains only the objects that have a selected leaf below them.
- Keys in Design: Up/Down move; Right expands (or steps into an expanded node); Left collapses (or goes to the parent when collapsed); SPACE toggles a leaf; Esc returns to Browse keeping the search text; `q` quits; `?`/F1 (and `h` in Design only) open help; Ctrl-R refreshes; Ctrl-C quits. Backspace and the followed-type history no longer exist in the interactive browser.
- Browse keys: Up/Down move; typing filters; Enter or Right opens the highlighted hit in Design mode; Esc quits; Left does nothing.
- Selection, expansion and cursor are kept per operation or type for the whole app session (stored when leaving Design with Esc, restored on re-entry). A schema refresh clears them. Nothing is persisted to disk.
- Recursion: children are built lazily; a node whose type has already appeared twice on the path from the root is shown as recursive (`↻`) and cannot be expanded.
- Query generation: `query UserRead($id: ID!) { userRead(id: $id) { id name } }` shape, two-space indentation, fields in schema order (not selection order), `... on Type` fragments for union and interface branches with `__typename` added first inside every union or interface selection set. Only required arguments (non-null with no default value) are emitted, as variables named after the argument; a later argument whose name is already used is named `<field><Arg>` (for example `$postId`), then `<name>2`, `<name>3`. No aliases, directives or fragment definitions. Subscriptions generate a query like any other operation.
- An operation whose return type is a scalar or enum needs no selection and always generates a query.
- CLI: `vega --query <Query|Mutation|Subscription>.<field> [--select <paths>] [--json]` prints the generated query and its variables and sends nothing. `--select` takes comma-separated paths relative to the operation's result (`id,friends.name`); a union or interface branch is written as the segment `on:Type`, for example `--query Query.search --select on:User.name,on:Post.title`. `--query` conflicts with `--search` and `--describe`; `--select` requires `--query`. The CLI path parser allows any depth, including through recursive types.
- Warnings and errors go to stderr; stdout stays machine-readable. The stale-cache warning also applies to `--query`.
- Existing environment variables, XDG paths and the API key rule are unchanged: the API key is never written to disk, the cache or logs.
- Commits follow AGENTS.md: atomic, `tag: subject` in present tense, subject and body lines at most 72 characters, tags `feat`, `fix`, `ref`, `doc`, `test`, `chore`, no references to plans, specs, tasks or steps, no AI attribution. Every commit passes `just check`.

## File Structure

```
crates/vega-core/
  src/lib.rs                           + pub mod selection; pub mod querygen;
  src/selection.rs                     NEW: Seg, Path, OperationRef, TreeRoot, Node, Row, tree functions, Selection, parse_select
  src/querygen.rs                      NEW: VariableDef, GeneratedQuery, GenerateError, generate
  src/testutil.rs                      + DESIGN_FIXTURE, design_schema()
  tests/fixtures/design.json           NEW: schema with union, interface, recursion, nested arguments
crates/vega/
  src/cli.rs                           + --query, --select
  src/main.rs                          + --query handling
  src/output.rs                        + query_text
  src/tui/app.rs                       REWRITTEN navigation: Browse/Design, tree cursor, selection, sessions
  src/tui/ui.rs                        REWRITTEN layout: tree, Description, Query, Variables
  tests/cli.rs                         + --query tests
docs, README.md, AGENTS.md             updated in the last task
```

Library docs: use the context7 MCP tools if a ratatui or clap API below does not compile as written; fix the call, keep the behavior.

---

### Task 1: Lazy schema tree model

**Files:**
- Create: `crates/vega-core/tests/fixtures/design.json`, `crates/vega-core/src/selection.rs`
- Modify: `crates/vega-core/src/lib.rs`, `crates/vega-core/src/testutil.rs`

**Interfaces:**
- Consumes: `Schema`, `FullType`, `Field`, `Root`, `TypeKind`, `Schema::{type_by_name, resolve_type, root_type}`, `Field::signature`, `InputValue::render`, `TypeRef::named` (phase 1).
- Produces (`vega_core::selection`):
  - `enum Seg { Field(String), Branch(String) }` (`Ord`, `Hash`), `type Path = Vec<Seg>`
  - `struct OperationRef { root: Root, field: String }` with `parse(&Schema, &str) -> Option<Self>`, `label(&self) -> String`, `field<'a>(&self, &'a Schema) -> Option<&'a Field>`
  - `enum TreeRoot { Operation(OperationRef), Type(String) }` with `parse(&Schema, &str) -> Option<Self>` (`Query.me` is an operation, `User` a type), `key(&self) -> String`, `operation(&self) -> Option<&OperationRef>`
  - `enum NodeKind { Leaf, Composite, Recursive }`
  - `struct Node { path: Path, label: String, kind: NodeKind, description: String }`
  - `struct Row { node: Node, depth: usize, expanded: bool }`
  - `root_node(&Schema, &TreeRoot) -> Option<Node>`
  - `children(&Schema, &TreeRoot, &[Seg]) -> Vec<Node>`
  - `rows(&Schema, &TreeRoot, &BTreeSet<Path>) -> Vec<Row>` (flattened visible rows; a node is expanded when its path is in the set)
- Produces (`#[cfg(test)] crate::testutil`): `DESIGN_FIXTURE: &str`, `design_schema() -> Schema`.

- [ ] **Step 1: Create the fixture**

`crates/vega-core/tests/fixtures/design.json`:

```json
{
  "data": {
    "__schema": {
      "queryType": { "name": "QueryType" },
      "mutationType": { "name": "MutationType" },
      "subscriptionType": null,
      "types": [
        {
          "kind": "OBJECT", "name": "QueryType", "description": null,
          "fields": [
            { "name": "me", "description": "The current user.", "args": [], "type": { "kind": "OBJECT", "name": "User", "ofType": null }, "isDeprecated": false, "deprecationReason": null },
            { "name": "node", "description": "Fetch any node by id.", "args": [ { "name": "id", "description": null, "type": { "kind": "NON_NULL", "name": null, "ofType": { "kind": "SCALAR", "name": "ID", "ofType": null } }, "defaultValue": null } ], "type": { "kind": "INTERFACE", "name": "Node", "ofType": null }, "isDeprecated": false, "deprecationReason": null },
            { "name": "search", "description": "Search users and posts.", "args": [ { "name": "term", "description": null, "type": { "kind": "NON_NULL", "name": null, "ofType": { "kind": "SCALAR", "name": "String", "ofType": null } }, "defaultValue": null }, { "name": "first", "description": null, "type": { "kind": "SCALAR", "name": "Int", "ofType": null }, "defaultValue": null } ], "type": { "kind": "NON_NULL", "name": null, "ofType": { "kind": "LIST", "name": null, "ofType": { "kind": "NON_NULL", "name": null, "ofType": { "kind": "UNION", "name": "SearchResult", "ofType": null } } } }, "isDeprecated": false, "deprecationReason": null },
            { "name": "version", "description": "Server version.", "args": [], "type": { "kind": "SCALAR", "name": "String", "ofType": null }, "isDeprecated": false, "deprecationReason": null }
          ],
          "inputFields": null, "enumValues": null, "possibleTypes": null
        },
        {
          "kind": "OBJECT", "name": "MutationType", "description": null,
          "fields": [
            { "name": "postCreate", "description": "Create a post.", "args": [ { "name": "input", "description": null, "type": { "kind": "NON_NULL", "name": null, "ofType": { "kind": "INPUT_OBJECT", "name": "PostInput", "ofType": null } }, "defaultValue": null } ], "type": { "kind": "OBJECT", "name": "Post", "ofType": null }, "isDeprecated": false, "deprecationReason": null }
          ],
          "inputFields": null, "enumValues": null, "possibleTypes": null
        },
        {
          "kind": "OBJECT", "name": "User", "description": "A person.",
          "fields": [
            { "name": "id", "description": null, "args": [], "type": { "kind": "NON_NULL", "name": null, "ofType": { "kind": "SCALAR", "name": "ID", "ofType": null } }, "isDeprecated": false, "deprecationReason": null },
            { "name": "name", "description": null, "args": [], "type": { "kind": "SCALAR", "name": "String", "ofType": null }, "isDeprecated": false, "deprecationReason": null },
            { "name": "role", "description": "Access role.", "args": [], "type": { "kind": "ENUM", "name": "Role", "ofType": null }, "isDeprecated": false, "deprecationReason": null },
            { "name": "friends", "description": null, "args": [], "type": { "kind": "NON_NULL", "name": null, "ofType": { "kind": "LIST", "name": null, "ofType": { "kind": "NON_NULL", "name": null, "ofType": { "kind": "OBJECT", "name": "User", "ofType": null } } } }, "isDeprecated": false, "deprecationReason": null },
            { "name": "posts", "description": null, "args": [ { "name": "first", "description": null, "type": { "kind": "NON_NULL", "name": null, "ofType": { "kind": "SCALAR", "name": "Int", "ofType": null } }, "defaultValue": null } ], "type": { "kind": "NON_NULL", "name": null, "ofType": { "kind": "LIST", "name": null, "ofType": { "kind": "NON_NULL", "name": null, "ofType": { "kind": "OBJECT", "name": "Post", "ofType": null } } } }, "isDeprecated": false, "deprecationReason": null },
            { "name": "post", "description": null, "args": [ { "name": "id", "description": null, "type": { "kind": "NON_NULL", "name": null, "ofType": { "kind": "SCALAR", "name": "ID", "ofType": null } }, "defaultValue": null } ], "type": { "kind": "OBJECT", "name": "Post", "ofType": null }, "isDeprecated": false, "deprecationReason": null }
          ],
          "inputFields": null, "enumValues": null, "possibleTypes": null
        },
        {
          "kind": "OBJECT", "name": "Post", "description": "A written post.",
          "fields": [
            { "name": "id", "description": null, "args": [], "type": { "kind": "NON_NULL", "name": null, "ofType": { "kind": "SCALAR", "name": "ID", "ofType": null } }, "isDeprecated": false, "deprecationReason": null },
            { "name": "title", "description": null, "args": [], "type": { "kind": "SCALAR", "name": "String", "ofType": null }, "isDeprecated": false, "deprecationReason": null },
            { "name": "author", "description": null, "args": [], "type": { "kind": "OBJECT", "name": "User", "ofType": null }, "isDeprecated": false, "deprecationReason": null }
          ],
          "inputFields": null, "enumValues": null, "possibleTypes": null
        },
        {
          "kind": "INTERFACE", "name": "Node", "description": "Anything with an id.",
          "fields": [
            { "name": "id", "description": null, "args": [], "type": { "kind": "NON_NULL", "name": null, "ofType": { "kind": "SCALAR", "name": "ID", "ofType": null } }, "isDeprecated": false, "deprecationReason": null }
          ],
          "inputFields": null, "enumValues": null,
          "possibleTypes": [ { "kind": "OBJECT", "name": "User", "ofType": null }, { "kind": "OBJECT", "name": "Post", "ofType": null } ]
        },
        {
          "kind": "UNION", "name": "SearchResult", "description": "A user or a post.",
          "fields": null, "inputFields": null, "enumValues": null,
          "possibleTypes": [ { "kind": "OBJECT", "name": "User", "ofType": null }, { "kind": "OBJECT", "name": "Post", "ofType": null } ]
        },
        {
          "kind": "INPUT_OBJECT", "name": "PostInput", "description": "Fields for a new post.",
          "fields": null,
          "inputFields": [ { "name": "title", "description": null, "type": { "kind": "NON_NULL", "name": null, "ofType": { "kind": "SCALAR", "name": "String", "ofType": null } }, "defaultValue": null } ],
          "enumValues": null, "possibleTypes": null
        },
        {
          "kind": "ENUM", "name": "Role", "description": "What a user may do.",
          "fields": null, "inputFields": null,
          "enumValues": [
            { "name": "ADMIN", "description": "Full access.", "isDeprecated": false, "deprecationReason": null },
            { "name": "VIEWER", "description": "Read-only.", "isDeprecated": false, "deprecationReason": null }
          ],
          "possibleTypes": null
        },
        { "kind": "SCALAR", "name": "ID", "description": null, "fields": null, "inputFields": null, "enumValues": null, "possibleTypes": null },
        { "kind": "SCALAR", "name": "String", "description": null, "fields": null, "inputFields": null, "enumValues": null, "possibleTypes": null },
        { "kind": "SCALAR", "name": "Int", "description": null, "fields": null, "inputFields": null, "enumValues": null, "possibleTypes": null },
        { "kind": "SCALAR", "name": "Boolean", "description": null, "fields": null, "inputFields": null, "enumValues": null, "possibleTypes": null }
      ]
    }
  }
}
```

- [ ] **Step 2: Add the test helper and write the failing tests**

Append to `crates/vega-core/src/testutil.rs`:

```rust
pub const DESIGN_FIXTURE: &str = include_str!("../tests/fixtures/design.json");

pub fn design_schema() -> Schema {
    serde_json::from_str::<IntrospectionResponse>(DESIGN_FIXTURE)
        .expect("design fixture parses")
        .data
        .schema
}
```

In `crates/vega-core/src/lib.rs` add `pub mod selection;` (keep modules alphabetical). Create `crates/vega-core/src/selection.rs` containing only the tests:

```rust
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
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p vega-core selection`
Expected: FAIL to compile (`cannot find type TreeRoot`, ...).

- [ ] **Step 4: Write the implementation**

Insert above `#[cfg(test)]` in `selection.rs`:

```rust
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
        Some(
            TypeKind::Object | TypeKind::Interface | TypeKind::Union | TypeKind::InputObject,
        ) => {
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
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p vega-core selection`
Expected: `test result: ok. 10 passed`.

- [ ] **Step 6: Check and commit**

Run: `just fix && just check`
Expected: exit code 0.

```bash
git add crates/vega-core/src/lib.rs crates/vega-core/src/selection.rs crates/vega-core/src/testutil.rs crates/vega-core/tests/fixtures/design.json
git commit -m "feat: add a lazy tree model over operations and types"
```

---

### Task 2: Leaf selection and select path parsing

**Files:**
- Modify: `crates/vega-core/src/selection.rs`

**Interfaces:**
- Consumes: Task 1 items.
- Produces (`vega_core::selection`):
  - `struct Selection` (`Default`, `Clone`, `PartialEq`, `Debug`) with `is_empty(&self) -> bool`, `contains(&self, &[Seg]) -> bool`, `has_under(&self, &[Seg]) -> bool` (any selected leaf equal to or below the prefix), `insert(&mut self, Path)`, `toggle_node(&mut self, &Node) -> bool` (toggles a `Leaf` node, returns whether the selection changed; other kinds change nothing and return false), `leaves(&self) -> impl Iterator<Item = &Path>`
  - `enum SelectError { Empty, Unknown(String), NotALeaf(String) }` (`thiserror`)
  - `parse_select(&Schema, &OperationRef, &str) -> Result<Selection, SelectError>`: comma-separated paths of `.`-separated segments, `on:Type` for branches; every path must end at a `Leaf` node; empty input is `Empty`.

- [ ] **Step 1: Write the failing tests**

Add inside the existing `mod tests` of `selection.rs`:

```rust
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
```

`SelectError` needs `Debug` and `PartialEq` for these assertions, and `Selection` needs `Debug` for `unwrap`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p vega-core selection`
Expected: FAIL to compile (`cannot find type Selection`).

- [ ] **Step 3: Write the implementation**

Insert above `#[cfg(test)]` in `selection.rs`:

```rust
/// The set of selected leaf fields. Objects are never stored: they are implied by
/// the leaves below them.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Selection {
    leaves: BTreeSet<Path>,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.leaves.is_empty()
    }

    pub fn contains(&self, path: &[Seg]) -> bool {
        self.leaves.contains(path)
    }

    /// True when a selected leaf equals `prefix` or lies below it.
    pub fn has_under(&self, prefix: &[Seg]) -> bool {
        self.leaves
            .range(prefix.to_vec()..)
            .next()
            .is_some_and(|path| path.starts_with(prefix))
    }

    pub fn insert(&mut self, path: Path) {
        self.leaves.insert(path);
    }

    /// Toggles a leaf node. Returns whether the selection changed; objects and
    /// recursive nodes are ignored.
    pub fn toggle_node(&mut self, node: &Node) -> bool {
        if node.kind != NodeKind::Leaf {
            return false;
        }
        if !self.leaves.remove(&node.path) {
            self.leaves.insert(node.path.clone());
        }
        true
    }

    pub fn leaves(&self) -> impl Iterator<Item = &Path> {
        self.leaves.iter()
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
            let seg = match segment.strip_prefix("on:") {
                Some(type_name) => Seg::Branch(type_name.to_string()),
                None => Seg::Field(segment.to_string()),
            };
            let wanted = join(&path, seg);
            let found = children(schema, &root, &path)
                .into_iter()
                .find(|node| node.path == wanted)
                .ok_or_else(|| SelectError::Unknown(item.to_string()))?;
            path = wanted;
            last = Some(found);
        }
        let node = last.ok_or_else(|| SelectError::Unknown(item.to_string()))?;
        if node.kind != NodeKind::Leaf {
            return Err(SelectError::NotALeaf(item.to_string()));
        }
        selection.insert(path);
    }
    if selection.is_empty() {
        Err(SelectError::Empty)
    } else {
        Ok(selection)
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p vega-core selection`
Expected: `test result: ok. 14 passed`.

- [ ] **Step 5: Check and commit**

Run: `just fix && just check`
Expected: exit code 0.

```bash
git add crates/vega-core/src/selection.rs
git commit -m "feat: add leaf selection and select path parsing"
```

---

### Task 3: Query generation

**Files:**
- Create: `crates/vega-core/src/querygen.rs`
- Modify: `crates/vega-core/src/lib.rs`

**Interfaces:**
- Consumes: `OperationRef`, `Selection`, `Seg`, `Path`, `parse_select` (Tasks 1 and 2); `Schema`, `FullType`, `Field`, `InputValue`, `TypeKind`, `Root`.
- Produces (`vega_core::querygen`):
  - `struct VariableDef { name: String, ty: String }` (serializes as `{"name":..,"type":..}`)
  - `struct GeneratedQuery { operation_name: String, text: String, variables: Vec<VariableDef> }` (serializes as `operationName`, `query`, `variables`)
  - `enum GenerateError { UnknownOperation(String), EmptySelection }` (`thiserror`, `PartialEq`)
  - `generate(&Schema, &OperationRef, &Selection) -> Result<GeneratedQuery, GenerateError>`

- [ ] **Step 1: Write the failing tests**

Add `pub mod querygen;` to `lib.rs`. Create `crates/vega-core/src/querygen.rs`:

```rust
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

    fn var(name: &str, ty: &str) -> VariableDef {
        VariableDef {
            name: name.to_string(),
            ty: ty.to_string(),
        }
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
        assert_eq!(q.variables, [var("first", "Int!")]);
    }

    #[test]
    fn optional_arguments_are_not_emitted() {
        let q = gen("Query.search", "on:User.name");
        assert_eq!(
            q.text,
            "query Search($term: String!) {\n  search(term: $term) {\n    __typename\n    ... on User {\n      name\n    }\n  }\n}"
        );
        assert_eq!(q.variables, [var("term", "String!")]);
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
        assert_eq!(q.variables, [var("id", "ID!"), var("postId", "ID!")]);
    }

    #[test]
    fn mutations_use_their_keyword_and_input_types() {
        let q = gen("Mutation.postCreate", "id");
        assert_eq!(
            q.text,
            "mutation PostCreate($input: PostInput!) {\n  postCreate(input: $input) {\n    id\n  }\n}"
        );
        assert_eq!(q.variables, [var("input", "PostInput!")]);
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
    }
}
```

`GeneratedQuery` and `GenerateError` need `Debug`/`PartialEq` for these assertions.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p vega-core querygen`
Expected: FAIL to compile (`cannot find function generate`).

- [ ] **Step 3: Write the implementation**

Insert above `#[cfg(test)]` in `querygen.rs`:

```rust
//! Turns a selection of leaf fields into GraphQL query text plus the variables the
//! query needs. Only required arguments are emitted (as variables); everything is
//! generated in schema order so the same selection always gives the same text.

use std::collections::HashSet;

use serde::Serialize;

use crate::schema::{Field, FullType, InputValue, Root, Schema, TypeKind};
use crate::selection::{OperationRef, Path, Seg, Selection};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VariableDef {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
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

fn is_required(arg: &InputValue) -> bool {
    arg.ty.kind == TypeKind::NonNull && arg.default_value.is_none()
}

struct Builder<'a> {
    schema: &'a Schema,
    selection: &'a Selection,
    variables: Vec<VariableDef>,
    used: HashSet<String>,
}

impl Builder<'_> {
    /// `name` or `name(arg: $arg, ...)`, registering a variable per required argument.
    fn call(&mut self, field: &Field) -> String {
        let args: Vec<String> = field
            .args
            .iter()
            .filter(|arg| is_required(arg))
            .map(|arg| {
                let variable = self.variable_for(&field.name, arg);
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
    fn variable_for(&mut self, field_name: &str, arg: &InputValue) -> String {
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
                let call = self.call(field);
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
    let call = builder.call(field);

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
            .map(|v| format!("${}: {}", v.name, v.ty))
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p vega-core querygen`
Expected: `test result: ok. 10 passed`.

- [ ] **Step 5: Check and commit**

Run: `just fix && just check`
Expected: exit code 0.

```bash
git add crates/vega-core/src/lib.rs crates/vega-core/src/querygen.rs
git commit -m "feat: generate queries from a selection"
```

---

### Task 4: `--query` and `--select` on the command line

**Files:**
- Modify: `crates/vega/src/cli.rs`, `crates/vega/src/output.rs`, `crates/vega/src/main.rs`
- Modify: `crates/vega/tests/cli.rs`

**Interfaces:**
- Consumes: `OperationRef::parse`, `parse_select`, `Selection`, `querygen::{generate, GeneratedQuery}`.
- Produces: `Cli.query: Option<String>`, `Cli.select: Option<String>`; `output::query_text(&GeneratedQuery) -> String`.

Output format (text): the query text, a newline, and when there are variables a blank line, `Variables:` and one `  $name: Type` line per variable. With `--json`: the `GeneratedQuery` JSON (`operationName`, `query`, `variables`).

- [ ] **Step 1: Write the failing tests**

Add to the test module in `crates/vega/src/output.rs`:

```rust
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
```

Add to `crates/vega/tests/cli.rs` (it already has `seeded_cache`, `vega`, `stdout`, `stderr` helpers; the seeded schema is the phase 1 fixture where `Query.userRead(id: ID!)` returns `User { id, name, role }`):

```rust
#[test]
fn query_prints_the_generated_query_and_its_variables() {
    let cache = seeded_cache(60);
    let out = vega(
        &cache,
        &["--query", "Query.userRead", "--select", "id,name"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "query UserRead($id: ID!) {\n  userRead(id: $id) {\n    id\n    name\n  }\n}\n\nVariables:\n  $id: ID!\n"
    );
    assert_eq!(stderr(&out), "");
}

#[test]
fn query_json_has_the_operation_name_query_and_variables() {
    let cache = seeded_cache(60);
    let out = vega(
        &cache,
        &["--query", "Query.userRead", "--select", "role", "--json"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(json["operationName"], "UserRead");
    assert!(json["query"].as_str().unwrap().contains("role"));
    assert_eq!(json["variables"][0]["name"], "id");
    assert_eq!(json["variables"][0]["type"], "ID!");
}

#[test]
fn query_without_select_on_an_object_asks_for_a_selection() {
    let cache = seeded_cache(60);
    let out = vega(&cache, &["--query", "Query.userRead"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("nothing selected"));
    assert_eq!(stdout(&out), "");
}

#[test]
fn query_with_an_unknown_operation_or_field_fails_with_a_message() {
    let cache = seeded_cache(60);
    let out = vega(&cache, &["--query", "Query.nope", "--select", "id"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("not found"));

    let out = vega(&cache, &["--query", "Query.userRead", "--select", "nope"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("no such field"));

    let out = vega(&cache, &["--query", "Query.userRead", "--select", "id.x"]);
    assert!(!out.status.success());
}

#[test]
fn select_requires_query_and_query_conflicts_with_search() {
    let cache = seeded_cache(60);
    let out = vega(&cache, &["--select", "id"]);
    assert!(!out.status.success());
    let out = vega(&cache, &["--query", "Query.userRead", "--search", "user"]);
    assert!(!out.status.success());
}

#[test]
fn query_warns_about_a_stale_cache_on_stderr_only() {
    let cache = seeded_cache(25 * 3600);
    let out = vega(
        &cache,
        &["--query", "Query.userRead", "--select", "id", "--json"],
    );
    assert!(out.status.success());
    assert!(stderr(&out).contains("warning"));
    assert!(serde_json::from_str::<serde_json::Value>(&stdout(&out)).is_ok());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p vega`
Expected: FAIL (`cannot find function query_text`; CLI tests fail because the flags do not exist).

- [ ] **Step 3: Implement**

`crates/vega/src/output.rs`: add the import `use vega_core::querygen::GeneratedQuery;` next to the existing `use vega_core::search::Hit;` and this function above the tests (remove the inner `use` lines from the two tests if the compiler reports them as redundant):

```rust
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
```

`crates/vega/src/cli.rs`: update the doc comment and add the two options. The struct's doc comment becomes:

```rust
/// Explore a GraphQL server's schema from the terminal.
///
/// Without --search, --describe or --query, opens the interactive browser.
/// The API key is read from the VEGA_GRAPHQL_KEY environment variable.
```

Add after `describe`:

```rust
    /// Print the query generated for OPERATION (e.g. Query.userRead); sends nothing
    #[arg(long, value_name = "OPERATION", conflicts_with_all = ["search", "describe"])]
    pub query: Option<String>,

    /// Fields to select with --query: comma-separated paths such as id,friends.name
    /// (union or interface branches as on:Type, e.g. on:User.name)
    #[arg(long, value_name = "PATHS", requires = "query")]
    pub select: Option<String>,
```

Update the `--json` doc to `Print JSON on stdout for --search, --describe and --query`.

`crates/vega/src/main.rs`: change the imports and the command block. Imports become:

```rust
use vega_core::schema::Schema;
use vega_core::selection::{self, OperationRef, Selection};
use vega_core::{cache, describe, querygen, search, xdg};
```

Replace the `if cli.search.is_some() || cli.describe.is_some() {` condition with `if cli.search.is_some() || cli.describe.is_some() || cli.query.is_some() {`, and extend the chain inside it by adding, after the `else if let Some(target) = &cli.describe { ... }` branch:

```rust
        } else if let Some(target) = &cli.query {
            print_query(schema, target, cli.select.as_deref(), cli.json)?;
        }
```

(the closing brace of the previous `else if` moves accordingly). Add this function to `main.rs`:

```rust
fn print_query(
    schema: &Schema,
    target: &str,
    select: Option<&str>,
    json: bool,
) -> anyhow::Result<()> {
    let operation = OperationRef::parse(schema, target).with_context(|| {
        format!("operation `{target}` not found: use Query.<field>, Mutation.<field> or Subscription.<field>")
    })?;
    let selection = match select {
        Some(text) => selection::parse_select(schema, &operation, text)?,
        None => Selection::default(),
    };
    let query = querygen::generate(schema, &operation, &selection)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&query)?);
    } else {
        print!("{}", output::query_text(&query));
    }
    Ok(())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p vega`
Expected: all pass (the 2 new unit tests and the 6 new CLI tests included).

- [ ] **Step 5: Check and commit in two commits**

Run: `just fix && just check`
Expected: exit code 0.

Commit the feature first, leaving the CLI tests for the second commit (stash or unstage `crates/vega/tests/cli.rs` for the first; `just check` must pass at each commit, so for the first commit temporarily revert the test file with `git stash push crates/vega/tests/cli.rs`, commit, then `git stash pop`):

```bash
git add crates/vega/src/cli.rs crates/vega/src/output.rs crates/vega/src/main.rs
git commit -m "feat: add --query and --select to print a generated query"
git add crates/vega/tests/cli.rs
git commit -m "test: cover --query and --select end to end"
```

---

### Task 5: Browse the schema as an expandable tree

**Files:**
- Modify: `crates/vega/src/tui/app.rs`, `crates/vega/src/tui/ui.rs`

**Interfaces:**
- Consumes: `selection::{rows, Row, NodeKind, Path, TreeRoot}` (Task 1).
- Produces (`crate::tui::app`):
  - `DesignState { root: TreeRoot, expanded: BTreeSet<Path>, cursor: usize }`; `App.design: Option<DesignState>` replaces `App.detail`; `DetailView`, `back_stack`, `follow_selected`, `go_back` and the use of `describe::detail` in the TUI are removed.
  - `App::design_rows(&self) -> Vec<Row>`, `App::preview_rows(&self) -> Vec<Row>` (highlighted hit with its root expanded, empty when there are no hits), `App::current_description(&self) -> String`, `App::help_keys(&self) -> &'static [(&'static str, &'static str)]`.
  - `BROWSE_KEYS`, `DESIGN_KEYS` replace `HELP_KEYS`.
- Layout: Browse is Results | (Details over Description); Design is Details over Description at full width (Query and Variables panes arrive in the next task). The Details title is always `Details`.

Behavior:
- Entering Design (Enter or Right on a hit, in Browse): `DesignState::new(root)` starts with the root expanded (`expanded` contains the empty path) and the cursor on row 0. Works for operations and for types.
- Design keys: Up/Down move the cursor (clamped to the visible rows); Right on a collapsed `Composite` row expands it, on an expanded `Composite` row with children moves the cursor to the first child, on `Leaf`/`Recursive` rows does nothing; Left on an expanded row collapses it, otherwise moves the cursor to the nearest previous row with `depth - 1` (does nothing on the root); Esc returns to Browse (`design = None`) and keeps the search text; `q` quits; `h`, `?`, F1 open help; Ctrl-C and Ctrl-R as before.

- [ ] **Step 1: Write the failing tests (app.rs)**

In the `tests` module of `crates/vega/src/tui/app.rs`:

1. Delete these tests: `enter_opens_follows_and_backspace_walks_back`, `right_opens_and_follows_and_left_walks_back`, `items_without_a_target_do_not_navigate`, and the helper `title`.
2. Rename the helper `open_detail` to `open_design` (same body, but assert `app.design.is_some()`), and update every remaining test that referred to `app.detail` or `open_detail` to use `app.design` and `open_design`. Rename tests that say "detail view" to say "design mode" (`h_types_in_the_search_list_but_opens_help_in_design_mode`, `esc_closes_only_the_help_in_design_mode`, `q_quits_in_design_mode`). In `enter_with_no_hits_does_nothing` replace `app.preview().is_none()` with `app.preview_rows().is_empty()`. In `esc_closes_only_the_help_in_design_mode` assert `app.design.is_some()` after Esc.
3. Add the design fixture and helpers next to the existing ones:

```rust
    const DESIGN_FIXTURE: &str = include_str!("../../../vega-core/tests/fixtures/design.json");

    fn design_app() -> App {
        let response: IntrospectionResponse = serde_json::from_str(DESIGN_FIXTURE).unwrap();
        App::new(response.data.schema, "https://example.com/graphql", 1_000)
    }

    /// Opens the design view of `Query.me` in the design fixture app.
    fn open_me(app: &mut App) {
        type_text(app, "me");
        assert_eq!(app.hits[0].path, "Query.me");
        press(app, KeyCode::Enter);
        assert!(app.design.is_some());
    }

    fn row_labels(app: &App) -> Vec<String> {
        app.design_rows().iter().map(|r| r.node.label.clone()).collect()
    }
```

4. Add these tests:

```rust
    #[test]
    fn enter_and_right_open_the_highlighted_hit_with_its_root_expanded() {
        for code in [KeyCode::Enter, KeyCode::Right] {
            let mut app = design_app();
            type_text(&mut app, "me");
            press(&mut app, code);
            let labels = row_labels(&app);
            assert_eq!(labels.len(), 7);
            assert_eq!(labels[0], "me: User");
            assert_eq!(labels[1], "id: ID!");
            assert_eq!(app.design.as_ref().unwrap().cursor, 0);
        }
    }

    #[test]
    fn a_type_hit_opens_the_same_tree() {
        let mut app = design_app();
        type_text(&mut app, "postinput");
        press(&mut app, KeyCode::Enter);
        assert_eq!(row_labels(&app), ["PostInput (INPUT_OBJECT)", "title: String!"]);
    }

    #[test]
    fn up_and_down_move_the_cursor_and_clamp() {
        let mut app = design_app();
        open_me(&mut app);
        press(&mut app, KeyCode::Up);
        assert_eq!(app.design.as_ref().unwrap().cursor, 0);
        for _ in 0..20 {
            press(&mut app, KeyCode::Down);
        }
        assert_eq!(app.design.as_ref().unwrap().cursor, 6);
        press(&mut app, KeyCode::Up);
        assert_eq!(app.design.as_ref().unwrap().cursor, 5);
    }

    #[test]
    fn right_expands_then_steps_into_a_composite_and_left_collapses_then_goes_up() {
        let mut app = design_app();
        open_me(&mut app);
        for _ in 0..4 {
            press(&mut app, KeyCode::Down); // friends: [User!]!
        }
        assert_eq!(app.design_rows()[4].node.label, "friends: [User!]!");

        press(&mut app, KeyCode::Right); // expand
        assert_eq!(app.design_rows().len(), 13);
        assert_eq!(app.design.as_ref().unwrap().cursor, 4);

        press(&mut app, KeyCode::Right); // step into the first child
        assert_eq!(app.design.as_ref().unwrap().cursor, 5);

        press(&mut app, KeyCode::Left); // collapsed leaf: go to the parent
        assert_eq!(app.design.as_ref().unwrap().cursor, 4);

        press(&mut app, KeyCode::Left); // expanded: collapse
        assert_eq!(app.design_rows().len(), 7);
        assert_eq!(app.design.as_ref().unwrap().cursor, 4);
    }

    #[test]
    fn right_does_nothing_on_leaves_and_recursive_nodes() {
        let mut app = design_app();
        open_me(&mut app);
        press(&mut app, KeyCode::Down); // id (leaf)
        press(&mut app, KeyCode::Right);
        assert_eq!(app.design_rows().len(), 7);
        assert_eq!(app.design.as_ref().unwrap().cursor, 1);

        // Expand friends, then move to its recursive `friends` child.
        for _ in 0..3 {
            press(&mut app, KeyCode::Down);
        }
        press(&mut app, KeyCode::Right);
        for _ in 0..4 {
            press(&mut app, KeyCode::Down);
        }
        assert_eq!(app.design_rows()[8].node.label, "friends: [User!]!");
        let before = app.design_rows().len();
        press(&mut app, KeyCode::Right);
        assert_eq!(app.design_rows().len(), before);
    }

    #[test]
    fn left_on_the_collapsed_root_does_nothing() {
        let mut app = design_app();
        open_me(&mut app);
        press(&mut app, KeyCode::Left); // collapse the root
        assert_eq!(app.design_rows().len(), 1);
        press(&mut app, KeyCode::Left);
        assert_eq!(app.design_rows().len(), 1);
        assert!(app.design.is_some());
        assert!(!app.should_quit);
    }

    #[test]
    fn esc_returns_to_the_results_and_keeps_the_search_text() {
        let mut app = design_app();
        open_me(&mut app);
        press(&mut app, KeyCode::Esc);
        assert!(app.design.is_none());
        assert_eq!(app.query, "me");
        assert!(!app.should_quit);
    }

    #[test]
    fn q_is_a_letter_in_the_results_and_quits_in_design() {
        let mut app = design_app();
        open_me(&mut app);
        press(&mut app, KeyCode::Char('q'));
        assert!(app.should_quit);
    }

    #[test]
    fn preview_rows_show_the_highlighted_hit_with_its_root_expanded() {
        let mut app = design_app();
        type_text(&mut app, "me");
        let preview = app.preview_rows();
        assert_eq!(preview.len(), 7);
        assert_eq!(preview[0].node.label, "me: User");
        assert!(preview[0].expanded);
        assert_eq!(app.current_description(), "The current user.");

        type_text(&mut app, "zzz");
        assert!(app.preview_rows().is_empty());
        assert_eq!(app.current_description(), "");
    }

    #[test]
    fn the_description_follows_the_cursor_in_design() {
        let mut app = design_app();
        open_me(&mut app);
        assert_eq!(app.current_description(), "The current user.");
        for _ in 0..3 {
            press(&mut app, KeyCode::Down); // role: Role
        }
        assert_eq!(app.current_description(), "Access role.");
    }

    #[test]
    fn help_lists_the_keys_of_the_current_mode() {
        let mut app = design_app();
        assert!(app.help_keys().iter().any(|(keys, _)| keys.contains("Enter")));
        assert!(!app.help_keys().iter().any(|(keys, _)| keys.contains('q')));
        open_me(&mut app);
        assert!(app.help_keys().iter().any(|(keys, _)| keys.contains("←")));
        assert!(app.help_keys().iter().any(|(keys, _)| *keys == "q"));
    }

    #[test]
    fn a_refresh_leaves_design_mode() {
        let mut app = design_app();
        open_me(&mut app);
        let response: IntrospectionResponse = serde_json::from_str(DESIGN_FIXTURE).unwrap();
        app.apply_refresh_ok(response.data.schema, 5_000);
        assert!(app.design.is_none());
    }
```

- [ ] **Step 2: Write the failing tests (ui.rs)**

In the `tests` module of `crates/vega/src/tui/ui.rs`:
- Replace the `HELP_KEYS` import use with `app.help_keys()`: in `help_modal_shows_the_version_and_every_key` iterate `for (keys, _) in app.help_keys()`.
- Replace `shows_the_host_hits_and_a_preview` with:

```rust
    #[test]
    fn browse_shows_results_a_details_tree_and_the_description() {
        let terminal = render(&app(), 1_000 + 3_600);
        let text = screen(&terminal);
        assert!(text.contains("nexus.example.com"));
        assert!(text.contains("schema fetched 1h ago"));
        assert!(text.contains("Results"));
        assert!(text.contains("Query.userRead"));
        assert!(text.contains("Mutation.userCreate"));
        assert!(text.contains("Details"));
        assert!(!text.contains("Preview"));
        assert!(text.contains("▾ userRead(id: ID!): User"));
        assert!(text.contains("• id: ID!"));
        assert!(text.contains("Description"));
        assert!(text.contains("Requires USER_READ"));
    }

    #[test]
    fn design_shows_the_tree_with_the_cursor_row_and_no_results() {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = app();
        for c in "userread".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.design.is_some());
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("Details"));
        assert!(text.contains("▾ userRead(id: ID!): User"));
        assert!(!text.contains("Results"));
        assert!(!text.contains("Search"));
        assert!(text.contains("Requires USER_READ"));
    }

    #[test]
    fn design_does_not_panic_on_tiny_terminals() {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = app();
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.design.is_some());
        for (w, h) in [(20, 5), (1, 1), (0, 0), (3, 40), (80, 24)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|frame| draw(frame, &app, 1_000)).unwrap();
        }
    }
```

(`app()` here is the existing ui-test helper built from the phase 1 fixture; its first hit is `Query.userRead`.)

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p vega tui`
Expected: FAIL to compile (`no field design on type App`, `no method design_rows`, ...).

- [ ] **Step 4: Rewrite the navigation in app.rs**

In `crates/vega/src/tui/app.rs` (non-test part):

Imports become:

```rust
use std::collections::BTreeSet;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use vega_core::cache::{format_age, is_stale};
use vega_core::schema::Schema;
use vega_core::search::{self, Hit};
use vega_core::selection::{self, NodeKind, Path, Row, TreeRoot};
```

Remove `DetailView` and `HELP_KEYS`. Add:

```rust
/// Hotkeys in Browse mode as (keys, description); shown by the help modal.
pub const BROWSE_KEYS: &[(&str, &str)] = &[
    ("↑ / ↓", "Move through the results"),
    ("Enter / →", "Open the highlighted item as a tree"),
    ("Esc", "Quit"),
    ("Ctrl-R", "Refresh the schema"),
    ("? / F1", "Show or hide this help"),
    ("Ctrl-C", "Quit"),
];

/// Hotkeys in Design mode.
pub const DESIGN_KEYS: &[(&str, &str)] = &[
    ("↑ / ↓", "Move through the tree"),
    ("→", "Expand a node, or step into it"),
    ("←", "Collapse a node, or go to its parent"),
    ("Esc", "Back to the results"),
    ("q", "Quit"),
    ("Ctrl-R", "Refresh the schema"),
    ("h / ? / F1", "Show or hide this help"),
    ("Ctrl-C", "Quit"),
];

/// The tree being explored: which root, which nodes are expanded, where the cursor is.
#[derive(Debug, Clone)]
pub struct DesignState {
    pub root: TreeRoot,
    pub expanded: BTreeSet<Path>,
    pub cursor: usize,
}

impl DesignState {
    fn new(root: TreeRoot) -> Self {
        let mut expanded = BTreeSet::new();
        expanded.insert(Path::new());
        Self {
            root,
            expanded,
            cursor: 0,
        }
    }
}
```

In `struct App` replace `pub detail: Option<DetailView>` and `back_stack: Vec<DetailView>` with `pub design: Option<DesignState>` (and update `App::new` accordingly: `design: None`, no `back_stack`).

In `handle_key`: replace both uses of `self.detail.is_some()` with `self.design.is_some()`, and route `if self.design.is_some() { self.handle_design_key(key.code); } else { self.handle_search_key(key.code); }`.

In `handle_search_key` the `KeyCode::Enter | KeyCode::Right` arm calls `self.start_design()`.

Replace `handle_detail_key`, `open_highlighted_hit`, `follow_selected`, `go_back` and `preview` with:

```rust
    fn handle_design_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Esc => self.design = None,
            KeyCode::Down => self.move_cursor(1),
            KeyCode::Up => self.move_cursor(-1),
            KeyCode::Right => self.expand_or_step_in(),
            KeyCode::Left => self.collapse_or_go_up(),
            _ => {}
        }
    }

    fn start_design(&mut self) {
        let Some(hit) = self.hits.get(self.hit_index) else {
            return;
        };
        if let Some(root) = TreeRoot::parse(&self.schema, &hit.path) {
            self.design = Some(DesignState::new(root));
        }
    }

    fn move_cursor(&mut self, delta: isize) {
        let last = self.design_rows().len().saturating_sub(1);
        if let Some(design) = self.design.as_mut() {
            design.cursor = design.cursor.saturating_add_signed(delta).min(last);
        }
    }

    fn expand_or_step_in(&mut self) {
        let rows = self.design_rows();
        let Some(design) = self.design.as_mut() else {
            return;
        };
        let Some(row) = rows.get(design.cursor) else {
            return;
        };
        if row.node.kind != NodeKind::Composite {
            return;
        }
        if !row.expanded {
            design.expanded.insert(row.node.path.clone());
        } else if rows
            .get(design.cursor + 1)
            .is_some_and(|next| next.depth > row.depth)
        {
            design.cursor += 1;
        }
    }

    fn collapse_or_go_up(&mut self) {
        let rows = self.design_rows();
        let Some(design) = self.design.as_mut() else {
            return;
        };
        let Some(row) = rows.get(design.cursor) else {
            return;
        };
        if row.expanded {
            design.expanded.remove(&row.node.path);
        } else if let Some(parent) = rows[..design.cursor]
            .iter()
            .rposition(|candidate| candidate.depth + 1 == row.depth)
        {
            design.cursor = parent;
        }
    }

    /// The visible rows of the tree being designed (empty in Browse mode).
    pub fn design_rows(&self) -> Vec<Row> {
        match &self.design {
            Some(design) => selection::rows(&self.schema, &design.root, &design.expanded),
            None => Vec::new(),
        }
    }

    /// Rows for the Details pane in Browse mode: the highlighted hit, root expanded.
    pub fn preview_rows(&self) -> Vec<Row> {
        let Some(hit) = self.hits.get(self.hit_index) else {
            return Vec::new();
        };
        let Some(root) = TreeRoot::parse(&self.schema, &hit.path) else {
            return Vec::new();
        };
        let mut expanded = BTreeSet::new();
        expanded.insert(Path::new());
        selection::rows(&self.schema, &root, &expanded)
    }

    /// Description of the node under the cursor (Design) or of the highlighted hit (Browse).
    pub fn current_description(&self) -> String {
        match &self.design {
            Some(design) => self
                .design_rows()
                .get(design.cursor)
                .map(|row| row.node.description.clone())
                .unwrap_or_default(),
            None => self
                .preview_rows()
                .first()
                .map(|row| row.node.description.clone())
                .unwrap_or_default(),
        }
    }

    pub fn help_keys(&self) -> &'static [(&'static str, &'static str)] {
        if self.design.is_some() {
            DESIGN_KEYS
        } else {
            BROWSE_KEYS
        }
    }
```

In `apply_refresh_ok` replace `self.detail = None; self.back_stack.clear();` with `self.design = None;`. Remove the now-unused `describe` import. Keep `Action`, `RefreshState`, `StatusLevel`, `host_of`, `status` and the refresh methods unchanged.

- [ ] **Step 5: Rewrite the layout in ui.rs**

In `crates/vega/src/tui/ui.rs`:

Imports: replace the `crate::tui::app::{App, StatusLevel, HELP_KEYS}` import with `crate::tui::app::{App, StatusLevel}` and add `use vega_core::selection::{NodeKind, Row};`.

`draw` becomes:

```rust
pub fn draw(frame: &mut Frame, app: &App, now: u64) {
    let area = frame.area();
    if app.design.is_some() {
        let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(area);
        draw_status(frame, rows[0], app, now);
        let tree = app.design_rows();
        let cursor = app.design.as_ref().map(|design| design.cursor);
        draw_details(frame, rows[1], app, &tree, cursor);
    } else {
        let rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(area);
        draw_status(frame, rows[0], app, now);
        draw_search(frame, rows[1], app);
        let columns = Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(rows[2]);
        draw_hits(frame, columns[0], app);
        draw_details(frame, columns[1], app, &app.preview_rows(), None);
    }
    if app.help_open {
        draw_help(frame, area, app);
    }
}
```

`draw_help` takes the keys from the app: change its signature to `fn draw_help(frame: &mut Frame, area: Rect, app: &App)` and replace every use of `HELP_KEYS` inside it with `app.help_keys()` (bind `let keys = app.help_keys();` at the top and iterate over `keys`).

`draw_search`: remove the `app.detail` checks. The title is always `"Search (type to filter · Enter/→ open · Esc quit · ? help)"` and the cursor is always positioned (Browse is the only mode that draws it).

`draw_hits`: show only the path and drop the dim styling:

```rust
fn draw_hits(frame: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = app
        .hits
        .iter()
        .map(|hit| ListItem::new(Line::raw(hit.path.clone())))
        .collect();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!("Results ({})", app.hits.len()));
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut state = ListState::default();
    if !app.hits.is_empty() {
        state.select(Some(app.hit_index));
    }
    frame.render_stateful_widget(list, area, &mut state);
}
```

Replace `draw_detail` with:

```rust
/// The Details tree over the Description pane.
fn draw_details(frame: &mut Frame, area: Rect, app: &App, rows: &[Row], cursor: Option<usize>) {
    let parts =
        Layout::vertical([Constraint::Percentage(70), Constraint::Percentage(30)]).split(area);

    let block = Block::default().borders(Borders::ALL).title("Details");
    if rows.is_empty() {
        frame.render_widget(Paragraph::new("No matches").block(block), parts[0]);
    } else {
        let items: Vec<ListItem> = rows.iter().map(tree_item).collect();
        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
        let mut state = ListState::default();
        state.select(cursor);
        frame.render_stateful_widget(list, parts[0], &mut state);
    }

    let description = Block::default().borders(Borders::ALL).title("Description");
    frame.render_widget(
        Paragraph::new(app.current_description())
            .wrap(Wrap { trim: false })
            .block(description),
        parts[1],
    );
}

fn tree_item(row: &Row) -> ListItem<'static> {
    let marker = match row.node.kind {
        NodeKind::Composite if row.expanded => "▾ ",
        NodeKind::Composite => "▸ ",
        NodeKind::Recursive => "↻ ",
        NodeKind::Leaf => "• ",
    };
    ListItem::new(Line::raw(format!(
        "{}{marker}{}",
        "  ".repeat(row.depth),
        row.node.label
    )))
}
```

Remove the now-unused imports (`Text` stays used by the help modal; `Span` is used by the help modal; drop anything the compiler reports as unused).

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p vega`
Expected: all pass. If an old test still refers to removed items, adapt it as described in Step 1 and report which one.

- [ ] **Step 7: Check and commit**

Run: `just fix && just check`
Expected: exit code 0.

```bash
git add crates/vega/src/tui/app.rs crates/vega/src/tui/ui.rs
git commit -m "feat: browse the schema as an expandable tree"
```

(This one commit replaces the detail-view navigation across `app.rs` and `ui.rs`; the two cannot be split while keeping every commit green.)

---

### Task 6: Select fields and show the generated query

**Files:**
- Modify: `crates/vega/src/tui/app.rs`, `crates/vega/src/tui/ui.rs`

**Interfaces:**
- Consumes: `Selection`, `querygen::{generate, GenerateError, GeneratedQuery}`, `TreeRoot::operation`, Task 5 state.
- Produces:
  - `DesignState.selection: Selection`; `DesignState::read_only(&self) -> bool` (true for `TreeRoot::Type`)
  - `App::sessions: HashMap<String, DesignState>` (private): Esc stores the design state under `root.key()`, opening restores it; a refresh clears them
  - `App::generated_query(&self) -> Option<Result<GeneratedQuery, GenerateError>>`: `None` outside Design or for a type root
  - `App::is_selected(&self, &[Seg]) -> bool`
- Behavior: SPACE on a `Leaf` row of an operation toggles it (nothing for objects, recursive nodes or type roots). Operation designs render two columns: left Details over Description, right Query over Variables. The Query pane shows the generated text, or `Select fields with Space to build the query` while `EmptySelection`. Variables lists `$name: Type` per variable, or `No variables`. Leaf rows show `☑ ` / `☐ ` in operation designs (`• ` stays in read-only type trees). `DESIGN_KEYS` gains `("Space", "Select or unselect a field")`.

- [ ] **Step 1: Write the failing tests**

In `crates/vega/src/tui/app.rs` tests add:

```rust
    fn query_text(app: &App) -> String {
        app.generated_query().unwrap().unwrap().text
    }

    #[test]
    fn space_toggles_leaves_and_the_query_follows() {
        let mut app = design_app();
        open_me(&mut app);
        assert!(matches!(
            app.generated_query(),
            Some(Err(vega_core::querygen::GenerateError::EmptySelection))
        ));

        press(&mut app, KeyCode::Down); // id
        press(&mut app, KeyCode::Char(' '));
        assert_eq!(query_text(&app), "query Me {\n  me {\n    id\n  }\n}");
        assert!(app.is_selected(&[Seg::Field("id".to_string())]));

        press(&mut app, KeyCode::Down); // name
        press(&mut app, KeyCode::Char(' '));
        assert_eq!(query_text(&app), "query Me {\n  me {\n    id\n    name\n  }\n}");

        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Char(' ')); // unselect id
        assert_eq!(query_text(&app), "query Me {\n  me {\n    name\n  }\n}");
    }

    #[test]
    fn space_on_an_object_does_nothing() {
        let mut app = design_app();
        open_me(&mut app);
        press(&mut app, KeyCode::Char(' ')); // the root object
        for _ in 0..4 {
            press(&mut app, KeyCode::Down); // friends
        }
        press(&mut app, KeyCode::Char(' '));
        assert!(matches!(app.generated_query(), Some(Err(_))));
    }

    #[test]
    fn selecting_a_nested_leaf_includes_its_ancestors_and_variables() {
        let mut app = design_app();
        open_me(&mut app);
        for _ in 0..5 {
            press(&mut app, KeyCode::Down); // posts(first: Int!)
        }
        press(&mut app, KeyCode::Right); // expand posts
        press(&mut app, KeyCode::Down); // id
        press(&mut app, KeyCode::Char(' '));
        let query = app.generated_query().unwrap().unwrap();
        assert_eq!(
            query.text,
            "query Me($first: Int!) {\n  me {\n    posts(first: $first) {\n      id\n    }\n  }\n}"
        );
        assert_eq!(query.variables.len(), 1);
    }

    #[test]
    fn scalar_operations_generate_without_a_selection() {
        let mut app = design_app();
        type_text(&mut app, "version");
        press(&mut app, KeyCode::Enter);
        let query = app.generated_query().unwrap().unwrap();
        assert_eq!(query.text, "query Version {\n  version\n}");
    }

    #[test]
    fn type_roots_are_read_only() {
        let mut app = design_app();
        type_text(&mut app, "postinput");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' '));
        assert!(app.generated_query().is_none());
        assert!(!app.is_selected(&[Seg::Field("title".to_string())]));
        assert!(app.design.as_ref().unwrap().read_only());
    }

    #[test]
    fn selection_expansion_and_cursor_survive_leaving_and_reentering() {
        let mut app = design_app();
        open_me(&mut app);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' ')); // id
        press(&mut app, KeyCode::Down); // name, cursor 2
        press(&mut app, KeyCode::Esc);
        assert!(app.design.is_none());

        press(&mut app, KeyCode::Enter);
        assert_eq!(app.design.as_ref().unwrap().cursor, 2);
        assert_eq!(query_text(&app), "query Me {\n  me {\n    id\n  }\n}");
    }

    #[test]
    fn a_refresh_clears_the_stored_sessions() {
        let mut app = design_app();
        open_me(&mut app);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Esc);
        let response: IntrospectionResponse = serde_json::from_str(DESIGN_FIXTURE).unwrap();
        app.apply_refresh_ok(response.data.schema, 5_000);
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.generated_query(), Some(Err(_))));
    }

    #[test]
    fn help_in_design_mentions_space() {
        let mut app = design_app();
        open_me(&mut app);
        assert!(app.help_keys().iter().any(|(keys, _)| *keys == "Space"));
    }
```

Add `use vega_core::selection::Seg;` to the test module imports.

In `crates/vega/src/tui/ui.rs` tests add (helper `design_app` is not available there; build the app from `DESIGN_FIXTURE` the same way):

```rust
    const DESIGN_FIXTURE: &str = include_str!("../../../vega-core/tests/fixtures/design.json");

    fn design_app_on(query: &str) -> App {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let response: IntrospectionResponse = serde_json::from_str(DESIGN_FIXTURE).unwrap();
        let mut app = App::new(response.data.schema, "https://example.com/graphql", 1_000);
        for c in query.chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.design.is_some());
        app
    }

    fn press_key(app: &mut App, code: ratatui::crossterm::event::KeyCode) {
        use ratatui::crossterm::event::{KeyEvent, KeyModifiers};
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    #[test]
    fn an_operation_design_shows_query_and_variables_panes() {
        use ratatui::crossterm::event::KeyCode;
        let mut app = design_app_on("me");
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("Query"));
        assert!(text.contains("Variables"));
        assert!(text.contains("Select fields with Space"));
        assert!(text.contains("No variables"));
        assert!(text.contains("☐ id: ID!"));

        press_key(&mut app, KeyCode::Down);
        press_key(&mut app, KeyCode::Char(' '));
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("☑ id: ID!"));
        assert!(text.contains("query Me {"));
        assert!(!text.contains("Select fields with Space"));
    }

    #[test]
    fn required_arguments_are_listed_as_variables() {
        use ratatui::crossterm::event::KeyCode;
        let mut app = design_app_on("node");
        press_key(&mut app, KeyCode::Down); // id
        press_key(&mut app, KeyCode::Char(' '));
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("$id: ID!"));
    }

    #[test]
    fn a_type_design_has_no_query_or_variables_panes_and_no_checkboxes() {
        let app = design_app_on("postinput");
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("Details"));
        assert!(text.contains("• title: String!"));
        assert!(!text.contains("Variables"));
        assert!(!text.contains("☐"));
    }

    #[test]
    fn selection_panes_do_not_panic_on_tiny_terminals() {
        use ratatui::crossterm::event::KeyCode;
        let mut app = design_app_on("me");
        press_key(&mut app, KeyCode::Down);
        press_key(&mut app, KeyCode::Char(' '));
        for (w, h) in [(20, 5), (1, 1), (0, 0), (3, 40), (80, 24)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|frame| draw(frame, &app, 1_000)).unwrap();
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p vega tui`
Expected: FAIL to compile (`no method generated_query`, `no method is_selected`, ...).

- [ ] **Step 3: Implement app.rs changes**

Imports: add `use std::collections::HashMap;`, `use vega_core::querygen::{self, GenerateError, GeneratedQuery};`, and extend the selection import to `use vega_core::selection::{self, NodeKind, Path, Row, Seg, Selection, TreeRoot};`.

Add `("Space", "Select or unselect a field")` to `DESIGN_KEYS`, after the `←` row.

`DesignState` gains `pub selection: Selection` (initialised with `Selection::default()` in `new`) and:

```rust
    /// A type tree is only for reading: nothing can be selected in it.
    pub fn read_only(&self) -> bool {
        matches!(self.root, TreeRoot::Type(_))
    }
```

`App` gains the private field `sessions: HashMap<String, DesignState>` (initialised empty).

Changes to existing methods:
- `handle_design_key`: Esc becomes `KeyCode::Esc => self.leave_design()`; add `KeyCode::Char(' ') => self.toggle_under_cursor()`.
- `start_design` restores a stored session:

```rust
        if let Some(root) = TreeRoot::parse(&self.schema, &hit.path) {
            let state = self
                .sessions
                .remove(&root.key())
                .unwrap_or_else(|| DesignState::new(root));
            self.design = Some(state);
        }
```
- `apply_refresh_ok`: also `self.sessions.clear();`.

New methods:

```rust
    fn leave_design(&mut self) {
        if let Some(design) = self.design.take() {
            self.sessions.insert(design.root.key(), design);
        }
    }

    fn toggle_under_cursor(&mut self) {
        let rows = self.design_rows();
        let Some(design) = self.design.as_mut() else {
            return;
        };
        if design.read_only() {
            return;
        }
        if let Some(row) = rows.get(design.cursor) {
            design.selection.toggle_node(&row.node);
        }
    }

    /// The query for the current selection; `None` outside Design or for a type tree.
    pub fn generated_query(&self) -> Option<Result<GeneratedQuery, GenerateError>> {
        let design = self.design.as_ref()?;
        let operation = design.root.operation()?;
        Some(querygen::generate(&self.schema, operation, &design.selection))
    }

    pub fn is_selected(&self, path: &[Seg]) -> bool {
        self.design
            .as_ref()
            .is_some_and(|design| design.selection.contains(path))
    }
```

- [ ] **Step 4: Implement ui.rs changes**

In the Design branch of `draw`, replace the call to `draw_details` with:

```rust
        let tree = app.design_rows();
        let cursor = app.design.as_ref().map(|design| design.cursor);
        let is_operation = app
            .design
            .as_ref()
            .is_some_and(|design| !design.read_only());
        if is_operation {
            let columns = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(rows[1]);
            draw_details(frame, columns[0], app, &tree, cursor);
            draw_query(frame, columns[1], app);
        } else {
            draw_details(frame, rows[1], app, &tree, cursor);
        }
```

Add:

```rust
/// The generated query over the variables it needs.
fn draw_query(frame: &mut Frame, area: Rect, app: &App) {
    let parts =
        Layout::vertical([Constraint::Percentage(65), Constraint::Percentage(35)]).split(area);
    let query = app.generated_query();

    let text = match &query {
        Some(Ok(generated)) => generated.text.clone(),
        Some(Err(_)) | None => "Select fields with Space to build the query".to_string(),
    };
    let block = Block::default().borders(Borders::ALL).title("Query");
    frame.render_widget(Paragraph::new(text).block(block), parts[0]);

    let variables = match &query {
        Some(Ok(generated)) if !generated.variables.is_empty() => generated
            .variables
            .iter()
            .map(|v| format!("${}: {}", v.name, v.ty))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => "No variables".to_string(),
    };
    let block = Block::default().borders(Borders::ALL).title("Variables");
    frame.render_widget(Paragraph::new(variables).block(block), parts[1]);
}
```

`tree_item` needs to know whether leaves are selectable and selected: change its signature to `fn tree_item(app: &App, row: &Row) -> ListItem<'static>` and call it as `rows.iter().map(|row| tree_item(app, row))`. The `Leaf` marker becomes:

```rust
        NodeKind::Leaf => match &app.design {
            Some(design) if !design.read_only() => {
                if app.is_selected(&row.node.path) { "☑ " } else { "☐ " }
            }
            _ => "• ",
        },
```

(Browse previews keep `• `, as the Task 5 test expects.)

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p vega`
Expected: all pass.

- [ ] **Step 6: Check and commit**

Run: `just fix && just check`
Expected: exit code 0.

```bash
git add crates/vega/src/tui/app.rs crates/vega/src/tui/ui.rs
git commit -m "feat: select fields and show the generated query"
```

---

### Task 7: Documentation

**Files:**
- Modify: `docs/superpowers/specs/2026-10-22-vega-phase-2-design.md`, `README.md`, `AGENTS.md`

- [ ] **Step 1: Update the spec**

In the phase 2 spec:
- Tree model bullet about recursion: replace "A type that already appears on the path from the root is shown as a collapsed node marked `↻` and cannot be expanded further." with "A type that has already appeared twice on the path from the root is shown as a collapsed node marked `↻` and cannot be expanded further, so one level of self-nesting (`me`, then `friends`) still works."
- Modes and layout table: the Browse row becomes `Browse   Results (names only)  | Details (tree, read-only preview)` followed by a line `         | Description` (Description is shown under Details in every mode).
- CLI parity section: state that `--select` paths may go through recursive types to any depth even though the interactive tree stops expanding them.

- [ ] **Step 2: Update README and AGENTS.md**

README, in the Usage block add:

```sh
vega --query Query.userRead --select id,name   # print the generated query and its variables
```

and add to the "What it does" list: `- Build a query by picking fields from a tree: expand with →, select with Space, and watch the generated query and its variables update.` Update the sentence "Composing and running queries is planned but not implemented yet." to "Selecting fields and generating the query works; editing variables and running queries against the server are planned."

AGENTS.md, Layout section: change the `vega-core` bullet to `Schema model, search, detail rendering, tree and selection model, query generation, cache, config, HTTP client. No UI code.` and update the Scope section to `Phase 2 (in progress): build queries by selecting fields. Step 1 (tree, selection, generated query, --query) is done; variables and running queries are next.`

- [ ] **Step 3: Check and commit**

Run: `just check`
Expected: exit code 0.

```bash
git add docs/superpowers/specs/2026-10-22-vega-phase-2-design.md README.md AGENTS.md
git commit -m "doc: describe the tree design mode and the --query flag"
```

---

### Task 8: End-to-end verification

**Files:** none (verification only).

- [ ] **Step 1: Run the full checks**

Run: `just check`
Expected: exit code 0.

- [ ] **Step 2: Try the command line against a seeded cache**

Use a scratch directory made with `mktemp -d`; seed `$XDG_CACHE_HOME/vega/` the way `crates/vega/tests/cli.rs` does (or run the CLI tests, which already do), then confirm by hand with the built binary:
`vega --query Query.userRead --select id,name` prints the query and `$id: ID!`; `--json` is valid JSON; `--query Query.userRead` without `--select` exits nonzero with `nothing selected`; stdout stays empty on errors.

- [ ] **Step 3: Hand the interactive check to the user**

The TUI cannot be driven without a terminal. Ask the user to run `vega` against their server and check: the Details title is always `Details`; Enter opens the tree; →/← expand and collapse; Space selects a leaf and the Query pane updates; required arguments appear under Variables; Esc returns with the search text kept and re-entering restores the selection; `?` lists the keys of the current mode; Ctrl-R refresh clears selections.

---

## Self-review (spec coverage)

| Spec requirement (step 1) | Task |
| --- | --- |
| Results names only, Details title fixed, Description pane | 5 |
| Tree in Details; Browse preview; Design two-column; type roots read-only | 5, 6 |
| Right/Left/Up/Down/Space/Esc/q keys; help per mode | 5, 6 |
| Lazy expansion and recursion limit | 1 |
| Leaf-only selection, derived query, objects implied | 2, 3, 6 |
| Query generation: shape, schema order, fragments, `__typename`, variables, collisions | 3 |
| Per-operation session state, cleared by refresh | 6 |
| `--query` / `--select` CLI | 4 |
| Unions and interfaces as branches | 1, 3 |
| Variables listed (not editable), required arguments only | 3, 6 |

Not in this plan by design: editable variables and optional-argument nodes (step 2), running queries, the Response pane and mutation confirmation (step 3).
