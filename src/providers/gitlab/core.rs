use chrono::Utc;
use log::{info, warn};

use crate::auth::Token;
use crate::error::Result;
use crate::insights::CIInsights;
use crate::providers::gitlab::client::pipelines::{fetch_pipeline_jobs, fetch_pipelines};
use crate::providers::gitlab::client::GitLabClient;

#[derive(Debug)]
pub struct GitLabPipeline {
    pub id: String,
    pub ref_: String,
    pub source: String,
    pub status: String,
    pub duration: usize,
    pub stages: Vec<String>,
    pub jobs: Vec<GitLabJob>,
}

#[derive(Debug)]
pub struct GitLabJob {
    pub name: String,
    pub stage: String,
    pub duration: f64,
    pub status: String,
    pub retried: bool,
    pub needs: Option<Vec<String>>,
}

pub struct GitLabProvider {
    pub client: GitLabClient,
    pub project_path: String,
}

impl GitLabProvider {
    pub fn new(base_url: &str, project_path: String, token: Option<Token>) -> Result<Self> {
        let client = GitLabClient::new(base_url, token)?;

        Ok(Self {
            client,
            project_path,
        })
    }

    async fn fetch_pipelines(
        &self,
        limit: usize,
        ref_: Option<&str>,
    ) -> Result<Vec<GitLabPipeline>> {
        info!("Fetching up to {limit} pipelines...");

        let pipeline_nodes = self
            .client
            .fetch_pipelines(&self.project_path, limit, ref_)
            .await?;

        info!(
            "Fetching jobs for {} pipelines in parallel...",
            pipeline_nodes.len()
        );

        // Fetch jobs for all pipelines concurrently
        let futures: Vec<_> = pipeline_nodes
            .into_iter()
            .map(|node| self.transform_pipeline_with_jobs(node))
            .collect();

        let results = futures::future::join_all(futures).await;

        // Collect successful results, filtering out pipelines without duration
        let pipelines: Vec<_> = results
            .into_iter()
            .filter_map(Result::transpose)
            .collect::<Result<_>>()?;

        info!("Processed {} pipelines", pipelines.len());

        Ok(pipelines)
    }

    async fn transform_pipeline_with_jobs(
        &self,
        node: fetch_pipelines::FetchPipelinesProjectPipelinesNodes,
    ) -> Result<Option<GitLabPipeline>> {
        // Only include pipelines with duration
        let Some(duration) = node.duration else {
            return Ok(None);
        };

        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let duration = duration as usize;

        // Fetch all jobs for this pipeline
        let job_nodes = self
            .client
            .fetch_pipeline_jobs(&self.project_path, &node.id)
            .await?;

        let jobs = Self::transform_job_nodes(job_nodes);

        // Extract stage order from pipeline metadata
        let stages = node
            .stages
            .map(|stages_conn| {
                stages_conn
                    .nodes
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter_map(|stage| stage.name)
                    .collect()
            })
            .unwrap_or_default();

        Ok(Some(GitLabPipeline {
            id: node.id,
            ref_: node.ref_.unwrap_or_default(),
            source: node.source.unwrap_or_default(),
            status: format!("{:?}", node.status).to_lowercase(),
            duration,
            stages,
            jobs,
        }))
    }

    fn transform_job_nodes(
        job_nodes: Vec<fetch_pipeline_jobs::FetchPipelineJobsProjectPipelineJobsNodes>,
    ) -> Vec<GitLabJob> {
        job_nodes
            .into_iter()
            .map(|job_node| {
                #[allow(clippy::cast_precision_loss)]
                GitLabJob {
                    name: job_node.name.unwrap_or_default(),
                    stage: job_node.stage.and_then(|s| s.name).unwrap_or_default(),
                    duration: job_node.duration.unwrap_or(0) as f64,
                    status: job_node
                        .status
                        .map(|s| format!("{s:?}"))
                        .unwrap_or_default(),
                    retried: job_node.retried.unwrap_or(false),
                    needs: job_node.needs.map(|needs_conn| {
                        needs_conn
                            .nodes
                            .into_iter()
                            .flatten()
                            .flatten()
                            .filter_map(|need| need.name)
                            .collect()
                    }),
                }
            })
            .collect()
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

        let pipeline_types = super::clustering::cluster_and_analyze(&pipelines);

        Ok(CIInsights {
            provider: "GitLab".to_string(),
            project: self.project_path.clone(),
            collected_at: Utc::now(),
            total_pipelines: pipelines.len(),
            total_pipeline_types: pipeline_types.len(),
            pipeline_types,
        })
    }
}
