use reqwest::Client;
use serde::Deserialize;
use url::Url;

use crate::auth::Token;
use crate::error::{CILensError, Result};

pub struct GitLabClient {
    client: Client,
    api_url: Url,
    token: Option<Token>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GitLabPipelineListDto {
    pub id: u32,
    pub status: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GitLabPipelineDto {
    pub status: String,
    pub duration: Option<usize>,
}

// Allowed statuses for list DTO
const ALLOWED_STATUSES: [&str; 2] = ["success", "failed"];

impl GitLabPipelineListDto {
    pub fn is_valid(&self) -> bool {
        ALLOWED_STATUSES.contains(&self.status.as_str())
    }
}

impl GitLabPipelineDto {
    pub fn is_valid(&self) -> bool {
        self.duration.is_some()
    }
}

impl GitLabClient {
    pub fn new(base_url: &str, token: Option<Token>) -> Result<Self> {
        let client = Client::builder()
            .user_agent("CILens/0.1.0")
            .build()
            .map_err(|e| CILensError::Config(format!("Failed to create HTTP client: {e}")))?;

        let api_url = Url::parse(base_url)
            .map_err(|e| CILensError::Config(format!("Invalid base URL: {e}")))?
            .join("api/v4/")
            .map_err(|e| CILensError::Config(format!("Invalid API base URL: {e}")))?;

        Ok(Self {
            client,
            api_url,
            token,
        })
    }

    /// Helper to build authenticated requests
    fn auth_request(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(token) = &self.token {
            request.bearer_auth(token.as_str())
        } else {
            request
        }
    }

    /// Construct project base URL
    fn project_url(&self, project_id: &str) -> Result<Url> {
        self.api_url
            .join(&format!("projects/{}/", urlencoding::encode(project_id)))
            .map_err(|e| CILensError::Config(format!("Invalid project URL: {e}")))
    }

    /// Construct full pipeline URL
    fn pipeline_url(&self, project_id: &str, pipeline_id: u32) -> Result<Url> {
        self.project_url(project_id)?
            .join(&format!("pipelines/{pipeline_id}"))
            .map_err(|e| CILensError::Config(format!("Invalid pipeline URL: {e}")))
    }

    /// Fetch a page of pipelines
    pub async fn fetch_pipeline_list_page(
        &self,
        project_id: &str,
        page: u32,
        per_page: u32,
        branch: Option<&str>,
    ) -> Result<Vec<GitLabPipelineListDto>> {
        let url = self
            .project_url(project_id)?
            .join("pipelines")
            .map_err(|e| CILensError::Config(format!("Invalid pipelines URL: {e}")))?;

        let mut request = self
            .client
            .get(url)
            .query(&[("page", page), ("per_page", per_page)]);
        if let Some(branch) = branch {
            request = request.query(&[("ref", branch)]);
        }
        request = self.auth_request(request);

        let response = request.send().await?.error_for_status()?;
        let pipelines = response.json::<Vec<GitLabPipelineListDto>>().await?;
        Ok(pipelines)
    }

    /// Fetch a single pipeline
    pub async fn fetch_pipeline(
        &self,
        project_id: &str,
        pipeline_id: u32,
    ) -> Result<GitLabPipelineDto> {
        let url = self.pipeline_url(project_id, pipeline_id)?;
        let request = self.auth_request(self.client.get(url));

        let response = request.send().await?.error_for_status()?;
        let pipeline = response.json::<GitLabPipelineDto>().await?;
        Ok(pipeline)
    }
}
