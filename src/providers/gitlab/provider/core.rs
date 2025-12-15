use crate::auth::Token;
use crate::error::Result;
use crate::providers::gitlab::client::GitLabClient;

pub struct GitLabProvider {
    pub client: GitLabClient,
    pub project_id: String,
}

impl GitLabProvider {
    pub fn new(base_url: &str, project_id: String, token: Option<Token>) -> Result<Self> {
        let client = GitLabClient::new(base_url, token)?;

        Ok(Self { client, project_id })
    }
}
