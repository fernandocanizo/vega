use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;

use vega_core::client::{execute, fetch_introspection, operation_body, ClientError};
use vega_core::headers::Header;

fn key(value: &str) -> Vec<Header> {
    vec![Header::new("X-API-Key", value).unwrap()]
}

const FIXTURE: &str = include_str!("fixtures/introspection.json");

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn read_request(stream: &mut TcpStream) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = stream.read(&mut chunk).unwrap();
        buf.extend_from_slice(&chunk[..n]);
        if n == 0 {
            break;
        }
        if let Some(pos) = find(&buf, b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&buf[..pos]).to_lowercase();
            let length = headers
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .map(|v| v.trim().parse::<usize>().unwrap())
                .unwrap_or(0);
            if buf.len() >= pos + 4 + length {
                break;
            }
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// Serves exactly one request on a random local port. Returns the endpoint URL and
/// a channel that yields the raw request text.
fn serve_once(status_line: &'static str, body: String) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let uri = format!("http://{}/graphql", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        tx.send(read_request(&mut stream)).unwrap();
        let response = format!(
            "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
    });
    (uri, rx)
}

#[test]
fn sends_a_post_with_the_api_key_header_and_parses_the_schema() {
    let (uri, rx) = serve_once("200 OK", FIXTURE.to_string());
    let response = fetch_introspection(&uri, &key("secret")).unwrap();
    assert_eq!(response.data.schema.types.len(), 10);

    let request = rx.recv().unwrap().to_lowercase();
    assert!(request.starts_with("post /graphql"));
    assert!(request.contains("x-api-key: secret"));
    assert!(request.contains("content-type: application/json"));
    assert!(request.contains("introspectionquery"));
}

#[test]
fn omits_the_api_key_header_when_no_key_is_set() {
    let (uri, rx) = serve_once("200 OK", FIXTURE.to_string());
    fetch_introspection(&uri, &[]).unwrap();
    assert!(!rx.recv().unwrap().to_lowercase().contains("x-api-key"));
}

#[test]
fn http_errors_surface_status_and_body() {
    let (uri, _rx) = serve_once("401 Unauthorized", "bad key".to_string());
    let err = fetch_introspection(&uri, &key("nope")).unwrap_err();
    assert!(matches!(&err, ClientError::Status { status: 401, body } if body == "bad key"));
}

#[test]
fn graphql_errors_in_a_200_response_are_reported() {
    let body = r#"{"errors":[{"message":"Introspection is disabled"}]}"#.to_string();
    let (uri, _rx) = serve_once("200 OK", body);
    let err = fetch_introspection(&uri, &[]).unwrap_err();
    assert!(matches!(&err, ClientError::Graphql(m) if m == "Introspection is disabled"));
}

#[test]
fn connection_failure_displays_without_repeating_the_cause() {
    let err = fetch_introspection("http://127.0.0.1:9/graphql", &[]).unwrap_err();
    assert!(matches!(err, ClientError::Http(_)));
    assert_eq!(err.to_string(), "request failed");
}

#[test]
fn server_error_bodies_do_not_leak_the_api_key() {
    let (uri, _rx) = serve_once("500 Internal Server Error", "bad key: secret-key!".into());
    let err = fetch_introspection(&uri, &key("secret-key")).unwrap_err();
    match err {
        ClientError::Status { body, .. } => {
            assert!(body.contains("***"), "{body}");
            assert!(!body.contains("secret-key"), "{body}");
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn a_secret_crossing_the_cut_of_an_error_body_does_not_leak() {
    let body = format!("{}secret-key-abcdefghij{}", "a".repeat(290), "b".repeat(50));
    let (uri, _rx) = serve_once("500 Internal Server Error", body);
    let err = fetch_introspection(&uri, &key("secret-key-abcdefghij")).unwrap_err();
    match err {
        ClientError::Status { body, .. } => {
            assert!(body.contains("***"), "{body}");
            assert!(!body.contains("secret"), "{body}");
        }
        other => panic!("unexpected error: {other}"),
    }
}

fn me_body() -> serde_json::Value {
    let variables = serde_json::from_str(r#"{"id":"1"}"#).unwrap();
    operation_body("Me", "query Me { me { id } }", &variables)
}

#[test]
fn execute_posts_the_operation_with_its_variables_and_the_key() {
    let (uri, rx) = serve_once("200 OK", r#"{"data":{"me":null}}"#.to_string());
    let response = execute(&uri, &key("secret"), &me_body()).unwrap();
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
    execute(&uri, &[], &me_body()).unwrap();
    assert!(!rx.recv().unwrap().to_lowercase().contains("x-api-key"));
}

#[test]
fn execute_returns_any_http_status_as_a_response() {
    let (uri, _rx) = serve_once("500 Internal Server Error", "boom".to_string());
    let response = execute(&uri, &[], &me_body()).unwrap();
    assert_eq!(response.status, 500);
    assert_eq!(response.body, "boom");
}

#[test]
fn execute_returns_graphql_errors_as_a_normal_response() {
    let body = r#"{"data":null,"errors":[{"message":"nope"}]}"#;
    let (uri, _rx) = serve_once("200 OK", body.to_string());
    let response = execute(&uri, &[], &me_body()).unwrap();
    assert_eq!(response.body, body);
}

#[test]
fn execute_masks_the_api_key_in_the_body() {
    let (uri, _rx) = serve_once("200 OK", "echo secret-key!".to_string());
    let response = execute(&uri, &key("secret-key"), &me_body()).unwrap();
    assert!(response.body.contains("***"), "{}", response.body);
    assert!(!response.body.contains("secret-key"), "{}", response.body);
}

#[test]
fn execute_reports_connection_failures() {
    let err = execute("http://127.0.0.1:9/graphql", &[], &me_body()).unwrap_err();
    assert!(matches!(err, ClientError::Http(_)));
}

#[test]
fn execute_sends_every_header_and_masks_each_value() {
    let (uri, rx) = serve_once("200 OK", "echo tok-123 and team-9".to_string());
    let headers = vec![
        Header::new("Authorization", "Bearer tok-123").unwrap(),
        Header::new("X-Tenant", "team-9").unwrap(),
    ];
    let response = execute(&uri, &headers, &me_body()).unwrap();
    let request = rx.recv().unwrap().to_lowercase();
    assert!(
        request.contains("authorization: bearer tok-123"),
        "{request}"
    );
    assert!(request.contains("x-tenant: team-9"), "{request}");
    assert!(!response.body.contains("tok-123"), "{}", response.body);
    assert!(!response.body.contains("team-9"), "{}", response.body);
}

/// Answers one request with a redirect (and an optional `Location`) and nothing else.
fn serve_redirect(status_line: &'static str, location: Option<String>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let uri = format!("http://{}/graphql", listener.local_addr().unwrap());
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        read_request(&mut stream);
        let location = location
            .map(|location| format!("Location: {location}\r\n"))
            .unwrap_or_default();
        let response = format!(
            "HTTP/1.1 {status_line}\r\n{location}Content-Length: 0\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(response.as_bytes()).unwrap();
    });
    uri
}

/// A listener on another origin, to check that a redirect never reaches it. Returns it
/// with its base URL.
fn bystander() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    (listener, base)
}

fn nothing_connected(listener: &TcpListener) -> bool {
    matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock)
}

#[test]
fn a_redirect_is_reported_and_nothing_is_sent_to_its_target() {
    for status_line in [
        "301 Moved Permanently",
        "302 Found",
        "303 See Other",
        "307 Temporary Redirect",
        "308 Permanent Redirect",
    ] {
        let (target_listener, base) = bystander();
        let uri = serve_redirect(status_line, Some(format!("{base}/landing")));
        let err = fetch_introspection(&uri, &key("k1-secret")).unwrap_err();
        let status: u16 = status_line[..3].parse().unwrap();
        match &err {
            ClientError::Redirected {
                status: got,
                target,
            } => {
                assert_eq!(*got, status, "{status_line}");
                assert_eq!(target.as_deref(), Some(format!("{base}/landing").as_str()));
            }
            other => panic!("{status_line}: unexpected error: {other}"),
        }
        assert!(
            nothing_connected(&target_listener),
            "the redirect target was contacted ({status_line})"
        );
    }
}

#[test]
fn execute_reports_a_redirect_too_and_contacts_nobody_else() {
    let (target_listener, base) = bystander();
    let uri = serve_redirect("307 Temporary Redirect", Some(format!("{base}/landing")));
    let err = execute(&uri, &key("k1-secret"), &me_body()).unwrap_err();
    assert!(
        matches!(err, ClientError::Redirected { status: 307, .. }),
        "{err}"
    );
    assert!(nothing_connected(&target_listener));
}

#[test]
fn the_redirect_message_names_the_status_and_the_target() {
    let (_target_listener, base) = bystander();
    let uri = serve_redirect("301 Moved Permanently", Some(format!("{base}/landing")));
    let message = fetch_introspection(&uri, &[]).unwrap_err().to_string();
    assert_eq!(
        message,
        format!(
            "server redirected the request (HTTP 301) to {base}/landing: use that URL as the endpoint"
        )
    );
}

#[test]
fn a_relative_location_is_resolved_against_the_endpoint() {
    let uri = serve_redirect("301 Moved Permanently", Some("/v2/graphql".to_string()));
    let origin = uri.trim_end_matches("/graphql").to_string();
    match fetch_introspection(&uri, &[]).unwrap_err() {
        ClientError::Redirected { target, .. } => {
            assert_eq!(target, Some(format!("{origin}/v2/graphql")));
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn the_target_has_no_query_fragment_or_credentials() {
    let (_target_listener, base) = bystander();
    let host = base.trim_start_matches("http://");
    let location = format!("http://user:pw-secret@{host}/landing?token=abc-secret#frag");
    let uri = serve_redirect("301 Moved Permanently", Some(location));
    let message = fetch_introspection(&uri, &[]).unwrap_err().to_string();
    assert!(
        message.contains(&format!("to {base}/landing:")),
        "{message}"
    );
    for secret in ["pw-secret", "abc-secret", "frag", "user"] {
        assert!(!message.contains(secret), "{secret} leaked: {message}");
    }
}

#[test]
fn a_redirect_without_a_location_says_so() {
    let uri = serve_redirect("301 Moved Permanently", None);
    let message = fetch_introspection(&uri, &[]).unwrap_err().to_string();
    assert_eq!(
        message,
        "server redirected the request (HTTP 301) without saying where to"
    );
}

#[test]
fn other_statuses_are_still_not_redirects() {
    let (uri, _rx) = serve_once("304 Not Modified", String::new());
    let err = fetch_introspection(&uri, &[]).unwrap_err();
    assert!(
        matches!(err, ClientError::Status { status: 304, .. }),
        "{err}"
    );
}
