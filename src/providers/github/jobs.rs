use std::collections::HashMap;

use crate::insights::{JobCountWithLinks, JobMetrics};

use super::types::GitHubJob;

/// Calculates comprehensive metrics for each job based on all its executions.
///
/// # Arguments
///
/// * `jobs` - All job executions
/// * `min_executions_percentage` - Minimum percentage of total executions for a job to be included
///
/// Filters out jobs below the minimum execution percentage threshold to reduce noise.
pub fn calculate_job_metrics(
    jobs: Vec<GitHubJob>,
    min_executions_percentage: f64,
) -> Vec<JobMetrics> {
    // Filter only completed jobs
    let completed_jobs: Vec<GitHubJob> = jobs
        .into_iter()
        .filter(|job| job.status == "completed")
        .collect();

    let mut metrics = Vec::new();
    let jobs_by_name = aggregate_jobs_by_name(completed_jobs);

    // Calculate total executions for filtering
    let total_executions: usize = jobs_by_name.values().map(Vec::len).sum();
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let min_executions =
        ((total_executions as f64) * (min_executions_percentage / 100.0)).ceil() as usize;

    for (name, executions) in &jobs_by_name {
        // Filter out jobs below the minimum execution threshold
        if executions.len() < min_executions {
            continue;
        }

        // Calculate reliability metrics (failure rate, retry rate)
        let reliability = calculate_job_reliability(executions);

        // All successful jobs for duration metrics
        let successful_durations: Vec<f64> = executions
            .iter()
            .filter(|job| job.conclusion.as_deref() == Some("success"))
            .map(|job| job.duration)
            .collect();

        let (duration_p50, duration_p95, duration_p99) =
            calculate_percentiles(&successful_durations);

        // Only successful first-try jobs for time-to-feedback
        #[allow(clippy::cast_precision_loss)]
        let time_to_feedback_values: Vec<f64> = executions
            .iter()
            .filter(|job| job.conclusion.as_deref() == Some("success") && job.run_attempt == 1)
            .filter_map(|job| {
                job.completed_at.map(|completed_at| {
                    (completed_at - job.workflow_run_started_at).num_seconds() as f64
                })
            })
            .collect();

        let (time_to_feedback_p50, time_to_feedback_p95, time_to_feedback_p99) =
            calculate_percentiles(&time_to_feedback_values);

        // GitHub API doesn't provide job dependencies in the response we're using
        // This could be enhanced by parsing workflow YAML files
        let predecessors = vec!["N/A".to_string()];

        metrics.push(JobMetrics {
            name: name.clone(),
            duration_p50,
            duration_p95,
            duration_p99,
            time_to_feedback_p50,
            time_to_feedback_p95,
            time_to_feedback_p99,
            predecessors,
            retry_rate: reliability.retry_rate,
            retried_executions: JobCountWithLinks {
                count: reliability.retried_jobs_count,
                links: reliability.retried_job_links,
            },
            failure_rate: reliability.failure_rate,
            failed_executions: JobCountWithLinks {
                count: reliability.failed_jobs_count,
                links: reliability.failed_job_links,
            },
            success_rate: reliability.success_rate,
            successful_executions: JobCountWithLinks {
                count: reliability.successful_jobs_count,
                links: reliability.successful_job_links,
            },
            total_executions: reliability.total_executions,
        });
    }

    // Sort by time_to_feedback_p50 descending (slowest typical feedback first)
    metrics.sort_by(|a, b| {
        b.time_to_feedback_p50
            .partial_cmp(&a.time_to_feedback_p50)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    metrics
}

struct JobReliabilityMetrics {
    total_executions: usize,
    retry_rate: f64,
    retried_jobs_count: usize,
    retried_job_links: Vec<String>,
    failure_rate: f64,
    failed_jobs_count: usize,
    failed_job_links: Vec<String>,
    success_rate: f64,
    successful_jobs_count: usize,
    successful_job_links: Vec<String>,
}

fn calculate_job_reliability(executions: &[GitHubJob]) -> JobReliabilityMetrics {
    let total_executions = executions.len();

    // GitHub uses run_attempt > 1 to indicate retries
    let retried_job_links: Vec<String> = executions
        .iter()
        .filter(|job| job.run_attempt > 1)
        .map(|job| job.html_url.clone())
        .collect();
    let retried_jobs_count = retried_job_links.len();
    let retry_rate = calculate_rate(retried_jobs_count, total_executions);

    let failed_job_links: Vec<String> = executions
        .iter()
        .filter(|job| job.conclusion.as_deref() == Some("failure"))
        .map(|job| job.html_url.clone())
        .collect();
    let failed_jobs_count = failed_job_links.len();
    let failure_rate = calculate_rate(failed_jobs_count, total_executions);

    let successful_job_links: Vec<String> = executions
        .iter()
        .filter(|job| job.conclusion.as_deref() == Some("success"))
        .map(|job| job.html_url.clone())
        .collect();
    let successful_jobs_count = successful_job_links.len();
    let success_rate = calculate_rate(successful_jobs_count, total_executions);

    JobReliabilityMetrics {
        total_executions,
        retry_rate,
        retried_jobs_count,
        retried_job_links,
        failure_rate,
        failed_jobs_count,
        failed_job_links,
        success_rate,
        successful_jobs_count,
        successful_job_links,
    }
}

/// Aggregates jobs by name, grouping all executions of the same job together.
fn aggregate_jobs_by_name(jobs: Vec<GitHubJob>) -> HashMap<String, Vec<GitHubJob>> {
    jobs.into_iter().fold(HashMap::new(), |mut acc, job| {
        acc.entry(job.name.clone()).or_default().push(job);
        acc
    })
}

#[allow(clippy::cast_precision_loss)]
fn calculate_rate(count: usize, total: usize) -> f64 {
    if total > 0 {
        (count as f64 / total as f64) * 100.0
    } else {
        0.0
    }
}

/// Calculates P50, P95, P99 percentiles for a set of values.
fn calculate_percentiles(values: &[f64]) -> (f64, f64, f64) {
    if values.is_empty() {
        return (0.0, 0.0, 0.0);
    }

    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let p50 = percentile(&sorted, 0.50);
    let p95 = percentile(&sorted, 0.95);
    let p99 = percentile(&sorted, 0.99);

    (p50, p95, p99)
}

/// Calculates a specific percentile from sorted values.
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let index = ((sorted.len() - 1) as f64 * p) as usize;
    sorted[index]
}

#[cfg(test)]
#[allow(clippy::similar_names, clippy::float_cmp)]
mod tests {
    use super::*;
    use chrono::{DateTime, TimeZone, Utc};

    fn create_test_job(
        id: u64,
        name: &str,
        conclusion: Option<&str>,
        run_attempt: u32,
        duration: f64,
        workflow_run_started_at: DateTime<Utc>,
        completed_at: Option<DateTime<Utc>>,
    ) -> GitHubJob {
        GitHubJob {
            id,
            run_id: 1,
            name: name.to_string(),
            workflow_name: "test-workflow".to_string(),
            status: "completed".to_string(),
            conclusion: conclusion.map(String::from),
            run_attempt,
            duration,
            started_at: Utc.timestamp_opt(1_609_459_200, 0).unwrap(),
            completed_at,
            workflow_run_started_at,
            html_url: format!("https://github.com/owner/repo/actions/runs/1/jobs/{id}"),
        }
    }

    #[test]
    fn test_calculate_job_metrics_empty_jobs() {
        let jobs = vec![];
        let metrics = calculate_job_metrics(jobs, 1.0);
        assert_eq!(metrics.len(), 0);
    }

    #[test]
    fn test_calculate_job_metrics_single_successful_job() {
        let workflow_start = Utc.timestamp_opt(1_609_459_200, 0).unwrap();
        let job_complete = Utc.timestamp_opt(1_609_459_260, 0).unwrap();

        let jobs = vec![create_test_job(
            1,
            "build",
            Some("success"),
            1,
            60.0,
            workflow_start,
            Some(job_complete),
        )];

        let metrics = calculate_job_metrics(jobs, 1.0);
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].name, "build");
        assert_eq!(metrics[0].duration_p50, 60.0);
        assert_eq!(metrics[0].duration_p95, 60.0);
        assert_eq!(metrics[0].duration_p99, 60.0);
        assert_eq!(metrics[0].time_to_feedback_p50, 60.0);
        assert_eq!(metrics[0].success_rate, 100.0);
        assert_eq!(metrics[0].failure_rate, 0.0);
        assert_eq!(metrics[0].retry_rate, 0.0);
        assert_eq!(metrics[0].total_executions, 1);
    }

    #[test]
    fn test_calculate_job_metrics_filters_incomplete_jobs() {
        let workflow_start = Utc.timestamp_opt(1_609_459_200, 0).unwrap();

        let mut jobs = vec![create_test_job(
            1,
            "build",
            Some("success"),
            1,
            60.0,
            workflow_start,
            Some(Utc.timestamp_opt(1_609_459_260, 0).unwrap()),
        )];

        // Add an incomplete job (status != "completed")
        let mut incomplete = jobs[0].clone();
        incomplete.id = 2;
        incomplete.status = "in_progress".to_string();
        jobs.push(incomplete);

        let metrics = calculate_job_metrics(jobs, 1.0);
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].total_executions, 1); // Only completed job counted
    }

    #[test]
    fn test_calculate_job_metrics_multiple_executions_same_job() {
        let workflow_start = Utc.timestamp_opt(1_609_459_200, 0).unwrap();

        let jobs = vec![
            create_test_job(
                1,
                "build",
                Some("success"),
                1,
                50.0,
                workflow_start,
                Some(Utc.timestamp_opt(1_609_459_250, 0).unwrap()),
            ),
            create_test_job(
                2,
                "build",
                Some("success"),
                1,
                100.0,
                workflow_start,
                Some(Utc.timestamp_opt(1_609_459_300, 0).unwrap()),
            ),
            create_test_job(
                3,
                "build",
                Some("success"),
                1,
                150.0,
                workflow_start,
                Some(Utc.timestamp_opt(1_609_459_350, 0).unwrap()),
            ),
        ];

        let metrics = calculate_job_metrics(jobs, 1.0);
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].name, "build");
        assert_eq!(metrics[0].total_executions, 3);
        assert_eq!(metrics[0].duration_p50, 100.0); // Middle value
    }

    #[test]
    fn test_calculate_job_metrics_retry_detection() {
        let workflow_start = Utc.timestamp_opt(1_609_459_200, 0).unwrap();

        let jobs = vec![
            create_test_job(
                1,
                "build",
                Some("success"),
                1,
                60.0,
                workflow_start,
                Some(Utc.timestamp_opt(1_609_459_260, 0).unwrap()),
            ),
            create_test_job(
                2,
                "build",
                Some("success"),
                2,
                60.0,
                workflow_start,
                Some(Utc.timestamp_opt(1_609_459_260, 0).unwrap()),
            ), // Retry
            create_test_job(
                3,
                "build",
                Some("success"),
                3,
                60.0,
                workflow_start,
                Some(Utc.timestamp_opt(1_609_459_260, 0).unwrap()),
            ), // Another retry
        ];

        let metrics = calculate_job_metrics(jobs, 1.0);
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].retry_rate, 66.666_666_666_666_66); // 2 out of 3
        assert_eq!(metrics[0].retried_executions.count, 2);
    }

    #[test]
    fn test_calculate_job_metrics_failure_rate() {
        let workflow_start = Utc.timestamp_opt(1_609_459_200, 0).unwrap();

        let jobs = vec![
            create_test_job(
                1,
                "test",
                Some("success"),
                1,
                60.0,
                workflow_start,
                Some(Utc.timestamp_opt(1_609_459_260, 0).unwrap()),
            ),
            create_test_job(
                2,
                "test",
                Some("failure"),
                1,
                30.0,
                workflow_start,
                Some(Utc.timestamp_opt(1_609_459_230, 0).unwrap()),
            ),
            create_test_job(
                3,
                "test",
                Some("failure"),
                1,
                40.0,
                workflow_start,
                Some(Utc.timestamp_opt(1_609_459_240, 0).unwrap()),
            ),
            create_test_job(
                4,
                "test",
                Some("success"),
                1,
                70.0,
                workflow_start,
                Some(Utc.timestamp_opt(1_609_459_270, 0).unwrap()),
            ),
        ];

        let metrics = calculate_job_metrics(jobs, 1.0);
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].failure_rate, 50.0); // 2 out of 4
        assert_eq!(metrics[0].failed_executions.count, 2);
        assert_eq!(metrics[0].success_rate, 50.0); // 2 out of 4
        assert_eq!(metrics[0].successful_executions.count, 2);
    }

    #[test]
    fn test_calculate_job_metrics_sorts_by_time_to_feedback() {
        let workflow_start = Utc.timestamp_opt(1_609_459_200, 0).unwrap();

        let jobs = vec![
            create_test_job(
                1,
                "fast",
                Some("success"),
                1,
                30.0,
                workflow_start,
                Some(Utc.timestamp_opt(1_609_459_230, 0).unwrap()),
            ), // TTF = 30s
            create_test_job(
                2,
                "slow",
                Some("success"),
                1,
                300.0,
                workflow_start,
                Some(Utc.timestamp_opt(1_609_459_500, 0).unwrap()),
            ), // TTF = 300s
            create_test_job(
                3,
                "medium",
                Some("success"),
                1,
                120.0,
                workflow_start,
                Some(Utc.timestamp_opt(1_609_459_320, 0).unwrap()),
            ), // TTF = 120s
        ];

        let metrics = calculate_job_metrics(jobs, 1.0);
        assert_eq!(metrics.len(), 3);
        // Should be sorted by time_to_feedback descending (slowest first)
        assert_eq!(metrics[0].name, "slow");
        assert_eq!(metrics[0].time_to_feedback_p50, 300.0);
        assert_eq!(metrics[1].name, "medium");
        assert_eq!(metrics[1].time_to_feedback_p50, 120.0);
        assert_eq!(metrics[2].name, "fast");
        assert_eq!(metrics[2].time_to_feedback_p50, 30.0);
    }

    #[test]
    fn test_calculate_percentiles_empty() {
        let values: Vec<f64> = vec![];
        let (p50, p95, p99) = calculate_percentiles(&values);
        assert_eq!(p50, 0.0);
        assert_eq!(p95, 0.0);
        assert_eq!(p99, 0.0);
    }

    #[test]
    fn test_calculate_percentiles_single_value() {
        let values = vec![42.0];
        let (p50, p95, p99) = calculate_percentiles(&values);
        assert_eq!(p50, 42.0);
        assert_eq!(p95, 42.0);
        assert_eq!(p99, 42.0);
    }

    #[test]
    fn test_calculate_rate_zero_total() {
        let result = calculate_rate(5, 0);
        assert_eq!(result, 0.0);
    }

    #[test]
    fn test_calculate_rate_percentage() {
        let result = calculate_rate(25, 100);
        assert_eq!(result, 25.0);
    }
}
