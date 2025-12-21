use graphql_client::GraphQLQuery;

use super::core::GitLabClient;
use crate::error::{CILensError, Result};

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/providers/gitlab/client/schema.json",
    query_path = "src/providers/gitlab/client/pipelines.graphql",
    response_derives = "Debug,PartialEq"
)]
pub struct FetchPipelines;

impl GitLabClient {
    pub async fn fetch_pipelines_graphql(
        &self,
        project_path: &str,
        limit: usize,
        ref_: Option<&str>,
    ) -> Result<Vec<fetch_pipelines::FetchPipelinesProjectPipelinesNodes>> {
        const PAGE_SIZE: i64 = 50;

        let mut all_pipelines = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let remaining = limit.saturating_sub(all_pipelines.len());
            if remaining == 0 {
                break;
            }

            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            let fetch_count = std::cmp::min(remaining, PAGE_SIZE as usize) as i64;

            let variables = fetch_pipelines::Variables {
                project_path: project_path.to_string(),
                first: fetch_count,
                after: cursor.clone(),
                ref_: ref_.map(std::string::ToString::to_string),
            };

            let request_body = FetchPipelines::build_query(variables);

            let request = self
                .client
                .post(self.graphql_url.clone())
                .json(&request_body);
            let request = self.auth_request(request);

            let response = request.send().await?;
            let response_body: graphql_client::Response<fetch_pipelines::ResponseData> =
                response.json().await?;

            if let Some(errors) = response_body.errors {
                let error_messages: Vec<String> =
                    errors.iter().map(|e| e.message.clone()).collect();
                let joined_errors = error_messages.join(", ");
                return Err(CILensError::Config(format!(
                    "GraphQL errors: {joined_errors}"
                )));
            }

            let data = response_body.data.ok_or_else(|| {
                CILensError::Config("GraphQL response contained no data".to_string())
            })?;

            let project = data.project.ok_or_else(|| {
                CILensError::Config(format!("Project '{project_path}' not found"))
            })?;

            let pipelines = project.pipelines.ok_or_else(|| {
                CILensError::Config(format!(
                    "No pipeline data available for project '{project_path}'"
                ))
            })?;

            all_pipelines.extend(pipelines.nodes.into_iter().flatten().flatten());

            if !pipelines.page_info.has_next_page || all_pipelines.len() >= limit {
                break;
            }

            cursor = pipelines.page_info.end_cursor;

            if cursor.is_none() {
                break;
            }
        }

        all_pipelines.truncate(limit);

        Ok(all_pipelines)
    }
}
