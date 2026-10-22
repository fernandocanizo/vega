//! Vega core: GraphQL introspection client, schema model, search and cache.
//! No UI dependencies live here.

pub mod schema;
pub mod search;

#[cfg(test)]
mod testutil;
