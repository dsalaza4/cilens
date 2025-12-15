use chrono::Utc;
use futures::{stream, StreamExt, TryStreamExt};
use log::{info, warn};
use serde::Deserialize;

use super::core::GitLabProvider;
use crate::error::Result;
use crate::insights::{CIInsights, PipelineSummary};

const CONCURRENCY: usize = 10;

#[derive(Debug, Deserialize)]
pub struct GitLabPipeline {
    status: String,
    duration: usize,
}

impl GitLabProvider {
    pub async fn fetch_pipelines(
        &self,
        limit: usize,
        branch: Option<&str>,
    ) -> Result<Vec<GitLabPipeline>> {
        let mut all_pipelines = Vec::with_capacity(limit);
        let mut page = 1;
        let per_page = 100;

        info!("Fetching up to {limit} pipelines...");

        #[allow(clippy::redundant_closure_for_method_calls)]
        while all_pipelines.len() < limit {
            // Fetch a page of pipeline list and pre-filter invalid ones
            let pipelines_list = self
                .client
                .fetch_pipeline_list_page(&self.project_id, page, per_page, branch)
                .await?
                .into_iter()
                .filter(|p| p.is_valid())
                .collect::<Vec<_>>();

            if pipelines_list.is_empty() {
                info!("No more pipelines returned by API, stopping");
                break;
            }

            // Fetch full pipeline data concurrently
            #[allow(clippy::redundant_closure_for_method_calls)]
            let pipelines: Vec<GitLabPipeline> = stream::iter(pipelines_list)
                .map(|p| async move { self.client.fetch_pipeline(&self.project_id, p.id).await })
                .buffer_unordered(CONCURRENCY)
                .try_collect::<Vec<_>>()
                .await?
                .into_iter()
                .filter(|p| p.is_valid())
                .take(limit.saturating_sub(all_pipelines.len())) // enforce remaining limit
                .map(|p| GitLabPipeline {
                    status: p.status,
                    duration: p
                        .duration
                        .expect("Pipeline duration should be Some, but found None"),
                })
                .collect();

            let fetched_count = pipelines.len();
            all_pipelines.extend(pipelines);

            info!(
                "Page {page}: fetched {fetched_count} pipelines (total: {})",
                all_pipelines.len()
            );

            page += 1;
        }

        Ok(all_pipelines)
    }

    fn calculate_summary(pipelines: &[GitLabPipeline]) -> PipelineSummary {
        let total_pipelines = pipelines.len();
        let successful_pipelines = pipelines.iter().filter(|p| p.status == "success").count();
        let failed_pipelines = pipelines.iter().filter(|p| p.status == "failed").count();

        #[allow(clippy::cast_precision_loss)]
        let pipeline_success_rate = if total_pipelines > 0 {
            (successful_pipelines as f64 / total_pipelines as f64) * 100.0
        } else {
            0.0
        };

        #[allow(clippy::cast_precision_loss)]
        let average_pipeline_duration = pipelines.iter().map(|p| p.duration as f64).sum::<f64>()
            / total_pipelines.max(1) as f64;

        PipelineSummary {
            total_pipelines,
            successful_pipelines,
            failed_pipelines,
            pipeline_success_rate,
            average_pipeline_duration,
        }
    }

    pub async fn collect_insights(
        &self,
        project_id: &str,
        limit: usize,
        branch: Option<&str>,
    ) -> Result<CIInsights> {
        info!("Starting insights collection for project: {project_id}");

        let pipelines = self.fetch_pipelines(limit, branch).await?;

        if pipelines.is_empty() {
            warn!("No pipelines found for project: {project_id}");
        }

        let pipeline_summary = Self::calculate_summary(&pipelines);

        Ok(CIInsights {
            provider: "GitLab".to_string(),
            project: project_id.to_string(),
            collected_at: Utc::now(),
            pipelines_analyzed: pipelines.len(),
            pipeline_summary,
        })
    }
}
