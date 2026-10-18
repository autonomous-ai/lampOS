//! `lamp-name`: listen to what the microphone hears, on the bus, for the
//! lamp's name, and say when it was heard. See `docs/name.md`.
use lamp_bus::{Bus, Notice, Publisher};
use lamp_msgs::{Audio, Diagnostic, Level, NameHeard};
use lamp_name::{
    hear::Hearer,
    quiet::{self, Loudness, Quiet},
    said::Hearings,
};
use std::{
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
    time::{Duration, Instant},
};

const USAGE: &str = "usage: lamp-name --models ABSOLUTE_DIR [--microphone mic1|mic2] [--least SCORE] [--quiet-below DBFS] [--bus-root ABSOLUTE_DIR]\n       lamp-name check MODELS_DIR [--quiet-below DBFS] FILE...";
const HELP: &str = "Hears the lamp's name in what its microphone hears. See docs/name.md.

  --models      a directory holding melspectrogram.onnx, embedding_model.onnx and name.onnx
  --microphone  whose sound to listen to, as the voice publishes it on the bus (mic2)
  --least       the score from which the name counts as heard, 0.5 to 1 (0.9)
  --quiet-below a level in dBFS, -90 to -30: the models rest while the last 2.03 s held
                no sample this loud, and give the same scores when one comes (off)
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
    bus: Option<PathBuf>,
}

fn parse(mut arguments: impl Iterator<Item = String>) -> Result<Option<Arguments>, String> {
    let (mut models, mut microphone, mut least, mut bus) = (None, None, None, None);
    let mut quiet_below = None;
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
        bus,
    }))
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
    let mut hearer = Quiet::new(
        Hearer::load(&arguments.models).map_err(io::Error::other)?,
        arguments.quiet_below,
    );
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
    log(&format!(
        "started models={} loaded_ms={} listening_to={topic} least={} quiet_below={} bus_root={}",
        arguments.models.display(),
        loaded.as_millis(),
        arguments.least,
        arguments
            .quiet_below
            .map_or("off".to_owned(), |db| format!("{db}dBFS")),
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
                            for score in scores {
                                name_said = name_said.or(hearings.score(score, now));
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
                "heard its name: score={:.3} began_ms_ago={}",
                name.score, name.ago_ms
            ));
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
        }
        // A line a minute of what it did, while there was sound: with how
        // loud each step's loudest sample was, the room's quiet and its
        // calls, to set `--quiet-below` by.
        if now.duration_since(told) >= Duration::from_secs(60) {
            told = now;
            let loudest = loudness.take();
            if counted.steps + counted.rested > 0 {
                log(&format!(
                    "last minute: steps={} rested={} ms_a_step={:.1} longest_block_ms={:.1} heard={} sound_lost={} steps_in_all={total_steps}{}",
                    counted.steps,
                    counted.rested,
                    counted.busy.as_secs_f64() * 1e3 / counted.steps.max(1) as f64,
                    counted.longest.as_secs_f64() * 1e3,
                    counted.heard,
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
fn check(models: &str, quiet_below: Option<f32>, files: &[String]) -> io::Result<()> {
    let mut hearer = Quiet::new(
        Hearer::load(std::path::Path::new(models)).map_err(io::Error::other)?,
        quiet_below,
    );
    let (mut steps, mut worked, mut busy) = (0u64, 0u64, Duration::ZERO);
    for file in files {
        let samples = samples_of(&std::fs::read(file)?);
        hearer.forget();
        let mut hearings = Hearings::new(DEFAULT_LEAST);
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

fn main() -> ExitCode {
    // Its log lines never block it or end it, and its last ones reach the
    // journal (`lamp_ipc::log_relay`).
    let _log = lamp_ipc::log_relay::guard();
    let given: Vec<String> = std::env::args().skip(1).collect();
    if let [command, models, rest @ ..] = given.as_slice()
        && command == "check"
    {
        let (quiet_below, files) = match rest {
            [option, value, files @ ..] if option == "--quiet-below" => match bar(value) {
                Ok(db) => (Some(db), files),
                Err(why) => {
                    eprintln!("lamp-name: {why}\n{USAGE}");
                    return ExitCode::from(2);
                }
            },
            files => (None, files),
        };
        if files.is_empty() {
            eprintln!("lamp-name: check needs a file\n{USAGE}");
            return ExitCode::from(2);
        }
        return match check(models, quiet_below, files) {
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
        for _ in 0..10 {
            renew(&mut heard, &mut rising);
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(
            (live(NameHeard::TOPIC), live(NameHeard::RISING_TOPIC)),
            (1, 1)
        );
        // The heartbeat as it was: only one renewed.
        for _ in 0..10 {
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
            "--models /m --quiet-below -10",
            "--models /m --quiet-below -20",
            "--models /m --quiet-below loud",
            "--models /m --quiet-below -50 --quiet-below -60",
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
