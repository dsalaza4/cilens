use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

use super::styling::{bright, bright_green, bright_yellow};

/// Progress tracking for multi-phase CI/CD insights collection.
///
/// Manages a spinner-based progress indicator through two phases:
/// 1. Fetching jobs from the CI provider
/// 2. Processing and analyzing the collected data
pub struct PhaseProgress {
    pb: ProgressBar,
}

impl PhaseProgress {
    /// Starts phase 1: Fetching jobs.
    ///
    /// Creates and displays a progress spinner for job fetching.
    #[must_use]
    pub fn start_phase_1() -> Self {
        eprintln!("{}  {}", bright("⚙️"), bright("Phases").underlined());
        let pb = create_spinner(bright_yellow("Phase 1/2: Fetching jobs").to_string());
        Self { pb }
    }

    /// Finishes phase 1 and starts phase 2: Processing insights.
    ///
    /// Marks job fetching as complete and starts processing progress.
    #[must_use]
    pub fn finish_phase_1_start_phase_2(self) -> Self {
        self.pb
            .finish_with_message(bright_green("Phase 1/2: Jobs fetched ✓").to_string());
        let pb = create_spinner(bright_yellow("Phase 2/2: Processing insights").to_string());
        Self { pb }
    }

    /// Finishes phase 2: Processing complete.
    ///
    /// Marks all phases as complete and clears the progress indicator.
    pub fn finish_phase_2(self) {
        self.pb
            .finish_with_message(bright_green("Phase 2/2: Insights processed ✓").to_string());
        eprintln!("\n");
    }
}

fn create_spinner(message: String) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_draw_target(ProgressDrawTarget::stderr());
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("  {msg} {spinner}")
            .unwrap(),
    );
    pb.set_message(message);
    pb.enable_steady_tick(std::time::Duration::from_millis(100));
    pb
}
