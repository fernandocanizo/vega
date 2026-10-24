# Show a position indicator in the query and results views

Status: open (not scheduled)

## Problem

When moving around a long query or response or the initial results page, there is no sign of how far along the user is.

## Proposal

Show an indicator at the bottom of the query view and the results view. A percentage may be enough; adding a line count is better, for example:

```
line 100/3400 3%
```

## Questions to settle

- Does the indicator show the line at the top of the viewport or the selected line? -> Bottom, always bottom
- Behavior for content shorter than the viewport (hide it, or show `100%`). -> 100%
- Where it sits relative to the status bar and the key hints, and how it behaves on narrow terminals. -> sits at the bottom of the window is displaying info for

## Acceptance criteria

- The formatting and the percentage math are in a pure function with unit tests, including empty content, one line, the first line, the last line and a very long response.
- The indicator updates on scroll and on resize.
