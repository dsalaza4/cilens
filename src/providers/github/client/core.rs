use log::warn;
use reqwest::{Client, Method, StatusCode};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use url::Url;

use crate::auth::Token;
use crate::error::{CILensError, Result};

const MAX_RETRIES: u32 = 30;
const RETRY_DELAY_SECONDS: u64 = 10;
const MAX_CONCURRENT_REQUESTS: usize = 500;
const MAX_WAIT_SECONDS: u64 = 300; // 5 minutes - max time to wait for rate limit reset
const GITHUB_API_VERSION: &str = "2022-11-28";
pub const PAGE_SIZE: usize = 100;

/// GitHub REST API client with built-in retry logic and concurrency control.
///
/// Handles authentication, rate limiting, and automatic retries for transient failures.
/// Limits concurrent requests to `MAX_CONCURRENT_REQUESTS` to avoid overwhelming the GitHub API.
#[derive(Clone)]
pub struct GitHubClient {
    /// HTTP client for making requests
    pub client: Client,
    /// GitHub API base URL (e.g., <https://api.github.com>)
    pub api_url: Url,
    /// Optional authentication token
    pub token: Option<Token>,
    /// Repository owner
    pub owner: String,
    /// Repository name
    pub repo: String,
    /// Semaphore to limit concurrent requests
    semaphore: Arc<Semaphore>,
}

impl GitHubClient {
    /// Creates a new GitHub client for the specified repository.
    ///
    /// # Arguments
    ///
    /// * `base_url` - GitHub base URL (e.g., <https://github.com>)
    /// * `repository` - Repository in "owner/repo" format
    /// * `token` - Optional authentication token for API access
    ///
    /// # Returns
    ///
    /// Configured client ready to make REST API requests.
    ///
    /// # Errors
    ///
    /// Returns an error if the base URL is invalid, cannot be parsed, or if
    /// the repository format is invalid (must be "owner/repo").
    pub fn new(base_url: &str, repository: &str, token: Option<Token>) -> Result<Self> {
        let (owner, repo) = Self::parse_repository(repository)?;
        let client = Self::build_http_client()?;
        let api_url = Self::build_api_url(base_url)?;

        Ok(Self {
            client,
            api_url,
            token,
            owner,
            repo,
            semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS)),
        })
    }

    /// Parses a repository string into owner and repo name.
    fn parse_repository(repository: &str) -> Result<(String, String)> {
        let parts: Vec<&str> = repository.split('/').collect();
        if parts.len() != 2 {
            return Err(CILensError::Config(format!(
                "Invalid repository format '{repository}'. Expected 'owner/repo'"
            )));
        }
        Ok((parts[0].to_string(), parts[1].to_string()))
    }

    /// Builds the HTTP client with user agent.
    fn build_http_client() -> Result<Client> {
        Client::builder()
            .user_agent("CILens/0.1.0")
            .build()
            .map_err(|e| CILensError::Config(format!("Failed to create HTTP client: {e}")))
    }

    /// Converts base URL to GitHub API URL.
    ///
    /// - <https://github.com> -> <https://api.github.com>
    /// - Custom GitHub Enterprise: <https://example.com> -> <https://example.com/api/v3>
    fn build_api_url(base_url: &str) -> Result<Url> {
        if base_url.contains("github.com") && !base_url.contains("api.github.com") {
            return Url::parse("https://api.github.com")
                .map_err(|e| CILensError::Config(format!("Invalid API URL: {e}")));
        }

        let base = Url::parse(base_url)
            .map_err(|e| CILensError::Config(format!("Invalid base URL: {e}")))?;
        base.join("api/v3")
            .map_err(|e| CILensError::Config(format!("Invalid API URL: {e}")))
    }

    /// Adds authentication and required headers to a request.
    ///
    /// # Arguments
    ///
    /// * `request` - Request builder to add headers to
    ///
    /// # Returns
    ///
    /// Request builder with authentication and GitHub-required headers added.
    pub fn auth_request(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let mut req = request
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", GITHUB_API_VERSION);

        if let Some(token) = &self.token {
            req = req.bearer_auth(token.as_str());
        }

        req
    }

    /// Parses a header value as a u64.
    fn parse_header_u64(headers: &reqwest::header::HeaderMap, key: &str) -> Option<u64> {
        headers.get(key)?.to_str().ok()?.parse().ok()
    }

    /// Gets the current Unix timestamp in seconds.
    fn current_timestamp() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    /// Execute a REST API request with automatic retry on network errors and rate limits.
    ///
    /// Handles GitHub-specific rate limiting by checking response headers and backing off
    /// when rate limits are hit.
    ///
    /// # Type Parameters
    ///
    /// * `T` - Response type that can be deserialized from JSON
    ///
    /// # Arguments
    ///
    /// * `method` - HTTP method (GET, POST, etc.)
    /// * `url` - Full URL to request
    ///
    /// # Returns
    ///
    /// Deserialized response data
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Maximum retries exceeded for network errors or rate limits
    /// - Non-retryable HTTP error occurs (4xx except 429)
    /// - Response cannot be deserialized
    pub(super) async fn execute_request<T>(&self, method: Method, url: Url) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let _permit = self.semaphore.acquire().await.unwrap();

        let mut retry_count = 0;
        loop {
            let response = self.send_request(&method, &url, &mut retry_count).await?;
            let status = response.status();

            Self::check_rate_limit_warning(response.headers());

            if let Some(delay) = Self::should_retry_on_error(status, response.headers())? {
                self.handle_retryable_error(status, delay, &mut retry_count)
                    .await?;
                continue;
            }

            if !status.is_success() {
                return self.handle_non_success_status(status, response).await;
            }

            return response.json().await.map_err(Into::into);
        }
    }

    /// Sends a single HTTP request with retry logic for network errors.
    async fn send_request(
        &self,
        method: &Method,
        url: &Url,
        retry_count: &mut u32,
    ) -> Result<reqwest::Response> {
        loop {
            let request = self.auth_request(self.client.request(method.clone(), url.clone()));

            match request.send().await {
                Ok(response) => return Ok(response),
                Err(e) if Self::is_retryable_network_error(&e) => {
                    if *retry_count >= MAX_RETRIES {
                        return Err(e.into());
                    }
                    warn!(
                        "Network error ({}), retrying in {}s ({}/{})...",
                        e,
                        RETRY_DELAY_SECONDS,
                        *retry_count + 1,
                        MAX_RETRIES
                    );
                    tokio::time::sleep(Duration::from_secs(RETRY_DELAY_SECONDS)).await;
                    *retry_count += 1;
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    /// Checks if a network error is retryable.
    fn is_retryable_network_error(error: &reqwest::Error) -> bool {
        error.is_connect() || error.is_timeout() || error.is_request()
    }

    /// Logs a warning if rate limit is exhausted.
    fn check_rate_limit_warning(headers: &reqwest::header::HeaderMap) {
        if Self::parse_header_u64(headers, "x-ratelimit-remaining") == Some(0) {
            warn!("GitHub API rate limit exhausted");
        }
    }

    /// Determines if an error status should be retried and returns the delay.
    fn should_retry_on_error(
        status: StatusCode,
        headers: &reqwest::header::HeaderMap,
    ) -> Result<Option<u64>> {
        if Self::is_rate_limit_error(status, headers) {
            let delay = Self::calculate_retry_delay(headers);
            Self::validate_rate_limit_delay(delay, headers)?;
            Ok(Some(delay))
        } else if status.is_server_error() {
            Ok(Some(RETRY_DELAY_SECONDS))
        } else {
            Ok(None)
        }
    }

    /// Checks if the status code indicates a rate limit error.
    fn is_rate_limit_error(status: StatusCode, headers: &reqwest::header::HeaderMap) -> bool {
        status == StatusCode::TOO_MANY_REQUESTS
            || (status == StatusCode::FORBIDDEN && headers.contains_key("x-ratelimit-remaining"))
    }

    /// Calculates retry delay from response headers.
    fn calculate_retry_delay(headers: &reqwest::header::HeaderMap) -> u64 {
        // Try retry-after header first
        if let Some(delay) = Self::parse_header_u64(headers, "retry-after") {
            return delay;
        }

        // Fall back to x-ratelimit-reset header
        if let Some(reset_timestamp) = Self::parse_header_u64(headers, "x-ratelimit-reset") {
            return reset_timestamp
                .saturating_sub(Self::current_timestamp())
                .max(1);
        }

        RETRY_DELAY_SECONDS
    }

    /// Validates that the rate limit delay is reasonable (< 5 minutes).
    fn validate_rate_limit_delay(delay: u64, headers: &reqwest::header::HeaderMap) -> Result<()> {
        if delay <= MAX_WAIT_SECONDS {
            return Ok(());
        }

        let minutes = delay / 60;
        let is_exhausted = Self::parse_header_u64(headers, "x-ratelimit-remaining") == Some(0);

        let message = if is_exhausted {
            format!(
                "GitHub API rate limit exceeded. Limit will reset in {minutes} minutes. \
                Try again later or use a token with higher rate limits."
            )
        } else {
            format!("GitHub API rate limit error. Retry after {minutes} minutes.")
        };

        Err(CILensError::ApiError {
            status: 429,
            message,
        })
    }

    /// Handles a retryable error by sleeping and incrementing retry count.
    async fn handle_retryable_error(
        &self,
        status: StatusCode,
        delay: u64,
        retry_count: &mut u32,
    ) -> Result<()> {
        if *retry_count >= MAX_RETRIES {
            return Err(CILensError::ApiErrorAfterRetries {
                status: status.as_u16(),
                retries: MAX_RETRIES,
            });
        }

        warn!(
            "GitHub API error (status {status}). Waiting {delay} seconds before retry {}/{}...",
            *retry_count + 1,
            MAX_RETRIES
        );

        tokio::time::sleep(Duration::from_secs(delay)).await;
        *retry_count += 1;
        Ok(())
    }

    /// Handles non-success HTTP status codes.
    async fn handle_non_success_status<T>(
        &self,
        status: StatusCode,
        response: reqwest::Response,
    ) -> Result<T> {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unable to read error response".to_string());

        Err(CILensError::ApiError {
            status: status.as_u16(),
            message: error_text,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::GitHubClient;
    use crate::auth::Token;
    use reqwest::header::HeaderMap;

    #[test]
    fn test_parse_repository_valid() {
        let result = GitHubClient::new("https://github.com", "owner/repo", None);
        assert!(result.is_ok());
        let client = result.unwrap();
        assert_eq!(client.owner, "owner");
        assert_eq!(client.repo, "repo");
    }

    #[test]
    fn test_parse_repository_invalid_single_part() {
        let result = GitHubClient::new("https://github.com", "justowner", None);
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.to_string().contains("Invalid repository format"));
        }
    }

    #[test]
    fn test_parse_repository_invalid_too_many_parts() {
        let result = GitHubClient::new("https://github.com", "owner/repo/extra", None);
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.to_string().contains("Invalid repository format"));
        }
    }

    #[test]
    fn test_parse_repository_empty() {
        let result = GitHubClient::new("https://github.com", "", None);
        assert!(result.is_err());
    }

    #[test]
    fn test_build_api_url_github_com() {
        let client = GitHubClient::new("https://github.com", "owner/repo", None).unwrap();
        assert_eq!(client.api_url.as_str(), "https://api.github.com/");
    }

    #[test]
    fn test_build_api_url_github_com_with_www() {
        let client = GitHubClient::new("https://www.github.com", "owner/repo", None).unwrap();
        assert_eq!(client.api_url.as_str(), "https://api.github.com/");
    }

    #[test]
    fn test_build_api_url_enterprise() {
        let client = GitHubClient::new("https://github.example.com", "owner/repo", None).unwrap();
        // URL parsing may or may not include trailing slash
        let url = client.api_url.as_str();
        assert!(url.starts_with("https://github.example.com/api/v3"));
    }

    #[test]
    fn test_build_api_url_enterprise_http() {
        let client = GitHubClient::new("http://github.example.com", "owner/repo", None).unwrap();
        // URL parsing may or may not include trailing slash
        let url = client.api_url.as_str();
        assert!(url.starts_with("http://github.example.com/api/v3"));
    }

    #[test]
    fn test_build_api_url_invalid() {
        let result = GitHubClient::new("not-a-url", "owner/repo", None);
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.to_string().contains("Invalid"));
        }
    }

    #[test]
    fn test_auth_request_without_token() {
        let client = GitHubClient::new("https://github.com", "owner/repo", None).unwrap();
        let request = client
            .client
            .request(reqwest::Method::GET, "https://api.github.com");
        let request = client.auth_request(request);

        // We can't easily inspect the built request without building it,
        // but we can verify it was created successfully
        assert!(request.build().is_ok());
    }

    #[test]
    fn test_auth_request_with_token() {
        let token = Token::from("ghp_test_token_1234567890");
        let client = GitHubClient::new("https://github.com", "owner/repo", Some(token)).unwrap();
        let request = client
            .client
            .request(reqwest::Method::GET, "https://api.github.com");
        let request = client.auth_request(request);

        // Verify request can be built
        assert!(request.build().is_ok());
    }

    #[test]
    fn test_parse_header_u64_valid() {
        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-remaining", "5000".parse().unwrap());

        let result = GitHubClient::parse_header_u64(&headers, "x-ratelimit-remaining");
        assert_eq!(result, Some(5000));
    }

    #[test]
    fn test_parse_header_u64_missing() {
        let headers = HeaderMap::new();
        let result = GitHubClient::parse_header_u64(&headers, "x-ratelimit-remaining");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_header_u64_invalid() {
        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-remaining", "not-a-number".parse().unwrap());

        let result = GitHubClient::parse_header_u64(&headers, "x-ratelimit-remaining");
        assert_eq!(result, None);
    }

    #[test]
    fn test_current_timestamp_is_reasonable() {
        let timestamp = GitHubClient::current_timestamp();
        // Should be a reasonable Unix timestamp (after 2020)
        assert!(timestamp > 1_600_000_000);
        // Should be before year 2100
        assert!(timestamp < 4_000_000_000);
    }

    #[test]
    fn test_calculate_retry_delay_with_retry_after() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", "60".parse().unwrap());

        let _client = GitHubClient::new("https://github.com", "owner/repo", None).unwrap();
        let delay = GitHubClient::calculate_retry_delay(&headers);
        assert_eq!(delay, 60);
    }

    #[test]
    fn test_calculate_retry_delay_with_ratelimit_reset() {
        let mut headers = HeaderMap::new();
        let reset_time = GitHubClient::current_timestamp() + 120;
        headers.insert("x-ratelimit-reset", reset_time.to_string().parse().unwrap());

        let _client = GitHubClient::new("https://github.com", "owner/repo", None).unwrap();
        let delay = GitHubClient::calculate_retry_delay(&headers);
        // Should be around 120 seconds (with some tolerance for execution time)
        assert!((119..=121).contains(&delay));
    }

    #[test]
    fn test_calculate_retry_delay_with_past_reset_time() {
        let mut headers = HeaderMap::new();
        let reset_time = GitHubClient::current_timestamp() - 10;
        headers.insert("x-ratelimit-reset", reset_time.to_string().parse().unwrap());

        let _client = GitHubClient::new("https://github.com", "owner/repo", None).unwrap();
        let delay = GitHubClient::calculate_retry_delay(&headers);
        // Should return at least 1 second
        assert_eq!(delay, 1);
    }

    #[test]
    fn test_calculate_retry_delay_no_headers() {
        let headers = HeaderMap::new();
        let _client = GitHubClient::new("https://github.com", "owner/repo", None).unwrap();
        let delay = GitHubClient::calculate_retry_delay(&headers);
        // Should return default delay
        assert_eq!(delay, 10); // RETRY_DELAY_SECONDS
    }

    #[test]
    fn test_validate_rate_limit_delay_acceptable() {
        let headers = HeaderMap::new();
        let _client = GitHubClient::new("https://github.com", "owner/repo", None).unwrap();
        let result = GitHubClient::validate_rate_limit_delay(120, &headers);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_rate_limit_delay_at_max() {
        let headers = HeaderMap::new();
        let _client = GitHubClient::new("https://github.com", "owner/repo", None).unwrap();
        let result = GitHubClient::validate_rate_limit_delay(300, &headers);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_rate_limit_delay_too_long() {
        let headers = HeaderMap::new();
        let _client = GitHubClient::new("https://github.com", "owner/repo", None).unwrap();
        let result = GitHubClient::validate_rate_limit_delay(400, &headers);
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.to_string().contains("Retry after"));
        }
    }

    #[test]
    fn test_validate_rate_limit_delay_exhausted() {
        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-remaining", "0".parse().unwrap());

        let _client = GitHubClient::new("https://github.com", "owner/repo", None).unwrap();
        let result = GitHubClient::validate_rate_limit_delay(400, &headers);
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.to_string().contains("exceeded"));
        }
    }

    #[test]
    fn test_is_rate_limit_error_429() {
        let headers = HeaderMap::new();
        assert!(GitHubClient::is_rate_limit_error(
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            &headers
        ));
    }

    #[test]
    fn test_is_rate_limit_error_403_with_header() {
        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-remaining", "0".parse().unwrap());

        assert!(GitHubClient::is_rate_limit_error(
            reqwest::StatusCode::FORBIDDEN,
            &headers
        ));
    }

    #[test]
    fn test_is_rate_limit_error_403_without_header() {
        let headers = HeaderMap::new();
        assert!(!GitHubClient::is_rate_limit_error(
            reqwest::StatusCode::FORBIDDEN,
            &headers
        ));
    }

    #[test]
    fn test_is_rate_limit_error_404() {
        let headers = HeaderMap::new();
        assert!(!GitHubClient::is_rate_limit_error(
            reqwest::StatusCode::NOT_FOUND,
            &headers
        ));
    }

    #[test]
    fn test_should_retry_on_error_rate_limit() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", "60".parse().unwrap());

        let _client = GitHubClient::new("https://github.com", "owner/repo", None).unwrap();
        let result =
            GitHubClient::should_retry_on_error(reqwest::StatusCode::TOO_MANY_REQUESTS, &headers);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(60));
    }

    #[test]
    fn test_should_retry_on_error_server_error() {
        let headers = HeaderMap::new();
        let _client = GitHubClient::new("https://github.com", "owner/repo", None).unwrap();
        let result = GitHubClient::should_retry_on_error(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            &headers,
        );

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(10)); // RETRY_DELAY_SECONDS
    }

    #[test]
    fn test_should_retry_on_error_503() {
        let headers = HeaderMap::new();
        let _client = GitHubClient::new("https://github.com", "owner/repo", None).unwrap();
        let result =
            GitHubClient::should_retry_on_error(reqwest::StatusCode::SERVICE_UNAVAILABLE, &headers);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(10));
    }

    #[test]
    fn test_should_retry_on_error_not_found() {
        let headers = HeaderMap::new();
        let _client = GitHubClient::new("https://github.com", "owner/repo", None).unwrap();
        let result = GitHubClient::should_retry_on_error(reqwest::StatusCode::NOT_FOUND, &headers);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None);
    }

    #[test]
    fn test_should_retry_on_error_bad_request() {
        let headers = HeaderMap::new();
        let _client = GitHubClient::new("https://github.com", "owner/repo", None).unwrap();
        let result =
            GitHubClient::should_retry_on_error(reqwest::StatusCode::BAD_REQUEST, &headers);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None);
    }
}
