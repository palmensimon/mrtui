use tokio::process::Command;

pub async fn add_worktree(repo_path: &str, worktree_path: &str, branch: &str) -> Result<String, String> {
    // Fetch the remote branch first (ignore failure — might be offline or branch already local)
    let _ = Command::new("git")
        .args(["-C", repo_path, "fetch", "origin", branch])
        .output()
        .await;

    // Try with --guess-remote (Git 2.26+): automatically tracks the remote branch
    let output = Command::new("git")
        .args(["-C", repo_path, "worktree", "add", "--guess-remote", worktree_path, branch])
        .output()
        .await
        .map_err(|e| format!("Failed to run git: {e}"))?;

    if output.status.success() {
        return Ok(format!("Worktree created at {worktree_path}"));
    }

    // Fallback: try without --guess-remote (the branch may already exist locally)
    let output2 = Command::new("git")
        .args(["-C", repo_path, "worktree", "add", worktree_path, branch])
        .output()
        .await
        .map_err(|e| format!("Failed to run git: {e}"))?;

    if output2.status.success() {
        Ok(format!("Worktree created at {worktree_path}"))
    } else {
        let stderr = String::from_utf8_lossy(&output2.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            "git worktree add failed".to_string()
        } else {
            stderr
        })
    }
}
