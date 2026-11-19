//! One UTC day's tally replayed from the voice's journal, off the lamp
//! (#7664): what `LAMP_VOICE_DAY_TALLY` would have counted of that day's
//! chat answers, from `journalctl -u lamp-voice -o short-iso` pulls saved
//! on another machine. Read only; numbers and no words
//! ([`lamp_voice::day_tally::replay`]).
//!
//! ```text
//! cargo run --release -p lamp-voice --example day_tally_replay -- \
//!     --utc-day 2026-11-18 PULL...
//! ```
//!
//! It prints the day as the journal's `thinking today (UTC)` line says it,
//! then the same counts as JSON, in `tally.json`'s form.
use lamp_voice::day_tally::{replay, told};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [flag, date, pulls @ ..] = arguments.as_slice() else {
        return Err("usage: day_tally_replay --utc-day YYYY-MM-DD PULL...".into());
    };
    let day = replay::day_named(date)
        .filter(|_| flag == "--utc-day" && !pulls.is_empty())
        .ok_or("usage: day_tally_replay --utc-day YYYY-MM-DD PULL...")?;
    let texts = pulls
        .iter()
        .map(std::fs::read_to_string)
        .collect::<Result<Vec<_>, _>>()?;
    let counted = replay::replayed(texts.iter().map(String::as_str), day);
    println!(
        "{date} (UTC), from {} pulls: {}",
        pulls.len(),
        told(&counted)
    );
    println!("{}", serde_json::to_string(&counted)?);
    Ok(())
}
