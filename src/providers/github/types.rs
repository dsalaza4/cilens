/// A GitHub Actions workflow run.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GitHubWorkflowRun {
    /// Workflow run ID
    pub id: u64,
    /// Workflow name (e.g., "CI", "Deploy")
    pub workflow_name: String,
    /// Git reference (branch/tag)
    pub head_branch: Option<String>,
    /// Trigger event (push, `pull_request`, schedule, etc.)
    pub event: String,
    /// Final status (completed, `in_progress`, queued)
    pub status: String,
    /// Conclusion (success, failure, cancelled, skipped, etc.)
    pub conclusion: Option<String>,
    /// When the workflow run started (ISO 8601 timestamp)
    pub run_started_at: Option<String>,
    /// Duration in milliseconds
    pub run_duration_ms: Option<u64>,
    /// All jobs in this workflow run
    pub jobs: Vec<GitHubJob>,
}

impl GitHubWorkflowRun {
    /// Returns true if workflow run is completed.
    #[allow(dead_code)]
    pub fn is_completed(&self) -> bool {
        self.status == "completed"
    }

    /// Returns true if workflow run succeeded.
    pub fn is_success(&self) -> bool {
        self.conclusion.as_deref() == Some("success")
    }

    /// Returns duration in seconds (converts from milliseconds).
    #[allow(dead_code)]
    pub fn duration_seconds(&self) -> Option<usize> {
        self.run_duration_ms.map(|ms| (ms / 1000) as usize)
    }
}

/// A job within a GitHub Actions workflow run.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GitHubJob {
    /// Job ID
    pub id: u64,
    /// Job name
    pub name: String,
    /// Final status (completed, `in_progress`, queued)
    pub status: String,
    /// Conclusion (success, failure, cancelled, skipped, etc.)
    pub conclusion: Option<String>,
    /// Started at timestamp
    pub started_at: Option<String>,
    /// Completed at timestamp
    pub completed_at: Option<String>,
    /// Job dependencies via `needs`
    pub needs: Option<Vec<String>>,
}

impl GitHubJob {
    /// Calculates job duration in seconds.
    #[allow(clippy::cast_precision_loss)]
    pub fn duration_seconds(&self) -> Option<f64> {
        match (&self.started_at, &self.completed_at) {
            (Some(start), Some(end)) => {
                use chrono::{DateTime, Utc};
                let start: DateTime<Utc> = start.parse().ok()?;
                let end: DateTime<Utc> = end.parse().ok()?;
                let duration = end.signed_duration_since(start);
                Some(duration.num_seconds() as f64)
            }
            _ => None,
        }
    }

    /// Calculates time-to-feedback: time from workflow start until this job completes.
    ///
    /// # Arguments
    /// * `workflow_started_at` - When the workflow run started (ISO 8601 timestamp)
    ///
    /// # Returns
    /// Time in seconds from workflow start to job completion, or None if timestamps are missing.
    #[allow(clippy::cast_precision_loss)]
    pub fn time_to_feedback(&self, workflow_started_at: Option<&str>) -> Option<f64> {
        match (workflow_started_at, &self.completed_at) {
            (Some(workflow_start), Some(job_end)) => {
                use chrono::{DateTime, Utc};
                let start: DateTime<Utc> = workflow_start.parse().ok()?;
                let end: DateTime<Utc> = job_end.parse().ok()?;
                let duration = end.signed_duration_since(start);
                Some(duration.num_seconds() as f64)
            }
            _ => None,
        }
    }

    /// Returns true if job succeeded.
    pub fn is_success(&self) -> bool {
        self.conclusion.as_deref() == Some("success")
    }

    /// Returns true if job failed.
    pub fn is_failure(&self) -> bool {
        self.conclusion.as_deref() == Some("failure")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workflow_run_is_completed() {
        let run = GitHubWorkflowRun {
            id: 1,
            workflow_name: "CI".to_string(),
            head_branch: None,
            event: "push".to_string(),
            status: "completed".to_string(),
            conclusion: Some("success".to_string()),
            run_started_at: Some("2025-01-01T00:00:00Z".to_string()),
            run_duration_ms: Some(60_000),
            jobs: vec![],
        };
        assert!(run.is_completed());
        assert!(run.is_success());
        assert_eq!(run.duration_seconds(), Some(60));
    }

    #[test]
    fn test_job_time_to_feedback() {
        let job = GitHubJob {
            id: 1,
            name: "test".to_string(),
            status: "completed".to_string(),
            conclusion: Some("success".to_string()),
            started_at: Some("2025-01-01T00:01:00Z".to_string()), // Started 1 min after workflow
            completed_at: Some("2025-01-01T00:05:00Z".to_string()), // Completed at 5 min
            needs: None,
        };
        // Duration: 5 - 1 = 4 minutes = 240 seconds
        assert_eq!(job.duration_seconds(), Some(240.0));
        // Time-to-feedback from workflow start at 00:00:00 to job completion at 00:05:00 = 5 minutes = 300 seconds
        assert_eq!(job.time_to_feedback(Some("2025-01-01T00:00:00Z")), Some(300.0));
    }

    #[test]
    fn test_job_duration_calculation() {
        let job = GitHubJob {
            id: 1,
            name: "test".to_string(),
            status: "completed".to_string(),
            conclusion: Some("success".to_string()),
            started_at: Some("2025-01-01T00:00:00Z".to_string()),
            completed_at: Some("2025-01-01T00:05:00Z".to_string()),
            needs: None,
        };
        assert_eq!(job.duration_seconds(), Some(300.0));
        assert!(job.is_success());
    }
}
