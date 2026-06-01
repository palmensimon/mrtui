use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct User {
    pub username: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct References {
    pub full: String, // e.g. "group/project!42"
}

#[derive(Debug, Clone, Deserialize)]
pub struct Milestone {
    pub title: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Pipeline {
    pub status: String,
}

impl Pipeline {
    pub fn icon(&self) -> &str {
        match self.status.as_str() {
            "success" => "✓",
            "failed" => "✗",
            "running" | "pending" => "◌",
            _ => "○",
        }
    }

    pub fn color_name(&self) -> &str {
        match self.status.as_str() {
            "success" => "green",
            "failed" => "red",
            "running" | "pending" => "yellow",
            _ => "dark_gray",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct MergeRequest {
    pub iid: u64,
    pub project_id: u64,
    pub references: References,
    pub title: String,
    pub description: Option<String>,
    pub state: String,
    pub source_branch: String,
    pub target_branch: String,
    pub author: User,
    #[serde(default)]
    pub reviewers: Vec<User>,
    #[serde(default)]
    pub detailed_merge_status: String,
    #[serde(default)]
    pub draft: bool,
    pub web_url: String,
    pub updated_at: String,
    #[serde(default)]
    pub labels: Vec<String>,
    pub milestone: Option<Milestone>,
    /// Returned by the list endpoint
    pub pipeline: Option<Pipeline>,
    /// Returned only by the single-MR GET endpoint
    pub head_pipeline: Option<Pipeline>,
}

impl MergeRequest {
    pub fn status_label(&self) -> &str {
        if self.draft {
            return "Draft";
        }
        match self.detailed_merge_status.as_str() {
            "mergeable" => "Mergeable",
            "not_approved" => "Needs Approval",
            "checking" => "Checking",
            "blocked_status" => "Blocked",
            "discussions_not_resolved" => "Open Discussions",
            "merge_request_blocked" => "MR Blocked",
            "ci_must_pass" => "CI Required",
            "ci_still_running" => "CI Running",
            _ => "Open",
        }
    }

    pub fn is_mergeable(&self) -> bool {
        self.detailed_merge_status == "mergeable"
    }

    /// Returns the best available pipeline, preferring head_pipeline (single-MR endpoint)
    /// and falling back to pipeline (list endpoint).
    pub fn any_pipeline(&self) -> Option<&Pipeline> {
        self.head_pipeline.as_ref().or(self.pipeline.as_ref())
    }

    pub fn formatted_updated(&self) -> String {
        self.updated_at.get(..10).unwrap_or(&self.updated_at).to_string()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Note {
    pub body: String,
    pub author: User,
    pub created_at: String,
    #[serde(default)]
    pub system: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileDiff {
    pub old_path: String,
    pub new_path: String,
    pub diff: String,
    #[serde(default)]
    pub new_file: bool,
    #[serde(default)]
    pub deleted_file: bool,
    #[serde(default)]
    pub renamed_file: bool,
}

#[derive(Debug, Deserialize)]
pub struct ChangesResponse {
    pub changes: Vec<FileDiff>,
}

pub fn diff_stats(diffs: &[FileDiff]) -> (usize, usize) {
    let mut additions = 0usize;
    let mut deletions = 0usize;
    for file in diffs {
        for line in file.diff.lines() {
            if line.starts_with('+') && !line.starts_with("+++") {
                additions += 1;
            } else if line.starts_with('-') && !line.starts_with("---") {
                deletions += 1;
            }
        }
    }
    (additions, deletions)
}
