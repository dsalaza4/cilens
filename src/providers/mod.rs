pub mod gitlab;

use crate::error::Result;
use crate::models::{CIInsights, PipelineSummary};
use async_trait::async_trait;

#[async_trait]
pub trait Pipeline {
    type PipelineData;

    fn build_url(&self, branch: Option<&str>) -> String;

    async fn fetch(&self, limit: usize, branch: Option<&str>) -> Result<Vec<Self::PipelineData>>;

    fn calculate_summary(
        &self,
        pipelines: &[Self::PipelineData],
        total_jobs: usize,
    ) -> PipelineSummary;
}

#[async_trait]
pub trait Provider {
    async fn collect_insights(
        &self,
        project: &str,
        limit: usize,
        branch: Option<&str>,
    ) -> Result<CIInsights>;
}
