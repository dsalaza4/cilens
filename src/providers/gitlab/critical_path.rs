use std::collections::HashMap;

use super::core::{GitLabJob, GitLabPipeline};
use crate::insights::{CriticalPath, CriticalPathJob};

pub fn calculate_critical_path(pipeline: &GitLabPipeline) -> Option<CriticalPath> {
    if pipeline.jobs.is_empty() {
        return None;
    }

    // Fast path: all jobs run in parallel (needs = Some([]))
    if all_jobs_parallel(&pipeline.jobs) {
        return parallel_jobs_path(pipeline);
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

    // Calculate when each job finishes and track the critical path
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

    // Find the job that finishes last
    let (&last_job, &total_time) = finish_times
        .iter()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())?;

    // Reconstruct the critical path
    let path = build_path(last_job, &predecessors);

    // Build detailed critical path structure
    build_critical_path(&path, &job_map, total_time)
}

fn all_jobs_parallel(jobs: &[GitLabJob]) -> bool {
    jobs.iter()
        .all(|j| matches!(j.needs, Some(ref v) if v.is_empty()))
}

fn parallel_jobs_path(pipeline: &GitLabPipeline) -> Option<CriticalPath> {
    let slowest = pipeline
        .jobs
        .iter()
        .max_by(|a, b| a.duration.partial_cmp(&b.duration).unwrap())?;

    let job = CriticalPathJob {
        name: slowest.name.clone(),
        avg_duration: slowest.duration,
        percentage_of_path: 100.0,
    };

    Some(CriticalPath {
        jobs: vec![job.clone()],
        total_duration: slowest.duration,
        bottleneck: Some(job),
    })
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

fn build_path(last_job: &str, predecessors: &HashMap<&str, &str>) -> Vec<String> {
    let mut path = vec![last_job.to_string()];
    let mut current = last_job;

    while let Some(&pred) = predecessors.get(current) {
        path.push(pred.to_string());
        current = pred;
    }

    path.reverse();
    path
}

fn build_critical_path(
    path: &[String],
    job_map: &HashMap<&str, &GitLabJob>,
    total_duration: f64,
) -> Option<CriticalPath> {
    let jobs: Vec<CriticalPathJob> = path
        .iter()
        .filter_map(|name| {
            let job = job_map.get(name.as_str())?;
            Some(create_job_detail(name, job.duration, total_duration))
        })
        .collect();

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
        total_duration,
        bottleneck,
    })
}

fn create_job_detail(name: &str, duration: f64, total_duration: f64) -> CriticalPathJob {
    #[allow(clippy::cast_precision_loss)]
    let percentage = if total_duration > 0.0 {
        (duration / total_duration) * 100.0
    } else {
        0.0
    };

    CriticalPathJob {
        name: name.to_string(),
        avg_duration: duration,
        percentage_of_path: percentage,
    }
}
