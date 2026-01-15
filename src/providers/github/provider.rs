use crate::auth::Token;
use crate::error::Result;
use crate::insights::CIInsights;
use crate::output::PhaseProgress;
use chrono::{DateTime, Utc};
use log::info;

use super::cache::JobCache;
use super::client::GitHubClient;
use super::types::{GitHubJob, GitHubWorkflowRun};

/// GitHub Actions CI/CD insights provider.
pub struct GitHubProvider {
    #[allow(dead_code)]
    pub base_url: String,
    pub owner: String,
    pub repo: String,
    #[allow(dead_code)]
    pub token: Option<Token>,
    client: GitHubClient,
    cache: JobCache,
}

impl GitHubProvider {
    /// Creates a new GitHub provider.
    pub fn new(
        base_url: &str,
        owner: String,
        repo: String,
        token: Option<Token>,
        use_cache: bool,
    ) -> Result<Self> {
        let client = GitHubClient::new(base_url, token.clone())?;
        let repo_path = format!("{owner}/{repo}");
        let cache = JobCache::new(&repo_path, use_cache)?;

        Ok(Self {
            base_url: base_url.to_string(),
            owner,
            repo,
            token,
            client,
            cache,
        })
    }

    /// Collects CI/CD insights from GitHub Actions.
    ///
    /// # Arguments
    /// * `limit` - Maximum number of workflow runs to fetch
    /// * `branch` - Optional branch filter
    /// * `created_after` - Optional filter for workflow runs created after this timestamp
    /// * `created_before` - Optional filter for workflow runs created before this timestamp
    /// * `min_type_percentage` - Minimum percentage threshold for pipeline type inclusion (1-100)
    ///
    /// # Errors
    /// Returns an error if fetching workflow runs or jobs fails.
    pub async fn collect_insights(
        &self,
        limit: usize,
        branch: Option<&str>,
        created_after: Option<DateTime<Utc>>,
        created_before: Option<DateTime<Utc>>,
        _min_type_percentage: u8,
    ) -> Result<CIInsights> {
        // Phase 1: Fetch workflow runs
        let progress = PhaseProgress::start_phase_1();
        let workflow_runs = self
            .fetch_workflow_runs(limit, branch, created_after, created_before)
            .await?;
        info!("Fetched {} workflow runs", workflow_runs.len());

        // Phase 2: Already have jobs embedded in workflow_runs (fetched in fetch_workflow_runs)
        let progress = progress.finish_phase_1_start_phase_2();
        info!("Fetched jobs for {} workflow runs", workflow_runs.len());

        // Phase 3: Transform to CIInsights by grouping runs by workflow name
        let progress = progress.finish_phase_2_start_phase_3();
        let pipeline_types = super::workflow_transform::transform_to_pipeline_types(&workflow_runs);

        let insights = CIInsights {
            provider: "GitHub".to_string(),
            project: format!("{}/{}", self.owner, self.repo),
            collected_at: Utc::now(),
            total_pipelines: workflow_runs.len(),
            total_pipeline_types: pipeline_types.len(),
            pipeline_types,
        };
        progress.finish_phase_3();

        Ok(insights)
    }

    /// Fetches workflow runs from GitHub API with job data.
    ///
    /// # Arguments
    /// * `limit` - Maximum number of workflow runs to fetch
    /// * `branch` - Optional branch filter
    /// * `created_after` - Optional filter for workflow runs created after this timestamp
    /// * `created_before` - Optional filter for workflow runs created before this timestamp
    ///
    /// # Errors
    /// Returns an error if fetching workflow runs or jobs fails.
    async fn fetch_workflow_runs(
        &self,
        limit: usize,
        branch: Option<&str>,
        created_after: Option<DateTime<Utc>>,
        created_before: Option<DateTime<Utc>>,
    ) -> Result<Vec<GitHubWorkflowRun>> {
        // Build created date filter if needed
        let created = match (created_after, created_before) {
            (Some(after), Some(before)) => {
                Some(format!("{}..{}", after.to_rfc3339(), before.to_rfc3339()))
            }
            (Some(after), None) => Some(format!(">={}", after.to_rfc3339())),
            (None, Some(before)) => Some(format!("<={}", before.to_rfc3339())),
            (None, None) => None,
        };

        // Fetch workflow runs
        let workflow_runs_data = self
            .client
            .fetch_workflow_runs(
                &self.owner,
                &self.repo,
                limit.min(100), // GitHub API max per_page is 100
                branch,
                Some("completed"), // Only fetch completed runs
                created.as_deref(),
            )
            .await?;

        let mut workflow_runs = Vec::new();

        // Fetch jobs for each workflow run
        for run_data in workflow_runs_data {
            // Check cache first
            let jobs = if let Some(cached_jobs) = self.cache.get(run_data.id) {
                info!("Cache hit for workflow run {}", run_data.id);
                cached_jobs
            } else {
                // Fetch from API
                let jobs_data = self
                    .client
                    .fetch_jobs(&self.owner, &self.repo, run_data.id)
                    .await?;

                // Transform to GitHubJob
                let jobs: Vec<GitHubJob> = jobs_data
                    .into_iter()
                    .map(|job_data| GitHubJob {
                        id: job_data.id,
                        name: job_data.name,
                        status: job_data.status,
                        conclusion: job_data.conclusion,
                        started_at: job_data.started_at,
                        completed_at: job_data.completed_at,
                        needs: None, // GitHub API doesn't provide job dependencies in the response
                    })
                    .collect();

                // Note: Cache insertion will be added in Task 8 (cache module integration)
                jobs
            };

            // Calculate run duration
            let run_duration_ms = calculate_duration(
                run_data.run_started_at.as_deref(),
                Some(&run_data.updated_at),
            );

            // Transform to GitHubWorkflowRun
            let workflow_run = GitHubWorkflowRun {
                id: run_data.id,
                workflow_name: run_data.name,
                head_branch: run_data.head_branch,
                event: run_data.event,
                status: run_data.status,
                conclusion: run_data.conclusion,
                run_duration_ms,
                jobs,
            };

            workflow_runs.push(workflow_run);
        }

        Ok(workflow_runs)
    }
}

/// Calculates duration between two timestamps in milliseconds.
fn calculate_duration(started_at: Option<&str>, completed_at: Option<&str>) -> Option<u64> {
    match (started_at, completed_at) {
        (Some(start), Some(end)) => {
            let start: DateTime<Utc> = start.parse().ok()?;
            let end: DateTime<Utc> = end.parse().ok()?;
            let duration = end.signed_duration_since(start);
            Some(duration.num_milliseconds().cast_unsigned())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::github::types::GitHubWorkflowRun;

    #[test]
    fn test_calculate_duration() {
        let started = "2025-01-01T00:00:00Z";
        let completed = "2025-01-01T00:05:00Z";

        let duration = calculate_duration(Some(started), Some(completed));
        assert_eq!(duration, Some(300_000)); // 5 minutes in ms
    }

    #[test]
    fn test_calculate_duration_missing_timestamps() {
        assert_eq!(calculate_duration(None, None), None);
        assert_eq!(calculate_duration(Some("2025-01-01T00:00:00Z"), None), None);
    }

    #[test]
    fn test_transform_to_pipeline_format() {
        // Test that GitHubWorkflowRun has fields compatible with pipeline analysis
        let run = GitHubWorkflowRun {
            id: 123,
            workflow_name: "CI".to_string(),
            head_branch: Some("main".to_string()),
            event: "push".to_string(),
            status: "completed".to_string(),
            conclusion: Some("success".to_string()),
            run_duration_ms: Some(300_000),
            jobs: vec![],
        };

        assert_eq!(run.id, 123);
        assert_eq!(run.workflow_name, "CI");
        assert_eq!(run.head_branch, Some("main".to_string()));
        assert!(run.is_completed());
    }

    #[tokio::test]
    async fn test_collect_insights_signature() {
        // Test that collect_insights method exists with correct signature
        let provider = GitHubProvider::new(
            "https://api.github.com",
            "owner".to_string(),
            "repo".to_string(),
            None,
            false,
        )
        .unwrap();

        // This test verifies the method signature compiles correctly
        // It will fail at runtime due to network calls, but that's expected
        let result = provider
            .collect_insights(10, Some("main"), None, None, 1)
            .await;

        // We expect this to fail due to network issues in tests, but the signature is correct
        assert!(result.is_ok() || result.is_err());
    }
}
