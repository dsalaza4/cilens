use chrono::Utc;
use log::{info, warn};

use crate::auth::Token;
use crate::error::Result;
use crate::insights::CIInsights;
use crate::output::PhaseProgress;
use crate::providers::github::client::{GitHubClient, Job, WorkflowRun, PAGE_SIZE};

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
        jobs.iter()
            .map(|job| (job.id, job.clone()))
            .collect()
    }

    /// Converts `HashMap` to Vec and sorts by `started_at` descending, taking only `limit` jobs.
    fn map_to_sorted_jobs(
        map: std::collections::HashMap<u64, GitHubJob>,
        limit: usize,
    ) -> Vec<GitHubJob> {
        let mut jobs: Vec<GitHubJob> = map.into_values().collect();
        jobs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
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
                info!(
                    "Cache has {} jobs, fetching {} more to reach limit",
                    cache.jobs.len(),
                    limit
                );
                let fresh_jobs = self.fetch_jobs_from_api(limit).await?;
                self.merge_and_cache(cache.jobs, fresh_jobs, limit)
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
    async fn fetch_jobs_from_api(&self, limit: usize) -> Result<Vec<GitHubJob>> {
        info!("Fetching GitHub Actions jobs (limit: {})...", limit);

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

            // Fetch jobs for each run
            for run in &runs {
                if all_jobs.len() >= limit {
                    info!("Reached job limit of {}", limit);
                    break;
                }

                // Calculate remaining jobs needed
                let remaining = limit.saturating_sub(all_jobs.len());
                let jobs = self.client.fetch_jobs_for_run(run.id, remaining).await?;

                // Transform API jobs to GitHubJob structs
                for job in jobs {
                    if all_jobs.len() >= limit {
                        break;
                    }

                    all_jobs.push(Self::transform_job(job, run));
                }
            }

            if all_jobs.len() >= limit {
                break;
            }

            // Check if there are more pages
            if runs.len() < PAGE_SIZE {
                info!("No more workflow runs available");
                break;
            }

            page += 1;
        }

        info!("Collected {} job executions", all_jobs.len());
        Ok(all_jobs)
    }

    /// Transforms a raw API job and run into a GitHubJob.
    fn transform_job(job: Job, run: &WorkflowRun) -> GitHubJob {
        // Calculate duration if job is completed
        let duration = if let (Some(started), Some(completed)) =
            (job.started_at, job.completed_at)
        {
            (completed - started).num_seconds() as f64
        } else {
            0.0
        };

        // Determine workflow name (prefer job's workflow_name, fallback to run's name/path)
        let workflow_name = job.workflow_name
            .or_else(|| run.name.clone())
            .or_else(|| run.path.clone())
            .unwrap_or_else(|| "unknown".to_string());

        GitHubJob {
            id: job.id,
            run_id: job.run_id,
            name: job.name,
            workflow_name,
            status: job.status,
            conclusion: job.conclusion,
            run_attempt: job.run_attempt,  // Job inherits workflow run's attempt
            duration,
            started_at: job.started_at.unwrap_or_else(chrono::Utc::now),
            completed_at: job.completed_at,
            workflow_run_started_at: run.run_started_at,
            html_url: job.html_url,
        }
    }

    /// Merges cached jobs with fresh jobs, deduplicates, sorts, limits, and saves to cache.
    fn merge_and_cache(
        &self,
        mut cached_jobs: std::collections::HashMap<u64, GitHubJob>,
        fresh_jobs: Vec<GitHubJob>,
        limit: usize,
    ) -> Vec<GitHubJob> {
        // Merge fresh jobs into cache (overwrites duplicates)
        for job in fresh_jobs {
            cached_jobs.insert(job.id, job);
        }

        info!("Merged {} unique jobs", cached_jobs.len());
        let result = Self::map_to_sorted_jobs(cached_jobs, limit);
        self.save_cache(&result);
        result
    }
}
