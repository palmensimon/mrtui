pub mod app;
pub mod views;

use std::io;
use tokio::sync::mpsc;

use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    crossterm::{
        event::{
            self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind,
            KeyModifiers, MouseEventKind,
        },
        execute,
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    },
};

use app::{App, AppEvent, AppView};
use views::{
    checkout_panel::{self, CheckoutPanelState},
    help,
    mr_detail::{self, DetailState},
    mr_list,
    settings::{self, SettingsState},
};

use crate::{config::Config, gitlab::GitLabClient};

pub async fn run_tui(config: Config) -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (event_tx, mut event_rx) = mpsc::channel::<AppEvent>(64);

    let client = if config.is_configured() {
        GitLabClient::new(&config.gitlab_url, &config.access_token, config.project_api_paths()).ok()
    } else {
        None
    };

    let mut app = App::new(config, client, event_tx);
    let mut detail_state = DetailState::new();
    let mut settings_state = SettingsState::new(&app.config);
    let mut checkout_state: Option<CheckoutPanelState> = None;

    if app.view == AppView::MrList {
        app.trigger_load();
    }

    loop {
        terminal.draw(|frame| {
            let full_area = frame.area();
            let (content_area, bar_area) = help::split_with_bar(full_area);

            match &app.view {
                AppView::MrList => mr_list::draw(&app, frame, content_area),
                AppView::MrDetail => mr_detail::draw(&app, &mut detail_state, frame, content_area),
                AppView::Settings => settings::draw(&app, &mut settings_state, frame, content_area),
            }

            if let Some(ref cs) = checkout_state {
                checkout_panel::draw(&app, cs, frame, content_area);
            }

            match &app.view {
                AppView::MrList => mr_list::draw_bar(&app, frame, bar_area),
                AppView::MrDetail => mr_detail::draw_bar(&app, &detail_state, frame, bar_area),
                AppView::Settings => help::draw_status_bar(
                    frame, bar_area,
                    &[("Ctrl+S", "save"), ("?", "help")],
                    false,
                    if let Some(e) = &app.error { Some(e.as_str()) } else { app.status_msg.as_deref() },
                ),
            }

            if app.show_help {
                help::draw(frame, full_area, app.help_scroll);
            }
        })?;

        tokio::select! {
            Some(app_event) = event_rx.recv() => {
                let was_settings = app.view == AppView::Settings;
                app.handle_event(app_event);
                if was_settings || app.view == AppView::Settings {
                    settings_state = SettingsState::new(&app.config);
                }
            }

            poll_result = tokio::task::spawn_blocking(|| event::poll(std::time::Duration::from_millis(50))) => {
                if !matches!(poll_result, Ok(Ok(true))) {
                    continue;
                }

                match event::read() {
                    Ok(Event::Mouse(mouse)) => {
                        match mouse.kind {
                            MouseEventKind::ScrollUp => {
                                match app.view {
                                    AppView::MrList => { for _ in 0..3 { app.move_up(); } }
                                    AppView::MrDetail => { detail_state.scroll = detail_state.scroll.saturating_sub(3); }
                                    _ => {}
                                }
                            }
                            MouseEventKind::ScrollDown => {
                                match app.view {
                                    AppView::MrList => { for _ in 0..3 { app.move_down(); } }
                                    AppView::MrDetail => { detail_state.scroll += 3; }
                                    _ => {}
                                }
                            }
                            _ => {}
                        }
                        continue;
                    }
                    Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                        // Global: ? toggles help
                        if key.code == KeyCode::Char('?') {
                            app.show_help = !app.show_help;
                            if app.show_help { app.help_scroll = 0; }
                            continue;
                        }

                        if app.show_help {
                            match key.code {
                                KeyCode::Up | KeyCode::Char('k') => app.help_scroll = app.help_scroll.saturating_sub(1),
                                KeyCode::Down | KeyCode::Char('j') => app.help_scroll += 1,
                                KeyCode::PageUp => app.help_scroll = app.help_scroll.saturating_sub(10),
                                KeyCode::PageDown => app.help_scroll += 10,
                                _ => app.show_help = false,
                            }
                            continue;
                        }

                        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                            break;
                        }
                        if key.code == KeyCode::Char('q') {
                            if app.view == AppView::MrList && !app.local_search_active && checkout_state.is_none() {
                                break;
                            }
                        }

                        app.status_msg = None;
                        if key.code != KeyCode::Char('q') {
                            app.error = None;
                        }

                        if checkout_state.is_some() {
                            checkout_panel::handle_key(&mut app, &mut checkout_state, key);
                            continue;
                        }

                        match app.view.clone() {
                            AppView::MrList => {
                                if key.code == KeyCode::Char('c') && !app.local_search_active {
                                    if let Some(mr) = app.selected_mr().cloned() {
                                        app.current_mr = Some(mr.clone());
                                        checkout_state = Some(CheckoutPanelState::new(&mr));
                                    }
                                } else {
                                    mr_list::handle_key(&mut app, key);
                                    if app.view == AppView::MrDetail {
                                        if let Some(ref mr) = app.current_mr {
                                            detail_state.reset_for_mr(mr);
                                        }
                                    }
                                }
                            }
                            AppView::MrDetail => {
                                if key.code == KeyCode::Char('c') {
                                    if let Some(ref mr) = app.current_mr.clone() {
                                        checkout_state = Some(CheckoutPanelState::new(mr));
                                    }
                                } else {
                                    mr_detail::handle_key(&mut app, &mut detail_state, key);
                                }
                            }
                            AppView::Settings => {
                                settings::handle_key(&mut app, &mut settings_state, key);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;
    Ok(())
}
