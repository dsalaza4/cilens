use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Top-level CI/CD insights for a project.
///
/// Contains aggregated metrics for all jobs, grouped by job name.
#[derive(Debug, Serialize, Deserialize)]
pub struct CIInsights {
    /// CI provider name (e.g., "GitLab")
    pub provider: String,
    /// Project identifier (e.g., "group/project")
    pub project: String,
    /// Timestamp when insights were collected
    pub collected_at: DateTime<Utc>,
    /// Total number of jobs analyzed
    pub total_jobs: usize,
    /// Detailed metrics for each job, sorted by `time_to_feedback_p95` descending
    pub jobs: Vec<JobMetrics>,
}

/// Job execution count with clickable URLs for investigation.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct JobCountWithLinks {
    /// Number of job executions
    pub count: usize,
    /// GitLab URLs to individual job runs
    pub links: Vec<String>,
}

/// Comprehensive metrics for a specific job across multiple executions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobMetrics {
    /// Job name
    pub name: String,
    /// Median job execution duration (seconds) - calculated from all successful jobs
    pub duration_p50: f64,
    /// 95th percentile job duration (seconds) - useful for SLA planning
    pub duration_p95: f64,
    /// 99th percentile job duration (seconds) - captures outliers
    pub duration_p99: f64,
    /// Median time from pipeline start to job completion (seconds) - calculated from successful first-try jobs only
    pub time_to_feedback_p50: f64,
    /// 95th percentile time-to-feedback (seconds) - use for planning - calculated from successful first-try jobs only
    pub time_to_feedback_p95: f64,
    /// 99th percentile time-to-feedback (seconds) - worst-case scenario - calculated from successful first-try jobs only
    pub time_to_feedback_p99: f64,
    /// Jobs that must complete before this job (critical path)
    pub predecessors: Vec<String>,
    /// Percentage of executions that were retried (0.0 if never retried)
    pub retry_rate: f64,
    /// Retried executions with clickable URLs
    pub retried_executions: JobCountWithLinks,
    /// Percentage of executions that failed and stayed failed
    pub failure_rate: f64,
    /// Failed executions (stayed failed) with clickable URLs
    pub failed_executions: JobCountWithLinks,
    /// Percentage of executions that succeeded
    pub success_rate: f64,
    /// Successful executions with clickable URLs
    pub successful_executions: JobCountWithLinks,
    /// Total executions across all pipelines (includes retries and failures)
    pub total_executions: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_job_count_with_links_default() {
        let default = JobCountWithLinks::default();
        assert_eq!(default.count, 0);
        assert_eq!(default.links.len(), 0);
    }

    #[test]
    fn test_job_count_with_links_creation() {
        let count_with_links = JobCountWithLinks {
            count: 5,
            links: vec![
                "https://gitlab.com/project/-/jobs/1".to_string(),
                "https://gitlab.com/project/-/jobs/2".to_string(),
            ],
        };
        assert_eq!(count_with_links.count, 5);
        assert_eq!(count_with_links.links.len(), 2);
    }

    #[test]
    fn test_ci_insights_creation() {
        let insights = CIInsights {
            provider: "GitLab".to_string(),
            project: "test/project".to_string(),
            collected_at: Utc::now(),
            total_jobs: 10,
            jobs: vec![],
        };
        assert_eq!(insights.provider, "GitLab");
        assert_eq!(insights.project, "test/project");
        assert_eq!(insights.total_jobs, 10);
        assert_eq!(insights.jobs.len(), 0);
    }

    #[test]
    fn test_job_metrics_creation() {
        let metrics = JobMetrics {
            name: "build".to_string(),
            duration_p50: 120.0,
            duration_p95: 180.0,
            duration_p99: 200.0,
            time_to_feedback_p50: 300.0,
            time_to_feedback_p95: 450.0,
            time_to_feedback_p99: 500.0,
            predecessors: vec!["lint".to_string()],
            retry_rate: 5.0,
            retried_executions: JobCountWithLinks {
                count: 2,
                links: vec![],
            },
            failure_rate: 10.0,
            failed_executions: JobCountWithLinks {
                count: 5,
                links: vec![],
            },
            success_rate: 85.0,
            successful_executions: JobCountWithLinks {
                count: 43,
                links: vec![],
            },
            total_executions: 50,
        };
        assert_eq!(metrics.name, "build");
        assert!((metrics.duration_p50 - 120.0).abs() < f64::EPSILON);
        assert_eq!(metrics.predecessors.len(), 1);
        assert_eq!(metrics.total_executions, 50);
    }

    #[test]
    fn test_predecessor_job_creation() {
        let predecessor = "lint".to_string();
        assert_eq!(predecessor, "lint");
    }
}
