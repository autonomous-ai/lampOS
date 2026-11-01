//! Runs a command with the lamp's own relay credentials in its environment,
//! for the memory bench (`lamp-memory-bench`, PR #4753): the bench reads
//! `LAMP_BENCH_CHAT_KEY` (sent as `Bearer`) and `LAMP_BENCH_SPEECH_KEY` (sent
//! as `Token`), and on a lamp both are the relay's, which the voice reads
//! from its second route (`second.json`). "Use it through the relayer, just
//! like how lamp does it" (the owner, 22 Oct).
//!
//! ```sh
//! sudo -n ./bench_with_lamp_keys -- ./run-live.sh
//! ```
//!
//! The file is read as the voice reads it ([`load_from_path`], then the
//! voice unit's `LAMP_SECOND_*` from the environment when set), as root
//! because `second.json` and its credential files are root's. The chat key
//! is the route's chat credential (`llm_credential_file`, the one the voice
//! sends to DeepSeek through the relay); the speech key is its listening
//! credential through the relay (`stt_credential_file`, or the chat's). A
//! route that listens to Deepgram directly is refused: that key is not the
//! relay's. Either key empty is refused, and so is a missing command.
//!
//! The two variables are set only in the command's environment, never in
//! this process's own; the command replaces this process (`exec`), so its
//! exit status is the command's. Nothing here prints, logs or writes a key:
//! errors name the file or the field, never a value.
use lamp_voice::second_config::{Stt, load_from_path};
use std::{
    convert::Infallible,
    error::Error,
    os::unix::process::CommandExt,
    path::Path,
    process::{Command, ExitCode},
};

/// Where the voice unit's second route is set on a lamp.
const SECOND: &str = "/etc/lampOS/voice/second.json";
const CHAT_KEY: &str = "LAMP_BENCH_CHAT_KEY";
const SPEECH_KEY: &str = "LAMP_BENCH_SPEECH_KEY";

type Outcome<T> = Result<T, Box<dyn Error + Send + Sync>>;

fn main() -> ExitCode {
    // `run` returns only when the command could not be started.
    match run() {
        Ok(never) => match never {},
        Err(error) => {
            eprintln!("bench_with_lamp_keys: {error}; nothing was started");
            ExitCode::from(2)
        }
    }
}

fn run() -> Outcome<Infallible> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let Some(("--", command)) = arguments.split_first().map(|(a, rest)| (a.as_str(), rest)) else {
        return Err("usage: bench_with_lamp_keys -- COMMAND [ARGUMENTS...]".into());
    };
    let Some((program, program_arguments)) = command.split_first() else {
        return Err("usage: bench_with_lamp_keys -- COMMAND [ARGUMENTS...]".into());
    };
    let config = load_from_path(Path::new(SECOND))
        .and_then(|config| config.with_environment(|name: &str| std::env::var_os(name)))
        .map_err(|error| format!("{SECOND}: {error}"))?;
    let chat = config.llm_key.expose();
    let speech = match &config.stt {
        Stt::Autonomous { key, .. } => key.expose(),
        Stt::Deepgram { .. } => {
            return Err(
                "the route listens to Deepgram directly; that key is not the relay's".into(),
            );
        }
    };
    if chat.trim().is_empty() {
        return Err("the route's chat credential is empty".into());
    }
    if speech.trim().is_empty() {
        return Err("the route's listening credential is empty".into());
    }
    eprintln!(
        "bench_with_lamp_keys: {CHAT_KEY} (the route's chat credential) and {SPEECH_KEY} (its listening credential, through the relay) set for {program}"
    );
    let error = Command::new(program)
        .args(program_arguments)
        .env(CHAT_KEY, chat)
        .env(SPEECH_KEY, speech)
        .exec();
    Err(format!("{program} could not be started: {error}").into())
}
