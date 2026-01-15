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

    // Calculate job metrics
    let jobs = calculate_job_metrics(workflow_name, &successful);

    // For time-to-feedback, we'll use the minimum job completion time across all runs
    // This represents when developers get the first feedback
    let time_to_feedback_values: Vec<f64> = successful
        .iter()
        .filter_map(|run| {
            run.jobs
                .iter()
                .filter_map(|job| job.duration_seconds())
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

/// Calculates metrics for individual jobs within a workflow.
fn calculate_job_metrics(
    workflow_name: &str,
    successful_runs: &[&GitHubWorkflowRun],
) -> Vec<JobMetrics> {
    use std::collections::HashMap;

    // Group jobs by name
    let mut job_data: HashMap<String, Vec<f64>> = HashMap::new();

    for run in successful_runs {
        for job in &run.jobs {
            if let Some(duration) = job.duration_seconds() {
                job_data.entry(job.name.clone()).or_default().push(duration);
            }
        }
    }

    // Calculate percentiles for each job
    let mut jobs: Vec<JobMetrics> = job_data
        .into_iter()
        .map(|(job_name, durations)| {
            let (duration_p50, duration_p95, duration_p99) = calculate_percentiles(&durations);

            JobMetrics {
                name: job_name,
                pipeline_type_id: workflow_name.to_string(),
                duration_p50,
                duration_p95,
                duration_p99,
                // For GitHub, time-to-feedback is same as duration (no dependencies available)
                time_to_feedback_p50: duration_p50,
                time_to_feedback_p95: duration_p95,
                time_to_feedback_p99: duration_p99,
                predecessors: vec![], // GitHub API doesn't provide job dependencies
                flakiness_rate: 0.0,  // Not tracking flakiness yet
                flaky_retries: JobCountWithLinks::default(),
                failed_executions: JobCountWithLinks::default(),
                failure_rate: 0.0, // Not tracking failures yet
                total_executions: durations.len(),
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
    ) -> GitHubWorkflowRun {
        GitHubWorkflowRun {
            id,
            workflow_name: workflow_name.to_string(),
            head_branch: Some("main".to_string()),
            event: "push".to_string(),
            status: "completed".to_string(),
            conclusion: conclusion.map(String::from),
            run_duration_ms: duration_ms,
            jobs: vec![],
        }
    }

    #[test]
    fn test_calculate_workflow_metrics() {
        let runs = vec![
            create_test_run(1, "CI", Some("success"), Some(300_000)),
            create_test_run(2, "CI", Some("success"), Some(400_000)),
            create_test_run(3, "CI", Some("failure"), Some(200_000)),
        ];

        let metrics = calculate_workflow_metrics("CI", &runs, 100.0);

        assert_eq!(metrics.total_pipelines, 3);
        assert_eq!(metrics.successful_pipelines.count, 2);
        assert_eq!(metrics.failed_pipelines.count, 1);
        assert!((metrics.success_rate - 66.666).abs() < 0.01);
        assert_eq!(metrics.percentage, 100.0);
    }

    #[test]
    fn test_job_metrics_calculation() {
        let mut run1 = create_test_run(1, "CI", Some("success"), Some(300_000));
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
            GitHubJob {
                id: 2,
                name: "test".to_string(),
                status: "completed".to_string(),
                conclusion: Some("success".to_string()),
                started_at: Some("2025-01-01T00:00:00Z".to_string()),
                completed_at: Some("2025-01-01T00:03:00Z".to_string()),
                needs: None,
            },
        ];

        let runs = vec![run1];
        let metrics = calculate_workflow_metrics("CI", &runs, 100.0);

        assert_eq!(metrics.jobs.len(), 2);
        // Jobs should be sorted by time_to_feedback_p95 descending
        assert_eq!(metrics.jobs[0].name, "build"); // 300s > 180s
        assert_eq!(metrics.jobs[1].name, "test");
    }
}
