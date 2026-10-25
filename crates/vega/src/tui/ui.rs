//! Rendering. Reads from `App`; never mutates it.

use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::tui::app::{list_offset, App, Edit, Focus, RunState, StatusLevel, VariableRow};
use crate::tui::connection::{AuthType, Field, Focus as ModalFocus, Modal};
use vega_core::response::LineKind;
use vega_core::selection::{NodeKind, Row};
use vega_core::variables::{self, State};

/// The color of whatever has the focus: the input box being typed in, the window border
/// being navigated and the selected row. Nothing else uses it, so the eye goes where the
/// keys go. It is the terminal's own magenta (purple in most palettes) until themes exist.
pub const FOCUS: Color = Color::Magenta;

/// The style of the selected row and of the input being edited. Reversed keeps the text
/// readable on any terminal palette.
fn focus_style() -> Style {
    Style::default().fg(FOCUS).add_modifier(Modifier::REVERSED)
}

/// A modal (the connection modal or the help) has the input focus, so nothing behind it
/// shows focus marks or the terminal cursor.
fn overlay_open(app: &App) -> bool {
    app.modal.is_some() || app.help_open
}

/// Version shown in the help modal title.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn draw(frame: &mut Frame, app: &App, now: u64) {
    let area = frame.area();
    if app.design.is_some() {
        let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(area);
        draw_status(frame, rows[0], app, now);
        let tree = app.design_rows();
        let cursor = app.design.as_ref().map(|design| design.cursor);
        let is_operation = app
            .design
            .as_ref()
            .is_some_and(|design| !design.read_only());
        if is_operation {
            let columns =
                Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(rows[1]);
            if app
                .design
                .as_ref()
                .is_some_and(|design| design.run.is_some())
            {
                draw_query(frame, columns[0], app);
                draw_response(frame, columns[1], app);
            } else {
                draw_details(frame, columns[0], app, &tree, cursor);
                draw_query(frame, columns[1], app);
            }
        } else {
            draw_details(frame, rows[1], app, &tree, cursor);
        }
    } else {
        let rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(area);
        draw_status(frame, rows[0], app, now);
        draw_search(frame, rows[1], app);
        let columns = Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(rows[2]);
        draw_hits(frame, columns[0], app);
        draw_details(frame, columns[1], app, &app.preview_rows(), None);
    }
    if app.help_open {
        draw_help(frame, area, app);
    }
    if let Some(modal) = &app.modal {
        draw_connection(frame, area, modal);
    }
}

fn draw_help(frame: &mut Frame, area: Rect, app: &App) {
    let keys = app.help_keys();
    let key_width = keys
        .iter()
        .map(|(keys, _)| keys.chars().count())
        .max()
        .unwrap_or(0);
    let mut lines = vec![Line::raw("")];
    lines.extend(keys.iter().map(|(keys, description)| {
        Line::from(vec![
            Span::styled(
                format!("{keys:<key_width$}"),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("  {description}")),
        ])
    }));
    lines.push(Line::styled(
        "Esc to close",
        Style::default().fg(Color::DarkGray),
    ));

    let title = format!("Vega - {VERSION}");
    let content_width = lines
        .iter()
        .map(Line::width)
        .chain(std::iter::once(title.chars().count()))
        .max()
        .unwrap_or(0);
    // Borders (2) plus one column of padding on each side.
    let width = u16::try_from(content_width + 4).unwrap_or(u16::MAX);
    let height = u16::try_from(lines.len() + 2).unwrap_or(u16::MAX);
    let width = width.min(area.width);
    let height = height.min(area.height);
    let popup = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );

    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
    let padded = Rect::new(
        inner.x.saturating_add(1),
        inner.y,
        inner.width.saturating_sub(2),
        inner.height,
    );
    frame.render_widget(Paragraph::new(Text::from(lines)), padded);
}

fn draw_connection(frame: &mut Frame, area: Rect, modal: &Modal) {
    let width = area.width.min(86);
    let height = area.height.min(23);
    let popup = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    let block = Block::default().borders(Borders::ALL).title("Connection");
    let inner = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);

    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(2),
    ])
    .split(inner);

    let uri_focused = modal.focus() == ModalFocus::Uri;
    frame.render_widget(
        Paragraph::new(field_text(
            modal.uri(),
            uri_focused,
            usize::from(rows[0].width.saturating_sub(2)),
        ))
        .block(pane("URI", uri_focused)),
        rows[0],
    );

    let name_focused = modal.focus() == ModalFocus::Name;
    frame.render_widget(
        Paragraph::new(field_text(
            modal.name(),
            name_focused,
            usize::from(rows[1].width.saturating_sub(2)),
        ))
        .block(pane("Save as (optional, Ctrl-S)", name_focused)),
        rows[1],
    );

    let columns = Layout::horizontal([Constraint::Length(18), Constraint::Min(1)]).split(rows[2]);
    draw_types(frame, columns[0], modal);
    draw_form(frame, columns[1], modal);

    let footer = match modal.error() {
        Some(message) => Line::styled(message.to_string(), Style::default().fg(Color::Red)),
        None => Line::raw(""),
    };
    let hint = Line::styled(
        "Tab next · Enter connect · Ctrl-S save · Esc cancel · Ctrl-T reveal · Ctrl-N add row",
        Style::default().fg(Color::DarkGray),
    );
    frame.render_widget(Paragraph::new(vec![footer, hint]), rows[3]);
}

fn draw_types(frame: &mut Frame, area: Rect, modal: &Modal) {
    let focused = modal.focus() == ModalFocus::Types;
    let items: Vec<ListItem> = AuthType::ALL
        .iter()
        .map(|auth| {
            let mark = if *auth == modal.selected() {
                "●"
            } else {
                "○"
            };
            ListItem::new(format!("{mark} {}", auth.label()))
        })
        .collect();
    let mut state = ListState::default();
    if focused {
        state.select(
            AuthType::ALL
                .iter()
                .position(|auth| *auth == modal.selected()),
        );
    }
    let list = List::new(items)
        .block(pane("Type", focused))
        .highlight_style(focus_style());
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_form(frame: &mut Frame, area: Rect, modal: &Modal) {
    let title = modal.selected().label();
    let fields = modal.fields();
    let block = pane(title, matches!(modal.focus(), ModalFocus::Field(_)));
    if fields.is_empty() {
        frame.render_widget(
            Paragraph::new("No credentials are sent.").block(block),
            area,
        );
        return;
    }
    let mut lines = Vec::new();
    let mut focus_line = 0;
    for (index, field) in fields.iter().enumerate() {
        let focused = modal.focus() == ModalFocus::Field(index);
        if focused {
            focus_line = lines.len();
        }
        lines.push(Line::styled(
            field.label().to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        ));
        lines.push(field_text(
            field,
            focused,
            usize::from(area.width.saturating_sub(2)),
        ));
        // A blank line between the rows of Custom headers.
        if modal.selected() == AuthType::CustomHeaders && index % 2 == 1 {
            lines.push(Line::raw(""));
        }
    }
    let visible = usize::from(area.height.saturating_sub(2));
    let skip = (focus_line + 2).saturating_sub(visible);
    let scroll = u16::try_from(skip).unwrap_or(0);
    frame.render_widget(Paragraph::new(lines).block(block).scroll((scroll, 0)), area);
}

/// A field's text, with a cursor mark when it has focus. Text wider than
/// `width` shows its tail, so the end being typed stays visible.
fn field_text(field: &Field, focused: bool, width: usize) -> Line<'static> {
    let mut text = field.display();
    if focused {
        text.push('▏');
    }
    let total = text.chars().count();
    if total > width {
        text = match width {
            0 => String::new(),
            1 => text.chars().skip(total - 1).collect(),
            _ => {
                let tail: String = text.chars().skip(total - (width - 1)).collect();
                format!("…{tail}")
            }
        };
    }
    if focused {
        return Line::styled(text, focus_style());
    }
    Line::raw(text)
}

fn draw_status(frame: &mut Frame, area: Rect, app: &App, now: u64) {
    let (text, level) = app.status(now);
    let style = match level {
        StatusLevel::Fresh => Style::default().fg(Color::White).bg(Color::DarkGray),
        StatusLevel::Stale => Style::default()
            .fg(Color::White)
            .bg(Color::Red)
            .add_modifier(Modifier::BOLD),
        StatusLevel::Notice => Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    };
    frame.render_widget(Paragraph::new(format!(" {text}")).style(style), area);
}

fn draw_search(frame: &mut Frame, area: Rect, app: &App) {
    let block = pane(
        "Search (type to filter · Enter/→ open · Ctrl-Q quit · ? help)",
        !overlay_open(app),
    );
    frame.render_widget(Paragraph::new(app.query.as_str()).block(block), area);
    if let Some(position) = search_cursor(app, area) {
        frame.set_cursor_position(position);
    }
}

/// Where the terminal cursor goes in the search box. There is none while a modal has the
/// input focus: a frame that sets no cursor leaves it hidden.
fn search_cursor(app: &App, area: Rect) -> Option<(u16, u16)> {
    if overlay_open(app) {
        return None;
    }
    let typed = u16::try_from(app.query.chars().count()).unwrap_or(u16::MAX);
    Some((area.x.saturating_add(1).saturating_add(typed), area.y + 1))
}

fn draw_hits(frame: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = app
        .hits
        .iter()
        .map(|hit| ListItem::new(Line::raw(hit.path.clone())))
        .collect();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!("Results ({})", app.hits.len()));
    let selected = (!app.hits.is_empty() && !overlay_open(app)).then_some(app.hit_index);
    draw_centered_list(frame, area, items, block, selected);
}

/// Renders `items` with the selected row kept in the middle of the pane and one empty line
/// after the last item, so the end of the list is visible.
fn draw_centered_list(
    frame: &mut Frame,
    area: Rect,
    mut items: Vec<ListItem>,
    block: Block,
    selected: Option<usize>,
) {
    let height = usize::from(block.inner(area).height);
    let offset = list_offset(items.len(), selected.unwrap_or(0), height);
    items.push(ListItem::new(""));
    let list = List::new(items).block(block).highlight_style(focus_style());
    let mut state = ListState::default().with_offset(offset);
    state.select(selected);
    frame.render_stateful_widget(list, area, &mut state);
}

/// Top pane share of every two-pane column, so paired panes line up.
const TOP_PANE_PERCENT: u16 = 70;

fn split_column(area: Rect) -> [Rect; 2] {
    let parts = Layout::vertical([
        Constraint::Percentage(TOP_PANE_PERCENT),
        Constraint::Percentage(100 - TOP_PANE_PERCENT),
    ])
    .split(area);
    [parts[0], parts[1]]
}

/// A bordered pane; the focused one gets a highlighted border.
fn pane<'a>(title: &'a str, focused: bool) -> Block<'a> {
    let block = Block::default().borders(Borders::ALL).title(title);
    if focused {
        block.border_style(Style::default().fg(FOCUS))
    } else {
        block
    }
}

fn has_focus(app: &App, focus: Focus) -> bool {
    !overlay_open(app)
        && app
            .design
            .as_ref()
            .is_some_and(|design| design.focus == focus)
}

/// The hint for the select-all keys in the corner of the Details pane while composing the
/// query of an operation, as much of it as fits next to the title.
fn select_all_hint(app: &App, width: u16) -> Option<&'static str> {
    let composing = app
        .design
        .as_ref()
        .is_some_and(|design| !design.read_only() && design.run.is_none());
    if !composing {
        return None;
    }
    // Two corners, the title and a space to tell them apart.
    let room = usize::from(width).saturating_sub(2 + "Details".len() + 1);
    [
        "Select all: Ctrl-A · with variables: Alt-A",
        "Select all: Ctrl-A",
    ]
    .into_iter()
    .find(|hint| hint.chars().count() <= room)
}

/// The Details tree over the Description pane.
fn draw_details(frame: &mut Frame, area: Rect, app: &App, rows: &[Row], cursor: Option<usize>) {
    let parts = split_column(area);

    let mut block = pane("Details", has_focus(app, Focus::Details));
    if let Some(hint) = select_all_hint(app, parts[0].width) {
        let hint = Span::styled(hint, Style::default().add_modifier(Modifier::DIM));
        block = block.title_top(Line::from(hint).right_aligned());
    }
    if rows.is_empty() {
        frame.render_widget(Paragraph::new("No matches").block(block), parts[0]);
    } else {
        let items: Vec<ListItem> = rows.iter().map(|row| tree_item(app, row)).collect();
        let selected = cursor.filter(|_| !overlay_open(app));
        draw_centered_list(frame, parts[0], items, block, selected);
    }

    let description = Block::default().borders(Borders::ALL).title("Description");
    frame.render_widget(
        Paragraph::new(app.current_description())
            .wrap(Wrap { trim: false })
            .block(description),
        parts[1],
    );
}

/// The Response pane: status and timing in the title, errors in red, data below.
fn draw_response(frame: &mut Frame, area: Rect, app: &App) {
    let focused = has_focus(app, Focus::Response);
    let Some(run) = app.design.as_ref().and_then(|design| design.run.as_ref()) else {
        return;
    };
    let red = Style::default().fg(Color::Red);
    let dim = Style::default().fg(Color::DarkGray);
    let (title, title_style, lines): (String, Style, Vec<Line>) = match &run.state {
        RunState::Running => (
            "Response · running…".to_string(),
            Style::default(),
            vec![Line::styled("Waiting for the server…", dim)],
        ),
        RunState::Failed(message) => (
            "Response · failed".to_string(),
            red,
            vec![Line::styled(message.clone(), red)],
        ),
        RunState::Done(view) => {
            let title = format!("Response · HTTP {} · {} ms", view.status, view.elapsed_ms);
            let style = if view.ok() { Style::default() } else { red };
            let lines = view
                .body_lines()
                .into_iter()
                .skip(run.scroll)
                .take(usize::from(area.height))
                .map(|(kind, text)| match kind {
                    LineKind::Error => Line::styled(text, red),
                    LineKind::Note => Line::styled(text, dim),
                    LineKind::Data if view.not_json => Line::styled(text, red),
                    LineKind::Data => Line::raw(text),
                })
                .collect();
            (title, style, lines)
        }
    };
    let mut block = pane(&title, focused);
    if title_style != Style::default() && !focused {
        block = block.title_style(title_style);
    }
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// The generated query over the variables it needs.
fn draw_query(frame: &mut Frame, area: Rect, app: &App) {
    let parts = split_column(area);
    let query = app.generated_query();

    let text = match &query {
        Some(Ok(generated)) => generated.text.clone(),
        Some(Err(_)) | None => "Select fields with Space to build the query".to_string(),
    };
    let block = pane("Query", has_focus(app, Focus::Query));
    frame.render_widget(Paragraph::new(text).block(block), parts[0]);

    let rows = app.variable_rows();
    let block = pane("Variables", has_focus(app, Focus::Variables));
    if rows.is_empty() {
        frame.render_widget(Paragraph::new("No variables").block(block), parts[1]);
        return;
    }
    let (cursor, edit) = match &app.design {
        Some(design) => (design.var_cursor.min(rows.len() - 1), design.edit.as_ref()),
        None => (0, None),
    };
    let items: Vec<ListItem> = rows
        .iter()
        .enumerate()
        .map(|(index, row)| variable_item(row, if index == cursor { edit } else { None }))
        .collect();
    let list = List::new(items).block(block).highlight_style(focus_style());
    let mut state = ListState::default();
    if has_focus(app, Focus::Variables) {
        state.select(Some(cursor));
    }
    frame.render_stateful_widget(list, parts[1], &mut state);
}

/// `$name: Type = value`, with a dim note for an empty optional and a red reason for a
/// value that cannot be sent.
fn variable_item(row: &VariableRow, edit: Option<&Edit>) -> ListItem<'static> {
    let mut spans = vec![Span::raw(format!("${}: {}", row.def.name, row.def.ty))];
    let dim = Style::default().fg(Color::DarkGray);
    let red = Style::default().fg(Color::Red);
    match edit {
        Some(Edit::Text(buffer)) => spans.push(Span::raw(format!(" = {buffer}▏"))),
        Some(Edit::Choice { options, index }) => {
            let shown = options
                .get(*index)
                .map(String::as_str)
                .filter(|option| !option.is_empty())
                .unwrap_or("(unset)");
            spans.push(Span::raw(format!(" = ‹{shown}›")));
        }
        None => match &row.state {
            State::Value(value) => {
                spans.push(Span::raw(format!(" = {}", variables::display(value))));
            }
            State::Unset => {
                let note = match &row.def.default {
                    Some(default) => format!("  (default {default})"),
                    None => "  (omitted)".to_string(),
                };
                spans.push(Span::styled(note, dim));
            }
            State::Invalid(reason) if row.raw.is_empty() => {
                spans.push(Span::styled(format!("  ← {reason}"), red));
            }
            State::Invalid(reason) => {
                spans.push(Span::styled(format!(" = {}  ← {reason}", row.raw), red));
            }
        },
    }
    ListItem::new(Line::from(spans))
}

fn tree_item(app: &App, row: &Row) -> ListItem<'static> {
    let editable = app
        .design
        .as_ref()
        .is_some_and(|design| !design.read_only());
    let checkbox = if app.is_selected(&row.node.path) {
        "☑ "
    } else {
        "☐ "
    };
    let marker = match row.node.kind {
        NodeKind::Composite if row.expanded => "▾ ",
        NodeKind::Composite => "▸ ",
        NodeKind::Recursive => "↻ ",
        NodeKind::Required if editable => "▪ ",
        NodeKind::Leaf | NodeKind::Optional if editable => checkbox,
        NodeKind::Leaf | NodeKind::Required | NodeKind::Optional => "• ",
    };
    // A leaf that has arguments also shows whether its `args` group is open.
    let fold = match (row.node.kind, row.node.has_args) {
        (NodeKind::Leaf, true) if row.expanded => "▾ ",
        (NodeKind::Leaf, true) => "▸ ",
        _ => "",
    };
    ListItem::new(Line::raw(format!(
        "{}{fold}{marker}{}",
        "  ".repeat(row.depth),
        row.node.label
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::App;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{backend::TestBackend, style::Color, Terminal};
    use std::time::Duration;
    use vega_core::client::Response;
    use vega_core::config::Settings;
    use vega_core::response;
    use vega_core::schema::IntrospectionResponse;

    const FIXTURE: &str = include_str!("../../../vega-core/tests/fixtures/introspection.json");

    fn render(app: &App, now: u64) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, app, now)).unwrap();
        terminal
    }

    fn screen(terminal: &Terminal<TestBackend>) -> String {
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    fn app() -> App {
        let response: IntrospectionResponse = serde_json::from_str(FIXTURE).unwrap();
        App::new(
            response.data.schema,
            "https://nexus.example.com/graphql",
            1_000,
        )
    }

    fn key(app: &mut App, code: KeyCode) {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn ctrl(app: &mut App, c: char) {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
    }

    fn empty_settings() -> Settings {
        Settings {
            uri: String::new(),
            headers: Vec::new(),
            key_only: false,
        }
    }

    #[test]
    fn the_modal_shows_the_uri_the_types_and_the_hint() {
        let app = App::empty(empty_settings());
        let text = screen(&render(&app, 0));
        assert!(text.contains("Connection"));
        assert!(text.contains("URI"));
        for label in ["None", "Basic", "API Key", "Bearer", "Custom headers"] {
            assert!(text.contains(label), "missing {label}");
        }
        assert!(text.contains("Ctrl-T"));
        assert!(text.contains("Esc"));
    }

    #[test]
    fn the_connection_modal_offers_save_as_and_ctrl_s() {
        let mut app = app();
        ctrl(&mut app, 'k');
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("Save as"), "{text}");
        assert!(text.contains("Ctrl-S save"), "{text}");
    }

    #[test]
    fn the_search_cursor_is_hidden_while_a_modal_has_the_focus() {
        let area = Rect::new(0, 1, 100, 3);
        let mut app = app();
        assert_eq!(search_cursor(&app, area), Some((1, 2)));
        app.help_open = true;
        assert_eq!(search_cursor(&app, area), None);

        let empty = App::empty(empty_settings());
        assert!(empty.modal.is_some());
        assert_eq!(search_cursor(&empty, area), None);
    }

    #[test]
    fn the_search_box_border_is_purple_only_while_it_has_the_focus() {
        let border = |app: &App| render(app, 1_000).backend().buffer()[(0, 1)].fg;
        let mut app = app();
        assert_eq!(border(&app), FOCUS);
        app.help_open = true;
        assert_ne!(border(&app), FOCUS);

        let empty = App::empty(empty_settings());
        assert_ne!(border(&empty), FOCUS);
    }

    #[test]
    fn the_selected_row_is_purple_and_reversed_until_a_modal_opens() {
        let row = |app: &App| {
            let terminal = render(app, 1_000);
            let cell = terminal.backend().buffer()[(1, 5)].clone();
            (cell.fg, cell.modifier.contains(Modifier::REVERSED))
        };
        let mut app = app();
        assert_eq!(row(&app), (FOCUS, true));
        app.help_open = true;
        assert!(!row(&app).1);
    }

    #[test]
    fn the_modal_marks_the_focused_pane_and_field_in_purple() {
        let mut app = App::empty(empty_settings());
        let terminal = render(&app, 0);
        let buffer = terminal.backend().buffer();
        // The URI box has the focus: some border cell and the input line are purple.
        let purple = buffer.content().iter().filter(|cell| cell.fg == FOCUS);
        assert!(purple.count() > 0, "nothing is purple");
        let reversed = buffer
            .content()
            .iter()
            .any(|cell| cell.fg == FOCUS && cell.modifier.contains(Modifier::REVERSED));
        assert!(reversed, "the focused input is not highlighted");

        // Moving the focus to the type list moves the purple border with it.
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        let after = render(&app, 0);
        assert_ne!(after.backend().buffer(), buffer);
    }

    #[test]
    fn the_modal_is_hidden_when_closed() {
        assert!(!screen(&render(&app(), 1_000)).contains("Connection"));
    }

    #[test]
    fn secrets_are_masked_until_revealed() {
        let mut app = App::empty(Settings {
            uri: "https://x.example.com/graphql".into(),
            ..empty_settings()
        });
        ctrl(&mut app, 'k');
        for _ in 0..3 {
            key(&mut app, KeyCode::Down); // Basic, API Key, Bearer
        }
        key(&mut app, KeyCode::Tab);
        for c in "tok-123".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        let text = screen(&render(&app, 0));
        assert!(!text.contains("tok-123"), "secret drawn in clear");
        assert!(text.contains("•••••••"));
        ctrl(&mut app, 't');
        assert!(screen(&render(&app, 0)).contains("tok-123"));
    }

    #[test]
    fn the_modal_shows_validation_errors() {
        let mut app = App::empty(empty_settings());
        key(&mut app, KeyCode::Enter);
        assert!(screen(&render(&app, 0)).contains("The URI is required"));
    }

    #[test]
    fn custom_headers_show_every_row() {
        let mut app = App::empty(Settings {
            uri: "https://x.example.com/graphql".into(),
            ..empty_settings()
        });
        ctrl(&mut app, 'k');
        for _ in 0..4 {
            key(&mut app, KeyCode::Down);
        }
        key(&mut app, KeyCode::Tab);
        for c in "X-One".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        ctrl(&mut app, 'n');
        for c in "X-Two".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        let text = screen(&render(&app, 0));
        assert!(text.contains("X-One"));
        assert!(text.contains("X-Two"));
    }

    #[test]
    fn the_modal_does_not_panic_on_tiny_terminals() {
        let mut app = App::empty(empty_settings());
        ctrl(&mut app, 'k');
        for (w, h) in [
            (20, 5),
            (1, 1),
            (0, 0),
            (3, 40),
            (200, 3),
            (4, 30),
            (5, 30),
            (6, 30),
        ] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|frame| draw(frame, &app, 0)).unwrap();
        }
    }

    #[test]
    fn a_long_uri_shows_its_end_and_the_cursor() {
        let uri = format!("https://{}/the-end", "a".repeat(104));
        let mut app = App::empty(empty_settings());
        for c in uri.chars() {
            key(&mut app, KeyCode::Char(c));
        }
        let text = screen(&render(&app, 0));
        assert!(text.contains("/the-end▏"));
        assert!(!text.contains("https://aaa"));
    }

    #[test]
    fn a_long_masked_secret_shows_the_tail_of_the_bullets() {
        let mut app = App::empty(Settings {
            uri: "https://x.example.com/graphql".into(),
            ..empty_settings()
        });
        ctrl(&mut app, 'k');
        for _ in 0..3 {
            key(&mut app, KeyCode::Down);
        }
        key(&mut app, KeyCode::Tab);
        for c in "s".repeat(80).chars() {
            key(&mut app, KeyCode::Char(c));
        }
        let text = screen(&render(&app, 0));
        assert!(text.contains("•▏"));
        assert!(!text.contains("ssss"));
        ctrl(&mut app, 't');
        assert!(screen(&render(&app, 0)).contains("ssss▏"));
    }

    #[test]
    fn custom_headers_keep_the_focused_row_visible_on_a_short_terminal() {
        let mut app = App::empty(Settings {
            uri: "https://x.example.com/graphql".into(),
            ..empty_settings()
        });
        ctrl(&mut app, 'k');
        for _ in 0..4 {
            key(&mut app, KeyCode::Down);
        }
        key(&mut app, KeyCode::Tab);
        for c in "X-First".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        for name in ["X-Two", "X-Three", "X-Four", "X-Five", "X-Last"] {
            ctrl(&mut app, 'n');
            for c in name.chars() {
                key(&mut app, KeyCode::Char(c));
            }
        }
        let mut terminal = Terminal::new(TestBackend::new(100, 14)).unwrap();
        terminal.draw(|frame| draw(frame, &app, 0)).unwrap();
        let text = screen(&terminal);
        assert!(text.contains("X-Last▏"));
        assert!(!text.contains("X-First"));
    }

    #[test]
    fn browse_shows_results_a_details_tree_and_the_description() {
        let terminal = render(&app(), 1_000 + 3_600);
        let text = screen(&terminal);
        assert!(text.contains("nexus.example.com"));
        assert!(text.contains("schema fetched 1h ago"));
        assert!(text.contains("Results"));
        assert!(text.contains("Query.userRead"));
        assert!(text.contains("Mutation.userCreate"));
        assert!(text.contains("Details"));
        assert!(!text.contains("Preview"));
        assert!(text.contains("▾ userRead(id: ID!): User"));
        assert!(text.contains("• id: ID!"));
        assert!(text.contains("Description"));
        assert!(text.contains("Requires USER_READ"));
    }

    #[test]
    fn design_shows_the_tree_with_the_cursor_row_and_no_results() {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = app();
        for c in "userread".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.design.is_some());
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("Details"));
        assert!(text.contains("▾ userRead(id: ID!): User"));
        assert!(!text.contains("Results"));
        assert!(!text.contains("Search"));
        assert!(text.contains("Requires USER_READ"));
    }

    #[test]
    fn design_does_not_panic_on_tiny_terminals() {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = app();
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.design.is_some());
        for (w, h) in [(20, 5), (1, 1), (0, 0), (3, 40), (80, 24)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|frame| draw(frame, &app, 1_000)).unwrap();
        }
    }

    #[test]
    fn help_modal_shows_the_version_and_every_key() {
        let mut app = app();
        app.help_open = true;
        let text = screen(&render(&app, 1_000));
        assert!(text.contains(&format!("Vega - {}", env!("CARGO_PKG_VERSION"))));
        for (keys, _) in app.help_keys() {
            assert!(text.contains(keys), "missing {keys}");
        }
        assert!(text.contains("Esc to close"));
    }

    #[test]
    fn help_modal_is_hidden_by_default() {
        assert!(!screen(&render(&app(), 1_000)).contains("Vega - "));
    }

    #[test]
    fn help_modal_does_not_panic_on_tiny_terminals() {
        let mut app = app();
        app.help_open = true;
        for (w, h) in [(20, 5), (1, 1), (0, 0), (3, 40)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|frame| draw(frame, &app, 1_000)).unwrap();
        }
    }

    const DESIGN_FIXTURE: &str = include_str!("../../../vega-core/tests/fixtures/design.json");

    fn design_app_on(query: &str) -> App {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let response: IntrospectionResponse = serde_json::from_str(DESIGN_FIXTURE).unwrap();
        let mut app = App::new(response.data.schema, "https://example.com/graphql", 1_000);
        for c in query.chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.design.is_some());
        app
    }

    fn press_key(app: &mut App, code: ratatui::crossterm::event::KeyCode) {
        use ratatui::crossterm::event::{KeyEvent, KeyModifiers};
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    #[test]
    fn an_operation_design_shows_query_and_variables_panes() {
        use ratatui::crossterm::event::KeyCode;
        let mut app = design_app_on("me");
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("Query"));
        assert!(text.contains("Variables"));
        assert!(text.contains("Select fields with Space"));
        assert!(text.contains("No variables"));
        assert!(text.contains("☐ id: ID!"));

        press_key(&mut app, KeyCode::Down);
        press_key(&mut app, KeyCode::Char(' '));
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("☑ id: ID!"));
        assert!(text.contains("query Me {"));
        assert!(!text.contains("Select fields with Space"));
    }

    #[test]
    fn panes_of_a_pair_share_their_height() {
        let app = design_app_on("me");
        let text = screen(&render(&app, 1_000));
        let row_of = |title: &str| text[..text.find(title).unwrap()].chars().count() / 100;
        assert_eq!(row_of("Description"), row_of("Variables"));
    }

    #[test]
    fn required_arguments_are_listed_as_variables() {
        use ratatui::crossterm::event::KeyCode;
        let mut app = design_app_on("node");
        press_key(&mut app, KeyCode::Down); // args
        press_key(&mut app, KeyCode::Down); // id
        press_key(&mut app, KeyCode::Char(' '));
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("$id: ID!"));
    }

    #[test]
    fn a_type_design_has_no_query_or_variables_panes_and_no_checkboxes() {
        let app = design_app_on("postinput");
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("Details"));
        assert!(text.contains("• title: String!"));
        assert!(!text.contains("Variables"));
        assert!(!text.contains("☐"));
    }

    #[test]
    fn selection_panes_do_not_panic_on_tiny_terminals() {
        use ratatui::crossterm::event::KeyCode;
        let mut app = design_app_on("me");
        press_key(&mut app, KeyCode::Down);
        press_key(&mut app, KeyCode::Char(' '));
        for (w, h) in [(20, 5), (1, 1), (0, 0), (3, 40), (80, 24)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|frame| draw(frame, &app, 1_000)).unwrap();
        }
    }

    #[test]
    fn status_bar_is_not_red_when_the_cache_is_fresh() {
        let terminal = render(&app(), 1_000 + 3_600);
        let cell = &terminal.backend().buffer()[(0, 0)];
        assert_ne!(cell.bg, Color::Red);
    }

    #[test]
    fn status_bar_turns_red_when_the_cache_is_over_24_hours_old() {
        let terminal = render(&app(), 1_000 + 24 * 3_600 + 1);
        let cell = &terminal.backend().buffer()[(0, 0)];
        assert_eq!(cell.bg, Color::Red);
        assert!(screen(&terminal).contains("schema fetched 1d ago"));
    }

    #[test]
    fn arguments_show_as_fixed_and_checkbox_rows() {
        use ratatui::crossterm::event::KeyCode;
        let mut app = design_app_on("node");
        press_key(&mut app, KeyCode::Down); // args group
        press_key(&mut app, KeyCode::Right);
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("▾ args"));
        assert!(text.contains("▪ id: ID!"));

        let mut app = design_app_on("users");
        press_key(&mut app, KeyCode::Down);
        press_key(&mut app, KeyCode::Right);
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("☐ role: Role"));
        assert!(text.contains("☐ limit: Int! = 10"));
        press_key(&mut app, KeyCode::Down);
        press_key(&mut app, KeyCode::Char(' '));
        assert!(screen(&render(&app, 1_000)).contains("☑ role: Role"));
    }

    #[test]
    fn a_missing_required_variable_is_listed_with_its_reason() {
        use ratatui::crossterm::event::KeyCode;
        let mut app = design_app_on("node");
        press_key(&mut app, KeyCode::Down);
        press_key(&mut app, KeyCode::Down);
        press_key(&mut app, KeyCode::Char(' ')); // id
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("$id: ID!"));
        assert!(text.contains("← required"));
    }

    #[test]
    fn variable_rows_survive_tiny_terminals() {
        use ratatui::crossterm::event::KeyCode;
        let mut app = design_app_on("node");
        press_key(&mut app, KeyCode::Down);
        press_key(&mut app, KeyCode::Down);
        press_key(&mut app, KeyCode::Char(' '));
        press_key(&mut app, KeyCode::Tab);
        press_key(&mut app, KeyCode::Tab);
        for (w, h) in [(20, 5), (1, 1), (0, 0), (3, 40), (80, 24)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|frame| draw(frame, &app, 1_000)).unwrap();
        }
    }

    #[test]
    fn an_edited_value_shows_next_to_its_variable_and_the_editor_shows_a_cursor() {
        use ratatui::crossterm::event::KeyCode;
        let mut app = design_app_on("node");
        press_key(&mut app, KeyCode::Down);
        press_key(&mut app, KeyCode::Down);
        press_key(&mut app, KeyCode::Char(' ')); // id
        press_key(&mut app, KeyCode::Tab);
        press_key(&mut app, KeyCode::Tab);
        press_key(&mut app, KeyCode::Enter);
        press_key(&mut app, KeyCode::Char('4'));
        press_key(&mut app, KeyCode::Char('2'));
        assert!(screen(&render(&app, 1_000)).contains("$id: ID! = 42▏"));
        press_key(&mut app, KeyCode::Enter);
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("$id: ID! = 42"));
        assert!(!text.contains("required"));
    }

    #[test]
    fn an_invalid_value_shows_its_reason() {
        use ratatui::crossterm::event::KeyCode;
        let mut app = design_app_on("users");
        press_key(&mut app, KeyCode::Down);
        press_key(&mut app, KeyCode::Down);
        press_key(&mut app, KeyCode::Char(' ')); // id, the cursor moves on to name
        press_key(&mut app, KeyCode::Up);
        press_key(&mut app, KeyCode::Up);
        press_key(&mut app, KeyCode::Right);
        for _ in 0..3 {
            press_key(&mut app, KeyCode::Down);
        }
        press_key(&mut app, KeyCode::Char(' ')); // floor
        press_key(&mut app, KeyCode::Tab);
        press_key(&mut app, KeyCode::Tab);
        press_key(&mut app, KeyCode::Enter);
        for c in "abc".chars() {
            press_key(&mut app, KeyCode::Char(c));
        }
        press_key(&mut app, KeyCode::Enter);
        assert!(screen(&render(&app, 1_000)).contains("= abc  ← expected a number"));
    }

    fn view_of(status: u16, body: &str) -> response::ResponseView {
        response::view(&Response {
            status,
            elapsed: Duration::from_millis(12),
            body: body.to_string(),
        })
    }

    /// Query.me with `id` selected, after pressing `r` (request 1 is in flight).
    fn running_app() -> App {
        use ratatui::crossterm::event::KeyCode;
        let mut app = design_app_on("me");
        press_key(&mut app, KeyCode::Down);
        press_key(&mut app, KeyCode::Char(' '));
        press_key(&mut app, KeyCode::Char('r'));
        app
    }

    #[test]
    fn run_mode_shows_query_variables_and_response_without_the_tree() {
        let mut app = running_app();
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("Query"));
        assert!(text.contains("Variables"));
        assert!(text.contains("Response"));
        assert!(text.contains("running"));
        assert!(text.contains("Waiting for the server"));
        assert!(!text.contains("Details"));
        assert!(!text.contains("Description"));

        app.apply_run_result(1, Ok(view_of(200, r#"{"data":{"me":{"id":"1"}}}"#)));
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("HTTP 200"));
        assert!(text.contains("12 ms"));
        assert!(text.contains(r#""id": "1""#));
    }

    #[test]
    fn graphql_errors_and_data_are_both_shown() {
        let mut app = running_app();
        app.apply_run_result(
            1,
            Ok(view_of(
                200,
                r#"{"data":{"me":null},"errors":[{"message":"boom"}]}"#,
            )),
        );
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("error: boom"));
        assert!(text.contains("null"));
    }

    #[test]
    fn failures_and_http_errors_show_in_the_response_pane() {
        let mut app = running_app();
        app.apply_run_result(1, Err("request failed: connection refused".to_string()));
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("failed"));
        assert!(text.contains("connection refused"));

        let mut app = running_app();
        app.apply_run_result(1, Ok(view_of(500, "boom")));
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("HTTP 500"));
        assert!(text.contains("boom"));
    }

    #[test]
    fn scrolling_moves_the_first_visible_line() {
        use ratatui::crossterm::event::KeyCode;
        let mut app = running_app();
        let numbers: Vec<String> = (1000..1060).map(|n| n.to_string()).collect();
        let body = format!(r#"{{"data":[{}]}}"#, numbers.join(","));
        app.apply_run_result(1, Ok(view_of(200, &body)));
        assert!(screen(&render(&app, 1_000)).contains("1000"));
        for _ in 0..3 {
            press_key(&mut app, KeyCode::PageDown);
        }
        let text = screen(&render(&app, 1_000));
        assert!(!text.contains("1000,"), "scrolled past the top");
        assert!(text.contains("1032"));
    }

    #[test]
    fn a_centered_list_keeps_the_selection_in_the_middle_and_one_blank_line_at_the_end() {
        let lines_at = |selected: usize| {
            let items: Vec<ListItem> = (0..30).map(|n| ListItem::new(format!("row{n}"))).collect();
            let mut terminal = Terminal::new(TestBackend::new(20, 12)).unwrap();
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    draw_centered_list(frame, area, items, Block::bordered(), Some(selected));
                })
                .unwrap();
            screen(&terminal)
        };
        let middle = lines_at(15);
        assert!(middle.contains("row10") && middle.contains("row19"));
        assert!(!middle.contains("row9 ") && !middle.contains("row20"));
        let end = lines_at(29);
        assert!(end.contains("row29") && end.contains("row21"));
        assert!(!end.contains("row20"));
        // 20 columns by 12 rows: the row above the bottom border is the blank line.
        let blank: String = end.chars().skip(20 * 10 + 1).take(18).collect();
        assert!(blank.trim().is_empty());
    }

    #[test]
    fn a_notice_shows_in_the_status_bar() {
        use ratatui::crossterm::event::KeyCode;
        let mut app = design_app_on("me");
        press_key(&mut app, KeyCode::Char('r'));
        let text = screen(&render(&app, 1_000));
        assert!(text.contains("select at least one field to run"));
    }

    #[test]
    fn run_mode_does_not_panic_on_tiny_terminals() {
        let mut app = running_app();
        app.apply_run_result(1, Ok(view_of(200, r#"{"data":{"me":{"id":"1"}}}"#)));
        for (w, h) in [(20, 5), (1, 1), (0, 0), (3, 40), (80, 24)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|frame| draw(frame, &app, 1_000)).unwrap();
        }
    }

    #[test]
    fn the_details_corner_hints_at_select_all_only_while_composing_an_operation() {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let key = |app: &mut App, code| app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));

        let browse = app();
        assert!(!screen(&render(&browse, 1_000)).contains("Select all"));

        let mut design = app();
        for c in "userread".chars() {
            key(&mut design, KeyCode::Char(c));
        }
        key(&mut design, KeyCode::Enter);
        let text = screen(&render(&design, 1_000));
        assert!(text.contains("Select all: Ctrl-A"), "{text}");

        // On the same row as the title, to the right of it.
        let terminal = render(&design, 1_000);
        let buffer = terminal.backend().buffer();
        let width = usize::from(buffer.area.width);
        let rows: Vec<String> = buffer
            .content()
            .chunks(width)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect())
            .collect();
        let row = rows.iter().find(|row| row.contains("Details")).unwrap();
        assert!(
            row.find("Details") < row.find("Select all: Ctrl-A"),
            "{row}"
        );
    }

    #[test]
    fn a_type_tree_has_no_select_all_hint() {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = app();
        for c in "user".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        let type_hit = app.hits.iter().position(|h| h.path == "User").unwrap();
        for _ in 0..type_hit {
            app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.design.as_ref().unwrap().read_only());
        assert!(!screen(&render(&app, 1_000)).contains("Select all"));
    }

    #[test]
    fn a_wide_pane_shows_both_select_all_keys_and_a_narrow_one_keeps_its_title() {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = app();
        for c in "userread".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        let wide = Terminal::new(TestBackend::new(200, 30)).unwrap();
        let mut wide = wide;
        wide.draw(|frame| draw(frame, &app, 1_000)).unwrap();
        let text = screen(&wide);
        assert!(
            text.contains("Select all: Ctrl-A · with variables: Alt-A"),
            "{text}"
        );

        for width in [10, 20, 30, 40] {
            let mut narrow = Terminal::new(TestBackend::new(width, 20)).unwrap();
            narrow.draw(|frame| draw(frame, &app, 1_000)).unwrap();
            let text = screen(&narrow);
            assert!(!text.contains("with variables"), "{width}: {text}");
        }
    }
}
