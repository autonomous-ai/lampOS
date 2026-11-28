//! `lamp-phonon serve`: Phonon, the voice that runs on the lamp
//! (docs/phonon.md, #8754). Not chosen (no `own_phonon.voice` credential),
//! it idles until stopped and touches nothing: no socket, no file, no
//! network. Chosen, it fetches the model once if it isn't kept, checks
//! every file against its pin, loads it, says one unheard warm-up word and
//! serves the voice's socket.
//!
//! `lamp-phonon bench`: on a lamp with the model kept, the time to first
//! audio, the real-time factor and the peak memory of three sentences, for
//! a person measuring the lamp (nothing is played or kept).
use lamp_phonon::{
    STATE, THREADS_ENVIRONMENT, chosen, cores,
    engine::{Phonon, Speaker},
    fetch, serve, threads,
};
use std::{
    os::unix::{fs::PermissionsExt, net::UnixListener},
    path::PathBuf,
    sync::Arc,
    time::Instant,
};

const USAGE: &str = "usage: lamp-phonon serve [--socket PATH] [--state DIR]\n\
     \x20      lamp-phonon bench [--state DIR] [--voice NAME] [--pin big|all]\n\
     LAMP_PHONON_THREADS (1-4, default 1), LAMP_PHONON_PIN (big, the default, or all)";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| {
        args.iter()
            .position(|arg| arg == name)
            .and_then(|at| args.get(at + 1))
            .cloned()
    };
    let state = PathBuf::from(flag("--state").unwrap_or_else(|| STATE.to_owned()));
    let threads = threads(std::env::var(THREADS_ENVIRONMENT).ok().as_deref());
    // The fastest cores, before any thread starts: every thread started
    // later (the model's) stays on them.
    let pin = flag("--pin").or_else(|| std::env::var(cores::PIN_ENVIRONMENT).ok());
    let pinned = if cores::pins(pin.as_deref()) {
        cores::pin_big()
    } else {
        "on every core (LAMP_PHONON_PIN=all)".to_owned()
    };
    eprintln!("lamp-phonon: {pinned}, {threads} tensor thread(s)");
    let result = match args.first().map(String::as_str) {
        Some("serve") => serve_chosen(
            flag("--socket").unwrap_or_else(|| lamp_msgs::phonon::SOCKET.to_owned()),
            &state,
            threads,
        ),
        Some("bench") => bench(&state, threads, flag("--voice").as_deref()),
        _ => Err(USAGE.to_owned()),
    };
    if let Err(why) = result {
        eprintln!("lamp-phonon: {why}");
        std::process::exit(1);
    }
}

/// The person's choice, from the unit's credentials.
fn chosen_voice() -> Option<lamp_msgs::phonon::Voice> {
    let folder = std::env::var_os("CREDENTIALS_DIRECTORY")?;
    let text =
        std::fs::read_to_string(PathBuf::from(folder).join(lamp_msgs::phonon::CHOSEN_CREDENTIAL))
            .ok();
    chosen(text.as_deref())
}

fn serve_chosen(socket: String, state: &std::path::Path, threads: usize) -> Result<(), String> {
    let Some(voice) = chosen_voice() else {
        eprintln!("lamp-phonon: not chosen; idle (no socket, no file, no network)");
        loop {
            std::thread::park();
        }
    };
    let mut web = lamp_update::fetch::Web::new().map_err(|e| e.to_string())?;
    let started = Instant::now();
    let ensured = fetch::ensure(state, &mut web)?;
    eprintln!(
        "lamp-phonon: the model kept ({} fetched, {} checked) in {} ms",
        ensured.fetched,
        ensured.kept,
        started.elapsed().as_millis()
    );
    let started = Instant::now();
    let phonon = Phonon::load(&fetch::folder(state), threads)?;
    phonon.speak("Ready.", voice.name, &mut |_| true)?;
    eprintln!(
        "lamp-phonon: loaded and warm in {} ms, {threads} threads, {} voices, chosen {}",
        started.elapsed().as_millis(),
        phonon.voices().len(),
        voice.name
    );
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).map_err(|e| format!("{socket}: {e}"))?;
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("{socket}: {e}"))?;
    serve::serve(listener, Arc::new(phonon)).map_err(|e| e.to_string())
}

/// Three sentences, as the first builder's bars are measured (#8754).
const BENCH: [&str; 3] = [
    "Sure, I can help with that.",
    "The meeting moved to half past three, so you still have about forty minutes to finish the draft.",
    "It's a little dark in here; do you want me to turn the light up a bit?",
];

fn bench(state: &std::path::Path, threads: usize, voice: Option<&str>) -> Result<(), String> {
    let voice = voice
        .and_then(lamp_msgs::phonon::voice)
        .unwrap_or(lamp_msgs::phonon::VOICES[0]);
    let started = Instant::now();
    let phonon = Phonon::load(&fetch::folder(state), threads)?;
    println!(
        "load {} ms, {threads} threads, voice {}",
        started.elapsed().as_millis(),
        voice.name
    );
    phonon.speak("Ready.", voice.name, &mut |_| true)?;
    for text in BENCH {
        let started = Instant::now();
        let mut first = None;
        let mut samples = 0;
        phonon.speak(text, voice.name, &mut |pcm| {
            first.get_or_insert_with(|| started.elapsed());
            samples += pcm.len();
            true
        })?;
        let made = started.elapsed().as_secs_f64();
        let audio = samples as f64 / f64::from(lamp_msgs::phonon::SAMPLE_RATE);
        println!(
            "first audio {} ms, {:.0} ms of audio in {:.0} ms, real-time factor {:.3}",
            first.unwrap_or_default().as_millis(),
            audio * 1000.0,
            made * 1000.0,
            made / audio.max(f64::EPSILON)
        );
    }
    if let Ok(status) = std::fs::read_to_string("/proc/self/status")
        && let Some(peak) = status.lines().find(|line| line.starts_with("VmHWM:"))
    {
        println!("peak memory {}", peak.trim_start_matches("VmHWM:").trim());
    }
    Ok(())
}
