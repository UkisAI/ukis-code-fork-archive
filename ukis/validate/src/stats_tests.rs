use super::*;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn sign_test_matches_exact_binomial() {
    // b=0, c=6: p = 2 * 0.5^6 = 0.03125 (the smallest all-one-way result that is significant).
    assert!(close(sign_test_p(6, 0), 0.03125));
    // b=1, c=5: 2 * (1 + 6) / 64 = 0.21875.
    assert!(close(sign_test_p(1, 5), 0.21875));
    assert!(close(sign_test_p(0, 0), 1.0));
    assert!(close(sign_test_p(10, 10), 1.0));
}

#[test]
fn sign_test_survives_large_n() {
    // 2^-3000 underflows in linear space; the log-space path must still give a sane tiny p.
    let p = sign_test_p(2000, 1000);
    assert!(p > 0.0 && p < 1e-30, "p = {p}");
    // Balanced large n is not significant.
    assert!(sign_test_p(1500, 1500) > 0.9);
}

#[test]
fn wilson_is_inside_unit_interval() {
    let (lo, hi) = wilson95(0, 10).unwrap();
    assert!(close(lo, 0.0) && hi > 0.2 && hi < 0.35);
    let (lo, hi) = wilson95(170, 198).unwrap();
    // Reference: rack score.txt style "85.86% Wilson95 80.3-90.0".
    assert!(
        (lo - 0.803).abs() < 0.001 && (hi - 0.900).abs() < 0.001,
        "{lo} {hi}"
    );
    assert_eq!(wilson95(1, 0), None);
}

#[test]
fn sd_and_median() {
    assert_eq!(sample_sd(&[1.0]), None);
    assert!(close(sample_sd(&[1.0, 3.0]).unwrap(), 2.0_f64.sqrt()));
    assert_eq!(median(&[3.0, 1.0, 2.0, 10.0]), Some(2.5));
    assert_eq!(median(&[]), None);
}
