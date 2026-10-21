//! Played "Stop." lines over the lamp's own voice, heard by the voice's
//! speech-to-text stop path (`LAMP_STOP_WORDS=heard-always`, #828, the Stop
//! words row) on each stage of the echo canceller: the speech-to-text
//! counterpart of `lamp-name`'s `stop_words_replay`, scored the same way.
//! It runs only when named. Unlike that one it opens a connection: each
//! source's audio goes, as the voice sends it, to the speech-to-text the
//! second route's configuration names (the Autonomous relay to Deepgram on
//! the product). Run it only on recordings made for the purpose, never a
//! person's voice.
//!
//! ```text
//! cargo run --release -p lamp-voice --example stop_words_stt_replay -- \
//!     [--second-config /etc/lampOS/voice/second.json] [--sources raw,linear,processed] \
//!     [--after-ms 1000] [--lamp-words TSV] [--all-sounding] [--words] BUNDLE[:PLAYS] ...
//! ```
//!
//! - `--second-config` is the voice's own second route (default the
//!   product's, [`lamp_voice::second_config::PRODUCT_PATH`]), read as the
//!   voice and `lamp-voice stt-check` read it, the voice unit's environment
//!   (`LAMP_SECOND_SPEECH_MODEL`, the key terms, Flux) over it. Its
//!   speech-to-text credential is the file it names: `deepgram_credential_file`,
//!   else `stt_credential_file`, else `llm_credential_file`, an absolute
//!   path, private. No key is printed: only the host the words are heard at.
//! - A `BUNDLE` is a `lamp-audio replay-aec … --linear-output` output folder
//!   (`operations.jsonl`, `segment-001-<source>.wav`), and `PLAYS` its plays
//!   file as `stop_words_replay` reads it (`start_us<TAB>duration_us…` on
//!   the lamp's clock, a header passed over, and with a fourth column only
//!   its `cut` lines; default `plays.tsv` in it).
//! - `--lamp-words` is what the lamp said, `at_us<TAB>text` on the same
//!   clock, given to the stop words as the voice gives its own words
//!   ([`StopWords::lamp_said`]), so that a "wait" in its story stops
//!   nothing. Without it nothing of the lamp's own words is known, and any
//!   stop word the speech-to-text writes from the lamp's voice counts.
//!
//! For each source asked for, `raw` (`pre_aec`), `linear` (`aec_linear`) and
//! `processed` (`aec_only`), the voice's own path hears the replay **in real
//! time**, the sources side by side, each with a stream of its own:
//! [`StopWords::always`] with [`StopWords::with_source`] over
//! [`RelayEars::start`], as the coordinator builds them, given each 10 ms
//! capture block when the lamp read it. Each block is told speech-like as
//! the lamp's detector scored it (`vad_score` in the recording, at 0.5, as
//! the coordinator tells [`StopWords::heard_speech`]), and the lamp is
//! sounding while the replayed canceller's `render_quiet_blocks` is under
//! the duplex rule's `clear_after_blocks`, as the coordinator decides it
//! (`--all-sounding`: every block). A reply is taken to be under way
//! throughout. A stop ends it, as the hand's Stop does on the lamp, and the
//! next block begins another, with a new stream. A stop is at the time the
//! lamp read the newest sample of the block after which it came.
//!
//! - **A catch:** the first stop from a line's start to its end + `--after-ms`
//!   (1 s, as on #828). Its latency is from the line's start.
//! - **Other:** a second stop in that window, or one in the second before a
//!   line starts: neither a catch nor false.
//! - **A false stop:** one outside every line's window, and a second or more
//!   before every line's start.
//!
//! With `--words`, each result the speech-to-text sent back is printed with
//! its time and whether it stopped the lamp.
use lamp_voice::{
    second_config,
    stop_words::{self, Ears, RelayEars, Source, StopWords},
};
use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

type Result<T> = std::result::Result<T, String>;

/// The sources, the canceller's stage each reads and the voice's name for it.
const SOURCES: [(&str, &str, Source); 3] = [
    ("pre_aec", "raw", Source::Raw),
    ("aec_linear", "linear", Source::Linear),
    ("aec_only", "processed", Source::Processed),
];
/// 10 ms, one capture block, as the coordinator hands them on.
const BLOCK: usize = 160;
const RATE: u64 = 16_000;
const BLOCK_US: u64 = BLOCK as u64 * 1_000_000 / RATE;
/// A stop this near before a line's start is not counted false.
const NEAR_US: u64 = 1_000_000;
/// After the last block, how long results are still waited for.
const TAIL_US: u64 = 3_000_000;
/// The lamp's words reach the stop words this long before they play.
const LAMP_LEAD_US: u64 = 2_000_000;

const USAGE: &str = "usage: stop_words_stt_replay [--second-config FILE] [--sources raw,linear,processed] [--after-ms MS] [--lamp-words TSV] [--all-sounding] [--words] BUNDLE[:PLAYS] ...";

fn main() {
    if let Err(why) = run(std::env::args().skip(1).collect()) {
        eprintln!("stop_words_stt_replay: {why}");
        std::process::exit(1);
    }
}

#[derive(Debug, PartialEq)]
struct Options {
    second_config: PathBuf,
    sources: Vec<usize>,
    after_us: u64,
    lamp_words: Option<PathBuf>,
    all_sounding: bool,
    words: bool,
    items: Vec<String>,
}

fn options(args: &[String]) -> Result<Options> {
    let (mut config, mut sources, mut after_us, mut lamp_words) = (None, None, None, None);
    let (mut all_sounding, mut words) = (false, false);
    let mut items = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = || args.next().cloned().ok_or_else(|| USAGE.to_owned());
        let repeated = match arg.as_str() {
            "--second-config" => config.replace(PathBuf::from(value()?)).is_some(),
            "--sources" => sources.replace(sources_of(&value()?)?).is_some(),
            "--after-ms" => {
                let ms: u64 = value()?
                    .parse()
                    .map_err(|_| "--after-ms is whole milliseconds".to_owned())?;
                after_us.replace(ms * 1_000).is_some()
            }
            "--lamp-words" => lamp_words.replace(PathBuf::from(value()?)).is_some(),
            "--all-sounding" => std::mem::replace(&mut all_sounding, true),
            "--words" => std::mem::replace(&mut words, true),
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
    Ok(Options {
        second_config: config.unwrap_or_else(|| PathBuf::from(second_config::PRODUCT_PATH)),
        sources: sources.unwrap_or_else(|| vec![0, 1, 2]),
        after_us: after_us.unwrap_or(1_000_000),
        lamp_words,
        all_sounding,
        words,
        items,
    })
}

/// `--sources`: the voice's names, by commas, each once, in [`SOURCES`]' order.
fn sources_of(value: &str) -> Result<Vec<usize>> {
    let mut picked = Vec::new();
    for name in value.split(',') {
        let index = SOURCES
            .iter()
            .position(|(_, voice, _)| *voice == name)
            .ok_or("--sources is raw, linear and processed, by commas")?;
        if picked.contains(&index) {
            return Err(format!("--sources names {name} twice"));
        }
        picked.push(index);
    }
    picked.sort_unstable();
    Ok(picked)
}

fn run(args: Vec<String>) -> Result<()> {
    let options = options(&args)?;
    // The voice's own files of what it heard are for the lamp, not a replay.
    for name in [
        stop_words::AUDIO_ENVIRONMENT,
        stop_words::COUNTS_ENVIRONMENT,
    ] {
        if std::env::var_os(name).is_some() {
            return Err(format!("{name} is set: unset it for a replay"));
        }
    }
    let config = second_config::load_from_path(&options.second_config)
        .and_then(|config| config.with_environment(|name| std::env::var_os(name)))
        .map_err(|error| format!("{}: {error}", options.second_config.display()))?;
    eprintln!(
        "stop_words_stt_replay: the words are heard at {}, in real time",
        stop_words::where_heard(&config)
    );
    let lamp_words = match &options.lamp_words {
        Some(path) => lamp_words_of(
            &fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?,
        ),
        None => Vec::new(),
    };
    let mut all = Vec::new();
    for item in &options.items {
        let bundle = resolve(item)?;
        let heard = hear_bundle(&bundle, &options, &lamp_words, |_| {
            RelayEars::start(config.clone(), true)
                .map(|ears| Box::new(ears) as Box<dyn Ears>)
                .map_err(|error| error.to_string())
        })?;
        print!("{}", report(&bundle.name, &heard, options.after_us));
        if options.words {
            print!("{}", words_heard(&heard, options.after_us));
        }
        all.push(heard);
    }
    if all.len() > 1 {
        print!("{}", summary(&all, options.after_us));
    }
    Ok(())
}

/// A bundle ready to hear: its name, its replay and its plays.
struct Bundle {
    name: String,
    replay: PathBuf,
    plays: Vec<Play>,
}

fn resolve(item: &str) -> Result<Bundle> {
    let (path, plays) = match item.rsplit_once(':') {
        Some((path, plays)) if !plays.is_empty() => (path, Some(PathBuf::from(plays))),
        _ => (item, None),
    };
    let given = PathBuf::from(path);
    let name = given
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .ok_or_else(|| format!("{path}: not a bundle"))?;
    let replay = find(&given, |dir| dir.join("segment-001-aec_only.wav").is_file())
        .ok_or_else(|| format!("{path}: not a replay-aec output (segment-001-aec_only.wav)"))?;
    let plays = match plays {
        Some(plays) => plays,
        None => [replay.join("plays.tsv"), given.join("plays.tsv")]
            .into_iter()
            .find(|plays| plays.is_file())
            .ok_or_else(|| format!("{path}: no plays.tsv in it; give BUNDLE:PLAYS"))?,
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

/// A played line, on the lamp's clock.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Play {
    start_us: u64,
    end_us: u64,
}

/// `start_us<TAB>duration_us[<TAB>text[<TAB>kind…]]` a line, as
/// `stop_words_replay` reads it: lines that are not (a header) are passed
/// over, and so is a line whose fourth column is there and not `cut`: a
/// request the recorder played, not a stop.
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

/// `at_us<TAB>text` a line, in time order; others are passed over.
fn lamp_words_of(text: &str) -> Vec<(u64, String)> {
    let mut words: Vec<(u64, String)> = text
        .lines()
        .filter_map(|line| {
            let (at, said) = line.split_once('\t')?;
            Some((at.trim().parse().ok()?, said.trim().to_owned()))
        })
        .collect();
    words.sort_by_key(|(at, _)| *at);
    words
}

/// One capture block of the replay: its first sample, when the lamp read
/// its newest, the lamp's detector's score and whether the lamp sounded.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Block {
    offset: usize,
    at_us: u64,
    vad: f32,
    sounding: bool,
}

/// The capture blocks in `operations.jsonl`, each dated as
/// `stop_words_replay` dates a sample: its `first_read_started_at_us` and
/// its samples at the capture rate.
fn blocks_of(operations: &str, all_sounding: bool) -> Result<Vec<Block>> {
    let clear = lamp_audio::duplex::Hearing::EVALUATED.clear_after_blocks;
    let mut blocks = Vec::new();
    for (number, line) in operations.lines().enumerate() {
        let record: serde_json::Value = serde_json::from_str(line)
            .map_err(|error| format!("operations.jsonl line {}: {error}", number + 1))?;
        if record["kind"] != "capture_pair" {
            continue;
        }
        let offset = record["segment_capture_sample_offset"].as_u64();
        let read_at = record["meta"]["first_read_started_at_us"].as_u64();
        let vad = record["meta"]["vad_score"].as_f64();
        let (Some(offset), Some(read_at), Some(vad)) = (offset, read_at, vad) else {
            return Err(format!(
                "operations.jsonl line {}: a capture_pair without its offset, read time or vad_score",
                number + 1
            ));
        };
        // The coordinator's `sounding`: the canceller's evidence for the
        // block, and the lamp's sound not quiet for `clear_after_blocks`.
        let quiet = record["echo_evidence"]["aec_only"]["render_quiet_blocks"].as_u64();
        blocks.push(Block {
            offset: offset as usize,
            at_us: read_at + (BLOCK as u64 - 1) * 1_000_000 / RATE,
            vad: vad as f32,
            sounding: all_sounding || quiet.is_some_and(|quiet| quiet < u64::from(clear)),
        });
    }
    if blocks.is_empty()
        || blocks
            .windows(2)
            .any(|pair| pair[1].offset <= pair[0].offset)
    {
        return Err("operations.jsonl: no capture blocks in order".to_owned());
    }
    Ok(blocks)
}

/// A stop, or a result that did not stop, at the lamp's time.
#[derive(Clone, Debug, PartialEq)]
struct Heard {
    at_us: u64,
    words: String,
    stopped: bool,
}

/// A bundle heard: per source what came back, or `None` for a source the
/// replay has not.
struct HeardBundle {
    plays: Vec<Play>,
    seconds: f64,
    blocks: usize,
    sounding: usize,
    sources: Vec<(&'static str, Option<Vec<Heard>>)>,
}

/// The ears given to [`StopWords`], every result they give noted on the way
/// to it.
struct Tap {
    ears: Box<dyn Ears>,
    results: Arc<Mutex<Vec<String>>>,
}

impl Ears for Tap {
    fn ready(&mut self) {
        self.ears.ready();
    }
    fn hear(&mut self, captured_at_us: u64, samples: &[i16]) {
        self.ears.hear(captured_at_us, samples);
    }
    fn finalize(&mut self) {
        self.ears.finalize();
    }
    fn flush(&mut self) {
        self.ears.flush();
    }
    fn words(&mut self) -> Option<String> {
        let words = self.ears.words();
        if let Some(words) = &words {
            self.results
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(words.clone());
        }
        words
    }
    fn forget(&mut self) {
        self.ears.forget();
    }
    fn rest(&mut self) {
        self.ears.rest();
    }
}

/// How the blocks are given: when the lamp read them, on this machine's
/// monotonic clock as the voice stamps them (`Live`), or at once, stamped
/// with the lamp's own time (`AtOnce`, the tests' scripted ears).
#[derive(Clone, Copy, PartialEq)]
struct Pace {
    live: bool,
}

impl Pace {
    const LIVE: Self = Self { live: true };
    #[cfg(test)]
    const AT_ONCE: Self = Self { live: false };
}

/// The source's samples heard by the voice's stop words, block by block.
fn drive(
    ears: Box<dyn Ears>,
    source: Source,
    samples: &[i16],
    blocks: &[Block],
    lamp_words: &[(u64, String)],
    pace: Pace,
) -> Vec<Heard> {
    let results = Arc::new(Mutex::new(Vec::new()));
    let tap = Tap {
        ears,
        results: Arc::clone(&results),
    };
    let mut words = StopWords::always_counted(Box::new(tap), None).with_source(source);
    let lamp0 = blocks[0].at_us;
    let live0 = lamp_ipc::monotonic_us();
    let stamp = |lamp_at: u64| {
        if pace.live {
            live0 + lamp_at.saturating_sub(lamp0)
        } else {
            lamp_at
        }
    };
    let mut heard = Vec::new();
    let mut said = lamp_words.iter().peekable();
    let last = blocks[blocks.len() - 1].at_us;
    // The replay's blocks, then the tail: the lamp silent, results waited for.
    let tail = (1..=TAIL_US / BLOCK_US).map(|n| Block {
        offset: usize::MAX,
        at_us: last + n * BLOCK_US,
        vad: 0.0,
        sounding: false,
    });
    for block in blocks.iter().copied().chain(tail) {
        let captured_at_us = stamp(block.at_us);
        if pace.live {
            let now = lamp_ipc::monotonic_us();
            if captured_at_us > now {
                std::thread::sleep(Duration::from_micros(captured_at_us - now));
            }
        }
        while let Some((at, text)) = said.next_if(|(at, _)| *at <= block.at_us + LAMP_LEAD_US) {
            words.lamp_said(text, stamp(*at), true);
        }
        let audio = samples
            .get(block.offset..block.offset.saturating_add(BLOCK))
            .unwrap_or(&[]);
        words.heard_speech(block.vad.is_finite() && block.vad >= 0.5);
        words.block(
            true,
            block.sounding,
            false,
            None,
            None,
            captured_at_us,
            audio,
        );
        let stopped = words.spoken_stop();
        let came: Vec<String> =
            std::mem::take(&mut *results.lock().unwrap_or_else(PoisonError::into_inner));
        let count = came.len();
        for (n, text) in came.into_iter().enumerate() {
            heard.push(Heard {
                at_us: block.at_us,
                words: text,
                // The stop is the block's last result read: the one that
                // stopped it ends the reading for that reply's stop.
                stopped: stopped && n + 1 == count,
            });
        }
        if stopped {
            // The reply ends, as the hand's Stop ends it; the next block
            // begins another.
            words.block(false, false, false, None, None, captured_at_us, &[]);
        }
    }
    words.block(false, false, false, None, None, stamp(last + TAIL_US), &[]);
    heard
}

fn hear_bundle(
    bundle: &Bundle,
    options: &Options,
    lamp_words: &[(u64, String)],
    ears: impl Fn(Source) -> Result<Box<dyn Ears>> + Sync,
) -> Result<HeardBundle> {
    hear_bundle_paced(bundle, options, lamp_words, ears, Pace::LIVE)
}

fn hear_bundle_paced(
    bundle: &Bundle,
    options: &Options,
    lamp_words: &[(u64, String)],
    ears: impl Fn(Source) -> Result<Box<dyn Ears>> + Sync,
    pace: Pace,
) -> Result<HeardBundle> {
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
    let blocks = blocks_of(
        &fs::read_to_string(&operations)
            .map_err(|error| format!("{}: {error}", operations.display()))?,
        options.all_sounding,
    )?;
    let mut wavs = Vec::new();
    for &index in &options.sources {
        let (stage, _, source) = SOURCES[index];
        let wav = bundle.replay.join(format!("segment-001-{stage}.wav"));
        wavs.push((
            stage,
            source,
            wav.is_file().then(|| wav_samples(&wav)).transpose()?,
        ));
    }
    let seconds = wavs
        .iter()
        .filter_map(|(_, _, samples)| samples.as_ref())
        .map(|samples| samples.len() as f64 / RATE as f64)
        .fold(0.0, f64::max);
    let ears = &ears;
    let blocks_ref = &blocks;
    let sources = std::thread::scope(|scope| {
        let running: Vec<_> = wavs
            .iter()
            .map(|(stage, source, samples)| {
                let handle = samples.as_ref().map(|samples| {
                    scope.spawn(move || {
                        ears(*source)
                            .map(|ears| drive(ears, *source, samples, blocks_ref, lamp_words, pace))
                    })
                });
                (*stage, handle)
            })
            .collect();
        running
            .into_iter()
            .map(|(stage, handle)| {
                let heard = match handle {
                    Some(handle) => Some(
                        handle
                            .join()
                            .map_err(|_| format!("{stage}: its thread failed"))??,
                    ),
                    None => None,
                };
                Ok((stage, heard))
            })
            .collect::<Result<Vec<_>>>()
    })?;
    Ok(HeardBundle {
        plays: bundle.plays.clone(),
        seconds,
        blocks: blocks.len(),
        sounding: blocks.iter().filter(|block| block.sounding).count(),
        sources,
    })
}

/// Each source's stops against the played lines.
#[derive(Debug, Default, PartialEq)]
struct Tally {
    caught: usize,
    false_stops: usize,
    other: usize,
    /// Each caught line's first stop, from the line's start.
    latencies_us: Vec<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Catch,
    Other,
    False,
}

/// The same rule as `stop_words_replay`'s: each stop, in order, against
/// the played lines.
fn kinds(heard: &[Heard], plays: &[Play], after_us: u64) -> Vec<(usize, Kind)> {
    let mut first = vec![false; plays.len()];
    let mut kinds = Vec::new();
    for (index, stop) in heard.iter().enumerate().filter(|(_, heard)| heard.stopped) {
        let at = stop.at_us;
        let kind = match plays
            .iter()
            .position(|play| play.start_us <= at && at < play.end_us + after_us)
        {
            Some(line) if !first[line] => {
                first[line] = true;
                Kind::Catch
            }
            Some(_) => Kind::Other,
            None if plays
                .iter()
                .any(|play| play.start_us.saturating_sub(NEAR_US) <= at && at < play.start_us) =>
            {
                Kind::Other
            }
            None => Kind::False,
        };
        kinds.push((index, kind));
    }
    kinds
}

fn judge(heard: &[Heard], plays: &[Play], after_us: u64) -> Tally {
    let mut tally = Tally::default();
    for (index, kind) in kinds(heard, plays, after_us) {
        match kind {
            Kind::Catch => {
                let at = heard[index].at_us;
                let line = plays
                    .iter()
                    .find(|play| play.start_us <= at && at < play.end_us + after_us)
                    .expect("a catch is in a line");
                tally.latencies_us.push(at - line.start_us);
            }
            Kind::Other => tally.other += 1,
            Kind::False => tally.false_stops += 1,
        }
    }
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

/// `stop_words_replay`'s table; the bar is the speech-to-text's.
const HEADER: &str = "source\tbar\tcatches\tfalse stops\tother\tfirst-catch latency (median, from the line's start)\n";

fn report(name: &str, heard: &HeardBundle, after_us: u64) -> String {
    let mut out = format!(
        "{name}: {} played lines, {:.1} min replayed, the lamp sounding in {} of {} blocks\n{HEADER}",
        heard.plays.len(),
        heard.seconds / 60.0,
        heard.sounding,
        heard.blocks
    );
    for (source, results) in &heard.sources {
        let Some(results) = results else {
            let _ = writeln!(out, "{source}\tstt\tnot in the replay");
            continue;
        };
        let tally = judge(results, &heard.plays, after_us);
        let _ = writeln!(
            out,
            "{source}\tstt\t{}/{}\t{}\t{}\t{}",
            tally.caught,
            heard.plays.len(),
            tally.false_stops,
            tally.other,
            latency(&tally.latencies_us)
        );
    }
    out
}

/// `--words`: each result, from the first block's time, and what it was.
fn words_heard(heard: &HeardBundle, after_us: u64) -> String {
    let mut out = String::new();
    let t0 = heard.plays.first().map_or(0, |play| play.start_us);
    for (source, results) in &heard.sources {
        let Some(results) = results else { continue };
        let kinds = kinds(results, &heard.plays, after_us);
        for (index, result) in results.iter().enumerate() {
            let kind =
                kinds
                    .iter()
                    .find(|(at, _)| *at == index)
                    .map_or("", |(_, kind)| match kind {
                        Kind::Catch => "\tstop: catch",
                        Kind::Other => "\tstop: other",
                        Kind::False => "\tstop: false",
                    });
            let _ = writeln!(
                out,
                "{source}\t{:+.2} s\t{}{kind}",
                (result.at_us as f64 - t0 as f64) / 1e6,
                result.words
            );
        }
    }
    out
}

/// Every bundle together, by source.
fn summary(all: &[HeardBundle], after_us: u64) -> String {
    let mut out = format!("all {} bundles\n{HEADER}", all.len());
    let mut names: Vec<&str> = Vec::new();
    for heard in all {
        for (source, _) in &heard.sources {
            if !names.contains(source) {
                names.push(source);
            }
        }
    }
    for source in names {
        let (mut lines, mut total, mut missing) = (0, Tally::default(), 0);
        for heard in all {
            match heard.sources.iter().find(|(name, _)| *name == source) {
                Some((_, Some(results))) => {
                    let tally = judge(results, &heard.plays, after_us);
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
            "{source}\tstt\t{}/{lines}\t{}\t{}\t{}{missing}",
            total.caught,
            total.false_stops,
            total.other,
            latency(&total.latencies_us)
        );
    }
    out
}

/// A 16 kHz, one-channel, 16-bit WAV's samples, as the voice reads one
/// (`stt-check`).
fn wav_samples(path: &Path) -> Result<Vec<i16>> {
    let bad = |why: String| format!("{}: {why}", path.display());
    let mut reader = hound::WavReader::open(path).map_err(|error| bad(error.to_string()))?;
    let spec = reader.spec();
    if (
        spec.sample_rate,
        spec.channels,
        spec.bits_per_sample,
        spec.sample_format,
    ) != (16_000, 1, 16, hound::SampleFormat::Int)
    {
        return Err(bad("not 16 kHz, one channel, 16-bit PCM".to_owned()));
    }
    reader
        .samples::<i16>()
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| bad(error.to_string()))
}

#[cfg(test)]
mod tests {
    //! Scripted ears in place of the relay: they give set words once a set
    //! capture time has been heard. So these cover the bundle's reading,
    //! the voice's own `StopWords` (its matcher, `heard-always`'s handling
    //! of results, the source's cadence, sending only while the lamp
    //! sounds), the reply begun again after a stop, the lamp's own words,
    //! and the counting. Not the relay or what Deepgram writes from the
    //! audio: `RelayEars` and `SttStream` are the voice's, tested in its
    //! crate, and only a run with a credential hears the bundles.
    use super::*;

    #[derive(Default, Debug, PartialEq)]
    struct Counts {
        ready: usize,
        heard: usize,
        flushes: usize,
        rests: usize,
    }

    /// Gives each `(at_us, words)` once a block captured at or after
    /// `at_us` was heard, while a reply is under way.
    struct Scripted {
        script: Vec<(u64, &'static str)>,
        newest: u64,
        open: bool,
        counts: Arc<Mutex<Counts>>,
    }

    impl Ears for Scripted {
        fn ready(&mut self) {
            self.open = true;
            self.counts.lock().unwrap().ready += 1;
        }
        fn hear(&mut self, captured_at_us: u64, _: &[i16]) {
            self.newest = captured_at_us;
            self.counts.lock().unwrap().heard += 1;
        }
        fn finalize(&mut self) {}
        fn flush(&mut self) {
            self.counts.lock().unwrap().flushes += 1;
        }
        fn words(&mut self) -> Option<String> {
            if !self.open || self.script.first().is_none_or(|(at, _)| *at > self.newest) {
                return None;
            }
            Some(self.script.remove(0).1.to_owned())
        }
        fn forget(&mut self) {}
        fn rest(&mut self) {
            self.open = false;
            self.counts.lock().unwrap().rests += 1;
        }
    }

    /// The lamp's clock at the first sample.
    const T0: u64 = 7_000_000_000;
    const SECONDS: usize = 30;

    fn us(ms: u64) -> u64 {
        T0 + ms * 1_000
    }

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

    /// Folders the tests made, removed when each ends.
    struct Folder(PathBuf);

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A replay folder as `replay-aec --linear-output` writes what this
    /// reads: the lamp sounding but in `quiet_ms`, speech-like in `vad_ms`.
    fn bundle(test: &str, stages: &[&str], quiet_ms: (u64, u64), plays: &str) -> (Folder, Bundle) {
        let dir = std::env::temp_dir().join(format!(
            "lamp-voice-swsr-test-{test}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let folder = Folder(dir.clone());
        for stage in stages {
            fs::write(
                dir.join(format!("segment-001-{stage}.wav")),
                wav_bytes(&vec![0i16; SECONDS * RATE as usize]),
            )
            .unwrap();
        }
        let mut operations = String::from("{\"kind\":\"segment_start\"}\n");
        for block in 0..SECONDS as u64 * 100 {
            let ms = block * 10;
            let quiet = if (quiet_ms.0..quiet_ms.1).contains(&ms) {
                64
            } else {
                0
            };
            let _ = writeln!(
                operations,
                "{{\"kind\":\"capture_pair\",\"segment_capture_sample_offset\":{},\"meta\":{{\"first_read_started_at_us\":{},\"vad_score\":0.1}},\"echo_evidence\":{{\"aec_only\":{{\"render_quiet_blocks\":{quiet}}}}}}}",
                block * 160,
                us(ms)
            );
        }
        fs::write(dir.join("operations.jsonl"), operations).unwrap();
        fs::write(dir.join("report.json"), "{\"segments\":[{}]}").unwrap();
        fs::write(dir.join("plays-stops.tsv"), plays).unwrap();
        let found = resolve(&format!(
            "{}:{}",
            dir.display(),
            dir.join("plays-stops.tsv").display()
        ))
        .unwrap();
        (folder, found)
    }

    fn opts(sources: &[usize]) -> Options {
        Options {
            second_config: PathBuf::from("/unused"),
            sources: sources.to_vec(),
            after_us: 1_000_000,
            lamp_words: None,
            all_sounding: false,
            words: false,
            items: Vec::new(),
        }
    }

    fn three_lines() -> String {
        format!(
            "start_us\tduration_us\ttext\n{}\t500000\tStop.\n{}\t500000\tLamp, stop.\n{}\t500000\tOkay, stop.\n",
            us(5_000),
            us(12_000),
            us(20_000)
        )
    }

    /// What each source's scripted ears say, as Deepgram might over the
    /// lamp: the first line caught 0.6 s in, a story's "wait" with no line,
    /// "don't stop" turned round, the second line caught, then a repeat in
    /// its window after the reply began again, and the third line missed.
    fn script() -> Vec<(u64, &'static str)> {
        vec![
            (us(5_600), "Stop."),
            (us(9_000), "and the fox had to wait"),
            (us(12_300), "don't stop"),
            (us(12_800), "Lamp, stop."),
            (us(13_200), "stop"),
            (us(20_900), "okay"),
        ]
    }

    #[test]
    fn the_voices_stop_words_are_counted_against_the_played_lines() {
        let (_folder, bundle) = bundle("counted", &["pre_aec", "aec_only"], (0, 0), &three_lines());
        assert_eq!(bundle.plays.len(), 3, "the header is passed over");
        let counts = Arc::new(Mutex::new(Counts::default()));
        let shared = Arc::clone(&counts);
        let heard = hear_bundle_paced(
            &bundle,
            &opts(&[0, 1, 2]),
            &[],
            move |source| {
                // Only the processed source's ears are counted.
                let counts = if source == Source::Processed {
                    Arc::clone(&shared)
                } else {
                    Arc::new(Mutex::new(Counts::default()))
                };
                Ok(Box::new(Scripted {
                    script: script(),
                    newest: 0,
                    open: false,
                    counts,
                }) as Box<dyn Ears>)
            },
            Pace::AT_ONCE,
        )
        .unwrap();
        for (source, results) in &heard.sources {
            if *source == "aec_linear" {
                assert!(results.is_none());
                continue;
            }
            let results = results.as_ref().unwrap();
            assert_eq!(results.len(), 6, "{source}: every result is kept");
            let tally = judge(results, &heard.plays, 1_000_000);
            assert_eq!(
                (tally.caught, tally.false_stops, tally.other),
                (2, 1, 1),
                "{source}: {results:?}"
            );
            assert_eq!(tally.latencies_us, vec![609_937, 809_937], "{source}");
        }
        // Four stops: the first reply, and one begun after each.
        let counts = counts.lock().unwrap();
        assert_eq!((counts.ready, counts.rests), (5, 5));
        assert_eq!(counts.heard, 3_000, "every block the lamp sounds in");
        let printed = report("synthetic", &heard, 1_000_000);
        assert!(
            printed.contains("pre_aec\tstt\t2/3\t1\t1\t0.61 s (max 0.81 s)"),
            "{printed}"
        );
        assert!(
            printed.contains("aec_linear\tstt\tnot in the replay"),
            "{printed}"
        );
        assert!(printed.contains("aec_only\tstt\t2/3\t1\t1\t"), "{printed}");
        let words = words_heard(&heard, 1_000_000);
        assert!(
            words.contains("aec_only\t+0.61 s\tStop.\tstop: catch"),
            "{words}"
        );
        assert!(
            words.contains("and the fox had to wait\tstop: false"),
            "{words}"
        );
        assert!(words.contains("don't stop\n"), "{words}");
        let both = summary(&[heard], 1_000_000);
        assert!(
            both.contains("aec_linear\tstt\t0/0\t0\t0\t- (1 bundles without it)"),
            "{both}"
        );
    }

    /// The lamp's own "wait", given as the voice gives its words, stops
    /// nothing: the voice's own `stop_over`.
    #[test]
    fn the_lamps_own_words_stop_nothing() {
        let (_folder, bundle) = bundle("own", &["aec_only"], (0, 0), &three_lines());
        let lamp = lamp_words_of(&format!(
            "at_us\ttext\n{}\tAnd the fox had to wait.\n",
            us(8_500)
        ));
        let heard = hear_bundle_paced(
            &bundle,
            &opts(&[2]),
            &lamp,
            |_| {
                Ok(Box::new(Scripted {
                    script: script(),
                    newest: 0,
                    open: false,
                    counts: Arc::default(),
                }) as Box<dyn Ears>)
            },
            Pace::AT_ONCE,
        )
        .unwrap();
        let tally = judge(
            heard.sources[0].1.as_ref().unwrap(),
            &heard.plays,
            1_000_000,
        );
        assert_eq!((tally.caught, tally.false_stops, tally.other), (2, 0, 1));
    }

    /// Only blocks while the lamp sounds go to the speech-to-text, and the
    /// raw source is asked for its words on the cadence too, as the voice
    /// does; the processed one only at a pause after speech.
    #[test]
    fn only_the_lamps_sounding_is_sent_and_raw_is_asked_on_the_cadence() {
        let (_folder, bundle) = bundle(
            "sounding",
            &["pre_aec", "aec_only"],
            (10_000, 30_000),
            "0\t1\n",
        );
        let opened = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&opened);
        let heard = hear_bundle_paced(
            &bundle,
            &opts(&[0, 2]),
            &[],
            move |source| {
                let counts = Arc::new(Mutex::new(Counts::default()));
                kept.lock().unwrap().push((source, Arc::clone(&counts)));
                Ok(Box::new(Scripted {
                    script: Vec::new(),
                    newest: 0,
                    open: false,
                    counts,
                }) as Box<dyn Ears>)
            },
            Pace::AT_ONCE,
        )
        .unwrap();
        assert_eq!((heard.sounding, heard.blocks), (1_000, 3_000));
        for (source, counts) in opened.lock().unwrap().iter() {
            let counts = counts.lock().unwrap();
            assert_eq!(counts.heard, 1_000, "{source:?}");
            let flushes = if *source == Source::Raw { 20 } else { 0 };
            assert_eq!(counts.flushes, flushes, "{source:?}");
        }
    }

    #[test]
    fn blocks_are_dated_and_scored_as_the_lamp_read_them() {
        let blocks = blocks_of(
            "{\"kind\":\"capture_pair\",\"segment_capture_sample_offset\":0,\"meta\":{\"first_read_started_at_us\":1000,\"vad_score\":0.7},\"echo_evidence\":{\"aec_only\":{\"render_quiet_blocks\":29}}}\n\
             {\"kind\":\"capture_pair\",\"segment_capture_sample_offset\":160,\"meta\":{\"first_read_started_at_us\":50000,\"vad_score\":0.2},\"echo_evidence\":{\"aec_only\":null}}\n\
             {\"kind\":\"capture_pair\",\"segment_capture_sample_offset\":320,\"meta\":{\"first_read_started_at_us\":60000,\"vad_score\":0.2},\"echo_evidence\":{\"aec_only\":{\"render_quiet_blocks\":30}}}\n",
            false,
        )
        .unwrap();
        assert_eq!(
            blocks
                .iter()
                .map(|b| (b.at_us, b.sounding))
                .collect::<Vec<_>>(),
            vec![(10_937, true), (59_937, false), (69_937, false)]
        );
        assert_eq!(blocks[0].vad, 0.7);
        let all = blocks_of(
            "{\"kind\":\"capture_pair\",\"segment_capture_sample_offset\":0,\"meta\":{\"first_read_started_at_us\":1000,\"vad_score\":0.7}}\n",
            true,
        )
        .unwrap();
        assert!(all[0].sounding);
        assert!(blocks_of("{\"kind\":\"capture_pair\",\"segment_capture_sample_offset\":0,\"meta\":{\"first_read_started_at_us\":1}}\n", false).is_err());
    }

    /// The recorder's own `plays.tsv`: only its `cut` lines are stops, as
    /// `stop_words_replay` reads them.
    #[test]
    fn only_cut_lines_are_stop_lines_in_a_mixed_plays_file() {
        let mixed = "start_us\tduration_us\ttext\tkind\n\
                     1000\t500\tTell me a story.\trequest\n\
                     5000\t400\tStop.\tcut\n\
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
    fn options_are_read() {
        let words = |text: &str| text.split(' ').map(str::to_owned).collect::<Vec<_>>();
        let read = options(&words(
            "--second-config /c --sources processed,raw --after-ms 2000 --words a b:p",
        ))
        .unwrap();
        assert_eq!(
            (
                read.second_config,
                read.sources,
                read.after_us,
                read.words,
                read.items
            ),
            (
                PathBuf::from("/c"),
                vec![0, 2],
                2_000_000,
                true,
                vec!["a".to_owned(), "b:p".to_owned()]
            )
        );
        let default = options(&words("a")).unwrap();
        assert_eq!(
            default.second_config,
            PathBuf::from(second_config::PRODUCT_PATH)
        );
        assert_eq!(default.sources, vec![0, 1, 2]);
        assert!(options(&words("--sources raw,raw a")).is_err());
        assert!(options(&words("--sources pre_aec a")).is_err());
        assert!(options(&words("--words --words a")).is_err());
        assert!(options(&words("--after-ms 1")).is_err());
    }
}
