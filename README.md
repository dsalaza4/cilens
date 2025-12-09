# CILens - CI/CD Insights Tool

A Rust CLI tool for collecting and analyzing CI/CD insights from GitLab.

## Features

-  **Pipeline Statistics** - Overall metrics and success rates

## Quick Start

```bash
# Get your GitLab token from: https://gitlab.com/-/profile/personal_access_tokens
# Required scope: read_api

export GITLAB_TOKEN="glpat-your-token"

# Analyze a project
cilens gitlab --project "group/project" --limit 20 --pretty
```

## Usage

```bash
# Basic usage
cilens gitlab --project "your/project"

# Save to file
cilens gitlab --project "your/project" --output insights.json --pretty

# Filter by branch
cilens gitlab --project "your/project" --branch main --limit 50

# Self-hosted GitLab
cilens gitlab --url "https://gitlab.example.com" --project "your/project"
```

## Options

**Global:**
- `-o, --output <FILE>` - Output file path (default: stdout)
- `-p, --pretty` - Pretty print JSON

**GitLab:**
- `-t, --token <TOKEN>` - GitLab token (or use `GITLAB_TOKEN` env var)
- `-u, --url <URL>` - GitLab instance URL (default: https://gitlab.com)
- `-P, --project <PROJECT>` - Project ID or path (e.g., "group/project")
- `-l, --limit <LIMIT>` - Number of pipelines to analyze (default: 20)
- `-b, --branch <BRANCH>` - Filter by branch (optional)

## Output Format

```json
{
  "provider": "GitLab",
  "project": "group/project",
  "collected_at": "2024-01-15T10:30:00Z",
  "pipelines_analyzed": 20,
  "pipeline_summary": {
    "total_pipelines": 20,
    "successful_pipelines": 18,
    "failed_pipelines": 2,
    "pipeline_success_rate": 90.0,
    "average_pipeline_duration_seconds": 600.5,
    "total_jobs_analyzed": 120
  }
}
```
