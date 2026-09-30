//! Parse explicit intervals without asking a model to interpret command syntax.
use crate::loop_scheduler::Cadence;
use crate::loop_scheduler::LIFETIME;
use std::time::Duration;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum LoopCommand {
    Help,
    List,
    StopAll,
    Stop(u64),
    Start {
        cadence: Cadence,
        prompt: Option<String>,
    },
}

pub(crate) fn parse(args: &str) -> Result<LoopCommand, String> {
    let args = args.trim();
    match args {
        "help" | "--help" => return Ok(LoopCommand::Help),
        "list" => return Ok(LoopCommand::List),
        "stop" | "stop all" => return Ok(LoopCommand::StopAll),
        _ => {}
    }
    if let Some(id) = args.strip_prefix("stop ") {
        return id
            .trim()
            .parse::<u64>()
            .ok()
            .filter(|id| *id > 0)
            .map(LoopCommand::Stop)
            .ok_or_else(|| "Use /loop stop <id> or /loop stop all.".into());
    }
    let (first, rest) = args.split_once(char::is_whitespace).unwrap_or((args, ""));
    let (cadence, prompt) =
        if first.starts_with(|ch: char| ch.is_ascii_digit() || ch == '-' || ch == '+') {
            (Cadence::Fixed(parse_interval(first)?), rest.trim())
        } else if let Some((prompt, suffix)) = args.rsplit_once(" every ") {
            let compact: String = suffix.chars().filter(|ch| !ch.is_whitespace()).collect();
            (Cadence::Fixed(parse_interval(&compact)?), prompt.trim())
        } else {
            (Cadence::Adaptive, args)
        };
    let prompt = unquote(prompt).trim();
    Ok(LoopCommand::Start {
        cadence,
        prompt: (!prompt.is_empty()).then(|| prompt.to_owned()),
    })
}

fn unquote(text: &str) -> &str {
    for quote in ['\"', '\''] {
        if let Some(inner) = text
            .strip_prefix(quote)
            .and_then(|text| text.strip_suffix(quote))
        {
            return inner;
        }
    }
    text
}

fn parse_interval(value: &str) -> Result<Duration, String> {
    let invalid = || {
        "Use a positive whole-number interval, such as 30s, 5m, 2h, or 1d (maximum 7 days)."
            .to_owned()
    };
    let split = value
        .find(|ch: char| !ch.is_ascii_digit())
        .ok_or_else(invalid)?;
    let amount = value[..split].parse::<u64>().map_err(|_| invalid())?;
    let multiplier = match value[split..].to_ascii_lowercase().as_str() {
        "s" | "second" | "seconds" => 1,
        "m" | "minute" | "minutes" => 60,
        "h" | "hour" | "hours" => 3600,
        "d" | "day" | "days" => 86400,
        _ => return Err(invalid()),
    };
    let seconds = amount
        .checked_mul(multiplier)
        .filter(|seconds| *seconds > 0)
        .ok_or_else(invalid)?;
    let seconds = seconds.checked_add(59).ok_or_else(invalid)? / 60 * 60;
    let interval = Duration::from_secs(seconds);
    if interval > LIFETIME {
        return Err(invalid());
    }
    Ok(interval)
}

#[cfg(test)]
#[path = "loop_command_tests.rs"]
mod tests;
