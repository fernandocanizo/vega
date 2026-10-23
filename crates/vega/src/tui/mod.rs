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
use vega_core::client;
use vega_core::config::Settings;
use vega_core::response::{self, ResponseView};

use crate::schema_source;
use app::{Action, App, RunRequest};

/// What background threads send back to the event loop.
enum Update {
    Schema(anyhow::Result<CacheEntry>),
    Run {
        seq: u64,
        outcome: Result<ResponseView, String>,
    },
}

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
    let (tx, rx) = mpsc::channel::<Update>();

    while !app.should_quit {
        terminal.draw(|frame| ui::draw(frame, &app, cache::now_secs()))?;

        while let Ok(update) = rx.try_recv() {
            match update {
                Update::Schema(result) => apply_fetch(&mut app, cache_file, result),
                Update::Run { seq, outcome } => app.apply_run_result(seq, outcome),
            }
        }

        // Short poll so the status bar age keeps ticking and results show up.
        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match app.handle_key(key) {
                        Action::None => {}
                        Action::Refresh => {
                            let settings = settings.clone();
                            let tx = tx.clone();
                            thread::spawn(move || {
                                let _ =
                                    tx.send(Update::Schema(schema_source::fetch_entry(&settings)));
                            });
                        }
                        Action::Run(request) => {
                            let settings = settings.clone();
                            let tx = tx.clone();
                            thread::spawn(move || {
                                let outcome = run_request(&settings, &request);
                                let _ = tx.send(Update::Run {
                                    seq: request.seq,
                                    outcome,
                                });
                            });
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Sends the request and summarizes the answer. A failure to get any HTTP answer comes
/// back as text for the Response pane, with the API key masked.
fn run_request(settings: &Settings, request: &RunRequest) -> Result<ResponseView, String> {
    client::execute(&settings.uri, settings.api_key.as_deref(), &request.body)
        .map(|response| response::view(&response))
        .map_err(|error| {
            let message = error_chain(&error);
            match settings.api_key.as_deref().filter(|key| !key.is_empty()) {
                Some(key) => message.replace(key, "***"),
                None => message,
            }
        })
}

/// An error and its sources on one line: `request failed: error sending request: ...`.
fn error_chain(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

/// Applies a fetch result. A failed fetch leaves the cache alone; a fetched schema
/// is used even when it cannot be written to the cache.
fn apply_fetch(app: &mut App, cache_file: &Path, result: anyhow::Result<CacheEntry>) {
    match result {
        Ok(entry) => {
            let stored = cache::store(cache_file, &entry);
            let (schema, fetched_at) = (entry.response.data.schema, entry.fetched_at);
            match stored {
                Ok(()) => app.apply_refresh_ok(schema, fetched_at),
                Err(e) => app.apply_refresh_unsaved(schema, fetched_at, e.to_string()),
            }
        }
        Err(e) => app.apply_refresh_err(format!("{e:#}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn settings(uri: &str, key: Option<&str>) -> Settings {
        Settings {
            uri: uri.to_string(),
            api_key: key.map(str::to_string),
        }
    }

    fn request() -> RunRequest {
        RunRequest {
            seq: 7,
            body: serde_json::json!({"query": "query Me { me { id } }", "operationName": "Me", "variables": {}}),
        }
    }

    /// Answers one request with `body` and returns the endpoint URL.
    fn serve_once(body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let uri = format!("http://{}/graphql", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 8192];
            let _ = stream.read(&mut buf).unwrap();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
        });
        uri
    }

    #[test]
    fn run_request_returns_the_view_of_the_response() {
        let uri = serve_once(r#"{"data":{"me":{"id":"1"}}}"#);
        let view = run_request(&settings(&uri, None), &request()).unwrap();
        assert_eq!(view.status, 200);
        assert!(view.ok());
        assert!(view.lines.iter().any(|line| line.contains("\"id\": \"1\"")));
    }

    #[test]
    fn run_request_reports_connection_failures_as_text() {
        let message = run_request(
            &settings("http://127.0.0.1:9/graphql", Some("secret")),
            &request(),
        )
        .unwrap_err();
        assert!(message.starts_with("request failed"), "{message}");
        assert!(!message.contains("secret"));
    }

    #[test]
    fn error_chain_joins_the_sources() {
        use std::fmt;

        #[derive(Debug)]
        struct Inner;
        impl fmt::Display for Inner {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "inner")
            }
        }
        impl std::error::Error for Inner {}

        #[derive(Debug)]
        struct Outer(Inner);
        impl fmt::Display for Outer {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "outer")
            }
        }
        impl std::error::Error for Outer {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }

        assert_eq!(error_chain(&Outer(Inner)), "outer: inner");
    }
}
