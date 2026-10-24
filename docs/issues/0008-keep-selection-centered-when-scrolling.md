# Keep the selected row centered while scrolling lists and the query tree

Status: open (not scheduled)

## Problem

In the Results window, when the user navigates down to the last query/mutation shown, the view pins the selection at the bottom line and the user loses context. The same happens in the query view where fields are selected to compose a query.

## Expected behavior

- The selected row stays in the middle of the viewport. Moving the selection one row down scrolls the list one row up.
- Near the end, the scroll stops when there are no more rows to show. Example: if the middle is 10 lines from the bottom and only 10 items remain, scrolling stops there; the selection then moves down the screen as usual.
- At the end of the list, show one extra empty line so the user can tell the end was reached, and no more than one.
- The same applies in reverse when going up toward the beginning of the list.
- The behavior applies to both the Results list and the field-selection tree used to compose a query.

## Notes

- Scroll and selection logic belongs in `tui/app.rs` as a pure state machine, so it can be unit tested without a terminal.
- The viewport height changes with the terminal size; the rule must hold after a resize.

## Acceptance criteria

- Unit tests cover: moving through the middle, reaching the end (one trailing empty line, no more), reaching the beginning, a list shorter than the viewport, and a resize.
- Both views use the same logic.
