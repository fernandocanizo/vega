# AGENTS.md

Vega is a terminal GraphQL client written in Rust. It introspects a server, caches the schema, and lets you search and navigate operations and types in a TUI or through CLI flags.

Design spec: `docs/superpowers/specs/2026-10-21-vega-design.md`. Plans: `docs/superpowers/plans/`.

## Scope

- Read-only exploration of the schema, plus building a query from selected fields. Sending queries to the server is not implemented yet.
- Do not add features beyond what the task asks.

## Commands

- `just check`: format check, clippy with `-D warnings`, all tests. Run before every commit. CI (`.github/workflows/ci.yml`) runs the same command on every pull request and on pushes to `main`.
- `just fix`: apply formatting and auto-fixable lints.
- `just install`: release build copied to `~/.local/bin/vega`.
- `just hooks`: enable the git hooks (pre-commit runs `just check`; commit-msg enforces the commit rules below).

## Layout

- `crates/vega-core`: library. Schema model, search, detail rendering, tree and selection model, query generation, variable typing, cache, config, HTTP client. No UI code.
- `crates/vega`: binary. clap CLI and ratatui TUI, thin layers over `vega-core`.
- `crates/vega-core/tests/fixtures/introspection.json`: shared schema fixture.

## Conventions

- Rust 2021. Lints are configured once in the root `Cargo.toml` (`[workspace.lints]`).
- Errors: `thiserror` types in `vega-core`, `anyhow` in the binary.
- No `unwrap`/`expect` in non-test code unless an invariant makes failure impossible.
- Tests: unit tests inline in modules, integration tests in `crates/*/tests/`. Tests never use the real network (a local `127.0.0.1` listener is fine). Use the shared fixture.
- TUI logic lives in `tui/app.rs` as a pure state machine (key in, state out) so it is testable without a terminal.

## Commits

- Atomic: one logical change per commit. A short task is one commit; a bigger task is split by step or concern (for example the model, then its tests' fixtures, then the wiring). Every commit must pass `just check` (the pre-commit hook enforces it), so never commit a red test on its own.
- Subject: `tag: what the commit does`, present tense, imperative, at most 72 characters, no trailing period.
- Tags are lowercase and at most 5 characters: `feat`, `fix`, `ref` (refactor), `doc`, `test`, `chore`. Add a new tag only when none fits, and keep it within 5 characters.
- Body (optional, separated by a blank line): explain why when it is not obvious from the diff. Wrap every line at 72 characters. Wrap with `fmt -w 72` (coreutils) or `fold -s -w 72`.
- Say what the commit does. Never reference plans, specs, tasks, steps or sections, and never add co-author, "generated with" or any other AI attribution lines.
- The `githooks/commit-msg` hook rejects messages that break these rules. Enable hooks once with `just hooks`.

## Versioning

- Semantic versioning, kept in one place: `version` under `[workspace.package]` in the root `Cargo.toml`, plus the `vega-core` dependency version in `crates/vega/Cargo.toml`, and `Cargo.lock`.
- Every new pull request bumps the minor version, in its own `chore: bump the version to X.Y.0` commit made when the work is ready for review.
- The maintainer bumps the major version. The patch version is for bug fixes and tiny changes and is also managed by the maintainer; do not bump it.

## Rules that must not be broken

- The API key comes only from the `VEGA_GRAPHQL_KEY` environment variable. Never write it to disk, the cache, logs or error messages, and never add a CLI flag for it.
- Variable values live only in memory for the session: never write them to disk, the cache or logs.
- Config and cache paths follow XDG (`$XDG_CONFIG_HOME/vega`, `$XDG_CACHE_HOME/vega`, `$XDG_DATA_HOME/vega`). Precedence: CLI flag, then environment, then `config.toml`.
- CLI commands print warnings to stderr only. Stdout (including `--json`) must stay machine-readable.
- A failed refresh must keep the existing cache.
