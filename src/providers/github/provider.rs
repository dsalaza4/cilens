use chrono::Utc;
use futures::stream::{self, StreamExt, TryStreamExt};
use log::{info, warn};

use crate::auth::Token;
use crate::error::Result;
use crate::insights::CIInsights;
use crate::output::PhaseProgress;
use crate::providers::github::client::{GitHubClient, Job, WorkflowRun};

use super::cache::JobCache;
use super::jobs::calculate_job_metrics;
use super::types::GitHubJob;

/// GitHub Actions insights provider.
///
/// Fetches job data from GitHub's REST API and calculates
/// comprehensive metrics including percentiles, success rates, retry detection,
/// and time-to-feedback analysis.
pub struct GitHubProvider {
    pub client: GitHubClient,
    pub repository: String,
}

impl GitHubProvider {
    /// Creates a new GitHub provider for the specified repository.
    ///
    /// # Arguments
    ///
    /// * `base_url` - GitHub base URL (e.g., <https://github.com>)
    /// * `repository` - Repository (e.g., "owner/repo")
    /// * `token` - Optional authentication token
    ///
    /// # Errors
    ///
    /// Returns an error if the repository format is invalid or client cannot be created.
    pub fn new(base_url: &str, repository: &str, token: Option<Token>) -> Result<Self> {
        let client = GitHubClient::new(base_url, repository, token)?;

        Ok(Self {
            client,
            repository: repository.to_string(),
        })
    }

    /// Converts a Vec of jobs to a `HashMap` indexed by job ID.
    fn jobs_to_map(jobs: &[GitHubJob]) -> std::collections::HashMap<u64, GitHubJob> {
        jobs.iter().map(|job| (job.id, job.clone())).collect()
    }

    /// Converts `HashMap` to Vec and sorts by `started_at` descending, taking only `limit` jobs.
    fn map_to_sorted_jobs(
        map: std::collections::HashMap<u64, GitHubJob>,
        limit: usize,
    ) -> Vec<GitHubJob> {
        let mut jobs: Vec<GitHubJob> = map.into_values().collect();
        jobs.sort_by_key(|b| std::cmp::Reverse(b.started_at));
        jobs.into_iter().take(limit).collect()
    }

    /// Saves jobs to cache, logging a warning if it fails.
    fn save_cache(&self, jobs: &[GitHubJob]) {
        let cache = JobCache::new(Self::jobs_to_map(jobs));
        if let Err(e) = cache.save(&self.repository) {
            warn!("Failed to save cache: {e}");
        }
    }

    /// Collects CI/CD insights for the configured repository.
    ///
    /// Fetches job data from GitHub Actions and calculates comprehensive metrics
    /// including duration percentiles, failure rates, and retry detection.
    ///
    /// Supports caching:
    /// - If cache exists and has enough jobs, uses cached data
    /// - If cache exists but doesn't have enough jobs, merges cached with fresh jobs
    /// - Otherwise fetches from API and saves to cache
    ///
    /// # Arguments
    ///
    /// * `limit` - Maximum number of jobs to fetch
    /// * `min_executions_percentage` - Minimum percentage of total executions for a job to be included
    ///
    /// # Returns
    ///
    /// Returns `CIInsights` containing aggregated metrics for each job.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - REST API requests fail
    /// - Repository data is not found
    /// - Network or parsing errors occur
    pub async fn collect_insights(
        &self,
        limit: usize,
        min_executions_percentage: f64,
    ) -> Result<CIInsights> {
        info!(
            "Starting insights collection for repository: {}",
            self.repository
        );

        // Phase 1: Check cache or fetch jobs
        let progress = PhaseProgress::start_phase_1();

        let jobs = if let Some(cache) = JobCache::load(&self.repository) {
            if cache.jobs.len() >= limit {
                info!("Using {} cached jobs (limit: {})", cache.jobs.len(), limit);
                Self::map_to_sorted_jobs(cache.jobs, limit)
            } else {
                // Cache has fewer jobs than requested, fetch until we have enough unique jobs
                info!(
                    "Cache has {} jobs, need {} total - fetching more...",
                    cache.jobs.len(),
                    limit
                );
                self.fetch_and_merge_until_limit(cache.jobs, limit).await?
            }
        } else {
            info!("No cache found, fetching from API");
            self.fetch_and_cache_jobs(limit).await?
        };

        // Phase 2: Processing insights
        let progress = progress.finish_phase_1_start_phase_2();

        let job_metrics = calculate_job_metrics(jobs, min_executions_percentage);

        let insights = CIInsights {
            provider: "GitHub".to_string(),
            project: self.repository.clone(),
            collected_at: Utc::now(),
            total_jobs: job_metrics.len(),
            jobs: job_metrics,
        };

        progress.finish_phase_2();

        Ok(insights)
    }

    /// Fetches jobs from API and saves to cache.
    async fn fetch_and_cache_jobs(&self, limit: usize) -> Result<Vec<GitHubJob>> {
        let jobs = self.fetch_jobs_from_api(limit).await?;
        self.save_cache(&jobs);
        Ok(jobs)
    }

    /// Fetches jobs from API without caching.
    ///
    /// Orchestrates fetching workflow runs and their jobs from the GitHub API.
    /// Fetches runs page-by-page, processing each page's jobs concurrently.
    /// Stops when the job limit is reached to avoid over-fetching runs.
    async fn fetch_jobs_from_api(&self, limit: usize) -> Result<Vec<GitHubJob>> {
        info!("Fetching GitHub Actions jobs (limit: {limit})...");

        let mut all_jobs = Vec::new();
        let mut page = 1;

        loop {
            info!(
                "Fetching workflow runs page {} ({} jobs collected so far)...",
                page,
                all_jobs.len()
            );

            let runs = self.client.fetch_runs(page).await?;

            if runs.is_empty() {
                info!("No more workflow runs found");
                break;
            }

            let jobs_from_page = self.fetch_jobs_for_runs(runs).await?;
            let new_job_count = jobs_from_page.len();
            all_jobs.extend(jobs_from_page);

            info!(
                "Collected {} jobs from this page ({} total)",
                new_job_count,
                all_jobs.len()
            );

            // Stop if we've reached the limit
            if all_jobs.len() >= limit {
                info!("Reached job limit of {limit}");
                all_jobs.truncate(limit);
                break;
            }

            page += 1;
        }

        info!("Collected {} job executions", all_jobs.len());
        Ok(all_jobs)
    }

    /// Fetches jobs for multiple workflow runs concurrently.
    ///
    /// Filters out jobs without required fields (`started_at`).
    async fn fetch_jobs_for_runs(&self, runs: Vec<WorkflowRun>) -> Result<Vec<GitHubJob>> {
        stream::iter(runs)
            .map(|run| {
                let client = self.client.clone();
                async move {
                    // Fetch all jobs for this run
                    let jobs = client.fetch_jobs_for_run(run.id, usize::MAX).await?;

                    // Transform API jobs to GitHubJob with run context, filtering out invalid jobs
                    let transformed: Vec<GitHubJob> = jobs
                        .into_iter()
                        .filter_map(|job| Self::transform_job(job, &run))
                        .collect();

                    Ok::<_, crate::error::CILensError>(transformed)
                }
            })
            // buffer_unordered allows high concurrency, but semaphore in client controls actual parallelism
            .buffer_unordered(1000)
            .try_collect::<Vec<Vec<GitHubJob>>>()
            .await
            .map(|jobs| jobs.into_iter().flatten().collect())
    }

    /// Fetches jobs and merges with cache until we have enough unique jobs.
    ///
    /// Handles deduplication by continuing to fetch until we reach the desired limit
    /// of unique jobs after merging with the cache.
    async fn fetch_and_merge_until_limit(
        &self,
        mut cached_jobs: std::collections::HashMap<u64, GitHubJob>,
        limit: usize,
    ) -> Result<Vec<GitHubJob>> {
        let mut page = 1;
        let initial_cache_size = cached_jobs.len();

        loop {
            let unique_count = cached_jobs.len();

            // Check if we have enough unique jobs
            if unique_count >= limit {
                info!(
                    "Reached {} unique jobs ({} from cache, {} new)",
                    unique_count,
                    initial_cache_size,
                    unique_count - initial_cache_size
                );
                break;
            }

            info!("Have {unique_count} unique jobs, need {limit} - fetching more (page {page})...");

            // Fetch next page of runs
            let runs = self.client.fetch_runs(page).await?;

            if runs.is_empty() {
                info!("No more workflow runs available. Collected {unique_count} unique jobs (requested {limit})");
                break;
            }

            let jobs_from_page = self.fetch_jobs_for_runs(runs).await?;

            // Merge into cache (deduplicates by job ID)
            let before_merge = cached_jobs.len();
            for job in jobs_from_page {
                cached_jobs.insert(job.id, job);
            }
            let after_merge = cached_jobs.len();
            let new_unique = after_merge - before_merge;

            info!("Page {page} added {new_unique} new unique jobs ({after_merge} total unique)");

            page += 1;
        }

        let result = Self::map_to_sorted_jobs(cached_jobs, limit);
        self.save_cache(&result);
        Ok(result)
    }

    /// Transforms a raw API job and run into a `GitHubJob`.
    ///
    /// Returns `None` if the job is missing required fields (`started_at`).
    fn transform_job(job: Job, run: &WorkflowRun) -> Option<GitHubJob> {
        // Filter out jobs without started_at timestamp
        let started_at = job.started_at?;

        // Calculate duration if job is completed
        #[allow(clippy::cast_precision_loss)]
        let duration = if let Some(completed) = job.completed_at {
            (completed - started_at).num_seconds() as f64
        } else {
            0.0
        };

        // Determine workflow name (prefer job's workflow_name, fallback to run's name/path)
        let workflow_name = job
            .workflow_name
            .or_else(|| run.name.clone())
            .or_else(|| run.path.clone())
            .unwrap_or_else(|| "unknown".to_string());

        Some(GitHubJob {
            id: job.id,
            run_id: job.run_id,
            name: job.name,
            workflow_name,
            status: job.status,
            conclusion: job.conclusion,
            run_attempt: job.run_attempt, // Job inherits workflow run's attempt
            duration,
            started_at,
            completed_at: job.completed_at,
            workflow_run_started_at: run.run_started_at,
            html_url: job.html_url,
        })
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::super::client::{Job, WorkflowRun};
    use super::super::types::GitHubJob;
    use super::GitHubProvider;
    use chrono::{TimeZone, Utc};
    use std::collections::HashMap;

    #[test]
    fn test_transform_job_with_all_fields() {
        let run = WorkflowRun {
            id: 123,
            name: Some("CI Pipeline".to_string()),
            path: Some(".github/workflows/ci.yml".to_string()),
            run_started_at: Utc.timestamp_opt(1_609_459_200, 0).unwrap(),
        };

        let job = Job {
            id: 456,
            run_id: 123,
            name: "build".to_string(),
            workflow_name: Some("Build Workflow".to_string()),
            status: "completed".to_string(),
            conclusion: Some("success".to_string()),
            run_attempt: 1,
            started_at: Some(Utc.timestamp_opt(1_609_459_210, 0).unwrap()),
            completed_at: Some(Utc.timestamp_opt(1_609_459_270, 0).unwrap()),
            html_url: "https://github.com/owner/repo/actions/runs/123/jobs/456".to_string(),
        };

        let github_job = GitHubProvider::transform_job(job, &run).unwrap();

        assert_eq!(github_job.id, 456);
        assert_eq!(github_job.run_id, 123);
        assert_eq!(github_job.name, "build");
        assert_eq!(github_job.workflow_name, "Build Workflow");
        assert_eq!(github_job.status, "completed");
        assert_eq!(github_job.conclusion, Some("success".to_string()));
        assert_eq!(github_job.run_attempt, 1);
        assert_eq!(github_job.duration, 60.0); // 270 - 210 = 60 seconds
        assert_eq!(
            github_job.started_at,
            Utc.timestamp_opt(1_609_459_210, 0).unwrap()
        );
        assert_eq!(
            github_job.completed_at,
            Some(Utc.timestamp_opt(1_609_459_270, 0).unwrap())
        );
        assert_eq!(
            github_job.workflow_run_started_at,
            Utc.timestamp_opt(1_609_459_200, 0).unwrap()
        );
        assert_eq!(
            github_job.html_url,
            "https://github.com/owner/repo/actions/runs/123/jobs/456"
        );
    }

    #[test]
    fn test_transform_job_workflow_name_fallback_to_run_name() {
        let run = WorkflowRun {
            id: 123,
            name: Some("CI Pipeline".to_string()),
            path: Some(".github/workflows/ci.yml".to_string()),
            run_started_at: Utc.timestamp_opt(1_609_459_200, 0).unwrap(),
        };

        let job = Job {
            id: 456,
            run_id: 123,
            name: "build".to_string(),
            workflow_name: None, // No workflow name on job
            status: "completed".to_string(),
            conclusion: Some("success".to_string()),
            run_attempt: 1,
            started_at: Some(Utc.timestamp_opt(1_609_459_210, 0).unwrap()),
            completed_at: Some(Utc.timestamp_opt(1_609_459_270, 0).unwrap()),
            html_url: "https://github.com/owner/repo/actions/runs/123/jobs/456".to_string(),
        };

        let github_job = GitHubProvider::transform_job(job, &run).unwrap();
        assert_eq!(github_job.workflow_name, "CI Pipeline");
    }

    #[test]
    fn test_transform_job_workflow_name_fallback_to_path() {
        let run = WorkflowRun {
            id: 123,
            name: None, // No name on run
            path: Some(".github/workflows/ci.yml".to_string()),
            run_started_at: Utc.timestamp_opt(1_609_459_200, 0).unwrap(),
        };

        let job = Job {
            id: 456,
            run_id: 123,
            name: "build".to_string(),
            workflow_name: None, // No workflow name on job
            status: "completed".to_string(),
            conclusion: Some("success".to_string()),
            run_attempt: 1,
            started_at: Some(Utc.timestamp_opt(1_609_459_210, 0).unwrap()),
            completed_at: Some(Utc.timestamp_opt(1_609_459_270, 0).unwrap()),
            html_url: "https://github.com/owner/repo/actions/runs/123/jobs/456".to_string(),
        };

        let github_job = GitHubProvider::transform_job(job, &run).unwrap();
        assert_eq!(github_job.workflow_name, ".github/workflows/ci.yml");
    }

    #[test]
    fn test_transform_job_workflow_name_fallback_to_unknown() {
        let run = WorkflowRun {
            id: 123,
            name: None,
            path: None,
            run_started_at: Utc.timestamp_opt(1_609_459_200, 0).unwrap(),
        };

        let job = Job {
            id: 456,
            run_id: 123,
            name: "build".to_string(),
            workflow_name: None,
            status: "completed".to_string(),
            conclusion: Some("success".to_string()),
            run_attempt: 1,
            started_at: Some(Utc.timestamp_opt(1_609_459_210, 0).unwrap()),
            completed_at: Some(Utc.timestamp_opt(1_609_459_270, 0).unwrap()),
            html_url: "https://github.com/owner/repo/actions/runs/123/jobs/456".to_string(),
        };

        let github_job = GitHubProvider::transform_job(job, &run).unwrap();
        assert_eq!(github_job.workflow_name, "unknown");
    }

    #[test]
    fn test_transform_job_incomplete_no_completed_at() {
        let run = WorkflowRun {
            id: 123,
            name: Some("CI Pipeline".to_string()),
            path: None,
            run_started_at: Utc.timestamp_opt(1_609_459_200, 0).unwrap(),
        };

        let job = Job {
            id: 456,
            run_id: 123,
            name: "build".to_string(),
            workflow_name: Some("Build Workflow".to_string()),
            status: "in_progress".to_string(),
            conclusion: None,
            run_attempt: 1,
            started_at: Some(Utc.timestamp_opt(1_609_459_210, 0).unwrap()),
            completed_at: None, // Job still running
            html_url: "https://github.com/owner/repo/actions/runs/123/jobs/456".to_string(),
        };

        let github_job = GitHubProvider::transform_job(job, &run).unwrap();

        assert_eq!(github_job.status, "in_progress");
        assert_eq!(github_job.duration, 0.0); // No duration if not completed
        assert_eq!(github_job.completed_at, None);
    }

    #[fixtura::test]
    fn test_jobs_to_map(
        #[fixtura(id = 1u64, name = "build".to_string())] job1: GitHubJob,
        #[fixtura(id = 2u64, name = "test".to_string())] job2: GitHubJob,
        #[fixtura(id = 3u64, name = "deploy".to_string())] job3: GitHubJob,
    ) {
        let all = vec![job1, job2, job3];
        let map = GitHubProvider::jobs_to_map(&all);

        assert_eq!(map.len(), 3);
        assert_eq!(map.get(&1).unwrap().name, "build");
        assert_eq!(map.get(&2).unwrap().name, "test");
        assert_eq!(map.get(&3).unwrap().name, "deploy");
    }

    #[fixtura::test]
    fn test_map_to_sorted_jobs(
        #[fixtura(id = 1u64, name = "job1".to_string(), started_at = Utc.timestamp_opt(1_609_459_210, 0).unwrap())]
        job1: GitHubJob,
        #[fixtura(id = 2u64, name = "job2".to_string(), started_at = Utc.timestamp_opt(1_609_459_230, 0).unwrap())]
        job2: GitHubJob,
        #[fixtura(id = 3u64, name = "job3".to_string(), started_at = Utc.timestamp_opt(1_609_459_220, 0).unwrap())]
        job3: GitHubJob,
    ) {
        let mut map = HashMap::new();
        map.insert(job1.id, job1);
        map.insert(job2.id, job2);
        map.insert(job3.id, job3);

        let sorted = GitHubProvider::map_to_sorted_jobs(map, 10);

        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0].name, "job2"); // 230 — newest
        assert_eq!(sorted[1].name, "job3"); // 220
        assert_eq!(sorted[2].name, "job1"); // 210 — oldest
    }

    #[fixtura::test]
    fn test_map_to_sorted_jobs_respects_limit(
        #[fixtura(id = 1u64, name = "job1".to_string(), started_at = Utc.timestamp_opt(1_609_459_210, 0).unwrap())]
        job1: GitHubJob,
        #[fixtura(id = 2u64, name = "job2".to_string(), started_at = Utc.timestamp_opt(1_609_459_230, 0).unwrap())]
        job2: GitHubJob,
        #[fixtura(id = 3u64, name = "job3".to_string(), started_at = Utc.timestamp_opt(1_609_459_220, 0).unwrap())]
        job3: GitHubJob,
    ) {
        let mut map = HashMap::new();
        map.insert(job1.id, job1);
        map.insert(job2.id, job2);
        map.insert(job3.id, job3);

        let sorted = GitHubProvider::map_to_sorted_jobs(map, 2);

        assert_eq!(sorted.len(), 2);
        assert_eq!(sorted[0].name, "job2"); // newest
        assert_eq!(sorted[1].name, "job3");
    }
}
