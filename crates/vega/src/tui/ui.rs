//! Rendering. Reads from `App`; never mutates it.

use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::tui::app::{App, StatusLevel, HELP_KEYS};

/// Version shown in the help modal title.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn draw(frame: &mut Frame, app: &App, now: u64) {
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Min(1),
    ])
    .split(frame.area());
    draw_status(frame, rows[0], app, now);
    draw_search(frame, rows[1], app);

    let columns =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).split(rows[2]);
    draw_hits(frame, columns[0], app);
    draw_detail(frame, columns[1], app);
    if app.help_open {
        draw_help(frame, frame.area());
    }
}

fn draw_help(frame: &mut Frame, area: Rect) {
    let key_width = HELP_KEYS
        .iter()
        .map(|(keys, _)| keys.chars().count())
        .max()
        .unwrap_or(0);
    let mut lines = vec![Line::raw("")];
    lines.extend(HELP_KEYS.iter().map(|(keys, description)| {
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
    let title = if app.detail.is_some() {
        "Search"
    } else {
        "Search (type to filter · Enter/→ open · Esc quit · ? help)"
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    frame.render_widget(Paragraph::new(app.query.as_str()).block(block), area);
    if app.detail.is_none() {
        let typed = u16::try_from(app.query.chars().count()).unwrap_or(u16::MAX);
        frame.set_cursor_position((area.x.saturating_add(1).saturating_add(typed), area.y + 1));
    }
}

fn draw_hits(frame: &mut Frame, area: Rect, app: &App) {
    let focused = app.detail.is_none();
    let dim = Style::default().fg(Color::DarkGray);
    let items: Vec<ListItem> = app
        .hits
        .iter()
        .map(|hit| {
            ListItem::new(Line::from(vec![
                Span::raw(hit.path.clone()),
                Span::styled(format!("  {}", hit.summary), dim),
            ]))
        })
        .collect();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!("Results ({})", app.hits.len()));
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .style(if focused { Style::default() } else { dim });
    let mut state = ListState::default();
    if !app.hits.is_empty() {
        state.select(Some(app.hit_index));
    }
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_detail(frame: &mut Frame, area: Rect, app: &App) {
    let preview = if app.detail.is_none() {
        app.preview()
    } else {
        None
    };
    let (detail, selected) = match &app.detail {
        Some(view) => (Some(&view.detail), Some(view.selected)),
        None => (preview.as_ref(), None),
    };

    let title = if selected.is_some() {
        "Detail (Enter/→ follow · Bksp/← back · q quit)"
    } else {
        "Preview"
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    let Some(detail) = detail else {
        frame.render_widget(Paragraph::new("No matches").block(block), area);
        return;
    };

    let inner = block.inner(area);
    frame.render_widget(block, area);
    let parts = Layout::vertical([
        Constraint::Percentage(30),
        Constraint::Percentage(45),
        Constraint::Percentage(25),
    ])
    .split(inner);

    let mut header = Text::default();
    header.push_line(Line::styled(
        detail.title.clone(),
        Style::default().add_modifier(Modifier::BOLD),
    ));
    header.push_line(Line::raw(""));
    for line in detail.body.lines() {
        header.push_line(Line::raw(line.to_string()));
    }
    frame.render_widget(Paragraph::new(header).wrap(Wrap { trim: false }), parts[0]);

    let dim = Style::default().fg(Color::DarkGray);
    let items: Vec<ListItem> = detail
        .nav
        .iter()
        .map(|item| {
            let style = if item.target.is_some() {
                Style::default()
            } else {
                dim
            };
            ListItem::new(Line::styled(item.label.clone(), style))
        })
        .collect();
    let list = List::new(items).highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut state = ListState::default();
    state.select(selected);
    frame.render_stateful_widget(list, parts[1], &mut state);

    let description = selected
        .and_then(|i| detail.nav.get(i))
        .map(|item| item.description.as_str())
        .unwrap_or("");
    let doc_block = Block::default().borders(Borders::TOP).title("Description");
    frame.render_widget(
        Paragraph::new(description)
            .wrap(Wrap { trim: false })
            .block(doc_block),
        parts[2],
    );
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
    fn shows_the_host_hits_and_a_preview() {
        let terminal = render(&app(), 1_000 + 3_600);
        let text = screen(&terminal);
        assert!(text.contains("nexus.example.com"));
        assert!(text.contains("schema fetched 1h ago"));
        assert!(text.contains("Query.userRead"));
        assert!(text.contains("Mutation.userCreate"));
        // Preview of the first hit (Query.userRead) in the detail pane.
        assert!(text.contains("Requires USER_READ"));
    }

    #[test]
    fn help_modal_shows_the_version_and_every_key() {
        let mut app = app();
        app.help_open = true;
        let text = screen(&render(&app, 1_000));
        assert!(text.contains(&format!("Vega - {}", env!("CARGO_PKG_VERSION"))));
        for (keys, _) in HELP_KEYS {
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
