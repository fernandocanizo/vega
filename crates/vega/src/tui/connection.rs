//! The connection modal: the endpoint plus how to authenticate, as a pure state machine
//! (key in, state out). Values live in memory only: the modal is dropped when it closes,
//! which also masks every field again.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use vega_core::config::Settings;
use vega_core::headers::{self, Header, DEFAULT_KEY_HEADER};

/// How to authenticate. Each type is a form that produces the request headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthType {
    NoAuth,
    Basic,
    ApiKey,
    Bearer,
    CustomHeaders,
}

impl AuthType {
    /// In the order the list shows them.
    pub const ALL: [AuthType; 5] = [
        AuthType::NoAuth,
        AuthType::Basic,
        AuthType::ApiKey,
        AuthType::Bearer,
        AuthType::CustomHeaders,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AuthType::NoAuth => "None",
            AuthType::Basic => "Basic",
            AuthType::ApiKey => "API Key",
            AuthType::Bearer => "Bearer",
            AuthType::CustomHeaders => "Custom headers",
        }
    }
}

/// Positions in `AuthType::ALL`.
const API_KEY: usize = 2;
const CUSTOM: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Uri,
    /// The list of authentication types.
    Types,
    /// A field of the selected type's form.
    Field(usize),
}

/// One single-line text input. It has no `Debug` on purpose: it may hold a secret.
#[derive(Clone, PartialEq, Eq)]
pub struct Field {
    label: &'static str,
    value: String,
    secret: bool,
    revealed: bool,
}

impl Field {
    fn new(label: &'static str, secret: bool) -> Self {
        Self {
            label,
            value: String::new(),
            secret,
            revealed: false,
        }
    }

    fn with_value(mut self, value: &str) -> Self {
        self.value = value.to_string();
        self
    }

    pub fn label(&self) -> &'static str {
        self.label
    }

    /// What to draw: the text, or one bullet per character for a secret that is not
    /// revealed.
    pub fn display(&self) -> String {
        if self.secret && !self.revealed {
            "•".repeat(self.value.chars().count())
        } else {
            self.value.clone()
        }
    }
}

/// What a key did to the modal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Open,
    Cancel,
    /// The user pressed Enter on a valid form.
    Submit(Settings),
}

/// A header name field followed by its value field, for Custom headers rows.
fn pair() -> [Field; 2] {
    [Field::new("Header name", false), Field::new("Value", true)]
}

pub struct Modal {
    uri: Field,
    /// Index into `AuthType::ALL`.
    selected: usize,
    focus: Focus,
    /// One form per type, in `AuthType::ALL` order. Custom headers holds name and value
    /// fields in pairs.
    forms: [Vec<Field>; 5],
    error: Option<String>,
}

impl Modal {
    /// Opens on the current settings: no headers means None, the API key alone means the
    /// API Key form, anything else means Custom headers with one row per header.
    pub fn new(settings: &Settings) -> Self {
        let mut forms = [
            Vec::new(),
            vec![Field::new("Username", false), Field::new("Password", true)],
            vec![
                Field::new("Header name", false).with_value(DEFAULT_KEY_HEADER),
                Field::new("Value", true),
            ],
            vec![Field::new("Token", true)],
            pair().to_vec(),
        ];
        let mut selected = 0;
        match settings.headers.as_slice() {
            [] => {}
            [key] if settings.key_only => {
                selected = API_KEY;
                forms[API_KEY] = vec![
                    Field::new("Header name", false).with_value(key.name()),
                    Field::new("Value", true).with_value(key.value()),
                ];
            }
            headers => {
                selected = CUSTOM;
                forms[CUSTOM] = headers
                    .iter()
                    .flat_map(|header| {
                        let [name, value] = pair();
                        [
                            name.with_value(header.name()),
                            value.with_value(header.value()),
                        ]
                    })
                    .collect();
            }
        }
        let focus = if settings.uri.is_empty() {
            Focus::Uri
        } else {
            Focus::Types
        };
        Self {
            uri: Field::new("URI", false).with_value(&settings.uri),
            selected,
            focus,
            forms,
            error: None,
        }
    }

    pub fn uri(&self) -> &Field {
        &self.uri
    }

    pub fn selected(&self) -> AuthType {
        AuthType::ALL[self.selected]
    }

    pub fn focus(&self) -> Focus {
        self.focus
    }

    /// The fields of the selected type's form.
    pub fn fields(&self) -> &[Field] {
        &self.forms[self.selected]
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Outcome {
        self.error = None;
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => return Outcome::Cancel,
            KeyCode::Enter => return self.submit(),
            KeyCode::Tab => self.cycle(true),
            KeyCode::BackTab => self.cycle(false),
            KeyCode::Up => self.move_vertically(false),
            KeyCode::Down => self.move_vertically(true),
            KeyCode::Backspace => {
                if let Some(field) = self.focused_mut() {
                    field.value.pop();
                }
            }
            KeyCode::Char('t') if ctrl => {
                if let Some(field) = self.focused_mut().filter(|field| field.secret) {
                    field.revealed = !field.revealed;
                }
            }
            KeyCode::Char('n') if ctrl => self.add_row(),
            KeyCode::Char(c) if !ctrl && !c.is_control() => self.insert(c.encode_utf8(&mut [0; 4])),
            _ => {}
        }
        Outcome::Open
    }

    /// A bracketed paste: goes to the focused field with line breaks removed.
    pub fn paste(&mut self, text: &str) {
        self.error = None;
        let clean: String = text.chars().filter(|c| !c.is_control()).collect();
        self.insert(&clean);
    }

    fn insert(&mut self, text: &str) {
        if let Some(field) = self.focused_mut() {
            field.value.push_str(text);
        }
    }

    fn focused_mut(&mut self) -> Option<&mut Field> {
        match self.focus {
            Focus::Uri => Some(&mut self.uri),
            Focus::Types => None,
            Focus::Field(index) => self.forms[self.selected].get_mut(index),
        }
    }

    /// Tab order: the URI, the type list, then the fields of the selected form.
    fn cycle(&mut self, forward: bool) {
        let mut order = vec![Focus::Uri, Focus::Types];
        order.extend((0..self.forms[self.selected].len()).map(Focus::Field));
        let at = order.iter().position(|f| *f == self.focus).unwrap_or(0);
        let next = if forward {
            (at + 1) % order.len()
        } else {
            (at + order.len() - 1) % order.len()
        };
        self.focus = order[next];
    }

    fn move_vertically(&mut self, down: bool) {
        match self.focus {
            Focus::Types => {
                self.selected = if down {
                    (self.selected + 1).min(AuthType::ALL.len() - 1)
                } else {
                    self.selected.saturating_sub(1)
                };
            }
            Focus::Uri => {
                if down {
                    self.focus = Focus::Types;
                }
            }
            Focus::Field(index) => {
                if !down {
                    self.focus = if index == 0 {
                        Focus::Types
                    } else {
                        Focus::Field(index - 1)
                    };
                } else if index + 1 < self.forms[self.selected].len() {
                    self.focus = Focus::Field(index + 1);
                }
            }
        }
    }

    /// Custom headers only: appends an empty row and focuses its name.
    fn add_row(&mut self) {
        if self.selected != CUSTOM {
            return;
        }
        self.forms[CUSTOM].extend(pair());
        self.focus = Focus::Field(self.forms[CUSTOM].len() - 2);
    }

    fn submit(&mut self) -> Outcome {
        match self.build() {
            Ok(settings) => Outcome::Submit(settings),
            Err(message) => {
                self.error = Some(message);
                Outcome::Open
            }
        }
    }

    /// Turns the forms into settings. Messages never contain a field value.
    fn build(&self) -> Result<Settings, String> {
        let uri = self.uri.value.trim();
        if uri.is_empty() {
            return Err("The URI is required".to_string());
        }
        let form = &self.forms[self.selected];
        let text = |index: usize| form[index].value.trim();
        let (headers, key_only) = match self.selected() {
            AuthType::NoAuth => (Vec::new(), false),
            AuthType::Basic => {
                let user = text(0);
                if user.is_empty() {
                    return Err("The username is required".to_string());
                }
                if user.contains(':') {
                    return Err("The username cannot contain ':'".to_string());
                }
                // A password may start or end with spaces, so it is not trimmed.
                (vec![headers::basic(user, &form[1].value)], false)
            }
            AuthType::ApiKey => {
                if text(1).is_empty() {
                    return Err("The API key is required".to_string());
                }
                let header = Header::new(text(0), text(1))
                    .map_err(|_| "The header name is not valid".to_string())?;
                (vec![header], true)
            }
            AuthType::Bearer => {
                if text(0).is_empty() {
                    return Err("The token is required".to_string());
                }
                (vec![headers::bearer(text(0))], false)
            }
            AuthType::CustomHeaders => {
                let mut rows = Vec::new();
                for (index, row) in form.chunks(2).enumerate() {
                    let (name, value) = (row[0].value.trim(), row[1].value.trim());
                    if name.is_empty() && value.is_empty() {
                        continue;
                    }
                    let number = index + 1;
                    if name.is_empty() {
                        return Err(format!("Row {number}: the header name is required"));
                    }
                    if value.is_empty() {
                        return Err(format!("Row {number}: the value is required"));
                    }
                    let header = Header::new(name, value)
                        .map_err(|_| format!("Row {number}: the header name is not valid"))?;
                    rows.push(header);
                }
                (headers::merge(Vec::new(), rows), false)
            }
        };
        Ok(Settings {
            uri: uri.to_string(),
            headers,
            key_only,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn settings(uri: &str, headers: Vec<Header>, key_only: bool) -> Settings {
        Settings {
            uri: uri.to_string(),
            headers,
            key_only,
        }
    }

    fn header(name: &str, value: &str) -> Header {
        Header::new(name, value).unwrap()
    }

    /// A modal with an endpoint set and no credentials: focus starts on the type list.
    fn modal() -> Modal {
        Modal::new(&settings("https://x.example.com/graphql", vec![], false))
    }

    fn press(modal: &mut Modal, code: KeyCode) -> Outcome {
        modal.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn ctrl(modal: &mut Modal, c: char) -> Outcome {
        modal.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
    }

    fn type_text(modal: &mut Modal, text: &str) {
        for c in text.chars() {
            press(modal, KeyCode::Char(c));
        }
    }

    /// Moves the type list to `target` and focuses its first field.
    fn pick(modal: &mut Modal, target: AuthType) {
        while modal.selected() != target {
            press(modal, KeyCode::Down);
        }
        if !modal.fields().is_empty() {
            press(modal, KeyCode::Tab);
        }
    }

    fn submitted(outcome: Outcome) -> Settings {
        match outcome {
            Outcome::Submit(settings) => settings,
            Outcome::Open => panic!("still open"),
            Outcome::Cancel => panic!("cancelled"),
        }
    }

    #[test]
    fn list_indexes_match_the_type_order() {
        assert_eq!(AuthType::ALL[API_KEY], AuthType::ApiKey);
        assert_eq!(AuthType::ALL[CUSTOM], AuthType::CustomHeaders);
        let labels: Vec<_> = AuthType::ALL.iter().map(|t| t.label()).collect();
        assert_eq!(
            labels,
            ["None", "Basic", "API Key", "Bearer", "Custom headers"]
        );
    }

    #[test]
    fn no_headers_opens_on_none_and_focuses_the_uri_only_when_it_is_empty() {
        let with_uri = modal();
        assert_eq!(with_uri.selected(), AuthType::NoAuth);
        assert_eq!(with_uri.focus(), Focus::Types);
        assert_eq!(with_uri.uri().display(), "https://x.example.com/graphql");

        let without = Modal::new(&settings("", vec![], false));
        assert_eq!(without.focus(), Focus::Uri);
    }

    #[test]
    fn the_key_alone_opens_on_api_key_prefilled() {
        let start = settings("https://x/graphql", vec![header("X-Token", "k-1")], true);
        let mut modal = Modal::new(&start);
        assert_eq!(modal.selected(), AuthType::ApiKey);
        assert_eq!(modal.fields()[0].display(), "X-Token");
        assert_eq!(submitted(press(&mut modal, KeyCode::Enter)), start);
    }

    #[test]
    fn other_headers_open_on_custom_headers_one_row_each() {
        let start = settings(
            "https://x/graphql",
            vec![
                header("Authorization", "Bearer abc"),
                header("X-Tenant", "t"),
            ],
            false,
        );
        let mut modal = Modal::new(&start);
        assert_eq!(modal.selected(), AuthType::CustomHeaders);
        assert_eq!(modal.fields().len(), 4);
        assert_eq!(modal.fields()[0].display(), "Authorization");
        assert_eq!(submitted(press(&mut modal, KeyCode::Enter)), start);
    }

    #[test]
    fn secret_fields_show_bullets_until_revealed() {
        let mut modal = modal();
        pick(&mut modal, AuthType::Bearer);
        type_text(&mut modal, "tok-123");
        assert_eq!(modal.fields()[0].display(), "•••••••");
        ctrl(&mut modal, 't');
        assert_eq!(modal.fields()[0].display(), "tok-123");
        ctrl(&mut modal, 't');
        assert_eq!(modal.fields()[0].display(), "•••••••");
    }

    #[test]
    fn reveal_does_nothing_on_plain_fields() {
        let mut modal = modal();
        pick(&mut modal, AuthType::Basic);
        type_text(&mut modal, "alice");
        ctrl(&mut modal, 't');
        assert_eq!(modal.fields()[0].display(), "alice");
    }

    #[test]
    fn none_submits_the_uri_without_headers() {
        let mut modal = modal();
        let result = submitted(press(&mut modal, KeyCode::Enter));
        assert_eq!(
            result,
            settings("https://x.example.com/graphql", vec![], false)
        );
    }

    #[test]
    fn a_missing_uri_is_an_error_and_the_modal_stays_open() {
        let mut modal = Modal::new(&settings("", vec![], false));
        assert_eq!(press(&mut modal, KeyCode::Enter), Outcome::Open);
        assert_eq!(modal.error(), Some("The URI is required"));
        press(&mut modal, KeyCode::Char('x'));
        assert_eq!(modal.error(), None);
    }

    #[test]
    fn typing_in_the_uri_field_edits_it() {
        let mut modal = Modal::new(&settings("", vec![], false));
        type_text(&mut modal, " https://y/graphql ");
        press(&mut modal, KeyCode::Backspace);
        press(&mut modal, KeyCode::Backspace);
        press(&mut modal, KeyCode::Char('l'));
        let result = submitted(press(&mut modal, KeyCode::Enter));
        assert_eq!(result.uri, "https://y/graphql");
    }

    #[test]
    fn basic_builds_the_authorization_header() {
        let mut modal = modal();
        pick(&mut modal, AuthType::Basic);
        type_text(&mut modal, "Aladdin");
        press(&mut modal, KeyCode::Tab);
        type_text(&mut modal, "open sesame");
        let result = submitted(press(&mut modal, KeyCode::Enter));
        assert_eq!(
            result.headers,
            vec![header(
                "Authorization",
                "Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ=="
            )]
        );
        assert!(!result.key_only);
    }

    #[test]
    fn basic_rejects_a_missing_user_and_a_user_with_a_colon() {
        let mut modal = modal();
        pick(&mut modal, AuthType::Basic);
        assert_eq!(press(&mut modal, KeyCode::Enter), Outcome::Open);
        assert_eq!(modal.error(), Some("The username is required"));
        type_text(&mut modal, "a:b");
        assert_eq!(press(&mut modal, KeyCode::Enter), Outcome::Open);
        assert_eq!(modal.error(), Some("The username cannot contain ':'"));
    }

    #[test]
    fn api_key_defaults_to_x_api_key_and_marks_key_only() {
        let mut modal = modal();
        pick(&mut modal, AuthType::ApiKey);
        assert_eq!(modal.fields()[0].display(), "X-API-Key");
        press(&mut modal, KeyCode::Tab);
        type_text(&mut modal, "secret-1");
        let result = submitted(press(&mut modal, KeyCode::Enter));
        assert_eq!(result.headers, vec![header("X-API-Key", "secret-1")]);
        assert!(result.key_only);
    }

    #[test]
    fn api_key_needs_a_value_and_a_valid_name_without_echoing_either() {
        let mut modal = modal();
        pick(&mut modal, AuthType::ApiKey);
        assert_eq!(press(&mut modal, KeyCode::Enter), Outcome::Open);
        assert_eq!(modal.error(), Some("The API key is required"));
        press(&mut modal, KeyCode::Tab);
        type_text(&mut modal, "secret-1");
        press(&mut modal, KeyCode::BackTab);
        type_text(&mut modal, " bad");
        assert_eq!(press(&mut modal, KeyCode::Enter), Outcome::Open);
        let message = modal.error().unwrap().to_string();
        assert_eq!(message, "The header name is not valid");
        assert!(!message.contains("secret-1"));
    }

    #[test]
    fn bearer_builds_the_authorization_header_and_trims_the_token() {
        let mut modal = modal();
        pick(&mut modal, AuthType::Bearer);
        type_text(&mut modal, "  tok-123 ");
        let result = submitted(press(&mut modal, KeyCode::Enter));
        assert_eq!(
            result.headers,
            vec![header("Authorization", "Bearer tok-123")]
        );
    }

    #[test]
    fn bearer_needs_a_token() {
        let mut modal = modal();
        pick(&mut modal, AuthType::Bearer);
        assert_eq!(press(&mut modal, KeyCode::Enter), Outcome::Open);
        assert_eq!(modal.error(), Some("The token is required"));
    }

    #[test]
    fn custom_headers_skip_empty_rows_and_keep_the_rest_in_order() {
        let mut modal = modal();
        pick(&mut modal, AuthType::CustomHeaders);
        type_text(&mut modal, "X-One");
        press(&mut modal, KeyCode::Tab);
        type_text(&mut modal, "1");
        ctrl(&mut modal, 'n'); // an empty second row
        ctrl(&mut modal, 'n'); // a third row, filled in
        type_text(&mut modal, "X-Three");
        press(&mut modal, KeyCode::Tab);
        type_text(&mut modal, "3");
        let result = submitted(press(&mut modal, KeyCode::Enter));
        assert_eq!(
            result.headers,
            vec![header("X-One", "1"), header("X-Three", "3")]
        );
        assert!(!result.key_only);
    }

    #[test]
    fn ctrl_n_adds_a_row_and_focuses_its_name() {
        let mut modal = modal();
        pick(&mut modal, AuthType::CustomHeaders);
        assert_eq!(modal.fields().len(), 2);
        ctrl(&mut modal, 'n');
        assert_eq!(modal.fields().len(), 4);
        assert_eq!(modal.focus(), Focus::Field(2));
    }

    #[test]
    fn ctrl_n_does_nothing_on_other_types() {
        let mut modal = modal();
        pick(&mut modal, AuthType::Bearer);
        ctrl(&mut modal, 'n');
        assert_eq!(modal.fields().len(), 1);
    }

    #[test]
    fn custom_rows_report_their_number_without_echoing_values() {
        let mut modal = modal();
        pick(&mut modal, AuthType::CustomHeaders);
        press(&mut modal, KeyCode::Tab);
        type_text(&mut modal, "secret-1"); // a value with no name
        assert_eq!(press(&mut modal, KeyCode::Enter), Outcome::Open);
        let message = modal.error().unwrap().to_string();
        assert_eq!(message, "Row 1: the header name is required");

        press(&mut modal, KeyCode::BackTab);
        type_text(&mut modal, "Bad Name");
        assert_eq!(press(&mut modal, KeyCode::Enter), Outcome::Open);
        let message = modal.error().unwrap().to_string();
        assert_eq!(message, "Row 1: the header name is not valid");
        assert!(!message.contains("secret-1"));
    }

    #[test]
    fn a_row_with_a_name_but_no_value_is_an_error() {
        let mut modal = modal();
        pick(&mut modal, AuthType::CustomHeaders);
        type_text(&mut modal, "X-One");
        assert_eq!(press(&mut modal, KeyCode::Enter), Outcome::Open);
        assert_eq!(modal.error(), Some("Row 1: the value is required"));
    }

    #[test]
    fn duplicate_names_keep_the_last_row() {
        let mut modal = modal();
        pick(&mut modal, AuthType::CustomHeaders);
        type_text(&mut modal, "X-A");
        press(&mut modal, KeyCode::Tab);
        type_text(&mut modal, "1");
        ctrl(&mut modal, 'n');
        type_text(&mut modal, "x-a");
        press(&mut modal, KeyCode::Tab);
        type_text(&mut modal, "2");
        let result = submitted(press(&mut modal, KeyCode::Enter));
        assert_eq!(result.headers, vec![header("x-a", "2")]);
    }

    #[test]
    fn tab_cycles_through_every_field_and_wraps() {
        let mut modal = Modal::new(&settings("", vec![], false));
        pick(&mut modal, AuthType::Basic); // Tab from Uri goes to Types first
        assert_eq!(modal.focus(), Focus::Field(0));
        press(&mut modal, KeyCode::Tab);
        assert_eq!(modal.focus(), Focus::Field(1));
        press(&mut modal, KeyCode::Tab);
        assert_eq!(modal.focus(), Focus::Uri);
        press(&mut modal, KeyCode::BackTab);
        assert_eq!(modal.focus(), Focus::Field(1));
    }

    #[test]
    fn up_and_down_move_the_type_selection_and_clamp() {
        let mut modal = modal();
        press(&mut modal, KeyCode::Up);
        assert_eq!(modal.selected(), AuthType::NoAuth);
        for _ in 0..10 {
            press(&mut modal, KeyCode::Down);
        }
        assert_eq!(modal.selected(), AuthType::CustomHeaders);
        assert_eq!(modal.focus(), Focus::Types);
    }

    #[test]
    fn up_and_down_move_between_fields_and_up_leaves_to_the_types() {
        let mut modal = modal();
        pick(&mut modal, AuthType::Basic);
        press(&mut modal, KeyCode::Down);
        assert_eq!(modal.focus(), Focus::Field(1));
        press(&mut modal, KeyCode::Down);
        assert_eq!(modal.focus(), Focus::Field(1));
        press(&mut modal, KeyCode::Up);
        press(&mut modal, KeyCode::Up);
        assert_eq!(modal.focus(), Focus::Types);
    }

    #[test]
    fn down_from_the_uri_goes_to_the_types() {
        let mut modal = Modal::new(&settings("", vec![], false));
        press(&mut modal, KeyCode::Down);
        assert_eq!(modal.focus(), Focus::Types);
    }

    #[test]
    fn each_type_keeps_its_own_values_when_switching() {
        let mut modal = modal();
        pick(&mut modal, AuthType::Bearer);
        type_text(&mut modal, "tok");
        press(&mut modal, KeyCode::Up); // back to the list
        press(&mut modal, KeyCode::Up); // API Key
        press(&mut modal, KeyCode::Down); // Bearer again
        assert_eq!(modal.selected(), AuthType::Bearer);
        assert_eq!(modal.fields()[0].display(), "•••");
    }

    #[test]
    fn paste_goes_to_the_focused_field_without_line_breaks() {
        let mut modal = modal();
        pick(&mut modal, AuthType::Bearer);
        modal.paste("tok-123\r\n");
        let result = submitted(press(&mut modal, KeyCode::Enter));
        assert_eq!(
            result.headers,
            vec![header("Authorization", "Bearer tok-123")]
        );
    }

    #[test]
    fn paste_is_ignored_while_the_type_list_has_focus() {
        let mut modal = modal();
        modal.paste("whatever");
        assert_eq!(modal.uri().display(), "https://x.example.com/graphql");
    }

    #[test]
    fn typing_is_ignored_while_the_type_list_has_focus() {
        let mut modal = modal();
        type_text(&mut modal, "abc");
        assert_eq!(modal.uri().display(), "https://x.example.com/graphql");
    }

    #[test]
    fn esc_cancels() {
        let mut modal = modal();
        assert_eq!(press(&mut modal, KeyCode::Esc), Outcome::Cancel);
    }

    #[test]
    fn other_control_chords_do_not_type() {
        let mut modal = Modal::new(&settings("", vec![], false));
        ctrl(&mut modal, 'x');
        assert_eq!(modal.uri().display(), "");
    }
}
