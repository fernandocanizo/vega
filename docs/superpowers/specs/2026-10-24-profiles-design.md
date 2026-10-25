# Saved server profiles

Issue: `docs/issues/0003-multiple-profiles.md`. Related: 0004 (connection modal, done), 0005 and 0014 (other forms of login), 0001 (version-aware cache).

## Goal

Let the user save servers (endpoint plus credentials) and switch between them, so the URI and the key are not typed on every start. Credentials are stored in the operating system keyring, never in a file Vega writes.

Out of scope for this step: a plaintext fallback when no keyring exists (later issue), saved queries (later issue), renaming a profile, themes and other per-profile settings.

## Storage decision

The secret goes to the OS keyring (the `keyring` crate: Secret Service on Linux, Keychain on macOS, Credential Manager on Windows). The OS encrypts and unlocks it.

Rejected for now: an encrypted file with a master passphrase (Vega would own the crypto and ask for the passphrase on each start), a `0600` plaintext file (secrets unencrypted on disk), and a command reference such as `pass show ...` (config could run arbitrary commands). The plaintext file is the planned fallback in a later step.

If no keyring is reachable, Vega refuses to save and says why. Environment variables and flags keep working as before.

## Data model (`vega-core`)

`config.toml` holds the non-secret part:

```toml
[profiles.staging]
uri = "https://staging.example.com/graphql"
kind = "api_key"          # "none", "api_key" or "headers"
key_header = "X-API-Key"  # api_key only; a header name is not a secret
```

- The existing top-level `uri` keeps working. A config without profiles needs no migration.
- A profile name matches `[A-Za-z0-9_-]+`. It is the TOML key and the keyring user.
- The keyring entry has service `vega` and user `<profile name>`, so all Vega secrets are grouped under `vega`. The secret string is the raw key for `api_key`, and the `Name: value` lines (the `VEGA_GRAPHQL_HEADERS` format) for `headers`. No header name or value of a custom-headers profile is written to disk.
- A profile with no authentication has no keyring entry. The connection modal's Basic, Bearer and API Key types produce headers, so they are saved as `api_key` when it is the single API key header, and as `headers` otherwise.
- A `SecretStore` trait (`get`, `set`, `delete` by profile name) isolates the keyring. The keyring implementation is the only code that touches it. Tests use an in-memory fake that can inject failures.
- `SecretError` (`thiserror`): `Unavailable`, `Locked`, `Backend(String)`. Messages never contain a secret.

## Startup and precedence

- Profile selection: `--profile <name>`, then `VEGA_PROFILE`. There is no default profile in the config.
- The profile supplies the URI and the credentials. The existing precedence still applies on top of it: `--uri` beats the profile's URI (the profile's credentials are then sent only if the new endpoint has the same origin as the profile's own, otherwise they are withheld with a warning), and environment credentials (`VEGA_GRAPHQL_KEY` or `VEGA_GRAPHQL_HEADERS`) replace the profile's credentials entirely, with no merge.
- An unknown profile is an error that lists the known names. A keyring failure is an error that names the profile and offers the environment variables as the alternative. Neither prints a secret.
- A profile whose keyring entry is missing connects with no headers, and the status says no stored credentials were found.
- `--search`, `--describe`, `--query` and `--run` accept `--profile`. Warnings go to stderr only, stdout stays machine-readable, and `--run` output stays masked.
- The cache is already keyed per URI. Profiles on the same endpoint share it.

## Saving and deleting

- Save order: the keyring first, then `config.toml` (written to a temporary file and renamed). If the keyring write fails, nothing is saved. If the config write fails after the keyring write, the keyring is put back as it was (the previous secret, or no entry). If that also fails, the status says an orphan entry `vega/<name>` may remain.
- Delete removes the config entry and the keyring entry. A missing keyring entry is not an error.
- An invalid name is rejected with the allowed characters in the message.

## TUI

### Picker (`Ctrl-P`, from any mode)

A pure state machine in `tui/profiles.rs` (key in, state out), routed by `tui/app.rs`. A centered modal lists `name  uri  (kind)` and marks the current profile.

- `↑`/`↓` move, `Enter` connects, `e` opens the Connection modal for the profile, `n` opens it empty, `d` deletes after a `y/n` confirm, `Esc` closes.
- Header values are never rendered. Focus follows the `AGENTS.md` rules: purple on the border and the selected row, terminal cursor hidden while the modal is open.
- With no profiles it shows `No saved servers. n to add one`.

### Connecting to a profile

A new connection, as in the connection-modal spec for a URI change: load the schema for that URI and clear the selection, query and variables. The secret is read from the keyring and kept in memory. A keyring failure shows a one-line status message and changes nothing.

### Saving from the Connection modal

- A "Save as" field sits under the URI, empty by default. `Ctrl-S` saves under that name and connects. An empty name does nothing and shows a message. `Enter` keeps its meaning: connect for the session without saving.
- `e` in the picker opens the modal pre-filled. The secret is read from the keyring and masked. `Ctrl-S` overwrites the profile.
- On a save error the modal stays open so the typed secret is not lost.

### Status bar and help

Idle: `Ctrl-R refresh · Ctrl-K connection · Ctrl-P profiles`. The no-endpoint and load-error messages mention `Ctrl-P` as well. When a profile is active its name is shown. The help tables get a `Ctrl-P` row, and the modal help gets a `Ctrl-S` row.

## Rules changes (`AGENTS.md`)

The header-value rule becomes: header values come from the environment, the modal, or the OS keyring. They are written only to the OS keyring, and only on an explicit save. They are never written to any file Vega writes, the cache or logs, and never appear in error messages. No CLI flag carries a value. The Response pane and `--run` output keep masking them. Variable values stay in memory only. Requests still never follow redirects.

## Testing

- `vega-core`: profile (de)serialization and name validation; secret-to-`Settings` mapping for both kinds; flag and environment override the profile; unknown profile lists the known names; a config without profiles loads unchanged; save and delete ordering and rollback through the fake store.
- `tui/profiles.rs`: keys, delete confirm, empty state, no header value rendered.
- Connection modal: `Ctrl-S` with and without a name, pre-fill on edit, and `Enter` still connects without saving.
- `app.rs` and `ui.rs`: `Ctrl-P` routing, status bar text, help rows.
- CLI integration for the unknown-profile error and a blank profile name; connecting, saving and rollback are tested in `vega-core` with the fake store.
- The real keyring is never used by `just check`. One `#[ignore]` test exercises it for manual runs.

## Deliverables

Code and tests, the `AGENTS.md` change, a README line, updated help text, a minor version bump in its own commit, and issue 0003 set to `done`. New issues: plaintext fallback for hosts without a keyring, and saved queries.
