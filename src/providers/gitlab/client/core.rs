use reqwest::Client;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::auth::Token;
use crate::error::{CILensError, Result};

/// GraphQL request payload
#[derive(Debug, Serialize)]
struct GraphQLRequest<V> {
    query: String,
    variables: V,
}

/// GraphQL response wrapper
#[derive(Debug, Deserialize)]
struct GraphQLResponse<T> {
    data: Option<T>,
    errors: Option<Vec<GraphQLError>>,
}

/// GraphQL error details
#[derive(Debug, Deserialize)]
struct GraphQLError {
    message: String,
}

pub struct GitLabClient {
    pub client: Client,
    pub api_url: Url,
    pub graphql_url: Url,
    pub token: Option<Token>,
}

impl GitLabClient {
    pub fn new(base_url: &str, token: Option<Token>) -> Result<Self> {
        let client = Client::builder()
            .user_agent("CILens/0.1.0")
            .build()
            .map_err(|e| CILensError::Config(format!("Failed to create HTTP client: {e}")))?;

        let base = Url::parse(base_url)
            .map_err(|e| CILensError::Config(format!("Invalid base URL: {e}")))?;

        let api_url = base
            .join("api/v4/")
            .map_err(|e| CILensError::Config(format!("Invalid API base URL: {e}")))?;

        let graphql_url = base
            .join("api/graphql")
            .map_err(|e| CILensError::Config(format!("Invalid GraphQL URL: {e}")))?;

        Ok(Self {
            client,
            api_url,
            graphql_url,
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

    /// Execute a GraphQL query against the GitLab GraphQL API
    ///
    /// # Type Parameters
    /// * `T` - The expected response data type (must implement Deserialize)
    /// * `V` - The variables type (must implement Serialize)
    ///
    /// # Arguments
    /// * `query` - The GraphQL query string
    /// * `variables` - The variables to pass to the query
    ///
    /// # Returns
    /// * `Result<T>` - The deserialized response data or an error
    ///
    /// # Errors
    /// Returns an error if:
    /// * The request fails to send
    /// * The response cannot be deserialized
    /// * The GraphQL API returns errors
    pub async fn graphql_query<T, V>(&self, query: &str, variables: V) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
        V: Serialize,
    {
        let request_body = GraphQLRequest {
            query: query.to_string(),
            variables,
        };

        let request = self.client.post(self.graphql_url.clone()).json(&request_body);
        let request = self.auth_request(request);

        let response = request.send().await?;
        let graphql_response: GraphQLResponse<T> = response.json().await?;

        // Check for GraphQL errors first
        if let Some(errors) = graphql_response.errors {
            let error_messages: Vec<String> =
                errors.iter().map(|e| e.message.clone()).collect();
            return Err(CILensError::Config(format!(
                "GraphQL errors: {}",
                error_messages.join(", ")
            )));
        }

        // Extract data or return an error if data is None
        graphql_response.data.ok_or_else(|| {
            CILensError::Config("GraphQL response contained no data".to_string())
        })
    }
}
