//! Small, dependency-free statistics. Every function here is deterministic and total:
//! "cannot compute" is `None`, never a made-up number.

/// z for a two-sided 95% interval.
const Z95: f64 = 1.959_963_984_540_054;

pub fn mean(xs: &[f64]) -> Option<f64> {
    if xs.is_empty() {
        return None;
    }
    Some(xs.iter().sum::<f64>() / xs.len() as f64)
}

/// Sample standard deviation (n - 1). One value has no spread, so `None`, not 0.
pub fn sample_sd(xs: &[f64]) -> Option<f64> {
    if xs.len() < 2 {
        return None;
    }
    let m = mean(xs)?;
    let ss: f64 = xs.iter().map(|x| (x - m).powi(2)).sum();
    Some((ss / (xs.len() - 1) as f64).sqrt())
}

/// Median like Python's `statistics.median` (mean of the two middle values when even),
/// so it matches what Benchmark_configs writes as `median_output_tokens`.
pub fn median(xs: &[f64]) -> Option<f64> {
    if xs.is_empty() {
        return None;
    }
    let mut v = xs.to_vec();
    v.sort_by(f64::total_cmp);
    let mid = v.len() / 2;
    if v.len() % 2 == 1 {
        Some(v[mid])
    } else {
        Some((v[mid - 1] + v[mid]) / 2.0)
    }
}

/// Wilson 95% interval for k successes out of n, as proportions in [0, 1].
/// Wilson instead of the normal approximation because it stays inside [0, 1] and is
/// honest near 0% and 100%, where small benchmarks often live.
pub fn wilson95(k: u64, n: u64) -> Option<(f64, f64)> {
    if n == 0 || k > n {
        return None;
    }
    let n = n as f64;
    let p = k as f64 / n;
    let z2 = Z95 * Z95;
    let denom = 1.0 + z2 / n;
    let center = (p + z2 / (2.0 * n)) / denom;
    let half = Z95 * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt() / denom;
    Some(((center - half).max(0.0), (center + half).min(1.0)))
}

/// Two-sided exact sign test: under H0 each non-tied pair is a fair coin, so
/// p = min(1, 2 * P(X <= min(wins, losses))) with X ~ Binomial(wins + losses, 0.5).
///
/// On paired 0/1 outcomes this IS the exact McNemar test (only discordant pairs count),
/// the same formula as `swift-pipeline/verify/verify_model.py`. Exact rather than
/// chi-square because discordant counts are often small.
pub fn sign_test_p(wins: u64, losses: u64) -> f64 {
    let n = wins + losses;
    if n == 0 {
        return 1.0;
    }
    let k = wins.min(losses);
    // Work in log space: 2^-n underflows f64 once n passes ~1074.
    let ln_choose: Vec<f64> = {
        let mut out = Vec::with_capacity(k as usize + 1);
        let mut acc = 0.0_f64;
        out.push(acc);
        for i in 0..k {
            acc += ((n - i) as f64).ln() - ((i + 1) as f64).ln();
            out.push(acc);
        }
        out
    };
    // Terms grow with i while i <= n/2, so the last one is the largest: factor it out.
    let top = ln_choose[k as usize];
    let scaled: f64 = ln_choose.iter().map(|lc| (lc - top).exp()).sum();
    let ln_tail = top + scaled.ln() - n as f64 * std::f64::consts::LN_2;
    (2.0 * ln_tail.exp()).min(1.0)
}

#[cfg(test)]
#[path = "stats_tests.rs"]
mod tests;
