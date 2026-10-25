//! Terminal setup and the event loop. Everything testable lives in `app`, `connection`
//! and `ui`.

pub mod app;
pub mod clipboard;
pub mod connection;
pub mod profiles;
pub mod ui;

use std::{
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Duration,
};

use ratatui::{
    crossterm::{
        event::{self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyEventKind},
        execute,
    },
    DefaultTerminal,
};
use vega_core::cache::{self, CacheEntry};
use vega_core::config::Settings;
use vega_core::response::{self, ResponseView};
use vega_core::secrets::KeyringStore;
use vega_core::{client, headers, profile};

use crate::schema_source;
use app::{Action, App, RunRequest};

/// What background threads send back to the event loop.
enum Update {
    /// A schema fetch. `seq` ties it to the load that started it, so the answer for an
    /// endpoint the user has since left is dropped.
    Schema {
        seq: u64,
        cache_file: PathBuf,
        result: anyhow::Result<CacheEntry>,
    },
    Run {
        seq: u64,
        outcome: Result<ResponseView, String>,
    },
}

/// What the TUI shows about the profile it starts on.
#[derive(Default)]
pub struct Startup {
    pub profile_name: Option<String>,
    pub notice: Option<String>,
}

/// Runs the TUI. `entry` is the cached schema for the endpoint, if any; without one the
/// schema is fetched in the background (or, with no endpoint, the connection modal
/// opens).
pub fn run(
    entry: Option<CacheEntry>,
    settings: Settings,
    cache_dir: PathBuf,
    config_dir: Option<PathBuf>,
    startup: Startup,
) -> anyhow::Result<()> {
    let app = match entry {
        Some(entry) => App::new(entry.response.data.schema, &settings.uri, entry.fetched_at)
            .with_settings(settings),
        None => App::empty(settings),
    }
    .with_profile_name(startup.profile_name)
    .with_notice(startup.notice);
    let mut terminal = ratatui::init();
    let _ = execute!(std::io::stdout(), EnableBracketedPaste);
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(std::io::stdout(), DisableBracketedPaste);
        previous_hook(info);
    }));
    let result = event_loop(&mut terminal, app, &cache_dir, config_dir.as_deref());
    let _ = execute!(std::io::stdout(), DisableBracketedPaste);
    ratatui::restore();
    result
}

fn event_loop(
    terminal: &mut DefaultTerminal,
    mut app: App,
    cache_dir: &Path,
    config_dir: Option<&Path>,
) -> anyhow::Result<()> {
    let (tx, rx) = mpsc::channel::<Update>();
    let mut ctx = Ctx {
        cache_dir,
        config_dir,
        load_seq: 0,
        tx,
    };

    if !app.is_loaded() && !app.settings().uri.is_empty() {
        app.begin_load();
        ctx.load_seq += 1;
        spawn_fetch(&app, cache_dir, ctx.load_seq, &ctx.tx);
    }

    while !app.should_quit {
        terminal.draw(|frame| ui::draw(frame, &app, cache::now_secs()))?;

        while let Ok(update) = rx.try_recv() {
            match update {
                Update::Schema {
                    seq,
                    cache_file,
                    result,
                } => {
                    if seq == ctx.load_seq {
                        apply_fetch(&mut app, &cache_file, result);
                    }
                }
                Update::Run { seq, outcome } => app.apply_run_result(seq, outcome),
            }
        }

        // Short poll so the status bar age keeps ticking and results show up.
        if event::poll(Duration::from_millis(200))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    let mut action = app.handle_key(key);
                    while let Some(next) = dispatch(&mut app, action, &mut ctx) {
                        action = next;
                    }
                }
                Event::Paste(text) => app.handle_paste(&text),
                _ => {}
            }
        }
    }
    Ok(())
}

/// What `dispatch` needs besides the app: directories, the load counter and the channel
/// background threads answer on.
struct Ctx<'a> {
    cache_dir: &'a Path,
    config_dir: Option<&'a Path>,
    load_seq: u64,
    tx: mpsc::Sender<Update>,
}

/// Performs one action. An action whose outcome the app must hear about returns the
/// follow-up action the app asks for.
fn dispatch(app: &mut App, action: Action, ctx: &mut Ctx) -> Option<Action> {
    match action {
        Action::None => {}
        Action::Refresh => {
            ctx.load_seq += 1;
            spawn_fetch(app, ctx.cache_dir, ctx.load_seq, &ctx.tx);
        }
        Action::Load => {
            ctx.load_seq += 1;
            let cache_file = cache::cache_path(ctx.cache_dir, &app.settings().uri);
            match cache::load(&cache_file) {
                Ok(Some(entry)) => {
                    app.apply_refresh_ok(entry.response.data.schema, entry.fetched_at)
                }
                _ => spawn_fetch(app, ctx.cache_dir, ctx.load_seq, &ctx.tx),
            }
        }
        Action::Copy(text) => clipboard::copy(&text),
        Action::Run(request) => {
            let settings = app.settings().clone();
            let tx = ctx.tx.clone();
            thread::spawn(move || {
                let outcome = run_request(&settings, &request);
                let _ = tx.send(Update::Run {
                    seq: request.seq,
                    outcome,
                });
            });
        }
        Action::SaveProfile { name, settings } => {
            let result = match ctx.config_dir {
                Some(dir) => {
                    profile::save(dir, &KeyringStore, &name, &settings).map_err(|e| e.to_string())
                }
                None => Err(NO_CONFIG_DIR.to_string()),
            };
            match result {
                Ok(()) => return Some(app.profile_saved(name, settings)),
                Err(message) => app.profile_save_failed(message),
            }
        }
        Action::OpenPicker => match read_profiles(ctx.config_dir) {
            Ok(file) => app.open_picker(entries_of(&file)),
            Err(message) => app.profile_notice(&message),
        },
        Action::ConnectProfile(name) => match connect_profile(ctx.config_dir, &name) {
            Ok(connected) => {
                notify_missing_credentials(app, &connected, &name);
                return Some(app.connect_profile(name, connected.settings));
            }
            Err(message) => app.profile_notice(&message),
        },
        Action::EditProfile(name) => match connect_profile(ctx.config_dir, &name) {
            Ok(connected) => {
                notify_missing_credentials(app, &connected, &name);
                app.edit_profile(name, connected.settings);
            }
            Err(message) => app.profile_notice(&message),
        },
        Action::DeleteProfile(name) => {
            let deleted = match ctx.config_dir {
                Some(dir) => profile::delete(dir, &KeyringStore, &name).map_err(|e| e.to_string()),
                None => Err(NO_CONFIG_DIR.to_string()),
            };
            match deleted {
                Ok(()) => {
                    app.profile_deleted(&name);
                    match read_profiles(ctx.config_dir) {
                        Ok(file) => app.open_picker(entries_of(&file)),
                        Err(message) => app.profile_notice(&message),
                    }
                }
                Err(message) => app.profile_notice(&message),
            }
        }
    }
    None
}

fn notify_missing_credentials(app: &mut App, connected: &profile::Connected, name: &str) {
    if connected.missing_credentials {
        app.profile_notice(&format!("no stored credentials found for profile {name}"));
    }
}

const NO_CONFIG_DIR: &str = "cannot locate the config directory: set XDG_CONFIG_HOME or HOME";

fn read_profiles(config_dir: Option<&Path>) -> Result<vega_core::config::FileConfig, String> {
    match config_dir {
        Some(dir) => vega_core::config::load_file_config(dir).map_err(|e| e.to_string()),
        None => Err(NO_CONFIG_DIR.to_string()),
    }
}

fn entries_of(file: &vega_core::config::FileConfig) -> Vec<profiles::Entry> {
    file.profiles
        .iter()
        .map(|(name, profile)| profiles::Entry {
            name: name.clone(),
            uri: profile.uri.clone(),
            kind: profile.kind,
        })
        .collect()
}

fn connect_profile(config_dir: Option<&Path>, name: &str) -> Result<profile::Connected, String> {
    let file = read_profiles(config_dir)?;
    profile::connect(&file, &KeyringStore, name).map_err(|e| e.to_string())
}

/// Fetches the schema of the app's current endpoint on a background thread.
fn spawn_fetch(app: &App, cache_dir: &Path, seq: u64, tx: &mpsc::Sender<Update>) {
    let settings = app.settings().clone();
    let cache_file = cache::cache_path(cache_dir, &settings.uri);
    let tx = tx.clone();
    thread::spawn(move || {
        let result = schema_source::fetch_entry(&settings)
            .map_err(|e| anyhow::anyhow!(headers::mask(&format!("{e:#}"), &settings.headers)));
        let _ = tx.send(Update::Schema {
            seq,
            cache_file,
            result,
        });
    });
}

/// Sends the request and summarizes the answer. A failure to get any HTTP answer comes
/// back as text for the Response pane, with every header value masked.
fn run_request(settings: &Settings, request: &RunRequest) -> Result<ResponseView, String> {
    client::execute(&settings.uri, &settings.headers, &request.body)
        .map(|response| response::view(&response))
        .map_err(|error| headers::mask(&error_chain(&error), &settings.headers))
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
        Err(e) => {
            let message = headers::mask(&format!("{e:#}"), &app.settings().headers);
            app.apply_refresh_err(message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use vega_core::headers::Header;

    fn settings(uri: &str, key: Option<&str>) -> Settings {
        Settings {
            uri: uri.to_string(),
            headers: key
                .map(|key| vec![Header::new("X-API-Key", key).unwrap()])
                .unwrap_or_default(),
            key_only: false,
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

    #[test]
    fn a_redirected_fetch_tells_the_user_where_to_point_vega() {
        let mut app = App::empty(settings("https://old.example.com/graphql", None));
        let redirect = client::ClientError::Redirected {
            status: 301,
            target: Some("https://new.example.com/graphql".to_string()),
        };
        let error = anyhow::Error::new(redirect)
            .context("fetching the schema from https://old.example.com/graphql");
        apply_fetch(&mut app, Path::new("/nonexistent/cache.json"), Err(error));
        let (text, _) = app.status(0);
        assert!(
            text.contains(
                "server redirected the request (HTTP 301) to https://new.example.com/graphql"
            ),
            "{text}"
        );
    }

    #[test]
    fn a_failed_fetch_shows_a_masked_error_in_the_status_bar() {
        let settings = settings("http://127.0.0.1:9/graphql", Some("secret-key"));
        let mut app = App::empty(settings);
        apply_fetch(
            &mut app,
            Path::new("/nonexistent/cache.json"),
            Err(anyhow::anyhow!("server answered HTTP 401: bad secret-key")),
        );
        let (text, _) = app.status(0);
        assert!(text.contains("could not load the schema"), "{text}");
        assert!(!text.contains("secret-key"), "{text}");
    }
}
