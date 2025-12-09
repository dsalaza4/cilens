use thiserror::Error;

#[derive(Error, Debug)]
pub enum CILensError {
    #[error("API request failed: {0}")]
    ApiError(String),

    #[error("Invalid configuration: {0}")]
    ConfigError(String),

    #[error("Network error: {0}")]
    NetworkError(#[from] reqwest::Error),

    #[error("JSON serialization error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, CILensError>;
