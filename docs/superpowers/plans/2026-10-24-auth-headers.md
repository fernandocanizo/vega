# Auth Headers Implementation Plan (part 1 of 2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the single API key with a list of secret HTTP headers, fed from `VEGA_GRAPHQL_KEY`, `VEGA_GRAPHQL_KEY_HEADER` / `--key-header` and `VEGA_GRAPHQL_HEADERS`, with every value masked in anything shown to the user.

**Architecture:** A new `vega-core::headers` module holds `Header` (value hidden in `Debug`), the `VEGA_GRAPHQL_HEADERS` parser, the case-insensitive merge and the masking function. `Settings` carries `headers: Vec<Header>` and `client` sends and masks them. The binary only wires environment and flags into `config::AuthInput`. The TUI modal is part 2 (`docs/superpowers/specs/2026-10-24-connection-modal-design.md`), which builds on this model.

**Tech Stack:** Rust 2021, `reqwest` (blocking), `thiserror`, `clap` derive, `just check`.

## Global Constraints

From `docs/superpowers/specs/2026-10-24-connection-modal-design.md` and `AGENTS.md`:

- URI precedence: `--uri`, `VEGA_GRAPHQL_URI`, `uri` in `config.toml`.
- `VEGA_GRAPHQL_KEY` becomes one header named `X-API-Key` unless `--key-header` or `VEGA_GRAPHQL_KEY_HEADER` says otherwise (flag first).
- `VEGA_GRAPHQL_HEADERS` entries are applied after the key; format is one `Name: value` per line, like `curl -H`; blank lines are ignored; a malformed line is an error that names the line number and never prints the value.
- Headers are merged by name, case-insensitively; on the same name the entry from `VEGA_GRAPHQL_HEADERS` wins.
- There is no flag for any secret value. `--key-header` is allowed because a header name is not a secret.
- Values are never written to disk, the cache, logs or error messages. The client masks every header value in the Response pane, in `--run` output and in errors.
- CLI commands print warnings to stderr only. Stdout (including `--json`) stays machine-readable.
- No `unwrap`/`expect` in non-test code. Tests never use the real network (a `127.0.0.1` listener is fine).
- Commits: `tag: what it does`, imperative, at most 72 characters, tags `feat`, `fix`, `ref`, `doc`, `test`, `chore`. Every commit passes `just check`. No AI attribution lines.
- Versioning: the final commit bumps the minor version (0.4.0 to 0.5.0) in its own `chore: bump the version to 0.5.0` commit.

Out of scope for this plan (part 2): the modal, `Ctrl-K`, a TUI that starts without a URI, Basic and Bearer form helpers.

---

### Task 1: The header model

**Files:**
- Create: `crates/vega-core/src/headers.rs`
- Modify: `crates/vega-core/src/lib.rs` (add `pub mod headers;` after `pub mod describe;`)

**Interfaces:**
- Produces (used by every later task):
  - `pub const DEFAULT_KEY_HEADER: &str = "X-API-Key";`
  - `pub struct InvalidName;` (unit error)
  - `pub enum HeaderError { Malformed { line: usize } }` (`Debug, Clone, Copy, PartialEq, Eq`, `thiserror::Error`)
  - `pub struct Header` with `Header::new(name: impl Into<String>, value: impl Into<String>) -> Result<Header, InvalidName>`, `name(&self) -> &str`, `value(&self) -> &str`. `Debug` prints `value: "***"`.
  - `pub fn parse_lines(text: &str) -> Result<Vec<Header>, HeaderError>`
  - `pub fn merge(base: Vec<Header>, overrides: Vec<Header>) -> Vec<Header>`
  - `pub fn mask(text: &str, headers: &[Header]) -> String`

- [ ] **Step 1: Write the failing tests**

Create `crates/vega-core/src/headers.rs` with only the test module (the items do not exist yet, so it will not compile):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn h(name: &str, value: &str) -> Header {
        Header::new(name, value).unwrap()
    }

    #[test]
    fn debug_output_hides_the_value() {
        let text = format!("{:?}", h("X-API-Key", "super-secret"));
        assert!(!text.contains("super-secret"), "{text}");
        assert!(text.contains("X-API-Key"), "{text}");
        assert!(text.contains("***"), "{text}");
    }

    #[test]
    fn invalid_names_are_rejected() {
        for name in ["", "Has Space", "Colon:Name", "Accént"] {
            assert_eq!(Header::new(name, "v"), Err(InvalidName), "{name:?}");
        }
        assert!(Header::new("X-Api_Key.1", "v").is_ok());
    }

    #[test]
    fn parse_lines_reads_name_value_pairs() {
        let parsed = parse_lines("Authorization: Bearer abc\r\n\n  X-Tenant :  team-9  \n").unwrap();
        assert_eq!(
            parsed,
            vec![h("Authorization", "Bearer abc"), h("X-Tenant", "team-9")]
        );
    }

    #[test]
    fn parse_lines_keeps_colons_inside_the_value() {
        let parsed = parse_lines("X-Url: https://example.com:8080/x").unwrap();
        assert_eq!(parsed, vec![h("X-Url", "https://example.com:8080/x")]);
    }

    #[test]
    fn parse_lines_of_empty_text_is_empty() {
        assert_eq!(parse_lines("").unwrap(), vec![]);
        assert_eq!(parse_lines("\n  \n").unwrap(), vec![]);
    }

    #[test]
    fn malformed_lines_name_the_line_and_never_the_value() {
        for text in [
            "Authorization Bearer tok-123",
            "X-Ok: fine\nBad Name: tok-123",
            "X-Ok: fine\nX-Empty:   ",
        ] {
            let err = parse_lines(text).unwrap_err();
            let message = err.to_string();
            assert!(!message.contains("tok-123"), "{message}");
            assert!(message.contains("line"), "{message}");
        }
        assert_eq!(
            parse_lines("X-Ok: fine\nBad Name: v").unwrap_err(),
            HeaderError::Malformed { line: 2 }
        );
    }

    #[test]
    fn merge_replaces_by_name_ignoring_case_and_the_override_wins() {
        let base = vec![h("X-API-Key", "k"), h("X-Keep", "1")];
        let overrides = vec![h("x-api-key", "other"), h("X-New", "2")];
        assert_eq!(
            merge(base, overrides),
            vec![h("X-Keep", "1"), h("x-api-key", "other"), h("X-New", "2")]
        );
    }

    #[test]
    fn merge_lets_a_later_override_win_over_an_earlier_one() {
        let merged = merge(vec![], vec![h("X-A", "1"), h("x-a", "2")]);
        assert_eq!(merged, vec![h("x-a", "2")]);
    }

    #[test]
    fn mask_hides_whole_values_and_the_last_word_of_a_value() {
        let headers = vec![h("Authorization", "Bearer tok-123"), h("X-Tenant", "team-9")];
        let text = "echo Bearer tok-123 / tok-123 / team-9";
        assert_eq!(mask(text, &headers), "echo *** / *** / ***");
    }

    #[test]
    fn mask_replaces_the_longest_secret_first() {
        let headers = vec![h("X-A", "abc"), h("X-B", "abcdef")];
        assert_eq!(mask("abcdef abc", &headers), "*** ***");
    }

    #[test]
    fn mask_ignores_empty_values_and_leaves_other_text_alone() {
        let headers = vec![h("X-Empty", "")];
        assert_eq!(mask("nothing to hide", &headers), "nothing to hide");
        assert_eq!(mask("no headers", &[]), "no headers");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p vega-core headers 2>&1 | tail -15`
Expected: compile errors such as `cannot find type Header in this scope`.

- [ ] **Step 3: Write the implementation**

Put this above the `#[cfg(test)]` module in `crates/vega-core/src/headers.rs`, and add `pub mod headers;` to `crates/vega-core/src/lib.rs` after `pub mod describe;`:

```rust
//! HTTP headers sent with every request. Every value is a secret: it is hidden in
//! `Debug` output and masked in anything shown to the user.

use std::{cmp::Reverse, fmt};

/// Name of the header that carries `VEGA_GRAPHQL_KEY` unless another one is configured.
pub const DEFAULT_KEY_HEADER: &str = "X-API-Key";

/// The text is not a valid HTTP header name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("not a valid header name")]
pub struct InvalidName;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HeaderError {
    /// The message names the line only: the line may hold a secret.
    #[error("VEGA_GRAPHQL_HEADERS line {line}: expected `Name: value`")]
    Malformed { line: usize },
}

#[derive(Clone, PartialEq, Eq)]
pub struct Header {
    name: String,
    value: String,
}

impl Header {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Result<Self, InvalidName> {
        let name = name.into();
        if !is_valid_name(&name) {
            return Err(InvalidName);
        }
        Ok(Self {
            name,
            value: value.into(),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}

impl fmt::Debug for Header {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Header")
            .field("name", &self.name)
            .field("value", &"***")
            .finish()
    }
}

/// An RFC 9110 token: ASCII letters, digits and `!#$%&'*+-.^_`|~`.
fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c))
}

/// Parses `Name: value` lines (the format of `VEGA_GRAPHQL_HEADERS`). Blank lines are
/// ignored; a line without a name, a colon or a value is an error.
pub fn parse_lines(text: &str) -> Result<Vec<Header>, HeaderError> {
    let mut headers = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let malformed = HeaderError::Malformed { line: index + 1 };
        let (name, value) = line.split_once(':').ok_or(malformed)?;
        let value = value.trim();
        if value.is_empty() {
            return Err(malformed);
        }
        headers.push(Header::new(name.trim(), value).map_err(|_| malformed)?);
    }
    Ok(headers)
}

/// Adds `overrides` after `base`, dropping any header of `base` (or an earlier override)
/// that has the same name, compared case-insensitively.
pub fn merge(mut base: Vec<Header>, overrides: Vec<Header>) -> Vec<Header> {
    for header in overrides {
        base.retain(|existing| !existing.name.eq_ignore_ascii_case(&header.name));
        base.push(header);
    }
    base
}

/// Replaces every secret in `text` with `***`. For a value such as `Bearer abc` both the
/// whole value and its last word count as secrets, because a server may echo either.
pub fn mask(text: &str, headers: &[Header]) -> String {
    let mut secrets: Vec<&str> = headers
        .iter()
        .flat_map(|header| {
            let value = header.value();
            [Some(value), value.rsplit_once(' ').map(|(_, last)| last)]
        })
        .flatten()
        .filter(|secret| !secret.is_empty())
        .collect();
    secrets.sort_by_key(|secret| Reverse(secret.len()));
    secrets
        .into_iter()
        .fold(text.to_string(), |text, secret| text.replace(secret, "***"))
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p vega-core headers 2>&1 | tail -15`
Expected: `test result: ok. 11 passed`.

- [ ] **Step 5: Run the full check and commit**

Run: `just check`
Expected: format, clippy and all tests pass.

```bash
git add crates/vega-core/src/headers.rs crates/vega-core/src/lib.rs
git commit -m "feat: add the header model for request credentials"
```

---

### Task 2: Send a list of headers instead of one API key

A behavior-preserving refactor: `VEGA_GRAPHQL_KEY` still produces `X-API-Key`, now through `Settings::headers`. It is one commit because the signatures must change together to compile.

**Files:**
- Modify: `crates/vega-core/src/client.rs` (`post`, `execute`, `fetch_introspection`, remove `mask_key`)
- Modify: `crates/vega-core/src/config.rs` (`Settings`, new `AuthInput`, `resolve_settings`)
- Modify: `crates/vega/src/main.rs`, `crates/vega/src/schema_source.rs`, `crates/vega/src/tui/mod.rs`
- Test: `crates/vega-core/tests/client.rs`, tests inside `config.rs` and `tui/mod.rs`

**Interfaces:**
- Consumes: `Header`, `headers::mask`, `headers::DEFAULT_KEY_HEADER` from Task 1.
- Produces:
  - `client::execute(uri: &str, headers: &[Header], body: &Value) -> Result<Response, ClientError>`
  - `client::fetch_introspection(uri: &str, headers: &[Header]) -> Result<IntrospectionResponse, ClientError>`
  - `config::AuthInput { pub key: Option<String> }` (`Default`; no `Debug` on purpose)
  - `config::Settings { pub uri: String, pub headers: Vec<Header> }` (`Debug, Clone, PartialEq, Eq`)
  - `config::resolve_settings(flag_uri: Option<String>, env_uri: Option<String>, auth: AuthInput, file: FileConfig) -> Result<Settings, ConfigError>`

- [ ] **Step 1: Update the tests to the new API**

In `crates/vega-core/tests/client.rs`:

1. Add after the existing `use` lines:

```rust
use vega_core::headers::Header;

fn key(value: &str) -> Vec<Header> {
    vec![Header::new("X-API-Key", value).unwrap()]
}
```

2. Run:

```bash
sed -i 's/Some("secret")/\&key("secret")/; s/Some("nope")/\&key("nope")/; s/Some("secret-key")/\&key("secret-key")/; s/, None)/, \&[])/; s/, None, /, \&[], /' crates/vega-core/tests/client.rs
```

3. Append this test at the end of the file:

```rust
#[test]
fn execute_sends_every_header_and_masks_each_value() {
    let (uri, rx) = serve_once("200 OK", "echo tok-123 and team-9".to_string());
    let headers = vec![
        Header::new("Authorization", "Bearer tok-123").unwrap(),
        Header::new("X-Tenant", "team-9").unwrap(),
    ];
    let response = execute(&uri, &headers, &me_body()).unwrap();
    let request = rx.recv().unwrap().to_lowercase();
    assert!(request.contains("authorization: bearer tok-123"), "{request}");
    assert!(request.contains("x-tenant: team-9"), "{request}");
    assert!(!response.body.contains("tok-123"), "{}", response.body);
    assert!(!response.body.contains("team-9"), "{}", response.body);
}
```

In `crates/vega-core/src/config.rs`, replace the whole `tests` module with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_hides_the_header_values() {
        let settings = Settings {
            uri: "https://x.example.com/graphql".into(),
            headers: vec![Header::new("X-API-Key", "super-secret").unwrap()],
        };
        let text = format!("{settings:?}");
        assert!(!text.contains("super-secret"), "{text}");
        assert!(text.contains("***"), "{text}");
    }

    fn file(uri: &str) -> FileConfig {
        FileConfig {
            uri: Some(uri.to_string()),
        }
    }

    fn s(v: &str) -> Option<String> {
        Some(v.to_string())
    }

    fn key(v: &str) -> AuthInput {
        AuthInput { key: s(v) }
    }

    #[test]
    fn flag_beats_env_beats_file() {
        let all = resolve_settings(s("flag"), s("env"), AuthInput::default(), file("file")).unwrap();
        assert_eq!(all.uri, "flag");
        let env_file = resolve_settings(None, s("env"), AuthInput::default(), file("file")).unwrap();
        assert_eq!(env_file.uri, "env");
        let only_file = resolve_settings(None, None, AuthInput::default(), file("file")).unwrap();
        assert_eq!(only_file.uri, "file");
    }

    #[test]
    fn empty_values_count_as_unset() {
        let settings =
            resolve_settings(s(""), s("env"), key(""), FileConfig::default()).unwrap();
        assert_eq!(settings.uri, "env");
        assert!(settings.headers.is_empty());
    }

    #[test]
    fn missing_uri_is_an_error_naming_the_ways_to_set_it() {
        let err =
            resolve_settings(None, None, AuthInput::default(), FileConfig::default()).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("--uri"));
        assert!(message.contains("VEGA_GRAPHQL_URI"));
        assert!(message.contains("config.toml"));
    }

    #[test]
    fn the_key_becomes_the_default_header() {
        let settings = resolve_settings(s("u"), None, key("secret"), FileConfig::default()).unwrap();
        assert_eq!(
            settings.headers,
            vec![Header::new("X-API-Key", "secret").unwrap()]
        );
    }

    #[test]
    fn missing_config_file_yields_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_file_config(dir.path()).unwrap().uri, None);
    }

    #[test]
    fn config_file_uri_is_read() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            "uri = \"https://x/graphql\"\n",
        )
        .unwrap();
        assert_eq!(
            load_file_config(dir.path()).unwrap().uri.as_deref(),
            Some("https://x/graphql")
        );
    }

    #[test]
    fn invalid_config_file_is_a_parse_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "uri = [").unwrap();
        assert!(matches!(
            load_file_config(dir.path()),
            Err(ConfigError::Parse { .. })
        ));
    }
}
```

In `crates/vega/src/tui/mod.rs`, inside `mod tests`, add `use vega_core::headers::Header;` next to the other `use` lines and replace the `settings` helper with:

```rust
    fn settings(uri: &str, key: Option<&str>) -> Settings {
        Settings {
            uri: uri.to_string(),
            headers: key
                .map(|key| vec![Header::new("X-API-Key", key).unwrap()])
                .unwrap_or_default(),
        }
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --workspace 2>&1 | grep -E "^error" | head`
Expected: compile errors (`AuthInput` not found, `fetch_introspection` takes `Option<&str>`, no field `headers`).

- [ ] **Step 3: Implement**

`crates/vega-core/src/client.rs`:

- Change `use crate::schema::IntrospectionResponse;` to:

```rust
use crate::headers::{self, Header};
use crate::schema::IntrospectionResponse;
```

- In `Response`, change the `body` doc comment to `/// The body as received, with every header value (if the server echoed one) masked.`
- Delete the `mask_key` function.
- Replace `post`, `execute` and `fetch_introspection` with:

```rust
/// POSTs `body` as JSON with `headers` and returns the HTTP status, the response text
/// and how long the round trip took.
fn post(
    uri: &str,
    headers: &[Header],
    body: &Value,
) -> Result<(u16, String, Duration), reqwest::Error> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?;
    let mut request = client.post(uri).json(body);
    for header in headers {
        request = request.header(header.name(), header.value());
    }
    let started = Instant::now();
    let response = request.send()?;
    let status = response.status().as_u16();
    let text = response.text()?;
    Ok((status, text, started.elapsed()))
}

/// Runs an operation. Any HTTP status is an `Ok` response; connection failures, timeouts
/// and unreadable bodies are errors.
pub fn execute(uri: &str, headers: &[Header], body: &Value) -> Result<Response, ClientError> {
    let (status, text, elapsed) = post(uri, headers, body)?;
    Ok(Response {
        status,
        elapsed,
        body: headers::mask(&text, headers),
    })
}

pub fn fetch_introspection(
    uri: &str,
    headers: &[Header],
) -> Result<IntrospectionResponse, ClientError> {
    let (status, text, _elapsed) = post(uri, headers, &request_body())?;
    if !(200..300).contains(&status) {
        let body: String = text.chars().take(300).collect();
        return Err(ClientError::Status {
            status,
            body: headers::mask(&body, headers),
        });
    }
    parse_response(&text)
}
```

`crates/vega-core/src/config.rs` (above the tests):

- Replace the module doc comment's second line with `//! Header values are read from the environment only and are never persisted.`
- Add `use crate::headers::{Header, DEFAULT_KEY_HEADER};` after `use serde::Deserialize;`.
- Replace `Settings` and its manual `Debug` impl with:

```rust
/// Credentials as read from the environment and flags. It has no `Debug` on purpose.
#[derive(Default)]
pub struct AuthInput {
    /// `VEGA_GRAPHQL_KEY`.
    pub key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub uri: String,
    /// Sent with every request. Values are secrets (`Header`'s `Debug` hides them).
    pub headers: Vec<Header>,
}
```

- Add this variant to `ConfigError`, after `MissingUri`:

```rust
    #[error("the key header name is not valid: fix --key-header or VEGA_GRAPHQL_KEY_HEADER")]
    InvalidKeyHeader,
```

- Replace `resolve_settings` with:

```rust
pub fn resolve_settings(
    flag_uri: Option<String>,
    env_uri: Option<String>,
    auth: AuthInput,
    file: FileConfig,
) -> Result<Settings, ConfigError> {
    let uri = non_empty(flag_uri)
        .or_else(|| non_empty(env_uri))
        .or_else(|| non_empty(file.uri))
        .ok_or(ConfigError::MissingUri)?;
    let mut headers = Vec::new();
    if let Some(key) = non_empty(auth.key) {
        let header =
            Header::new(DEFAULT_KEY_HEADER, key).map_err(|_| ConfigError::InvalidKeyHeader)?;
        headers.push(header);
    }
    Ok(Settings { uri, headers })
}
```

`crates/vega/src/main.rs`:

- Change `use vega_core::config::{self, FileConfig, Settings};` to `use vega_core::config::{self, AuthInput, FileConfig, Settings};`.
- Replace the line `        env_var("VEGA_GRAPHQL_KEY"),` with:

```rust
        AuthInput {
            key: env_var("VEGA_GRAPHQL_KEY"),
        },
```

- Change `client::execute(&settings.uri, settings.api_key.as_deref(), &body)` to `client::execute(&settings.uri, &settings.headers, &body)`.

`crates/vega/src/schema_source.rs`: change `client::fetch_introspection(&settings.uri, settings.api_key.as_deref())` to `client::fetch_introspection(&settings.uri, &settings.headers)`.

`crates/vega/src/tui/mod.rs`:

- Change `use vega_core::client;` to `use vega_core::{client, headers};` (keep the other `use vega_core::...` lines).
- Replace `run_request` and its doc comment with:

```rust
/// Sends the request and summarizes the answer. A failure to get any HTTP answer comes
/// back as text for the Response pane, with every header value masked.
fn run_request(settings: &Settings, request: &RunRequest) -> Result<ResponseView, String> {
    client::execute(&settings.uri, &settings.headers, &request.body)
        .map(|response| response::view(&response))
        .map_err(|error| headers::mask(&error_chain(&error), &settings.headers))
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --workspace 2>&1 | grep -E "test result|FAILED|panicked" `
Expected: every `test result` line says `ok`; no `FAILED`.

- [ ] **Step 5: Run the full check and commit**

Run: `just fix && just check`
Expected: passes.

```bash
git add -A crates
git commit -m "ref: send a list of headers instead of one API key"
```

---

### Task 3: Read extra headers and a key header name

**Files:**
- Modify: `crates/vega-core/src/config.rs`
- Modify: `crates/vega/src/cli.rs`, `crates/vega/src/main.rs`
- Test: `crates/vega-core/src/config.rs` (unit), `crates/vega/tests/cli.rs` (integration)

**Interfaces:**
- Consumes: `headers::{parse_lines, merge, HeaderError}`, `AuthInput`, `ConfigError`.
- Produces:
  - `AuthInput { pub key: Option<String>, pub key_header: Option<String>, pub headers: Option<String> }`
  - `ConfigError::Headers(#[from] HeaderError)`
  - `Cli::key_header: Option<String>` (`--key-header NAME`)

- [ ] **Step 1: Write the failing unit tests**

In the `tests` module of `crates/vega-core/src/config.rs`, replace the `key` helper with the version below (the struct now has more fields, so the update syntax is needed and clippy's `needless_update` no longer fires), and add the new tests:

```rust
    fn key(v: &str) -> AuthInput {
        AuthInput {
            key: s(v),
            ..AuthInput::default()
        }
    }
```

```rust
    fn auth(key: Option<&str>, key_header: Option<&str>, headers: Option<&str>) -> AuthInput {
        AuthInput {
            key: key.map(str::to_string),
            key_header: key_header.map(str::to_string),
            headers: headers.map(str::to_string),
        }
    }

    fn header(name: &str, value: &str) -> Header {
        Header::new(name, value).unwrap()
    }

    #[test]
    fn the_key_header_name_can_be_changed() {
        let settings = resolve_settings(
            s("u"),
            None,
            auth(Some("k"), Some("X-Token"), None),
            FileConfig::default(),
        )
        .unwrap();
        assert_eq!(settings.headers, vec![header("X-Token", "k")]);
    }

    #[test]
    fn an_empty_key_header_name_falls_back_to_the_default() {
        let settings = resolve_settings(
            s("u"),
            None,
            auth(Some("k"), Some(""), None),
            FileConfig::default(),
        )
        .unwrap();
        assert_eq!(settings.headers, vec![header("X-API-Key", "k")]);
    }

    #[test]
    fn an_invalid_key_header_name_is_an_error_that_hides_the_key() {
        let err = resolve_settings(
            s("u"),
            None,
            auth(Some("super-secret"), Some("Bad Name"), None),
            FileConfig::default(),
        )
        .unwrap_err();
        assert!(matches!(err, ConfigError::InvalidKeyHeader));
        assert!(!err.to_string().contains("super-secret"));
    }

    #[test]
    fn headers_from_the_environment_follow_the_key_and_win_on_the_same_name() {
        let settings = resolve_settings(
            s("u"),
            None,
            auth(Some("k"), None, Some("x-api-key: other\nX-Tenant: t")),
            FileConfig::default(),
        )
        .unwrap();
        assert_eq!(
            settings.headers,
            vec![header("x-api-key", "other"), header("X-Tenant", "t")]
        );
    }

    #[test]
    fn headers_work_without_a_key() {
        let settings = resolve_settings(
            s("u"),
            None,
            auth(None, None, Some("Authorization: Bearer abc")),
            FileConfig::default(),
        )
        .unwrap();
        assert_eq!(settings.headers, vec![header("Authorization", "Bearer abc")]);
    }

    #[test]
    fn a_malformed_headers_variable_is_an_error_without_the_value() {
        let err = resolve_settings(
            s("u"),
            None,
            auth(None, None, Some("Authorization Bearer tok-123")),
            FileConfig::default(),
        )
        .unwrap_err();
        let message = err.to_string();
        assert!(message.contains("VEGA_GRAPHQL_HEADERS line 1"), "{message}");
        assert!(!message.contains("tok-123"), "{message}");
    }
```

- [ ] **Step 2: Write the failing integration tests**

In `crates/vega/tests/cli.rs`, add this helper after `vega_with`, and make `vega_with` also remove the two new variables by adding
`.env_remove("VEGA_GRAPHQL_HEADERS").env_remove("VEGA_GRAPHQL_KEY_HEADER")` to its `command` chain (right after the `.env("VEGA_GRAPHQL_URI", uri)` line):

```rust
/// Like `vega_with`, with any environment variables (the three credential ones are
/// removed first).
fn vega_env(cache: &TempDir, uri: &str, envs: &[(&str, &str)], args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_vega"));
    command
        .args(args)
        .env("XDG_CACHE_HOME", cache.path())
        .env("XDG_CONFIG_HOME", cache.path().join("config"))
        .env("VEGA_GRAPHQL_URI", uri)
        .env_remove("VEGA_GRAPHQL_KEY")
        .env_remove("VEGA_GRAPHQL_KEY_HEADER")
        .env_remove("VEGA_GRAPHQL_HEADERS")
        .stdin(Stdio::null());
    for (name, value) in envs {
        command.env(name, value);
    }
    command.output().unwrap()
}

const RUN_ARGS: [&str; 7] = [
    "--query",
    "Query.userRead",
    "--select",
    "id",
    "--var",
    "id=1",
    "--run",
];
```

Append these tests at the end of the file:

```rust
#[test]
fn headers_variable_sends_every_header_and_hides_the_values() {
    let (uri, rx) = serve_once("200 OK", r#"{"data":{"userRead":{"id":"echo tok-123 team-9"}}}"#);
    let cache = seeded_cache_for(&uri, 60);
    let out = vega_env(
        &cache,
        &uri,
        &[(
            "VEGA_GRAPHQL_HEADERS",
            "Authorization: Bearer tok-123\nX-Tenant: team-9",
        )],
        &RUN_ARGS,
    );
    let request = rx.recv().unwrap().to_lowercase();
    assert!(request.contains("authorization: bearer tok-123"), "{request}");
    assert!(request.contains("x-tenant: team-9"), "{request}");
    for text in [stdout(&out), stderr(&out)] {
        assert!(!text.contains("tok-123"), "{text}");
        assert!(!text.contains("team-9"), "{text}");
    }
}

#[test]
fn key_header_flag_renames_the_key_header() {
    let (uri, rx) = serve_once("200 OK", r#"{"data":{"userRead":null}}"#);
    let cache = seeded_cache_for(&uri, 60);
    let mut args = vec!["--key-header", "X-Token"];
    args.extend(RUN_ARGS);
    vega_env(&cache, &uri, &[("VEGA_GRAPHQL_KEY", "k-1")], &args);
    let request = rx.recv().unwrap().to_lowercase();
    assert!(request.contains("x-token: k-1"), "{request}");
    assert!(!request.contains("x-api-key"), "{request}");
}

#[test]
fn key_header_variable_is_read_and_the_flag_beats_it() {
    let (uri, rx) = serve_once("200 OK", r#"{"data":{"userRead":null}}"#);
    let cache = seeded_cache_for(&uri, 60);
    vega_env(
        &cache,
        &uri,
        &[
            ("VEGA_GRAPHQL_KEY", "k-1"),
            ("VEGA_GRAPHQL_KEY_HEADER", "X-From-Env"),
        ],
        &RUN_ARGS,
    );
    assert!(rx.recv().unwrap().to_lowercase().contains("x-from-env: k-1"));

    let (uri, rx) = serve_once("200 OK", r#"{"data":{"userRead":null}}"#);
    let cache = seeded_cache_for(&uri, 60);
    let mut args = vec!["--key-header", "X-From-Flag"];
    args.extend(RUN_ARGS);
    vega_env(
        &cache,
        &uri,
        &[
            ("VEGA_GRAPHQL_KEY", "k-1"),
            ("VEGA_GRAPHQL_KEY_HEADER", "X-From-Env"),
        ],
        &args,
    );
    let request = rx.recv().unwrap().to_lowercase();
    assert!(request.contains("x-from-flag: k-1"), "{request}");
    assert!(!request.contains("x-from-env"), "{request}");
}

#[test]
fn an_explicit_header_overrides_the_key_on_the_same_name() {
    let (uri, rx) = serve_once("200 OK", r#"{"data":{"userRead":null}}"#);
    let cache = seeded_cache_for(&uri, 60);
    vega_env(
        &cache,
        &uri,
        &[
            ("VEGA_GRAPHQL_KEY", "from-key"),
            ("VEGA_GRAPHQL_HEADERS", "x-api-key: from-headers"),
        ],
        &RUN_ARGS,
    );
    let request = rx.recv().unwrap().to_lowercase();
    assert!(request.contains("x-api-key: from-headers"), "{request}");
    assert!(!request.contains("from-key"), "{request}");
}

#[test]
fn a_malformed_headers_variable_fails_without_printing_the_value() {
    let cache = seeded_cache(60);
    let out = vega_env(
        &cache,
        URI,
        &[("VEGA_GRAPHQL_HEADERS", "Authorization Bearer tok-123")],
        &["--search", "x"],
    );
    assert!(!out.status.success());
    assert!(stderr(&out).contains("VEGA_GRAPHQL_HEADERS line 1"), "{}", stderr(&out));
    assert!(!stderr(&out).contains("tok-123"), "{}", stderr(&out));
    assert_eq!(stdout(&out), "");
}

#[test]
fn help_mentions_the_header_variables() {
    let cache = seeded_cache(60);
    let out = vega_env(&cache, URI, &[], &["--help"]);
    let help = stdout(&out);
    assert!(help.contains("VEGA_GRAPHQL_KEY"), "{help}");
    assert!(help.contains("VEGA_GRAPHQL_HEADERS"), "{help}");
    assert!(help.contains("--key-header"), "{help}");
}
```

Also add one regression test that the previous behavior still holds with the new variables unset, next to the others:

```rust
#[test]
fn the_key_alone_still_goes_in_the_x_api_key_header() {
    let (uri, rx) = serve_once("200 OK", r#"{"data":{"userRead":null}}"#);
    let cache = seeded_cache_for(&uri, 60);
    vega_env(&cache, &uri, &[("VEGA_GRAPHQL_KEY", "k-1")], &RUN_ARGS);
    assert!(rx.recv().unwrap().to_lowercase().contains("x-api-key: k-1"));
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --workspace 2>&1 | grep -E "^error|FAILED" | head`
Expected: compile errors for the new `AuthInput` fields (`key_header`, `headers`).

- [ ] **Step 4: Implement**

`crates/vega-core/src/config.rs`:

- Replace `AuthInput` with:

```rust
/// Credentials as read from the environment and flags. It has no `Debug` on purpose.
#[derive(Default)]
pub struct AuthInput {
    /// `VEGA_GRAPHQL_KEY`.
    pub key: Option<String>,
    /// `--key-header`, else `VEGA_GRAPHQL_KEY_HEADER`.
    pub key_header: Option<String>,
    /// `VEGA_GRAPHQL_HEADERS`: one `Name: value` per line.
    pub headers: Option<String>,
}
```

- Change the `use crate::headers::{Header, DEFAULT_KEY_HEADER};` line to `use crate::headers::{self, Header, HeaderError, DEFAULT_KEY_HEADER};`.
- Add to `ConfigError`, after `InvalidKeyHeader`:

```rust
    #[error(transparent)]
    Headers(#[from] HeaderError),
```

- Replace the header-building part of `resolve_settings` (from `let mut headers = Vec::new();` to `Ok(Settings { uri, headers })`) with:

```rust
    let mut headers = Vec::new();
    if let Some(key) = non_empty(auth.key) {
        let name = non_empty(auth.key_header).unwrap_or_else(|| DEFAULT_KEY_HEADER.to_string());
        let header = Header::new(name, key).map_err(|_| ConfigError::InvalidKeyHeader)?;
        headers.push(header);
    }
    if let Some(text) = auth.headers {
        headers = headers::merge(headers, headers::parse_lines(&text)?);
    }
    Ok(Settings { uri, headers })
```

`crates/vega/src/cli.rs`: replace the doc comment of `Cli` and add the field after `uri`:

```rust
/// Explore a GraphQL server's schema from the terminal.
///
/// Without --search, --describe or --query, opens the interactive browser.
///
/// Credentials are read from the environment only, because command line arguments are
/// visible to other users of the machine. VEGA_GRAPHQL_KEY holds an API key, sent in the
/// X-API-Key header (or the one named by --key-header or VEGA_GRAPHQL_KEY_HEADER).
/// VEGA_GRAPHQL_HEADERS holds any other headers, one `Name: value` per line, and wins on
/// the same name. See docs/authentication.md.
```

```rust
    /// Header name that carries VEGA_GRAPHQL_KEY (default X-API-Key; overrides
    /// VEGA_GRAPHQL_KEY_HEADER)
    #[arg(long, value_name = "NAME")]
    pub key_header: Option<String>,
```

`crates/vega/src/main.rs`: replace the `AuthInput { ... }` argument with:

```rust
        AuthInput {
            key: env_var("VEGA_GRAPHQL_KEY"),
            key_header: cli
                .key_header
                .clone()
                .or_else(|| env_var("VEGA_GRAPHQL_KEY_HEADER")),
            headers: env_var("VEGA_GRAPHQL_HEADERS"),
        },
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --workspace 2>&1 | grep -E "test result|FAILED|panicked"`
Expected: all `ok`.

- [ ] **Step 6: Run the full check and commit**

Run: `just fix && just check`
Expected: passes.

```bash
git add -A crates
git commit -m "feat: read extra headers and a key header name"
```

---

### Task 4: Documentation and rules

**Files:**
- Create: `docs/authentication.md`
- Modify: `README.md`, `AGENTS.md`, `docs/superpowers/specs/2026-10-21-vega-design.md`, `docs/issues/0004-paste-api-key-in-app.md`

- [ ] **Step 1: Write `docs/authentication.md`**

````markdown
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

Vega never writes a header value to disk, the cache or logs, and masks every value (and, for values such as `Bearer abc`, the part after the space) in the Response pane, in `--run` output and in error messages.

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

## Troubleshooting

- `server answered HTTP 401` or `403`: the header name or value is wrong. Check which header the server expects.
- `VEGA_GRAPHQL_HEADERS line N: expected `Name: value``: line N has no colon, an empty value or an invalid header name (names allow letters, digits and ``!#$%&'*+-.^_`|~``).
- `the key header name is not valid`: fix `--key-header` or `VEGA_GRAPHQL_KEY_HEADER`.
````

- [ ] **Step 2: Update the README**

In `README.md`:

1. Replace the sentence ending the second paragraph, `it works with any server that allows introspection and, optionally, authenticates with an API key sent in the `X-API-Key` header.`, with: `it works with any server that allows introspection and, optionally, authenticates with an API key, a bearer token, basic authentication or custom HTTP headers (see [Authentication](docs/authentication.md)).`
2. In the Features list, replace the bullet starting `- credentials are read only from the environment:` with: `- credentials are read only from the environment (API key, bearer token, basic authentication or any HTTP headers, see [Authentication](docs/authentication.md)): they are never written to disk, the cache or logs, and they never appear in the response pane or in `--run` output.`
3. In the Planned list, replace `- other forms of login besides the API key.` with `- interactive logins such as OAuth 2.0.`
4. Replace the Configuration table rows and add the new ones so the table reads:

```markdown
| Setting         | Flag           | Environment variable      | Config file            |
| --------------- | -------------- | ------------------------- | ---------------------- |
| Endpoint URL    | `--uri`        | `VEGA_GRAPHQL_URI`        | `uri` in `config.toml` |
| API key         | none           | `VEGA_GRAPHQL_KEY`        | not supported          |
| API key header  | `--key-header` | `VEGA_GRAPHQL_KEY_HEADER` | not supported          |
| Other headers   | none           | `VEGA_GRAPHQL_HEADERS`    | not supported          |

Secrets have no flag on purpose, and `VEGA_GRAPHQL_HEADERS` holds one `Name: value` per line. See [Authentication](docs/authentication.md) for the recipes.
```

- [ ] **Step 3: Update `AGENTS.md`**

Replace the first bullet under `## Rules that must not be broken` with:

```markdown
- Header values (the API key from `VEGA_GRAPHQL_KEY` and every header in `VEGA_GRAPHQL_HEADERS`) come only from the environment. Never write one to disk, the cache, logs or error messages, and never add a CLI flag for a value (a flag that names a header, such as `--key-header`, is fine). No header value appears in the Response pane or in `--run` output: the client masks them all.
```

- [ ] **Step 4: Update the 2026-10-21 design spec**

In `docs/superpowers/specs/2026-10-21-vega-design.md`, in the "Configuration and credentials" section, replace the lines starting `- Environment variables:`, `- The API key is sent as` and `- The API key is never written` with:

```markdown
- Environment variables: `VEGA_GRAPHQL_URI` (endpoint), `VEGA_GRAPHQL_KEY` (API key), `VEGA_GRAPHQL_KEY_HEADER` (header that carries the key, default `X-API-Key`) and `VEGA_GRAPHQL_HEADERS` (other headers, one `Name: value` per line). See `docs/authentication.md`.
- Requests are a `POST` with `Content-Type: application/json` and these headers, as in `curl -X POST -H "Content-Type: application/json" -H "X-API-Key: $VEGA_GRAPHQL_KEY" -d @- "$VEGA_GRAPHQL_URI"`.
- Header values are never written to disk, the cache, or logs, and are masked in everything shown to the user.
```

- [ ] **Step 5: Mark the issue as in progress**

In `docs/issues/0004-paste-api-key-in-app.md` change the line `Status: open (needs brainstorming)` to `Status: doing`.

- [ ] **Step 6: Check and commit**

Run: `just check && just issues doing`
Expected: passes; `just issues doing` lists `0004`.

```bash
git add README.md AGENTS.md docs
git commit -m "doc: describe the authentication headers"
```

---

### Task 5: Bump the version

**Files:**
- Modify: `Cargo.toml`, `crates/vega/Cargo.toml`, `Cargo.lock`

- [ ] **Step 1: Bump**

```bash
sed -i 's/^version = "0.4.0"/version = "0.5.0"/' Cargo.toml
sed -i 's/vega-core = { version = "0.4.0"/vega-core = { version = "0.5.0"/' crates/vega/Cargo.toml
cargo check --workspace
```

Expected: `Cargo.lock` now lists `vega` and `vega-core` at `0.5.0`.

- [ ] **Step 2: Verify and commit**

Run: `git diff --stat && just check`
Expected: three files changed; passes.

```bash
git add Cargo.toml crates/vega/Cargo.toml Cargo.lock
git commit -m "chore: bump the version to 0.5.0"
```

---

## Self-review notes

- Spec coverage for part 1: sources and precedence (Task 3), merge rule and key header (Tasks 1 and 3), malformed-line error (Tasks 1 and 3), masking of every value (Tasks 1 and 2, integration in Task 3), no secret flags (Task 3, `--key-header` only), rule and doc changes (Task 4), version bump (Task 5). The modal, `Ctrl-K`, startup without a URI and Basic/Bearer form helpers are part 2.
- The existing integration tests that pass the key through `VEGA_GRAPHQL_KEY` (`run_sends_the_api_key_and_never_prints_it`, `run_never_prints_the_api_key_when_the_connection_fails`) keep covering the unchanged behavior.
- Types used across tasks: `Header::new`, `headers::{merge, parse_lines, mask}`, `AuthInput { key, key_header, headers }`, `Settings { uri, headers }`, `ConfigError::{InvalidKeyHeader, Headers}`.
