use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;

use vega_core::client::{fetch_introspection, ClientError};

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
    let response = fetch_introspection(&uri, Some("secret")).unwrap();
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
    fetch_introspection(&uri, None).unwrap();
    assert!(!rx.recv().unwrap().to_lowercase().contains("x-api-key"));
}

#[test]
fn http_errors_surface_status_and_body() {
    let (uri, _rx) = serve_once("401 Unauthorized", "bad key".to_string());
    let err = fetch_introspection(&uri, Some("nope")).unwrap_err();
    assert!(matches!(&err, ClientError::Status { status: 401, body } if body == "bad key"));
}

#[test]
fn graphql_errors_in_a_200_response_are_reported() {
    let body = r#"{"errors":[{"message":"Introspection is disabled"}]}"#.to_string();
    let (uri, _rx) = serve_once("200 OK", body);
    let err = fetch_introspection(&uri, None).unwrap_err();
    assert!(matches!(&err, ClientError::Graphql(m) if m == "Introspection is disabled"));
}
