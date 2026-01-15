use std::collections::HashMap;

use super::types::GitHubWorkflowRun;

/// Groups workflow runs by their workflow name.
///
/// Unlike GitLab's job-centric approach that requires clustering by job signatures,
/// GitHub Actions workflows are static YAML files with fixed names. We simply
/// group runs by workflow name (e.g., "CI", "Deploy") rather than computing
/// job signatures.
///
/// # Arguments
/// * `workflow_runs` - All workflow runs to group
///
/// # Returns
/// A map from workflow name to the list of runs with that workflow name
pub fn group_by_workflow_name(
    workflow_runs: &[GitHubWorkflowRun],
) -> HashMap<String, Vec<GitHubWorkflowRun>> {
    let mut grouped: HashMap<String, Vec<GitHubWorkflowRun>> = HashMap::new();

    for run in workflow_runs {
        grouped
            .entry(run.workflow_name.clone())
            .or_default()
            .push(run.clone());
    }

    grouped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::github::types::GitHubJob;

    fn create_test_workflow_run(id: u64, workflow_name: &str) -> GitHubWorkflowRun {
        GitHubWorkflowRun {
            id,
            workflow_name: workflow_name.to_string(),
            head_branch: Some("main".to_string()),
            event: "push".to_string(),
            status: "completed".to_string(),
            conclusion: Some("success".to_string()),
            run_duration_ms: Some(300_000),
            jobs: vec![],
        }
    }

    #[test]
    fn test_group_by_workflow_name() {
        let runs = vec![
            create_test_workflow_run(1, "CI"),
            create_test_workflow_run(2, "Deploy"),
            create_test_workflow_run(3, "CI"),
            create_test_workflow_run(4, "CI"),
            create_test_workflow_run(5, "Deploy"),
        ];

        let grouped = group_by_workflow_name(&runs);

        assert_eq!(grouped.len(), 2);
        assert_eq!(grouped.get("CI").unwrap().len(), 3);
        assert_eq!(grouped.get("Deploy").unwrap().len(), 2);
    }

    #[test]
    fn test_group_single_workflow() {
        let runs = vec![
            create_test_workflow_run(1, "CI"),
            create_test_workflow_run(2, "CI"),
        ];

        let grouped = group_by_workflow_name(&runs);

        assert_eq!(grouped.len(), 1);
        assert_eq!(grouped.get("CI").unwrap().len(), 2);
    }

    #[test]
    fn test_group_empty_runs() {
        let runs: Vec<GitHubWorkflowRun> = vec![];
        let grouped = group_by_workflow_name(&runs);
        assert_eq!(grouped.len(), 0);
    }

    #[test]
    fn test_workflow_names_preserved() {
        let runs = vec![
            create_test_workflow_run(1, "Build and Test"),
            create_test_workflow_run(2, "Deploy to Production"),
        ];

        let grouped = group_by_workflow_name(&runs);

        assert!(grouped.contains_key("Build and Test"));
        assert!(grouped.contains_key("Deploy to Production"));
    }
}
