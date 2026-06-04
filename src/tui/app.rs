use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::{
    config::Config,
    gitlab::{FileDiff, GitLabClient, MergeRequest, Pipeline, User},
    gitlab::types::CurrentUser,
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
    ApproveDone(Result<(), String>),
    ApprovalsLoaded(HashMap<(u64, u64), Vec<User>>),
    PipelinesLoaded(HashMap<(u64, u64), Pipeline>),
    WorktreeCreated(Result<String, String>),
    WorktreesLoaded(HashMap<String, String>),
    UserLoaded(Result<CurrentUser, String>),
    ConfigSaved(Config),
    /// Git task requests the TUI to suspend so it can use the terminal.
    GitSuspendRequest(tokio::sync::oneshot::Sender<()>),
    /// Git task is done; the TUI can resume.
    GitResumed,
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

    // Current user for author highlighting
    pub current_username: Option<String>,
    pub current_user_id: Option<u64>,

    // Approvals keyed by (project_id, iid)
    pub approvals: HashMap<(u64, u64), Vec<User>>,
    // Most recent pipeline per MR keyed by (project_id, iid)
    pub pipelines: HashMap<(u64, u64), Pipeline>,

    // branch → absolute worktree path for all active worktrees
    pub checked_out_worktrees: HashMap<String, String>,

    // Infrastructure
    pub config: Arc<Config>,
    pub client: Option<Arc<GitLabClient>>,
    pub event_tx: mpsc::Sender<AppEvent>,
    pub repo_path: String,

    // Pending refresh state — all three must arrive before we swap the live data in.
    // This prevents the status column from flickering between partial states.
    pending_mrs: Option<Vec<MergeRequest>>,
    pending_pipelines: Option<HashMap<(u64, u64), Pipeline>>,
    pending_approvals: Option<HashMap<(u64, u64), Vec<User>>>,
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
            current_username: None,
            current_user_id: None,
            approvals: HashMap::new(),
            pipelines: HashMap::new(),
            checked_out_worktrees: HashMap::new(),
            config: Arc::new(config),
            client: client.map(Arc::new),
            event_tx,
            repo_path,
            pending_mrs: None,
            pending_pipelines: None,
            pending_approvals: None,
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

    pub fn trigger_approve(&mut self, project_id: u64, iid: u64) {
        let Some(client) = self.client.clone() else {
            self.error = Some("Not connected".to_string());
            return;
        };
        let tx = self.event_tx.clone();
        tokio::spawn(async move {
            let result = client.approve_mr(project_id, iid).await.map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::ApproveDone(result)).await;
        });
    }

    fn trigger_load_pipelines_for(&mut self, keys: Vec<(u64, u64)>) {
        let Some(client) = self.client.clone() else { return };
        if keys.is_empty() { return; }
        let tx = self.event_tx.clone();
        tokio::spawn(async move {
            let mut set = tokio::task::JoinSet::new();
            for (pid, iid) in keys {
                let client = client.clone();
                set.spawn(async move {
                    let pipeline = client.get_pipeline_status(pid, iid).await.unwrap_or(None);
                    ((pid, iid), pipeline)
                });
            }
            let mut results = HashMap::new();
            while let Some(Ok(((pid, iid), Some(pipeline)))) = set.join_next().await {
                results.insert((pid, iid), pipeline);
            }
            let _ = tx.send(AppEvent::PipelinesLoaded(results)).await;
        });
    }

    fn trigger_load_approvals_for(&mut self, keys: Vec<(u64, u64)>) {
        let Some(client) = self.client.clone() else { return };
        if keys.is_empty() { return; }
        let tx = self.event_tx.clone();
        tokio::spawn(async move {
            let mut set = tokio::task::JoinSet::new();
            for (pid, iid) in keys {
                let client = client.clone();
                set.spawn(async move {
                    let approvers = client.get_approvals(pid, iid).await.unwrap_or_default();
                    ((pid, iid), approvers)
                });
            }
            let mut results = HashMap::new();
            while let Some(Ok(((pid, iid), approvers))) = set.join_next().await {
                results.insert((pid, iid), approvers);
            }
            let _ = tx.send(AppEvent::ApprovalsLoaded(results)).await;
        });
    }

    /// Atomically apply pending MR/pipeline/approval data once all three have arrived.
    /// Pipeline data is merged (not replaced) so existing "failed" indicators survive
    /// transient fetch errors for individual MRs.
    fn try_apply_pending(&mut self) {
        if self.pending_mrs.is_none() || self.pending_pipelines.is_none() || self.pending_approvals.is_none() {
            return;
        }
        let new_mrs = self.pending_mrs.take().unwrap();
        let new_pipelines = self.pending_pipelines.take().unwrap();
        let new_approvals = self.pending_approvals.take().unwrap();

        // Merge new pipeline results into existing map so that an MR whose pipeline
        // fetch returned None (transient error) keeps its previous entry.
        for (key, val) in new_pipelines {
            self.pipelines.insert(key, val);
        }
        let current_keys: std::collections::HashSet<_> =
            new_mrs.iter().map(|m| (m.project_id, m.iid)).collect();
        self.pipelines.retain(|k, _| current_keys.contains(k));

        self.approvals = new_approvals;
        let max = new_mrs.len().saturating_sub(1);
        if self.selected_row > max { self.selected_row = max; }
        self.mrs = new_mrs;
        self.loading = false;
    }

    pub fn trigger_load_user(&mut self) {
        let Some(client) = self.client.clone() else { return };
        let tx = self.event_tx.clone();
        tokio::spawn(async move {
            let result = client.get_current_user().await;
            let _ = tx.send(AppEvent::UserLoaded(result)).await;
        });
    }

    pub fn trigger_check_worktrees(&mut self) {
        let repo_path = self.repo_path.clone();
        let tx = self.event_tx.clone();
        tokio::spawn(async move {
            let worktrees = crate::git::list_worktrees(&repo_path).await;
            let _ = tx.send(AppEvent::WorktreesLoaded(worktrees)).await;
        });
    }

    pub fn handle_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::MrsLoaded(result) => {
                match result {
                    Ok(mrs) => {
                        let keys: Vec<(u64, u64)> = mrs.iter().map(|m| (m.project_id, m.iid)).collect();
                        // Reset pending state for this refresh cycle.
                        self.pending_mrs = Some(mrs);
                        self.pending_pipelines = None;
                        self.pending_approvals = None;
                        if keys.is_empty() {
                            // No secondary fetches needed; apply immediately.
                            self.pending_pipelines = Some(HashMap::new());
                            self.pending_approvals = Some(HashMap::new());
                            self.try_apply_pending();
                        } else {
                            self.trigger_load_pipelines_for(keys.clone());
                            self.trigger_load_approvals_for(keys);
                        }
                    }
                    Err(e) => {
                        self.loading = false;
                        self.error = Some(e);
                    }
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
            AppEvent::ApproveDone(result) => {
                match result {
                    Ok(()) => {
                        self.status_msg = Some("Approved!".to_string());
                        self.trigger_load();
                    }
                    Err(e) => self.error = Some(format!("Approve failed: {e}")),
                }
            }
            AppEvent::ApprovalsLoaded(approvals) => {
                self.pending_approvals = Some(approvals);
                self.try_apply_pending();
            }
            AppEvent::PipelinesLoaded(pipelines) => {
                self.pending_pipelines = Some(pipelines);
                self.try_apply_pending();
            }
            AppEvent::WorktreeCreated(result) => {
                match result {
                    Ok(msg) => {
                        self.status_msg = Some(msg);
                        self.trigger_check_worktrees();
                    }
                    Err(e) => self.error = Some(format!("Checkout failed: {e}")),
                }
            }
            AppEvent::WorktreesLoaded(worktrees) => {
                self.checked_out_worktrees = worktrees;
            }
            AppEvent::UserLoaded(result) => {
                if let Ok(user) = result {
                    self.current_user_id = Some(user.id);
                    self.current_username = Some(user.username);
                }
            }
            AppEvent::GitSuspendRequest(_) | AppEvent::GitResumed => {
                // Handled directly in tui/mod.rs before reaching here.
            }
            AppEvent::ConfigSaved(config) => {
                let client = GitLabClient::new(&config.gitlab_url, &config.access_token, config.project_api_paths()).ok();
                self.client = client.map(Arc::new);
                self.config = Arc::new(config);
                self.view = AppView::MrList;
                self.trigger_load();
                self.trigger_load_user();
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
            || mr.status_label().to_lowercase().contains(token)
            || mr.milestone.as_ref().map(|m| m.title.to_lowercase().contains(token)).unwrap_or(false)
            || mr.labels.iter().any(|l| l.to_lowercase().contains(token))
    })
}
