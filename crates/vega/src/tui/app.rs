//! Pure application state for the TUI: keys in, state out. No terminal access.

use std::collections::{BTreeSet, HashMap};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use vega_core::cache::{format_age, is_stale};
use vega_core::querygen::{self, GenerateError, GeneratedQuery, VariableDef};
use vega_core::schema::Schema;
use vega_core::search::{self, Hit};
use vega_core::selection::{self, Path, Row, Seg, Selection, TreeRoot};
use vega_core::variables::{self, State, Values};

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

/// Hotkeys in Browse mode as (keys, description); shown by the help modal.
pub const BROWSE_KEYS: &[(&str, &str)] = &[
    ("↑ / ↓", "Move through the results"),
    ("Enter / →", "Open the highlighted item as a tree"),
    ("Ctrl-R", "Refresh the schema"),
    ("? / F1", "Show or hide this help"),
    ("Ctrl-Q", "Quit"),
];

/// Hotkeys in Design mode.
pub const DESIGN_KEYS: &[(&str, &str)] = &[
    ("↑ / ↓", "Move through the tree"),
    ("→", "Expand a node, or step into it"),
    ("←", "Collapse a node, or go to its parent"),
    ("Space", "Select or unselect a field"),
    ("Tab", "Move to the next pane"),
    ("Esc", "Back to the results"),
    ("q / Ctrl-Q", "Quit"),
    ("Ctrl-R", "Refresh the schema"),
    ("h / ? / F1", "Show or hide this help"),
];

/// The pane that receives the keys in Design mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Details,
    Query,
    Variables,
}

/// An edit in progress in the Variables pane.
#[allow(dead_code)] // the variants are only rendered until editing exists
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// Typing a value (numbers, IDs, strings, JSON).
    Text(String),
    /// Picking one of a list of values (enums); an empty option means "unset".
    Choice { options: Vec<String>, index: usize },
}

/// One line of the Variables pane.
#[derive(Debug, Clone)]
pub struct VariableRow {
    pub def: VariableDef,
    /// What the user typed ("" when nothing).
    pub raw: String,
    pub state: State,
}

/// The tree being explored: which root, which nodes are expanded, where the cursor is.
#[derive(Debug, Clone)]
pub struct DesignState {
    pub root: TreeRoot,
    pub expanded: BTreeSet<Path>,
    pub cursor: usize,
    pub selection: Selection,
    pub focus: Focus,
    pub values: Values,
    pub var_cursor: usize,
    pub edit: Option<Edit>,
}

impl DesignState {
    fn new(root: TreeRoot) -> Self {
        let mut expanded = BTreeSet::new();
        expanded.insert(Path::new());
        Self {
            root,
            expanded,
            cursor: 0,
            selection: Selection::default(),
            focus: Focus::Details,
            values: Values::new(),
            var_cursor: 0,
            edit: None,
        }
    }

    /// A type tree is only for reading: nothing can be selected in it.
    pub fn read_only(&self) -> bool {
        matches!(self.root, TreeRoot::Type(_))
    }
}

pub struct App {
    schema: Schema,
    host: String,
    fetched_at: u64,
    pub query: String,
    pub hits: Vec<Hit>,
    pub hit_index: usize,
    pub design: Option<DesignState>,
    sessions: HashMap<String, DesignState>,
    pub refresh: RefreshState,
    pub should_quit: bool,
    pub help_open: bool,
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
            design: None,
            sessions: HashMap::new(),
            refresh: RefreshState::Idle,
            should_quit: false,
            help_open: false,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('q') => {
                    self.should_quit = true;
                    Action::None
                }
                KeyCode::Char('r') => self.request_refresh(),
                _ => Action::None,
            };
        }
        if self.help_open {
            if matches!(
                key.code,
                KeyCode::Esc | KeyCode::F(1) | KeyCode::Char('h' | '?')
            ) {
                self.help_open = false;
            }
            return Action::None;
        }
        let opens_help = match key.code {
            KeyCode::F(1) | KeyCode::Char('?') => true,
            KeyCode::Char('h') => self.design.is_some(),
            _ => false,
        };
        if opens_help {
            self.help_open = true;
            return Action::None;
        }
        if self.design.is_some() {
            self.handle_design_key(key.code);
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
            KeyCode::Enter | KeyCode::Right => self.start_design(),
            _ => {}
        }
    }

    fn handle_design_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Esc => self.leave_design(),
            KeyCode::Tab => self.next_focus(),
            KeyCode::Up if self.focused(Focus::Variables) => self.move_var_cursor(-1),
            KeyCode::Down if self.focused(Focus::Variables) => self.move_var_cursor(1),
            _ if self
                .design
                .as_ref()
                .is_some_and(|design| design.focus != Focus::Details) => {}
            KeyCode::Char(' ') => self.toggle_under_cursor(),
            KeyCode::Down => self.move_cursor(1),
            KeyCode::Up => self.move_cursor(-1),
            KeyCode::Right => self.expand_or_step_in(),
            KeyCode::Left => self.collapse_or_go_up(),
            _ => {}
        }
    }

    /// Details, then Query, then Variables, then back. A type tree has only Details.
    fn next_focus(&mut self) {
        let Some(design) = self.design.as_mut() else {
            return;
        };
        if design.read_only() {
            return;
        }
        design.focus = match design.focus {
            Focus::Details => Focus::Query,
            Focus::Query => Focus::Variables,
            Focus::Variables => Focus::Details,
        };
    }

    fn rerun_search(&mut self) {
        self.hits = search::search(&self.schema, &self.query);
        self.hit_index = 0;
    }

    fn start_design(&mut self) {
        let Some(hit) = self.hits.get(self.hit_index) else {
            return;
        };
        if let Some(root) = TreeRoot::parse(&self.schema, &hit.path) {
            let state = self
                .sessions
                .remove(&root.key())
                .unwrap_or_else(|| DesignState::new(root));
            self.design = Some(state);
        }
    }

    fn leave_design(&mut self) {
        if let Some(design) = self.design.take() {
            self.sessions.insert(design.root.key(), design);
        }
    }

    fn toggle_under_cursor(&mut self) {
        let rows = self.design_rows();
        let Some(design) = self.design.as_mut() else {
            return;
        };
        if design.read_only() {
            return;
        }
        if let Some(row) = rows.get(design.cursor) {
            design.selection.toggle_node(&row.node);
        }
    }

    /// The query for the current selection; `None` outside Design or for a type tree.
    pub fn generated_query(&self) -> Option<Result<GeneratedQuery, GenerateError>> {
        let design = self.design.as_ref()?;
        let operation = design.root.operation()?;
        Some(querygen::generate(
            &self.schema,
            operation,
            &design.selection,
        ))
    }

    /// The variables of the generated query with their typed values (empty when there is
    /// no valid query to show).
    pub fn variable_rows(&self) -> Vec<VariableRow> {
        let (Some(design), Some(Ok(query))) = (self.design.as_ref(), self.generated_query()) else {
            return Vec::new();
        };
        query
            .variables
            .into_iter()
            .map(|def| {
                let typed = design.values.get(&def.path).map(String::as_str);
                let state = variables::state(&self.schema, &def, typed);
                VariableRow {
                    raw: typed.unwrap_or_default().to_string(),
                    def,
                    state,
                }
            })
            .collect()
    }

    fn focused(&self, focus: Focus) -> bool {
        self.design
            .as_ref()
            .is_some_and(|design| design.focus == focus)
    }

    fn move_var_cursor(&mut self, delta: isize) {
        let last = self.variable_rows().len().saturating_sub(1);
        if let Some(design) = self.design.as_mut() {
            design.var_cursor = design.var_cursor.min(last);
            design.var_cursor = design.var_cursor.saturating_add_signed(delta).min(last);
        }
    }

    /// True when the leaf at `path` is selected, or the optional argument at it is opted in.
    pub fn is_selected(&self, path: &[Seg]) -> bool {
        self.design.as_ref().is_some_and(|design| {
            design.selection.contains(path) || design.selection.arg_included(path)
        })
    }

    fn move_cursor(&mut self, delta: isize) {
        let last = self.design_rows().len().saturating_sub(1);
        if let Some(design) = self.design.as_mut() {
            design.cursor = design.cursor.saturating_add_signed(delta).min(last);
        }
    }

    fn expand_or_step_in(&mut self) {
        let rows = self.design_rows();
        let Some(design) = self.design.as_mut() else {
            return;
        };
        let Some(row) = rows.get(design.cursor) else {
            return;
        };
        if !row.node.expandable() {
            return;
        }
        if !row.expanded {
            design.expanded.insert(row.node.path.clone());
        } else if rows
            .get(design.cursor + 1)
            .is_some_and(|next| next.depth > row.depth)
        {
            design.cursor += 1;
        }
    }

    fn collapse_or_go_up(&mut self) {
        let rows = self.design_rows();
        let Some(design) = self.design.as_mut() else {
            return;
        };
        let Some(row) = rows.get(design.cursor) else {
            return;
        };
        if row.expanded {
            design.expanded.remove(&row.node.path);
        } else if let Some(parent) = rows[..design.cursor]
            .iter()
            .rposition(|candidate| candidate.depth + 1 == row.depth)
        {
            design.cursor = parent;
        }
    }

    /// The visible rows of the tree being designed (empty in Browse mode).
    pub fn design_rows(&self) -> Vec<Row> {
        match &self.design {
            Some(design) => selection::rows(&self.schema, &design.root, &design.expanded),
            None => Vec::new(),
        }
    }

    /// Rows for the Details pane in Browse mode: the highlighted hit, root expanded.
    pub fn preview_rows(&self) -> Vec<Row> {
        let Some(hit) = self.hits.get(self.hit_index) else {
            return Vec::new();
        };
        let Some(root) = TreeRoot::parse(&self.schema, &hit.path) else {
            return Vec::new();
        };
        let mut expanded = BTreeSet::new();
        expanded.insert(Path::new());
        selection::rows(&self.schema, &root, &expanded)
    }

    /// Description of the node under the cursor (Design) or of the highlighted hit (Browse).
    pub fn current_description(&self) -> String {
        match &self.design {
            Some(design) => self
                .design_rows()
                .get(design.cursor)
                .map(|row| row.node.description.clone())
                .unwrap_or_default(),
            None => self
                .preview_rows()
                .first()
                .map(|row| row.node.description.clone())
                .unwrap_or_default(),
        }
    }

    pub fn help_keys(&self) -> &'static [(&'static str, &'static str)] {
        if self.design.is_some() {
            DESIGN_KEYS
        } else {
            BROWSE_KEYS
        }
    }

    pub fn apply_refresh_ok(&mut self, schema: Schema, fetched_at: u64) {
        self.schema = schema;
        self.fetched_at = fetched_at;
        self.design = None;
        self.sessions.clear();
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
    use vega_core::selection::Seg;

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
    fn esc_does_not_quit_the_search_list_and_q_is_just_a_letter_there() {
        let mut app = app();
        press(&mut app, KeyCode::Char('q'));
        assert!(!app.should_quit);
        assert_eq!(app.query, "q");
        press(&mut app, KeyCode::Esc);
        assert!(!app.should_quit);
        assert_eq!(app.query, "q");
    }

    #[test]
    fn left_in_the_search_list_does_nothing() {
        let mut app = app();
        type_text(&mut app, "user");
        let hits = app.hits.len();
        press(&mut app, KeyCode::Left);
        assert_eq!(app.query, "user");
        assert_eq!(app.hits.len(), hits);
        assert!(app.design.is_none());
        assert!(!app.should_quit);
    }

    const DESIGN_FIXTURE: &str = include_str!("../../../vega-core/tests/fixtures/design.json");

    fn design_app() -> App {
        let response: IntrospectionResponse = serde_json::from_str(DESIGN_FIXTURE).unwrap();
        App::new(response.data.schema, "https://example.com/graphql", 1_000)
    }

    /// Opens the design view of `Query.me` in the design fixture app.
    fn open_me(app: &mut App) {
        type_text(app, "me");
        assert_eq!(app.hits[0].path, "Query.me");
        press(app, KeyCode::Enter);
        assert!(app.design.is_some());
    }

    fn row_labels(app: &App) -> Vec<String> {
        app.design_rows()
            .iter()
            .map(|r| r.node.label.clone())
            .collect()
    }

    fn open_design(app: &mut App) {
        type_text(app, "userread");
        press(app, KeyCode::Enter);
        assert!(app.design.is_some());
    }

    #[test]
    fn f1_and_question_mark_open_help_in_both_modes() {
        for code in [KeyCode::F(1), KeyCode::Char('?')] {
            let mut app = app();
            press(&mut app, code);
            assert!(app.help_open);

            let mut app = self::app();
            open_design(&mut app);
            press(&mut app, code);
            assert!(app.help_open);
        }
    }

    #[test]
    fn question_mark_is_not_typed_into_the_query() {
        let mut app = app();
        type_text(&mut app, "user");
        press(&mut app, KeyCode::Char('?'));
        assert_eq!(app.query, "user");
        assert!(app.help_open);
    }

    #[test]
    fn h_types_in_the_search_list_but_opens_help_in_design_mode() {
        let mut app = app();
        press(&mut app, KeyCode::Char('h'));
        assert_eq!(app.query, "h");
        assert!(!app.help_open);

        let mut app = self::app();
        open_design(&mut app);
        press(&mut app, KeyCode::Char('h'));
        assert!(app.help_open);
        assert!(app.design.is_some());
    }

    #[test]
    fn help_closes_with_esc_h_question_mark_and_f1() {
        for code in [
            KeyCode::Esc,
            KeyCode::Char('h'),
            KeyCode::Char('?'),
            KeyCode::F(1),
        ] {
            let mut app = app();
            press(&mut app, KeyCode::F(1));
            press(&mut app, code);
            assert!(!app.help_open, "{code:?}");
            assert!(!app.should_quit, "{code:?}");
            assert_eq!(app.query, "");
        }
    }

    #[test]
    fn esc_closes_only_the_help_in_design_mode() {
        let mut app = app();
        open_design(&mut app);
        press(&mut app, KeyCode::Char('?'));
        press(&mut app, KeyCode::Esc);
        assert!(!app.help_open);
        assert!(app.design.is_some());
        assert!(!app.should_quit);
    }

    #[test]
    fn other_keys_are_ignored_while_help_is_open() {
        let mut app = app();
        open_design(&mut app);
        press(&mut app, KeyCode::Char('?'));
        for code in [
            KeyCode::Down,
            KeyCode::Right,
            KeyCode::Left,
            KeyCode::Enter,
            KeyCode::Backspace,
            KeyCode::Char('q'),
            KeyCode::Char('x'),
        ] {
            press(&mut app, code);
        }
        assert!(app.help_open);
        assert!(!app.should_quit);
        assert!(app.design.is_some());
        assert_eq!(app.design.as_ref().unwrap().cursor, 0);

        let mut app = self::app();
        press(&mut app, KeyCode::F(1));
        type_text(&mut app, "abc");
        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.query, "");
        assert_eq!(app.hit_index, 0);
    }

    #[test]
    fn ctrl_q_quits_and_ctrl_r_refreshes_while_help_is_open() {
        let mut app = app();
        press(&mut app, KeyCode::F(1));
        assert_eq!(ctrl(&mut app, 'r'), Action::Refresh);
        assert!(app.help_open);
        ctrl(&mut app, 'q');
        assert!(app.should_quit);
    }

    #[test]
    fn q_quits_in_design_mode() {
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
        assert!(app.design.is_none());
        assert!(app.preview_rows().is_empty());
    }

    #[test]
    fn ctrl_q_quits_anywhere_and_ctrl_c_no_longer_does() {
        let mut app = app();
        ctrl(&mut app, 'c');
        assert!(!app.should_quit);
        ctrl(&mut app, 'q');
        assert!(app.should_quit);

        let mut app = design_app();
        open_me(&mut app);
        ctrl(&mut app, 'q');
        assert!(app.should_quit);
    }

    #[test]
    fn backspace_edits_the_search_in_browse_and_does_nothing_in_design() {
        let mut app = design_app();
        open_me(&mut app);
        press(&mut app, KeyCode::Backspace);
        assert!(app.design.is_some());
        assert!(!app.should_quit);

        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.query, "m");
    }

    #[test]
    fn tab_cycles_the_focus_and_the_tree_keys_only_act_on_details() {
        use super::Focus;
        let mut app = design_app();
        open_me(&mut app);
        assert_eq!(app.design.as_ref().unwrap().focus, Focus::Details);

        press(&mut app, KeyCode::Tab);
        assert_eq!(app.design.as_ref().unwrap().focus, Focus::Query);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' '));
        assert_eq!(app.design.as_ref().unwrap().cursor, 0);
        assert!(matches!(app.generated_query(), Some(Err(_))));

        press(&mut app, KeyCode::Tab);
        assert_eq!(app.design.as_ref().unwrap().focus, Focus::Variables);
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.design.as_ref().unwrap().focus, Focus::Details);
        press(&mut app, KeyCode::Down);
        assert_eq!(app.design.as_ref().unwrap().cursor, 1);
    }

    #[test]
    fn esc_goes_back_from_any_focus_and_tab_does_nothing_on_a_type_tree() {
        use super::Focus;
        let mut app = design_app();
        open_me(&mut app);
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Esc);
        assert!(app.design.is_none());

        let mut app = design_app();
        type_text(&mut app, "postinput");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.design.as_ref().unwrap().focus, Focus::Details);
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
        assert!(app.design.is_none());
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

    #[test]
    fn enter_and_right_open_the_highlighted_hit_with_its_root_expanded() {
        for code in [KeyCode::Enter, KeyCode::Right] {
            let mut app = design_app();
            type_text(&mut app, "me");
            press(&mut app, code);
            let labels = row_labels(&app);
            assert_eq!(labels.len(), 7);
            assert_eq!(labels[0], "me: User");
            assert_eq!(labels[1], "id: ID!");
            assert_eq!(app.design.as_ref().unwrap().cursor, 0);
        }
    }

    #[test]
    fn a_type_hit_opens_the_same_tree() {
        let mut app = design_app();
        type_text(&mut app, "postinput");
        press(&mut app, KeyCode::Enter);
        assert_eq!(
            row_labels(&app),
            ["PostInput (INPUT_OBJECT)", "title: String!"]
        );
    }

    #[test]
    fn up_and_down_move_the_cursor_and_clamp() {
        let mut app = design_app();
        open_me(&mut app);
        press(&mut app, KeyCode::Up);
        assert_eq!(app.design.as_ref().unwrap().cursor, 0);
        for _ in 0..20 {
            press(&mut app, KeyCode::Down);
        }
        assert_eq!(app.design.as_ref().unwrap().cursor, 6);
        press(&mut app, KeyCode::Up);
        assert_eq!(app.design.as_ref().unwrap().cursor, 5);
    }

    #[test]
    fn right_expands_then_steps_into_a_composite_and_left_collapses_then_goes_up() {
        let mut app = design_app();
        open_me(&mut app);
        for _ in 0..4 {
            press(&mut app, KeyCode::Down); // friends: [User!]!
        }
        assert_eq!(app.design_rows()[4].node.label, "friends: [User!]!");

        press(&mut app, KeyCode::Right); // expand
        assert_eq!(app.design_rows().len(), 13);
        assert_eq!(app.design.as_ref().unwrap().cursor, 4);

        press(&mut app, KeyCode::Right); // step into the first child
        assert_eq!(app.design.as_ref().unwrap().cursor, 5);

        press(&mut app, KeyCode::Left); // collapsed leaf: go to the parent
        assert_eq!(app.design.as_ref().unwrap().cursor, 4);

        press(&mut app, KeyCode::Left); // expanded: collapse
        assert_eq!(app.design_rows().len(), 7);
        assert_eq!(app.design.as_ref().unwrap().cursor, 4);
    }

    #[test]
    fn right_does_nothing_on_leaves_and_recursive_nodes() {
        let mut app = design_app();
        open_me(&mut app);
        press(&mut app, KeyCode::Down); // id (leaf)
        press(&mut app, KeyCode::Right);
        assert_eq!(app.design_rows().len(), 7);
        assert_eq!(app.design.as_ref().unwrap().cursor, 1);

        // Expand friends, then move to its recursive `friends` child.
        for _ in 0..3 {
            press(&mut app, KeyCode::Down);
        }
        press(&mut app, KeyCode::Right);
        for _ in 0..4 {
            press(&mut app, KeyCode::Down);
        }
        assert_eq!(app.design_rows()[8].node.label, "friends: [User!]!");
        let before = app.design_rows().len();
        press(&mut app, KeyCode::Right);
        assert_eq!(app.design_rows().len(), before);
    }

    #[test]
    fn left_on_the_collapsed_root_does_nothing() {
        let mut app = design_app();
        open_me(&mut app);
        press(&mut app, KeyCode::Left); // collapse the root
        assert_eq!(app.design_rows().len(), 1);
        press(&mut app, KeyCode::Left);
        assert_eq!(app.design_rows().len(), 1);
        assert!(app.design.is_some());
        assert!(!app.should_quit);
    }

    #[test]
    fn esc_returns_to_the_results_and_keeps_the_search_text() {
        let mut app = design_app();
        open_me(&mut app);
        press(&mut app, KeyCode::Esc);
        assert!(app.design.is_none());
        assert_eq!(app.query, "me");
        assert!(!app.should_quit);
    }

    #[test]
    fn q_is_a_letter_in_the_results_and_quits_in_design() {
        let mut app = design_app();
        open_me(&mut app);
        press(&mut app, KeyCode::Char('q'));
        assert!(app.should_quit);
    }

    #[test]
    fn preview_rows_show_the_highlighted_hit_with_its_root_expanded() {
        let mut app = design_app();
        type_text(&mut app, "me");
        let preview = app.preview_rows();
        assert_eq!(preview.len(), 7);
        assert_eq!(preview[0].node.label, "me: User");
        assert!(preview[0].expanded);
        assert_eq!(app.current_description(), "The current user.");

        type_text(&mut app, "zzz");
        assert!(app.preview_rows().is_empty());
        assert_eq!(app.current_description(), "");
    }

    #[test]
    fn the_description_follows_the_cursor_in_design() {
        let mut app = design_app();
        open_me(&mut app);
        assert_eq!(app.current_description(), "The current user.");
        for _ in 0..3 {
            press(&mut app, KeyCode::Down); // role: Role
        }
        assert_eq!(app.current_description(), "Access role.");
    }

    #[test]
    fn help_lists_the_keys_of_the_current_mode() {
        let mut app = design_app();
        assert!(app
            .help_keys()
            .iter()
            .any(|(keys, _)| keys.contains("Enter")));
        assert!(!app.help_keys().iter().any(|(keys, _)| keys.contains('q')));
        open_me(&mut app);
        assert!(app.help_keys().iter().any(|(keys, _)| keys.contains("←")));
        assert!(app
            .help_keys()
            .iter()
            .any(|(keys, _)| keys.starts_with("q")));
    }

    #[test]
    fn a_refresh_leaves_design_mode() {
        let mut app = design_app();
        open_me(&mut app);
        let response: IntrospectionResponse = serde_json::from_str(DESIGN_FIXTURE).unwrap();
        app.apply_refresh_ok(response.data.schema, 5_000);
        assert!(app.design.is_none());
    }

    fn query_text(app: &App) -> String {
        app.generated_query().unwrap().unwrap().text
    }

    #[test]
    fn space_toggles_leaves_and_the_query_follows() {
        let mut app = design_app();
        open_me(&mut app);
        assert!(matches!(
            app.generated_query(),
            Some(Err(vega_core::querygen::GenerateError::EmptySelection))
        ));

        press(&mut app, KeyCode::Down); // id
        press(&mut app, KeyCode::Char(' '));
        assert_eq!(query_text(&app), "query Me {\n  me {\n    id\n  }\n}");
        assert!(app.is_selected(&[Seg::Field("id".to_string())]));

        press(&mut app, KeyCode::Down); // name
        press(&mut app, KeyCode::Char(' '));
        assert_eq!(
            query_text(&app),
            "query Me {\n  me {\n    id\n    name\n  }\n}"
        );

        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Char(' ')); // unselect id
        assert_eq!(query_text(&app), "query Me {\n  me {\n    name\n  }\n}");
    }

    #[test]
    fn space_on_an_object_does_nothing() {
        let mut app = design_app();
        open_me(&mut app);
        press(&mut app, KeyCode::Char(' ')); // the root object
        for _ in 0..4 {
            press(&mut app, KeyCode::Down); // friends
        }
        press(&mut app, KeyCode::Char(' '));
        assert!(matches!(app.generated_query(), Some(Err(_))));
    }

    #[test]
    fn selecting_a_nested_leaf_includes_its_ancestors_and_variables() {
        let mut app = design_app();
        open_me(&mut app);
        for _ in 0..5 {
            press(&mut app, KeyCode::Down); // posts(first: Int!)
        }
        press(&mut app, KeyCode::Right); // expand posts
        press(&mut app, KeyCode::Down); // args
        press(&mut app, KeyCode::Down); // id
        press(&mut app, KeyCode::Char(' '));
        let query = app.generated_query().unwrap().unwrap();
        assert_eq!(
            query.text,
            "query Me($first: Int!) {\n  me {\n    posts(first: $first) {\n      id\n    }\n  }\n}"
        );
        assert_eq!(query.variables.len(), 1);
    }

    #[test]
    fn scalar_operations_generate_without_a_selection() {
        let mut app = design_app();
        type_text(&mut app, "version");
        press(&mut app, KeyCode::Enter);
        let query = app.generated_query().unwrap().unwrap();
        assert_eq!(query.text, "query Version {\n  version\n}");
    }

    #[test]
    fn type_roots_are_read_only() {
        let mut app = design_app();
        type_text(&mut app, "postinput");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' '));
        assert!(app.generated_query().is_none());
        assert!(!app.is_selected(&[Seg::Field("title".to_string())]));
        assert!(app.design.as_ref().unwrap().read_only());
    }

    #[test]
    fn selection_expansion_and_cursor_survive_leaving_and_reentering() {
        let mut app = design_app();
        open_me(&mut app);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' ')); // id
        press(&mut app, KeyCode::Down); // name, cursor 2
        press(&mut app, KeyCode::Esc);
        assert!(app.design.is_none());

        press(&mut app, KeyCode::Enter);
        assert_eq!(app.design.as_ref().unwrap().cursor, 2);
        assert_eq!(query_text(&app), "query Me {\n  me {\n    id\n  }\n}");
    }

    #[test]
    fn a_refresh_clears_the_stored_sessions() {
        let mut app = design_app();
        open_me(&mut app);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Esc);
        let response: IntrospectionResponse = serde_json::from_str(DESIGN_FIXTURE).unwrap();
        app.apply_refresh_ok(response.data.schema, 5_000);
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.generated_query(), Some(Err(_))));
    }

    #[test]
    fn help_in_design_mentions_space() {
        let mut app = design_app();
        open_me(&mut app);
        assert!(app.help_keys().iter().any(|(keys, _)| *keys == "Space"));
    }

    #[test]
    fn space_toggles_optional_arguments_and_ignores_required_ones() {
        let mut app = design_app();
        type_text(&mut app, "search");
        assert_eq!(app.hits[0].path, "Query.search");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down); // args group
        press(&mut app, KeyCode::Right); // expand it
        press(&mut app, KeyCode::Down); // term (required)
        press(&mut app, KeyCode::Char(' '));
        assert!(!app.is_selected(&[Seg::Args, Seg::Arg("term".to_string())]));
        press(&mut app, KeyCode::Down); // first (optional)
        press(&mut app, KeyCode::Char(' '));
        assert!(app.is_selected(&[Seg::Args, Seg::Arg("first".to_string())]));
        press(&mut app, KeyCode::Char(' '));
        assert!(!app.is_selected(&[Seg::Args, Seg::Arg("first".to_string())]));
    }

    #[test]
    fn opted_in_arguments_appear_in_the_generated_query() {
        let mut app = design_app();
        type_text(&mut app, "users");
        assert_eq!(app.hits[0].path, "Query.users");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' ')); // id
        press(&mut app, KeyCode::Up); // args group
        press(&mut app, KeyCode::Right); // expand it
        press(&mut app, KeyCode::Down); // role
        press(&mut app, KeyCode::Char(' '));
        assert_eq!(
            query_text(&app),
            "query Users($role: Role) {\n  users(role: $role) {\n    id\n  }\n}"
        );
    }

    /// Opens Query.node with `id` selected and the Variables pane focused.
    fn node_variables(app: &mut App) {
        type_text(app, "node");
        assert_eq!(app.hits[0].path, "Query.node");
        press(app, KeyCode::Enter);
        press(app, KeyCode::Down);
        press(app, KeyCode::Down);
        press(app, KeyCode::Char(' ')); // id
        press(app, KeyCode::Tab);
        press(app, KeyCode::Tab);
    }

    #[test]
    fn variable_rows_follow_the_included_arguments() {
        let mut app = design_app();
        open_me(&mut app);
        assert!(app.variable_rows().is_empty());

        let mut app = design_app();
        node_variables(&mut app);
        let rows = app.variable_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].def.name, "id");
        assert_eq!(rows[0].raw, "");
        assert_eq!(
            rows[0].state,
            vega_core::variables::State::Invalid("required".to_string())
        );
    }

    #[test]
    fn up_and_down_move_the_variable_cursor_when_variables_has_focus() {
        use super::Focus;
        let mut app = design_app();
        type_text(&mut app, "users");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' ')); // id
        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Right);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' ')); // role
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' ')); // active
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.design.as_ref().unwrap().focus, Focus::Variables);
        assert_eq!(app.variable_rows().len(), 2);

        let tree_cursor = app.design.as_ref().unwrap().cursor;
        press(&mut app, KeyCode::Down);
        assert_eq!(app.design.as_ref().unwrap().var_cursor, 1);
        press(&mut app, KeyCode::Down);
        assert_eq!(app.design.as_ref().unwrap().var_cursor, 1);
        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Up);
        assert_eq!(app.design.as_ref().unwrap().var_cursor, 0);
        assert_eq!(app.design.as_ref().unwrap().cursor, tree_cursor);
    }
}
