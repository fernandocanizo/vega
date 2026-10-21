# Vega design

Date: 2026-10-21
Status: approved design, pending implementation plan

## Purpose

Vega is a general-purpose GraphQL client for the terminal, written in Rust. It is not tied to any specific server. It is being built to work well against the Nexus GraphQL API (used by the Rocky project) and replaces Altair for that workflow.

Motivations:

- Find operations quickly by partial name and read their docs, arguments and return types.
- Authenticate with an API key instead of copying JWTs between tools.
- Expose every capability as a CLI flag so agents and scripts can use it without MCP.

## Phases

### Phase 1 (v1): search and navigate (read-only)

- Interactive TUI (`vega` with no arguments).
- Non-interactive commands over the same core:
  - `vega --search <text>`: list matching operations and types.
  - `vega --describe <Type|Type.field>`: print the definition, docs, arguments and return type.
  - `--json` on both for machine-readable stdout.
  - `--refresh`: refetch the schema before running.
- Schema comes from introspection, cached on disk per server.

### Phase 2: compose and run (out of scope for v1)

- Composer pane: write queries by hand or select fields (SPACE selects a field and auto-selects its ancestors).
- Variables support.
- Submit (ENTER) and show the response.
- A query builder module in the core generates query text from the selection tree.

### Deferred, not designed here

- Named server profiles (config file already reserved).
- Saved queries and multiple panes.
- MCP server (the CLI flags cover the agent use case).
- Query syntax validation with `apollo-parser`.

## Configuration and credentials

- Precedence: CLI flags, then environment variables, then `config.toml`.
- Environment variables: `GRAPHQL_URI` (endpoint) and `GRAPHQL_KEY` (API key).
- The API key is sent as the `X-API-Key` header on a `POST` with `Content-Type: application/json`, matching `example.curl.query.sh`.
- The API key is never written to disk, the cache, or logs.

## Architecture

Cargo workspace with two crates:

- `vega-core` (library, no UI dependencies):
  - HTTP client (`reqwest`).
  - Introspection fetch.
  - Schema model: own `serde` structs for the introspection types (`__Schema`, `__Type`, `__Field`, `__InputValue`, `__EnumValue`).
  - Search over operations and types.
  - Cache read/write.
- `vega` (binary): `ratatui` TUI and CLI argument handling. Both are thin layers over `vega-core`.

Rationale for not using a GraphQL parser or compiler crate in v1: the introspection JSON has a fixed shape defined by the spec, and the TUI needs a navigable tree, not a validated schema. Phase 2 builds query strings from its own selection tree.

## Schema cache

- One file per server in `$XDG_CACHE_HOME/vega/` (fallback `~/.cache/vega/`), named by a hash of the endpoint URL.
- File contents: the introspection JSON plus a `fetched_at` timestamp stored inside the file (not file mtime).
- No cache: the first run fetches automatically.
- Refresh: `--refresh` flag, and Ctrl-R in the TUI.
- A failed refresh keeps the existing cache and reports the error.
- CLI commands print a warning to stderr when the cache is older than 24 hours. Stdout, including `--json`, stays clean.

## TUI (phase 1)

Layout: a status bar on top, a search input, a results list on the left, and a detail pane on the right.

- Status bar: server host, "schema fetched <age> ago", and the refresh key hint.
  - Neutral colour under 24 hours.
  - Bold red over 24 hours.
  - A "no schema yet" state before the first fetch.
  - A "refreshing…" state during refresh. The UI stays responsive (refresh runs off the UI thread).
- Search: typing filters results by partial name.
- Navigation: arrow keys move through the list, Enter drills into the selected item. In a type, Enter on a field jumps to its return type. Backspace goes back through the history. `q` or Esc quits.
- Detail pane: signature, description (including permission notes such as "Requires USER_READ"), arguments, and return type fields.

## XDG layout

- Cache: `$XDG_CACHE_HOME/vega/`.
- Config: `$XDG_CONFIG_HOME/vega/config.toml` (optional defaults in phase 1; profiles later).
- Data: `$XDG_DATA_HOME/vega/` (reserved for saved queries).
- Install: `cargo install --path .` to `~/.cargo/bin`; a `just install` recipe may copy the binary to `~/.local/bin`.
- Path crate: `etcetera` or `directories`. Decide at plan time by checking that it yields XDG paths on Linux.

## Tooling and automation

- `rustfmt` with a committed `rustfmt.toml`.
- `clippy` with `-D warnings`; lint levels in `[workspace.lints]` in the root `Cargo.toml`.
- `rust-toolchain.toml` pins the toolchain with the `rustfmt` and `clippy` components.
- `just check` runs fmt-check, clippy and tests. `just fix` applies formatting.
- Git pre-commit hook runs `just check`.
- Optional later: `cargo-deny`.
- `curl` is used only as a development aid for probing the server, not as a dependency.

## Agent documentation

- `AGENTS.md` at the repository root is the agent-neutral source of truth. It covers: purpose, workspace layout, `just` commands, code conventions, XDG and credential rules (no API key on disk or in logs), and phase scope.
- `CLAUDE.md` contains only `@AGENTS.md`.

## Testing

- `vega-core` unit tests: introspection JSON deserialization (using a small fixture), search ranking and matching, cache read/write including a failed-refresh case, and staleness calculation.
- CLI tests: `--search` and `--describe` output (text and `--json`), the stderr staleness warning with clean stdout.
- TUI: state-transition tests on the app model (key in, state out) without rendering a terminal.
- Network access is mocked in tests. No test depends on a live server.

## Open items for the implementation plan

- Choose `etcetera` vs `directories` for XDG paths.
- Confirm the Nexus server's introspection response works with the fixture shape (probe with curl during development).
- Define the search matching rules (case-insensitive substring in v1; ranking if needed).
