# Authentication

Vega sends HTTP headers with every request. All the supported methods are ways to produce those headers, so any server that authenticates through headers works.

| Method | What you set | Header sent |
| --- | --- | --- |
| None | nothing | none |
| API key | `VEGA_GRAPHQL_KEY` | `X-API-Key: <key>` |
| API key, other header name | `VEGA_GRAPHQL_KEY` and `--key-header` or `VEGA_GRAPHQL_KEY_HEADER` | `<name>: <key>` |
| Bearer token | `VEGA_GRAPHQL_HEADERS` | `Authorization: Bearer <token>` |
| Basic | `VEGA_GRAPHQL_HEADERS` | `Authorization: Basic <base64 of user:password>` |
| Custom headers | `VEGA_GRAPHQL_HEADERS` | whatever you list |

## Why credentials are not flags

Command line arguments are visible to other users of the machine (`ps`, `/proc/<pid>/cmdline`) and end up in the shell history. Vega therefore reads every secret from the environment and has no flag for one. `--key-header` only names a header, which is not a secret.

Vega never follows redirects, so your headers only ever go to the endpoint you chose and never to another origin a server points to.

Vega never writes a header value to a file, the cache or logs (the only place a secret is stored is the OS keyring, when you save a profile), and masks every value (and, for values such as `Bearer abc`, the part after the space) in the Response pane, in `--run` output and in error messages.

## Saved profiles

A profile saves an endpoint and its credentials under a name, so you do not retype them.

- Where things go: `config.toml` holds the endpoint and the kind of authentication, and the secret goes to the OS keyring (service `vega`, user = the profile name). The secret is the raw key for an API key profile, or the `Name: value` lines for a custom headers profile. Basic and Bearer from the modal are saved as headers. No header name or value of a headers profile is written to a file.
- Names: letters, digits, `-` and `_`.
- Saving needs a keyring (Secret Service on Linux, for example GNOME Keyring or KWallet). Without one, saving fails with a message saying so and nothing is saved; the environment variables and flags keep working. A plaintext fallback is tracked in issue 0016.
- Save from the TUI: `Ctrl-K`, fill the form, type a name in "Save as", press `Ctrl-S`. `Enter` still connects without saving. In the picker (`Ctrl-P`), `Enter` connects, `e` edits (the secret is read from the keyring and masked), `n` adds, `d` deletes after `y`, `Esc` closes.
- Use from the command line: `--profile NAME` or `VEGA_PROFILE=NAME` (the flag wins). A blank name means no profile. An unknown name is an error that lists the saved ones.
- `--uri` (or `VEGA_GRAPHQL_URI`) overrides the profile's endpoint. The profile's credentials are sent only when that endpoint has the same origin (scheme, host and port) as the profile's own; otherwise they are withheld and a warning says so. Credentials from the environment are never withheld.
- Credentials from the environment (`VEGA_GRAPHQL_KEY` or `VEGA_GRAPHQL_HEADERS`) replace the profile's credentials entirely; they are not merged.
- A profile whose keyring entry is missing connects with no headers and prints a warning on stderr.

Example `config.toml`:

```toml
[profiles.staging]
uri = "https://staging.example.com/graphql"
kind = "api_key"          # "none", "api_key" or "headers"
key_header = "X-API-Key"  # api_key only; a header name is not a secret
```

## Variables

- `VEGA_GRAPHQL_KEY`: an API key.
- `VEGA_GRAPHQL_KEY_HEADER` / `--key-header NAME`: the header that carries the key. Default `X-API-Key`. The flag wins.
- `VEGA_GRAPHQL_HEADERS`: any other headers, one `Name: value` per line, like `curl -H`. Blank lines are ignored. The value may contain colons. A malformed line stops vega with an error that names the line number and never prints the value.

Headers are merged by name, ignoring case. The key is applied first and `VEGA_GRAPHQL_HEADERS` after it, so on the same name the entry from `VEGA_GRAPHQL_HEADERS` wins.

## Recipes

Set the endpoint with `--uri`, `VEGA_GRAPHQL_URI` or `uri` in `config.toml`; the examples use `VEGA_GRAPHQL_URI`.

### API key in `X-API-Key`

```sh
export VEGA_GRAPHQL_URI=https://api.example.com/graphql
export VEGA_GRAPHQL_KEY=...
vega --search user
```

### API key in another header

```sh
VEGA_GRAPHQL_KEY=... vega --key-header X-Token --search user
# or
VEGA_GRAPHQL_KEY=... VEGA_GRAPHQL_KEY_HEADER=X-Token vega --search user
```

### Bearer token

```sh
export VEGA_GRAPHQL_HEADERS="Authorization: Bearer $TOKEN"
vega --search user
```

### Basic authentication

The header value is the base64 of `user:password`:

```sh
export VEGA_GRAPHQL_HEADERS="Authorization: Basic $(printf '%s' 'user:password' | base64)"
vega --search user
```

### Several headers

```sh
export VEGA_GRAPHQL_HEADERS="Authorization: Bearer $TOKEN
X-Tenant: team-9"
vega --query Query.me --select id --run
```

### Keeping secrets out of the shell history

Read the secret without echoing it, or take it from a password manager:

```sh
read -rs VEGA_GRAPHQL_KEY && export VEGA_GRAPHQL_KEY
VEGA_GRAPHQL_KEY=$(pass show api/example) vega --search user
```

### Scripts and coding agents

Everything above works without a terminal: set the variables on the process and use `--json`:

```sh
VEGA_GRAPHQL_URI=https://api.example.com/graphql \
VEGA_GRAPHQL_HEADERS="Authorization: Bearer $TOKEN" \
  vega --query Query.me --select id --run --json
```

Mutations need `--yes` when there is no terminal.

## In the terminal UI

You do not need any environment variable to start vega. With no endpoint configured it opens the connection modal right away, and `Ctrl-K` opens it again from any screen.

- The endpoint is the field at the top. Below it, the methods are listed on the left (None, Basic, API Key, Bearer, Custom headers) and the form of the selected method is on the right. Only one method is active.
- `Tab` and `Shift-Tab` move between the fields, `Up` and `Down` move inside the list or the form, `Enter` connects and `Esc` cancels.
- Secret fields show bullets. `Ctrl-T` reveals or hides the focused field until the modal closes.
- Paste works in any field (the trailing newline is dropped).
- Custom headers: `Ctrl-N` adds a row. A row with both fields empty is ignored.
- The modal opens filled in from the environment: `VEGA_GRAPHQL_KEY` alone opens on API Key, `VEGA_GRAPHQL_HEADERS` opens on Custom headers (one row per header), nothing opens on None. Basic and Bearer values set through the environment are plain `Authorization` headers, so they appear as Custom headers rows.
- Connecting with only new credentials keeps the schema, your query and its variables, and retries the schema load if it had failed. Changing the endpoint starts a new connection: the schema is loaded for it (from the cache when there is one) and the selection, query and variables are cleared.
- Nothing you enter is saved unless you press `Ctrl-S` to save a profile. Otherwise it lives in memory until you quit and is never written back to the environment, the cache or any file.
- A server that rejects the schema request (a 401, for example) no longer stops vega from starting: the status bar shows the error, and `Ctrl-K` lets you fix the credentials.

## Troubleshooting

- `server redirected the request (HTTP 301) to <url>`: the endpoint has moved. Vega does not follow redirects (a followed redirect would turn the POST into a GET and could send your credentials to another origin). Use the URL in the message as the endpoint.
- `server answered HTTP 401` or `403`: the header name or value is wrong. Check which header the server expects.
- `VEGA_GRAPHQL_HEADERS line N: expected `Name: value``: line N has no colon, an empty value or an invalid header name (names allow letters, digits and ``!#$%&'*+-.^_`|~``).
- `the key header name is not valid`: fix `--key-header` or `VEGA_GRAPHQL_KEY_HEADER`.

A saved profile's URI is written to `config.toml` in the clear. Vega refuses to save a URI that has a username or password; also do not put a token in its query string.
