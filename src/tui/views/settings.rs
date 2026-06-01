use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tui_textarea::TextArea;

use crate::{
    config::{parse_projects_text, projects_to_text, save_config, Config},
    tui::app::{App, AppEvent, AppView},
};

const F_URL: usize = 0;
const F_TOKEN: usize = 1;
const F_BROWSER: usize = 2;
const F_PROJECTS: usize = 3;
const FIELD_COUNT: usize = 4;

pub struct SettingsState {
    inputs: [TextArea<'static>; FIELD_COUNT],
    active: usize,
    editing: bool,
}

impl SettingsState {
    pub fn new(config: &Config) -> Self {
        let mut inputs = std::array::from_fn(|_| TextArea::default());
        inputs[F_URL] = single_line_area(&config.gitlab_url);
        inputs[F_TOKEN] = single_line_area(&config.access_token);
        inputs[F_BROWSER] = single_line_area(config.browser.as_deref().unwrap_or(""));
        inputs[F_PROJECTS] = multi_line_area(&projects_to_text(&config.projects));

        let mut state = Self { inputs, active: 0, editing: false };
        state.refresh_styles();
        state
    }

    fn refresh_styles(&mut self) {
        for (i, input) in self.inputs.iter_mut().enumerate() {
            let focused = i == self.active;
            let editing = focused && self.editing;
            let multiline = i == F_PROJECTS;
            update_field_block(input, field_label(i), focused, editing, multiline);
        }
    }

    fn move_to(&mut self, idx: usize) {
        self.editing = false;
        self.active = idx;
        self.refresh_styles();
    }

    fn move_next(&mut self) { self.move_to((self.active + 1) % FIELD_COUNT); }
    fn move_prev(&mut self) { self.move_to((self.active + FIELD_COUNT - 1) % FIELD_COUNT); }

    fn first_line(&self, idx: usize) -> String {
        self.inputs[idx].lines().first().cloned().unwrap_or_default().trim().to_string()
    }

    fn projects_text(&self) -> String {
        self.inputs[F_PROJECTS].lines().join("\n")
    }

    fn build_config(&self) -> Result<Config, String> {
        let gitlab_url = self.first_line(F_URL);
        let access_token = self.first_line(F_TOKEN);
        let browser = {
            let s = self.first_line(F_BROWSER);
            if s.is_empty() { None } else { Some(s) }
        };
        let projects = parse_projects_text(&self.projects_text());

        if gitlab_url.is_empty() {
            return Err("GitLab URL is required (e.g. https://gitlab.com)".to_string());
        }
        if access_token.is_empty() {
            return Err("Access token is required".to_string());
        }
        Ok(Config { gitlab_url, access_token, projects, browser })
    }
}

pub fn handle_key(app: &mut App, state: &mut SettingsState, key: KeyEvent) {
    if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
        match state.build_config() {
            Ok(new_cfg) => {
                if let Err(e) = save_config(&new_cfg) {
                    app.error = Some(format!("Save failed: {e}"));
                } else {
                    let tx = app.event_tx.clone();
                    let cfg = new_cfg.clone();
                    tokio::spawn(async move {
                        let _ = tx.send(AppEvent::ConfigSaved(cfg)).await;
                    });
                }
            }
            Err(msg) => app.error = Some(msg),
        }
        return;
    }

    if state.editing {
        match key.code {
            KeyCode::Esc => {
                state.editing = false;
                state.refresh_styles();
            }
            KeyCode::Enter if state.active != F_PROJECTS => {
                state.editing = false;
                state.refresh_styles();
            }
            KeyCode::Tab if state.active == F_PROJECTS => {
                state.editing = false;
                state.refresh_styles();
                state.move_next();
            }
            _ => { state.inputs[state.active].input(key); }
        }
        return;
    }

    match key.code {
        KeyCode::Esc => { app.view = AppView::MrList; app.error = None; }
        KeyCode::Char(' ') | KeyCode::Enter => {
            state.editing = true;
            state.refresh_styles();
        }
        KeyCode::Tab | KeyCode::Down => state.move_next(),
        KeyCode::BackTab | KeyCode::Up => state.move_prev(),
        KeyCode::Char('1') => state.move_to(F_URL),
        KeyCode::Char('2') => state.move_to(F_TOKEN),
        KeyCode::Char('3') => state.move_to(F_BROWSER),
        KeyCode::Char('4') => state.move_to(F_PROJECTS),
        _ => {}
    }
}

pub fn draw(_app: &App, state: &mut SettingsState, frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(4),
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            " mrtui  Settings",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )))
        .block(Block::default().borders(Borders::BOTTOM).border_style(Style::default().fg(Color::DarkGray))),
        chunks[0],
    );

    frame.render_widget(&state.inputs[F_URL], chunks[1]);
    frame.render_widget(&state.inputs[F_TOKEN], chunks[2]);
    frame.render_widget(&state.inputs[F_BROWSER], chunks[3]);
    frame.render_widget(&state.inputs[F_PROJECTS], chunks[4]);
}

fn field_label(idx: usize) -> &'static str {
    match idx {
        F_URL => "[1] GitLab URL",
        F_TOKEN => "[2] Personal Access Token",
        F_BROWSER => "[3] Browser  (optional — e.g. firefox, chromium, /usr/bin/brave)",
        F_PROJECTS => "[4] Projects  (one URL or path per line)",
        _ => "",
    }
}

fn update_field_block(ta: &mut TextArea<'static>, label: &str, focused: bool, editing: bool, multiline: bool) {
    let border_style = if editing {
        Style::default().fg(Color::Green)
    } else if focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let title = if focused && !editing {
        let hint = if multiline { "Space to edit · Tab to move on" } else { "Space to edit" };
        format!(" {label} — {hint} ")
    } else {
        format!(" {label} ")
    };
    ta.set_block(Block::default().borders(Borders::ALL).title(title).border_style(border_style));
    ta.set_cursor_style(if editing {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    });
}

fn single_line_area(value: &str) -> TextArea<'static> {
    let mut ta = TextArea::from([value]);
    ta.move_cursor(tui_textarea::CursorMove::End);
    ta
}

fn multi_line_area(value: &str) -> TextArea<'static> {
    let lines: Vec<&str> = value.lines().collect();
    let mut ta = if lines.is_empty() { TextArea::default() } else { TextArea::from(lines) };
    ta.move_cursor(tui_textarea::CursorMove::End);
    ta
}
