//! `lamp-name`: listen to what the microphone hears, on the bus, for the
//! lamp's name, and say when it was heard. See `docs/name.md`.
use lamp_bus::{Bus, Notice, Publisher};
use lamp_msgs::{
    Audio, BodyGesture, Diagnostic, GestureState, Level, NameHeard, StopHeard, Voice, VoiceState,
};
use lamp_name::{
    hear::Hearer,
    moving::{self, DEFAULT_MOVING_LEAST, Moving},
    not_self::Speaking,
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

const USAGE: &str = "usage: lamp-name --models ABSOLUTE_DIR [--microphone mic1|mic2] [--least SCORE] [--quiet-below DBFS] [--step-ms 80|40|20|10 [--wake SCORE]] [--stop-least SCORE] [--rescue-model ABSOLUTE_FILE --rescue-sha256 HEX [--rescue-gate SCORE]] [--near SCORE] [--moving-least SCORE] [--bus-root ABSOLUTE_DIR]\n       lamp-name check MODELS_DIR [--quiet-below DBFS] [--step-ms 80|40|20|10 [--wake SCORE]] [--stop-least SCORE] [--rescue-model FILE --rescue-sha256 HEX [--rescue-gate SCORE]] FILE...";
const HELP: &str = "Hears the lamp's name in what its microphone hears. See docs/name.md.

  --models      a directory holding melspectrogram.onnx, embedding_model.onnx and name.onnx
  --microphone  whose sound to listen to, as the voice publishes it on the bus (mic2)
  --least       the score from which the name counts as heard, 0.5 to 1 (0.9)
  --quiet-below a level in dBFS, -90 to -30: the models rest while the last 2.03 s held
                no sample this loud, and give the same scores when one comes (off)
  --step-ms     80, 40, 20 or 10: a step's score is the highest of 80 / STEP_MS grids, each this
                much later than the one before, so a name between two steps of 80 ms is
                scored where it lies (#694); the models' work grows as many times (80)
  --wake        with --step-ms under 80: the later grids run only while the 80 ms grid's
                name score is over this, 0.01 to 0.5, and briefly after, scoring the steps
                just before as they wake; a name they hear best comes up to 0.4 s late (off)
  --stop-least  also run stop.onnx (beside the others, SHA-256 checked) on the same numbers,
                and publish stop/heard at the step its score first reaches this, 0.5 to 1;
                then nothing new for 1 s (#828, docs/stop-word.md). Off unless set
  --rescue-model  a second name model on the same numbers (#1546, docs/name.md): where
                name.onnx reached --rescue-gate (0.5) in the last second, a step's score
                is the higher of the two. Refused unless its SHA-256 is --rescue-sha256.
                Off unless set
  --near        also say each hearing that reaches this lower bar, 0.05 to under --least,
                on name/near when it is over, with its highest score; nothing else changes.
                LAMP_NAME_NEAR=on is --near 0.5. Off unless set
  --moving-least  a hearing that began while the body played a gesture (body/gesture),
                or within 0.8 s after, is said only at this higher bar, over --least, to 1.
                LAMP_NAME_MOVING_LEAST=on is --moving-least 0.99. Off unless set
  --bus-root    the bus's directory (LAMP_BUS_ROOT, or /run/lampos-bus)

It listens to MICROPHONE/audio and publishes name/rising as each hearing begins,
name/heard when it is over, name/near with --near, and how it is on diagnostics.
It keeps no sound and writes none down.

lamp-name check MODELS_DIR FILE... runs the same models over files of sound (16 kHz, one
channel, 16 bits, raw or WAV) and prints for each every time it would have said the name
was heard (seconds into the file, and the score), its highest score and when, and then how
long a step took: for trying the models where they are installed. With --quiet-below it
rests as the program would, and says how many steps rested. It touches no bus.";

/// The name it publishes under, and reports as.
const NAME: &str = "name";
const DEFAULT_LEAST: f32 = 0.9;
/// `--near` when `LAMP_NAME_NEAR=on` gives none: a call misheard at a
/// line's start scored 0.97 to 0.99 in its clean file and was not heard
/// on lamp-4ace at 0.9 (22 Oct); a line with no name scored 0.5 or more
/// in 1 of 24.
const DEFAULT_NEAR: f32 = 0.5;
/// Sound that stops coming for this long is a gap: what the models held
/// is forgotten, and a hearing that was going on is said.
const GAP: Duration = Duration::from_millis(500);
/// The floor of a run of scores that stayed under both bars and is still
/// worth a journal line (#3923): a "Lamp" misheard as "Ma'am" or "Blake"
/// is lost under the near bar, and the line tells about 0.3 from about 0.
/// A quiet room on lamp-c0a4 scored runs of 0.06 to 0.18 every 2 s with no
/// one talking (#3923), so a line is only written from 0.2.
const UNDER_BARS_LEAST: f32 = 0.2;
/// At most this many "under its bars" lines a minute. `HOLD_OFF` alone
/// lets one through about every 1.1 s, so a room that talks all day (a
/// television) would write some 3,000 an hour; ten a minute still shows
/// what a person's test call scored.
const UNDER_BARS_A_MINUTE: u32 = 10;

#[derive(Debug, PartialEq)]
struct Arguments {
    models: PathBuf,
    microphone: String,
    least: f32,
    quiet_below: Option<f32>,
    /// `--step-ms`: the grids a step's score is the highest of (`hear::Grids`).
    step_ms: usize,
    /// `--wake`: the 80 ms grid's score that wakes the others (`Hearer::with_wake`).
    wake: Option<f32>,
    /// `--stop-least`: the stop model's bar, and with it the model.
    stop_least: Option<f32>,
    /// `--rescue-model`, its SHA-256 and its gate.
    rescue: Option<RescueModel>,
    /// Pass over a hearing that began while the lamp spoke (#705).
    not_while_speaking: bool,
    /// `--near`: the lower bar of the hearings said on `name/near`.
    near: Option<f32>,
    /// `--moving-least`: the bar while the body moves (#4208), over `--least`.
    moving_least: Option<f32>,
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
    let (mut quiet_below, mut stop_least, mut step_ms) = (None, None, None);
    let mut wake = None;
    let mut rescue = RescueGiven::default();
    let mut not_while_speaking = None;
    let mut near = None;
    let mut moving_least = None;
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
            "--step-ms" => step_ms.replace(step_of(&value)?).is_some(),
            "--wake" => wake.replace(wake_of(&value)?).is_some(),
            "--near" => near.replace(near_of(&value)?).is_some(),
            "--moving-least" => moving_least
                .replace(
                    value
                        .parse::<f32>()
                        .ok()
                        .filter(|score| (0.5..=1.0).contains(score))
                        .ok_or("--moving-least is a score from 0.5 to 1, over --least")?,
                )
                .is_some(),
            "--not-while-speaking" => not_while_speaking
                .replace(match value.as_str() {
                    "on" => true,
                    "off" => false,
                    _ => return Err("--not-while-speaking is on or off".into()),
                })
                .is_some(),
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
    let step_ms = step_ms.unwrap_or(80);
    woken_step(wake, step_ms)?;
    let least = least.unwrap_or(DEFAULT_LEAST);
    if near.is_some_and(|near| near >= least) {
        return Err("--near is under --least".into());
    }
    if moving_least.is_some_and(|moving| moving <= least) {
        return Err("--moving-least is over --least".into());
    }
    Ok(Some(Arguments {
        models: models.ok_or("--models is required")?,
        microphone: microphone.unwrap_or_else(|| "mic2".to_owned()),
        least,
        quiet_below,
        step_ms,
        wake,
        stop_least,
        rescue: rescue.done()?,
        not_while_speaking: not_while_speaking.unwrap_or(false),
        near,
        moving_least,
        bus,
    }))
}

/// `--near`'s score: from 0.05 to under 1 (and under `--least`, checked
/// once both are read).
fn near_of(value: &str) -> Result<f32, String> {
    value
        .parse()
        .ok()
        .filter(|score| (0.05..1.0).contains(score))
        .ok_or_else(|| "--near is a score from 0.05 to under --least".to_owned())
}

/// The bar while the body plays a gesture: `--moving-least` when given, else
/// [`DEFAULT_MOVING_LEAST`] when `LAMP_NAME_MOVING_LEAST` is `on` and over
/// `least`, else none (#4208).
fn moving_from(given: Option<f32>, switch: Option<&str>, least: f32) -> Option<f32> {
    given.or_else(|| {
        (switch == Some("on") && DEFAULT_MOVING_LEAST > least).then_some(DEFAULT_MOVING_LEAST)
    })
}

/// The switch for the bar while the body moves (#4208), off unless `on`.
const MOVING_ENVIRONMENT: &str = "LAMP_NAME_MOVING_LEAST";

/// The near bar: `--near` as given, else [`DEFAULT_NEAR`] when the switch
/// (`LAMP_NAME_NEAR`, `switch`) is `on` and under `least`, else none.
fn near_from(given: Option<f32>, switch: Option<&str>, least: f32) -> Option<f32> {
    given.or_else(|| (switch == Some("on") && DEFAULT_NEAR < least).then_some(DEFAULT_NEAR))
}

/// `--step-ms`'s step: one of `hear::STEPS_MS`.
fn step_of(value: &str) -> Result<usize, String> {
    value
        .parse()
        .ok()
        .filter(|ms| lamp_name::hear::STEPS_MS.contains(ms))
        .ok_or_else(|| "--step-ms is 80, 40, 20 or 10".to_owned())
}

/// Whether a block follows on from the last one: by the microphone's own
/// count, and by its owner's monotonic clock when it sent each, never by
/// when this process got to them, nor by a wall clock. A wake's catch-up can
/// take most of a second on a lamp (`Hearer::with_wake`), and the blocks sent
/// meanwhile wait unread: timed by their arrival they would look like a gap
/// (reviewer-two on #2759). A lamp with no battery clock has its wall clock
/// set 30 to 70 s after boot (`lamp-schedule.service`), which would make a
/// gap of nothing or hide a real one. Times count within one publisher (its
/// name and the boot it picked); a new one is a fresh start.
#[derive(Default)]
struct Source {
    /// The publisher's name and boot, its next count, and when it sent the
    /// last block, by its monotonic clock.
    epoch: Option<(String, String)>,
    next: Option<u64>,
    at_us: Option<u64>,
}

#[derive(Debug, PartialEq)]
enum Follows {
    On,
    /// The count skipped: sound was lost.
    Lost,
    /// Sent [`GAP`] or more after the last by its owner's clock (the
    /// microphone was closed, or its owner stalled), or by another
    /// publisher, or with that clock gone back.
    Gap,
}

impl Source {
    fn block(&mut self, source: &str, boot: &str, sequence: u64, at_us: u64) -> Follows {
        let same = self
            .epoch
            .as_ref()
            .is_some_and(|(name, picked)| name == source && picked == boot);
        let first = self.epoch.is_none();
        if !same {
            self.epoch = Some((source.to_owned(), boot.to_owned()));
        }
        let lost = same && self.next.is_some_and(|next| sequence != next);
        let gap = !first
            && (!same
                || self
                    .at_us
                    .is_some_and(|at| at_us < at || at_us - at >= GAP.as_micros() as u64));
        self.next = Some(sequence + 1);
        self.at_us = Some(at_us);
        if lost {
            Follows::Lost
        } else if gap {
            Follows::Gap
        } else {
            Follows::On
        }
    }
}

/// `--wake`'s score: from 0.01 to 0.5.
fn wake_of(value: &str) -> Result<f32, String> {
    value
        .parse()
        .ok()
        .filter(|score| (0.01..=0.5).contains(score))
        .ok_or_else(|| "--wake is a score from 0.01 to 0.5".to_owned())
}

/// `--wake` is for the later grids: refused with the 80 ms grid alone.
fn woken_step(wake: Option<f32>, step_ms: usize) -> Result<(), String> {
    if wake.is_some() && step_ms == 80 {
        return Err("--wake needs --step-ms under 80".to_owned());
    }
    Ok(())
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

/// ` after_speech_ms=N` for a hearing that began within 3 s after the lamp
/// stopped speaking (0 inside it), so the lamp's own hearings say how far
/// `not_self::TAIL` has to reach; nothing otherwise.
fn after_speech(speaking: &Speaking, name: &NameHeard, now: Instant) -> String {
    speaking
        .after_speech(Duration::from_millis(u64::from(name.ago_ms)), now)
        .map_or(String::new(), |after| {
            format!(" after_speech_ms={}", after.as_millis())
        })
}

/// Both event topics are shown alive while no name is heard, so that a
/// quiet longer than the bus's lease never takes either for gone (#891).
fn renew(heard: &mut Publisher, rising: &mut Publisher) {
    let _ = heard.alive();
    let _ = rising.alive();
}

fn run(arguments: &Arguments) -> io::Result<()> {
    let started = Instant::now();
    let mut models = Hearer::load(&arguments.models)
        .and_then(|models| models.with_step_ms(arguments.step_ms))
        .and_then(|models| match arguments.wake {
            Some(bar) => models.with_wake(bar, arguments.least),
            None => Ok(models),
        })
        .map_err(io::Error::other)?;
    if let Some(least) = arguments.stop_least {
        models = models
            .with_stop(&arguments.models)
            .map_err(io::Error::other)?
            .stop_heard_at(least);
    }
    if let Some(given) = &arguments.rescue {
        models = models
            .with_rescue(&given.path, &given.sha256)
            .map_err(io::Error::other)?;
    }
    let mut hearer = Quiet::new(models, arguments.quiet_below);
    // `--wake`'s counts at the last minute line, to say the minute's.
    let mut woken_before = (0u64, 0u64, 0u64);
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
    // Hearings that reach the lower bar (`--near`), on a topic of their own.
    let mut nears = arguments.near.map(Hearings::new);
    let mut near_heard: Option<Publisher> = match arguments.near {
        Some(_) => {
            Some(bus.publisher_declared(NameHeard::NEAR_TOPIC, &NameHeard::declaration(), NAME)?)
        }
        None => None,
    };
    let mut stop_heard: Option<Publisher> = match arguments.stop_least {
        Some(_) => {
            Some(bus.publisher_declared(StopHeard::TOPIC, &StopHeard::declaration(), NAME)?)
        }
        None => None,
    };
    // When the lamp itself speaks, with `--not-while-speaking on`.
    let mut voice = arguments
        .not_while_speaking
        .then(|| bus.subscribe_typed(VoiceState::TOPIC, NAME, VoiceState::TYPE))
        .transpose()?;
    let mut speaking = Speaking::default();
    // When the body moves, with `--moving-least` (#4208).
    let mut gestures = arguments
        .moving_least
        .map(|_| bus.subscribe_typed(BodyGesture::TOPIC, NAME, BodyGesture::TYPE))
        .transpose()?;
    let mut moving = Moving::default();
    if let Some(bar) = arguments.moving_least {
        log(&format!(
            "moving_least={bar}: while the body moves, a hearing needs that"
        ));
    }
    log(&format!(
        "started models={} loaded_ms={} listening_to={topic} least={} near={} quiet_below={} step_ms={} wake={} stop_least={} rescue={} not_while_speaking={} bus_root={}",
        arguments.models.display(),
        loaded.as_millis(),
        arguments.least,
        arguments
            .near
            .map_or("off".to_owned(), |score| score.to_string()),
        arguments
            .quiet_below
            .map_or("off".to_owned(), |db| format!("{db}dBFS")),
        arguments.step_ms,
        arguments
            .wake
            .map_or("off".to_owned(), |score| score.to_string()),
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
        if arguments.not_while_speaking {
            "on"
        } else {
            "off"
        },
        bus.root().display(),
    ));
    let mut hearings = Hearings::new(arguments.least);
    // Whatever the switches are: a missed call shows how near it came.
    let mut under_bars = UnderBars::new(arguments.near.unwrap_or(arguments.least));
    if let Some(line) = under_bars.off() {
        log(&line);
    }
    let mut counted = Counted::default();
    let (mut total_heard, mut total_steps) = (0u64, 0u64);
    let mut source = Source::default();
    let mut last_sound: Option<Instant> = None;
    let mut source_gone = false;
    let mut failed: Option<String> = None;
    let mut reported = Instant::now();
    let mut told = Instant::now();
    let at_us = |now: Instant| now.saturating_duration_since(started).as_micros() as u64;
    loop {
        let message = sound.recv(Duration::from_millis(100))?;
        let now = Instant::now();
        if let Some(voice) = voice.as_mut() {
            while let Ok(Some(state)) = voice.recv(Duration::ZERO) {
                if let Ok(state) = state.parse::<VoiceState>() {
                    speaking.saw(state.state == Voice::Speaking, now);
                }
            }
            if voice
                .notices()
                .iter()
                .any(|notice| matches!(notice, Notice::SourceGone))
            {
                speaking.gone(now);
            }
        }
        if let Some(gestures) = gestures.as_mut() {
            while let Ok(Some(gesture)) = gestures.recv(Duration::ZERO) {
                // The body's own shifts at rest and its look around are not
                // a gesture played (#4385).
                if let Some(gesture) = gesture
                    .parse::<BodyGesture>()
                    .ok()
                    .filter(|gesture| moving::counts(&gesture.name))
                {
                    match gesture.state {
                        GestureState::Began => moving.began(now),
                        GestureState::Over | GestureState::Dropped => moving.ended(now),
                    }
                }
            }
        }
        // Whether a hearing said now, begun `ago_ms` before, goes out:
        // never one begun while the lamp spoke, and one begun while the body
        // moved only at `--moving-least`.
        let mut lets_through = |name: &NameHeard| {
            let ago = Duration::from_millis(u64::from(name.ago_ms));
            if !(voice.is_none() || speaking.lets_through(ago, now)) {
                return false;
            }
            match arguments.moving_least {
                Some(bar) => moving.lets_through(name.score, bar, ago, now),
                None => true,
            }
        };
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
        let mut near_said = None;
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
                    let follows = source.block(
                        &message.header.source,
                        &message.header.boot,
                        header.sequence,
                        message.header.at_us,
                    );
                    let lost = follows == Follows::Lost;
                    if follows != Follows::On {
                        counted.lost += u64::from(lost);
                        hearer.forget();
                        if let Some(rescue) = rescue.as_mut() {
                            rescue.forget();
                        }
                        name_said = hearings.cut(now);
                        near_said = nears.as_mut().and_then(|nears| nears.cut(now));
                        if let Some(line) = under_bars.cut(now) {
                            log(&line);
                        }
                    }
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
                                if let Some(nears) = nears.as_mut() {
                                    near_said = near_said.or(nears.score(judged, now));
                                }
                                if let Some(line) = under_bars.score(judged, now) {
                                    log(&line);
                                }
                                // A step only the rescue brought to the bar,
                                // in a hearing (not one passed over after one).
                                if judged >= arguments.least
                                    && score.name < arguments.least
                                    && hearings.is_open()
                                {
                                    counted.rescued += 1;
                                    rescued_in_hearing = true;
                                }
                                if let Some(first) = hearings.rising(now)
                                    && lets_through(&first)
                                {
                                    let _ = rising.publish_json(at_us(now), &first);
                                }
                            }
                            // The models rest, after this block's scores: a
                            // window that held the name is behind, and a
                            // hearing still open is over (as `check` does).
                            if rested {
                                name_said = name_said.or(hearings.cut(now));
                                if let Some(nears) = nears.as_mut() {
                                    near_said = near_said.or(nears.cut(now));
                                }
                                if let Some(line) = under_bars.cut(now) {
                                    log(&line);
                                }
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
        if let Some(line) = under_bars.tick(now) {
            log(&line);
        }
        // A hearing that reached the lower bar, said as it is over: never
        // one begun while the lamp spoke, as for the name.
        if let (Some(near), Some(publisher)) = (
            near_said.or_else(|| nears.as_mut().and_then(|nears| nears.tick(now))),
            near_heard.as_mut(),
        ) && lets_through(&near)
        {
            let _ = publisher.publish_json(at_us(now), &near);
            log(&format!(
                "near its name: score={:.3} began_ms_ago={}",
                near.score, near.ago_ms
            ));
        }
        if let Some(name) = name_said
            && !lets_through(&name)
        {
            let began = now
                .checked_sub(Duration::from_millis(u64::from(name.ago_ms)))
                .unwrap_or(now);
            if voice.is_some() && speaking.covers(began, now) {
                log(&format!(
                    "passed over a hearing that began while the lamp spoke: score={:.3} began_ms_ago={}{}",
                    name.score,
                    name.ago_ms,
                    after_speech(&speaking, &name, now)
                ));
            } else {
                log(&format!(
                    "passed over a hearing that began while the body moved: score={:.3} began_ms_ago={} under moving_least={}",
                    name.score,
                    name.ago_ms,
                    arguments.moving_least.unwrap_or(1.0)
                ));
            }
        } else if let Some(name) = name_said {
            total_heard += 1;
            counted.heard += 1;
            let _ = heard.publish_json(at_us(now), &name);
            log(&format!(
                "heard its name: score={:.3} began_ms_ago={}{}{}",
                name.score,
                name.ago_ms,
                if rescue.is_none() {
                    ""
                } else if rescued_in_hearing {
                    " rescued=yes"
                } else {
                    " rescued=no"
                },
                if voice.is_some() {
                    after_speech(&speaking, &name, now)
                } else {
                    String::new()
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
            if let Some(publisher) = near_heard.as_mut() {
                let _ = publisher.alive();
            }
        }
        // A line a minute of what it did, while there was sound: with how
        // loud each step's loudest sample was, the room's quiet and its
        // calls, to set `--quiet-below` by.
        if now.duration_since(told) >= Duration::from_secs(60) {
            told = now;
            let loudest = loudness.take();
            let models = hearer.models();
            let woken = (models.wakes, models.woken_steps, models.overruns());
            let wakes = arguments.wake.map_or(String::new(), |_| {
                format!(
                    " wakes={} woken_steps={} overruns={}",
                    woken.0 - woken_before.0,
                    woken.1 - woken_before.1,
                    woken.2 - woken_before.2
                )
            });
            woken_before = woken;
            if counted.steps + counted.rested > 0 {
                log(&format!(
                    "last minute: steps={} rested={} ms_a_step={:.1} longest_block_ms={:.1} heard={} stops={} rescued_steps={} sound_lost={}{wakes} steps_in_all={total_steps} passed_over_in_all={} passed_over_moving_in_all={}{}",
                    counted.steps,
                    counted.rested,
                    counted.busy.as_secs_f64() * 1e3 / counted.steps.max(1) as f64,
                    counted.longest.as_secs_f64() * 1e3,
                    counted.heard,
                    counted.stops,
                    counted.rescued,
                    counted.lost,
                    speaking.passed_over,
                    moving.passed_over,
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
/// Runs of scores that stayed under both bars (#3923), for the journal
/// only: never published. A run from [`UNDER_BARS_LEAST`] whose best
/// stayed under `--near` (or `--least` without it) is one line, at most
/// [`UNDER_BARS_A_MINUTE`] a minute; those not written are counted on the
/// next line written.
struct UnderBars {
    hearings: Hearings,
    /// The lowest bar it says: `--near`, else `--least`.
    bar: f32,
    /// When this minute's lines began, and how many were written in it.
    minute: Option<(Instant, u32)>,
    not_written: u64,
}

impl UnderBars {
    fn new(bar: f32) -> Self {
        Self {
            hearings: Hearings::new(UNDER_BARS_LEAST),
            bar,
            minute: None,
            not_written: 0,
        }
    }

    /// The start-up line when no run can ever be written: a `--near` at or
    /// under [`UNDER_BARS_LEAST`] leaves nothing between the floor and the
    /// bar (Sixth's P3 on #3947).
    fn off(&self) -> Option<String> {
        (self.bar <= UNDER_BARS_LEAST).then(|| {
            format!(
                "under its bars is off: bar={} is at or under its floor {UNDER_BARS_LEAST}",
                self.bar
            )
        })
    }

    /// A judged score, as the name's hearings are given it.
    fn score(&mut self, score: f32, now: Instant) -> Option<String> {
        let over = self.hearings.score(score, now);
        self.line(over, now)
    }

    /// Sound lost or rested: a run going on is over.
    fn cut(&mut self, now: Instant) -> Option<String> {
        let over = self.hearings.cut(now);
        self.line(over, now)
    }

    fn tick(&mut self, now: Instant) -> Option<String> {
        let over = self.hearings.tick(now);
        self.line(over, now)
    }

    fn line(&mut self, over: Option<NameHeard>, now: Instant) -> Option<String> {
        let run = over.filter(|run| run.score < self.bar)?;
        let written = match &mut self.minute {
            Some((began, written))
                if now.saturating_duration_since(*began) < Duration::from_secs(60) =>
            {
                written
            }
            minute => &mut minute.insert((now, 0)).1,
        };
        if *written >= UNDER_BARS_A_MINUTE {
            self.not_written += 1;
            return None;
        }
        *written += 1;
        let not_written = std::mem::take(&mut self.not_written);
        Some(format!(
            "under its bars: best={:.3} began_ms_ago={}{}",
            run.score,
            run.ago_ms,
            if not_written == 0 {
                String::new()
            } else {
                format!(" not_written_before={not_written}")
            }
        ))
    }
}

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
        step_ms,
        wake,
        stop_least,
        rescue: given,
        files,
    } = options;
    let directory = std::path::Path::new(models);
    let mut loaded = Hearer::load(directory)
        .and_then(|loaded| loaded.with_step_ms(*step_ms))
        .and_then(|loaded| match wake {
            // Each block waits for the woken grids, so a file scores the
            // same every time.
            Some(bar) => loaded
                .with_wake(*bar, DEFAULT_LEAST)
                .map(|loaded| loaded.waiting(true)),
            None => Ok(loaded),
        })
        .map_err(io::Error::other)?;
    if let Some(least) = stop_least {
        loaded = loaded
            .with_stop(directory)
            .map_err(io::Error::other)?
            .stop_heard_at(*least);
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
    step_ms: usize,
    wake: Option<f32>,
    stop_least: Option<f32>,
    rescue: Option<RescueModel>,
    files: &'a [String],
}

/// `check`'s options before its files: `--quiet-below`, `--step-ms`,
/// `--stop-least` and the three `--rescue-*`, each at most once, in any
/// order.
fn check_options(rest: &[String]) -> Result<CheckOptions<'_>, String> {
    let (mut quiet_below, mut stop_least, mut files) = (None, None, rest);
    let (mut step_ms, mut wake) = (None, None);
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
            [option, value, more @ ..] if option == "--step-ms" && step_ms.is_none() => {
                step_ms = Some(step_of(value)?);
                files = more;
            }
            [option, value, more @ ..] if option == "--wake" && wake.is_none() => {
                wake = Some(wake_of(value)?);
                files = more;
            }
            [option, value, more @ ..] if option.starts_with("--rescue-") => {
                if !rescue.take(option, value)? {
                    return Err(format!("unknown option {option}"));
                }
                files = more;
            }
            _ => {
                let step_ms = step_ms.unwrap_or(80);
                woken_step(wake, step_ms)?;
                return Ok(CheckOptions {
                    quiet_below,
                    step_ms,
                    wake,
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
        Ok(Some(arguments)) => Arguments {
            near: near_from(
                arguments.near,
                std::env::var(NameHeard::NEAR_ENVIRONMENT).ok().as_deref(),
                arguments.least,
            ),
            moving_least: moving_from(
                arguments.moving_least,
                std::env::var(MOVING_ENVIRONMENT).ok().as_deref(),
                arguments.least,
            ),
            ..arguments
        },
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
        // The heartbeat as it was: only one renewed, every 100 ms, until the
        // other has lapsed, not a fixed half second (#1806).
        let until = std::time::Instant::now() + Duration::from_secs(5);
        while live(NameHeard::RISING_TOPIC) != 0 {
            assert!(std::time::Instant::now() < until, "never lapsed");
            let _ = heard.alive();
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(
            (live(NameHeard::TOPIC), live(NameHeard::RISING_TOPIC)),
            (1, 0)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `--near` (`LAMP_NAME_NEAR`): a lower bar under `--least`, from the
    /// flag or, with the switch `on` and no flag, 0.5; off otherwise. A run
    /// of scores that reaches it and not the bar is a near hearing only:
    /// `name/heard` and `name/rising` are as without it.
    #[test]
    fn a_near_bar_says_hearings_under_the_bar_and_changes_nothing_else() {
        let near = |text: &str| of(text).map(|read| read.unwrap().near);
        assert_eq!(near("--models /m"), Ok(None));
        assert_eq!(near("--models /m --near 0.5"), Ok(Some(0.5)));
        assert_eq!(near("--models /m --least 0.95 --near 0.9"), Ok(Some(0.9)));
        for refused in [
            "--models /m --near 0.9",
            "--models /m --near 0.95",
            "--models /m --near 0.01",
            "--models /m --near x",
            "--models /m --near 0.5 --near 0.6",
        ] {
            assert!(of(refused).is_err(), "{refused}");
        }
        assert_eq!(near_from(None, Some("on"), 0.9), Some(DEFAULT_NEAR));
        assert_eq!(near_from(Some(0.6), Some("on"), 0.9), Some(0.6));
        assert_eq!(near_from(Some(0.6), None, 0.9), Some(0.6));
        for off in [None, Some("off"), Some("ON"), Some("1")] {
            assert_eq!(near_from(None, off, 0.9), None, "{off:?}");
        }
        assert_eq!(near_from(None, Some("on"), 0.5), None, "never at the bar");
        // One run of scores up to 0.62: near, and not the name.
        let start = Instant::now();
        let (mut name, mut nears) = (Hearings::new(0.9), Hearings::new(DEFAULT_NEAR));
        let (mut heard, mut near) = (Vec::new(), Vec::new());
        for (step, score) in [0.1, 0.55, 0.62, 0.58, 0.2, 0.1].into_iter().enumerate() {
            let at = start + Duration::from_millis(80 * step as u64);
            heard.extend(name.score(score, at));
            near.extend(nears.score(score, at));
        }
        assert!(heard.is_empty());
        assert_eq!(
            near,
            [NameHeard {
                score: 0.62,
                ago_ms: 240
            }]
        );
    }

    /// #3923: a run that stayed under both bars is one journal line with
    /// its best; one that reached the near bar is said as near, not here.
    #[test]
    fn a_run_under_both_bars_is_one_line_and_one_at_the_near_bar_is_none() {
        let start = Instant::now();
        let at = |step: u64| start + Duration::from_millis(80 * step);
        let mut under = UnderBars::new(DEFAULT_NEAR);
        let lines: Vec<String> = [0.01, 0.12, 0.31, 0.22, 0.02]
            .into_iter()
            .zip(0..)
            .filter_map(|(score, step)| under.score(score, at(step)))
            .collect();
        assert_eq!(lines, ["under its bars: best=0.310 began_ms_ago=160"]);
        // A quiet room's runs, under the floor, are no line.
        let mut under = UnderBars::new(DEFAULT_NEAR);
        let lines: Vec<String> = [0.01, 0.12, 0.18, 0.15, 0.02]
            .into_iter()
            .zip(0..)
            .filter_map(|(score, step)| under.score(score, at(step)))
            .collect();
        assert!(lines.is_empty(), "{lines:?}");
        let mut under = UnderBars::new(DEFAULT_NEAR);
        let lines: Vec<String> = [0.01, 0.2, 0.5, 0.3, 0.02]
            .into_iter()
            .zip(0..)
            .filter_map(|(score, step)| under.score(score, at(step)))
            .collect();
        assert!(lines.is_empty(), "{lines:?}");
    }

    /// A near bar at or under the floor leaves no run to write, and says so
    /// once at start; any bar over it says nothing.
    #[test]
    fn under_its_bars_says_at_start_when_it_is_off() {
        assert_eq!(
            UnderBars::new(0.2).off().as_deref(),
            Some("under its bars is off: bar=0.2 is at or under its floor 0.2")
        );
        assert!(UnderBars::new(0.05).off().is_some());
        assert_eq!(UnderBars::new(0.21).off(), None);
        assert_eq!(UnderBars::new(DEFAULT_NEAR).off(), None);
        assert_eq!(UnderBars::new(DEFAULT_LEAST).off(), None);
        // And it is true: a run of 0.19 under a 0.2 bar is no line.
        let start = Instant::now();
        let mut under = UnderBars::new(0.2);
        let lines: Vec<String> = [0.01, 0.19, 0.19, 0.01]
            .into_iter()
            .zip(0..)
            .filter_map(|(score, step)| {
                under.score(score, start + Duration::from_millis(80 * step))
            })
            .collect();
        assert!(lines.is_empty(), "{lines:?}");
    }

    /// A room that talks all day writes ten lines a minute, not one a
    /// second; the next line written says how many were not.
    #[test]
    fn under_its_bars_is_ten_lines_a_minute_at_most() {
        let start = Instant::now();
        let mut under = UnderBars::new(DEFAULT_NEAR);
        let mut lines = Vec::new();
        // A run of 0.3 every 1.2 s for two minutes.
        for run in 0..100u64 {
            let began = start + Duration::from_millis(1_200 * run);
            lines.extend(under.score(0.3, began));
            lines.extend(under.score(0.01, began + Duration::from_millis(80)));
        }
        assert_eq!(lines.len(), 20, "{lines:?}");
        assert!(!lines[9].contains("not_written_before"));
        assert!(
            lines[10].ends_with(" not_written_before=40"),
            "{}",
            lines[10]
        );
    }

    /// `--moving-least` (#4208): a score over `--least`, from 0.5 to 1; and
    /// `LAMP_NAME_MOVING_LEAST=on` is 0.99 when that is over `--least`.
    #[test]
    fn the_bar_while_the_body_moves_is_over_the_usual_one_and_off_unless_given() {
        let moving = |line: &str| of(line).unwrap().unwrap().moving_least;
        assert_eq!(moving("--models /m"), None);
        assert_eq!(moving("--models /m --moving-least 0.99"), Some(0.99));
        for refused in [
            "--models /m --moving-least 0.9",
            "--models /m --moving-least 0.4",
            "--models /m --moving-least 1.1",
            "--models /m --moving-least high",
            "--models /m --least 0.995 --moving-least 0.99",
            "--models /m --moving-least 0.99 --moving-least 0.99",
        ] {
            assert!(of(refused).is_err(), "{refused}");
        }
        assert_eq!(moving_from(None, None, 0.9), None);
        assert_eq!(moving_from(None, Some("off"), 0.9), None);
        assert_eq!(
            moving_from(None, Some("on"), 0.9),
            Some(DEFAULT_MOVING_LEAST)
        );
        assert_eq!(
            moving_from(None, Some("on"), 0.995),
            None,
            "not over --least"
        );
        assert_eq!(
            moving_from(Some(0.97), Some("on"), 0.9),
            Some(0.97),
            "given wins"
        );
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
                step_ms: 80,
                wake: None,
                stop_least: None,
                rescue: None,
                not_while_speaking: false,
                near: None,
                moving_least: None,
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
                step_ms: 80,
                wake: None,
                stop_least: None,
                rescue: None,
                not_while_speaking: false,
                near: None,
                moving_least: None,
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
        assert!(
            of("--models /m --not-while-speaking on")
                .unwrap()
                .unwrap()
                .not_while_speaking
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
                step_ms: 80,
                wake: None,
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

    /// `--step-ms` (#694): 80 unless given, 40, 20 or 10 when it is, in the
    /// program and in `check`; any other step is refused.
    #[test]
    fn the_step_is_read_in_the_program_and_in_check() {
        assert_eq!(of("--models /m --step-ms 40").unwrap().unwrap().step_ms, 40);
        assert_eq!(of("--step-ms 20 --models /m").unwrap().unwrap().step_ms, 20);
        assert_eq!(of("--models /m --step-ms 10").unwrap().unwrap().step_ms, 10);
        assert_eq!(
            of("--models /m --step-ms 30").unwrap_err(),
            "--step-ms is 80, 40, 20 or 10"
        );
        assert_eq!(
            of("--models /m --step-ms 40 --step-ms 40").unwrap_err(),
            "--step-ms was given twice"
        );
        let words = |text: &str| {
            text.split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        let given = words("--step-ms 40 --quiet-below -50 a.wav");
        let read = check_options(&given).unwrap();
        assert_eq!(
            (read.step_ms, read.quiet_below, read.files.len()),
            (40, Some(-50.0), 1)
        );
        assert!(check_options(&words("--step-ms 30 a.wav")).is_err());
    }

    /// Reviewer-two's two P2s on #2759: blocks sent 20 ms apart by their
    /// owner's monotonic clock follow on however late they are read (a
    /// wake's second of catch-up), and whatever the wall clock does; 500 ms
    /// or more between two is a gap, so is the clock gone back or another
    /// publisher, and a skipped count is lost.
    #[test]
    fn a_block_follows_on_by_its_owners_clock_not_when_it_was_read() {
        let mut source = Source::default();
        let ms = |ms: u64| ms * 1_000;
        assert_eq!(source.block("voice", "b1", 0, ms(1_000)), Follows::On);
        // Fifty blocks read in a burst after a slow step: sent 20 ms apart.
        for k in 1..=50 {
            assert_eq!(
                source.block("voice", "b1", k, ms(1_000 + 20 * k)),
                Follows::On,
                "{k}"
            );
        }
        // The owner stopped sending for half a second: a gap.
        assert_eq!(source.block("voice", "b1", 51, ms(2_500)), Follows::Gap);
        assert_eq!(source.block("voice", "b1", 52, ms(2_520)), Follows::On);
        // A count skipped: lost, whatever the times.
        assert_eq!(source.block("voice", "b1", 54, ms(2_540)), Follows::Lost);
        assert_eq!(source.block("voice", "b1", 55, ms(2_560)), Follows::On);
        // Its clock gone back within one publisher: not trusted, a gap.
        assert_eq!(source.block("voice", "b1", 56, ms(1_200)), Follows::Gap);
        assert_eq!(source.block("voice", "b1", 57, ms(1_220)), Follows::On);
        // The voice restarted: a new boot, its count and clock from the
        // start, is a fresh start, not lost.
        assert_eq!(source.block("voice", "b2", 0, ms(10)), Follows::Gap);
        assert_eq!(source.block("voice", "b2", 1, ms(30)), Follows::On);
        // Another publisher on the topic, likewise.
        assert_eq!(source.block("other", "b2", 2, ms(50)), Follows::Gap);
    }

    /// Reviewer-two's control on #2759: the first grid hears a call at 0 ms
    /// and it falls at 80 ms; that call's fine score, a woken grid's, comes
    /// 1.2 s later, past the hold-off. It is heard once, not twice. A fine
    /// score of a call the first grid missed is heard, once.
    #[test]
    fn a_late_fine_score_of_a_call_already_heard_is_not_heard_again() {
        use lamp_name::hear::{Grids, Score};
        let score = |name| Score {
            name,
            stop: None,
            rescue: None,
        };
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let heard = |late_number: u64, first: &[f32]| {
            let mut grids = Grids::new(2);
            grids.hearing_at(DEFAULT_LEAST);
            let mut hearings = Hearings::new(DEFAULT_LEAST);
            let mut said = 0;
            for (step, name) in first.iter().enumerate() {
                if step == 15 {
                    grids.later(1, late_number, score(0.99));
                }
                let judged = grids.first(score(*name)).name;
                let now = at(80 * step as u64);
                said += usize::from(hearings.score(judged, now).is_some());
                said += usize::from(hearings.tick(now).is_some());
            }
            said += usize::from(hearings.cut(at(80 * first.len() as u64)).is_some());
            said
        };
        let mut first = vec![0.0; 30];
        first[0] = 0.95;
        assert_eq!(
            heard(0, &first),
            1,
            "the call the first grid heard, heard once"
        );
        assert_eq!(
            heard(0, &[0.0; 30]),
            1,
            "a call only the fine grid heard, heard once"
        );
        // Reviewer-two's 09:37: the same call's fine score at 2560 ms, its
        // first-grid high gone from the history, is let go as gone by, not
        // heard again.
        let late = |at_step: usize| {
            let mut grids = Grids::new(2);
            grids.hearing_at(DEFAULT_LEAST);
            let mut hearings = Hearings::new(DEFAULT_LEAST);
            let mut said = 0;
            for step in 0..70 {
                if step == at_step {
                    grids.later(1, 0, score(0.99));
                }
                let name = if step == 0 { 0.95 } else { 0.0 };
                let now = at(80 * step as u64);
                said += usize::from(hearings.score(grids.first(score(name)).name, now).is_some());
                said += usize::from(hearings.tick(now).is_some());
            }
            said
        };
        for at_step in [31, 32, 40, 42, 43, 60] {
            assert_eq!(late(at_step), 1, "at {} ms", 80 * at_step);
        }
        // And a stop only a later grid heard, a second late, comes through
        // the name being let go: one stop word, as with no name before it.
        let stops = |name_first: f32| {
            let mut grids = Grids::new(2);
            grids.hearing_at(DEFAULT_LEAST);
            grids.stop_heard_at(0.9);
            let mut stops = Stops::new(0.9);
            let mut said = 0;
            for step in 0..30u64 {
                if step == 15 {
                    grids.later(
                        1,
                        0,
                        Score {
                            name: 0.1,
                            stop: Some(0.99),
                            rescue: None,
                        },
                    );
                }
                let name = if step == 0 { name_first } else { 0.0 };
                let given = grids.first(Score {
                    name,
                    stop: Some(0.1),
                    rescue: None,
                });
                said += usize::from(
                    stops
                        .score(given.stop.unwrap_or(0.0), at(80 * step))
                        .is_some(),
                );
            }
            said
        };
        assert_eq!(stops(0.95), 1, "after the name heard");
        assert_eq!(stops(0.0), 1, "with no name before it");
    }

    /// `--wake` (#694): off unless given, a score from 0.01 to 0.5, and only
    /// with a step under 80, in the program and in `check`.
    #[test]
    fn the_wake_bar_is_read_with_a_step_under_80_only() {
        assert_eq!(of("--models /m").unwrap().unwrap().wake, None);
        let read = of("--models /m --step-ms 10 --wake 0.03").unwrap().unwrap();
        assert_eq!((read.step_ms, read.wake), (10, Some(0.03)));
        assert_eq!(
            of("--models /m --wake 0.03").unwrap_err(),
            "--wake needs --step-ms under 80"
        );
        for bad in ["0", "0.6", "x"] {
            assert_eq!(
                of(&format!("--models /m --step-ms 10 --wake {bad}")).unwrap_err(),
                "--wake is a score from 0.01 to 0.5"
            );
        }
        let words = |text: &str| {
            text.split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        let given = words("--wake 0.05 --step-ms 20 a.wav");
        let read = check_options(&given).unwrap();
        assert_eq!(
            (read.step_ms, read.wake, read.files.len()),
            (20, Some(0.05), 1)
        );
        assert!(check_options(&words("--wake 0.05 a.wav")).is_err());
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
            "--models /m --not-while-speaking yes",
            "--models /m --not-while-speaking on --not-while-speaking off",
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
