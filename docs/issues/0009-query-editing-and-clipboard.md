# Analyze query editing and copy/paste support

Status: open (needs analysis)

## Problem

Queries are composed by selecting fields, and variables are edited in the TUI. There is no way to copy a generated query out, or paste text in. We need to decide how far editing should go.

## Options to analyze

- No editable buffer. Only `Ctrl-C` copies the current query (and perhaps the response) to the clipboard and `Ctrl-V` pastes into the variables editor.
- A full editable text buffer for the query.

The leaning is the first option: copy and paste mappings without an editable buffer.

## Questions to settle

- Clipboard access in a terminal: a crate such as `arboard`, OSC 52 escape sequences (works over SSH, depends on terminal support), or both with a fallback. Check Wayland and X11 behavior.
- `Ctrl-C` is normally the interrupt key. How does it interact with raw mode and with quitting?
- What can be copied (query text, variables, response) and what can be pasted (variables only?).
- Bracketed paste handling in the TUI.
- Variable values live only in memory and must never reach disk, the cache or logs; the clipboard must not change that rule silently.

## Deliverable

A recommendation with tradeoffs, then implementation issues. No code in this issue.
