use std::cmp::Ordering;

/// Compares two f64 values for sorting.
///
/// Handles NaN values by treating them as equal to avoid panics in sort operations.
pub fn cmp_f64(a: f64, b: f64) -> Ordering {
    a.partial_cmp(&b).unwrap_or(Ordering::Equal)
}

/// Calculates P50, P95, P99 percentiles from a list of values.
///
/// # Arguments
/// * `values` - Slice of numeric values to calculate percentiles from
///
/// # Returns
/// Tuple of (p50, p95, p99). Returns (0.0, 0.0, 0.0) for empty input.
/// For single-value input, returns (val, val, val).
///
/// # Algorithm
/// Uses standard percentile calculation with integer arithmetic for index computation.
/// Sorts input values in ascending order and selects values at percentile indices.
pub fn calculate_percentiles(values: &[f64]) -> (f64, f64, f64) {
    if values.is_empty() {
        return (0.0, 0.0, 0.0);
    }

    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| cmp_f64(*a, *b));

    let len = sorted.len();

    // For small datasets, return the same value (best we can do)
    if len == 1 {
        let val = sorted[0];
        return (val, val, val);
    }

    // Calculate percentile indices using integer arithmetic
    let p50_idx = (len / 2).min(len - 1);
    let p95_idx = (len * 95 / 100).min(len - 1);
    let p99_idx = (len * 99 / 100).min(len - 1);

    let p50 = sorted[p50_idx];
    let p95 = sorted[p95_idx];
    let p99 = sorted[p99_idx];

    (p50, p95, p99)
}

/// Calculates success rate as a percentage.
///
/// # Arguments
/// * `successful` - Number of successful items
/// * `total` - Total number of items
///
/// # Returns
/// Success rate as a percentage (0.0 to 100.0).
/// Returns 0.0 if total is 0 to avoid division by zero.
#[allow(clippy::cast_precision_loss)]
pub fn calculate_success_rate(successful: usize, total: usize) -> f64 {
    if total == 0 {
        return 0.0;
    }
    (successful as f64 / total as f64) * 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmp_f64() {
        assert_eq!(cmp_f64(1.0, 2.0), Ordering::Less);
        assert_eq!(cmp_f64(2.0, 1.0), Ordering::Greater);
        assert_eq!(cmp_f64(1.0, 1.0), Ordering::Equal);

        // Test NaN handling
        assert_eq!(cmp_f64(f64::NAN, 1.0), Ordering::Equal);
        assert_eq!(cmp_f64(1.0, f64::NAN), Ordering::Equal);
    }

    #[test]
    fn test_calculate_percentiles() {
        let values = vec![10.0, 20.0, 30.0, 40.0, 50.0];
        let (p50, p95, p99) = calculate_percentiles(&values);

        assert_eq!(p50, 30.0);
        assert_eq!(p95, 50.0);
        assert_eq!(p99, 50.0);
    }

    #[test]
    fn test_calculate_percentiles_empty() {
        let values: Vec<f64> = vec![];
        let (p50, p95, p99) = calculate_percentiles(&values);

        assert_eq!(p50, 0.0);
        assert_eq!(p95, 0.0);
        assert_eq!(p99, 0.0);
    }

    #[test]
    fn test_calculate_percentiles_single_value() {
        let values = vec![42.0];
        let (p50, p95, p99) = calculate_percentiles(&values);

        assert_eq!(p50, 42.0);
        assert_eq!(p95, 42.0);
        assert_eq!(p99, 42.0);
    }

    #[test]
    fn test_calculate_success_rate() {
        assert_eq!(calculate_success_rate(8, 10), 80.0);
        assert_eq!(calculate_success_rate(10, 10), 100.0);
        assert_eq!(calculate_success_rate(0, 10), 0.0);
        assert_eq!(calculate_success_rate(0, 0), 0.0); // Edge case
    }

    #[test]
    fn test_calculate_percentiles_large_dataset() {
        let values: Vec<f64> = (1..=100).map(|x| x as f64).collect();
        let (p50, p95, p99) = calculate_percentiles(&values);

        // P50 should be around 50
        assert!((p50 - 50.0).abs() < 1.0);
        // P95 should be around 95
        assert!((p95 - 95.0).abs() < 1.0);
        // P99 should be around 99
        assert!((p99 - 99.0).abs() < 1.0);
    }
}
