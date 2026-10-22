//! Rendering. Reads from `App`; never mutates it.

use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::tui::app::{App, StatusLevel};
use vega_core::selection::{NodeKind, Row};

/// Version shown in the help modal title.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn draw(frame: &mut Frame, app: &App, now: u64) {
    let area = frame.area();
    if app.design.is_some() {
        let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(area);
        draw_status(frame, rows[0], app, now);
        let tree = app.design_rows();
        let cursor = app.design.as_ref().map(|design| design.cursor);
        draw_details(frame, rows[1], app, &tree, cursor);
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

fn draw_status(frame: &mut Frame, area: Rect, app: &App, now: u64) {
    let (text, level) = app.status(now);
    let style = match level {
        StatusLevel::Fresh => Style::default().fg(Color::White).bg(Color::DarkGray),
        StatusLevel::Stale => Style::default()
            .fg(Color::White)
            .bg(Color::Red)
            .add_modifier(Modifier::BOLD),
    };
    frame.render_widget(Paragraph::new(format!(" {text}")).style(style), area);
}

fn draw_search(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Search (type to filter · Enter/→ open · Esc quit · ? help)");
    frame.render_widget(Paragraph::new(app.query.as_str()).block(block), area);
    let typed = u16::try_from(app.query.chars().count()).unwrap_or(u16::MAX);
    frame.set_cursor_position((area.x.saturating_add(1).saturating_add(typed), area.y + 1));
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
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut state = ListState::default();
    if !app.hits.is_empty() {
        state.select(Some(app.hit_index));
    }
    frame.render_stateful_widget(list, area, &mut state);
}

/// The Details tree over the Description pane.
fn draw_details(frame: &mut Frame, area: Rect, app: &App, rows: &[Row], cursor: Option<usize>) {
    let parts =
        Layout::vertical([Constraint::Percentage(70), Constraint::Percentage(30)]).split(area);

    let block = Block::default().borders(Borders::ALL).title("Details");
    if rows.is_empty() {
        frame.render_widget(Paragraph::new("No matches").block(block), parts[0]);
    } else {
        let items: Vec<ListItem> = rows.iter().map(tree_item).collect();
        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
        let mut state = ListState::default();
        state.select(cursor);
        frame.render_stateful_widget(list, parts[0], &mut state);
    }

    let description = Block::default().borders(Borders::ALL).title("Description");
    frame.render_widget(
        Paragraph::new(app.current_description())
            .wrap(Wrap { trim: false })
            .block(description),
        parts[1],
    );
}

fn tree_item(row: &Row) -> ListItem<'static> {
    let marker = match row.node.kind {
        NodeKind::Composite if row.expanded => "▾ ",
        NodeKind::Composite => "▸ ",
        NodeKind::Recursive => "↻ ",
        NodeKind::Leaf => "• ",
    };
    ListItem::new(Line::raw(format!(
        "{}{marker}{}",
        "  ".repeat(row.depth),
        row.node.label
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::App;
    use ratatui::{backend::TestBackend, style::Color, Terminal};
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
}
