use indexmap::IndexMap;
use std::collections::HashMap;

use super::core::{GitLabJob, GitLabPipeline};
use crate::insights::{CriticalPath, FlakyJobMetrics, TypeMetrics};

pub fn calculate_type_metrics(pipelines: &[&GitLabPipeline]) -> TypeMetrics {
    let total_pipelines = pipelines.len();

    let successful_pipelines: Vec<_> = pipelines
        .iter()
        .filter(|p| p.status == "success")
        .copied()
        .collect();

    let failed_pipelines = pipelines.iter().filter(|p| p.status == "failed").count();

    #[allow(clippy::cast_precision_loss)]
    let success_rate = (successful_pipelines.len() as f64 / total_pipelines.max(1) as f64) * 100.0;

    #[allow(clippy::cast_precision_loss)]
    let average_duration_seconds = if successful_pipelines.is_empty() {
        0.0
    } else {
        successful_pipelines
            .iter()
            .map(|p| p.duration as f64)
            .sum::<f64>()
            / successful_pipelines.len() as f64
    };

    let critical_path = aggregate_critical_paths(&successful_pipelines);
    let flaky_jobs = find_flaky_jobs(pipelines);

    TypeMetrics {
        total_pipelines,
        successful_pipelines: successful_pipelines.len(),
        failed_pipelines,
        success_rate,
        average_duration_seconds,
        critical_path,
        flaky_jobs,
    }
}

fn aggregate_critical_paths(pipelines: &[&GitLabPipeline]) -> Option<CriticalPath> {
    let critical_paths: Vec<_> = pipelines
        .iter()
        .filter_map(|p| super::critical_path::calculate_critical_path(p))
        .collect();

    if critical_paths.is_empty() {
        return None;
    }

    #[allow(clippy::cast_precision_loss)]
    let average_duration = critical_paths
        .iter()
        .map(|cp| cp.average_duration_seconds)
        .sum::<f64>()
        / critical_paths.len() as f64;

    Some(CriticalPath {
        jobs: critical_paths[0].jobs.clone(),
        average_duration_seconds: average_duration,
    })
}

fn find_flaky_jobs(pipelines: &[&GitLabPipeline]) -> IndexMap<String, FlakyJobMetrics> {
    let mut flaky_counts: HashMap<String, usize> = HashMap::new();
    let mut total_counts: HashMap<String, usize> = HashMap::new();

    // Analyze each pipeline for flaky jobs
    for pipeline in pipelines {
        let mut jobs_by_name: HashMap<&str, Vec<&GitLabJob>> = HashMap::new();
        for job in &pipeline.jobs {
            jobs_by_name.entry(job.name.as_str()).or_default().push(job);
        }

        for (name, jobs) in jobs_by_name {
            *total_counts.entry(name.to_string()).or_insert(0) += 1;
            if is_flaky(&jobs) {
                *flaky_counts.entry(name.to_string()).or_insert(0) += 1;
            }
        }
    }

    // Calculate scores and return top 5
    let mut results: Vec<(String, FlakyJobMetrics)> = flaky_counts
        .into_iter()
        .filter_map(|(name, flaky_count)| {
            let total = *total_counts.get(&name)?;
            if total < 2 {
                return None; // Filter noise
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

    results.sort_by(|a, b| {
        b.1.flakiness_score
            .partial_cmp(&a.1.flakiness_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.into_iter().take(5).collect()
}

fn is_flaky(jobs: &[&GitLabJob]) -> bool {
    // Flaky = failed initially but succeeded after retry
    let was_retried = jobs.iter().any(|j| j.retried);
    let final_succeeded = jobs
        .iter()
        .find(|j| !j.retried)
        .is_some_and(|j| j.status == "SUCCESS");

    was_retried && final_succeeded
}
