# Do not panic when stdout is closed early

Status: open
Related: 0006 (find public GraphQL servers)

## Problem

Piping a CLI command into a program that stops reading, such as `head`, makes Vega panic and exit with status 101:

```
$ vega --uri https://rickandmortyapi.com/graphql --search "" | head -1
Query.character  character(id: ID!): Character

thread 'main' panicked at library/std/src/io/stdio.rs:1168:9:
failed printing to stdout: Broken pipe (os error 32)
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
```

`println!` and `print!` panic on a closed pipe. The CLI output is meant to be piped (`--json`, `--search`, `--describe`, `--query`), so this shows up in ordinary use, for example `vega --search "" | head`.

## Expected behavior

A closed stdout ends the command quietly, with no panic message. Other write errors keep being reported as errors.

## Notes

- Stdout must stay machine-readable and warnings go to stderr only (`AGENTS.md`).
- A test can close the read end of the pipe after the first line and check the exit status and that stderr is empty.
