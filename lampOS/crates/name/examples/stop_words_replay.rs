//! Offline: played "Stop." lines over the lamp's own voice, heard by
//! `lamp-name`'s stop model on each stage of the echo canceller (#828, the
//! Stop words row). It runs only when named, on this machine's files: it
//! opens no device and no connection, and the sound never leaves the Mac.
//!
//! ```text
//! cargo run --release -p lamp-name --example stop_words_replay -- \
//!     --models MODELS_DIR [--stop-least 0.5[,0.9…]] [--after-ms 1000] \
//!     [--lamp-audio BIN] [--workspace DIR] BUNDLE[:PLAYS] ...
//! ```
//!
//! - `MODELS_DIR` holds the three name models and `stop.onnx`, as on the
//!   lamp (`/opt/lampOS/name/models`; `stop.onnx` is checked against its
//!   pinned SHA-256 as `lamp-name --stop-least` checks it).
//! - A `BUNDLE` is a `lamp-audio replay-aec … --linear-output` output (its
//!   `operations.jsonl` and `segment-001-<source>.wav`), or a recording that
//!   tool reads (`events.jsonl`, `capture-audio/`, `render-audio/`), as a
//!   folder or a `.tar`. A recording is replayed first with `--lamp-audio`,
//!   from `--workspace` (a lampOS checkout; default the current folder),
//!   into a fresh leaf under its `artifacts/` that is removed afterwards.
//! - `PLAYS` is when each "Stop." was played: `start_us<TAB>duration_us…`
//!   on the lamp's clock, the clock of each capture block's
//!   `first_read_started_at_us` (a header line is passed over). Without it,
//!   `plays.tsv` in the bundle (or its replay), then `plays-<name>.tsv`
//!   beside it with any `out-` left off. A line with a fourth column is a
//!   stop line only when that column is `cut` (the recorder's `plays.tsv`
//!   lists the requests played too); a line without one is a stop line.
//!
//! For each source, `pre_aec` (the microphone before the canceller, what
//! `LAMP_STOP_WORDS_SOURCE=raw` takes), `aec_linear` (the canceller's linear
//! stage, `linear`; 64 samples, 4 ms, ahead of the others) and `aec_only`
//! (its output, `processed`, what `mic2/audio` carries and the lamp's
//! `lamp-name` hears), the same whole replay is given to the product's own
//! listener 20 ms at a time: `Hearer` with the stop model, under `Quiet`
//! without a rest bar as the product unit runs it, and `Stops` at each bar,
//! with its 1 s hold-off. A stop is at the time the lamp read the newest
//! sample of its step.
//!
//! - **A catch:** the first stop from a line's start to its end + `--after-ms`
//!   (1 s, as on #828). Its latency is from the line's start.
//! - **Other:** a second stop in that window, or one in the second before a
//!   line starts: neither a catch nor false.
//! - **A false stop:** one outside every line's window, and a second or more
//!   before every line's start.
use lamp_name::{
    hear::{Hearer, STEP, WINDOW},
    quiet::{Models, Quiet},
    said::Stops,
};
use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, String>;

/// The canceller's stages, in the order they come from the microphone.
const SOURCES: [&str; 3] = ["pre_aec", "aec_linear", "aec_only"];
/// 20 ms, as the voice puts it on the bus and `lamp-name check` gives it.
const BLOCK: usize = 320;
const RATE: u64 = 16_000;
/// A stop this near before a line's start is not counted false.
const NEAR_US: u64 = 1_000_000;

const USAGE: &str = "usage: stop_words_replay --models DIR [--stop-least 0.5[,0.9…]] [--after-ms MS] [--lamp-audio BIN] [--workspace DIR] BUNDLE[:PLAYS] ...";

fn main() {
    if let Err(why) = run(std::env::args().skip(1).collect()) {
        eprintln!("stop_words_replay: {why}");
        std::process::exit(1);
    }
}

#[derive(Debug, PartialEq)]
struct Options {
    models: PathBuf,
    bars: Vec<f32>,
    after_us: u64,
    lamp_audio: Option<PathBuf>,
    workspace: PathBuf,
    items: Vec<String>,
}

fn options(args: &[String]) -> Result<Options> {
    let (mut models, mut bars, mut after_us, mut lamp_audio, mut workspace) =
        (None, None, None, None, None);
    let mut items = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = || args.next().cloned().ok_or_else(|| USAGE.to_owned());
        let repeated = match arg.as_str() {
            "--models" => models.replace(PathBuf::from(value()?)).is_some(),
            "--stop-least" => bars.replace(bars_of(&value()?)?).is_some(),
            "--after-ms" => {
                let ms: u64 = value()?
                    .parse()
                    .map_err(|_| "--after-ms is whole milliseconds".to_owned())?;
                after_us.replace(ms * 1_000).is_some()
            }
            "--lamp-audio" => lamp_audio.replace(PathBuf::from(value()?)).is_some(),
            "--workspace" => workspace.replace(PathBuf::from(value()?)).is_some(),
            option if option.starts_with("--") => return Err(USAGE.to_owned()),
            item => {
                items.push(item.to_owned());
                false
            }
        };
        if repeated {
            return Err(format!("{arg} given twice"));
        }
    }
    if items.is_empty() {
        return Err(USAGE.to_owned());
    }
    let workspace = match workspace {
        Some(workspace) => workspace,
        None => std::env::current_dir().map_err(|error| error.to_string())?,
    };
    Ok(Options {
        models: models.ok_or("--models DIR is needed")?,
        bars: bars.unwrap_or_else(|| vec![0.5]),
        after_us: after_us.unwrap_or(1_000_000),
        lamp_audio,
        workspace,
        items,
    })
}

/// `--stop-least`'s bars: as `lamp-name` takes them, 0.5 to 1.
fn bars_of(value: &str) -> Result<Vec<f32>> {
    value
        .split(',')
        .map(|bar| {
            bar.parse::<f32>()
                .ok()
                .filter(|bar| (0.5..=1.0).contains(bar))
                .ok_or_else(|| "--stop-least is scores from 0.5 to 1, by commas".to_owned())
        })
        .collect()
}

fn run(args: Vec<String>) -> Result<()> {
    let options = options(&args)?;
    let hearer = Hearer::load(&options.models)?.with_stop(&options.models)?;
    let mut quiet = Quiet::new(hearer, None);
    let mut scratch = Scratch::default();
    let mut all = Vec::new();
    for item in &options.items {
        let bundle = resolve(item, &options, &mut scratch)?;
        let heard = hear_bundle(&mut quiet, &bundle)?;
        print!(
            "{}",
            report(&bundle.name, &heard, &options.bars, options.after_us)
        );
        all.push(heard);
    }
    if all.len() > 1 {
        print!("{}", summary(&all, &options.bars, options.after_us));
    }
    Ok(())
}

/// Folders this run made, removed when it ends, however it ends.
#[derive(Default)]
struct Scratch(Vec<PathBuf>);

impl Scratch {
    fn new_folder(&mut self, under: &Path, prefix: &str) -> PathBuf {
        let path = under.join(format!("{prefix}-{}-{}", std::process::id(), self.0.len()));
        self.0.push(path.clone());
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = fs::remove_dir_all(path);
        }
    }
}

/// A bundle ready to hear: its name, its replay and its plays.
struct Bundle {
    name: String,
    replay: PathBuf,
    plays: Vec<Play>,
}

fn resolve(item: &str, options: &Options, scratch: &mut Scratch) -> Result<Bundle> {
    let (path, plays) = match item.rsplit_once(':') {
        Some((path, plays)) if !plays.is_empty() => (path, Some(PathBuf::from(plays))),
        _ => (item, None),
    };
    let given = PathBuf::from(path);
    let name = given
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.trim_end_matches(".tar").to_owned())
        .ok_or_else(|| format!("{path}: not a bundle"))?;
    let mut root = given.clone();
    if path.ends_with(".tar") {
        let into = scratch.new_folder(&std::env::temp_dir(), "lamp-name-swr");
        fs::create_dir_all(&into).map_err(|error| format!("{}: {error}", into.display()))?;
        let status = Command::new("tar")
            .arg("-xf")
            .arg(&given)
            .arg("-C")
            .arg(&into)
            .status()
            .map_err(|error| format!("tar: {error}"))?;
        if !status.success() {
            return Err(format!("{path}: tar -xf failed ({status})"));
        }
        root = into;
    }
    let replay = match find(&root, |dir| dir.join("segment-001-aec_only.wav").is_file()) {
        Some(replay) => replay,
        None => {
            let recording = find(&root, |dir| {
                dir.join("events.jsonl").is_file() && dir.join("capture-audio").is_dir()
            })
            .ok_or_else(|| {
                format!("{path}: neither a replay (segment-001-aec_only.wav) nor a recording (events.jsonl, capture-audio/)")
            })?;
            replay_recording(&recording, options, scratch)?
        }
    };
    let plays = match plays {
        Some(plays) => plays,
        None => [
            replay.join("plays.tsv"),
            root.join("plays.tsv"),
            given
                .parent()
                .unwrap_or(Path::new("."))
                .join(format!("plays-{}.tsv", name.trim_start_matches("out-"))),
        ]
        .into_iter()
        .find(|plays| plays.is_file())
        .ok_or_else(|| format!("{path}: no plays.tsv in it or beside it; give BUNDLE:PLAYS"))?,
    };
    let text =
        fs::read_to_string(&plays).map_err(|error| format!("{}: {error}", plays.display()))?;
    let plays = plays_of(&text);
    if plays.is_empty() {
        return Err(format!("{path}: no played lines in its plays"));
    }
    Ok(Bundle {
        name,
        replay,
        plays,
    })
}

/// The first folder at or under `root` that `is` holds, nearest first.
fn find(root: &Path, is: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let mut queue = vec![root.to_path_buf()];
    while !queue.is_empty() {
        let mut next = Vec::new();
        for dir in queue {
            if is(&dir) {
                return Some(dir);
            }
            if let Ok(entries) = fs::read_dir(&dir) {
                let mut dirs: Vec<PathBuf> = entries
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|path| path.is_dir())
                    .collect();
                dirs.sort();
                next.extend(dirs);
            }
        }
        queue = next;
    }
    None
}

/// `lamp-audio replay-aec RECORDING OUT --linear-output`, run from the
/// workspace: it writes only to a fresh leaf under the workspace's
/// `artifacts/`.
fn replay_recording(recording: &Path, options: &Options, scratch: &mut Scratch) -> Result<PathBuf> {
    let lamp_audio = options
        .lamp_audio
        .as_ref()
        .ok_or("a recording is replayed first: give --lamp-audio BIN")?;
    let absolute = |path: &Path| {
        std::path::absolute(path).map_err(|error| format!("{}: {error}", path.display()))
    };
    let (lamp_audio, recording, workspace) = (
        absolute(lamp_audio)?,
        absolute(recording)?,
        absolute(&options.workspace)?,
    );
    let artifacts = workspace.join("artifacts");
    fs::create_dir_all(&artifacts).map_err(|error| format!("{}: {error}", artifacts.display()))?;
    let out = scratch.new_folder(&artifacts, "stop-words-replay");
    let done = Command::new(&lamp_audio)
        .arg("replay-aec")
        .arg(&recording)
        .arg(&out)
        .arg("--linear-output")
        .current_dir(&workspace)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| format!("{}: {error}", lamp_audio.display()))?;
    if !done.status.success() {
        return Err(format!(
            "lamp-audio replay-aec {} failed ({}):\n{}",
            recording.display(),
            done.status,
            String::from_utf8_lossy(&done.stderr).trim_end()
        ));
    }
    Ok(out)
}

/// A played line, on the lamp's clock.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Play {
    start_us: u64,
    end_us: u64,
}

/// `start_us<TAB>duration_us[<TAB>text[<TAB>kind…]]` a line; lines that
/// are not (a header) are passed over, and so is a line whose fourth
/// column is there and not `cut`: a request the recorder played, not a stop.
fn plays_of(text: &str) -> Vec<Play> {
    text.lines()
        .filter_map(|line| {
            let mut columns = line.split('\t');
            let start_us: u64 = columns.next()?.trim().parse().ok()?;
            let duration_us: u64 = columns.next()?.trim().parse().ok()?;
            if columns.nth(1).is_some_and(|kind| kind.trim() != "cut") {
                return None;
            }
            Some(Play {
                start_us,
                end_us: start_us + duration_us,
            })
        })
        .collect()
}

/// Sample to the lamp's clock: each capture block's first sample and the
/// time its read began (`capture_pair` in `operations.jsonl`).
struct Clock(Vec<(usize, u64)>);

impl Clock {
    fn of(operations: &str) -> Result<Self> {
        let mut blocks = Vec::new();
        for (number, line) in operations.lines().enumerate() {
            let record: serde_json::Value = serde_json::from_str(line)
                .map_err(|error| format!("operations.jsonl line {}: {error}", number + 1))?;
            if record["kind"] != "capture_pair" {
                continue;
            }
            let offset = record["segment_capture_sample_offset"].as_u64();
            let read_at = record["meta"]["first_read_started_at_us"].as_u64();
            let (Some(offset), Some(read_at)) = (offset, read_at) else {
                return Err(format!(
                    "operations.jsonl line {}: a capture_pair without its offset or read time",
                    number + 1
                ));
            };
            blocks.push((offset as usize, read_at));
        }
        if blocks.is_empty() || blocks.windows(2).any(|pair| pair[1].0 <= pair[0].0) {
            return Err("operations.jsonl: no capture blocks in order".to_owned());
        }
        Ok(Self(blocks))
    }

    /// When the lamp read `sample`.
    fn at(&self, sample: usize) -> u64 {
        let index = self.0.partition_point(|(offset, _)| *offset <= sample);
        let (offset, read_at) = self.0[index.saturating_sub(1)];
        read_at + (sample.saturating_sub(offset) as u64 * 1_000_000) / RATE
    }
}

/// The stop model's scores, each at the lamp's time.
type Scores = Vec<(u64, f32)>;

/// A bundle heard: per source its stop scores, or `None` for a source the
/// replay has not.
struct Heard {
    plays: Vec<Play>,
    seconds: f64,
    sources: Vec<(&'static str, Option<Scores>)>,
}

fn hear_bundle<M: Models>(quiet: &mut Quiet<M>, bundle: &Bundle) -> Result<Heard> {
    let report = bundle.replay.join("report.json");
    if let Ok(text) = fs::read_to_string(&report) {
        let report: serde_json::Value =
            serde_json::from_str(&text).map_err(|error| format!("report.json: {error}"))?;
        let segments = report["segments"].as_array().map_or(0, Vec::len);
        if segments != 1 {
            return Err(format!(
                "{segments} segments in the replay; only one is read"
            ));
        }
    }
    let operations = bundle.replay.join("operations.jsonl");
    let clock = Clock::of(
        &fs::read_to_string(&operations)
            .map_err(|error| format!("{}: {error}", operations.display()))?,
    )?;
    let mut sources = Vec::new();
    let mut seconds = 0.0;
    for source in SOURCES {
        let wav = bundle.replay.join(format!("segment-001-{source}.wav"));
        if !wav.is_file() {
            sources.push((source, None));
            continue;
        }
        let samples = wav_samples(&wav)?;
        seconds = f64::max(seconds, samples.len() as f64 / RATE as f64);
        sources.push((source, Some(scores(quiet, &samples, &clock)?)));
    }
    Ok(Heard {
        plays: bundle.plays.clone(),
        seconds,
        sources,
    })
}

/// The stop model's score at each step, as `lamp-name` gives it sound,
/// each at the time the lamp read the step's newest sample.
fn scores<M: Models>(
    quiet: &mut Quiet<M>,
    samples: &[i16],
    clock: &Clock,
) -> Result<Vec<(u64, f32)>> {
    quiet.forget();
    let mut all = Vec::new();
    for block in samples.chunks(BLOCK) {
        let heard = quiet.hear(block)?;
        let newest = quiet.steps();
        let first = (newest + 1).saturating_sub(heard.scores.len());
        for (step, score) in (first..).zip(heard.scores) {
            let stop = score.stop.ok_or("the stop model gave no score")?;
            // Step k (from 1) ends at sample WINDOW + (k - 1) * STEP.
            all.push((clock.at(WINDOW + (step - 1) * STEP - 1), stop));
        }
    }
    Ok(all)
}

/// Each source at one bar.
#[derive(Debug, Default, PartialEq)]
struct Tally {
    caught: usize,
    false_stops: usize,
    other: usize,
    /// Each caught line's first stop, from the line's start.
    latencies_us: Vec<u64>,
}

/// Stops at `bar` (the product's `Stops`, with its 1 s hold-off, on the
/// lamp's time) against the played lines.
fn judge(scores: &[(u64, f32)], bar: f32, plays: &[Play], after_us: u64) -> Tally {
    let mut stops = Stops::new(bar);
    let base = Instant::now();
    let t0 = scores.first().map_or(0, |(at, _)| *at);
    let mut first = vec![None; plays.len()];
    let mut tally = Tally::default();
    for &(at, score) in scores {
        if stops
            .score(score, base + Duration::from_micros(at.saturating_sub(t0)))
            .is_none()
        {
            continue;
        }
        match plays
            .iter()
            .position(|play| play.start_us <= at && at < play.end_us + after_us)
        {
            Some(line) if first[line].is_none() => first[line] = Some(at - plays[line].start_us),
            Some(_) => tally.other += 1,
            None if plays
                .iter()
                .any(|play| play.start_us.saturating_sub(NEAR_US) <= at && at < play.start_us) =>
            {
                tally.other += 1
            }
            None => tally.false_stops += 1,
        }
    }
    tally.latencies_us = first.into_iter().flatten().collect();
    tally.caught = tally.latencies_us.len();
    tally
}

fn latency(latencies_us: &[u64]) -> String {
    if latencies_us.is_empty() {
        return "-".to_owned();
    }
    let mut sorted = latencies_us.to_vec();
    sorted.sort_unstable();
    let median = sorted[(sorted.len() - 1) / 2];
    format!(
        "{:.2} s (max {:.2} s)",
        median as f64 / 1e6,
        sorted[sorted.len() - 1] as f64 / 1e6
    )
}

const HEADER: &str = "source\tbar\tcatches\tfalse stops\tother\tfirst-catch latency (median, from the line's start)\n";

fn report(name: &str, heard: &Heard, bars: &[f32], after_us: u64) -> String {
    let mut out = format!(
        "{name}: {} played lines, {:.1} min replayed\n{HEADER}",
        heard.plays.len(),
        heard.seconds / 60.0
    );
    for &bar in bars {
        for (source, scores) in &heard.sources {
            let Some(scores) = scores else {
                let _ = writeln!(out, "{source}\t{bar:.2}\tnot in the replay");
                continue;
            };
            let tally = judge(scores, bar, &heard.plays, after_us);
            let _ = writeln!(
                out,
                "{source}\t{bar:.2}\t{}/{}\t{}\t{}\t{}",
                tally.caught,
                heard.plays.len(),
                tally.false_stops,
                tally.other,
                latency(&tally.latencies_us)
            );
        }
    }
    out
}

/// Every bundle together, by source and bar.
fn summary(all: &[Heard], bars: &[f32], after_us: u64) -> String {
    let mut out = format!("all {} bundles\n{HEADER}", all.len());
    for &bar in bars {
        for source in SOURCES {
            let (mut lines, mut total, mut missing) = (0, Tally::default(), 0);
            for heard in all {
                match heard.sources.iter().find(|(name, _)| *name == source) {
                    Some((_, Some(scores))) => {
                        let tally = judge(scores, bar, &heard.plays, after_us);
                        lines += heard.plays.len();
                        total.caught += tally.caught;
                        total.false_stops += tally.false_stops;
                        total.other += tally.other;
                        total.latencies_us.extend(tally.latencies_us);
                    }
                    _ => missing += 1,
                }
            }
            let missing = if missing > 0 {
                format!(" ({missing} bundles without it)")
            } else {
                String::new()
            };
            let _ = writeln!(
                out,
                "{source}\t{bar:.2}\t{}/{lines}\t{}\t{}\t{}{missing}",
                total.caught,
                total.false_stops,
                total.other,
                latency(&total.latencies_us)
            );
        }
    }
    out
}

/// A 16 kHz, one-channel, 16-bit WAV's samples, by its `fmt ` and `data`
/// chunks.
fn wav_samples(path: &Path) -> Result<Vec<i16>> {
    let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let bad = |why: &str| format!("{}: {why}", path.display());
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(bad("not a WAV file"));
    }
    let (mut at, mut format_ok) = (12, false);
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]])
            as usize;
        let body = bytes
            .get(at + 8..(at + 8 + size).min(bytes.len()))
            .ok_or_else(|| bad("a chunk past its end"))?;
        if id == b"fmt " && body.len() >= 16 {
            let u16_at = |i: usize| u16::from_le_bytes([body[i], body[i + 1]]);
            let rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
            format_ok = u16_at(0) == 1 && u16_at(2) == 1 && rate == RATE as u32 && u16_at(14) == 16;
        } else if id == b"data" {
            if !format_ok {
                return Err(bad("not 16 kHz, one channel, 16-bit PCM"));
            }
            return Ok(body
                .chunks_exact(2)
                .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
                .collect());
        }
        at += 8 + size + size % 2;
    }
    Err(bad("no data chunk"))
}

#[cfg(test)]
mod tests {
    //! A synthetic bundle, built here: three sources of silence with a loud
    //! burst where "Stop." would be, and a stand-in for the models that
    //! scores a step by its loudest sample. So these cover the bundle's
    //! reading, the lamp's clock, `Quiet` and `Stops` as the product runs
    //! them, and the counting; not what `stop.onnx` hears, which needs the
    //! models (not in the repository) and a recording.
    use super::*;
    use lamp_name::hear::Score;

    /// Scores each step by its loudest sample: 0.99 for a burst, else 0.01.
    #[derive(Default)]
    struct Loudness {
        fed: usize,
        newest: Vec<i16>,
    }

    impl Models for Loudness {
        fn hear(&mut self, samples: &[i16]) -> std::result::Result<Vec<Score>, String> {
            let mut scores = Vec::new();
            for &sample in samples {
                self.newest.push(sample);
                if self.newest.len() > STEP {
                    self.newest.remove(0);
                }
                self.fed += 1;
                if self.fed >= WINDOW && (self.fed - WINDOW).is_multiple_of(STEP) {
                    let loud = self
                        .newest
                        .iter()
                        .any(|sample| sample.unsigned_abs() > 20_000);
                    scores.push(Score {
                        name: 0.0,
                        stop: Some(if loud { 0.99 } else { 0.01 }),
                        rescue: None,
                    });
                }
            }
            Ok(scores)
        }

        fn forget(&mut self) {
            *self = Self::default();
        }
    }

    /// The lamp's clock at the first sample.
    const T0: u64 = 7_000_000_000;
    const SECONDS: usize = 30;

    fn wav_bytes(samples: &[i16]) -> Vec<u8> {
        let data = (samples.len() * 2) as u32;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&16_000u32.to_le_bytes());
        out.extend_from_slice(&32_000u32.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data.to_le_bytes());
        for sample in samples {
            out.extend_from_slice(&sample.to_le_bytes());
        }
        out
    }

    /// Silence with a 100 ms burst starting at each of `at_ms`.
    fn sound(at_ms: &[u64]) -> Vec<i16> {
        let mut samples = vec![0i16; SECONDS * RATE as usize];
        for &ms in at_ms {
            let from = (ms * RATE / 1_000) as usize;
            for sample in &mut samples[from..from + 1_600] {
                *sample = 30_000;
            }
        }
        samples
    }

    /// A replay folder as `lamp-audio replay-aec --linear-output` writes
    /// what this reads; one folder per test, by its own prefix.
    fn bundle(test: &str, sources: &[(&str, Vec<i16>)], plays: &str) -> (Scratch, Bundle) {
        let mut scratch = Scratch::default();
        let dir = scratch.new_folder(&std::env::temp_dir(), &format!("lamp-name-swr-test-{test}"));
        fs::create_dir_all(&dir).unwrap();
        for (source, samples) in sources {
            fs::write(
                dir.join(format!("segment-001-{source}.wav")),
                wav_bytes(samples),
            )
            .unwrap();
        }
        // 10 ms capture blocks, read 10 ms apart on the lamp's clock.
        let mut operations = String::from("{\"kind\":\"segment_start\"}\n");
        for block in 0..SECONDS * 100 {
            let _ = writeln!(
                operations,
                "{{\"kind\":\"capture_pair\",\"segment_capture_sample_offset\":{},\"meta\":{{\"first_read_started_at_us\":{}}}}}",
                block * 160,
                T0 + block as u64 * 10_000
            );
        }
        fs::write(dir.join("operations.jsonl"), operations).unwrap();
        fs::write(dir.join("report.json"), "{\"segments\":[{}]}").unwrap();
        fs::write(dir.join("plays.tsv"), plays).unwrap();
        let options = Options {
            models: PathBuf::from("/unused"),
            bars: vec![0.5],
            after_us: 1_000_000,
            lamp_audio: None,
            workspace: dir.clone(),
            items: Vec::new(),
        };
        let found = resolve(dir.to_str().unwrap(), &options, &mut scratch).unwrap();
        (scratch, found)
    }

    fn us(ms: u64) -> u64 {
        T0 + ms * 1_000
    }

    /// Three "Stop." lines at 5, 12 and 20 s, 500 ms each. pre_aec hears
    /// all three and one burst at 26 s with no line; aec_only only the
    /// second, 300 ms in; aec_linear nothing.
    #[test]
    fn each_source_is_counted_against_the_played_lines() {
        let plays = format!(
            "start_us\tduration_us\ttext\n{}\t500000\tStop.\n{}\t500000\tStop.\n{}\t500000\tStop.\n",
            us(5_000),
            us(12_000),
            us(20_000)
        );
        let (_scratch, bundle) = bundle(
            "counted",
            &[
                ("pre_aec", sound(&[5_100, 12_100, 20_100, 26_000])),
                ("aec_linear", sound(&[])),
                ("aec_only", sound(&[12_300])),
            ],
            &plays,
        );
        assert_eq!(bundle.plays.len(), 3, "the header is passed over");
        let heard = hear_bundle(&mut Quiet::new(Loudness::default(), None), &bundle).unwrap();
        let tally = |source: &str| {
            let (_, scores) = heard
                .sources
                .iter()
                .find(|(name, _)| *name == source)
                .unwrap();
            judge(scores.as_ref().unwrap(), 0.5, &heard.plays, 1_000_000)
        };
        let pre = tally("pre_aec");
        assert_eq!((pre.caught, pre.false_stops, pre.other), (3, 1, 0));
        // A burst is first in the step that ends 0–80 ms after it starts.
        for latency in &pre.latencies_us {
            assert!((100_000..=180_000).contains(latency), "{latency}");
        }
        assert_eq!(tally("aec_linear"), Tally::default());
        let only = tally("aec_only");
        assert_eq!((only.caught, only.false_stops), (1, 0));
        assert!((300_000..=380_000).contains(&only.latencies_us[0]));
        let printed = report("synthetic", &heard, &[0.5], 1_000_000);
        assert!(printed.contains("pre_aec\t0.50\t3/3\t1\t0\t"), "{printed}");
        assert!(
            printed.contains("aec_linear\t0.50\t0/3\t0\t0\t-"),
            "{printed}"
        );
        assert!(printed.contains("aec_only\t0.50\t1/3\t0\t0\t"), "{printed}");
    }

    /// One word heard over several steps is one stop (the product's 1 s
    /// hold-off), and a second word 1.2 s later in the same line's window
    /// is "other", not a second catch nor false. A bar above every score
    /// hears nothing.
    #[test]
    fn the_hold_off_and_the_bar_are_the_products() {
        let plays = format!("{}\t1500000\n", us(5_000));
        let long_word = sound(&[5_000, 5_100, 5_200, 6_400]);
        let (_scratch, bundle) = bundle("hold-off", &[("aec_only", long_word)], &plays);
        let heard = hear_bundle(&mut Quiet::new(Loudness::default(), None), &bundle).unwrap();
        let scores = heard.sources[2].1.as_ref().unwrap();
        let tally = judge(scores, 0.5, &heard.plays, 1_000_000);
        assert_eq!((tally.caught, tally.other, tally.false_stops), (1, 1, 0));
        assert_eq!(
            judge(scores, 1.0, &heard.plays, 1_000_000),
            Tally::default()
        );
    }

    /// A replay made without `--linear-output` says so for that source; it
    /// does not count it as hearing nothing.
    #[test]
    fn a_missing_source_is_said_not_zero() {
        let plays = format!("{}\t500000\n", us(5_000));
        let (_scratch, bundle) = bundle(
            "missing",
            &[("pre_aec", sound(&[5_100])), ("aec_only", sound(&[5_100]))],
            &plays,
        );
        let heard = hear_bundle(&mut Quiet::new(Loudness::default(), None), &bundle).unwrap();
        let printed = report("no-linear", &heard, &[0.5], 1_000_000);
        assert!(
            printed.contains("aec_linear\t0.50\tnot in the replay"),
            "{printed}"
        );
        let both = summary(&[heard], &[0.5], 1_000_000);
        assert!(
            both.contains("aec_linear\t0.50\t0/0\t0\t0\t- (1 bundles without it)"),
            "{both}"
        );
    }

    /// The lamp's clock is the capture block's read time, not the sample
    /// count: a block read late moves the samples after it.
    #[test]
    fn a_sample_is_dated_by_its_blocks_read_time() {
        let clock = Clock::of(
            "{\"kind\":\"capture_pair\",\"segment_capture_sample_offset\":0,\"meta\":{\"first_read_started_at_us\":1000}}\n\
             {\"kind\":\"capture_pair\",\"segment_capture_sample_offset\":160,\"meta\":{\"first_read_started_at_us\":50000}}\n",
        )
        .unwrap();
        assert_eq!(clock.at(0), 1_000);
        assert_eq!(clock.at(80), 6_000);
        assert_eq!(clock.at(160), 50_000);
        assert_eq!(clock.at(320), 60_000);
        assert!(Clock::of("{\"kind\":\"capture_pair\"}\n").is_err());
    }

    /// The recorder's `plays.tsv` lists the requests it played beside the
    /// stops: only a `cut` line is a stop line where the fourth column is
    /// there, and every line where it is not.
    #[test]
    fn only_cut_lines_are_stop_lines_in_a_mixed_plays_file() {
        let mixed = "start_us\tduration_us\ttext\tkind\n\
                     1000\t500\tTell me a story.\trequest\n\
                     5000\t400\tStop.\tcut\n\
                     9000\t600\tWhat is the weather?\trequest\n\
                     12000\t300\tLamp, stop.\t cut \n";
        assert_eq!(
            plays_of(mixed),
            vec![
                Play {
                    start_us: 5000,
                    end_us: 5400
                },
                Play {
                    start_us: 12000,
                    end_us: 12300
                }
            ]
        );
        assert_eq!(plays_of("1000\t500\tStop.\n2000\t500\n").len(), 2);
    }

    #[test]
    fn options_are_read_as_lamp_name_reads_its_bar() {
        let words = |text: &str| text.split(' ').map(str::to_owned).collect::<Vec<_>>();
        let read = options(&words(
            "--models /m --stop-least 0.5,0.9 --after-ms 2000 a b:p",
        ))
        .unwrap();
        assert_eq!(
            (read.bars, read.after_us, read.items),
            (
                vec![0.5, 0.9],
                2_000_000,
                vec!["a".to_owned(), "b:p".to_owned()]
            )
        );
        assert!(options(&words("--models /m --stop-least 0.4 a")).is_err());
        assert!(options(&words("--models /m --models /n a")).is_err());
        assert!(options(&words("--stop-least 0.5 a")).is_err());
        assert!(options(&words("--models /m")).is_err());
    }
}
