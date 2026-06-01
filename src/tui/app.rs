use std::sync::Arc;
use tokio::sync::mpsc;

use crate::{
    config::Config,
    gitlab::{FileDiff, GitLabClient, MergeRequest},
};

#[derive(Debug, Clone, PartialEq)]
pub enum AppView {
    MrList,
    MrDetail,
    Settings,
}

pub enum AppEvent {
    MrsLoaded(Result<Vec<MergeRequest>, String>),
    DiffLoaded(Result<Vec<FileDiff>, String>),
    MergeDone(Result<(), String>),
    WorktreeCreated(Result<String, String>),
    ConfigSaved(Config),
}

pub struct App {
    pub view: AppView,

    // MR list state
    pub mrs: Vec<MergeRequest>,
    pub loading: bool,
    pub local_search: String,
    pub local_search_active: bool,
    pub selected_row: usize,

    // Detail state (shared with mod.rs overlay logic)
    pub current_mr: Option<MergeRequest>,
    pub current_diff: Vec<FileDiff>,
    pub diff_loading: bool,

    // Global UI state
    pub error: Option<String>,
    pub status_msg: Option<String>,
    pub show_help: bool,
    pub help_scroll: u16,

    // Infrastructure
    pub config: Arc<Config>,
    pub client: Option<Arc<GitLabClient>>,
    pub event_tx: mpsc::Sender<AppEvent>,
    pub repo_path: String,
}

impl App {
    pub fn new(config: Config, client: Option<GitLabClient>, event_tx: mpsc::Sender<AppEvent>) -> Self {
        let repo_path = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let initial_view = if config.is_configured() { AppView::MrList } else { AppView::Settings };

        Self {
            view: initial_view,
            mrs: Vec::new(),
            loading: false,
            local_search: String::new(),
            local_search_active: false,
            selected_row: 0,
            current_mr: None,
            current_diff: Vec::new(),
            diff_loading: false,
            error: None,
            status_msg: None,
            show_help: false,
            help_scroll: 0,
            config: Arc::new(config),
            client: client.map(Arc::new),
            event_tx,
            repo_path,
        }
    }

    pub fn visible_mrs(&self) -> Vec<&MergeRequest> {
        if self.local_search.is_empty() {
            self.mrs.iter().collect()
        } else {
            let q = self.local_search.to_lowercase();
            self.mrs.iter().filter(|m| mr_matches(m, &q)).collect()
        }
    }

    pub fn selected_mr(&self) -> Option<&MergeRequest> {
        let visible = self.visible_mrs();
        visible.get(self.selected_row).copied()
    }

    pub fn move_up(&mut self) {
        if self.selected_row > 0 {
            self.selected_row -= 1;
        }
    }

    pub fn move_down(&mut self) {
        let max = self.visible_mrs().len().saturating_sub(1);
        if self.selected_row < max {
            self.selected_row += 1;
        }
    }

    pub fn trigger_load(&mut self) {
        let Some(client) = self.client.clone() else { return };
        self.loading = true;
        self.error = None;
        let tx = self.event_tx.clone();
        tokio::spawn(async move {
            let result = client.list_mrs().await.map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::MrsLoaded(result)).await;
        });
    }

    pub fn trigger_load_diff(&mut self, project_id: u64, iid: u64) {
        let Some(client) = self.client.clone() else { return };
        self.diff_loading = true;
        self.current_diff.clear();
        let tx = self.event_tx.clone();
        tokio::spawn(async move {
            let result = client.get_diff(project_id, iid).await.map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::DiffLoaded(result)).await;
        });
    }

    pub fn trigger_merge(&mut self, project_id: u64, iid: u64) {
        let Some(client) = self.client.clone() else {
            self.error = Some("Not connected".to_string());
            return;
        };
        let tx = self.event_tx.clone();
        tokio::spawn(async move {
            let result = client.merge_mr(project_id, iid).await.map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::MergeDone(result)).await;
        });
    }

    pub fn handle_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::MrsLoaded(result) => {
                self.loading = false;
                match result {
                    Ok(mrs) => {
                        self.mrs = mrs;
                        // Keep selection in bounds
                        let max = self.mrs.len().saturating_sub(1);
                        if self.selected_row > max { self.selected_row = max; }
                    }
                    Err(e) => self.error = Some(e),
                }
            }
            AppEvent::DiffLoaded(result) => {
                self.diff_loading = false;
                match result {
                    Ok(diff) => self.current_diff = diff,
                    Err(e) => self.error = Some(e),
                }
            }
            AppEvent::MergeDone(result) => {
                match result {
                    Ok(()) => {
                        self.status_msg = Some("Merged successfully!".to_string());
                        self.trigger_load();
                    }
                    Err(e) => self.error = Some(format!("Merge failed: {e}")),
                }
            }
            AppEvent::WorktreeCreated(result) => {
                match result {
                    Ok(msg) => self.status_msg = Some(msg),
                    Err(e) => self.error = Some(format!("Checkout failed: {e}")),
                }
            }
            AppEvent::ConfigSaved(config) => {
                let client = GitLabClient::new(&config.gitlab_url, &config.access_token, config.project_api_paths()).ok();
                self.client = client.map(Arc::new);
                self.config = Arc::new(config);
                self.view = AppView::MrList;
                self.trigger_load();
            }
        }
    }
}

fn mr_matches(mr: &MergeRequest, q: &str) -> bool {
    q.split_whitespace().all(|token| {
        mr.title.to_lowercase().contains(token)
            || mr.source_branch.to_lowercase().contains(token)
            || mr.author.username.to_lowercase().contains(token)
            || mr.author.name.to_lowercase().contains(token)
            || mr.iid.to_string().contains(token)
    })
}
