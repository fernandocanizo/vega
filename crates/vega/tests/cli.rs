use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::thread;

use tempfile::TempDir;
use vega_core::cache::{cache_path, now_secs, store, CacheEntry};
use vega_core::schema::IntrospectionResponse;

const FIXTURE: &str = include_str!("../../vega-core/tests/fixtures/introspection.json");
// Port 9 (discard) refuses connections: any accidental network use fails loudly.
const URI: &str = "http://127.0.0.1:9/graphql";

/// A temp XDG cache dir holding a schema fetched `age_secs` ago.
fn seeded_cache(age_secs: u64) -> TempDir {
    seeded_cache_for(URI, age_secs)
}

fn seeded_cache_for(uri: &str, age_secs: u64) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let response: IntrospectionResponse = serde_json::from_str(FIXTURE).unwrap();
    let entry = CacheEntry {
        endpoint: uri.to_string(),
        fetched_at: now_secs() - age_secs,
        response,
    };
    store(&cache_path(&dir.path().join("vega"), uri), &entry).unwrap();
    dir
}

fn vega(cache: &TempDir, args: &[&str]) -> Output {
    vega_with(cache, URI, None, args)
}

fn vega_with(cache: &TempDir, uri: &str, key: Option<&str>, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_vega"));
    command
        .args(args)
        .env("XDG_CACHE_HOME", cache.path())
        .env("XDG_CONFIG_HOME", cache.path().join("config"))
        .env("VEGA_GRAPHQL_URI", uri)
        .env_remove("VEGA_GRAPHQL_HEADERS")
        .env_remove("VEGA_GRAPHQL_KEY_HEADER")
        .stdin(Stdio::null());
    match key {
        Some(key) => command.env("VEGA_GRAPHQL_KEY", key),
        None => command.env_remove("VEGA_GRAPHQL_KEY"),
    };
    command.output().unwrap()
}

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

/// Serves exactly one request on a random local port; returns the endpoint URL and a
/// channel that yields the raw request text.
fn serve_once(status_line: &'static str, body: &str) -> (String, mpsc::Receiver<String>) {
    serve_with_headers(status_line, "", body)
}

/// Like `serve_once`, with extra response header lines (each ending in `\r\n`).
fn serve_with_headers(
    status_line: &'static str,
    extra_headers: &'static str,
    body: &str,
) -> (String, mpsc::Receiver<String>) {
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
            "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\n{extra_headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
    });
    (uri, rx)
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn search_prints_matching_operations_and_types() {
    let cache = seeded_cache(60);
    let out = vega(&cache, &["--search", "user"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("Query.userRead  userRead(id: ID!): User\n"));
    assert!(text.contains("Mutation.userCreate  "));
    assert!(text.contains("UserInput  INPUT_OBJECT\n"));
    assert_eq!(stderr(&out), "");
}

#[test]
fn search_json_is_valid_and_ranked() {
    let cache = seeded_cache(60);
    let out = vega(&cache, &["--search", "user", "--json"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 5);
    assert_eq!(json[0]["path"], "Query.userRead");
    assert_eq!(json[0]["kind"], "query");
}

#[test]
fn describe_prints_docs_and_navigation_items() {
    let cache = seeded_cache(60);
    let out = vega(&cache, &["--describe", "Query.userRead"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.starts_with("Query.userRead(id: ID!): User\n"));
    assert!(text.contains("Requires USER_READ"));
    assert!(text.contains("  returns User\n"));
}

#[test]
fn describe_json_matches_the_detail_shape() {
    let cache = seeded_cache(60);
    let out = vega(&cache, &["--describe", "Role", "--json"]);
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(json["title"], "Role (ENUM)");
    assert_eq!(json["nav"][1]["label"], "VIEWER");
}

#[test]
fn describe_of_an_unknown_target_fails_with_a_message() {
    let cache = seeded_cache(60);
    let out = vega(&cache, &["--describe", "Nope"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("type `Nope` not found"));
    assert_eq!(stdout(&out), "");
}

#[test]
fn stale_cache_warns_on_stderr_and_keeps_stdout_clean() {
    let cache = seeded_cache(25 * 3600);
    let out = vega(&cache, &["--search", "userread", "--json"]);
    assert!(out.status.success());
    let err = stderr(&out);
    assert!(err.contains("warning"));
    assert!(err.contains("1d"));
    assert!(err.contains("--refresh"));
    assert!(serde_json::from_str::<serde_json::Value>(&stdout(&out)).is_ok());
}

#[test]
fn fresh_cache_prints_no_warning() {
    let cache = seeded_cache(3600);
    let out = vega(&cache, &["--search", "userread"]);
    assert_eq!(stderr(&out), "");
}

#[test]
fn missing_endpoint_explains_how_to_set_one() {
    let cache = seeded_cache(60);
    let out = Command::new(env!("CARGO_BIN_EXE_vega"))
        .args(["--search", "x"])
        .env("XDG_CACHE_HOME", cache.path())
        .env("XDG_CONFIG_HOME", cache.path().join("config"))
        .env_remove("VEGA_GRAPHQL_URI")
        .env_remove("VEGA_GRAPHQL_KEY")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(stderr(&out).contains("--uri"));
}

#[test]
fn the_prefixed_endpoint_variable_is_read() {
    let cache = seeded_cache(60);
    let out = Command::new(env!("CARGO_BIN_EXE_vega"))
        .args(["--search", "userread"])
        .env("XDG_CACHE_HOME", cache.path())
        .env("XDG_CONFIG_HOME", cache.path().join("config"))
        .env("VEGA_GRAPHQL_URI", URI)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
}

#[test]
fn failed_refresh_reports_an_error_and_keeps_the_cache() {
    let cache = seeded_cache(60);
    let path = cache_path(&cache.path().join("vega"), URI);
    let before = std::fs::read(&path).unwrap();
    let out = vega(&cache, &["--refresh", "--search", "x"]);
    assert!(!out.status.success());
    assert!(!stderr(&out).trim().is_empty());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn cached_run_without_refresh_does_not_touch_the_network() {
    let cache = seeded_cache(60);
    let out = vega(&cache, &["--search", "x"]);
    assert!(out.status.success(), "{}", stderr(&out));
}

#[test]
fn broken_config_file_is_ignored_when_an_endpoint_is_given() {
    let cache = seeded_cache(60);
    let config_dir = cache.path().join("config").join("vega");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(config_dir.join("config.toml"), "uri = [not toml").unwrap();
    let out = vega(&cache, &["--search", "user"]);
    assert!(out.status.success(), "{}", stderr(&out));
}

#[test]
fn query_prints_the_generated_query_and_its_variables() {
    let cache = seeded_cache(60);
    let out = vega(
        &cache,
        &["--query", "Query.userRead", "--select", "id,name"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "query UserRead($id: ID!) {\n  userRead(id: $id) {\n    id\n    name\n  }\n}\n\nVariables:\n  $id: ID!\n"
    );
    assert_eq!(stderr(&out), "");
}

#[test]
fn query_json_has_the_operation_name_query_and_variables() {
    let cache = seeded_cache(60);
    let out = vega(
        &cache,
        &["--query", "Query.userRead", "--select", "role", "--json"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(json["operationName"], "UserRead");
    assert!(json["query"].as_str().unwrap().contains("role"));
    assert_eq!(json["variables"][0]["name"], "id");
    assert_eq!(json["variables"][0]["type"], "ID!");
}

#[test]
fn query_without_select_on_an_object_asks_for_a_selection() {
    let cache = seeded_cache(60);
    let out = vega(&cache, &["--query", "Query.userRead"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("nothing selected"));
    assert_eq!(stdout(&out), "");
}

#[test]
fn query_with_an_unknown_operation_or_field_fails_with_a_message() {
    let cache = seeded_cache(60);
    let out = vega(&cache, &["--query", "Query.nope", "--select", "id"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("not found"));

    let out = vega(&cache, &["--query", "Query.userRead", "--select", "nope"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("no such field"));

    let out = vega(&cache, &["--query", "Query.userRead", "--select", "id.x"]);
    assert!(!out.status.success());
}

#[test]
fn select_requires_query_and_query_conflicts_with_search() {
    let cache = seeded_cache(60);
    let out = vega(&cache, &["--select", "id"]);
    assert!(!out.status.success());
    let out = vega(&cache, &["--query", "Query.userRead", "--search", "user"]);
    assert!(!out.status.success());
}

#[test]
fn query_warns_about_a_stale_cache_on_stderr_only() {
    let cache = seeded_cache(25 * 3600);
    let out = vega(
        &cache,
        &["--query", "Query.userRead", "--select", "id", "--json"],
    );
    assert!(out.status.success());
    assert!(stderr(&out).contains("warning"));
    assert!(serde_json::from_str::<serde_json::Value>(&stdout(&out)).is_ok());
}

#[test]
fn var_sets_the_value_shown_next_to_the_variable() {
    let cache = seeded_cache(60);
    let out = vega(
        &cache,
        &[
            "--query",
            "Query.userRead",
            "--select",
            "id,name",
            "--var",
            "id=42",
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "query UserRead($id: ID!) {\n  userRead(id: $id) {\n    id\n    name\n  }\n}\n\nVariables:\n  $id: ID! = 42\n"
    );
    assert_eq!(stderr(&out), "");
}

#[test]
fn var_value_appears_in_the_json_output() {
    let cache = seeded_cache(60);
    let out = vega(
        &cache,
        &[
            "--query",
            "Query.userRead",
            "--select",
            "id",
            "--var",
            "id=42",
            "--json",
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(json["variables"][0]["name"], "id");
    assert_eq!(json["variables"][0]["value"], "42");
}

#[test]
fn select_can_opt_an_optional_argument_in() {
    let cache = seeded_cache(60);
    let out = vega(
        &cache,
        &[
            "--query",
            "Query.userSearch",
            "--select",
            "id,arg:term",
            "--var",
            "term=bob",
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).starts_with(
        "query UserSearch($term: String) {\n  userSearch(term: $term) {\n    id\n  }\n}"
    ));
    assert!(stdout(&out).contains("$term: String = bob"));
}

#[test]
fn var_for_an_unknown_variable_fails_with_a_message() {
    let cache = seeded_cache(60);
    let out = vega(
        &cache,
        &[
            "--query",
            "Query.userRead",
            "--select",
            "id",
            "--var",
            "nope=1",
        ],
    );
    assert!(!out.status.success());
    assert!(stderr(&out).contains("no such variable"));
    assert_eq!(stdout(&out), "");
}

#[test]
fn var_with_an_invalid_value_fails_and_prints_nothing() {
    let cache = seeded_cache(60);
    let out = vega(
        &cache,
        &[
            "--query",
            "Mutation.userCreate",
            "--select",
            "id",
            "--var",
            "input=notjson",
        ],
    );
    assert!(!out.status.success());
    assert!(stderr(&out).contains("not valid JSON"));
    assert_eq!(stdout(&out), "");
}

#[test]
fn var_requires_query() {
    let cache = seeded_cache(60);
    let out = vega(&cache, &["--var", "id=1"]);
    assert!(!out.status.success());
}

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
        &[
            "--query",
            "Query.userRead",
            "--select",
            "id,name",
            "--var",
            "id=42",
            "--run",
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("\"name\": \"Ann\""),
        "{}",
        stdout(&out)
    );
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
        &[
            "--query",
            "Query.userRead",
            "--select",
            "id",
            "--var",
            "id=42",
            "--run",
            "--json",
        ],
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
        &[
            "--query",
            "Query.userRead",
            "--select",
            "id",
            "--var",
            "id=1",
            "--run",
        ],
    );
    assert!(rx
        .recv()
        .unwrap()
        .to_lowercase()
        .contains("x-api-key: secret-key"));
    assert!(!stdout(&out).contains("secret-key"), "{}", stdout(&out));
    assert!(!stderr(&out).contains("secret-key"), "{}", stderr(&out));
}

#[test]
fn run_never_prints_the_api_key_when_the_connection_fails() {
    let cache = seeded_cache(60); // the endpoint refuses connections
    let out = vega_with(
        &cache,
        URI,
        Some("secret-key"),
        &[
            "--query",
            "Query.userRead",
            "--select",
            "id",
            "--var",
            "id=1",
            "--run",
        ],
    );
    assert!(!out.status.success());
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
        &[
            "--query",
            "Query.userRead",
            "--select",
            "id",
            "--var",
            "id=1",
            "--run",
        ],
    );
    assert!(!out.status.success());
    assert!(stdout(&out).contains("nope"));
    assert!(
        stderr(&out).contains("GraphQL errors: nope"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn run_reports_http_errors() {
    let (uri, _rx) = serve_once("500 Internal Server Error", "boom");
    let cache = seeded_cache_for(&uri, 60);
    let out = vega_with(
        &cache,
        &uri,
        None,
        &[
            "--query",
            "Query.userRead",
            "--select",
            "id",
            "--var",
            "id=1",
            "--run",
        ],
    );
    assert!(!out.status.success());
    assert!(stderr(&out).contains("HTTP 500"), "{}", stderr(&out));
}

#[test]
fn run_refuses_a_missing_required_variable_without_sending() {
    let cache = seeded_cache(60); // the endpoint refuses connections
    let out = vega(
        &cache,
        &["--query", "Query.userRead", "--select", "id", "--run"],
    );
    assert!(!out.status.success());
    assert!(stderr(&out).contains("required"), "{}", stderr(&out));
    assert_eq!(stdout(&out), "");
}

#[test]
fn a_mutation_needs_yes_without_a_terminal() {
    let input = r#"input={"name":"Ann"}"#;
    let cache = seeded_cache(60);
    let out = vega(
        &cache,
        &[
            "--query",
            "Mutation.userCreate",
            "--select",
            "id",
            "--var",
            input,
            "--run",
        ],
    );
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("without a terminal"),
        "{}",
        stderr(&out)
    );

    let (uri, rx) = serve_once("200 OK", r#"{"data":{"userCreate":{"id":"1"}}}"#);
    let cache = seeded_cache_for(&uri, 60);
    let out = vega_with(
        &cache,
        &uri,
        None,
        &[
            "--query",
            "Mutation.userCreate",
            "--select",
            "id",
            "--var",
            input,
            "--run",
            "--yes",
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(rx
        .recv()
        .unwrap()
        .contains(r#""operationName":"UserCreate""#));
}

#[test]
fn run_warns_about_a_stale_cache_on_stderr_only() {
    let (uri, _rx) = serve_once("200 OK", r#"{"data":{"userRead":{"id":"1"}}}"#);
    let cache = seeded_cache_for(&uri, 25 * 3600);
    let out = vega_with(
        &cache,
        &uri,
        None,
        &[
            "--query",
            "Query.userRead",
            "--select",
            "id",
            "--var",
            "id=1",
            "--run",
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stderr(&out).contains("warning"));
    assert!(serde_json::from_str::<serde_json::Value>(&stdout(&out)).is_ok());
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
        &[
            "--query",
            "Query.userRead",
            "--select",
            "id",
            "--var",
            "id=1",
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));
}

#[test]
fn headers_variable_sends_every_header_and_hides_the_values() {
    let (uri, rx) = serve_once(
        "200 OK",
        r#"{"data":{"userRead":{"id":"echo tok-123 team-9"}}}"#,
    );
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
    assert!(
        request.contains("authorization: bearer tok-123"),
        "{request}"
    );
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
    assert!(rx
        .recv()
        .unwrap()
        .to_lowercase()
        .contains("x-from-env: k-1"));

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
    assert!(
        stderr(&out).contains("VEGA_GRAPHQL_HEADERS line 1"),
        "{}",
        stderr(&out)
    );
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

#[test]
fn the_key_alone_still_goes_in_the_x_api_key_header() {
    let (uri, rx) = serve_once("200 OK", r#"{"data":{"userRead":null}}"#);
    let cache = seeded_cache_for(&uri, 60);
    vega_env(&cache, &uri, &[("VEGA_GRAPHQL_KEY", "k-1")], &RUN_ARGS);
    assert!(rx.recv().unwrap().to_lowercase().contains("x-api-key: k-1"));
}

#[test]
fn a_redirect_is_reported_and_the_key_never_reaches_the_target() {
    let target = TcpListener::bind("127.0.0.1:0").unwrap();
    target.set_nonblocking(true).unwrap();
    let landing = format!("http://{}/graphql", target.local_addr().unwrap());
    let location: &'static str = Box::leak(format!("Location: {landing}\r\n").into_boxed_str());
    let (uri, _rx) = serve_with_headers("301 Moved Permanently", location, "");
    let cache = tempfile::tempdir().unwrap(); // nothing cached: vega has to fetch
    let out = vega_env(
        &cache,
        &uri,
        &[("VEGA_GRAPHQL_KEY", "k1-secret")],
        &["--search", "x"],
    );
    assert!(!out.status.success());
    assert_eq!(stdout(&out), "");
    let message = stderr(&out);
    assert!(
        message.contains("server redirected the request (HTTP 301)"),
        "{message}"
    );
    assert!(message.contains(&landing), "{message}");
    assert!(!message.contains("k1-secret"), "{message}");
    assert!(
        matches!(target.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
        "the redirect target was contacted"
    );
}
