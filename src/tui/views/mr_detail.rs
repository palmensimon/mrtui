use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    gitlab::{FileDiff, MergeRequest, Pipeline, User},
    gitlab::types::diff_stats,
    tui::app::{App, AppView},
};

use super::mr_list::{hints_line, label_color, open_in_editor, right_label};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DetailTab {
    Description,
    Diff,
}

impl DetailTab {
    fn next(self) -> Self {
        match self {
            DetailTab::Description => DetailTab::Diff,
            DetailTab::Diff => DetailTab::Description,
        }
    }
    fn prev(self) -> Self {
        match self {
            DetailTab::Description => DetailTab::Diff,
            DetailTab::Diff => DetailTab::Description,
        }
    }
    fn label(self) -> &'static str {
        match self {
            DetailTab::Description => "Description",
            DetailTab::Diff => "Diff",
        }
    }
}

pub struct DetailState {
    pub tab: DetailTab,
    pub scroll: u16,
}

impl DetailState {
    pub fn new() -> Self {
        Self { tab: DetailTab::Description, scroll: 0 }
    }

    pub fn reset_for_mr(&mut self, _mr: &MergeRequest) {
        self.tab = DetailTab::Description;
        self.scroll = 0;
    }
}

pub fn handle_key(app: &mut App, state: &mut DetailState, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Backspace => {
            app.view = AppView::MrList;
            app.current_mr = None;
            app.current_diff.clear();
            app.error = None;
            state.scroll = 0;
            return;
        }
        KeyCode::Char(']') => {
            state.tab = state.tab.next();
            state.scroll = 0;
        }
        KeyCode::Char('[') => {
            state.tab = state.tab.prev();
            state.scroll = 0;
        }
        KeyCode::Up | KeyCode::Char('k') => { state.scroll = state.scroll.saturating_sub(1); }
        KeyCode::Down | KeyCode::Char('j') => { state.scroll += 1; }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.scroll = state.scroll.saturating_sub(10);
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.scroll += 10;
        }
        KeyCode::PageUp => { state.scroll = state.scroll.saturating_sub(20); }
        KeyCode::PageDown => { state.scroll += 20; }
        KeyCode::Char('m') => {
            if let Some(ref mr) = app.current_mr.clone() {
                if mr.is_mergeable() {
                    app.trigger_merge(mr.project_id, mr.iid);
                } else {
                    app.error = Some(format!("Cannot merge: {}", mr.status_label()));
                }
            }
            return;
        }
        KeyCode::Char('o') => {
            if let Some(branch) = app.current_mr.as_ref().map(|mr| mr.source_branch.clone()) {
                open_in_editor(app, &branch);
            }
            return;
        }
        KeyCode::Char('a') => {
            if let Some(ref mr) = app.current_mr.clone() {
                app.trigger_approve(mr.project_id, mr.iid);
            }
            return;
        }
        KeyCode::Char('b') => {
            if let Some(ref mr) = app.current_mr {
                let url = mr.web_url.clone();
                let browser = app.config.browser.clone();
                open_url(url, browser);
            }
            return;
        }
        // Jump between files in diff — only active when on the Diff tab
        KeyCode::Tab if state.tab == DetailTab::Diff => {
            if !app.current_diff.is_empty() {
                let offsets = file_line_offsets(&app.current_diff);
                let cur = file_from_scroll(&offsets, state.scroll);
                let next = (cur + 1).min(app.current_diff.len().saturating_sub(1));
                state.scroll = offsets[next];
            }
            return;
        }
        KeyCode::BackTab if state.tab == DetailTab::Diff => {
            if !app.current_diff.is_empty() {
                let offsets = file_line_offsets(&app.current_diff);
                let cur = file_from_scroll(&offsets, state.scroll);
                state.scroll = offsets[cur.saturating_sub(1)];
            }
            return;
        }
        _ => {}
    }

    // Auto-load diff when switching to diff tab
    if state.tab == DetailTab::Diff && app.current_diff.is_empty() && !app.diff_loading {
        if let Some(ref mr) = app.current_mr {
            app.trigger_load_diff(mr.project_id, mr.iid);
        }
    }
}

pub fn draw(app: &App, state: &mut DetailState, frame: &mut Frame, area: Rect) {
    let Some(ref mr) = app.current_mr else { return };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // title  (1 row + bottom border)
            Constraint::Length(3), // info   (author/status/labels row + branch row + bottom border)
            Constraint::Length(2), // tabs   (1 row + bottom border)
            Constraint::Min(0),    // content
            Constraint::Length(2), // footer (top border + 1 row)
        ])
        .split(area);

    let approvers = app.approvals
        .get(&(mr.project_id, mr.iid))
        .map(|v| v.as_slice())
        .unwrap_or(&[]);
    let pipeline = app.pipelines.get(&(mr.project_id, mr.iid));
    draw_title(mr, frame, chunks[0]);
    draw_info(mr, approvers, pipeline, frame, chunks[1]);
    draw_tab_bar(app, state, frame, chunks[2]);

    match state.tab {
        DetailTab::Description => draw_description(mr, state, frame, chunks[3]),
        DetailTab::Diff => draw_diff(app, state, frame, chunks[3]),
    }

    draw_footer(mr, frame, chunks[4]);
}

fn section_block() -> Block<'static> {
    Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(Color::DarkGray))
}

fn draw_title(mr: &MergeRequest, frame: &mut Frame, area: Rect) {
    let block = section_block();
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut spans = vec![Span::raw(" ")];
    if mr.draft {
        spans.push(Span::styled("[Draft] ", Style::default().fg(Color::DarkGray)));
    }
    spans.push(Span::styled(
        mr.title.clone(),
        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
    ));

    let state_color = match mr.state.as_str() {
        "merged" => Color::Magenta,
        "closed" => Color::Red,
        _ => Color::Green,
    };
    spans.push(Span::raw("  "));
    spans.push(Span::styled(
        format!("[{}]", mr.state),
        Style::default().fg(state_color),
    ));

    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
}

fn draw_info(mr: &MergeRequest, approvers: &[User], pipeline: Option<&Pipeline>, frame: &mut Frame, area: Rect) {
    let block = section_block();
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    // Row 1: Author  Status  Pipeline  Labels  Milestone
    let merge_color = match mr.detailed_merge_status.as_str() {
        "mergeable"                => Color::Green,
        "discussions_not_resolved" => Color::Yellow,
        _                          => Color::Gray,
    };
    let (status_text, status_color) = pipeline_status_override(pipeline)
        .unwrap_or_else(|| (mr.status_label().to_string(), merge_color));
    let mut top = vec![
        Span::styled("  Author ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("@{}", mr.author.username), Style::default().fg(Color::Cyan)),
    ];
    if !mr.reviewers.is_empty() {
        top.push(Span::styled(" → ", Style::default().fg(Color::DarkGray)));
        top.push(Span::styled(
            mr.reviewers.iter().map(|r| format!("@{}", r.username)).collect::<Vec<_>>().join(", "),
            Style::default().fg(Color::Cyan),
        ));
    }
    top.extend([
        Span::styled("   Status ", Style::default().fg(Color::DarkGray)),
        Span::styled(status_text, Style::default().fg(status_color)),
    ]);
    if !approvers.is_empty() {
        top.push(Span::styled("   Approved by ", Style::default().fg(Color::DarkGray)));
        let names = approvers.iter()
            .map(|u| format!("@{}", u.username))
            .collect::<Vec<_>>()
            .join(", ");
        top.push(Span::styled(names, Style::default().fg(Color::Green)));
    }
    if !mr.labels.is_empty() {
        top.push(Span::styled("   Labels ", Style::default().fg(Color::DarkGray)));
        for (i, label) in mr.labels.iter().enumerate() {
            if i > 0 { top.push(Span::raw(" ")); }
            top.push(Span::styled(
                format!("[{label}]"),
                Style::default().fg(label_color(label)),
            ));
        }
    }
    if let Some(ref ms) = mr.milestone {
        top.push(Span::styled("   Milestone ", Style::default().fg(Color::DarkGray)));
        top.push(Span::styled(ms.title.clone(), Style::default().fg(Color::White)));
    }
    frame.render_widget(Paragraph::new(Line::from(top)), rows[0]);

    // Row 2: Branch source → target
    let bot = vec![
        Span::styled("  Branch ", Style::default().fg(Color::DarkGray)),
        Span::styled(mr.source_branch.clone(), Style::default().fg(Color::Blue)),
        Span::styled(" → ", Style::default().fg(Color::DarkGray)),
        Span::styled(mr.target_branch.clone(), Style::default().fg(Color::Blue)),
    ];
    frame.render_widget(Paragraph::new(Line::from(bot)), rows[1]);
}

fn draw_tab_bar(app: &App, state: &DetailState, frame: &mut Frame, area: Rect) {
    let block = section_block();
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut spans = vec![Span::raw(" ")];
    for tab in [DetailTab::Description, DetailTab::Diff] {
        let label = match tab {
            DetailTab::Diff if app.diff_loading => "Diff (loading…)".to_string(),
            _ => tab.label().to_string(),
        };
        let style = if tab == state.tab {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        spans.push(Span::styled(label, style));
        spans.push(Span::styled("   ", Style::default()));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
}

fn draw_footer(mr: &MergeRequest, frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!(" {}  ·  {}", mr.references.full, mr.formatted_updated()),
            Style::default().fg(Color::DarkGray),
        ))),
        inner,
    );
}

fn draw_description(mr: &MergeRequest, state: &DetailState, frame: &mut Frame, area: Rect) {
    let text = mr.description.as_deref().unwrap_or("*No description.*");
    let lines = crate::markdown::render(text);
    frame.render_widget(Paragraph::new(lines).scroll((state.scroll, 0)), area);
}

fn open_url(url: String, browser: Option<String>) {
    tokio::spawn(async move {
        if let Some(cmd) = browser {
            std::process::Command::new(&cmd).arg(&url).spawn().ok();
        } else {
            open::that(url).ok();
        }
    });
}

fn draw_diff(app: &App, state: &DetailState, frame: &mut Frame, area: Rect) {
    if app.diff_loading {
        frame.render_widget(
            Paragraph::new(Span::styled(" Loading diff…", Style::default().fg(Color::DarkGray))),
            area,
        );
        return;
    }

    if app.current_diff.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(" No changes found.", Style::default().fg(Color::DarkGray))),
            area,
        );
        return;
    }

    let sidebar_w = (area.width / 4).clamp(16, 32);
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(sidebar_w), Constraint::Min(0)])
        .split(area);

    let offsets = file_line_offsets(&app.current_diff);
    let current_file = file_from_scroll(&offsets, state.scroll);

    draw_diff_sidebar(&app.current_diff, current_file, frame, chunks[0]);
    draw_diff_content(&app.current_diff, state.scroll, frame, chunks[1]);
}

fn draw_diff_sidebar(diff: &[FileDiff], current_file: usize, frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 { return; }

    let name_width = inner.width.saturating_sub(2) as usize;

    let (adds, dels) = diff_stats(diff);
    let header = if adds > 0 || dels > 0 {
        format!(" Files ({})  +{adds}/-{dels}", diff.len())
    } else {
        format!(" Files ({})", diff.len())
    };

    let mut items: Vec<ListItem> = vec![
        ListItem::new(Line::from(Span::styled(
            header,
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
        ))),
    ];

    for (i, file) in diff.iter().enumerate() {
        let name = truncate_path(&file.new_path, name_width);
        let (prefix, style) = if i == current_file {
            ("▶ ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        } else {
            ("  ", Style::default().fg(Color::DarkGray))
        };
        items.push(ListItem::new(Line::from(vec![
            Span::styled(prefix, style),
            Span::styled(name, style),
        ])));
    }

    // Keep current file visible: offset by 1 for the header row
    let selected = current_file + 1;
    let mut list_state = ListState::default().with_selected(Some(selected));
    frame.render_stateful_widget(
        List::new(items).block(Block::default().borders(Borders::NONE)),
        inner,
        &mut list_state,
    );
}

fn draw_diff_content(diff: &[FileDiff], scroll: u16, frame: &mut Frame, area: Rect) {
    let lines = diff_to_lines(diff);
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), area);
}

fn diff_to_lines(diff: &[FileDiff]) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for file in diff {
        let file_label = if file.new_file {
            format!(" [new] {}", file.new_path)
        } else if file.deleted_file {
            format!(" [del] {}", file.old_path)
        } else if file.renamed_file {
            format!(" [ren] {} → {}", file.old_path, file.new_path)
        } else {
            format!(" {}", file.new_path)
        };
        lines.push(Line::from(Span::styled(
            file_label,
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )));
        for diff_line in file.diff.lines() {
            let (text, style) = if diff_line.starts_with('+') && !diff_line.starts_with("+++") {
                (diff_line.to_string(), Style::default().fg(Color::Green))
            } else if diff_line.starts_with('-') && !diff_line.starts_with("---") {
                (diff_line.to_string(), Style::default().fg(Color::Red))
            } else if diff_line.starts_with("@@") {
                (diff_line.to_string(), Style::default().fg(Color::Cyan))
            } else {
                (diff_line.to_string(), Style::default().fg(Color::DarkGray))
            };
            lines.push(Line::from(Span::styled(text, style)));
        }
        lines.push(Line::raw(""));
    }
    lines
}

/// Line number (0-based) where each file's diff starts in the flat line list.
fn file_line_offsets(diff: &[FileDiff]) -> Vec<u16> {
    let mut offsets = Vec::with_capacity(diff.len());
    let mut line = 0u16;
    for file in diff {
        offsets.push(line);
        line += 1; // file header line
        line += file.diff.lines().count() as u16;
        line += 1; // blank separator
    }
    offsets
}

fn pipeline_status_override(pipeline: Option<&Pipeline>) -> Option<(String, Color)> {
    let p = pipeline?;
    match p.status.as_str() {
        "failed"                                                      => Some(("Build Failed".into(),  Color::Red)),
        "running"                                                     => Some(("Build Running".into(), Color::LightBlue)),
        "pending" | "created" | "waiting_for_resource" | "preparing" => Some(("Build Pending".into(), Color::Yellow)),
        _ => None,
    }
}

/// Which file index the current scroll position is within.
fn file_from_scroll(offsets: &[u16], scroll: u16) -> usize {
    offsets.iter().rposition(|&o| o <= scroll).unwrap_or(0)
}

/// Show just the filename if the full path is too long, with a `…/` prefix.
fn truncate_path(path: &str, max_width: usize) -> String {
    if max_width == 0 { return String::new(); }
    if path.len() <= max_width { return path.to_string(); }
    let filename = path.split('/').last().unwrap_or(path);
    let with_ellipsis = format!("…/{filename}");
    if with_ellipsis.len() <= max_width {
        return with_ellipsis;
    }
    // Last resort: truncate from the left
    let keep = max_width.saturating_sub(1);
    format!("…{}", &path[path.len().saturating_sub(keep)..])
}

pub fn draw_bar(app: &App, state: &DetailState, frame: &mut Frame, area: Rect) {
    if let Some(err) = &app.error {
        frame.render_widget(
            Paragraph::new(Span::styled(format!(" {err}"), Style::default().fg(Color::Red))),
            area,
        );
        return;
    }
    if let Some(msg) = &app.status_msg {
        frame.render_widget(
            Paragraph::new(Span::styled(format!(" {msg}"), Style::default().fg(Color::Green))),
            area,
        );
        return;
    }

    let line = match state.tab {
        DetailTab::Description => hints_line(&[
            ("o", "open"), ("m", "merge"), ("a", "approve"), ("c", "checkout"), ("b", "browser"), ("[ ]", "switch tabs"), ("?", "help"),
        ]),
        DetailTab::Diff => hints_line(&[
            ("Ctrl+D/U", "half page"), ("Tab/⇧Tab", "next/prev file"), ("[ ]", "switch tabs"), ("?", "help"),
        ]),
    };
    frame.render_widget(Paragraph::new(line), area);

    if app.diff_loading {
        right_label(frame, area, "loading… ", Color::DarkGray);
    }
}
