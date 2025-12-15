use reqwest::Client;
use url::Url;

use crate::auth::Token;
use crate::error::{CILensError, Result};

pub struct GitLabClient {
    pub client: Client,
    pub api_url: Url,
    pub token: Option<Token>,
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

    pub fn auth_request(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(token) = &self.token {
            request.bearer_auth(token.as_str())
        } else {
            request
        }
    }

    pub fn project_url(&self, project_id: &str) -> Result<Url> {
        self.api_url
            .join(&format!("projects/{}/", urlencoding::encode(project_id)))
            .map_err(|e| CILensError::Config(format!("Invalid project URL: {e}")))
    }
}
