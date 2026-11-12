//! The real `lamp-voice` on a developer's computer (`lamp-sim run DIR --real-voice --microphone`): the
//! voice's LIVE route with one Google (Gemini) key, from `~/.config/lampos/voice-keys.toml` or the
//! environment (`keys.rs`). This is the voice's Live route, not the route a customer lamp runs (every
//! product lamp answers through its second route); `--real-voice=second` starts that second route on the
//! developer's own DeepSeek, Deepgram and ElevenLabs-or-OpenAI keys ([`prepare_second`]).
//!
//! lamp-sim writes the voice's private files in `DIR/voice` (folder 0700, files 0600, gone with the
//! lamp's folder) and starts `lamp-voice serve` with the least argv that starts it. The key goes into
//! ONE file, `gemini.key` in a private folder (`key_home`: `$XDG_RUNTIME_DIR`'s memory-backed storage when there is one, else
//! `DIR/voice/keep`), removed when the run ends or fails; it is never in an argument, in the environment lamp-voice is given,
//! in `provider.json` (which names the file) or in anything lamp-sim prints. Without a key, one plain
//! line says so and nothing starts. Linux only: lamp-voice's capture and speaker workers are ALSA, and
//! its start asks `systemctl` whether the legacy services are idle (`coordinator.rs`
//! `refuse_legacy_owners`).
use crate::keys::Key;
use std::{
    ffi::{OsStr, OsString},
    fs,
    io::{self, Write},
    os::unix::{
        ffi::OsStrExt,
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::Command,
};

/// The flag that asks for it; off unless given (a key costs money and the microphone is used).
pub const FLAG: &str = "--real-voice";
/// The binary, beside `lamp-control` and `lamp-body`.
pub const PROGRAM: &str = "lamp-voice";
/// Google's own Live endpoint, as `lamp_gemini::GOOGLE_ENDPOINT` (`crates/gemini/src/config.rs`): the one
/// `config.rs`'s `google_credential_file` sends an API key to. The product's `voice-provider.json` names
/// its relay instead, whose key is never a developer's.
pub const ENDPOINT: &str = "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent";
/// The product's model name (`deploy/product/settings/voice-provider.json`). It may differ on Google's own
/// API; `LAMPOS_VOICE_GEMINI_MODEL` names another, and the first real run settles it.
pub const DEFAULT_MODEL: &str = "gemini-3.8-live";
/// The variable that names another model.
pub const MODEL_ENVIRONMENT: &str = "LAMPOS_VOICE_GEMINI_MODEL";
/// The ALSA device lamp-voice plays to (`--speaker-device`); the host's default unless this names another.
pub const SPEAKER_ENVIRONMENT: &str = "LAMP_SIM_VOICE_SPEAKER";
/// The names `lamp-voice` reads its own keys by (`second_config.rs`): each file in `CREDENTIALS_DIRECTORY`.
const OWN_DEEPSEEK_KEY: &str = "own_deepseek.key";
const OWN_DEEPSEEK_URL: &str = "own_deepseek.url";
const OWN_DEEPGRAM_KEY: &str = "own_deepgram.key";
/// The ONLY names of lamp-sim's own environment that reach the voice, besides what [`launch`] sets by hand: a login's
/// basics, the locale and the sound and session variables a capture and a speaker need. Everything else stays with
/// lamp-sim: a developer's `LAMP_*` switches (the product turns many on, a desktop run none: `docs`/K3), every
/// `LAMPOS_VOICE_*` key, and any other secret their shell holds (a `FOO_TOKEN`). If a real lamp-voice needs another
/// name, the first real run says so.
const PASSED_NAMES: [&str; 18] = [
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "LANG",
    "TZ",
    "XDG_RUNTIME_DIR",
    "DBUS_SESSION_BUS_ADDRESS",
    // Behind a proxy, or with a custom CA store, the voice cannot reach Google without these. A proxy URL may carry a
    // password: it is passed to lamp-voice only and never printed.
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "ALL_PROXY",
    "all_proxy",
    "NO_PROXY",
    "no_proxy",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
];
/// And the names that start with one of these.
const PASSED_PREFIXES: [&str; 4] = ["LC_", "PULSE_", "PIPEWIRE_", "ALSA_"];

/// `--real-voice=second`: the voice's SECOND route, the one a customer lamp runs, on the developer's own keys.
pub const FLAG_SECOND: &str = "--real-voice=second";

/// Whether `--real-voice` (the Live route) or `--real-voice=second` was given to `lamp-sim run`.
pub fn wanted(args: &[String]) -> bool {
    args.iter().any(|arg| arg == FLAG || arg == FLAG_SECOND)
}

/// Whether it is the second route that was asked for.
pub fn second_wanted(args: &[String]) -> bool {
    args.iter().any(|arg| arg == FLAG_SECOND)
}

/// Why the voice cannot be started with these flags and this bin folder, said before anything starts.
pub fn check(args: &[String], bin: &Path) -> Result<(), String> {
    if !cfg!(target_os = "linux") {
        return Err(format!(
            "{FLAG} is for Linux only: lamp-voice's capture and speaker are ALSA, and macOS has no lamp-voice"
        ));
    }
    if !args.iter().any(|arg| arg == "--microphone") {
        return Err(format!(
            "{FLAG} needs --microphone: the voice hears the room through the host's microphone on the lamp's bus"
        ));
    }
    if !bin.join(PROGRAM).is_file() {
        return Err(format!(
            "{FLAG} needs {PROGRAM} in {} (build it: cargo build --release -p lamp-voice)",
            bin.display()
        ));
    }
    Ok(())
}

/// The one line when no key is found: where to put it, and what runs instead.
pub fn no_key_line(file: Option<&Path>) -> String {
    format!(
        "no voice key: set main.gemini in {} (or LAMPOS_VOICE_GEMINI_KEY); using the stand-in voice",
        file.map_or_else(
            || "~/.config/lampos/voice-keys.toml".to_owned(),
            |path| path.display().to_string()
        )
    )
}

/// Where the key's one copy lives for the run: lamp-voice reads its credential only from a file, so a
/// private copy is unavoidable, but it is kept in memory-backed storage (`$XDG_RUNTIME_DIR`, a tmpfs the
/// user owns) when there is one, and removed when the voice or lamp-sim ends. Without one it is on the
/// disk, in `DIR/voice/keep`, and lamp-sim says so.
#[derive(Debug, PartialEq, Eq)]
pub struct KeyHome {
    pub path: PathBuf,
    /// In the runtime folder (memory), not under the lamp's folder.
    pub in_memory: bool,
}

/// The key's folder for the lamp in `dir`: `$XDG_RUNTIME_DIR/lamp-sim-voice-<name>-<hash>` when that
/// variable names an absolute folder that is the user's own and mode 0700; else `dir/voice/keep`.
pub fn key_home(dir: &Path, runtime: Option<&OsStr>) -> KeyHome {
    use std::hash::{Hash, Hasher};
    let usable = runtime
        .map(Path::new)
        .filter(|path| path.is_absolute())
        .filter(|path| {
            fs::metadata(path).is_ok_and(|info| {
                info.is_dir()
                    && info.uid() == rustix::process::geteuid().as_raw()
                    && info.mode() & 0o077 == 0
            })
        });
    match usable {
        Some(runtime) => {
            let name: String = dir
                .file_name()
                .map_or_else(|| "lamp".to_owned(), |n| n.to_string_lossy().into_owned())
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '-' {
                        c
                    } else {
                        '_'
                    }
                })
                .take(32)
                .collect();
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            dir.hash(&mut hasher);
            KeyHome {
                path: runtime.join(format!(
                    "lamp-sim-voice-{name}-{:08x}",
                    hasher.finish() as u32
                )),
                in_memory: true,
            }
        }
        None => KeyHome {
            path: dir.join("voice").join("keep"),
            in_memory: false,
        },
    }
}

/// The line said when the key's copy is on the disk.
pub fn on_disk_line(home: &KeyHome) -> String {
    format!(
        "the key's copy is on disk in {} until lamp-sim ends (no private runtime folder: XDG_RUNTIME_DIR)",
        home.path.display()
    )
}

/// Removes the key's copy (the folder and what it holds); absent is fine.
pub fn remove_key_home(path: &Path) {
    let _ = fs::remove_dir_all(path);
}

/// Removes the key's copy when it is dropped, on every way out (an early `?`, a panic, the end of the run): the copy
/// cannot outlive the code that made it. `prepare` holds one until it has returned a `Launch`, then hands the job to
/// the run (`disarm` returns the folder to keep); `run` holds the next.
pub struct KeyGuard(Option<PathBuf>);

impl KeyGuard {
    pub fn new(path: &Path) -> Self {
        Self(Some(path.to_owned()))
    }

    /// Stops it removing anything (`prepare` succeeded: the caller's own guard takes over).
    pub fn disarm(mut self) {
        self.0 = None;
    }
}

impl Drop for KeyGuard {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            remove_key_home(path);
        }
    }
}

/// One new file in `folder`, mode 0600 (never over an existing one).
fn write_private(folder: &Path, name: &str, text: &str) -> io::Result<PathBuf> {
    let path = folder.join(name);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    file.write_all(text.as_bytes())?;
    Ok(path)
}

/// What to run: the program, its arguments and the variables set (the rest of its environment is [`PASSED_NAMES`]).
#[derive(Debug, PartialEq, Eq)]
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub set: Vec<(String, String)>,
    pub session: PathBuf,
    /// The folder that holds the key's copy (and `provider.json`): removed when the voice ends.
    pub key_home: PathBuf,
}

/// An ALSA device name as `--speaker-device` takes one: 1 to 64 of letters, digits and `: , . _ = -`,
/// starting with a letter or a digit (`crates/voice/src/options.rs`).
pub fn speaker_name_ok(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ":,._=-".contains(c))
}

/// What a start is made of, besides the key: the lamp's folder, the folder of the binaries, the bus root,
/// the model and the speaker (see [`DEFAULT_MODEL`], [`SPEAKER_ENVIRONMENT`]), a unique number for the
/// session folder, and where the key's copy goes.
#[derive(Clone, Copy)]
pub struct Setup<'a> {
    pub dir: &'a Path,
    pub bin: &'a Path,
    pub bus_root: &'a Path,
    pub model: &'a str,
    pub speaker: &'a str,
    pub unix_ms: u128,
    pub home: &'a KeyHome,
}

/// The paths of a [`Setup`] made absolute (lamp-voice refuses a relative `--controls-mute`, `credential_file` or bus
/// root): the lamp's folder, the binaries, the bus root and the key's folder. Nothing is resolved through symbolic
/// links; a relative path is joined to the working directory.
fn absolute_paths(setup: &Setup) -> io::Result<(PathBuf, PathBuf, PathBuf, KeyHome)> {
    Ok((
        std::path::absolute(setup.dir)?,
        std::path::absolute(setup.bin)?,
        std::path::absolute(setup.bus_root)?,
        KeyHome {
            path: std::path::absolute(&setup.home.path)?,
            in_memory: setup.home.in_memory,
        },
    ))
}

/// A model name as `lamp_gemini` takes one: letters, digits, `.`, `_` and `-`, at most 128 (so not `models/...`).
fn model_name_ok(model: &str) -> bool {
    !model.is_empty()
        && model.len() <= 128
        && model
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

/// The voice's folder `DIR/voice`, its files, and what to start. `key` is written to `gemini.key` and
/// nowhere else; `model` and `speaker` are the choices (see [`DEFAULT_MODEL`], [`SPEAKER_ENVIRONMENT`]).
pub fn prepare(setup: &Setup, key: &Key) -> io::Result<Launch> {
    let (dir_abs, bin_abs, bus_abs, home_abs) = absolute_paths(setup)?;
    let setup = &Setup {
        dir: &dir_abs,
        bin: &bin_abs,
        bus_root: &bus_abs,
        home: &home_abs,
        ..*setup
    };
    let Setup {
        dir,
        model,
        speaker,
        home,
        ..
    } = *setup;
    if !speaker_name_ok(speaker) {
        return Err(io::Error::other(format!(
            "{SPEAKER_ENVIRONMENT} is not an ALSA device name (letters, digits and : , . _ = -, 1 to 64, starting with a letter or digit)"
        )));
    }
    if !model_name_ok(model) {
        return Err(io::Error::other(format!(
            "{MODEL_ENVIRONMENT} is not a model name (letters, digits, . _ - only: write gemini-3.8-live, not models/gemini-3.8-live)"
        )));
    }
    let voice = dir.join("voice");
    // A stale copy from a killed run goes first, then the folders are made private.
    remove_key_home(&home.path);
    let _ = fs::remove_dir_all(&voice);
    fs::DirBuilder::new().mode(0o700).create(&voice)?;
    fs::DirBuilder::new().mode(0o700).create(&home.path)?;
    // From here the copy exists: any early return below removes it again.
    let guard = KeyGuard::new(&home.path);
    let credential = write_private(&home.path, "gemini.key", key.expose())?;
    let provider = serde_json::json!({
        "endpoint": ENDPOINT,
        "model": model,
        "voice": "Kore",
        "thinking_budget": 0,
        "resumption": "off",
        "credential_file": credential,
        "authentication": "api_key",
    });
    let provider = write_private(&home.path, "provider.json", &provider.to_string())?;
    let language = write_private(&voice, "language", "en\n")?;
    guard.disarm();
    Ok(launch(setup, &provider, &language, Vec::new()))
}

/// What to start, whichever route: the same argv and the same fenced environment, with `extra` set besides (the second
/// route's switches). `provider` is the file `serve` is given first; `language` is the owner's language file.
fn launch(setup: &Setup, provider: &Path, language: &Path, extra: Vec<(String, String)>) -> Launch {
    let voice = setup.dir.join("voice");
    let session = voice.join(format!("session-{}", setup.unix_ms));
    let path = |name: &str| voice.join(name).display().to_string();
    let mut set = vec![
        ("LAMP_BUS".to_owned(), "on".to_owned()),
        (
            "LAMP_BUS_ROOT".to_owned(),
            setup.bus_root.display().to_string(),
        ),
        ("LAMP_PRIVACY_CONTROLS".to_owned(), "no-slide".to_owned()),
        (
            "LAMP_LANGUAGE_FILE".to_owned(),
            language.display().to_string(),
        ),
        ("LAMP_REGION_FILE".to_owned(), path("region")),
        ("LAMP_THREAD_FILE".to_owned(), path("thread.json")),
    ];
    set.extend(extra);
    Launch {
        program: setup.bin.join(PROGRAM),
        args: vec![
            "serve".to_owned(),
            provider.display().to_string(),
            session.display().to_string(),
            "--microphone".to_owned(),
            "mic1".to_owned(),
            "--microphone-device".to_owned(),
            "bus:mic1".to_owned(),
            "--speaker-device".to_owned(),
            setup.speaker.to_owned(),
            "--controls-stop".to_owned(),
            "--controls-mute".to_owned(),
            path("mute.json"),
        ],
        set,
        session,
        key_home: setup.home.path.clone(),
    }
}

/// What speaks on the second route: ElevenLabs or OpenAI, with the developer's key for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speech {
    ElevenLabs,
    OpenAi,
}

impl Speech {
    /// `tts_provider` and `LAMP_SECOND_SPEECH` as lamp-voice names it.
    fn name(self) -> &'static str {
        match self {
            Self::ElevenLabs => "elevenlabs",
            Self::OpenAi => "openai",
        }
    }

    /// The provider's own API, as `net::tts` joins its paths onto it (`{base}/text-to-speech/..`,
    /// `{base}/audio/speech`): with the `/v1`, so the request is the one the provider documents, the key in its
    /// own header (`xi-api-key`, a bearer) and nothing through the company's relay.
    fn base_url(self) -> &'static str {
        match self {
            Self::ElevenLabs => "https://api.elevenlabs.io/v1",
            Self::OpenAi => "https://api.openai.com/v1",
        }
    }
}

/// The developer's own keys for the second route: DeepSeek (text, with its endpoint when it is not DeepSeek's own),
/// Deepgram (speech to text) and the one that speaks.
pub struct SecondKeys<'a> {
    pub deepseek: &'a Key,
    pub deepseek_url: Option<&'a Key>,
    pub deepgram: &'a Key,
    pub speech: Speech,
    pub speech_key: &'a Key,
}

/// The second route's keys from what was found: all three services, or why not (said before anything starts).
pub fn second_keys(keys: &crate::keys::Keys) -> Result<SecondKeys<'_>, String> {
    let mut missing = Vec::new();
    if keys.deepseek.is_none() {
        missing.push("own.deepseek");
    }
    if keys.deepgram.is_none() {
        missing.push("own.deepgram");
    }
    let speech = match (&keys.elevenlabs, &keys.openai) {
        (Some(key), _) => Some((Speech::ElevenLabs, key)),
        (None, Some(key)) => Some((Speech::OpenAi, key)),
        (None, None) => {
            missing.push("own.elevenlabs or own.openai");
            None
        }
    };
    if let Some(url) = &keys.deepseek_url
        && !url.expose().trim().starts_with("https://")
    {
        return Err(
            "own.deepseek_url must start with https:// (your DeepSeek key is sent there; nothing is started)"
                .to_owned(),
        );
    }
    match (&keys.deepseek, &keys.deepgram, speech) {
        (Some(deepseek), Some(deepgram), Some((speech, speech_key))) => Ok(SecondKeys {
            deepseek,
            deepseek_url: keys.deepseek_url.as_ref(),
            deepgram,
            speech,
            speech_key,
        }),
        _ => Err(format!(
            "{FLAG_SECOND} needs {} in ~/.config/lampos/voice-keys.toml (or the LAMPOS_VOICE_* variables); `lamp-sim voice-keys` says which are set",
            missing.join(", ")
        )),
    }
}

/// The product's own second-route file. lamp-sim never names it: lamp-voice WRITES it when it is asked for exactly
/// there and nothing is, and a run on a developer's keys must never touch the product's place. Built from two
/// pieces so that no file of lamp-sim holds the whole name (the test below looks for it).
fn product_second_path() -> String {
    ["/etc/lampOS", "voice/second.json"].join("/")
}

/// The second route (`LAMP_VOICE_ROUTE=second`) on the developer's own keys: `DIR/voice` as for the Live route, and in
/// the key's folder (`CREDENTIALS_DIRECTORY` for the voice) one file per key, `second.json` (the route's own
/// configuration, which names the key files and holds no key) and a `provider.json` the voice reads first. Every
/// copy of a key is in that one folder, which a [`KeyGuard`] removes on any failure here, and the run's own guard and
/// the voice's watcher remove at the end.
pub fn prepare_second(setup: &Setup, keys: &SecondKeys) -> io::Result<Launch> {
    let (dir_abs, bin_abs, bus_abs, home_abs) = absolute_paths(setup)?;
    let setup = &Setup {
        dir: &dir_abs,
        bin: &bin_abs,
        bus_root: &bus_abs,
        home: &home_abs,
        ..*setup
    };
    let Setup {
        dir, speaker, home, ..
    } = *setup;
    if !speaker_name_ok(speaker) {
        return Err(io::Error::other(format!(
            "{SPEAKER_ENVIRONMENT} is not an ALSA device name (letters, digits and : , . _ = -, 1 to 64, starting with a letter or digit)"
        )));
    }
    // The file the voice is told to read, and the folder it takes its credentials from, are in the key's folder: neither may
    // be the product's file nor anywhere under the product's own folder, checked before anything is written.
    let config_path = home.path.join("second.json");
    let product = product_second_path();
    let product_folder = Path::new(&product)
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf);
    let under_product = |path: &Path| {
        path == Path::new(&product)
            || product_folder
                .as_deref()
                .is_some_and(|folder| path.starts_with(folder))
    };
    if under_product(&config_path) || under_product(&home.path) {
        return Err(io::Error::other(
            "the second route's configuration and credentials may not be the product's own",
        ));
    }
    let voice = dir.join("voice");
    remove_key_home(&home.path);
    let _ = fs::remove_dir_all(&voice);
    fs::DirBuilder::new().mode(0o700).create(&voice)?;
    fs::DirBuilder::new().mode(0o700).create(&home.path)?;
    let guard = KeyGuard::new(&home.path);
    let deepseek = write_private(&home.path, OWN_DEEPSEEK_KEY, keys.deepseek.expose())?;
    if let Some(url) = keys.deepseek_url {
        write_private(&home.path, OWN_DEEPSEEK_URL, url.expose())?;
    }
    let deepgram = write_private(&home.path, OWN_DEEPGRAM_KEY, keys.deepgram.expose())?;
    let speech = write_private(&home.path, "speech.key", keys.speech_key.expose())?;
    // The route's required fields name key files; the chat is DeepSeek's (LAMP_SECOND_CHAT=deepseek) with the
    // developer's own key over the relay's (LAMP_OWN_KEYS=on), so `llm_*` is DeepSeek's too and is never the company's.
    let deepseek_base = keys
        .deepseek_url
        .map_or("https://api.deepseek.com", |url| url.expose().trim());
    let second = serde_json::json!({
        "llm_base_url": deepseek_base,
        "llm_model": "deepseek-chat",
        "llm_credential_file": deepseek,
        "deepgram_credential_file": deepgram,
        "tts_provider": keys.speech.name(),
        "tts_base_url": keys.speech.base_url(),
        "tts_credential_file": speech,
    });
    let second = write_private(&home.path, "second.json", &second.to_string())?;
    // `serve`'s first argument: the Live route's file, which the second route still reads for the character. Its credential
    // is a file of its own holding a fixed word that is no key, never another service's key paired with Google's endpoint:
    // whatever path ever opened the Live route under the second route would send that word and nothing of the developer's.
    let placeholder = write_private(&home.path, "provider.key", "no-key-on-the-second-route")?;
    let provider = serde_json::json!({
        "endpoint": ENDPOINT,
        "model": DEFAULT_MODEL,
        "voice": "Kore",
        "thinking_budget": 0,
        "resumption": "off",
        "credential_file": placeholder,
        "authentication": "api_key",
    });
    let provider = write_private(&home.path, "provider.json", &provider.to_string())?;
    let language = write_private(&voice, "language", "en\n")?;
    guard.disarm();
    let extra = vec![
        ("LAMP_VOICE_ROUTE".to_owned(), "second".to_owned()),
        (
            "LAMP_VOICE_SECOND_CONFIG".to_owned(),
            second.display().to_string(),
        ),
        ("LAMP_OWN_KEYS".to_owned(), "on".to_owned()),
        ("LAMP_SECOND_CHAT".to_owned(), "deepseek".to_owned()),
        (
            "LAMP_SECOND_SPEECH".to_owned(),
            keys.speech.name().to_owned(),
        ),
        (
            "CREDENTIALS_DIRECTORY".to_owned(),
            home.path.display().to_string(),
        ),
    ];
    Ok(launch(setup, &provider, &language, extra))
}

/// The environment lamp-voice is given: what lamp-sim inherited (any bytes: a name or a value need not be
/// Unicode), only the names in [`PASSED_NAMES`] and [`PASSED_PREFIXES`], plus exactly `launch.set`. The caller clears
/// the command's own environment and gives it this (`env_clear`, `envs`), so nothing else leaks in.
pub fn environment(
    launch: &Launch,
    inherited: impl Iterator<Item = (OsString, OsString)>,
) -> Vec<(OsString, OsString)> {
    let mut out: Vec<(OsString, OsString)> = inherited
        .filter(|(name, _)| {
            (PASSED_NAMES
                .iter()
                .any(|kept| name.as_bytes() == kept.as_bytes())
                || PASSED_PREFIXES
                    .iter()
                    .any(|prefix| name.as_bytes().starts_with(prefix.as_bytes())))
                && !launch
                    .set
                    .iter()
                    .any(|(set, _)| name.as_bytes() == set.as_bytes())
        })
        .collect();
    out.extend(
        launch
            .set
            .iter()
            .map(|(name, value)| (OsString::from(name), OsString::from(value))),
    );
    out
}

/// The voice under a watcher shell, as lamp-sim's other parts are (it stops when `owner` is gone, even
/// killed), that also removes `clean` (the key's folder) once the voice has ended for any reason, and
/// gets exactly `environment` (the watcher's own `OWNER` and `CLEAN` besides). Its own process group.
pub fn watched_command(
    launch: &Launch,
    owner: u32,
    environment: Vec<(OsString, OsString)>,
) -> Command {
    let mut command = crate::keys::command("sh");
    command
        .args([
            "-c",
            "\"$@\" & F=$!; \
             while kill -0 \"$OWNER\" 2>/dev/null && kill -0 $F 2>/dev/null; do sleep 1; done; \
             kill $F 2>/dev/null; wait $F 2>/dev/null; rm -rf -- \"$CLEAN\"",
            "lamp-part",
        ])
        .arg(&launch.program)
        .args(&launch.args)
        .env_clear()
        .envs(environment)
        .env("OWNER", owner.to_string())
        .env("CLEAN", &launch.key_home)
        .process_group(0);
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{self, Origin};
    use std::os::unix::fs::PermissionsExt;

    const FAKE_KEY: &str = "test-key-not-real-for-the-voice";

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lsm-voice-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A stand-in for `$XDG_RUNTIME_DIR`: the user's own folder, mode 0700.
    fn runtime(name: &str) -> PathBuf {
        let dir = folder(name);
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        dir
    }

    fn a_key(value: &str) -> Key {
        let environment = {
            let value = value.to_owned();
            move |name: &str| (name == "LAMPOS_VOICE_GEMINI_KEY").then(|| value.clone().into())
        };
        keys::load(&environment, None).unwrap().gemini.unwrap()
    }

    /// `prepare` with the arguments in the order a test reads them.
    #[allow(clippy::too_many_arguments)]
    fn prep(
        dir: &Path,
        bin: &Path,
        bus_root: &Path,
        key: &Key,
        model: &str,
        speaker: &str,
        unix_ms: u128,
        home: &KeyHome,
    ) -> io::Result<Launch> {
        prepare(
            &Setup {
                dir,
                bin,
                bus_root,
                model,
                speaker,
                unix_ms,
                home,
            },
            key,
        )
    }

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_owned()).collect()
    }

    /// Every file under `root`, with its text where it is text.
    fn files_under(root: &Path) -> Vec<(PathBuf, String)> {
        let mut found = Vec::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(path) = pending.pop() {
            for entry in fs::read_dir(&path).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    let text = fs::read_to_string(&path).unwrap_or_default();
                    found.push((path, text));
                }
            }
        }
        found
    }

    #[test]
    fn the_flag_is_off_unless_exact_and_needs_the_microphone_and_the_binary() {
        assert!(!wanted(&args(&["lamp-sim", "run", "/d", "--microphone"])));
        assert!(wanted(&args(&["lamp-sim", "run", "/d", "--real-voice"])));
        assert!(!wanted(&args(&[
            "lamp-sim",
            "run",
            "/d",
            "--real-voice=on"
        ])));
        let bin = folder("bin");
        let without_mic = check(&args(&["run", "/d", "--real-voice"]), &bin).unwrap_err();
        assert!(without_mic.contains("needs --microphone"), "{without_mic}");
        let without_binary =
            check(&args(&["run", "/d", "--real-voice", "--microphone"]), &bin).unwrap_err();
        assert!(
            without_binary.contains("needs lamp-voice")
                && without_binary.contains("cargo build --release -p lamp-voice"),
            "{without_binary}"
        );
        fs::write(bin.join(PROGRAM), "#!/bin/sh\n").unwrap();
        assert!(check(&args(&["run", "/d", "--real-voice", "--microphone"]), &bin).is_ok());
        let _ = fs::remove_dir_all(&bin);
    }

    #[test]
    fn no_key_is_one_plain_line_that_names_the_file_and_the_variable() {
        let line = no_key_line(Some(Path::new("/h/.config/lampos/voice-keys.toml")));
        assert_eq!(
            line,
            "no voice key: set main.gemini in /h/.config/lampos/voice-keys.toml (or LAMPOS_VOICE_GEMINI_KEY); using the stand-in voice"
        );
        assert!(!line.contains('\n'));
    }

    #[test]
    fn the_keys_copy_goes_to_the_runtime_folder_when_it_is_the_users_own_and_private() {
        let dir = Path::new("/lamps/desk");
        let own = runtime("runtime-own");
        let home = key_home(dir, Some(own.as_os_str()));
        assert!(home.in_memory);
        assert!(home.path.starts_with(&own), "{}", home.path.display());
        assert!(
            home.path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("lamp-sim-voice-desk-"),
            "{}",
            home.path.display()
        );
        // The same lamp folder is the same place; another lamp folder of the same name is another.
        assert_eq!(key_home(dir, Some(own.as_os_str())), home);
        assert_ne!(
            key_home(Path::new("/other/desk"), Some(own.as_os_str())).path,
            home.path
        );
        // Not usable: unset, relative, missing, or open to others: the lamp's folder, and a line says so.
        let open = folder("runtime-open");
        fs::set_permissions(&open, fs::Permissions::from_mode(0o755)).unwrap();
        for bad in [
            None,
            Some(OsStr::new("relative")),
            Some(OsStr::new("/no/such/folder")),
            Some(open.as_os_str()),
        ] {
            let fallback = key_home(dir, bad);
            assert_eq!(
                (fallback.in_memory, &fallback.path),
                (false, &dir.join("voice").join("keep")),
                "{bad:?}"
            );
            assert!(
                on_disk_line(&fallback)
                    .contains("on disk in /lamps/desk/voice/keep until lamp-sim ends")
            );
        }
        let _ = (fs::remove_dir_all(&own), fs::remove_dir_all(&open));
    }

    #[test]
    fn a_failed_prepare_leaves_no_copy_of_the_key() {
        let dir = folder("late-failure");
        // The key's folder is where `prepare` later writes the language file, so that last write fails after the key and
        // `provider.json` are on disk.
        let home = KeyHome {
            path: dir.join("voice").join("language"),
            in_memory: false,
        };
        let key = a_key(FAKE_KEY);
        let result = prep(
            &dir,
            Path::new("/bin-dir"),
            Path::new("/lamp/bus"),
            &key,
            DEFAULT_MODEL,
            "default",
            1,
            &home,
        );
        assert!(result.is_err(), "the language file cannot be made");
        assert!(
            !home.path.exists(),
            "the key's folder is gone with the key in it"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_dropped_guard_removes_the_key_folder_and_a_disarmed_one_keeps_it() {
        let dir = folder("guard");
        let held = dir.join("held");
        fs::create_dir(&held).unwrap();
        fs::write(held.join("gemini.key"), "x").unwrap();
        drop(KeyGuard::new(&held));
        assert!(!held.exists());
        fs::create_dir(&held).unwrap();
        KeyGuard::new(&held).disarm();
        assert!(held.exists(), "disarmed: the caller's guard takes over");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_key_is_in_one_private_file_in_memory_storage_and_gone_when_it_is_removed() {
        let dir = folder("files");
        let run = runtime("files-runtime");
        let home = key_home(&dir, Some(run.as_os_str()));
        let key = a_key(FAKE_KEY);
        assert_eq!(key.origin(), Origin::Environment);
        let launch = prep(
            &dir,
            Path::new("/bin-dir"),
            Path::new("/lamp/bus"),
            &key,
            DEFAULT_MODEL,
            "default",
            1234,
            &home,
        )
        .unwrap();
        assert_eq!(launch.key_home, home.path);
        let voice = dir.join("voice");
        assert_eq!(fs::metadata(&home.path).unwrap().mode() & 0o777, 0o700);
        assert_eq!(fs::metadata(&voice).unwrap().mode() & 0o777, 0o700);
        for path in [
            home.path.join("gemini.key"),
            home.path.join("provider.json"),
            voice.join("language"),
        ] {
            assert_eq!(
                fs::metadata(&path).unwrap().mode() & 0o777,
                0o600,
                "{}",
                path.display()
            );
        }
        assert_eq!(
            fs::read_to_string(home.path.join("gemini.key")).unwrap(),
            FAKE_KEY
        );
        // Nothing under the lamp's folder holds the key; in the runtime folder only gemini.key does.
        for (path, text) in files_under(&dir) {
            assert!(!text.contains(FAKE_KEY), "the key is in {}", path.display());
        }
        for (path, text) in files_under(&run) {
            assert_eq!(
                text.contains(FAKE_KEY),
                path.file_name().unwrap() == "gemini.key",
                "{}",
                path.display()
            );
        }
        assert!(!format!("{launch:?}").contains(FAKE_KEY));
        // provider.json names the file and Google's own endpoint, with an API key.
        let provider: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(home.path.join("provider.json")).unwrap())
                .unwrap();
        assert_eq!(
            provider["credential_file"],
            home.path.join("gemini.key").display().to_string()
        );
        assert_eq!(
            (
                provider["endpoint"].as_str(),
                provider["authentication"].as_str(),
                provider["model"].as_str()
            ),
            (Some(ENDPOINT), Some("api_key"), Some(DEFAULT_MODEL))
        );
        // A stale copy from a killed run is replaced at the next start; removing the copy takes the folder.
        assert!(
            prep(
                &dir,
                Path::new("/bin-dir"),
                Path::new("/lamp/bus"),
                &a_key("test-key-not-real-second"),
                DEFAULT_MODEL,
                "default",
                5678,
                &home
            )
            .is_ok()
        );
        assert_eq!(
            fs::read_to_string(home.path.join("gemini.key")).unwrap(),
            "test-key-not-real-second"
        );
        remove_key_home(&launch.key_home);
        assert!(!home.path.exists());
        assert!(
            files_under(&run).is_empty(),
            "nothing of the key's is left in the runtime folder"
        );
        let _ = (fs::remove_dir_all(&dir), fs::remove_dir_all(&run));
    }

    #[test]
    fn the_argv_is_the_least_that_starts_and_the_environment_is_exactly_what_a_desktop_run_needs() {
        let dir = folder("argv");
        let run = runtime("argv-runtime");
        let home = key_home(&dir, Some(run.as_os_str()));
        let launch = prep(
            &dir,
            Path::new("/bin-dir"),
            Path::new("/lamp/bus"),
            &a_key(FAKE_KEY),
            "m-1",
            "plug:test_dev",
            42,
            &home,
        )
        .unwrap();
        let voice = dir.join("voice");
        let v = |name: &str| voice.join(name).display().to_string();
        assert_eq!(launch.program, Path::new("/bin-dir/lamp-voice"));
        assert_eq!(
            launch.args,
            [
                "serve".to_owned(),
                home.path.join("provider.json").display().to_string(),
                v("session-42"),
                "--microphone".to_owned(),
                "mic1".to_owned(),
                "--microphone-device".to_owned(),
                "bus:mic1".to_owned(),
                "--speaker-device".to_owned(),
                "plug:test_dev".to_owned(),
                "--controls-stop".to_owned(),
                "--controls-mute".to_owned(),
                v("mute.json"),
            ]
        );
        assert_eq!(
            launch.set,
            [
                ("LAMP_BUS".to_owned(), "on".to_owned()),
                ("LAMP_BUS_ROOT".to_owned(), "/lamp/bus".to_owned()),
                ("LAMP_PRIVACY_CONTROLS".to_owned(), "no-slide".to_owned()),
                ("LAMP_LANGUAGE_FILE".to_owned(), v("language")),
                ("LAMP_REGION_FILE".to_owned(), v("region")),
                ("LAMP_THREAD_FILE".to_owned(), v("thread.json")),
            ]
        );
        // Not given: the product's tuning (waits for evidence) and every feature switch of the K3 table.
        for left_out in [
            "--noise-suppression",
            "--turn-taking",
            "--endpoint",
            "--playout-start-ms",
            "--end-silence-ms",
            "--failure-notice-dir",
            "--pins",
        ] {
            assert!(!launch.args.iter().any(|a| a == left_out), "{left_out}");
        }
        let names: Vec<&str> = launch.set.iter().map(|(n, _)| n.as_str()).collect();
        for switch in [
            "LAMP_SAY",
            "LAMP_MAKE_SKILL",
            "LAMP_ONBOARDING",
            "LAMP_SAME_VOICE_FOR",
            "LAMP_SERVICES",
            "LAMP_APP_MAKE_SKILL",
            "LAMP_FIRST_OUTPUT",
            "LAMP_LIGHT_KEEP",
            "LAMP_STALLS",
            "LAMP_TASKS_TOLD",
            "LAMP_VOICE_ROUTE",
            "LAMP_VOICE_SECOND_CONFIG",
            "LAMP_PERSON_NAME",
            "LAMP_BUS_AUDIO",
        ] {
            assert!(!names.contains(&switch), "{switch} must not be set");
        }
        assert!(!launch.set.iter().any(|(_, value)| value.contains(FAKE_KEY)));
        // A speaker name that is not an ALSA name, or a model with a control character, is refused.
        for bad in ["", "-x", "a b", "a;b", &"a".repeat(65)] {
            assert!(
                prep(
                    &dir,
                    Path::new("/b"),
                    Path::new("/bus"),
                    &a_key(FAKE_KEY),
                    "m",
                    bad,
                    1,
                    &home
                )
                .is_err(),
                "{bad:?}"
            );
        }
        assert!(
            prep(
                &dir,
                Path::new("/b"),
                Path::new("/bus"),
                &a_key(FAKE_KEY),
                "m\n",
                "default",
                1,
                &home
            )
            .is_err()
        );
        let _ = (fs::remove_dir_all(&dir), fs::remove_dir_all(&run));
    }

    #[test]
    fn a_fake_voice_gets_that_argv_and_none_of_the_names_a_desktop_must_not_pass_and_its_key_copy_goes_when_it_ends()
     {
        use std::os::unix::ffi::OsStringExt;
        let _serial = crate::FAKE_PROGRAM
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = folder("fake");
        let run = runtime("fake-runtime");
        let home = key_home(&dir, Some(run.as_os_str()));
        let bin = dir.join("bin");
        fs::create_dir_all(&bin).unwrap();
        let record = dir.join("seen");
        let program = bin.join(PROGRAM);
        fs::write(&program, format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > {0}\nenv | sort >> {0}\ncat \"$LC_CLEAN_PROBE/gemini.key\" > {1} 2>&1\n", record.display(), dir.join("read-key").display())).unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        let launch = prep(
            &dir,
            &bin,
            Path::new("/lamp/bus"),
            &a_key(FAKE_KEY),
            DEFAULT_MODEL,
            "default",
            7,
            &home,
        )
        .unwrap();
        // What lamp-sim itself holds: switches the product sets, a route, an unrelated name, the key and the
        // model by their variables, and a value that is not Unicode (std::env::vars() would panic on it).
        let mut inherited: Vec<(OsString, OsString)> = [
            ("LAMP_SAY", "on"),
            ("LAMP_VOICE_ROUTE", "second"),
            ("LAMP_ONBOARDING", "on"),
            ("LAMP_BUS_AUDIO", "on"),
            ("LAMP_SIM_MIC", "x"),
            ("LAMPOS_VOICE_GEMINI_KEY", FAKE_KEY),
            ("LAMPOS_VOICE_GEMINI_MODEL", "m"),
            ("HOME", "/h"),
            ("PATH", "/usr/bin:/bin"),
            ("FOO_TOKEN", "test-not-real-other-secret"),
            ("HTTPS_PROXY", "http://proxy.test-not-real.invalid:3128"),
            ("SSL_CERT_FILE", "/etc/ssl/test-ca.pem"),
            ("XDG_RUNTIME_DIR", "/run/user/1"),
            ("PULSE_SERVER", "unix:/p"),
            ("LAMP_BUS", "off"),
            ("LC_CLEAN_PROBE", home.path.to_str().unwrap()),
        ]
        .iter()
        .map(|(n, v)| (OsString::from(n), OsString::from(v)))
        .collect();
        inherited.push((
            OsString::from("LC_ODD_VALUE"),
            OsString::from_vec(vec![b'o', 0xff, 0xfe, b'k']),
        ));
        inherited.push((
            OsString::from_vec(vec![b'L', b'C', b'_', b'O', 0xff]),
            OsString::from("name"),
        ));
        let given = environment(&launch, inherited.into_iter());
        assert!(
            given
                .iter()
                .any(|(n, v)| n == "LC_ODD_VALUE" && v.as_bytes() == [b'o', 0xff, 0xfe, b'k']),
            "a value that is not Unicode is passed through"
        );
        assert!(
            watched_command(&launch, std::process::id(), given)
                .status()
                .unwrap()
                .success()
        );
        let bytes = fs::read(&record).unwrap();
        assert!(
            bytes.windows(4).any(|w| w == [b'o', 0xff, 0xfe, b'k']),
            "the voice saw the value that is not Unicode, byte for byte"
        );
        let seen = String::from_utf8_lossy(&bytes).into_owned();
        assert!(
            !seen.contains(FAKE_KEY),
            "the key reached the voice's argv or environment:\n{seen}"
        );
        for gone in [
            "LAMP_SAY=",
            "LAMP_VOICE_ROUTE=",
            "LAMP_ONBOARDING=",
            "LAMP_BUS_AUDIO=",
            "LAMP_SIM_MIC=",
            "LAMPOS_VOICE_",
            "FOO_TOKEN",
            "test-not-real-other-secret",
        ] {
            assert!(!seen.contains(gone), "{gone} reached the voice:\n{seen}");
        }
        // The names that are not switches stay (the voice needs a PATH); a switch lamp-sim sets wins over an inherited one.
        for given in [
            "HOME=/h",
            "PATH=/usr/bin:/bin",
            "XDG_RUNTIME_DIR=/run/user/1",
            "PULSE_SERVER=unix:/p",
            "HTTPS_PROXY=http://proxy.test-not-real.invalid:3128",
            "SSL_CERT_FILE=/etc/ssl/test-ca.pem",
            "LAMP_BUS=on",
            "LAMP_BUS_ROOT=/lamp/bus",
            "LAMP_PRIVACY_CONTROLS=no-slide",
            "--microphone-device\nbus:mic1",
            "--controls-mute",
        ] {
            assert!(seen.contains(given), "{given} missing:\n{seen}");
        }
        assert!(!seen.contains("LAMP_BUS=off"));
        // The voice could read its credential while it ran, and when it ended its watcher removed the copy.
        assert_eq!(fs::read_to_string(dir.join("read-key")).unwrap(), FAKE_KEY);
        assert!(
            !home.path.exists(),
            "the key's copy is gone once the voice has ended"
        );
        let _ = (fs::remove_dir_all(&dir), fs::remove_dir_all(&run));
    }

    #[test]
    fn a_relative_lamp_folder_gives_only_absolute_paths() {
        // lamp-voice refuses a relative --controls-mute, credential_file or bus root.
        let relative = PathBuf::from(format!("lsm-relative-{}", std::process::id()));
        let _ = fs::remove_dir_all(&relative);
        fs::create_dir_all(&relative).unwrap();
        let home = key_home(&relative, None);
        assert!(
            home.path.is_relative(),
            "the fallback home follows the relative folder"
        );
        let launch = prep(
            &relative,
            Path::new("bin"),
            &relative.join("bus"),
            &a_key(FAKE_KEY),
            DEFAULT_MODEL,
            "default",
            1,
            &home,
        )
        .unwrap();
        let absolute = |what: &str, path: &str| {
            assert!(Path::new(path).is_absolute(), "{what} is relative: {path}")
        };
        absolute("the program", &launch.program.display().to_string());
        absolute("the provider file", &launch.args[1]);
        absolute("the session folder", &launch.args[2]);
        let mute = launch
            .args
            .iter()
            .position(|a| a == "--controls-mute")
            .unwrap();
        absolute("--controls-mute", &launch.args[mute + 1]);
        for (name, value) in &launch.set {
            if name.ends_with("_ROOT") || name.ends_with("_FILE") {
                absolute(name, value);
            }
        }
        absolute("key_home", &launch.key_home.display().to_string());
        let provider: serde_json::Value =
            serde_json::from_slice(&fs::read(Path::new(&launch.args[1])).unwrap()).unwrap();
        absolute(
            "credential_file",
            provider["credential_file"].as_str().unwrap(),
        );
        let _ = fs::remove_dir_all(&relative);
    }

    #[test]
    fn a_model_name_is_letters_digits_and_dots_and_models_slash_is_refused_in_words() {
        let dir = folder("model");
        let run = runtime("model-run");
        let home = key_home(&dir, Some(run.as_os_str()));
        let key = a_key(FAKE_KEY);
        for bad in ["models/gemini-3.8-live", "a b", "m\u{e9}", "", "x?y"] {
            let why = prep(
                &dir,
                Path::new("/b"),
                Path::new("/bus"),
                &key,
                bad,
                "default",
                1,
                &home,
            )
            .err()
            .unwrap()
            .to_string();
            assert!(
                why.contains("is not a model name") && why.contains("models/gemini-3.8-live"),
                "{bad}: {why}"
            );
            assert!(!home.path.exists(), "nothing was written for {bad}");
        }
        assert!(
            prep(
                &dir,
                Path::new("/b"),
                Path::new("/bus"),
                &key,
                "gemini-3.8-live_2.5",
                "default",
                1,
                &home
            )
            .is_ok()
        );
        let _ = (fs::remove_dir_all(&dir), fs::remove_dir_all(&run));
    }

    const OWN: [(&str, &str); 5] = [
        ("LAMPOS_VOICE_DEEPSEEK_KEY", "test-not-real-deepseek-key"),
        (
            "LAMPOS_VOICE_DEEPSEEK_URL",
            "https://deepseek.test-not-real.invalid/v1",
        ),
        ("LAMPOS_VOICE_DEEPGRAM_KEY", "test-not-real-deepgram-key"),
        (
            "LAMPOS_VOICE_ELEVENLABS_KEY",
            "test-not-real-elevenlabs-key",
        ),
        ("LAMPOS_VOICE_OPENAI_KEY", "test-not-real-openai-key"),
    ];

    /// The keys found with exactly these variables set (no file).
    fn keys_with(set: &[(&str, &str)]) -> keys::Keys {
        let given: Vec<(String, String)> = set
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        let environment = move |name: &str| {
            given
                .iter()
                .find(|(set, _)| set == name)
                .map(|(_, value)| OsString::from(value))
        };
        keys::load(&environment, None).unwrap()
    }

    fn prep_second(dir: &Path, home: &KeyHome, set: &[(&str, &str)]) -> io::Result<Launch> {
        let found = keys_with(set);
        let own = second_keys(&found).unwrap();
        prepare_second(
            &Setup {
                dir,
                bin: Path::new("/bin-dir"),
                bus_root: Path::new("/lamp/bus"),
                model: DEFAULT_MODEL,
                speaker: "default",
                unix_ms: 7,
                home,
            },
            &own,
        )
    }

    #[test]
    fn the_second_route_needs_the_three_services_and_says_which_are_missing() {
        let found = keys_with(&OWN[..1]);
        let why = second_keys(&found).err().unwrap();
        assert!(
            why.contains("own.deepgram, own.elevenlabs or own.openai"),
            "{why}"
        );
        assert!(!why.contains("own.deepseek,"), "{why}");
        for (_, value) in OWN {
            assert!(!why.contains(value), "no value in the words");
        }
        // ElevenLabs speaks when both are set; OpenAI when it is the only one.
        let both = keys_with(&OWN);
        assert_eq!(second_keys(&both).unwrap().speech, Speech::ElevenLabs);
        let openai = keys_with(&[OWN[0], OWN[2], OWN[4]]);
        assert_eq!(second_keys(&openai).unwrap().speech, Speech::OpenAi);
    }

    #[test]
    fn the_second_route_writes_each_key_once_in_one_private_folder_and_names_them_in_its_files() {
        let dir = folder("second");
        let run = runtime("second-run");
        let home = key_home(&dir, Some(run.as_os_str()));
        let launch = prep_second(&dir, &home, &OWN).unwrap();
        // One folder holds every copy, mode 0700, each file 0600.
        assert_eq!(fs::metadata(&home.path).unwrap().mode() & 0o777, 0o700);
        let mut names: Vec<String> = fs::read_dir(&home.path)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "own_deepgram.key",
                "own_deepseek.key",
                "own_deepseek.url",
                "provider.json",
                "provider.key",
                "second.json",
                "speech.key"
            ]
        );
        for name in &names {
            assert_eq!(
                fs::metadata(home.path.join(name)).unwrap().mode() & 0o777,
                0o600,
                "{name}"
            );
        }
        assert_eq!(
            fs::read_to_string(home.path.join("speech.key")).unwrap(),
            OWN[3].1,
            "ElevenLabs speaks"
        );
        // The route's own file names the key files and holds no key; the provider file neither.
        let second: serde_json::Value =
            serde_json::from_slice(&fs::read(home.path.join("second.json")).unwrap()).unwrap();
        assert_eq!(second["tts_provider"], "elevenlabs");
        assert_eq!(
            second["tts_base_url"], "https://api.elevenlabs.io/v1",
            "the provider's own address, with its /v1"
        );
        assert_eq!(
            second["llm_base_url"], OWN[1].1,
            "the developer's own DeepSeek endpoint"
        );
        assert_eq!(
            second["deepgram_credential_file"],
            home.path.join("own_deepgram.key").display().to_string()
        );
        for file in ["second.json", "provider.json"] {
            let text = fs::read_to_string(home.path.join(file)).unwrap();
            for (name, value) in OWN {
                if name != "LAMPOS_VOICE_DEEPSEEK_URL" {
                    assert!(!text.contains(value), "{file} holds a key");
                }
            }
        }
        // The environment names the second route and the credentials folder; neither it nor the argv holds a key.
        let set: std::collections::HashMap<_, _> = launch.set.iter().cloned().collect();
        assert_eq!(set["LAMP_VOICE_ROUTE"], "second");
        assert_eq!(
            set["LAMP_VOICE_SECOND_CONFIG"],
            home.path.join("second.json").display().to_string()
        );
        assert_eq!(set["LAMP_OWN_KEYS"], "on");
        assert_eq!(set["LAMP_SECOND_CHAT"], "deepseek");
        assert_eq!(set["LAMP_SECOND_SPEECH"], "elevenlabs");
        assert_eq!(
            set["CREDENTIALS_DIRECTORY"],
            home.path.display().to_string()
        );
        let everything = format!("{:?} {:?}", launch.args, launch.set);
        for (_, value) in OWN {
            assert!(
                !everything.contains(value),
                "a key is in the argv or the environment"
            );
        }
        let _ = (fs::remove_dir_all(&dir), fs::remove_dir_all(&run));
    }

    #[test]
    fn a_failed_second_prepare_leaves_no_copy_of_any_key() {
        let dir = folder("second-late");
        let home = KeyHome {
            path: dir.join("voice").join("language"),
            in_memory: false,
        };
        assert!(
            prep_second(&dir, &home, &OWN).is_err(),
            "the language file cannot be made"
        );
        assert!(!home.path.exists(), "every key's folder is gone");
        let _ = fs::remove_dir_all(&dir);
    }

    /// The DANGER TEST: lamp-voice WRITES the product's second.json (under /etc/lampOS/voice) when the second route is configured at exactly
    /// that path and it is absent (`second_config.rs` `place_product_if_missing`). lamp-sim never names it, in the
    /// files it writes, the argv and environment it builds, or its own source.
    #[test]
    fn lamp_sim_never_names_the_products_second_json() {
        let product = product_second_path();
        assert_eq!(
            product,
            ["/etc", "lampOS", "voice", "second.json"].join("/"),
            "the name the test looks for is lamp-voice's"
        );
        // The name is the one lamp-voice has as its `PRODUCT_PATH`.
        let voice_source = include_str!("../../voice/src/second_config.rs");
        assert!(
            voice_source.contains(&format!("pub const PRODUCT_PATH: &str = \"{product}\";")),
            "lamp-voice's PRODUCT_PATH is no longer the name this test guards"
        );
        let dir = folder("danger");
        let run = runtime("danger-run");
        let home = key_home(&dir, Some(run.as_os_str()));
        let launch = prep_second(&dir, &home, &OWN).unwrap();
        let built = format!("{:?} {:?}", launch.args, launch.set);
        assert!(
            !built.contains(&product),
            "the argv or the environment names the product's file"
        );
        for entry in fs::read_dir(&home.path).unwrap() {
            let text = fs::read_to_string(entry.unwrap().path()).unwrap();
            assert!(
                !text.contains(&product),
                "a written file names the product's file"
            );
        }
        // Where the second route is configured is inside the lamp's own folder, never the product's place.
        let set: std::collections::HashMap<_, _> = launch.set.iter().cloned().collect();
        assert_ne!(set["LAMP_VOICE_SECOND_CONFIG"], product);
        // And no source file of this crate holds the whole name (this test builds it from pieces).
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        for entry in fs::read_dir(&source).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|ext| ext == "rs") {
                let text = fs::read_to_string(&path).unwrap();
                assert!(
                    !text.contains(&product),
                    "{} names the product's second.json",
                    path.display()
                );
            }
        }
        let _ = (fs::remove_dir_all(&dir), fs::remove_dir_all(&run));
    }

    #[test]
    fn no_file_pairs_one_services_key_with_anothers_endpoint() {
        let dir = folder("pairing");
        let run = runtime("pairing-run");
        let home = key_home(&dir, Some(run.as_os_str()));
        prep_second(&dir, &home, &OWN).unwrap();
        let provider: serde_json::Value =
            serde_json::from_slice(&fs::read(home.path.join("provider.json")).unwrap()).unwrap();
        // Google's endpoint is paired with a file that holds no key of anyone's.
        assert_eq!(provider["endpoint"], ENDPOINT);
        let credential = provider["credential_file"].as_str().unwrap();
        for key_file in [OWN_DEEPSEEK_KEY, OWN_DEEPGRAM_KEY, "speech.key"] {
            assert!(
                !credential.ends_with(key_file),
                "provider.json names {key_file}"
            );
        }
        let held = fs::read_to_string(credential).unwrap();
        assert_eq!(held, "no-key-on-the-second-route");
        for (_, value) in OWN {
            assert!(!held.contains(value));
        }
        // And second.json pairs each key with its own service only: DeepSeek's file with DeepSeek's address, the speech key
        // with its provider's.
        let second: serde_json::Value =
            serde_json::from_slice(&fs::read(home.path.join("second.json")).unwrap()).unwrap();
        assert_eq!(
            second["llm_credential_file"],
            home.path.join(OWN_DEEPSEEK_KEY).display().to_string()
        );
        assert_eq!(
            second["tts_credential_file"],
            home.path.join("speech.key").display().to_string()
        );
        assert!(
            second["llm_base_url"]
                .as_str()
                .unwrap()
                .starts_with("https://deepseek.")
        );
        assert_eq!(second["tts_base_url"], "https://api.elevenlabs.io/v1");
        let _ = (fs::remove_dir_all(&dir), fs::remove_dir_all(&run));
    }

    #[test]
    fn the_second_routes_files_may_not_be_the_products() {
        let dir = folder("product-home");
        // A key folder under the product's own folder (where lamp-voice would place the product's second.json).
        let home = KeyHome {
            path: PathBuf::from(["/etc", "lampOS", "voice", "mine"].join("/")),
            in_memory: false,
        };
        let why = prep_second(&dir, &home, &OWN).err().unwrap().to_string();
        assert!(why.contains("may not be the product's own"), "{why}");
        assert!(
            !home.path.exists() && !dir.join("voice").exists(),
            "nothing was written"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_deepseek_address_that_is_not_https_is_refused_before_anything_is_written() {
        let plain = keys_with(&[
            OWN[0],
            (
                "LAMPOS_VOICE_DEEPSEEK_URL",
                "http://deepseek.test-not-real.invalid",
            ),
            OWN[2],
            OWN[3],
        ]);
        let why = second_keys(&plain).err().unwrap();
        assert!(why.contains("must start with https://"), "{why}");
        assert!(
            !why.contains("test-not-real"),
            "no value in the words: {why}"
        );
        assert!(second_keys(&keys_with(&OWN)).is_ok());
    }
}
