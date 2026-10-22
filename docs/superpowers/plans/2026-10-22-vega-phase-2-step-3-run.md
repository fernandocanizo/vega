# Vega Phase 2, Step 3: Run Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Run the generated query against the server. In the TUI, `r` sends the operation and a Run mode shows the Response (HTTP status, elapsed time, GraphQL errors, pretty-printed data, scrolling), with a confirmation before mutations. On the command line, `--run` sends it and prints the response (`--yes` confirms mutations without a terminal).

**Architecture:** `vega-core` gains `client::execute` (the HTTP code shared with the introspection fetch), `client::operation_body`, `variables::to_json` and a pure `response` module that turns a raw HTTP response into what the pane and the CLI show. The TUI app gets a Run mode inside `DesignState`, a request/confirmation/in-flight state machine and a new `Action::Run`; the event loop runs requests on a background thread and feeds results back through the same channel as schema refreshes. Spec: `docs/superpowers/specs/2026-10-22-vega-phase-2-design.md` (sections Running, Keys, CLI parity).

**Tech Stack:** Rust (stable, edition 2021), `serde_json`, `thiserror`, `clap`, `anyhow`, `reqwest` (blocking), `ratatui` 0.30. No new dependencies.

## Decisions made while planning (review these)

1. **Response cap (the spec's open item):** the TUI shows at most 10,000 lines of the pretty-printed response and appends a note with the real line count; the command line prints everything. Lines are not wrapped; long lines are clipped.
2. **`--run` output:** stdout is the pretty-printed response body (valid JSON when the server sent JSON). With `--json`, stdout is an envelope `{"operationName", "status", "elapsedMs", "response"}` where `response` is the parsed body (a string when it is not JSON).
3. **`--run` exit status:** non-zero when the request fails, the HTTP status is not 2xx, the body is not JSON, or the response contains GraphQL `errors`. The body is still printed on stdout; the reason goes to stderr.
4. **Mutations on the command line:** `--run` on a mutation asks `Run mutation <Name>? y/N` on stderr and reads stdin when stdin is a terminal; without a terminal it refuses unless `--yes` is given. Declining exits non-zero (`cancelled`). `--yes` requires `--run`.
5. **TUI Run mode keys:** Tab toggles the focus between Response and Variables (Query is skipped); Up/Down scroll one line and PgUp/PgDn ten lines when Response has focus; Esc returns to Design.
6. **`r` works from Design with any pane focused** (not while a variable is being edited, where it is text). It does nothing on a type tree (a notice says so).
7. **One request in flight:** `r` while one is running shows a notice. Leaving Run mode (Esc) while a request runs discards its result when it arrives.
8. **Network failures** (no HTTP response) show in the Response pane as a red message. A non-2xx status shows `HTTP <status>` in red in the pane title and the body lines as received.
9. **Subscriptions** generate a query but cannot be run: `r` and `--run` say so.

## Global Constraints

- The screen shows at most two main panes side by side, with shorter panes below them. Paired panes share one height (70/30 in two-pane columns).
- Run layout: left column Query over Variables, right column Response. Details and Description are hidden. The title of the Response pane starts with `Response`.
- Running is blocked while any variable is invalid or a required one is empty; the status line names the problem (`cannot run: variable $id: required`). A mutation asks `Run mutation <Name>? y/N` in the status line first; only `y`/`Y` sends it, any other key cancels.
- The request is a `POST` with `query`, `variables` and `operationName` and the `X-API-Key` header, sent from a background thread, one request at a time. An empty optional variable is omitted from `variables`.
- Variable values and the API key are never written to disk, the cache or logs, and the API key never appears in the Response pane or in `--run` output (the client masks it in bodies and messages).
- Keys: Run mode: Up/Down/PgUp/PgDn scroll the response, `r` runs again, Tab switches Response/Variables, Esc returns to Design with the selection intact, `q` quits, Ctrl-Q quits from anywhere (also while editing), `?`/F1/`h` open help. While a variable is being edited every key is text except Ctrl-Q and Ctrl-R.
- CLI: warnings and errors go to stderr only; stdout (including `--json`) stays machine-readable. The stale-cache warning also applies to `--run`. `--run` requires `--query`.
- Tests never use the real network: a local `127.0.0.1` listener or a refused port (`127.0.0.1:9`).
- Commits follow AGENTS.md: atomic, `tag: subject` in present tense, subject and body lines at most 72 characters, tags `feat`, `fix`, `ref`, `doc`, `test`, `chore`, no references to plans, specs, tasks or steps, no AI attribution. Every commit passes `just check`.
- Versioning (AGENTS.md): the last task bumps the minor version in its own `chore` commit.

## File Structure

```
crates/vega-core/
  src/client.rs                         + Response, operation_body, execute (shared post helper)
  src/response.rs                       NEW: ResponseView, LineKind, view, pretty_body, MAX_LINES
  src/variables.rs                      + to_json
  src/lib.rs                            + pub mod response;
  tests/client.rs                       + execute tests (local listener)
crates/vega/
  src/cli.rs                            + --run, --yes
  src/main.rs                           + --run handling
  src/output.rs                         + run_json, run_failure
  src/tui/app.rs                        + Run mode state machine, confirmation, notices
  src/tui/ui.rs                         + Run layout, Response pane, notice status
  src/tui/mod.rs                        + background request, Update channel
  tests/cli.rs                          + --run tests (local listener)
README.md, AGENTS.md, spec, Cargo.*     updated in the last task
```

Library docs: use the context7 MCP tools if a ratatui, clap or reqwest API below does not compile as written; fix the call, keep the behavior. Plan code was written against the current files: read each file before editing and keep the behavior if rustfmt's layout differs.

---

### Task 1: Run an operation through the client

**Files:**
- Modify: `crates/vega-core/src/client.rs`, `crates/vega-core/tests/client.rs`

**Interfaces:**
- Produces (`vega_core::client`):
  - `struct Response { status: u16, elapsed: Duration, body: String }` (`Debug, Clone, PartialEq, Eq`); `body` is the text as received with the API key masked as `***`.
  - `operation_body(operation_name: &str, query: &str, variables: &serde_json::Map<String, Value>) -> Value` (`{"query","variables","operationName"}`).
  - `execute(uri: &str, api_key: Option<&str>, body: &Value) -> Result<Response, ClientError>`: any HTTP status is `Ok`; only a failure to get an HTTP response is `Err(ClientError::Http)`.
- `fetch_introspection` keeps its behavior and error values; both share one private `post` helper.

- [ ] **Step 1: Write the failing tests**

Add to `crates/vega-core/tests/client.rs` (extend the `use` line to `use vega_core::client::{execute, fetch_introspection, operation_body, ClientError};`):

```rust
fn me_body() -> serde_json::Value {
    let variables = serde_json::from_str(r#"{"id":"1"}"#).unwrap();
    operation_body("Me", "query Me { me { id } }", &variables)
}

#[test]
fn execute_posts_the_operation_with_its_variables_and_the_key() {
    let (uri, rx) = serve_once("200 OK", r#"{"data":{"me":null}}"#.to_string());
    let response = execute(&uri, Some("secret"), &me_body()).unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.body, r#"{"data":{"me":null}}"#);

    let request = rx.recv().unwrap();
    let lower = request.to_lowercase();
    assert!(lower.starts_with("post /graphql"));
    assert!(lower.contains("x-api-key: secret"));
    assert!(request.contains(r#""operationName":"Me""#));
    assert!(request.contains(r#""variables":{"id":"1"}"#));
    assert!(request.contains("query Me { me { id } }"));
}

#[test]
fn execute_sends_no_key_header_without_a_key() {
    let (uri, rx) = serve_once("200 OK", "{}".to_string());
    execute(&uri, None, &me_body()).unwrap();
    assert!(!rx.recv().unwrap().to_lowercase().contains("x-api-key"));
}

#[test]
fn execute_returns_any_http_status_as_a_response() {
    let (uri, _rx) = serve_once("500 Internal Server Error", "boom".to_string());
    let response = execute(&uri, None, &me_body()).unwrap();
    assert_eq!(response.status, 500);
    assert_eq!(response.body, "boom");
}

#[test]
fn execute_returns_graphql_errors_as_a_normal_response() {
    let body = r#"{"data":null,"errors":[{"message":"nope"}]}"#;
    let (uri, _rx) = serve_once("200 OK", body.to_string());
    let response = execute(&uri, None, &me_body()).unwrap();
    assert_eq!(response.body, body);
}

#[test]
fn execute_masks_the_api_key_in_the_body() {
    let (uri, _rx) = serve_once("200 OK", "echo secret-key!".to_string());
    let response = execute(&uri, Some("secret-key"), &me_body()).unwrap();
    assert!(response.body.contains("***"), "{}", response.body);
    assert!(!response.body.contains("secret-key"), "{}", response.body);
}

#[test]
fn execute_reports_connection_failures() {
    let err = execute("http://127.0.0.1:9/graphql", None, &me_body()).unwrap_err();
    assert!(matches!(err, ClientError::Http(_)));
}
```

Add to the `tests` module of `crates/vega-core/src/client.rs`:

```rust
    #[test]
    fn operation_body_carries_query_variables_and_name() {
        let mut variables = serde_json::Map::new();
        variables.insert("id".to_string(), json!("1"));
        let body = operation_body("Me", "query Me { me { id } }", &variables);
        assert_eq!(body["operationName"], "Me");
        assert_eq!(body["query"], "query Me { me { id } }");
        assert_eq!(body["variables"]["id"], "1");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p vega-core`
Expected: FAIL to compile (`unresolved import execute`, `cannot find function operation_body`).

- [ ] **Step 3: Implement**

In `client.rs`, change the import to `use std::time::{Duration, Instant};` and `use serde_json::{json, Map, Value};`. Add above `fetch_introspection`:

```rust
/// What came back from running an operation. Any HTTP status counts as a response; only
/// failing to get one at all is an error (see `execute`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub elapsed: Duration,
    /// The body as received, with the API key (if the server echoed it) masked.
    pub body: String,
}

/// The POST body that runs an operation.
pub fn operation_body(operation_name: &str, query: &str, variables: &Map<String, Value>) -> Value {
    json!({ "query": query, "variables": variables, "operationName": operation_name })
}

/// Replaces every occurrence of the API key with `***`.
fn mask_key(text: String, api_key: Option<&str>) -> String {
    match api_key.filter(|key| !key.is_empty()) {
        Some(key) => text.replace(key, "***"),
        None => text,
    }
}

/// POSTs `body` as JSON and returns the HTTP status, the response text and how long the
/// round trip took.
fn post(
    uri: &str,
    api_key: Option<&str>,
    body: &Value,
) -> Result<(u16, String, Duration), reqwest::Error> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?;
    let mut request = client.post(uri).json(body);
    if let Some(key) = api_key {
        request = request.header("X-API-Key", key);
    }
    let started = Instant::now();
    let response = request.send()?;
    let status = response.status().as_u16();
    let text = response.text()?;
    Ok((status, text, started.elapsed()))
}

/// Runs an operation. Any HTTP status is an `Ok` response; connection failures, timeouts
/// and unreadable bodies are errors.
pub fn execute(uri: &str, api_key: Option<&str>, body: &Value) -> Result<Response, ClientError> {
    let (status, text, elapsed) = post(uri, api_key, body)?;
    Ok(Response {
        status,
        elapsed,
        body: mask_key(text, api_key),
    })
}
```

Replace the body of `fetch_introspection` with:

```rust
    let (status, text, _elapsed) = post(uri, api_key, &request_body())?;
    if !(200..300).contains(&status) {
        let body: String = text.chars().take(300).collect();
        return Err(ClientError::Status {
            status,
            body: mask_key(body, api_key),
        });
    }
    parse_response(&text)
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --workspace`
Expected: all pass, including the existing `fetch_introspection` tests unchanged.

- [ ] **Step 5: Check and commit**

Run: `just fix && just check`
Expected: exit code 0.

```bash
git add crates/vega-core/src/client.rs crates/vega-core/tests/client.rs
git commit -m "feat: run an operation through the client"
```

---

### Task 2: Variables JSON and the response view

**Files:**
- Create: `crates/vega-core/src/response.rs`
- Modify: `crates/vega-core/src/variables.rs`, `crates/vega-core/src/lib.rs`

**Interfaces:**
- Consumes: `client::Response` (Task 1), `variables::{state, State, Values}`, `VariableDef`.
- Produces:
  - `variables::to_json(&Schema, &[VariableDef], &Values) -> Result<serde_json::Map<String, Value>, String>`: the `variables` object of a request (valid values only, unset ones omitted); the error is `$name: reason` for the first invalid or missing required variable.
  - `vega_core::response`:
    - `const MAX_LINES: usize = 10_000`
    - `enum LineKind { Error, Data, Note }`
    - `struct ResponseView { status: u16, elapsed_ms: u64, errors: Vec<String>, not_json: bool, lines: Vec<String>, total_lines: usize }` (`Debug, Clone, PartialEq, Eq`)
    - `ResponseView::ok(&self) -> bool` (2xx, JSON, no GraphQL errors) and `ResponseView::body_lines(&self) -> Vec<(LineKind, String)>` (errors as `error: <message>`, a blank line when both errors and data exist, the data lines, then a `Note` line when output was capped)
    - `view(&Response) -> ResponseView`, `pretty_body(&str) -> String`

- [ ] **Step 1: Write the failing tests (variables.rs)**

Add to the `tests` module of `crates/vega-core/src/variables.rs`:

```rust
    #[test]
    fn to_json_keeps_valid_values_and_omits_unset_ones() {
        let (schema, query) = users();
        let mut values = Values::new();
        let role = query.variables.iter().find(|v| v.name == "role").unwrap();
        values.insert(role.path.clone(), "ADMIN".to_string());
        let map = to_json(&schema, &query.variables, &values).unwrap();
        assert_eq!(serde_json::Value::Object(map), json!({"role": "ADMIN"}));
    }

    #[test]
    fn to_json_names_the_first_problem() {
        let (schema, query) = users();
        let mut values = Values::new();
        let floor = query.variables.iter().find(|v| v.name == "floor").unwrap();
        values.insert(floor.path.clone(), "abc".to_string());
        assert_eq!(
            to_json(&schema, &query.variables, &values),
            Err("$floor: expected a number".to_string())
        );

        let op = OperationRef::parse(&schema, "Query.node").unwrap();
        let selection = parse_select(&schema, &op, "id").unwrap();
        let node = generate(&schema, &op, &selection).unwrap();
        assert_eq!(
            to_json(&schema, &node.variables, &Values::new()),
            Err("$id: required".to_string())
        );
    }
```

- [ ] **Step 2: Write the failing tests (response.rs)**

Add `pub mod response;` to `lib.rs` (alphabetical). Create `crates/vega-core/src/response.rs` containing only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn response(status: u16, body: &str) -> Response {
        Response {
            status,
            elapsed: Duration::from_millis(42),
            body: body.to_string(),
        }
    }

    fn texts(view: &ResponseView) -> Vec<String> {
        view.body_lines().into_iter().map(|(_, text)| text).collect()
    }

    #[test]
    fn data_is_pretty_printed_with_status_and_time() {
        let view = view(&response(200, r#"{"data":{"me":{"id":"1"}}}"#));
        assert_eq!(view.status, 200);
        assert_eq!(view.elapsed_ms, 42);
        assert!(view.ok());
        assert_eq!(
            texts(&view),
            ["{", r#"  "me": {"#, r#"    "id": "1""#, "  }", "}"]
        );
    }

    #[test]
    fn data_and_errors_show_both_errors_first() {
        let body = r#"{"data":{"me":null},"errors":[{"message":"boom"},{"code":7}]}"#;
        let view = view(&response(200, body));
        assert!(!view.ok());
        assert_eq!(view.errors, ["boom", r#"{"code":7}"#]);
        let lines = view.body_lines();
        assert_eq!(lines[0], (LineKind::Error, "error: boom".to_string()));
        assert_eq!(lines[1], (LineKind::Error, r#"error: {"code":7}"#.to_string()));
        assert_eq!(lines[2], (LineKind::Data, String::new()));
        assert_eq!(lines[3].1, "{");
    }

    #[test]
    fn errors_without_data_show_only_the_errors() {
        let view = view(&response(200, r#"{"errors":[{"message":"nope"}]}"#));
        assert_eq!(texts(&view), ["error: nope"]);
        let view = self::view(&response(200, r#"{"data":null,"errors":[{"message":"nope"}]}"#));
        assert_eq!(texts(&view), ["error: nope"]);
    }

    #[test]
    fn a_body_without_data_or_errors_is_shown_whole() {
        let view = view(&response(200, r#"{"hello":1}"#));
        assert!(view.ok());
        assert_eq!(texts(&view), ["{", r#"  "hello": 1"#, "}"]);
    }

    #[test]
    fn a_body_that_is_not_json_is_shown_as_received_and_flagged() {
        let view = view(&response(502, "<html>\nbad gateway\n</html>"));
        assert!(view.not_json);
        assert!(!view.ok());
        assert_eq!(texts(&view), ["<html>", "bad gateway", "</html>"]);
    }

    #[test]
    fn a_non_2xx_status_is_not_ok_even_with_json() {
        let view = view(&response(500, r#"{"data":{"a":1}}"#));
        assert!(!view.ok());
        assert_eq!(view.status, 500);
    }

    #[test]
    fn long_output_is_capped_with_a_note() {
        let numbers: Vec<String> = (0..12_000).map(|n| n.to_string()).collect();
        let body = format!(r#"{{"data":[{}]}}"#, numbers.join(","));
        let view = view(&response(200, &body));
        assert_eq!(view.lines.len(), MAX_LINES);
        assert_eq!(view.total_lines, 12_002);
        let lines = view.body_lines();
        let (kind, note) = lines.last().unwrap();
        assert_eq!(*kind, LineKind::Note);
        assert!(note.contains("10000 of 12002"), "{note}");
    }

    #[test]
    fn pretty_body_formats_json_and_leaves_other_text_alone() {
        assert_eq!(pretty_body(r#"{"a":1}"#), "{\n  \"a\": 1\n}");
        assert_eq!(pretty_body("plain"), "plain");
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p vega-core`
Expected: FAIL to compile (`cannot find function to_json`, `cannot find type ResponseView`, ...).

- [ ] **Step 4: Implement**

In `variables.rs`, add `Map` to the import (`use serde_json::{Map, Number, Value};`) and insert above `parse`:

```rust
/// The `variables` object of a request: every valid value, unset ones left out. The error
/// names the first variable that is invalid or missing (`$id: required`).
pub fn to_json(
    schema: &Schema,
    defs: &[VariableDef],
    values: &Values,
) -> Result<Map<String, Value>, String> {
    let mut map = Map::new();
    for def in defs {
        match state(schema, def, values.get(&def.path).map(String::as_str)) {
            State::Value(value) => {
                map.insert(def.name.clone(), value);
            }
            State::Unset => {}
            State::Invalid(reason) => return Err(format!("${}: {reason}", def.name)),
        }
    }
    Ok(map)
}
```

Insert above `#[cfg(test)]` in `response.rs`:

```rust
//! Turns a raw HTTP response into what the Response pane and `--run` show. Pure data:
//! no terminal code.

use serde_json::Value;

use crate::client::Response;

/// The terminal pane keeps at most this many lines of output; the command line prints
/// everything.
pub const MAX_LINES: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// A GraphQL error message.
    Error,
    /// A line of the pretty-printed data (or of a body that is not JSON).
    Data,
    /// A remark about the output itself, such as it being capped.
    Note,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseView {
    pub status: u16,
    pub elapsed_ms: u64,
    /// Messages of the GraphQL `errors`, if any.
    pub errors: Vec<String>,
    /// The body was not JSON and is shown as received.
    pub not_json: bool,
    /// The output lines, at most `MAX_LINES`.
    pub lines: Vec<String>,
    /// How many lines the full output has.
    pub total_lines: usize,
}

impl ResponseView {
    /// A 2xx answer with a JSON body that holds no GraphQL errors.
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status) && !self.not_json && self.errors.is_empty()
    }

    /// Everything the pane lists, in order: errors, a blank line when data follows, the
    /// data lines, and a note when the output was capped.
    pub fn body_lines(&self) -> Vec<(LineKind, String)> {
        let mut out: Vec<(LineKind, String)> = self
            .errors
            .iter()
            .map(|message| (LineKind::Error, format!("error: {message}")))
            .collect();
        if !self.errors.is_empty() && !self.lines.is_empty() {
            out.push((LineKind::Data, String::new()));
        }
        out.extend(self.lines.iter().map(|line| (LineKind::Data, line.clone())));
        if self.total_lines > self.lines.len() {
            out.push((
                LineKind::Note,
                format!(
                    "… output capped: showing {} of {} lines (--run prints everything)",
                    self.lines.len(),
                    self.total_lines
                ),
            ));
        }
        out
    }
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

/// The messages of a GraphQL `errors` array (a whole item when it has no `message`).
fn error_messages(value: &Value) -> Vec<String> {
    value
        .get("errors")
        .and_then(Value::as_array)
        .map(|errors| {
            errors
                .iter()
                .map(|error| match error.get("message").and_then(Value::as_str) {
                    Some(message) => message.to_string(),
                    None => error.to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The body pretty-printed when it is JSON, as received otherwise.
pub fn pretty_body(body: &str) -> String {
    match serde_json::from_str::<Value>(body) {
        Ok(value) => pretty(&value),
        Err(_) => body.to_string(),
    }
}

pub fn view(response: &Response) -> ResponseView {
    let elapsed_ms = u64::try_from(response.elapsed.as_millis()).unwrap_or(u64::MAX);
    let (errors, not_json, text) = match serde_json::from_str::<Value>(&response.body) {
        Ok(value) => {
            let errors = error_messages(&value);
            let text = match value.get("data") {
                Some(data) if !data.is_null() => pretty(data),
                _ if errors.is_empty() => pretty(&value),
                _ => String::new(),
            };
            (errors, false, text)
        }
        Err(_) => (Vec::new(), true, response.body.clone()),
    };
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let total_lines = lines.len();
    lines.truncate(MAX_LINES);
    ResponseView {
        status: response.status,
        elapsed_ms,
        errors,
        not_json,
        lines,
        total_lines,
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p vega-core`
Expected: all pass.

- [ ] **Step 6: Check and commit**

Run: `just fix && just check`
Expected: exit code 0.

```bash
git add crates/vega-core/src/lib.rs crates/vega-core/src/response.rs crates/vega-core/src/variables.rs
git commit -m "feat: build request variables and summarize responses"
```

---

### Task 3: `--run` and `--yes` on the command line

**Files:**
- Modify: `crates/vega/src/cli.rs`, `crates/vega/src/output.rs`, `crates/vega/src/main.rs`, `crates/vega/tests/cli.rs`

**Interfaces:**
- Consumes: `client::{execute, operation_body, Response}`, `variables::to_json`, `response::{view, pretty_body, ResponseView}`.
- Produces: `Cli.run: bool` (`--run`, requires `--query`), `Cli.yes: bool` (`--yes`, requires `--run`); `output::run_json(&GeneratedQuery, &Response) -> serde_json::Value`; `output::run_failure(&ResponseView) -> String`.

Behavior (see Decisions 2-4): `--run` checks everything before sending (subscription, required/invalid variables, mutation confirmation), sends the operation and prints the response body; exit status is non-zero when `ResponseView::ok()` is false (the body is still printed, the reason is on stderr).

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module of `crates/vega/src/output.rs`:

```rust
    #[test]
    fn run_json_wraps_the_parsed_body_in_an_envelope() {
        use std::time::Duration;
        use vega_core::client::Response;
        let query = GeneratedQuery {
            operation_name: "Me".into(),
            text: "query Me { me { id } }".into(),
            variables: Vec::new(),
        };
        let response = Response {
            status: 200,
            elapsed: Duration::from_millis(7),
            body: r#"{"data":{"me":null}}"#.into(),
        };
        let json = run_json(&query, &response);
        assert_eq!(json["operationName"], "Me");
        assert_eq!(json["status"], 200);
        assert_eq!(json["elapsedMs"], 7);
        assert_eq!(json["response"]["data"]["me"], serde_json::Value::Null);

        let raw = Response {
            body: "<html>".into(),
            ..response
        };
        assert_eq!(run_json(&query, &raw)["response"], "<html>");
    }

    #[test]
    fn run_failure_names_the_reason() {
        use vega_core::response::ResponseView;
        let view = |status, not_json, errors: Vec<&str>| ResponseView {
            status,
            elapsed_ms: 1,
            errors: errors.into_iter().map(str::to_string).collect(),
            not_json,
            lines: Vec::new(),
            total_lines: 0,
        };
        assert_eq!(run_failure(&view(500, false, vec![])), "server answered HTTP 500");
        assert_eq!(
            run_failure(&view(200, true, vec![])),
            "the response is not valid JSON"
        );
        assert_eq!(
            run_failure(&view(200, false, vec!["a", "b"])),
            "GraphQL errors: a; b"
        );
    }
```

In `crates/vega/tests/cli.rs`, first generalize the helpers: rename `seeded_cache(age_secs)` into `seeded_cache_for(uri: &str, age_secs: u64)` (using `uri` in place of `URI` for the entry and the cache path) and keep `fn seeded_cache(age_secs: u64) -> TempDir { seeded_cache_for(URI, age_secs) }`; rename `vega(cache, args)` into `vega_with(cache: &TempDir, uri: &str, key: Option<&str>, args: &[&str]) -> Output` (setting `VEGA_GRAPHQL_URI` to `uri`, and `VEGA_GRAPHQL_KEY` to the key when given, else removing it) and keep `fn vega(cache, args) -> Output { vega_with(cache, URI, None, args) }`. Add the listener helper (copied in spirit from `crates/vega-core/tests/client.rs`):

```rust
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;

/// Serves exactly one request on a random local port; returns the endpoint URL and a
/// channel that yields the raw request text.
fn serve_once(status_line: &'static str, body: &str) -> (String, mpsc::Receiver<String>) {
    let body = body.to_string();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let uri = format!("http://{}/graphql", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = stream.read(&mut chunk).unwrap();
            buf.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&buf).to_lowercase();
            if let Some(pos) = text.find("\r\n\r\n") {
                let length = text[..pos]
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .map(|v| v.trim().parse::<usize>().unwrap())
                    .unwrap_or(0);
                if buf.len() >= pos + 4 + length {
                    break;
                }
            }
            if n == 0 {
                break;
            }
        }
        tx.send(String::from_utf8_lossy(&buf).into_owned()).unwrap();
        let response = format!(
            "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
    });
    (uri, rx)
}
```

Then add the tests (the seeded schema is the phase 1 fixture: `Query.userRead(id: ID!)` returns `User { id, name, ... }`, `Mutation.userCreate(input: UserInput!)`; look at `UserInput` in `crates/vega-core/tests/fixtures/introspection.json` and use a JSON object with its required fields wherever a test below says `<valid input>`):

```rust
#[test]
fn run_prints_the_response_body_and_sends_the_operation() {
    let (uri, rx) = serve_once(
        "200 OK",
        r#"{"data":{"userRead":{"id":"42","name":"Ann"}}}"#,
    );
    let cache = seeded_cache_for(&uri, 60);
    let out = vega_with(
        &cache,
        &uri,
        None,
        &["--query", "Query.userRead", "--select", "id,name", "--var", "id=42", "--run"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("\"name\": \"Ann\""), "{}", stdout(&out));
    assert!(serde_json::from_str::<serde_json::Value>(&stdout(&out)).is_ok());
    assert_eq!(stderr(&out), "");

    let request = rx.recv().unwrap();
    assert!(request.contains(r#""operationName":"UserRead""#));
    assert!(request.contains(r#""variables":{"id":"42"}"#));
}

#[test]
fn run_json_wraps_the_response_in_an_envelope() {
    let (uri, _rx) = serve_once("200 OK", r#"{"data":{"userRead":{"id":"42"}}}"#);
    let cache = seeded_cache_for(&uri, 60);
    let out = vega_with(
        &cache,
        &uri,
        None,
        &["--query", "Query.userRead", "--select", "id", "--var", "id=42", "--run", "--json"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(json["operationName"], "UserRead");
    assert_eq!(json["status"], 200);
    assert_eq!(json["response"]["data"]["userRead"]["id"], "42");
    assert!(json["elapsedMs"].is_u64());
}

#[test]
fn run_sends_the_api_key_and_never_prints_it() {
    let body = r#"{"data":null,"errors":[{"message":"bad secret-key"}]}"#;
    let (uri, rx) = serve_once("200 OK", body);
    let cache = seeded_cache_for(&uri, 60);
    let out = vega_with(
        &cache,
        &uri,
        Some("secret-key"),
        &["--query", "Query.userRead", "--select", "id", "--var", "id=1", "--run"],
    );
    assert!(rx.recv().unwrap().to_lowercase().contains("x-api-key: secret-key"));
    assert!(!stdout(&out).contains("secret-key"), "{}", stdout(&out));
    assert!(!stderr(&out).contains("secret-key"), "{}", stderr(&out));
}

#[test]
fn run_exits_non_zero_on_graphql_errors_but_still_prints_the_body() {
    let (uri, _rx) = serve_once(
        "200 OK",
        r#"{"data":{"userRead":null},"errors":[{"message":"nope"}]}"#,
    );
    let cache = seeded_cache_for(&uri, 60);
    let out = vega_with(
        &cache,
        &uri,
        None,
        &["--query", "Query.userRead", "--select", "id", "--var", "id=1", "--run"],
    );
    assert!(!out.status.success());
    assert!(stdout(&out).contains("nope"));
    assert!(stderr(&out).contains("GraphQL errors: nope"), "{}", stderr(&out));
}

#[test]
fn run_reports_http_errors() {
    let (uri, _rx) = serve_once("500 Internal Server Error", "boom");
    let cache = seeded_cache_for(&uri, 60);
    let out = vega_with(
        &cache,
        &uri,
        None,
        &["--query", "Query.userRead", "--select", "id", "--var", "id=1", "--run"],
    );
    assert!(!out.status.success());
    assert!(stderr(&out).contains("HTTP 500"), "{}", stderr(&out));
}

#[test]
fn run_refuses_a_missing_required_variable_without_sending() {
    let cache = seeded_cache(60); // the endpoint refuses connections
    let out = vega(&cache, &["--query", "Query.userRead", "--select", "id", "--run"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("required"), "{}", stderr(&out));
    assert_eq!(stdout(&out), "");
}

#[test]
fn a_mutation_needs_yes_without_a_terminal() {
    let input = r#"input=<valid input>"#; // replace with a valid UserInput JSON object
    let cache = seeded_cache(60);
    let out = vega(
        &cache,
        &["--query", "Mutation.userCreate", "--select", "id", "--var", input, "--run"],
    );
    assert!(!out.status.success());
    assert!(stderr(&out).contains("without a terminal"), "{}", stderr(&out));

    let (uri, rx) = serve_once("200 OK", r#"{"data":{"userCreate":{"id":"1"}}}"#);
    let cache = seeded_cache_for(&uri, 60);
    let out = vega_with(
        &cache,
        &uri,
        None,
        &["--query", "Mutation.userCreate", "--select", "id", "--var", input, "--run", "--yes"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(rx.recv().unwrap().contains(r#""operationName":"UserCreate""#));
}

#[test]
fn run_requires_query_and_yes_requires_run() {
    let cache = seeded_cache(60);
    assert!(!vega(&cache, &["--run"]).status.success());
    let out = vega(
        &cache,
        &["--query", "Query.userRead", "--select", "id", "--yes"],
    );
    assert!(!out.status.success());
}

#[test]
fn query_without_run_still_sends_nothing() {
    let cache = seeded_cache(60); // the endpoint refuses connections
    let out = vega(
        &cache,
        &["--query", "Query.userRead", "--select", "id", "--var", "id=1"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p vega`
Expected: FAIL (`run_json` does not exist; `--run` is an unknown argument).

- [ ] **Step 3: Implement**

`crates/vega/src/cli.rs`: update the struct doc to `Without --search, --describe or --query, opens the interactive browser.`, the `--query` doc to `Print the query generated for OPERATION (e.g. Query.userRead); sends nothing unless --run is given`, the `--json` doc to `Print JSON on stdout for --search, --describe, --query and --run`, and add after `vars`:

```rust
    /// Send the --query operation to the server and print the response
    #[arg(long, requires = "query")]
    pub run: bool,

    /// Run a mutation without asking first (required when there is no terminal)
    #[arg(long, requires = "run")]
    pub yes: bool,
```

`crates/vega/src/output.rs`: add `use vega_core::client::Response;`, `use vega_core::response::ResponseView;` and:

```rust
/// What `--run --json` prints: the response parsed when it is JSON, a string otherwise.
pub fn run_json(query: &GeneratedQuery, response: &Response) -> serde_json::Value {
    let body = serde_json::from_str::<serde_json::Value>(&response.body)
        .unwrap_or_else(|_| serde_json::Value::String(response.body.clone()));
    serde_json::json!({
        "operationName": query.operation_name,
        "status": response.status,
        "elapsedMs": u64::try_from(response.elapsed.as_millis()).unwrap_or(u64::MAX),
        "response": body,
    })
}

/// One line saying why a run did not succeed.
pub fn run_failure(view: &ResponseView) -> String {
    if !(200..300).contains(&view.status) {
        format!("server answered HTTP {}", view.status)
    } else if view.not_json {
        "the response is not valid JSON".to_string()
    } else {
        format!("GraphQL errors: {}", view.errors.join("; "))
    }
}
```

`crates/vega/src/main.rs`: add imports `use vega_core::client;`, `use vega_core::config::Settings;`, `use vega_core::response;`, `use vega_core::schema::Root;`. Replace the call `print_query(schema, target, cli.select.as_deref(), &cli.vars, cli.json)?` with:

```rust
            let args = QueryArgs {
                target,
                select: cli.select.as_deref(),
                vars: &cli.vars,
                json: cli.json,
                run: cli.run,
                yes: cli.yes,
            };
            print_query(schema, &settings, &args)?;
```

and replace `print_query` with the following (keeping `parse_vars` as is):

```rust
struct QueryArgs<'a> {
    target: &'a str,
    select: Option<&'a str>,
    vars: &'a [String],
    json: bool,
    run: bool,
    yes: bool,
}

fn print_query(schema: &Schema, settings: &Settings, args: &QueryArgs) -> anyhow::Result<()> {
    let target = args.target;
    let operation = OperationRef::parse(schema, target).with_context(|| {
        format!("operation `{target}` not found: use Query.<field>, Mutation.<field> or Subscription.<field>")
    })?;
    let selection = match args.select {
        Some(text) => selection::parse_select(schema, &operation, text)?,
        None => Selection::default(),
    };
    let query = querygen::generate(schema, &operation, &selection)?;
    let values = parse_vars(&query, args.vars)?;
    let mut states = Vec::new();
    for def in &query.variables {
        let typed = values.get(&def.path).map(String::as_str);
        let state = variables::state(schema, def, typed);
        // A missing required value is only listed (running is what needs it).
        if let (State::Invalid(reason), Some(_)) = (&state, typed) {
            anyhow::bail!("variable ${}: {reason}", def.name);
        }
        states.push(state);
    }
    if args.run {
        return run_query(schema, settings, &operation, &query, &values, args);
    }
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&output::query_json(&query, &states)?)?
        );
    } else {
        print!("{}", output::query_text(&query, &states));
    }
    Ok(())
}

/// Sends the operation and prints the response. Everything that can be checked locally is
/// checked before anything is sent.
fn run_query(
    schema: &Schema,
    settings: &Settings,
    operation: &OperationRef,
    query: &GeneratedQuery,
    values: &Values,
    args: &QueryArgs,
) -> anyhow::Result<()> {
    if operation.root == Root::Subscription {
        anyhow::bail!("subscriptions cannot be run (they need WebSockets)");
    }
    let variables = variables::to_json(schema, &query.variables, values)
        .map_err(|problem| anyhow::anyhow!("cannot run: variable {problem}"))?;
    if operation.root == Root::Mutation && !args.yes {
        confirm_mutation(&query.operation_name)?;
    }
    let body = client::operation_body(&query.operation_name, &query.text, &variables);
    let response = client::execute(&settings.uri, settings.api_key.as_deref(), &body)
        .with_context(|| format!("running {} on {}", query.operation_name, settings.uri))?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&output::run_json(query, &response))?
        );
    } else {
        println!("{}", response::pretty_body(&response.body));
    }
    let view = response::view(&response);
    if !view.ok() {
        anyhow::bail!("{}", output::run_failure(&view));
    }
    Ok(())
}

/// Asks on the terminal; without one, a mutation needs `--yes`.
fn confirm_mutation(name: &str) -> anyhow::Result<()> {
    use std::io::{IsTerminal, Write};
    if !std::io::stdin().is_terminal() {
        anyhow::bail!("refusing to run mutation {name} without a terminal: pass --yes to confirm");
    }
    eprint!("Run mutation {name}? y/N ");
    std::io::stderr().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    if answer.trim().eq_ignore_ascii_case("y") {
        Ok(())
    } else {
        anyhow::bail!("cancelled")
    }
}
```

(`Root` derives `PartialEq`; `OperationRef.root` is public. The existing stale-cache warning in `main` already covers `--run` because it triggers on `cli.query.is_some()`.)

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p vega`
Expected: all pass. Confirm `cargo run -q -p vega -- --help` lists `--run` and `--yes`.

- [ ] **Step 5: Check and commit**

Run: `just fix && just check`
Expected: exit code 0.

```bash
git add crates/vega/src/cli.rs crates/vega/src/output.rs crates/vega/src/main.rs crates/vega/tests/cli.rs
git commit -m "feat: add --run and --yes to send the generated query"
```

---

### Task 4: Run mode in the TUI state machine

**Files:**
- Modify: `crates/vega/src/tui/app.rs`

**Interfaces:**
- Consumes: `client::operation_body`, `variables::to_json`, `response::{ResponseView, LineKind}`, `OperationRef::root`.
- Produces (`crate::tui::app`):
  - `Action` becomes `Debug, Clone, PartialEq` with a new variant `Run(RunRequest)`; `struct RunRequest { seq: u64, body: serde_json::Value }` (`Debug, Clone, PartialEq`).
  - `enum RunState { Running, Done(ResponseView), Failed(String) }`, `struct RunView { state: RunState, scroll: usize }`; `DesignState.run: Option<RunView>` (`Some` means Run mode); `Focus::Response`.
  - `App::apply_run_result(&mut self, seq: u64, outcome: Result<ResponseView, String>)`.
  - `App::notice: Option<String>` (cleared by the next key); `StatusLevel::Notice`; `App::status` shows the pending mutation prompt `Run mutation <Name>? y/N` or the notice instead of the host text.
  - `RUN_KEYS`; `help_keys` returns it in Run mode; `DESIGN_KEYS` gains `("r", "Run the query")`.
- Behavior: see Global Constraints and Decisions 5-9. `r` in Design (any pane, not while editing) validates and either returns `Action::Run(request)` (entering Run mode with `RunState::Running`, focus Response), asks for confirmation (mutations) or sets a notice (`select at least one field to run`, `cannot run: variable $id: required`, `subscriptions cannot be run (they need WebSockets)`, `a request is already running`, `type trees cannot be run`). Run mode keys: `r` runs again, Tab toggles Response/Variables, Up/Down (Response focus) scroll one line, PgUp/PgDn ten lines, clamped to the output length; Variables focus keeps Up/Down/Enter from Task 7 of step 2; Esc leaves Run mode (focus back to Details); `q` quits. A late result (leaving Run mode or a refresh happened) is discarded, but the in-flight flag is always cleared.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module of `crates/vega/src/tui/app.rs` (the module already has `design_app`, `open_me`, `press`, `ctrl`, `type_text`, `node_variables`, `users_variables`; add `use std::time::Duration;` and `use vega_core::client::Response;` and `use vega_core::response;` to its imports):

```rust
    fn view_of(status: u16, body: &str) -> response::ResponseView {
        response::view(&Response {
            status,
            elapsed: Duration::from_millis(5),
            body: body.to_string(),
        })
    }

    /// Opens Query.me with `id` selected.
    fn me_with_id(app: &mut App) {
        open_me(app);
        press(app, KeyCode::Down);
        press(app, KeyCode::Char(' '));
    }

    fn run_request(action: Action) -> RunRequest {
        match action {
            Action::Run(request) => request,
            other => panic!("expected Action::Run, got {other:?}"),
        }
    }

    #[test]
    fn r_sends_the_generated_query_and_enters_run_mode() {
        let mut app = design_app();
        me_with_id(&mut app);
        let request = run_request(press(&mut app, KeyCode::Char('r')));
        assert_eq!(request.seq, 1);
        assert_eq!(request.body["operationName"], "Me");
        assert_eq!(request.body["query"], "query Me {\n  me {\n    id\n  }\n}");
        assert_eq!(request.body["variables"], serde_json::json!({}));
        let design = app.design.as_ref().unwrap();
        assert_eq!(design.run.as_ref().unwrap().state, RunState::Running);
        assert_eq!(design.focus, Focus::Response);
    }

    #[test]
    fn r_uses_the_typed_variable_values() {
        let mut app = design_app();
        node_variables(&mut app);
        press(&mut app, KeyCode::Enter);
        type_text(&mut app, "42");
        press(&mut app, KeyCode::Enter);
        let request = run_request(press(&mut app, KeyCode::Char('r')));
        assert_eq!(request.body["variables"], serde_json::json!({"id": "42"}));
    }

    #[test]
    fn r_is_text_while_a_variable_is_being_edited() {
        let mut app = design_app();
        node_variables(&mut app);
        press(&mut app, KeyCode::Enter);
        assert_eq!(press(&mut app, KeyCode::Char('r')), Action::None);
        assert!(app.is_editing());
    }

    #[test]
    fn r_explains_why_it_cannot_run() {
        let mut app = design_app();
        open_me(&mut app);
        assert_eq!(press(&mut app, KeyCode::Char('r')), Action::None);
        assert!(app.status(1_000).0.contains("select at least one field to run"));
        assert_eq!(app.status(1_000).1, StatusLevel::Notice);
        press(&mut app, KeyCode::Down); // any key clears the notice
        assert!(!app.status(1_000).0.contains("select at least one field"));

        let mut app = design_app();
        type_text(&mut app, "node");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' ')); // id, but $id is empty
        assert_eq!(press(&mut app, KeyCode::Char('r')), Action::None);
        assert!(app
            .status(1_000)
            .0
            .contains("cannot run: variable $id: required"));
        assert!(app.design.as_ref().unwrap().run.is_none());

        let mut app = design_app();
        type_text(&mut app, "postinput");
        press(&mut app, KeyCode::Enter);
        assert_eq!(press(&mut app, KeyCode::Char('r')), Action::None);
        assert!(app.status(1_000).0.contains("type trees cannot be run"));
    }

    #[test]
    fn only_one_request_runs_at_a_time() {
        let mut app = design_app();
        me_with_id(&mut app);
        run_request(press(&mut app, KeyCode::Char('r')));
        assert_eq!(press(&mut app, KeyCode::Char('r')), Action::None);
        assert!(app.status(1_000).0.contains("a request is already running"));

        app.apply_run_result(1, Ok(view_of(200, r#"{"data":{"me":null}}"#)));
        let again = run_request(press(&mut app, KeyCode::Char('r')));
        assert_eq!(again.seq, 2);
    }

    #[test]
    fn a_mutation_asks_for_confirmation_first() {
        let mut app = design_app();
        type_text(&mut app, "postCreate");
        assert_eq!(app.hits[0].path, "Mutation.postCreate");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' ')); // id
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Enter);
        type_text(&mut app, r#"{"title":"x"}"#);
        press(&mut app, KeyCode::Enter);

        assert_eq!(press(&mut app, KeyCode::Char('r')), Action::None);
        assert_eq!(
            app.status(1_000).0,
            "Run mutation PostCreate? y/N"
        );
        assert_eq!(press(&mut app, KeyCode::Char('n')), Action::None);
        assert!(app.design.as_ref().unwrap().run.is_none());
        assert!(!app.status(1_000).0.contains("Run mutation"));

        press(&mut app, KeyCode::Char('r'));
        let request = run_request(press(&mut app, KeyCode::Char('y')));
        assert_eq!(request.body["operationName"], "PostCreate");
        assert_eq!(
            request.body["variables"],
            serde_json::json!({"input": {"title": "x"}})
        );
    }

    #[test]
    fn results_fill_the_response_and_stale_ones_are_discarded() {
        let mut app = design_app();
        me_with_id(&mut app);
        run_request(press(&mut app, KeyCode::Char('r')));
        app.apply_run_result(1, Ok(view_of(200, r#"{"data":{"me":{"id":"1"}}}"#)));
        let run = app.design.as_ref().unwrap().run.as_ref().unwrap();
        assert!(matches!(&run.state, RunState::Done(view) if view.status == 200));

        // A result for a request that was left behind changes nothing.
        press(&mut app, KeyCode::Char('r'));
        press(&mut app, KeyCode::Esc); // back to Design while request 2 runs
        assert!(app.design.as_ref().unwrap().run.is_none());
        app.apply_run_result(2, Ok(view_of(200, "{}")));
        assert!(app.design.as_ref().unwrap().run.is_none());
        // ...but it freed the "in flight" slot.
        run_request(press(&mut app, KeyCode::Char('r')));
        app.apply_run_result(3, Err("request failed".to_string()));
        let run = app.design.as_ref().unwrap().run.as_ref().unwrap();
        assert_eq!(run.state, RunState::Failed("request failed".to_string()));
    }

    #[test]
    fn run_mode_keys_scroll_switch_panes_and_go_back() {
        let mut app = design_app();
        me_with_id(&mut app);
        run_request(press(&mut app, KeyCode::Char('r')));
        let numbers: Vec<String> = (0..50).map(|n| n.to_string()).collect();
        let body = format!(r#"{{"data":[{}]}}"#, numbers.join(","));
        app.apply_run_result(1, Ok(view_of(200, &body)));

        let scroll = |app: &App| app.design.as_ref().unwrap().run.as_ref().unwrap().scroll;
        press(&mut app, KeyCode::Up);
        assert_eq!(scroll(&app), 0);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        assert_eq!(scroll(&app), 2);
        press(&mut app, KeyCode::PageDown);
        assert_eq!(scroll(&app), 12);
        press(&mut app, KeyCode::PageUp);
        assert_eq!(scroll(&app), 2);
        for _ in 0..10 {
            press(&mut app, KeyCode::PageDown);
        }
        assert_eq!(scroll(&app), 51); // 52 lines: the last line stays visible

        // Tab switches between Response and Variables; the arrows then move variables.
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.design.as_ref().unwrap().focus, Focus::Variables);
        press(&mut app, KeyCode::Down);
        assert_eq!(scroll(&app), 51);
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.design.as_ref().unwrap().focus, Focus::Response);

        // Esc returns to Design with the selection intact.
        press(&mut app, KeyCode::Esc);
        let design = app.design.as_ref().unwrap();
        assert!(design.run.is_none());
        assert_eq!(design.focus, Focus::Details);
        assert!(app.is_selected(&[Seg::Field("id".to_string())]));
    }

    #[test]
    fn q_quits_in_run_mode_and_help_lists_the_run_keys() {
        let mut app = design_app();
        me_with_id(&mut app);
        run_request(press(&mut app, KeyCode::Char('r')));
        assert!(app.help_keys().iter().any(|(keys, _)| keys.contains("PgDn")));
        press(&mut app, KeyCode::Char('q'));
        assert!(app.should_quit);

        let mut app = design_app();
        open_me(&mut app);
        assert!(app.help_keys().iter().any(|(keys, _)| *keys == "r"));
    }

    #[test]
    fn a_refresh_drops_run_mode_and_a_late_result() {
        let mut app = design_app();
        me_with_id(&mut app);
        run_request(press(&mut app, KeyCode::Char('r')));
        let response: IntrospectionResponse = serde_json::from_str(DESIGN_FIXTURE).unwrap();
        app.apply_refresh_ok(response.data.schema, 5_000);
        assert!(app.design.is_none());
        app.apply_run_result(1, Ok(view_of(200, "{}")));
        assert!(app.design.is_none());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p vega tui`
Expected: FAIL to compile (`no variant Run`, `no type RunRequest`, ...).

- [ ] **Step 3: Implement**

Imports: add `use vega_core::client;`, `use vega_core::response::ResponseView;`, `use vega_core::schema::Root;` and extend the selection import so `OperationRef` is not needed (use `design.root.operation()`; its `root` field is `Root`).

`Action`, `RunRequest`, `RunState`, `RunView`:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    None,
    /// The event loop should start a background schema refresh.
    Refresh,
    /// The event loop should send this request on a background thread.
    Run(RunRequest),
}

/// A request ready to be sent; `seq` ties the result to it.
#[derive(Debug, Clone, PartialEq)]
pub struct RunRequest {
    pub seq: u64,
    pub body: serde_json::Value,
}

/// What the Response pane shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunState {
    Running,
    Done(ResponseView),
    /// No HTTP response at all (connection failure, timeout).
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunView {
    pub state: RunState,
    /// First visible line of `ResponseView::body_lines`.
    pub scroll: usize,
}

/// A mutation waiting for `y`.
#[derive(Debug, Clone)]
struct PendingRun {
    request: RunRequest,
    name: String,
}
```

Where the compiler needs it, remove `Copy`/`Eq` assumptions on `Action` (the event loop compares with `==`; Task 6 turns that into a `match`; for this task keep `crates/vega/src/tui/mod.rs` compiling by changing `app.handle_key(key) == Action::Refresh` to `matches!(app.handle_key(key), Action::Refresh)` and ignoring `Action::Run` for now: `Task 6` wires it).

`StatusLevel` gains `Notice`; `Focus` gains `Response`. `DesignState` gains `pub run: Option<RunView>` (initialised to `None`). `App` gains `notice: Option<String>`, `confirm: Option<PendingRun>`, `run_seq: u64`, `run_in_flight: bool` (initialised `None`, `None`, `0`, `false`).

Keys tables: add `("r", "Run the query")` to `DESIGN_KEYS` after the `Enter` row, and:

```rust
/// Hotkeys in Run mode.
pub const RUN_KEYS: &[(&str, &str)] = &[
    ("↑ / ↓", "Scroll the response (move through the variables when they have focus)"),
    ("PgUp / PgDn", "Scroll the response ten lines"),
    ("Tab", "Switch between Response and Variables"),
    ("Enter", "Edit the highlighted variable (Variables pane)"),
    ("r", "Run the query again"),
    ("Esc", "Back to Design"),
    ("q / Ctrl-Q", "Quit"),
    ("Ctrl-R", "Refresh the schema"),
    ("h / ? / F1", "Show or hide this help"),
];
```

`help_keys` returns `RUN_KEYS` when `self.design` has `run.is_some()`.

`handle_key` changes: it starts with `self.notice = None;`; the Ctrl block stays; after the `help_open` block and before the editing route add:

```rust
        if let Some(pending) = self.confirm.take() {
            return if matches!(key.code, KeyCode::Char('y' | 'Y')) {
                self.start_run(pending.request)
            } else {
                Action::None
            };
        }
```

and the routing at the end becomes `if self.design.is_some() { return self.handle_design_key(key.code); } self.handle_search_key(key.code); Action::None`; `handle_design_key` now returns `Action`. Its body:

```rust
    fn handle_design_key(&mut self, code: KeyCode) -> Action {
        if self.in_run_mode() {
            return self.handle_run_key(code);
        }
        match code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('r') => return self.request_run(),
            KeyCode::Esc => self.leave_design(),
            KeyCode::Tab => self.next_focus(),
            KeyCode::Up if self.focused(Focus::Variables) => self.move_var_cursor(-1),
            KeyCode::Down if self.focused(Focus::Variables) => self.move_var_cursor(1),
            KeyCode::Enter if self.focused(Focus::Variables) => self.start_edit(),
            _ if self
                .design
                .as_ref()
                .is_some_and(|design| design.focus != Focus::Details) => {}
            KeyCode::Char(' ') => self.toggle_under_cursor(),
            KeyCode::Down => self.move_cursor(1),
            KeyCode::Up => self.move_cursor(-1),
            KeyCode::Right => self.expand_or_step_in(),
            KeyCode::Left => self.collapse_or_go_up(),
            _ => {}
        }
        Action::None
    }
```

(Keep whatever arms the current function has; the point is the new `r` arm, the `Action` return and the Run delegation. `Focus::Query` keeps its Task 7 behavior: nothing.)

New methods:

```rust
    fn in_run_mode(&self) -> bool {
        self.design.as_ref().is_some_and(|design| design.run.is_some())
    }

    fn handle_run_key(&mut self, code: KeyCode) -> Action {
        match code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('r') => return self.request_run(),
            KeyCode::Esc => {
                if let Some(design) = self.design.as_mut() {
                    design.run = None;
                    design.focus = Focus::Details;
                }
            }
            KeyCode::Tab => {
                if let Some(design) = self.design.as_mut() {
                    design.focus = if design.focus == Focus::Response {
                        Focus::Variables
                    } else {
                        Focus::Response
                    };
                }
            }
            KeyCode::Up if self.focused(Focus::Variables) => self.move_var_cursor(-1),
            KeyCode::Down if self.focused(Focus::Variables) => self.move_var_cursor(1),
            KeyCode::Enter if self.focused(Focus::Variables) => self.start_edit(),
            KeyCode::Up if self.focused(Focus::Response) => self.scroll_response(-1),
            KeyCode::Down if self.focused(Focus::Response) => self.scroll_response(1),
            KeyCode::PageUp if self.focused(Focus::Response) => self.scroll_response(-10),
            KeyCode::PageDown if self.focused(Focus::Response) => self.scroll_response(10),
            _ => {}
        }
        Action::None
    }

    fn scroll_response(&mut self, delta: isize) {
        let last = match self.design.as_ref().and_then(|design| design.run.as_ref()) {
            Some(RunView {
                state: RunState::Done(view),
                ..
            }) => view.body_lines().len().saturating_sub(1),
            _ => 0,
        };
        if let Some(run) = self.design.as_mut().and_then(|design| design.run.as_mut()) {
            run.scroll = run.scroll.saturating_add_signed(delta).min(last);
        }
    }

    /// `r`: check that the query can run, then send it (a mutation asks first).
    fn request_run(&mut self) -> Action {
        let Some(design) = self.design.as_ref() else {
            return Action::None;
        };
        let Some(operation) = design.root.operation().cloned() else {
            return self.notify("type trees cannot be run");
        };
        if operation.root == Root::Subscription {
            return self.notify("subscriptions cannot be run (they need WebSockets)");
        }
        if self.run_in_flight {
            return self.notify("a request is already running");
        }
        let query = match self.generated_query() {
            Some(Ok(query)) => query,
            _ => return self.notify("select at least one field to run"),
        };
        let variables = match variables::to_json(&self.schema, &query.variables, &design.values) {
            Ok(variables) => variables,
            Err(problem) => return self.notify(&format!("cannot run: variable {problem}")),
        };
        self.run_seq += 1;
        let request = RunRequest {
            seq: self.run_seq,
            body: client::operation_body(&query.operation_name, &query.text, &variables),
        };
        if operation.root == Root::Mutation {
            self.confirm = Some(PendingRun {
                request,
                name: query.operation_name,
            });
            return Action::None;
        }
        self.start_run(request)
    }

    fn notify(&mut self, message: &str) -> Action {
        self.notice = Some(message.to_string());
        Action::None
    }

    fn start_run(&mut self, request: RunRequest) -> Action {
        self.run_in_flight = true;
        if let Some(design) = self.design.as_mut() {
            design.run = Some(RunView {
                state: RunState::Running,
                scroll: 0,
            });
            design.focus = Focus::Response;
        }
        Action::Run(request)
    }

    /// The result of request `seq`. It is shown only if Run mode is still open for that
    /// request; either way the one request slot is free again.
    pub fn apply_run_result(&mut self, seq: u64, outcome: Result<ResponseView, String>) {
        self.run_in_flight = false;
        if seq != self.run_seq {
            return;
        }
        if let Some(run) = self.design.as_mut().and_then(|design| design.run.as_mut()) {
            run.state = match outcome {
                Ok(view) => RunState::Done(view),
                Err(message) => RunState::Failed(message),
            };
            run.scroll = 0;
        }
    }
```

The borrow checker may object to holding `design` (from `self.design.as_ref()`) across `self.notify(...)`/`self.run_seq += 1` in `request_run`: if so, clone what you need first (`operation`, `values`) and release `design` before mutating `self`; keep behavior.

`apply_refresh_ok` also sets `self.confirm = None;` (keep `run_in_flight` as is). `status()` starts with:

```rust
        if let Some(pending) = &self.confirm {
            return (format!("Run mutation {}? y/N", pending.name), StatusLevel::Notice);
        }
        if let Some(notice) = &self.notice {
            return (notice.clone(), StatusLevel::Notice);
        }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --workspace`
Expected: all pass. Existing tests that compared `Action` values keep working (`Action::Refresh`, `Action::None`); if `Action` no longer being `Copy` breaks one, adapt it and report which.

- [ ] **Step 5: Check and commit**

Run: `just fix && just check`
Expected: exit code 0.

```bash
git add crates/vega/src/tui/app.rs crates/vega/src/tui/mod.rs
git commit -m "feat: add a Run mode with confirmation to the TUI state"
```

---

### Task 5: Draw Run mode and the Response pane

**Files:**
- Modify: `crates/vega/src/tui/ui.rs`

**Interfaces:**
- Consumes: `RunState`, `RunView`, `Focus::Response`, `StatusLevel::Notice`, `ResponseView::body_lines`, `LineKind`.
- Produces: Run layout (left Query over Variables, right Response, Details and Description hidden); the Response pane title `Response` plus `· running…`, `· failed` or `· HTTP <status> · <ms> ms` (red when the response is not ok); body lines coloured (errors red, notes dim) and sliced from `scroll`; the notice/prompt status style.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module of `crates/vega/src/tui/ui.rs` (it already has `design_app_on`, `press_key`, `render`, `screen`; add `use std::time::Duration;`, `use vega_core::client::Response;` and `use vega_core::response;` as needed):

```rust
    fn view_of(status: u16, body: &str) -> response::ResponseView {
        response::view(&Response {
            status,
            elapsed: Duration::from_millis(12),
            body: body.to_string(),
        })
    }

    /// Query.me with `id` selected, after pressing `r` (request 1 is in flight).
    fn running_app() -> App {
        use ratatui::crossterm::event::KeyCode;
        let mut app = design_app_on("me");
        press_key(&mut app, KeyCode::Down);
        press_key(&mut app, KeyCode::Char(' '));
        press_key(&mut app, KeyCode::Char('r'));
        app
    }

    #[test]
    fn run_mode_shows_query_variables_and_response_without_the_tree() {
        let mut app = running_app();
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("Query"));
        assert!(text.contains("Variables"));
        assert!(text.contains("Response"));
        assert!(text.contains("running"));
        assert!(text.contains("Waiting for the server"));
        assert!(!text.contains("Details"));
        assert!(!text.contains("Description"));

        app.apply_run_result(1, Ok(view_of(200, r#"{"data":{"me":{"id":"1"}}}"#)));
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("HTTP 200"));
        assert!(text.contains("12 ms"));
        assert!(text.contains(r#""id": "1""#));
    }

    #[test]
    fn graphql_errors_and_data_are_both_shown() {
        let mut app = running_app();
        app.apply_run_result(
            1,
            Ok(view_of(200, r#"{"data":{"me":null},"errors":[{"message":"boom"}]}"#)),
        );
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("error: boom"));
        assert!(text.contains("null"));
    }

    #[test]
    fn failures_and_http_errors_show_in_the_response_pane() {
        let mut app = running_app();
        app.apply_run_result(1, Err("request failed: connection refused".to_string()));
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("failed"));
        assert!(text.contains("connection refused"));

        let mut app = running_app();
        app.apply_run_result(1, Ok(view_of(500, "boom")));
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("HTTP 500"));
        assert!(text.contains("boom"));
    }

    #[test]
    fn scrolling_moves_the_first_visible_line() {
        use ratatui::crossterm::event::KeyCode;
        let mut app = running_app();
        let numbers: Vec<String> = (1000..1060).map(|n| n.to_string()).collect();
        let body = format!(r#"{{"data":[{}]}}"#, numbers.join(","));
        app.apply_run_result(1, Ok(view_of(200, &body)));
        assert!(screen(&render(&app, 1_000)).contains("1000"));
        for _ in 0..3 {
            press_key(&mut app, KeyCode::PageDown);
        }
        let text = screen(&render(&app, 1_000));
        assert!(!text.contains("1000,"), "scrolled past the top");
        assert!(text.contains("1032"));
    }

    #[test]
    fn a_notice_shows_in_the_status_bar() {
        use ratatui::crossterm::event::KeyCode;
        let mut app = design_app_on("me");
        press_key(&mut app, KeyCode::Char('r'));
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("select at least one field to run"));
    }

    #[test]
    fn run_mode_does_not_panic_on_tiny_terminals() {
        let mut app = running_app();
        app.apply_run_result(1, Ok(view_of(200, r#"{"data":{"me":{"id":"1"}}}"#)));
        for (w, h) in [(20, 5), (1, 1), (0, 0), (3, 40), (80, 24)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|frame| draw(frame, &app, 1_000)).unwrap();
        }
    }
```

(In `scrolling_moves_the_first_visible_line`: the pretty-printed array has one number per line, so after 30 lines of scroll the first visible number is `1030` and the pane (30 rows) shows up to about `1055`; if the exact numbers differ with the test terminal's height, adjust the asserted number to one that is visible after scrolling and not before, and say so.)

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p vega tui`
Expected: FAIL (`Run mode` renders the Design layout: no `Response` text).

- [ ] **Step 3: Implement**

Imports: `use crate::tui::app::{App, Edit, Focus, RunState, StatusLevel, VariableRow};` and `use vega_core::response::LineKind;`.

In `draw`, inside the operation branch (`is_operation`), replace the two-column drawing with:

```rust
        if is_operation {
            let columns =
                Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(rows[1]);
            if app.design.as_ref().is_some_and(|design| design.run.is_some()) {
                draw_query(frame, columns[0], app);
                draw_response(frame, columns[1], app);
            } else {
                draw_details(frame, columns[0], app, &tree, cursor);
                draw_query(frame, columns[1], app);
            }
        }
```

`draw_status`: add a style for the new level next to the others:

```rust
        StatusLevel::Notice => Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
```

Add:

```rust
/// The Response pane: status and timing in the title, errors in red, data below.
fn draw_response(frame: &mut Frame, area: Rect, app: &App) {
    let focused = has_focus(app, Focus::Response);
    let Some(run) = app.design.as_ref().and_then(|design| design.run.as_ref()) else {
        return;
    };
    let red = Style::default().fg(Color::Red);
    let dim = Style::default().fg(Color::DarkGray);
    let (title, title_style, lines): (String, Style, Vec<Line>) = match &run.state {
        RunState::Running => (
            "Response · running…".to_string(),
            Style::default(),
            vec![Line::styled("Waiting for the server…", dim)],
        ),
        RunState::Failed(message) => (
            "Response · failed".to_string(),
            red,
            vec![Line::styled(message.clone(), red)],
        ),
        RunState::Done(view) => {
            let title = format!("Response · HTTP {} · {} ms", view.status, view.elapsed_ms);
            let style = if view.ok() { Style::default() } else { red };
            let lines = view
                .body_lines()
                .into_iter()
                .skip(run.scroll)
                .take(usize::from(area.height))
                .map(|(kind, text)| match kind {
                    LineKind::Error => Line::styled(text, red),
                    LineKind::Note => Line::styled(text, dim),
                    LineKind::Data if view.not_json => Line::styled(text, red),
                    LineKind::Data => Line::raw(text),
                })
                .collect();
            (title, style, lines)
        }
    };
    let mut block = pane(&title, focused);
    if title_style != Style::default() && !focused {
        block = block.title_style(title_style);
    }
    frame.render_widget(Paragraph::new(lines).block(block), area);
}
```

(If `pane` takes `&'a str` and the borrow of `title` conflicts, build the block inline; keep the focused cyan border behavior. A non-JSON body is drawn red as the "not JSON" signal.)

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --workspace`
Expected: all pass.

- [ ] **Step 5: Check and commit**

Run: `just fix && just check`
Expected: exit code 0.

```bash
git add crates/vega/src/tui/ui.rs
git commit -m "feat: draw the Run layout and the Response pane"
```

---

### Task 6: Send requests from the event loop

**Files:**
- Modify: `crates/vega/src/tui/mod.rs`

**Interfaces:**
- Consumes: `Action::Run(RunRequest)`, `App::apply_run_result`, `client::execute`, `response::view`.
- Produces: `enum Update { Schema(anyhow::Result<CacheEntry>), Run { seq: u64, outcome: Result<ResponseView, String> } }` on the existing channel; `fn run_request(settings: &Settings, request: &RunRequest) -> Result<ResponseView, String>`; `fn error_chain(error: &dyn std::error::Error) -> String` (the error and its sources joined with `: `, with the API key masked).

- [ ] **Step 1: Write the failing tests**

Add a `tests` module to `crates/vega/src/tui/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn settings(uri: &str, key: Option<&str>) -> Settings {
        Settings {
            uri: uri.to_string(),
            api_key: key.map(str::to_string),
        }
    }

    fn request() -> RunRequest {
        RunRequest {
            seq: 7,
            body: serde_json::json!({"query": "query Me { me { id } }", "operationName": "Me", "variables": {}}),
        }
    }

    /// Answers one request with `body` and returns the endpoint URL.
    fn serve_once(body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let uri = format!("http://{}/graphql", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 8192];
            let _ = stream.read(&mut buf).unwrap();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
        });
        uri
    }

    #[test]
    fn run_request_returns_the_view_of_the_response() {
        let uri = serve_once(r#"{"data":{"me":{"id":"1"}}}"#);
        let view = run_request(&settings(&uri, None), &request()).unwrap();
        assert_eq!(view.status, 200);
        assert!(view.ok());
        assert!(view.lines.iter().any(|line| line.contains("\"id\": \"1\"")));
    }

    #[test]
    fn run_request_reports_connection_failures_as_text() {
        let message = run_request(&settings("http://127.0.0.1:9/graphql", Some("secret")), &request())
            .unwrap_err();
        assert!(message.starts_with("request failed"), "{message}");
        assert!(!message.contains("secret"));
    }

    #[test]
    fn error_chain_joins_the_sources() {
        #[derive(Debug, thiserror::Error)]
        #[error("outer")]
        struct Outer(#[source] Inner);
        #[derive(Debug, thiserror::Error)]
        #[error("inner")]
        struct Inner;
        assert_eq!(error_chain(&Outer(Inner)), "outer: inner");
    }
}
```

(`thiserror` is not a dependency of the `vega` crate; if it is missing, write the two error types by hand with `std::fmt::Display` and `std::error::Error::source` instead.)

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p vega tui::tests`
Expected: FAIL to compile (`cannot find function run_request`).

- [ ] **Step 3: Implement**

Replace the channel type and loop body in `mod.rs`:

```rust
use vega_core::client;
use vega_core::response::{self, ResponseView};

use app::{Action, App, RunRequest};

/// What background threads send back to the event loop.
enum Update {
    Schema(anyhow::Result<CacheEntry>),
    Run {
        seq: u64,
        outcome: Result<ResponseView, String>,
    },
}
```

In `event_loop`: `let (tx, rx) = mpsc::channel::<Update>();`, and

```rust
        while let Ok(update) = rx.try_recv() {
            match update {
                Update::Schema(result) => apply_fetch(&mut app, cache_file, result),
                Update::Run { seq, outcome } => app.apply_run_result(seq, outcome),
            }
        }

        // Short poll so the status bar age keeps ticking and results show up.
        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match app.handle_key(key) {
                        Action::None => {}
                        Action::Refresh => {
                            let settings = settings.clone();
                            let tx = tx.clone();
                            thread::spawn(move || {
                                let _ = tx.send(Update::Schema(schema_source::fetch_entry(&settings)));
                            });
                        }
                        Action::Run(request) => {
                            let settings = settings.clone();
                            let tx = tx.clone();
                            thread::spawn(move || {
                                let outcome = run_request(&settings, &request);
                                let _ = tx.send(Update::Run { seq: request.seq, outcome });
                            });
                        }
                    }
                }
            }
        }
```

Add:

```rust
/// Sends the request and summarizes the answer. A failure to get any HTTP answer comes
/// back as text for the Response pane, with the API key masked.
fn run_request(settings: &Settings, request: &RunRequest) -> Result<ResponseView, String> {
    client::execute(&settings.uri, settings.api_key.as_deref(), &request.body)
        .map(|response| response::view(&response))
        .map_err(|error| {
            let message = error_chain(&error);
            match settings.api_key.as_deref().filter(|key| !key.is_empty()) {
                Some(key) => message.replace(key, "***"),
                None => message,
            }
        })
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
```

Remove the temporary `matches!`/ignored-`Run` code from Task 4.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --workspace`
Expected: all pass.

- [ ] **Step 5: Check and commit**

Run: `just fix && just check`
Expected: exit code 0.

```bash
git add crates/vega/src/tui/mod.rs
git commit -m "feat: send queries from the event loop on a background thread"
```

---

### Task 7: Documentation and version

**Files:**
- Modify: `docs/superpowers/specs/2026-10-22-vega-phase-2-design.md`, `README.md`, `AGENTS.md`, `Cargo.toml`, `crates/vega/Cargo.toml`, `Cargo.lock`

- [ ] **Step 1: Update the spec**

- Header: change `Status: approved design, pending implementation plan` to `Status: implemented`.
- Keys section, Run line: replace with `Run: Up/Down scroll the response a line and PgUp/PgDn ten lines; `r` runs again; Tab switches between Response and Variables; Esc returns to Design with the selection intact.` and add to the Design line that `r` runs the query from any pane.
- Running section: add bullets `The Response pane shows at most 10,000 lines of the pretty-printed response and a note with the real line count when it is capped; lines are not wrapped. --run prints everything.` and `Leaving Run mode while a request is running discards its result; only one request runs at a time.`
- CLI parity section: replace the `--run` bullets with `--run sends the request and prints the response body, pretty-printed, on stdout; --json prints an envelope {operationName, status, elapsedMs, response} instead. The exit status is non-zero when the request fails, the HTTP status is not 2xx, the body is not JSON, or the response has GraphQL errors (the body is still printed; the reason goes to stderr). A mutation asks "Run mutation <Name>? y/N" on the terminal; without a terminal it requires --yes. --yes requires --run. Subscriptions cannot be run.`
- Delivery section: add `(done)` to step 3. In "Open items for the plans", remove the response-cap bullet (decided above).

- [ ] **Step 2: Update README and AGENTS.md**

README, in the Usage block add:

```sh
vega --query Query.userRead --select id,name --var id=42 --run   # run it and print the response
```

and extend the "What it does" bullet about building queries with `, press r to run it and read the response (mutations ask first)`. Replace the sentence "Building the query and editing its variables works; running queries against the server is planned." with "Building, editing and running queries works; subscriptions can be built but not run."

AGENTS.md: in the Scope section change the first bullet to `- Explore the schema, build a query from selected fields, edit its variables and run it against the server. Subscriptions can be built but not run.` and in the Rules section change the API key rule to also say the key never appears in the Response pane or in `--run` output (the client masks it).

- [ ] **Step 3: Bump the version**

Per AGENTS.md, the final commit of the work bumps the minor version: set `version = "0.4.0"` under `[workspace.package]` in `Cargo.toml`, the `vega-core` dependency version in `crates/vega/Cargo.toml` to `0.4.0`, and let `cargo build` update `Cargo.lock`.

- [ ] **Step 4: Check and commit in two commits**

Run: `just check`
Expected: exit code 0.

```bash
git add docs/superpowers/specs/2026-10-22-vega-phase-2-design.md README.md AGENTS.md
git commit -m "doc: describe running queries and the --run flag"
git add Cargo.toml crates/vega/Cargo.toml Cargo.lock
git commit -m "chore: bump the version to 0.4.0"
```

---

### Task 8: End-to-end verification

**Files:** none (verification only).

- [ ] **Step 1: Run the full checks**

Run: `just check`
Expected: exit code 0.

- [ ] **Step 2: Try `--run` against a local listener**

Run `cargo test -p vega --test cli run` (they use a local `127.0.0.1` listener and a seeded cache) and confirm by reading the assertions that: the response body is printed and the request carries `operationName` and `variables`; `--json` prints the envelope; GraphQL errors and HTTP errors exit non-zero with the reason on stderr and the key never appears; a mutation needs `--yes` without a terminal; a missing required variable fails before anything is sent.

- [ ] **Step 3: Hand the interactive check to the user**

The TUI cannot be driven without a terminal. Ask the user to run `vega` against their server and check: in Design, `r` on a query with a selection switches to Run mode (Query over Variables on the left, Response on the right, no Details or Description) and shows `running…` then `HTTP 200 · N ms` with the pretty-printed data; GraphQL errors show in red above the data; Up/Down and PgUp/PgDn scroll; Tab switches to Variables and Enter edits a value, then `r` runs again with it; Esc returns to Design with the selection intact; `r` on a mutation asks `Run mutation <Name>? y/N` in the status bar and only `y` sends it; `r` with a missing required variable shows the reason in the status bar and does not send; pointing `--uri` at a dead port shows a red `failed` message in the pane; the API key never shows anywhere.

---

## Self-review (spec coverage)

| Spec requirement (step 3) | Task |
| --- | --- |
| `client::execute` sharing the HTTP code with introspection | 1 |
| Request is a POST with query, variables, operationName and the X-API-Key header; empty optional variable omitted | 1, 2, 4 |
| Running blocked while a variable is invalid or required is empty; status line names the problem | 2, 4 |
| Mutation confirmation `Run mutation <name>? y/N` | 4 |
| Background thread, one request in flight | 4, 6 |
| Response pane: pretty JSON, HTTP status, elapsed time, GraphQL errors highlighted, data and errors both shown | 2, 5 |
| Network failures, non-2xx statuses and invalid JSON shown as errors in the same pane | 2, 5, 6 |
| Run layout (Query over Variables, Response) and Run keys | 4, 5 |
| Variable values and the API key never written or shown | 1, 6 |
| Subscriptions generate a query but cannot be run | 3, 4 |
| `--run`, `--yes`, `--json`; mutation without a terminal requires `--yes`; stderr/stdout discipline | 3 |
| Very large responses (open item) | 2, 5 (Decision 1) |
| `execute` tests with a local listener; CLI tests with a seeded cache and a listener | 1, 3 |
