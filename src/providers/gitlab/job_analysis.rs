use std::collections::HashMap;

use super::types::{GitLabJob, GitLabPipeline};
use crate::insights::{JobMetrics, PredecessorJob};

pub fn calculate_job_metrics(pipeline: &GitLabPipeline) -> Vec<JobMetrics> {
    if pipeline.jobs.is_empty() {
        return vec![];
    }

    // Build job lookup map
    let job_map: HashMap<&str, &GitLabJob> =
        pipeline.jobs.iter().map(|j| (j.name.as_str(), j)).collect();

    // Build stage index for dependency resolution
    let stage_index: HashMap<&str, usize> = pipeline
        .stages
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i))
        .collect();

    // Calculate when each job finishes and track the critical predecessor
    let mut finish_times = HashMap::new();
    let mut predecessors = HashMap::new();

    for &job_name in job_map.keys() {
        calculate_finish_time(
            job_name,
            &job_map,
            &stage_index,
            &mut finish_times,
            &mut predecessors,
        );
    }

    // Build metrics for all jobs
    let mut metrics: Vec<JobMetrics> = job_map
        .iter()
        .map(|(&name, job)| {
            let avg_duration_seconds = job.duration;
            let avg_time_to_feedback_seconds = *finish_times.get(name).unwrap_or(&0.0);
            let predecessor_list = build_predecessor_list(name, &predecessors, &job_map);

            JobMetrics {
                name: name.to_string(),
                avg_duration_seconds,
                avg_time_to_feedback_seconds,
                predecessors: predecessor_list,
                flakiness_score: 0.0,
                flaky_retries: 0,
                total_executions: 0,
            }
        })
        .collect();

    // Sort by time to feedback descending (longest time-to-feedback first)
    metrics.sort_by(|a, b| {
        b.avg_time_to_feedback_seconds
            .partial_cmp(&a.avg_time_to_feedback_seconds)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    metrics
}

fn build_predecessor_list(
    job_name: &str,
    predecessors: &HashMap<&str, &str>,
    job_map: &HashMap<&str, &GitLabJob>,
) -> Vec<PredecessorJob> {
    std::iter::successors(Some(job_name), |&current| predecessors.get(current).copied())
        .skip(1)
        .filter_map(|name| {
            job_map.get(name).map(|job| PredecessorJob {
                name: name.to_string(),
                avg_duration_seconds: job.duration,
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn calculate_finish_time<'a>(
    job_name: &'a str,
    job_map: &HashMap<&'a str, &'a GitLabJob>,
    stage_index: &HashMap<&str, usize>,
    finish_times: &mut HashMap<&'a str, f64>,
    predecessors: &mut HashMap<&'a str, &'a str>,
) -> f64 {
    // Return cached result
    if let Some(&time) = finish_times.get(job_name) {
        return time;
    }

    // Missing job (referenced in needs but doesn't exist)
    let Some(job) = job_map.get(job_name) else {
        finish_times.insert(job_name, 0.0);
        return 0.0;
    };

    let deps = get_dependencies(job, job_map, stage_index);

    // No dependencies - starts immediately
    if deps.is_empty() {
        finish_times.insert(job_name, job.duration);
        return job.duration;
    }

    // Find slowest dependency (the one this job must wait for)
    let (slowest_dep, slowest_time) = deps
        .iter()
        .map(|&dep| {
            let time = calculate_finish_time(dep, job_map, stage_index, finish_times, predecessors);
            (dep, time)
        })
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .unwrap_or(("", 0.0));

    // This job finishes when slowest dependency finishes + this job's duration
    let finish_time = slowest_time + job.duration;
    finish_times.insert(job_name, finish_time);

    // Track the critical predecessor for path reconstruction
    if slowest_time > 0.0 {
        predecessors.insert(job_name, slowest_dep);
    }

    finish_time
}

fn get_dependencies<'a>(
    job: &'a GitLabJob,
    job_map: &HashMap<&'a str, &'a GitLabJob>,
    stage_index: &HashMap<&str, usize>,
) -> Vec<&'a str> {
    match &job.needs {
        // needs = Some([]) -> no dependencies, starts immediately
        Some(needs) if needs.is_empty() => vec![],
        // needs = Some([...]) -> explicit dependencies
        Some(needs) => needs.iter().map(String::as_str).collect(),
        // needs = None -> depends on all jobs in previous stages
        None => {
            let current_stage = stage_index.get(job.stage.as_str()).copied().unwrap_or(0);
            job_map
                .iter()
                .filter_map(|(&name, other)| {
                    let other_stage = stage_index.get(other.stage.as_str()).copied().unwrap_or(0);
                    (other_stage < current_stage).then_some(name)
                })
                .collect()
        }
    }
}
