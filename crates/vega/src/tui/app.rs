//! Pure application state for the TUI: keys in, state out. No terminal access.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use vega_core::cache::{format_age, is_stale};
use vega_core::describe::{self, Detail};
use vega_core::schema::Schema;
use vega_core::search::{self, Hit};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    /// The event loop should start a background schema refresh.
    Refresh,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshState {
    Idle,
    Running,
    Failed(String),
    /// The new schema is applied but could not be saved to the cache.
    CacheWriteFailed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusLevel {
    Fresh,
    Stale,
}

#[derive(Debug)]
pub struct DetailView {
    pub detail: Detail,
    pub selected: usize,
}

pub struct App {
    schema: Schema,
    host: String,
    fetched_at: u64,
    pub query: String,
    pub hits: Vec<Hit>,
    pub hit_index: usize,
    pub detail: Option<DetailView>,
    back_stack: Vec<DetailView>,
    pub refresh: RefreshState,
    pub should_quit: bool,
}

/// `https://nexus.example.com/graphql` becomes `nexus.example.com`.
fn host_of(uri: &str) -> String {
    let rest = uri.split_once("://").map_or(uri, |(_, rest)| rest);
    let authority = rest.split('/').next().unwrap_or(rest);
    authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host)
        .to_string()
}

impl App {
    pub fn new(schema: Schema, uri: &str, fetched_at: u64) -> Self {
        let hits = search::search(&schema, "");
        Self {
            schema,
            host: host_of(uri),
            fetched_at,
            query: String::new(),
            hits,
            hit_index: 0,
            detail: None,
            back_stack: Vec::new(),
            refresh: RefreshState::Idle,
            should_quit: false,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('c') => {
                    self.should_quit = true;
                    Action::None
                }
                KeyCode::Char('r') => self.request_refresh(),
                _ => Action::None,
            };
        }
        if self.detail.is_some() {
            self.handle_detail_key(key.code);
        } else {
            self.handle_search_key(key.code);
        }
        Action::None
    }

    fn request_refresh(&mut self) -> Action {
        if self.refresh == RefreshState::Running {
            return Action::None;
        }
        self.refresh = RefreshState::Running;
        Action::Refresh
    }

    fn handle_search_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => self.should_quit = true,
            KeyCode::Char(c) => {
                self.query.push(c);
                self.rerun_search();
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.rerun_search();
            }
            KeyCode::Down => {
                self.hit_index = (self.hit_index + 1).min(self.hits.len().saturating_sub(1));
            }
            KeyCode::Up => self.hit_index = self.hit_index.saturating_sub(1),
            KeyCode::Enter | KeyCode::Right => self.open_highlighted_hit(),
            _ => {}
        }
    }

    fn handle_detail_key(&mut self, code: KeyCode) {
        let Some(view) = self.detail.as_mut() else {
            return;
        };
        match code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Esc | KeyCode::Backspace | KeyCode::Left => self.go_back(),
            KeyCode::Down => {
                view.selected = (view.selected + 1).min(view.detail.nav.len().saturating_sub(1));
            }
            KeyCode::Up => view.selected = view.selected.saturating_sub(1),
            KeyCode::Enter | KeyCode::Right => self.follow_selected(),
            _ => {}
        }
    }

    fn rerun_search(&mut self) {
        self.hits = search::search(&self.schema, &self.query);
        self.hit_index = 0;
    }

    fn open_highlighted_hit(&mut self) {
        let Some(hit) = self.hits.get(self.hit_index) else {
            return;
        };
        if let Ok(detail) = describe::detail(&self.schema, &hit.path) {
            self.detail = Some(DetailView {
                detail,
                selected: 0,
            });
        }
    }

    fn follow_selected(&mut self) {
        let Some(view) = &self.detail else {
            return;
        };
        let Some(target) = view
            .detail
            .nav
            .get(view.selected)
            .and_then(|item| item.target.clone())
        else {
            return;
        };
        let Ok(next) = describe::detail(&self.schema, &target) else {
            return;
        };
        let next = DetailView {
            detail: next,
            selected: 0,
        };
        if let Some(current) = self.detail.replace(next) {
            self.back_stack.push(current);
        }
    }

    /// Pops the history; with nothing left, returns to the search list.
    fn go_back(&mut self) {
        self.detail = self.back_stack.pop();
    }

    /// Detail of the highlighted search hit, shown while no detail is open.
    pub fn preview(&self) -> Option<Detail> {
        let hit = self.hits.get(self.hit_index)?;
        describe::detail(&self.schema, &hit.path).ok()
    }

    pub fn apply_refresh_ok(&mut self, schema: Schema, fetched_at: u64) {
        self.schema = schema;
        self.fetched_at = fetched_at;
        self.detail = None;
        self.back_stack.clear();
        self.refresh = RefreshState::Idle;
        self.rerun_search();
    }

    /// The fetch succeeded but the cache write failed: use the new schema anyway
    /// and surface the write error in the status bar.
    pub fn apply_refresh_unsaved(&mut self, schema: Schema, fetched_at: u64, message: String) {
        self.apply_refresh_ok(schema, fetched_at);
        self.refresh = RefreshState::CacheWriteFailed(message);
    }

    pub fn apply_refresh_err(&mut self, message: String) {
        self.refresh = RefreshState::Failed(message);
    }

    /// Status bar text and its staleness level at time `now` (unix seconds).
    pub fn status(&self, now: u64) -> (String, StatusLevel) {
        let age = now.saturating_sub(self.fetched_at);
        let level = if is_stale(age) {
            StatusLevel::Stale
        } else {
            StatusLevel::Fresh
        };
        let mut text = format!("{} · schema fetched {} ago", self.host, format_age(age));
        match &self.refresh {
            RefreshState::Idle => text.push_str(" · Ctrl-R refresh"),
            RefreshState::Running => text.push_str(" · refreshing…"),
            RefreshState::CacheWriteFailed(message) => {
                let first_line = message.lines().next().unwrap_or("");
                text.push_str(&format!(
                    " · schema updated but the cache could not be written: {first_line} · Ctrl-R refresh"
                ));
            }
            RefreshState::Failed(message) => {
                let first_line = message.lines().next().unwrap_or("");
                text.push_str(&format!(" · refresh failed: {first_line} · Ctrl-R retry"));
            }
        }
        (text, level)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use vega_core::schema::IntrospectionResponse;

    const FIXTURE: &str = include_str!("../../../vega-core/tests/fixtures/introspection.json");

    fn app() -> App {
        let response: IntrospectionResponse = serde_json::from_str(FIXTURE).unwrap();
        App::new(
            response.data.schema,
            "https://nexus.example.com/graphql",
            1_000,
        )
    }

    fn press(app: &mut App, code: KeyCode) -> Action {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn ctrl(app: &mut App, c: char) -> Action {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
    }

    fn type_text(app: &mut App, text: &str) {
        for c in text.chars() {
            press(app, KeyCode::Char(c));
        }
    }

    fn title(app: &App) -> Option<&str> {
        app.detail.as_ref().map(|v| v.detail.title.as_str())
    }

    #[test]
    fn starts_with_every_hit_listed() {
        assert_eq!(app().hits.len(), 10);
    }

    #[test]
    fn typing_filters_and_backspace_widens() {
        let mut app = app();
        type_text(&mut app, "usersearch");
        assert_eq!(app.query, "usersearch");
        assert_eq!(app.hits.len(), 1);
        assert_eq!(app.hits[0].path, "Query.userSearch");
        press(&mut app, KeyCode::Backspace);
        press(&mut app, KeyCode::Backspace);
        press(&mut app, KeyCode::Backspace);
        press(&mut app, KeyCode::Backspace);
        press(&mut app, KeyCode::Backspace);
        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.query, "user");
        assert_eq!(app.hits.len(), 5);
    }

    #[test]
    fn arrows_move_the_highlight_and_clamp() {
        let mut app = app();
        press(&mut app, KeyCode::Up);
        assert_eq!(app.hit_index, 0);
        for _ in 0..20 {
            press(&mut app, KeyCode::Down);
        }
        assert_eq!(app.hit_index, 9);
        press(&mut app, KeyCode::Up);
        assert_eq!(app.hit_index, 8);
    }

    #[test]
    fn esc_quits_from_the_search_list_and_q_is_just_a_letter_there() {
        let mut app = app();
        press(&mut app, KeyCode::Char('q'));
        assert!(!app.should_quit);
        assert_eq!(app.query, "q");
        press(&mut app, KeyCode::Esc);
        assert!(app.should_quit);
    }

    #[test]
    fn enter_opens_follows_and_backspace_walks_back() {
        let mut app = app();
        type_text(&mut app, "userread");
        press(&mut app, KeyCode::Enter);
        assert_eq!(title(&app), Some("Query.userRead(id: ID!): User"));

        // First nav item is "returns User".
        press(&mut app, KeyCode::Enter);
        assert_eq!(title(&app), Some("User (OBJECT)"));

        // Down to `role: Role`, follow it.
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        assert_eq!(title(&app), Some("Role (ENUM)"));

        press(&mut app, KeyCode::Backspace);
        assert_eq!(title(&app), Some("User (OBJECT)"));
        press(&mut app, KeyCode::Esc);
        assert_eq!(title(&app), Some("Query.userRead(id: ID!): User"));
        press(&mut app, KeyCode::Backspace);
        assert_eq!(title(&app), None);
        assert_eq!(app.query, "userread");
        assert!(!app.should_quit);
    }

    #[test]
    fn right_opens_and_follows_and_left_walks_back() {
        let mut app = app();
        type_text(&mut app, "userread");
        press(&mut app, KeyCode::Right);
        assert_eq!(title(&app), Some("Query.userRead(id: ID!): User"));

        press(&mut app, KeyCode::Right);
        assert_eq!(title(&app), Some("User (OBJECT)"));

        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Right);
        assert_eq!(title(&app), Some("Role (ENUM)"));

        press(&mut app, KeyCode::Left);
        assert_eq!(title(&app), Some("User (OBJECT)"));
        press(&mut app, KeyCode::Left);
        assert_eq!(title(&app), Some("Query.userRead(id: ID!): User"));
        press(&mut app, KeyCode::Left);
        assert_eq!(title(&app), None);
        assert_eq!(app.query, "userread");
        assert!(!app.should_quit);
    }

    #[test]
    fn left_in_the_search_list_does_nothing() {
        let mut app = app();
        type_text(&mut app, "user");
        let hits = app.hits.len();
        press(&mut app, KeyCode::Left);
        assert_eq!(app.query, "user");
        assert_eq!(app.hits.len(), hits);
        assert!(app.detail.is_none());
        assert!(!app.should_quit);
    }

    #[test]
    fn items_without_a_target_do_not_navigate() {
        let mut app = app();
        type_text(&mut app, "role");
        press(&mut app, KeyCode::Enter);
        assert_eq!(title(&app), Some("Role (ENUM)"));
        press(&mut app, KeyCode::Enter);
        assert_eq!(title(&app), Some("Role (ENUM)"));
    }

    #[test]
    fn q_quits_from_a_detail_view() {
        let mut app = app();
        type_text(&mut app, "user");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('q'));
        assert!(app.should_quit);
    }

    #[test]
    fn enter_with_no_hits_does_nothing() {
        let mut app = app();
        type_text(&mut app, "zzz");
        assert!(app.hits.is_empty());
        press(&mut app, KeyCode::Enter);
        assert!(app.detail.is_none());
        assert!(app.preview().is_none());
    }

    #[test]
    fn ctrl_c_quits_anywhere() {
        let mut app = app();
        ctrl(&mut app, 'c');
        assert!(app.should_quit);
    }

    #[test]
    fn ctrl_r_requests_one_refresh_at_a_time() {
        let mut app = app();
        assert_eq!(ctrl(&mut app, 'r'), Action::Refresh);
        assert_eq!(app.refresh, RefreshState::Running);
        assert_eq!(ctrl(&mut app, 'r'), Action::None);
        assert_eq!(app.query, "");
    }

    #[test]
    fn successful_refresh_swaps_the_schema_and_resets_navigation() {
        let mut app = app();
        type_text(&mut app, "user");
        press(&mut app, KeyCode::Enter);
        ctrl(&mut app, 'r');

        let response: IntrospectionResponse = serde_json::from_str(FIXTURE).unwrap();
        app.apply_refresh_ok(response.data.schema, 5_000);
        assert_eq!(app.refresh, RefreshState::Idle);
        assert!(app.detail.is_none());
        assert_eq!(app.hits.len(), 5);
        assert!(app.status(5_000).0.contains("<1m"));
    }

    #[test]
    fn unsaved_refresh_applies_the_schema_and_reports_the_write_error() {
        let mut app = app();
        ctrl(&mut app, 'r');
        let mut response: IntrospectionResponse = serde_json::from_str(FIXTURE).unwrap();
        response.data.schema.types.truncate(3);
        let expected = search::search(&response.data.schema, "").len();
        app.apply_refresh_unsaved(response.data.schema, 5_000, "disk full\nmore".to_string());
        assert_eq!(app.hits.len(), expected);
        assert!(expected < 10);
        let (text, _) = app.status(5_000);
        assert!(text.contains("schema updated but the cache could not be written: disk full"));
        assert!(!text.contains("more"));
        assert_eq!(ctrl(&mut app, 'r'), Action::Refresh);
    }

    #[test]
    fn failed_refresh_keeps_the_schema_and_shows_the_error() {
        let mut app = app();
        ctrl(&mut app, 'r');
        app.apply_refresh_err("connection refused\nsecond line".to_string());
        assert_eq!(app.hits.len(), 10);
        let (text, _) = app.status(1_000);
        assert!(text.contains("refresh failed: connection refused"));
        assert!(!text.contains("second line"));
        // A new refresh is possible again.
        assert_eq!(ctrl(&mut app, 'r'), Action::Refresh);
    }

    #[test]
    fn status_shows_host_age_and_staleness() {
        let app = app();
        let (text, level) = app.status(1_000 + 3 * 3_600);
        assert!(text.contains("nexus.example.com"));
        assert!(text.contains("schema fetched 3h ago"));
        assert!(text.contains("Ctrl-R"));
        assert_eq!(level, StatusLevel::Fresh);

        assert_eq!(app.status(1_000 + 24 * 3_600).1, StatusLevel::Fresh);
        assert_eq!(app.status(1_000 + 24 * 3_600 + 1).1, StatusLevel::Stale);
    }

    #[test]
    fn status_shows_refreshing_while_running() {
        let mut app = app();
        ctrl(&mut app, 'r');
        assert!(app.status(1_000).0.contains("refreshing"));
    }

    #[test]
    fn host_of_strips_scheme_and_path() {
        assert_eq!(
            host_of("https://nexus.example.com/graphql"),
            "nexus.example.com"
        );
        assert_eq!(host_of("http://127.0.0.1:4000/graphql"), "127.0.0.1:4000");
        assert_eq!(host_of("localhost:4000"), "localhost:4000");
        assert_eq!(
            host_of("https://user:pw@nexus.example.com/graphql"),
            "nexus.example.com"
        );
        assert_eq!(host_of("http://user@127.0.0.1:4000"), "127.0.0.1:4000");
    }
}
