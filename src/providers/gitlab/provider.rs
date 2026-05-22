use chrono::Utc;
use log::{info, warn};

use crate::auth::Token;
use crate::error::Result;
use crate::insights::CIInsights;
use crate::output::PhaseProgress;
use crate::providers::gitlab::client::jobs::FetchJobsProjectJobsNodes;
use crate::providers::gitlab::client::GitLabClient;

use super::cache::JobCache;
use super::jobs::calculate_job_metrics;
use super::types::GitLabJob;

/// GitLab CI/CD insights provider.
///
/// Fetches job data from GitLab's GraphQL API and calculates
/// comprehensive metrics including percentiles, success rates, retry detection,
/// and time-to-feedback analysis.
pub struct GitLabProvider {
    pub client: GitLabClient,
    pub base_url: String,
    pub project_path: String,
}

impl GitLabProvider {
    /// Creates a new GitLab provider for the specified project.
    ///
    /// # Arguments
    ///
    /// * `base_url` - GitLab instance base URL (e.g., <https://gitlab.com>)
    /// * `project_path` - Project path (e.g., "group/project")
    /// * `token` - Optional authentication token
    ///
    /// # Errors
    ///
    /// Returns an error if the GraphQL endpoint URL cannot be created.
    pub fn new(base_url: &str, project_path: &str, token: Option<Token>) -> Result<Self> {
        let client = GitLabClient::new(base_url, token)?;

        Ok(Self {
            client,
            base_url: base_url.to_string(),
            project_path: project_path.to_string(),
        })
    }

    /// Transforms job nodes from the GraphQL API into `GitLabJob` types.
    ///
    /// Filters out jobs without required fields (id, name, `finished_at`).
    fn transform_job_nodes(job_nodes: Vec<FetchJobsProjectJobsNodes>) -> Vec<GitLabJob> {
        job_nodes
            .into_iter()
            .filter_map(|node| {
                // Only process jobs with all required fields
                let id = node.id?;
                let name = node.name?;
                let finished_at = node.finished_at?; // Filter out non-finished jobs

                #[allow(clippy::cast_precision_loss, clippy::cast_sign_loss)]
                Some(GitLabJob {
                    id,
                    name,
                    duration: node.duration.unwrap_or(0) as f64,
                    status: node
                        .status
                        .map_or_else(|| "UNKNOWN".to_string(), |s| format!("{s:?}")),
                    retried: node.retried.unwrap_or(false),
                    needs: node
                        .needs
                        .map(|conn| {
                            conn.nodes
                                .into_iter()
                                .flatten()
                                .flatten()
                                .filter_map(|n| n.name)
                                .collect()
                        })
                        .unwrap_or_default(),
                    created_at: node.created_at,
                    finished_at,
                })
            })
            .collect()
    }

    /// Converts a Vec of jobs to a `HashMap` indexed by job ID.
    fn jobs_to_map(jobs: &[GitLabJob]) -> std::collections::HashMap<String, GitLabJob> {
        jobs.iter()
            .map(|job| (job.id.clone(), job.clone()))
            .collect()
    }

    /// Converts `HashMap` to Vec and sorts by `created_at` descending, taking only `limit` jobs.
    fn map_to_sorted_jobs(
        map: std::collections::HashMap<String, GitLabJob>,
        limit: usize,
    ) -> Vec<GitLabJob> {
        let mut jobs: Vec<GitLabJob> = map.into_values().collect();
        jobs.sort_by_key(|b| std::cmp::Reverse(b.created_at));
        jobs.into_iter().take(limit).collect()
    }

    /// Saves jobs to cache, logging a warning if it fails.
    fn save_cache(&self, jobs: &[GitLabJob]) {
        let cache = JobCache::new(Self::jobs_to_map(jobs));
        if let Err(e) = cache.save(&self.project_path) {
            warn!("Failed to save cache: {e}");
        }
    }

    /// Collects CI/CD insights for the configured project.
    ///
    /// Fetches job data from GitLab and calculates comprehensive metrics
    /// including duration percentiles, failure rates, and retry detection.
    ///
    /// Supports caching:
    /// - If cache exists and has enough jobs, uses cached data
    /// - If cache exists but doesn't have enough jobs, merges cached with fresh jobs
    /// - Otherwise fetches from API and saves to cache
    /// - Cache is bypassed when `branch` or `tag` filter is specified
    ///
    /// # Arguments
    ///
    /// * `limit` - Maximum number of jobs to fetch
    /// * `min_executions_percentage` - Minimum percentage of total executions for a job to be included
    /// * `branch` - Optional branch name to filter jobs by (e.g., "main")
    /// * `tag` - Optional tag name to filter jobs by (e.g., "v1.0.0")
    ///
    /// # Returns
    ///
    /// Returns `CIInsights` containing aggregated metrics for each job.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - GraphQL API requests fail
    /// - Project data is not found
    /// - Network or parsing errors occur
    pub async fn collect_insights(
        &self,
        limit: usize,
        min_executions_percentage: f64,
        branch: Option<&str>,
        tag: Option<&str>,
    ) -> Result<CIInsights> {
        info!(
            "Starting insights collection for project: {}",
            self.project_path
        );

        // Phase 1: Check cache or fetch jobs
        let progress = PhaseProgress::start_phase_1();

        let jobs = if let Some(branch_name) = branch {
            info!("Branch filter active: '{branch_name}' - fetching from API (cache bypassed)");
            self.fetch_jobs_by_branch(branch_name, limit).await?
        } else if let Some(tag_name) = tag {
            info!("Tag filter active: '{tag_name}' - fetching from API (cache bypassed)");
            self.fetch_jobs_by_tag(tag_name, limit).await?
        } else if let Some(cache) = JobCache::load(&self.project_path) {
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

        let job_metrics = calculate_job_metrics(
            jobs,
            &self.base_url,
            &self.project_path,
            min_executions_percentage,
        );

        let insights = CIInsights {
            provider: "GitLab".to_string(),
            project: self.project_path.clone(),
            collected_at: Utc::now(),
            total_jobs: job_metrics.len(),
            jobs: job_metrics,
        };

        progress.finish_phase_2();

        Ok(insights)
    }

    /// Fetches jobs from API and saves to cache.
    async fn fetch_and_cache_jobs(&self, limit: usize) -> Result<Vec<GitLabJob>> {
        let jobs = self.fetch_jobs_from_api(limit).await?;
        self.save_cache(&jobs);
        Ok(jobs)
    }

    /// Fetches jobs from API without caching.
    async fn fetch_jobs_from_api(&self, limit: usize) -> Result<Vec<GitLabJob>> {
        let job_nodes = self.client.fetch_jobs(&self.project_path, limit).await?;
        info!("Fetched {} jobs from API", job_nodes.len());
        Ok(Self::transform_job_nodes(job_nodes))
    }

    /// Fetches jobs filtered by branch name (bypasses cache).
    ///
    /// Does NOT pass `refType` so GitLab returns all pipeline types for the branch
    /// (push pipelines AND merge request pipelines).
    async fn fetch_jobs_by_branch(&self, branch: &str, limit: usize) -> Result<Vec<GitLabJob>> {
        let jobs = self
            .client
            .fetch_jobs_by_ref(&self.project_path, branch, None, limit)
            .await?;
        info!("Fetched {} jobs for branch '{}'", jobs.len(), branch);
        Ok(jobs)
    }

    /// Fetches jobs filtered by tag name (bypasses cache).
    async fn fetch_jobs_by_tag(&self, tag: &str, limit: usize) -> Result<Vec<GitLabJob>> {
        use super::client::jobs::GitLabRefType;
        let jobs = self
            .client
            .fetch_jobs_by_ref(&self.project_path, tag, Some(GitLabRefType::TAGS), limit)
            .await?;
        info!("Fetched {} jobs for tag '{}'", jobs.len(), tag);
        Ok(jobs)
    }

    /// Fetches jobs and merges with cache until we have enough unique jobs.
    ///
    /// Handles deduplication by continuing to fetch until we reach the desired limit
    /// of unique jobs after merging with the cache.
    async fn fetch_and_merge_until_limit(
        &self,
        mut cached_jobs: std::collections::HashMap<String, GitLabJob>,
        limit: usize,
    ) -> Result<Vec<GitLabJob>> {
        let initial_cache_size = cached_jobs.len();

        // Fetch in batches, keep going until we have enough unique jobs
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

            info!("Have {unique_count} unique jobs, need {limit} - fetching more...");

            let before_fetch = cached_jobs.len();
            let fresh_jobs = self.fetch_jobs_from_api(limit - unique_count).await?;

            if fresh_jobs.is_empty() {
                info!("No more jobs available. Collected {unique_count} unique jobs (requested {limit})");
                break;
            }

            // Merge into cache (deduplicates by job ID)
            for job in fresh_jobs {
                cached_jobs.insert(job.id.clone(), job);
            }

            let after_fetch = cached_jobs.len();
            let new_unique = after_fetch - before_fetch;

            info!("Fetch added {new_unique} new unique jobs ({after_fetch} total unique)");

            // If we didn't get any new unique jobs, we've exhausted the API
            if new_unique == 0 {
                info!("No new unique jobs found, stopping fetch");
                break;
            }
        }

        let result = Self::map_to_sorted_jobs(cached_jobs, limit);
        self.save_cache(&result);
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn make_job(id: &str, created_at: chrono::DateTime<Utc>) -> GitLabJob {
        GitLabJob {
            id: id.to_string(),
            name: "test-job".to_string(),
            duration: 60.0,
            status: "SUCCESS".to_string(),
            retried: false,
            needs: vec![],
            created_at,
            finished_at: created_at + chrono::Duration::seconds(60),
        }
    }

    #[test]
    fn test_jobs_to_map() {
        let jobs = vec![
            make_job("id1", Utc.timestamp_opt(1_000, 0).unwrap()),
            make_job("id2", Utc.timestamp_opt(2_000, 0).unwrap()),
        ];
        let map = GitLabProvider::jobs_to_map(&jobs);
        assert_eq!(map.len(), 2);
        assert!(map.contains_key("id1"));
        assert!(map.contains_key("id2"));
    }

    #[test]
    fn test_map_to_sorted_jobs_order() {
        let mut map = std::collections::HashMap::new();
        map.insert(
            "older".to_string(),
            make_job("older", Utc.timestamp_opt(1_000, 0).unwrap()),
        );
        map.insert(
            "newer".to_string(),
            make_job("newer", Utc.timestamp_opt(9_000, 0).unwrap()),
        );
        map.insert(
            "middle".to_string(),
            make_job("middle", Utc.timestamp_opt(5_000, 0).unwrap()),
        );

        let sorted = GitLabProvider::map_to_sorted_jobs(map, 10);

        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0].id, "newer");
        assert_eq!(sorted[1].id, "middle");
        assert_eq!(sorted[2].id, "older");
    }

    #[test]
    fn test_map_to_sorted_jobs_respects_limit() {
        let mut map = std::collections::HashMap::new();
        for i in 0..10u64 {
            let id = format!("job-{i}");
            map.insert(
                id.clone(),
                make_job(&id, Utc.timestamp_opt(i as i64 * 1000, 0).unwrap()),
            );
        }

        let sorted = GitLabProvider::map_to_sorted_jobs(map, 3);
        assert_eq!(sorted.len(), 3);
    }
}
