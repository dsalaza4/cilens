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
    /// Job lifecycle status (e.g., "queued", "`in_progress`", "completed")
    pub status: String,
    /// Job outcome (e.g., "success", "failure", "cancelled")
    pub conclusion: Option<String>,
    /// Workflow run attempt number (1 = first try, >1 = workflow was retried)
    /// Note: Jobs inherit their `run_attempt` from the workflow run
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

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_github_job_serialization() {
        let job = GitHubJob {
            id: 123,
            run_id: 456,
            name: "build".to_string(),
            workflow_name: "CI Pipeline".to_string(),
            status: "completed".to_string(),
            conclusion: Some("success".to_string()),
            run_attempt: 1,
            duration: 120.5,
            started_at: Utc.timestamp_opt(1_609_459_200, 0).unwrap(),
            completed_at: Some(Utc.timestamp_opt(1_609_459_320, 0).unwrap()),
            workflow_run_started_at: Utc.timestamp_opt(1_609_459_200, 0).unwrap(),
            html_url: "https://github.com/owner/repo/actions/runs/456/jobs/123".to_string(),
        };

        let json = serde_json::to_string(&job).unwrap();
        assert!(json.contains("\"id\":123"));
        assert!(json.contains("\"run_id\":456"));
        assert!(json.contains("\"name\":\"build\""));
        assert!(json.contains("\"workflow_name\":\"CI Pipeline\""));
        assert!(json.contains("\"status\":\"completed\""));
        assert!(json.contains("\"conclusion\":\"success\""));
        assert!(json.contains("\"run_attempt\":1"));
        assert!(json.contains("\"duration\":120.5"));
    }

    #[test]
    fn test_github_job_deserialization() {
        let json = r#"{
            "id": 789,
            "run_id": 101112,
            "name": "test",
            "workflow_name": "Test Suite",
            "status": "completed",
            "conclusion": "failure",
            "run_attempt": 2,
            "duration": 45.3,
            "started_at": "2021-01-01T00:00:00Z",
            "completed_at": "2021-01-01T00:00:45Z",
            "workflow_run_started_at": "2021-01-01T00:00:00Z",
            "html_url": "https://github.com/owner/repo/actions/runs/101112/jobs/789"
        }"#;

        let job: GitHubJob = serde_json::from_str(json).unwrap();
        assert_eq!(job.id, 789);
        assert_eq!(job.run_id, 101_112);
        assert_eq!(job.name, "test");
        assert_eq!(job.workflow_name, "Test Suite");
        assert_eq!(job.status, "completed");
        assert_eq!(job.conclusion, Some("failure".to_string()));
        assert_eq!(job.run_attempt, 2);
        assert_eq!(job.duration, 45.3);
        assert_eq!(
            job.html_url,
            "https://github.com/owner/repo/actions/runs/101112/jobs/789"
        );
    }

    #[test]
    fn test_github_job_deserialization_with_null_conclusion() {
        let json = r#"{
            "id": 999,
            "run_id": 888,
            "name": "deploy",
            "workflow_name": "Deploy",
            "status": "in_progress",
            "conclusion": null,
            "run_attempt": 1,
            "duration": 0.0,
            "started_at": "2021-01-01T00:00:00Z",
            "completed_at": null,
            "workflow_run_started_at": "2021-01-01T00:00:00Z",
            "html_url": "https://github.com/owner/repo/actions/runs/888/jobs/999"
        }"#;

        let job: GitHubJob = serde_json::from_str(json).unwrap();
        assert_eq!(job.id, 999);
        assert_eq!(job.status, "in_progress");
        assert_eq!(job.conclusion, None);
        assert_eq!(job.completed_at, None);
        assert_eq!(job.duration, 0.0);
    }

    #[test]
    fn test_github_job_roundtrip() {
        let original = GitHubJob {
            id: 12345,
            run_id: 67890,
            name: "integration-test".to_string(),
            workflow_name: "Full CI".to_string(),
            status: "completed".to_string(),
            conclusion: Some("success".to_string()),
            run_attempt: 3,
            duration: 300.75,
            started_at: Utc.timestamp_opt(1_609_459_200, 0).unwrap(),
            completed_at: Some(Utc.timestamp_opt(1_609_459_500, 0).unwrap()),
            workflow_run_started_at: Utc.timestamp_opt(1_609_459_100, 0).unwrap(),
            html_url: "https://github.com/owner/repo/actions/runs/67890/jobs/12345".to_string(),
        };

        let json = serde_json::to_string(&original).unwrap();
        let deserialized: GitHubJob = serde_json::from_str(&json).unwrap();

        assert_eq!(original.id, deserialized.id);
        assert_eq!(original.run_id, deserialized.run_id);
        assert_eq!(original.name, deserialized.name);
        assert_eq!(original.workflow_name, deserialized.workflow_name);
        assert_eq!(original.status, deserialized.status);
        assert_eq!(original.conclusion, deserialized.conclusion);
        assert_eq!(original.run_attempt, deserialized.run_attempt);
        assert_eq!(original.duration, deserialized.duration);
        assert_eq!(original.started_at, deserialized.started_at);
        assert_eq!(original.completed_at, deserialized.completed_at);
        assert_eq!(
            original.workflow_run_started_at,
            deserialized.workflow_run_started_at
        );
        assert_eq!(original.html_url, deserialized.html_url);
    }

    #[test]
    fn test_github_job_clone() {
        let job = GitHubJob {
            id: 1,
            run_id: 2,
            name: "test".to_string(),
            workflow_name: "CI".to_string(),
            status: "completed".to_string(),
            conclusion: Some("success".to_string()),
            run_attempt: 1,
            duration: 60.0,
            started_at: Utc.timestamp_opt(1_609_459_200, 0).unwrap(),
            completed_at: Some(Utc.timestamp_opt(1_609_459_260, 0).unwrap()),
            workflow_run_started_at: Utc.timestamp_opt(1_609_459_200, 0).unwrap(),
            html_url: "https://github.com/owner/repo/actions/runs/2/jobs/1".to_string(),
        };

        let cloned = job.clone();
        assert_eq!(job.id, cloned.id);
        assert_eq!(job.name, cloned.name);
        assert_eq!(job.status, cloned.status);
    }
}
