use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectEntry {
    pub path: String,
}

impl ProjectEntry {
    /// Bare path with the gitlab_url prefix stripped (for API calls).
    pub fn normalized(&self, gitlab_url: &str) -> String {
        let base = format!("{}/", gitlab_url.trim_end_matches('/'));
        self.path
            .trim()
            .strip_prefix(base.as_str())
            .unwrap_or(self.path.trim())
            .trim_start_matches('/')
            .to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub gitlab_url: String,
    #[serde(default)]
    pub access_token: String,
    pub my_username: Option<String>,
    #[serde(default)]
    pub projects: Vec<ProjectEntry>,
    pub browser: Option<String>,
    pub default_worktree_path: Option<String>,
    pub ide_command: Option<String>,
}

impl Config {
    pub fn is_configured(&self) -> bool {
        !self.gitlab_url.is_empty() && !self.access_token.is_empty()
    }

    /// Returns normalized bare paths for API calls.
    pub fn project_api_paths(&self) -> Vec<String> {
        self.projects
            .iter()
            .filter_map(|e| {
                let p = e.normalized(&self.gitlab_url);
                if p.is_empty() { None } else { Some(p) }
            })
            .collect()
    }

}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("mrtui")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn load_config() -> Config {
    let path = config_path();
    if !path.exists() {
        return Config::default();
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Config::default(),
    };
    toml::from_str(&content).unwrap_or_default()
}

pub fn save_config(config: &Config) -> Result<(), String> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create config dir: {e}"))?;
    let content = toml::to_string_pretty(config).map_err(|e| format!("Serialize error: {e}"))?;
    std::fs::write(config_path(), content).map_err(|e| format!("Write error: {e}"))
}

/// Parse settings textarea text into ProjectEntry list.
/// Format: one URL or path per line.
pub fn parse_projects_text(text: &str) -> Vec<ProjectEntry> {
    text.lines()
        .filter_map(|line| {
            let path = line.trim().to_string();
            if path.is_empty() { None } else { Some(ProjectEntry { path }) }
        })
        .collect()
}

/// Convert ProjectEntry list to textarea display text.
pub fn projects_to_text(projects: &[ProjectEntry]) -> String {
    projects.iter().map(|e| e.path.as_str()).collect::<Vec<_>>().join("\n")
}
