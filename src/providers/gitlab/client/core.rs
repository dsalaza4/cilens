use graphql_client::Response as GraphQLResponse;
use log::warn;
use reqwest::Client;
use std::time::Duration;
use url::Url;

use crate::auth::Token;
use crate::error::{CILensError, Result};

const MAX_RETRIES: u32 = 30;
const RETRY_DELAY_SECONDS: u64 = 10;
pub(super) const PAGE_SIZE: usize = 100;

/// GitLab GraphQL API client with built-in retry logic.
///
/// Handles authentication, rate limiting, and automatic retries for transient failures.
/// Uses sequential cursor-based pagination (inherent to GraphQL cursor pagination).
pub struct GitLabClient {
    /// HTTP client for making requests
    pub client: Client,
    /// GitLab GraphQL API endpoint URL
    pub graphql_url: Url,
    /// Optional authentication token
    pub token: Option<Token>,
}

impl GitLabClient {
    /// Creates a new GitLab client for the specified instance.
    ///
    /// # Arguments
    ///
    /// * `base_url` - GitLab instance base URL (e.g., <https://gitlab.com>)
    /// * `token` - Optional authentication token for API access
    ///
    /// # Returns
    ///
    /// Configured client ready to make GraphQL requests.
    ///
    /// # Errors
    ///
    /// Returns an error if the base URL is invalid or cannot be parsed.
    pub fn new(base_url: &str, token: Option<Token>) -> Result<Self> {
        let client = Client::builder()
            .user_agent("CILens/0.1.0")
            .build()
            .map_err(|e| CILensError::Config(format!("Failed to create HTTP client: {e}")))?;

        let base = Url::parse(base_url)
            .map_err(|e| CILensError::Config(format!("Invalid base URL: {e}")))?;

        let graphql_url = base
            .join("api/graphql")
            .map_err(|e| CILensError::Config(format!("Invalid GraphQL URL: {e}")))?;

        Ok(Self {
            client,
            graphql_url,
            token,
        })
    }

    /// Adds authentication header to a request if a token is configured.
    ///
    /// # Arguments
    ///
    /// * `request` - Request builder to add authentication to
    ///
    /// # Returns
    ///
    /// Request builder with Bearer token header added (if token is present).
    pub fn auth_request(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(token) = &self.token {
            request.bearer_auth(token.as_str())
        } else {
            request
        }
    }

    /// Execute a GraphQL request with automatic retry on network errors and rate limits
    /// Returns the data from the GraphQL response after checking for errors
    pub(super) async fn execute_graphql_request<T>(
        &self,
        request_body: &impl serde::Serialize,
    ) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let mut retry_count = 0;
        loop {
            let request = self.auth_request(
                self.client
                    .post(self.graphql_url.clone())
                    .json(request_body),
            );

            let response = match request.send().await {
                Ok(resp) => resp,
                Err(e) if e.is_connect() || e.is_timeout() || e.is_request() => {
                    if retry_count >= MAX_RETRIES {
                        return Err(e.into());
                    }
                    warn!(
                        "Network error ({}), retrying in {}s ({}/{})...",
                        e,
                        RETRY_DELAY_SECONDS,
                        retry_count + 1,
                        MAX_RETRIES
                    );
                    tokio::time::sleep(Duration::from_secs(RETRY_DELAY_SECONDS)).await;
                    retry_count += 1;
                    continue;
                }
                Err(e) => return Err(e.into()),
            };

            // Check for rate limiting or other HTTP errors before parsing JSON
            let status = response.status();

            if status == 429 || status.is_server_error() {
                if retry_count >= MAX_RETRIES {
                    return Err(CILensError::ApiErrorAfterRetries {
                        status: status.as_u16(),
                        retries: MAX_RETRIES,
                    });
                }

                warn!(
                    "GitLab API error (status {status}). Waiting {RETRY_DELAY_SECONDS} seconds before retry {}/{}...",
                    retry_count + 1,
                    MAX_RETRIES
                );

                tokio::time::sleep(Duration::from_secs(RETRY_DELAY_SECONDS)).await;
                retry_count += 1;
                continue;
            }

            if !status.is_success() {
                let error_text = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unable to read error response".to_string());
                return Err(CILensError::ApiError {
                    status: status.as_u16(),
                    message: error_text,
                });
            }

            // Parse GraphQL response and check for errors
            let response_body: GraphQLResponse<T> = response.json().await?;

            if let Some(errors) = response_body.errors {
                return Err(CILensError::GraphQLError {
                    query_type: std::any::type_name::<T>().to_string(),
                    errors: errors
                        .iter()
                        .map(|e| &e.message)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", "),
                });
            }

            return response_body
                .data
                .ok_or_else(|| CILensError::NoResponseData);
        }
    }
}
