use anyhow::Result;
use clap::{Parser, Subcommand};
use log::info;

use crate::auth::Token;
use crate::providers::{GitLabProvider, JobCache};

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
        conflicts_with = "json_pretty",
        help = "Output compact JSON instead of human-readable summary"
    )]
    json: bool,

    #[arg(
        long,
        global = true,
        conflicts_with = "json",
        help = "Output pretty-printed JSON instead of human-readable summary"
    )]
    json_pretty: bool,
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

        #[arg(long, default_value_t = 2000, help = "Maximum number of jobs to fetch")]
        limit: usize,

        #[arg(long, help = "Clear the job cache before running")]
        clear_cache: bool,

        #[arg(
            long,
            default_value_t = 0.2,
            help = "Minimum execution percentage to include a job (e.g., 0.2 means jobs must have at least 0.2% of total executions)"
        )]
        min_executions_percentage: f64,
    },
}

impl Cli {
    /// Executes GitLab insights collection.
    async fn execute_gitlab(
        &self,
        project_path: &str,
        base_url: &str,
        token: Option<&String>,
        limit: usize,
        clear_cache: bool,
        min_executions_percentage: f64,
    ) -> Result<()> {
        if clear_cache {
            JobCache::clear(project_path)?;
            info!("Cache cleared successfully");
            return Ok(());
        }

        let token = token.map(|t| Token::from(t.as_str()));
        let provider = GitLabProvider::new(base_url, project_path, token)?;

        info!("Collecting GitLab insights for project: {project_path}");
        let insights = provider
            .collect_insights(limit, min_executions_percentage)
            .await?;

        if self.json_pretty {
            let json_output = serde_json::to_string_pretty(&insights)?;
            println!("{json_output}");
        } else if self.json {
            let json_output = serde_json::to_string(&insights)?;
            println!("{json_output}");
        } else {
            crate::output::print_summary(&insights);
        }

        Ok(())
    }

    /// Executes the CLI command.
    pub async fn execute(&self) -> Result<()> {
        match &self.command {
            Commands::Gitlab {
                project_path,
                base_url,
                token,
                limit,
                clear_cache,
                min_executions_percentage,
            } => {
                self.execute_gitlab(
                    project_path,
                    base_url,
                    token.as_ref(),
                    *limit,
                    *clear_cache,
                    *min_executions_percentage,
                )
                .await
            }
        }
    }
}
