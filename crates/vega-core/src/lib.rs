//! Vega core: GraphQL introspection client, schema model, search and cache.
//! No UI dependencies live here.

pub mod cache;
pub mod client;
pub mod config;
pub mod describe;
pub mod export;
pub mod headers;
pub mod querygen;
pub mod response;
pub mod schema;
pub mod search;
pub mod selection;
pub mod variables;
pub mod xdg;

#[cfg(test)]
mod testutil;
