use super::*;
use serde_json::json;

#[test]
fn key_splits_at_first_underscore() {
    assert_eq!(parse_key("s0_017"), Some((0, "017".to_string())));
    assert_eq!(
        parse_key("s3_accountant-12_b"),
        Some((3, "accountant-12_b".to_string()))
    );
    assert_eq!(parse_key("x0_017"), None);
    assert_eq!(parse_key("s0"), None);
}

#[test]
fn status_follows_runner_fields() {
    let ok = json!({"content": "Answer: B", "finish_reason": "stop"});
    let err = json!({"content": "", "finish_reason": "error", "error": "URLError: boom"});
    let timeout = json!({"content": "", "finish_reason": "error", "error": "TimeoutError: The read operation timed out"});
    let trunc = json!({"content": "Answer: B", "finish_reason": "length"});
    let empty = json!({"content": "  ", "finish_reason": "stop"});
    let null_error = json!({"content": "Answer: B", "finish_reason": "stop", "error": null});
    let got: Vec<Status> = [ok, err, timeout, trunc, empty, null_error]
        .iter()
        .map(status_of)
        .collect();
    assert_eq!(
        got,
        vec![
            Status::Ok,
            Status::Error,
            Status::Timeout,
            Status::Truncated,
            Status::Empty,
            Status::Ok
        ]
    );
}

#[test]
fn graders_match_the_runner() {
    let right = json!({"content": "Answer: B", "finish_reason": "stop", "pred": "B", "gold": "B", "prompt": "p"});
    let truncated_right =
        json!({"content": "Answer: B", "finish_reason": "length", "pred": "B", "gold": "B"});
    // GPQA uses ok(): a truncated answer is wrong.
    assert_eq!(grade("gpqa", &right, None, 0), Ok(true));
    assert_eq!(grade("gpqa", &truncated_right, None, 0), Ok(false));
    // C-Eval / MMLU-Pro only check `error`, so the runner counts a truncated answer right.
    // The port is faithful; `run` then scores it wrong and flags the summary disagreement.
    assert_eq!(grade("ceval", &truncated_right, None, 0), Ok(true));
    // AIME: leading zeros ignored on both sides.
    let aime =
        json!({"content": "\\boxed{042}", "finish_reason": "stop", "pred": "042", "gold": "42"});
    assert_eq!(grade("aime_2025", &aime, None, 0), Ok(true));
}

#[test]
fn graders_that_need_code_are_refused() {
    let x = json!({"content": "x", "finish_reason": "stop"});
    for bench in ["hmmt", "lcb"] {
        let err = grade(bench, &x, None, 0).expect_err("must not guess a grade");
        assert!(err.contains("patch the runner"), "{err}");
    }
}
