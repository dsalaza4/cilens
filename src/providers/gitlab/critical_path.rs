use std::collections::HashMap;

use super::core::{GitLabJob, GitLabPipeline};
use crate::insights::CriticalPath;

pub fn calculate_critical_path(pipeline: &GitLabPipeline) -> Option<CriticalPath> {
    if pipeline.jobs.is_empty() {
        return None;
    }

    // Fast path: if all jobs start immediately (needs = Some([])), return slowest job
    if pipeline
        .jobs
        .iter()
        .all(|j| matches!(j.needs, Some(ref v) if v.is_empty()))
    {
        return slowest_job_path(pipeline);
    }

    // Build job map
    let job_map: HashMap<&str, &GitLabJob> =
        pipeline.jobs.iter().map(|j| (j.name.as_str(), j)).collect();

    let (finish_times, predecessors) = calculate_finish_times(&job_map, &pipeline.stages);

    // Find and reconstruct critical path
    let (&critical_job, &total_time) = finish_times
        .iter()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())?;

    let path = reconstruct_path(critical_job, &predecessors);

    Some(CriticalPath {
        jobs: path,
        average_duration_seconds: total_time,
    })
}

fn slowest_job_path(pipeline: &GitLabPipeline) -> Option<CriticalPath> {
    let slowest_job = pipeline
        .jobs
        .iter()
        .max_by(|a, b| a.duration.partial_cmp(&b.duration).unwrap())?;

    Some(CriticalPath {
        jobs: vec![slowest_job.name.clone()],
        average_duration_seconds: slowest_job.duration,
    })
}

fn calculate_finish_times<'a>(
    job_map: &HashMap<&'a str, &'a GitLabJob>,
    stage_order: &[String],
) -> (HashMap<&'a str, f64>, HashMap<&'a str, &'a str>) {
    let mut finish_times: HashMap<&str, f64> = HashMap::new();
    let mut predecessors: HashMap<&str, &str> = HashMap::new();

    // Build stage index map for quick lookup
    let stage_index: HashMap<&str, usize> = stage_order
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i))
        .collect();

    // Calculate finish times for all jobs using memoization
    for &job_name in job_map.keys() {
        calculate_job_finish_time(
            job_name,
            job_map,
            &stage_index,
            &mut finish_times,
            &mut predecessors,
        );
    }

    (finish_times, predecessors)
}

fn get_dependency_names<'a>(
    job: &'a GitLabJob,
    job_map: &HashMap<&'a str, &'a GitLabJob>,
    stage_index: &HashMap<&str, usize>,
) -> Vec<&'a str> {
    match &job.needs {
        Some(needs) if needs.is_empty() => vec![],
        Some(needs) => needs.iter().map(String::as_str).collect(),
        None => {
            let current_stage_idx = stage_index.get(job.stage.as_str()).copied().unwrap_or(0);
            job_map
                .iter()
                .filter(|(_, other_job)| {
                    let other_stage_idx = stage_index
                        .get(other_job.stage.as_str())
                        .copied()
                        .unwrap_or(0);
                    other_stage_idx < current_stage_idx
                })
                .map(|(&name, _)| name)
                .collect()
        }
    }
}

fn find_critical_predecessor<'a>(
    dependencies: &[&'a str],
    job_map: &HashMap<&'a str, &'a GitLabJob>,
    stage_index: &HashMap<&str, usize>,
    finish_times: &mut HashMap<&'a str, f64>,
    predecessors: &mut HashMap<&'a str, &'a str>,
) -> (&'a str, f64) {
    dependencies
        .iter()
        .map(|&dep_name| {
            let time = calculate_job_finish_time(
                dep_name,
                job_map,
                stage_index,
                finish_times,
                predecessors,
            );
            (dep_name, time)
        })
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .unwrap_or(("", 0.0))
}

fn calculate_job_finish_time<'a>(
    job_name: &'a str,
    job_map: &HashMap<&'a str, &'a GitLabJob>,
    stage_index: &HashMap<&str, usize>,
    finish_times: &mut HashMap<&'a str, f64>,
    predecessors: &mut HashMap<&'a str, &'a str>,
) -> f64 {
    // Return memoized result if already calculated
    if let Some(&time) = finish_times.get(job_name) {
        return time;
    }

    // Job not in map - treat as duration 0
    let Some(job) = job_map.get(job_name) else {
        finish_times.insert(job_name, 0.0);
        return 0.0;
    };

    // Get dependencies based on needs field
    let dependencies = get_dependency_names(job, job_map, stage_index);

    // No dependencies - job starts immediately
    if dependencies.is_empty() {
        finish_times.insert(job_name, job.duration);
        return job.duration;
    }

    // Find the critical (slowest) predecessor
    let (critical_pred, max_pred_time) = find_critical_predecessor(
        &dependencies,
        job_map,
        stage_index,
        finish_times,
        predecessors,
    );

    let finish_time = max_pred_time + job.duration;
    finish_times.insert(job_name, finish_time);

    if max_pred_time > 0.0 {
        predecessors.insert(job_name, critical_pred);
    }

    finish_time
}

fn reconstruct_path(critical_job: &str, predecessors: &HashMap<&str, &str>) -> Vec<String> {
    let mut path = vec![critical_job.to_string()];
    let mut current = critical_job;

    while let Some(&pred) = predecessors.get(current) {
        path.push(pred.to_string());
        current = pred;
    }

    path.reverse();
    path
}
