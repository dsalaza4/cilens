use chrono::{DateTime, Utc};
use indexmap::IndexMap;
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
pub struct CriticalPath {
    pub jobs: Vec<String>,
    pub average_duration_seconds: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PipelineType {
    pub label: String,
    pub count: usize,
    pub percentage: f64,
    pub stages: Vec<String>,
    pub ref_patterns: Vec<String>,
    pub sources: Vec<String>,
    pub metrics: TypeMetrics,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TypeMetrics {
    pub total_pipelines: usize,
    pub successful_pipelines: usize,
    pub failed_pipelines: usize,
    pub success_rate: f64,
    pub average_duration_seconds: f64,
    pub critical_path: Option<CriticalPath>,
    pub retry_rates: IndexMap<String, f64>,
}
