use std::collections::HashMap;

use super::types::{GitLabJob, GitLabPipeline};
use crate::insights::{JobMetrics, PredecessorJob, TypeMetrics};

pub fn calculate_type_metrics(pipelines: &[&GitLabPipeline], percentage: f64) -> TypeMetrics {
    let total_pipelines = pipelines.len();
    let successful: Vec<_> = pipelines
        .iter()
        .filter(|p| p.status == "success")
        .copied()
        .collect();

    let failed = pipelines.iter().filter(|p| p.status == "failed").count();

    let jobs = calculate_all_job_metrics(&successful, pipelines);

    TypeMetrics {
        percentage,
        total_pipelines,
        successful_pipelines: successful.len(),
        failed_pipelines: failed,
        success_rate: calculate_success_rate(successful.len(), total_pipelines),
        avg_duration_seconds: calculate_avg_duration(&successful),
        jobs,
    }
}

fn calculate_success_rate(successful: usize, total: usize) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let rate = (successful as f64 / total.max(1) as f64) * 100.0;
    rate
}

fn calculate_avg_duration(pipelines: &[&GitLabPipeline]) -> f64 {
    if pipelines.is_empty() {
        return 0.0;
    }

    #[allow(clippy::cast_precision_loss)]
    let avg = pipelines.iter().map(|p| p.duration as f64).sum::<f64>() / pipelines.len() as f64;
    avg
}

fn calculate_all_job_metrics(
    successful_pipelines: &[&GitLabPipeline],
    all_pipelines: &[&GitLabPipeline],
) -> Vec<JobMetrics> {
    if successful_pipelines.is_empty() {
        return vec![];
    }

    // Get per-pipeline job metrics using functional composition
    let all_metrics: Vec<Vec<JobMetrics>> = successful_pipelines
        .iter()
        .map(|p| super::job_analysis::calculate_job_metrics(p))
        .collect();

    // Fold all metrics into aggregated job data
    let job_data = all_metrics
        .iter()
        .flat_map(|metrics| metrics.iter())
        .fold(HashMap::new(), accumulate_job_data);

    // Calculate averaged durations using functional transform
    let avg_durations = compute_avg_durations(&job_data);

    // Get execution counts and flakiness data from all pipelines
    let (execution_counts, flaky_data) = calculate_flakiness(all_pipelines);

    // Transform job data into final metrics and sort
    let mut jobs: Vec<JobMetrics> = job_data
        .into_iter()
        .map(|(name, data)| {
            build_job_metrics(&name, &data, &avg_durations, &execution_counts, &flaky_data)
        })
        .collect();

    // Sort by time to feedback descending (longest time-to-feedback first)
    jobs.sort_by(|a, b| {
        b.avg_time_to_feedback_seconds
            .partial_cmp(&a.avg_time_to_feedback_seconds)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    jobs
}

#[derive(Clone)]
struct JobData {
    durations: Vec<f64>,
    total_durations: Vec<f64>,
    all_predecessors: Vec<Vec<PredecessorJob>>,
}

impl JobData {
    fn new() -> Self {
        Self {
            durations: vec![],
            total_durations: vec![],
            all_predecessors: vec![],
        }
    }

    fn with_metric(mut self, metric: &JobMetrics) -> Self {
        self.durations.push(metric.avg_duration_seconds);
        self.total_durations
            .push(metric.avg_time_to_feedback_seconds);
        self.all_predecessors.push(metric.predecessors.clone());
        self
    }
}

fn accumulate_job_data(
    mut acc: HashMap<String, JobData>,
    job_metric: &JobMetrics,
) -> HashMap<String, JobData> {
    acc.entry(job_metric.name.clone())
        .and_modify(|data| {
            *data = data.clone().with_metric(job_metric);
        })
        .or_insert_with(|| JobData::new().with_metric(job_metric));
    acc
}

fn compute_avg_durations(job_data: &HashMap<String, JobData>) -> HashMap<String, f64> {
    job_data
        .iter()
        .map(|(name, data)| (name.clone(), compute_mean(&data.durations)))
        .collect()
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
    execution_counts: &HashMap<String, usize>,
    flaky_data: &HashMap<String, FlakinessMetrics>,
) -> JobMetrics {
    let avg_duration_seconds = *avg_durations.get(name).unwrap_or(&0.0);
    let avg_time_to_feedback_seconds = compute_mean(&data.total_durations);
    let predecessors = aggregate_predecessors(&data.all_predecessors, avg_durations);
    let total_executions = *execution_counts.get(name).unwrap_or(&0);
    let (flakiness_score, flaky_retries) = flaky_data
        .get(name)
        .map_or((0.0, 0), |f| (f.score, f.flaky_retries));

    JobMetrics {
        name: name.to_string(),
        avg_duration_seconds,
        avg_time_to_feedback_seconds,
        predecessors,
        flakiness_score,
        flaky_retries,
        total_executions,
    }
}

struct FlakinessMetrics {
    score: f64,
    flaky_retries: usize,
}

fn aggregate_predecessors(
    all_predecessors: &[Vec<PredecessorJob>],
    avg_durations: &HashMap<String, f64>,
) -> Vec<PredecessorJob> {
    if all_predecessors.is_empty() {
        return vec![];
    }

    // Collect unique predecessor names using fold instead of mutable HashSet
    let predecessor_names = all_predecessors
        .iter()
        .flat_map(|preds| preds.iter())
        .map(|pred| pred.name.clone())
        .fold(
            std::collections::HashSet::new(),
            |mut names, name| {
                names.insert(name);
                names
            },
        );

    // Transform names into predecessor jobs with averaged durations, then sort
    let mut result: Vec<PredecessorJob> = predecessor_names
        .into_iter()
        .filter_map(|name| create_predecessor_job(name, avg_durations))
        .collect();

    // Sort by avg_duration_seconds descending (slowest first)
    result.sort_by(|a, b| {
        b.avg_duration_seconds
            .partial_cmp(&a.avg_duration_seconds)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    result
}

fn create_predecessor_job(
    name: String,
    avg_durations: &HashMap<String, f64>,
) -> Option<PredecessorJob> {
    avg_durations.get(&name).map(|&avg_duration_seconds| {
        PredecessorJob {
            name,
            avg_duration_seconds,
        }
    })
}

fn calculate_flakiness(
    pipelines: &[&GitLabPipeline],
) -> (HashMap<String, usize>, HashMap<String, FlakinessMetrics>) {
    // Fold all pipelines into execution and retry counts
    let (execution_counts, flaky_retries) = pipelines
        .iter()
        .fold(
            (HashMap::new(), HashMap::new()),
            |(mut exec_counts, mut retry_counts), pipeline| {
                process_pipeline_flakiness(pipeline, &mut exec_counts, &mut retry_counts);
                (exec_counts, retry_counts)
            },
        );

    // Calculate flakiness scores from retry counts
    let flaky_data = compute_flakiness_scores(&flaky_retries, &execution_counts);

    (execution_counts, flaky_data)
}

fn process_pipeline_flakiness(
    pipeline: &GitLabPipeline,
    exec_counts: &mut HashMap<String, usize>,
    retry_counts: &mut HashMap<String, usize>,
) {
    // Group jobs by name and process each group
    let jobs_by_name = group_jobs_by_name(&pipeline.jobs);

    for (name, jobs) in jobs_by_name {
        // Count total executions
        *exec_counts.entry(name.to_string()).or_insert(0) += jobs.len();

        // Count retries if job is flaky
        if is_job_flaky(&jobs) {
            let retries = count_retries(&jobs);
            *retry_counts.entry(name.to_string()).or_insert(0) += retries;
        }
    }
}

fn count_retries(jobs: &[&GitLabJob]) -> usize {
    jobs.iter().filter(|j| j.retried).count()
}

fn compute_flakiness_scores(
    retry_counts: &HashMap<String, usize>,
    execution_counts: &HashMap<String, usize>,
) -> HashMap<String, FlakinessMetrics> {
    retry_counts
        .iter()
        .filter_map(|(name, &flaky_retries)| {
            create_flakiness_metric(name.clone(), flaky_retries, execution_counts)
        })
        .collect()
}

#[allow(clippy::cast_precision_loss)]
fn create_flakiness_metric(
    name: String,
    flaky_retries: usize,
    execution_counts: &HashMap<String, usize>,
) -> Option<(String, FlakinessMetrics)> {
    if flaky_retries == 0 {
        return None;
    }

    let total_executions = *execution_counts.get(&name)?;
    let score = (flaky_retries as f64 / total_executions as f64) * 100.0;

    Some((
        name,
        FlakinessMetrics {
            score,
            flaky_retries,
        },
    ))
}

fn group_jobs_by_name(jobs: &[GitLabJob]) -> HashMap<&str, Vec<&GitLabJob>> {
    jobs.iter().fold(HashMap::new(), |mut grouped, job| {
        grouped
            .entry(job.name.as_str())
            .or_insert_with(Vec::new)
            .push(job);
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
