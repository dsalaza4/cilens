# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

CILens is a Rust CLI tool for collecting and analyzing CI/CD pipeline insights from GitLab and GitHub Actions. It uses GraphQL (GitLab) and REST API (GitHub) to fetch pipeline data, groups pipelines by job signature, calculates percentile-based metrics (P50/P95/P99), tracks job reliability (flakiness/failures), and provides actionable optimization insights.

## Essential Commands

### Development
```bash
# Build the project
cargo build

# Run the CLI locally (GitLab)
cargo run -- gitlab group/project

# Run the CLI locally (GitHub Actions)
cargo run -- github owner/repo

# Run with GitLab token
export GITLAB_TOKEN="glpat-your-token"
cargo run -- gitlab group/project

# Run with GitHub token
export GITHUB_TOKEN="ghp_your-token"
cargo run -- github owner/repo

# Format code (required before commits)
cargo fmt

# Run pedantic linting (zero warnings required)
cargo lint

# Run all tests
cargo test

# Run specific test
cargo test test_name

# Run tests with output
cargo test -- --nocapture

# Configure commit message template
git config commit.template .gitmessage
```

### Release
```bash
# Releases use cargo-dist - see .github/workflows/release.yml
# Create a git tag to trigger release workflow
git tag -a v0.x.x -m "Release v0.x.x"
git push --tags
```

## High-Level Architecture

### Core Design Principles
1. **Percentiles over averages** - Use P50/P95/P99 for realistic performance expectations; averages hide outliers
2. **Time-to-feedback matters** - Track when developers get results, not just job duration
3. **Smart caching** - Completed pipelines are immutable; cache them aggressively (90%+ speedup on subsequent runs)
4. **Pipeline type clustering** - Group pipelines by job signature (exact job set match) to get meaningful statistics
5. **Flakiness detection** - Track jobs that fail then succeed on retry (intermittent failures)

### Module Organization
```
src/
├── main.rs           - Entry point, async runtime setup
├── cli.rs            - Clap command-line interface
├── auth.rs           - Token wrapper with secure Debug impl
├── error.rs          - Error types (thiserror)
├── insights.rs       - Provider-agnostic domain model (CIInsights, JobMetrics, PipelineType)
├── output/           - Display layer (summary tables, JSON, progress spinner)
└── providers/
    ├── gitlab/       - GitLab GraphQL provider
    │   ├── provider.rs         - Main GitLabProvider entry point
    │   ├── client/             - GraphQL client (pipelines.graphql, schema.json)
    │   │   ├── core.rs         - HTTP client with retry logic
    │   │   └── pipelines.rs    - GraphQL query execution
    │   ├── pipeline_types.rs   - Group pipelines by job signature
    │   ├── pipeline_metrics.rs - Calculate P50/P95/P99 for pipeline types
    │   ├── job_metrics.rs      - Calculate time-to-feedback per job
    │   ├── job_reliability.rs  - Track failures and flakiness
    │   ├── cache.rs            - Persistent JSON cache
    │   ├── types.rs            - GitLab-specific data models
    │   └── links.rs            - Generate GitLab URLs for pipelines/jobs
    └── github/       - GitHub REST API provider
        ├── provider.rs      - Main GitHubProvider entry point
        ├── client/          - REST API client
        │   └── core.rs      - HTTP client with retry logic
        ├── types.rs         - GitHub-specific data models (WorkflowRun, Job)
        └── cache.rs         - Persistent JSON cache
```

### Data Flow
1. **Fetch** - GraphQL queries fetch pipelines and jobs (with retry logic for rate limits/failures)
2. **Cache** - Check cache for completed pipelines; save fetched jobs to cache
3. **Transform** - Group pipelines by job signature, filter rare types (<1% by default)
4. **Calculate** - Compute percentiles (P50/P95/P99) for pipeline duration, job duration, time-to-feedback
5. **Analyze** - Track job reliability (failure rate, flakiness rate, predecessors)
6. **Display** - Output JSON or human-readable summary with color-coded tables

### Key Algorithms

**Pipeline Type Clustering** (`pipeline_types.rs::group_pipeline_types`):
- Groups pipelines by sorted job names (job signature)
- Filters out rare pipeline types below threshold (default 1%)
- Assigns unique IDs to each pipeline type (e.g., "type-0", "type-1")

**Time-to-Feedback** (`job_metrics.rs::calculate_finish_time`):
- Recursively calculates when each job completes based on dependencies
- Accounts for job duration + time waiting for predecessors
- Shows developers "when will I get results" instead of just "how long did it run"

**Flakiness Detection** (`job_reliability.rs::calculate_job_reliability`):
- Tracks jobs that failed then succeeded on retry (flaky)
- Distinguishes from jobs that failed and stayed failed (catching real bugs)
- Provides links to specific flaky job runs for investigation

**Percentile Calculation** (`pipeline_metrics.rs::calculate_percentiles`):
- Uses standard percentile formula on sorted durations
- Returns P50 (median), P95 (planning metric), P99 (outlier detection)

### GraphQL Integration
- Schema: `src/providers/gitlab/client/schema.json` (GitLab GraphQL schema)
- Queries: `src/providers/gitlab/client/pipelines.graphql` (FetchPipelines, FetchPipelineJobs)
- Code generation: `graphql_client` crate generates Rust types from schema + queries

### Caching Strategy
- **Location**: Platform-specific cache directories (via `dirs` crate)
  - Linux: `~/.cache/cilens/gitlab/`
  - macOS: `~/Library/Caches/cilens/gitlab/`
  - Windows: `%LOCALAPPDATA%\cilens\gitlab\`
- **Key**: Pipeline ID
- **Value**: Job data (name, status, duration, needs, stage, retried)
- **Immutability**: Only caches SUCCESS/FAILED pipelines (not running/canceled)
- **Performance**: 90%+ speedup on subsequent runs due to cache hits

### Testing Philosophy
- **Prefer unit tests** - Faster, simpler, more reproducible than integration tests
- **Inline tests** - Tests live in `#[cfg(test)]` modules within each source file
- **Test fixtures** - Helper functions create test data (e.g., `create_test_pipeline()`)
- **Zero warnings** - Pedantic clippy (`cargo lint`) catches issues early

## Commit Message Format

Follow [Conventional Commits](https://www.conventionalcommits.org/):
```
type(scope): description

[optional body]
```

**Types**: `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`, `build`, `perf`, `revert`

**Scopes**: `core`, `ci`, `gitlab`, `github`, `dist`

**Examples**:
- `feat(gitlab): add support for merge request pipelines`
- `feat(github): add REST API client with authentication`
- `fix(core): handle corrupted cache files gracefully`
- `docs(core): update installation instructions`
- `chore(ci): update GitHub Actions workflow`

## Extension Points

### GitHub vs GitLab Differences
- **GitHub uses REST API** (workflow runs API) vs GitLab's GraphQL
- **GitHub job dependencies**: `needs` field is `None` (GitHub API doesn't provide this data directly - would need workflow YAML parsing)
- **GitHub timestamps**: ISO 8601 format, need to calculate durations from start/end times
- **Both transform to same `CIInsights` domain model** for consistent analysis
- **Cache locations**: Both use `~/.cache/cilens/{provider}/` (Linux) or equivalent platform dirs

### Adding a New Provider (e.g., Jenkins, CircleCI)
1. Create `src/providers/{provider}/`
2. Implement data fetching (REST/GraphQL client)
3. Transform to `insights::CIInsights` domain model (provider-agnostic)
4. Add CLI subcommand in `cli.rs`: `cilens {provider} ...`

### Adding New Metrics
1. Add fields to `insights.rs` domain model
2. Calculate in `pipeline_metrics.rs` or `job_metrics.rs`
3. Display in `output/summary.rs` (human-readable) or serialize to JSON

## Performance Characteristics
- **First run**: ~30-60 seconds for 500 pipelines (network-bound)
- **Cached run**: ~5 seconds for 500 pipelines (90%+ cache hit rate)
- **Concurrency**: Max 500 parallel requests (to avoid overwhelming GitLab API)
- **Retry logic**: Up to 30 retries with 10s delay (handles rate limits, 5xx errors)
- **Memory**: ~50-100MB peak (all data in memory during processing)
