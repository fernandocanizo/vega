mod cli;
mod output;
mod schema_source;
mod tui;

use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use vega_core::config::{self, AuthInput, FileConfig, Settings};
use vega_core::querygen::GeneratedQuery;
use vega_core::schema::{Root, Schema};
use vega_core::secrets::KeyringStore;
use vega_core::selection::{self, OperationRef, Selection};
use vega_core::variables::{self, State, Values};
use vega_core::{cache, client, describe, profile, querygen, response, search, xdg};

use crate::cli::Cli;

fn env_var(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let one_shot = cli.search.is_some() || cli.describe.is_some() || cli.query.is_some();

    let env_uri = env_var("VEGA_GRAPHQL_URI");
    let profile_name = cli
        .profile
        .clone()
        .or_else(|| env_var("VEGA_PROFILE"))
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty());
    let has_endpoint = [cli.uri.as_deref(), env_uri.as_deref()]
        .into_iter()
        .flatten()
        .any(|uri| !uri.is_empty());
    let mut file_config = match xdg::config_dir() {
        Some(dir) if profile_name.is_some() || !has_endpoint => config::load_file_config(&dir)?,
        _ => FileConfig::default(),
    };
    let auth = AuthInput {
        key: env_var("VEGA_GRAPHQL_KEY"),
        key_header: cli
            .key_header
            .clone()
            .or_else(|| env_var("VEGA_GRAPHQL_KEY_HEADER")),
        headers: env_var("VEGA_GRAPHQL_HEADERS"),
    };
    let env_credentials = auth.key.as_deref().is_some_and(|k| !k.is_empty())
        || auth
            .headers
            .as_deref()
            .is_some_and(|h| !h.trim().is_empty());
    let connected = match &profile_name {
        Some(name) => {
            let connected =
                profile::connect(&file_config, &KeyringStore, name).with_context(|| {
                    format!(
                        "cannot use profile `{name}` (VEGA_GRAPHQL_URI and VEGA_GRAPHQL_KEY work without the keyring)"
                    )
                })?;
            if connected.missing_credentials && !env_credentials {
                eprintln!("warning: no stored credentials found for profile `{name}`");
            }
            // The profile sits between the environment and the top-level `uri`.
            file_config.uri = None;
            Some(connected)
        }
        None => None,
    };
    // The terminal UI can start with no endpoint (it opens the connection modal); the
    // one-shot commands cannot.
    let mut settings =
        config::resolve_settings_partial(cli.uri.clone(), env_uri, auth, file_config)?;
    if let Some(connected) = connected {
        settings = config::with_profile(settings, connected.settings, env_credentials);
    }
    if one_shot && settings.uri.is_empty() {
        return Err(config::ConfigError::MissingUri.into());
    }

    let cache_dir = xdg::cache_dir()
        .context("cannot locate the cache directory: set XDG_CACHE_HOME or HOME")?;
    if !one_shot {
        return start_tui(&cli, settings, cache_dir);
    }
    let cache_file = cache::cache_path(&cache_dir, &settings.uri);
    let entry = schema_source::load_or_fetch(&settings, &cache_file, cli.refresh)?;
    let age = entry.age_secs(cache::now_secs());
    let schema = &entry.response.data.schema;

    if cache::is_stale(age) {
        eprintln!(
            "warning: the schema cache for {} was fetched {} ago; run with --refresh to update it",
            settings.uri,
            cache::format_age(age)
        );
    }
    if let Some(text) = &cli.search {
        let hits = search::search(schema, text);
        if cli.json {
            println!("{}", serde_json::to_string_pretty(&hits)?);
        } else {
            print!("{}", output::hits_text(&hits));
        }
    } else if let Some(target) = &cli.describe {
        let detail = describe::detail(schema, target)?;
        if cli.json {
            println!("{}", serde_json::to_string_pretty(&detail)?);
        } else {
            print!("{}", describe::render_text(&detail));
        }
    } else if let Some(target) = &cli.query {
        let args = QueryArgs {
            target,
            select: cli.select.as_deref(),
            vars: &cli.vars,
            json: cli.json,
            run: cli.run,
            yes: cli.yes,
        };
        print_query(schema, &settings, &args)?;
    }
    Ok(())
}

/// Starts the terminal UI. The schema is loaded inside it, in the background, so a
/// missing endpoint or a failing server never stops it from opening.
fn start_tui(cli: &Cli, settings: Settings, cache_dir: PathBuf) -> anyhow::Result<()> {
    let entry = if settings.uri.is_empty() || cli.refresh {
        None
    } else {
        match cache::load(&cache::cache_path(&cache_dir, &settings.uri)) {
            Ok(entry) => entry,
            Err(e) => {
                eprintln!("warning: ignoring unusable cache ({e}); fetching a fresh schema");
                None
            }
        }
    };
    tui::run(entry, settings, cache_dir, xdg::config_dir())
}

struct QueryArgs<'a> {
    target: &'a str,
    select: Option<&'a str>,
    vars: &'a [String],
    json: bool,
    run: bool,
    yes: bool,
}

fn print_query(schema: &Schema, settings: &Settings, args: &QueryArgs) -> anyhow::Result<()> {
    let target = args.target;
    let operation = OperationRef::parse(schema, target).with_context(|| {
        format!("operation `{target}` not found: use Query.<field>, Mutation.<field> or Subscription.<field>")
    })?;
    let selection = match args.select {
        Some(text) => selection::parse_select(schema, &operation, text)?,
        None => Selection::default(),
    };
    let query = querygen::generate(schema, &operation, &selection)?;
    let values = parse_vars(&query, args.vars)?;
    let mut states = Vec::new();
    for def in &query.variables {
        let typed = values.get(&def.path).map(String::as_str);
        let state = variables::state(schema, def, typed);
        // A missing required value is only listed (running is what needs it).
        if let (State::Invalid(reason), Some(_)) = (&state, typed) {
            anyhow::bail!("variable ${}: {reason}", def.name);
        }
        states.push(state);
    }
    if args.run {
        return run_query(schema, settings, &operation, &query, &values, args);
    }
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&output::query_json(&query, &states)?)?
        );
    } else {
        print!("{}", output::query_text(&query, &states));
    }
    Ok(())
}

/// Sends the operation and prints the response. Everything that can be checked locally is
/// checked before anything is sent.
fn run_query(
    schema: &Schema,
    settings: &Settings,
    operation: &OperationRef,
    query: &GeneratedQuery,
    values: &Values,
    args: &QueryArgs,
) -> anyhow::Result<()> {
    if operation.root == Root::Subscription {
        anyhow::bail!("subscriptions cannot be run (they need WebSockets)");
    }
    let variables = variables::to_json(schema, &query.variables, values)
        .map_err(|problem| anyhow::anyhow!("cannot run: variable {problem}"))?;
    if operation.root == Root::Mutation && !args.yes {
        confirm_mutation(&query.operation_name)?;
    }
    let body = client::operation_body(&query.operation_name, &query.text, &variables);
    let response = client::execute(&settings.uri, &settings.headers, &body)
        .with_context(|| format!("running {} on {}", query.operation_name, settings.uri))?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&output::run_json(query, &response))?
        );
    } else {
        println!("{}", response::pretty_body(&response.body));
    }
    let view = response::view(&response);
    if !view.ok() {
        anyhow::bail!("{}", output::run_failure(&view));
    }
    Ok(())
}

/// Asks on the terminal; without one, a mutation needs `--yes`.
fn confirm_mutation(name: &str) -> anyhow::Result<()> {
    use std::io::{IsTerminal, Write};
    if !std::io::stdin().is_terminal() {
        anyhow::bail!("refusing to run mutation {name} without a terminal: pass --yes to confirm");
    }
    eprint!("Run mutation {name}? y/N ");
    std::io::stderr().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    if answer.trim().eq_ignore_ascii_case("y") {
        Ok(())
    } else {
        anyhow::bail!("cancelled")
    }
}

/// Matches each `NAME=VALUE` with the variable of that name in the generated query.
fn parse_vars(query: &GeneratedQuery, vars: &[String]) -> anyhow::Result<Values> {
    let mut values = Values::new();
    for item in vars {
        let (name, value) = item
            .split_once('=')
            .with_context(|| format!("--var `{item}`: expected NAME=VALUE"))?;
        let name = name.trim_start_matches('$');
        let def = query
            .variables
            .iter()
            .find(|variable| variable.name == name)
            .with_context(|| {
                let known: Vec<&str> = query.variables.iter().map(|v| v.name.as_str()).collect();
                format!(
                    "--var `{name}`: no such variable in this query (variables: {})",
                    if known.is_empty() {
                        "none".to_string()
                    } else {
                        known.join(", ")
                    }
                )
            })?;
        values.insert(def.path.clone(), value.to_string());
    }
    Ok(values)
}
