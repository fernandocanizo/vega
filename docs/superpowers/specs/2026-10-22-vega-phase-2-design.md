# Vega phase 2 design: build and run queries

Date: 2026-10-22
Status: approved design, pending implementation plan
Builds on: `2026-10-21-vega-design.md` (phase 1). This document replaces that spec's phase 2 sketch and its TUI navigation rules (Backspace history, following a field to its type) for the interactive browser.

## Purpose

Let the user build a GraphQL operation by picking fields from the schema, supply its variables, run it against the server, and read the response, all without typing a query by hand.

## Decisions

- No free-form query editing. The query is always generated from a selection, so it cannot contain syntax errors. (An "edit as text" escape hatch may come later; it is out of scope.)
- Variables are edited as typed rows, not as one JSON blob.
- SPACE selects leaf fields only. Objects are never selected directly, so an object with no selected leaf below it does not appear in the query and the query is always valid.
- Selection and variable values are kept per operation for the whole app session and are never reset by running a query. They are lost when the app quits.
- Running a mutation asks for confirmation. Queries run immediately.
- Subscriptions generate a query but cannot be run (they need WebSockets).

## Modes and layout

The screen always shows at most two main panes side by side, with shorter panes below them.

```
Browse   Results (names only)  | Details (tree, read-only preview)
                               | Description
Design   Details (tree)        | Query (read-only)
         Description           | Variables
Run      Query                 | Response
         Variables             |
```

- The Details pane title is always `Details` (no `Preview`).
- Results show only the path (`Query.userRead`), not the signature; the signature is shown in Details.
- Design mode hides Results. Run mode hides Details and Description; Query moves to the left column and the Response takes the right column. Variables stays visible under Query.
- Enter on a Results item that is a type (not an operation) opens Design layout with only the tree, read-only: no Query or Variables panes and no selection.

## Keys

- Browse: Up/Down move through Results; typing filters; Enter or Right starts designing the highlighted operation; Esc does nothing (quit with Ctrl-Q).
- Design (tree focus): Up/Down move; Right expands a node; Left collapses it, or jumps to its parent when already collapsed; SPACE toggles a leaf (does nothing on an object); Tab cycles focus through Details, Query and Variables (the tree keys act only on Details); `r` runs; Esc or Backspace returns to Browse, keeping the search text.
- Variables focus: Up/Down pick a variable; Enter edits it; Esc cancels an edit; Tab returns to the tree.
- Run: Up/Down and PgUp/PgDn scroll the response; `r` runs again; Tab focuses Variables; Esc returns to Design with the selection intact.
- Anywhere: `?` or F1 opens help (listing the keys of the current mode), Ctrl-R refreshes the schema, Ctrl-Q quits (`q` also quits outside text entry).
- Backspace and the type-following history are removed from the interactive browser.

## Tree model

- Children are built lazily when a node is expanded, so recursive types never expand forever. A type that has already appeared twice on the path from the root is shown as a collapsed node marked `↻` and cannot be expanded further, so one level of self-nesting (`me`, then `friends`) still works.
- A field with arguments has an `args` child group. Required arguments are included automatically and shown as fixed rows; optional arguments are toggled with SPACE.
- A union or interface field expands into one branch per possible type (`on User`, `on Org`). Fields of an interface can also be selected directly.
- The selection is the set of selected leaf paths. The query is derived from it after every change.

## Query generation

- Shape: `query UserRead($id: ID!) { userRead(id: $id) { id name } }`. The operation keyword comes from the root (`query`, `mutation`, `subscription`) and the operation name is the field name capitalised.
- Fields are emitted in schema order, not selection order, so the same selection always produces the same text. Indentation is two spaces.
- Selecting a leaf under a union or interface branch emits an inline fragment `... on Type { ... }`. `__typename` is added automatically to every union or interface selection set.
- Not supported: aliases, directives, fragment definitions, and selecting the same field twice with different arguments.

## Variables

- Every argument that is included (required or opted in) becomes a variable named after the argument, with the argument's type. If two fields use the same argument name, later ones are prefixed with the field name (`$friendsFirst`).
- Rows read `$id: ID! = 42`. A missing or invalid value is shown in red with a short reason.
- Editing by type: `Int` and `Float` are validated as numbers; `Boolean` toggles with Enter; `ID` and `String` take free text; enums are picked from a list; custom scalars are sent as strings without validation; input objects and lists are edited as JSON for that single variable and validated against the type (missing required fields, wrong types, unknown enum values).
- An empty optional variable is omitted from the variables JSON (it is not sent as `null`).

## Running

- Running is blocked while any variable is invalid or a required one is empty. The status line names the problem.
- A mutation shows `Run mutation <name>? y/N` before sending.
- The request is a `POST` with `query`, `variables` and `operationName`, plus the `X-API-Key` header, sent from a background thread (one request in flight at a time).
- The Response pane shows the pretty-printed JSON, the HTTP status, the elapsed time, and highlights GraphQL `errors`. A response with both `data` and `errors` shows both. Network failures, non-2xx statuses and invalid JSON are shown as errors in the same pane.
- Variable values are never written to the cache or any log, and the API key never appears in the Response pane or any message.

## CLI parity

- `vega --query <Type.field> --select <paths> --var name=value ...` prints the generated query and the variables and sends nothing. `--select` takes comma-separated paths relative to the operation (`id,role,friends.name`; union branches as `on:Type`, for example `search.on:User.name`).
- `--select` paths may go through recursive types to any depth, even though the interactive tree stops expanding a type that already appears twice on the path.
- Adding `--run` sends the request and prints the response. `--json` gives machine-readable output on stdout.
- A mutation run without a terminal requires `--yes`.
- Warnings and errors go to stderr only; stdout stays machine-readable.

## Architecture

- `vega-core` (no UI code): `selection` (lazy tree model, toggling, cycle detection), `querygen` (selection to query text and variable definitions), `variables` (definitions, typed validation, scalar parsing), and `client::execute` (send a request, return status, body and elapsed time, sharing the HTTP code with the introspection fetch).
- `vega`: a `Browse/Design/Run` mode state in the TUI app, one module per pane, and the new CLI flags next to the existing ones.

## Testing

- The shared fixture grows to cover a union, a recursive type, a nested field with arguments, and an input object.
- Query generation: exact expected strings, including stable ordering, fragments and variable naming collisions.
- Variable validation: table-driven tests per type.
- TUI: state-machine tests per mode; each of the three layouts rendered at 80x24 and at a tiny size.
- `execute`: tests against a local `127.0.0.1` listener (variables in the body, headers, GraphQL errors, non-JSON, HTTP errors).
- CLI: `--query`, `--run` and the `--yes` rule, using a seeded cache and a local listener. No test uses the real network.

## Delivery in three steps

1. Design mode: the tree, SPACE selection, the live Query pane, `--query` printing (variables listed but not yet editable).
2. Variables: the typed rows, validation, the optional-arguments nodes.
3. Run: the Response pane, mutation confirmation, `--run` and `--yes`.

Each step ends in a usable state and gets its own implementation plan.

## Open items for the plans

- Exact `--select` syntax for union branches.
- Whether very large responses need a display cap (decide when step 3 is planned).
