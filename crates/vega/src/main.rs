mod cli;
mod output;
mod schema_source;
mod tui;

use anyhow::Context;
use clap::Parser;
use vega_core::config::{self, FileConfig};
use vega_core::querygen::GeneratedQuery;
use vega_core::schema::Schema;
use vega_core::selection::{self, OperationRef, Selection};
use vega_core::variables::{self, State, Values};
use vega_core::{cache, describe, querygen, search, xdg};

use crate::cli::Cli;

fn env_var(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let env_uri = env_var("VEGA_GRAPHQL_URI");
    let has_endpoint = [cli.uri.as_deref(), env_uri.as_deref()]
        .into_iter()
        .flatten()
        .any(|uri| !uri.is_empty());
    let file_config = match xdg::config_dir() {
        Some(dir) if !has_endpoint => config::load_file_config(&dir)?,
        _ => FileConfig::default(),
    };
    let settings = config::resolve_settings(
        cli.uri.clone(),
        env_uri,
        env_var("VEGA_GRAPHQL_KEY"),
        file_config,
    )?;

    let cache_dir = xdg::cache_dir()
        .context("cannot locate the cache directory: set XDG_CACHE_HOME or HOME")?;
    let cache_file = cache::cache_path(&cache_dir, &settings.uri);
    let entry = schema_source::load_or_fetch(&settings, &cache_file, cli.refresh)?;
    let age = entry.age_secs(cache::now_secs());
    let schema = &entry.response.data.schema;

    if cli.search.is_some() || cli.describe.is_some() || cli.query.is_some() {
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
            print_query(schema, target, cli.select.as_deref(), &cli.vars, cli.json)?;
        }
        return Ok(());
    }

    tui::run(entry, settings, cache_file)
}

fn print_query(
    schema: &Schema,
    target: &str,
    select: Option<&str>,
    vars: &[String],
    json: bool,
) -> anyhow::Result<()> {
    let operation = OperationRef::parse(schema, target).with_context(|| {
        format!("operation `{target}` not found: use Query.<field>, Mutation.<field> or Subscription.<field>")
    })?;
    let selection = match select {
        Some(text) => selection::parse_select(schema, &operation, text)?,
        None => Selection::default(),
    };
    let query = querygen::generate(schema, &operation, &selection)?;
    let values = parse_vars(&query, vars)?;
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
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&output::query_json(&query, &states)?)?
        );
    } else {
        print!("{}", output::query_text(&query, &states));
    }
    Ok(())
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
