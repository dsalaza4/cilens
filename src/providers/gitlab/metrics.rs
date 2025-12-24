use indexmap::IndexMap;
use std::collections::HashMap;

use super::core::{GitLabJob, GitLabPipeline};
use crate::insights::{CriticalPath, CriticalPathJob, FlakyJobMetrics, TypeMetrics};

pub fn calculate_type_metrics(pipelines: &[&GitLabPipeline]) -> TypeMetrics {
    let total_pipelines = pipelines.len();
    let successful: Vec<_> = pipelines
        .iter()
        .filter(|p| p.status == "success")
        .copied()
        .collect();

    let failed = pipelines.iter().filter(|p| p.status == "failed").count();

    TypeMetrics {
        total_pipelines,
        successful_pipelines: successful.len(),
        failed_pipelines: failed,
        success_rate: calculate_success_rate(successful.len(), total_pipelines),
        average_duration_seconds: calculate_avg_duration(&successful),
        critical_path: aggregate_critical_paths(&successful),
        flaky_jobs: find_flaky_jobs(pipelines),
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

fn aggregate_critical_paths(pipelines: &[&GitLabPipeline]) -> Option<CriticalPath> {
    // Calculate critical path for each pipeline
    let paths: Vec<_> = pipelines
        .iter()
        .filter_map(|p| super::critical_path::calculate_critical_path(p))
        .collect();

    if paths.is_empty() {
        return None;
    }

    // Average total duration across all paths
    let avg_duration = average_duration(&paths);

    // Collect all job durations by name
    let job_durations = collect_job_durations(&paths);

    // Use first path's job order as canonical
    let canonical_order = &paths[0].jobs;

    // Build aggregated jobs in canonical order
    let jobs = build_aggregated_jobs(canonical_order, &job_durations, avg_duration);

    // Find the slowest job
    let bottleneck = jobs
        .iter()
        .max_by(|a, b| {
            a.avg_duration
                .partial_cmp(&b.avg_duration)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .cloned();

    Some(CriticalPath {
        jobs,
        total_duration: avg_duration,
        bottleneck,
    })
}

fn average_duration(paths: &[CriticalPath]) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let avg = paths.iter().map(|cp| cp.total_duration).sum::<f64>() / paths.len() as f64;
    avg
}

fn collect_job_durations(paths: &[CriticalPath]) -> HashMap<String, Vec<f64>> {
    let mut durations: HashMap<String, Vec<f64>> = HashMap::new();

    for path in paths {
        for job in &path.jobs {
            durations
                .entry(job.name.clone())
                .or_default()
                .push(job.avg_duration);
        }
    }

    durations
}

fn build_aggregated_jobs(
    canonical_order: &[CriticalPathJob],
    job_durations: &HashMap<String, Vec<f64>>,
    total_duration: f64,
) -> Vec<CriticalPathJob> {
    canonical_order
        .iter()
        .filter_map(|canonical_job| {
            let durations = job_durations.get(&canonical_job.name)?;

            #[allow(clippy::cast_precision_loss)]
            let avg_duration = durations.iter().sum::<f64>() / durations.len() as f64;

            #[allow(clippy::cast_precision_loss)]
            let percentage = if total_duration > 0.0 {
                (avg_duration / total_duration) * 100.0
            } else {
                0.0
            };

            Some(CriticalPathJob {
                name: canonical_job.name.clone(),
                avg_duration,
                percentage_of_path: percentage,
            })
        })
        .collect()
}

fn find_flaky_jobs(pipelines: &[&GitLabPipeline]) -> IndexMap<String, FlakyJobMetrics> {
    let (flaky_counts, total_counts) = count_flaky_jobs(pipelines);

    // Calculate flakiness scores
    let mut scored: Vec<_> = flaky_counts
        .into_iter()
        .filter_map(|(name, flaky_count)| {
            let total = *total_counts.get(&name)?;

            // Ignore jobs that appear only once (no retry data)
            if total < 2 {
                return None;
            }

            #[allow(clippy::cast_precision_loss)]
            let score = (flaky_count as f64 / total as f64) * 100.0;

            Some((
                name,
                FlakyJobMetrics {
                    total_occurrences: total,
                    retry_count: flaky_count,
                    flakiness_score: score,
                },
            ))
        })
        .collect();

    // Sort by flakiness score (highest first)
    scored.sort_by(|a, b| {
        b.1.flakiness_score
            .partial_cmp(&a.1.flakiness_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Return top 5 flakiest jobs
    scored.into_iter().take(5).collect()
}

fn count_flaky_jobs(
    pipelines: &[&GitLabPipeline],
) -> (HashMap<String, usize>, HashMap<String, usize>) {
    let mut flaky_counts = HashMap::new();
    let mut total_counts = HashMap::new();

    for pipeline in pipelines {
        // Group jobs by name (a job may appear multiple times if retried)
        let jobs_by_name = group_jobs_by_name(&pipeline.jobs);

        for (name, jobs) in jobs_by_name {
            *total_counts.entry(name.to_string()).or_insert(0) += 1;

            if is_flaky(&jobs) {
                *flaky_counts.entry(name.to_string()).or_insert(0) += 1;
            }
        }
    }

    (flaky_counts, total_counts)
}

fn group_jobs_by_name(jobs: &[GitLabJob]) -> HashMap<&str, Vec<&GitLabJob>> {
    let mut grouped = HashMap::new();

    for job in jobs {
        grouped.entry(job.name.as_str()).or_insert_with(Vec::new).push(job);
    }

    grouped
}

fn is_flaky(jobs: &[&GitLabJob]) -> bool {
    // Flaky = job was retried AND eventually succeeded
    let was_retried = jobs.iter().any(|j| j.retried);
    let final_succeeded = jobs
        .iter()
        .find(|j| !j.retried)
        .is_some_and(|j| j.status == "SUCCESS");

    was_retried && final_succeeded
}
