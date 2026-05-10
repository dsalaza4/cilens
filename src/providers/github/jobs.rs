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
    use chrono::{TimeZone, Utc};

    #[test]
    fn test_calculate_job_metrics_empty_jobs() {
        let metrics = calculate_job_metrics(vec![], 1.0);
        assert_eq!(metrics.len(), 0);
    }

    #[fixtura::test]
    fn test_calculate_job_metrics_single_successful_job(
        #[fixtura(
            name = "build".to_string(),
            conclusion = Some("success".to_string()),
            run_attempt = 1u32,
            duration = 60.0_f64,
            status = "completed".to_string(),
            workflow_run_started_at = Utc.timestamp_opt(1_609_459_200, 0).unwrap(),
            completed_at = Some(Utc.timestamp_opt(1_609_459_260, 0).unwrap())
        )]
        job: GitHubJob,
    ) {
        let metrics = calculate_job_metrics(vec![job], 1.0);
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

    #[fixtura::test]
    fn test_calculate_job_metrics_filters_incomplete_jobs(
        #[fixtura(
            name = "build".to_string(),
            conclusion = Some("success".to_string()),
            run_attempt = 1u32,
            duration = 60.0_f64,
            status = "completed".to_string(),
            workflow_run_started_at = Utc.timestamp_opt(1_609_459_200, 0).unwrap(),
            completed_at = Some(Utc.timestamp_opt(1_609_459_260, 0).unwrap())
        )]
        completed: GitHubJob,
        #[fixtura(name = "build".to_string(), status = "in_progress".to_string())]
        incomplete: GitHubJob,
    ) {
        let metrics = calculate_job_metrics(vec![completed, incomplete], 1.0);
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].total_executions, 1);
    }

    #[fixtura::test]
    fn test_calculate_job_metrics_multiple_executions_same_job(
        #[fixtura(
            name = "build".to_string(),
            conclusion = Some("success".to_string()),
            run_attempt = 1u32,
            duration = 50.0_f64,
            status = "completed".to_string(),
            workflow_run_started_at = Utc.timestamp_opt(1_609_459_200, 0).unwrap(),
            completed_at = Some(Utc.timestamp_opt(1_609_459_250, 0).unwrap())
        )]
        job1: GitHubJob,
        #[fixtura(
            name = "build".to_string(),
            conclusion = Some("success".to_string()),
            run_attempt = 1u32,
            duration = 100.0_f64,
            status = "completed".to_string(),
            workflow_run_started_at = Utc.timestamp_opt(1_609_459_200, 0).unwrap(),
            completed_at = Some(Utc.timestamp_opt(1_609_459_300, 0).unwrap())
        )]
        job2: GitHubJob,
        #[fixtura(
            name = "build".to_string(),
            conclusion = Some("success".to_string()),
            run_attempt = 1u32,
            duration = 150.0_f64,
            status = "completed".to_string(),
            workflow_run_started_at = Utc.timestamp_opt(1_609_459_200, 0).unwrap(),
            completed_at = Some(Utc.timestamp_opt(1_609_459_350, 0).unwrap())
        )]
        job3: GitHubJob,
    ) {
        let metrics = calculate_job_metrics(vec![job1, job2, job3], 1.0);
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].name, "build");
        assert_eq!(metrics[0].total_executions, 3);
        assert_eq!(metrics[0].duration_p50, 100.0);
    }

    #[fixtura::test]
    fn test_calculate_job_metrics_retry_detection(
        #[fixtura(
            name = "build".to_string(),
            conclusion = Some("success".to_string()),
            run_attempt = 1u32,
            duration = 60.0_f64,
            status = "completed".to_string(),
            workflow_run_started_at = Utc.timestamp_opt(1_609_459_200, 0).unwrap(),
            completed_at = Some(Utc.timestamp_opt(1_609_459_260, 0).unwrap())
        )]
        job1: GitHubJob,
        #[fixtura(
            name = "build".to_string(),
            conclusion = Some("success".to_string()),
            run_attempt = 2u32,
            duration = 60.0_f64,
            status = "completed".to_string(),
            workflow_run_started_at = Utc.timestamp_opt(1_609_459_200, 0).unwrap(),
            completed_at = Some(Utc.timestamp_opt(1_609_459_260, 0).unwrap())
        )]
        job2: GitHubJob,
        #[fixtura(
            name = "build".to_string(),
            conclusion = Some("success".to_string()),
            run_attempt = 3u32,
            duration = 60.0_f64,
            status = "completed".to_string(),
            workflow_run_started_at = Utc.timestamp_opt(1_609_459_200, 0).unwrap(),
            completed_at = Some(Utc.timestamp_opt(1_609_459_260, 0).unwrap())
        )]
        job3: GitHubJob,
    ) {
        let metrics = calculate_job_metrics(vec![job1, job2, job3], 1.0);
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].retry_rate, 66.666_666_666_666_66);
        assert_eq!(metrics[0].retried_executions.count, 2);
    }

    #[fixtura::test]
    fn test_calculate_job_metrics_failure_rate(
        #[fixtura(
            name = "test".to_string(),
            conclusion = Some("success".to_string()),
            run_attempt = 1u32,
            duration = 60.0_f64,
            status = "completed".to_string(),
            workflow_run_started_at = Utc.timestamp_opt(1_609_459_200, 0).unwrap(),
            completed_at = Some(Utc.timestamp_opt(1_609_459_260, 0).unwrap())
        )]
        job1: GitHubJob,
        #[fixtura(
            name = "test".to_string(),
            conclusion = Some("failure".to_string()),
            run_attempt = 1u32,
            duration = 30.0_f64,
            status = "completed".to_string(),
            workflow_run_started_at = Utc.timestamp_opt(1_609_459_200, 0).unwrap(),
            completed_at = Some(Utc.timestamp_opt(1_609_459_230, 0).unwrap())
        )]
        job2: GitHubJob,
        #[fixtura(
            name = "test".to_string(),
            conclusion = Some("failure".to_string()),
            run_attempt = 1u32,
            duration = 40.0_f64,
            status = "completed".to_string(),
            workflow_run_started_at = Utc.timestamp_opt(1_609_459_200, 0).unwrap(),
            completed_at = Some(Utc.timestamp_opt(1_609_459_240, 0).unwrap())
        )]
        job3: GitHubJob,
        #[fixtura(
            name = "test".to_string(),
            conclusion = Some("success".to_string()),
            run_attempt = 1u32,
            duration = 70.0_f64,
            status = "completed".to_string(),
            workflow_run_started_at = Utc.timestamp_opt(1_609_459_200, 0).unwrap(),
            completed_at = Some(Utc.timestamp_opt(1_609_459_270, 0).unwrap())
        )]
        job4: GitHubJob,
    ) {
        let metrics = calculate_job_metrics(vec![job1, job2, job3, job4], 1.0);
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].failure_rate, 50.0);
        assert_eq!(metrics[0].failed_executions.count, 2);
        assert_eq!(metrics[0].success_rate, 50.0);
        assert_eq!(metrics[0].successful_executions.count, 2);
    }

    #[fixtura::test]
    fn test_calculate_job_metrics_sorts_by_time_to_feedback(
        #[fixtura(
            name = "fast".to_string(),
            conclusion = Some("success".to_string()),
            run_attempt = 1u32,
            duration = 30.0_f64,
            status = "completed".to_string(),
            workflow_run_started_at = Utc.timestamp_opt(1_609_459_200, 0).unwrap(),
            completed_at = Some(Utc.timestamp_opt(1_609_459_230, 0).unwrap())
        )]
        fast: GitHubJob,
        #[fixtura(
            name = "slow".to_string(),
            conclusion = Some("success".to_string()),
            run_attempt = 1u32,
            duration = 300.0_f64,
            status = "completed".to_string(),
            workflow_run_started_at = Utc.timestamp_opt(1_609_459_200, 0).unwrap(),
            completed_at = Some(Utc.timestamp_opt(1_609_459_500, 0).unwrap())
        )]
        slow: GitHubJob,
        #[fixtura(
            name = "medium".to_string(),
            conclusion = Some("success".to_string()),
            run_attempt = 1u32,
            duration = 120.0_f64,
            status = "completed".to_string(),
            workflow_run_started_at = Utc.timestamp_opt(1_609_459_200, 0).unwrap(),
            completed_at = Some(Utc.timestamp_opt(1_609_459_320, 0).unwrap())
        )]
        medium: GitHubJob,
    ) {
        let metrics = calculate_job_metrics(vec![fast, slow, medium], 1.0);
        assert_eq!(metrics.len(), 3);
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
