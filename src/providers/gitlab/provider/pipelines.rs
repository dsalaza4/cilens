use chrono::Utc;
use indexmap::IndexMap;
use log::{info, warn};
use std::collections::HashMap;

use super::core::GitLabProvider;
use crate::error::Result;
use crate::insights::{CIInsights, CriticalPath, FlakyJobMetrics, PipelineType, TypeMetrics};
use crate::providers::gitlab::client::pipelines::fetch_pipelines;

#[derive(Debug)]
struct GitLabPipeline {
    ref_: String,
    source: String,
    status: String,
    duration: usize,
    jobs: Vec<GitLabJob>,
}

#[derive(Debug)]
struct GitLabJob {
    name: String,
    stage: String,
    duration: f64,
    status: String,
    retried: bool,
    needs: Vec<String>,
}

impl GitLabProvider {
    async fn fetch_pipelines(
        &self,
        limit: usize,
        ref_: Option<&str>,
    ) -> Result<Vec<GitLabPipeline>> {
        info!("Fetching up to {limit} pipelines...");

        let pipeline_nodes = self
            .client
            .fetch_pipelines_graphql(&self.project_path, limit, ref_)
            .await?;

        let pipelines: Vec<GitLabPipeline> = pipeline_nodes
            .into_iter()
            .filter_map(Self::transform_pipeline_node)
            .collect();

        info!("Processed {} pipelines", pipelines.len());

        Ok(pipelines)
    }

    fn transform_pipeline_node(
        node: fetch_pipelines::FetchPipelinesProjectPipelinesNodes,
    ) -> Option<GitLabPipeline> {
        // Only include completed pipelines with duration
        if !((node.status == fetch_pipelines::PipelineStatusEnum::SUCCESS
            || node.status == fetch_pipelines::PipelineStatusEnum::FAILED)
            && node.duration.is_some())
        {
            return None;
        }

        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let duration = node.duration.unwrap() as usize;
        let jobs = Self::transform_jobs(node.jobs);

        Some(GitLabPipeline {
            ref_: node.ref_.unwrap_or_default(),
            source: node.source.unwrap_or_default(),
            status: format!("{:?}", node.status).to_lowercase(),
            duration,
            jobs,
        })
    }

    fn transform_jobs(
        job_conn: Option<fetch_pipelines::FetchPipelinesProjectPipelinesNodesJobs>,
    ) -> Vec<GitLabJob> {
        job_conn
            .map(|conn| {
                conn.nodes
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter_map(|job_node| {
                        #[allow(clippy::cast_precision_loss)]
                        job_node.duration.map(|dur| GitLabJob {
                            name: job_node.name.unwrap_or_default(),
                            stage: job_node.stage.and_then(|s| s.name).unwrap_or_default(),
                            duration: dur as f64,
                            status: job_node
                                .status
                                .map(|s| format!("{s:?}"))
                                .unwrap_or_default(),
                            retried: job_node.retried.unwrap_or(false),
                            needs: job_node
                                .needs
                                .map(|needs_conn| {
                                    needs_conn
                                        .nodes
                                        .into_iter()
                                        .flatten()
                                        .flatten()
                                        .filter_map(|need| need.name)
                                        .collect()
                                })
                                .unwrap_or_default(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn calculate_critical_path(pipeline: &GitLabPipeline) -> Option<CriticalPath> {
        if pipeline.jobs.is_empty() {
            return None;
        }

        // Fast path: if no dependencies, return slowest job
        if !pipeline.jobs.iter().any(|j| !j.needs.is_empty()) {
            return Self::slowest_job_path(pipeline);
        }

        // Calculate finish times for dependency graph
        let job_map: HashMap<&str, &GitLabJob> =
            pipeline.jobs.iter().map(|j| (j.name.as_str(), j)).collect();

        let (finish_times, predecessors) = Self::calculate_finish_times(&job_map);

        // Find and reconstruct critical path
        let (&critical_job, &total_time) = finish_times
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())?;

        let path = Self::reconstruct_path(critical_job, &predecessors);

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
    ) -> (HashMap<&'a str, f64>, HashMap<&'a str, &'a str>) {
        let mut finish_times: HashMap<&str, f64> = HashMap::new();
        let mut predecessors: HashMap<&str, &str> = HashMap::new();

        // Calculate finish times for all jobs using memoization
        for &job_name in job_map.keys() {
            Self::calculate_job_finish_time(
                job_name,
                job_map,
                &mut finish_times,
                &mut predecessors,
            );
        }

        (finish_times, predecessors)
    }

    fn calculate_job_finish_time<'a>(
        job_name: &'a str,
        job_map: &HashMap<&'a str, &'a GitLabJob>,
        finish_times: &mut HashMap<&'a str, f64>,
        predecessors: &mut HashMap<&'a str, &'a str>,
    ) -> f64 {
        // Return memoized result if already calculated
        if let Some(&time) = finish_times.get(job_name) {
            return time;
        }

        // Job not in map (filtered out or missing) - treat as duration 0
        let Some(job) = job_map.get(job_name) else {
            finish_times.insert(job_name, 0.0);
            return 0.0;
        };

        // Base case: no dependencies
        if job.needs.is_empty() {
            finish_times.insert(job_name, job.duration);
            return job.duration;
        }

        // Recursive case: find critical predecessor
        let (critical_pred, max_pred_time) = job
            .needs
            .iter()
            .map(|need| {
                let need_str = need.as_str();
                let time =
                    Self::calculate_job_finish_time(need_str, job_map, finish_times, predecessors);
                (need_str, time)
            })
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .unwrap();

        let finish_time = max_pred_time + job.duration;
        finish_times.insert(job_name, finish_time);
        predecessors.insert(job_name, critical_pred);

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

    pub async fn collect_insights(&self, limit: usize, ref_: Option<&str>) -> Result<CIInsights> {
        info!(
            "Starting insights collection for project: {}",
            self.project_path
        );

        let pipelines = self.fetch_pipelines(limit, ref_).await?;

        if pipelines.is_empty() {
            warn!("No pipelines found for project: {}", self.project_path);
        }

        let pipeline_types = Self::cluster_and_analyze(&pipelines);

        Ok(CIInsights {
            provider: "GitLab".to_string(),
            project: self.project_path.clone(),
            collected_at: Utc::now(),
            total_pipelines: pipelines.len(),
            total_pipeline_types: pipeline_types.len(),
            pipeline_types,
        })
    }

    // Pipeline clustering by job signature
    fn cluster_and_analyze(pipelines: &[GitLabPipeline]) -> Vec<PipelineType> {
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
                Self::create_pipeline_type(&job_names, &cluster_pipelines, total_pipelines)
            })
            .collect();

        pipeline_types.sort_by(|a, b| b.count.cmp(&a.count));
        pipeline_types
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
            lower.contains("staging") || lower.contains("dev")
        }) {
            "Development Pipeline".to_string()
        } else if job_names.iter().any(|j| {
            let lower = j.to_lowercase();
            lower.contains("test") || lower.contains("qa")
        }) {
            "Test Pipeline".to_string()
        } else {
            let key_jobs: Vec<&String> = job_names.iter().take(3).collect();
            format!(
                "Pipeline: {}",
                key_jobs
                    .iter()
                    .map(|j| j.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };

        // Extract common characteristics
        let (stages, ref_patterns, sources) = Self::extract_characteristics(pipelines);

        // Calculate metrics
        let metrics = Self::calculate_type_metrics(pipelines);

        PipelineType {
            label,
            count,
            percentage,
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

        let stages = Self::extract_common(pipelines, threshold * 2, |p| {
            p.jobs.iter().map(|j| j.stage.clone()).collect()
        });

        let ref_patterns = Self::extract_common(pipelines, threshold, |p| vec![p.ref_.clone()]);

        let sources = Self::extract_common(pipelines, threshold, |p| vec![p.source.clone()]);

        (stages, ref_patterns, sources)
    }

    fn extract_common<F>(pipelines: &[&GitLabPipeline], threshold: usize, extract: F) -> Vec<String>
    where
        F: Fn(&GitLabPipeline) -> Vec<String>,
    {
        let mut counts: HashMap<String, usize> = HashMap::new();

        for pipeline in pipelines {
            for value in extract(pipeline) {
                *counts.entry(value).or_insert(0) += 1;
            }
        }

        let mut items: Vec<(String, usize)> = counts
            .into_iter()
            .filter(|(_, count)| *count >= threshold)
            .collect();

        items.sort_by(|a, b| b.1.cmp(&a.1));
        items.into_iter().take(5).map(|(name, _)| name).collect()
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

    fn find_flaky_jobs(pipelines: &[&GitLabPipeline]) -> IndexMap<String, FlakyJobMetrics> {
        let mut flaky_counts: HashMap<String, usize> = HashMap::new();
        let mut total_counts: HashMap<String, usize> = HashMap::new();

        // Analyze each pipeline for flaky jobs
        for pipeline in pipelines {
            let mut jobs_by_name: HashMap<String, Vec<&GitLabJob>> = HashMap::new();
            for job in &pipeline.jobs {
                jobs_by_name.entry(job.name.clone()).or_default().push(job);
            }

            for (name, jobs) in jobs_by_name {
                *total_counts.entry(name.clone()).or_insert(0) += 1;
                if Self::is_flaky(&jobs) {
                    *flaky_counts.entry(name).or_insert(0) += 1;
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

    fn calculate_type_metrics(pipelines: &[&GitLabPipeline]) -> TypeMetrics {
        let total_pipelines = pipelines.len();

        let successful_pipelines: Vec<_> = pipelines
            .iter()
            .filter(|p| p.status == "success")
            .copied()
            .collect();

        let failed_pipelines = pipelines.iter().filter(|p| p.status == "failed").count();

        #[allow(clippy::cast_precision_loss)]
        let success_rate = if total_pipelines > 0 {
            (successful_pipelines.len() as f64 / total_pipelines as f64) * 100.0
        } else {
            0.0
        };

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

        let critical_paths: Vec<_> = successful_pipelines
            .iter()
            .filter_map(|p| Self::calculate_critical_path(p))
            .collect();

        let critical_path = if critical_paths.is_empty() {
            None
        } else {
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
        };

        let flaky_jobs = Self::find_flaky_jobs(pipelines);

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
}
