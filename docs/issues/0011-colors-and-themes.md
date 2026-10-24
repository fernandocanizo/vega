# Brainstorm: better colors, a switch to turn them off, and themes

Status: open (needs brainstorming)

## Problem

Colors in the TUI are fixed. Minimal or old terminals do not handle colors well, and users cannot adapt the look to their taste.

## Ideas to evaluate

- Better coloring of the TUI panes, types, fields and the response.
- A switch to turn colors off completely, for minimal or old terminals. Consider honoring the `NO_COLOR` convention, a config option and a flag.
- Theme switching with a couple of basic built-in themes (for example dark and light).
- User-defined themes loaded from files, maybe `$XDG_CONFIG_HOME/vega/themes/*.{json,ini}`. Choose the format (TOML would match `config.toml`).

## Questions to settle

- Color depth: 16 colors, 256, truecolor. How do themes degrade?
- Theme file schema, validation, and what happens with a broken theme file (warn on stderr and fall back).
- How a theme is selected (config key, flag, in-TUI switch) and its place in the precedence rule: CLI flag, then environment, then `config.toml`.
- Interaction with profiles (0003).
- Keep the red "stale" status bar meaningful when colors are off.

## Deliverable

A recommendation with tradeoffs, then implementation issues. No code in this issue.

## Current rule

Until themes exist, the focus color is fixed to the terminal's magenta (`FOCUS` in `crates/vega/src/tui/ui.rs`, see `AGENTS.md`). A theme should make that one color configurable, and the "no color" mode must still show the focus (reversed video already does, whatever the color).
