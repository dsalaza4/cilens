use std::fmt::Write;

use crate::insights::{CIInsights, JobMetrics};
use comfy_table::{Cell, Color as TableColor};

use super::styling::{bright, bright_yellow, cyan, dim};
use super::tables::{
    color_coded_duration_cell, color_coded_failure_cell, color_coded_retry_cell, create_table,
};

/// Prints a human-readable summary of CI/CD insights to stdout.
///
/// Displays color-coded tables showing:
/// - Overview: Project name, job counts, collected date
/// - Top 10 Slowest Jobs: Jobs with highest P50 (median) time-to-feedback
/// - Top 10 Failing Jobs: Most unreliable jobs by failure rate
/// - Top 10 Retried Jobs: Most intermittent jobs by retry rate
/// - Next Steps: Actionable recommendations
///
/// Color coding:
/// - Green: Good values (failures <25%, retries <5%, durations ≤10min)
/// - Yellow: Warning (failures 25-50%, retries 5-10%, durations 10-15min)
/// - Red: Critical (failures ≥50%, retries ≥10%, durations >15min)
pub fn print_summary(insights: &CIInsights) {
    println!("{}", render_summary(insights));
}

// Helper functions

fn create_cyan_header(labels: &[&str]) -> Vec<Cell> {
    labels
        .iter()
        .map(|label| Cell::new(*label).fg(TableColor::Cyan))
        .collect()
}

fn add_section_header(output: &mut String, emoji: &str, title: &str) {
    let _ = writeln!(output, "{} {}", bright(emoji), bright(title).underlined());
}

fn sort_jobs_by<'a, F>(jobs: &[&'a JobMetrics], compare: F) -> Vec<&'a JobMetrics>
where
    F: Fn(&JobMetrics, &JobMetrics) -> std::cmp::Ordering,
{
    let mut sorted = jobs.to_vec();
    sorted.sort_by(|a, b| compare(a, b));
    sorted
}

fn format_critical_path(job: &JobMetrics) -> String {
    if job.predecessors.is_empty() {
        "None".to_string()
    } else {
        job.predecessors.join("\n")
    }
}

#[allow(clippy::too_many_lines, clippy::format_push_string)]
fn render_summary(insights: &CIInsights) -> String {
    let mut output = String::new();

    // Overview section
    add_section_header(&mut output, "📊", "Overview");

    let total_executions: usize = insights.jobs.iter().map(|job| job.total_executions).sum();

    output.push_str(&format!(
        "  {} {}\n  {} {}\n  {} {}\n  {} {}\n\n",
        dim("Project:"),
        cyan(&insights.project),
        dim("Unique jobs:"),
        bright_yellow(insights.total_jobs),
        dim("Analyzed executions:"),
        bright_yellow(total_executions),
        dim("Analysis date:"),
        dim(insights.collected_at.format("%Y-%m-%d %H:%M UTC"))
    ));

    if insights.jobs.is_empty() {
        output.push_str(&format!("{}\n", bright_yellow("No job data found.")));
        return output;
    }

    // Top 10 Slowest Jobs (by time-to-feedback P50)
    add_section_header(&mut output, "🐌", "Top 10 Slowest Jobs");

    let slowest_jobs = sort_jobs_by(&insights.jobs.iter().collect::<Vec<_>>(), |a, b| {
        b.time_to_feedback_p50
            .partial_cmp(&a.time_to_feedback_p50)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut slow_table = create_table();
    slow_table.set_header(create_cyan_header(&[
        "Job",
        "P50 Feedback",
        "P50 Duration",
        "Total Executions",
        "Critical Path",
    ]));

    for job in slowest_jobs.iter().take(10) {
        let feedback_cell = color_coded_duration_cell(job.time_to_feedback_p50);
        let duration_cell = color_coded_duration_cell(job.duration_p50);
        let total_executions_cell = Cell::new(job.total_executions);
        let critical_path_cell = Cell::new(format_critical_path(job));

        slow_table.add_row(vec![
            Cell::new(&job.name),
            feedback_cell,
            duration_cell,
            total_executions_cell,
            critical_path_cell,
        ]);
    }

    output.push_str(&format!("{slow_table}\n\n"));

    // Top 10 Failing Jobs
    add_section_header(&mut output, "❌", "Top 10 Failing Jobs");

    let failing_jobs = sort_jobs_by(&insights.jobs.iter().collect::<Vec<_>>(), |a, b| {
        b.failure_rate
            .partial_cmp(&a.failure_rate)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut fail_table = create_table();
    fail_table.set_header(create_cyan_header(&[
        "Job",
        "Failure %",
        "Total Executions",
        "Critical Path",
    ]));

    for job in failing_jobs.iter().take(10) {
        let failure_cell = color_coded_failure_cell(job.failure_rate);
        let total_executions_cell = Cell::new(job.total_executions);
        let critical_path_cell = Cell::new(format_critical_path(job));

        fail_table.add_row(vec![
            Cell::new(&job.name),
            failure_cell,
            total_executions_cell,
            critical_path_cell,
        ]);
    }

    output.push_str(&format!("{fail_table}\n\n"));

    // Top 10 Retried Jobs
    add_section_header(&mut output, "⚡", "Top 10 Retried Jobs");

    let retried_jobs = sort_jobs_by(&insights.jobs.iter().collect::<Vec<_>>(), |a, b| {
        b.retry_rate
            .partial_cmp(&a.retry_rate)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut retry_table = create_table();
    retry_table.set_header(create_cyan_header(&[
        "Job",
        "Retry %",
        "Total Executions",
        "Critical Path",
    ]));

    for job in retried_jobs.iter().take(10) {
        let retry_cell = color_coded_retry_cell(job.retry_rate);
        let total_executions_cell = Cell::new(job.total_executions);
        let critical_path_cell = Cell::new(format_critical_path(job));

        retry_table.add_row(vec![
            Cell::new(&job.name),
            retry_cell,
            total_executions_cell,
            critical_path_cell,
        ]);
    }

    output.push_str(&format!("{retry_table}\n\n"));

    // Next Steps
    add_section_header(&mut output, "💡", "Next Steps");

    output.push_str(&format!(
        "  {} {}\n",
        dim("1."),
        "Optimize slowest jobs to reduce overall feedback time"
    ));
    output.push_str(&format!(
        "  {} {}\n",
        dim("2."),
        "Investigate failing jobs with high failure rates"
    ));
    output.push_str(&format!(
        "  {} {}\n",
        dim("3."),
        "Fix retried jobs to improve reliability"
    ));
    output.push_str(&format!(
        "  {} {}\n",
        dim("4."),
        "Focus on critical path jobs for maximum impact"
    ));

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::insights::JobCountWithLinks;
    use chrono::Utc;

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    fn create_test_job(name: &str, failure_rate: f64, retry_rate: f64) -> JobMetrics {
        JobMetrics {
            name: name.to_string(),
            duration_p50: 60.0,
            duration_p95: 120.0,
            duration_p99: 180.0,
            time_to_feedback_p50: 100.0,
            time_to_feedback_p95: 200.0,
            time_to_feedback_p99: 300.0,
            predecessors: vec![],
            retry_rate,
            retried_executions: JobCountWithLinks {
                count: (retry_rate * 10.0) as usize,
                links: vec![],
            },
            failed_executions: JobCountWithLinks {
                count: (failure_rate * 10.0) as usize,
                links: vec![],
            },
            success_rate: 100.0 - failure_rate - retry_rate,
            successful_executions: JobCountWithLinks {
                count: ((100.0 - failure_rate - retry_rate) * 10.0) as usize,
                links: vec![],
            },
            failure_rate,
            total_executions: 100,
        }
    }

    #[test]
    fn test_render_summary_basic() {
        let insights = CIInsights {
            provider: "GitLab".to_string(),
            project: "group/project".to_string(),
            collected_at: Utc::now(),
            total_jobs: 3,
            jobs: vec![
                create_test_job("build", 10.0, 2.0),
                create_test_job("test", 20.0, 5.0),
                create_test_job("deploy", 5.0, 1.0),
            ],
        };

        let output = render_summary(&insights);

        // Check that overview section exists
        assert!(output.contains("Overview"));
        assert!(output.contains("group/project"));
        assert!(output.contains("Unique jobs"));

        // Check that job sections exist
        assert!(output.contains("Top 10 Slowest Jobs"));
        assert!(output.contains("Top 10 Failing Jobs"));
        assert!(output.contains("Top 10 Retried Jobs"));

        // Check that next steps exist
        assert!(output.contains("Next Steps"));
    }

    #[test]
    fn test_render_summary_empty_jobs() {
        let insights = CIInsights {
            provider: "GitLab".to_string(),
            project: "group/project".to_string(),
            collected_at: Utc::now(),
            total_jobs: 0,
            jobs: vec![],
        };

        let output = render_summary(&insights);

        assert!(output.contains("No job data found"));
    }

    #[test]
    fn test_format_critical_path() {
        let job_with_preds = JobMetrics {
            name: "test".to_string(),
            duration_p50: 60.0,
            duration_p95: 120.0,
            duration_p99: 180.0,
            time_to_feedback_p50: 100.0,
            time_to_feedback_p95: 200.0,
            time_to_feedback_p99: 300.0,
            predecessors: vec!["build".to_string(), "lint".to_string()],
            retry_rate: 0.0,
            retried_executions: JobCountWithLinks::default(),
            failed_executions: JobCountWithLinks::default(),
            success_rate: 100.0,
            successful_executions: JobCountWithLinks::default(),
            failure_rate: 0.0,
            total_executions: 100,
        };

        let path = format_critical_path(&job_with_preds);
        assert!(path.contains("build"));
        assert!(path.contains("lint"));
    }
}
