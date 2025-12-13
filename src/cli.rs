use anyhow::Result;
use clap::{Parser, Subcommand};
use log::info;
use std::path::PathBuf;

use crate::providers::gitlab::GitLabProvider;

#[derive(Parser)]
#[command(name = "cilens")]
#[command(author, version, about = "CI/CD Insights Tool", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Output file path (defaults to stdout)
    #[arg(short, long, global = true)]
    output: Option<PathBuf>,

    /// Pretty print JSON output
    #[arg(short, long, global = true, default_value_t = false)]
    pretty: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Collect insights from GitLab
    Gitlab {
        /// GitLab API token (optional, required for private projects)
        #[arg(short, long, env = "GITLAB_TOKEN")]
        token: Option<String>,

        /// GitLab instance URL
        #[arg(short, long, default_value = "https://gitlab.com")]
        url: String,

        /// Project ID or path (e.g., "group/project")
        #[arg(short = 'P', long)]
        project: String,

        /// Number of pipelines to analyze
        #[arg(short, long, default_value_t = 20)]
        limit: usize,

        /// Branch name to filter pipelines (optional)
        #[arg(short, long)]
        branch: Option<String>,
    },
}

impl Cli {
    pub async fn execute(&self) -> Result<()> {
        match &self.command {
            Commands::Gitlab {
                token,
                url,
                project,
                limit,
                branch,
            } => {
                info!("Collecting GitLab insights for project: {}", project);

                let token_value = token.clone().unwrap_or_default();
                let provider = GitLabProvider::new(url.clone(), project.clone(), token_value)?;
                let insights = provider
                    .collect_insights(project, *limit, branch.as_deref())
                    .await?;

                // Serialize to JSON
                let json_output = if self.pretty {
                    serde_json::to_string_pretty(&insights)?
                } else {
                    serde_json::to_string(&insights)?
                };

                // Write to output
                if let Some(output_path) = &self.output {
                    std::fs::write(output_path, json_output)?;
                    info!("Insights written to: {}", output_path.display());
                } else {
                    println!("{}", json_output);
                }

                Ok(())
            }
        }
    }
}
