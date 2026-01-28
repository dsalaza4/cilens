use chrono::{DateTime, Utc};
use reqwest::Method;
use serde::Deserialize;

use super::core::{GitHubClient, PAGE_SIZE};
use crate::error::Result;

/// GitHub Actions workflow run response
#[derive(Debug, Deserialize)]
struct WorkflowRunsResponse {
    workflow_runs: Vec<WorkflowRun>,
}

/// Individual workflow run from GitHub API
#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowRun {
    pub id: u64,
    pub name: Option<String>,
    pub path: Option<String>,
    pub run_started_at: DateTime<Utc>,
}

/// GitHub Actions jobs response
#[derive(Debug, Deserialize)]
struct JobsResponse {
    total_count: usize,
    jobs: Vec<Job>,
}

/// Individual job from GitHub API
#[derive(Debug, Clone, Deserialize)]
pub struct Job {
    pub id: u64,
    pub run_id: u64,
    pub name: String,
    pub workflow_name: Option<String>,
    pub status: String,
    pub conclusion: Option<String>,
    pub run_attempt: u32,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub html_url: String,
}

impl GitHubClient {
    /// Fetch a page of workflow runs from GitHub Actions.
    ///
    /// Low-level method that fetches a single page of runs without any business logic.
    ///
    /// # Arguments
    ///
    /// * `page` - Page number to fetch (1-indexed)
    ///
    /// # Returns
    ///
    /// Vector of `WorkflowRun` structs from the API response
    ///
    /// # Errors
    ///
    /// Returns an error if the API request fails or response cannot be parsed
    pub async fn fetch_runs(&self, page: usize) -> Result<Vec<WorkflowRun>> {
        let path = format!(
            "repos/{}/{}/actions/runs?per_page={}&page={}",
            self.owner, self.repo, PAGE_SIZE, page
        );

        let url = self
            .api_url
            .join(&path)
            .map_err(|e| crate::error::CILensError::Config(format!("Invalid URL: {e}")))?;

        let runs_response: WorkflowRunsResponse = self.execute_request(Method::GET, url).await?;

        Ok(runs_response.workflow_runs)
    }

    /// Fetch jobs for a specific workflow run up to the specified limit.
    ///
    /// Low-level method that fetches jobs for a single run without transformation.
    ///
    /// # Arguments
    ///
    /// * `run_id` - Workflow run ID
    /// * `limit` - Maximum number of jobs to fetch for this run
    ///
    /// # Returns
    ///
    /// Vector of Job structs from the API (up to `limit` jobs)
    ///
    /// # Errors
    ///
    /// Returns an error if the API request fails or response cannot be parsed
    pub async fn fetch_jobs_for_run(&self, run_id: u64, limit: usize) -> Result<Vec<Job>> {
        let mut all_jobs = Vec::new();
        let mut page = 1;

        loop {
            let path = format!(
                "repos/{}/{}/actions/runs/{}/jobs?per_page={}&page={}",
                self.owner, self.repo, run_id, PAGE_SIZE, page
            );

            let url = self
                .api_url
                .join(&path)
                .map_err(|e| crate::error::CILensError::Config(format!("Invalid URL: {e}")))?;

            let jobs_response: JobsResponse = self.execute_request(Method::GET, url).await?;

            all_jobs.extend(jobs_response.jobs);

            // Stop if we've reached the requested limit
            if all_jobs.len() >= limit {
                all_jobs.truncate(limit);
                break;
            }

            // Stop if we've fetched all available jobs
            if all_jobs.len() >= jobs_response.total_count {
                break;
            }

            page += 1;
        }

        Ok(all_jobs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_runs_url_construction() {
        // Test that the URL path is correctly formatted
        let owner = "test-owner";
        let repo = "test-repo";
        let page = 1;

        let expected_path =
            format!("repos/{owner}/{repo}/actions/runs?per_page={PAGE_SIZE}&page={page}");

        assert_eq!(
            expected_path,
            "repos/test-owner/test-repo/actions/runs?per_page=100&page=1"
        );
    }

    #[test]
    fn test_fetch_runs_url_construction_with_different_page() {
        let owner = "test-owner";
        let repo = "test-repo";
        let page = 5;

        let expected_path =
            format!("repos/{owner}/{repo}/actions/runs?per_page={PAGE_SIZE}&page={page}");

        assert_eq!(
            expected_path,
            "repos/test-owner/test-repo/actions/runs?per_page=100&page=5"
        );
    }

    #[test]
    fn test_fetch_jobs_for_run_url_construction() {
        let owner = "test-owner";
        let repo = "test-repo";
        let run_id = 12345u64;
        let page = 1;

        let expected_path = format!(
            "repos/{owner}/{repo}/actions/runs/{run_id}/jobs?per_page={PAGE_SIZE}&page={page}"
        );

        assert_eq!(
            expected_path,
            "repos/test-owner/test-repo/actions/runs/12345/jobs?per_page=100&page=1"
        );
    }

    #[test]
    fn test_fetch_jobs_for_run_url_construction_with_different_run_id() {
        let owner = "kubernetes";
        let repo = "kubernetes";
        let run_id = 9_876_543_210u64;
        let page = 3;

        let expected_path = format!(
            "repos/{owner}/{repo}/actions/runs/{run_id}/jobs?per_page={PAGE_SIZE}&page={page}"
        );

        assert_eq!(
            expected_path,
            "repos/kubernetes/kubernetes/actions/runs/9876543210/jobs?per_page=100&page=3"
        );
    }

    #[test]
    fn test_workflow_runs_response_deserialization() {
        let json = r#"{
            "total_count": 2,
            "workflow_runs": [
                {
                    "id": 123,
                    "name": "CI",
                    "path": ".github/workflows/ci.yml",
                    "run_started_at": "2021-01-01T00:00:00Z"
                },
                {
                    "id": 456,
                    "name": null,
                    "path": ".github/workflows/test.yml",
                    "run_started_at": "2021-01-02T00:00:00Z"
                }
            ]
        }"#;

        let response: WorkflowRunsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.workflow_runs.len(), 2);
        assert_eq!(response.workflow_runs[0].id, 123);
        assert_eq!(response.workflow_runs[0].name, Some("CI".to_string()));
        assert_eq!(response.workflow_runs[1].name, None);
    }

    #[test]
    fn test_jobs_response_deserialization() {
        let json = r#"{
            "total_count": 1,
            "jobs": [
                {
                    "id": 789,
                    "run_id": 123,
                    "name": "build",
                    "workflow_name": "CI Workflow",
                    "status": "completed",
                    "conclusion": "success",
                    "run_attempt": 1,
                    "started_at": "2021-01-01T00:00:00Z",
                    "completed_at": "2021-01-01T00:05:00Z",
                    "html_url": "https://github.com/owner/repo/actions/runs/123/jobs/789"
                }
            ]
        }"#;

        let response: JobsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.total_count, 1);
        assert_eq!(response.jobs.len(), 1);
        assert_eq!(response.jobs[0].id, 789);
        assert_eq!(response.jobs[0].run_id, 123);
        assert_eq!(response.jobs[0].name, "build");
        assert_eq!(
            response.jobs[0].workflow_name,
            Some("CI Workflow".to_string())
        );
        assert_eq!(response.jobs[0].status, "completed");
        assert_eq!(response.jobs[0].conclusion, Some("success".to_string()));
        assert_eq!(response.jobs[0].run_attempt, 1);
        assert_eq!(
            response.jobs[0].html_url,
            "https://github.com/owner/repo/actions/runs/123/jobs/789"
        );
    }

    #[test]
    fn test_job_deserialization_with_null_fields() {
        let json = r#"{
            "id": 789,
            "run_id": 123,
            "name": "build",
            "workflow_name": null,
            "status": "in_progress",
            "conclusion": null,
            "run_attempt": 1,
            "started_at": "2021-01-01T00:00:00Z",
            "completed_at": null,
            "html_url": "https://github.com/owner/repo/actions/runs/123/jobs/789"
        }"#;

        let job: Job = serde_json::from_str(json).unwrap();
        assert_eq!(job.id, 789);
        assert_eq!(job.workflow_name, None);
        assert_eq!(job.status, "in_progress");
        assert_eq!(job.conclusion, None);
        assert_eq!(job.completed_at, None);
    }

    #[test]
    fn test_workflow_run_deserialization_minimal() {
        let json = r#"{
            "id": 123,
            "name": null,
            "path": null,
            "run_started_at": "2021-01-01T00:00:00Z"
        }"#;

        let run: WorkflowRun = serde_json::from_str(json).unwrap();
        assert_eq!(run.id, 123);
        assert_eq!(run.name, None);
        assert_eq!(run.path, None);
    }
}
