use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

pub fn draw(frame: &mut Frame, area: Rect, scroll: u16) {
    let popup = centered_rect(62, 88, area);
    frame.render_widget(Clear, popup);

    let sections: &[(&str, &[(&str, &str)])] = &[
        (
            "Global",
            &[
                ("↑/↓  j/k", "Navigate / scroll"),
                ("Ctrl+D / Ctrl+U", "Half page down / up"),
                ("q  Ctrl+C", "Quit"),
                ("?", "Toggle this help"),
            ],
        ),
        (
            "MR List",
            &[
                ("Enter", "Open detail"),
                ("/", "Quick search"),
                ("c", "Checkout for review"),
                ("r", "Refresh"),
                ("s", "Settings"),
            ],
        ),
        (
            "MR Detail",
            &[
                ("[ ]", "Switch tabs"),
                ("m", "Merge MR (when mergeable)"),
                ("c", "Checkout for review"),
                ("b", "Open MR in browser"),
                ("o", "Open image/video URL in browser"),
                ("Esc / ⌫", "Back to list"),
                ("Tab / Shift+Tab", "Next / prev file  (Diff tab)"),
            ],
        ),
        (
            "Checkout Panel",
            &[
                ("Tab", "Switch between review worktree / current location"),
                ("Enter", "Confirm checkout"),
                ("Esc", "Cancel"),
            ],
        ),
        (
            "Settings",
            &[
                ("Tab / ↑↓", "Move between fields"),
                ("Space / Enter", "Edit field"),
                ("Ctrl+S", "Save"),
                ("Esc", "Cancel"),
            ],
        ),
    ];

    let mut lines: Vec<Line> = vec![];
    for (section, bindings) in sections {
        lines.push(Line::from(Span::styled(
            format!("  {section}"),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )));
        for (key, action) in *bindings {
            lines.push(Line::from(vec![
                Span::styled(format!("    {key:<24}"), Style::default().fg(Color::Yellow)),
                Span::styled(*action, Style::default().fg(Color::White)),
            ]));
        }
        lines.push(Line::raw(""));
    }

    let inner_height = popup.height.saturating_sub(2);
    let total = lines.len() as u16;
    let effective_scroll = scroll.min(total.saturating_sub(inner_height));
    let title = if total > inner_height {
        " Keybindings  [?] close  ↑↓ scroll "
    } else {
        " Keybindings  [?] close "
    };

    let outer = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(title);
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);
    frame.render_widget(Paragraph::new(lines).scroll((effective_scroll, 0)), inner);
}

/// Renders the global bottom status bar with `[key] action` hints.
/// `right_msg` (green) or `right_err` (red) appears right-aligned.
pub fn draw_status_bar(
    frame: &mut Frame,
    area: Rect,
    hints: &[(&str, &str)],
    loading: bool,
    right_msg: Option<&str>,
) {
    let mut spans = vec![Span::raw(" ")];
    for (i, (key, action)) in hints.iter().enumerate() {
        if i > 0 { spans.push(Span::raw("  ")); }
        spans.push(Span::styled(
            format!("[{key}] {action}"),
            Style::default().fg(Color::DarkGray),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);

    let label = if loading {
        Some(("loading… ", Color::DarkGray))
    } else {
        right_msg.map(|m| (m, Color::Green))
    };

    if let Some((text, color)) = label {
        let label = format!("{text} ");
        let w = label.chars().count() as u16;
        if w <= area.width {
            frame.render_widget(
                Paragraph::new(Span::styled(label, Style::default().fg(color))),
                Rect { x: area.x + area.width - w, y: area.y, width: w, height: 1 },
            );
        }
    }
}

pub fn split_with_bar(area: Rect) -> (Rect, Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(area);
    (chunks[0], chunks[1])
}

pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_width = area.width * percent_x / 100;
    let popup_height = area.height * percent_y / 100;
    let x = area.x + (area.width.saturating_sub(popup_width)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_height)) / 2;
    Rect { x, y, width: popup_width.min(area.width), height: popup_height.min(area.height) }
}
