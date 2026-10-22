mod cli;
mod output;
mod schema_source;
mod tui;

use anyhow::Context;
use clap::Parser;
use vega_core::config::{self, FileConfig};
use vega_core::{cache, describe, search, xdg};

use crate::cli::Cli;

fn env_var(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let file_config = match xdg::config_dir() {
        Some(dir) => config::load_file_config(&dir)?,
        None => FileConfig::default(),
    };
    let settings = config::resolve_settings(
        cli.uri.clone(),
        env_var("GRAPHQL_URI"),
        env_var("GRAPHQL_KEY"),
        file_config,
    )?;

    let cache_dir = xdg::cache_dir()
        .context("cannot locate the cache directory: set XDG_CACHE_HOME or HOME")?;
    let cache_file = cache::cache_path(&cache_dir, &settings.uri);
    let entry = schema_source::load_or_fetch(&settings, &cache_file, cli.refresh)?;
    let age = entry.age_secs(cache::now_secs());
    let schema = &entry.response.data.schema;

    if cli.search.is_some() || cli.describe.is_some() {
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
        }
        return Ok(());
    }

    anyhow::bail!("the interactive browser is not wired up yet; use --search or --describe")
}
