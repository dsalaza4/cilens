use std::collections::HashMap;

use crate::insights::{JobCountWithLinks, JobMetrics};

use super::types::GitLabJob;

/// Calculates comprehensive metrics for each job based on all its executions.
///
/// # Arguments
///
/// * `jobs` - All job executions
/// * `base_url` - GitLab instance base URL
/// * `project_path` - Project path
/// * `min_executions_percentage` - Minimum percentage of total executions for a job to be included (e.g., 0.2 means 0.2%)
///
/// Filters out jobs below the minimum execution percentage threshold to reduce noise.
pub fn calculate_job_metrics(
    jobs: Vec<GitLabJob>,
    base_url: &str,
    project_path: &str,
    min_executions_percentage: f64,
) -> Vec<JobMetrics> {
    let mut metrics = Vec::new();
    let jobs_by_name = aggregate_jobs_by_name(jobs);

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
        let reliability = calculate_job_reliability(base_url, project_path, executions);

        // All successful jobs for duration metrics
        let successful: Vec<_> = executions
            .iter()
            .filter(|job| job.status == "SUCCESS")
            .collect();

        let (duration_p50, duration_p95, duration_p99) = calculate_percentiles(
            &successful
                .iter()
                .map(|job| job.duration)
                .collect::<Vec<_>>(),
        );

        // Only successful first-try jobs for time-to-feedback
        // (retried jobs have created_at from retry time, not pipeline start)
        let successful_first_try: Vec<_> = executions
            .iter()
            .filter(|job| job.status == "SUCCESS" && !job.retried)
            .collect();

        let (time_to_feedback_p50, time_to_feedback_p95, time_to_feedback_p99) =
            calculate_percentiles(
                &successful_first_try
                    .iter()
                    .map(|job| {
                        #[allow(clippy::cast_precision_loss)]
                        let seconds = (job.finished_at - job.created_at).num_seconds() as f64;
                        seconds
                    })
                    .collect::<Vec<f64>>(),
            );

        // Extract predecessors from needs field
        let predecessors = extract_predecessors(executions);

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

fn calculate_job_reliability(
    base_url: &str,
    project_path: &str,
    executions: &[GitLabJob],
) -> JobReliabilityMetrics {
    let total_executions = executions.len();

    let retried_jobs: Vec<&GitLabJob> = executions.iter().filter(|job| job.retried).collect();
    let retried_jobs_count = retried_jobs.len();
    let retry_rate = calculate_rate(retried_jobs_count, total_executions);
    let retried_job_links = retried_jobs
        .iter()
        .map(|job| job_id_to_url(base_url, project_path, &job.id))
        .collect::<Vec<String>>();

    let failed_jobs: Vec<&GitLabJob> = executions
        .iter()
        .filter(|job| job.status == "FAILED")
        .collect();
    let failed_jobs_count = failed_jobs.len();
    let failure_rate = calculate_rate(failed_jobs_count, total_executions);
    let failed_job_links = failed_jobs
        .iter()
        .map(|job| job_id_to_url(base_url, project_path, &job.id))
        .collect::<Vec<String>>();

    let successful_jobs: Vec<&GitLabJob> = executions
        .iter()
        .filter(|job| job.status == "SUCCESS")
        .collect();
    let successful_jobs_count = successful_jobs.len();
    let success_rate = calculate_rate(successful_jobs_count, total_executions);
    let successful_job_links = successful_jobs
        .iter()
        .map(|job| job_id_to_url(base_url, project_path, &job.id))
        .collect::<Vec<String>>();

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
fn aggregate_jobs_by_name(jobs: Vec<GitLabJob>) -> HashMap<String, Vec<GitLabJob>> {
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

/// Extracts predecessor jobs based on the 'needs' field.
fn extract_predecessors(executions: &[GitLabJob]) -> Vec<String> {
    // Collect all unique predecessor names from needs fields
    let mut names: Vec<String> = executions
        .iter()
        .flat_map(|job| job.needs.iter())
        .cloned()
        .collect();

    names.sort();
    names.dedup();
    names
}

/// Converts a GitLab job GID to a clickable web URL.
///
/// Extracts the numeric ID from a GraphQL Global ID (GID) and constructs
/// a direct link to the job's web page in GitLab.
///
/// # Arguments
///
/// * `base_url` - GitLab instance base URL (e.g., <https://gitlab.com>)
/// * `project_path` - Project path (e.g., "group/project")
/// * `gid` - GraphQL Global ID (e.g., <gid://gitlab/Ci::Job/456>)
///
/// # Returns
///
/// Clickable URL to the job (e.g., <https://gitlab.com/group/project/-/jobs/456>)
pub fn job_id_to_url(base_url: &str, project_path: &str, gid: &str) -> String {
    let id = gid.rsplit('/').next().unwrap_or(gid);
    format!("{base_url}/{project_path}/-/jobs/{id}")
}

#[cfg(test)]
#[allow(clippy::similar_names, clippy::float_cmp)]
mod tests {
    use super::*;

    #[cfg(test)]
    mod calculate_rate {
        use super::*;

        #[test]
        fn returns_zero_when_total_is_zero() {
            let result = calculate_rate(5, 0);
            assert_eq!(result, 0.0, "Should return 0.0 when total is 0");
        }

        #[test]
        fn returns_zero_when_count_is_zero() {
            let result = calculate_rate(0, 100);
            assert_eq!(result, 0.0, "Should return 0.0 when count is 0");
        }

        #[test]
        fn calculates_percentage_correctly() {
            let result = calculate_rate(25, 100);
            assert_eq!(result, 25.0, "Should calculate 25% correctly");
        }

        #[test]
        fn calculates_fifty_percent() {
            let result = calculate_rate(50, 100);
            assert_eq!(result, 50.0, "Should calculate 50% correctly");
        }

        #[test]
        fn calculates_one_hundred_percent() {
            let result = calculate_rate(100, 100);
            assert_eq!(result, 100.0, "Should calculate 100% correctly");
        }

        #[test]
        fn handles_fractional_percentages() {
            let result = calculate_rate(1, 3);
            assert!(
                (result - 33.333_333).abs() < 0.001,
                "Should handle fractional percentages, got {result}",
            );
        }

        #[test]
        fn handles_small_numbers() {
            let result = calculate_rate(1, 1);
            assert_eq!(result, 100.0, "Should handle 1/1 = 100%");
        }

        #[test]
        fn handles_large_numbers() {
            let result = calculate_rate(999, 1000);
            assert_eq!(result, 99.9, "Should handle large numbers correctly");
        }

        #[test]
        fn returns_zero_for_zero_count_and_total() {
            let result = calculate_rate(0, 0);
            assert_eq!(result, 0.0, "Should return 0.0 when both are 0");
        }

        #[test]
        fn test_job_id_to_url() {
            let url = job_id_to_url(
                "https://gitlab.com",
                "group/project",
                "gid://gitlab/Ci::Job/789012",
            );
            assert_eq!(url, "https://gitlab.com/group/project/-/jobs/789012");
        }

        #[test]
        fn test_job_id_to_url_with_numeric_id() {
            let url = job_id_to_url("https://gitlab.com", "group/project", "123456");
            assert_eq!(url, "https://gitlab.com/group/project/-/jobs/123456");
        }

        #[test]
        fn test_job_id_to_url_with_self_hosted() {
            let url = job_id_to_url(
                "https://gitlab.example.com",
                "org/repo",
                "gid://gitlab/Ci::Job/999",
            );
            assert_eq!(url, "https://gitlab.example.com/org/repo/-/jobs/999");
        }
    }

    mod aggregate_jobs_by_name {
        use super::*;

        #[fixtura::test]
        fn aggregates_jobs_by_name(
            #[fixtura(name = "build".to_string())] job1: GitLabJob,
            #[fixtura(name = "test".to_string())] job2: GitLabJob,
            #[fixtura(name = "build".to_string())] job3: GitLabJob,
        ) {
            let result = aggregate_jobs_by_name(vec![job1, job2, job3]);
            assert_eq!(result.len(), 2);
            assert_eq!(result.get("build").unwrap().len(), 2);
            assert_eq!(result.get("test").unwrap().len(), 1);
        }

        #[test]
        fn handles_empty_list() {
            let result = aggregate_jobs_by_name(vec![]);
            assert_eq!(result.len(), 0);
        }

        #[fixtura::test]
        fn handles_single_job(#[fixtura(name = "deploy".to_string())] job: GitLabJob) {
            let result = aggregate_jobs_by_name(vec![job]);
            assert_eq!(result.len(), 1);
            assert_eq!(result.get("deploy").unwrap().len(), 1);
        }
    }

    mod calculate_percentiles {
        use super::*;

        #[test]
        fn calculates_percentiles_for_odd_length() {
            let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
            let (p50, p95, p99) = calculate_percentiles(&values);
            assert_eq!(p50, 3.0); // index = (5-1) * 0.50 = 2 → values[2]
            assert_eq!(p95, 4.0); // index = (5-1) * 0.95 = 3.8 → 3 → values[3]
            assert_eq!(p99, 4.0); // index = (5-1) * 0.99 = 3.96 → 3 → values[3]
        }

        #[test]
        fn calculates_percentiles_for_even_length() {
            let values = vec![1.0, 2.0, 3.0, 4.0];
            let (p50, _p95, _p99) = calculate_percentiles(&values);
            assert_eq!(p50, 2.0);
        }

        #[test]
        fn handles_empty_values() {
            let values = vec![];
            let (p50, p95, p99) = calculate_percentiles(&values);
            assert_eq!(p50, 0.0);
            assert_eq!(p95, 0.0);
            assert_eq!(p99, 0.0);
        }

        #[test]
        fn handles_single_value() {
            let values = vec![42.0];
            let (p50, p95, p99) = calculate_percentiles(&values);
            assert_eq!(p50, 42.0);
            assert_eq!(p95, 42.0);
            assert_eq!(p99, 42.0);
        }

        #[test]
        fn sorts_unsorted_values() {
            let values = vec![5.0, 1.0, 3.0, 2.0, 4.0];
            let (p50, _, _) = calculate_percentiles(&values);
            assert_eq!(p50, 3.0);
        }
    }

    mod extract_predecessors {
        use super::*;

        #[fixtura::test]
        fn extracts_unique_predecessors(
            #[fixtura(needs = vec!["build".to_string(), "lint".to_string()])] job1: GitLabJob,
            #[fixtura(needs = vec!["build".to_string()])] job2: GitLabJob,
        ) {
            let executions = vec![job1, job2];
            let result = extract_predecessors(&executions);
            assert_eq!(result.len(), 2);
            assert!(result.contains(&"build".to_string()));
            assert!(result.contains(&"lint".to_string()));
        }

        #[fixtura::test]
        fn handles_no_predecessors(#[fixtura(needs = vec![])] job: GitLabJob) {
            let result = extract_predecessors(&[job]);
            assert_eq!(result.len(), 0);
        }

        #[fixtura::test]
        fn deduplicates_predecessors(
            #[fixtura(needs = vec!["build".to_string()])] job1: GitLabJob,
            #[fixtura(needs = vec!["build".to_string()])] job2: GitLabJob,
            #[fixtura(needs = vec!["build".to_string()])] job3: GitLabJob,
        ) {
            let executions = vec![job1, job2, job3];
            let result = extract_predecessors(&executions);
            assert_eq!(result.len(), 1);
            assert_eq!(result[0], "build");
        }

        #[fixtura::test]
        fn returns_sorted_predecessors(
            #[fixtura(needs = vec!["zyx".to_string(), "abc".to_string(), "mno".to_string()])]
            job: GitLabJob,
        ) {
            let result = extract_predecessors(&[job]);
            assert_eq!(result, vec!["abc", "mno", "zyx"]);
        }
    }
}
