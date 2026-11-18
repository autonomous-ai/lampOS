//! `lamp-ears-body-kit` (#7685): the body sounds' bars measured on a lamp
//! with a consenting person. Steps: docs/body-sounds-kit.md.
//!
//! - `take DIR LABEL [--count N] [--seconds S] [--model PATH] [--bus-root R]`
//!   records N clips (20) of S seconds (3) of the person's own sound from
//!   `mic1/audio` into DIR (0700, files 0600), one per Enter, and scores each
//!   patch with lamp-ears' own frontend and model, the scores beside the
//!   clip (`cough-07.wav`, `cough-07.csv`). `r` and Enter take the last one
//!   again; `q` and Enter stop.
//! - `office DIR [--minutes M] [--bus-root R]` logs the body scores of each
//!   window `ears/classes` publishes to DIR/office.csv for M minutes (60):
//!   numbers only, no sound.
//! - `suggest DIR` gives a bar per class from the clips and the office log:
//!   recall on the person's sounds, precision, no false hit on the others,
//!   alarms an hour, and `body_sounds::BARS` as it would read.
//! - `delete DIR` removes the kit's files (each clip zero-filled first) and
//!   the folder, and nothing else.
use lamp_ears::body_kit::{
    self, Clip, OFFICE_HEADER, OTHERS, OfficeRow, class_of, plain_label, score_clip,
};
use lamp_ears::classes::{Frontend, Model};
use lamp_msgs::{Audio, BodyClass, SoundClasses};
use std::{
    fs,
    io::{self, BufRead, Seek, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::ExitCode,
    time::{Duration, Instant},
};

const USAGE: &str = "usage: lamp-ears-body-kit take DIR LABEL [--count N] [--seconds S] [--model PATH] [--bus-root R]
       lamp-ears-body-kit office DIR [--minutes M] [--bus-root R]
       lamp-ears-body-kit suggest DIR
       lamp-ears-body-kit delete DIR";
const MODEL: &str = "/opt/lampOS/ears/models/yamnet.onnx";
const RATE: u32 = 16_000;
const OFFICE: &str = "office.csv";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("take") => take(&args[1..]),
        Some("office") => office(&args[1..]),
        Some("suggest") => suggest(&args[1..]),
        Some("delete") => delete(&args[1..]),
        _ => Err(USAGE.to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lamp-ears-body-kit: {error}");
            ExitCode::from(2)
        }
    }
}

/// The folder (absolute, never `/`) and the options after it.
fn folder(args: &[String]) -> Result<(PathBuf, &[String]), String> {
    let [dir, rest @ ..] = args else {
        return Err(USAGE.to_owned());
    };
    let dir = PathBuf::from(dir);
    if !dir.is_absolute() || dir.parent().is_none() {
        return Err(format!("{}: an absolute folder, not /", dir.display()));
    }
    Ok((dir, rest))
}

/// `--name value` pairs of `allowed`.
fn options<'a>(args: &'a [String], allowed: &[&str]) -> Result<Vec<(&'a str, &'a str)>, String> {
    let mut found = Vec::new();
    let mut args = args.iter();
    while let Some(name) = args.next() {
        let value = args.next().ok_or_else(|| format!("{name} needs a value"))?;
        if !allowed.contains(&name.as_str()) {
            return Err(USAGE.to_owned());
        }
        found.push((name.as_str(), value.as_str()));
    }
    Ok(found)
}

fn bus(root: Option<&str>) -> Result<lamp_bus::Bus, String> {
    match root {
        Some(root) => lamp_bus::Bus::new(root),
        None => lamp_bus::Bus::from_environment(),
    }
    .map_err(|error| format!("the bus: {error}"))
}

/// The private folder: made 0700 if new; never a link.
fn private(dir: &Path) -> Result<(), String> {
    match fs::symlink_metadata(dir) {
        Ok(meta) if meta.is_dir() => Ok(()),
        Ok(_) => Err(format!("{} is not a folder", dir.display())),
        Err(_) => fs::DirBuilder::new()
            .mode(0o700)
            .create(dir)
            .map_err(|error| format!("{}: {error}", dir.display())),
    }
}

fn create(path: &Path) -> Result<fs::File, String> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| format!("{}: {error}", path.display()))
}

/// A clip's stem (`cough-07`) as label and number.
fn clip_name(stem: &str) -> Option<(&str, u32)> {
    let (label, number) = stem.rsplit_once('-')?;
    let number = number.parse().ok().filter(|_| number.len() >= 2)?;
    plain_label(label).then_some((label, number))
}

fn take(args: &[String]) -> Result<(), String> {
    let [dir, label, rest @ ..] = args else {
        return Err(USAGE.to_owned());
    };
    let (dir, _) = folder(std::slice::from_ref(dir))?;
    if !plain_label(label) {
        return Err(format!(
            "{label}: a label is lowercase letters, digits and _"
        ));
    }
    let (mut count, mut seconds, mut model, mut root) = (20u32, 3.0f64, MODEL, None);
    for (name, value) in options(rest, &["--count", "--seconds", "--model", "--bus-root"])? {
        match name {
            "--count" => {
                count = value
                    .parse()
                    .ok()
                    .filter(|n| (1..=100).contains(n))
                    .ok_or("--count is 1 to 100")?;
            }
            "--seconds" => {
                seconds = value
                    .parse()
                    .ok()
                    .filter(|s| (1.5..=10.0).contains(s))
                    .ok_or("--seconds is 1.5 to 10")?;
            }
            "--model" => model = value,
            _ => root = Some(value),
        }
    }
    let kind = match class_of(label) {
        Some(class) => format!("the person's {}", class.name()),
        None if OTHERS.contains(&label.as_str()) => "another sound".to_owned(),
        None => "another sound (not one of the session's six)".to_owned(),
    };
    let model = Model::load(Path::new(model))?.with_body_sounds(true);
    let frontend = Frontend::new();
    private(&dir)?;
    let bus = bus(root)?;
    let mut mic = bus
        .subscribe(Audio::TOPICS[0], "body_kit")
        .map_err(|error| format!("{}: {error}", Audio::TOPICS[0]))?;
    let mut next = fs::read_dir(&dir)
        .map_err(|error| format!("{}: {error}", dir.display()))?
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            let stem = name.strip_suffix(".wav")?.to_owned();
            clip_name(&stem)
                .filter(|(one, _)| one == label)
                .map(|(_, n)| n)
        })
        .max()
        .unwrap_or(0)
        + 1;
    let samples = (seconds * f64::from(RATE)) as usize;
    let mut taken = 0;
    let mut last: Option<PathBuf> = None;
    let stdin = io::stdin();
    while taken < count {
        eprint!(
            "{label} ({kind}), take {} of {count}: Enter, then the sound at \"now\" (r: again, q: stop) ",
            taken + 1
        );
        let mut line = String::new();
        if stdin
            .lock()
            .read_line(&mut line)
            .map_err(|e| e.to_string())?
            == 0
        {
            break;
        }
        match line.trim() {
            "q" => break,
            "r" => {
                let Some(stem) = last.take() else {
                    eprintln!("no take to do again");
                    continue;
                };
                remove_clip(&stem)?;
                next -= 1;
                taken -= 1;
                eprintln!("{} removed", stem.display());
                continue;
            }
            _ => {}
        }
        // What waited in the socket is from before: only what comes next.
        while matches!(mic.recv(Duration::ZERO), Ok(Some(_))) {}
        eprintln!("now");
        let pcm = match record(&mut mic, samples, seconds) {
            Ok(pcm) => pcm,
            Err(error) => {
                eprintln!("{error}: not kept, take it again");
                continue;
            }
        };
        let stem = dir.join(format!("{label}-{next:02}"));
        write_wav(&stem.with_extension("wav"), &pcm)?;
        let scaled: Vec<f32> = pcm.iter().map(|&x| f32::from(x) / 32768.0).collect();
        let patches = score_clip(&model, &frontend, &scaled)?;
        create(&stem.with_extension("csv"))?
            .write_all(body_kit::clip_csv(&patches).as_bytes())
            .map_err(|error| format!("{}: {error}", stem.display()))?;
        let clip = Clip::parse(label, &body_kit::clip_csv(&patches))?;
        let peaks: Vec<String> = BodyClass::ALL
            .iter()
            .map(|&class| format!("{} {:.2}", class.name(), clip.peak(class)))
            .collect();
        eprintln!("{}: peak {}", stem.display(), peaks.join(", "));
        last = Some(stem);
        next += 1;
        taken += 1;
    }
    eprintln!("{taken} {label} clips taken into {}", dir.display());
    Ok(())
}

/// `samples` of mic1, from blocks in order with none lost.
fn record(
    mic: &mut lamp_bus::Subscriber,
    samples: usize,
    seconds: f64,
) -> Result<Vec<i16>, String> {
    let until = Instant::now() + Duration::from_secs_f64(seconds + 1.0);
    let mut pcm = Vec::with_capacity(samples);
    let mut sequence = None;
    while pcm.len() < samples {
        if Instant::now() > until {
            return Err("mic1/audio stopped (is LAMP_BUS_AUDIO=on for lamp-ears?)".into());
        }
        let Some(message) = mic
            .recv(Duration::from_millis(100))
            .map_err(|error| format!("mic1/audio: {error}"))?
        else {
            continue;
        };
        let (header, bytes) =
            Audio::split(&message.payload).map_err(|error| format!("mic1/audio: {error}"))?;
        if header.sample_rate != RATE
            || header.channels != 1
            || header.sample_format != Audio::S16LE
        {
            return Err("mic1/audio is not 16 kHz mono S16LE".into());
        }
        if sequence.is_some_and(|last: u64| header.sequence != last + 1) {
            return Err("sound was lost in the take".into());
        }
        sequence = Some(header.sequence);
        pcm.extend(
            bytes
                .chunks_exact(2)
                .map(|pair| i16::from_le_bytes([pair[0], pair[1]])),
        );
    }
    pcm.truncate(samples);
    Ok(pcm)
}

fn write_wav(path: &Path, pcm: &[i16]) -> Result<(), String> {
    let said = |error: hound::Error| format!("{}: {error}", path.display());
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer =
        hound::WavWriter::new(io::BufWriter::new(create(path)?), spec).map_err(said)?;
    for &sample in pcm {
        writer.write_sample(sample).map_err(said)?;
    }
    writer.finalize().map_err(said)
}

/// A clip's sound zero-filled, then it and its scores removed.
fn remove_clip(stem: &Path) -> Result<(), String> {
    let wav = stem.with_extension("wav");
    zero(&wav)?;
    for path in [wav, stem.with_extension("csv")] {
        if fs::symlink_metadata(&path).is_ok() {
            fs::remove_file(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        }
    }
    Ok(())
}

/// Best effort, as for the ears' own buffers: a flash card may keep the old
/// blocks, so the folder stays private until it is gone.
fn zero(path: &Path) -> Result<(), String> {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    if !meta.is_file() {
        return Ok(());
    }
    // The folder is the operator's own, 0700: a file checked here is the
    // file opened.
    let said = |error: io::Error| format!("{}: {error}", path.display());
    let mut file = fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(said)?;
    file.seek(io::SeekFrom::Start(0)).map_err(said)?;
    io::copy(&mut io::Read::take(io::repeat(0), meta.len()), &mut file).map_err(said)?;
    file.sync_all().map_err(said)
}

fn office(args: &[String]) -> Result<(), String> {
    let (dir, rest) = folder(args)?;
    let (mut minutes, mut root) = (60.0f64, None);
    for (name, value) in options(rest, &["--minutes", "--bus-root"])? {
        match name {
            "--minutes" => {
                minutes = value
                    .parse()
                    .ok()
                    .filter(|m| (0.1..=600.0).contains(m))
                    .ok_or("--minutes is 0.1 to 600")?;
            }
            _ => root = Some(value),
        }
    }
    private(&dir)?;
    let bus = bus(root)?;
    let mut classes = bus
        .subscribe(SoundClasses::TOPIC, "body_kit")
        .map_err(|error| format!("{}: {error}", SoundClasses::TOPIC))?;
    let path = dir.join(OFFICE);
    let mut out = io::BufWriter::new(create(&path)?);
    let said = |error: io::Error| format!("{}: {error}", path.display());
    writeln!(out, "{OFFICE_HEADER}").map_err(said)?;
    let until = Instant::now() + Duration::from_secs_f64(minutes * 60.0);
    let (mut windows, mut body) = (0usize, false);
    eprintln!("logging body scores only, no sound, for {minutes} minutes");
    while Instant::now() < until {
        let Some(message) = classes
            .recv(Duration::from_millis(250))
            .map_err(|error| format!("{}: {error}", SoundClasses::TOPIC))?
        else {
            continue;
        };
        if let Ok(SoundClasses::Scored {
            window,
            scores,
            own_voice,
            ..
        }) = message.parse::<SoundClasses>()
        {
            let row = OfficeRow::of(window.end_us, own_voice, &scores);
            body |= row.scores.iter().any(|&score| score > 0.0);
            writeln!(out, "{}", row.csv()).map_err(said)?;
            out.flush().map_err(said)?;
            windows += 1;
        }
    }
    eprintln!("{windows} windows logged to {}", path.display());
    if windows > 0 && !body {
        eprintln!("no body score above 0: is LAMP_EARS_BODY_SOUNDS=on for lamp-ears?");
    }
    Ok(())
}

fn suggest(args: &[String]) -> Result<(), String> {
    let (dir, rest) = folder(args)?;
    if !rest.is_empty() {
        return Err(USAGE.to_owned());
    }
    let mut clips = Vec::new();
    let mut office = None;
    let mut names: Vec<String> = fs::read_dir(&dir)
        .map_err(|error| format!("{}: {error}", dir.display()))?
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .collect();
    names.sort();
    for name in names {
        let path = dir.join(&name);
        let text = || fs::read_to_string(&path).map_err(|error| format!("{name}: {error}"));
        if name == OFFICE {
            let mut rows = Vec::new();
            for line in text()?.lines() {
                rows.extend(OfficeRow::parse(line)?);
            }
            office = Some(rows);
        } else if let Some((label, _)) = name.strip_suffix(".csv").and_then(clip_name) {
            clips.push(Clip::parse(label, &text()?)?);
        }
    }
    if office.is_none() {
        eprintln!("no {OFFICE}: alarms an hour unknown, so no class is publishable yet");
    }
    let suggestions = body_kit::suggest(&clips, office.as_deref());
    print!("{}", body_kit::report(&clips, &suggestions));
    Ok(())
}

fn delete(args: &[String]) -> Result<(), String> {
    let (dir, rest) = folder(args)?;
    if !rest.is_empty() {
        return Err(USAGE.to_owned());
    }
    let meta = fs::symlink_metadata(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    if !meta.is_dir() {
        return Err(format!("{} is not a folder", dir.display()));
    }
    let mut kept = Vec::new();
    let mut removed = 0;
    for entry in fs::read_dir(&dir).map_err(|error| format!("{}: {error}", dir.display()))? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let file = fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file());
        let ours = name == OFFICE
            || name
                .strip_suffix(".wav")
                .or_else(|| name.strip_suffix(".csv"))
                .is_some_and(|stem| clip_name(stem).is_some());
        if file && ours {
            zero(&path)?;
            fs::remove_file(&path).map_err(|error| format!("{name}: {error}"))?;
            removed += 1;
        } else {
            kept.push(name);
        }
    }
    if kept.is_empty() {
        fs::remove_dir(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
        println!("{removed} files and {} removed", dir.display());
    } else {
        println!(
            "{removed} files removed; {} kept, not the kit's: {}",
            dir.display(),
            kept.join(", ")
        );
    }
    Ok(())
}
