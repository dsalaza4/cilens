use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct CIInsights {
    pub provider: String,
    pub project: String,
    pub collected_at: DateTime<Utc>,
    pub total_pipelines: usize,
    pub total_pipeline_types: usize,
    pub pipeline_types: Vec<PipelineType>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredecessorJob {
    pub name: String,
    pub avg_duration_seconds: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobMetrics {
    pub name: String,
    pub avg_duration_seconds: f64,
    pub avg_time_to_feedback_seconds: f64,
    pub predecessors: Vec<PredecessorJob>,
    pub flakiness_score: f64,
    pub flaky_retries: usize,
    pub total_executions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineType {
    pub label: String,
    pub pipeline_ids: Vec<String>,
    pub stages: Vec<String>,
    pub ref_patterns: Vec<String>,
    pub sources: Vec<String>,
    pub metrics: TypeMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeMetrics {
    pub percentage: f64,
    pub total_pipelines: usize,
    pub successful_pipelines: usize,
    pub failed_pipelines: usize,
    pub success_rate: f64,
    pub avg_duration_seconds: f64,
    pub jobs: Vec<JobMetrics>,
}
