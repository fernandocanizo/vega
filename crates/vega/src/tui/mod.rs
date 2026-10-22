//! Terminal setup and the event loop. Everything testable lives in `app` and `ui`.

pub mod app;
pub mod ui;

use std::{
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Duration,
};

use ratatui::{
    crossterm::event::{self, Event, KeyEventKind},
    DefaultTerminal,
};
use vega_core::cache::{self, CacheEntry};
use vega_core::config::Settings;

use crate::schema_source;
use app::{Action, App};

pub fn run(entry: CacheEntry, settings: Settings, cache_file: PathBuf) -> anyhow::Result<()> {
    let app = App::new(entry.response.data.schema, &settings.uri, entry.fetched_at);
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, app, &settings, &cache_file);
    ratatui::restore();
    result
}

fn event_loop(
    terminal: &mut DefaultTerminal,
    mut app: App,
    settings: &Settings,
    cache_file: &Path,
) -> anyhow::Result<()> {
    let (tx, rx) = mpsc::channel::<anyhow::Result<CacheEntry>>();

    while !app.should_quit {
        terminal.draw(|frame| ui::draw(frame, &app, cache::now_secs()))?;

        while let Ok(result) = rx.try_recv() {
            match persist(cache_file, result) {
                Ok(entry) => app.apply_refresh_ok(entry.response.data.schema, entry.fetched_at),
                Err(e) => app.apply_refresh_err(format!("{e:#}")),
            }
        }

        // Short poll so the status bar age keeps ticking and refresh results show up.
        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press && app.handle_key(key) == Action::Refresh {
                    let settings = settings.clone();
                    let tx = tx.clone();
                    thread::spawn(move || {
                        let _ = tx.send(schema_source::fetch_entry(&settings));
                    });
                }
            }
        }
    }
    Ok(())
}

/// Writes a freshly fetched schema to the cache. A failed fetch leaves the cache alone.
fn persist(cache_file: &Path, result: anyhow::Result<CacheEntry>) -> anyhow::Result<CacheEntry> {
    let entry = result?;
    cache::store(cache_file, &entry)?;
    Ok(entry)
}
