//! Fetches the schema through a standard GraphQL introspection query.

use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

use crate::headers::{self, Header};
use crate::schema::IntrospectionResponse;

/// Standard introspection query. It leaves out `interfaces`, `isRepeatable` and
/// directives, which phase 1 does not use and which older servers may reject.
pub const INTROSPECTION_QUERY: &str = r#"
query IntrospectionQuery {
  __schema {
    queryType { name }
    mutationType { name }
    subscriptionType { name }
    types { ...FullType }
  }
}
fragment FullType on __Type {
  kind name description
  fields(includeDeprecated: true) {
    name description
    args { ...InputValue }
    type { ...TypeRef }
    isDeprecated deprecationReason
  }
  inputFields { ...InputValue }
  enumValues(includeDeprecated: true) { name description isDeprecated deprecationReason }
  possibleTypes { ...TypeRef }
}
fragment InputValue on __InputValue {
  name description
  type { ...TypeRef }
  defaultValue
}
fragment TypeRef on __Type {
  kind name
  ofType { kind name ofType { kind name ofType { kind name ofType { kind name ofType { kind name ofType { kind name ofType { kind name } } } } } } }
}
"#;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("request failed")]
    Http(#[from] reqwest::Error),
    #[error("server answered HTTP {status}: {body}")]
    Status { status: u16, body: String },
    /// Redirects are never followed: the request would become a `GET`, and the headers
    /// (which hold the credentials) would go to whatever origin the answer names.
    #[error("server redirected the request (HTTP {status}) {}", redirect_hint(.target))]
    Redirected { status: u16, target: Option<String> },
    #[error("response is not a valid introspection result: {0}")]
    InvalidJson(serde_json::Error),
    #[error("server returned GraphQL errors: {0}")]
    Graphql(String),
    #[error("response has no `data`")]
    MissingData,
}

fn redirect_hint(target: &Option<String>) -> String {
    match target {
        Some(target) => format!("to {target}: use that URL as the endpoint"),
        None => "without saying where to".to_string(),
    }
}

pub fn request_body() -> Value {
    json!({ "query": INTROSPECTION_QUERY, "operationName": "IntrospectionQuery" })
}

pub fn parse_response(body: &str) -> Result<IntrospectionResponse, ClientError> {
    let value: Value = serde_json::from_str(body).map_err(ClientError::InvalidJson)?;
    let has_data = matches!(value.get("data"), Some(data) if !data.is_null());
    if !has_data {
        let messages = value
            .get("errors")
            .and_then(Value::as_array)
            .map(|errors| {
                errors
                    .iter()
                    .filter_map(|e| e.get("message").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .filter(|joined| !joined.is_empty());
        return Err(match messages {
            Some(messages) => ClientError::Graphql(messages),
            None => ClientError::MissingData,
        });
    }
    serde_json::from_value(value).map_err(ClientError::InvalidJson)
}

/// What came back from running an operation. Any HTTP status counts as a response; only
/// failing to get one at all is an error (see `execute`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub elapsed: Duration,
    /// The body as received, with every header value (if the server echoed one) masked.
    pub body: String,
}

/// The POST body that runs an operation.
pub fn operation_body(operation_name: &str, query: &str, variables: &Map<String, Value>) -> Value {
    json!({ "query": query, "variables": variables, "operationName": operation_name })
}

/// An answer as it came back.
struct Raw {
    status: u16,
    text: String,
    elapsed: Duration,
    /// The `Location` header, for redirects.
    location: Option<String>,
}

/// POSTs `body` as JSON with `headers`. Redirects are not followed: see
/// `ClientError::Redirected`.
fn post(uri: &str, headers: &[Header], body: &Value) -> Result<Raw, reqwest::Error> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let mut request = client.post(uri).json(body);
    for header in headers {
        request = request.header(header.name(), header.value());
    }
    let started = Instant::now();
    let response = request.send()?;
    let status = response.status().as_u16();
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let text = response.text()?;
    Ok(Raw {
        status,
        text,
        elapsed: started.elapsed(),
        location,
    })
}

/// A redirect is an error that says where the server wanted to go.
fn check_redirect(uri: &str, raw: &Raw) -> Result<(), ClientError> {
    if matches!(raw.status, 301 | 302 | 303 | 307 | 308) {
        let target = raw
            .location
            .as_deref()
            .and_then(|location| redirect_target(uri, location));
        return Err(ClientError::Redirected {
            status: raw.status,
            target,
        });
    }
    Ok(())
}

/// The target as an absolute URL, resolved against the endpoint, without user info, query
/// or fragment: those may hold secrets, and the user only needs the address to switch to.
fn redirect_target(uri: &str, location: &str) -> Option<String> {
    let mut target = reqwest::Url::parse(uri).ok()?.join(location).ok()?;
    target.set_query(None);
    target.set_fragment(None);
    let _ = target.set_username("");
    let _ = target.set_password(None);
    Some(target.to_string())
}

/// Runs an operation. Any other HTTP status is an `Ok` response; redirects, connection
/// failures, timeouts and unreadable bodies are errors.
pub fn execute(uri: &str, headers: &[Header], body: &Value) -> Result<Response, ClientError> {
    let raw = post(uri, headers, body)?;
    check_redirect(uri, &raw)?;
    Ok(Response {
        status: raw.status,
        elapsed: raw.elapsed,
        body: headers::mask(&raw.text, headers),
    })
}

pub fn fetch_introspection(
    uri: &str,
    headers: &[Header],
) -> Result<IntrospectionResponse, ClientError> {
    let raw = post(uri, headers, &request_body())?;
    check_redirect(uri, &raw)?;
    if !(200..300).contains(&raw.status) {
        let body: String = headers::mask(&raw.text, headers)
            .chars()
            .take(300)
            .collect();
        return Err(ClientError::Status {
            status: raw.status,
            body,
        });
    }
    parse_response(&raw.text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::FIXTURE;

    #[test]
    fn operation_body_carries_query_variables_and_name() {
        let mut variables = serde_json::Map::new();
        variables.insert("id".to_string(), json!("1"));
        let body = operation_body("Me", "query Me { me { id } }", &variables);
        assert_eq!(body["operationName"], "Me");
        assert_eq!(body["query"], "query Me { me { id } }");
        assert_eq!(body["variables"]["id"], "1");
    }

    #[test]
    fn request_body_names_the_operation() {
        let body = request_body();
        assert_eq!(body["operationName"], "IntrospectionQuery");
        assert!(body["query"].as_str().unwrap().contains("__schema"));
    }

    #[test]
    fn parses_a_successful_response() {
        let response = parse_response(FIXTURE).unwrap();
        assert_eq!(response.data.schema.types.len(), 10);
    }

    #[test]
    fn graphql_errors_are_reported_with_their_messages() {
        let body = r#"{"errors":[{"message":"Introspection is disabled"},{"message":"second"}]}"#;
        let err = parse_response(body).unwrap_err();
        assert!(
            matches!(&err, ClientError::Graphql(m) if m == "Introspection is disabled; second")
        );
    }

    #[test]
    fn null_data_without_errors_is_missing_data() {
        assert!(matches!(
            parse_response(r#"{"data":null}"#),
            Err(ClientError::MissingData)
        ));
    }

    #[test]
    fn non_json_is_invalid_json() {
        assert!(matches!(
            parse_response("<html>"),
            Err(ClientError::InvalidJson(_))
        ));
    }
}
