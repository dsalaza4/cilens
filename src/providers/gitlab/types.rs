use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A job within a GitLab CI/CD pipeline.
///
/// Represents a single job execution with its dependencies and execution details.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct GitLabJob {
    /// GraphQL Global ID (e.g., <gid://gitlab/Ci::Job/456>)
    pub id: String,
    /// Job name as defined in .gitlab-ci.yml
    pub name: String,
    /// Job execution duration in seconds
    pub duration: f64,
    /// Final job status (e.g., "SUCCESS", "FAILED")
    pub status: String,
    /// Whether this job was retried (intermittent failure indicator)
    pub retried: bool,
    /// Explicit job dependencies via `needs` keyword
    pub needs: Vec<String>,
    /// Timestamp when the job was created
    pub created_at: DateTime<Utc>,
    /// Timestamp when the job finished
    pub finished_at: DateTime<Utc>,
}
