use serde::Deserialize;

/// Top-level GraphQL response for pipeline queries
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineQueryResponse {
    pub project: Option<ProjectData>,
}

/// Project data containing pipeline information
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectData {
    pub pipelines: Option<PipelineConnection>,
}

/// Connection type for paginated pipeline results
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineConnection {
    pub page_info: PageInfo,
    pub nodes: Vec<PipelineNode>,
}

/// Pagination information
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageInfo {
    pub has_next_page: bool,
    pub end_cursor: Option<String>,
}

/// Individual pipeline node containing pipeline details
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineNode {
    pub iid: String,
    pub status: String,
    pub duration: Option<u32>,
    pub jobs: Option<JobConnection>,
}

/// Connection type for jobs within a pipeline
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobConnection {
    pub nodes: Vec<JobNode>,
}

/// Individual job node containing job details
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobNode {
    pub name: String,
    pub status: String,
    pub duration: Option<f64>,
    pub needs: Option<NeedsConnection>,
}

/// Connection type for job dependencies (needs)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NeedsConnection {
    pub nodes: Vec<NeedNode>,
}

/// Individual need node representing a job dependency
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NeedNode {
    pub name: String,
}
