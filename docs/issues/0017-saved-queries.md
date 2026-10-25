# Saved queries

Status: open (needs brainstorming)
Related: 0003 (saved profiles)

## Problem

A query built from selected fields is lost when Vega closes. Users who run the same queries again and again have to rebuild them each time.

## Questions to settle

- Save and recall named queries and their variables, possibly per profile?
- Where do they live, and in what format?
- Variable values must stay out of files (see `AGENTS.md`). Are only the variable names and types saved?
- How are they listed and picked in the TUI, and can the CLI run one by name?

## Deliverable

A short design note with a recommendation and the tradeoffs, then implementation issues split from it. No code in this issue.
