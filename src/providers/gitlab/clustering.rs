use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};

use super::core::GitLabPipeline;
use crate::insights::{PipelineType, TypeMetrics};

pub fn cluster_and_analyze(pipelines: &[GitLabPipeline]) -> Vec<PipelineType> {
    let similarity_threshold = 0.8;
    let mut clusters: HashMap<Vec<String>, Vec<&GitLabPipeline>> = HashMap::new();

    // Group pipelines by their job signature
    for pipeline in pipelines {
        let mut job_names: Vec<String> = pipeline.jobs.iter().map(|j| j.name.clone()).collect();
        job_names.sort();
        job_names.dedup();

        clusters.entry(job_names).or_default().push(pipeline);
    }

    let total_pipelines = pipelines.len();
    let mut pipeline_types: Vec<PipelineType> = clusters
        .into_iter()
        .map(|(job_names, cluster_pipelines)| {
            create_pipeline_type(&job_names, &cluster_pipelines, total_pipelines)
        })
        .collect();

    // Merge similar pipeline types
    pipeline_types = merge_similar_types(pipeline_types, total_pipelines, similarity_threshold);

    pipeline_types.sort_by(|a, b| b.count.cmp(&a.count));
    pipeline_types
}

fn jaccard_similarity(jobs1: &[String], jobs2: &[String]) -> f64 {
    let set1: HashSet<_> = jobs1.iter().collect();
    let set2: HashSet<_> = jobs2.iter().collect();

    let intersection = set1.intersection(&set2).count();
    let union = set1.union(&set2).count();

    if union == 0 {
        0.0
    } else {
        #[allow(clippy::cast_precision_loss)]
        {
            intersection as f64 / union as f64
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
fn merge_similar_types(
    pipeline_types: Vec<PipelineType>,
    total_pipelines: usize,
    similarity_threshold: f64,
) -> Vec<PipelineType> {
    let mut merged: Vec<PipelineType> = Vec::new();
    let mut used = vec![false; pipeline_types.len()];

    for i in 0..pipeline_types.len() {
        if used[i] {
            continue;
        }

        // Find all types similar to this one
        let mut similar_indices = vec![i];
        for j in (i + 1)..pipeline_types.len() {
            if used[j] {
                continue;
            }

            let similarity = jaccard_similarity(&pipeline_types[i].jobs, &pipeline_types[j].jobs);

            if similarity >= similarity_threshold {
                similar_indices.push(j);
                used[j] = true;
            }
        }

        used[i] = true;

        // If only one type, clone it
        if similar_indices.len() == 1 {
            merged.push(pipeline_types[i].clone());
            continue;
        }

        // Merge multiple similar types
        let types_to_merge: Vec<_> = similar_indices
            .iter()
            .map(|&idx| &pipeline_types[idx])
            .collect();

        merged.push(merge_pipeline_types(&types_to_merge, total_pipelines));
    }

    merged
}

fn merge_pipeline_types(types: &[&PipelineType], total_pipelines: usize) -> PipelineType {
    // Collect all unique jobs (union)
    let mut all_jobs: HashSet<String> = HashSet::new();
    for pt in types {
        all_jobs.extend(pt.jobs.iter().cloned());
    }
    let mut jobs: Vec<String> = all_jobs.into_iter().collect();
    jobs.sort();

    // Collect all pipeline IDs
    let mut all_ids = Vec::new();
    for pt in types {
        all_ids.extend(pt.ids.clone());
    }

    // Use the label from the largest type
    let label = types
        .iter()
        .max_by_key(|pt| pt.count)
        .map(|pt| pt.label.clone())
        .unwrap_or_default();

    // Combine metrics - use the label and jobs to look up pipelines
    // For now, just sum up counts and recalculate percentage
    let count = all_ids.len();

    #[allow(clippy::cast_precision_loss)]
    let percentage = (count as f64 / total_pipelines.max(1) as f64) * 100.0;

    // Merge other fields
    let mut all_stages: HashSet<String> = HashSet::new();
    let mut all_ref_patterns: HashSet<String> = HashSet::new();
    let mut all_sources: HashSet<String> = HashSet::new();

    for pt in types {
        all_stages.extend(pt.stages.iter().cloned());
        all_ref_patterns.extend(pt.ref_patterns.iter().cloned());
        all_sources.extend(pt.sources.iter().cloned());
    }

    let stages: Vec<_> = all_stages.into_iter().collect();
    let ref_patterns: Vec<_> = all_ref_patterns.into_iter().collect();
    let sources: Vec<_> = all_sources.into_iter().collect();

    // For metrics, take weighted average or use the largest type's metrics
    // For simplicity, use the largest type's metrics
    let metrics = types.iter().max_by_key(|pt| pt.count).map_or_else(
        || TypeMetrics {
            total_pipelines: count,
            successful_pipelines: 0,
            failed_pipelines: 0,
            success_rate: 0.0,
            average_duration_seconds: 0.0,
            critical_path: None,
            flaky_jobs: IndexMap::new(),
        },
        |pt| pt.metrics.clone(),
    );

    PipelineType {
        label,
        count,
        percentage,
        jobs,
        ids: all_ids,
        stages,
        ref_patterns,
        sources,
        metrics,
    }
}

fn create_pipeline_type(
    job_names: &[String],
    pipelines: &[&GitLabPipeline],
    total_pipelines: usize,
) -> PipelineType {
    let count = pipelines.len();
    #[allow(clippy::cast_precision_loss)]
    let percentage = (count as f64 / total_pipelines.max(1) as f64) * 100.0;

    // Generate label from job names
    let label = if job_names.iter().any(|j| j.to_lowercase().contains("prod")) {
        "Production Pipeline".to_string()
    } else if job_names.iter().any(|j| {
        let lower = j.to_lowercase();
        lower.contains("staging")
            || lower.contains("dev")
            || lower.contains("test")
            || lower.contains("qa")
    }) {
        "Development Pipeline".to_string()
    } else {
        format!(
            "Pipeline: {}",
            job_names
                .iter()
                .take(3)
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        )
    };

    // Extract common characteristics
    let (stages, ref_patterns, sources) = extract_characteristics(pipelines);

    // Collect pipeline IDs
    let ids: Vec<String> = pipelines.iter().map(|p| p.id.clone()).collect();

    // Calculate metrics
    let metrics = super::metrics::calculate_type_metrics(pipelines);

    PipelineType {
        label,
        count,
        percentage,
        jobs: job_names.to_vec(),
        ids,
        stages,
        ref_patterns,
        sources,
        metrics,
    }
}

fn extract_characteristics(
    pipelines: &[&GitLabPipeline],
) -> (Vec<String>, Vec<String>, Vec<String>) {
    let threshold = pipelines.len() / 10;

    let stages = extract_common(pipelines, threshold * 2, |p| {
        p.jobs.iter().map(|j| j.stage.as_str()).collect()
    });

    let ref_patterns = extract_common(pipelines, threshold, |p| vec![p.ref_.as_str()]);

    let sources = extract_common(pipelines, threshold, |p| vec![p.source.as_str()]);

    (stages, ref_patterns, sources)
}

fn extract_common<F>(pipelines: &[&GitLabPipeline], threshold: usize, extract: F) -> Vec<String>
where
    F: Fn(&GitLabPipeline) -> Vec<&str>,
{
    let mut counts: HashMap<&str, usize> = HashMap::new();

    for pipeline in pipelines {
        for value in extract(pipeline) {
            *counts.entry(value).or_insert(0) += 1;
        }
    }

    let mut items: Vec<(&str, usize)> = counts
        .into_iter()
        .filter(|(_, count)| *count >= threshold)
        .collect();

    items.sort_by(|a, b| b.1.cmp(&a.1));
    items
        .into_iter()
        .take(5)
        .map(|(name, _)| name.to_string())
        .collect()
}
