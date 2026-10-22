use std::process::{Command, Output};

use tempfile::TempDir;
use vega_core::cache::{cache_path, now_secs, store, CacheEntry};
use vega_core::schema::IntrospectionResponse;

const FIXTURE: &str = include_str!("../../vega-core/tests/fixtures/introspection.json");
// Port 9 (discard) refuses connections: any accidental network use fails loudly.
const URI: &str = "http://127.0.0.1:9/graphql";

/// A temp XDG cache dir holding a schema fetched `age_secs` ago.
fn seeded_cache(age_secs: u64) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let response: IntrospectionResponse = serde_json::from_str(FIXTURE).unwrap();
    let entry = CacheEntry {
        endpoint: URI.to_string(),
        fetched_at: now_secs() - age_secs,
        response,
    };
    store(&cache_path(&dir.path().join("vega"), URI), &entry).unwrap();
    dir
}

fn vega(cache: &TempDir, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_vega"))
        .args(args)
        .env("XDG_CACHE_HOME", cache.path())
        .env("XDG_CONFIG_HOME", cache.path().join("config"))
        .env("VEGA_GRAPHQL_URI", URI)
        .env_remove("VEGA_GRAPHQL_KEY")
        .output()
        .unwrap()
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
