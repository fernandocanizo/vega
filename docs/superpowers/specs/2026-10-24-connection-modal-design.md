# Connection modal and authentication headers

Issue: `docs/issues/0004-paste-api-key-in-app.md`. Related: 0003 (profiles), 0005 (other forms of login), 0007 (authentication research), 0010 (no-endpoint error).

## Goal

Let the user enter the endpoint and the credentials inside the TUI, keep them in memory only, and make vega start with nothing configured. The modal is built so that new authentication types are added as new units, without changing the modal itself.

## Reference: Altair

Altair offers None, API Key (name and value), Basic (username and password), Bearer (token) and OAuth 2.0 (authorization code, authorization code with PKCE, client credentials). Sources: https://altairgraphql.dev/docs/features/auth and https://altairgraphql.dev/api/core/oauth2/types/enumerations/OAuth2Type. Vega follows the first four and adds Custom headers. OAuth 2.0 is out of scope.

## Connection model

A connection is the endpoint plus a list of HTTP headers:

```
Connection { uri: String, headers: Vec<Header> }
Header { name: String, value: Secret }
```

Every authentication type only produces headers. Header values are always treated as secrets.

## Sources and precedence

URI, highest first: `--uri`, `VEGA_GRAPHQL_URI`, `uri` in `config.toml`, the modal.

Headers:

1. `VEGA_GRAPHQL_KEY` becomes one header. Its name is `X-API-Key` unless `--key-header` or `VEGA_GRAPHQL_KEY_HEADER` says otherwise (flag first).
2. `VEGA_GRAPHQL_HEADERS` entries are applied after it. Format: one `Name: value` per line, like `curl -H`. Blank lines are ignored. A malformed line is an error that names the line number and never prints the value.
3. Headers are merged by name, case-insensitively. On the same name the entry from `VEGA_GRAPHQL_HEADERS` wins.
4. Whatever the user submits in the modal replaces the result of 1 to 3 for the session.

There is no flag for any secret value. `--key-header` is allowed because a header name is not a secret.

## Startup

- With no URI, the TUI opens the modal right away. It does not fail.
- The TUI no longer loads the schema before it starts. It starts at once and loads the schema in a background thread: from the cache when one exists and `--refresh` is not given, otherwise from the server. The status bar says the schema is loading. A failed load (a 401, say) does not stop the program: the status bar shows the first line of the error with hints for `Ctrl-K` (connection) and `Ctrl-R` (retry).
- A missing key never opens the modal, because a public server needs none.
- Non-TUI commands (`--search`, `--describe`, `--query`, `--run`) keep a clear error for a missing URI, as described in issue 0010, still load the schema before they run, and treat a failed load as an error. They never open a modal.

## The modal (`crates/vega/src/tui/connection.rs`)

A pure state machine (key in, state out), with no terminal code, as `AGENTS.md` requires. `tui/app.rs` only routes keys to it and applies its result.

Layout:

```
┌ Connection ──────────────────────────────────┐
│ URI: https://api.example.com/graphql         │
├───────────────┬──────────────────────────────┤
│ ○ None        │ (form of the selected type)  │
│ ○ Basic       │                              │
│ ● API Key     │                              │
│ ○ Bearer      │                              │
│ ○ Custom hdrs │                              │
└───────────────┴──────────────────────────────┘
```

- The URI field is above the split and is always visible. The left pane lists the types, the right pane shows the form of the selected one.
- Types are mutually exclusive. Custom headers is one more type, not an addition to the others.
- Each type is a unit with its own fields and a function from those fields to `Vec<Header>`:

| Type | Fields | Headers produced |
|---|---|---|
| None | none | none |
| Basic | username, password | `Authorization: Basic base64(user:password)` |
| API Key | header name (default `X-API-Key`), value | one header |
| Bearer | token | `Authorization: Bearer <token>` |
| Custom headers | rows of name and value, with an add-row action | the rows |

- Start state follows the effective settings. No headers: None. Headers that are exactly the API key (`VEGA_GRAPHQL_KEY` alone, or a submitted API Key form): API Key, pre-filled. Any other headers: Custom headers, one row per header with values masked. Basic or Bearer values set through `VEGA_GRAPHQL_HEADERS`, or submitted earlier in the modal, are plain `Authorization` headers and therefore reopen as Custom headers rows. The distinction is stored as `Settings::key_only`, and `Settings` is also what the modal returns.

Keys:

- `Ctrl-K` opens the modal from any mode. It is listed in both help tables and in the hint line.
- `Ctrl-T` reveals or masks the focused field. Everything is masked when the modal opens and again when it closes.
- Bracketed paste is enabled. A paste goes only into the focused field, the trailing newline is stripped, and nothing is echoed. The alternate screen keeps the value out of the scrollback.
- Outside the modal, a paste is typed into the search box or a text variable editor and ignored everywhere else, so pasted text can never trigger a command key.
- `Enter` submits, `Esc` cancels.

## Submitting

- Only headers changed: keep the schema, query and variables. New headers apply from the next request. If the schema had failed to load, load it again at once.
- URI changed: a new connection. Load the schema for the new URI (the cache is keyed per URI) and clear the selection, query and variables.
- Cancel: no change. If there is no URI yet, the app stays on an empty screen with a hint for `Ctrl-K`.
- Choosing None and submitting removes all headers.

## Secrets

- Values from the modal, `VEGA_GRAPHQL_KEY` and `VEGA_GRAPHQL_HEADERS` are never written to disk, the cache, logs or error messages.
- The client masks every header value in the Response pane, in `--run` output and in errors, not only the API key.
- Modal edits live only in memory and are dropped on exit. They are never written back to the environment.

## Testing

- Unit tests in `connection.rs` for each type's fields-to-headers function, navigation, reveal toggle, paste handling and the start state for each environment combination.
- Unit tests for the header merge (case-insensitive, explicit wins) and the `VEGA_GRAPHQL_HEADERS` parser, including malformed lines without leaking the value.
- Tests in `app.rs` for the submit cases (headers only, URI changed, cancel) and for opening on a missing URI.
- Integration tests with a local `127.0.0.1` listener that check the headers received and that no value appears in the output.

## Rule and doc changes

- `AGENTS.md`: the key is no longer environment-only. It comes from the environment or the modal, never from a flag and never to disk. The masking rule covers every header value.
- `docs/superpowers/specs/2026-10-21-vega-design.md`: update the configuration section.
- README: a short list of the supported methods (None, Basic, API Key, Bearer, Custom headers) with a link to `docs/authentication.md`.
- `docs/authentication.md`: examples for every method in the modal, in `--run`, and for agents (environment recipes for API key, Bearer, Basic and custom headers).
- Issue 0010 is partly covered: the TUI no longer fails without a URI. Its message for the CLI is still needed.

## Out of scope

- OAuth 2.0 and any interactive login flow.
- Profiles (0003). One `Connection` per profile is the natural fit later.
- Persisting any header value.
