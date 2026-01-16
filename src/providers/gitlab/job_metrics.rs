use std::collections::HashMap;

use super::types::{GitLabJob, GitLabPipeline};
use crate::providers::utils::cmp_f64;
use crate::insights::{JobCountWithLinks, JobMetrics, PredecessorJob};

/// Calculates metrics for all jobs in a single pipeline.
///
/// Analyzes job dependencies (explicit via `needs` and implicit via stages) to compute
/// time-to-feedback for each job. Time-to-feedback represents when a job completes
/// relative to the pipeline start, accounting for dependencies that must complete first.
///
/// # Arguments
///
/// * `pipeline` - Pipeline containing jobs to analyze
///
/// # Returns
///
/// Vector of `JobMetrics` sorted by time-to-feedback (slowest first), with each job's
/// duration, time-to-feedback, and predecessor list. For a single pipeline, all percentiles
/// (P50/P95/P99) are identical since there's only one data point per job.
///
/// Reliability metrics (`flakiness_rate`, `failure_rate`, etc.) are set to zero/empty as they
/// require analysis across multiple pipeline executions.
pub fn calculate_job_metrics(pipeline: &GitLabPipeline) -> Vec<JobMetrics> {
    if pipeline.jobs.is_empty() {
        return vec![];
    }

    let job_map: HashMap<&str, &GitLabJob> =
        pipeline.jobs.iter().map(|j| (j.name.as_str(), j)).collect();

    let stage_index: HashMap<&str, usize> = pipeline
        .stages
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i))
        .collect();

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

    let mut metrics: Vec<JobMetrics> = job_map
        .iter()
        .filter(|(_, job)| job.status == "SUCCESS")
        .map(|(&name, job)| {
            // For a single pipeline, all percentiles are the same (only 1 value)
            let duration = job.duration;
            let time_to_feedback = *finish_times.get(name).unwrap_or(&0.0);
            let predecessor_list = build_predecessor_list(name, &predecessors, &job_map);

            JobMetrics {
                name: name.to_string(),
                pipeline_type_id: String::new(), // Temporary, will be set by aggregate_job_metrics
                duration_p50: duration,
                duration_p95: duration,
                duration_p99: duration,
                time_to_feedback_p50: time_to_feedback,
                time_to_feedback_p95: time_to_feedback,
                time_to_feedback_p99: time_to_feedback,
                predecessors: predecessor_list,
                flakiness_rate: 0.0,
                flaky_retries: JobCountWithLinks::default(),
                failed_executions: JobCountWithLinks::default(),
                failure_rate: 0.0,
                total_executions: 0,
            }
        })
        .collect();

    metrics.sort_by(|a, b| cmp_f64(b.time_to_feedback_p50, a.time_to_feedback_p50));

    metrics
}

fn build_predecessor_list(
    job_name: &str,
    predecessors: &HashMap<&str, &str>,
    job_map: &HashMap<&str, &GitLabJob>,
) -> Vec<PredecessorJob> {
    let mut result: Vec<PredecessorJob> = std::iter::successors(Some(job_name), |&current| {
        predecessors.get(current).copied()
    })
    .skip(1)
    .filter_map(|name| {
        job_map.get(name).map(|job| PredecessorJob {
            name: name.to_string(),
            duration_p50: job.duration,
        })
    })
    .collect();
    result.reverse();
    result
}

fn calculate_finish_time<'a>(
    job_name: &'a str,
    job_map: &HashMap<&'a str, &'a GitLabJob>,
    stage_index: &HashMap<&str, usize>,
    finish_times: &mut HashMap<&'a str, f64>,
    predecessors: &mut HashMap<&'a str, &'a str>,
) -> f64 {
    if let Some(&time) = finish_times.get(job_name) {
        return time;
    }

    let Some(job) = job_map.get(job_name) else {
        finish_times.insert(job_name, 0.0);
        return 0.0;
    };

    let deps = get_dependencies(job, job_map, stage_index);

    if deps.is_empty() {
        finish_times.insert(job_name, job.duration);
        return job.duration;
    }

    let (slowest_dep, slowest_time) = deps
        .iter()
        .map(|&dep| {
            let time = calculate_finish_time(dep, job_map, stage_index, finish_times, predecessors);
            (dep, time)
        })
        .max_by(|a, b| cmp_f64(a.1, b.1))
        .unwrap_or(("", 0.0));

    let finish_time = slowest_time + job.duration;
    finish_times.insert(job_name, finish_time);

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

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    // Helper function to create a test GitLabJob
    fn create_job(name: &str, stage: &str, duration: f64, needs: Option<Vec<String>>) -> GitLabJob {
        GitLabJob {
            id: name.to_string(),
            name: name.to_string(),
            stage: stage.to_string(),
            duration,
            status: "SUCCESS".to_string(),
            retried: false,
            needs,
        }
    }

    // Helper function to create a test GitLabPipeline
    fn create_pipeline(stages: Vec<String>, jobs: Vec<GitLabJob>) -> GitLabPipeline {
        GitLabPipeline {
            id: "test-pipeline".to_string(),
            ref_: "main".to_string(),
            source: "push".to_string(),
            status: "success".to_string(),
            duration: 100,
            stages,
            jobs,
        }
    }

    mod get_dependencies_tests {
        use super::*;

        #[test]
        fn test_needs_none_depends_on_all_previous_stages() {
            // Arrange: Create jobs in multiple stages
            let job1 = create_job("job1", "build", 10.0, None);
            let job2 = create_job("job2", "build", 15.0, None);
            let job3 = create_job("job3", "test", 20.0, None);

            let job_map: HashMap<&str, &GitLabJob> =
                [("job1", &job1), ("job2", &job2), ("job3", &job3)]
                    .into_iter()
                    .collect();

            let stage_index: HashMap<&str, usize> =
                [("build", 0), ("test", 1)].into_iter().collect();

            // Act: Get dependencies for job3 (in stage 1)
            let deps = get_dependencies(&job3, &job_map, &stage_index);

            // Assert: Should depend on all jobs in earlier stages (job1 and job2)
            assert_eq!(deps.len(), 2);
            assert!(deps.contains(&"job1"));
            assert!(deps.contains(&"job2"));
        }

        #[test]
        fn test_needs_none_first_stage_has_no_dependencies() {
            // Arrange: Create job in first stage
            let job1 = create_job("job1", "build", 10.0, None);

            let job_map: HashMap<&str, &GitLabJob> = [("job1", &job1)].into_iter().collect();

            let stage_index: HashMap<&str, usize> = [("build", 0)].into_iter().collect();

            // Act: Get dependencies for job1 (in stage 0)
            let deps = get_dependencies(&job1, &job_map, &stage_index);

            // Assert: Should have no dependencies
            assert_eq!(deps.len(), 0);
        }

        #[test]
        fn test_needs_empty_array_no_dependencies() {
            // Arrange: Create job with empty needs array
            let job1 = create_job("job1", "test", 10.0, Some(vec![]));

            let job_map: HashMap<&str, &GitLabJob> = [("job1", &job1)].into_iter().collect();

            let stage_index: HashMap<&str, usize> = [("test", 1)].into_iter().collect();

            // Act: Get dependencies
            let deps = get_dependencies(&job1, &job_map, &stage_index);

            // Assert: Should have no dependencies (starts immediately)
            assert_eq!(deps.len(), 0);
        }

        #[test]
        fn test_needs_explicit_dependencies() {
            // Arrange: Create job with explicit dependencies
            let job1 = create_job("job1", "build", 10.0, None);
            let job2 = create_job("job2", "build", 15.0, None);
            let job3 = create_job(
                "job3",
                "test",
                20.0,
                Some(vec!["job1".to_string(), "job2".to_string()]),
            );

            let job_map: HashMap<&str, &GitLabJob> =
                [("job1", &job1), ("job2", &job2), ("job3", &job3)]
                    .into_iter()
                    .collect();

            let stage_index: HashMap<&str, usize> =
                [("build", 0), ("test", 1)].into_iter().collect();

            // Act: Get dependencies
            let deps = get_dependencies(&job3, &job_map, &stage_index);

            // Assert: Should return explicit dependencies
            assert_eq!(deps.len(), 2);
            assert!(deps.contains(&"job1"));
            assert!(deps.contains(&"job2"));
        }

        #[test]
        fn test_needs_explicit_single_dependency() {
            // Arrange: Create job with single explicit dependency
            let job1 = create_job("job1", "build", 10.0, None);
            let job2 = create_job("job2", "test", 20.0, Some(vec!["job1".to_string()]));

            let job_map: HashMap<&str, &GitLabJob> =
                [("job1", &job1), ("job2", &job2)].into_iter().collect();

            let stage_index: HashMap<&str, usize> =
                [("build", 0), ("test", 1)].into_iter().collect();

            // Act: Get dependencies
            let deps = get_dependencies(&job2, &job_map, &stage_index);

            // Assert: Should return single dependency
            assert_eq!(deps.len(), 1);
            assert_eq!(deps[0], "job1");
        }

        #[test]
        fn test_needs_none_multiple_stages() {
            // Arrange: Create jobs across three stages
            let job1 = create_job("job1", "build", 10.0, None);
            let job2 = create_job("job2", "test", 15.0, None);
            let job3 = create_job("job3", "deploy", 20.0, None);

            let job_map: HashMap<&str, &GitLabJob> =
                [("job1", &job1), ("job2", &job2), ("job3", &job3)]
                    .into_iter()
                    .collect();

            let stage_index: HashMap<&str, usize> = [("build", 0), ("test", 1), ("deploy", 2)]
                .into_iter()
                .collect();

            // Act: Get dependencies for job3 (deploy stage)
            let deps = get_dependencies(&job3, &job_map, &stage_index);

            // Assert: Should depend on all jobs in earlier stages
            assert_eq!(deps.len(), 2);
            assert!(deps.contains(&"job1"));
            assert!(deps.contains(&"job2"));
        }
    }

    mod calculate_finish_time_tests {
        use super::*;

        #[test]
        fn test_job_no_dependencies_starts_at_zero() {
            // Arrange: Job with no dependencies
            let job1 = create_job("job1", "build", 10.0, Some(vec![]));
            let job_map: HashMap<&str, &GitLabJob> = [("job1", &job1)].into_iter().collect();
            let stage_index: HashMap<&str, usize> = [("build", 0)].into_iter().collect();
            let mut finish_times = HashMap::new();
            let mut predecessors = HashMap::new();

            // Act: Calculate finish time
            let time = calculate_finish_time(
                "job1",
                &job_map,
                &stage_index,
                &mut finish_times,
                &mut predecessors,
            );

            // Assert: Finish time should equal job duration (starts at 0)
            assert_eq!(time, 10.0);
            assert_eq!(finish_times.get("job1"), Some(&10.0));
            assert!(!predecessors.contains_key("job1"));
        }

        #[test]
        fn test_job_with_single_dependency() {
            // Arrange: Two jobs with dependency
            let job1 = create_job("job1", "build", 10.0, Some(vec![]));
            let job2 = create_job("job2", "test", 15.0, Some(vec!["job1".to_string()]));

            let job_map: HashMap<&str, &GitLabJob> =
                [("job1", &job1), ("job2", &job2)].into_iter().collect();

            let stage_index: HashMap<&str, usize> =
                [("build", 0), ("test", 1)].into_iter().collect();

            let mut finish_times = HashMap::new();
            let mut predecessors = HashMap::new();

            // Act: Calculate finish time for job2
            let time = calculate_finish_time(
                "job2",
                &job_map,
                &stage_index,
                &mut finish_times,
                &mut predecessors,
            );

            // Assert: Finish time should be job1_duration + job2_duration
            assert_eq!(time, 25.0); // 10.0 + 15.0
            assert_eq!(finish_times.get("job2"), Some(&25.0));
            assert_eq!(predecessors.get("job2"), Some(&"job1"));
        }

        #[test]
        fn test_job_with_multiple_dependencies_picks_slowest() {
            // Arrange: Three jobs - job3 depends on both job1 and job2
            let job1 = create_job("job1", "build", 10.0, Some(vec![]));
            let job2 = create_job("job2", "build", 30.0, Some(vec![])); // Slower
            let job3 = create_job(
                "job3",
                "test",
                5.0,
                Some(vec!["job1".to_string(), "job2".to_string()]),
            );

            let job_map: HashMap<&str, &GitLabJob> =
                [("job1", &job1), ("job2", &job2), ("job3", &job3)]
                    .into_iter()
                    .collect();

            let stage_index: HashMap<&str, usize> =
                [("build", 0), ("test", 1)].into_iter().collect();

            let mut finish_times = HashMap::new();
            let mut predecessors = HashMap::new();

            // Act: Calculate finish time for job3
            let time = calculate_finish_time(
                "job3",
                &job_map,
                &stage_index,
                &mut finish_times,
                &mut predecessors,
            );

            // Assert: Should wait for slowest dependency (job2 at 30.0) + job3 duration (5.0)
            assert_eq!(time, 35.0);
            assert_eq!(predecessors.get("job3"), Some(&"job2"));
        }

        #[test]
        fn test_memoization_same_job_calculated_multiple_times() {
            // Arrange: Create a diamond dependency pattern
            //   job1
            //   /  \
            // job2  job3
            //   \  /
            //   job4
            let job1 = create_job("job1", "build", 10.0, Some(vec![]));
            let job2 = create_job("job2", "test", 5.0, Some(vec!["job1".to_string()]));
            let job3 = create_job("job3", "test", 8.0, Some(vec!["job1".to_string()]));
            let job4 = create_job(
                "job4",
                "deploy",
                3.0,
                Some(vec!["job2".to_string(), "job3".to_string()]),
            );

            let job_map: HashMap<&str, &GitLabJob> = [
                ("job1", &job1),
                ("job2", &job2),
                ("job3", &job3),
                ("job4", &job4),
            ]
            .into_iter()
            .collect();

            let stage_index: HashMap<&str, usize> = [("build", 0), ("test", 1), ("deploy", 2)]
                .into_iter()
                .collect();

            let mut finish_times = HashMap::new();
            let mut predecessors = HashMap::new();

            // Act: Calculate finish time for job4 (which will calculate job1 twice)
            let time = calculate_finish_time(
                "job4",
                &job_map,
                &stage_index,
                &mut finish_times,
                &mut predecessors,
            );

            // Assert:
            // job1 finishes at 10.0
            // job2 finishes at 15.0 (10 + 5)
            // job3 finishes at 18.0 (10 + 8)
            // job4 waits for job3 (slower) and finishes at 21.0 (18 + 3)
            assert_eq!(time, 21.0);

            // Verify job1 was memoized (only calculated once)
            assert_eq!(finish_times.get("job1"), Some(&10.0));
            assert_eq!(finish_times.get("job2"), Some(&15.0));
            assert_eq!(finish_times.get("job3"), Some(&18.0));
            assert_eq!(finish_times.get("job4"), Some(&21.0));

            // Verify predecessor tracking
            assert_eq!(predecessors.get("job4"), Some(&"job3"));
        }

        #[test]
        fn test_nonexistent_job_returns_zero() {
            // Arrange: Empty job map
            let job_map: HashMap<&str, &GitLabJob> = HashMap::new();
            let stage_index: HashMap<&str, usize> = HashMap::new();
            let mut finish_times = HashMap::new();
            let mut predecessors = HashMap::new();

            // Act: Try to calculate finish time for nonexistent job
            let time = calculate_finish_time(
                "nonexistent",
                &job_map,
                &stage_index,
                &mut finish_times,
                &mut predecessors,
            );

            // Assert: Should return 0.0
            assert_eq!(time, 0.0);
            assert_eq!(finish_times.get("nonexistent"), Some(&0.0));
        }
    }

    mod build_predecessor_list_tests {
        use super::*;

        #[test]
        fn test_job_with_no_predecessors() {
            // Arrange: Job with no predecessors
            let job1 = create_job("job1", "build", 10.0, Some(vec![]));
            let job_map: HashMap<&str, &GitLabJob> = [("job1", &job1)].into_iter().collect();
            let predecessors: HashMap<&str, &str> = HashMap::new();

            // Act: Build predecessor list
            let pred_list = build_predecessor_list("job1", &predecessors, &job_map);

            // Assert: Should be empty
            assert_eq!(pred_list.len(), 0);
        }

        #[test]
        fn test_job_with_single_predecessor() {
            // Arrange: Two jobs with single predecessor relationship
            let job1 = create_job("job1", "build", 10.0, Some(vec![]));
            let job2 = create_job("job2", "test", 15.0, Some(vec!["job1".to_string()]));

            let job_map: HashMap<&str, &GitLabJob> =
                [("job1", &job1), ("job2", &job2)].into_iter().collect();

            let predecessors: HashMap<&str, &str> = [("job2", "job1")].into_iter().collect();

            // Act: Build predecessor list for job2
            let pred_list = build_predecessor_list("job2", &predecessors, &job_map);

            // Assert: Should contain job1
            assert_eq!(pred_list.len(), 1);
            assert_eq!(pred_list[0].name, "job1");
            assert_eq!(pred_list[0].duration_p50, 10.0);
        }

        #[test]
        fn test_job_with_chain_of_predecessors() {
            // Arrange: Chain of dependencies: job1 -> job2 -> job3
            let job1 = create_job("job1", "build", 10.0, Some(vec![]));
            let job2 = create_job("job2", "test", 15.0, Some(vec!["job1".to_string()]));
            let job3 = create_job("job3", "deploy", 20.0, Some(vec!["job2".to_string()]));

            let job_map: HashMap<&str, &GitLabJob> =
                [("job1", &job1), ("job2", &job2), ("job3", &job3)]
                    .into_iter()
                    .collect();

            let predecessors: HashMap<&str, &str> =
                [("job2", "job1"), ("job3", "job2")].into_iter().collect();

            // Act: Build predecessor list for job3
            let pred_list = build_predecessor_list("job3", &predecessors, &job_map);

            // Assert: Should contain job1 and job2 in order
            assert_eq!(pred_list.len(), 2);
            // List should be reversed (earliest first)
            assert_eq!(pred_list[0].name, "job1");
            assert_eq!(pred_list[0].duration_p50, 10.0);
            assert_eq!(pred_list[1].name, "job2");
            assert_eq!(pred_list[1].duration_p50, 15.0);
        }

        #[test]
        fn test_predecessor_list_ordering() {
            // Arrange: Longer chain to verify ordering: A -> B -> C -> D
            let job_a = create_job("job_a", "stage1", 5.0, Some(vec![]));
            let job_b = create_job("job_b", "stage2", 10.0, None);
            let job_c = create_job("job_c", "stage3", 15.0, None);
            let job_d = create_job("job_d", "stage4", 20.0, None);

            let job_map: HashMap<&str, &GitLabJob> = [
                ("job_a", &job_a),
                ("job_b", &job_b),
                ("job_c", &job_c),
                ("job_d", &job_d),
            ]
            .into_iter()
            .collect();

            let predecessors: HashMap<&str, &str> =
                [("job_b", "job_a"), ("job_c", "job_b"), ("job_d", "job_c")]
                    .into_iter()
                    .collect();

            // Act: Build predecessor list for job_d
            let pred_list = build_predecessor_list("job_d", &predecessors, &job_map);

            // Assert: Should contain all predecessors in chronological order
            assert_eq!(pred_list.len(), 3);
            assert_eq!(pred_list[0].name, "job_a");
            assert_eq!(pred_list[1].name, "job_b");
            assert_eq!(pred_list[2].name, "job_c");
        }
    }

    mod calculate_job_metrics_tests {
        use super::*;

        #[test]
        fn test_empty_pipeline() {
            // Arrange: Empty pipeline
            let pipeline = create_pipeline(vec![], vec![]);

            // Act: Calculate metrics
            let metrics = calculate_job_metrics(&pipeline);

            // Assert: Should return empty vec
            assert_eq!(metrics.len(), 0);
        }

        #[test]
        fn test_pipeline_with_single_job() {
            // Arrange: Pipeline with single job
            let job1 = create_job("job1", "build", 10.0, Some(vec![]));
            let pipeline = create_pipeline(vec!["build".to_string()], vec![job1]);

            // Act: Calculate metrics
            let metrics = calculate_job_metrics(&pipeline);

            // Assert: Should have one metric
            assert_eq!(metrics.len(), 1);
            assert_eq!(metrics[0].name, "job1");
            assert_eq!(metrics[0].duration_p50, 10.0);
            assert_eq!(metrics[0].time_to_feedback_p50, 10.0);
            assert_eq!(metrics[0].predecessors.len(), 0);
        }

        #[test]
        fn test_pipeline_with_linear_dependency_chain() {
            // Arrange: Linear chain: job1 -> job2 -> job3
            let job1 = create_job("job1", "build", 10.0, None);
            let job2 = create_job("job2", "test", 15.0, None);
            let job3 = create_job("job3", "deploy", 20.0, None);

            let pipeline = create_pipeline(
                vec![
                    "build".to_string(),
                    "test".to_string(),
                    "deploy".to_string(),
                ],
                vec![job1, job2, job3],
            );

            // Act: Calculate metrics
            let metrics = calculate_job_metrics(&pipeline);

            // Assert: Should have three metrics
            assert_eq!(metrics.len(), 3);

            // Find each job's metrics
            let job1_metrics = metrics.iter().find(|m| m.name == "job1").unwrap();
            let job2_metrics = metrics.iter().find(|m| m.name == "job2").unwrap();
            let job3_metrics = metrics.iter().find(|m| m.name == "job3").unwrap();

            // Verify time-to-feedback calculations
            assert_eq!(job1_metrics.time_to_feedback_p50, 10.0); // 0 + 10
            assert_eq!(job2_metrics.time_to_feedback_p50, 25.0); // 10 + 15
            assert_eq!(job3_metrics.time_to_feedback_p50, 45.0); // 25 + 20

            // Verify predecessor chains
            assert_eq!(job1_metrics.predecessors.len(), 0);
            assert_eq!(job2_metrics.predecessors.len(), 1);
            assert_eq!(job2_metrics.predecessors[0].name, "job1");
            assert_eq!(job3_metrics.predecessors.len(), 2);
            assert_eq!(job3_metrics.predecessors[0].name, "job1");
            assert_eq!(job3_metrics.predecessors[1].name, "job2");

            // Verify metrics are sorted by time_to_feedback (descending)
            assert_eq!(metrics[0].name, "job3");
            assert_eq!(metrics[1].name, "job2");
            assert_eq!(metrics[2].name, "job1");
        }

        #[test]
        fn test_pipeline_with_parallel_jobs() {
            // Arrange: Parallel jobs in same stage
            let job1 = create_job("job1", "test", 10.0, Some(vec![]));
            let job2 = create_job("job2", "test", 15.0, Some(vec![]));
            let job3 = create_job("job3", "test", 5.0, Some(vec![]));

            let pipeline = create_pipeline(vec!["test".to_string()], vec![job1, job2, job3]);

            // Act: Calculate metrics
            let metrics = calculate_job_metrics(&pipeline);

            // Assert: All jobs should start at time 0
            assert_eq!(metrics.len(), 3);

            for metric in &metrics {
                assert_eq!(metric.duration_p50, metric.time_to_feedback_p50);
                assert_eq!(metric.predecessors.len(), 0);
            }

            // Verify sorting by time_to_feedback (descending)
            assert_eq!(metrics[0].name, "job2"); // 15.0
            assert_eq!(metrics[1].name, "job1"); // 10.0
            assert_eq!(metrics[2].name, "job3"); // 5.0
        }

        #[test]
        fn test_pipeline_with_complex_dag() {
            // Arrange: Complex DAG structure
            //        job1 (10s)
            //        /        \
            //   job2 (5s)    job3 (8s)
            //        \        /
            //       job4 (needs: []) (3s, starts immediately)
            //            |
            //       job5 (needs: [job2, job3, job4]) (7s)

            let job1 = create_job("job1", "build", 10.0, Some(vec![]));
            let job2 = create_job("job2", "test", 5.0, Some(vec!["job1".to_string()]));
            let job3 = create_job("job3", "test", 8.0, Some(vec!["job1".to_string()]));
            let job4 = create_job("job4", "test", 3.0, Some(vec![])); // Parallel to job1
            let job5 = create_job(
                "job5",
                "deploy",
                7.0,
                Some(vec![
                    "job2".to_string(),
                    "job3".to_string(),
                    "job4".to_string(),
                ]),
            );

            let pipeline = create_pipeline(
                vec![
                    "build".to_string(),
                    "test".to_string(),
                    "deploy".to_string(),
                ],
                vec![job1, job2, job3, job4, job5],
            );

            // Act: Calculate metrics
            let metrics = calculate_job_metrics(&pipeline);

            // Assert
            assert_eq!(metrics.len(), 5);

            let job1_m = metrics.iter().find(|m| m.name == "job1").unwrap();
            let job2_m = metrics.iter().find(|m| m.name == "job2").unwrap();
            let job3_m = metrics.iter().find(|m| m.name == "job3").unwrap();
            let job4_m = metrics.iter().find(|m| m.name == "job4").unwrap();
            let job5_m = metrics.iter().find(|m| m.name == "job5").unwrap();

            // Verify time-to-feedback:
            // job1: 0 + 10 = 10
            assert_eq!(job1_m.time_to_feedback_p50, 10.0);
            // job2: 10 + 5 = 15
            assert_eq!(job2_m.time_to_feedback_p50, 15.0);
            // job3: 10 + 8 = 18
            assert_eq!(job3_m.time_to_feedback_p50, 18.0);
            // job4: 0 + 3 = 3 (starts immediately)
            assert_eq!(job4_m.time_to_feedback_p50, 3.0);
            // job5: max(15, 18, 3) + 7 = 18 + 7 = 25
            assert_eq!(job5_m.time_to_feedback_p50, 25.0);

            // Verify predecessor for job5 is job3 (slowest dependency)
            assert_eq!(job5_m.predecessors.len(), 2);
            assert_eq!(job5_m.predecessors[0].name, "job1");
            assert_eq!(job5_m.predecessors[1].name, "job3");

            // Verify sorting (job5 should be first with highest time_to_feedback)
            assert_eq!(metrics[0].name, "job5");
        }

        #[test]
        fn test_pipeline_with_needs_empty_bypasses_stages() {
            // Arrange: Job with needs=[] in later stage starts immediately
            let job1 = create_job("job1", "build", 10.0, None);
            let job2 = create_job("job2", "test", 5.0, None);
            let job3 = create_job("job3", "deploy", 3.0, Some(vec![])); // Bypasses all stages

            let pipeline = create_pipeline(
                vec![
                    "build".to_string(),
                    "test".to_string(),
                    "deploy".to_string(),
                ],
                vec![job1, job2, job3],
            );

            // Act: Calculate metrics
            let metrics = calculate_job_metrics(&pipeline);

            // Assert
            let job1_m = metrics.iter().find(|m| m.name == "job1").unwrap();
            let job2_m = metrics.iter().find(|m| m.name == "job2").unwrap();
            let job3_m = metrics.iter().find(|m| m.name == "job3").unwrap();

            // job1: 10 (starts at 0)
            assert_eq!(job1_m.time_to_feedback_p50, 10.0);
            // job2: 10 + 5 = 15 (waits for job1)
            assert_eq!(job2_m.time_to_feedback_p50, 15.0);
            // job3: 3 (starts at 0, bypasses all stages)
            assert_eq!(job3_m.time_to_feedback_p50, 3.0);
            assert_eq!(job3_m.predecessors.len(), 0);
        }

        #[test]
        fn test_percentiles_are_same_for_single_pipeline() {
            // Arrange: Single job
            let job1 = create_job("job1", "build", 10.0, Some(vec![]));
            let pipeline = create_pipeline(vec!["build".to_string()], vec![job1]);

            // Act: Calculate metrics
            let metrics = calculate_job_metrics(&pipeline);

            // Assert: All percentiles should be identical for single pipeline
            assert_eq!(metrics[0].duration_p50, 10.0);
            assert_eq!(metrics[0].duration_p95, 10.0);
            assert_eq!(metrics[0].duration_p99, 10.0);
            assert_eq!(metrics[0].time_to_feedback_p50, 10.0);
            assert_eq!(metrics[0].time_to_feedback_p95, 10.0);
            assert_eq!(metrics[0].time_to_feedback_p99, 10.0);
        }

        #[test]
        fn test_default_fields_are_set() {
            // Arrange: Simple pipeline
            let job1 = create_job("job1", "build", 10.0, Some(vec![]));
            let pipeline = create_pipeline(vec!["build".to_string()], vec![job1]);

            // Act: Calculate metrics
            let metrics = calculate_job_metrics(&pipeline);

            // Assert: Default fields should be set correctly
            assert_eq!(metrics[0].flakiness_rate, 0.0);
            assert_eq!(metrics[0].flaky_retries.count, 0);
            assert_eq!(metrics[0].flaky_retries.links.len(), 0);
            assert_eq!(metrics[0].failed_executions.count, 0);
            assert_eq!(metrics[0].failed_executions.links.len(), 0);
            assert_eq!(metrics[0].failure_rate, 0.0);
            assert_eq!(metrics[0].total_executions, 0);
        }
    }
}
