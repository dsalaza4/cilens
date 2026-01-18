# Architecture

CILens is a CLI tool for collecting and analyzing CI/CD pipeline insights. This document explains the high-level design.

## Design Philosophy

### Core Principles

1. **Simplicity first** - Search for ways to make things simple while generating maximum value. Avoid over-engineering.
2. **Unit tests win** - Unit tests are faster, simpler, and more reproducible than integration tests. Prefer them.
3. **Minimum configurability** - Correctly opinionated tool > very configurable tool. Make good default choices.
4. **Simple tools that work** - cargo-dist for releases, nix flakes for reproducible builds. No complex CI/CD pipelines.
5. **Strictest linting** - `cargo lint` = pedantic clippy. Catch issues early.

### Domain Principles

1. **Percentiles over averages** - P50/P95/P99 give realistic expectations; averages hide outliers
2. **Time-to-feedback matters** - Developers care about "when will I get results", not just "how long did the job run"
3. **Detect retries automatically** - Track retries and intermittent failures
4. **Optimize the critical path** - Show job dependencies to identify blockers
5. **Cache aggressively** - Immutable jobs (SUCCESS/FAILED) don't change; cache them indefinitely

## Module Structure

```text
cilens/
├── cli.rs              # Command-line interface (clap)
├── auth.rs             # Token wrapper with secure Debug impl
├── error.rs            # Error types (thiserror)
├── insights.rs         # Domain model (CIInsights, JobMetrics, etc.)
├── output/             # Display layer
│   ├── summary.rs      # Human-readable tables
│   ├── progress.rs     # 2-phase progress spinner
│   ├── tables.rs       # Color-coded table helpers
│   └── styling.rs      # Terminal styling functions
└── providers/
    └── gitlab/
        ├── provider.rs  # Main entry point
        ├── client/      # GraphQL API client
        ├── jobs.rs      # Job metrics calculation and URL generation
        ├── cache.rs     # Persistent job cache
        └── types.rs     # GitLab-specific data models
```

## Data Flow

```text
1. CLI parses arguments
   └─> GitLabProvider.collect_insights()

2. Fetch jobs (GraphQL)
   ├─> Check cache for job data
   ├─> Fetch missing jobs (GraphQL, paginated)
   ├─> Merge cached + fresh jobs
   └─> Save to cache

3. Transform GitLab data → Domain model
   ├─> Aggregate jobs by name (jobs.rs)
   ├─> Filter jobs below minimum execution threshold
   ├─> Calculate job metrics (jobs.rs)
   │   ├─> Duration percentiles (P50/P95/P99)
   │   ├─> Time-to-feedback percentiles (P50/P95/P99)
   │   ├─> Extract predecessor dependencies
   │   └─> Calculate reliability (retry rate, failure rate, success rate)
   └─> Return CIInsights

4. Display results
   ├─> JSON output (--json or --json-pretty)
   └─> Human-readable summary (output/summary.rs)
```

## Key Design Decisions

### 1. Percentiles (P50/P95/P99)

**Why:** Averages are misleading for skewed distributions. A job that takes 5min 99% of the time but 60min 1% of the time has a 5.5min average (useless for planning).

**Where:** `jobs.rs::calculate_percentiles()`

### 2. Time-to-Feedback vs Duration

**Why:** Developers care about "when do I get feedback" more than "how long did the job run". A 2-minute job that waits 10 minutes for dependencies has 12min time-to-feedback.

**Where:** `jobs.rs::calculate_job_metrics()` - calculates time from pipeline start (`created_at`) to job completion (`finished_at`).

**Important:** Time-to-feedback is calculated only from successful first-try jobs. Retried jobs have `created_at` set to the retry trigger time, not the original pipeline start time, making them unsuitable for this metric.

### 3. Retry Detection

**Why:** Intermittent failures waste CI resources. Jobs with high retry rates need fixing.

**Where:** `jobs.rs::calculate_job_reliability()` - tracks retried executions using GitLab's `retried` flag.

**Design:**

- `retry_rate`: Percentage of executions that were retries
- `retried_executions`: Count and clickable URLs to investigate
- Distinguished from `failure_rate` which tracks jobs that failed and stayed failed

### 4. Smart Caching

**Why:** Completed jobs don't change. Fetching jobs is expensive. Cache eliminates redundant API calls on subsequent runs.

**Where:** `cache.rs` - per-project JSON cache in platform-specific cache directory.

**Design:**

- Cache key: job ID
- Cache value: complete job data
- Immutable: SUCCESS and FAILED jobs cached indefinitely
- Merging: fresh jobs merged with cached jobs, deduplicated by ID
- Platform-aware: uses OS-specific cache directories

### 5. Noise Filtering

**Why:** Rarely-executed jobs add noise to the analysis. Most insights come from frequently-run jobs.

**Where:** `jobs.rs::calculate_job_metrics()` - filters jobs below `min_executions_percentage` (default 0.2%).

**Design:**

- Calculates total executions across all jobs
- Filters out jobs representing less than threshold percentage
- Configurable via `--min-executions-percentage` flag

## Extension Points

### Adding a New Provider (e.g., GitHub Actions)

1. Create `providers/github/`
2. Implement data fetching (REST/GraphQL)
3. Transform to `CIInsights` domain model
4. Add CLI subcommand `cilens github ...`

**Key:** The `insights.rs` domain model is provider-agnostic. New providers just need to produce `CIInsights`.

### Adding New Metrics

1. Add fields to `insights.rs` (e.g., `cost_per_pipeline: f64`)
2. Calculate in `pipeline_metrics.rs` or `job_metrics.rs`
3. Display in `output/summary.rs`

### Adding Export Formats

1. Domain model already has `#[derive(Serialize)]`
2. Add format in `cli.rs::execute_gitlab()`:
   - CSV: serialize to CSV
   - HTML: template engine
   - Prometheus: `/metrics` endpoint

## Performance Characteristics

- **First run:** ~30-60 seconds for 500 pipelines (network bound)
- **Cached run:** ~5 seconds for 500 pipelines (90%+ cache hit rate)
- **Concurrency:** Max 500 parallel requests (configurable)
- **Retry logic:** Up to 30 retries with 10s delay (handles rate limits)
- **Memory:** ~50-100MB peak (all data in memory during processing)

## Testing Strategy

- **Unit tests:** Inline with `#[cfg(test)]` (100 tests)
- **Test fixtures:** Helper functions in each test module
