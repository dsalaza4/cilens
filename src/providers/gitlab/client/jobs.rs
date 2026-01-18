use chrono::{DateTime as ChronoDateTime, Utc};
use graphql_client::GraphQLQuery;

use super::core::{GitLabClient, PAGE_SIZE};
use crate::error::{CILensError, Result};

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
}
