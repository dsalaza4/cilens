use serde::{Deserialize, Serialize};

use super::core::GitLabClient;
use crate::error::{CILensError, Result};

/// GraphQL query for fetching pipelines with jobs and dependencies
const PIPELINES_QUERY: &str = r#"
query($projectPath: ID!, $first: Int!, $after: String, $ref: String) {
  project(fullPath: $projectPath) {
    pipelines(first: $first, after: $after, ref: $ref) {
      pageInfo {
        hasNextPage
        endCursor
      }
      nodes {
        iid
        status
        duration
        jobs {
          nodes {
            name
            status
            duration
            needs {
              nodes {
                name
              }
            }
          }
        }
      }
    }
  }
}
"#;

/// Variables for pipeline GraphQL queries
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineQueryVariables {
    pub project_path: String,
    pub first: i32,
    pub after: Option<String>,
    #[serde(rename = "ref")]
    pub ref_name: Option<String>,
}

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

impl GitLabClient {
    /// Fetch pipelines using GraphQL with cursor-based pagination
    ///
    /// # Arguments
    /// * `project_path` - The full path of the project (e.g., "group/project")
    /// * `limit` - Maximum number of pipelines to fetch
    /// * `branch` - Optional branch name to filter pipelines
    ///
    /// # Returns
    /// * `Result<Vec<PipelineNode>>` - Vector of pipeline nodes or an error
    ///
    /// # Errors
    /// Returns an error if:
    /// * The GraphQL query fails
    /// * The project is not found
    /// * The response cannot be deserialized
    ///
    /// # Example
    /// ```no_run
    /// # use cilens::providers::gitlab::client::core::GitLabClient;
    /// # async fn example() -> cilens::error::Result<()> {
    /// let client = GitLabClient::new("https://gitlab.com", None)?;
    /// let pipelines = client.fetch_pipelines_graphql("group/project", 10, Some("main")).await?;
    /// println!("Found {} pipelines", pipelines.len());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn fetch_pipelines_graphql(
        &self,
        project_path: &str,
        limit: usize,
        branch: Option<&str>,
    ) -> Result<Vec<PipelineNode>> {
        let mut all_pipelines = Vec::new();
        let mut cursor: Option<String> = None;

        // GitLab GraphQL typically allows up to 100 items per page
        const PAGE_SIZE: i32 = 100;

        loop {
            // Calculate how many more pipelines we need
            let remaining = limit.saturating_sub(all_pipelines.len());
            if remaining == 0 {
                break;
            }

            // Request at most PAGE_SIZE items, but no more than what we need
            let fetch_count = std::cmp::min(remaining, PAGE_SIZE as usize) as i32;

            let variables = PipelineQueryVariables {
                project_path: project_path.to_string(),
                first: fetch_count,
                after: cursor.clone(),
                ref_name: branch.map(|b| b.to_string()),
            };

            let response: PipelineQueryResponse =
                self.graphql_query(PIPELINES_QUERY, variables).await?;

            // Extract project data or return error if not found
            let project = response.project.ok_or_else(|| {
                CILensError::Config(format!("Project '{}' not found", project_path))
            })?;

            // Extract pipelines or return error if not available
            let pipelines = project.pipelines.ok_or_else(|| {
                CILensError::Config(format!(
                    "No pipeline data available for project '{}'",
                    project_path
                ))
            })?;

            // Collect pipeline nodes
            all_pipelines.extend(pipelines.nodes);

            // Check if there are more pages and we haven't reached the limit
            if !pipelines.page_info.has_next_page || all_pipelines.len() >= limit {
                break;
            }

            // Update cursor for next iteration
            cursor = pipelines.page_info.end_cursor;

            // Safety check: if we have an empty cursor but hasNextPage is true, break to avoid infinite loop
            if cursor.is_none() {
                break;
            }
        }

        // Ensure we don't return more than the requested limit
        all_pipelines.truncate(limit);

        Ok(all_pipelines)
    }
}
