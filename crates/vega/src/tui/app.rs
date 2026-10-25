//! Pure application state for the TUI: keys in, state out. No terminal access.

use std::collections::{BTreeSet, HashMap};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use vega_core::cache::{format_age, is_stale};
use vega_core::client;
use vega_core::config::Settings;
use vega_core::export;
use vega_core::import::{self, Imported};
use vega_core::querygen::{self, GenerateError, GeneratedQuery, VariableDef};
use vega_core::response::{LineKind, ResponseView};
use vega_core::schema::{Root, Schema};
use vega_core::search::{self, Hit};
use vega_core::selection::{self, NodeKind, Path, Row, Seg, Selection, TreeRoot};
use vega_core::variables::{self, Editor, State, Values};

use crate::tui::connection::{Modal, Outcome};
use crate::tui::profiles::{Entry, Outcome as PickerOutcome, Picker};

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    None,
    /// The event loop should start a background schema refresh.
    Refresh,
    /// The event loop should send this request on a background thread.
    Run(RunRequest),
    /// The event loop should put this text on the clipboard.
    Copy(String),
    /// The endpoint changed: the event loop should load the schema for it, from the
    /// cache when there is one, otherwise from the server.
    Load,
    /// Save this connection as a profile (keyring and `config.toml`); the loop answers
    /// with `profile_saved` or `profile_save_failed`.
    SaveProfile {
        name: String,
        settings: Settings,
    },
    /// Read the saved profiles and open the picker.
    OpenPicker,
    /// Connect to this saved profile.
    ConnectProfile(String),
    /// Open this saved profile in the connection modal.
    EditProfile(String),
    /// Delete this saved profile (config entry and keyring secret).
    DeleteProfile(String),
}

/// A request ready to be sent; `seq` ties the result to it.
#[derive(Debug, Clone, PartialEq)]
pub struct RunRequest {
    pub seq: u64,
    pub body: serde_json::Value,
}

/// What the Response pane shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunState {
    Running,
    Done(ResponseView),
    /// No HTTP response at all (connection failure, timeout).
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunView {
    pub state: RunState,
    /// First visible line of `ResponseView::body_lines`.
    pub scroll: usize,
}

/// A mutation waiting for `y`.
#[derive(Debug, Clone)]
struct PendingRun {
    request: RunRequest,
    name: String,
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
    Notice,
}

/// Rows a page key moves through in the Results list.
const PAGE: usize = 10;

/// Hotkeys in Browse mode as (keys, description); shown by the help modal.
pub const BROWSE_KEYS: &[(&str, &str)] = &[
    ("↑ / ↓", "Move through the results"),
    ("PgUp / PgDn", "Move ten results"),
    ("Ctrl-Home / Ctrl-End", "Go to the first or last result"),
    ("Enter / →", "Open the highlighted item as a tree"),
    ("Paste", "Import a query (GraphQL or a JSON request body)"),
    ("Ctrl-R", "Refresh the schema"),
    ("Ctrl-K", "Open the connection (endpoint and credentials)"),
    ("Ctrl-P", "Open the saved server profiles"),
    ("? / F1", "Show or hide this help"),
    ("Ctrl-Q", "Quit"),
];

/// Hotkeys in Design mode.
pub const DESIGN_KEYS: &[(&str, &str)] = &[
    ("↑ / ↓", "Move through the tree"),
    ("PgUp / PgDn", "Move ten rows"),
    ("Ctrl-Home / Ctrl-End", "Go to the first or last row"),
    ("→", "Expand a node, or step into it"),
    ("←", "Collapse a node, or go to its parent"),
    (
        "Space",
        "Select or unselect a field, then move to the next one",
    ),
    (
        "Ctrl-A",
        "Select every field that needs no variable, three levels deep (again to clear)",
    ),
    (
        "Alt-A",
        "Select every field, variables included (again to clear)",
    ),
    ("Tab", "Move to the next pane"),
    ("Enter", "Edit the highlighted variable (Variables pane)"),
    ("Ctrl-C", "Copy the query as a GraphQL document"),
    (
        "Ctrl-J",
        "Copy the query and variables as a JSON request body",
    ),
    ("Paste", "Import a query, replacing the current one"),
    ("r", "Run the query"),
    ("Esc", "Back to the results"),
    ("q / Ctrl-Q", "Quit"),
    ("Ctrl-R", "Refresh the schema"),
    ("Ctrl-K", "Open the connection (endpoint and credentials)"),
    ("Ctrl-P", "Open the saved server profiles"),
    ("h / ? / F1", "Show or hide this help"),
];

/// Hotkeys in Run mode.
pub const RUN_KEYS: &[(&str, &str)] = &[
    (
        "↑ / ↓",
        "Scroll the response (move through the variables when they have focus)",
    ),
    ("PgUp / PgDn", "Scroll the response ten lines"),
    ("Tab", "Switch between Response and Variables"),
    ("Enter", "Edit the highlighted variable (Variables pane)"),
    (
        "Ctrl-C",
        "Copy the response (Response pane) or the query as GraphQL (Variables pane)",
    ),
    (
        "Ctrl-J",
        "Copy the query and variables as a JSON request body (Variables pane)",
    ),
    ("r", "Run the query again"),
    ("Esc", "Back to Design"),
    ("q / Ctrl-Q", "Quit"),
    ("Ctrl-R", "Refresh the schema"),
    ("Ctrl-K", "Open the connection (endpoint and credentials)"),
    ("Ctrl-P", "Open the saved server profiles"),
    ("h / ? / F1", "Show or hide this help"),
];

/// The pane that receives the keys in Design mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Details,
    Query,
    Variables,
    Response,
}

/// An edit in progress in the Variables pane.
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
    /// `Some` means Run mode.
    pub run: Option<RunView>,
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
            run: None,
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
    /// The saved profile the connection came from, shown in the status bar.
    profile_name: Option<String>,
    settings: Settings,
    /// The connection modal, when it is open.
    pub modal: Option<Modal>,
    /// The saved-profile picker, when it is open.
    pub picker: Option<Picker>,
    /// False until a schema has been loaded for the current endpoint.
    loaded: bool,
    fetched_at: u64,
    pub query: String,
    pub hits: Vec<Hit>,
    pub hit_index: usize,
    pub design: Option<DesignState>,
    sessions: HashMap<String, DesignState>,
    pub refresh: RefreshState,
    pub should_quit: bool,
    pub help_open: bool,
    /// A one-key message shown in the status line; the next key clears it.
    notice: Option<String>,
    confirm: Option<PendingRun>,
    /// A pasted query waiting for `y` because it would replace a selection.
    pending_import: Option<Imported>,
    run_seq: u64,
    run_in_flight: bool,
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

fn empty_schema() -> Schema {
    Schema {
        query_type: None,
        mutation_type: None,
        subscription_type: None,
        types: Vec::new(),
    }
}

/// First visible row of a list that keeps the selection in the middle of a viewport
/// `height` rows tall. The list is treated as one row longer than it is, so the last item
/// can be centered no further than one empty line from the end.
pub fn list_offset(len: usize, selected: usize, height: usize) -> usize {
    let last = (len + 1).saturating_sub(height);
    selected.saturating_sub(height / 2).min(last)
}

/// Stores the text typed for a variable; empty text clears it.
fn set_value(design: &mut DesignState, path: &Path, raw: &str) {
    if raw.is_empty() {
        design.values.remove(path);
    } else {
        design.values.insert(path.clone(), raw.to_string());
    }
}

impl App {
    pub fn new(schema: Schema, uri: &str, fetched_at: u64) -> Self {
        let hits = search::search(&schema, "");
        Self {
            schema,
            host: host_of(uri),
            profile_name: None,
            settings: Settings {
                uri: uri.to_string(),
                headers: Vec::new(),
                key_only: false,
            },
            modal: None,
            picker: None,
            loaded: true,
            fetched_at,
            query: String::new(),
            hits,
            hit_index: 0,
            design: None,
            sessions: HashMap::new(),
            refresh: RefreshState::Idle,
            should_quit: false,
            help_open: false,
            notice: None,
            confirm: None,
            pending_import: None,
            run_seq: 0,
            run_in_flight: false,
        }
    }

    /// Replaces the connection settings (used once, at startup).
    pub fn with_settings(mut self, settings: Settings) -> Self {
        self.host = host_of(&settings.uri);
        self.settings = settings;
        self
    }

    /// An app with no schema yet. Without an endpoint it opens the connection modal.
    pub fn empty(settings: Settings) -> Self {
        let modal = settings.uri.is_empty().then(|| Modal::new(&settings));
        let mut app = Self::new(empty_schema(), &settings.uri, 0).with_settings(settings);
        app.loaded = false;
        app.modal = modal;
        app
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// A schema load was started: the status bar says so.
    pub fn begin_load(&mut self) {
        self.refresh = RefreshState::Running;
    }

    /// Text pasted into the terminal (bracketed paste). It goes to the modal when it is
    /// open. While composing or searching, a pasted query is imported; any other text
    /// goes only into the search box or a text variable editor, so pasted text can never
    /// act as command keys.
    pub fn handle_paste(&mut self, text: &str) {
        if let Some(modal) = self.modal.as_mut() {
            modal.paste(text);
            return;
        }
        if self.help_open
            || self.picker.is_some()
            || self.confirm.is_some()
            || self.pending_import.is_some()
        {
            return;
        }
        let in_text_editor = self
            .design
            .as_ref()
            .is_some_and(|design| matches!(design.edit, Some(Edit::Text(_))));
        let composing = self
            .design
            .as_ref()
            .is_none_or(|design| design.run.is_none());
        if composing && !in_text_editor && import::looks_like_query(text) {
            self.import_query(text);
            return;
        }
        let chars = text.chars().filter(|c| !c.is_control());
        if self.design.is_none() {
            self.notice = None;
            self.query.extend(chars);
            self.rerun_search();
        } else if in_text_editor {
            self.notice = None;
            for c in chars {
                self.handle_edit_key(KeyCode::Char(c));
            }
        }
    }

    /// The modal returned new settings.
    fn apply_connection(&mut self, settings: Settings) -> Action {
        let uri_changed = settings.uri != self.settings.uri;
        self.settings = settings;
        if uri_changed {
            self.reset_connection();
            self.refresh = RefreshState::Running;
            Action::Load
        } else if !self.loaded {
            self.refresh = RefreshState::Running;
            Action::Refresh
        } else {
            Action::None
        }
    }

    /// The loop saved the profile: it becomes the current connection.
    pub fn profile_saved(&mut self, name: String, settings: Settings) -> Action {
        self.modal = None;
        self.profile_name = Some(name);
        self.apply_connection(settings)
    }

    /// The save failed: the modal stays open so the typed secret is not lost.
    pub fn profile_save_failed(&mut self, message: String) {
        if let Some(modal) = self.modal.as_mut() {
            modal.set_error(message);
        }
    }

    /// The loop read the profiles: show the picker.
    pub fn open_picker(&mut self, entries: Vec<Entry>) {
        self.picker = Some(Picker::new(entries, self.profile_name.clone()));
    }

    /// The loop could not do what the picker asked; the picker stays open.
    pub fn profile_notice(&mut self, message: &str) {
        self.notice = Some(message.to_string());
    }

    /// The loop read the profile's settings: it becomes the current connection.
    pub fn connect_profile(&mut self, name: String, settings: Settings) -> Action {
        self.picker = None;
        self.profile_name = Some(name);
        self.apply_connection(settings)
    }

    /// The loop read the profile's settings: edit them in the connection modal.
    pub fn edit_profile(&mut self, name: String, settings: Settings) {
        self.picker = None;
        self.modal = Some(Modal::for_profile(&name, &settings));
    }

    /// A deleted profile that was in use stays connected, but is no longer named.
    pub fn profile_deleted(&mut self, name: &str) {
        if self.profile_name.as_deref() == Some(name) {
            self.profile_name = None;
        }
    }

    /// The server shown in the status bar: the profile name when there is one.
    fn server_label(&self) -> String {
        match &self.profile_name {
            Some(name) => format!("{name} ({})", self.host),
            None => self.host.clone(),
        }
    }

    /// A new endpoint: nothing of the old schema (selection, query, variables) applies.
    fn reset_connection(&mut self) {
        self.schema = empty_schema();
        self.loaded = false;
        self.fetched_at = 0;
        self.host = host_of(&self.settings.uri);
        self.query.clear();
        self.hits = search::search(&self.schema, "");
        self.hit_index = 0;
        self.design = None;
        self.sessions.clear();
        self.confirm = None;
        self.pending_import = None;
        self.help_open = false;
        self.run_in_flight = false;
        self.run_seq += 1;
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        self.notice = None;
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if let Some(modal) = self.modal.as_mut() {
            if ctrl && key.code == KeyCode::Char('q') {
                self.should_quit = true;
                return Action::None;
            }
            return match modal.handle_key(key) {
                Outcome::Open => Action::None,
                Outcome::Cancel => {
                    self.modal = None;
                    Action::None
                }
                Outcome::Submit(settings) => {
                    self.modal = None;
                    self.profile_name = None;
                    self.apply_connection(settings)
                }
                Outcome::Save(name, settings) => Action::SaveProfile { name, settings },
            };
        }
        if let Some(picker) = self.picker.as_mut() {
            if ctrl && key.code == KeyCode::Char('q') {
                self.should_quit = true;
                return Action::None;
            }
            return match picker.handle_key(key) {
                PickerOutcome::Open => Action::None,
                PickerOutcome::Close => {
                    self.picker = None;
                    Action::None
                }
                PickerOutcome::Connect(name) => Action::ConnectProfile(name),
                PickerOutcome::Edit(name) => Action::EditProfile(name),
                PickerOutcome::Delete(name) => Action::DeleteProfile(name),
                PickerOutcome::New => {
                    self.picker = None;
                    self.modal = Some(Modal::new(&Settings {
                        uri: String::new(),
                        headers: Vec::new(),
                        key_only: false,
                    }));
                    Action::None
                }
            };
        }
        if key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Char('a') {
            return self.toggle_select_all(true);
        }
        if ctrl {
            return match key.code {
                KeyCode::Char('q') => {
                    self.should_quit = true;
                    Action::None
                }
                KeyCode::Char('r') => self.request_refresh(),
                KeyCode::Char('c') => self.copy_query(false),
                KeyCode::Char('j') => self.copy_query(true),
                KeyCode::Char('a') => self.toggle_select_all(false),
                KeyCode::Home => {
                    self.jump_to_end(false);
                    Action::None
                }
                KeyCode::End => {
                    self.jump_to_end(true);
                    Action::None
                }
                KeyCode::Char('k') => {
                    self.help_open = false;
                    self.modal = Some(Modal::new(&self.settings));
                    Action::None
                }
                KeyCode::Char('p') => {
                    self.help_open = false;
                    Action::OpenPicker
                }
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
        if let Some(found) = self.pending_import.take() {
            if matches!(key.code, KeyCode::Char('y' | 'Y')) {
                self.apply_import(found);
            }
            return Action::None;
        }
        if let Some(pending) = self.confirm.take() {
            return if matches!(key.code, KeyCode::Char('y' | 'Y')) {
                self.start_run(pending.request)
            } else {
                Action::None
            };
        }
        if self.is_editing() {
            self.handle_edit_key(key.code);
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
            return self.handle_design_key(key.code);
        }
        self.handle_search_key(key.code);
        Action::None
    }

    pub fn is_editing(&self) -> bool {
        self.design
            .as_ref()
            .is_some_and(|design| design.edit.is_some())
    }

    /// Enter on a variable: toggle a boolean, or open the editor its type needs.
    fn start_edit(&mut self) {
        let rows = self.variable_rows();
        let Some(design) = self.design.as_mut() else {
            return;
        };
        design.var_cursor = design.var_cursor.min(rows.len().saturating_sub(1));
        let Some(row) = rows.get(design.var_cursor) else {
            return;
        };
        match variables::editor_for(&self.schema, &row.def.type_ref) {
            Editor::Toggle => {
                let next = match row.raw.as_str() {
                    "true" => "false",
                    "false" if !row.def.required => "",
                    _ => "true",
                };
                set_value(design, &row.def.path, next);
            }
            Editor::Choice(names) => {
                let mut options = names;
                if !row.def.required {
                    options.insert(0, String::new());
                }
                let index = options
                    .iter()
                    .position(|option| *option == row.raw)
                    .unwrap_or(0);
                design.edit = Some(Edit::Choice { options, index });
            }
            Editor::Text | Editor::Json => design.edit = Some(Edit::Text(row.raw.clone())),
        }
    }

    /// Keys while a variable is being edited: Enter commits, Esc cancels.
    fn handle_edit_key(&mut self, code: KeyCode) {
        let rows = self.variable_rows();
        let Some(design) = self.design.as_mut() else {
            return;
        };
        let Some(mut edit) = design.edit.take() else {
            return;
        };
        let Some(row) = rows.get(design.var_cursor) else {
            return;
        };
        let finished = match (&mut edit, code) {
            (_, KeyCode::Esc) => true,
            (Edit::Text(buffer), KeyCode::Char(c)) => {
                buffer.push(c);
                false
            }
            (Edit::Text(buffer), KeyCode::Backspace) => {
                buffer.pop();
                false
            }
            (Edit::Text(buffer), KeyCode::Enter) => {
                set_value(design, &row.def.path, buffer);
                true
            }
            (Edit::Choice { options, index }, KeyCode::Down) => {
                *index = (*index + 1).min(options.len().saturating_sub(1));
                false
            }
            (Edit::Choice { index, .. }, KeyCode::Up) => {
                *index = index.saturating_sub(1);
                false
            }
            (Edit::Choice { options, index }, KeyCode::Enter) => {
                if let Some(option) = options.get(*index) {
                    set_value(design, &row.def.path, option);
                }
                true
            }
            _ => false,
        };
        if !finished {
            design.edit = Some(edit);
        }
    }

    fn request_refresh(&mut self) -> Action {
        if self.settings.uri.is_empty() {
            return self.notify("no endpoint: press Ctrl-K to set one");
        }
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
            KeyCode::PageDown => {
                self.hit_index = (self.hit_index + PAGE).min(self.hits.len().saturating_sub(1));
            }
            KeyCode::PageUp => self.hit_index = self.hit_index.saturating_sub(PAGE),
            KeyCode::Enter | KeyCode::Right => self.start_design(),
            _ => {}
        }
    }

    fn handle_design_key(&mut self, code: KeyCode) -> Action {
        if self.in_run_mode() {
            return self.handle_run_key(code);
        }
        match code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('r') => return self.request_run(),
            KeyCode::Esc => self.leave_design(),
            KeyCode::Tab => self.next_focus(),
            KeyCode::Up if self.focused(Focus::Variables) => self.move_var_cursor(-1),
            KeyCode::Down if self.focused(Focus::Variables) => self.move_var_cursor(1),
            KeyCode::Enter if self.focused(Focus::Variables) => self.start_edit(),
            _ if self
                .design
                .as_ref()
                .is_some_and(|design| design.focus != Focus::Details) => {}
            KeyCode::Char(' ') => self.toggle_under_cursor(),
            KeyCode::Down => self.move_cursor(1),
            KeyCode::Up => self.move_cursor(-1),
            KeyCode::PageDown => self.move_cursor(PAGE as isize),
            KeyCode::PageUp => self.move_cursor(-(PAGE as isize)),
            KeyCode::Right => self.expand_or_step_in(),
            KeyCode::Left => self.collapse_or_go_up(),
            _ => {}
        }
        Action::None
    }

    fn in_run_mode(&self) -> bool {
        self.design
            .as_ref()
            .is_some_and(|design| design.run.is_some())
    }

    fn handle_run_key(&mut self, code: KeyCode) -> Action {
        match code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('r') => return self.request_run(),
            KeyCode::Esc => {
                if let Some(design) = self.design.as_mut() {
                    design.run = None;
                    design.focus = Focus::Details;
                }
            }
            KeyCode::Tab => {
                if let Some(design) = self.design.as_mut() {
                    design.focus = if design.focus == Focus::Response {
                        Focus::Variables
                    } else {
                        Focus::Response
                    };
                }
            }
            KeyCode::Up if self.focused(Focus::Variables) => self.move_var_cursor(-1),
            KeyCode::Down if self.focused(Focus::Variables) => self.move_var_cursor(1),
            KeyCode::Enter if self.focused(Focus::Variables) => self.start_edit(),
            KeyCode::Up if self.focused(Focus::Response) => self.scroll_response(-1),
            KeyCode::Down if self.focused(Focus::Response) => self.scroll_response(1),
            KeyCode::PageUp if self.focused(Focus::Response) => self.scroll_response(-10),
            KeyCode::PageDown if self.focused(Focus::Response) => self.scroll_response(10),
            _ => {}
        }
        Action::None
    }

    fn scroll_response(&mut self, delta: isize) {
        let last = match self.design.as_ref().and_then(|design| design.run.as_ref()) {
            Some(RunView {
                state: RunState::Done(view),
                ..
            }) => view.body_lines().len().saturating_sub(1),
            _ => 0,
        };
        if let Some(run) = self.design.as_mut().and_then(|design| design.run.as_mut()) {
            run.scroll = run.scroll.saturating_add_signed(delta).min(last);
        }
    }

    /// `r`: check that the query can run, then send it (a mutation asks first).
    fn request_run(&mut self) -> Action {
        let Some(design) = self.design.as_ref() else {
            return Action::None;
        };
        let values = design.values.clone();
        let Some(operation) = design.root.operation().cloned() else {
            return self.notify("type trees cannot be run");
        };
        if operation.root == Root::Subscription {
            return self.notify("subscriptions cannot be run (they need WebSockets)");
        }
        if self.run_in_flight {
            return self.notify("a request is already running");
        }
        let query = match self.generated_query() {
            Some(Ok(query)) => query,
            _ => return self.notify("select at least one field to run"),
        };
        let variables = match variables::to_json(&self.schema, &query.variables, &values) {
            Ok(variables) => variables,
            Err(problem) => return self.notify(&format!("cannot run: variable {problem}")),
        };
        self.run_seq += 1;
        let request = RunRequest {
            seq: self.run_seq,
            body: client::operation_body(&query.operation_name, &query.text, &variables),
        };
        if operation.root == Root::Mutation {
            self.confirm = Some(PendingRun {
                request,
                name: query.operation_name,
            });
            return Action::None;
        }
        self.start_run(request)
    }

    /// Ctrl-C copies the query being built as a GraphQL document, Ctrl-J (`json`) as the
    /// JSON body of a request with the variables typed so far. Only while composing or in
    /// Run mode; with the Response pane focused Ctrl-C copies the response instead.
    fn copy_query(&mut self, json: bool) -> Action {
        let busy = self.help_open || self.confirm.is_some() || self.pending_import.is_some();
        if busy || self.is_editing() {
            return Action::None;
        }
        if self.focused(Focus::Response) {
            return if json {
                Action::None
            } else {
                self.copy_response()
            };
        }
        let query = match self.generated_query() {
            Some(Ok(query)) => query,
            Some(Err(_)) => return self.notify("nothing to copy: select at least one field"),
            None => return Action::None,
        };
        if !json {
            self.notice = Some("copied the query".to_string());
            return Action::Copy(query.text);
        }
        let values = self
            .design
            .as_ref()
            .map(|design| design.values.clone())
            .unwrap_or_default();
        let copy = export::request_json(&self.schema, &query, &values);
        self.notice = Some(match copy.missing {
            0 => "copied the query as JSON".to_string(),
            1 => "copied the query as JSON; 1 variable is unset or invalid, left out".to_string(),
            n => format!("copied the query as JSON; {n} variables are unset or invalid, left out"),
        });
        Action::Copy(copy.text)
    }

    /// Ctrl-A (without variables) and Alt-A (`with_variables`) in the Details pane: selects
    /// every field of the operation (see `selection::select_all`), or clears the fields
    /// when all of them are selected already.
    fn toggle_select_all(&mut self, with_variables: bool) -> Action {
        let busy = self.help_open || self.confirm.is_some() || self.pending_import.is_some();
        let Some(design) = self.design.as_mut() else {
            return Action::None;
        };
        if busy || design.edit.is_some() || design.run.is_some() {
            return Action::None;
        }
        if design.read_only() || design.focus != Focus::Details {
            return Action::None;
        }
        let all = selection::select_all(&self.schema, &design.root, with_variables);
        if all.selection.is_empty() {
            return self.notify("nothing to select");
        }
        if design.selection.has_all_of(&all.selection) {
            design.selection.remove_all_of(&all.selection);
            return self.notify("cleared the selection");
        }
        design.selection.extend_with(&all.selection);
        let count = design.selection.leaf_count();
        let args = design.selection.optional_args().count();
        let mut message = format!("selected {count} fields");
        match args {
            0 => {}
            1 => message.push_str(" and 1 optional argument"),
            n => message.push_str(&format!(" and {n} optional arguments")),
        }
        if all.capped {
            message.push_str("; the limit was reached, the deepest fields are left out");
        }
        self.notice = Some(message);
        Action::None
    }

    /// The response as the pane shows it (errors, then data), without the note about the
    /// output being capped. A capped response is copied capped, and the notice says so.
    fn copy_response(&mut self) -> Action {
        let state = self
            .design
            .as_ref()
            .and_then(|design| design.run.as_ref())
            .map(|run| &run.state);
        let Some(RunState::Done(view)) = state else {
            return self.notify("no response to copy yet");
        };
        let text = view
            .body_lines()
            .into_iter()
            .filter(|(kind, _)| *kind != LineKind::Note)
            .map(|(_, line)| line)
            .collect::<Vec<_>>()
            .join("\n");
        self.notice = Some(if view.total_lines > view.lines.len() {
            format!(
                "copied the response, capped: {} of {} lines",
                view.lines.len(),
                view.total_lines
            )
        } else {
            "copied the response".to_string()
        });
        Action::Copy(text)
    }

    /// A pasted query: opens its design, asking first when that would replace a selection.
    fn import_query(&mut self, text: &str) {
        let found = match import::import(&self.schema, text) {
            Ok(found) => found,
            Err(problem) => {
                self.notice = Some(format!("cannot import the pasted query: {problem}"));
                return;
            }
        };
        let key = TreeRoot::Operation(found.operation.clone()).key();
        let replaces = self
            .design
            .as_ref()
            .is_some_and(|design| !design.selection.is_empty())
            || self
                .sessions
                .get(&key)
                .is_some_and(|design| !design.selection.is_empty());
        if replaces {
            self.pending_import = Some(found);
        } else {
            self.apply_import(found);
        }
    }

    /// Replaces the open design with the imported one, expanded down to what is selected.
    fn apply_import(&mut self, found: Imported) {
        let label = found.operation.label();
        let root = TreeRoot::Operation(found.operation);
        self.sessions.remove(&root.key());
        let mut design = DesignState::new(root);
        for path in found
            .selection
            .leaves()
            .chain(found.selection.optional_args())
        {
            for end in 1..path.len() {
                design.expanded.insert(path[..end].to_vec());
            }
        }
        design.selection = found.selection;
        design.values = found.values;
        self.design = Some(design);
        self.notice = Some(match found.discarded {
            0 => format!("imported {label}"),
            1 => format!("imported {label}; 1 item not in the schema was discarded"),
            n => format!("imported {label}; {n} items not in the schema were discarded"),
        });
    }

    fn notify(&mut self, message: &str) -> Action {
        self.notice = Some(message.to_string());
        Action::None
    }

    fn start_run(&mut self, request: RunRequest) -> Action {
        self.run_in_flight = true;
        if let Some(design) = self.design.as_mut() {
            design.run = Some(RunView {
                state: RunState::Running,
                scroll: 0,
            });
            design.focus = Focus::Response;
        }
        Action::Run(request)
    }

    /// The result of request `seq`. It is shown only if Run mode is still open for that
    /// request; either way the one request slot is free again.
    pub fn apply_run_result(&mut self, seq: u64, outcome: Result<ResponseView, String>) {
        self.run_in_flight = false;
        if seq != self.run_seq {
            return;
        }
        if let Some(run) = self.design.as_mut().and_then(|design| design.run.as_mut()) {
            run.state = match outcome {
                Ok(view) => RunState::Done(view),
                Err(message) => RunState::Failed(message),
            };
            run.scroll = 0;
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
            Focus::Variables | Focus::Response => Focus::Details,
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
        let changed = rows
            .get(design.cursor)
            .is_some_and(|row| design.selection.toggle_node(&row.node));
        if changed {
            self.move_to_next_selectable();
        }
    }

    /// After Space: the cursor goes to the next field that can be selected. Collapsed
    /// objects on the way are opened (`args` groups are not), and when nothing further can
    /// be selected nothing moves and nothing opens.
    fn move_to_next_selectable(&mut self) {
        /// A bound for schemas with a great many nested objects and no field to select.
        const MAX_STEPS: usize = 10_000;
        let Some(design) = self.design.as_mut() else {
            return;
        };
        let mut expanded = design.expanded.clone();
        for index in (design.cursor + 1..).take(MAX_STEPS) {
            let rows = selection::rows(&self.schema, &design.root, &expanded);
            let Some(row) = rows.get(index) else {
                return;
            };
            match row.node.kind {
                NodeKind::Leaf | NodeKind::Optional => {
                    design.expanded = expanded;
                    design.cursor = index;
                    return;
                }
                NodeKind::Composite
                    if !row.expanded && row.node.path.last() != Some(&Seg::Args) =>
                {
                    expanded.insert(row.node.path.clone());
                }
                _ => {}
            }
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

    /// Ctrl-Home / Ctrl-End: selects the first or last row of the Results list or of the
    /// tree. Does nothing while a modal, the help or an edit has the keys, or when another
    /// pane has the focus.
    fn jump_to_end(&mut self, last: bool) {
        if self.help_open || self.is_editing() || self.in_run_mode() {
            return;
        }
        if self.design.is_none() {
            self.hit_index = if last {
                self.hits.len().saturating_sub(1)
            } else {
                0
            };
        } else if self.focused(Focus::Details) {
            let end = self.design_rows().len().saturating_sub(1);
            if let Some(design) = self.design.as_mut() {
                design.cursor = if last { end } else { 0 };
            }
        }
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
        if self.in_run_mode() {
            RUN_KEYS
        } else if self.design.is_some() {
            DESIGN_KEYS
        } else {
            BROWSE_KEYS
        }
    }

    pub fn apply_refresh_ok(&mut self, schema: Schema, fetched_at: u64) {
        self.loaded = true;
        self.schema = schema;
        self.fetched_at = fetched_at;
        self.design = None;
        self.confirm = None;
        self.pending_import = None;
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
        if let Some(pending) = &self.confirm {
            return (
                format!("Run mutation {}? y/N", pending.name),
                StatusLevel::Notice,
            );
        }
        if self.pending_import.is_some() {
            return (
                "Replace the current selection with the pasted query? y/N".to_string(),
                StatusLevel::Notice,
            );
        }
        if let Some(notice) = &self.notice {
            return (notice.clone(), StatusLevel::Notice);
        }
        if !self.loaded {
            return self.status_without_schema();
        }
        let age = now.saturating_sub(self.fetched_at);
        let level = if is_stale(age) {
            StatusLevel::Stale
        } else {
            StatusLevel::Fresh
        };
        let mut text = format!(
            "{} · schema fetched {} ago",
            self.server_label(),
            format_age(age)
        );
        match &self.refresh {
            RefreshState::Idle => {
                text.push_str(" · Ctrl-R refresh · Ctrl-K connection · Ctrl-P profiles")
            }
            RefreshState::Running => text.push_str(" · refreshing…"),
            RefreshState::CacheWriteFailed(message) => {
                let first_line = message.lines().next().unwrap_or("");
                text.push_str(&format!(
                    " · schema updated but the cache could not be written: {first_line} · Ctrl-R refresh · Ctrl-K connection · Ctrl-P profiles"
                ));
            }
            RefreshState::Failed(message) => {
                let first_line = message.lines().next().unwrap_or("");
                text.push_str(&format!(
                    " · Ctrl-R retry · Ctrl-K connection · Ctrl-P profiles · refresh failed: {first_line}"
                ));
            }
        }
        (text, level)
    }

    /// The status bar while no schema is loaded for the endpoint.
    fn status_without_schema(&self) -> (String, StatusLevel) {
        if self.settings.uri.is_empty() {
            return (
                "no endpoint · Ctrl-K connection · Ctrl-P profiles".to_string(),
                StatusLevel::Notice,
            );
        }
        let text = match &self.refresh {
            RefreshState::Running => format!("{} · loading the schema…", self.server_label()),
            RefreshState::Failed(message) => {
                let first_line = message.lines().next().unwrap_or("");
                format!(
                    "{} · Ctrl-K connection · Ctrl-P profiles · Ctrl-R retry · could not load the schema: {first_line}",
                    self.server_label()
                )
            }
            _ => format!(
                "{} · no schema loaded · Ctrl-K connection · Ctrl-P profiles · Ctrl-R load",
                self.server_label()
            ),
        };
        (text, StatusLevel::Notice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::time::Duration;
    use vega_core::client::Response;
    use vega_core::config::Settings;
    use vega_core::response;
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

    fn entry(name: &str) -> crate::tui::profiles::Entry {
        crate::tui::profiles::Entry {
            name: name.to_string(),
            uri: format!("https://{name}/graphql"),
            kind: vega_core::profile::Kind::NoAuth,
        }
    }

    #[test]
    fn ctrl_p_asks_the_loop_to_list_the_profiles() {
        let mut app = app();
        assert_eq!(ctrl(&mut app, 'p'), Action::OpenPicker);
    }

    #[test]
    fn the_picker_routes_its_outcomes_to_actions() {
        let mut app = app();
        app.open_picker(vec![entry("a")]);
        assert_eq!(
            press(&mut app, KeyCode::Enter),
            Action::ConnectProfile("a".into())
        );
        assert_eq!(
            press(&mut app, KeyCode::Char('e')),
            Action::EditProfile("a".into())
        );
        press(&mut app, KeyCode::Char('d'));
        assert_eq!(
            press(&mut app, KeyCode::Char('y')),
            Action::DeleteProfile("a".into())
        );
        assert_eq!(press(&mut app, KeyCode::Esc), Action::None);
        assert!(app.picker.is_none());
    }

    #[test]
    fn n_in_the_picker_opens_an_empty_connection_modal() {
        let mut app = app();
        app.open_picker(vec![]);
        press(&mut app, KeyCode::Char('n'));
        assert!(app.picker.is_none());
        assert_eq!(app.modal.as_ref().unwrap().uri().display(), "");
    }

    #[test]
    fn connecting_to_a_profile_loads_its_schema_and_names_it_in_the_status() {
        let mut app = app();
        app.open_picker(vec![entry("a")]);
        let action = app.connect_profile("a".into(), settings("https://other/graphql"));
        assert_eq!(action, Action::Load);
        assert!(app.picker.is_none());
        assert!(app.status(0).0.contains("a (other)"), "{}", app.status(0).0);
    }

    #[test]
    fn editing_a_profile_opens_the_modal_with_its_name() {
        let mut app = app();
        app.open_picker(vec![entry("a")]);
        app.edit_profile("a".into(), settings("https://x/graphql"));
        assert!(app.picker.is_none());
        assert_eq!(app.modal.as_ref().unwrap().name().display(), "a");
    }

    #[test]
    fn the_status_bar_names_the_profiles_key() {
        assert!(app().status(1_000).0.contains("Ctrl-P profiles"));
        assert_eq!(
            empty_app().status(0).0,
            "no endpoint · Ctrl-K connection · Ctrl-P profiles"
        );
    }

    #[test]
    fn a_paste_while_the_picker_is_open_goes_nowhere() {
        let mut app = app();
        app.open_picker(vec![entry("a")]);
        app.handle_paste("abc");
        assert_eq!(app.query, "");
        assert!(app.picker.is_some());
    }

    #[test]
    fn every_help_table_lists_ctrl_p() {
        for keys in [BROWSE_KEYS, DESIGN_KEYS, RUN_KEYS] {
            assert!(keys.iter().any(|(key, _)| *key == "Ctrl-P"));
        }
    }

    fn type_text(app: &mut App, text: &str) {
        for c in text.chars() {
            press(app, KeyCode::Char(c));
        }
    }

    fn settings(uri: &str) -> Settings {
        Settings {
            uri: uri.to_string(),
            headers: Vec::new(),
            key_only: false,
        }
    }

    fn empty_app() -> App {
        App::empty(settings(""))
    }

    /// Opens the modal, picks Bearer, types `token` and presses Enter.
    fn submit_bearer(app: &mut App, token: &str) -> Action {
        ctrl(app, 'k');
        press(app, KeyCode::Down); // Basic
        press(app, KeyCode::Down); // API Key
        press(app, KeyCode::Down); // Bearer
        press(app, KeyCode::Tab); // the token field
        type_text(app, token);
        press(app, KeyCode::Enter)
    }

    fn press_ctrl(app: &mut App, code: KeyCode) {
        app.handle_key(KeyEvent::new(code, KeyModifiers::CONTROL));
    }

    #[test]
    fn ctrl_home_and_end_jump_to_the_ends_of_the_results() {
        let mut app = app();
        app.hits = (0..25)
            .map(|n| vega_core::search::Hit {
                kind: vega_core::search::HitKind::Query,
                path: format!("f{n}"),
                summary: String::new(),
            })
            .collect();
        press_ctrl(&mut app, KeyCode::End);
        assert_eq!(app.hit_index, 24);
        press_ctrl(&mut app, KeyCode::Home);
        assert_eq!(app.hit_index, 0);
    }

    #[test]
    fn page_keys_and_ctrl_home_end_move_the_tree_cursor() {
        let mut app = design_app();
        open_me(&mut app);
        let last = app.design_rows().len() - 1;
        press(&mut app, KeyCode::PageDown);
        assert_eq!(app.design.as_ref().unwrap().cursor, last.min(10));
        press_ctrl(&mut app, KeyCode::End);
        assert_eq!(app.design.as_ref().unwrap().cursor, last);
        press(&mut app, KeyCode::PageUp);
        assert_eq!(app.design.as_ref().unwrap().cursor, last.saturating_sub(10));
        press_ctrl(&mut app, KeyCode::Home);
        assert_eq!(app.design.as_ref().unwrap().cursor, 0);
    }

    #[test]
    fn page_keys_move_ten_results_and_stop_at_the_ends() {
        let mut app = app();
        app.hits = (0..25)
            .map(|n| vega_core::search::Hit {
                kind: vega_core::search::HitKind::Query,
                path: format!("f{n}"),
                summary: String::new(),
            })
            .collect();
        press(&mut app, KeyCode::PageDown);
        assert_eq!(app.hit_index, 10);
        press(&mut app, KeyCode::PageDown);
        press(&mut app, KeyCode::PageDown);
        assert_eq!(app.hit_index, 24);
        press(&mut app, KeyCode::PageUp);
        assert_eq!(app.hit_index, 14);
        press(&mut app, KeyCode::PageUp);
        press(&mut app, KeyCode::PageUp);
        assert_eq!(app.hit_index, 0);
    }

    #[test]
    fn list_offset_keeps_the_selection_in_the_middle() {
        // 100 items, 10 rows: the selection sits on the sixth row.
        assert_eq!(list_offset(100, 50, 10), 45);
        assert_eq!(list_offset(100, 51, 10), 46);
    }

    #[test]
    fn list_offset_stops_one_empty_line_after_the_end() {
        // The last item is the 99th row of 100; one blank row follows it.
        assert_eq!(list_offset(100, 99, 10), 91);
        assert_eq!(list_offset(100, 96, 10), 91);
    }

    #[test]
    fn list_offset_stays_at_the_top_near_the_beginning() {
        assert_eq!(list_offset(100, 0, 10), 0);
        assert_eq!(list_offset(100, 5, 10), 0);
        assert_eq!(list_offset(100, 6, 10), 1);
    }

    #[test]
    fn list_offset_is_zero_when_the_list_fits() {
        assert_eq!(list_offset(5, 4, 10), 0);
        assert_eq!(list_offset(0, 0, 10), 0);
        assert_eq!(list_offset(9, 8, 10), 0);
    }

    #[test]
    fn list_offset_follows_a_resize() {
        assert_eq!(list_offset(100, 50, 10), 45);
        assert_eq!(list_offset(100, 50, 20), 40);
        assert_eq!(list_offset(100, 99, 20), 81);
    }

    #[test]
    fn an_app_without_an_endpoint_opens_the_modal_and_says_so() {
        let app = empty_app();
        assert!(app.modal.is_some());
        assert!(!app.is_loaded());
        assert_eq!(
            app.status(0).0,
            "no endpoint · Ctrl-K connection · Ctrl-P profiles"
        );
        assert!(app.hits.is_empty());
    }

    #[test]
    fn esc_closes_the_startup_modal_and_ctrl_r_points_to_ctrl_k() {
        let mut app = empty_app();
        press(&mut app, KeyCode::Esc);
        assert!(app.modal.is_none());
        assert_eq!(ctrl(&mut app, 'r'), Action::None);
        assert_eq!(app.status(0).0, "no endpoint: press Ctrl-K to set one");
        assert_eq!(app.refresh, RefreshState::Idle);
    }

    #[test]
    fn saving_from_the_modal_asks_the_loop_to_save_and_keeps_the_modal_open() {
        let mut app = app();
        ctrl(&mut app, 'k');
        press(&mut app, KeyCode::BackTab);
        type_text(&mut app, "staging");
        let action = ctrl(&mut app, 's');
        assert!(matches!(&action, Action::SaveProfile { name, .. } if name == "staging"));
        assert!(app.modal.is_some());
    }

    #[test]
    fn a_saved_profile_becomes_the_current_connection() {
        let mut app = app();
        ctrl(&mut app, 'k');
        let settings = settings("https://other/graphql");
        let action = app.profile_saved("staging".to_string(), settings.clone());
        assert!(app.modal.is_none());
        assert_eq!(app.settings(), &settings);
        assert_eq!(action, Action::Load);
        assert!(app.status(0).0.contains("staging"));
    }

    #[test]
    fn a_failed_save_keeps_the_modal_and_shows_the_reason() {
        let mut app = app();
        ctrl(&mut app, 'k');
        app.profile_save_failed("no keyring is available on this system".to_string());
        assert!(app
            .modal
            .as_ref()
            .unwrap()
            .error()
            .unwrap()
            .contains("no keyring"));
    }

    #[test]
    fn connecting_through_the_modal_forgets_the_profile_name() {
        let mut app = app();
        app.profile_saved("staging".to_string(), settings("https://x/graphql"));
        submit_bearer(&mut app, "t");
        assert!(!app.status(0).0.contains("staging"));
    }

    #[test]
    fn ctrl_k_opens_the_modal_and_esc_cancels_without_changes() {
        let mut app = app();
        ctrl(&mut app, 'k');
        assert!(app.modal.is_some());
        type_text(&mut app, "zzz");
        assert_eq!(app.query, "");
        press(&mut app, KeyCode::Esc);
        assert!(app.modal.is_none());
        assert_eq!(app.settings().uri, "https://nexus.example.com/graphql");
    }

    #[test]
    fn ctrl_k_works_from_design_mode_and_closes_help() {
        let mut app = design_app();
        open_me(&mut app);
        press(&mut app, KeyCode::F(1));
        ctrl(&mut app, 'k');
        assert!(app.modal.is_some());
        assert!(!app.help_open);
        assert!(app.design.is_some());
    }

    #[test]
    fn ctrl_q_quits_even_with_the_modal_open() {
        let mut app = app();
        ctrl(&mut app, 'k');
        ctrl(&mut app, 'q');
        assert!(app.should_quit);
    }

    #[test]
    fn submitting_only_new_headers_keeps_the_schema_and_the_work() {
        let mut app = design_app();
        open_me(&mut app);
        assert_eq!(submit_bearer(&mut app, "tok-1"), Action::None);
        assert!(app.modal.is_none());
        assert!(app.design.is_some());
        assert!(app.is_loaded());
        assert_eq!(app.settings().headers.len(), 1);
        assert_eq!(app.settings().headers[0].value(), "Bearer tok-1");
    }

    #[test]
    fn submitting_headers_without_a_schema_retries_the_load() {
        let mut app = App::empty(settings("https://x.example.com/graphql"));
        app.apply_refresh_err("server answered HTTP 401".to_string());
        assert!(app
            .status(0)
            .0
            .contains("could not load the schema: server answered HTTP 401"));
        assert_eq!(submit_bearer(&mut app, "tok-1"), Action::Refresh);
        assert_eq!(app.refresh, RefreshState::Running);
        assert!(app.status(0).0.contains("loading the schema"));
    }

    #[test]
    fn changing_the_uri_starts_a_new_connection() {
        let mut app = design_app();
        open_me(&mut app);
        ctrl(&mut app, 'k');
        press(&mut app, KeyCode::BackTab); // from the type list to Save as
        press(&mut app, KeyCode::BackTab); // and on to the URI
        type_text(&mut app, "/v2");
        assert_eq!(press(&mut app, KeyCode::Enter), Action::Load);
        assert_eq!(app.settings().uri, "https://example.com/graphql/v2");
        assert!(app.design.is_none());
        assert!(!app.is_loaded());
        assert!(app.hits.is_empty());
        assert_eq!(app.query, "");
        assert!(app.status(0).0.contains("loading the schema"));
    }

    #[test]
    fn a_loaded_schema_after_a_new_connection_makes_the_app_usable_again() {
        let mut app = empty_app();
        type_text(&mut app, "https://x/graphql");
        assert_eq!(press(&mut app, KeyCode::Enter), Action::Load);
        let response: IntrospectionResponse = serde_json::from_str(FIXTURE).unwrap();
        app.apply_refresh_ok(response.data.schema, 5_000);
        assert!(app.is_loaded());
        assert_eq!(app.hits.len(), 10);
        assert!(app.status(5_000).0.starts_with("x ·"));
    }

    #[test]
    fn paste_types_into_the_search_box() {
        let mut app = app();
        app.handle_paste("user\r\n");
        assert_eq!(app.query, "user");
    }

    #[test]
    fn paste_goes_to_the_modal_when_it_is_open() {
        let mut app = app();
        ctrl(&mut app, 'k');
        press(&mut app, KeyCode::Down); // Basic
        press(&mut app, KeyCode::Down); // API Key
        press(&mut app, KeyCode::Down); // Bearer
        press(&mut app, KeyCode::Tab);
        app.handle_paste("tok-9\n");
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.settings().headers[0].value(), "Bearer tok-9");
        assert_eq!(app.query, "");
    }

    #[test]
    fn paste_never_triggers_commands_in_design_mode() {
        let mut app = design_app();
        open_me(&mut app);
        app.handle_paste("rq");
        assert!(!app.should_quit);
        assert!(app
            .design
            .as_ref()
            .is_some_and(|design| design.run.is_none()));
    }

    #[test]
    fn paste_keeps_question_marks_in_the_search_box() {
        let mut app = app();
        app.handle_paste("a?b");
        assert_eq!(app.query, "a?b");
        assert!(!app.help_open);
    }

    #[test]
    fn paste_keeps_question_marks_in_a_text_variable_editor() {
        let mut app = design_app();
        node_variables(&mut app);
        press(&mut app, KeyCode::Enter);
        app.handle_paste("1?2");
        press(&mut app, KeyCode::Enter);
        assert_eq!(raw_values(&app), ["1?2"]);
        assert!(!app.help_open);
    }

    #[test]
    fn paste_is_ignored_while_help_is_open() {
        let mut app = app();
        press(&mut app, KeyCode::F(1));
        app.handle_paste("abc");
        assert_eq!(app.query, "");
    }

    #[test]
    fn every_help_table_lists_ctrl_k() {
        for keys in [BROWSE_KEYS, DESIGN_KEYS, RUN_KEYS] {
            assert!(keys.iter().any(|(key, _)| *key == "Ctrl-K"));
        }
    }

    #[test]
    fn the_status_bar_hints_at_ctrl_k() {
        assert!(app().status(1_000).0.contains("Ctrl-K"));
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
        press(&mut app, KeyCode::Char(' ')); // the cursor moves on to name
        assert_eq!(query_text(&app), "query Me {\n  me {\n    id\n  }\n}");
        assert!(app.is_selected(&[Seg::Field("id".to_string())]));

        press(&mut app, KeyCode::Char(' ')); // name
        assert_eq!(
            query_text(&app),
            "query Me {\n  me {\n    id\n    name\n  }\n}"
        );

        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Char(' ')); // unselect id, and move on all the same
        assert_eq!(query_text(&app), "query Me {\n  me {\n    name\n  }\n}");
        assert_eq!(app.design.as_ref().unwrap().cursor, 2);
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
        press(&mut app, KeyCode::Char(' ')); // id, the cursor moves on to name (2)
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
        let first = app.design.as_ref().unwrap().cursor;
        press(&mut app, KeyCode::Char(' '));
        assert!(app.is_selected(&[Seg::Args, Seg::Arg("first".to_string())]));
        app.design.as_mut().unwrap().cursor = first; // Space moved on: come back
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
        press(&mut app, KeyCode::Char(' ')); // id, the cursor moves on to name
        press(&mut app, KeyCode::Up);
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
        press(&mut app, KeyCode::Char(' ')); // id, the cursor moves on to name
        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Right);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' ')); // role, the cursor moves on to active
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

    /// Opens Query.users, selects `id` and the optional arguments `role`, `active` and
    /// `floor`, and focuses the Variables pane (variables: role, active, floor).
    fn users_variables(app: &mut App) {
        type_text(app, "users");
        assert_eq!(app.hits[0].path, "Query.users");
        press(app, KeyCode::Enter);
        press(app, KeyCode::Down);
        press(app, KeyCode::Down);
        press(app, KeyCode::Char(' ')); // id, the cursor moves on to name
        press(app, KeyCode::Up);
        press(app, KeyCode::Up);
        press(app, KeyCode::Right); // expand args
        press(app, KeyCode::Down);
        for _ in 0..3 {
            press(app, KeyCode::Char(' ')); // role, active, floor: each moves on
        }
        press(app, KeyCode::Tab);
        press(app, KeyCode::Tab);
    }

    fn raw_values(app: &App) -> Vec<String> {
        app.variable_rows().into_iter().map(|row| row.raw).collect()
    }

    #[test]
    fn typing_a_value_and_pressing_enter_stores_it() {
        use vega_core::variables::State;
        let mut app = design_app();
        node_variables(&mut app);
        press(&mut app, KeyCode::Enter);
        assert!(app.is_editing());
        type_text(&mut app, "42");
        press(&mut app, KeyCode::Backspace);
        type_text(&mut app, "3");
        press(&mut app, KeyCode::Enter);
        assert!(!app.is_editing());
        let rows = app.variable_rows();
        assert_eq!(rows[0].raw, "43");
        assert_eq!(rows[0].state, State::Value(serde_json::json!("43")));
    }

    #[test]
    fn esc_cancels_an_edit_and_keeps_the_old_value() {
        let mut app = design_app();
        node_variables(&mut app);
        press(&mut app, KeyCode::Enter);
        type_text(&mut app, "42");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Enter);
        type_text(&mut app, "99");
        press(&mut app, KeyCode::Esc);
        assert!(!app.is_editing());
        assert!(app.design.is_some(), "Esc only cancels the edit");
        assert_eq!(raw_values(&app), ["42"]);
    }

    #[test]
    fn committing_empty_text_removes_the_value() {
        let mut app = design_app();
        node_variables(&mut app);
        press(&mut app, KeyCode::Enter);
        type_text(&mut app, "7");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Backspace);
        press(&mut app, KeyCode::Enter);
        assert_eq!(raw_values(&app), [""]);
    }

    #[test]
    fn every_key_is_text_while_editing_but_ctrl_q_still_quits() {
        let mut app = design_app();
        node_variables(&mut app);
        press(&mut app, KeyCode::Enter);
        type_text(&mut app, "qh?");
        assert!(!app.should_quit);
        assert!(!app.help_open);
        press(&mut app, KeyCode::Enter);
        assert_eq!(raw_values(&app), ["qh?"]);

        press(&mut app, KeyCode::Enter);
        ctrl(&mut app, 'q');
        assert!(app.should_quit);
    }

    #[test]
    fn booleans_cycle_with_enter_and_optional_ones_return_to_empty() {
        let mut app = design_app();
        users_variables(&mut app);
        press(&mut app, KeyCode::Down); // active
        let mut seen = Vec::new();
        for _ in 0..4 {
            press(&mut app, KeyCode::Enter);
            assert!(!app.is_editing());
            seen.push(raw_values(&app)[1].clone());
        }
        assert_eq!(seen, ["true", "false", "", "true"]);
    }

    #[test]
    fn enums_are_picked_from_a_list_with_an_unset_option() {
        let mut app = design_app();
        users_variables(&mut app); // cursor on role
        press(&mut app, KeyCode::Enter);
        assert!(app.is_editing());
        press(&mut app, KeyCode::Down); // ADMIN
        press(&mut app, KeyCode::Down); // VIEWER
        press(&mut app, KeyCode::Down); // clamped
        press(&mut app, KeyCode::Up); // ADMIN
        press(&mut app, KeyCode::Enter);
        assert_eq!(raw_values(&app)[0], "ADMIN");

        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Up); // (unset)
        press(&mut app, KeyCode::Enter);
        assert_eq!(raw_values(&app)[0], "");

        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Esc);
        assert_eq!(raw_values(&app)[0], "");
    }

    #[test]
    fn invalid_values_are_reported_per_variable() {
        use vega_core::variables::State;
        let mut app = design_app();
        users_variables(&mut app);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down); // floor
        press(&mut app, KeyCode::Enter);
        type_text(&mut app, "abc");
        press(&mut app, KeyCode::Enter);
        assert_eq!(
            app.variable_rows()[2].state,
            State::Invalid("expected a number".to_string())
        );
    }

    #[test]
    fn values_survive_leaving_and_reentering_and_a_refresh_clears_them() {
        let mut app = design_app();
        node_variables(&mut app);
        press(&mut app, KeyCode::Enter);
        type_text(&mut app, "42");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Esc);
        assert!(app.design.is_none());
        press(&mut app, KeyCode::Enter);
        assert_eq!(raw_values(&app), ["42"]);

        press(&mut app, KeyCode::Esc);
        let response: IntrospectionResponse = serde_json::from_str(DESIGN_FIXTURE).unwrap();
        app.apply_refresh_ok(response.data.schema, 5_000);
        press(&mut app, KeyCode::Enter);
        assert!(app.variable_rows().is_empty());
    }

    #[test]
    fn help_in_design_mentions_editing() {
        let mut app = design_app();
        open_me(&mut app);
        assert!(app.help_keys().iter().any(|(keys, _)| *keys == "Enter"));
    }

    fn view_of(status: u16, body: &str) -> response::ResponseView {
        response::view(&Response {
            status,
            elapsed: Duration::from_millis(5),
            body: body.to_string(),
        })
    }

    /// Opens Query.me with `id` selected.
    fn me_with_id(app: &mut App) {
        open_me(app);
        press(app, KeyCode::Down);
        press(app, KeyCode::Char(' '));
    }

    fn run_request(action: Action) -> RunRequest {
        match action {
            Action::Run(request) => request,
            other => panic!("expected Action::Run, got {other:?}"),
        }
    }

    #[test]
    fn r_sends_the_generated_query_and_enters_run_mode() {
        let mut app = design_app();
        me_with_id(&mut app);
        let request = run_request(press(&mut app, KeyCode::Char('r')));
        assert_eq!(request.seq, 1);
        assert_eq!(request.body["operationName"], "Me");
        assert_eq!(request.body["query"], "query Me {\n  me {\n    id\n  }\n}");
        assert_eq!(request.body["variables"], serde_json::json!({}));
        let design = app.design.as_ref().unwrap();
        assert_eq!(design.run.as_ref().unwrap().state, RunState::Running);
        assert_eq!(design.focus, Focus::Response);
    }

    #[test]
    fn r_uses_the_typed_variable_values() {
        let mut app = design_app();
        node_variables(&mut app);
        press(&mut app, KeyCode::Enter);
        type_text(&mut app, "42");
        press(&mut app, KeyCode::Enter);
        let request = run_request(press(&mut app, KeyCode::Char('r')));
        assert_eq!(request.body["variables"], serde_json::json!({"id": "42"}));
    }

    #[test]
    fn r_is_text_while_a_variable_is_being_edited() {
        let mut app = design_app();
        node_variables(&mut app);
        press(&mut app, KeyCode::Enter);
        assert_eq!(press(&mut app, KeyCode::Char('r')), Action::None);
        assert!(app.is_editing());
    }

    #[test]
    fn r_explains_why_it_cannot_run() {
        let mut app = design_app();
        open_me(&mut app);
        assert_eq!(press(&mut app, KeyCode::Char('r')), Action::None);
        assert!(app
            .status(1_000)
            .0
            .contains("select at least one field to run"));
        assert_eq!(app.status(1_000).1, StatusLevel::Notice);
        press(&mut app, KeyCode::Down); // any key clears the notice
        assert!(!app.status(1_000).0.contains("select at least one field"));

        let mut app = design_app();
        type_text(&mut app, "node");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' ')); // id, but $id is empty
        assert_eq!(press(&mut app, KeyCode::Char('r')), Action::None);
        assert!(app
            .status(1_000)
            .0
            .contains("cannot run: variable $id: required"));
        assert!(app.design.as_ref().unwrap().run.is_none());

        let mut app = design_app();
        type_text(&mut app, "postinput");
        press(&mut app, KeyCode::Enter);
        assert_eq!(press(&mut app, KeyCode::Char('r')), Action::None);
        assert!(app.status(1_000).0.contains("type trees cannot be run"));
    }

    #[test]
    fn only_one_request_runs_at_a_time() {
        let mut app = design_app();
        me_with_id(&mut app);
        run_request(press(&mut app, KeyCode::Char('r')));
        assert_eq!(press(&mut app, KeyCode::Char('r')), Action::None);
        assert!(app.status(1_000).0.contains("a request is already running"));

        app.apply_run_result(1, Ok(view_of(200, r#"{"data":{"me":null}}"#)));
        let again = run_request(press(&mut app, KeyCode::Char('r')));
        assert_eq!(again.seq, 2);
    }

    #[test]
    fn a_mutation_asks_for_confirmation_first() {
        let mut app = design_app();
        type_text(&mut app, "postCreate");
        assert_eq!(app.hits[0].path, "Mutation.postCreate");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' ')); // id
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Enter);
        type_text(&mut app, r#"{"title":"x"}"#);
        press(&mut app, KeyCode::Enter);

        assert_eq!(press(&mut app, KeyCode::Char('r')), Action::None);
        assert_eq!(app.status(1_000).0, "Run mutation PostCreate? y/N");
        assert_eq!(press(&mut app, KeyCode::Char('n')), Action::None);
        assert!(app.design.as_ref().unwrap().run.is_none());
        assert!(!app.status(1_000).0.contains("Run mutation"));

        press(&mut app, KeyCode::Char('r'));
        let request = run_request(press(&mut app, KeyCode::Char('y')));
        assert_eq!(request.body["operationName"], "PostCreate");
        assert_eq!(
            request.body["variables"],
            serde_json::json!({"input": {"title": "x"}})
        );
    }

    #[test]
    fn results_fill_the_response_and_stale_ones_are_discarded() {
        let mut app = design_app();
        me_with_id(&mut app);
        run_request(press(&mut app, KeyCode::Char('r')));
        app.apply_run_result(1, Ok(view_of(200, r#"{"data":{"me":{"id":"1"}}}"#)));
        let run = app.design.as_ref().unwrap().run.as_ref().unwrap();
        assert!(matches!(&run.state, RunState::Done(view) if view.status == 200));

        // A result for a request that was left behind changes nothing.
        press(&mut app, KeyCode::Char('r'));
        press(&mut app, KeyCode::Esc); // back to Design while request 2 runs
        assert!(app.design.as_ref().unwrap().run.is_none());
        app.apply_run_result(2, Ok(view_of(200, "{}")));
        assert!(app.design.as_ref().unwrap().run.is_none());
        // ...but it freed the "in flight" slot.
        run_request(press(&mut app, KeyCode::Char('r')));
        app.apply_run_result(3, Err("request failed".to_string()));
        let run = app.design.as_ref().unwrap().run.as_ref().unwrap();
        assert_eq!(run.state, RunState::Failed("request failed".to_string()));
    }

    #[test]
    fn run_mode_keys_scroll_switch_panes_and_go_back() {
        let mut app = design_app();
        me_with_id(&mut app);
        run_request(press(&mut app, KeyCode::Char('r')));
        let numbers: Vec<String> = (0..50).map(|n| n.to_string()).collect();
        let body = format!(r#"{{"data":[{}]}}"#, numbers.join(","));
        app.apply_run_result(1, Ok(view_of(200, &body)));

        let scroll = |app: &App| app.design.as_ref().unwrap().run.as_ref().unwrap().scroll;
        press(&mut app, KeyCode::Up);
        assert_eq!(scroll(&app), 0);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        assert_eq!(scroll(&app), 2);
        press(&mut app, KeyCode::PageDown);
        assert_eq!(scroll(&app), 12);
        press(&mut app, KeyCode::PageUp);
        assert_eq!(scroll(&app), 2);
        for _ in 0..10 {
            press(&mut app, KeyCode::PageDown);
        }
        assert_eq!(scroll(&app), 51); // 52 lines: the last line stays visible

        // Tab switches between Response and Variables; the arrows then move variables.
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.design.as_ref().unwrap().focus, Focus::Variables);
        press(&mut app, KeyCode::Down);
        assert_eq!(scroll(&app), 51);
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.design.as_ref().unwrap().focus, Focus::Response);

        // Esc returns to Design with the selection intact.
        press(&mut app, KeyCode::Esc);
        let design = app.design.as_ref().unwrap();
        assert!(design.run.is_none());
        assert_eq!(design.focus, Focus::Details);
        assert!(app.is_selected(&[Seg::Field("id".to_string())]));
    }

    #[test]
    fn q_quits_in_run_mode_and_help_lists_the_run_keys() {
        let mut app = design_app();
        me_with_id(&mut app);
        run_request(press(&mut app, KeyCode::Char('r')));
        assert!(app
            .help_keys()
            .iter()
            .any(|(keys, _)| keys.contains("PgDn")));
        press(&mut app, KeyCode::Char('q'));
        assert!(app.should_quit);

        let mut app = design_app();
        open_me(&mut app);
        assert!(app.help_keys().iter().any(|(keys, _)| *keys == "r"));
    }

    #[test]
    fn a_refresh_drops_run_mode_and_a_late_result() {
        let mut app = design_app();
        me_with_id(&mut app);
        run_request(press(&mut app, KeyCode::Char('r')));
        let response: IntrospectionResponse = serde_json::from_str(DESIGN_FIXTURE).unwrap();
        app.apply_refresh_ok(response.data.schema, 5_000);
        assert!(app.design.is_none());
        app.apply_run_result(1, Ok(view_of(200, "{}")));
        assert!(app.design.is_none());
    }

    fn copied(action: Action) -> serde_json::Value {
        match action {
            Action::Copy(text) => serde_json::from_str(&text).expect("a JSON body"),
            other => panic!("expected Action::Copy, got {other:?}"),
        }
    }

    #[test]
    fn ctrl_j_copies_the_query_with_the_variables_typed_so_far() {
        let mut app = design_app();
        node_variables(&mut app);
        press(&mut app, KeyCode::Enter);
        type_text(&mut app, "42");
        press(&mut app, KeyCode::Enter);
        let body = copied(ctrl(&mut app, 'j'));
        assert_eq!(
            body["query"],
            "query Node($id: ID!) {\n  node(id: $id) {\n    __typename\n    id\n  }\n}"
        );
        assert_eq!(body["variables"], serde_json::json!({"id": "42"}));
        assert_eq!(app.status(1_000).0, "copied the query as JSON");
    }

    #[test]
    fn ctrl_c_copies_the_query_as_plain_graphql_with_real_newlines() {
        let mut app = design_app();
        node_variables(&mut app);
        press(&mut app, KeyCode::Enter);
        type_text(&mut app, "42");
        press(&mut app, KeyCode::Enter);
        match ctrl(&mut app, 'c') {
            Action::Copy(text) => assert_eq!(
                text,
                "query Node($id: ID!) {\n  node(id: $id) {\n    __typename\n    id\n  }\n}"
            ),
            other => panic!("expected Action::Copy, got {other:?}"),
        }
        assert_eq!(app.status(1_000).0, "copied the query");
    }

    #[test]
    fn a_plain_copy_pastes_back_as_the_same_design() {
        let mut app = design_app();
        open_me(&mut app);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' '));
        let Action::Copy(text) = ctrl(&mut app, 'c') else {
            panic!("expected a copy");
        };
        let mut other = design_app();
        other.handle_paste(&text);
        assert_eq!(query_text(&other), query_text(&app));
    }

    #[test]
    fn ctrl_j_copies_while_composing_and_says_what_was_left_out() {
        let mut app = design_app();
        node_variables(&mut app);
        press(&mut app, KeyCode::Tab); // back to Details
        let body = copied(ctrl(&mut app, 'j'));
        assert!(body.get("variables").is_none());
        assert!(app.status(1_000).0.contains("1 variable is unset"));
    }

    #[test]
    fn ctrl_c_and_ctrl_j_with_nothing_selected_explain_themselves() {
        let mut app = design_app();
        open_me(&mut app);
        for key in ['c', 'j'] {
            assert_eq!(ctrl(&mut app, key), Action::None);
            assert!(app.status(1_000).0.contains("nothing to copy"));
            press(&mut app, KeyCode::Down); // any key clears the notice
            press(&mut app, KeyCode::Up);
        }
    }

    #[test]
    fn the_copy_keys_do_nothing_in_browse_on_a_type_tree_or_while_editing() {
        let mut app = design_app();
        assert_eq!(ctrl(&mut app, 'j'), Action::None);
        assert!(!app.status(1_000).0.contains("copy"));

        let mut app = design_app();
        type_text(&mut app, "user");
        press(&mut app, KeyCode::Enter);
        assert_eq!(ctrl(&mut app, 'j'), Action::None);

        let mut app = design_app();
        node_variables(&mut app);
        press(&mut app, KeyCode::Enter);
        assert!(app.is_editing());
        assert_eq!(ctrl(&mut app, 'j'), Action::None);
    }

    #[test]
    fn the_copy_keys_in_run_mode_copy_from_the_variables_pane_only() {
        let mut app = design_app();
        open_me(&mut app);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' ')); // id
        run_request(press(&mut app, KeyCode::Char('r')));
        assert!(app.focused(Focus::Response));
        assert_eq!(ctrl(&mut app, 'j'), Action::None);

        press(&mut app, KeyCode::Tab);
        assert!(app.focused(Focus::Variables));
        let body = copied(ctrl(&mut app, 'j'));
        assert_eq!(body["query"], "query Me {\n  me {\n    id\n  }\n}");
    }

    #[test]
    fn the_design_and_run_help_list_both_copy_keys() {
        for keys in [DESIGN_KEYS, RUN_KEYS] {
            for wanted in ["Ctrl-C", "Ctrl-J"] {
                assert!(keys.iter().any(|(key, _)| *key == wanted));
            }
        }
    }

    #[test]
    fn pasting_a_query_in_the_results_opens_its_design_as_if_built_by_hand() {
        let mut app = design_app();
        app.handle_paste("query   X { me { name id friends { name } } }");
        assert!(app.design.is_some());
        assert_eq!(
            query_text(&app),
            "query Me {\n  me {\n    id\n    name\n    friends {\n      name\n    }\n  }\n}"
        );
        assert_eq!(app.status(1_000).0, "imported Query.me");
        assert!(app.query.is_empty());
        let shown: Vec<String> = row_labels(&app);
        assert!(shown.iter().any(|label| label.starts_with("friends")));
        assert!(app.is_selected(&[
            Seg::Field("friends".to_string()),
            Seg::Field("name".to_string())
        ]));
    }

    #[test]
    fn a_pasted_json_body_fills_the_variables() {
        let mut app = design_app();
        app.handle_paste(
            r#"{"query": "query Q($i: ID!) { node(id: $i) { id } }", "variables": {"i": "42"}}"#,
        );
        assert_eq!(app.variable_rows()[0].raw, "42");
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Tab);
        let request = run_request(press(&mut app, KeyCode::Char('r')));
        assert_eq!(request.body["variables"], serde_json::json!({"id": "42"}));
    }

    #[test]
    fn a_paste_that_is_not_a_query_still_goes_to_the_search_box() {
        let mut app = design_app();
        app.handle_paste("userread");
        assert!(app.design.is_none());
        assert_eq!(app.query, "userread");
    }

    #[test]
    fn a_query_pasted_while_composing_replaces_the_design_in_any_pane() {
        for panes in 0..3 {
            let mut app = design_app();
            open_me(&mut app);
            for _ in 0..panes {
                press(&mut app, KeyCode::Tab);
            }
            app.handle_paste("{ me { name } }");
            assert_eq!(query_text(&app), "query Me {\n  me {\n    name\n  }\n}");
        }
    }

    #[test]
    fn replacing_a_selection_asks_first() {
        let mut app = design_app();
        open_me(&mut app);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' ')); // id
        app.handle_paste("{ me { name } }");
        assert!(app
            .status(1_000)
            .0
            .contains("Replace the current selection"));
        assert_eq!(query_text(&app), "query Me {\n  me {\n    id\n  }\n}");

        press(&mut app, KeyCode::Char('n'));
        assert_eq!(query_text(&app), "query Me {\n  me {\n    id\n  }\n}");

        app.handle_paste("{ me { name } }");
        press(&mut app, KeyCode::Char('y'));
        assert_eq!(query_text(&app), "query Me {\n  me {\n    name\n  }\n}");
    }

    #[test]
    fn a_saved_selection_of_the_same_operation_is_also_asked_about() {
        let mut app = design_app();
        open_me(&mut app);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Esc); // back to the results, the session is kept
        assert!(app.design.is_none());
        app.handle_paste("{ me { name } }");
        assert!(app.design.is_none());
        assert!(app
            .status(1_000)
            .0
            .contains("Replace the current selection"));
    }

    #[test]
    fn an_empty_design_is_replaced_without_asking() {
        let mut app = design_app();
        open_me(&mut app);
        app.handle_paste("{ me { name } }");
        assert_eq!(query_text(&app), "query Me {\n  me {\n    name\n  }\n}");
    }

    #[test]
    fn a_pasted_query_the_schema_does_not_know_changes_nothing() {
        let mut app = design_app();
        app.handle_paste("{ nope { id } }");
        assert!(app.design.is_none());
        assert!(app
            .status(1_000)
            .0
            .contains("none of the pasted root fields exist in the schema"));
        assert!(app.query.is_empty());
    }

    #[test]
    fn discarded_items_are_counted_in_the_notice() {
        let mut app = design_app();
        app.handle_paste("{ me { id nope other } }");
        assert!(app
            .status(1_000)
            .0
            .contains("imported Query.me; 2 items not in the schema were discarded"));
    }

    #[test]
    fn a_query_is_not_imported_in_run_mode_or_into_a_text_editor() {
        let mut app = design_app();
        open_me(&mut app);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' '));
        run_request(press(&mut app, KeyCode::Char('r')));
        app.handle_paste("{ me { name } }");
        assert_eq!(query_text(&app), "query Me {\n  me {\n    id\n  }\n}");

        let mut app = design_app();
        node_variables(&mut app);
        press(&mut app, KeyCode::Enter);
        app.handle_paste("{ me { name } }");
        assert!(app.is_editing());
        assert_eq!(app.query, "node"); // the search text is untouched
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.variable_rows()[0].raw, "{ me { name } }");
    }

    #[test]
    fn the_browse_and_design_help_mention_pasting() {
        for keys in [BROWSE_KEYS, DESIGN_KEYS] {
            assert!(keys.iter().any(|(key, _)| *key == "Paste"));
        }
    }

    fn ran_with_response(body: &str) -> App {
        let mut app = design_app();
        me_with_id(&mut app);
        run_request(press(&mut app, KeyCode::Char('r')));
        app.apply_run_result(1, Ok(view_of(200, body)));
        assert!(app.focused(Focus::Response));
        app
    }

    #[test]
    fn ctrl_c_in_the_response_pane_copies_what_the_pane_shows() {
        let mut app = ran_with_response(r#"{"data":{"me":{"id":"1"}}}"#);
        let expected = match &app.design.as_ref().unwrap().run.as_ref().unwrap().state {
            RunState::Done(view) => view.lines.join("\n"),
            other => panic!("{other:?}"),
        };
        assert!(expected.contains("\"id\": \"1\""));
        assert_eq!(ctrl(&mut app, 'c'), Action::Copy(expected));
        assert_eq!(app.status(1_000).0, "copied the response");
    }

    #[test]
    fn a_copied_response_starts_with_its_errors() {
        let mut app = ran_with_response(r#"{"errors":[{"message":"boom"}],"data":{"me":null}}"#);
        let Action::Copy(text) = ctrl(&mut app, 'c') else {
            panic!("expected a copy");
        };
        assert!(text.starts_with("error: boom\n\n"), "{text}");
    }

    #[test]
    fn a_capped_response_is_copied_capped_and_the_notice_says_so() {
        let mut app = ran_with_response("{}");
        let capped = ResponseView {
            status: 200,
            elapsed_ms: 1,
            errors: Vec::new(),
            not_json: false,
            lines: vec!["a".to_string(), "b".to_string()],
            total_lines: 5,
        };
        app.design.as_mut().unwrap().run.as_mut().unwrap().state = RunState::Done(capped);
        assert_eq!(ctrl(&mut app, 'c'), Action::Copy("a\nb".to_string()));
        assert_eq!(
            app.status(1_000).0,
            "copied the response, capped: 2 of 5 lines"
        );
    }

    #[test]
    fn there_is_no_response_to_copy_while_running_or_after_a_failure() {
        let mut app = design_app();
        me_with_id(&mut app);
        run_request(press(&mut app, KeyCode::Char('r')));
        assert_eq!(ctrl(&mut app, 'c'), Action::None);
        assert!(app.status(1_000).0.contains("no response to copy yet"));

        app.apply_run_result(1, Err("request failed".to_string()));
        assert_eq!(ctrl(&mut app, 'c'), Action::None);
        assert!(app.status(1_000).0.contains("no response to copy yet"));
    }

    #[test]
    fn ctrl_j_in_the_response_pane_does_nothing() {
        let mut app = ran_with_response(r#"{"data":{"me":{"id":"1"}}}"#);
        assert_eq!(ctrl(&mut app, 'j'), Action::None);
    }

    fn alt(app: &mut App, c: char) -> Action {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT))
    }

    #[test]
    fn ctrl_a_selects_the_fields_that_need_no_variable_and_a_second_press_clears_them() {
        let mut app = design_app();
        open_me(&mut app);
        ctrl(&mut app, 'a');
        assert_eq!(app.status(1_000).0, "selected 3 fields");
        assert_eq!(
            query_text(&app),
            "query Me {\n  me {\n    id\n    name\n    role\n  }\n}"
        );

        ctrl(&mut app, 'a');
        assert_eq!(app.status(1_000).0, "cleared the selection");
        assert!(matches!(
            app.generated_query(),
            Some(Err(vega_core::querygen::GenerateError::EmptySelection))
        ));
    }

    #[test]
    fn alt_a_selects_every_field_including_those_that_need_variables() {
        let mut app = design_app();
        open_me(&mut app);
        alt(&mut app, 'a');
        assert_eq!(app.status(1_000).0, "selected 7 fields");
        let text = query_text(&app);
        for wanted in [
            "id",
            "name",
            "role",
            "posts(first: $first)",
            "post(id: $id)",
        ] {
            assert!(text.contains(wanted), "{wanted} in {text}");
        }
        assert!(!text.contains("friends"), "{text}");

        alt(&mut app, 'a');
        assert_eq!(app.status(1_000).0, "cleared the selection");
    }

    #[test]
    fn select_all_completes_a_partial_selection_instead_of_clearing_it() {
        let mut app = design_app();
        open_me(&mut app);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' ')); // id only
        ctrl(&mut app, 'a');
        assert_eq!(app.status(1_000).0, "selected 3 fields");
        alt(&mut app, 'a');
        assert_eq!(app.status(1_000).0, "selected 7 fields");
    }

    #[test]
    fn ctrl_a_works_only_in_the_details_pane_of_an_operation() {
        let mut app = design_app();
        assert_eq!(ctrl(&mut app, 'a'), Action::None); // results list
        assert!(app.design.is_none());

        let mut app = design_app();
        open_me(&mut app);
        press(&mut app, KeyCode::Tab); // Query pane
        ctrl(&mut app, 'a');
        alt(&mut app, 'a');
        assert!(matches!(
            app.generated_query(),
            Some(Err(vega_core::querygen::GenerateError::EmptySelection))
        ));

        let mut app = design_app();
        type_text(&mut app, "version");
        press(&mut app, KeyCode::Enter); // a scalar operation: nothing to select
        ctrl(&mut app, 'a');
        assert_eq!(app.status(1_000).0, "nothing to select");
    }

    #[test]
    fn ctrl_a_does_nothing_in_run_mode_or_while_editing() {
        let mut app = design_app();
        node_variables(&mut app);
        press(&mut app, KeyCode::Enter);
        assert!(app.is_editing());
        ctrl(&mut app, 'a');
        assert!(app.is_editing());

        let mut app = design_app();
        open_me(&mut app);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' '));
        run_request(press(&mut app, KeyCode::Char('r')));
        ctrl(&mut app, 'a');
        assert_eq!(query_text(&app), "query Me {\n  me {\n    id\n  }\n}");
    }

    #[test]
    fn the_design_help_lists_both_select_all_keys() {
        for wanted in ["Ctrl-A", "Alt-A"] {
            assert!(DESIGN_KEYS.iter().any(|(key, _)| *key == wanted));
        }
    }

    #[test]
    fn space_selects_the_field_and_moves_to_the_next_selectable_one() {
        let mut app = design_app();
        open_me(&mut app);
        press(&mut app, KeyCode::Down); // id
        press(&mut app, KeyCode::Char(' '));
        assert_eq!(query_text(&app), "query Me {\n  me {\n    id\n  }\n}");
        assert_eq!(app.design.as_ref().unwrap().cursor, 2);
        assert_eq!(row_labels(&app)[2], "name: String");
    }

    #[test]
    fn space_opens_a_collapsed_object_and_lands_on_its_first_field() {
        let mut app = design_app();
        open_me(&mut app);
        for _ in 0..3 {
            press(&mut app, KeyCode::Down); // id, name, role
        }
        press(&mut app, KeyCode::Char(' ')); // role: friends is next, and collapsed
        let design = app.design.as_ref().unwrap();
        assert!(design
            .expanded
            .contains(&vec![Seg::Field("friends".to_string())]));
        let rows = app.design_rows();
        assert_eq!(
            rows[design.cursor].node.path,
            [
                Seg::Field("friends".to_string()),
                Seg::Field("id".to_string())
            ]
        );
    }

    #[test]
    fn space_does_not_open_args_groups_on_its_way() {
        let mut app = design_app();
        open_me(&mut app);
        for _ in 0..3 {
            press(&mut app, KeyCode::Down);
        }
        // role, then friends.id, friends.name and friends.role; `friends.friends` is
        // recursive and `friends.posts` takes arguments: its `args` group stays closed.
        for _ in 0..4 {
            press(&mut app, KeyCode::Char(' '));
        }
        let design = app.design.as_ref().unwrap();
        let friends_posts = vec![
            Seg::Field("friends".to_string()),
            Seg::Field("posts".to_string()),
        ];
        let mut args = friends_posts.clone();
        args.push(Seg::Args);
        assert!(design.expanded.contains(&friends_posts));
        assert!(!design.expanded.contains(&args));
        let rows = app.design_rows();
        let mut id = friends_posts;
        id.push(Seg::Field("id".to_string()));
        assert_eq!(rows[design.cursor].node.path, id);
    }

    #[test]
    fn unselecting_moves_on_too_and_an_object_row_does_nothing() {
        let mut app = design_app();
        open_me(&mut app);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' ')); // select id
        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Char(' ')); // unselect id: moves on
        assert_eq!(app.design.as_ref().unwrap().cursor, 2);

        for _ in 0..2 {
            press(&mut app, KeyCode::Down); // role, then friends (an object)
        }
        let before = app.design.as_ref().unwrap().cursor;
        press(&mut app, KeyCode::Char(' '));
        assert_eq!(app.design.as_ref().unwrap().cursor, before);
    }

    #[test]
    fn alt_a_also_opts_in_the_optional_arguments_and_ctrl_a_does_not() {
        let mut app = design_app();
        type_text(&mut app, "users");
        press(&mut app, KeyCode::Enter);
        ctrl(&mut app, 'a');
        assert_eq!(app.status(1_000).0, "selected 3 fields");
        assert!(!query_text(&app).contains("$role"));

        alt(&mut app, 'a');
        assert_eq!(
            app.status(1_000).0,
            "selected 7 fields and 6 optional arguments"
        );
        let text = query_text(&app);
        for wanted in [
            "$role: Role",
            "$active: Boolean",
            "$limit: Int! = 10",
            "users(role: $role",
        ] {
            assert!(text.contains(wanted), "{wanted} in {text}");
        }
        // The six optional arguments, plus `posts(first)` and `post(id)`.
        assert_eq!(app.variable_rows().len(), 8);

        alt(&mut app, 'a');
        assert_eq!(app.status(1_000).0, "cleared the selection");
        assert!(app.variable_rows().is_empty());
    }
}
