use super::types::GitHubWorkflowRun;
use super::workflow_grouping::group_by_workflow_name;
use super::workflow_metrics::calculate_workflow_metrics;
use crate::insights::PipelineType;

/// Transforms grouped workflow runs into PipelineType structures.
///
/// Unlike GitLab which clusters pipelines by job signatures, GitHub Actions
/// workflows are static YAML files. We simply group runs by workflow name
/// and use the workflow name as the pipeline type identifier.
///
/// # Arguments
///
/// * `workflow_runs` - All workflow runs to transform
///
/// # Returns
///
/// A vector of `PipelineType` structures, one for each unique workflow name,
/// sorted by the number of runs (most common workflows first).
pub fn transform_to_pipeline_types(workflow_runs: &[GitHubWorkflowRun]) -> Vec<PipelineType> {
    if workflow_runs.is_empty() {
        return vec![];
    }

    // Group runs by workflow name
    let grouped = group_by_workflow_name(workflow_runs);

    let total_runs = workflow_runs.len();

    // Transform each workflow group into a PipelineType
    let mut pipeline_types: Vec<PipelineType> = grouped
        .into_iter()
        .map(|(workflow_name, runs)| {
            #[allow(clippy::cast_precision_loss)]
            let percentage = (runs.len() as f64 / total_runs as f64) * 100.0;

            let metrics = calculate_workflow_metrics(&workflow_name, &runs, percentage);

            // Extract unique branches and trigger events
            let ref_patterns = extract_unique_branches(&runs);
            let sources = extract_unique_events(&runs);

            PipelineType {
                id: workflow_name.clone(),
                label: workflow_name.clone(),
                stages: vec![], // GitHub doesn't have explicit stages like GitLab
                ref_patterns,
                sources,
                metrics,
            }
        })
        .collect();

    // Sort by number of runs (most common first)
    pipeline_types.sort_by(|a, b| b.metrics.total_pipelines.cmp(&a.metrics.total_pipelines));

    pipeline_types
}

/// Extracts unique branch names from workflow runs.
fn extract_unique_branches(runs: &[GitHubWorkflowRun]) -> Vec<String> {
    let mut branches: Vec<String> = runs
        .iter()
        .filter_map(|r| r.head_branch.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    branches.sort();
    branches
}

/// Extracts unique trigger events from workflow runs.
fn extract_unique_events(runs: &[GitHubWorkflowRun]) -> Vec<String> {
    let mut events: Vec<String> = runs
        .iter()
        .map(|r| r.event.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    events.sort();
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::github::types::GitHubWorkflowRun;

    fn create_test_run(
        id: u64,
        workflow_name: &str,
        branch: Option<&str>,
        event: &str,
    ) -> GitHubWorkflowRun {
        GitHubWorkflowRun {
            id,
            workflow_name: workflow_name.to_string(),
            head_branch: branch.map(String::from),
            event: event.to_string(),
            status: "completed".to_string(),
            conclusion: Some("success".to_string()),
            run_started_at: Some("2025-01-01T00:00:00Z".to_string()),
            run_duration_ms: Some(300_000),
            jobs: vec![],
        }
    }

    #[test]
    fn test_transform_empty_runs() {
        let runs: Vec<GitHubWorkflowRun> = vec![];
        let pipeline_types = transform_to_pipeline_types(&runs);
        assert_eq!(pipeline_types.len(), 0);
    }

    #[test]
    fn test_transform_single_workflow() {
        let runs = vec![
            create_test_run(1, "CI", Some("main"), "push"),
            create_test_run(2, "CI", Some("main"), "push"),
        ];

        let pipeline_types = transform_to_pipeline_types(&runs);

        assert_eq!(pipeline_types.len(), 1);
        assert_eq!(pipeline_types[0].id, "CI");
        assert_eq!(pipeline_types[0].label, "CI");
        assert_eq!(pipeline_types[0].metrics.total_pipelines, 2);
        assert_eq!(pipeline_types[0].metrics.percentage, 100.0);
    }

    #[test]
    fn test_transform_multiple_workflows() {
        let runs = vec![
            create_test_run(1, "CI", Some("main"), "push"),
            create_test_run(2, "CI", Some("main"), "push"),
            create_test_run(3, "CI", Some("main"), "push"),
            create_test_run(4, "Deploy", Some("main"), "workflow_dispatch"),
            create_test_run(5, "Deploy", Some("main"), "workflow_dispatch"),
        ];

        let pipeline_types = transform_to_pipeline_types(&runs);

        assert_eq!(pipeline_types.len(), 2);

        // Should be sorted by total_pipelines (most common first)
        assert_eq!(pipeline_types[0].id, "CI");
        assert_eq!(pipeline_types[0].metrics.total_pipelines, 3);
        assert!((pipeline_types[0].metrics.percentage - 60.0).abs() < 0.01);

        assert_eq!(pipeline_types[1].id, "Deploy");
        assert_eq!(pipeline_types[1].metrics.total_pipelines, 2);
        assert!((pipeline_types[1].metrics.percentage - 40.0).abs() < 0.01);
    }

    #[test]
    fn test_extract_unique_branches() {
        let runs = vec![
            create_test_run(1, "CI", Some("main"), "push"),
            create_test_run(2, "CI", Some("develop"), "push"),
            create_test_run(3, "CI", Some("main"), "push"),
            create_test_run(4, "CI", None, "push"),
        ];

        let branches = extract_unique_branches(&runs);

        assert_eq!(branches.len(), 2);
        assert!(branches.contains(&"main".to_string()));
        assert!(branches.contains(&"develop".to_string()));
    }

    #[test]
    fn test_extract_unique_events() {
        let runs = vec![
            create_test_run(1, "CI", Some("main"), "push"),
            create_test_run(2, "CI", Some("main"), "pull_request"),
            create_test_run(3, "CI", Some("main"), "push"),
        ];

        let events = extract_unique_events(&runs);

        assert_eq!(events.len(), 2);
        assert!(events.contains(&"push".to_string()));
        assert!(events.contains(&"pull_request".to_string()));
    }

    #[test]
    fn test_no_stages_for_github() {
        let runs = vec![create_test_run(1, "CI", Some("main"), "push")];

        let pipeline_types = transform_to_pipeline_types(&runs);

        assert_eq!(pipeline_types[0].stages.len(), 0);
    }

    #[test]
    fn test_workflow_name_as_id_and_label() {
        let runs = vec![create_test_run(1, "Build and Test", Some("main"), "push")];

        let pipeline_types = transform_to_pipeline_types(&runs);

        assert_eq!(pipeline_types[0].id, "Build and Test");
        assert_eq!(pipeline_types[0].label, "Build and Test");
    }
}
