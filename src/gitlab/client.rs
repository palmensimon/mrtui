use reqwest::{Client, header};

use super::types::{ChangesResponse, FileDiff, MergeRequest};

pub struct GitLabClient {
    client: Client,
    base_url: String,
    /// Bare project paths, e.g. ["group/sub/project"]. Empty = use global endpoint.
    projects: Vec<String>,
}

impl GitLabClient {
    pub fn new(gitlab_url: &str, access_token: &str, projects: Vec<String>) -> Result<Self, String> {
        let mut headers = header::HeaderMap::new();
        let token_val = header::HeaderValue::from_str(access_token)
            .map_err(|_| "Invalid access token".to_string())?;
        headers.insert("PRIVATE-TOKEN", token_val);

        let client = Client::builder()
            .default_headers(headers)
            .build()
            .map_err(|e| format!("HTTP client error: {e}"))?;

        Ok(Self {
            client,
            base_url: gitlab_url.trim_end_matches('/').to_string(),
            projects,
        })
    }

    fn project_url(&self, project_id: u64, path: &str) -> String {
        format!("{}/api/v4/projects/{}/{}", self.base_url, project_id, path)
    }

    pub async fn list_mrs(&self) -> Result<Vec<MergeRequest>, String> {
        if !self.projects.is_empty() {
            return self.list_mrs_from_projects(&self.projects.clone()).await;
        }

        // Try global endpoint (works for PATs with api/read_api scope)
        let url = format!(
            "{}/api/v4/merge_requests?state=opened&per_page=100&order_by=updated_at&sort=desc",
            self.base_url
        );
        let resp = self.client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?;

        if resp.status().is_success() {
            return resp.json::<Vec<MergeRequest>>().await.map_err(|e| format!("Parse error: {e}"));
        }

        // 403 = restricted token; fall back to membership-based project discovery
        if resp.status() == 403 {
            return self.list_mrs_via_membership().await;
        }

        Err(format!("API error: {}", resp.status()))
    }

    async fn list_mrs_from_projects(&self, paths: &[String]) -> Result<Vec<MergeRequest>, String> {
        let mut all_mrs: Vec<MergeRequest> = Vec::new();
        let mut last_err: Option<String> = None;

        for path in paths {
            let encoded = path.replace('/', "%2F");
            let url = format!(
                "{}/api/v4/projects/{}/merge_requests?state=opened&per_page=100&order_by=updated_at&sort=desc",
                self.base_url, encoded
            );
            match self.client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    match resp.json::<Vec<MergeRequest>>().await {
                        Ok(mrs) => all_mrs.extend(mrs),
                        Err(e) => last_err = Some(format!("Parse error for {path}: {e}")),
                    }
                }
                Ok(resp) => last_err = Some(format!("API error for {path}: {}", resp.status())),
                Err(e) => last_err = Some(format!("Request failed for {path}: {e}")),
            }
        }

        if all_mrs.is_empty() {
            if let Some(err) = last_err {
                return Err(err);
            }
        }

        all_mrs.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(all_mrs)
    }

    async fn list_mrs_via_membership(&self) -> Result<Vec<MergeRequest>, String> {
        let url = format!(
            "{}/api/v4/projects?membership=true&with_merge_requests_enabled=true&per_page=100&archived=false",
            self.base_url
        );
        let projects: Vec<serde_json::Value> = self.client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to list projects: {e}"))?
            .error_for_status()
            .map_err(|e| format!("Failed to list projects: {e}"))?
            .json()
            .await
            .map_err(|e| format!("Failed to parse projects: {e}"))?;

        let paths: Vec<String> = projects
            .iter()
            .filter_map(|p| p["path_with_namespace"].as_str().map(str::to_string))
            .collect();

        self.list_mrs_from_projects(&paths).await
    }

    pub async fn get_diff(&self, project_id: u64, iid: u64) -> Result<Vec<FileDiff>, String> {
        let url = self.project_url(project_id, &format!("merge_requests/{iid}/changes"));
        let resp: ChangesResponse = self.client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?
            .error_for_status()
            .map_err(|e| format!("API error: {e}"))?
            .json()
            .await
            .map_err(|e| format!("Parse error: {e}"))?;
        Ok(resp.changes)
    }

    pub async fn merge_mr(&self, project_id: u64, iid: u64) -> Result<(), String> {
        let url = self.project_url(project_id, &format!("merge_requests/{iid}/merge"));
        self.client
            .put(&url)
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?
            .error_for_status()
            .map_err(|e| format!("API error: {e}"))?;
        Ok(())
    }
}
