# Brainstorm: paste an API key into the app and keep it in memory

Status: open (needs brainstorming)

## Problem

Today the API key can only come from `VEGA_GRAPHQL_KEY`. A user who has the key in a password manager has to export it in the shell before starting vega. Letting the user paste it inside the TUI, with the app keeping it in memory only, would be more convenient.

## Constraints from the project rules

- The key must never be written to disk, the cache, logs or error messages.
- There must be no CLI flag for the key.
- The key never appears in the Response pane or `--run` output (the client masks it).
- The rules say the key comes only from `VEGA_GRAPHQL_KEY`. Pasting it in the TUI is a new source, so `AGENTS.md` and the design spec must be updated deliberately if this goes ahead.

## Questions to settle

- Entry UI: a modal with a masked input? Is the pasted text visible at any moment?
- Lifetime: for the session only; cleared on exit, and a way to clear it explicitly?
- Precedence between a pasted key and the environment variable.
- Terminal paste handling (bracketed paste) so the key does not leak through the scrollback or key events.
- Interaction with profiles (0003): one in-memory key per profile?

## Deliverable

A recommendation with tradeoffs and the required rule changes. No code in this issue.
