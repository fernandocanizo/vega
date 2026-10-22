use clap::Parser;

/// Explore a GraphQL server's schema from the terminal.
///
/// Without --search or --describe, opens the interactive browser.
/// The API key is read from the GRAPHQL_KEY environment variable.
#[derive(Debug, Parser)]
#[command(name = "vega", version)]
pub struct Cli {
    /// GraphQL endpoint (overrides GRAPHQL_URI and config.toml)
    #[arg(long)]
    pub uri: Option<String>,

    /// Refetch the schema from the server instead of using the cache
    #[arg(long)]
    pub refresh: bool,

    /// Print operations and types whose name contains TEXT (use "" for all)
    #[arg(long, value_name = "TEXT", conflicts_with = "describe")]
    pub search: Option<String>,

    /// Print the definition of TYPE or TYPE.FIELD, e.g. Query.userRead
    #[arg(long, value_name = "TARGET")]
    pub describe: Option<String>,

    /// Print JSON on stdout for --search and --describe
    #[arg(long)]
    pub json: bool,
}
