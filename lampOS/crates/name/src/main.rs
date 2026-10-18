//! `lamp-name`: listen to what the microphone hears, on the bus, for the
//! lamp's name, and say when it was heard. See `docs/name.md`.
use lamp_bus::{Bus, Notice, Publisher};
use lamp_msgs::{Audio, Diagnostic, Level, NameHeard, StopHeard};
use lamp_name::{
    hear::Hearer,
    quiet::{self, Loudness, Quiet},
    rescue::Rescue,
    said::{Hearings, Stops},
};
use std::{
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
    time::{Duration, Instant},
};

const USAGE: &str = "usage: lamp-name --models ABSOLUTE_DIR [--microphone mic1|mic2] [--least SCORE] [--quiet-below DBFS] [--stop-least SCORE] [--rescue-model ABSOLUTE_FILE --rescue-sha256 HEX [--rescue-gate SCORE]] [--bus-root ABSOLUTE_DIR]\n       lamp-name check MODELS_DIR [--quiet-below DBFS] [--stop-least SCORE] [--rescue-model FILE --rescue-sha256 HEX [--rescue-gate SCORE]] FILE...";
const HELP: &str = "Hears the lamp's name in what its microphone hears. See docs/name.md.

  --models      a directory holding melspectrogram.onnx, embedding_model.onnx and name.onnx
  --microphone  whose sound to listen to, as the voice publishes it on the bus (mic2)
  --least       the score from which the name counts as heard, 0.5 to 1 (0.9)
  --quiet-below a level in dBFS, -90 to -30: the models rest while the last 2.03 s held
                no sample this loud, and give the same scores when one comes (off)
  --stop-least  also run stop.onnx (beside the others, SHA-256 checked) on the same numbers,
                and publish stop/heard at the step its score first reaches this, 0.5 to 1;
                then nothing new for 1 s (#828, docs/stop-word.md). Off unless set
  --rescue-model  a second name model on the same numbers (#1546, docs/name.md): where
                name.onnx reached --rescue-gate (0.5) in the last second, a step's score
                is the higher of the two. Refused unless its SHA-256 is --rescue-sha256.
                Off unless set
  --bus-root    the bus's directory (LAMP_BUS_ROOT, or /run/lampos-bus)

It listens to MICROPHONE/audio and publishes name/rising as each hearing begins,
name/heard when it is over, and how it is on diagnostics.
It keeps no sound and writes none down.

lamp-name check MODELS_DIR FILE... runs the same models over files of sound (16 kHz, one
channel, 16 bits, raw or WAV) and prints for each every time it would have said the name
was heard (seconds into the file, and the score), its highest score and when, and then how
long a step took: for trying the models where they are installed. With --quiet-below it
rests as the program would, and says how many steps rested. It touches no bus.";

/// The name it publishes under, and reports as.
const NAME: &str = "name";
const DEFAULT_LEAST: f32 = 0.9;
/// Sound that stops coming for this long is a gap: what the models held
/// is forgotten, and a hearing that was going on is said.
const GAP: Duration = Duration::from_millis(500);

#[derive(Debug, PartialEq)]
struct Arguments {
    models: PathBuf,
    microphone: String,
    least: f32,
    quiet_below: Option<f32>,
    /// `--stop-least`: the stop model's bar, and with it the model.
    stop_least: Option<f32>,
    /// `--rescue-model`, its SHA-256 and its gate.
    rescue: Option<RescueModel>,
    bus: Option<PathBuf>,
}

/// The rescue model (`rescue.rs`): its file, the SHA-256 it must have,
/// and the installed model's score that opens it.
#[derive(Clone, Debug, PartialEq)]
struct RescueModel {
    path: PathBuf,
    sha256: String,
    gate: f32,
}

/// The gate when `--rescue-gate` is left out: what was measured (#1546).
const DEFAULT_RESCUE_GATE: f32 = 0.5;

/// The three `--rescue-*` options as given, read in any order.
#[derive(Default)]
struct RescueGiven {
    path: Option<PathBuf>,
    sha256: Option<String>,
    gate: Option<f32>,
}

impl RescueGiven {
    /// One of the three: whether it is one, and an error if it is given
    /// twice or its value is not of its kind.
    fn take(&mut self, option: &str, value: &str) -> Result<bool, String> {
        let twice = match option {
            "--rescue-model" => self.path.replace(PathBuf::from(value)).is_some(),
            "--rescue-sha256" => {
                if value.len() != 64
                    || !value
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                {
                    return Err("--rescue-sha256 is 64 lower-case hex digits".into());
                }
                self.sha256.replace(value.to_owned()).is_some()
            }
            "--rescue-gate" => {
                let gate: f32 = value
                    .parse()
                    .ok()
                    .filter(|gate| (0.05..=0.95).contains(gate))
                    .ok_or("--rescue-gate is a score from 0.05 to 0.95")?;
                self.gate.replace(gate).is_some()
            }
            _ => return Ok(false),
        };
        if twice {
            return Err(format!("{option} was given twice"));
        }
        Ok(true)
    }

    /// The rescue, if a model was given: with its SHA-256, which it needs.
    fn done(self) -> Result<Option<RescueModel>, String> {
        match (self.path, self.sha256) {
            (Some(path), Some(sha256)) => Ok(Some(RescueModel {
                path,
                sha256,
                gate: self.gate.unwrap_or(DEFAULT_RESCUE_GATE),
            })),
            (Some(_), None) => Err("--rescue-model needs --rescue-sha256".into()),
            (None, sha256) if sha256.is_some() || self.gate.is_some() => {
                Err("--rescue-sha256 and --rescue-gate need --rescue-model".into())
            }
            (None, _) => Ok(None),
        }
    }
}

fn parse(mut arguments: impl Iterator<Item = String>) -> Result<Option<Arguments>, String> {
    let (mut models, mut microphone, mut least, mut bus) = (None, None, None, None);
    let (mut quiet_below, mut stop_least) = (None, None);
    let mut rescue = RescueGiven::default();
    while let Some(option) = arguments.next() {
        if option == "--help" || option == "-h" {
            return Ok(None);
        }
        let value = arguments
            .next()
            .ok_or_else(|| format!("{option} needs a value"))?;
        let absolute = |value: &str| {
            let path = PathBuf::from(value);
            path.is_absolute()
                .then_some(path)
                .ok_or_else(|| format!("{option} must be an absolute path"))
        };
        let twice = match option.as_str() {
            "--models" => models.replace(absolute(&value)?).is_some(),
            "--bus-root" => bus.replace(absolute(&value)?).is_some(),
            "--microphone" => {
                if !Audio::TOPICS
                    .iter()
                    .any(|topic| topic.strip_suffix("/audio") == Some(value.as_str()))
                {
                    return Err("--microphone is mic1 or mic2".into());
                }
                microphone.replace(value).is_some()
            }
            "--least" => {
                let score: f32 = value
                    .parse()
                    .ok()
                    .filter(|score| (0.5..=1.0).contains(score))
                    .ok_or("--least is a score from 0.5 to 1")?;
                least.replace(score).is_some()
            }
            "--quiet-below" => quiet_below.replace(bar(&value)?).is_some(),
            "--stop-least" => stop_least.replace(stop_bar(&value)?).is_some(),
            // Given twice is said by `take`.
            "--rescue-model" => {
                absolute(&value)?;
                rescue.take(&option, &value)?;
                false
            }
            other if rescue.take(other, &value)? => false,
            other => return Err(format!("unknown option {other}")),
        };
        if twice {
            return Err(format!("{option} was given twice"));
        }
    }
    Ok(Some(Arguments {
        models: models.ok_or("--models is required")?,
        microphone: microphone.unwrap_or_else(|| "mic2".to_owned()),
        least: least.unwrap_or(DEFAULT_LEAST),
        quiet_below,
        stop_least,
        rescue: rescue.done()?,
        bus,
    }))
}

/// `--stop-least`'s score.
fn stop_bar(value: &str) -> Result<f32, String> {
    value
        .parse()
        .ok()
        .filter(|score| (0.5..=1.0).contains(score))
        .ok_or_else(|| "--stop-least is a score from 0.5 to 1".to_owned())
}

/// `--quiet-below`'s level, in dBFS.
fn bar(value: &str) -> Result<f32, String> {
    value
        .parse()
        .ok()
        .filter(|db| quiet::BARS_DB.contains(db))
        .ok_or_else(|| "--quiet-below is a level in dBFS from -90 to -30".to_owned())
}

/// One line to the journal. Never a score's cause: nothing of the sound
/// can be in one, because nothing of the sound is kept.
fn log(line: &str) {
    let _ = writeln!(io::stderr(), "lamp-name: {line}");
}

/// What is counted between two reports.
#[derive(Default)]
struct Counted {
    steps: u64,
    /// Steps not worked out while the room was quiet (`--quiet-below`).
    rested: u64,
    heard: u64,
    /// Stop words heard (`--stop-least`).
    stops: u64,
    /// Steps the rescue model alone brought to the bar (`--rescue-model`).
    rescued: u64,
    busy: Duration,
    /// The longest one block took: with `--quiet-below`, the start after a
    /// rest, read on the lamp beside `sound_lost`.
    longest: Duration,
    lost: u64,
}

/// How the part is, for the lamp's health.
fn report(
    sound: bool,
    source_gone: bool,
    failed: Option<&str>,
    counted: &Counted,
    total_heard: u64,
) -> Diagnostic {
    let (level, message) = if let Some(why) = failed {
        (Level::Error, format!("its models stopped working: {why}"))
    } else if source_gone {
        (
            Level::Warn,
            "no sound reaches it: the voice is not putting the microphone's sound on the bus"
                .to_owned(),
        )
    } else {
        (Level::Ok, String::new())
    };
    let each_ms = if counted.steps == 0 {
        0.0
    } else {
        counted.busy.as_secs_f64() * 1e3 / counted.steps as f64
    };
    Diagnostic::new(level, NAME, &message)
        .with("sound", if sound { "coming" } else { "none" })
        .with("heard", total_heard)
        .with("ms_a_step", format!("{each_ms:.1}"))
        .with("sound_lost", counted.lost)
}

/// Both event topics are shown alive while no name is heard, so that a
/// quiet longer than the bus's lease never takes either for gone (#891).
fn renew(heard: &mut Publisher, rising: &mut Publisher) {
    let _ = heard.alive();
    let _ = rising.alive();
}

fn run(arguments: &Arguments) -> io::Result<()> {
    let started = Instant::now();
    let mut models = Hearer::load(&arguments.models).map_err(io::Error::other)?;
    if arguments.stop_least.is_some() {
        models = models
            .with_stop(&arguments.models)
            .map_err(io::Error::other)?;
    }
    if let Some(given) = &arguments.rescue {
        models = models
            .with_rescue(&given.path, &given.sha256)
            .map_err(io::Error::other)?;
    }
    let mut hearer = Quiet::new(models, arguments.quiet_below);
    let mut stops = arguments.stop_least.map(Stops::new);
    let mut rescue = arguments
        .rescue
        .as_ref()
        .map(|given| Rescue::new(given.gate));
    // Whether a step of the hearing going on was the rescue model's alone.
    let mut rescued_in_hearing = false;
    let mut loudness = Loudness::default();
    let loaded = started.elapsed();
    let bus = match &arguments.bus {
        Some(root) => Bus::new(root)?,
        None => Bus::from_environment()?,
    };
    let topic = format!("{}/audio", arguments.microphone);
    let mut sound = bus.subscribe_typed(&topic, NAME, Audio::TYPE)?;
    let mut heard: Publisher =
        bus.publisher_declared(NameHeard::TOPIC, &NameHeard::declaration(), NAME)?;
    // The first sign of each hearing, as soon as the score reaches the bar
    // (#782): what looks up at the name, and a call's wait, read it.
    let mut rising: Publisher =
        bus.publisher_declared(NameHeard::RISING_TOPIC, &NameHeard::declaration(), NAME)?;
    let mut health: Publisher =
        bus.publisher_declared(Diagnostic::TOPIC, &Diagnostic::declaration(), NAME)?;
    let mut stop_heard: Option<Publisher> = match arguments.stop_least {
        Some(_) => {
            Some(bus.publisher_declared(StopHeard::TOPIC, &StopHeard::declaration(), NAME)?)
        }
        None => None,
    };
    log(&format!(
        "started models={} loaded_ms={} listening_to={topic} least={} quiet_below={} stop_least={} rescue={} bus_root={}",
        arguments.models.display(),
        loaded.as_millis(),
        arguments.least,
        arguments
            .quiet_below
            .map_or("off".to_owned(), |db| format!("{db}dBFS")),
        arguments
            .stop_least
            .map_or("off".to_owned(), |score| score.to_string()),
        arguments
            .rescue
            .as_ref()
            .map_or("off".to_owned(), |given| format!(
                "{}@{}:{}",
                given.path.display(),
                given.gate,
                &given.sha256[..12]
            )),
        bus.root().display(),
    ));
    let mut hearings = Hearings::new(arguments.least);
    let mut counted = Counted::default();
    let (mut total_heard, mut total_steps) = (0u64, 0u64);
    let mut next_sequence: Option<u64> = None;
    let mut last_sound: Option<Instant> = None;
    let mut source_gone = false;
    let mut failed: Option<String> = None;
    let mut reported = Instant::now();
    let mut told = Instant::now();
    let at_us = |now: Instant| now.saturating_duration_since(started).as_micros() as u64;
    loop {
        let message = sound.recv(Duration::from_millis(100))?;
        let now = Instant::now();
        for notice in sound.notices() {
            match notice {
                Notice::SourceGone => {
                    source_gone = true;
                    log("no sound reaches it: nobody publishes the microphone's sound");
                }
                Notice::SourceBack => {
                    source_gone = false;
                    log("the microphone's sound is published again");
                }
                _ => {}
            }
        }
        let mut name_said = None;
        if let Some(message) = message
            && failed.is_none()
        {
            match Audio::split(&message.payload) {
                Ok((header, samples))
                    if header.sample_rate == 16_000
                        && header.channels == 1
                        && header.sample_format == Audio::S16LE
                        && samples.len().is_multiple_of(2) =>
                {
                    // Sound that was lost, or that begins again after the
                    // microphone was closed: the models start afresh.
                    let lost = next_sequence.is_some_and(|next| header.sequence != next);
                    let gap = last_sound.is_some_and(|at| now.duration_since(at) >= GAP);
                    if lost || gap {
                        counted.lost += u64::from(lost);
                        hearer.forget();
                        if let Some(rescue) = rescue.as_mut() {
                            rescue.forget();
                        }
                        name_said = hearings.cut(now);
                    }
                    next_sequence = Some(header.sequence + 1);
                    last_sound = Some(now);
                    let block: Vec<i16> = samples
                        .chunks_exact(2)
                        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
                        .collect();
                    loudness.add(&block);
                    let rested_before = hearer.rested_steps;
                    let began = Instant::now();
                    match hearer.hear(&block) {
                        Ok(quiet::Heard { scores, rested }) => {
                            let took = began.elapsed();
                            counted.busy += took;
                            counted.longest = counted.longest.max(took);
                            counted.rested += hearer.rested_steps - rested_before;
                            counted.steps += scores.len() as u64;
                            total_steps += scores.len() as u64;
                            // Each score is of a step counted back from the
                            // newest whole one, as in `check`.
                            let first = hearer.steps() as u64 + 1 - scores.len() as u64;
                            for (step, score) in (first..).zip(scores) {
                                // A stop is said at once, while it is heard.
                                if let (Some(stop), Some(stops), Some(publisher)) =
                                    (score.stop, stops.as_mut(), stop_heard.as_mut())
                                    && let Some(heard) = stops.score(stop, now)
                                {
                                    counted.stops += 1;
                                    let _ = publisher.publish_json(at_us(now), &heard);
                                    log(&format!("heard a stop word: score={:.3}", heard.score));
                                }
                                let judged = judged(&mut rescue, step, &score);
                                name_said = name_said.or(hearings.score(judged, now));
                                // A step only the rescue brought to the bar,
                                // in a hearing (not one passed over after one).
                                if judged >= arguments.least
                                    && score.name < arguments.least
                                    && hearings.is_open()
                                {
                                    counted.rescued += 1;
                                    rescued_in_hearing = true;
                                }
                                if let Some(first) = hearings.rising(now) {
                                    let _ = rising.publish_json(at_us(now), &first);
                                }
                            }
                            // The models rest, after this block's scores: a
                            // window that held the name is behind, and a
                            // hearing still open is over (as `check` does).
                            if rested {
                                name_said = name_said.or(hearings.cut(now));
                            }
                        }
                        Err(why) => {
                            log(&format!("its models stopped working: {why}"));
                            failed = Some(why);
                        }
                    }
                }
                // Not sound of the kind this listens to: passed over.
                _ => counted.lost += 1,
            }
        }
        let name_said = name_said.or_else(|| hearings.tick(now));
        if let Some(name) = name_said {
            total_heard += 1;
            counted.heard += 1;
            let _ = heard.publish_json(at_us(now), &name);
            log(&format!(
                "heard its name: score={:.3} began_ms_ago={}{}",
                name.score,
                name.ago_ms,
                if rescue.is_none() {
                    ""
                } else if rescued_in_hearing {
                    " rescued=yes"
                } else {
                    " rescued=no"
                }
            ));
            rescued_in_hearing = false;
        }
        if now.duration_since(reported) >= Duration::from_millis(950) {
            reported = now;
            let coming = last_sound.is_some_and(|at| now.duration_since(at) < GAP);
            let _ = health.publish_json(
                at_us(now),
                &report(
                    coming,
                    source_gone,
                    failed.as_deref(),
                    &counted,
                    total_heard,
                ),
            );
            renew(&mut heard, &mut rising);
            if let Some(publisher) = stop_heard.as_mut() {
                let _ = publisher.alive();
            }
        }
        // A line a minute of what it did, while there was sound: with how
        // loud each step's loudest sample was, the room's quiet and its
        // calls, to set `--quiet-below` by.
        if now.duration_since(told) >= Duration::from_secs(60) {
            told = now;
            let loudest = loudness.take();
            if counted.steps + counted.rested > 0 {
                log(&format!(
                    "last minute: steps={} rested={} ms_a_step={:.1} longest_block_ms={:.1} heard={} stops={} rescued_steps={} sound_lost={} steps_in_all={total_steps}{}",
                    counted.steps,
                    counted.rested,
                    counted.busy.as_secs_f64() * 1e3 / counted.steps.max(1) as f64,
                    counted.longest.as_secs_f64() * 1e3,
                    counted.heard,
                    counted.stops,
                    counted.rescued,
                    counted.lost,
                    loudest.map_or(String::new(), |(median, top)| format!(
                        " loudest_db_p50={median:.1} loudest_db_p99={top:.1}"
                    )),
                ));
            }
            counted = Counted::default();
        }
    }
}

/// The score the hearing rule judges: the name's, or with a rescue,
/// what [`Rescue::score`] makes of it and the rescue model's.
fn judged(rescue: &mut Option<Rescue>, step: u64, score: &lamp_name::hear::Score) -> f32 {
    match (rescue.as_mut(), score.rescue) {
        (Some(rescue), Some(trained)) => rescue.score(step, score.name, trained),
        _ => score.name,
    }
}

/// The samples of a file of sound: raw, or after a WAV file's 44 bytes.
fn samples_of(bytes: &[u8]) -> Vec<i16> {
    let sound = if bytes.starts_with(b"RIFF") && bytes.len() >= 44 {
        &bytes[44..]
    } else {
        bytes
    };
    sound
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
        .collect()
}

/// `check`: the models over files, as the loop gives them sound, 20 ms at
/// a time, rested as the loop rests them with `quiet_below`.
fn check(models: &str, options: &CheckOptions<'_>) -> io::Result<()> {
    let CheckOptions {
        quiet_below,
        stop_least,
        rescue: given,
        files,
    } = options;
    let directory = std::path::Path::new(models);
    let mut loaded = Hearer::load(directory).map_err(io::Error::other)?;
    if stop_least.is_some() {
        loaded = loaded.with_stop(directory).map_err(io::Error::other)?;
    }
    if let Some(given) = given {
        loaded = loaded
            .with_rescue(&given.path, &given.sha256)
            .map_err(io::Error::other)?;
    }
    let mut hearer = Quiet::new(loaded, *quiet_below);
    let (mut steps, mut worked, mut busy) = (0u64, 0u64, Duration::ZERO);
    for file in *files {
        let samples = samples_of(&std::fs::read(file)?);
        hearer.forget();
        let mut rescue = given.as_ref().map(|given| Rescue::new(given.gate));
        let mut hearings = Hearings::new(DEFAULT_LEAST);
        let mut stops = stop_least.map(Stops::new);
        let (mut highest, mut at_step) = (0f32, 0u64);
        // The first score is of the first 2.03 s; each later one of 80 ms more.
        let seconds = |step: u64| 2.03 + step.saturating_sub(1) as f64 * 0.08;
        let clock = Instant::now();
        // The file's own time, not this machine's.
        let at = |step: u64| clock + Duration::from_millis(80 * step);
        let said = |heard: Option<lamp_msgs::NameHeard>, step: u64| {
            if let Some(heard) = heard {
                let began_s = seconds(step) - f64::from(heard.ago_ms) / 1e3;
                println!("{file}\theard\t{began_s:.2}\t{:.4}", heard.score);
            }
        };
        for block in samples.chunks(320) {
            let began = Instant::now();
            let heard = hearer.hear(block).map_err(io::Error::other)?;
            busy += began.elapsed();
            // Each score is of a step counted back from the newest whole one.
            let newest = hearer.steps() as u64;
            let first = newest + 1 - heard.scores.len() as u64;
            worked += heard.scores.len() as u64;
            for (step, score) in (first..).zip(heard.scores) {
                if let (Some(stop), Some(stops)) = (score.stop, stops.as_mut())
                    && let Some(heard) = stops.score(stop, at(step))
                {
                    println!("{file}\tstop\t{:.2}\t{:.4}", seconds(step), heard.score);
                }
                let score = judged(&mut rescue, step, &score);
                if score > highest {
                    (highest, at_step) = (score, step);
                }
                said(hearings.score(score, at(step)), step);
            }
            if heard.rested {
                said(hearings.cut(at(newest)), newest);
            }
        }
        steps += hearer.steps() as u64;
        println!("{file}\thighest\t{:.2}\t{highest:.4}", seconds(at_step));
    }
    if worked > 0 {
        eprintln!(
            "lamp-name: {steps} steps of 80 ms, {worked} worked out, {:.2} ms each",
            busy.as_secs_f64() * 1e3 / worked as f64
        );
    }
    Ok(())
}

/// `check`'s options, and the files after them.
#[derive(Debug, PartialEq)]
struct CheckOptions<'a> {
    quiet_below: Option<f32>,
    stop_least: Option<f32>,
    rescue: Option<RescueModel>,
    files: &'a [String],
}

/// `check`'s options before its files: `--quiet-below`, `--stop-least` and
/// the three `--rescue-*`, each at most once, in any order.
fn check_options(rest: &[String]) -> Result<CheckOptions<'_>, String> {
    let (mut quiet_below, mut stop_least, mut files) = (None, None, rest);
    let mut rescue = RescueGiven::default();
    loop {
        match files {
            [option, value, more @ ..] if option == "--quiet-below" && quiet_below.is_none() => {
                quiet_below = Some(bar(value)?);
                files = more;
            }
            [option, value, more @ ..] if option == "--stop-least" && stop_least.is_none() => {
                stop_least = Some(stop_bar(value)?);
                files = more;
            }
            [option, value, more @ ..] if option.starts_with("--rescue-") => {
                if !rescue.take(option, value)? {
                    return Err(format!("unknown option {option}"));
                }
                files = more;
            }
            _ => {
                return Ok(CheckOptions {
                    quiet_below,
                    stop_least,
                    rescue: rescue.done()?,
                    files,
                });
            }
        }
    }
}

fn main() -> ExitCode {
    // Its log lines never block it or end it, and its last ones reach the
    // journal (`lamp_ipc::log_relay`).
    let _log = lamp_ipc::log_relay::guard();
    let given: Vec<String> = std::env::args().skip(1).collect();
    if let [command, models, rest @ ..] = given.as_slice()
        && command == "check"
    {
        let options = match check_options(rest) {
            Ok(read) => read,
            Err(why) => {
                eprintln!("lamp-name: {why}\n{USAGE}");
                return ExitCode::from(2);
            }
        };
        if options.files.is_empty() {
            eprintln!("lamp-name: check needs a file\n{USAGE}");
            return ExitCode::from(2);
        }
        return match check(models, &options) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("lamp-name: {error}");
                ExitCode::FAILURE
            }
        };
    }
    let arguments = match parse(given.into_iter()) {
        Ok(Some(arguments)) => arguments,
        Ok(None) => {
            println!("{USAGE}\n\n{HELP}");
            return ExitCode::SUCCESS;
        }
        Err(why) => {
            eprintln!("lamp-name: {why}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(&arguments) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lamp-name: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(text: &str) -> Result<Option<Arguments>, String> {
        parse(text.split_whitespace().map(str::to_owned))
    }

    /// #891: a quiet longer than the lease leaves both topics with their
    /// publisher, as the heartbeat renews both; renewing one alone loses
    /// the other.
    #[test]
    fn a_quiet_longer_than_the_lease_keeps_both_topics_alive() {
        let root = std::path::PathBuf::from(format!("/tmp/lnr-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap().lease(Duration::from_millis(300));
        let declared = NameHeard::declaration();
        let mut heard = bus
            .publisher_declared(NameHeard::TOPIC, &declared, NAME)
            .unwrap();
        let mut rising = bus
            .publisher_declared(NameHeard::RISING_TOPIC, &declared, NAME)
            .unwrap();
        let live = |topic: &str| bus.publishers(topic).unwrap().len();
        // Each half is half a second, still longer than the lease, renewed
        // every 100 ms as before: what lapses is 200 ms past its lease at
        // the check, and what is renewed is never within 200 ms of it
        // (#1128).
        for _ in 0..5 {
            renew(&mut heard, &mut rising);
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(
            (live(NameHeard::TOPIC), live(NameHeard::RISING_TOPIC)),
            (1, 1)
        );
        // The heartbeat as it was: only one renewed.
        for _ in 0..5 {
            let _ = heard.alive();
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(
            (live(NameHeard::TOPIC), live(NameHeard::RISING_TOPIC)),
            (1, 0)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_options_are_read_and_the_rest_have_their_defaults() {
        assert_eq!(
            of("--models /opt/lampOS/name/models").unwrap().unwrap(),
            Arguments {
                models: "/opt/lampOS/name/models".into(),
                microphone: "mic2".into(),
                least: 0.9,
                quiet_below: None,
                stop_least: None,
                rescue: None,
                bus: None,
            }
        );
        assert_eq!(
            of("--least 0.999 --microphone mic1 --bus-root /run/lampos-bus --models /m")
                .unwrap()
                .unwrap(),
            Arguments {
                models: "/m".into(),
                microphone: "mic1".into(),
                least: 0.999,
                quiet_below: None,
                stop_least: None,
                rescue: None,
                bus: Some("/run/lampos-bus".into()),
            }
        );
        assert_eq!(of("--help").unwrap(), None);
        assert_eq!(
            of("--models /m --quiet-below -55")
                .unwrap()
                .unwrap()
                .quiet_below,
            Some(-55.0)
        );
    }

    /// `--stop-least` (#828): the stop model's bar, in the program and in
    /// `check`, where it comes with `--quiet-below` in either order.
    #[test]
    fn the_stop_bar_is_read_in_the_program_and_in_check() {
        assert_eq!(
            of("--models /m --stop-least 0.9")
                .unwrap()
                .unwrap()
                .stop_least,
            Some(0.9)
        );
        let words = |text: &str| {
            text.split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        let given = words("--stop-least 0.95 --quiet-below -50 a.wav b.wav");
        let read = check_options(&given).unwrap();
        assert_eq!(
            (read.quiet_below, read.stop_least, read.files.len()),
            (Some(-50.0), Some(0.95), 2)
        );
        let plain = words("a.wav");
        assert_eq!(
            check_options(&plain).unwrap(),
            CheckOptions {
                quiet_below: None,
                stop_least: None,
                rescue: None,
                files: &plain[..]
            }
        );
        assert!(check_options(&words("--stop-least 0.2 a.wav")).is_err());
        // Given twice, the second is taken for a file, which check refuses
        // to read as an option.
        let twice = words("--stop-least 0.9 --stop-least 0.95 a.wav");
        assert_eq!(check_options(&twice).unwrap().files.len(), 3);
    }

    /// `--rescue-model` (#1546): its SHA-256 is required with it, the gate
    /// is 0.5 unless given, and in `check` the three come in any order.
    #[test]
    fn the_rescue_is_read_with_its_sha256_in_the_program_and_in_check() {
        let sha = "ab".repeat(32);
        let read = of(&format!(
            "--models /m --rescue-model /var/lib/lampOS/name/rescue.onnx --rescue-sha256 {sha}"
        ))
        .unwrap()
        .unwrap();
        assert_eq!(
            read.rescue,
            Some(RescueModel {
                path: "/var/lib/lampOS/name/rescue.onnx".into(),
                sha256: sha.clone(),
                gate: 0.5,
            })
        );
        let gated = of(&format!(
            "--rescue-gate 0.3 --models /m --rescue-sha256 {sha} --rescue-model /r.onnx"
        ))
        .unwrap()
        .unwrap();
        assert_eq!(gated.rescue.unwrap().gate, 0.3);
        let words = |text: &str| {
            text.split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        let given = words(&format!(
            "--rescue-sha256 {sha} --quiet-below -50 --rescue-model r.onnx --rescue-gate 0.1 a.wav"
        ));
        let read = check_options(&given).unwrap();
        assert_eq!(
            (read.rescue.unwrap(), read.quiet_below, read.files.len()),
            (
                RescueModel {
                    path: "r.onnx".into(),
                    sha256: sha.clone(),
                    gate: 0.1
                },
                Some(-50.0),
                1
            )
        );
        for refused in [
            "--rescue-model r.onnx a.wav".to_owned(),
            format!("--rescue-sha256 {sha} a.wav"),
            "--rescue-gate 0.5 a.wav".to_owned(),
            format!("--rescue-model r.onnx --rescue-sha256 {sha} --rescue-gate 0.99 a.wav"),
            format!("--rescue-model r.onnx --rescue-sha256 {sha} --rescue-model s.onnx a.wav"),
            "--rescue-modl r.onnx a.wav".to_owned(),
        ] {
            assert!(check_options(&words(&refused)).is_err(), "{refused}");
        }
    }

    #[test]
    fn anything_else_is_refused_with_the_reason() {
        for refused in [
            "",
            "--models relative",
            "--models /m --microphone mic3",
            "--models /m --least 0.2",
            "--models /m --least high",
            "--models /m --least 1.5",
            "--models /m --bus-root relative",
            "--models /m --models /n",
            "--models /m --record /tmp/sound",
            "--models /m --quiet-below -95",
            "--models /m --stop-least 0.4",
            "--models /m --stop-least 1.5",
            "--models /m --stop-least high",
            "--models /m --stop-least 0.9 --stop-least 0.95",
            "--models /m --quiet-below -10",
            "--models /m --quiet-below -20",
            "--models /m --quiet-below loud",
            "--models /m --quiet-below -50 --quiet-below -60",
            "--models /m --rescue-model /r.onnx",
            "--models /m --rescue-model r.onnx --rescue-sha256 abababababababababababababababababababababababababababababababab",
            "--models /m --rescue-sha256 abababababababababababababababababababababababababababababababab",
            "--models /m --rescue-gate 0.5",
            "--models /m --rescue-model /r.onnx --rescue-sha256 ABABABABABABABABABABABABABABABABABABABABABABABABABABABABABABABAB",
            "--models /m --rescue-model /r.onnx --rescue-sha256 abab",
            "--models /m --rescue-model /r.onnx --rescue-sha256 abababababababababababababababababababababababababababababababab --rescue-gate 0.01",
            "--models /m --rescue-model /r.onnx --rescue-model /s.onnx --rescue-sha256 abababababababababababababababababababababababababababababababab",
            "--models",
        ] {
            assert!(of(refused).is_err(), "{refused:?}");
        }
        assert_eq!(of("").unwrap_err(), "--models is required");
    }

    #[test]
    fn the_part_is_well_while_it_listens_and_says_when_no_sound_reaches_it() {
        let counted = Counted {
            steps: 100,
            rested: 0,
            heard: 2,
            stops: 0,
            rescued: 0,
            busy: Duration::from_millis(210),
            longest: Duration::from_millis(4),
            lost: 1,
        };
        let well = report(true, false, None, &counted, 7);
        assert_eq!(
            (well.level, well.name.as_str(), well.message.as_str()),
            (Level::Ok, "name", "")
        );
        let facts: Vec<(&str, &str)> = well
            .values
            .iter()
            .map(|fact| (fact.key.as_str(), fact.value.as_str()))
            .collect();
        assert_eq!(
            facts,
            [
                ("sound", "coming"),
                ("heard", "7"),
                ("ms_a_step", "2.1"),
                ("sound_lost", "1")
            ]
        );
        // The microphone closed: no sound, and nothing the matter.
        let quiet = report(false, false, None, &Counted::default(), 7);
        assert_eq!(quiet.level, Level::Ok);
        assert_eq!(quiet.values[0].value, "none");
        assert_eq!(quiet.values[2].value, "0.0");
        // Nobody publishes the sound at all.
        let deaf = report(false, true, None, &Counted::default(), 0);
        assert_eq!(deaf.level, Level::Warn);
        assert!(deaf.message.starts_with("no sound reaches it"));
        // Its models failed, which is said before anything else.
        let broken = report(false, true, Some("name.onnx: no score"), &counted, 7);
        assert_eq!(broken.level, Level::Error);
        assert_eq!(
            broken.message,
            "its models stopped working: name.onnx: no score"
        );
    }
}
