use super::*;
use pretty_assertions::assert_eq;

#[test]
fn intervals_and_quoted_prompts_are_preserved() {
    for (input, seconds, prompt) in [
        ("5m check CI", 300, "check CI"),
        ("30s \"check CI\"", 60, "check CI"),
        ("61s 'check CI'", 120, "check CI"),
        ("7m check CI", 420, "check CI"),
        ("90m check CI", 5400, "check CI"),
        ("check CI every 2 hours", 7200, "check CI"),
        ("1d /review-pr 42", 86400, "/review-pr 42"),
        (
            "5m !echo do not run directly",
            300,
            "!echo do not run directly",
        ),
    ] {
        assert_eq!(
            parse(input),
            Ok(LoopCommand::Start {
                cadence: Cadence::Fixed(Duration::from_secs(seconds)),
                prompt: Some(prompt.into()),
            })
        );
    }
}

#[test]
fn adaptive_defaults_and_management_do_not_get_confused_with_prompts() {
    assert_eq!(
        parse(""),
        Ok(LoopCommand::Start {
            cadence: Cadence::Adaptive,
            prompt: None
        })
    );
    assert_eq!(
        parse("check CI"),
        Ok(LoopCommand::Start {
            cadence: Cadence::Adaptive,
            prompt: Some("check CI".into())
        })
    );
    assert_eq!(
        parse("15m"),
        Ok(LoopCommand::Start {
            cadence: Cadence::Fixed(Duration::from_secs(900)),
            prompt: None
        })
    );
    assert_eq!(parse("stop 12"), Ok(LoopCommand::Stop(12)));
    assert_eq!(parse("stop all"), Ok(LoopCommand::StopAll));
    assert_eq!(parse("list"), Ok(LoopCommand::List));
    assert_eq!(parse("help"), Ok(LoopCommand::Help));
    assert_eq!(
        parse("5m stop"),
        Ok(LoopCommand::Start {
            cadence: Cadence::Fixed(Duration::from_secs(300)),
            prompt: Some("stop".into())
        })
    );
}

#[test]
fn invalid_and_overflowing_intervals_fail_instead_of_scheduling() {
    for args in [
        "0m check",
        "-1m check",
        "1.5m check",
        "9w check",
        "8d check",
        "5 check",
        "18446744073709551615d check",
        "stop nope",
        "stop 0",
        "check every 0 seconds",
    ] {
        assert!(parse(args).is_err(), "{args}");
    }
}

#[test]
fn every_in_an_ordinary_or_quoted_prompt_is_not_a_schedule() {
    for prompt in [
        "check every PR",
        "check logs every morning",
        "check logs every 5m",
    ] {
        let args = format!(r#""{prompt}""#);
        assert_eq!(
            parse(&args),
            Ok(LoopCommand::Start {
                cadence: Cadence::Adaptive,
                prompt: Some(prompt.into())
            })
        );
    }
    assert_eq!(
        parse("check every PR"),
        Ok(LoopCommand::Start {
            cadence: Cadence::Adaptive,
            prompt: Some("check every PR".into())
        })
    );
}
