use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use tui_textarea::TextArea;

use crate::{
    git::add_worktree,
    gitlab::MergeRequest,
    tui::app::{App, AppEvent},
};

pub struct CheckoutPanelState {
    pub input: TextArea<'static>,
}

impl CheckoutPanelState {
    pub fn new(mr: &MergeRequest) -> Self {
        let path = format!("../{}", mr.source_branch);

        let mut input = TextArea::from([path.as_str()]);
        input.move_cursor(tui_textarea::CursorMove::End);
        input.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Worktree path — Enter to confirm, Esc to cancel ")
                .border_style(Style::default().fg(Color::Cyan)),
        );
        input.set_cursor_style(Style::default().add_modifier(Modifier::REVERSED));

        Self { input }
    }
}

pub fn handle_key(
    app: &mut App,
    state: &mut Option<CheckoutPanelState>,
    key: KeyEvent,
) {
    let Some(panel) = state else { return };

    match key.code {
        KeyCode::Esc => {
            *state = None;
        }
        KeyCode::Enter => {
            let path = panel.input.lines().first().cloned().unwrap_or_default().trim().to_string();
            if path.is_empty() {
                return;
            }
            // Get branch from current_mr
            let branch = app.current_mr.as_ref().map(|mr| mr.source_branch.clone()).unwrap_or_default();
            let repo_path = app.repo_path.clone();
            let tx = app.event_tx.clone();

            tokio::spawn(async move {
                let result = add_worktree(&repo_path, &path, &branch).await;
                let _ = tx.send(AppEvent::WorktreeCreated(result)).await;
            });

            *state = None;
        }
        _ => {
            panel.input.input(key);
        }
    }
}

pub fn draw(app: &App, state: &CheckoutPanelState, frame: &mut Frame, area: Rect) {
    let popup_w = area.width.saturating_sub(8).min(90);
    let popup_h = 5u16;
    let x = area.x + area.width.saturating_sub(popup_w) / 2;
    let y = area.y + area.height.saturating_sub(popup_h) / 2;
    let popup = Rect::new(x, y, popup_w, popup_h);

    frame.render_widget(Clear, popup);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(popup);

    frame.render_widget(&state.input, chunks[0]);

    let branch = app.current_mr.as_ref().map(|mr| mr.source_branch.as_str()).unwrap_or("");
    let hint = Line::from(vec![
        Span::styled(
            format!("  Branch: {branch}  ·  repo: {}", app.repo_path),
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    frame.render_widget(Paragraph::new(hint), chunks[1]);
}
