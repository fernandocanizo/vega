# Connection Modal Implementation Plan (part 2 of 2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the connection modal to the TUI (endpoint plus None / Basic / API Key / Bearer / Custom headers), opened with `Ctrl-K` or automatically when no endpoint is configured, and let the TUI start with nothing configured by loading the schema in the background.

**Architecture:** `tui/connection.rs` is a pure state machine (key in, state out) whose output is a `vega_core::config::Settings`. `App` owns the current `Settings`, opens the modal, applies its result and reports what the event loop must do (`Action::Load` for a new endpoint, `Action::Refresh` to retry). The event loop loads schemas in background threads, tags each load with a sequence number so stale results are dropped, and enables bracketed paste.

**Tech Stack:** Rust 2021, `ratatui` 0.30 (`crossterm` re-export), `base64` 0.22 (already in `Cargo.lock`), `just check`.

This is part 2 of one pull request. Part 1 is `docs/superpowers/plans/2026-10-24-auth-headers.md` (already implemented, version already bumped to 0.5.0; this part does **not** bump the version again).

## Global Constraints

From `docs/superpowers/specs/2026-10-24-connection-modal-design.md` and `AGENTS.md`:

- Types are mutually exclusive; list order: None, Basic, API Key, Bearer, Custom headers.
- A URI field sits above the split and is always visible. The left pane lists the types, the right pane shows the form of the selected one.
- `Ctrl-K` opens the modal from any mode and is listed in both help tables and in the hint line. `Ctrl-T` reveals or masks the focused field; everything is masked when the modal opens. Bracketed paste goes only to the focused field, trailing newline stripped, nothing echoed. `Enter` submits, `Esc` cancels.
- With no URI the TUI opens the modal at startup and does not fail. A missing key never opens the modal. Non-TUI commands keep their clear error for a missing URI and never open a modal.
- Submitting: only headers changed keeps schema, query and variables (and retries the load at once if the schema had failed to load); URI changed is a new connection (schema from the cache keyed per URI, else the server; selection, query and variables cleared); `Esc` changes nothing; at startup with no URI `Esc` leaves an empty screen with a hint for `Ctrl-K`; None removes all headers.
- Values from the modal live in memory only, are never written to disk, the cache, logs or error messages, and every header value is masked in everything shown to the user.
- TUI logic is a pure state machine testable without a terminal. No `unwrap`/`expect` in non-test code. Tests never use the real network.
- Commits: `tag: what it does`, imperative, at most 72 characters, tags `feat`, `fix`, `ref`, `doc`, `test`, `chore`. Every commit passes `just check`. No AI attribution lines.
- Warnings and errors go to stderr for CLI commands; stdout stays machine-readable.

Run workspace tests with `cargo test --workspace --no-fail-fast 2>&1 | grep -aE "passed|failed"` (the shell output filter can hide the `test result` lines).

---

### Task 6: Amend the design spec

The code review found two things the spec did not say: the TUI must start without a schema, and the modal needs to know whether the current headers are "just the key". This task records them before any code is written.

**Files:**
- Modify: `docs/superpowers/specs/2026-10-24-connection-modal-design.md`

- [ ] **Step 1: Replace the "Startup" section**

Replace the whole `## Startup` section (its heading and the three bullets under it) with:

```markdown
## Startup

- With no URI, the TUI opens the modal right away. It does not fail.
- The TUI no longer loads the schema before it starts. It starts at once and loads the schema in a background thread: from the cache when one exists and `--refresh` is not given, otherwise from the server. The status bar says the schema is loading. A failed load (a 401, say) does not stop the program: the status bar shows the first line of the error with hints for `Ctrl-K` (connection) and `Ctrl-R` (retry).
- A missing key never opens the modal, because a public server needs none.
- Non-TUI commands (`--search`, `--describe`, `--query`, `--run`) keep a clear error for a missing URI, as described in issue 0010, still load the schema before they run, and treat a failed load as an error. They never open a modal.
```

- [ ] **Step 2: Replace the "Start state" bullet**

In the `## The modal` section, replace the bullet that starts `- Start state follows the environment:` and the bullet after it that starts `- Basic or Bearer values that come from the environment` with this single bullet:

```markdown
- Start state follows the effective settings. No headers: None. Headers that are exactly the API key (`VEGA_GRAPHQL_KEY` alone, or a submitted API Key form): API Key, pre-filled. Any other headers: Custom headers, one row per header with values masked. Basic or Bearer values set through `VEGA_GRAPHQL_HEADERS`, or submitted earlier in the modal, are plain `Authorization` headers and therefore reopen as Custom headers rows. The distinction is stored as `Settings::key_only`, and `Settings` is also what the modal returns.
```

- [ ] **Step 3: Add the paste rule**

In the `Keys:` list of the same section, after the bracketed paste bullet, add:

```markdown
- Outside the modal, a paste is typed into the search box or a text variable editor and ignored everywhere else, so pasted text can never trigger a command key.
```

- [ ] **Step 4: Check and commit**

Run: `just check`
Expected: passes.

```bash
git add docs/superpowers/specs/2026-10-24-connection-modal-design.md
git commit -m "doc: record background loading in the connection modal design"
```

---

### Task 7: Basic and Bearer header helpers

**Files:**
- Modify: `crates/vega-core/Cargo.toml` (add `base64 = "0.22.1"` under `[dependencies]`)
- Modify: `crates/vega-core/src/headers.rs`
- Modify: `Cargo.lock` (updated by cargo)

**Interfaces:**
- Produces:
  - `pub fn bearer(token: &str) -> Header` (`Authorization: Bearer <token>`)
  - `pub fn basic(user: &str, password: &str) -> Header` (`Authorization: Basic <base64 of user:password>`; the caller rejects a user name containing `:`)

- [ ] **Step 1: Write the failing tests**

Add inside `mod tests` of `crates/vega-core/src/headers.rs`:

```rust
    #[test]
    fn bearer_builds_the_authorization_header() {
        let header = bearer("abc");
        assert_eq!(header.name(), "Authorization");
        assert_eq!(header.value(), "Bearer abc");
    }

    #[test]
    fn basic_encodes_user_and_password_in_base64() {
        // The example of RFC 7617.
        let header = basic("Aladdin", "open sesame");
        assert_eq!(header.name(), "Authorization");
        assert_eq!(header.value(), "Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ==");
    }

    #[test]
    fn basic_encodes_non_ascii_text_as_utf8() {
        assert_eq!(basic("user", "pé").value(), "Basic dXNlcjpww6k=");
    }

    #[test]
    fn basic_allows_colons_and_empty_passwords_in_the_password() {
        assert_eq!(basic("user", "").value(), "Basic dXNlcjo=");
        assert_eq!(basic("user", "a:b").value(), "Basic dXNlcjphOmI=");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p vega-core headers 2>&1 | grep -aE "^error|cannot find" | head`
Expected: `cannot find function bearer` / `basic`.

- [ ] **Step 3: Implement**

In `crates/vega-core/Cargo.toml`, add to `[dependencies]` (keep alphabetical order):

```toml
base64 = "0.22.1"
```

In `crates/vega-core/src/headers.rs`, add the import after `use std::{cmp::Reverse, fmt};`:

```rust
use base64::{engine::general_purpose::STANDARD, Engine};
```

and add these functions after `merge`:

```rust
/// `Authorization: Bearer <token>`.
pub fn bearer(token: &str) -> Header {
    Header {
        name: "Authorization".to_string(),
        value: format!("Bearer {token}"),
    }
}

/// `Authorization: Basic <base64 of user:password>`. A user name cannot contain `:`
/// (RFC 7617); callers reject it before calling this.
pub fn basic(user: &str, password: &str) -> Header {
    let encoded = STANDARD.encode(format!("{user}:{password}"));
    Header {
        name: "Authorization".to_string(),
        value: format!("Basic {encoded}"),
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p vega-core headers 2>&1 | grep -aE "passed|failed"`
Expected: all pass, 0 failed.

- [ ] **Step 5: Check and commit**

Run: `just fix && just check`
Expected: passes. `git diff --stat Cargo.lock` shows only the `vega-core` dependency list gaining `base64`.

```bash
git add crates/vega-core/Cargo.toml crates/vega-core/src/headers.rs Cargo.lock
git commit -m "feat: add Basic and Bearer header builders"
```

---

### Task 8: Settings remember whether the headers are just the key, and allow a missing URI

**Files:**
- Modify: `crates/vega-core/src/config.rs`
- Modify: `crates/vega/src/tui/mod.rs` (only the `settings` test helper)

**Interfaces:**
- Consumes: `AuthInput`, `headers::{merge, parse_lines}`.
- Produces:
  - `Settings { pub uri: String, pub headers: Vec<Header>, pub key_only: bool }` (`key_only` is true when `headers` is exactly the single API key header). An empty `uri` means "not set".
  - `pub fn resolve_settings_partial(flag_uri, env_uri, auth: AuthInput, file: FileConfig) -> Result<Settings, ConfigError>` (never fails for a missing URI)
  - `resolve_settings` keeps its signature and still returns `ConfigError::MissingUri`.

- [ ] **Step 1: Write the failing tests**

In `crates/vega-core/src/config.rs`:

1. In the existing test `debug_output_hides_the_header_values`, add `key_only: true,` to the `Settings { .. }` literal.
2. Add these tests to `mod tests`:

```rust
    #[test]
    fn key_only_is_true_for_the_key_alone_and_false_otherwise() {
        let key_only = resolve_settings(
            s("u"),
            None,
            auth(Some("k"), None, None),
            FileConfig::default(),
        )
        .unwrap();
        assert!(key_only.key_only);

        let with_headers = resolve_settings(
            s("u"),
            None,
            auth(Some("k"), None, Some("X-Tenant: t")),
            FileConfig::default(),
        )
        .unwrap();
        assert!(!with_headers.key_only);

        let headers_only = resolve_settings(
            s("u"),
            None,
            auth(None, None, Some("X-Tenant: t")),
            FileConfig::default(),
        )
        .unwrap();
        assert!(!headers_only.key_only);

        let nothing = resolve_settings(s("u"), None, auth(None, None, None), FileConfig::default())
            .unwrap();
        assert!(!nothing.key_only);
        assert!(nothing.headers.is_empty());
    }

    #[test]
    fn an_empty_headers_variable_keeps_the_key_only() {
        let settings = resolve_settings(
            s("u"),
            None,
            auth(Some("k"), None, Some("\n")),
            FileConfig::default(),
        )
        .unwrap();
        assert!(settings.key_only);
    }

    #[test]
    fn partial_settings_allow_a_missing_uri_and_keep_the_headers() {
        let settings = resolve_settings_partial(
            None,
            None,
            auth(Some("k"), None, None),
            FileConfig::default(),
        )
        .unwrap();
        assert_eq!(settings.uri, "");
        assert_eq!(settings.headers, vec![header("X-API-Key", "k")]);
        assert!(settings.key_only);
    }

    #[test]
    fn partial_settings_still_reject_malformed_headers() {
        let err = resolve_settings_partial(
            None,
            None,
            auth(None, None, Some("no colon here")),
            FileConfig::default(),
        )
        .unwrap_err();
        assert!(matches!(err, ConfigError::Headers(_)));
    }
```

3. In `crates/vega/src/tui/mod.rs` (`mod tests`, the `settings` helper) add `key_only: false,` to the `Settings { .. }` literal.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --workspace --no-fail-fast 2>&1 | grep -aE "^error" | head`
Expected: `no field key_only` / `cannot find function resolve_settings_partial`.

- [ ] **Step 3: Implement**

In `crates/vega-core/src/config.rs`:

- Replace `Settings` with:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// The endpoint; empty when none is configured yet (only the TUI allows that).
    pub uri: String,
    /// Sent with every request. Values are secrets (`Header`'s `Debug` hides them).
    pub headers: Vec<Header>,
    /// True when `headers` is exactly the single API key header, which the connection
    /// modal shows on its API Key form.
    pub key_only: bool,
}
```

- Replace `resolve_settings` with these two functions:

```rust
/// Like `resolve_settings`, but a missing endpoint is not an error: `uri` is empty.
pub fn resolve_settings_partial(
    flag_uri: Option<String>,
    env_uri: Option<String>,
    auth: AuthInput,
    file: FileConfig,
) -> Result<Settings, ConfigError> {
    let uri = non_empty(flag_uri)
        .or_else(|| non_empty(env_uri))
        .or_else(|| non_empty(file.uri))
        .unwrap_or_default();
    let mut headers = Vec::new();
    let mut key_only = false;
    if let Some(key) = non_empty(auth.key) {
        let name = non_empty(auth.key_header).unwrap_or_else(|| DEFAULT_KEY_HEADER.to_string());
        let header = Header::new(name, key).map_err(|_| ConfigError::InvalidKeyHeader)?;
        headers.push(header);
        key_only = true;
    }
    if let Some(text) = auth.headers {
        let extra = headers::parse_lines(&text)?;
        if !extra.is_empty() {
            key_only = false;
            headers = headers::merge(headers, extra);
        }
    }
    Ok(Settings {
        uri,
        headers,
        key_only,
    })
}

pub fn resolve_settings(
    flag_uri: Option<String>,
    env_uri: Option<String>,
    auth: AuthInput,
    file: FileConfig,
) -> Result<Settings, ConfigError> {
    let settings = resolve_settings_partial(flag_uri, env_uri, auth, file)?;
    if settings.uri.is_empty() {
        return Err(ConfigError::MissingUri);
    }
    Ok(settings)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --workspace --no-fail-fast 2>&1 | grep -aE "passed|failed"`
Expected: all pass, 0 failed (the existing precedence and error tests keep passing).

- [ ] **Step 5: Check and commit**

Run: `just fix && just check`
Expected: passes.

```bash
git add crates/vega-core/src/config.rs crates/vega/src/tui/mod.rs
git commit -m "feat: let settings allow a missing endpoint and track the key form"
```

---

### Task 9: The connection modal state machine

The module is complete and tested here but not wired in yet, so it is allowed to be unused until Task 11 (the `#[allow(dead_code)]` is removed there).

**Files:**
- Create: `crates/vega/src/tui/connection.rs`
- Modify: `crates/vega/src/tui/mod.rs` (add the module declaration)

**Interfaces:**
- Consumes: `config::Settings`, `headers::{Header, basic, bearer, merge, DEFAULT_KEY_HEADER}`.
- Produces (used by Tasks 10 and 11):
  - `pub enum AuthType { NoAuth, Basic, ApiKey, Bearer, CustomHeaders }` with `AuthType::ALL: [AuthType; 5]` (list order) and `label(self) -> &'static str`.
  - `pub enum Focus { Uri, Types, Field(usize) }` (`Copy`).
  - `pub struct Field` with `label() -> &'static str`, `is_secret() -> bool`, `is_revealed() -> bool`, `display() -> String` (bullets for an unrevealed secret). No `Debug` on purpose.
  - `pub enum Outcome { Open, Cancel, Submit(Settings) }`.
  - `pub struct Modal` (no `Debug`) with `Modal::new(&Settings) -> Modal`, `handle_key(&mut self, KeyEvent) -> Outcome`, `paste(&mut self, &str)`, `uri() -> &Field`, `selected() -> AuthType`, `focus() -> Focus`, `fields() -> &[Field]`, `error() -> Option<&str>`.

- [ ] **Step 1: Write the failing tests**

Create `crates/vega/src/tui/connection.rs` containing only this test module (it will not compile yet), and add to `crates/vega/src/tui/mod.rs`, after `pub mod app;`:

```rust
#[allow(dead_code)] // wired into the app in a later commit
pub mod connection;
```

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn settings(uri: &str, headers: Vec<Header>, key_only: bool) -> Settings {
        Settings {
            uri: uri.to_string(),
            headers,
            key_only,
        }
    }

    fn header(name: &str, value: &str) -> Header {
        Header::new(name, value).unwrap()
    }

    /// A modal with an endpoint set and no credentials: focus starts on the type list.
    fn modal() -> Modal {
        Modal::new(&settings("https://x.example.com/graphql", vec![], false))
    }

    fn press(modal: &mut Modal, code: KeyCode) -> Outcome {
        modal.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn ctrl(modal: &mut Modal, c: char) -> Outcome {
        modal.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
    }

    fn type_text(modal: &mut Modal, text: &str) {
        for c in text.chars() {
            press(modal, KeyCode::Char(c));
        }
    }

    /// Moves the type list to `target` and focuses its first field.
    fn pick(modal: &mut Modal, target: AuthType) {
        while modal.selected() != target {
            press(modal, KeyCode::Down);
        }
        if !modal.fields().is_empty() {
            press(modal, KeyCode::Tab);
        }
    }

    fn submitted(outcome: Outcome) -> Settings {
        match outcome {
            Outcome::Submit(settings) => settings,
            Outcome::Open => panic!("still open"),
            Outcome::Cancel => panic!("cancelled"),
        }
    }

    #[test]
    fn list_indexes_match_the_type_order() {
        assert_eq!(AuthType::ALL[API_KEY], AuthType::ApiKey);
        assert_eq!(AuthType::ALL[CUSTOM], AuthType::CustomHeaders);
        let labels: Vec<_> = AuthType::ALL.iter().map(|t| t.label()).collect();
        assert_eq!(
            labels,
            ["None", "Basic", "API Key", "Bearer", "Custom headers"]
        );
    }

    #[test]
    fn no_headers_opens_on_none_and_focuses_the_uri_only_when_it_is_empty() {
        let with_uri = modal();
        assert_eq!(with_uri.selected(), AuthType::NoAuth);
        assert_eq!(with_uri.focus(), Focus::Types);
        assert_eq!(with_uri.uri().display(), "https://x.example.com/graphql");

        let without = Modal::new(&settings("", vec![], false));
        assert_eq!(without.focus(), Focus::Uri);
    }

    #[test]
    fn the_key_alone_opens_on_api_key_prefilled() {
        let start = settings("https://x/graphql", vec![header("X-Token", "k-1")], true);
        let mut modal = Modal::new(&start);
        assert_eq!(modal.selected(), AuthType::ApiKey);
        assert_eq!(modal.fields()[0].display(), "X-Token");
        assert_eq!(submitted(press(&mut modal, KeyCode::Enter)), start);
    }

    #[test]
    fn other_headers_open_on_custom_headers_one_row_each() {
        let start = settings(
            "https://x/graphql",
            vec![header("Authorization", "Bearer abc"), header("X-Tenant", "t")],
            false,
        );
        let mut modal = Modal::new(&start);
        assert_eq!(modal.selected(), AuthType::CustomHeaders);
        assert_eq!(modal.fields().len(), 4);
        assert_eq!(modal.fields()[0].display(), "Authorization");
        assert_eq!(submitted(press(&mut modal, KeyCode::Enter)), start);
    }

    #[test]
    fn secret_fields_show_bullets_until_revealed() {
        let mut modal = modal();
        pick(&mut modal, AuthType::Bearer);
        type_text(&mut modal, "tok-123");
        assert_eq!(modal.fields()[0].display(), "•••••••");
        assert!(!modal.fields()[0].is_revealed());
        ctrl(&mut modal, 't');
        assert_eq!(modal.fields()[0].display(), "tok-123");
        ctrl(&mut modal, 't');
        assert_eq!(modal.fields()[0].display(), "•••••••");
    }

    #[test]
    fn reveal_does_nothing_on_plain_fields() {
        let mut modal = modal();
        pick(&mut modal, AuthType::Basic);
        type_text(&mut modal, "alice");
        ctrl(&mut modal, 't');
        assert_eq!(modal.fields()[0].display(), "alice");
        assert!(!modal.fields()[0].is_revealed());
    }

    #[test]
    fn none_submits_the_uri_without_headers() {
        let mut modal = modal();
        let result = submitted(press(&mut modal, KeyCode::Enter));
        assert_eq!(result, settings("https://x.example.com/graphql", vec![], false));
    }

    #[test]
    fn a_missing_uri_is_an_error_and_the_modal_stays_open() {
        let mut modal = Modal::new(&settings("", vec![], false));
        assert_eq!(press(&mut modal, KeyCode::Enter), Outcome::Open);
        assert_eq!(modal.error(), Some("The URI is required"));
        press(&mut modal, KeyCode::Char('x'));
        assert_eq!(modal.error(), None);
    }

    #[test]
    fn typing_in_the_uri_field_edits_it() {
        let mut modal = Modal::new(&settings("", vec![], false));
        type_text(&mut modal, " https://y/graphql ");
        press(&mut modal, KeyCode::Backspace);
        press(&mut modal, KeyCode::Backspace);
        press(&mut modal, KeyCode::Char('l'));
        let result = submitted(press(&mut modal, KeyCode::Enter));
        assert_eq!(result.uri, "https://y/graphql");
    }

    #[test]
    fn basic_builds_the_authorization_header() {
        let mut modal = modal();
        pick(&mut modal, AuthType::Basic);
        type_text(&mut modal, "Aladdin");
        press(&mut modal, KeyCode::Tab);
        type_text(&mut modal, "open sesame");
        let result = submitted(press(&mut modal, KeyCode::Enter));
        assert_eq!(
            result.headers,
            vec![header("Authorization", "Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ==")]
        );
        assert!(!result.key_only);
    }

    #[test]
    fn basic_rejects_a_missing_user_and_a_user_with_a_colon() {
        let mut modal = modal();
        pick(&mut modal, AuthType::Basic);
        assert_eq!(press(&mut modal, KeyCode::Enter), Outcome::Open);
        assert_eq!(modal.error(), Some("The username is required"));
        type_text(&mut modal, "a:b");
        assert_eq!(press(&mut modal, KeyCode::Enter), Outcome::Open);
        assert_eq!(modal.error(), Some("The username cannot contain ':'"));
    }

    #[test]
    fn api_key_defaults_to_x_api_key_and_marks_key_only() {
        let mut modal = modal();
        pick(&mut modal, AuthType::ApiKey);
        assert_eq!(modal.fields()[0].display(), "X-API-Key");
        press(&mut modal, KeyCode::Tab);
        type_text(&mut modal, "secret-1");
        let result = submitted(press(&mut modal, KeyCode::Enter));
        assert_eq!(result.headers, vec![header("X-API-Key", "secret-1")]);
        assert!(result.key_only);
    }

    #[test]
    fn api_key_needs_a_value_and_a_valid_name_without_echoing_either() {
        let mut modal = modal();
        pick(&mut modal, AuthType::ApiKey);
        assert_eq!(press(&mut modal, KeyCode::Enter), Outcome::Open);
        assert_eq!(modal.error(), Some("The API key is required"));
        press(&mut modal, KeyCode::Tab);
        type_text(&mut modal, "secret-1");
        press(&mut modal, KeyCode::BackTab);
        type_text(&mut modal, " bad");
        assert_eq!(press(&mut modal, KeyCode::Enter), Outcome::Open);
        let message = modal.error().unwrap().to_string();
        assert_eq!(message, "The header name is not valid");
        assert!(!message.contains("secret-1"));
    }

    #[test]
    fn bearer_builds_the_authorization_header_and_trims_the_token() {
        let mut modal = modal();
        pick(&mut modal, AuthType::Bearer);
        type_text(&mut modal, "  tok-123 ");
        let result = submitted(press(&mut modal, KeyCode::Enter));
        assert_eq!(result.headers, vec![header("Authorization", "Bearer tok-123")]);
    }

    #[test]
    fn bearer_needs_a_token() {
        let mut modal = modal();
        pick(&mut modal, AuthType::Bearer);
        assert_eq!(press(&mut modal, KeyCode::Enter), Outcome::Open);
        assert_eq!(modal.error(), Some("The token is required"));
    }

    #[test]
    fn custom_headers_skip_empty_rows_and_keep_the_rest_in_order() {
        let mut modal = modal();
        pick(&mut modal, AuthType::CustomHeaders);
        type_text(&mut modal, "X-One");
        press(&mut modal, KeyCode::Tab);
        type_text(&mut modal, "1");
        ctrl(&mut modal, 'n'); // an empty second row
        ctrl(&mut modal, 'n'); // a third row, filled in
        type_text(&mut modal, "X-Three");
        press(&mut modal, KeyCode::Tab);
        type_text(&mut modal, "3");
        let result = submitted(press(&mut modal, KeyCode::Enter));
        assert_eq!(
            result.headers,
            vec![header("X-One", "1"), header("X-Three", "3")]
        );
        assert!(!result.key_only);
    }

    #[test]
    fn ctrl_n_adds_a_row_and_focuses_its_name() {
        let mut modal = modal();
        pick(&mut modal, AuthType::CustomHeaders);
        assert_eq!(modal.fields().len(), 2);
        ctrl(&mut modal, 'n');
        assert_eq!(modal.fields().len(), 4);
        assert_eq!(modal.focus(), Focus::Field(2));
    }

    #[test]
    fn ctrl_n_does_nothing_on_other_types() {
        let mut modal = modal();
        pick(&mut modal, AuthType::Bearer);
        ctrl(&mut modal, 'n');
        assert_eq!(modal.fields().len(), 1);
    }

    #[test]
    fn custom_rows_report_their_number_without_echoing_values() {
        let mut modal = modal();
        pick(&mut modal, AuthType::CustomHeaders);
        press(&mut modal, KeyCode::Tab);
        type_text(&mut modal, "secret-1"); // a value with no name
        assert_eq!(press(&mut modal, KeyCode::Enter), Outcome::Open);
        let message = modal.error().unwrap().to_string();
        assert_eq!(message, "Row 1: the header name is required");

        press(&mut modal, KeyCode::BackTab);
        type_text(&mut modal, "Bad Name");
        assert_eq!(press(&mut modal, KeyCode::Enter), Outcome::Open);
        let message = modal.error().unwrap().to_string();
        assert_eq!(message, "Row 1: the header name is not valid");
        assert!(!message.contains("secret-1"));
    }

    #[test]
    fn a_row_with_a_name_but_no_value_is_an_error() {
        let mut modal = modal();
        pick(&mut modal, AuthType::CustomHeaders);
        type_text(&mut modal, "X-One");
        assert_eq!(press(&mut modal, KeyCode::Enter), Outcome::Open);
        assert_eq!(modal.error(), Some("Row 1: the value is required"));
    }

    #[test]
    fn duplicate_names_keep_the_last_row() {
        let mut modal = modal();
        pick(&mut modal, AuthType::CustomHeaders);
        type_text(&mut modal, "X-A");
        press(&mut modal, KeyCode::Tab);
        type_text(&mut modal, "1");
        ctrl(&mut modal, 'n');
        type_text(&mut modal, "x-a");
        press(&mut modal, KeyCode::Tab);
        type_text(&mut modal, "2");
        let result = submitted(press(&mut modal, KeyCode::Enter));
        assert_eq!(result.headers, vec![header("x-a", "2")]);
    }

    #[test]
    fn tab_cycles_through_every_field_and_wraps() {
        let mut modal = Modal::new(&settings("", vec![], false));
        pick(&mut modal, AuthType::Basic); // Tab from Uri goes to Types first
        assert_eq!(modal.focus(), Focus::Field(0));
        press(&mut modal, KeyCode::Tab);
        assert_eq!(modal.focus(), Focus::Field(1));
        press(&mut modal, KeyCode::Tab);
        assert_eq!(modal.focus(), Focus::Uri);
        press(&mut modal, KeyCode::BackTab);
        assert_eq!(modal.focus(), Focus::Field(1));
    }

    #[test]
    fn up_and_down_move_the_type_selection_and_clamp() {
        let mut modal = modal();
        press(&mut modal, KeyCode::Up);
        assert_eq!(modal.selected(), AuthType::NoAuth);
        for _ in 0..10 {
            press(&mut modal, KeyCode::Down);
        }
        assert_eq!(modal.selected(), AuthType::CustomHeaders);
        assert_eq!(modal.focus(), Focus::Types);
    }

    #[test]
    fn up_and_down_move_between_fields_and_up_leaves_to_the_types() {
        let mut modal = modal();
        pick(&mut modal, AuthType::Basic);
        press(&mut modal, KeyCode::Down);
        assert_eq!(modal.focus(), Focus::Field(1));
        press(&mut modal, KeyCode::Down);
        assert_eq!(modal.focus(), Focus::Field(1));
        press(&mut modal, KeyCode::Up);
        press(&mut modal, KeyCode::Up);
        assert_eq!(modal.focus(), Focus::Types);
    }

    #[test]
    fn down_from_the_uri_goes_to_the_types() {
        let mut modal = Modal::new(&settings("", vec![], false));
        press(&mut modal, KeyCode::Down);
        assert_eq!(modal.focus(), Focus::Types);
    }

    #[test]
    fn each_type_keeps_its_own_values_when_switching() {
        let mut modal = modal();
        pick(&mut modal, AuthType::Bearer);
        type_text(&mut modal, "tok");
        press(&mut modal, KeyCode::Up); // back to the list
        press(&mut modal, KeyCode::Up); // API Key
        press(&mut modal, KeyCode::Down); // Bearer again
        assert_eq!(modal.selected(), AuthType::Bearer);
        assert_eq!(modal.fields()[0].display(), "•••");
    }

    #[test]
    fn paste_goes_to_the_focused_field_without_line_breaks() {
        let mut modal = modal();
        pick(&mut modal, AuthType::Bearer);
        modal.paste("tok-123\r\n");
        let result = submitted(press(&mut modal, KeyCode::Enter));
        assert_eq!(result.headers, vec![header("Authorization", "Bearer tok-123")]);
    }

    #[test]
    fn paste_is_ignored_while_the_type_list_has_focus() {
        let mut modal = modal();
        modal.paste("whatever");
        assert_eq!(modal.uri().display(), "https://x.example.com/graphql");
    }

    #[test]
    fn typing_is_ignored_while_the_type_list_has_focus() {
        let mut modal = modal();
        type_text(&mut modal, "abc");
        assert_eq!(modal.uri().display(), "https://x.example.com/graphql");
    }

    #[test]
    fn esc_cancels() {
        let mut modal = modal();
        assert_eq!(press(&mut modal, KeyCode::Esc), Outcome::Cancel);
    }

    #[test]
    fn other_control_chords_do_not_type() {
        let mut modal = Modal::new(&settings("", vec![], false));
        ctrl(&mut modal, 'x');
        assert_eq!(modal.uri().display(), "");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p vega connection 2>&1 | grep -aE "^error" | head -5`
Expected: compile errors such as `cannot find type Modal in this scope`.

- [ ] **Step 3: Implement**

Put this above the `#[cfg(test)]` module in `crates/vega/src/tui/connection.rs`:

```rust
//! The connection modal: the endpoint plus how to authenticate, as a pure state machine
//! (key in, state out). Values live in memory only: the modal is dropped when it closes,
//! which also masks every field again.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use vega_core::config::Settings;
use vega_core::headers::{self, Header, DEFAULT_KEY_HEADER};

/// How to authenticate. Each type is a form that produces the request headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthType {
    NoAuth,
    Basic,
    ApiKey,
    Bearer,
    CustomHeaders,
}

impl AuthType {
    /// In the order the list shows them.
    pub const ALL: [AuthType; 5] = [
        AuthType::NoAuth,
        AuthType::Basic,
        AuthType::ApiKey,
        AuthType::Bearer,
        AuthType::CustomHeaders,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AuthType::NoAuth => "None",
            AuthType::Basic => "Basic",
            AuthType::ApiKey => "API Key",
            AuthType::Bearer => "Bearer",
            AuthType::CustomHeaders => "Custom headers",
        }
    }
}

/// Positions in `AuthType::ALL`.
const API_KEY: usize = 2;
const CUSTOM: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Uri,
    /// The list of authentication types.
    Types,
    /// A field of the selected type's form.
    Field(usize),
}

/// One single-line text input. It has no `Debug` on purpose: it may hold a secret.
#[derive(Clone, PartialEq, Eq)]
pub struct Field {
    label: &'static str,
    value: String,
    secret: bool,
    revealed: bool,
}

impl Field {
    fn new(label: &'static str, secret: bool) -> Self {
        Self {
            label,
            value: String::new(),
            secret,
            revealed: false,
        }
    }

    fn with_value(mut self, value: &str) -> Self {
        self.value = value.to_string();
        self
    }

    pub fn label(&self) -> &'static str {
        self.label
    }

    pub fn is_secret(&self) -> bool {
        self.secret
    }

    pub fn is_revealed(&self) -> bool {
        self.revealed
    }

    /// What to draw: the text, or one bullet per character for a secret that is not
    /// revealed.
    pub fn display(&self) -> String {
        if self.secret && !self.revealed {
            "•".repeat(self.value.chars().count())
        } else {
            self.value.clone()
        }
    }
}

/// What a key did to the modal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Open,
    Cancel,
    /// The user pressed Enter on a valid form.
    Submit(Settings),
}

/// A header name field followed by its value field, for Custom headers rows.
fn pair() -> [Field; 2] {
    [Field::new("Header name", false), Field::new("Value", true)]
}

pub struct Modal {
    uri: Field,
    /// Index into `AuthType::ALL`.
    selected: usize,
    focus: Focus,
    /// One form per type, in `AuthType::ALL` order. Custom headers holds name and value
    /// fields in pairs.
    forms: [Vec<Field>; 5],
    error: Option<String>,
}

impl Modal {
    /// Opens on the current settings: no headers means None, the API key alone means the
    /// API Key form, anything else means Custom headers with one row per header.
    pub fn new(settings: &Settings) -> Self {
        let mut forms = [
            Vec::new(),
            vec![Field::new("Username", false), Field::new("Password", true)],
            vec![
                Field::new("Header name", false).with_value(DEFAULT_KEY_HEADER),
                Field::new("Value", true),
            ],
            vec![Field::new("Token", true)],
            pair().to_vec(),
        ];
        let mut selected = 0;
        match settings.headers.as_slice() {
            [] => {}
            [key] if settings.key_only => {
                selected = API_KEY;
                forms[API_KEY] = vec![
                    Field::new("Header name", false).with_value(key.name()),
                    Field::new("Value", true).with_value(key.value()),
                ];
            }
            headers => {
                selected = CUSTOM;
                forms[CUSTOM] = headers
                    .iter()
                    .flat_map(|header| {
                        let [name, value] = pair();
                        [name.with_value(header.name()), value.with_value(header.value())]
                    })
                    .collect();
            }
        }
        let focus = if settings.uri.is_empty() {
            Focus::Uri
        } else {
            Focus::Types
        };
        Self {
            uri: Field::new("URI", false).with_value(&settings.uri),
            selected,
            focus,
            forms,
            error: None,
        }
    }

    pub fn uri(&self) -> &Field {
        &self.uri
    }

    pub fn selected(&self) -> AuthType {
        AuthType::ALL[self.selected]
    }

    pub fn focus(&self) -> Focus {
        self.focus
    }

    /// The fields of the selected type's form.
    pub fn fields(&self) -> &[Field] {
        &self.forms[self.selected]
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Outcome {
        self.error = None;
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => return Outcome::Cancel,
            KeyCode::Enter => return self.submit(),
            KeyCode::Tab => self.cycle(true),
            KeyCode::BackTab => self.cycle(false),
            KeyCode::Up => self.move_vertically(false),
            KeyCode::Down => self.move_vertically(true),
            KeyCode::Backspace => {
                if let Some(field) = self.focused_mut() {
                    field.value.pop();
                }
            }
            KeyCode::Char('t') if ctrl => {
                if let Some(field) = self.focused_mut().filter(|field| field.secret) {
                    field.revealed = !field.revealed;
                }
            }
            KeyCode::Char('n') if ctrl => self.add_row(),
            KeyCode::Char(c) if !ctrl && !c.is_control() => self.insert(c.encode_utf8(&mut [0; 4])),
            _ => {}
        }
        Outcome::Open
    }

    /// A bracketed paste: goes to the focused field with line breaks removed.
    pub fn paste(&mut self, text: &str) {
        self.error = None;
        let clean: String = text.chars().filter(|c| !c.is_control()).collect();
        self.insert(&clean);
    }

    fn insert(&mut self, text: &str) {
        if let Some(field) = self.focused_mut() {
            field.value.push_str(text);
        }
    }

    fn focused_mut(&mut self) -> Option<&mut Field> {
        match self.focus {
            Focus::Uri => Some(&mut self.uri),
            Focus::Types => None,
            Focus::Field(index) => self.forms[self.selected].get_mut(index),
        }
    }

    /// Tab order: the URI, the type list, then the fields of the selected form.
    fn cycle(&mut self, forward: bool) {
        let mut order = vec![Focus::Uri, Focus::Types];
        order.extend((0..self.forms[self.selected].len()).map(Focus::Field));
        let at = order.iter().position(|f| *f == self.focus).unwrap_or(0);
        let next = if forward {
            (at + 1) % order.len()
        } else {
            (at + order.len() - 1) % order.len()
        };
        self.focus = order[next];
    }

    fn move_vertically(&mut self, down: bool) {
        match self.focus {
            Focus::Types => {
                self.selected = if down {
                    (self.selected + 1).min(AuthType::ALL.len() - 1)
                } else {
                    self.selected.saturating_sub(1)
                };
            }
            Focus::Uri => {
                if down {
                    self.focus = Focus::Types;
                }
            }
            Focus::Field(index) => {
                if !down {
                    self.focus = if index == 0 {
                        Focus::Types
                    } else {
                        Focus::Field(index - 1)
                    };
                } else if index + 1 < self.forms[self.selected].len() {
                    self.focus = Focus::Field(index + 1);
                }
            }
        }
    }

    /// Custom headers only: appends an empty row and focuses its name.
    fn add_row(&mut self) {
        if self.selected != CUSTOM {
            return;
        }
        self.forms[CUSTOM].extend(pair());
        self.focus = Focus::Field(self.forms[CUSTOM].len() - 2);
    }

    fn submit(&mut self) -> Outcome {
        match self.build() {
            Ok(settings) => Outcome::Submit(settings),
            Err(message) => {
                self.error = Some(message);
                Outcome::Open
            }
        }
    }

    /// Turns the forms into settings. Messages never contain a field value.
    fn build(&self) -> Result<Settings, String> {
        let uri = self.uri.value.trim();
        if uri.is_empty() {
            return Err("The URI is required".to_string());
        }
        let form = &self.forms[self.selected];
        let text = |index: usize| form[index].value.trim();
        let (headers, key_only) = match self.selected() {
            AuthType::NoAuth => (Vec::new(), false),
            AuthType::Basic => {
                let user = text(0);
                if user.is_empty() {
                    return Err("The username is required".to_string());
                }
                if user.contains(':') {
                    return Err("The username cannot contain ':'".to_string());
                }
                // A password may start or end with spaces, so it is not trimmed.
                (vec![headers::basic(user, &form[1].value)], false)
            }
            AuthType::ApiKey => {
                if text(1).is_empty() {
                    return Err("The API key is required".to_string());
                }
                let header = Header::new(text(0), text(1))
                    .map_err(|_| "The header name is not valid".to_string())?;
                (vec![header], true)
            }
            AuthType::Bearer => {
                if text(0).is_empty() {
                    return Err("The token is required".to_string());
                }
                (vec![headers::bearer(text(0))], false)
            }
            AuthType::CustomHeaders => {
                let mut rows = Vec::new();
                for (index, row) in form.chunks(2).enumerate() {
                    let (name, value) = (row[0].value.trim(), row[1].value.trim());
                    if name.is_empty() && value.is_empty() {
                        continue;
                    }
                    let number = index + 1;
                    if name.is_empty() {
                        return Err(format!("Row {number}: the header name is required"));
                    }
                    if value.is_empty() {
                        return Err(format!("Row {number}: the value is required"));
                    }
                    let header = Header::new(name, value)
                        .map_err(|_| format!("Row {number}: the header name is not valid"))?;
                    rows.push(header);
                }
                (headers::merge(Vec::new(), rows), false)
            }
        };
        Ok(Settings {
            uri: uri.to_string(),
            headers,
            key_only,
        })
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p vega connection 2>&1 | grep -aE "passed|failed|panicked"`
Expected: all pass, 0 failed. If a test fails because of a real mistake in this plan's code or test (not because of a mismatch with the described behavior), fix the smaller of the two and report it.

- [ ] **Step 5: Check and commit**

Run: `just fix && just check`
Expected: passes (the `#[allow(dead_code)]` on the module keeps the unused items quiet).

```bash
git add crates/vega/src/tui/connection.rs crates/vega/src/tui/mod.rs
git commit -m "feat: add the connection modal state machine"
```

---

### Task 10: Wire the modal into the app, the event loop and startup

One commit because `Action::Load`, the new `run` signature and the new `main` flow must compile together.

**Files:**
- Modify: `crates/vega/src/tui/app.rs`
- Modify: `crates/vega/src/tui/mod.rs`
- Modify: `crates/vega/src/main.rs`
- Test: inline tests in `app.rs` and `mod.rs`

**Interfaces:**
- Consumes: `Modal`, `Outcome` (Task 9), `Settings` with `key_only` and `resolve_settings_partial` (Task 8).
- Produces:
  - `Action::Load` (new endpoint: the event loop loads from the cache, else fetches).
  - `App::empty(settings: Settings) -> App`, `App::with_settings(self, Settings) -> App`, `App::settings(&self) -> &Settings`, `App::is_loaded(&self) -> bool`, `App::begin_load(&mut self)`, `App::handle_paste(&mut self, &str)`, `pub modal: Option<Modal>`.
  - `tui::run(entry: Option<CacheEntry>, settings: Settings, cache_dir: PathBuf) -> anyhow::Result<()>`.

- [ ] **Step 1: Write the failing App tests**

In `crates/vega/src/tui/app.rs`, inside `mod tests` add `use vega_core::config::Settings;` next to the other `use` lines and these tests (after the `type_text` helper):

```rust
    fn settings(uri: &str) -> Settings {
        Settings {
            uri: uri.to_string(),
            headers: Vec::new(),
            key_only: false,
        }
    }

    fn empty_app() -> App {
        App::empty(settings(""))
    }

    /// Opens the modal, picks Bearer, types `token` and presses Enter.
    fn submit_bearer(app: &mut App, token: &str) -> Action {
        ctrl(app, 'k');
        press(app, KeyCode::Down); // Basic
        press(app, KeyCode::Down); // API Key
        press(app, KeyCode::Down); // Bearer
        press(app, KeyCode::Tab); // the token field
        type_text(app, token);
        press(app, KeyCode::Enter)
    }

    #[test]
    fn an_app_without_an_endpoint_opens_the_modal_and_says_so() {
        let app = empty_app();
        assert!(app.modal.is_some());
        assert!(!app.is_loaded());
        assert_eq!(app.status(0).0, "no endpoint · Ctrl-K connection");
        assert!(app.hits.is_empty());
    }

    #[test]
    fn esc_closes_the_startup_modal_and_ctrl_r_points_to_ctrl_k() {
        let mut app = empty_app();
        press(&mut app, KeyCode::Esc);
        assert!(app.modal.is_none());
        assert_eq!(ctrl(&mut app, 'r'), Action::None);
        assert!(app.status(0).0.contains("Ctrl-K"));
        assert_eq!(app.refresh, RefreshState::Idle);
    }

    #[test]
    fn ctrl_k_opens_the_modal_and_esc_cancels_without_changes() {
        let mut app = app();
        ctrl(&mut app, 'k');
        assert!(app.modal.is_some());
        type_text(&mut app, "zzz");
        assert_eq!(app.query, "");
        press(&mut app, KeyCode::Esc);
        assert!(app.modal.is_none());
        assert_eq!(app.settings().uri, "https://nexus.example.com/graphql");
    }

    #[test]
    fn ctrl_k_works_from_design_mode_and_closes_help() {
        let mut app = design_app();
        open_me(&mut app);
        press(&mut app, KeyCode::F(1));
        ctrl(&mut app, 'k');
        assert!(app.modal.is_some());
        assert!(!app.help_open);
        assert!(app.design.is_some());
    }

    #[test]
    fn ctrl_q_quits_even_with_the_modal_open() {
        let mut app = app();
        ctrl(&mut app, 'k');
        ctrl(&mut app, 'q');
        assert!(app.should_quit);
    }

    #[test]
    fn submitting_only_new_headers_keeps_the_schema_and_the_work() {
        let mut app = design_app();
        open_me(&mut app);
        assert_eq!(submit_bearer(&mut app, "tok-1"), Action::None);
        assert!(app.modal.is_none());
        assert!(app.design.is_some());
        assert!(app.is_loaded());
        assert_eq!(app.settings().headers.len(), 1);
        assert_eq!(app.settings().headers[0].value(), "Bearer tok-1");
    }

    #[test]
    fn submitting_headers_without_a_schema_retries_the_load() {
        let mut app = App::empty(settings("https://x.example.com/graphql"));
        app.apply_refresh_err("server answered HTTP 401".to_string());
        assert!(app.status(0).0.contains("could not load the schema: server answered HTTP 401"));
        assert_eq!(submit_bearer(&mut app, "tok-1"), Action::Refresh);
        assert_eq!(app.refresh, RefreshState::Running);
        assert!(app.status(0).0.contains("loading the schema"));
    }

    #[test]
    fn changing_the_uri_starts_a_new_connection() {
        let mut app = design_app();
        open_me(&mut app);
        ctrl(&mut app, 'k');
        press(&mut app, KeyCode::BackTab); // from the type list to the URI
        type_text(&mut app, "/v2");
        assert_eq!(press(&mut app, KeyCode::Enter), Action::Load);
        assert_eq!(app.settings().uri, "https://example.com/graphql/v2");
        assert!(app.design.is_none());
        assert!(!app.is_loaded());
        assert!(app.hits.is_empty());
        assert_eq!(app.query, "");
        assert!(app.status(0).0.contains("loading the schema"));
    }

    #[test]
    fn a_loaded_schema_after_a_new_connection_makes_the_app_usable_again() {
        let mut app = empty_app();
        type_text(&mut app, "https://x/graphql");
        assert_eq!(press(&mut app, KeyCode::Enter), Action::Load);
        let response: IntrospectionResponse = serde_json::from_str(FIXTURE).unwrap();
        app.apply_refresh_ok(response.data.schema, 5_000);
        assert!(app.is_loaded());
        assert_eq!(app.hits.len(), 10);
        assert!(app.status(5_000).0.contains("x"));
    }

    #[test]
    fn paste_types_into_the_search_box() {
        let mut app = app();
        app.handle_paste("user\r\n");
        assert_eq!(app.query, "user");
    }

    #[test]
    fn paste_goes_to_the_modal_when_it_is_open() {
        let mut app = app();
        ctrl(&mut app, 'k');
        press(&mut app, KeyCode::Down); // Basic
        press(&mut app, KeyCode::Down); // API Key
        press(&mut app, KeyCode::Down); // Bearer
        press(&mut app, KeyCode::Tab);
        app.handle_paste("tok-9\n");
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.settings().headers[0].value(), "Bearer tok-9");
        assert_eq!(app.query, "");
    }

    #[test]
    fn paste_never_triggers_commands_in_design_mode() {
        let mut app = design_app();
        open_me(&mut app);
        app.handle_paste("rq");
        assert!(!app.should_quit);
        assert!(app.design.as_ref().is_some_and(|design| design.run.is_none()));
    }

    #[test]
    fn paste_is_ignored_while_help_is_open() {
        let mut app = app();
        press(&mut app, KeyCode::F(1));
        app.handle_paste("abc");
        assert_eq!(app.query, "");
    }

    #[test]
    fn every_help_table_lists_ctrl_k() {
        for keys in [BROWSE_KEYS, DESIGN_KEYS, RUN_KEYS] {
            assert!(keys.iter().any(|(key, _)| *key == "Ctrl-K"));
        }
    }

    #[test]
    fn the_status_bar_hints_at_ctrl_k() {
        assert!(app().status(1_000).0.contains("Ctrl-K"));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p vega 2>&1 | grep -aE "^error" | head -5`
Expected: compile errors (`no function App::empty`, `no variant Load`, ...).

- [ ] **Step 3: Implement `app.rs`**

3a. Imports at the top: add

```rust
use vega_core::config::Settings;

use crate::tui::connection::{Modal, Outcome};
```

3b. `Action`: add after `Run(RunRequest),`:

```rust
    /// The endpoint changed: the event loop should load the schema for it, from the
    /// cache when there is one, otherwise from the server.
    Load,
```

3c. Help tables: in each of `BROWSE_KEYS`, `DESIGN_KEYS` and `RUN_KEYS`, add right after the `("Ctrl-R", "Refresh the schema"),` entry:

```rust
    ("Ctrl-K", "Open the connection (endpoint and credentials)"),
```

3d. `App` struct: add three fields after `host: String,`:

```rust
    settings: Settings,
    /// The connection modal, when it is open.
    pub modal: Option<Modal>,
    /// False until a schema has been loaded for the current endpoint.
    loaded: bool,
```

3e. Add this free function next to `host_of`:

```rust
fn empty_schema() -> Schema {
    Schema {
        query_type: None,
        mutation_type: None,
        subscription_type: None,
        types: Vec::new(),
    }
}
```

3f. In `App::new`, add to the struct literal after `host: host_of(uri),`:

```rust
            settings: Settings {
                uri: uri.to_string(),
                headers: Vec::new(),
                key_only: false,
            },
            modal: None,
            loaded: true,
```

3g. Add these methods to `impl App` right after `new`:

```rust
    /// Replaces the connection settings (used once, at startup).
    pub fn with_settings(mut self, settings: Settings) -> Self {
        self.host = host_of(&settings.uri);
        self.settings = settings;
        self
    }

    /// An app with no schema yet. Without an endpoint it opens the connection modal.
    pub fn empty(settings: Settings) -> Self {
        let modal = settings.uri.is_empty().then(|| Modal::new(&settings));
        let mut app = Self::new(empty_schema(), &settings.uri, 0).with_settings(settings);
        app.loaded = false;
        app.modal = modal;
        app
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// A schema load was started: the status bar says so.
    pub fn begin_load(&mut self) {
        self.refresh = RefreshState::Running;
    }

    /// Text pasted into the terminal (bracketed paste). It goes to the modal when it is
    /// open, otherwise only into the search box or a text variable editor, so pasted
    /// text can never act as command keys.
    pub fn handle_paste(&mut self, text: &str) {
        if let Some(modal) = self.modal.as_mut() {
            modal.paste(text);
            return;
        }
        if self.help_open || self.confirm.is_some() {
            return;
        }
        let in_text_editor = self
            .design
            .as_ref()
            .is_some_and(|design| matches!(design.edit, Some(Edit::Text(_))));
        if self.design.is_none() || in_text_editor {
            for c in text.chars().filter(|c| !c.is_control()) {
                self.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
            }
        }
    }

    /// The modal returned new settings.
    fn apply_connection(&mut self, settings: Settings) -> Action {
        let uri_changed = settings.uri != self.settings.uri;
        self.settings = settings;
        if uri_changed {
            self.reset_connection();
            self.refresh = RefreshState::Running;
            Action::Load
        } else if !self.loaded {
            self.refresh = RefreshState::Running;
            Action::Refresh
        } else {
            Action::None
        }
    }

    /// A new endpoint: nothing of the old schema (selection, query, variables) applies.
    fn reset_connection(&mut self) {
        self.schema = empty_schema();
        self.loaded = false;
        self.fetched_at = 0;
        self.host = host_of(&self.settings.uri);
        self.query.clear();
        self.hits = search::search(&self.schema, "");
        self.hit_index = 0;
        self.design = None;
        self.sessions.clear();
        self.confirm = None;
        self.help_open = false;
        self.run_seq += 1;
    }
```

3h. Replace the start of `handle_key`. Currently:

```rust
    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        self.notice = None;
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('q') => {
                    self.should_quit = true;
                    Action::None
                }
                KeyCode::Char('r') => self.request_refresh(),
                _ => Action::None,
            };
        }
```

with:

```rust
    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        self.notice = None;
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if let Some(modal) = self.modal.as_mut() {
            if ctrl && key.code == KeyCode::Char('q') {
                self.should_quit = true;
                return Action::None;
            }
            return match modal.handle_key(key) {
                Outcome::Open => Action::None,
                Outcome::Cancel => {
                    self.modal = None;
                    Action::None
                }
                Outcome::Submit(settings) => {
                    self.modal = None;
                    self.apply_connection(settings)
                }
            };
        }
        if ctrl {
            return match key.code {
                KeyCode::Char('q') => {
                    self.should_quit = true;
                    Action::None
                }
                KeyCode::Char('r') => self.request_refresh(),
                KeyCode::Char('k') => {
                    self.help_open = false;
                    self.modal = Some(Modal::new(&self.settings));
                    Action::None
                }
                _ => Action::None,
            };
        }
```

3i. `request_refresh`: add at its start:

```rust
        if self.settings.uri.is_empty() {
            return self.notify("no endpoint: press Ctrl-K to set one");
        }
```

3j. `apply_refresh_ok`: add `self.loaded = true;` as its first statement.

3k. `status`: right after the `if let Some(notice) = &self.notice { ... }` block add:

```rust
        if !self.loaded {
            return self.status_without_schema();
        }
```

in the `RefreshState::Idle` arm change the text to `" · Ctrl-R refresh · Ctrl-K connection"`, and in the `Failed` arm change the format to
`" · refresh failed: {first_line} · Ctrl-R retry · Ctrl-K connection"`. In the `CacheWriteFailed` arm append ` · Ctrl-K connection` the same way. Then add this method to `impl App`:

```rust
    /// The status bar while no schema is loaded for the endpoint.
    fn status_without_schema(&self) -> (String, StatusLevel) {
        if self.settings.uri.is_empty() {
            return (
                "no endpoint · Ctrl-K connection".to_string(),
                StatusLevel::Notice,
            );
        }
        let text = match &self.refresh {
            RefreshState::Running => format!("{} · loading the schema…", self.host),
            RefreshState::Failed(message) => {
                let first_line = message.lines().next().unwrap_or("");
                format!(
                    "{} · could not load the schema: {first_line} · Ctrl-K connection · Ctrl-R retry",
                    self.host
                )
            }
            _ => format!(
                "{} · no schema loaded · Ctrl-K connection · Ctrl-R load",
                self.host
            ),
        };
        (text, StatusLevel::Notice)
    }
```

- [ ] **Step 4: Implement `tui/mod.rs`**

Replace everything above `#[cfg(test)] mod tests` with:

```rust
//! Terminal setup and the event loop. Everything testable lives in `app`, `connection`
//! and `ui`.

pub mod app;
#[allow(dead_code)] // the drawing code in `ui` uses the rest in a later commit
pub mod connection;
pub mod ui;

use std::{
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Duration,
};

use ratatui::{
    crossterm::{
        event::{self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyEventKind},
        execute,
    },
    DefaultTerminal,
};
use vega_core::cache::{self, CacheEntry};
use vega_core::config::Settings;
use vega_core::response::{self, ResponseView};
use vega_core::{client, headers};

use crate::schema_source;
use app::{Action, App, RunRequest};

/// What background threads send back to the event loop.
enum Update {
    /// A schema fetch. `seq` ties it to the load that started it, so the answer for an
    /// endpoint the user has since left is dropped.
    Schema {
        seq: u64,
        cache_file: PathBuf,
        result: anyhow::Result<CacheEntry>,
    },
    Run {
        seq: u64,
        outcome: Result<ResponseView, String>,
    },
}

/// Runs the TUI. `entry` is the cached schema for the endpoint, if any; without one the
/// schema is fetched in the background (or, with no endpoint, the connection modal
/// opens).
pub fn run(
    entry: Option<CacheEntry>,
    settings: Settings,
    cache_dir: PathBuf,
) -> anyhow::Result<()> {
    let app = match entry {
        Some(entry) => App::new(entry.response.data.schema, &settings.uri, entry.fetched_at)
            .with_settings(settings),
        None => App::empty(settings),
    };
    let mut terminal = ratatui::init();
    let _ = execute!(std::io::stdout(), EnableBracketedPaste);
    let result = event_loop(&mut terminal, app, &cache_dir);
    let _ = execute!(std::io::stdout(), DisableBracketedPaste);
    ratatui::restore();
    result
}

fn event_loop(
    terminal: &mut DefaultTerminal,
    mut app: App,
    cache_dir: &Path,
) -> anyhow::Result<()> {
    let (tx, rx) = mpsc::channel::<Update>();
    let mut load_seq: u64 = 0;

    if !app.is_loaded() && !app.settings().uri.is_empty() {
        app.begin_load();
        load_seq += 1;
        spawn_fetch(&app, cache_dir, load_seq, &tx);
    }

    while !app.should_quit {
        terminal.draw(|frame| ui::draw(frame, &app, cache::now_secs()))?;

        while let Ok(update) = rx.try_recv() {
            match update {
                Update::Schema {
                    seq,
                    cache_file,
                    result,
                } => {
                    if seq == load_seq {
                        apply_fetch(&mut app, &cache_file, result);
                    }
                }
                Update::Run { seq, outcome } => app.apply_run_result(seq, outcome),
            }
        }

        // Short poll so the status bar age keeps ticking and results show up.
        if event::poll(Duration::from_millis(200))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => match app.handle_key(key) {
                    Action::None => {}
                    Action::Refresh => {
                        load_seq += 1;
                        spawn_fetch(&app, cache_dir, load_seq, &tx);
                    }
                    Action::Load => {
                        load_seq += 1;
                        let cache_file = cache::cache_path(cache_dir, &app.settings().uri);
                        match cache::load(&cache_file) {
                            Ok(Some(entry)) => {
                                app.apply_refresh_ok(entry.response.data.schema, entry.fetched_at)
                            }
                            _ => spawn_fetch(&app, cache_dir, load_seq, &tx),
                        }
                    }
                    Action::Run(request) => {
                        let settings = app.settings().clone();
                        let tx = tx.clone();
                        thread::spawn(move || {
                            let outcome = run_request(&settings, &request);
                            let _ = tx.send(Update::Run {
                                seq: request.seq,
                                outcome,
                            });
                        });
                    }
                },
                Event::Paste(text) => app.handle_paste(&text),
                _ => {}
            }
        }
    }
    Ok(())
}

/// Fetches the schema of the app's current endpoint on a background thread.
fn spawn_fetch(app: &App, cache_dir: &Path, seq: u64, tx: &mpsc::Sender<Update>) {
    let settings = app.settings().clone();
    let cache_file = cache::cache_path(cache_dir, &settings.uri);
    let tx = tx.clone();
    thread::spawn(move || {
        let result = schema_source::fetch_entry(&settings);
        let _ = tx.send(Update::Schema {
            seq,
            cache_file,
            result,
        });
    });
}

/// Sends the request and summarizes the answer. A failure to get any HTTP answer comes
/// back as text for the Response pane, with every header value masked.
fn run_request(settings: &Settings, request: &RunRequest) -> Result<ResponseView, String> {
    client::execute(&settings.uri, &settings.headers, &request.body)
        .map(|response| response::view(&response))
        .map_err(|error| headers::mask(&error_chain(&error), &settings.headers))
}

/// An error and its sources on one line: `request failed: error sending request: ...`.
fn error_chain(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

/// Applies a fetch result. A failed fetch leaves the cache alone; a fetched schema
/// is used even when it cannot be written to the cache.
fn apply_fetch(app: &mut App, cache_file: &Path, result: anyhow::Result<CacheEntry>) {
    match result {
        Ok(entry) => {
            let stored = cache::store(cache_file, &entry);
            let (schema, fetched_at) = (entry.response.data.schema, entry.fetched_at);
            match stored {
                Ok(()) => app.apply_refresh_ok(schema, fetched_at),
                Err(e) => app.apply_refresh_unsaved(schema, fetched_at, e.to_string()),
            }
        }
        Err(e) => {
            let message = headers::mask(&format!("{e:#}"), &app.settings().headers);
            app.apply_refresh_err(message);
        }
    }
}
```

Keep the existing `#[cfg(test)] mod tests` below it, and add these tests to it (after `error_chain_joins_the_sources`):

```rust
    #[test]
    fn a_failed_fetch_shows_a_masked_error_in_the_status_bar() {
        let settings = settings("http://127.0.0.1:9/graphql", Some("secret-key"));
        let mut app = App::empty(settings);
        apply_fetch(
            &mut app,
            Path::new("/nonexistent/cache.json"),
            Err(anyhow::anyhow!("server answered HTTP 401: bad secret-key")),
        );
        let (text, _) = app.status(0);
        assert!(text.contains("could not load the schema"), "{text}");
        assert!(!text.contains("secret-key"), "{text}");
    }
```

- [ ] **Step 5: Implement `main.rs`**

- Add `use std::path::PathBuf;` at the top of the imports.
- Replace the body of `main` from `let cli = Cli::parse();` up to and including the call `let settings = config::resolve_settings(...)?;` with:

```rust
    let cli = Cli::parse();
    let one_shot = cli.search.is_some() || cli.describe.is_some() || cli.query.is_some();

    let env_uri = env_var("VEGA_GRAPHQL_URI");
    let has_endpoint = [cli.uri.as_deref(), env_uri.as_deref()]
        .into_iter()
        .flatten()
        .any(|uri| !uri.is_empty());
    let file_config = match xdg::config_dir() {
        Some(dir) if !has_endpoint => config::load_file_config(&dir)?,
        _ => FileConfig::default(),
    };
    let auth = AuthInput {
        key: env_var("VEGA_GRAPHQL_KEY"),
        key_header: cli
            .key_header
            .clone()
            .or_else(|| env_var("VEGA_GRAPHQL_KEY_HEADER")),
        headers: env_var("VEGA_GRAPHQL_HEADERS"),
    };
    // The terminal UI can start with no endpoint (it opens the connection modal); the
    // one-shot commands cannot.
    let settings = if one_shot {
        config::resolve_settings(cli.uri.clone(), env_uri, auth, file_config)?
    } else {
        config::resolve_settings_partial(cli.uri.clone(), env_uri, auth, file_config)?
    };

    let cache_dir = xdg::cache_dir()
        .context("cannot locate the cache directory: set XDG_CACHE_HOME or HOME")?;
    if !one_shot {
        return start_tui(&cli, settings, cache_dir);
    }
```

  (Keep the `let cache_file = cache::cache_path(&cache_dir, &settings.uri);` line and everything after it, and remove the old `let cache_dir = ...` statement that is now duplicated.)
- The block `if cli.search.is_some() || cli.describe.is_some() || cli.query.is_some() { ... return Ok(()); }` is now always true: remove the `if` and dedent its body, turning the final `return Ok(());` into the function's tail `Ok(())`, and delete the trailing `tui::run(entry, settings, cache_file)` line. `cargo fmt` reflows the indentation.
- Add this function after `main`:

```rust
/// Starts the terminal UI. The schema is loaded inside it, in the background, so a
/// missing endpoint or a failing server never stops it from opening.
fn start_tui(cli: &Cli, settings: Settings, cache_dir: PathBuf) -> anyhow::Result<()> {
    let entry = if settings.uri.is_empty() || cli.refresh {
        None
    } else {
        match cache::load(&cache::cache_path(&cache_dir, &settings.uri)) {
            Ok(entry) => entry,
            Err(e) => {
                eprintln!("warning: ignoring unusable cache ({e}); fetching a fresh schema");
                None
            }
        }
    };
    tui::run(entry, settings, cache_dir)
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test --workspace --no-fail-fast 2>&1 | grep -aE "passed|failed|panicked"`
Expected: all pass, 0 failed. Existing app tests keep passing (they use `App::new`, so they are loaded apps with no modal).

- [ ] **Step 7: Check and commit**

Run: `just fix && just check`
Expected: passes.

```bash
git add crates/vega/src
git commit -m "feat: open the connection modal and load the schema in the TUI"
```

---

### Task 11: Draw the modal

**Files:**
- Modify: `crates/vega/src/tui/ui.rs`
- Modify: `crates/vega/src/tui/mod.rs` (remove the `#[allow(dead_code)]` line above `pub mod connection;`)
- Test: inline tests in `ui.rs`

**Interfaces:**
- Consumes: `Modal` accessors, `AuthType`, `Focus` (Task 9), `App::modal` (Task 10).

- [ ] **Step 1: Write the failing tests**

In `crates/vega/src/tui/ui.rs`, inside `mod tests` add `use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};` and `use vega_core::config::Settings;` with the other `use` lines, then:

```rust
    fn key(app: &mut App, code: KeyCode) {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn ctrl(app: &mut App, c: char) {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
    }

    fn empty_settings() -> Settings {
        Settings {
            uri: String::new(),
            headers: Vec::new(),
            key_only: false,
        }
    }

    #[test]
    fn the_modal_shows_the_uri_the_types_and_the_hint() {
        let app = App::empty(empty_settings());
        let text = screen(&render(&app, 0));
        assert!(text.contains("Connection"));
        assert!(text.contains("URI"));
        for label in ["None", "Basic", "API Key", "Bearer", "Custom headers"] {
            assert!(text.contains(label), "missing {label}");
        }
        assert!(text.contains("Ctrl-T"));
        assert!(text.contains("Esc"));
    }

    #[test]
    fn the_modal_is_hidden_when_closed() {
        assert!(!screen(&render(&app(), 1_000)).contains("Connection"));
    }

    #[test]
    fn secrets_are_masked_until_revealed() {
        let mut app = App::empty(Settings {
            uri: "https://x.example.com/graphql".into(),
            ..empty_settings()
        });
        ctrl(&mut app, 'k');
        for _ in 0..3 {
            key(&mut app, KeyCode::Down); // Basic, API Key, Bearer
        }
        key(&mut app, KeyCode::Tab);
        for c in "tok-123".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        let text = screen(&render(&app, 0));
        assert!(!text.contains("tok-123"), "secret drawn in clear");
        assert!(text.contains("•••••••"));
        ctrl(&mut app, 't');
        assert!(screen(&render(&app, 0)).contains("tok-123"));
    }

    #[test]
    fn the_modal_shows_validation_errors() {
        let mut app = App::empty(empty_settings());
        key(&mut app, KeyCode::Enter);
        assert!(screen(&render(&app, 0)).contains("The URI is required"));
    }

    #[test]
    fn custom_headers_show_every_row() {
        let mut app = App::empty(Settings {
            uri: "https://x.example.com/graphql".into(),
            ..empty_settings()
        });
        ctrl(&mut app, 'k');
        for _ in 0..4 {
            key(&mut app, KeyCode::Down);
        }
        key(&mut app, KeyCode::Tab);
        for c in "X-One".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        ctrl(&mut app, 'n');
        for c in "X-Two".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        let text = screen(&render(&app, 0));
        assert!(text.contains("X-One"));
        assert!(text.contains("X-Two"));
    }

    #[test]
    fn the_modal_does_not_panic_on_tiny_terminals() {
        let mut app = App::empty(empty_settings());
        ctrl(&mut app, 'k');
        for (w, h) in [(20, 5), (1, 1), (0, 0), (3, 40), (200, 3)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|frame| draw(frame, &app, 0)).unwrap();
        }
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p vega ui::tests 2>&1 | grep -aE "passed|failed|panicked" | head`
Expected: the new tests fail (the modal is not drawn yet).

- [ ] **Step 3: Implement**

In `crates/vega/src/tui/mod.rs` delete the line `#[allow(dead_code)] // the drawing code in `ui` uses the rest in a later commit` above `pub mod connection;`.

In `crates/vega/src/tui/ui.rs`:

- Add to the imports: `use crate::tui::connection::{AuthType, Field, Focus, Modal};`.
- At the end of `draw`, after the `if app.help_open { ... }` block, add:

```rust
    if let Some(modal) = &app.modal {
        draw_connection(frame, area, modal);
    }
```

- Add these functions after `draw_help`:

```rust
fn draw_connection(frame: &mut Frame, area: Rect, modal: &Modal) {
    let width = area.width.min(76);
    let height = area.height.min(20);
    let popup = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    let block = Block::default().borders(Borders::ALL).title("Connection");
    let inner = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);

    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(2),
    ])
    .split(inner);

    let uri_focused = modal.focus() == Focus::Uri;
    frame.render_widget(
        Paragraph::new(field_text(modal.uri(), uri_focused)).block(pane("URI", uri_focused)),
        rows[0],
    );

    let columns = Layout::horizontal([Constraint::Length(18), Constraint::Min(1)]).split(rows[1]);
    draw_types(frame, columns[0], modal);
    draw_form(frame, columns[1], modal);

    let footer = match modal.error() {
        Some(message) => Line::styled(message.to_string(), Style::default().fg(Color::Red)),
        None => Line::raw(""),
    };
    let hint = Line::styled(
        "Tab next · Enter connect · Esc cancel · Ctrl-T reveal · Ctrl-N add row",
        Style::default().fg(Color::DarkGray),
    );
    frame.render_widget(Paragraph::new(vec![footer, hint]), rows[2]);
}

fn draw_types(frame: &mut Frame, area: Rect, modal: &Modal) {
    let focused = modal.focus() == Focus::Types;
    let items: Vec<ListItem> = AuthType::ALL
        .iter()
        .map(|auth| {
            let mark = if *auth == modal.selected() { "●" } else { "○" };
            ListItem::new(format!("{mark} {}", auth.label()))
        })
        .collect();
    let mut state = ListState::default();
    if focused {
        state.select(AuthType::ALL.iter().position(|auth| *auth == modal.selected()));
    }
    let list = List::new(items)
        .block(pane("Type", focused))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_form(frame: &mut Frame, area: Rect, modal: &Modal) {
    let title = modal.selected().label();
    let fields = modal.fields();
    let block = pane(title, matches!(modal.focus(), Focus::Field(_)));
    if fields.is_empty() {
        frame.render_widget(Paragraph::new("No credentials are sent.").block(block), area);
        return;
    }
    let mut lines = Vec::new();
    let mut focus_line = 0;
    for (index, field) in fields.iter().enumerate() {
        let focused = modal.focus() == Focus::Field(index);
        if focused {
            focus_line = lines.len();
        }
        lines.push(Line::styled(
            field.label().to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        ));
        lines.push(field_text(field, focused));
        // A blank line between the rows of Custom headers.
        if modal.selected() == AuthType::CustomHeaders && index % 2 == 1 {
            lines.push(Line::raw(""));
        }
    }
    let visible = usize::from(area.height.saturating_sub(2));
    let skip = (focus_line + 2).saturating_sub(visible);
    let scroll = u16::try_from(skip).unwrap_or(0);
    frame.render_widget(Paragraph::new(lines).block(block).scroll((scroll, 0)), area);
}

/// A field's text, with a cursor mark when it has focus.
fn field_text(field: &Field, focused: bool) -> Line<'static> {
    let mut text = field.display();
    if focused {
        text.push('▏');
        return Line::styled(text, Style::default().add_modifier(Modifier::REVERSED));
    }
    Line::raw(text)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --workspace --no-fail-fast 2>&1 | grep -aE "passed|failed|panicked"`
Expected: all pass, 0 failed.

- [ ] **Step 5: Smoke test in a real terminal**

Build and run vega in a detached `tmux` session with nothing configured (use a throwaway config and cache dir):

```bash
cargo build -q -p vega
dir=$(mktemp -d)
tmux new-session -d -s vegasmoke -x 100 -y 30 "env -u VEGA_GRAPHQL_URI -u VEGA_GRAPHQL_KEY -u VEGA_GRAPHQL_HEADERS -u VEGA_GRAPHQL_KEY_HEADER XDG_CONFIG_HOME=$dir/config XDG_CACHE_HOME=$dir/cache target/debug/vega"
sleep 1
tmux capture-pane -t vegasmoke -p | grep -F "Connection"      # the modal is open
tmux send-keys -t vegasmoke Escape
sleep 0.5
tmux capture-pane -t vegasmoke -p | grep -F "no endpoint"     # empty screen with the hint
tmux send-keys -t vegasmoke C-k
sleep 0.5
tmux capture-pane -t vegasmoke -p | grep -F "Connection"      # reopened with Ctrl-K
tmux send-keys -t vegasmoke C-q
sleep 0.5
tmux has-session -t vegasmoke 2>&1 | head -1                  # the session is gone
```

Expected: each `grep` prints a matching line and the last command reports the session does not exist. If `tmux` is not installed, say so in the report and skip this step. Kill any leftover session with `tmux kill-session -t vegasmoke`.

- [ ] **Step 6: Check and commit**

Run: `just fix && just check`
Expected: passes.

```bash
git add crates/vega/src/tui
git commit -m "feat: draw the connection modal"
```

---

### Task 12: Documentation, rules and issue status

**Files:**
- Modify: `docs/authentication.md`, `README.md`, `AGENTS.md`, `docs/issues/0004-paste-api-key-in-app.md`

- [ ] **Step 1: Add the TUI section to `docs/authentication.md`**

Insert this section before `## Troubleshooting`:

```markdown
## In the terminal UI

You do not need any environment variable to start vega. With no endpoint configured it opens the connection modal right away, and `Ctrl-K` opens it again from any screen.

- The endpoint is the field at the top. Below it, the methods are listed on the left (None, Basic, API Key, Bearer, Custom headers) and the form of the selected method is on the right. Only one method is active.
- `Tab` and `Shift-Tab` move between the fields, `Up` and `Down` move inside the list or the form, `Enter` connects and `Esc` cancels.
- Secret fields show bullets. `Ctrl-T` reveals or hides the focused field until the modal closes.
- Paste works in any field (the trailing newline is dropped).
- Custom headers: `Ctrl-N` adds a row. A row with both fields empty is ignored.
- The modal opens filled in from the environment: `VEGA_GRAPHQL_KEY` alone opens on API Key, `VEGA_GRAPHQL_HEADERS` opens on Custom headers (one row per header), nothing opens on None. Basic and Bearer values set through the environment are plain `Authorization` headers, so they appear as Custom headers rows.
- Connecting with only new credentials keeps the schema, your query and its variables, and retries the schema load if it had failed. Changing the endpoint starts a new connection: the schema is loaded for it (from the cache when there is one) and the selection, query and variables are cleared.
- Nothing you enter is saved. It lives in memory until you quit and is never written back to the environment, the cache or any file.
- A server that rejects the schema request (a 401, for example) no longer stops vega from starting: the status bar shows the error, and `Ctrl-K` lets you fix the credentials.
```

- [ ] **Step 2: README**

In `README.md`:

1. In the Features list, after the bullet that starts `- credentials are read only from the environment`, add: `- starts with nothing configured: enter the endpoint and credentials (API key, basic authentication, bearer token or custom headers) in the connection modal, opened with Ctrl-K. What you type stays in memory for the session only (see [Authentication](docs/authentication.md)).`
2. Delete the Planned bullet `- paste credentials inside the app and keep it in memory for the session only.`

- [ ] **Step 3: `AGENTS.md`**

1. Replace the first bullet under `## Rules that must not be broken` with:

```markdown
- Header values (the API key from `VEGA_GRAPHQL_KEY`, every header in `VEGA_GRAPHQL_HEADERS`, and what is typed in the connection modal) come only from the environment or the modal. Never write one to disk, the cache, logs or error messages, and never add a CLI flag for a value (a flag that names a header, such as `--key-header`, is fine). Modal values live in memory only. No header value appears in the Response pane or in `--run` output: the client masks them all.
```

2. In `## Conventions`, replace the bullet that starts `- TUI logic lives in` with:

```markdown
- TUI logic lives in `tui/app.rs` and `tui/connection.rs` as pure state machines (key in, state out) so it is testable without a terminal.
```

- [ ] **Step 4: Issue status**

In `docs/issues/0004-paste-api-key-in-app.md` change `Status: doing` to `Status: done`.

- [ ] **Step 5: Check and commit**

Run: `just check && just issues done`
Expected: passes; `just issues done` lists `0002` and `0004`.

```bash
git add docs AGENTS.md README.md
git commit -m "doc: describe the connection modal and close issue 0004"
```

---

## Self-review notes

- Spec coverage: layout and types (Tasks 9, 11), `Ctrl-K` / `Ctrl-T` / paste / `Enter` / `Esc` (Tasks 9, 10), startup without a URI and background loading (Tasks 6, 10), submit behavior for headers-only, URI change and cancel (Task 10), start state from the effective settings through `Settings::key_only` (Tasks 8, 9), masking and no persistence (Tasks 7, 9, 10, 12), rule and doc changes (Task 12). The version stays at 0.5.0 from part 1.
- Names used across tasks: `Settings { uri, headers, key_only }`, `resolve_settings_partial`, `Modal::new(&Settings)`, `Outcome::{Open, Cancel, Submit}`, `AuthType`, `Focus`, `Field`, `Action::Load`, `App::{empty, with_settings, settings, is_loaded, begin_load, handle_paste, modal}`, `tui::run(entry, settings, cache_dir)`.
- Known limitations, deliberate: reopening the modal after a Basic or Bearer submit shows Custom headers rows (the spec says so); a stale in-flight run result after a URI change is dropped by `run_seq`, but the one-request slot is freed by the stale answer (existing behavior).
