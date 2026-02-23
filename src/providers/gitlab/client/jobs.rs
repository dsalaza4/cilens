use chrono::{DateTime as ChronoDateTime, Utc};
use graphql_client::GraphQLQuery;
use log::info;

use super::core::{GitLabClient, PAGE_SIZE};

/// Number of pipelines to fetch per page when filtering by branch/tag.
/// Kept small to limit response payload size — each pipeline carries up to 100 jobs.
const PIPELINE_PAGE_SIZE: i64 = 20;
use crate::error::{CILensError, Result};
use crate::providers::gitlab::types::GitLabJob;

pub type JobID = String;
pub type Time = ChronoDateTime<Utc>;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/providers/gitlab/client/schema.json",
    query_path = "src/providers/gitlab/client/jobs.graphql",
    response_derives = "Debug"
)]
pub struct FetchJobs;

pub use fetch_jobs::*;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/providers/gitlab/client/schema.json",
    query_path = "src/providers/gitlab/client/jobs_by_ref.graphql",
    response_derives = "Debug",
    variables_derives = "Clone"
)]
pub struct FetchJobsByRef;

pub use fetch_jobs_by_ref::RefType as GitLabRefType;

impl GitLabClient {
    /// Fetches all jobs for a project with SUCCESS or FAILED status.
    ///
    /// This function uses pagination to retrieve all jobs matching the filter criteria.
    /// Jobs with other statuses (CANCELED, SKIPPED, etc.) are excluded.
    ///
    /// # Arguments
    ///
    /// * `project_path` - Full path to the GitLab project (e.g., "group/project")
    ///
    /// # Returns
    ///
    /// Vector of all jobs matching the criteria.
    ///
    /// # Errors
    ///
    /// Returns an error if the project is not found or the API request fails.
    pub async fn fetch_jobs(
        &self,
        project_path: &str,
        limit: usize,
    ) -> Result<Vec<FetchJobsProjectJobsNodes>> {
        let mut all_jobs = Vec::new();
        let mut cursor: Option<String> = None;

        while all_jobs.len() < limit {
            #[allow(clippy::cast_possible_wrap)]
            let variables = Variables {
                project_path: project_path.to_string(),
                first: PAGE_SIZE as i64,
                after: cursor.clone(),
            };

            let request_body = FetchJobs::build_query(variables);

            let data: ResponseData = self.execute_graphql_request(&request_body).await?;

            let project = data
                .project
                .ok_or_else(|| CILensError::ProjectNotFound(project_path.to_string()))?;

            let jobs = project
                .jobs
                .ok_or_else(|| CILensError::NoJobData(project_path.to_string()))?;

            all_jobs.extend(jobs.nodes.into_iter().flatten().flatten());

            if !jobs.page_info.has_next_page {
                break;
            }

            cursor = jobs.page_info.end_cursor;
        }

        Ok(all_jobs)
    }

    /// Fetches jobs for a project filtered by branch or tag ref.
    ///
    /// Queries pipelines filtered by `git_ref` and `ref_type`, then collects all
    /// SUCCESS/FAILED jobs from those pipelines. Paginates through pipeline pages.
    ///
    /// # Arguments
    ///
    /// * `project_path` - Full path to the GitLab project (e.g., "group/project")
    /// * `git_ref` - Branch or tag name to filter by (e.g., "main", "v1.0.0")
    /// * `ref_type` - Whether `git_ref` is a branch (`Heads`) or tag (`Tags`)
    /// * `limit` - Maximum number of jobs to collect
    ///
    /// # Returns
    ///
    /// Vector of `GitLabJob` structs collected from matching pipelines.
    ///
    /// # Errors
    ///
    /// Returns an error if the project is not found or the API request fails.
    /// Fetches jobs for a project filtered by branch or tag ref.
    ///
    /// Queries pipelines filtered by `git_ref` and optionally `ref_type`, then collects all
    /// SUCCESS/FAILED jobs from those pipelines. Paginates through pipeline pages.
    ///
    /// `ref_type` should be:
    /// - `None` for branch filtering (includes both push and MR pipelines for the branch)
    /// - `Some(RefType::TAGS)` for tag filtering
    ///
    /// # Arguments
    ///
    /// * `project_path` - Full path to the GitLab project (e.g., "group/project")
    /// * `git_ref` - Branch or tag name to filter by (e.g., "main", "v1.0.0")
    /// * `ref_type` - Optional ref type (`None` = any, `Some(TAGS)` = tags only)
    /// * `limit` - Maximum number of jobs to collect
    pub async fn fetch_jobs_by_ref(
        &self,
        project_path: &str,
        git_ref: &str,
        ref_type: Option<fetch_jobs_by_ref::RefType>,
        limit: usize,
    ) -> Result<Vec<GitLabJob>> {
        let mut all_jobs: Vec<GitLabJob> = Vec::new();
        let mut cursor: Option<String> = None;

        while all_jobs.len() < limit {
            #[allow(clippy::cast_possible_wrap)]
            let variables = fetch_jobs_by_ref::Variables {
                project_path: project_path.to_string(),
                git_ref: git_ref.to_string(),
                ref_type: ref_type.clone(),
                first_pipelines: PIPELINE_PAGE_SIZE,
                after_pipeline: cursor.clone(),
            };

            let request_body = FetchJobsByRef::build_query(variables);

            let data: fetch_jobs_by_ref::ResponseData =
                self.execute_graphql_request(&request_body).await?;

            let project = data
                .project
                .ok_or_else(|| CILensError::ProjectNotFound(project_path.to_string()))?;

            let pipelines = project
                .pipelines
                .ok_or_else(|| CILensError::NoJobData(project_path.to_string()))?;

            let page_info = pipelines.page_info;

            for pipeline_node in pipelines.nodes.into_iter().flatten().flatten() {
                if let Some(jobs_conn) = pipeline_node.jobs {
                    for job_node in jobs_conn.nodes.into_iter().flatten().flatten() {
                        if let Some(gitlab_job) = Self::transform_pipeline_job(job_node) {
                            all_jobs.push(gitlab_job);
                        }
                    }
                }
            }

            info!(
                "Collected {} jobs from pipelines for ref '{}'",
                all_jobs.len(),
                git_ref
            );

            if !page_info.has_next_page {
                break;
            }

            cursor = page_info.end_cursor;
        }

        all_jobs.truncate(limit);
        Ok(all_jobs)
    }

    /// Transforms a raw pipeline job node into a `GitLabJob`.
    ///
    /// Returns `None` if required fields (id, name, finished_at) are missing.
    fn transform_pipeline_job(
        node: fetch_jobs_by_ref::FetchJobsByRefProjectPipelinesNodesJobsNodes,
    ) -> Option<GitLabJob> {
        let id = node.id?;
        let name = node.name?;
        let finished_at = node.finished_at?;

        #[allow(clippy::cast_precision_loss, clippy::cast_sign_loss)]
        Some(GitLabJob {
            id,
            name,
            duration: node.duration.unwrap_or(0) as f64,
            status: node
                .status
                .map_or_else(|| "UNKNOWN".to_string(), |s| format!("{s:?}")),
            retried: node.retried.unwrap_or(false),
            needs: node
                .needs
                .map(|conn| {
                    conn.nodes
                        .into_iter()
                        .flatten()
                        .flatten()
                        .filter_map(|n| n.name)
                        .collect()
                })
                .unwrap_or_default(),
            created_at: node.created_at,
            finished_at,
        })
    }
}
