use clap::Parser;

/// Explore a GraphQL server's schema from the terminal.
///
/// Without --search, --describe or --query, opens the interactive browser.
/// The API key is read from the VEGA_GRAPHQL_KEY environment variable.
#[derive(Debug, Parser)]
#[command(name = "vega", version)]
pub struct Cli {
    /// GraphQL endpoint (overrides VEGA_GRAPHQL_URI and config.toml)
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

    /// Print the query generated for OPERATION (e.g. Query.userRead); sends nothing
    #[arg(long, value_name = "OPERATION", conflicts_with_all = ["search", "describe"])]
    pub query: Option<String>,

    /// Fields to select with --query: comma-separated paths such as id,friends.name
    /// (union or interface branches as on:Type, e.g. on:User.name)
    #[arg(long, value_name = "PATHS", requires = "query")]
    pub select: Option<String>,

    /// Print JSON on stdout for --search, --describe and --query
    #[arg(long)]
    pub json: bool,
}
