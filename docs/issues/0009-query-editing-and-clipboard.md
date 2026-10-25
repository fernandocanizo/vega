# Analyze query editing and copy/paste support

Status: done

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

## Decision

No editable query buffer. Copy and paste of whole queries, nothing else.

- `Ctrl-C` copies the query as a plain GraphQL document with real newlines. `Ctrl-J` copies the JSON body of a GraphQL request, `{"query": ..., "variables": {...}}`, pretty-printed, so it can be fed to `curl -d @-` (JSON cannot hold raw newlines in a string). Only variables that are set and valid are included, and no header ever is. It works while composing and with the Variables pane focused in Run mode. Copying the response waits for issue 0013.
- The clipboard is written with OSC 52 only: it works over SSH and needs no clipboard crate. The README lists the terminals and the tmux setting.
- There is no `Ctrl-V` handler: the terminal's own paste arrives as bracketed paste. While composing, and in the results list, a paste that is a JSON body or a GraphQL operation is imported; anything else behaves as before. In Run mode a paste only goes into the variable being edited.
- Importing rebuilds the design from the schema: the first root field that exists is kept, what the schema does not know is discarded and counted, variables are matched by where they are used, and the query is regenerated, so it looks like one built by hand. A non-empty selection is replaced only after confirmation.
