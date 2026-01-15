use anyhow::Result;
use chrono::{DateTime, NaiveDate, Utc};
use clap::{value_parser, Parser, Subcommand};
use log::info;

use crate::auth::Token;
use crate::providers::{github, GitHubProvider, GitLabProvider, JobCache};

/// Command-line interface for `CILens`.
///
/// Provides access to CI/CD insights from various providers (currently GitLab).
/// Supports both JSON output for programmatic use and human-readable summaries
/// for quick analysis.
#[derive(Parser)]
#[command(name = "cilens")]
#[command(author, version, about = "CI/CD Insights Tool", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(
        short,
        long,
        global = true,
        default_value_t = false,
        help = "Output JSON instead of human-readable summary"
    )]
    json: bool,

    #[arg(
        short,
        long,
        global = true,
        default_value_t = false,
        help = "Pretty-print JSON output (only works with --json)"
    )]
    pretty: bool,
}

/// Configuration for GitLab insights collection.
///
/// Encapsulates all parameters needed to fetch and analyze GitLab pipeline data.
struct GitLabConfig<'a> {
    token: Option<&'a String>,
    base_url: &'a str,
    project_path: &'a str,
    limit: usize,
    ref_: Option<&'a str>,
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
    min_type_percentage: u8,
    no_cache: bool,
    clear_cache: bool,
}

/// Configuration for GitHub insights collection.
///
/// Encapsulates all parameters needed to fetch and analyze GitHub Actions workflow data.
struct GitHubConfig<'a> {
    token: Option<&'a String>,
    base_url: &'a str,
    owner: &'a str,
    repo: &'a str,
    limit: usize,
    branch: Option<&'a str>,
    created_after: Option<DateTime<Utc>>,
    created_before: Option<DateTime<Utc>>,
    min_type_percentage: u8,
    no_cache: bool,
    clear_cache: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Collect CI/CD insights from GitLab
    Gitlab {
        #[arg(help = "GitLab project path (e.g., 'group/project')")]
        project_path: String,

        #[arg(
            long,
            env = "GITLAB_TOKEN",
            help = "GitLab personal access token (or set GITLAB_TOKEN env var)"
        )]
        token: Option<String>,

        #[arg(
            long,
            default_value = "https://gitlab.com",
            help = "GitLab instance base URL"
        )]
        base_url: String,

        #[arg(
            long,
            default_value_t = 500,
            help = "Maximum number of pipelines to fetch"
        )]
        limit: usize,

        #[arg(long, name = "ref", help = "Filter pipelines by git ref (branch/tag)")]
        ref_: Option<String>,

        #[arg(long, help = "Fetch pipelines since this date (YYYY-MM-DD)")]
        since: Option<NaiveDate>,

        #[arg(long, help = "Fetch pipelines until this date (YYYY-MM-DD)")]
        until: Option<NaiveDate>,

        #[arg(
            long,
            default_value_t = 1,
            help = "Minimum percentage for pipeline type filtering (0-100)",
            value_parser = value_parser!(u8).range(0..=100),
        )]
        min_type_percentage: u8,

        #[arg(long, help = "Disable job caching (fetch all data fresh)")]
        no_cache: bool,

        #[arg(long, help = "Clear the job cache before running")]
        clear_cache: bool,
    },

    /// Collect CI/CD insights from GitHub Actions
    Github {
        #[arg(help = "GitHub repository path (e.g., 'owner/repo')")]
        repo_path: String,

        #[arg(
            long,
            env = "GITHUB_TOKEN",
            help = "GitHub personal access token (or set GITHUB_TOKEN env var)"
        )]
        token: Option<String>,

        #[arg(
            long,
            default_value = "https://api.github.com",
            help = "GitHub API base URL"
        )]
        base_url: String,

        #[arg(
            long,
            default_value_t = 100,
            help = "Maximum number of workflow runs to fetch"
        )]
        limit: usize,

        #[arg(long, help = "Filter workflow runs by branch")]
        branch: Option<String>,

        #[arg(
            long,
            help = "Fetch workflow runs created after this date (YYYY-MM-DD)"
        )]
        created_after: Option<NaiveDate>,

        #[arg(
            long,
            help = "Fetch workflow runs created before this date (YYYY-MM-DD)"
        )]
        created_before: Option<NaiveDate>,

        #[arg(
            long,
            default_value_t = 1,
            help = "Minimum percentage for pipeline type filtering (0-100)",
            value_parser = value_parser!(u8).range(0..=100),
        )]
        min_type_percentage: u8,

        #[arg(long, help = "Disable run caching (fetch all data fresh)")]
        no_cache: bool,

        #[arg(long, help = "Clear the run cache before running")]
        clear_cache: bool,
    },
}

impl Cli {
    /// Executes GitLab insights collection with the provided configuration.
    ///
    /// # Arguments
    ///
    /// * `config` - GitLab configuration including authentication, project path, and filters
    ///
    /// # Returns
    ///
    /// `Ok(())` on success, or an error if fetching/processing fails.
    ///
    /// # Behavior
    ///
    /// - If `clear_cache` is true, clears the cache and returns without fetching insights
    /// - Otherwise, fetches pipelines from GitLab and displays results in the requested format
    async fn execute_gitlab(&self, config: GitLabConfig<'_>) -> Result<()> {
        // Handle cache-only operations
        if config.clear_cache {
            JobCache::clear_project_cache(config.project_path)?;
            info!("Cache cleared successfully");
            return Ok(());
        }

        let token = config.token.map(|t| Token::from(t.as_str()));

        let provider = GitLabProvider::new(
            config.base_url,
            config.project_path.to_owned(),
            token,
            !config.no_cache,
        )?;

        // Normal insights collection
        info!(
            "Collecting GitLab insights for project: {}",
            config.project_path
        );
        if config.since.is_some() || config.until.is_some() {
            info!(
                "Date range: {} to {}",
                config
                    .since
                    .map_or_else(|| "beginning".to_string(), |d| d.date_naive().to_string()),
                config
                    .until
                    .map_or_else(|| "now".to_string(), |d| d.date_naive().to_string())
            );
        }

        let insights = provider
            .collect_insights(
                config.limit,
                config.ref_,
                config.since,
                config.until,
                config.min_type_percentage,
            )
            .await?;

        if self.json {
            // JSON output mode
            let json_output = if self.pretty {
                serde_json::to_string_pretty(&insights)?
            } else {
                serde_json::to_string(&insights)?
            };
            println!("{json_output}");
        } else {
            // Summary output mode (default)
            crate::output::print_summary(&insights);
        }

        Ok(())
    }

    /// Executes GitHub insights collection with the provided configuration.
    ///
    /// # Arguments
    ///
    /// * `config` - GitHub configuration including authentication, repository, and filters
    ///
    /// # Returns
    ///
    /// `Ok(())` on success, or an error if fetching/processing fails.
    ///
    /// # Behavior
    ///
    /// - If `clear_cache` is true, clears the cache and returns without fetching insights
    /// - Otherwise, fetches workflow runs from GitHub and displays results in the requested format
    async fn execute_github(&self, config: GitHubConfig<'_>) -> Result<()> {
        // Handle cache-only operations
        if config.clear_cache {
            let repo_path = format!("{}/{}", config.owner, config.repo);
            github::JobCache::clear_project_cache(&repo_path)?;
            info!("Cache cleared successfully");
            return Ok(());
        }

        let token = config.token.map(|t| Token::from(t.as_str()));

        let provider = GitHubProvider::new(
            config.base_url,
            config.owner.to_owned(),
            config.repo.to_owned(),
            token,
            !config.no_cache,
        )?;

        // Normal insights collection
        info!(
            "Collecting GitHub Actions insights for repository: {}/{}",
            config.owner, config.repo
        );
        if config.created_after.is_some() || config.created_before.is_some() {
            info!(
                "Date range: {} to {}",
                config
                    .created_after
                    .map_or_else(|| "beginning".to_string(), |d| d.date_naive().to_string()),
                config
                    .created_before
                    .map_or_else(|| "now".to_string(), |d| d.date_naive().to_string())
            );
        }

        let insights = provider
            .collect_insights(
                config.limit,
                config.branch,
                config.created_after,
                config.created_before,
                config.min_type_percentage,
            )
            .await?;

        if self.json {
            // JSON output mode
            let json_output = if self.pretty {
                serde_json::to_string_pretty(&insights)?
            } else {
                serde_json::to_string(&insights)?
            };
            println!("{json_output}");
        } else {
            // Summary output mode (default)
            crate::output::print_summary(&insights);
        }

        Ok(())
    }

    /// Executes the CLI command.
    ///
    /// Parses the subcommand and routes to the appropriate handler.
    ///
    /// # Returns
    ///
    /// `Ok(())` on successful execution, or an error if the command fails.
    pub async fn execute(&self) -> Result<()> {
        match &self.command {
            Commands::Gitlab {
                token,
                base_url,
                project_path,
                limit,
                ref_,
                since,
                until,
                min_type_percentage,
                no_cache,
                clear_cache,
            } => {
                // Convert NaiveDate to DateTime<Utc> (start of day UTC)
                let since_datetime =
                    since.map(|date| date.and_hms_opt(0, 0, 0).expect("Valid time").and_utc());

                // For until, use end of day (23:59:59) to be inclusive
                let until_datetime =
                    until.map(|date| date.and_hms_opt(23, 59, 59).expect("Valid time").and_utc());

                let config = GitLabConfig {
                    token: token.as_ref(),
                    base_url,
                    project_path,
                    limit: *limit,
                    ref_: ref_.as_deref(),
                    since: since_datetime,
                    until: until_datetime,
                    min_type_percentage: *min_type_percentage,
                    no_cache: *no_cache,
                    clear_cache: *clear_cache,
                };

                self.execute_gitlab(config).await
            }
            Commands::Github {
                token,
                base_url,
                repo_path,
                limit,
                branch,
                created_after,
                created_before,
                min_type_percentage,
                no_cache,
                clear_cache,
            } => {
                // Parse owner/repo from repo_path
                let parts: Vec<&str> = repo_path.split('/').collect();
                if parts.len() != 2 {
                    return Err(anyhow::anyhow!(
                        "Invalid repo_path format. Expected 'owner/repo', got: {repo_path}"
                    ));
                }
                let owner = parts[0];
                let repo = parts[1];

                // Convert NaiveDate to DateTime<Utc> (start of day UTC)
                let created_after_datetime = created_after
                    .map(|date| date.and_hms_opt(0, 0, 0).expect("Valid time").and_utc());

                // For created_before, use end of day (23:59:59) to be inclusive
                let created_before_datetime = created_before
                    .map(|date| date.and_hms_opt(23, 59, 59).expect("Valid time").and_utc());

                let config = GitHubConfig {
                    token: token.as_ref(),
                    base_url,
                    owner,
                    repo,
                    limit: *limit,
                    branch: branch.as_deref(),
                    created_after: created_after_datetime,
                    created_before: created_before_datetime,
                    min_type_percentage: *min_type_percentage,
                    no_cache: *no_cache,
                    clear_cache: *clear_cache,
                };

                self.execute_github(config).await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_github_command_parsing() {
        let args = vec!["cilens", "github", "owner/repo", "--token", "ghp_test"];
        let cli = Cli::parse_from(args);

        match cli.command {
            Commands::Github {
                repo_path,
                token,
                base_url,
                limit,
                branch,
                created_after,
                created_before,
                min_type_percentage,
                no_cache,
                clear_cache,
            } => {
                assert_eq!(repo_path, "owner/repo");
                assert_eq!(token, Some("ghp_test".to_string()));
                assert_eq!(base_url, "https://api.github.com");
                assert_eq!(limit, 100);
                assert_eq!(branch, None);
                assert_eq!(created_after, None);
                assert_eq!(created_before, None);
                assert_eq!(min_type_percentage, 1);
                assert!(!no_cache);
                assert!(!clear_cache);
            }
            Commands::Gitlab { .. } => panic!("Expected Github command variant"),
        }
    }
}
