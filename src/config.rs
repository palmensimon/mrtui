use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectEntry {
    pub path: String,
    #[serde(default = "default_color")]
    pub color: String,
}

fn default_color() -> String {
    "white".to_string()
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
    #[serde(default)]
    pub projects: Vec<ProjectEntry>,
    pub browser: Option<String>,
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
/// Format: one per line, `url-or-path [color]`
pub fn parse_projects_text(text: &str) -> Vec<ProjectEntry> {
    let valid_colors = ["cyan", "green", "blue", "magenta", "red", "yellow", "white"];
    text.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            // Check if last whitespace-separated word is a color name
            if let Some(idx) = trimmed.rfind(' ') {
                let maybe_color = trimmed[idx + 1..].to_lowercase();
                if valid_colors.contains(&maybe_color.as_str()) {
                    return Some(ProjectEntry {
                        path: trimmed[..idx].trim().to_string(),
                        color: maybe_color,
                    });
                }
            }
            Some(ProjectEntry {
                path: trimmed.to_string(),
                color: default_color(),
            })
        })
        .collect()
}

/// Convert ProjectEntry list to textarea display text.
pub fn projects_to_text(projects: &[ProjectEntry]) -> String {
    projects
        .iter()
        .map(|e| {
            if e.color == "white" || e.color.is_empty() {
                e.path.clone()
            } else {
                format!("{} {}", e.path, e.color)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
