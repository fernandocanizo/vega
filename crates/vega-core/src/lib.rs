//! Vega core: GraphQL introspection client, schema model, search and cache.
//! No UI dependencies live here.

pub mod cache;
pub mod describe;
pub mod schema;
pub mod search;
pub mod xdg;

#[cfg(test)]
mod testutil;
