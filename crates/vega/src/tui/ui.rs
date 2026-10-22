//! Rendering. Reads from `App`; never mutates it.

use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::tui::app::{App, Focus, StatusLevel};
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
        let is_operation = app
            .design
            .as_ref()
            .is_some_and(|design| !design.read_only());
        if is_operation {
            let columns =
                Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(rows[1]);
            draw_details(frame, columns[0], app, &tree, cursor);
            draw_query(frame, columns[1], app);
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
        .title("Search (type to filter · Enter/→ open · Ctrl-Q quit · ? help)");
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
        block.border_style(Style::default().fg(Color::Cyan))
    } else {
        block
    }
}

fn has_focus(app: &App, focus: Focus) -> bool {
    app.design
        .as_ref()
        .is_some_and(|design| design.focus == focus)
}

/// The Details tree over the Description pane.
fn draw_details(frame: &mut Frame, area: Rect, app: &App, rows: &[Row], cursor: Option<usize>) {
    let parts = split_column(area);

    let block = pane("Details", has_focus(app, Focus::Details));
    if rows.is_empty() {
        frame.render_widget(Paragraph::new("No matches").block(block), parts[0]);
    } else {
        let items: Vec<ListItem> = rows.iter().map(|row| tree_item(app, row)).collect();
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

    let variables = match &query {
        Some(Ok(generated)) if !generated.variables.is_empty() => generated
            .variables
            .iter()
            .map(|v| format!("${}: {}", v.name, v.ty))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => "No variables".to_string(),
    };
    let block = pane("Variables", has_focus(app, Focus::Variables));
    frame.render_widget(Paragraph::new(variables).block(block), parts[1]);
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
}
