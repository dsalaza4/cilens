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

