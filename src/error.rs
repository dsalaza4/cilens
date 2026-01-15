use thiserror::Error;

/// Error types for `CILens` operations.
///
/// Covers configuration errors, API failures, network issues, and data parsing problems.
#[derive(Error, Debug)]
pub enum CILensError {
    #[error("Invalid configuration: {0}")]
    Config(String),

    #[error("Project '{0}' not found")]
    ProjectNotFound(String),

    #[error("Pipeline '{0}' not found")]
    PipelineNotFound(String),

    #[error("No pipeline data available for project '{0}'")]
    NoPipelineData(String),

    #[error("No job data available for pipeline '{0}'")]
    NoJobData(String),

    #[error("GraphQL errors in {query_type}: {errors}")]
    GraphQLError { query_type: String, errors: String },

    #[error("GitLab API returned status {status}: {message}")]
    ApiError { status: u16, message: String },

    #[error("GitLab API error (status {status}) after {retries} retries. Please wait a few minutes and try again, or reduce --limit.")]
    ApiErrorAfterRetries { status: u16, retries: u32 },

    #[error("GitHub API error: {0}")]
    GitHubApi(String),

    #[error("GraphQL response contained no data")]
    NoResponseData,

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Cache error: {0}")]
    Cache(String),
}

/// Result type alias using `CILensError` as the error type.
pub type Result<T> = std::result::Result<T, CILensError>;
