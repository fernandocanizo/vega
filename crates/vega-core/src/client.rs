//! Fetches the schema through a standard GraphQL introspection query.

use std::time::Duration;

use serde_json::{json, Value};

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
    #[error("request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("server answered HTTP {status}: {body}")]
    Status { status: u16, body: String },
    #[error("response is not a valid introspection result: {0}")]
    InvalidJson(serde_json::Error),
    #[error("server returned GraphQL errors: {0}")]
    Graphql(String),
    #[error("response has no `data`")]
    MissingData,
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

pub fn fetch_introspection(
    uri: &str,
    api_key: Option<&str>,
) -> Result<IntrospectionResponse, ClientError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?;
    let mut request = client.post(uri).json(&request_body());
    if let Some(key) = api_key {
        request = request.header("X-API-Key", key);
    }
    let response = request.send()?;
    let status = response.status();
    let body = response.text()?;
    if !status.is_success() {
        return Err(ClientError::Status {
            status: status.as_u16(),
            body: body.chars().take(300).collect(),
        });
    }
    parse_response(&body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::FIXTURE;

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
