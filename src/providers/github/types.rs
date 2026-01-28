use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A job within a GitHub Actions workflow run.
///
/// Represents a single job execution with its workflow context and execution details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubJob {
    /// GitHub job ID
    pub id: u64,
    /// Workflow run ID that contains this job
    pub run_id: u64,
    /// Job name as defined in the workflow YAML
    pub name: String,
    /// Workflow name containing this job
    pub workflow_name: String,
    /// Job lifecycle status (e.g., "queued", "in_progress", "completed")
    pub status: String,
    /// Job outcome (e.g., "success", "failure", "cancelled")
    pub conclusion: Option<String>,
    /// Workflow run attempt number (1 = first try, >1 = workflow was retried)
    /// Note: Jobs inherit their run_attempt from the workflow run
    pub run_attempt: u32,
    /// Job execution duration in seconds
    pub duration: f64,
    /// Timestamp when the job started
    pub started_at: DateTime<Utc>,
    /// Timestamp when the job completed (None if still running)
    pub completed_at: Option<DateTime<Utc>>,
    /// Timestamp when the workflow run started (for time to feedback calculation)
    pub workflow_run_started_at: DateTime<Utc>,
    /// GitHub web URL to view this job
    pub html_url: String,
}
