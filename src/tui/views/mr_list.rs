use std::collections::HashMap;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
};
use ratatui::crossterm::event::{KeyCode, KeyEvent};

use crate::{
    gitlab::MergeRequest,
    tui::app::{App, AppView},
};

pub fn handle_key(app: &mut App, key: KeyEvent) {
    if app.local_search_active {
        handle_search_key(app, key);
        return;
    }

    match key.code {
        KeyCode::Up | KeyCode::Char('k') => app.move_up(),
        KeyCode::Down | KeyCode::Char('j') => app.move_down(),
        KeyCode::Enter => {
            if let Some(mr) = app.selected_mr().cloned() {
                app.current_mr = Some(mr);
                app.current_diff.clear();
                app.view = AppView::MrDetail;
            }
        }
        KeyCode::Char('o') => {
            if let Some(mr) = app.selected_mr() {
                open_in_editor(app, &mr.source_branch.clone());
            }
        }
        KeyCode::Char('m') => {
            if let Some(mr) = app.selected_mr().cloned() {
                if mr.is_mergeable() {
                    app.trigger_merge(mr.project_id, mr.iid);
                } else {
                    app.error = Some(format!("Cannot merge: {}", mr.status_label()));
                }
            }
        }
        KeyCode::Char('b') => {
            if let Some(mr) = app.selected_mr().cloned() {
                let url = mr.web_url.clone();
                let browser = app.config.browser.clone();
                tokio::spawn(async move {
                    if let Some(cmd) = browser {
                        std::process::Command::new(&cmd).arg(&url).spawn().ok();
                    } else {
                        open::that(url).ok();
                    }
                });
            }
        }
        KeyCode::Char('r') => app.trigger_load(),
        KeyCode::Char('s') => { app.view = AppView::Settings; app.error = None; }
        KeyCode::Char('/') => {
            app.local_search_active = true;
            app.local_search.clear();
            app.selected_row = 0;
        }
        _ => {}
    }
}

pub fn open_in_editor(app: &mut App, branch: &str) {
    let Some(cmd) = app.config.ide_command.clone() else {
        app.error = Some("No editor configured — set IDE command in Settings [5]".to_string());
        return;
    };
    let Some(path) = app.checked_out_worktrees.get(branch).cloned() else {
        app.error = Some("MR is not checked out".to_string());
        return;
    };
    tokio::spawn(async move {
        std::process::Command::new(&cmd).arg(&path).spawn().ok();
    });
}

fn handle_search_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.local_search_active = false;
            app.local_search.clear();
            app.selected_row = 0;
        }
        KeyCode::Enter => { app.local_search_active = false; }
        KeyCode::Backspace => { app.local_search.pop(); app.selected_row = 0; }
        KeyCode::Char(c) => { app.local_search.push(c); app.selected_row = 0; }
        _ => {}
    }
}

pub fn draw(app: &App, frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(0)])
        .split(area);

    draw_header(app, frame, chunks[0]);

    if app.loading && app.mrs.is_empty() {
        let label = "Fetching MRs…";
        let y = chunks[1].y + chunks[1].height / 2;
        let x = chunks[1].x + chunks[1].width.saturating_sub(label.len() as u16) / 2;
        frame.render_widget(
            Paragraph::new(Span::styled(label, Style::default().fg(Color::DarkGray))),
            Rect::new(x, y, label.len() as u16, 1),
        );
        return;
    }

    draw_table(app, frame, chunks[1]);
}

fn draw_header(app: &App, frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let count_label = format!("{} open MRs", app.mrs.len());
    let right_width = (count_label.len() as u16 + 1).min(area.width.saturating_sub(20));
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(right_width)])
        .split(inner);

    let title = Line::from(vec![
        Span::styled(" mrtui ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled("│ ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            app.config.gitlab_url.trim_start_matches("https://"),
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    frame.render_widget(Paragraph::new(title), chunks[0]);
    if right_width > 0 {
        frame.render_widget(
            Paragraph::new(Span::styled(count_label, Style::default().fg(Color::DarkGray))),
            chunks[1],
        );
    }
}

fn draw_table(app: &App, frame: &mut Frame, area: Rect) {
    let selected_style = Style::default().bg(Color::Rgb(40, 40, 60)).add_modifier(Modifier::BOLD);
    let header_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);

    let header = Row::new(vec![
        Cell::from("TITLE").style(header_style),
        Cell::from("AUTHOR").style(header_style),
        Cell::from("LABELS").style(header_style),
        Cell::from("MILESTONE").style(header_style),
        Cell::from("STATUS").style(header_style),
    ]);

    let visible = app.visible_mrs();
    let current_username = app.current_username.as_deref();
    let checked_out = &app.checked_out_worktrees;
    let rows: Vec<Row> = visible.iter().map(|mr| build_row(mr, current_username, checked_out)).collect();

    let widths = [
        Constraint::Min(36),
        Constraint::Length(16),
        Constraint::Length(22),
        Constraint::Length(16),
        Constraint::Length(16),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::NONE))
        .row_highlight_style(selected_style)
        .highlight_symbol("▶ ")
        .column_spacing(2);

    let mut state = TableState::default().with_selected(Some(app.selected_row));
    frame.render_stateful_widget(table, area, &mut state);
}

fn build_row<'a>(mr: &'a MergeRequest, current_username: Option<&str>, checked_out: &HashMap<String, String>) -> Row<'a> {
    let is_mine = current_username.map(|u| u == mr.author.username.as_str()).unwrap_or(false);
    let is_checked_out = checked_out.contains_key(&mr.source_branch);

    let title_style = if mr.draft || is_mine {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::White)
    };

    let title_line = if is_checked_out {
        Line::from(vec![
            Span::styled("⊙ ", Style::default().fg(Color::Cyan)),
            Span::styled(mr.title.clone(), title_style),
        ])
    } else {
        Line::from(Span::styled(mr.title.clone(), title_style))
    };
    let title_cell = Cell::from(title_line);

    let labels_cell = Cell::from(labels_line(&mr.labels, 2));
    let milestone_str = mr.milestone.as_ref().map(|m| m.title.clone()).unwrap_or_default();
    let status_color = status_color(mr);

    Row::new(vec![
        title_cell,
        Cell::from(mr.author.username.clone()).style(Style::default().fg(Color::DarkGray)),
        labels_cell,
        Cell::from(milestone_str).style(Style::default().fg(Color::White)),
        Cell::from(mr.status_label()).style(Style::default().fg(status_color)),
    ])
}

fn status_color(mr: &MergeRequest) -> Color {
    if mr.draft { return Color::DarkGray; }
    match mr.detailed_merge_status.as_str() {
        "mergeable" => Color::Green,
        "not_approved" => Color::Yellow,
        "blocked_status" | "merge_request_blocked" | "discussions_not_resolved" => Color::Red,
        "ci_still_running" => Color::Yellow,
        _ => Color::White,
    }
}

/// Deterministic color for a label — same name always gets the same color.
/// Uses FNV-1a for good distribution across a 16-color Rgb palette.
pub fn label_color(name: &str) -> Color {
    const PALETTE: &[Color] = &[
        Color::Rgb(255, 127,  80),  // coral
        Color::Rgb(  0, 206, 209),  // dark turquoise
        Color::Rgb(148, 103, 189),  // medium purple
        Color::Rgb(255, 165,   0),  // orange
        Color::Rgb(255, 105, 180),  // hot pink
        Color::Rgb( 50, 205,  50),  // lime green
        Color::Rgb(135, 206, 250),  // light sky blue
        Color::Rgb(255, 215,   0),  // gold
        Color::Rgb(220,  20,  60),  // crimson
        Color::Rgb( 64, 224, 208),  // turquoise
        Color::Rgb(138,  43, 226),  // blue violet
        Color::Rgb(  0, 255, 127),  // spring green
        Color::Rgb( 30, 144, 255),  // dodger blue
        Color::Rgb(255,  20, 147),  // deep pink
        Color::Rgb(100, 221, 120),  // medium sea green
        Color::Rgb(255, 160,  90),  // sandy orange
    ];
    // FNV-1a hash
    let mut h: u64 = 14695981039346656037;
    for b in name.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    PALETTE[(h as usize) % PALETTE.len()]
}

/// Build a colored label line. `max_shown` caps how many labels are displayed before "+N".
pub fn labels_line(labels: &[String], max_shown: usize) -> Line<'static> {
    if labels.is_empty() {
        return Line::raw("");
    }
    let shown = labels.len().min(max_shown);
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (i, label) in labels.iter().take(shown).enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(
            format!("[{label}]"),
            Style::default().fg(label_color(label)),
        ));
    }
    if labels.len() > max_shown {
        spans.push(Span::styled(
            format!(" +{}", labels.len() - max_shown),
            Style::default().fg(Color::DarkGray),
        ));
    }
    Line::from(spans)
}

pub fn draw_bar(app: &App, frame: &mut Frame, area: Rect) {
    let content = if app.local_search_active {
        Line::from(vec![
            Span::styled("/", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(app.local_search.clone(), Style::default().fg(Color::White)),
            Span::styled("█", Style::default().fg(Color::Yellow)),
            Span::styled(
                format!("  ({} matches — Esc clear, Enter confirm)", app.visible_mrs().len()),
                Style::default().fg(Color::DarkGray),
            ),
        ])
    } else if !app.local_search.is_empty() {
        Line::from(vec![
            Span::styled("/", Style::default().fg(Color::DarkGray)),
            Span::styled(app.local_search.clone(), Style::default().fg(Color::White)),
            Span::styled(
                format!("  ({} matches)", app.visible_mrs().len()),
                Style::default().fg(Color::DarkGray),
            ),
        ])
    } else if let Some(err) = &app.error {
        Line::from(Span::styled(format!(" {err}"), Style::default().fg(Color::Red)))
    } else if let Some(msg) = &app.status_msg {
        Line::from(Span::styled(format!(" {msg}"), Style::default().fg(Color::Green)))
    } else {
        hints_line(&[
            ("o", "open"),
            ("m", "merge"),
            ("c", "checkout"),
            ("b", "browser"),
            ("/", "search"),
            ("r", "refresh"),
            ("s", "settings"),
            ("?", "help"),
        ])
    };

    frame.render_widget(Paragraph::new(content), area);

    if app.loading {
        right_label(frame, area, "loading… ", Color::DarkGray);
    }
}

pub fn hints_line(hints: &[(&str, &str)]) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    for (i, (key, action)) in hints.iter().enumerate() {
        if i > 0 { spans.push(Span::raw("  ")); }
        spans.push(Span::styled(
            format!("[{key}] {action}"),
            Style::default().fg(Color::DarkGray),
        ));
    }
    Line::from(spans)
}

pub fn right_label(frame: &mut Frame, area: Rect, label: &str, color: Color) {
    let w = label.len() as u16;
    if w <= area.width {
        frame.render_widget(
            Paragraph::new(Span::styled(label.to_string(), Style::default().fg(color))),
            Rect { x: area.x + area.width - w, y: area.y, width: w, height: 1 },
        );
    }
}
