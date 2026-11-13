//! The voice's stand-in on the say socket, and lamp-actions' on its call
//! socket (the volume). A line in, `begun`, the host's speech engine, then
//! the outcome `spoken`, as lamp-control reads them (`parts.rs`). One line at
//! a time: a second `say` waits for the first. A computer's `stop` writes
//! `{"cancel":true}` on its say's connection and the speech ends at once,
//! answered `dropped`. A computer's word-for-word `say` is also told on
//! `voice/line` as it begins, as the real voice does, so `lamp.said` reaches
//! a computer holding `speech`.
use lamp_bus::{Bus, Publisher};
use lamp_msgs::{Audio, LineBy, LineSource, SaidLine, SpeakerVolume};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::Path,
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

/// The most a line is spoken for when the host has no speech engine.
const SILENT_MAX: Duration = Duration::from_secs(4);

/// Serves the say socket at `path`, forever, telling each line on `bus`'s
/// `voice/line`. The topic is declared before the socket exists, so a socket
/// that is there means a subscriber can already find the topic.
pub fn serve_say(path: &Path, bus: &Bus) -> std::io::Result<()> {
    let lines = Arc::new(Mutex::new(bus.publisher_declared(
        SaidLine::TOPIC,
        &SaidLine::declaration(),
        "voice",
    )?));
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    for stream in listener.incoming().flatten() {
        let lines = Arc::clone(&lines);
        let bus = bus.clone();
        thread::spawn(move || {
            let _ = say(stream, &lines, &bus);
        });
    }
    Ok(())
}

/// Tells a computer's word-for-word line on `voice/line`, as the real voice
/// does when it begins to play (`SaidLine`, docs/msgs.md): the words, `by`
/// `computer`, `source` `exact`, and the computer's id as `app`. Nothing else
/// is told: a line without `by` is a written turn for the model (there is
/// none here), a skill's or a cue's has no words to tell, and an empty one is none.
fn tell(lines: &Mutex<Publisher>, asked: &Value) {
    if asked["skill"] != "control" || asked["exact"] != true {
        return;
    }
    let (Some(text), Some(by)) = (asked["text"].as_str(), asked["by"].as_str()) else {
        return;
    };
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    let line = SaidLine {
        text: text.to_owned(),
        by: LineBy::Computer,
        source: LineSource::Exact,
        app: Some(by.to_owned()),
        request: None,
        cut: false,
        scope: None,
    };
    let mut lines = lines
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _ = lines.publish_json(lamp_ipc::monotonic_us(), &line);
}

/// How often the voice tells lamp-control how its stream is doing (`stats`).
const STATS_EVERY: Duration = Duration::from_secs(1);
/// How long the stream waits for a block before it looks at the socket again.
const BLOCK_WAIT: Duration = Duration::from_millis(50);

/// The host program that plays the stream's raw sound (24 kHz mono S16LE on its standard input), the first of the host's players
/// that starts: `paplay`, `aplay`, `ffplay`. None (`LAMP_SIM_SPEAKER=silent`, or no player): the blocks are counted, not played.
fn speaker_player() -> Option<std::process::Child> {
    if std::env::var("LAMP_SIM_SPEAKER").is_ok_and(|v| v == "silent") {
        return None;
    }
    let players: [(&str, &[&str]); 3] = [
        (
            "paplay",
            &["--raw", "--rate=24000", "--format=s16le", "--channels=1"],
        ),
        (
            "aplay",
            &[
                "-q", "-t", "raw", "-f", "S16_LE", "-r", "24000", "-c", "1", "-",
            ],
        ),
        (
            "ffplay",
            &[
                "-nodisp",
                "-autoexit",
                "-loglevel",
                "quiet",
                "-f",
                "s16le",
                "-ar",
                "24000",
                "-ac",
                "1",
                "-i",
                "-",
            ],
        ),
    ];
    for (program, args) in players {
        if let Ok(mut child) = crate::keys::command(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            // A player that is installed but fails at once (no sound server) is passed over for the next.
            thread::sleep(Duration::from_millis(150));
            if matches!(child.try_wait(), Ok(None)) {
                return Some(child);
            }
        }
    }
    None
}

/// Blocks queued for the player's writer thread at most (about a second and a quarter of 20 ms blocks).
const PLAYER_QUEUED: usize = 64;
/// How long a player is given to play what was queued once the stream has ended; then it is killed.
const PLAYER_GRACE: Duration = Duration::from_secs(2);

/// Ends the player: what is queued is written and its stdin closed (the writer thread's end), it plays on to
/// the end for up to [`PLAYER_GRACE`], then it is killed whatever it does, and waited for: a player that
/// never reads does not hold the stream or leave a process behind.
fn end_player(mut child: std::process::Child, writer: Option<thread::JoinHandle<()>>) {
    let until = std::time::Instant::now() + PLAYER_GRACE;
    loop {
        let written = writer.as_ref().is_none_or(thread::JoinHandle::is_finished);
        if written && matches!(child.try_wait(), Ok(Some(_))) {
            break;
        }
        if std::time::Instant::now() >= until {
            let _ = child.kill();
            let _ = child.wait();
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    if let Some(writer) = writer {
        // After the kill the pipe is broken, so a write that was stuck has ended.
        let _ = writer.join();
    }
}

/// The voice's side of `speaker.stream`: lamp-control asks on the say socket (`{"skill":"control","stream":{"by":CLIENT}}`), the
/// voice answers `{"event":"stream","state":"open"}` and then reads `speaker/audio` (lamp-control publishes each block of the
/// computer's sound there, 24 kHz mono S16LE) while the connection stays open, playing it on the host if there is a player and
/// telling `stats` each second (with the blocks and milliseconds heard: lamp-control reads only its three counters). Dropping the
/// connection ends the stream. A topic nobody declared (the owner's switch is off) is refused in words.
fn speaker_stream(
    mut stream: UnixStream,
    asked: &Value,
    bus: &Bus,
    mut player: Option<std::process::Child>,
) -> std::io::Result<()> {
    let by = asked["stream"]["by"]
        .as_str()
        .unwrap_or("a computer")
        .to_owned();
    let mut blocks = match bus.subscribe_typed(Audio::SPEAKER_TOPIC, "sim_speaker", Audio::TYPE) {
        Ok(blocks) => blocks,
        Err(error) => {
            // The player was probed before the answer: it is ended and waited for, not left.
            if let Some(mut child) = player {
                let _ = child.kill();
                let _ = child.wait();
            }
            return writeln!(
                stream,
                "{}",
                json!({"event": "stream", "state": "refused", "why": format!("the sim's speaker is not on the bus: {error}")})
            );
        }
    };
    writeln!(stream, "{}", json!({"event": "stream", "state": "open"}))?;
    // The door closing its end is the end of the stream.
    let closed = Arc::new(AtomicBool::new(false));
    {
        let (reader, closed) = (stream.try_clone()?, Arc::clone(&closed));
        thread::spawn(move || {
            for _ in BufReader::new(reader).lines().map_while(Result::ok) {}
            closed.store(true, Ordering::SeqCst);
        });
    }
    // The player is fed by a thread of its own through a bounded queue: a player that does not read (stalled,
    // a full pipe) holds that thread, never the stats or the door's closing. A block the queue has no room for
    // is not given to the player, and counted.
    let (mut feed, writer) = match player.as_mut().and_then(|child| child.stdin.take()) {
        Some(mut stdin) => {
            let (feed, queued) = mpsc::sync_channel::<Vec<u8>>(PLAYER_QUEUED);
            let writer = thread::spawn(move || {
                for bytes in queued {
                    if stdin.write_all(&bytes).is_err() {
                        break;
                    }
                }
            });
            (Some(feed), Some(writer))
        }
        None => (None, None),
    };
    let mut not_played = 0u64;
    let (mut heard, mut samples, mut next_stats) =
        (0u64, 0u64, std::time::Instant::now() + STATS_EVERY);
    while !closed.load(Ordering::SeqCst) {
        if let Ok(Some(message)) = blocks.recv(BLOCK_WAIT)
            && let Ok((header, bytes)) = Audio::split(&message.payload)
            && header.sample_rate == Audio::SPEAKER_SAMPLE_RATE
            && header.channels == 1
            && bytes.len() % 2 == 0
        {
            heard += 1;
            samples += (bytes.len() / 2) as u64;
            if let Some(queue) = &feed {
                match queue.try_send(bytes.to_vec()) {
                    Ok(()) => {}
                    Err(mpsc::TrySendError::Full(_)) => not_played += 1,
                    // The player ended: go on counting.
                    Err(mpsc::TrySendError::Disconnected(_)) => feed = None,
                }
            }
        }
        if std::time::Instant::now() >= next_stats {
            next_stats += STATS_EVERY;
            let ms = samples * 1000 / u64::from(Audio::SPEAKER_SAMPLE_RATE);
            if writeln!(stream, "{}", json!({"event": "stats", "buffered_ms": 0, "underruns": 0, "limited": 0, "blocks": heard, "ms": ms})).is_err() {
                break;
            }
        }
    }
    drop(feed);
    if let Some(child) = player {
        end_player(child, writer);
    }
    eprintln!(
        "lamp-sim: the speaker stream of {by} ended: {heard} blocks, {} ms{}",
        samples * 1000 / u64::from(Audio::SPEAKER_SAMPLE_RATE),
        if not_played > 0 {
            format!(" ({not_played} not given to the player: it was not keeping up)")
        } else {
            String::new()
        }
    );
    Ok(())
}

/// One line is spoken at a time.
static SPEAKING: Mutex<()> = Mutex::new(());

fn say(mut stream: UnixStream, lines: &Mutex<Publisher>, bus: &Bus) -> std::io::Result<()> {
    let mut line = String::new();
    BufReader::new(stream.try_clone()?).read_line(&mut line)?;
    let Ok(asked) = serde_json::from_str::<Value>(&line) else {
        return Ok(());
    };
    // A computer's own sound for the speaker (`speaker.stream`): not a line to say.
    if asked["stream"].is_object() {
        return speaker_stream(stream, &asked, bus, speaker_player());
    }
    // The cancel line, whenever it comes, on this connection.
    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let (reader, cancelled) = (stream.try_clone()?, Arc::clone(&cancelled));
        thread::spawn(move || {
            for line in BufReader::new(reader).lines().map_while(Result::ok) {
                if serde_json::from_str::<Value>(&line).is_ok_and(|v| v["cancel"] == true) {
                    cancelled.store(true, Ordering::SeqCst);
                }
            }
            // The door closed the connection: nothing waits for the words.
            cancelled.store(true, Ordering::SeqCst);
        });
    }
    // A second line waits its turn (and is let go if it is cancelled waiting).
    let _turn = loop {
        match SPEAKING.try_lock() {
            Ok(guard) => break guard,
            // A line that panicked while speaking left no state to protect.
            Err(std::sync::TryLockError::Poisoned(poisoned)) => break poisoned.into_inner(),
            Err(_) if cancelled.load(Ordering::SeqCst) => {
                writeln!(
                    stream,
                    "{}",
                    json!({"outcome": "dropped", "why": "stopped by its asker"})
                )?;
                return Ok(());
            }
            Err(_) => thread::sleep(Duration::from_millis(20)),
        }
    };
    writeln!(
        stream,
        "{}",
        json!({"event": "begun", "at_us": lamp_ipc::monotonic_us(), "id": 1})
    )?;
    tell(lines, &asked);
    match (asked["text"].as_str(), asked["clip"].as_str()) {
        (Some(text), _) => speak(text, &cancelled),
        // A computer's clip, played by the host: the door gives the path of a clip it checked.
        (None, Some(clip)) => play(clip, &cancelled),
        // The lamp's own hum: a short while of sound.
        (None, None) => wait(Duration::from_millis(600), &cancelled),
    }
    if cancelled.load(Ordering::SeqCst) {
        writeln!(
            stream,
            "{}",
            json!({"outcome": "dropped", "why": "stopped by its asker"})
        )?;
    } else {
        writeln!(stream, "{}", json!({"outcome": "spoken"}))?;
    }
    Ok(())
}

/// Waits `long`, or until `cancelled`.
fn wait(long: Duration, cancelled: &AtomicBool) {
    let end = std::time::Instant::now() + long;
    while std::time::Instant::now() < end && !cancelled.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(20));
    }
}

/// Whether `path` may be handed to a player as its argument: an absolute path,
/// never one that starts with `-` (read as an option).
fn playable(path: &str) -> bool {
    path.starts_with('/')
}

/// Plays the clip at `path` on the host: the first of the host's players that
/// starts, else a silent wait; cut off when `cancelled`.
fn play(path: &str, cancelled: &AtomicBool) {
    // The players read a path that starts with `-` as an option (`ffplay -f ...`):
    // lamp-control gives the path of a clip it checked, which is absolute; anything
    // else is not played.
    if !playable(path) {
        wait(Duration::from_millis(600), cancelled);
        return;
    }
    let engines: [(&str, &[&str]); 4] = [
        ("paplay", &[path]),
        ("aplay", &["-q", path]),
        ("afplay", &[path]),
        (
            "ffplay",
            &["-nodisp", "-autoexit", "-loglevel", "quiet", path],
        ),
    ];
    if !speak_with(&engines, "", cancelled) {
        wait(Duration::from_millis(600), cancelled);
    }
}

/// The host's speech engines, in the order tried, each with the arguments that
/// make it read its standard input and (for `spd-say`) wait until it has spoken.
/// `spd-say` is last: speech-dispatcher (Ubuntu and most Linux desktops have it,
/// and often no other engine) hands the line to its own synthesizer.
const ENGINES: &[(&str, &[&str])] = &[
    ("say", &["-f", "-"]),
    ("espeak-ng", &["--stdin"]),
    ("espeak", &["--stdin"]),
    ("spd-say", &["--wait", "--pipe-mode"]),
];

/// Speaks `text` on the host and returns when done: the host's own engine
/// where there is one, else a silent wait as long as the line. The text goes
/// to the engine on its standard input, never as an argument, so a line that
/// starts with `-` is words, not an option (`say -f FILE` would read a file
/// aloud).
fn speak(text: &str, cancelled: &AtomicBool) {
    if !speak_with(ENGINES, text, cancelled) {
        wait(
            Duration::from_millis(60 * text.chars().count() as u64).min(SILENT_MAX),
            cancelled,
        );
    }
}

/// Speaks `text` with the first of `engines` that starts and does not fail at
/// once (a player that is installed but exits with an error, such as `paplay`
/// with no sound server, is passed over for the next); false if none spoke.
fn speak_with(engines: &[(&str, &[&str])], text: &str, cancelled: &AtomicBool) -> bool {
    for (program, args) in engines {
        let Ok(mut child) = crate::keys::command(program)
            .args(*args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue;
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        // Until it ends, or the words are cancelled: then it is cut off.
        loop {
            match child.try_wait() {
                // It ended: spoken if it says so, else the next engine tries.
                Ok(Some(status)) if status.success() => return true,
                Ok(Some(_)) => break,
                _ => {}
            }
            if cancelled.load(Ordering::SeqCst) {
                let _ = child.kill();
                let _ = child.wait();
                cut_off(program);
                return true;
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
    false
}

/// Cuts off what an engine has already handed on, when its client is killed:
/// speech-dispatcher goes on speaking a queued line after `spd-say` is gone, so
/// `spd-say --cancel` stops it (measured: a killed client's line ran to its end,
/// and was silent within half a second of the cancel). The other engines speak
/// only as long as they run. Known limit: `--cancel` is "CANCEL ALL", so it also
/// cuts any other speech of the same user's speech-dispatcher (a screen reader);
/// speech-dispatcher's own socket has a per-client `CANCEL SELF`, not used here.
fn cut_off(program: &str) {
    if Path::new(program)
        .file_name()
        .is_some_and(|name| name == "spd-say")
    {
        let _ = crate::keys::command(program)
            .arg("--cancel")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// The speaker's volume as lamp-actions keeps it (actions/src/sound.rs): a level of 0 to 100, a step of 20 with a floor of 10
/// that a "quieter" does not go under, a mute that is a level of 0 and remembers where it was, and an unmute that goes back there
/// (the middle when it was never muted here). No hardware: the level is the whole of it.
pub struct Speaker {
    level: u8,
    before_mute: Option<u8>,
}

const STEP: u8 = 20;
const FLOOR: u8 = 10;

impl Speaker {
    /// Half way, as a lamp comes up with no memory of its last level.
    pub fn new() -> Self {
        Self {
            level: 50,
            before_mute: None,
        }
    }

    pub fn level(&self) -> u8 {
        self.level
    }

    /// One app call (`volume_level`, `volume_to`, `volume_step`, `volume_muted`); whether the level changed. An unknown call or
    /// argument changes nothing and is answered with the level, as the stand-in always did.
    pub fn call(&mut self, name: &str, args: &Value) -> bool {
        let before = self.level;
        match name {
            "volume_to" => {
                if let Some(to) = args["volume"].as_u64() {
                    self.level = u8::try_from(to.min(100)).unwrap_or(100);
                    self.before_mute = None;
                }
            }
            "volume_step" => match args["step"].as_str() {
                Some("up") => {
                    self.level = self.level.saturating_add(STEP).min(100);
                    self.before_mute = None;
                }
                Some("down") if self.level > FLOOR => {
                    self.level = FLOOR.max(self.level.saturating_sub(STEP))
                }
                _ => {}
            },
            "volume_muted" => match args["muted"].as_bool() {
                Some(true) if self.level > 0 => {
                    self.before_mute = Some(self.level);
                    self.level = 0;
                }
                Some(false) if self.level == 0 => {
                    self.level = self.before_mute.take().unwrap_or(50).clamp(1, 100);
                }
                _ => {}
            },
            _ => {}
        }
        self.level != before
    }

    /// lamp-actions' answer to an app call.
    pub fn answer(&self) -> Value {
        json!({"ok": true, "level": {"volume": self.level, "raw": self.level, "max_volume": 100}})
    }
}

impl Default for Speaker {
    fn default() -> Self {
        Self::new()
    }
}

/// Serves lamp-actions' call socket at `path`, forever: the four app calls of the volume, and `speaker/volume` said on `bus` once
/// at the start and after every change, as lamp-actions says it, so lamp-control's `get volume` and `volume.changed` work.
pub fn serve_actions(path: &Path, bus: &Bus) -> std::io::Result<()> {
    let mut publisher = bus.publisher_declared(
        SpeakerVolume::TOPIC,
        &SpeakerVolume::declaration(),
        SpeakerVolume::SOURCE,
    )?;
    let speaker = Arc::new(Mutex::new(Speaker::new()));
    let _ = publisher.publish_json(
        lamp_ipc::monotonic_us(),
        &SpeakerVolume {
            volume: Speaker::new().level(),
            muted: false,
        },
    );
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    let publisher = Arc::new(Mutex::new(publisher));
    for stream in listener.incoming().flatten() {
        let speaker = Arc::clone(&speaker);
        let publisher = Arc::clone(&publisher);
        thread::spawn(move || {
            let _ = action(stream, &speaker, &publisher);
        });
    }
    Ok(())
}

fn action(
    mut stream: UnixStream,
    speaker: &Mutex<Speaker>,
    publisher: &Mutex<Publisher>,
) -> std::io::Result<()> {
    let mut line = String::new();
    BufReader::new(stream.try_clone()?).read_line(&mut line)?;
    let Ok(asked) = serde_json::from_str::<Value>(&line) else {
        return Ok(());
    };
    let (answer, news) = {
        let mut speaker = speaker.lock().unwrap_or_else(|e| e.into_inner());
        let changed = speaker.call(asked["name"].as_str().unwrap_or(""), &asked["args"]);
        (speaker.answer(), changed.then(|| speaker.level()))
    };
    if let Some(level) = news
        && let Ok(mut publisher) = publisher.lock()
    {
        let _ = publisher.publish_json(
            lamp_ipc::monotonic_us(),
            &SpeakerVolume {
                volume: level,
                muted: level == 0,
            },
        );
    }
    writeln!(stream, "{answer}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// A folder to run a fake engine from: under the crate, since a temporary
    /// folder may not allow running a program.
    fn scratch(name: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("test-scratch-{name}-{}", std::process::id()))
    }

    #[test]
    fn a_text_that_starts_with_a_dash_reaches_the_engine_as_words_on_stdin() {
        let _one_at_a_time = crate::FAKE_PROGRAM
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = scratch("voice");
        std::fs::create_dir_all(&dir).unwrap();
        let engine = dir.join("engine.sh");
        let (args, said) = (dir.join("args"), dir.join("said"));
        std::fs::write(
            &engine,
            format!(
                "#!/bin/sh\necho \"$@\" > {}\ncat > {}\n",
                args.display(),
                said.display()
            ),
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let program = engine.to_str().unwrap();
        let never = AtomicBool::new(false);
        assert!(speak_with(
            &[(program, &["--stdin"])],
            "-f /etc/hostname",
            &never
        ));
        assert_eq!(std::fs::read_to_string(&args).unwrap().trim(), "--stdin");
        assert_eq!(std::fs::read_to_string(&said).unwrap(), "-f /etc/hostname");
        assert!(!speak_with(
            &[("lamp-sim-no-such-engine", &[])],
            "hello",
            &never
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cancel_cuts_the_speech_off_at_once() {
        let _one_at_a_time = crate::FAKE_PROGRAM
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = scratch("cancel");
        std::fs::create_dir_all(&dir).unwrap();
        let engine = dir.join("slow.sh");
        std::fs::write(&engine, "#!/bin/sh\ncat > /dev/null\nsleep 30\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        let later = Arc::clone(&cancelled);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(200));
            later.store(true, Ordering::SeqCst);
        });
        let started = std::time::Instant::now();
        assert!(speak_with(
            &[(engine.to_str().unwrap(), &[])],
            "a long line",
            &cancelled
        ));
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_clip_is_handed_to_the_player_as_its_path() {
        let _one_at_a_time = crate::FAKE_PROGRAM
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = scratch("clip");
        std::fs::create_dir_all(&dir).unwrap();
        let engine = dir.join("player.sh");
        let args = dir.join("args");
        std::fs::write(
            &engine,
            format!("#!/bin/sh\necho \"$@\" > {}\n", args.display()),
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let never = AtomicBool::new(false);
        assert!(speak_with(
            &[(engine.to_str().unwrap(), &["-q", "/clips/a.wav"])],
            "",
            &never
        ));
        assert_eq!(
            std::fs::read_to_string(&args).unwrap().trim(),
            "-q /clips/a.wav"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_computers_exact_line_is_told_on_voice_line_and_nothing_else_is() {
        let dir = crate::test_root().join(format!("lsv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bus = Bus::new(dir.join("bus")).unwrap();
        let lines = Mutex::new(
            bus.publisher_declared(SaidLine::TOPIC, &SaidLine::declaration(), "voice")
                .unwrap(),
        );
        let mut heard = bus
            .subscribe_typed(SaidLine::TOPIC, "test", SaidLine::TYPE)
            .unwrap();
        // Told: a computer's word-for-word say, as lamp-control sends it.
        tell(
            &lines,
            &json!({"skill": "control", "text": " Hello there. ", "exact": true, "by": "c_7"}),
        );
        // Not told: a written turn for the model (no `by`), a skill's line, a cue, an empty one.
        tell(&lines, &json!({"skill": "control", "text": "no by"}));
        tell(
            &lines,
            &json!({"skill": "control", "text": "no by", "exact": true}),
        );
        tell(
            &lines,
            &json!({"skill": "timer", "text": "skill", "exact": true, "by": "c_7"}),
        );
        tell(&lines, &json!({"skill": "control", "cue": "hum"}));
        tell(
            &lines,
            &json!({"skill": "control", "text": "  ", "exact": true, "by": "c_7"}),
        );
        let message = heard.recv(Duration::from_secs(2)).unwrap().unwrap();
        let line: SaidLine = message.parse().unwrap();
        assert_eq!(
            line,
            SaidLine {
                text: "Hello there.".to_owned(),
                by: LineBy::Computer,
                source: LineSource::Exact,
                app: Some("c_7".to_owned()),
                request: None,
                cut: false,
                scope: None,
            }
        );
        assert!(heard.recv(Duration::from_millis(300)).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn spd_say_is_an_engine_that_reads_stdin_and_waits_and_is_cancelled_by_its_cancel() {
        let _one_at_a_time = crate::FAKE_PROGRAM
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // Listed, with the text on stdin (never an argument), after the engines that were.
        assert_eq!(
            ENGINES.last(),
            Some(&("spd-say", &["--wait", "--pipe-mode"][..]))
        );
        assert_eq!(ENGINES.len(), 4);
        // A fake `spd-say` that logs how it was called and speaks "for ever". It starts slowly (0.6 s before its first
        // line), as a just-written executable can on macOS: the cancel must wait for that line, not for a fixed time.
        let dir = scratch("spd");
        std::fs::create_dir_all(&dir).unwrap();
        let engine = dir.join("spd-say");
        let log = dir.join("calls");
        std::fs::write(
            &engine,
            format!(
                "#!/bin/sh\nsleep 0.6\necho \"$@\" >> {}\n[ \"$1\" = --cancel ] && exit 0\ncat > /dev/null\nsleep 30\n",
                log.display()
            ),
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let program = engine.to_str().unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancelled);
        let watched = log.clone();
        // The cancel is raised once the fake has logged its first call (up to 5 s; a fixed wait raced a slow start).
        let late = thread::spawn(move || {
            let until = std::time::Instant::now() + Duration::from_secs(5);
            while std::time::Instant::now() < until
                && !std::fs::read_to_string(&watched).is_ok_and(|text| text.contains('\n'))
            {
                thread::sleep(Duration::from_millis(10));
            }
            flag.store(true, Ordering::SeqCst);
        });
        let began = std::time::Instant::now();
        assert!(speak_with(
            &[(program, &["--wait", "--pipe-mode"])],
            "hello",
            &cancelled
        ));
        late.join().unwrap();
        // Cut off at the cancel (not after the fake's 30 s), and the client told
        // speech-dispatcher to stop what it had queued.
        assert!(began.elapsed() < Duration::from_secs(10));
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "--wait --pipe-mode\n--cancel\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_player_that_fails_at_once_is_passed_over_for_the_next_and_a_dash_path_is_no_clip() {
        let _one_at_a_time = crate::FAKE_PROGRAM
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = scratch("fails");
        std::fs::create_dir_all(&dir).unwrap();
        let script = |name: &str, body: &str| {
            use std::os::unix::fs::PermissionsExt;
            let path = dir.join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
            path
        };
        let (log, first, second) = (dir.join("log"), script("first", ""), script("second", ""));
        std::fs::write(
            &first,
            format!("#!/bin/sh\necho first >> {}\nexit 1\n", log.display()),
        )
        .unwrap();
        std::fs::write(
            &second,
            format!("#!/bin/sh\necho second >> {}\nexit 0\n", log.display()),
        )
        .unwrap();
        let cancelled = AtomicBool::new(false);
        let (a, b) = (first.to_str().unwrap(), second.to_str().unwrap());
        // The first fails at once, so the second is tried and speaks.
        assert!(speak_with(&[(a, &[]), (b, &[])], "", &cancelled));
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "first\nsecond\n");
        // Every engine failing is no engine: the caller's silent wait follows.
        assert!(!speak_with(&[(a, &[]), (a, &[])], "", &cancelled));
        // A path that starts with a dash is no clip for a player.
        assert!(playable("/clips/a.wav"));
        assert!(!playable("-f/etc/hostname") && !playable("clips/a.wav"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_speaker_steps_by_twenty_with_a_floor_of_ten_and_mutes_and_unmutes_as_lamp_actions_do() {
        let mut speaker = Speaker::new();
        assert_eq!(speaker.level(), 50);
        assert!(speaker.call("volume_step", &json!({"step": "down"})));
        assert_eq!(speaker.level(), 30);
        assert!(speaker.call("volume_step", &json!({"step": "down"})));
        assert_eq!(speaker.level(), 10);
        assert!(
            !speaker.call("volume_step", &json!({"step": "down"})),
            "the floor: quieter does not go under it"
        );
        assert!(speaker.call("volume_step", &json!({"step": "up"})));
        assert_eq!(speaker.level(), 30);
        assert!(speaker.call("volume_to", &json!({"volume": 90})));
        assert!(speaker.call("volume_step", &json!({"step": "up"})));
        assert_eq!(speaker.level(), 100, "the ceiling");
        assert!(!speaker.call("volume_step", &json!({"step": "up"})));
        // Mute remembers where it was; unmute goes back; a second of either changes nothing.
        assert!(speaker.call("volume_to", &json!({"volume": 70})));
        assert!(speaker.call("volume_muted", &json!({"muted": true})));
        assert_eq!(speaker.level(), 0);
        assert_eq!(
            speaker.answer()["level"]["raw"],
            0,
            "lamp-control reads muted as a raw level of nothing"
        );
        assert!(!speaker.call("volume_muted", &json!({"muted": true})));
        assert!(
            !speaker.call("volume_step", &json!({"step": "down"})),
            "a muted speaker is not stepped down"
        );
        assert!(speaker.call("volume_muted", &json!({"muted": false})));
        assert_eq!(speaker.level(), 70);
        assert!(!speaker.call("volume_muted", &json!({"muted": false})));
        // Never muted here: an unmute of a speaker brought to 0 by volume_to goes to the middle.
        assert!(speaker.call("volume_to", &json!({"volume": 0})));
        assert!(speaker.call("volume_muted", &json!({"muted": false})));
        assert_eq!(speaker.level(), 50);
        // A call it does not know, or a bad argument, changes nothing and is answered.
        assert!(!speaker.call("volume_level", &json!({})));
        assert!(!speaker.call("volume_step", &json!({"step": "sideways"})));
        assert!(!speaker.call("volume_to", &json!({"volume": "loud"})));
        assert_eq!(
            speaker.answer(),
            json!({"ok": true, "level": {"volume": 50, "raw": 50, "max_volume": 100}})
        );
    }

    #[test]
    fn a_change_is_told_on_speaker_volume_and_the_socket_answers_each_call() {
        let dir = crate::test_root().join(format!("lamp-sim-volume-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let bus = Bus::new(dir.join("bus")).unwrap();
        let mut told = bus
            .subscribe_typed(SpeakerVolume::TOPIC, "volume_test", SpeakerVolume::TYPE)
            .unwrap();
        let socket = dir.join("act.sock");
        let (path, served) = (socket.clone(), bus.clone());
        thread::spawn(move || {
            let _ = serve_actions(&path, &served);
        });
        for _ in 0..100 {
            if socket.exists() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let call = |name: &str, args: Value| -> Value {
            let mut stream = UnixStream::connect(&socket).unwrap();
            writeln!(stream, "{}", json!({"name": name, "args": args})).unwrap();
            let mut line = String::new();
            BufReader::new(stream).read_line(&mut line).unwrap();
            serde_json::from_str(&line).unwrap()
        };
        let said = |told: &mut lamp_bus::Subscriber| -> Option<SpeakerVolume> {
            told.recv(Duration::from_millis(500))
                .unwrap()
                .map(|m| serde_json::from_slice(&m.payload).unwrap())
        };
        // Said once at the start (the middle), then after each change and only then.
        assert_eq!(
            said(&mut told),
            Some(SpeakerVolume {
                volume: 50,
                muted: false
            })
        );
        assert_eq!(
            call("volume_step", json!({"step": "down"}))["level"]["volume"],
            30
        );
        assert_eq!(
            said(&mut told),
            Some(SpeakerVolume {
                volume: 30,
                muted: false
            })
        );
        assert_eq!(
            call("volume_muted", json!({"muted": true}))["level"]["raw"],
            0
        );
        assert_eq!(
            said(&mut told),
            Some(SpeakerVolume {
                volume: 0,
                muted: true
            })
        );
        assert_eq!(call("volume_level", json!({}))["level"]["volume"], 0);
        assert_eq!(
            said(&mut told),
            None,
            "a read changes nothing and says nothing"
        );
        assert_eq!(
            call("volume_muted", json!({"muted": false}))["level"]["volume"],
            30
        );
        assert_eq!(
            said(&mut told),
            Some(SpeakerVolume {
                volume: 30,
                muted: false
            })
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn scratch_bus(name: &str) -> (std::path::PathBuf, Bus) {
        let dir = crate::test_root().join(format!("lsp-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bus = Bus::new(dir.join("bus")).unwrap();
        (dir, bus)
    }

    #[test]
    fn a_speaker_stream_is_opened_counted_told_in_stats_and_ends_when_the_door_closes() {
        let (dir, bus) = scratch_bus("stream");
        let mut publisher = bus
            .publisher_declared(
                Audio::SPEAKER_TOPIC,
                &Audio::speaker_declaration(),
                "control",
            )
            .unwrap();
        let (door, voice) = UnixStream::pair().unwrap();
        let served = bus.clone();
        let ended = thread::spawn(move || {
            let asked = json!({"skill": "control", "stream": {"by": "c_9"}, "expires_in_ms": 5000});
            speaker_stream(voice, &asked, &served, None).unwrap();
        });
        let mut lines = BufReader::new(door.try_clone().unwrap());
        let mut next = || -> Value {
            let mut line = String::new();
            lines.read_line(&mut line).unwrap();
            serde_json::from_str(&line).unwrap()
        };
        // The first line is the voice's yes, as lamp-control reads it.
        assert_eq!(next(), json!({"event": "stream", "state": "open"}));
        // Three 100 ms blocks of 24 kHz mono S16LE, and one that is not the speaker's rate (dropped, not counted).
        let block = |rate: u32, sequence: u64| {
            Audio {
                sample_rate: rate,
                channels: 1,
                sample_format: Audio::S16LE.to_owned(),
                sequence,
            }
            .block(&vec![0u8; 4_800])
            .unwrap()
        };
        for sequence in 0..3 {
            publisher
                .publish_bytes(
                    lamp_ipc::monotonic_us(),
                    Audio::ENCODING,
                    &block(24_000, sequence),
                )
                .unwrap();
        }
        publisher
            .publish_bytes(lamp_ipc::monotonic_us(), Audio::ENCODING, &block(16_000, 3))
            .unwrap();
        // The stats line says what was heard, and carries lamp-control's three counters.
        let stats = next();
        assert_eq!(stats["event"], "stats");
        assert_eq!(
            (
                stats["buffered_ms"].as_u64(),
                stats["underruns"].as_u64(),
                stats["limited"].as_u64()
            ),
            (Some(0), Some(0), Some(0))
        );
        assert_eq!(
            (stats["blocks"].as_u64(), stats["ms"].as_u64()),
            (Some(3), Some(300))
        );
        // The door closing its end ends the stream.
        drop(lines);
        drop(door);
        ended.join().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_speaker_topic_declared_as_another_type_is_refused_in_words() {
        let (dir, bus) = scratch_bus("refused");
        // Another program has declared the topic as something else: the
        // typed subscription is refused (an undeclared topic is not: it waits).
        let _other = bus
            .publisher_declared(
                Audio::SPEAKER_TOPIC,
                &lamp_msgs::Light::declaration(),
                "other",
            )
            .unwrap();
        let (door, voice) = UnixStream::pair().unwrap();
        speaker_stream(
            voice,
            &json!({"skill": "control", "stream": {"by": "c_9"}}),
            &bus,
            None,
        )
        .unwrap();
        let mut line = String::new();
        BufReader::new(door).read_line(&mut line).unwrap();
        let said: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(
            (said["event"].as_str(), said["state"].as_str()),
            (Some("stream"), Some("refused"))
        );
        assert!(
            said["why"]
                .as_str()
                .unwrap()
                .starts_with("the sim's speaker is not on the bus")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_player_that_never_reads_holds_neither_the_stats_nor_the_doors_closing_and_is_waited_for() {
        let (dir, bus) = scratch_bus("stalled");
        let mut publisher = bus
            .publisher_declared(
                Audio::SPEAKER_TOPIC,
                &Audio::speaker_declaration(),
                "control",
            )
            .unwrap();
        // `sleep` never reads its standard input: once the pipe is full a write to it blocks for good.
        let player = Command::new("sleep")
            .arg("60")
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        let pid = player.id();
        let (door, voice) = UnixStream::pair().unwrap();
        let served = bus.clone();
        let (done, finished) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let asked = json!({"skill": "control", "stream": {"by": "c_9"}, "expires_in_ms": 5000});
            speaker_stream(voice, &asked, &served, Some(player)).unwrap();
            let _ = done.send(());
        });
        let mut lines = BufReader::new(door.try_clone().unwrap());
        let mut next = || -> Value {
            let mut line = String::new();
            lines.read_line(&mut line).unwrap();
            serde_json::from_str(&line).unwrap()
        };
        assert_eq!(next(), json!({"event": "stream", "state": "open"}));
        // Far more sound than the pipe and the queue hold: 400 blocks of 100 ms (1.9 MB).
        let block = Audio {
            sample_rate: 24_000,
            channels: 1,
            sample_format: Audio::S16LE.to_owned(),
            sequence: 0,
        }
        .block(&vec![0u8; 4_800])
        .unwrap();
        for _ in 0..400 {
            publisher
                .publish_bytes(lamp_ipc::monotonic_us(), Audio::ENCODING, &block)
                .unwrap();
        }
        // Stats still come, again and again, though the player took none of it.
        let (first, second) = (next(), next());
        assert_eq!(
            (first["event"].as_str(), second["event"].as_str()),
            (Some("stats"), Some("stats"))
        );
        assert!(second["blocks"].as_u64().unwrap() > 0);
        // The door closing ends the stream, and the stalled player is killed and waited for.
        drop(lines);
        drop(door);
        finished
            .recv_timeout(Duration::from_secs(8))
            .expect("the stream did not end when the door closed: the player held it");
        assert!(
            !Path::new(&format!("/proc/{pid}")).exists(),
            "the stalled player was left behind (not waited for)"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_player_probed_before_a_refused_stream_is_ended_and_waited_for() {
        let (dir, bus) = scratch_bus("refused-player");
        let _other = bus
            .publisher_declared(
                Audio::SPEAKER_TOPIC,
                &lamp_msgs::Light::declaration(),
                "other",
            )
            .unwrap();
        let player = Command::new("sleep")
            .arg("60")
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        let pid = player.id();
        let (door, voice) = UnixStream::pair().unwrap();
        speaker_stream(
            voice,
            &json!({"skill": "control", "stream": {"by": "c_9"}}),
            &bus,
            Some(player),
        )
        .unwrap();
        let mut line = String::new();
        BufReader::new(door).read_line(&mut line).unwrap();
        assert!(line.contains("refused"), "{line}");
        assert!(
            !Path::new(&format!("/proc/{pid}")).exists(),
            "the probed player was left behind"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
