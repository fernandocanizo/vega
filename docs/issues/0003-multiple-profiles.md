# Brainstorm: multiple server profiles

Status: open (needs brainstorming)

## Problem

Vega is configured for a single endpoint (`--uri`, `VEGA_GRAPHQL_URI` or `uri` in `config.toml`). Users who work with several servers (staging, production, third-party APIs) have to change flags or environment variables each time. Other issues already assume profiles will exist (see 0001).

## Questions to settle

- Config shape: `[profiles.<name>]` tables in `config.toml`? Which settings are per profile (`uri`, `version_query`, `version_path`, theme, download dir)?
- Selection: a `--profile` flag, a `VEGA_PROFILE` variable, a default profile in the config, and a profile switcher in the TUI?
- Precedence must stay: CLI flag, then environment, then `config.toml`.
- Cache: one cache entry per profile (keyed by name or by endpoint)?
- API key: it must keep coming only from `VEGA_GRAPHQL_KEY`. How does a user with one key per profile work without a per-profile flag or a key on disk? (See 0004.)
- Migration: a config without profiles must keep working unchanged.

## Deliverable

A short design note in `docs/` with a recommendation and the tradeoffs, then implementation issues split from it. No code in this issue.

## Out of scope

- Implementation.
