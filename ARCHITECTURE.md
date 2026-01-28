# Architecture

CILens is a CLI tool for collecting and analyzing CI/CD pipeline insights from GitHub Actions and GitLab.

## Design Philosophy

1. **Simplicity first** - Avoid over-engineering. Make things as simple as possible while delivering maximum value.
2. **Percentiles over averages** - P50/P95/P99 give realistic expectations; averages hide outliers
3. **Time-to-feedback matters** - Developers care about "when will I get results", not just "how long did the job run"
4. **Cache aggressively** - Immutable jobs (SUCCESS/FAILED) don't change; cache them indefinitely
5. **Provider-agnostic domain model** - The `insights.rs` types work for any CI/CD system

## High-Level Flow

```text
CLI → Provider (github/gitlab) → Fetch & Cache Jobs → Calculate Metrics → Output (JSON/Summary)
```

1. **Fetch**: Query provider API with pagination and retry logic
2. **Cache**: Merge with existing cache, deduplicate by job ID
3. **Transform**: Aggregate jobs by name, filter noise
4. **Calculate**: Percentiles, retry/failure rates, time-to-feedback
5. **Display**: JSON or human-readable tables

## Key Design Decisions

### Percentiles (P50/P95/P99)

Averages are misleading for skewed distributions. A job that takes 5 minutes 99% of the time but 60 minutes 1% of the time has a 5.5-minute average (useless for planning). Percentiles show the real distribution.

### Time-to-Feedback vs Duration

Developers care about "when do I get feedback" more than "how long did the job run". A 2-minute job that waits 10 minutes for dependencies has 12-minute time-to-feedback. We measure from workflow start to job completion using only successful first-try jobs.

### Retry Detection

Intermittent failures waste CI resources. Jobs with high retry rates need fixing.

- **GitLab**: Uses `retried` flag on jobs
- **GitHub**: Uses `run_attempt > 1` (jobs inherit attempt from workflow run)

### Smart Caching

Completed jobs don't change. We cache SUCCESS and FAILED jobs indefinitely in platform-specific directories, deduplicated by job ID. Fresh API data is merged with cached data on subsequent runs.

### Noise Filtering

Rarely-executed jobs add noise. By default, jobs representing less than 0.2% of total executions are filtered out (configurable via `--min-executions-percentage`).

## Provider Differences

| Feature | GitHub Actions | GitLab |
|---------|----------------|--------|
| **API** | REST (two-phase: runs → jobs) | GraphQL (single query) |
| **Pagination** | 100 per page | 50 per page |
| **Concurrency** | 300 requests | 500 requests |
| **Retry Detection** | `run_attempt > 1` | `retried` flag |
| **Predecessors** | N/A (needs YAML parsing) | `needs` keyword |
| **Time-to-Feedback** | From `workflow_run.run_started_at` | From `pipeline.created_at` |

**GitHub Notes:**
- Matrix jobs treated independently (e.g., "test (ubuntu)", "test (macos)")
- Jobs inherit `run_attempt` from workflow run (not individual job retries)

## Extension Points

**Adding a new provider:** Create `providers/newprovider/`, fetch data from API, transform to `CIInsights`, add CLI subcommand. The domain model is provider-agnostic.

**Adding new metrics:** Add fields to `insights.rs`, calculate in `jobs.rs`, display in `output/summary.rs`.

**Adding export formats:** Domain model has `#[derive(Serialize)]`. Add new format in `cli.rs` (CSV, HTML, Prometheus, etc.).
