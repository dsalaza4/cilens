use super::types::GitHubWorkflowRun;
use crate::insights::{JobCountWithLinks, JobMetrics, PipelineCountWithLinks, TypeMetrics};
use crate::providers::utils::{calculate_percentiles, calculate_success_rate, cmp_f64};

/// Calculates comprehensive metrics for a workflow (pipeline type).
///
/// Analyzes a group of workflow runs to compute success rates, duration percentiles,
/// and basic job statistics.
///
/// # Arguments
///
/// * `workflow_name` - Name of the workflow (used as pipeline type ID)
/// * `runs` - Collection of workflow runs with this workflow name
/// * `percentage` - Percentage of total runs this workflow represents (0-100)
///
/// # Returns
///
/// `TypeMetrics` containing success rate, duration percentiles (P50/P95/P99),
/// and per-job metrics.
pub fn calculate_workflow_metrics(
    workflow_name: &str,
    runs: &[GitHubWorkflowRun],
    percentage: f64,
) -> TypeMetrics {
    let total_pipelines = runs.len();

    let (successful, failed): (Vec<_>, Vec<_>) =
        runs.iter().partition(|r| r.is_success());

    let successful_pipelines = PipelineCountWithLinks {
        count: successful.len(),
        links: vec![], // TODO: Generate GitHub workflow run URLs
    };

    let failed_pipelines = PipelineCountWithLinks {
        count: failed.len(),
        links: vec![], // TODO: Generate GitHub workflow run URLs
    };

    // Calculate duration percentiles from successful runs
    #[allow(clippy::cast_precision_loss)]
    let durations: Vec<f64> = successful
        .iter()
        .filter_map(|r| r.run_duration_ms)
        .map(|ms| ms as f64 / 1000.0) // Convert to seconds
        .collect();

    let (duration_p50, duration_p95, duration_p99) = calculate_percentiles(&durations);

    // Calculate job metrics from ALL runs (to track failures properly)
    let jobs = calculate_job_metrics(workflow_name, runs);

    // For workflow-level time-to-feedback, calculate time from workflow start to first job completion
    // This represents when developers get the first feedback from this workflow
    let time_to_feedback_values: Vec<f64> = successful
        .iter()
        .filter_map(|run| {
            run.jobs
                .iter()
                .filter_map(|job| job.time_to_feedback(run.run_started_at.as_deref()))
                .min_by(|a, b| cmp_f64(*a, *b))
        })
        .collect();

    let (time_to_feedback_p50, time_to_feedback_p95, time_to_feedback_p99) =
        calculate_percentiles(&time_to_feedback_values);

    TypeMetrics {
        percentage,
        total_pipelines,
        successful_pipelines,
        failed_pipelines,
        success_rate: calculate_success_rate(successful.len(), total_pipelines),
        duration_p50,
        duration_p95,
        duration_p99,
        time_to_feedback_p50,
        time_to_feedback_p95,
        time_to_feedback_p99,
        jobs,
    }
}

/// Job execution data collected across all runs.
struct JobExecutionData {
    /// Duration values (from successful executions)
    durations: Vec<f64>,
    /// Time-to-feedback values (from successful executions)
    time_to_feedback_values: Vec<f64>,
    /// Total successful executions
    successful_count: usize,
    /// Total failed executions
    failed_count: usize,
}

/// Calculates metrics for individual jobs within a workflow.
///
/// Analyzes jobs from ALL runs (not just successful ones) to properly track
/// duration percentiles (from successful jobs) and failure rates.
fn calculate_job_metrics(
    workflow_name: &str,
    all_runs: &[GitHubWorkflowRun],
) -> Vec<JobMetrics> {
    use std::collections::HashMap;

    // Collect job execution data from all runs
    let mut job_data: HashMap<String, JobExecutionData> = HashMap::new();

    for run in all_runs {
        let run_start = run.run_started_at.as_deref();

        for job in &run.jobs {
            let entry = job_data.entry(job.name.clone()).or_insert_with(|| JobExecutionData {
                durations: Vec::new(),
                time_to_feedback_values: Vec::new(),
                successful_count: 0,
                failed_count: 0,
            });

            if job.is_success() {
                // Only collect duration/time-to-feedback from successful jobs
                if let Some(duration) = job.duration_seconds() {
                    entry.durations.push(duration);
                }
                if let Some(ttf) = job.time_to_feedback(run_start) {
                    entry.time_to_feedback_values.push(ttf);
                }
                entry.successful_count += 1;
            } else if job.is_failure() {
                entry.failed_count += 1;
            }
        }
    }

    // Calculate percentiles and metrics for each job
    let mut jobs: Vec<JobMetrics> = job_data
        .into_iter()
        .map(|(job_name, data)| {
            let (duration_p50, duration_p95, duration_p99) = calculate_percentiles(&data.durations);
            let (ttf_p50, ttf_p95, ttf_p99) = calculate_percentiles(&data.time_to_feedback_values);

            let total_executions = data.successful_count + data.failed_count;
            let failure_rate = calculate_success_rate(data.failed_count, total_executions);

            JobMetrics {
                name: job_name,
                pipeline_type_id: workflow_name.to_string(),
                duration_p50,
                duration_p95,
                duration_p99,
                time_to_feedback_p50: ttf_p50,
                time_to_feedback_p95: ttf_p95,
                time_to_feedback_p99: ttf_p99,
                predecessors: vec![], // GitHub API doesn't provide job dependencies
                flakiness_rate: 0.0,  // Not tracking flakiness yet
                flaky_retries: JobCountWithLinks::default(),
                failed_executions: JobCountWithLinks {
                    count: data.failed_count,
                    links: vec![], // TODO: Generate GitHub job URLs
                },
                failure_rate,
                total_executions,
            }
        })
        .collect();

    // Sort by time_to_feedback_p95 descending (most impactful jobs first)
    jobs.sort_by(|a, b| cmp_f64(b.time_to_feedback_p95, a.time_to_feedback_p95));

    jobs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::github::types::{GitHubJob, GitHubWorkflowRun};

    fn create_test_run(
        id: u64,
        workflow_name: &str,
        conclusion: Option<&str>,
        duration_ms: Option<u64>,
        run_started_at: Option<&str>,
    ) -> GitHubWorkflowRun {
        GitHubWorkflowRun {
            id,
            workflow_name: workflow_name.to_string(),
            head_branch: Some("main".to_string()),
            event: "push".to_string(),
            status: "completed".to_string(),
            conclusion: conclusion.map(String::from),
            run_started_at: run_started_at.map(String::from),
            run_duration_ms: duration_ms,
            jobs: vec![],
        }
    }

    #[test]
    fn test_calculate_workflow_metrics() {
        let runs = vec![
            create_test_run(1, "CI", Some("success"), Some(300_000), Some("2025-01-01T00:00:00Z")),
            create_test_run(2, "CI", Some("success"), Some(400_000), Some("2025-01-01T00:00:00Z")),
            create_test_run(3, "CI", Some("failure"), Some(200_000), Some("2025-01-01T00:00:00Z")),
        ];

        let metrics = calculate_workflow_metrics("CI", &runs, 100.0);

        assert_eq!(metrics.total_pipelines, 3);
        assert_eq!(metrics.successful_pipelines.count, 2);
        assert_eq!(metrics.failed_pipelines.count, 1);
        assert!((metrics.success_rate - 66.666).abs() < 0.01);
        assert_eq!(metrics.percentage, 100.0);
    }

    #[test]
    fn test_time_to_feedback_calculation() {
        // Workflow starts at 00:00:00, job completes at 00:05:00
        // Time-to-feedback should be 300 seconds (5 minutes)
        let mut run1 = create_test_run(
            1, "CI", Some("success"), Some(300_000),
            Some("2025-01-01T00:00:00Z")
        );
        run1.jobs = vec![
            GitHubJob {
                id: 1,
                name: "build".to_string(),
                status: "completed".to_string(),
                conclusion: Some("success".to_string()),
                started_at: Some("2025-01-01T00:01:00Z".to_string()), // Job started 1 min after workflow
                completed_at: Some("2025-01-01T00:05:00Z".to_string()), // Job completed at 5 min
                needs: None,
            },
        ];

        let runs = vec![run1];
        let metrics = calculate_workflow_metrics("CI", &runs, 100.0);

        assert_eq!(metrics.jobs.len(), 1);
        // Time-to-feedback = job completed_at - workflow run_started_at = 5 minutes = 300 seconds
        assert_eq!(metrics.jobs[0].time_to_feedback_p50, 300.0);
        // Duration = job completed_at - job started_at = 4 minutes = 240 seconds
        assert_eq!(metrics.jobs[0].duration_p50, 240.0);
    }

    #[test]
    fn test_job_failure_rate_tracking() {
        // Run 1: successful with successful jobs
        let mut run1 = create_test_run(
            1, "CI", Some("success"), Some(300_000),
            Some("2025-01-01T00:00:00Z")
        );
        run1.jobs = vec![
            GitHubJob {
                id: 1,
                name: "build".to_string(),
                status: "completed".to_string(),
                conclusion: Some("success".to_string()),
                started_at: Some("2025-01-01T00:00:00Z".to_string()),
                completed_at: Some("2025-01-01T00:05:00Z".to_string()),
                needs: None,
            },
        ];

        // Run 2: failed due to build job failure
        let mut run2 = create_test_run(
            2, "CI", Some("failure"), Some(100_000),
            Some("2025-01-01T00:00:00Z")
        );
        run2.jobs = vec![
            GitHubJob {
                id: 2,
                name: "build".to_string(),
                status: "completed".to_string(),
                conclusion: Some("failure".to_string()),
                started_at: Some("2025-01-01T00:00:00Z".to_string()),
                completed_at: Some("2025-01-01T00:02:00Z".to_string()),
                needs: None,
            },
        ];

        let runs = vec![run1, run2];
        let metrics = calculate_workflow_metrics("CI", &runs, 100.0);

        assert_eq!(metrics.jobs.len(), 1);
        assert_eq!(metrics.jobs[0].name, "build");
        // 1 successful + 1 failed = 2 total, failure_rate = 50%
        assert_eq!(metrics.jobs[0].total_executions, 2);
        assert_eq!(metrics.jobs[0].failed_executions.count, 1);
        assert!((metrics.jobs[0].failure_rate - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_job_metrics_sorted_by_time_to_feedback() {
        let mut run1 = create_test_run(
            1, "CI", Some("success"), Some(600_000),
            Some("2025-01-01T00:00:00Z")
        );
        run1.jobs = vec![
            GitHubJob {
                id: 1,
                name: "build".to_string(),
                status: "completed".to_string(),
                conclusion: Some("success".to_string()),
                started_at: Some("2025-01-01T00:00:00Z".to_string()),
                completed_at: Some("2025-01-01T00:05:00Z".to_string()), // 5 min time-to-feedback
                needs: None,
            },
            GitHubJob {
                id: 2,
                name: "test".to_string(),
                status: "completed".to_string(),
                conclusion: Some("success".to_string()),
                started_at: Some("2025-01-01T00:05:00Z".to_string()),
                completed_at: Some("2025-01-01T00:10:00Z".to_string()), // 10 min time-to-feedback
                needs: None,
            },
        ];

        let runs = vec![run1];
        let metrics = calculate_workflow_metrics("CI", &runs, 100.0);

        assert_eq!(metrics.jobs.len(), 2);
        // Jobs should be sorted by time_to_feedback_p95 descending
        assert_eq!(metrics.jobs[0].name, "test"); // 600s > 300s
        assert_eq!(metrics.jobs[1].name, "build");
    }
}
