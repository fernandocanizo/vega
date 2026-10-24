# Export results to a file

Status: open (not scheduled)

## Problem

There is no way to save a response to disk from the TUI.

## Proposal

Add an export action that works like a browser download:

- The default destination is the user's download folder, taken from the XDG user directory for downloads (`XDG_DOWNLOAD_DIR`, as defined in `user-dirs.dirs`), with a sensible fallback when it is not set.
- If the user picks another folder, vega remembers it for next time.
- The user can create a new folder from the TUI while choosing the destination.
- The interaction is a modal window.

## Questions to settle

- Where the remembered folder is stored. The paths rules say config lives under `$XDG_CONFIG_HOME/vega` and state under `$XDG_DATA_HOME/vega`; a remembered folder is a user preference, not a secret.
- Default file name (for example operation name plus a timestamp), and what happens when the file already exists (never overwrite silently).
- What is exported: the response body as shown, and whether the query text is included.
- Permissions on the created file and error handling for unwritable folders.

## Constraints

- The API key never appears in the exported content (the client already masks it in responses).
- Variable values must not be written to disk. Export only the response, never the variables, unless this rule is changed deliberately.
- Tests use a temporary directory, never the real download folder.

## Acceptance criteria

- Modal state and path handling are testable in the pure state machine in `tui/app.rs`.
- Tests cover: default folder, remembered folder, new folder creation, existing file, unwritable folder.
