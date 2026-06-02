use std::collections::HashMap;
use std::process::Command;
use tokio::process::Command as AsyncCommand;

/// Create or reuse a worktree. Blocking — must be called from spawn_blocking.
/// stdin/stdout/stderr are inherited so SSH passphrase prompts reach the user.
pub fn add_worktree(repo_path: &str, worktree_path: &str, branch: &str) -> Result<String, String> {
    // If path is already a worktree (.git is a file pointer), just switch branch there.
    let git_file = std::path::Path::new(worktree_path).join(".git");
    if git_file.exists() && git_file.is_file() {
        return checkout_branch(worktree_path, branch)
            .map(|_| format!("Switched to {branch} in existing worktree at {worktree_path}"));
    }

    // Fetch (failure is OK — might be offline or key already local)
    let _ = Command::new("git")
        .args(["-C", repo_path, "fetch", "origin", branch])
        .status();

    // Try --guess-remote first (Git 2.26+)
    let ok = Command::new("git")
        .args(["-C", repo_path, "worktree", "add", "--guess-remote", worktree_path, branch])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if ok {
        return Ok(format!("Worktree created at {worktree_path}"));
    }

    // Fallback without --guess-remote
    let ok2 = Command::new("git")
        .args(["-C", repo_path, "worktree", "add", worktree_path, branch])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if ok2 {
        Ok(format!("Worktree created at {worktree_path}"))
    } else {
        Err("git worktree add failed".to_string())
    }
}

/// Checkout a branch in-place. Blocking — must be called from spawn_blocking.
/// stdin/stdout/stderr are inherited so SSH passphrase prompts reach the user.
pub fn checkout_branch(repo_path: &str, branch: &str) -> Result<String, String> {
    let _ = Command::new("git")
        .args(["-C", repo_path, "fetch", "origin", branch])
        .status();

    let ok = Command::new("git")
        .args(["-C", repo_path, "checkout", branch])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if ok {
        Ok(format!("Checked out {branch}"))
    } else {
        Err("git checkout failed".to_string())
    }
}

/// Return a map of branch → absolute worktree path for all active worktrees.
/// Async, read-only — no passphrase needed.
pub async fn list_worktrees(repo_path: &str) -> HashMap<String, String> {
    let Ok(output) = AsyncCommand::new("git")
        .args(["-C", repo_path, "worktree", "list", "--porcelain"])
        .output()
        .await
    else {
        return HashMap::new();
    };

    if !output.status.success() {
        return HashMap::new();
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut map = HashMap::new();
    let mut current_path: Option<String> = None;

    for line in text.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current_path = Some(path.to_string());
        } else if let Some(branch) = line.strip_prefix("branch refs/heads/") {
            if let Some(path) = current_path.take() {
                map.insert(branch.to_string(), path);
            }
        }
    }

    map
}
