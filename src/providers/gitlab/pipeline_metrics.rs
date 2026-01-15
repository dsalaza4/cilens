use std::collections::HashMap;

use super::job_reliability::{calculate_job_reliability, JobReliabilityMetrics};
use super::links::pipeline_id_to_url;
use super::types::GitLabPipeline;
use crate::insights::{
    JobCountWithLinks, JobMetrics, PipelineCountWithLinks, PredecessorJob, TypeMetrics,
};
use crate::providers::utils::{calculate_percentiles, calculate_success_rate, cmp_f64};

/// Calculates comprehensive metrics for a pipeline type.
///
/// Analyzes a group of pipelines to compute success rates, duration percentiles,
/// time-to-feedback metrics, and per-job statistics. Only successful pipelines
/// are used for duration and time-to-feedback calculations.
///
/// # Arguments
///
/// * `pipelines` - Collection of pipelines in this type (all with the same job signature)
/// * `percentage` - Percentage of total pipelines this type represents (0-100)
/// * `base_url` - GitLab instance base URL for generating clickable pipeline/job URLs
/// * `project_path` - Project path for generating URLs
///
/// # Returns
///
/// `TypeMetrics` containing success rate, duration percentiles (P50/P95/P99),
/// time-to-feedback percentiles, and detailed per-job metrics with clickable URLs
/// to failed pipelines and flaky job runs.
pub fn calculate_type_metrics(
    pipeline_type_id: &str,
    pipelines: &[&GitLabPipeline],
    percentage: f64,
    base_url: &str,
    project_path: &str,
) -> TypeMetrics {
    let total_pipelines = pipelines.len();

    let (successful, failed): (Vec<_>, Vec<_>) =
        pipelines.iter().partition(|p| p.status == "success");

    let successful_pipelines = to_pipeline_links(&successful, base_url, project_path);
    let failed_pipelines = to_pipeline_links(&failed, base_url, project_path);

    // Calculate duration percentiles from successful pipelines
    #[allow(clippy::cast_precision_loss)]
    let durations: Vec<f64> = successful.iter().map(|p| p.duration as f64).collect();
    let (duration_p50, duration_p95, duration_p99) = calculate_percentiles(&durations);

    let (jobs, time_to_feedback_percentiles) = aggregate_job_metrics(
        pipeline_type_id,
        &successful,
        pipelines,
        base_url,
        project_path,
    );

    TypeMetrics {
        percentage,
        total_pipelines,
        successful_pipelines,
        failed_pipelines,
        success_rate: calculate_success_rate(successful.len(), total_pipelines),
        duration_p50,
        duration_p95,
        duration_p99,
        time_to_feedback_p50: time_to_feedback_percentiles.0,
        time_to_feedback_p95: time_to_feedback_percentiles.1,
        time_to_feedback_p99: time_to_feedback_percentiles.2,
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
fn aggregate_job_metrics(
    pipeline_type_id: &str,
    successful_pipelines: &[&GitLabPipeline],
    all_pipelines: &[&GitLabPipeline],
    base_url: &str,
    project_path: &str,
) -> (Vec<JobMetrics>, (f64, f64, f64)) {
    if successful_pipelines.is_empty() {
        return (vec![], (0.0, 0.0, 0.0));
    }

    // Calculate job metrics once per pipeline
    let per_pipeline_metrics: Vec<Vec<JobMetrics>> = successful_pipelines
        .iter()
        .map(|p| super::job_metrics::calculate_job_metrics(p))
        .collect();

    // Calculate pipeline-level time_to_feedback percentiles from per-pipeline data
    let first_feedback_times: Vec<f64> = per_pipeline_metrics
        .iter()
        .filter_map(|pipeline_metrics| {
            pipeline_metrics
                .iter()
                .map(|job| job.time_to_feedback_p50)
                .min_by(|a, b| cmp_f64(*a, *b))
        })
        .collect();

    let time_to_feedback_percentiles = calculate_percentiles(&first_feedback_times);

    // Aggregate job data across all pipelines
    let mut job_data: HashMap<String, JobData> = HashMap::new();
    for metrics in &per_pipeline_metrics {
        for job_metric in metrics {
            let data = job_data.entry(job_metric.name.clone()).or_default();
            data.durations.push(job_metric.duration_p50);
            data.time_to_feedbacks.push(job_metric.time_to_feedback_p50);
            data.all_predecessor_names.push(
                job_metric
                    .predecessors
                    .iter()
                    .map(|p| p.name.clone())
                    .collect(),
            );
        }
    }

    // Calculate percentiles for all jobs first (needed for predecessor lookups)
    let all_percentiles: HashMap<String, (f64, f64, f64)> = job_data
        .iter()
        .map(|(name, data)| (name.clone(), calculate_percentiles(&data.durations)))
        .collect();

    let reliability_data = calculate_job_reliability(all_pipelines, base_url, project_path);

    let mut jobs: Vec<JobMetrics> = job_data
        .into_iter()
        .map(|(name, data)| {
            build_job_metrics(
                pipeline_type_id,
                &name,
                &data,
                &all_percentiles,
                &reliability_data,
            )
        })
        .collect();

    jobs.sort_by(|a, b| cmp_f64(b.time_to_feedback_p95, a.time_to_feedback_p95));

    (jobs, time_to_feedback_percentiles)
}

#[derive(Default)]
struct JobData {
    durations: Vec<f64>,
    time_to_feedbacks: Vec<f64>,
    all_predecessor_names: Vec<Vec<String>>,
}

fn build_job_metrics(
    pipeline_type_id: &str,
    name: &str,
    data: &JobData,
    all_percentiles: &HashMap<String, (f64, f64, f64)>,
    reliability_data: &HashMap<String, JobReliabilityMetrics>,
) -> JobMetrics {
    let (duration_p50, duration_p95, duration_p99) = calculate_percentiles(&data.durations);
    let (time_to_feedback_p50, time_to_feedback_p95, time_to_feedback_p99) =
        calculate_percentiles(&data.time_to_feedbacks);

    let predecessors = aggregate_predecessors(&data.all_predecessor_names, all_percentiles);

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
            None => (
                0,
                0.0,
                JobCountWithLinks::default(),
                0.0,
                JobCountWithLinks::default(),
            ),
        };

    JobMetrics {
        name: name.to_string(),
        pipeline_type_id: pipeline_type_id.to_string(),
        duration_p50,
        duration_p95,
        duration_p99,
        time_to_feedback_p50,
        time_to_feedback_p95,
        time_to_feedback_p99,
        predecessors,
        flakiness_rate,
        flaky_retries,
        failed_executions,
        failure_rate,
        total_executions,
    }
}

fn aggregate_predecessors(
    all_predecessor_names: &[Vec<String>],
    all_percentiles: &HashMap<String, (f64, f64, f64)>,
) -> Vec<PredecessorJob> {
    let mut result: Vec<PredecessorJob> = all_predecessor_names
        .iter()
        .flat_map(|names| names.iter())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .filter_map(|name| {
            all_percentiles
                .get(name)
                .map(|(duration_p50, _, _)| PredecessorJob {
                    name: name.clone(),
                    duration_p50: *duration_p50,
                })
        })
        .collect();

    result.sort_by(|a, b| cmp_f64(b.duration_p50, a.duration_p50));
    result
}

#[cfg(test)]
#[allow(clippy::cast_lossless)]
mod tests {
    use super::*;

    #[allow(clippy::float_cmp)]
    mod calculate_percentiles {
        use super::*;

        #[test]
        fn returns_zeros_for_empty_dataset() {
            let values: Vec<f64> = vec![];
            let (p50, p95, p99) = calculate_percentiles(&values);
            assert_eq!(p50, 0.0);
            assert_eq!(p95, 0.0);
            assert_eq!(p99, 0.0);
        }

        #[test]
        fn returns_same_value_for_single_element() {
            let values = vec![42.5];
            let (p50, p95, p99) = calculate_percentiles(&values);
            assert_eq!(p50, 42.5);
            assert_eq!(p95, 42.5);
            assert_eq!(p99, 42.5);
        }

        #[test]
        fn handles_two_element_dataset() {
            let values = vec![10.0, 20.0];
            let (p50, p95, p99) = calculate_percentiles(&values);
            // With 2 elements: p50_idx=1, p95_idx=1, p99_idx=1
            assert_eq!(p50, 20.0);
            assert_eq!(p95, 20.0);
            assert_eq!(p99, 20.0);
        }

        #[test]
        fn calculates_percentiles_for_small_sorted_dataset() {
            let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
            let (p50, p95, p99) = calculate_percentiles(&values);
            // len=5: p50_idx=2 (3.0), p95_idx=4 (5.0), p99_idx=4 (5.0)
            assert_eq!(p50, 3.0);
            assert_eq!(p95, 5.0);
            assert_eq!(p99, 5.0);
        }

        #[test]
        fn calculates_percentiles_for_small_unsorted_dataset() {
            let values = vec![5.0, 2.0, 4.0, 1.0, 3.0];
            let (p50, p95, p99) = calculate_percentiles(&values);
            // Should sort to [1.0, 2.0, 3.0, 4.0, 5.0]
            // len=5: p50_idx=2 (3.0), p95_idx=4 (5.0), p99_idx=4 (5.0)
            assert_eq!(p50, 3.0);
            assert_eq!(p95, 5.0);
            assert_eq!(p99, 5.0);
        }

        #[test]
        fn calculates_percentiles_for_medium_dataset() {
            // 100 elements from 1.0 to 100.0
            let values: Vec<f64> = (1..=100).map(|i| i as f64).collect();
            let (p50, p95, p99) = calculate_percentiles(&values);
            // len=100: p50_idx=50, p95_idx=95, p99_idx=99
            assert_eq!(p50, 51.0); // Index 50 is value 51
            assert_eq!(p95, 96.0); // Index 95 is value 96
            assert_eq!(p99, 100.0); // Index 99 is value 100
        }

        #[test]
        fn calculates_percentiles_for_large_dataset() {
            // 1000 elements from 0.0 to 999.0
            let values: Vec<f64> = (0..1000).map(|i| i as f64).collect();
            let (p50, p95, p99) = calculate_percentiles(&values);
            // len=1000: p50_idx=500, p95_idx=950, p99_idx=990
            assert_eq!(p50, 500.0);
            assert_eq!(p95, 950.0);
            assert_eq!(p99, 990.0);
        }

        #[test]
        fn handles_dataset_with_duplicate_values() {
            let values = vec![5.0, 5.0, 5.0, 5.0, 5.0];
            let (p50, p95, p99) = calculate_percentiles(&values);
            assert_eq!(p50, 5.0);
            assert_eq!(p95, 5.0);
            assert_eq!(p99, 5.0);
        }

        #[test]
        fn handles_dataset_with_negative_values() {
            let values = vec![-10.0, -5.0, 0.0, 5.0, 10.0];
            let (p50, p95, p99) = calculate_percentiles(&values);
            // len=5: p50_idx=2 (0.0), p95_idx=4 (10.0), p99_idx=4 (10.0)
            assert_eq!(p50, 0.0);
            assert_eq!(p95, 10.0);
            assert_eq!(p99, 10.0);
        }

        #[test]
        fn handles_dataset_with_very_large_values() {
            let values = vec![1e10, 2e10, 3e10, 4e10, 5e10];
            let (p50, p95, p99) = calculate_percentiles(&values);
            assert_eq!(p50, 3e10);
            assert_eq!(p95, 5e10);
            assert_eq!(p99, 5e10);
        }

        #[test]
        fn handles_dataset_with_very_small_values() {
            let values = vec![1e-10, 2e-10, 3e-10, 4e-10, 5e-10];
            let (p50, p95, p99) = calculate_percentiles(&values);
            assert_eq!(p50, 3e-10);
            assert_eq!(p95, 5e-10);
            assert_eq!(p99, 5e-10);
        }

        #[test]
        fn handles_dataset_with_mixed_magnitude_values() {
            let values = vec![0.001, 1.0, 100.0, 10000.0, 1_000_000.0];
            let (p50, p95, p99) = calculate_percentiles(&values);
            assert_eq!(p50, 100.0);
            assert_eq!(p95, 1_000_000.0);
            assert_eq!(p99, 1_000_000.0);
        }

        #[test]
        fn calculates_correct_indices_for_boundary_sizes() {
            // Test with 10 elements to verify index calculation
            let values: Vec<f64> = (1..=10).map(|i| i as f64).collect();
            let (p50, p95, p99) = calculate_percentiles(&values);
            // len=10: p50_idx=5, p95_idx=9, p99_idx=9
            assert_eq!(p50, 6.0); // Index 5 is value 6
            assert_eq!(p95, 10.0); // Index 9 is value 10
            assert_eq!(p99, 10.0); // Index 9 is value 10
        }

        #[test]
        fn handles_dataset_with_three_elements() {
            let values = vec![1.0, 2.0, 3.0];
            let (p50, p95, p99) = calculate_percentiles(&values);
            // len=3: p50_idx=1 (2.0), p95_idx=2 (3.0), p99_idx=2 (3.0)
            assert_eq!(p50, 2.0);
            assert_eq!(p95, 3.0);
            assert_eq!(p99, 3.0);
        }

        #[test]
        fn preserves_precision_for_decimal_values() {
            let values = vec![1.123, 2.456, 3.789, 4.012, 5.345];
            let (p50, p95, p99) = calculate_percentiles(&values);
            assert_eq!(p50, 3.789);
            assert_eq!(p95, 5.345);
            assert_eq!(p99, 5.345);
        }

        #[test]
        fn handles_zero_values() {
            let values = vec![0.0, 0.0, 0.0, 0.0, 0.0];
            let (p50, p95, p99) = calculate_percentiles(&values);
            assert_eq!(p50, 0.0);
            assert_eq!(p95, 0.0);
            assert_eq!(p99, 0.0);
        }

        #[test]
        fn handles_dataset_with_infinity() {
            let values = vec![1.0, 2.0, f64::INFINITY];
            let (p50, p95, p99) = calculate_percentiles(&values);
            // len=3: p50_idx=1 (2.0), p95_idx=2 (infinity), p99_idx=2 (infinity)
            assert_eq!(p50, 2.0);
            assert_eq!(p95, f64::INFINITY);
            assert_eq!(p99, f64::INFINITY);
        }

        #[test]
        fn handles_dataset_with_negative_infinity() {
            let values = vec![f64::NEG_INFINITY, 1.0, 2.0];
            let (p50, p95, p99) = calculate_percentiles(&values);
            // After sorting: [NEG_INFINITY, 1.0, 2.0]
            // len=3: p50_idx=1 (1.0), p95_idx=2 (2.0), p99_idx=2 (2.0)
            assert_eq!(p50, 1.0);
            assert_eq!(p95, 2.0);
            assert_eq!(p99, 2.0);
        }
    }

    #[allow(clippy::float_cmp)]
    mod calculate_success_rate {
        use super::*;

        #[test]
        fn calculates_100_percent_when_all_successful() {
            let rate = calculate_success_rate(10, 10);
            assert_eq!(rate, 100.0);
        }

        #[test]
        fn calculates_0_percent_when_none_successful() {
            let rate = calculate_success_rate(0, 10);
            assert_eq!(rate, 0.0);
        }

        #[test]
        fn calculates_50_percent_for_half_successful() {
            let rate = calculate_success_rate(5, 10);
            assert_eq!(rate, 50.0);
        }

        #[test]
        fn calculates_25_percent_for_quarter_successful() {
            let rate = calculate_success_rate(1, 4);
            assert_eq!(rate, 25.0);
        }

        #[test]
        fn calculates_75_percent_for_three_quarters_successful() {
            let rate = calculate_success_rate(3, 4);
            assert_eq!(rate, 75.0);
        }

        #[test]
        fn handles_zero_total_without_panic() {
            // When total is 0, it should use max(1) to avoid division by zero
            let rate = calculate_success_rate(0, 0);
            assert_eq!(rate, 0.0);
        }

        #[test]
        fn handles_single_success_out_of_one() {
            let rate = calculate_success_rate(1, 1);
            assert_eq!(rate, 100.0);
        }

        #[test]
        fn handles_large_numbers() {
            let rate = calculate_success_rate(9999, 10000);
            assert_eq!(rate, 99.99);
        }

        #[test]
        fn calculates_fractional_percentages() {
            let rate = calculate_success_rate(1, 3);
            // 1/3 * 100 = 33.333...
            assert!((rate - 33.333_333_333_333_336).abs() < 1e-10);
        }

        #[test]
        fn calculates_small_success_rate() {
            let rate = calculate_success_rate(1, 100);
            assert_eq!(rate, 1.0);
        }

        #[test]
        fn calculates_high_success_rate() {
            let rate = calculate_success_rate(99, 100);
            assert_eq!(rate, 99.0);
        }

        #[test]
        fn handles_very_large_numbers() {
            let rate = calculate_success_rate(1_000_000, 1_000_000);
            assert_eq!(rate, 100.0);
        }

        #[test]
        fn calculates_precise_decimal_percentage() {
            let rate = calculate_success_rate(7, 10);
            assert_eq!(rate, 70.0);
        }

        #[test]
        fn handles_edge_case_successful_greater_than_total_should_not_occur() {
            // This shouldn't happen in practice, but testing defensive behavior
            // The function doesn't validate this, it will just calculate > 100%
            let rate = calculate_success_rate(15, 10);
            assert_eq!(rate, 150.0);
        }
    }

    #[allow(clippy::float_cmp)]
    mod cmp_f64 {
        use super::*;
        use std::cmp::Ordering;

        #[test]
        fn returns_less_when_first_is_smaller() {
            assert_eq!(cmp_f64(1.0, 2.0), Ordering::Less);
        }

        #[test]
        fn returns_greater_when_first_is_larger() {
            assert_eq!(cmp_f64(2.0, 1.0), Ordering::Greater);
        }

        #[test]
        fn returns_equal_when_values_are_equal() {
            assert_eq!(cmp_f64(1.0, 1.0), Ordering::Equal);
        }

        #[test]
        fn returns_equal_for_zero_values() {
            assert_eq!(cmp_f64(0.0, 0.0), Ordering::Equal);
        }

        #[test]
        fn handles_negative_values() {
            assert_eq!(cmp_f64(-2.0, -1.0), Ordering::Less);
            assert_eq!(cmp_f64(-1.0, -2.0), Ordering::Greater);
        }

        #[test]
        fn handles_negative_and_positive() {
            assert_eq!(cmp_f64(-1.0, 1.0), Ordering::Less);
            assert_eq!(cmp_f64(1.0, -1.0), Ordering::Greater);
        }

        #[test]
        fn handles_very_small_differences() {
            let a = 1.0;
            let b = 1.0 + f64::EPSILON;
            assert_eq!(cmp_f64(a, b), Ordering::Less);
        }

        #[test]
        fn handles_infinity() {
            assert_eq!(cmp_f64(1.0, f64::INFINITY), Ordering::Less);
            assert_eq!(cmp_f64(f64::INFINITY, 1.0), Ordering::Greater);
        }

        #[test]
        fn handles_negative_infinity() {
            assert_eq!(cmp_f64(f64::NEG_INFINITY, 1.0), Ordering::Less);
            assert_eq!(cmp_f64(1.0, f64::NEG_INFINITY), Ordering::Greater);
        }

        #[test]
        fn handles_both_infinity() {
            assert_eq!(cmp_f64(f64::INFINITY, f64::INFINITY), Ordering::Equal);
            assert_eq!(
                cmp_f64(f64::NEG_INFINITY, f64::NEG_INFINITY),
                Ordering::Equal
            );
        }

        #[test]
        fn handles_negative_and_positive_infinity() {
            assert_eq!(cmp_f64(f64::NEG_INFINITY, f64::INFINITY), Ordering::Less);
            assert_eq!(cmp_f64(f64::INFINITY, f64::NEG_INFINITY), Ordering::Greater);
        }

        #[test]
        fn handles_nan_with_normal_value() {
            // NaN comparisons return None, so our function returns Equal
            assert_eq!(cmp_f64(f64::NAN, 1.0), Ordering::Equal);
            assert_eq!(cmp_f64(1.0, f64::NAN), Ordering::Equal);
        }

        #[test]
        fn handles_both_nan() {
            // NaN == NaN is false, partial_cmp returns None
            assert_eq!(cmp_f64(f64::NAN, f64::NAN), Ordering::Equal);
        }

        #[test]
        fn handles_nan_with_infinity() {
            assert_eq!(cmp_f64(f64::NAN, f64::INFINITY), Ordering::Equal);
            assert_eq!(cmp_f64(f64::INFINITY, f64::NAN), Ordering::Equal);
        }

        #[test]
        fn handles_very_large_values() {
            assert_eq!(cmp_f64(f64::MAX, f64::MAX), Ordering::Equal);
            assert_eq!(cmp_f64(f64::MIN, f64::MAX), Ordering::Less);
        }

        #[test]
        fn handles_very_small_positive_values() {
            assert_eq!(
                cmp_f64(f64::MIN_POSITIVE, f64::MIN_POSITIVE),
                Ordering::Equal
            );
            assert_eq!(cmp_f64(0.0, f64::MIN_POSITIVE), Ordering::Less);
        }

        #[test]
        fn handles_positive_and_negative_zero() {
            // In IEEE 754, +0.0 and -0.0 compare as equal
            assert_eq!(cmp_f64(0.0, -0.0), Ordering::Equal);
            assert_eq!(cmp_f64(-0.0, 0.0), Ordering::Equal);
        }

        #[test]
        fn handles_decimal_precision() {
            assert_eq!(cmp_f64(1.123_456_789, 1.123_456_789), Ordering::Equal);
            assert_eq!(cmp_f64(1.123_456_788, 1.123_456_789), Ordering::Less);
            assert_eq!(cmp_f64(1.123_456_790, 1.123_456_789), Ordering::Greater);
        }
    }
}
