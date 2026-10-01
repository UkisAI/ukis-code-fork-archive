use pretty_assertions::assert_eq;

use super::unbacked_claims;

#[test]
fn flags_percentage_without_evidence() {
    assert_eq!(
        unbacked_claims("Swift-7B gets 87.3% on GPQA Diamond. Nice."),
        vec!["Swift-7B gets 87.3% on GPQA Diamond".to_string()]
    );
}

#[test]
fn flags_metric_word_with_decimal() {
    assert_eq!(
        unbacked_claims("The accuracy is 0.873 on the dev split"),
        vec!["The accuracy is 0.873 on the dev split".to_string()]
    );
}

#[test]
fn accepts_claim_with_run_tag() {
    assert_eq!(
        unbacked_claims("Swift-7B gets 61.2% on GPQA Diamond [run:gpqa-swift7b-s0]."),
        Vec::<String>::new()
    );
}

#[test]
fn ignores_unknown_answers_and_model_names() {
    assert_eq!(
        unbacked_claims(
            "The GPQA Diamond accuracy of Swift-7B is unknown.\nNo pass@1 run exists for it yet."
        ),
        Vec::<String>::new()
    );
}

#[test]
fn tags_only_cover_their_own_sentence() {
    assert_eq!(
        unbacked_claims("Baseline is 55.0% [run:base-1]. Swift-7B is 87.3%."),
        vec!["Swift-7B is 87.3%".to_string()]
    );
}
