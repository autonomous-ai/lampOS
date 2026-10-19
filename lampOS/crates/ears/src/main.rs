//! `lamp-ears`: listen to the room microphone and say how loud the room is
//! and when a sound rises suddenly out of its quiet. See `docs/ears.md`.
use lamp_ears::{BLOCK, Listener, Settings};
use std::{
    io::{self, Write},
    process::ExitCode,
};

#[cfg(target_os = "linux")]
mod privacy_runtime;

const USAGE: &str = "usage: lamp-ears [--device ALSA_NAME] [--rise-db DB] [--bus-root ABSOLUTE_DIR]\n       lamp-ears check [--rise-db DB] FILE...";
const HELP: &str = "Listens to the room microphone and says how loud the room is. See docs/ears.md.

  --device    the room microphone (plug:device_micro1)
  --rise-db   how far over the room's quiet a sound must rise to be sudden, 10 to 60 (30)
  --bus-root  the bus's directory (LAMP_BUS_ROOT, or /run/lampos-bus)

It publishes ears/level ten times a second (the last 100 ms against the room's quiet),
ears/sudden when a sound rises suddenly out of the quiet, and how it is on diagnostics.
While the voice says it is speaking, and half a second after, nothing is sudden.
By default it keeps no sound and writes none down; only metadata leaves it.
LAMP_BUS_AUDIO=on exports raw mic1/audio (16 kHz mono S16LE), only with
LAMP_PRIVACY_CONTROLS=on. Missing privacy enablement refuses startup.
Ears remains the only microphone reader; an explicit bus recorder may retain PCM.

lamp-ears check FILE... runs the same listening over files of sound (16 kHz, one channel,
16 bits, raw or WAV) and prints each sudden sound (seconds into the file, how loud, out of
what quiet, for how long) and the file's quiet and loudest level. It touches no bus.";

/// The name it publishes under, and reports as.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const NAME: &str = "ears";

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn log(line: &str) {
    let _ = writeln!(io::stderr(), "lamp-ears: {line}");
}

#[derive(Debug, PartialEq)]
struct Arguments {
    device: String,
    rise_db: f32,
    bus: Option<std::path::PathBuf>,
}

fn parse(mut arguments: impl Iterator<Item = String>) -> Result<Option<Arguments>, String> {
    let (mut device, mut rise, mut bus) = (None, None, None);
    while let Some(option) = arguments.next() {
        if option == "--help" || option == "-h" {
            return Ok(None);
        }
        let value = arguments
            .next()
            .ok_or_else(|| format!("{option} needs a value"))?;
        let twice = match option.as_str() {
            "--device" if !value.is_empty() => device.replace(value).is_some(),
            "--rise-db" => rise.replace(rise_db(&value)?).is_some(),
            "--bus-root" => {
                let path = std::path::PathBuf::from(&value);
                if !path.is_absolute() {
                    return Err("--bus-root must be an absolute path".into());
                }
                bus.replace(path).is_some()
            }
            other => return Err(format!("unknown option {other}")),
        };
        if twice {
            return Err(format!("{option} was given twice"));
        }
    }
    Ok(Some(Arguments {
        device: device.unwrap_or_else(|| "plug:device_micro1".to_owned()),
        rise_db: rise.unwrap_or(Settings::default().rise_db),
        bus,
    }))
}

fn rise_db(value: &str) -> Result<f32, String> {
    value
        .parse::<f32>()
        .ok()
        .filter(|db| (10.0..=60.0).contains(db))
        .ok_or_else(|| "--rise-db is from 10 to 60".to_owned())
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

/// `check`: the listening over files, 20 ms at a time, the lamp silent.
fn check(rise_db: f32, files: &[String]) -> io::Result<()> {
    for file in files {
        let samples = samples_of(&std::fs::read(file)?);
        let mut listener = Listener::new(Settings {
            rise_db,
            ..Settings::default()
        });
        let (mut loudest, mut sudden) = (f32::MIN, 0u32);
        for block in samples.chunks_exact(BLOCK) {
            let heard = listener.block(block, false);
            if let Some(level) = heard.level {
                loudest = loudest.max(level.level_db);
            }
            if let Some(sound) = heard.sudden {
                sudden += 1;
                let began = (listener.now_ms() - u64::from(sound.ago_ms)) as f64 / 1e3;
                println!(
                    "{file}\tsudden\t{began:.2}\tpeak={:.1}\tfloor={:.1}\tloud_ms={}\t{}",
                    sound.peak_db,
                    sound.floor_db,
                    sound.loud_ms,
                    if sound.short { "short" } else { "went on" }
                );
            }
        }
        let floor = listener
            .floor_db()
            .map_or("unknown".to_owned(), |floor| format!("{floor:.1}"));
        println!("{file}\tquiet\t{floor}\tloudest={loudest:.1}\tsudden={sudden}");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
mod serve {
    use super::{Arguments, NAME, log};
    use alsa::{
        Direction, ValueOr,
        pcm::{Access, Format, HwParams, PCM},
    };
    use lamp_bus::{Bus, Publisher};
    use lamp_ears::{BLOCK, Listener, RATE, Settings};
    use lamp_msgs::{Diagnostic, Level, RoomSound, SuddenSound, Voice, VoiceState};
    use std::{
        io,
        time::{Duration, Instant},
    };

    /// A voice state older than this says nothing of now.
    const STATE_FRESH_MS: u64 = 3_000;

    fn open(device: &str) -> Result<PCM, alsa::Error> {
        let pcm = PCM::new(device, Direction::Capture, false)?;
        {
            let hardware = HwParams::any(&pcm)?;
            hardware.set_access(Access::RWInterleaved)?;
            hardware.set_format(Format::s16())?;
            hardware.set_channels(1)?;
            hardware.set_rate(RATE, ValueOr::Nearest)?;
            hardware.set_period_size_near(BLOCK as i64, ValueOr::Nearest)?;
            hardware.set_buffer_size_near(BLOCK as i64 * 10)?;
            pcm.hw_params(&hardware)?;
        }
        let rate = pcm.hw_params_current()?.get_rate()?;
        if rate != RATE {
            return Err(alsa::Error::unsupported("a rate other than 16 kHz"));
        }
        pcm.start()?;
        Ok(pcm)
    }

    /// Whether the voice says, freshly, that it is speaking.
    fn speaking(bus: &Bus) -> bool {
        let Ok(Some(message)) = bus.latest(VoiceState::TOPIC) else {
            return false;
        };
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() as u64);
        now_ms.saturating_sub(message.header.at_unix_ms) < STATE_FRESH_MS
            && message
                .parse::<VoiceState>()
                .is_ok_and(|state| state.state == Voice::Speaking)
    }

    #[derive(Default)]
    struct Counted {
        blocks: u64,
        sudden: u64,
        overruns: u64,
        failures: u64,
    }

    pub fn run(arguments: &Arguments) -> io::Result<()> {
        let privacy = std::env::var("LAMP_PRIVACY_CONTROLS").ok();
        lamp_ears::pcm::export_enabled(
            std::env::var("LAMP_BUS_AUDIO").ok().as_deref(),
            privacy.as_deref(),
        )?;
        if lamp_ears::privacy::enabled(privacy.as_deref()) {
            return crate::privacy_runtime::run(arguments);
        }
        if let Some(value) = privacy.filter(|value| !value.is_empty() && value != "off") {
            eprintln!(
                "lamp-ears: LAMP_PRIVACY_CONTROLS={value:?} is not \"on\"; the room microphone ignores the privacy switch"
            );
        }
        let started = Instant::now();
        let at_us = |now: Instant| now.saturating_duration_since(started).as_micros() as u64;
        let bus = match &arguments.bus {
            Some(root) => Bus::new(root)?,
            None => Bus::from_environment()?,
        };
        let mut levels: Publisher =
            bus.publisher_declared(RoomSound::TOPIC, &RoomSound::declaration(), NAME)?;
        let mut sudden: Publisher =
            bus.publisher_declared(SuddenSound::TOPIC, &SuddenSound::declaration(), NAME)?;
        let mut health: Publisher =
            bus.publisher_declared(Diagnostic::TOPIC, &Diagnostic::declaration(), NAME)?;
        log(&format!(
            "started device={} rise_db={} bus_root={}",
            arguments.device,
            arguments.rise_db,
            bus.root().display()
        ));
        let mut listener = Listener::new(Settings {
            rise_db: arguments.rise_db,
            ..Settings::default()
        });
        let mut counted = Counted::default();
        let mut total_sudden = 0u64;
        let mut lamp_speaking = false;
        let mut looked = Instant::now();
        let mut reported = Instant::now();
        let mut told = Instant::now();
        let mut pcm: Option<PCM> = None;
        let mut why: Option<String> = None;
        let mut block = vec![0i16; BLOCK];
        loop {
            let now = Instant::now();
            if pcm.is_none() {
                match open(&arguments.device) {
                    Ok(opened) => {
                        if why.take().is_some() {
                            log("the microphone is open again");
                        }
                        pcm = Some(opened);
                    }
                    Err(error) => {
                        let line = format!("the microphone does not open: {error}");
                        if why.as_deref() != Some(line.as_str()) {
                            log(&line);
                        }
                        why = Some(line);
                        counted.failures += 1;
                    }
                }
            }
            if let Some(capture) = &pcm {
                match capture.io_i16().and_then(|reader| reader.readi(&mut block)) {
                    Ok(read) if read == BLOCK => {
                        counted.blocks += 1;
                        let heard = listener.block(&block, lamp_speaking);
                        if let Some(level) = heard.level {
                            let _ = levels.publish_json(at_us(now), &level);
                        }
                        if let Some(sound) = heard.sudden {
                            counted.sudden += 1;
                            total_sudden += 1;
                            let _ = sudden.publish_json(at_us(now), &sound);
                            log(&format!(
                                "sudden sound: peak={:.1} floor={:.1} loud_ms={} {}",
                                sound.peak_db,
                                sound.floor_db,
                                sound.loud_ms,
                                if sound.short { "short" } else { "went on" }
                            ));
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        // An overrun is recovered where it happened; anything
                        // else closes the microphone to open it again.
                        counted.overruns += 1;
                        if capture.try_recover(error, true).is_err() || capture.start().is_err() {
                            why = Some(format!("the microphone stopped: {error}"));
                            log(why.as_deref().unwrap_or_default());
                            pcm = None;
                        }
                    }
                }
            } else {
                std::thread::sleep(Duration::from_secs(2));
            }
            if now.duration_since(looked) >= Duration::from_millis(100) {
                looked = now;
                lamp_speaking = speaking(&bus);
            }
            if now.duration_since(reported) >= Duration::from_millis(950) {
                reported = now;
                let (level, message) = match &why {
                    Some(why) => (Level::Error, why.clone()),
                    None => (Level::Ok, String::new()),
                };
                let report = Diagnostic::new(level, NAME, &message)
                    .with(
                        "quiet_db",
                        listener
                            .floor_db()
                            .map_or("unknown".to_owned(), |floor| format!("{floor:.1}")),
                    )
                    .with("sudden", total_sudden)
                    .with("overruns", counted.overruns);
                let _ = health.publish_json(at_us(now), &report);
                let _ = sudden.alive();
            }
            if now.duration_since(told) >= Duration::from_secs(60) {
                told = now;
                log(&format!(
                    "last minute: blocks={} quiet_db={} sudden={} overruns={} open_failures={}",
                    counted.blocks,
                    listener
                        .floor_db()
                        .map_or("unknown".to_owned(), |floor| format!("{floor:.1}")),
                    counted.sudden,
                    counted.overruns,
                    counted.failures,
                ));
                counted = Counted::default();
            }
        }
    }
}

fn main() -> ExitCode {
    // Its log lines never block it or end it, and its last ones reach the
    // journal (`lamp_ipc::log_relay`).
    let _log = lamp_ipc::log_relay::guard();
    let given: Vec<String> = std::env::args().skip(1).collect();
    if let [command, rest @ ..] = given.as_slice()
        && command == "check"
        && !rest.is_empty()
    {
        let (rise, files) = match rest {
            [option, value, files @ ..] if option == "--rise-db" => match rise_db(value) {
                Ok(rise) => (rise, files),
                Err(why) => {
                    eprintln!("lamp-ears: {why}");
                    return ExitCode::from(2);
                }
            },
            files => (Settings::default().rise_db, files),
        };
        return match check(rise, files) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("lamp-ears: {error}");
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
            eprintln!("lamp-ears: {why}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    #[cfg(target_os = "linux")]
    {
        match serve::run(&arguments) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("lamp-ears: {error}");
                ExitCode::FAILURE
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!(
            "lamp-ears: listening to {} needs the lamp's Linux; `check` runs anywhere",
            arguments.device
        );
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(text: &str) -> Result<Option<Arguments>, String> {
        parse(text.split_whitespace().map(str::to_owned))
    }

    #[test]
    fn the_options_are_read_and_the_rest_have_their_defaults() {
        assert_eq!(
            of("").unwrap().unwrap(),
            Arguments {
                device: "plug:device_micro1".into(),
                rise_db: 30.0,
                bus: None,
            }
        );
        assert_eq!(
            of("--device hw:2,0 --rise-db 25 --bus-root /run/lampos-bus")
                .unwrap()
                .unwrap(),
            Arguments {
                device: "hw:2,0".into(),
                rise_db: 25.0,
                bus: Some("/run/lampos-bus".into()),
            }
        );
        assert_eq!(of("--help").unwrap(), None);
        for refused in [
            "--bus-root relative",
            "--device a --device b",
            "--loud 3",
            "--rise-db 5",
            "--rise-db loud",
            "--device",
        ] {
            assert!(of(refused).is_err(), "{refused:?}");
        }
    }

    #[test]
    fn a_wav_file_is_read_after_its_header_and_a_raw_one_whole() {
        let mut wav = b"RIFF".to_vec();
        wav.extend([0u8; 40]);
        wav.extend([1, 0, 255, 255]);
        assert_eq!(samples_of(&wav), [1, -1]);
        assert_eq!(samples_of(&[2, 0]), [2]);
    }
}
