# Vega Phase 2, Step 2: Variables Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user opt optional arguments into the query from the tree, see every included argument as a typed variable row, edit its value with an editor that fits its type (text, toggle, list of choices, JSON), see invalid values in red with a reason, and set values from the command line with `--var`. Nothing is sent to the server yet (step 3).

**Architecture:** `vega-core` gains argument nodes in the tree model (`Seg::Args`, `Seg::Arg`, `NodeKind::Required/Optional`), an argument set in `Selection`, richer `VariableDef`s from `querygen`, and a pure `variables` module (editor choice, typed parsing and validation). The binary gets `--var` and a Variables pane with focus, a cursor and an inline editor. Spec: `docs/superpowers/specs/2026-10-22-vega-phase-2-design.md` (sections Tree model, Variables, CLI parity).

**Tech Stack:** Rust (stable, edition 2021), `serde`, `serde_json`, `thiserror`, `clap`, `anyhow`, `ratatui` 0.30. No new dependencies.

## Decisions made while planning (review these)

1. `--var name=value` lands in this step (it only prints); `--run`/`--yes` stay in step 3.
2. CLI syntax to opt an optional argument into the query: the path segment `arg:<name>` in `--select`, mirroring `on:<Type>`. Examples: `--select id,arg:role` (argument of the operation), `--select friends.name,friends.arg:first` (argument of a nested field). Naming a required argument is an error (it is always included).
3. An argument with a default value counts as optional and its variable is declared with that default (`$limit: Int! = 10`). Leaving its value empty keeps the default.
4. Argument rows are always leaves: an input object or list argument is edited as one JSON value, never expanded into input fields.
5. The `args` group appears under every field that has arguments, also in read-only type trees (shown with plain `• ` markers there).
6. A leaf field that has arguments (a scalar operation such as `Query.postCount(since)`) can still be expanded to reach its `args` group.
7. Typed values are stored per argument path (not per variable name), so a renamed variable (collision prefixes) never swaps values.
8. While a variable is being edited, every key is text input except Ctrl-Q and Ctrl-R. So `q`, `h` and `?` can be typed.
9. Boolean variables cycle with Enter: empty, `true`, `false`, then empty again (required ones skip empty).

## Global Constraints

- The screen shows at most two main panes side by side, with shorter panes below them. Paired panes share one height (70/30 split in every two-pane column).
- Details pane title is always `Details`. Design layout for an operation: left column Details (tree) over Description; right column Query over Variables. For a type: tree and Description only, read-only, no selection.
- SPACE toggles leaf fields and optional arguments; on objects, groups, required arguments and recursive nodes it does nothing. The query is derived from the selection.
- Keys in Design: Up/Down move (in Details: the tree cursor; in Variables: the variable cursor); Right/Left expand/collapse; SPACE toggles (Details only); Tab cycles Details, Query, Variables; Enter edits the highlighted variable (Variables only); Esc goes back (cancels an edit while editing); `q` quits (Ctrl-Q quits from anywhere, also while editing); `?`/F1 (and `h` in Design) open help. Backspace does nothing in Design (it deletes text while editing).
- Query generation: required arguments are always emitted as variables; optional ones only when opted in. Variable named after the argument; on a name clash `<field><Arg>`, then `<name>2`, `<name>3`. Variables keep schema order. A variable with a default is declared `$name: Type = default`.
- Variable rules (spec): `Int` (32-bit) and `Float` are numbers; `Boolean` toggles; `ID` and `String` are free text; enums are picked from a list; custom scalars are strings without validation; lists and input objects are JSON validated against the type (missing required fields, unknown fields, wrong types, unknown enum values). An empty optional variable is omitted; an empty required one is invalid (`required`).
- Values are kept per operation for the whole app session (the existing `sessions` map), cleared by a schema refresh, never written to disk, the cache or logs. The API key rule is unchanged: only `VEGA_GRAPHQL_KEY`, never written anywhere.
- CLI: warnings and errors go to stderr only; stdout (including `--json`) stays machine-readable. `--var` requires `--query`.
- Commits follow AGENTS.md: atomic, `tag: subject` in present tense, subject and body lines at most 72 characters, tags `feat`, `fix`, `ref`, `doc`, `test`, `chore`, no references to plans, specs, tasks or steps, no AI attribution. Every commit passes `just check`.

## File Structure

```
crates/vega-core/
  src/schema.rs                         + InputValue::is_required, TypeRef: PartialEq/Eq
  src/selection.rs                      + Seg::Args/Arg, NodeKind::Required/Optional, Node::has_args/expandable, args group, Selection args, parse_select `arg:`
  src/querygen.rs                       + opted-in optional arguments, defaults, richer VariableDef
  src/variables.rs                      NEW: Values, State, Editor, editor_for, state, display
  src/lib.rs                            + pub mod variables;
  tests/fixtures/design.json            + Query.users, Query.postCount, Float scalar
crates/vega/
  src/cli.rs                            + --var, --select help
  src/main.rs                           + --var handling
  src/output.rs                         query_text/query_json take variable states
  src/tui/app.rs                        + Edit, VariableRow, values, variable cursor, editing
  src/tui/ui.rs                         + argument markers, Variables rows
  tests/cli.rs                          + --var tests
README.md, AGENTS.md, spec              updated in the last task
```

Library docs: use the context7 MCP tools if a ratatui or clap API below does not compile as written; fix the call, keep the behavior. Plan code that was written against the step 1 files may differ from rustfmt's layout of the current files: read each file before editing and keep the behavior.

---

### Task 1: Required-argument helper and a richer design fixture

**Files:**
- Modify: `crates/vega-core/src/schema.rs`, `crates/vega-core/tests/fixtures/design.json`

**Interfaces:**
- Produces: `InputValue::is_required(&self) -> bool` (non-null and no default value); `TypeRef` derives `PartialEq, Eq`.
- Fixture additions used by every later task: `Query.users(role: Role, active: Boolean, floor: Float, ids: [ID!], filter: PostInput, limit: Int! = 10): [User!]!`, `Query.postCount(since: String): Int`, scalar `Float`.

- [ ] **Step 1: Extend the fixture**

In `crates/vega-core/tests/fixtures/design.json`, inside `QueryType`'s `fields`, replace the last field (`version`) so it ends with a comma and is followed by the two new fields:

```json
            { "name": "version", "description": "Server version.", "args": [], "type": { "kind": "SCALAR", "name": "String", "ofType": null }, "isDeprecated": false, "deprecationReason": null },
            { "name": "users", "description": "List users.", "args": [
                { "name": "role", "description": "Only this role.", "type": { "kind": "ENUM", "name": "Role", "ofType": null }, "defaultValue": null },
                { "name": "active", "description": null, "type": { "kind": "SCALAR", "name": "Boolean", "ofType": null }, "defaultValue": null },
                { "name": "floor", "description": null, "type": { "kind": "SCALAR", "name": "Float", "ofType": null }, "defaultValue": null },
                { "name": "ids", "description": null, "type": { "kind": "LIST", "name": null, "ofType": { "kind": "NON_NULL", "name": null, "ofType": { "kind": "SCALAR", "name": "ID", "ofType": null } } }, "defaultValue": null },
                { "name": "filter", "description": null, "type": { "kind": "INPUT_OBJECT", "name": "PostInput", "ofType": null }, "defaultValue": null },
                { "name": "limit", "description": "Page size.", "type": { "kind": "NON_NULL", "name": null, "ofType": { "kind": "SCALAR", "name": "Int", "ofType": null } }, "defaultValue": "10" }
              ], "type": { "kind": "NON_NULL", "name": null, "ofType": { "kind": "LIST", "name": null, "ofType": { "kind": "NON_NULL", "name": null, "ofType": { "kind": "OBJECT", "name": "User", "ofType": null } } } }, "isDeprecated": false, "deprecationReason": null },
            { "name": "postCount", "description": "Number of posts.", "args": [ { "name": "since", "description": null, "type": { "kind": "SCALAR", "name": "String", "ofType": null }, "defaultValue": null } ], "type": { "kind": "SCALAR", "name": "Int", "ofType": null }, "isDeprecated": false, "deprecationReason": null }
```

And add the `Float` scalar next to the other scalars (after the `Int` scalar line):

```json
        { "kind": "SCALAR", "name": "Float", "description": null, "fields": null, "inputFields": null, "enumValues": null, "possibleTypes": null },
```

- [ ] **Step 2: Write the failing test**

Add inside the `tests` module of `crates/vega-core/src/schema.rs`:

```rust
    #[test]
    fn arguments_are_required_when_non_null_without_a_default() {
        use crate::testutil::design_schema;
        let schema = design_schema();
        let field = |name: &str| {
            schema
                .root_type(Root::Query)
                .unwrap()
                .fields
                .as_ref()
                .unwrap()
                .iter()
                .find(|f| f.name == name)
                .unwrap()
                .clone()
        };
        let flags = |name: &str| -> Vec<(String, bool)> {
            field(name)
                .args
                .iter()
                .map(|a| (a.name.clone(), a.is_required()))
                .collect()
        };
        assert_eq!(
            flags("search"),
            [("term".to_string(), true), ("first".to_string(), false)]
        );
        // `limit` is non-null but has a default value, so it is optional.
        assert_eq!(
            flags("users"),
            [
                ("role".to_string(), false),
                ("active".to_string(), false),
                ("floor".to_string(), false),
                ("ids".to_string(), false),
                ("filter".to_string(), false),
                ("limit".to_string(), false)
            ]
        );
    }
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p vega-core arguments_are_required`
Expected: FAIL to compile (`no method named is_required`).

- [ ] **Step 4: Implement**

In `schema.rs`, add to the existing `impl InputValue` block:

```rust
    /// Required means non-null with no default value: the server cannot do without it.
    pub fn is_required(&self) -> bool {
        self.ty.kind == TypeKind::NonNull && self.default_value.is_none()
    }
```

And change the derive of `TypeRef` to `#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]`.

- [ ] **Step 5: Run the tests to verify everything passes**

Run: `cargo test --workspace`
Expected: all pass (the new fixture fields must not break any existing test; if a search-based test now matches a new field, report which one and adapt the search text, not the fixture).

- [ ] **Step 6: Check and commit**

Run: `just fix && just check`
Expected: exit code 0.

```bash
git add crates/vega-core/src/schema.rs crates/vega-core/tests/fixtures/design.json
git commit -m "feat: detect required arguments and extend the design fixture"
```

---

### Task 2: Arguments in the schema tree

**Files:**
- Modify: `crates/vega-core/src/selection.rs`, `crates/vega/src/tui/app.rs`, `crates/vega/src/tui/ui.rs`

**Interfaces:**
- Consumes: `InputValue::is_required` (Task 1).
- Produces (`vega_core::selection`):
  - `Seg` gains `Args` (the `args` group of a field) and `Arg(String)` (one argument in the group).
  - `NodeKind` gains `Required` (always included, fixed) and `Optional` (opt in with SPACE).
  - `Node` gains `has_args: bool` and `Node::expandable(&self) -> bool` (`Composite`, or a `Leaf` that has arguments).
  - `children`: a node whose field has arguments lists an `args` group first (`label "args"`, `Composite`, path `[.., Args]`); children of that group are one node per argument (`Required`/`Optional`, path `[.., Args, Arg(name)]`, label `InputValue::render`).
  - `Selection`: `arg_included(&self, &[Seg]) -> bool`, `insert_arg(&mut self, Path)`; `toggle_node` toggles `Optional` nodes into the argument set and ignores `Required`; `is_empty` is true only when no leaf and no argument is selected.
  - `SelectError::RequiredArgument(String)`; `parse_select` accepts `arg:<name>` segments.
- TUI: `NodeKind` markers, `App::is_selected` also reports opted-in arguments, expanding works on any `expandable()` node.

- [ ] **Step 1: Write the failing tests (selection.rs)**

Add inside the `tests` module of `crates/vega-core/src/selection.rs`:

```rust
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
```

Update two existing tests in the same module, because the `args` group now comes first:

- In `interfaces_list_shared_fields_then_branches`, the expected labels become `["args", "id: ID!", "on User", "on Post"]`, the kinds `[Composite, Leaf, Composite, Composite]`, `kids[1].path` becomes `kids[2].path` (still `vec![branch("User")]`).
- In `unions_list_only_branches`, the expected labels become `["args", "on User", "on Post"]`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p vega-core selection`
Expected: FAIL to compile (`no variant Args`, ...).

- [ ] **Step 3: Implement in selection.rs**

`Seg` becomes:

```rust
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
```

`NodeKind` and `Node` become:

```rust
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
```

In `root_node`, set `has_args: !field.args.is_empty()` for the operation node and `has_args: false` for the type node. In `children`, every existing `Node { .. }` literal gets `has_args`: fields use `!field.args.is_empty()`, input fields, enum values and branches use `false`.

In `resolve`, make the `match seg` reject argument segments:

```rust
        ty = match seg {
            Seg::Field(name) => schema.type_by_name(field_type_name(ty, name)?)?,
            Seg::Branch(name) => schema.type_by_name(name)?,
            Seg::Args | Seg::Arg(_) => return None,
        };
```

Add above `children`:

```rust
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
```

At the top of `children`, before the `resolve` call:

```rust
    if path.last() == Some(&Seg::Args) {
        return arg_nodes(schema, root, path);
    }
```

and right after `let mut nodes = Vec::new();`:

```rust
    if field_at(schema, root, path).is_some_and(|field| !field.args.is_empty()) {
        nodes.push(Node {
            path: join(path, Seg::Args),
            label: "args".to_string(),
            kind: NodeKind::Composite,
            description: "Arguments of this field".to_string(),
            has_args: false,
        });
    }
```

In `push_rows`, `let is_expanded = node.kind == NodeKind::Composite && expanded.contains(&node.path);` becomes:

```rust
    let is_expanded = node.expandable() && expanded.contains(&node.path);
```

`Selection` becomes:

```rust
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
}
```

Add the error variant to `SelectError`:

```rust
    #[error("`{0}` is a required argument: it is always included")]
    RequiredArgument(String),
```

Replace the body of the `for item in ...` loop in `parse_select` with:

```rust
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
```

- [ ] **Step 4: Fix the TUI so it compiles and keeps working**

`crates/vega/src/tui/app.rs`:
- In `expand_or_step_in`, replace `if row.node.kind != NodeKind::Composite { return; }` with `if !row.node.expandable() { return; }`. Remove the `NodeKind` import if it becomes unused.
- `is_selected` becomes:

```rust
    /// True when the leaf at `path` is selected, or the optional argument at it is opted in.
    pub fn is_selected(&self, path: &[Seg]) -> bool {
        self.design.as_ref().is_some_and(|design| {
            design.selection.contains(path) || design.selection.arg_included(path)
        })
    }
```

`crates/vega/src/tui/ui.rs`: replace `tree_item` with:

```rust
fn tree_item(app: &App, row: &Row) -> ListItem<'static> {
    let editable = app
        .design
        .as_ref()
        .is_some_and(|design| !design.read_only());
    let checkbox = if app.is_selected(&row.node.path) {
        "☑ "
    } else {
        "☐ "
    };
    let marker = match row.node.kind {
        NodeKind::Composite if row.expanded => "▾ ",
        NodeKind::Composite => "▸ ",
        NodeKind::Recursive => "↻ ",
        NodeKind::Required if editable => "▪ ",
        NodeKind::Leaf | NodeKind::Optional if editable => checkbox,
        NodeKind::Leaf | NodeKind::Required | NodeKind::Optional => "• ",
    };
    // A leaf that has arguments also shows whether its `args` group is open.
    let fold = match (row.node.kind, row.node.has_args) {
        (NodeKind::Leaf, true) if row.expanded => "▾ ",
        (NodeKind::Leaf, true) => "▸ ",
        _ => "",
    };
    ListItem::new(Line::raw(format!(
        "{}{fold}{marker}{}",
        "  ".repeat(row.depth),
        row.node.label
    )))
}
```

Existing TUI tests that walk into an expanded field with arguments now meet the `args` group first. Update them (and only them) by adding one `press(.., KeyCode::Down)`:
- `crates/vega/src/tui/app.rs`, `selecting_a_nested_leaf_includes_its_ancestors_and_variables`: after `press(&mut app, KeyCode::Right); // expand posts` the next `Down` lands on `args`; add a second `Down` so the cursor reaches `id`.
- `crates/vega/src/tui/ui.rs`, `required_arguments_are_listed_as_variables`: add a second `press_key(&mut app, KeyCode::Down)` before the Space.

Run `cargo test --workspace` and adapt any other test that fails only because an `args` group row now exists; list each in your report.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --workspace`
Expected: all pass.

- [ ] **Step 6: Check and commit**

Run: `just fix && just check`
Expected: exit code 0.

```bash
git add crates/vega-core/src/selection.rs crates/vega/src/tui/app.rs crates/vega/src/tui/ui.rs
git commit -m "feat: show arguments as a group in the schema tree"
```

---

### Task 3: Opted-in arguments in generated queries

**Files:**
- Modify: `crates/vega-core/src/querygen.rs`, `crates/vega/src/output.rs`, `crates/vega/src/tui/app.rs`, `crates/vega/src/tui/ui.rs`

**Interfaces:**
- Consumes: `Selection::arg_included`, `Seg::Args/Arg`, `InputValue::is_required`, `TypeRef: Clone`.
- Produces: `VariableDef { name, ty, required, default: Option<String>, path: Path, type_ref: TypeRef }` (`path` and `type_ref` are `#[serde(skip)]`; `default` is skipped when `None`; JSON gains `required`). `generate` emits every required argument plus the opted-in optional ones, declares variables with their default (`$limit: Int! = 10`).

- [ ] **Step 1: Write the failing tests (querygen.rs)**

In the `tests` module of `crates/vega-core/src/querygen.rs`, replace the `var` helper with:

```rust
    fn pairs(q: &GeneratedQuery) -> Vec<(&str, &str)> {
        q.variables
            .iter()
            .map(|v| (v.name.as_str(), v.ty.as_str()))
            .collect()
    }
```

and change the four assertions that compare `q.variables` to `[var(..)]` into `assert_eq!(pairs(&q), [("first", "Int!")]);`, `[("term", "String!")]`, `[("id", "ID!"), ("postId", "ID!")]` and `[("input", "PostInput!")]` respectively. In `serializes_for_the_command_line` also add `assert_eq!(json["variables"][0]["required"], true);` and `assert!(json["variables"][0].get("path").is_none());`.

Add these tests:

```rust
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
```

In `crates/vega/src/output.rs` tests, add the helper and use it in the two `query_text` tests in place of the `VariableDef { name, ty }` literals (`variables: vec![id_variable()]`):

```rust
    fn id_variable() -> vega_core::querygen::VariableDef {
        use vega_core::schema::{TypeKind, TypeRef};
        vega_core::querygen::VariableDef {
            name: "id".into(),
            ty: "ID!".into(),
            required: true,
            default: None,
            path: Vec::new(),
            type_ref: TypeRef {
                kind: TypeKind::Scalar,
                name: Some("ID".into()),
                of_type: None,
            },
        }
    }
```

- [ ] **Step 2: Write the failing tests (TUI)**

In `crates/vega/src/tui/app.rs` tests add:

```rust
    #[test]
    fn space_toggles_optional_arguments_and_ignores_required_ones() {
        let mut app = design_app();
        type_text(&mut app, "search");
        assert_eq!(app.hits[0].path, "Query.search");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down); // args group
        press(&mut app, KeyCode::Right); // expand it
        press(&mut app, KeyCode::Down); // term (required)
        press(&mut app, KeyCode::Char(' '));
        assert!(!app.is_selected(&[Seg::Args, Seg::Arg("term".to_string())]));
        press(&mut app, KeyCode::Down); // first (optional)
        press(&mut app, KeyCode::Char(' '));
        assert!(app.is_selected(&[Seg::Args, Seg::Arg("first".to_string())]));
        press(&mut app, KeyCode::Char(' '));
        assert!(!app.is_selected(&[Seg::Args, Seg::Arg("first".to_string())]));
    }

    #[test]
    fn opted_in_arguments_appear_in_the_generated_query() {
        let mut app = design_app();
        type_text(&mut app, "users");
        assert_eq!(app.hits[0].path, "Query.users");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' ')); // id
        press(&mut app, KeyCode::Up); // args group
        press(&mut app, KeyCode::Right); // expand it
        press(&mut app, KeyCode::Down); // role
        press(&mut app, KeyCode::Char(' '));
        assert_eq!(
            query_text(&app),
            "query Users($role: Role) {\n  users(role: $role) {\n    id\n  }\n}"
        );
    }
```

In `crates/vega/src/tui/ui.rs` tests add:

```rust
    #[test]
    fn arguments_show_as_fixed_and_checkbox_rows() {
        use ratatui::crossterm::event::KeyCode;
        let mut app = design_app_on("node");
        press_key(&mut app, KeyCode::Down); // args group
        press_key(&mut app, KeyCode::Right);
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("▾ args"));
        assert!(text.contains("▪ id: ID!"));

        let mut app = design_app_on("users");
        press_key(&mut app, KeyCode::Down);
        press_key(&mut app, KeyCode::Right);
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("☐ role: Role"));
        assert!(text.contains("☐ limit: Int! = 10"));
        press_key(&mut app, KeyCode::Down);
        press_key(&mut app, KeyCode::Char(' '));
        assert!(screen(&render(&app, 1_000)).contains("☑ role: Role"));
    }
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --workspace`
Expected: FAIL to compile (`no field required on VariableDef`, ...).

- [ ] **Step 4: Implement in querygen.rs**

Imports become `use crate::schema::{Field, FullType, InputValue, Root, Schema, TypeKind, TypeRef};` and `VariableDef` becomes:

```rust
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
```

Delete the private `is_required` function. Add:

```rust
fn arg_path(field_path: &[Seg], name: &str) -> Path {
    let mut path = field_path.to_vec();
    path.push(Seg::Args);
    path.push(Seg::Arg(name.to_string()));
    path
}
```

Replace `call` and `variable_for` in `impl Builder`:

```rust
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
```

In `selection_set`, the call becomes `let call = self.call(field, path);` (where `path` is the `&mut Path` that already holds the field: pass `&path[..]`). In `generate`, `let call = builder.call(field, &[]);`. In the header, the definitions become:

```rust
        let defs: Vec<String> = builder
            .variables
            .iter()
            .map(|v| match &v.default {
                Some(default) => format!("${}: {} = {default}", v.name, v.ty),
                None => format!("${}: {}", v.name, v.ty),
            })
            .collect();
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --workspace`
Expected: all pass.

- [ ] **Step 6: Check and commit**

Run: `just fix && just check`
Expected: exit code 0.

```bash
git add crates/vega-core/src/querygen.rs crates/vega/src/output.rs crates/vega/src/tui/app.rs crates/vega/src/tui/ui.rs
git commit -m "feat: include opted-in optional arguments in generated queries"
```

---

### Task 4: Typed variable values

**Files:**
- Create: `crates/vega-core/src/variables.rs`
- Modify: `crates/vega-core/src/lib.rs`

**Interfaces:**
- Consumes: `VariableDef` (Task 3), `Schema`, `TypeRef`, `TypeKind`, `InputValue::is_required`, `Path`.
- Produces (`vega_core::variables`):
  - `type Values = BTreeMap<Path, String>` (what the user typed, keyed by argument path)
  - `enum State { Unset, Value(serde_json::Value), Invalid(String) }`
  - `enum Editor { Text, Toggle, Choice(Vec<String>), Json }`
  - `editor_for(&Schema, &TypeRef) -> Editor`
  - `state(&Schema, &VariableDef, Option<&str>) -> State` (empty text: `Invalid("required")` for a required variable, `Unset` otherwise)
  - `display(&Value) -> String` (strings without quotes, everything else compact JSON)

- [ ] **Step 1: Write the failing tests**

Add `pub mod variables;` to `crates/vega-core/src/lib.rs` (alphabetical). Create `crates/vega-core/src/variables.rs` containing only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::querygen::{generate, GeneratedQuery};
    use crate::selection::{parse_select, OperationRef};
    use crate::testutil::design_schema;
    use serde_json::json;

    fn users() -> (Schema, GeneratedQuery) {
        let schema = design_schema();
        let op = OperationRef::parse(&schema, "Query.users").unwrap();
        let selection = parse_select(
            &schema,
            &op,
            "id,arg:role,arg:active,arg:floor,arg:ids,arg:filter,arg:limit",
        )
        .unwrap();
        let query = generate(&schema, &op, &selection).unwrap();
        (schema, query)
    }

    fn state_of(name: &str, raw: &str) -> State {
        let (schema, query) = users();
        let def = query.variables.iter().find(|v| v.name == name).unwrap();
        state(&schema, def, Some(raw))
    }

    fn invalid(reason: &str) -> State {
        State::Invalid(reason.to_string())
    }

    #[test]
    fn scalars_and_enums_are_parsed_by_type() {
        let cases: Vec<(&str, &str, State)> = vec![
            ("active", "true", State::Value(json!(true))),
            ("active", "false", State::Value(json!(false))),
            ("active", "yes", invalid("expected true or false")),
            ("floor", "1.5", State::Value(json!(1.5))),
            ("floor", "-2", State::Value(json!(-2.0))),
            ("floor", "abc", invalid("expected a number")),
            ("limit", "5", State::Value(json!(5))),
            ("limit", "5.5", invalid("expected a whole number")),
            ("limit", "99999999999", invalid("expected a whole number")),
            ("role", "ADMIN", State::Value(json!("ADMIN"))),
            ("role", "root", invalid("`root` is not a value of Role")),
        ];
        for (name, raw, expected) in cases {
            assert_eq!(state_of(name, raw), expected, "{name} = {raw}");
        }
    }

    #[test]
    fn lists_and_input_objects_are_json_validated_against_the_type() {
        let cases: Vec<(&str, &str, State)> = vec![
            ("ids", r#"["1","2"]"#, State::Value(json!(["1", "2"]))),
            ("ids", "[1, 2]", State::Value(json!([1, 2]))),
            ("ids", r#""x""#, invalid("expected a list")),
            ("ids", "[null]", invalid("[0]: must not be null")),
            ("filter", r#"{"title":"x"}"#, State::Value(json!({"title": "x"}))),
            ("filter", "{}", invalid("missing field `title`")),
            ("filter", r#"{"title":"x","z":1}"#, invalid("unknown field `z`")),
            ("filter", r#"{"title":1}"#, invalid("title: expected a string")),
            ("filter", "null", State::Value(serde_json::Value::Null)),
        ];
        for (name, raw, expected) in cases {
            assert_eq!(state_of(name, raw), expected, "{name} = {raw}");
        }
        assert!(matches!(
            state_of("ids", "["),
            State::Invalid(reason) if reason.starts_with("not valid JSON")
        ));
    }

    #[test]
    fn empty_text_leaves_an_optional_variable_unset() {
        assert_eq!(state_of("role", ""), State::Unset);
        assert_eq!(state_of("role", "   "), State::Unset);
        // `limit` is non-null but has a default, so it is optional too.
        assert_eq!(state_of("limit", ""), State::Unset);
    }

    #[test]
    fn required_variables_cannot_be_empty_and_text_keeps_its_spaces() {
        let schema = design_schema();
        let op = OperationRef::parse(&schema, "Query.node").unwrap();
        let selection = parse_select(&schema, &op, "id").unwrap();
        let query = generate(&schema, &op, &selection).unwrap();
        let def = &query.variables[0];
        assert_eq!(state(&schema, def, None), invalid("required"));
        assert_eq!(state(&schema, def, Some("")), invalid("required"));
        assert_eq!(state(&schema, def, Some("42")), State::Value(json!("42")));
        assert_eq!(
            state(&schema, def, Some(" a b ")),
            State::Value(json!(" a b "))
        );
    }

    #[test]
    fn editors_follow_the_type() {
        let (schema, query) = users();
        let editor = |name: &str| {
            let def = query.variables.iter().find(|v| v.name == name).unwrap();
            editor_for(&schema, &def.type_ref)
        };
        assert_eq!(
            editor("role"),
            Editor::Choice(vec!["ADMIN".to_string(), "VIEWER".to_string()])
        );
        assert_eq!(editor("active"), Editor::Toggle);
        assert_eq!(editor("floor"), Editor::Text);
        assert_eq!(editor("limit"), Editor::Text);
        assert_eq!(editor("ids"), Editor::Json);
        assert_eq!(editor("filter"), Editor::Json);
    }

    #[test]
    fn display_shows_strings_without_quotes() {
        assert_eq!(display(&json!("42")), "42");
        assert_eq!(display(&json!(7)), "7");
        assert_eq!(display(&json!({"a": 1})), r#"{"a":1}"#);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p vega-core variables`
Expected: FAIL to compile (`cannot find function state`, ...).

- [ ] **Step 3: Implement**

Insert above `#[cfg(test)]` in `variables.rs`:

```rust
//! Typed values for the variables of a generated query: which editor a type needs and
//! whether the text typed so far is a valid value for it. Pure data: no terminal code.

use std::collections::BTreeMap;

use serde_json::{Number, Value};

use crate::querygen::VariableDef;
use crate::schema::{Schema, TypeKind, TypeRef};
use crate::selection::Path;

/// What the user typed for each variable, keyed by where its argument sits in the tree.
pub type Values = BTreeMap<Path, String>;

#[derive(Debug, Clone, PartialEq)]
pub enum State {
    /// Nothing typed and nothing needed: the variable is left out of the request.
    Unset,
    Value(Value),
    /// Why the text typed so far cannot be sent.
    Invalid(String),
}

/// How the user edits a value of a given type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Editor {
    /// Free text (numbers, IDs, strings, custom scalars).
    Text,
    /// `Boolean`: cycles with Enter.
    Toggle,
    /// An enum: pick one of its values.
    Choice(Vec<String>),
    /// A list or an input object: one JSON value.
    Json,
}

fn strip_non_null(ty: &TypeRef) -> &TypeRef {
    match (ty.kind, ty.of_type.as_deref()) {
        (TypeKind::NonNull, Some(inner)) => inner,
        _ => ty,
    }
}

fn enum_names(schema: &Schema, name: &str) -> Vec<String> {
    schema
        .type_by_name(name)
        .and_then(|ty| ty.enum_values.as_ref())
        .map(|values| values.iter().map(|value| value.name.clone()).collect())
        .unwrap_or_default()
}

pub fn editor_for(schema: &Schema, ty: &TypeRef) -> Editor {
    let ty = strip_non_null(ty);
    match ty.kind {
        TypeKind::List | TypeKind::InputObject => Editor::Json,
        TypeKind::Enum => Editor::Choice(enum_names(schema, ty.name.as_deref().unwrap_or_default())),
        TypeKind::Scalar if ty.name.as_deref() == Some("Boolean") => Editor::Toggle,
        _ => Editor::Text,
    }
}

/// The state of `def` given the text typed for it (`None` when nothing was typed).
pub fn state(schema: &Schema, def: &VariableDef, raw: Option<&str>) -> State {
    let text = raw.unwrap_or_default();
    if text.trim().is_empty() {
        return if def.required {
            State::Invalid("required".to_string())
        } else {
            State::Unset
        };
    }
    match parse(schema, &def.type_ref, text) {
        Ok(value) => State::Value(value),
        Err(reason) => State::Invalid(reason),
    }
}

/// A value as shown to the user: strings without quotes, everything else compact JSON.
pub fn display(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn parse(schema: &Schema, ty: &TypeRef, text: &str) -> Result<Value, String> {
    let ty = strip_non_null(ty);
    match ty.kind {
        TypeKind::List | TypeKind::InputObject => {
            let value: Value =
                serde_json::from_str(text.trim()).map_err(|e| format!("not valid JSON: {e}"))?;
            validate(schema, ty, &value)?;
            Ok(value)
        }
        _ => parse_scalar(schema, ty, text),
    }
}

fn parse_scalar(schema: &Schema, ty: &TypeRef, text: &str) -> Result<Value, String> {
    let trimmed = text.trim();
    let name = ty.name.as_deref().unwrap_or_default();
    match (ty.kind, name) {
        (TypeKind::Enum, _) => {
            if enum_names(schema, name).iter().any(|n| n == trimmed) {
                Ok(Value::String(trimmed.to_string()))
            } else {
                Err(format!("`{trimmed}` is not a value of {name}"))
            }
        }
        (_, "Int") => trimmed
            .parse::<i32>()
            .map(Value::from)
            .map_err(|_| "expected a whole number".to_string()),
        (_, "Float") => trimmed
            .parse::<f64>()
            .ok()
            .and_then(Number::from_f64)
            .map(Value::Number)
            .ok_or_else(|| "expected a number".to_string()),
        (_, "Boolean") => match trimmed {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => Err("expected true or false".to_string()),
        },
        // ID, String and custom scalars: the text as typed.
        _ => Ok(Value::String(text.to_string())),
    }
}

fn validate(schema: &Schema, ty: &TypeRef, value: &Value) -> Result<(), String> {
    match ty.kind {
        TypeKind::NonNull => {
            if value.is_null() {
                return Err("must not be null".to_string());
            }
            match ty.of_type.as_deref() {
                Some(inner) => validate(schema, inner, value),
                None => Ok(()),
            }
        }
        _ if value.is_null() => Ok(()),
        TypeKind::List => {
            let Some(items) = value.as_array() else {
                return Err("expected a list".to_string());
            };
            let Some(inner) = ty.of_type.as_deref() else {
                return Ok(());
            };
            for (index, item) in items.iter().enumerate() {
                validate(schema, inner, item).map_err(|reason| format!("[{index}]: {reason}"))?;
            }
            Ok(())
        }
        TypeKind::InputObject => validate_object(
            schema,
            ty.name.as_deref().unwrap_or_default(),
            value,
        ),
        _ => validate_scalar(schema, ty, value),
    }
}

fn validate_object(schema: &Schema, name: &str, value: &Value) -> Result<(), String> {
    let Some(object) = value.as_object() else {
        return Err(format!("expected an object of type {name}"));
    };
    let Some(fields) = schema
        .type_by_name(name)
        .and_then(|ty| ty.input_fields.as_ref())
    else {
        return Ok(());
    };
    for key in object.keys() {
        if !fields.iter().any(|field| &field.name == key) {
            return Err(format!("unknown field `{key}`"));
        }
    }
    for field in fields {
        match object.get(&field.name) {
            Some(inner) => validate(schema, &field.ty, inner)
                .map_err(|reason| format!("{}: {reason}", field.name))?,
            None if field.is_required() => return Err(format!("missing field `{}`", field.name)),
            None => {}
        }
    }
    Ok(())
}

fn validate_scalar(schema: &Schema, ty: &TypeRef, value: &Value) -> Result<(), String> {
    let name = ty.name.as_deref().unwrap_or_default();
    let ok = |condition: bool, reason: &str| {
        if condition {
            Ok(())
        } else {
            Err(reason.to_string())
        }
    };
    match (ty.kind, name) {
        (TypeKind::Enum, _) => match value.as_str() {
            Some(text) if enum_names(schema, name).iter().any(|n| n == text) => Ok(()),
            Some(text) => Err(format!("`{text}` is not a value of {name}")),
            None => Err(format!("expected a value of {name}")),
        },
        (_, "Int") => ok(
            value.as_i64().is_some_and(|n| i32::try_from(n).is_ok()),
            "expected a whole number",
        ),
        (_, "Float") => ok(value.is_number(), "expected a number"),
        (_, "Boolean") => ok(value.is_boolean(), "expected true or false"),
        (_, "String") => ok(value.is_string(), "expected a string"),
        // IDs may be written as strings or whole numbers.
        (_, "ID") => ok(
            value.is_string() || value.as_i64().is_some(),
            "expected an ID",
        ),
        // Custom scalars are not validated.
        _ => Ok(()),
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p vega-core variables`
Expected: `test result: ok. 6 passed`.

- [ ] **Step 5: Check and commit**

Run: `just fix && just check`
Expected: exit code 0.

```bash
git add crates/vega-core/src/lib.rs crates/vega-core/src/variables.rs
git commit -m "feat: validate variable values by type"
```

---

### Task 5: `--var` on the command line

**Files:**
- Modify: `crates/vega/src/cli.rs`, `crates/vega/src/output.rs`, `crates/vega/src/main.rs`, `crates/vega/tests/cli.rs`

**Interfaces:**
- Consumes: `variables::{state, display, State, Values}`, `VariableDef::path`, `parse_select` with `arg:`.
- Produces: `Cli.vars: Vec<String>` (`--var NAME=VALUE`, repeatable, requires `--query`); `output::query_text(&GeneratedQuery, &[State]) -> String`; `output::query_json(&GeneratedQuery, &[State]) -> anyhow::Result<serde_json::Value>`.

Output (text): the query, then `Variables:` with one `  $name: Type` line per variable, with ` = value` appended when a valid value was given. With `--json`: the `GeneratedQuery` JSON where each variable also gets `"value"` when it has one. A value that does not validate is an error (exit non-zero, message on stderr, nothing on stdout). A required variable with no `--var` is only listed (running is step 3).

- [ ] **Step 1: Write the failing tests**

In the `tests` module of `crates/vega/src/output.rs`, replace the two `query_text` tests with these (they use the `id_variable` helper from Task 3):

```rust
    #[test]
    fn query_text_lists_the_variables_after_the_query() {
        let query = GeneratedQuery {
            operation_name: "UserRead".into(),
            text: "query UserRead($id: ID!) {\n  userRead(id: $id) {\n    id\n  }\n}".into(),
            variables: vec![id_variable()],
        };
        assert_eq!(
            query_text(&query, &[State::Unset]),
            "query UserRead($id: ID!) {\n  userRead(id: $id) {\n    id\n  }\n}\n\nVariables:\n  $id: ID!\n"
        );
    }

    #[test]
    fn query_text_shows_the_value_of_a_variable_that_has_one() {
        let query = GeneratedQuery {
            operation_name: "UserRead".into(),
            text: "query UserRead($id: ID!) {\n  userRead(id: $id) {\n    id\n  }\n}".into(),
            variables: vec![id_variable()],
        };
        let states = [State::Value(serde_json::json!("42"))];
        assert!(query_text(&query, &states).ends_with("Variables:\n  $id: ID! = 42\n"));
    }

    #[test]
    fn query_text_without_variables_is_just_the_query() {
        let query = GeneratedQuery {
            operation_name: "Version".into(),
            text: "query Version {\n  version\n}".into(),
            variables: Vec::new(),
        };
        assert_eq!(query_text(&query, &[]), "query Version {\n  version\n}\n");
    }

    #[test]
    fn query_json_adds_the_value_to_variables_that_have_one() {
        let query = GeneratedQuery {
            operation_name: "UserRead".into(),
            text: "query UserRead($id: ID!) {\n  userRead(id: $id) {\n    id\n  }\n}".into(),
            variables: vec![id_variable()],
        };
        let json = query_json(&query, &[State::Value(serde_json::json!("42"))]).unwrap();
        assert_eq!(json["variables"][0]["value"], "42");
        let json = query_json(&query, &[State::Unset]).unwrap();
        assert!(json["variables"][0].get("value").is_none());
    }
```

Add to `crates/vega/tests/cli.rs` (the seeded schema is the phase 1 fixture: `Query.userRead(id: ID!)`, `Query.userSearch(term: String)`, `Mutation.userCreate(input: UserInput!)`):

```rust
#[test]
fn var_sets_the_value_shown_next_to_the_variable() {
    let cache = seeded_cache(60);
    let out = vega(
        &cache,
        &["--query", "Query.userRead", "--select", "id,name", "--var", "id=42"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "query UserRead($id: ID!) {\n  userRead(id: $id) {\n    id\n    name\n  }\n}\n\nVariables:\n  $id: ID! = 42\n"
    );
    assert_eq!(stderr(&out), "");
}

#[test]
fn var_value_appears_in_the_json_output() {
    let cache = seeded_cache(60);
    let out = vega(
        &cache,
        &["--query", "Query.userRead", "--select", "id", "--var", "id=42", "--json"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(json["variables"][0]["name"], "id");
    assert_eq!(json["variables"][0]["value"], "42");
}

#[test]
fn select_can_opt_an_optional_argument_in() {
    let cache = seeded_cache(60);
    let out = vega(
        &cache,
        &["--query", "Query.userSearch", "--select", "id,arg:term", "--var", "term=bob"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).starts_with(
        "query UserSearch($term: String) {\n  userSearch(term: $term) {\n    id\n  }\n}"
    ));
    assert!(stdout(&out).contains("$term: String = bob"));
}

#[test]
fn var_for_an_unknown_variable_fails_with_a_message() {
    let cache = seeded_cache(60);
    let out = vega(
        &cache,
        &["--query", "Query.userRead", "--select", "id", "--var", "nope=1"],
    );
    assert!(!out.status.success());
    assert!(stderr(&out).contains("no such variable"));
    assert_eq!(stdout(&out), "");
}

#[test]
fn var_with_an_invalid_value_fails_and_prints_nothing() {
    let cache = seeded_cache(60);
    let out = vega(
        &cache,
        &["--query", "Mutation.userCreate", "--select", "id", "--var", "input=notjson"],
    );
    assert!(!out.status.success());
    assert!(stderr(&out).contains("not valid JSON"));
    assert_eq!(stdout(&out), "");
}

#[test]
fn var_requires_query() {
    let cache = seeded_cache(60);
    let out = vega(&cache, &["--var", "id=1"]);
    assert!(!out.status.success());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p vega`
Expected: FAIL (`query_text` takes one argument; `--var` is unknown).

- [ ] **Step 3: Implement**

`crates/vega/src/output.rs`: add `use vega_core::variables::{self, State};` and replace `query_text`:

```rust
/// The query text, then the variables it needs (if any), one per line, each with its
/// value when a valid one was given.
pub fn query_text(query: &GeneratedQuery, states: &[State]) -> String {
    let mut out = query.text.clone();
    out.push('\n');
    if !query.variables.is_empty() {
        out.push_str("\nVariables:\n");
        for (index, variable) in query.variables.iter().enumerate() {
            out.push_str(&format!("  ${}: {}", variable.name, variable.ty));
            if let Some(State::Value(value)) = states.get(index) {
                out.push_str(&format!(" = {}", variables::display(value)));
            }
            out.push('\n');
        }
    }
    out
}

/// The generated query as JSON; each variable that has a valid value also gets `value`.
pub fn query_json(
    query: &GeneratedQuery,
    states: &[State],
) -> anyhow::Result<serde_json::Value> {
    let mut json = serde_json::to_value(query)?;
    if let Some(entries) = json["variables"].as_array_mut() {
        for (entry, state) in entries.iter_mut().zip(states) {
            if let (State::Value(value), Some(object)) = (state, entry.as_object_mut()) {
                object.insert("value".to_string(), value.clone());
            }
        }
    }
    Ok(json)
}
```

`crates/vega/src/cli.rs`: update the `--select` doc to `Fields to select with --query: comma-separated paths such as id,friends.name (union or interface branches as on:Type, optional arguments as arg:name, e.g. friends.arg:first)` and add after `select`:

```rust
    /// Set a variable of the --query operation, for example --var id=42 (repeatable)
    #[arg(long = "var", value_name = "NAME=VALUE", requires = "query")]
    pub vars: Vec<String>,
```

`crates/vega/src/main.rs`: add imports `use vega_core::querygen::GeneratedQuery;` and `use vega_core::variables::{self, State, Values};`, pass `&cli.vars` to `print_query`, and replace `print_query` with:

```rust
fn print_query(
    schema: &Schema,
    target: &str,
    select: Option<&str>,
    vars: &[String],
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
    let values = parse_vars(&query, vars)?;
    let mut states = Vec::new();
    for def in &query.variables {
        let typed = values.get(&def.path).map(String::as_str);
        let state = variables::state(schema, def, typed);
        // A missing required value is only listed (running is what needs it).
        if let (State::Invalid(reason), Some(_)) = (&state, typed) {
            anyhow::bail!("variable ${}: {reason}", def.name);
        }
        states.push(state);
    }
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&output::query_json(&query, &states)?)?
        );
    } else {
        print!("{}", output::query_text(&query, &states));
    }
    Ok(())
}

/// Matches each `NAME=VALUE` with the variable of that name in the generated query.
fn parse_vars(query: &GeneratedQuery, vars: &[String]) -> anyhow::Result<Values> {
    let mut values = Values::new();
    for item in vars {
        let (name, value) = item
            .split_once('=')
            .with_context(|| format!("--var `{item}`: expected NAME=VALUE"))?;
        let name = name.trim_start_matches('$');
        let def = query
            .variables
            .iter()
            .find(|variable| variable.name == name)
            .with_context(|| {
                let known: Vec<&str> = query.variables.iter().map(|v| v.name.as_str()).collect();
                format!(
                    "--var `{name}`: no such variable in this query (variables: {})",
                    if known.is_empty() { "none".to_string() } else { known.join(", ") }
                )
            })?;
        values.insert(def.path.clone(), value.to_string());
    }
    Ok(values)
}
```

Update the existing call site of `print_query` to pass `&cli.vars`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p vega`
Expected: all pass. Confirm `vega --help` lists `--var` and the updated `--select` text.

- [ ] **Step 5: Check and commit**

Run: `just fix && just check`
Expected: exit code 0.

```bash
git add crates/vega/src/cli.rs crates/vega/src/output.rs crates/vega/src/main.rs crates/vega/tests/cli.rs
git commit -m "feat: add --var to set variables when printing a query"
```

---

### Task 6: Variables pane rows

**Files:**
- Modify: `crates/vega/src/tui/app.rs`, `crates/vega/src/tui/ui.rs`

**Interfaces:**
- Consumes: `variables::{state, State, Values}`, `VariableDef`.
- Produces (`crate::tui::app`):
  - `DesignState` gains `values: Values`, `var_cursor: usize`, `edit: Option<Edit>` (the last is used by Task 7; define it here so the pane can render it).
  - `enum Edit { Text(String), Choice { options: Vec<String>, index: usize } }`
  - `struct VariableRow { def: VariableDef, raw: String, state: State }`
  - `App::variable_rows(&self) -> Vec<VariableRow>` (empty outside an operation design or while the selection is empty)
  - With Variables focused, Up/Down move `var_cursor` (clamped).
- Rendering: each row reads `$name: Type = value`; an empty optional shows a dim `(omitted)` (or `(default X)`); an invalid or missing required value is red with `← reason`. The highlighted row is reversed only while Variables has focus.

- [ ] **Step 1: Write the failing tests**

In `crates/vega/src/tui/app.rs` tests add:

```rust
    /// Opens Query.node with `id` selected and the Variables pane focused.
    fn node_variables(app: &mut App) {
        type_text(app, "node");
        assert_eq!(app.hits[0].path, "Query.node");
        press(app, KeyCode::Enter);
        press(app, KeyCode::Down);
        press(app, KeyCode::Down);
        press(app, KeyCode::Char(' ')); // id
        press(app, KeyCode::Tab);
        press(app, KeyCode::Tab);
    }

    #[test]
    fn variable_rows_follow_the_included_arguments() {
        let mut app = design_app();
        open_me(&mut app);
        assert!(app.variable_rows().is_empty());

        let mut app = design_app();
        node_variables(&mut app);
        let rows = app.variable_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].def.name, "id");
        assert_eq!(rows[0].raw, "");
        assert_eq!(
            rows[0].state,
            vega_core::variables::State::Invalid("required".to_string())
        );
    }

    #[test]
    fn up_and_down_move_the_variable_cursor_when_variables_has_focus() {
        use super::Focus;
        let mut app = design_app();
        type_text(&mut app, "users");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' ')); // id
        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Right);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' ')); // role
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' ')); // active
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.design.as_ref().unwrap().focus, Focus::Variables);
        assert_eq!(app.variable_rows().len(), 2);

        let tree_cursor = app.design.as_ref().unwrap().cursor;
        press(&mut app, KeyCode::Down);
        assert_eq!(app.design.as_ref().unwrap().var_cursor, 1);
        press(&mut app, KeyCode::Down);
        assert_eq!(app.design.as_ref().unwrap().var_cursor, 1);
        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Up);
        assert_eq!(app.design.as_ref().unwrap().var_cursor, 0);
        assert_eq!(app.design.as_ref().unwrap().cursor, tree_cursor);
    }
```

In `crates/vega/src/tui/ui.rs` tests add:

```rust
    #[test]
    fn a_missing_required_variable_is_listed_with_its_reason() {
        use ratatui::crossterm::event::KeyCode;
        let mut app = design_app_on("node");
        press_key(&mut app, KeyCode::Down);
        press_key(&mut app, KeyCode::Down);
        press_key(&mut app, KeyCode::Char(' ')); // id
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("$id: ID!"));
        assert!(text.contains("← required"));
    }

    #[test]
    fn variable_rows_survive_tiny_terminals() {
        use ratatui::crossterm::event::KeyCode;
        let mut app = design_app_on("node");
        press_key(&mut app, KeyCode::Down);
        press_key(&mut app, KeyCode::Down);
        press_key(&mut app, KeyCode::Char(' '));
        press_key(&mut app, KeyCode::Tab);
        press_key(&mut app, KeyCode::Tab);
        for (w, h) in [(20, 5), (1, 1), (0, 0), (3, 40), (80, 24)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|frame| draw(frame, &app, 1_000)).unwrap();
        }
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p vega tui`
Expected: FAIL to compile (`no method variable_rows`, `no field var_cursor`).

- [ ] **Step 3: Implement app.rs**

Imports: add `use vega_core::querygen::VariableDef;` (extend the existing `querygen` import) and `use vega_core::variables::{self, State, Values};`.

Add above `DesignState`:

```rust
/// An edit in progress in the Variables pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// Typing a value (numbers, IDs, strings, JSON).
    Text(String),
    /// Picking one of a list of values (enums); an empty option means "unset".
    Choice { options: Vec<String>, index: usize },
}

/// One line of the Variables pane.
#[derive(Debug, Clone)]
pub struct VariableRow {
    pub def: VariableDef,
    /// What the user typed ("" when nothing).
    pub raw: String,
    pub state: State,
}
```

`DesignState` gains `pub values: Values`, `pub var_cursor: usize`, `pub edit: Option<Edit>` (initialised with `Values::new()`, `0`, `None` in `new`).

Add to `impl App`:

```rust
    /// The variables of the generated query with their typed values (empty when there is
    /// no valid query to show).
    pub fn variable_rows(&self) -> Vec<VariableRow> {
        let (Some(design), Some(Ok(query))) = (self.design.as_ref(), self.generated_query())
        else {
            return Vec::new();
        };
        query
            .variables
            .into_iter()
            .map(|def| {
                let typed = design.values.get(&def.path).map(String::as_str);
                let state = variables::state(&self.schema, &def, typed);
                VariableRow {
                    raw: typed.unwrap_or_default().to_string(),
                    def,
                    state,
                }
            })
            .collect()
    }

    fn focused(&self, focus: Focus) -> bool {
        self.design
            .as_ref()
            .is_some_and(|design| design.focus == focus)
    }

    fn move_var_cursor(&mut self, delta: isize) {
        let last = self.variable_rows().len().saturating_sub(1);
        if let Some(design) = self.design.as_mut() {
            design.var_cursor = design.var_cursor.min(last);
            design.var_cursor = design.var_cursor.saturating_add_signed(delta).min(last);
        }
    }
```

In `handle_design_key`, after the `KeyCode::Tab` arm and before the `_ if ... focus != Focus::Details => {}` guard add:

```rust
            KeyCode::Up if self.focused(Focus::Variables) => self.move_var_cursor(-1),
            KeyCode::Down if self.focused(Focus::Variables) => self.move_var_cursor(1),
```

- [ ] **Step 4: Implement ui.rs**

Imports: `use crate::tui::app::{App, Edit, Focus, StatusLevel, VariableRow};` and `use vega_core::variables::{self, State};`.

Replace the variables half of `draw_query` (everything after the Query pane) with:

```rust
    let rows = app.variable_rows();
    let block = pane("Variables", has_focus(app, Focus::Variables));
    if rows.is_empty() {
        frame.render_widget(Paragraph::new("No variables").block(block), parts[1]);
        return;
    }
    let (cursor, edit) = match &app.design {
        Some(design) => (
            design.var_cursor.min(rows.len() - 1),
            design.edit.as_ref(),
        ),
        None => (0, None),
    };
    let items: Vec<ListItem> = rows
        .iter()
        .enumerate()
        .map(|(index, row)| variable_item(row, if index == cursor { edit } else { None }))
        .collect();
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut state = ListState::default();
    if has_focus(app, Focus::Variables) {
        state.select(Some(cursor));
    }
    frame.render_stateful_widget(list, parts[1], &mut state);
```

(The Query pane code above it keeps using `let query = app.generated_query();`; keep that line.) Add:

```rust
/// `$name: Type = value`, with a dim note for an empty optional and a red reason for a
/// value that cannot be sent.
fn variable_item(row: &VariableRow, edit: Option<&Edit>) -> ListItem<'static> {
    let mut spans = vec![Span::raw(format!("${}: {}", row.def.name, row.def.ty))];
    let dim = Style::default().fg(Color::DarkGray);
    let red = Style::default().fg(Color::Red);
    match edit {
        Some(Edit::Text(buffer)) => spans.push(Span::raw(format!(" = {buffer}▏"))),
        Some(Edit::Choice { options, index }) => {
            let shown = options
                .get(*index)
                .map(String::as_str)
                .filter(|option| !option.is_empty())
                .unwrap_or("(unset)");
            spans.push(Span::raw(format!(" = ‹{shown}›")));
        }
        None => match &row.state {
            State::Value(value) => {
                spans.push(Span::raw(format!(" = {}", variables::display(value))));
            }
            State::Unset => {
                let note = match &row.def.default {
                    Some(default) => format!("  (default {default})"),
                    None => "  (omitted)".to_string(),
                };
                spans.push(Span::styled(note, dim));
            }
            State::Invalid(reason) if row.raw.is_empty() => {
                spans.push(Span::styled(format!("  ← {reason}"), red));
            }
            State::Invalid(reason) => {
                spans.push(Span::styled(format!(" = {}  ← {reason}", row.raw), red));
            }
        },
    }
    ListItem::new(Line::from(spans))
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --workspace`
Expected: all pass. Existing tests that assert the old plain Variables text (`$id: ID!`, `No variables`) must still pass; if one fails only because of the new `← required` suffix, adapt that assertion and report it.

- [ ] **Step 6: Check and commit**

Run: `just fix && just check`
Expected: exit code 0.

```bash
git add crates/vega/src/tui/app.rs crates/vega/src/tui/ui.rs
git commit -m "feat: list variables with their values in the Variables pane"
```

---

### Task 7: Edit variable values by type

**Files:**
- Modify: `crates/vega/src/tui/app.rs`, `crates/vega/src/tui/ui.rs`

**Interfaces:**
- Consumes: `variables::{editor_for, Editor}`, `Edit`, `DesignState::{values, var_cursor, edit}` (Task 6).
- Produces: with Variables focused, Enter starts the editor for the highlighted variable: `Boolean` cycles immediately (empty, `true`, `false`, then empty again; a required variable skips empty); an enum opens a `Choice` picker (Up/Down move, Enter accepts, Esc cancels; an optional variable gets a leading "unset" option); anything else opens a `Text` editor (typing, Backspace, Enter commits, Esc cancels). While `edit.is_some()` every key goes to the editor except Ctrl-Q and Ctrl-R (so `q`, `h`, `?` are text). Committing an empty text removes the stored value. `App::is_editing(&self) -> bool`. `DESIGN_KEYS` gains `("Enter", "Edit the highlighted variable")`.

- [ ] **Step 1: Write the failing tests**

In `crates/vega/src/tui/app.rs` tests add:

```rust
    /// Opens Query.users, selects `id` and the optional arguments `role`, `active` and
    /// `floor`, and focuses the Variables pane (variables: role, active, floor).
    fn users_variables(app: &mut App) {
        type_text(app, "users");
        assert_eq!(app.hits[0].path, "Query.users");
        press(app, KeyCode::Enter);
        press(app, KeyCode::Down);
        press(app, KeyCode::Down);
        press(app, KeyCode::Char(' ')); // id
        press(app, KeyCode::Up);
        press(app, KeyCode::Right); // expand args
        for _ in 0..3 {
            press(app, KeyCode::Down);
            press(app, KeyCode::Char(' ')); // role, active, floor
        }
        press(app, KeyCode::Tab);
        press(app, KeyCode::Tab);
    }

    fn raw_values(app: &App) -> Vec<String> {
        app.variable_rows().into_iter().map(|row| row.raw).collect()
    }

    #[test]
    fn typing_a_value_and_pressing_enter_stores_it() {
        use vega_core::variables::State;
        let mut app = design_app();
        node_variables(&mut app);
        press(&mut app, KeyCode::Enter);
        assert!(app.is_editing());
        type_text(&mut app, "42");
        press(&mut app, KeyCode::Backspace);
        type_text(&mut app, "3");
        press(&mut app, KeyCode::Enter);
        assert!(!app.is_editing());
        let rows = app.variable_rows();
        assert_eq!(rows[0].raw, "43");
        assert_eq!(rows[0].state, State::Value(serde_json::json!("43")));
    }

    #[test]
    fn esc_cancels_an_edit_and_keeps_the_old_value() {
        let mut app = design_app();
        node_variables(&mut app);
        press(&mut app, KeyCode::Enter);
        type_text(&mut app, "42");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Enter);
        type_text(&mut app, "99");
        press(&mut app, KeyCode::Esc);
        assert!(!app.is_editing());
        assert!(app.design.is_some(), "Esc only cancels the edit");
        assert_eq!(raw_values(&app), ["42"]);
    }

    #[test]
    fn committing_empty_text_removes_the_value() {
        let mut app = design_app();
        node_variables(&mut app);
        press(&mut app, KeyCode::Enter);
        type_text(&mut app, "7");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Backspace);
        press(&mut app, KeyCode::Enter);
        assert_eq!(raw_values(&app), [""]);
    }

    #[test]
    fn every_key_is_text_while_editing_but_ctrl_q_still_quits() {
        let mut app = design_app();
        node_variables(&mut app);
        press(&mut app, KeyCode::Enter);
        type_text(&mut app, "qh?");
        assert!(!app.should_quit);
        assert!(!app.help_open);
        press(&mut app, KeyCode::Enter);
        assert_eq!(raw_values(&app), ["qh?"]);

        press(&mut app, KeyCode::Enter);
        ctrl(&mut app, 'q');
        assert!(app.should_quit);
    }

    #[test]
    fn booleans_cycle_with_enter_and_optional_ones_return_to_empty() {
        let mut app = design_app();
        users_variables(&mut app);
        press(&mut app, KeyCode::Down); // active
        let mut seen = Vec::new();
        for _ in 0..4 {
            press(&mut app, KeyCode::Enter);
            assert!(!app.is_editing());
            seen.push(raw_values(&app)[1].clone());
        }
        assert_eq!(seen, ["true", "false", "", "true"]);
    }

    #[test]
    fn enums_are_picked_from_a_list_with_an_unset_option() {
        let mut app = design_app();
        users_variables(&mut app); // cursor on role
        press(&mut app, KeyCode::Enter);
        assert!(app.is_editing());
        press(&mut app, KeyCode::Down); // ADMIN
        press(&mut app, KeyCode::Down); // VIEWER
        press(&mut app, KeyCode::Down); // clamped
        press(&mut app, KeyCode::Up); // ADMIN
        press(&mut app, KeyCode::Enter);
        assert_eq!(raw_values(&app)[0], "ADMIN");

        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Up); // (unset)
        press(&mut app, KeyCode::Enter);
        assert_eq!(raw_values(&app)[0], "");

        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Esc);
        assert_eq!(raw_values(&app)[0], "");
    }

    #[test]
    fn invalid_values_are_reported_per_variable() {
        use vega_core::variables::State;
        let mut app = design_app();
        users_variables(&mut app);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down); // floor
        press(&mut app, KeyCode::Enter);
        type_text(&mut app, "abc");
        press(&mut app, KeyCode::Enter);
        assert_eq!(
            app.variable_rows()[2].state,
            State::Invalid("expected a number".to_string())
        );
    }

    #[test]
    fn values_survive_leaving_and_reentering_and_a_refresh_clears_them() {
        let mut app = design_app();
        node_variables(&mut app);
        press(&mut app, KeyCode::Enter);
        type_text(&mut app, "42");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Esc);
        assert!(app.design.is_none());
        press(&mut app, KeyCode::Enter);
        assert_eq!(raw_values(&app), ["42"]);

        press(&mut app, KeyCode::Esc);
        let response: IntrospectionResponse = serde_json::from_str(DESIGN_FIXTURE).unwrap();
        app.apply_refresh_ok(response.data.schema, 5_000);
        press(&mut app, KeyCode::Enter);
        assert!(app.variable_rows().is_empty());
    }

    #[test]
    fn help_in_design_mentions_editing() {
        let mut app = design_app();
        open_me(&mut app);
        assert!(app.help_keys().iter().any(|(keys, _)| *keys == "Enter"));
    }
```

In `crates/vega/src/tui/ui.rs` tests add:

```rust
    #[test]
    fn an_edited_value_shows_next_to_its_variable_and_the_editor_shows_a_cursor() {
        use ratatui::crossterm::event::KeyCode;
        let mut app = design_app_on("node");
        press_key(&mut app, KeyCode::Down);
        press_key(&mut app, KeyCode::Down);
        press_key(&mut app, KeyCode::Char(' ')); // id
        press_key(&mut app, KeyCode::Tab);
        press_key(&mut app, KeyCode::Tab);
        press_key(&mut app, KeyCode::Enter);
        press_key(&mut app, KeyCode::Char('4'));
        press_key(&mut app, KeyCode::Char('2'));
        assert!(screen(&render(&app, 1_000)).contains("$id: ID! = 42▏"));
        press_key(&mut app, KeyCode::Enter);
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("$id: ID! = 42"));
        assert!(!text.contains("required"));
    }

    #[test]
    fn an_invalid_value_shows_its_reason() {
        use ratatui::crossterm::event::KeyCode;
        let mut app = design_app_on("users");
        press_key(&mut app, KeyCode::Down);
        press_key(&mut app, KeyCode::Down);
        press_key(&mut app, KeyCode::Char(' ')); // id
        press_key(&mut app, KeyCode::Up);
        press_key(&mut app, KeyCode::Right);
        for _ in 0..3 {
            press_key(&mut app, KeyCode::Down);
        }
        press_key(&mut app, KeyCode::Char(' ')); // floor
        press_key(&mut app, KeyCode::Tab);
        press_key(&mut app, KeyCode::Tab);
        press_key(&mut app, KeyCode::Enter);
        for c in "abc".chars() {
            press_key(&mut app, KeyCode::Char(c));
        }
        press_key(&mut app, KeyCode::Enter);
        assert!(screen(&render(&app, 1_000)).contains("= abc  ← expected a number"));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p vega tui`
Expected: FAIL to compile (`no method is_editing`).

- [ ] **Step 3: Implement app.rs**

Imports: extend to `use vega_core::variables::{self, Editor, State, Values};`.

Add `("Enter", "Edit the highlighted variable (Variables pane)")` to `DESIGN_KEYS`, after the `Tab` row.

Add a free function above `impl App`:

```rust
/// Stores the text typed for a variable; empty text clears it.
fn set_value(design: &mut DesignState, path: &Path, raw: &str) {
    if raw.is_empty() {
        design.values.remove(path);
    } else {
        design.values.insert(path.clone(), raw.to_string());
    }
}
```

Add to `impl App`:

```rust
    pub fn is_editing(&self) -> bool {
        self.design
            .as_ref()
            .is_some_and(|design| design.edit.is_some())
    }

    /// Enter on a variable: toggle a boolean, or open the editor its type needs.
    fn start_edit(&mut self) {
        let rows = self.variable_rows();
        let Some(design) = self.design.as_mut() else {
            return;
        };
        design.var_cursor = design.var_cursor.min(rows.len().saturating_sub(1));
        let Some(row) = rows.get(design.var_cursor) else {
            return;
        };
        match variables::editor_for(&self.schema, &row.def.type_ref) {
            Editor::Toggle => {
                let next = match row.raw.as_str() {
                    "true" => "false",
                    "false" if !row.def.required => "",
                    _ => "true",
                };
                set_value(design, &row.def.path, next);
            }
            Editor::Choice(names) => {
                let mut options = names;
                if !row.def.required {
                    options.insert(0, String::new());
                }
                let index = options
                    .iter()
                    .position(|option| *option == row.raw)
                    .unwrap_or(0);
                design.edit = Some(Edit::Choice { options, index });
            }
            Editor::Text | Editor::Json => design.edit = Some(Edit::Text(row.raw.clone())),
        }
    }

    /// Keys while a variable is being edited: Enter commits, Esc cancels.
    fn handle_edit_key(&mut self, code: KeyCode) {
        let rows = self.variable_rows();
        let Some(design) = self.design.as_mut() else {
            return;
        };
        let Some(mut edit) = design.edit.take() else {
            return;
        };
        let Some(row) = rows.get(design.var_cursor) else {
            return;
        };
        let finished = match (&mut edit, code) {
            (_, KeyCode::Esc) => true,
            (Edit::Text(buffer), KeyCode::Char(c)) => {
                buffer.push(c);
                false
            }
            (Edit::Text(buffer), KeyCode::Backspace) => {
                buffer.pop();
                false
            }
            (Edit::Text(buffer), KeyCode::Enter) => {
                set_value(design, &row.def.path, buffer);
                true
            }
            (Edit::Choice { options, index }, KeyCode::Down) => {
                *index = (*index + 1).min(options.len().saturating_sub(1));
                false
            }
            (Edit::Choice { index, .. }, KeyCode::Up) => {
                *index = index.saturating_sub(1);
                false
            }
            (Edit::Choice { options, index }, KeyCode::Enter) => {
                if let Some(option) = options.get(*index) {
                    set_value(design, &row.def.path, option);
                }
                true
            }
            _ => false,
        };
        if !finished {
            design.edit = Some(edit);
        }
    }
```

In `handle_key`, after the `if self.help_open { ... }` block and before `let opens_help = ...`, add:

```rust
        if self.is_editing() {
            self.handle_edit_key(key.code);
            return Action::None;
        }
```

In `handle_design_key`, after the two Variables Up/Down arms from Task 6 add:

```rust
            KeyCode::Enter if self.focused(Focus::Variables) => self.start_edit(),
```

(`handle_edit_key` returns early when the cursor row no longer exists, dropping the edit; that is intended.)

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --workspace`
Expected: all pass.

- [ ] **Step 5: Check and commit**

Run: `just fix && just check`
Expected: exit code 0.

```bash
git add crates/vega/src/tui/app.rs crates/vega/src/tui/ui.rs
git commit -m "feat: edit variable values by type"
```

---

### Task 8: Documentation

**Files:**
- Modify: `docs/superpowers/specs/2026-10-22-vega-phase-2-design.md`, `README.md`, `AGENTS.md`

- [ ] **Step 1: Update the spec**

In the phase 2 spec:
- CLI parity section: add a bullet `Optional arguments are opted in with the path segment `arg:<name>` in `--select` (`--select id,arg:role`, or `friends.arg:first` for a nested field); naming a required argument is an error. `--var name=value` (repeatable, by variable name) sets a value; an invalid value is an error, a missing required one is only listed.`
- Variables section: add a bullet `An argument with a default value counts as optional; its variable is declared with that default (`$limit: Int! = 10`) and an empty value keeps it.`
- Tree model section: add `A leaf field that has arguments can still be expanded to reach its `args` group.`
- Keys section, Variables focus line: append `While a variable is being edited every key is text except Ctrl-Q and Ctrl-R; Boolean toggles empty, true, false with Enter.`
- Delivery section: after step 2 add `(done)`.

- [ ] **Step 2: Update README and AGENTS.md**

README, in the Usage block add:

```sh
vega --query Query.users --select id,arg:role --var role=ADMIN   # opt in an optional argument and set a variable
```

and update the "What it does" bullet about building queries to mention optional arguments and variables: `- Build a query by picking fields from a tree: expand with →, select with Space (optional arguments too), edit the typed variables with Tab and Enter, and watch the generated query update.` Update the sentence "Selecting fields and generating the query works; editing variables and running queries against the server are planned." to "Building the query and editing its variables works; running queries against the server is planned."

AGENTS.md, Layout section: change the `vega-core` bullet to `Schema model, search, detail rendering, tree and selection model, query generation, variable typing, cache, config, HTTP client. No UI code.` and in the Rules section add `- Variable values live only in memory for the session: never write them to disk, the cache or logs.`

- [ ] **Step 3: Check and commit**

Run: `just check`
Expected: exit code 0.

```bash
git add docs/superpowers/specs/2026-10-22-vega-phase-2-design.md README.md AGENTS.md
git commit -m "doc: describe variables, optional arguments and --var"
```

---

### Task 9: End-to-end verification

**Files:** none (verification only).

- [ ] **Step 1: Run the full checks**

Run: `just check`
Expected: exit code 0.

- [ ] **Step 2: Try the command line against a seeded cache**

Run the CLI tests (`cargo test -p vega --test cli`), which seed a temp cache the way a real run would, and confirm by reading their assertions that: `--var id=42` prints `$id: ID! = 42`; `--json` carries `value`; `arg:term` opts `$term: String` in; an unknown variable and an invalid value exit non-zero with the message on stderr and empty stdout.

- [ ] **Step 3: Hand the interactive check to the user**

The TUI cannot be driven without a terminal. Ask the user to run `vega` against their server and check: an operation with arguments shows an `args` group (▸ args); expanding it shows required arguments as `▪` fixed rows and optional ones as `☐` that SPACE toggles; the Query pane gains the variable when an optional argument is checked; Tab twice reaches Variables (cyan border), Up/Down move the highlight; Enter on a text variable opens an inline editor with a cursor, Enter commits, Esc cancels, and `q`/`h`/`?` type text; Enter on a Boolean cycles empty/true/false; Enter on an enum opens a picker; an invalid value shows red with a reason; values survive Esc to Browse and back, and Ctrl-R clears them.

---

## Self-review (spec coverage)

| Spec requirement (step 2) | Task |
| --- | --- |
| Optional-arguments nodes: `args` group, required fixed, optional toggled with SPACE | 2, 3 |
| Variables from included arguments, naming collisions, schema order | 3 |
| Rows `$id: ID! = 42`, missing/invalid in red with a reason | 6, 7 |
| Typed editing: Int/Float numbers, Boolean toggle, ID/String text, enums list, custom scalars strings, input objects and lists as JSON validated against the type | 4, 7 |
| Empty optional variable omitted; empty required invalid | 4, 6 |
| Variables focus keys: Up/Down, Enter edits, Esc cancels, Tab returns | 6, 7 |
| Values kept per operation for the session, cleared by refresh, never persisted | 7 |
| CLI parity for variables (`--var`), no `--run` | 5 |
| Variable validation table-driven tests, TUI state-machine tests, tiny-terminal renders | 4, 6, 7 |

Not in this plan by design: running queries, the Response pane, mutation confirmation, `--run` and `--yes` (step 3).
