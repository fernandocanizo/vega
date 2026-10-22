use crate::schema::{IntrospectionResponse, Schema};

pub const FIXTURE: &str = include_str!("../tests/fixtures/introspection.json");

pub fn fixture_response() -> IntrospectionResponse {
    serde_json::from_str(FIXTURE).expect("fixture parses")
}

pub fn fixture_schema() -> Schema {
    fixture_response().data.schema
}
