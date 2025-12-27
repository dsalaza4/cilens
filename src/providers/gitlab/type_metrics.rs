use std::cmp::Ordering;
use std::collections::HashMap;

use super::types::{GitLabJob, GitLabPipeline};
use super::url_utils::{job_id_to_url, pipeline_id_to_url};
use crate::insights::{
    JobCountWithLinks, JobMetrics, PipelineCountWithLinks, PredecessorJob, TypeMetrics,
};

fn cmp_f64(a: &f64, b: &f64) -> Ordering {
    a.partial_cmp(b).unwrap_or(Ordering::Equal)
}

#[allow(clippy::cast_precision_loss)]
fn calculate_rate(count: usize, total: usize) -> f64 {
    if total > 0 {
        (count as f64 / total as f64) * 100.0
    } else {
        0.0
    }
}

fn empty_job_count() -> JobCountWithLinks {
    JobCountWithLinks {
        count: 0,
        links: vec![],
    }
}

pub fn calculate_type_metrics(
    pipelines: &[&GitLabPipeline],
    percentage: f64,
    base_url: &str,
    project_path: &str,
) -> TypeMetrics {
    let total_pipelines = pipelines.len();

    let successful: Vec<_> = pipelines
        .iter()
        .filter(|p| p.status == "success")
        .copied()
        .collect();

    let failed: Vec<_> = pipelines
        .iter()
        .filter(|p| p.status == "failed")
        .copied()
        .collect();

    let successful_pipelines = to_pipeline_links(&successful, base_url, project_path);
    let failed_pipelines = to_pipeline_links(&failed, base_url, project_path);

    let (jobs, avg_time_to_feedback_seconds) =
        aggregate_job_metrics(&successful, pipelines, base_url, project_path);

    TypeMetrics {
        percentage,
        total_pipelines,
        successful_pipelines,
        failed_pipelines,
        success_rate: calculate_success_rate(successful.len(), total_pipelines),
        avg_duration_seconds: calculate_avg_duration(&successful),
        avg_time_to_feedback_seconds,
        jobs,
    }
}

fn to_pipeline_links(
    pipelines: &[&GitLabPipeline],
    base_url: &str,
    project_path: &str,
) -> PipelineCountWithLinks {
    PipelineCountWithLinks {
        count: pipelines.len(),
        links: pipelines
            .iter()
            .map(|p| pipeline_id_to_url(base_url, project_path, &p.id))
            .collect(),
    }
}

#[allow(clippy::cast_precision_loss)]
fn calculate_success_rate(successful: usize, total: usize) -> f64 {
    (successful as f64 / total.max(1) as f64) * 100.0
}

#[allow(clippy::cast_precision_loss)]
fn calculate_avg_duration(pipelines: &[&GitLabPipeline]) -> f64 {
    if pipelines.is_empty() {
        return 0.0;
    }
    pipelines.iter().map(|p| p.duration as f64).sum::<f64>() / pipelines.len() as f64
}

#[allow(clippy::cast_precision_loss)]
fn aggregate_job_metrics(
    successful_pipelines: &[&GitLabPipeline],
    all_pipelines: &[&GitLabPipeline],
    base_url: &str,
    project_path: &str,
) -> (Vec<JobMetrics>, f64) {
    if successful_pipelines.is_empty() {
        return (vec![], 0.0);
    }

    // Calculate job metrics once per pipeline
    let per_pipeline_metrics: Vec<Vec<JobMetrics>> = successful_pipelines
        .iter()
        .map(|p| super::job_analysis::calculate_job_metrics(p))
        .collect();

    // Calculate pipeline-level avg_time_to_feedback from per-pipeline data
    let first_feedback_times: Vec<f64> = per_pipeline_metrics
        .iter()
        .filter_map(|pipeline_metrics| {
            pipeline_metrics
                .iter()
                .map(|job| job.avg_time_to_feedback_seconds)
                .min_by(cmp_f64)
        })
        .collect();

    let avg_time_to_feedback = if first_feedback_times.is_empty() {
        0.0
    } else {
        first_feedback_times.iter().sum::<f64>() / first_feedback_times.len() as f64
    };

    // Aggregate job data across all pipelines
    let mut job_data: HashMap<String, JobData> = HashMap::new();
    for metrics in &per_pipeline_metrics {
        for job_metric in metrics {
            let data = job_data.entry(job_metric.name.clone()).or_default();
            data.durations.push(job_metric.avg_duration_seconds);
            data.total_durations
                .push(job_metric.avg_time_to_feedback_seconds);
            let predecessor_names = job_metric
                .predecessors
                .iter()
                .map(|p| p.name.clone())
                .collect();
            data.all_predecessor_names.push(predecessor_names);
        }
    }

    let avg_durations: HashMap<String, f64> = job_data
        .iter()
        .map(|(name, data)| (name.clone(), compute_mean(&data.durations)))
        .collect();

    let reliability_data = calculate_job_reliability(all_pipelines, base_url, project_path);

    let mut jobs: Vec<JobMetrics> = job_data
        .into_iter()
        .map(|(name, data)| build_job_metrics(&name, &data, &avg_durations, &reliability_data))
        .collect();

    jobs.sort_by(|a, b| cmp_f64(&b.avg_time_to_feedback_seconds, &a.avg_time_to_feedback_seconds));

    (jobs, avg_time_to_feedback)
}

#[derive(Default)]
struct JobData {
    durations: Vec<f64>,
    total_durations: Vec<f64>,
    all_predecessor_names: Vec<Vec<String>>,
}

#[allow(clippy::cast_precision_loss)]
fn compute_mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

fn build_job_metrics(
    name: &str,
    data: &JobData,
    avg_durations: &HashMap<String, f64>,
    reliability_data: &HashMap<String, JobReliabilityMetrics>,
) -> JobMetrics {
    let avg_duration_seconds = *avg_durations.get(name).unwrap_or(&0.0);
    let avg_time_to_feedback_seconds = compute_mean(&data.total_durations);
    let predecessors = aggregate_predecessors(&data.all_predecessor_names, avg_durations);

    let (total_executions, flakiness_rate, flaky_retries, failure_rate, failed_executions) =
        match reliability_data.get(name) {
            Some(r) => (
                r.total_executions,
                r.flakiness_rate,
                JobCountWithLinks {
                    count: r.flaky_retries,
                    links: r.flaky_job_links.clone(),
                },
                r.failure_rate,
                JobCountWithLinks {
                    count: r.failed_executions,
                    links: r.failed_job_links.clone(),
                },
            ),
            None => (0, 0.0, empty_job_count(), 0.0, empty_job_count()),
        };

    JobMetrics {
        name: name.to_string(),
        avg_duration_seconds,
        avg_time_to_feedback_seconds,
        predecessors,
        flakiness_rate,
        flaky_retries,
        failed_executions,
        failure_rate,
        total_executions,
    }
}

struct JobReliabilityMetrics {
    total_executions: usize,
    flakiness_rate: f64,
    flaky_retries: usize,
    flaky_job_links: Vec<String>,
    failure_rate: f64,
    failed_executions: usize,
    failed_job_links: Vec<String>,
}

fn aggregate_predecessors(
    all_predecessor_names: &[Vec<String>],
    avg_durations: &HashMap<String, f64>,
) -> Vec<PredecessorJob> {
    if all_predecessor_names.is_empty() {
        return vec![];
    }

    let predecessor_names: std::collections::HashSet<String> = all_predecessor_names
        .iter()
        .flat_map(|names| names.iter())
        .cloned()
        .collect();

    let mut result: Vec<PredecessorJob> = predecessor_names
        .into_iter()
        .filter_map(|name| create_predecessor_job(name, avg_durations))
        .collect();

    result.sort_by(|a, b| cmp_f64(&b.avg_duration_seconds, &a.avg_duration_seconds));

    result
}

fn create_predecessor_job(
    name: String,
    avg_durations: &HashMap<String, f64>,
) -> Option<PredecessorJob> {
    avg_durations
        .get(&name)
        .map(|&avg_duration_seconds| PredecessorJob {
            name,
            avg_duration_seconds,
        })
}

fn calculate_job_reliability(
    pipelines: &[&GitLabPipeline],
    base_url: &str,
    project_path: &str,
) -> HashMap<String, JobReliabilityMetrics> {
    let mut execution_counts: HashMap<String, usize> = HashMap::new();
    let mut flaky_retries: HashMap<String, usize> = HashMap::new();
    let mut flaky_job_links: HashMap<String, Vec<String>> = HashMap::new();
    let mut failed_executions: HashMap<String, usize> = HashMap::new();
    let mut failed_job_links: HashMap<String, Vec<String>> = HashMap::new();

    for pipeline in pipelines {
        let jobs_by_name = group_jobs_by_name(&pipeline.jobs);

        for (name, jobs) in jobs_by_name {
            *execution_counts.entry(name.to_string()).or_insert(0) += jobs.len();

            if is_job_flaky(&jobs) {
                let retry_links: Vec<String> = jobs
                    .iter()
                    .filter(|j| j.retried)
                    .map(|j| job_id_to_url(base_url, project_path, &j.id))
                    .collect();
                *flaky_retries.entry(name.to_string()).or_insert(0) += retry_links.len();
                flaky_job_links
                    .entry(name.to_string())
                    .or_default()
                    .extend(retry_links);
            } else if is_job_failed(&jobs) {
                *failed_executions.entry(name.to_string()).or_insert(0) += 1;
                // Get the final non-retried job (the one that failed)
                if let Some(final_job) = jobs.iter().find(|j| !j.retried) {
                    failed_job_links
                        .entry(name.to_string())
                        .or_default()
                        .push(job_id_to_url(base_url, project_path, &final_job.id));
                }
            }
        }
    }

    compute_reliability_metrics(
        &flaky_retries,
        &flaky_job_links,
        &failed_executions,
        &failed_job_links,
        &execution_counts,
    )
}

fn compute_reliability_metrics(
    retry_counts: &HashMap<String, usize>,
    retry_job_links: &HashMap<String, Vec<String>>,
    failure_counts: &HashMap<String, usize>,
    failure_job_links: &HashMap<String, Vec<String>>,
    execution_counts: &HashMap<String, usize>,
) -> HashMap<String, JobReliabilityMetrics> {
    execution_counts
        .iter()
        .map(|(name, &total_executions)| {
            let flaky_retries = *retry_counts.get(name).unwrap_or(&0);
            let failed_executions = *failure_counts.get(name).unwrap_or(&0);
            let flaky_job_links = retry_job_links.get(name).cloned().unwrap_or_default();
            let failed_job_links = failure_job_links.get(name).cloned().unwrap_or_default();

            (
                name.clone(),
                JobReliabilityMetrics {
                    total_executions,
                    flakiness_rate: calculate_rate(flaky_retries, total_executions),
                    flaky_retries,
                    flaky_job_links,
                    failure_rate: calculate_rate(failed_executions, total_executions),
                    failed_executions,
                    failed_job_links,
                },
            )
        })
        .collect()
}

fn group_jobs_by_name(jobs: &[GitLabJob]) -> HashMap<&str, Vec<&GitLabJob>> {
    jobs.iter().fold(HashMap::new(), |mut grouped, job| {
        grouped.entry(job.name.as_str()).or_default().push(job);
        grouped
    })
}

fn is_job_flaky(jobs: &[&GitLabJob]) -> bool {
    // Flaky = job was retried AND eventually succeeded
    let was_retried = jobs.iter().any(|j| j.retried);
    let final_succeeded = jobs
        .iter()
        .find(|j| !j.retried)
        .is_some_and(|j| j.status == "SUCCESS");

    was_retried && final_succeeded
}

fn is_job_failed(jobs: &[&GitLabJob]) -> bool {
    // Failed = job did not eventually succeed (opposite of flaky)
    // A job failed if there's no successful non-retried job
    jobs.iter()
        .find(|j| !j.retried)
        .is_none_or(|j| j.status != "SUCCESS")
}
