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
