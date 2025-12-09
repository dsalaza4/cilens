use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct CIInsights {
    pub provider: String,
    pub project: String,
    pub collected_at: DateTime<Utc>,
    pub pipelines_analyzed: usize,
    pub pipeline_summary: PipelineSummary,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PipelineSummary {
    pub total_pipelines: usize,
    pub successful_pipelines: usize,
    pub failed_pipelines: usize,
    pub pipeline_success_rate: f64,
    pub average_pipeline_duration_seconds: f64,
    pub total_jobs_analyzed: usize,
}
