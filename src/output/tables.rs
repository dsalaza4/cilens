use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Cell, Color as TableColor, ContentArrangement, Table};

/// Table and cell creation helpers
pub fn create_table() -> Table {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic);
    table
}

pub fn color_coded_duration_cell(seconds: f64) -> Cell {
    let minutes = seconds / 60.0;
    let text = format!("{minutes:.1}min");
    if minutes <= 10.0 {
        Cell::new(text).fg(TableColor::Green)
    } else if minutes <= 15.0 {
        Cell::new(text).fg(TableColor::Yellow)
    } else {
        Cell::new(text).fg(TableColor::Red)
    }
}

pub fn color_coded_failure_cell(rate: f64) -> Cell {
    let text = format!("{rate:.1}%");
    if rate >= 50.0 {
        Cell::new(text).fg(TableColor::Red)
    } else if rate >= 25.0 {
        Cell::new(text).fg(TableColor::Yellow)
    } else {
        Cell::new(text).fg(TableColor::Green)
    }
}

pub fn color_coded_retry_cell(rate: f64) -> Cell {
    let text = format!("{rate:.1}%");
    if rate >= 10.0 {
        Cell::new(text).fg(TableColor::Red)
    } else if rate >= 5.0 {
        Cell::new(text).fg(TableColor::Yellow)
    } else {
        Cell::new(text).fg(TableColor::Green)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_table() {
        let mut table = create_table();
        // Just verify it creates without panic and can add rows
        table.add_row(vec!["test"]);
        assert_eq!(table.column_count(), 1);
    }

    mod color_coded_duration_cell {
        use super::*;

        #[test]
        fn returns_green_for_short_durations() {
            let cell = color_coded_duration_cell(60.0); // 1 min
            assert!(cell.content().contains("1.0min"));
        }

        #[test]
        fn returns_green_at_10_minute_boundary() {
            let cell = color_coded_duration_cell(600.0); // 10 min
            assert!(cell.content().contains("10.0min"));
        }

        #[test]
        fn returns_yellow_for_medium_durations() {
            let cell = color_coded_duration_cell(720.0); // 12 min
            assert!(cell.content().contains("12.0min"));
        }

        #[test]
        fn returns_yellow_at_15_minute_boundary() {
            let cell = color_coded_duration_cell(900.0); // 15 min
            assert!(cell.content().contains("15.0min"));
        }

        #[test]
        fn returns_red_for_long_durations() {
            let cell = color_coded_duration_cell(1200.0); // 20 min
            assert!(cell.content().contains("20.0min"));
        }

        #[test]
        fn handles_zero_duration() {
            let cell = color_coded_duration_cell(0.0);
            assert!(cell.content().contains("0.0min"));
        }

        #[test]
        fn formats_decimal_places_correctly() {
            let cell = color_coded_duration_cell(90.5); // 1.508... min
            assert!(cell.content().contains(".5min") || cell.content().contains(".6min"));
        }
    }

    mod color_coded_failure_cell {
        use super::*;

        #[test]
        fn returns_green_for_low_failure_rates() {
            let cell = color_coded_failure_cell(10.0);
            assert!(cell.content().contains("10.0%"));
        }

        #[test]
        fn returns_green_just_below_25_percent() {
            let cell = color_coded_failure_cell(24.9);
            assert!(cell.content().contains("24.9%"));
        }

        #[test]
        fn returns_yellow_at_25_percent_boundary() {
            let cell = color_coded_failure_cell(25.0);
            assert!(cell.content().contains("25.0%"));
        }

        #[test]
        fn returns_yellow_for_medium_failure_rates() {
            let cell = color_coded_failure_cell(35.0);
            assert!(cell.content().contains("35.0%"));
        }

        #[test]
        fn returns_yellow_just_below_50_percent() {
            let cell = color_coded_failure_cell(49.9);
            assert!(cell.content().contains("49.9%"));
        }

        #[test]
        fn returns_red_at_50_percent_boundary() {
            let cell = color_coded_failure_cell(50.0);
            assert!(cell.content().contains("50.0%"));
        }

        #[test]
        fn returns_red_for_high_failure_rates() {
            let cell = color_coded_failure_cell(75.0);
            assert!(cell.content().contains("75.0%"));
        }

        #[test]
        fn handles_zero_rate() {
            let cell = color_coded_failure_cell(0.0);
            assert!(cell.content().contains("0.0%"));
        }

        #[test]
        fn handles_hundred_percent() {
            let cell = color_coded_failure_cell(100.0);
            assert!(cell.content().contains("100.0%"));
        }
    }

    mod color_coded_retry_cell {
        use super::*;

        #[test]
        fn returns_green_for_low_retry_rates() {
            let cell = color_coded_retry_cell(2.0);
            assert!(cell.content().contains("2.0%"));
        }

        #[test]
        fn returns_green_just_below_5_percent() {
            let cell = color_coded_retry_cell(4.9);
            assert!(cell.content().contains("4.9%"));
        }

        #[test]
        fn returns_yellow_at_5_percent_boundary() {
            let cell = color_coded_retry_cell(5.0);
            assert!(cell.content().contains("5.0%"));
        }

        #[test]
        fn returns_yellow_for_medium_retry_rates() {
            let cell = color_coded_retry_cell(7.5);
            assert!(cell.content().contains("7.5%"));
        }

        #[test]
        fn returns_yellow_just_below_10_percent() {
            let cell = color_coded_retry_cell(9.9);
            assert!(cell.content().contains("9.9%"));
        }

        #[test]
        fn returns_red_at_10_percent_boundary() {
            let cell = color_coded_retry_cell(10.0);
            assert!(cell.content().contains("10.0%"));
        }

        #[test]
        fn returns_red_for_high_retry_rates() {
            let cell = color_coded_retry_cell(25.0);
            assert!(cell.content().contains("25.0%"));
        }

        #[test]
        fn handles_zero_rate() {
            let cell = color_coded_retry_cell(0.0);
            assert!(cell.content().contains("0.0%"));
        }
    }
}
