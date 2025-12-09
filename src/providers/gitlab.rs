use async_trait::async_trait;
use chrono::Utc;
use log::{info, warn};
use reqwest::Client;
use serde::Deserialize;

use crate::error::{CILensError, Result};
use crate::models::{CIInsights, PipelineSummary};
use crate::providers::{Pipeline, Provider};

#[derive(Debug, Deserialize)]
pub struct GitLabPipeline {
    status: String,
    duration: Option<f64>,
}

pub struct GitLabProvider {
    client: Client,
    project_url: String,
    token: String,
}

impl GitLabProvider {
    pub fn new(base_url: String, project: String, token: String) -> Result<Self> {
        let client = Client::builder()
            .user_agent("CILens/0.1.0")
            .build()
            .map_err(|e| {
                CILensError::ConfigError(format!("Failed to create HTTP client: {}", e))
            })?;

        let project_url = format!(
            "{}/api/v4/projects/{}/pipelines",
            base_url.trim_end_matches('/'),
            urlencoding::encode(&project)
        );

        Ok(Self {
            client,
            project_url,
            token,
        })
    }
}

#[async_trait]
impl Pipeline for GitLabProvider {
    type PipelineData = GitLabPipeline;

    fn build_url(&self, branch: Option<&str>, page: u32, per_page: usize) -> String {
        let mut params = vec![
            ("per_page", per_page.to_string()),
            ("page", page.to_string()),
        ];

        if let Some(branch_name) = branch {
            params.push(("ref", branch_name.to_string()));
        }

        let query_string = params
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&");

        format!("{}?{}", self.project_url, query_string)
    }

    async fn fetch(&self, limit: usize, branch: Option<&str>) -> Result<Vec<GitLabPipeline>> {
        let mut all_pipelines = Vec::new();
        let mut page = 1;
        let per_page = 100;

        info!("Fetching up to {} pipelines...", limit);

        loop {
            let url = self.build_url(branch, page, per_page);

            let mut request = self.client.get(&url);
            if !self.token.is_empty() {
                request = request.header("PRIVATE-TOKEN", &self.token);
            }

            let response = request.send().await?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(CILensError::ApiError(format!(
                    "Failed to fetch pipelines: {} - {}",
                    status, body
                )));
            }

            let mut pipelines: Vec<GitLabPipeline> = response.json().await?;

            // Filter out incomplete pipelines
            pipelines.retain(|p| {
                let unfinished_statuses = ["running", "pending", "created"];
                !unfinished_statuses.contains(&p.status.as_str())
            });

            let fetched_count = pipelines.len();
            all_pipelines.extend(pipelines);

            info!(
                "Page {}: fetched {} completed pipelines (total: {})",
                page,
                fetched_count,
                all_pipelines.len()
            );

            // Stop if we have enough or if the last page returned fewer than per_page items
            if all_pipelines.len() >= limit {
                break;
            }

            page += 1;
        }

        all_pipelines.truncate(limit);
        info!("Returning {} completed pipelines", all_pipelines.len());
        Ok(all_pipelines)
    }

    fn calculate_summary(
        &self,
        pipelines: &[GitLabPipeline],
        total_jobs: usize,
    ) -> PipelineSummary {
        let total_pipelines = pipelines.len();
        let successful_pipelines = pipelines.iter().filter(|p| p.status == "success").count();
        let failed_pipelines = pipelines.iter().filter(|p| p.status == "failed").count();

        let pipeline_success_rate = if total_pipelines > 0 {
            (successful_pipelines as f64 / total_pipelines as f64) * 100.0
        } else {
            0.0
        };

        let average_pipeline_duration = pipelines.iter().filter_map(|p| p.duration).sum::<f64>()
            / total_pipelines.max(1) as f64;

        PipelineSummary {
            total_pipelines,
            successful_pipelines,
            failed_pipelines,
            pipeline_success_rate,
            average_pipeline_duration_seconds: average_pipeline_duration,
            total_jobs_analyzed: total_jobs,
        }
    }
}

#[async_trait]
impl Provider for GitLabProvider {
    async fn collect_insights(
        &self,
        project: &str,
        limit: usize,
        branch: Option<&str>,
    ) -> Result<CIInsights> {
        info!("Starting insights collection for project: {}", project);

        let pipelines = self.fetch(limit, branch).await?;

        if pipelines.is_empty() {
            warn!("No pipelines found for project: {}", project);
        }

        let pipeline_summary = self.calculate_summary(&pipelines, 0);

        Ok(CIInsights {
            provider: "GitLab".to_string(),
            project: project.to_string(),
            collected_at: Utc::now(),
            pipelines_analyzed: pipelines.len(),
            pipeline_summary,
        })
    }
}
