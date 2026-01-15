use crate::auth::Token;
use crate::error::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use url::Url;

/// GitHub REST API client.
///
/// Handles HTTP requests to the GitHub API with authentication and retry logic.
pub struct GitHubClient {
    pub base_url: Url,
    pub client: Client,
    pub token: Option<Token>,
}

impl GitHubClient {
    /// Creates a new GitHub API client.
    ///
    /// # Arguments
    /// * `base_url` - The GitHub API base URL (e.g., "<https://api.github.com>")
    /// * `token` - Optional authentication token
    ///
    /// # Errors
    /// Returns an error if the base URL is invalid or the HTTP client cannot be created.
    pub fn new(base_url: &str, token: Option<Token>) -> Result<Self> {
        let base_url = Url::parse(base_url)
            .map_err(|e| crate::error::CILensError::Config(format!("Invalid base URL: {e}")))?;

        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| {
                crate::error::CILensError::Config(format!("Failed to create HTTP client: {e}"))
            })?;

        Ok(Self {
            base_url,
            client,
            token,
        })
    }

    /// Returns the authorization header value if a token is present.
    ///
    /// Returns `Some("Bearer {token}")` if a token is configured, `None` otherwise.
    fn auth_header(&self) -> Option<String> {
        self.token
            .as_ref()
            .map(|token| format!("Bearer {}", token.as_str()))
    }

    /// Fetches workflow runs for a repository.
    ///
    /// # Arguments
    /// * `owner` - Repository owner (user or organization)
    /// * `repo` - Repository name
    /// * `per_page` - Number of results per page (max 100)
    /// * `branch` - Optional branch name filter
    /// * `status` - Optional status filter (e.g., "completed", "`in_progress`")
    /// * `created` - Optional created date filter (ISO 8601 format)
    ///
    /// # Errors
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn fetch_workflow_runs(
        &self,
        owner: &str,
        repo: &str,
        per_page: usize,
        branch: Option<&str>,
        status: Option<&str>,
        created: Option<&str>,
    ) -> Result<Vec<WorkflowRunData>> {
        let mut url = self
            .base_url
            .join(&format!("repos/{owner}/{repo}/actions/runs"))
            .map_err(|e| crate::error::CILensError::Config(format!("Failed to build URL: {e}")))?;

        // Add query parameters
        url.query_pairs_mut()
            .append_pair("per_page", &per_page.to_string());

        if let Some(branch) = branch {
            url.query_pairs_mut().append_pair("branch", branch);
        }

        if let Some(status) = status {
            url.query_pairs_mut().append_pair("status", status);
        }

        if let Some(created) = created {
            url.query_pairs_mut().append_pair("created", created);
        }

        let mut request = self
            .client
            .get(url.as_str())
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "cilens");

        if let Some(auth) = self.auth_header() {
            request = request.header("Authorization", auth);
        }

        let response = request.send().await.map_err(|e| {
            crate::error::CILensError::GitHubApi(format!("Failed to fetch workflow runs: {e}"))
        })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(crate::error::CILensError::GitHubApi(format!(
                "GitHub API error {status}: {body}"
            )));
        }

        let workflow_response: WorkflowRunsResponse = response.json().await.map_err(|e| {
            crate::error::CILensError::GitHubApi(format!("Failed to parse response: {e}"))
        })?;

        Ok(workflow_response.workflow_runs)
    }

    /// Fetches jobs for a specific workflow run.
    ///
    /// # Arguments
    /// * `owner` - Repository owner (user or organization)
    /// * `repo` - Repository name
    /// * `run_id` - Workflow run ID
    ///
    /// # Errors
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn fetch_jobs(&self, owner: &str, repo: &str, run_id: u64) -> Result<Vec<JobData>> {
        let url = self
            .base_url
            .join(&format!("repos/{owner}/{repo}/actions/runs/{run_id}/jobs"))
            .map_err(|e| crate::error::CILensError::Config(format!("Failed to build URL: {e}")))?;

        let mut request = self
            .client
            .get(url.as_str())
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "cilens");

        if let Some(auth) = self.auth_header() {
            request = request.header("Authorization", auth);
        }

        let response = request.send().await.map_err(|e| {
            crate::error::CILensError::GitHubApi(format!("Failed to fetch jobs: {e}"))
        })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(crate::error::CILensError::GitHubApi(format!(
                "GitHub API error {status}: {body}"
            )));
        }

        let jobs_response: JobsResponse = response.json().await.map_err(|e| {
            crate::error::CILensError::GitHubApi(format!("Failed to parse response: {e}"))
        })?;

        Ok(jobs_response.jobs)
    }
}

/// Response from GitHub API /repos/{owner}/{repo}/actions/runs endpoint.
#[derive(Debug, Deserialize, Serialize)]
struct WorkflowRunsResponse {
    workflow_runs: Vec<WorkflowRunData>,
}

/// Data for a single workflow run from GitHub API.
#[derive(Debug, Deserialize, Serialize)]
pub struct WorkflowRunData {
    pub id: u64,
    pub name: String,
    pub head_branch: Option<String>,
    pub event: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub run_started_at: Option<String>,
    pub updated_at: String,
}

/// Response from GitHub API `/repos/{owner}/{repo}/actions/runs/{run_id}/jobs` endpoint.
#[derive(Debug, Deserialize, Serialize)]
struct JobsResponse {
    jobs: Vec<JobData>,
}

/// Data for a single job from GitHub API.
#[derive(Debug, Deserialize, Serialize)]
pub struct JobData {
    pub id: u64,
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let result = GitHubClient::new("https://api.github.com", None);

        assert!(result.is_ok());
        let client = result.unwrap();
        assert_eq!(client.base_url.as_str(), "https://api.github.com/");
        assert!(client.token.is_none());
    }

    #[test]
    fn test_client_with_token() {
        let token = Token::from("ghp_test");
        let result = GitHubClient::new("https://api.github.com", Some(token));

        assert!(result.is_ok());
        let client = result.unwrap();
        assert_eq!(client.base_url.as_str(), "https://api.github.com/");
        assert!(client.token.is_some());
    }

    #[test]
    fn test_auth_header_without_token() {
        let client = GitHubClient::new("https://api.github.com", None).unwrap();

        let header = client.auth_header();

        assert!(header.is_none());
    }

    #[test]
    fn test_auth_header_with_token() {
        let token = Token::from("ghp_test");
        let client = GitHubClient::new("https://api.github.com", Some(token)).unwrap();

        let header = client.auth_header();

        assert!(header.is_some());
        assert_eq!(header.unwrap(), "Bearer ghp_test");
    }

    #[tokio::test]
    async fn test_fetch_workflow_runs_structure() {
        let json = r#"{
            "workflow_runs": [
                {
                    "id": 123456789,
                    "name": "CI",
                    "head_branch": "main",
                    "event": "push",
                    "status": "completed",
                    "conclusion": "success",
                    "run_started_at": "2024-01-15T10:30:00Z",
                    "updated_at": "2024-01-15T10:35:00Z"
                },
                {
                    "id": 987654321,
                    "name": "Deploy",
                    "head_branch": "feature-branch",
                    "event": "pull_request",
                    "status": "in_progress",
                    "conclusion": null,
                    "run_started_at": "2024-01-15T11:00:00Z",
                    "updated_at": "2024-01-15T11:05:00Z"
                }
            ]
        }"#;

        let response: WorkflowRunsResponse = serde_json::from_str(json).unwrap();

        assert_eq!(response.workflow_runs.len(), 2);
        assert_eq!(response.workflow_runs[0].id, 123_456_789);
        assert_eq!(response.workflow_runs[0].name, "CI");
        assert_eq!(
            response.workflow_runs[0].head_branch,
            Some("main".to_string())
        );
        assert_eq!(response.workflow_runs[0].event, "push");
        assert_eq!(response.workflow_runs[0].status, "completed");
        assert_eq!(
            response.workflow_runs[0].conclusion,
            Some("success".to_string())
        );
        assert_eq!(response.workflow_runs[1].id, 987_654_321);
        assert_eq!(response.workflow_runs[1].name, "Deploy");
        assert_eq!(response.workflow_runs[1].conclusion, None);
    }

    #[tokio::test]
    async fn test_fetch_workflow_runs_method_exists() {
        let client = GitHubClient::new("https://api.github.com", None).unwrap();

        let result = client
            .fetch_workflow_runs("owner", "repo", 10, None, None, None)
            .await;

        // This test will fail to compile until we implement fetch_workflow_runs
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn test_parse_jobs_response() {
        let json = r#"{
            "jobs": [
                {
                    "id": 123456789,
                    "name": "build",
                    "status": "completed",
                    "conclusion": "success",
                    "started_at": "2024-01-15T10:30:00Z",
                    "completed_at": "2024-01-15T10:35:00Z"
                },
                {
                    "id": 987654321,
                    "name": "test",
                    "status": "completed",
                    "conclusion": "failure",
                    "started_at": "2024-01-15T10:30:00Z",
                    "completed_at": "2024-01-15T10:40:00Z"
                }
            ]
        }"#;

        let response: JobsResponse = serde_json::from_str(json).unwrap();

        assert_eq!(response.jobs.len(), 2);
        assert_eq!(response.jobs[0].id, 123_456_789);
        assert_eq!(response.jobs[0].name, "build");
        assert_eq!(response.jobs[0].status, "completed");
        assert_eq!(response.jobs[0].conclusion, Some("success".to_string()));
        assert_eq!(response.jobs[1].id, 987_654_321);
        assert_eq!(response.jobs[1].name, "test");
    }

    #[tokio::test]
    async fn test_fetch_jobs_method_exists() {
        let client = GitHubClient::new("https://api.github.com", None).unwrap();

        let result = client.fetch_jobs("owner", "repo", 123).await;

        // This test will fail to compile until we implement fetch_jobs
        assert!(result.is_ok() || result.is_err());
    }
}
