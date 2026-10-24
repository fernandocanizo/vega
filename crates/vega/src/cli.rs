use clap::Parser;

/// Explore a GraphQL server's schema from the terminal.
///
/// Without --search, --describe or --query, opens the interactive browser.
///
/// Credentials never go on the command line, because arguments are visible to other users
/// of the machine. They come from the environment or, in the interactive UI, from the
/// connection modal (Ctrl-K). VEGA_GRAPHQL_KEY holds an API key, sent in the
/// X-API-Key header (or the one named by --key-header or VEGA_GRAPHQL_KEY_HEADER).
/// VEGA_GRAPHQL_HEADERS holds any other headers, one `Name: value` per line, and wins on
/// the same name. See docs/authentication.md.
#[derive(Debug, Parser)]
#[command(name = "vega", version)]
pub struct Cli {
    /// GraphQL endpoint (overrides VEGA_GRAPHQL_URI and config.toml)
    #[arg(long)]
    pub uri: Option<String>,

    /// Header name that carries VEGA_GRAPHQL_KEY (default X-API-Key; overrides
    /// VEGA_GRAPHQL_KEY_HEADER)
    #[arg(long, value_name = "NAME")]
    pub key_header: Option<String>,

    /// Refetch the schema from the server instead of using the cache
    #[arg(long)]
    pub refresh: bool,

    /// Print operations and types whose name contains TEXT (use "" for all)
    #[arg(long, value_name = "TEXT", conflicts_with = "describe")]
    pub search: Option<String>,

    /// Print the definition of TYPE or TYPE.FIELD, e.g. Query.userRead
    #[arg(long, value_name = "TARGET")]
    pub describe: Option<String>,

    /// Print the query generated for OPERATION (e.g. Query.userRead); sends nothing unless
    /// --run is given
    #[arg(long, value_name = "OPERATION", conflicts_with_all = ["search", "describe"])]
    pub query: Option<String>,

    /// Fields to select with --query: comma-separated paths such as id,friends.name
    /// (union or interface branches as on:Type, optional arguments as arg:name,
    /// e.g. friends.arg:first)
    #[arg(long, value_name = "PATHS", requires = "query")]
    pub select: Option<String>,

    /// Set a variable of the --query operation, for example --var id=42 (repeatable)
    #[arg(long = "var", value_name = "NAME=VALUE", requires = "query")]
    pub vars: Vec<String>,

    /// Send the --query operation to the server and print the response
    #[arg(long, requires = "query")]
    pub run: bool,

    /// Run a mutation without asking first (required when there is no terminal)
    #[arg(long, requires = "run")]
    pub yes: bool,

    /// Print JSON on stdout for --search, --describe, --query and --run
    #[arg(long)]
    pub json: bool,
}
