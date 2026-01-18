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

    #[error("No job data available for project '{0}'")]
    NoJobData(String),

    #[error("GraphQL errors in {query_type}: {errors}")]
    GraphQLError { query_type: String, errors: String },

    #[error("GitLab API returned status {status}: {message}")]
    ApiError { status: u16, message: String },

    #[error("GitLab API error (status {status}) after {retries} retries. Please wait a few minutes and try again, or reduce --limit.")]
    ApiErrorAfterRetries { status: u16, retries: u32 },

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_error_displays_correctly() {
        let error = CILensError::Config("invalid token".to_string());
        let message = error.to_string();
        assert!(message.contains("Invalid configuration"));
        assert!(message.contains("invalid token"));
    }

    #[test]
    fn test_project_not_found_error() {
        let error = CILensError::ProjectNotFound("group/project".to_string());
        let message = error.to_string();
        assert!(message.contains("Project"));
        assert!(message.contains("group/project"));
        assert!(message.contains("not found"));
    }

    #[test]
    fn test_no_job_data_error() {
        let error = CILensError::NoJobData("test/project".to_string());
        let message = error.to_string();
        assert!(message.contains("No job data"));
        assert!(message.contains("test/project"));
    }

    #[test]
    fn test_graphql_error() {
        let error = CILensError::GraphQLError {
            query_type: "fetchJobs".to_string(),
            errors: "Field not found".to_string(),
        };
        let message = error.to_string();
        assert!(message.contains("GraphQL errors"));
        assert!(message.contains("fetchJobs"));
        assert!(message.contains("Field not found"));
    }

    #[test]
    fn test_api_error() {
        let error = CILensError::ApiError {
            status: 404,
            message: "Not Found".to_string(),
        };
        let message = error.to_string();
        assert!(message.contains("404"));
        assert!(message.contains("Not Found"));
    }

    #[test]
    fn test_api_error_after_retries() {
        let error = CILensError::ApiErrorAfterRetries {
            status: 429,
            retries: 30,
        };
        let message = error.to_string();
        assert!(message.contains("429"));
        assert!(message.contains("30 retries"));
        assert!(message.contains("wait"));
    }

    #[test]
    fn test_no_response_data_error() {
        let error = CILensError::NoResponseData;
        let message = error.to_string();
        assert!(message.contains("no data"));
    }

    #[test]
    fn test_cache_error() {
        let error = CILensError::Cache("Failed to write cache file".to_string());
        let message = error.to_string();
        assert!(message.contains("Cache error"));
        assert!(message.contains("Failed to write"));
    }

    #[test]
    fn test_io_error_conversion() {
        let io_error = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let error: CILensError = io_error.into();
        let message = error.to_string();
        assert!(message.contains("IO error"));
        assert!(message.contains("file not found"));
    }

    #[test]
    fn test_json_error_conversion() {
        // Create an invalid JSON and try to parse it
        let json_error = serde_json::from_str::<serde_json::Value>("{invalid}").unwrap_err();
        let error: CILensError = json_error.into();
        let message = error.to_string();
        assert!(message.contains("JSON"));
    }

    #[test]
    fn test_result_type_alias() {
        let success: Result<i32> = Ok(42);
        assert!(success.is_ok());
        if let Ok(value) = success {
            assert_eq!(value, 42);
        }

        let failure: Result<i32> = Err(CILensError::NoResponseData);
        assert!(failure.is_err());
    }
}
