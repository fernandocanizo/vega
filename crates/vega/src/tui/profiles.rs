//! The profile picker: a pure state machine (key in, outcome out), like the connection
//! modal. It never holds or shows a header value.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use vega_core::profile::Kind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub uri: String,
    pub kind: Kind,
}

/// What a key did to the picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Open,
    Close,
    Connect(String),
    Edit(String),
    New,
    Delete(String),
}

pub struct Picker {
    entries: Vec<Entry>,
    selected: usize,
    current: Option<String>,
    confirming: bool,
}

impl Picker {
    pub fn new(entries: Vec<Entry>, current: Option<String>) -> Self {
        let selected = current
            .as_deref()
            .and_then(|name| entries.iter().position(|entry| entry.name == name))
            .unwrap_or(0);
        Self {
            entries,
            selected,
            current,
            confirming: false,
        }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    /// The profile in use, which the list marks.
    pub fn current(&self) -> Option<&str> {
        self.current.as_deref()
    }

    /// True while the delete question is showing.
    pub fn confirming(&self) -> bool {
        self.confirming
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Outcome {
        let name = self.entries.get(self.selected).map(|e| e.name.clone());
        // Control and Alt chords are never the plain letter keys.
        let code = if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            match key.code {
                KeyCode::Char(_) => KeyCode::Null,
                other => other,
            }
        } else {
            key.code
        };
        if self.confirming {
            self.confirming = false;
            return match (code, name) {
                (KeyCode::Char('y' | 'Y'), Some(name)) => Outcome::Delete(name),
                _ => Outcome::Open,
            };
        }
        match code {
            KeyCode::Esc => Outcome::Close,
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                Outcome::Open
            }
            KeyCode::Down => {
                self.selected = (self.selected + 1).min(self.entries.len().saturating_sub(1));
                Outcome::Open
            }
            KeyCode::Enter => name.map_or(Outcome::Open, Outcome::Connect),
            KeyCode::Char('e') => name.map_or(Outcome::Open, Outcome::Edit),
            KeyCode::Char('n') => Outcome::New,
            KeyCode::Char('d') => {
                self.confirming = name.is_some();
                Outcome::Open
            }
            _ => Outcome::Open,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn entry(name: &str) -> Entry {
        Entry {
            name: name.to_string(),
            uri: format!("https://{name}/graphql"),
            kind: Kind::ApiKey,
        }
    }

    fn picker() -> Picker {
        Picker::new(vec![entry("a"), entry("b"), entry("c")], None)
    }

    #[test]
    fn arrows_move_and_stop_at_the_ends() {
        let mut p = picker();
        p.handle_key(key(KeyCode::Up));
        assert_eq!(p.selected(), 0);
        p.handle_key(key(KeyCode::Down));
        p.handle_key(key(KeyCode::Down));
        p.handle_key(key(KeyCode::Down));
        assert_eq!(p.selected(), 2);
    }

    #[test]
    fn it_opens_on_the_current_profile() {
        let p = Picker::new(vec![entry("a"), entry("b")], Some("b".to_string()));
        assert_eq!(p.selected(), 1);
        assert_eq!(p.current(), Some("b"));
    }

    #[test]
    fn enter_connects_and_e_edits_the_selected_profile() {
        let mut p = picker();
        p.handle_key(key(KeyCode::Down));
        assert_eq!(
            p.handle_key(key(KeyCode::Enter)),
            Outcome::Connect("b".into())
        );
        assert_eq!(
            p.handle_key(key(KeyCode::Char('e'))),
            Outcome::Edit("b".into())
        );
    }

    #[test]
    fn n_adds_and_esc_closes() {
        let mut p = picker();
        assert_eq!(p.handle_key(key(KeyCode::Char('n'))), Outcome::New);
        assert_eq!(p.handle_key(key(KeyCode::Esc)), Outcome::Close);
    }

    #[test]
    fn delete_needs_a_yes() {
        let mut p = picker();
        assert_eq!(p.handle_key(key(KeyCode::Char('d'))), Outcome::Open);
        assert!(p.confirming());
        assert_eq!(
            p.handle_key(key(KeyCode::Char('y'))),
            Outcome::Delete("a".into())
        );
        assert!(!p.confirming());
        p.handle_key(key(KeyCode::Char('d')));
        assert_eq!(p.handle_key(key(KeyCode::Char('x'))), Outcome::Open);
        assert!(!p.confirming());
    }

    #[test]
    fn an_empty_list_only_offers_new_and_close() {
        let mut p = Picker::new(vec![], None);
        assert_eq!(p.handle_key(key(KeyCode::Enter)), Outcome::Open);
        assert_eq!(p.handle_key(key(KeyCode::Char('e'))), Outcome::Open);
        p.handle_key(key(KeyCode::Char('d')));
        assert!(!p.confirming());
        assert_eq!(p.handle_key(key(KeyCode::Char('n'))), Outcome::New);
    }

    #[test]
    fn control_and_alt_chords_are_not_the_plain_keys() {
        let mut p = picker();
        for modifier in [KeyModifiers::CONTROL, KeyModifiers::ALT] {
            let chord = |c| KeyEvent::new(KeyCode::Char(c), modifier);
            assert_eq!(p.handle_key(chord('d')), Outcome::Open);
            assert!(!p.confirming());
            assert_eq!(p.handle_key(chord('e')), Outcome::Open);
            assert_eq!(p.handle_key(chord('n')), Outcome::Open);
        }
        p.handle_key(key(KeyCode::Char('d')));
        assert!(p.confirming());
        assert_eq!(
            p.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL)),
            Outcome::Open
        );
        assert!(!p.confirming());
    }
}
