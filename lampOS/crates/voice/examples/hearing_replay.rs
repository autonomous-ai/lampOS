//! Offline: a `lamp-audio replay-aec` output heard as the voice hears it
//! (#828).
//!
//! Local files only. It opens no device and no connection, and it is not a
//! device result: the replayed canceller ran on the host.
//!
//! ```text
//! hearing_replay REPLAY_DIR [--signal aec_only|aec_linear|aec_linear_gated|aec_ns|pre_aec] [--explained PERCENT]
//! ```
//!
//! For each replayed capture block (`capture_pair` in `operations.jsonl`),
//! one replayed signal (`segment-001-<signal>.wav`, the AEC-only output by
//! default; `aec_linear` is what `LAMP_VAD_INPUT=linear` scores, and
//! `aec_linear_gated` is `LAMP_VAD_INPUT=linear-gated`: the linear output on a
//! block the AEC-only evidence admits, the AEC-only output elsewhere) is scored by
//! the voice's own detector, one stream as the coordinator runs it, and the
//! block is judged by `Hearing::EVALUATED` against the AEC-only canceller's
//! echo evidence, as the duplex rules judge it. `--explained` changes only
//! `explained_percent`, the share of the block the render may explain for
//! speech over the lamp to be a person (#828: on the linear output the lamp
//! alone was 93 % explained or more, a played stop 0 % at its p10). It
//! writes `REPLAY_DIR/segment-001-hearing.tsv` for the default, and
//! `segment-001-<signal>[-explained-N]-hearing.tsv` otherwise, one line per
//! block with the time the lamp read it, so that a played word's own time
//! finds its blocks; `over_run` counts the blocks heard as a person over the
//! lamp in a row. `onset_needed` is how many in a row yield there: 10 in the
//! reply's first 1.2 s (`early_onset_blocks`), 7 after (`onset_blocks`). And
//! `step` is what `Interruption` (`Confirmation::EVALUATED`) does with the
//! block: `Yield`, `Commit`, `Resume` or `Carry`. The lamp counts as having
//! a reply to yield while its render was heard in the last 300 ms
//! (`clear_after_blocks`) or a yield is in force: an approximation of the
//! voice's own "reply audio to hand on", which the replay does not have.
//! The voice's first-match guard (no yield before the render has matched the
//! microphone a few times in a reply) is not applied. Nothing is decided
//! here: yields and stops are counted from the table. A replay of more than
//! one segment is refused.
use lamp_audio::{
    CAPTURE_SAMPLES, CaptureBlock,
    duplex::{Confirmation, Heard, Hearing, Interruption},
    evidence::EchoEvidence,
};
use lamp_voice::activity::{DetectorInput, SpeechProbability, detector_input};
use serde_json::Value;
use std::{
    fs,
    io::{self, BufRead, BufWriter, Write},
    path::Path,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn main() -> Result<()> {
    let usage = || {
        io::Error::other(
            "usage: hearing_replay REPLAY_DIR [--signal aec_only|aec_linear|aec_linear_gated|aec_ns|pre_aec] [--explained PERCENT]",
        )
    };
    let mut args = std::env::args().skip(1);
    let directory = args.next().ok_or_else(usage)?;
    let directory = Path::new(&directory);
    let mut hearing = Hearing::EVALUATED;
    let (mut signal, mut explained) = ("aec_only".to_owned(), None);
    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(usage)?;
        match flag.as_str() {
            "--signal"
                if [
                    "aec_only",
                    "aec_linear",
                    "aec_linear_gated",
                    "aec_ns",
                    "pre_aec",
                ]
                .contains(&value.as_str()) =>
            {
                signal = value;
            }
            "--explained" => {
                let percent: u8 = value.parse()?;
                if percent > 100 {
                    return Err(usage().into());
                }
                hearing.explained_percent = percent;
                explained = Some(percent);
            }
            _ => return Err(usage().into()),
        }
    }
    let table_name = match (signal.as_str(), explained) {
        ("aec_only", None) => "segment-001-hearing.tsv".to_owned(),
        (signal, None) => format!("segment-001-{signal}-hearing.tsv"),
        (signal, Some(percent)) => format!("segment-001-{signal}-explained-{percent}-hearing.tsv"),
    };
    let report: Value = serde_json::from_slice(&fs::read(directory.join("report.json"))?)?;
    let segments = report["segments"].as_array().map_or(0, Vec::len);
    if segments != 1 {
        return Err(io::Error::other(format!("{segments} segments; only one is read")).into());
    }
    let read = |signal: &str| -> Result<Vec<i16>> {
        Ok(
            hound::WavReader::open(directory.join(format!("segment-001-{signal}.wav")))?
                .into_samples::<i16>()
                .collect::<std::result::Result<_, _>>()?,
        )
    };
    // `aec_linear_gated` reads the AEC-only output and the linear one, and
    // picks between them block by block.
    let gated = signal == "aec_linear_gated";
    let samples = read(if gated { "aec_only" } else { &signal })?;
    let linear_samples = if gated {
        Some(read("aec_linear")?)
    } else {
        None
    };
    let mut table = BufWriter::new(fs::File::create(directory.join(&table_name))?);
    writeln!(
        table,
        "block\tread_at_us\tvad\trender_quiet\tmatched\tcapture_explained\tresidual_floor\talignment_age\tcapture_tenths\tresidual_tenths\theard\tplain\tover_run\tonset_needed\tstep"
    )?;
    let mut over_run = 0u32;
    let mut interruption = Interruption::new(Confirmation::EVALUATED);
    let mut detector = SpeechProbability::default();
    let mut blocks = 0usize;
    let operations = io::BufReader::new(fs::File::open(directory.join("operations.jsonl"))?);
    for line in operations.lines() {
        let record: Value = serde_json::from_str(&line?)?;
        if record["kind"] != "capture_pair" {
            continue;
        }
        let offset = record["segment_capture_sample_offset"]
            .as_u64()
            .ok_or_else(|| io::Error::other("capture_pair without a sample offset"))?
            as usize;
        if offset != blocks * CAPTURE_SAMPLES {
            return Err(io::Error::other(format!("block {blocks} at sample {offset}")).into());
        }
        let block: CaptureBlock = samples
            .get(offset..offset + CAPTURE_SAMPLES)
            .ok_or_else(|| io::Error::other("the output is shorter than its records"))?
            .try_into()?;
        let read_at = record["meta"]["first_read_started_at_us"]
            .as_u64()
            .ok_or_else(|| io::Error::other("capture_pair without its read time"))?;
        let evidence: Option<EchoEvidence> =
            serde_json::from_value(record["echo_evidence"]["aec_only"].clone())?;
        let linear: Option<CaptureBlock> = linear_samples
            .as_ref()
            .map(|linear| {
                linear
                    .get(offset..offset + CAPTURE_SAMPLES)
                    .ok_or_else(|| {
                        io::Error::other("the linear output is shorter than its records")
                    })?
                    .try_into()
                    .map_err(|_| io::Error::other("a short linear block"))
            })
            .transpose()?;
        let probability = detector.analyze(detector_input(
            DetectorInput::LinearGated,
            evidence.as_ref(),
            &block,
            linear.as_ref(),
        ));
        let (heard, plain) = evidence.map_or((Heard::Unknown, false), |evidence| {
            let heard = hearing.heard(probability, &evidence);
            (heard, hearing.plain(heard, &evidence))
        });
        let matched = evidence.and_then(|evidence| evidence.echo);
        over_run = if heard == Heard::Over {
            over_run + 1
        } else {
            0
        };
        let speaking = evidence
            .is_some_and(|evidence| evidence.render_quiet_blocks < hearing.clear_after_blocks)
            || interruption.yielded();
        let step = interruption.observe(heard, plain, speaking);
        writeln!(
            table,
            "{blocks}\t{read_at}\t{probability:.4}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{heard:?}\t{}\t{over_run}\t{}\t{step:?}",
            evidence.map_or(-1, |evidence| i32::from(evidence.render_quiet_blocks)),
            u8::from(matched.is_some()),
            matched.map_or(0, |m| m.capture_explained),
            matched.map_or(0, |m| m.residual_playback_floor),
            matched.map_or(0, |m| m.alignment_age_blocks),
            evidence.map_or(-960, |evidence| evidence.capture_level.tenths_of_db()),
            evidence.map_or(-960, |evidence| evidence.residual_level.tenths_of_db()),
            u8::from(plain),
            interruption.onset_needed(),
        )?;
        blocks += 1;
    }
    table.flush()?;
    eprintln!("hearing_replay: {blocks} blocks of {signal} in {table_name}");
    Ok(())
}
