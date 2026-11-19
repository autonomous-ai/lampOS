//! One UTC day's tally replayed from the voice's journal, off the lamp
//! (#7664): what `LAMP_VOICE_DAY_TALLY` would have counted of that day's
//! chat answers, from `journalctl -u lamp-voice -o short-iso` pulls saved
//! on another machine, or from a lamp-side token TSV (`--tsv`). Read only;
//! numbers and no words ([`lamp_voice::day_tally::replay`]).
//!
//! ```text
//! cargo run --release -p lamp-voice --example day_tally_replay -- \
//!     --utc-day 2026-11-18 PULL...
//! cargo run --release -p lamp-voice --example day_tally_replay -- \
//!     --utc-day 2026-11-19 [--from 2026-11-19T08:30:00+07:00] --tsv PATH
//! ```
//!
//! It prints the day as the journal's `thinking today (UTC)` line says it,
//! then the same counts as JSON, in `tally.json`'s form. With `--from`, the
//! lamp-side window's start, the first line says which part of the day is
//! covered; a TSV's refused rows are named by line number.
use lamp_voice::day_tally::{replay, told};

const USAGE: &str = "usage: day_tally_replay --utc-day YYYY-MM-DD [--from RFC3339] \
                     (--tsv PATH | PULL...)";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut date = None;
    let mut from = None;
    let mut tsv = None;
    let mut pulls = Vec::new();
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--utc-day" => date = arguments.next(),
            "--from" => from = Some(arguments.next().ok_or(USAGE)?),
            "--tsv" => tsv = Some(arguments.next().ok_or(USAGE)?),
            _ => pulls.push(argument),
        }
    }
    let date = date.ok_or(USAGE)?;
    let day = replay::day_named(&date).ok_or(USAGE)?;
    let window = match &from {
        Some(from) => format!(
            ", {}",
            replay::window_told(from, day).ok_or("--from is not an RFC 3339 time")?
        ),
        None => String::new(),
    };
    let (counted, source) = match (tsv, pulls.is_empty()) {
        (Some(path), true) => {
            let (counted, refused) = replay::tsv_replayed(&std::fs::read_to_string(path)?, day);
            let refused = if refused.is_empty() {
                String::new()
            } else {
                format!(", rows refused by line: {refused:?}")
            };
            (counted, format!("a lamp-side TSV{refused}"))
        }
        (None, false) => {
            let texts = pulls
                .iter()
                .map(std::fs::read_to_string)
                .collect::<Result<Vec<_>, _>>()?;
            let counted = replay::replayed(texts.iter().map(String::as_str), day);
            (counted, format!("{} pulls", pulls.len()))
        }
        _ => return Err(USAGE.into()),
    };
    println!("{date} (UTC){window}, from {source}: {}", told(&counted));
    println!("{}", serde_json::to_string(&counted)?);
    Ok(())
}
