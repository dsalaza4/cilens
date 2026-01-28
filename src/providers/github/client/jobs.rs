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
    /// Vector of WorkflowRun structs from the API response
    ///
    /// # Errors
    ///
    /// Returns an error if the API request fails or response cannot be parsed
    pub async fn fetch_runs(&self, page: usize) -> Result<Vec<WorkflowRun>> {
        let path = format!(
            "repos/{}/{}/actions/runs?per_page={}&page={}",
            self.owner, self.repo, PAGE_SIZE, page
        );

        let url = self.api_url.join(&path)
            .map_err(|e| crate::error::CILensError::Config(format!("Invalid URL: {e}")))?;

        let runs_response: WorkflowRunsResponse =
            self.execute_request(Method::GET, url).await?;

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

            let url = self.api_url.join(&path)
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
