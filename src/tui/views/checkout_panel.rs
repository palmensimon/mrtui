use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use tui_textarea::TextArea;

use tokio::sync::mpsc;

use crate::tui::app::{App, AppEvent};

/// Suspend the TUI, run `work` on a blocking thread with full terminal access
/// (so SSH passphrase prompts work), then resume the TUI.
async fn run_with_terminal<F, T>(tx: &mpsc::Sender<AppEvent>, work: F) -> T
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<()>();
    let _ = tx.send(AppEvent::GitSuspendRequest(ready_tx)).await;
    let _ = ready_rx.await; // wait until TUI has left the screen

    let result = tokio::task::spawn_blocking(work).await.expect("git task panicked");

    let _ = tx.send(AppEvent::GitResumed).await;
    result
}

#[derive(Clone, Copy, PartialEq)]
pub enum CheckoutMode {
    Worktree,
    Local,
}

pub struct CheckoutPanelState {
    pub input: TextArea<'static>,
    pub mode: CheckoutMode,
}

impl CheckoutPanelState {
    pub fn new(default_path: &str) -> Self {
        let path = default_path.to_string();

        let mut input = TextArea::from([path.as_str()]);
        input.move_cursor(tui_textarea::CursorMove::End);
        update_input_style(&mut input, CheckoutMode::Worktree);

        Self { input, mode: CheckoutMode::Worktree }
    }
}

fn update_input_style(input: &mut TextArea<'static>, mode: CheckoutMode) {
    let (title, color) = match mode {
        CheckoutMode::Worktree => (
            " Worktree path — Enter to confirm, Esc to cancel ",
            Color::Cyan,
        ),
        CheckoutMode::Local => (
            " Checkout in current repo — Enter to confirm, Esc to cancel ",
            Color::Yellow,
        ),
    };
    input.set_block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(color)),
    );
    input.set_cursor_style(Style::default().add_modifier(Modifier::REVERSED));
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
        KeyCode::Tab => {
            panel.mode = match panel.mode {
                CheckoutMode::Worktree => CheckoutMode::Local,
                CheckoutMode::Local => CheckoutMode::Worktree,
            };
            update_input_style(&mut panel.input, panel.mode);
        }
        KeyCode::Enter => {
            let branch = app.current_mr.as_ref().map(|mr| mr.source_branch.clone()).unwrap_or_default();
            let repo_path = app.repo_path.clone();
            let tx = app.event_tx.clone();
            let mode = panel.mode;

            match mode {
                CheckoutMode::Worktree => {
                    let path = panel.input.lines().first().cloned().unwrap_or_default().trim().to_string();
                    if path.is_empty() { return; }
                    let ide = app.config.ide_command.clone();
                    tokio::spawn(async move {
                        let result = run_with_terminal(&tx, move || {
                            let r = crate::git::add_worktree(&repo_path, &path, &branch);
                            if r.is_ok() {
                                if let Some(cmd) = ide {
                                    std::process::Command::new(&cmd).arg(&path).spawn().ok();
                                }
                            }
                            r
                        }).await;
                        let _ = tx.send(AppEvent::WorktreeCreated(result)).await;
                    });
                }
                CheckoutMode::Local => {
                    tokio::spawn(async move {
                        let result = run_with_terminal(&tx, move || {
                            crate::git::checkout_branch(&repo_path, &branch)
                        }).await;
                        let _ = tx.send(AppEvent::WorktreeCreated(result)).await;
                    });
                }
            }

            *state = None;
        }
        _ => {
            if panel.mode == CheckoutMode::Worktree {
                panel.input.input(key);
            }
        }
    }
}

pub fn draw(app: &App, state: &CheckoutPanelState, frame: &mut Frame, area: Rect) {
    let popup_w = area.width.saturating_sub(8).min(90);
    let popup_h = 6u16;
    let x = area.x + area.width.saturating_sub(popup_w) / 2;
    let y = area.y + area.height.saturating_sub(popup_h) / 2;
    let popup = Rect::new(x, y, popup_w, popup_h);

    frame.render_widget(Clear, popup);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Length(1), Constraint::Length(1), Constraint::Min(0)])
        .split(popup);

    match state.mode {
        CheckoutMode::Worktree => {
            frame.render_widget(&state.input, chunks[0]);
        }
        CheckoutMode::Local => {
            let branch = app.current_mr.as_ref().map(|mr| mr.source_branch.as_str()).unwrap_or("");
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!("  Checkout branch '{branch}' in current repo"),
                    Style::default().fg(Color::White),
                )))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Checkout in current repo — Enter to confirm, Esc to cancel ")
                        .border_style(Style::default().fg(Color::Yellow)),
                ),
                chunks[0],
            );
        }
    }

    let branch = app.current_mr.as_ref().map(|mr| mr.source_branch.as_str()).unwrap_or("");
    let info = Line::from(vec![
        Span::styled(
            format!("  Branch: {branch}  ·  repo: {}", app.repo_path),
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    frame.render_widget(Paragraph::new(info), chunks[1]);

    let mode_label = match state.mode {
        CheckoutMode::Worktree => "[Tab] switch to: Local checkout",
        CheckoutMode::Local => "[Tab] switch to: Worktree",
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("  {mode_label}"),
            Style::default().fg(Color::DarkGray),
        ))),
        chunks[2],
    );
}
