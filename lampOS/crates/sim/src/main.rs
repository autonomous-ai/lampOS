//! `lamp-sim`: the virtual desktop lamp's hardware edge (docs/simulator-lamp.md).
//!
//! lamp-sim run DIR [--bin-dir BIN] [--listen ADDR] [--camera|--camera=always [--camera-gain N] [--camera-warp=off]] [--microphone] [--controls]
//!     Starts everything in DIR (short, private, outside a world-writable
//!     folder): the simulated servo bus, the real lamp-body on it, the ring,
//!     the voice and the volume, and the real lamp-control; prints a pairing
//!     code on demand with `lamp-sim code DIR`.
use lamp_sim::{
    camera::{Camera, Warp},
    mic,
    people::People,
    pty::Port,
    ring::Ring,
    servo::Bus,
    stimuli::Stimuli,
    voice,
};
use std::{
    os::unix::{fs::symlink, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command},
    thread,
    time::Duration,
};

/// Every grant, as lamp-control names them: the sim is a developer's own desk
/// lamp, so its pairing gives all of them unless `--grants` says fewer.
const ALL_GRANTS: &str =
    "body,light,speak,speech,look,hear,sound,skills,air,device,camera,microphone,notes";

/// A short id of the lamp's folder, so that two virtual lamps (or one made
/// again elsewhere) are two lamps to a computer's key store, not one.
fn lamp_name(dir: &Path) -> String {
    let full = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_owned());
    let hash = full
        .as_os_str()
        .as_encoded_bytes()
        .iter()
        .fold(0xcbf29ce484222325u64, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
        });
    format!("virtual-{:06x}", hash & 0xff_ffff)
}

const USAGE: &str = "usage: lamp-sim people N DIR | hear TEXT DIR | hand DIR | sound music|laughter|door|footsteps DIR | touch tap|pet DIR | air KEY=VALUE... DIR (a person in front of the lamp, by hand) | lamp-sim run DIR [--bin-dir BIN] [--listen ADDR] [--camera|--camera=always [--camera-gain N] [--camera-warp=off]] [--microphone] [--controls (the real lamp-controls beside the lamp: controls/state, no slide)] [--real-voice[=second|=local] (the real lamp-voice: the Live route with your Gemini key, =second its second route with your DeepSeek, Deepgram and ElevenLabs/OpenAI keys, or =local (macOS) the second route with your own LLM endpoint and lamp-sim speech and listen started beside it [--whisper URL] [--speech-listen ADDR] [--stt-listen ADDR]; needs --microphone)] | lamp-sim code DIR [--grants a,b,...] (default: every grant) | lamp-sim servo DIR | lamp-sim speech DIR [--listen 127.0.0.1:PORT] [--engine resident|say] [--language en-US] (a local OpenAI-compatible /audio/speech in the host's voice, pcm only, for the real voice's second route; its token in DIR/speech.token; on macOS the resident synthesizer unless --engine say) | lamp-sim listen DIR [--listen 127.0.0.1:PORT] [--whisper http://127.0.0.1:PORT] (a local speech-to-text socket in Deepgram's protocol, before your own whisper-server, for the real voice's second route; its token in DIR/listen.token) | lamp-sim voice-keys (which of your voice keys are set, from the file or the environment; never a value)";

fn option(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|k| args.get(k + 1))
        .cloned()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().skip(1).any(|a| a == "--help" || a == "-h") || args.len() == 1 {
        println!("{USAGE}");
        return;
    }
    // `voice-keys`: which of the developer's voice keys are set, and from where; no lamp folder, never a value.
    if args.get(1).map(String::as_str) == Some("voice-keys") && args.len() == 2 {
        let (text, code) = lamp_sim::keys::report(&|name| std::env::var_os(name));
        if code == 0 {
            print!("{text}");
        } else {
            eprint!("{text}");
        }
        std::process::exit(code);
    }
    if let Some(done) = stimulus(&args) {
        if let Err(error) = done {
            eprintln!("lamp-sim: {error}");
            std::process::exit(1);
        }
        return;
    }
    let dir = args.get(2).map(PathBuf::from);
    let result = match (args.get(1).map(String::as_str), dir) {
        (Some("servo"), Some(dir)) => servo(&dir),
        (Some("run"), Some(dir)) => run(&dir, &args),
        (Some("code"), Some(dir)) => code(&dir, &args),
        (Some("speech"), Some(dir)) => speech(&dir, &args),
        (Some("listen"), Some(dir)) => listen(&dir, &args),
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };
    if let Err(error) = result {
        eprintln!("lamp-sim: {error}");
        std::process::exit(1);
    }
}

/// A fresh token for a local service, written to `DIR/NAME` (0600, in a 0700
/// folder made if need be) for the route's credential file: 16 random bytes.
fn new_token(dir: &Path, name: &str) -> std::io::Result<(String, PathBuf)> {
    use std::io::Read;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    let mut random = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
    let token: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let file = dir.join(name);
    let _ = std::fs::remove_file(&file);
    std::io::Write::write_all(
        &mut std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&file)?,
        format!("{token}\n").as_bytes(),
    )?;
    Ok((token, file))
}

/// `listen DIR [--listen ADDR] [--whisper http://127.0.0.1:PORT]`: the local
/// speech-to-text socket (listen.rs) on loopback, before the developer's own
/// whisper-server, its token written to `DIR/listen.token` (0600). Runs until
/// stopped.
fn listen(dir: &Path, args: &[String]) -> std::io::Result<()> {
    let listen = option(args, "--listen").unwrap_or_else(|| "127.0.0.1:7733".to_owned());
    let address = listen
        .parse()
        .map_err(|_| std::io::Error::other(format!("--listen {listen}: not an address")))?;
    let whisper = option(args, "--whisper").unwrap_or_else(|| "http://127.0.0.1:7734".to_owned());
    let recognizer = lamp_sim::listen::Whisper::at(&whisper)?;
    let listener = lamp_sim::listen::bind(address)?;
    let (token, file) = new_token(dir, "listen.token")?;
    eprintln!(
        "lamp-sim listen: on ws://{} (/ws/audio/transcriptions, /v1/listen; linear16 16 kHz), whisper-server at {whisper}, token in {}",
        listener.local_addr()?,
        file.display()
    );
    lamp_sim::listen::serve(listener, token, std::sync::Arc::new(recognizer))
}

/// `speech DIR [--listen ADDR]`: the local text-to-speech service (speech.rs) on
/// loopback, its token written to `DIR/speech.token` (0600) for the route's
/// TTS credential file. Runs until stopped.
fn speech(dir: &Path, args: &[String]) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    let listen = option(args, "--listen").unwrap_or_else(|| "127.0.0.1:7732".to_owned());
    let address = listen
        .parse()
        .map_err(|_| std::io::Error::other(format!("--listen {listen}: not an address")))?;
    let listener = lamp_sim::speech::bind(address)?;
    // The token is written when the engine is ready (a reader waits for it): a stale one goes first.
    let file = dir.join("speech.token");
    let _ = std::fs::remove_file(&file);
    let folder = dir.join("speech");
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&folder)?;
    // A killed run leaves its ready say's file behind: only the engine's own
    // files (`say-<pid>-<n>.wav`) are cleared, never anything else a folder of
    // that name may hold (nine's P3 on #6954).
    for entry in std::fs::read_dir(&folder)?.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("say-") && name.ends_with(".wav") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    // macOS: the resident synthesizer (apple_speech.rs) unless `--engine say`;
    // it owns the main thread, so the HTTP side runs on another. Elsewhere, say.
    let engine = option(args, "--engine").unwrap_or_else(|| "resident".to_owned());
    if engine != "resident" && engine != "say" {
        return Err(std::io::Error::other(format!(
            "--engine {engine}: resident or say"
        )));
    }
    eprintln!(
        "lamp-sim speech: on http://{} (POST /audio/speech, pcm), token in {}, engine {}",
        listener.local_addr()?,
        file.display(),
        if cfg!(target_os = "macos") {
            engine.as_str()
        } else {
            "say"
        }
    );
    #[cfg(target_os = "macos")]
    if engine == "resident" {
        let language = option(args, "--language").unwrap_or_else(|| "en-US".to_owned());
        let (resident, lines) = lamp_sim::apple_speech::resident();
        let dir = dir.to_path_buf();
        let server = std::thread::Builder::new()
            .name("speech-http".to_owned())
            .spawn(move || {
                // The synthesizer's first line costs about 1.5 s (it loads the voice), every later one about 50 ms
                // (macOS 26, first byte over HTTP): paid here, on one line rendered to nobody, before the token says
                // the service is ready, not on the lamp's first answer.
                let warm = std::time::Instant::now();
                let warmed = lamp_sim::speech::Engine::speak(&resident, "Ready.", &mut |_| Ok(()));
                eprintln!(
                    "lamp-sim speech: the voice is warm after {} ms{}",
                    warm.elapsed().as_millis(),
                    warmed
                        .err()
                        .map_or_else(String::new, |error| format!(" ({error})"))
                );
                let (token, _) = new_token(&dir, "speech.token")?;
                lamp_sim::speech::serve(listener, token, std::sync::Arc::new(resident))
            })?;
        lamp_sim::apple_speech::serve_on_main_thread(lines, &language)?;
        return server
            .join()
            .unwrap_or_else(|_| Err(std::io::Error::other("the speech service ended")));
    }
    let say = lamp_sim::speech::Say::new(folder);
    let (token, _) = new_token(dir, "speech.token")?;
    lamp_sim::speech::serve(listener, token, std::sync::Arc::new(say))
}

/// `people N DIR`, `hear TEXT DIR`, `hand DIR`, `air KEY=VALUE... DIR`: what a
/// person in front of the lamp does, written as a file in the lamp's folder
/// for the running lamp to take. None if `args` is not one of these.
fn stimulus(args: &[String]) -> Option<std::io::Result<()>> {
    let (what, rest) = (args.get(1)?.as_str(), &args[2..]);
    let (file, text, dir) = match (what, rest) {
        ("people", [n, dir]) if n.parse::<u8>().is_ok() => ("people", n.clone(), dir),
        ("hear", [text, dir]) => ("hear", text.clone(), dir),
        ("sound", [class, dir]) if lamp_sim::stimuli::SOUND_CLASSES.contains(&class.as_str()) => {
            ("sound", class.clone(), dir)
        }
        ("touch", [kind, dir]) if kind == "tap" || kind == "pet" => ("touch", kind.clone(), dir),
        ("hand", [dir]) => ("hand", "wave".to_owned(), dir),
        ("air", [words @ .., dir]) if !words.is_empty() => ("air", words.join(" "), dir),
        _ => return None,
    };
    if what == "air" && lamp_sim::stimuli::air_of(&text).is_none() {
        return Some(Err(std::io::Error::other(
            "air takes key=value words: pm1_0 pm2_5 pm4_0 pm10 humidity temperature co2_ppm",
        )));
    }
    if what == "people" && text.trim() == "0" {
        eprintln!(
            "lamp-sim: nobody in view; person.arrived comes from `people 1` only after person.left, which needs 5 s of nobody (arrival itself wants 3 s since the last face; watch.rs)"
        );
    }
    Some(std::fs::write(Path::new(dir).join(file), text))
}

/// The servo bus alone, on a pty linked at `DIR/servo`.
fn servo(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut port = Port::open()?;
    let link = dir.join("servo");
    let _ = std::fs::remove_file(&link);
    symlink(&port.slave, &link)?;
    eprintln!(
        "lamp-sim: servo bus on {} ({})",
        link.display(),
        port.slave.display()
    );
    port.serve(&mut found_bus(&body_counts()?)?, &link, || false)
}

/// Servos found folded at the body counts' home (lamp-fc4b's, or lamp-75c9's): base
/// yaw, shoulder, elbow, head roll, head pitch.
fn found_bus(counts: &lamp_sim::servo::Counts) -> std::io::Result<Bus> {
    // The limits the servos store: all 0..=4095 unless `LAMP_SIM_SERVO_RANGES` says (README).
    let ranges =
        lamp_sim::servo::ranges_from(std::env::var("LAMP_SIM_SERVO_RANGES").ok().as_deref())
            .map_err(std::io::Error::other)?;
    Ok(Bus::new(counts.found, ranges))
}

/// The body's counts: lamp-fc4b's unless `LAMP_SIM_ARM=75c9` (README).
fn body_counts() -> std::io::Result<lamp_sim::servo::Counts> {
    lamp_sim::servo::counts_from(std::env::var("LAMP_SIM_ARM").ok().as_deref())
        .map_err(std::io::Error::other)
}

/// `1,2,3` for `--home` and the like.
fn counts_arg(counts: &[u16]) -> String {
    counts
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// Why `lamp-body` would refuse an ancestor of its lock folder, the rule of
/// `trusted_metadata` in crates/body/src/linux.rs, copied: any group or other
/// write bit (a sticky `/tmp` and a root-owned folder are NOT exempt), and an
/// owner that is neither root nor the user running it. (Its last rule, the lock
/// folder's own mode 0700, is lamp-sim's own `private_dir`.)
fn ancestor_trouble(mode: u32, owner: u32, uid: u32) -> Option<&'static str> {
    if owner != 0 && owner != uid {
        Some("owned by another user")
    } else if mode & 0o022 != 0 {
        Some("writable by group or others")
    } else {
        None
    }
}

/// The first folder above (or at) `path` that `lamp-body` would refuse, and why.
/// The body walks the literal path with no symlink followed (so a path with a
/// link in it fails there whatever it points to); this walks the real path, so
/// it never refuses a folder the body would take.
fn writable_ancestor(path: &Path) -> Option<(PathBuf, u32, &'static str)> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let uid = rustix::process::geteuid().as_raw();
    let full = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    full.ancestors().find_map(|folder| {
        let metadata = std::fs::metadata(folder).ok()?;
        let mode = metadata.permissions().mode() & 0o7777;
        ancestor_trouble(mode, metadata.uid(), uid).map(|why| (folder.to_owned(), mode, why))
    })
}

fn private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir.join("locks"))?;
    std::fs::create_dir_all(dir.join("state"))?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    std::fs::set_permissions(dir.join("locks"), std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn run(dir: &Path, args: &[String]) -> std::io::Result<()> {
    private_dir(dir)?;
    // One absolute folder for the whole lamp: the real voice's files and bus root must be absolute paths.
    let dir = &dir.canonicalize()?;
    // Say it here, not as five silent body restarts: the real lamp-body refuses
    // a lock folder under a folder others can write to.
    if let Some((folder, mode, why)) = writable_ancestor(dir) {
        return Err(std::io::Error::other(format!(
            "{} is {why} (mode {mode:o}): lamp-body refuses a lock folder under it. \
             Make the lamp folder under a private parent, e.g. `mkdir -m 700 ~/.lampsim` and `lamp-sim run ~/.lampsim/b`",
            folder.display()
        )));
    }
    let bin = option(args, "--bin-dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(Path::to_path_buf))
                .unwrap_or_default()
        });
    // `--controls` without the binary is said before anything starts.
    if lamp_sim::controls::wanted(args) {
        lamp_sim::controls::present(&bin).map_err(std::io::Error::other)?;
    }
    let listen = option(args, "--listen").unwrap_or_else(|| "127.0.0.1:7731".to_owned());
    // Say so here, not as a pairing failure later: another lamp-sim (or any
    // program) already has the door's address.
    if let Err(error) = std::net::TcpListener::bind(&listen) {
        return Err(std::io::Error::other(format!(
            "cannot listen on {listen}: {error} (another lamp-sim runs there? pick another with --listen 127.0.0.1:PORT)"
        )));
    }
    let bus_root = dir.join("bus");
    let _ = std::fs::remove_dir_all(&bus_root);
    // `--real-voice`: the real lamp-voice on the Live route with the developer's Gemini key (real_voice.rs). Said
    // before anything starts when it cannot be; with no key, one line and the stand-in voice as always. The key
    // is read into its file here and dropped (zeroed) at once.
    // `--real-voice=local`: the host's speech and listening, started before the voice and ended with the lamp.
    let mut local_services: Vec<(&str, Child)> = Vec::new();
    let real_voice = if lamp_sim::real_voice::wanted(args) {
        lamp_sim::real_voice::check(args, &bin).map_err(std::io::Error::other)?;
        let environment = |name: &str| std::env::var_os(name);
        let file = lamp_sim::keys::path_from(&environment);
        let keys =
            lamp_sim::keys::load(&environment, file.as_deref()).map_err(std::io::Error::other)?;
        // The Live route needs the Gemini key; the second route (`--real-voice=second`) needs the three own keys, and
        // the local one (`--real-voice=local`) the developer's LLM.
        let second = lamp_sim::real_voice::second_wanted(args);
        let second_keys = if second {
            Some(lamp_sim::real_voice::second_keys(&keys))
        } else {
            None
        };
        let local_keys = if lamp_sim::real_voice::local_wanted(args) {
            Some(lamp_sim::real_voice::local_keys(&keys))
        } else {
            None
        };
        let ready = match (&second_keys, &local_keys) {
            (Some(Ok(_)), _) | (_, Some(Ok(_))) => true,
            (Some(Err(line)), _) | (_, Some(Err(line))) => {
                eprintln!("lamp-sim: {line}; using the stand-in voice");
                false
            }
            (None, None) => keys.gemini.is_some(),
        };
        if ready {
            lamp_sim::controls::present(&bin).map_err(std::io::Error::other)?;
            let model = std::env::var(lamp_sim::real_voice::MODEL_ENVIRONMENT)
                .unwrap_or_else(|_| lamp_sim::real_voice::DEFAULT_MODEL.to_owned());
            let speaker = std::env::var(lamp_sim::real_voice::SPEAKER_ENVIRONMENT)
                .unwrap_or_else(|_| "default".to_owned());
            let unix_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_millis());
            // The keys' one folder: in memory-backed storage when there is any, and said when not.
            let home =
                lamp_sim::real_voice::key_home(dir, std::env::var_os("XDG_RUNTIME_DIR").as_deref());
            if !home.in_memory {
                eprintln!("lamp-sim: {}", lamp_sim::real_voice::on_disk_line(&home));
            }
            let setup = lamp_sim::real_voice::Setup {
                dir,
                bin: &bin,
                bus_root: &bus_root,
                model: &model,
                speaker: &speaker,
                unix_ms,
                home: &home,
            };
            Some(match (&second_keys, &local_keys, &keys.gemini) {
                (Some(Ok(own)), _, _) => lamp_sim::real_voice::prepare_second(&setup, own)?,
                (_, Some(Ok(llm)), _) => {
                    let started = start_local_services(dir, args)?;
                    let launch =
                        lamp_sim::real_voice::prepare_local(&setup, llm, &started.services())?;
                    local_services = started.children;
                    launch
                }
                (_, _, Some(key)) => lamp_sim::real_voice::prepare(&setup, key)?,
                _ => unreachable!("ready means a key set for the route"),
            })
        } else {
            if second_keys.is_none() && local_keys.is_none() {
                eprintln!(
                    "lamp-sim: {}",
                    lamp_sim::real_voice::no_key_line(file.as_deref())
                );
            }
            None
        }
    } else {
        None
    };
    // The key's copy goes on every way out of this function (an error below, a signal's end, the normal end).
    let _key_guard = real_voice
        .as_ref()
        .map(|launch| lamp_sim::real_voice::KeyGuard::new(&launch.key_home));
    // The servo bus, on its own thread.
    let mut port = Port::open()?;
    let link = dir.join("servo");
    let _ = std::fs::remove_file(&link);
    symlink(&port.slave, &link)?;
    let counts = body_counts()?;
    let mut servo_bus = found_bus(&counts)?;
    let servo_pose = servo_bus.pose();
    let mut threads = vec![spawn_named("servo bus", move || {
        let _ = port.serve(&mut servo_bus, &link, || false);
    })];
    // The voice and the volume. The bus first: the voice tells its lines on it.
    let bus = lamp_bus::Bus::new(&bus_root)?;
    let (say, act) = (dir.join("say.sock"), dir.join("act.sock"));
    {
        let say = say.clone();
        let voice_bus = bus.clone();
        threads.push(spawn_named("say socket", move || {
            if let Err(error) = voice::serve_say(&say, &voice_bus) {
                eprintln!("lamp-sim: the say socket ended: {error}");
            }
        }));
        let act = act.clone();
        let actions_bus = bus.clone();
        threads.push(spawn_named("volume socket", move || {
            if let Err(error) = voice::serve_actions(&act, &actions_bus) {
                eprintln!("lamp-sim: the volume socket ended: {error}");
            }
        }));
    }
    // Skill push: the real `lamp-skills asked` on a socket of ours, as systemd's
    // manage socket starts it on a lamp, if lamp-skills is beside the others;
    // without it skill push is off, as on a lamp that has no helper.
    let skills_bin = bin.join("lamp-skills");
    let manage_on = skills_bin.is_file();
    if manage_on {
        let (socket, log) = (dir.join("manage.sock"), dir.join("skills.log"));
        let folders = lamp_sim::manage::Folders::in_dir(dir);
        threads.push(spawn_named("skills manage socket", move || {
            if let Err(error) = lamp_sim::manage::serve(&socket, &skills_bin, folders, &log) {
                // lamp-control is still given the socket: without a word here,
                // a skill push would just fail (a long lamp folder path is the usual cause).
                eprintln!("lamp-sim: the skills manage socket ended: {error}");
            }
        }));
    } else {
        eprintln!("lamp-sim: no lamp-skills beside lamp-sim: skill.push is off");
    }
    // The ring, before lamp-control asks it anything.
    // One boot id for the ring, lamp-control and lamp-body (lib.rs `boot_id`).
    let clock_id = lamp_sim::boot_id();
    let boot_id = clock_id.clone();
    let mut ring = Ring::on(&bus)?;
    let mut people = People::on(&bus, dir)?;
    let mut stimuli = Stimuli::on(&bus, dir)?;
    let view = dir.join("ring.json");
    let sight = dir.join("sight.jpg");
    // The picture turns with the head (`--camera-warp=off` leaves it as it is);
    // `--camera-gain=N` makes the head's small turns N times bigger for the camera.
    let gain: f64 = option(args, "--camera-gain")
        .and_then(|g| g.parse().ok())
        .unwrap_or(1.0);
    let warp = (!args.iter().any(|a| a == "--camera-warp=off")).then(|| Warp {
        pose: servo_pose.clone(),
        gain,
    });
    let mut camera = Camera::new(&sight, warp);
    // The host's camera is off unless asked for. `--camera=always`: open for the
    // lamp's whole life, as the eyes' is (the host's camera light stays on).
    // `--camera`: open while a computer's camera mark is on the ring (the ring
    // renews it every 250 ms while it is held, and one seen in the last 1.5 s
    // counts), and `Camera`'s own hold keeps it open 10 s after the last such
    // moment: about 10 to 11.5 s after the mark is let go. So the host's camera
    // light means someone is looking. It needs lamp-control's
    // `--camera-opens-on-demand`, which is passed. Each picture still goes only
    // while the ring shows the camera's mark.
    let camera_always = args.iter().any(|a| a == "--camera=always");
    let camera_on_mark = !camera_always && args.iter().any(|a| a == "--camera");
    // The microphone the same way: only with --microphone. The ears' sound
    // still goes to a computer only while the ring shows its mark.
    let microphone_on = args.iter().any(|a| a == "--microphone");
    if microphone_on {
        let bus = lamp_bus::Bus::new(&bus_root)?;
        threads.push(spawn_named("microphone", move || {
            if let Err(error) = mic::run(&bus) {
                eprintln!("lamp-sim: the microphone ended: {error}");
            }
        }));
    }
    threads.push(spawn_named("ring", move || {
        let mut tick = 0u32;
        loop {
            if ring.step(&clock_id).is_err() {
                thread::sleep(Duration::from_millis(100));
            }
            let marked = ring.camera_marked_us != 0
                && lamp_ipc::monotonic_us().saturating_sub(ring.camera_marked_us) < 1_500_000;
            camera.step(camera_always || (camera_on_mark && marked));
            if tick.is_multiple_of(5) {
                let _ = people.say();
            }
            let _ = stimuli.step();
            tick += 1;
            // Whole or not at all, for a reader (a viewer) that looks at any time.
            let temp = view.with_extension("json.tmp");
            if std::fs::write(&temp, ring.view().to_string()).is_ok() {
                let _ = std::fs::rename(&temp, &view);
            }
            thread::sleep(Duration::from_millis(40));
        }
    }));
    // The real body, then the real door.
    let mut children: Vec<Child> = Vec::new();
    let d = |name: &str| dir.join(name).display().to_string();
    let mut body = body_command(&bin, dir, &bus_root)?.spawn()?;
    thread::sleep(Duration::from_secs(1));
    // `--controls`: the real lamp-controls, which publishes `controls/state` for a real voice (controls.rs);
    // off unless asked. It ends with the lamp, as the others do.
    let mut controls: Option<Child> = if lamp_sim::controls::wanted(args) || real_voice.is_some() {
        let launch =
            lamp_sim::controls::launch(&bin, &bus_root, &boot_id).map_err(std::io::Error::other)?;
        let mut command = watched(launch.program);
        command.args(&launch.args);
        for (name, value) in &launch.env {
            command.env(name, value);
        }
        Some(command.spawn()?)
    } else {
        None
    };
    children.push(
        watched(bin.join("lamp-control"))
            .env("LAMP_CONTROL", "on")
            .args([
                "serve",
                "--state-dir",
                &d("state"),
                "--listen",
                &listen,
                "--admin-socket",
                &d("a.sock"),
            ])
            .args([
                "--boot-id",
                &boot_id,
                "--name",
                &lamp_name(dir),
                "--bus-root",
                &bus_root.display().to_string(),
            ])
            .args([
                "--mood-socket",
                &d("mood.sock"),
                "--lease-socket",
                &d("lease.sock"),
            ])
            .args([
                "--say-socket",
                &d("say.sock"),
                "--actions-socket",
                &d("act.sock"),
            ])
            .args(["--sight-file", &d("sight.jpg")])
            .args(camera_on_mark.then_some("--camera-opens-on-demand"))
            .args(
                manage_on
                    .then(|| ["--manage-socket".to_owned(), d("manage.sock")])
                    .into_iter()
                    .flatten(),
            )
            .args(["--microphone", if microphone_on { "on" } else { "off" }])
            .env("LAMP_BUS_AUDIO", "on")
            .process_group(0)
            .spawn()?,
    );
    eprintln!(
        "lamp-sim: up on {listen}; pair with: lamp-sim code {}",
        dir.display()
    );
    // The real voice, last (it asks the bus and the door's parts): under the same watcher, with the environment
    // real_voice.rs names and none of the switches a desktop must not pass. If it ends, one line and the lamp stays up.
    let mut voice: Option<Child> = match &real_voice {
        Some(launch) => {
            let given = lamp_sim::real_voice::environment(launch, std::env::vars_os());
            let child =
                lamp_sim::real_voice::watched_command(launch, std::process::id(), given).spawn()?;
            let route = if lamp_sim::real_voice::local_wanted(args) {
                "the second route on your LLM and the host's speech and listening"
            } else if lamp_sim::real_voice::second_wanted(args) {
                "the second route on your own keys"
            } else {
                "the Live route, not a customer lamp's"
            };
            eprintln!(
                "lamp-sim: real voice started ({route}; session {})",
                launch.session.display()
            );
            Some(child)
        }
        None => None,
    };
    // The body is restarted when it ends (a busy laptop can make it miss a
    // servo reply and the real body then stops, as it should), with its why
    // said; more than five times in ten minutes, or the door or a part of lamp-sim
    // ending, ends the lamp.
    let mut restarts: Vec<std::time::Instant> = Vec::new();
    loop {
        thread::sleep(Duration::from_millis(500));
        let mut ended = false;
        if let Some(thread) = threads.iter().find(|t| t.is_finished()) {
            eprintln!(
                "lamp-sim: its {} thread ended (a panic, if nothing was said above)",
                thread.thread().name().unwrap_or("unnamed")
            );
            ended = true;
        }
        if let Ok(Some(status)) = body.try_wait() {
            eprintln!(
                "lamp-sim: lamp-body ended ({status}): {}",
                why_body_ended(dir)
            );
            restarts.retain(|at| at.elapsed() < Duration::from_secs(600));
            if restarts.len() >= 5 {
                eprintln!("lamp-sim: lamp-body ended five times in ten minutes: giving up");
                ended = true;
            } else {
                restarts.push(std::time::Instant::now());
                match body_command(&bin, dir, &bus_root).and_then(|mut c| c.spawn()) {
                    Ok(next) => {
                        body = next;
                        eprintln!("lamp-sim: lamp-body started again");
                    }
                    Err(error) => {
                        eprintln!("lamp-sim: lamp-body did not start again: {error}");
                        ended = true;
                    }
                }
            }
        }
        if children
            .iter_mut()
            .any(|c| matches!(c.try_wait(), Ok(Some(_))))
        {
            eprintln!("lamp-sim: lamp-control ended");
            ended = true;
        }
        // A local service that ends is said once; the voice then hears or speaks nothing more, and the lamp stays up.
        local_services.retain_mut(|(name, child)| match child.try_wait() {
            Ok(Some(status)) => {
                eprintln!("lamp-sim: lamp-sim {name} ended ({status}); the real voice cannot use it until the next run");
                false
            }
            _ => true,
        });
        if let Some(child) = voice.as_mut()
            && let Ok(Some(status)) = child.try_wait()
        {
            eprintln!(
                "lamp-sim: the real voice ended ({status}); the lamp stays up, its say and volume stand-ins still serve"
            );
            // Its watcher has removed the key's copy; this makes sure.
            if let Some(launch) = &real_voice {
                lamp_sim::real_voice::remove_key_home(&launch.key_home);
            }
            voice = None;
        }
        if let Some(child) = controls.as_mut()
            && matches!(child.try_wait(), Ok(Some(_)))
        {
            eprintln!("lamp-sim: lamp-controls ended");
            ended = true;
        }
        if ended {
            // The parts run in their own process groups under a watcher shell:
            // end each group, not only the shell.
            children.push(body);
            children.extend(controls);
            children.extend(voice);
            children.extend(local_services.drain(..).map(|(_, child)| child));
            if let Some(launch) = &real_voice {
                lamp_sim::real_voice::remove_key_home(&launch.key_home);
            }
            for child in &mut children {
                if let Some(group) = rustix::process::Pid::from_raw(child.id() as i32) {
                    let _ =
                        rustix::process::kill_process_group(group, rustix::process::Signal::TERM);
                }
                let _ = child.wait();
            }
            return Err(std::io::Error::other("the lamp ended"));
        }
    }
}

/// The real `lamp-body` on the simulated servo bus; its exit reports go to
/// `DIR/body.log`, one after another.
fn body_command(bin: &Path, dir: &Path, bus_root: &Path) -> std::io::Result<Command> {
    let counts = body_counts()?;
    let d = |name: &str| dir.join(name).display().to_string();
    let mut command = watched(bin.join("lamp-body"));
    command
        .env("LAMP_BUS", "on")
        .env("LAMP_BUS_ROOT", bus_root)
        .args([
            "--device",
            &d("servo"),
            "--unit-id",
            "lamp-sim",
            "--lock-directory",
            &d("locks"),
        ])
        .args([
            "--mood-socket",
            &d("mood.sock"),
            "--lease-socket",
            &d("lease.sock"),
        ])
        .args([
            "--reach",
            "150,15,1,250",
            "--speed",
            "300",
            "--servo-speed",
            "1000",
        ])
        .args([
            "--servo-acceleration",
            "60",
            "--stand-share",
            "100",
            "--stand-seconds",
            "6",
        ])
        .args([
            "--awake-seconds",
            "90",
            "--idle",
            "on",
            "--force-limit",
            "600",
        ])
        .args([
            "--home",
            &counts_arg(&counts.home),
            "--folded-arm",
            &counts_arg(&counts.folded_arm),
        ])
        .args([
            "--stand",
            &counts_arg(&counts.stand),
            "--pitch-window",
            "1740,2030",
            "--torque",
            "on",
        ]);
    // On a desktop lamp-control runs as the developer, not as `lampos` under
    // systemd, so lamp-body must be told to let this user hold the body (its
    // own opt-in, never passed by a product unit). A lamp-body from before that
    // option does not start: its reason is in body.log.
    command.args([
        "--lease-holder-uid",
        &rustix::process::getuid().as_raw().to_string(),
    ]);
    command
        .stdout(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("body.log"))?,
        )
        .process_group(0);
    Ok(command)
}

/// Why the body ended, from the exit report it printed (`body.stop`), else
/// the last line it wrote.
fn why_body_ended(dir: &Path) -> String {
    let text = std::fs::read_to_string(dir.join("body.log")).unwrap_or_default();
    why_in(&text)
}

/// The reason in a body log: the stop of the last exit report, else (a body that
/// refused to start prints its reason as the report's other fields or as plain
/// text) the last line of the log itself, cut short.
fn why_in(text: &str) -> String {
    let last = text.lines().rev().find(|line| !line.trim().is_empty());
    let stop = last.and_then(|line| {
        let report: serde_json::Value = serde_json::from_str(line).ok()?;
        report["body"]["stop"].as_str().map(str::to_owned)
    });
    match (stop, last) {
        (Some(stop), _) => stop,
        (None, Some(line)) => {
            let line = line.trim();
            let cut: String = line.chars().take(400).collect();
            if line.chars().count() > 400 {
                format!("{cut}...")
            } else {
                cut
            }
        }
        (None, None) => "no exit report".to_owned(),
    }
}

/// The host's speech and listening for `--real-voice=local`, as started: each one's address and token file, and the
/// processes (named) for the run to watch.
struct LocalStarted {
    speech_url: String,
    listen_url: String,
    speech_token: PathBuf,
    listen_token: PathBuf,
    children: Vec<(&'static str, Child)>,
}

impl LocalStarted {
    fn services(&self) -> lamp_sim::real_voice::LocalServices<'_> {
        lamp_sim::real_voice::LocalServices {
            speech_url: &self.speech_url,
            speech_token: &self.speech_token,
            listen_url: &self.listen_url,
            listen_token: &self.listen_token,
        }
    }
}

/// How long a local service may take to bind its address and write its token: `speech` starts a `say` or the
/// resident synthesizer first, which takes about 0.4 s.
const LOCAL_SERVICE_WAIT: Duration = Duration::from_secs(5);

/// `lamp-sim speech` and `lamp-sim listen` in `DIR/local`, each under the watcher that ends it with the lamp, on
/// `--speech-listen` and `--stt-listen` (127.0.0.1:7732 and :7733 unless given), `listen` before `--whisper`. Each is
/// ready when its token file is whole: it writes the token after its socket is bound, so lamp-voice's first request
/// then waits in the socket's queue, never on a refused connection. Said before the voice starts when an address is
/// taken or a service does not come up; a service already started is stopped then.
fn start_local_services(dir: &Path, args: &[String]) -> std::io::Result<LocalStarted> {
    use std::os::unix::fs::DirBuilderExt;
    let folder = dir.join("local");
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&folder)?;
    let speech_at = option(args, "--speech-listen").unwrap_or_else(|| "127.0.0.1:7732".to_owned());
    let listen_at = option(args, "--stt-listen").unwrap_or_else(|| "127.0.0.1:7733".to_owned());
    let whisper = option(args, "--whisper").unwrap_or_else(|| "http://127.0.0.1:7734".to_owned());
    for (flag, at) in [
        ("--speech-listen", &speech_at),
        ("--stt-listen", &listen_at),
    ] {
        let address: std::net::SocketAddr = at
            .parse()
            .map_err(|_| std::io::Error::other(format!("{flag} {at}: not an address")))?;
        if !address.ip().is_loopback() {
            return Err(std::io::Error::other(format!(
                "{flag} {at}: the host's speech and listening are on loopback only"
            )));
        }
        if let Err(error) = std::net::TcpListener::bind(address) {
            return Err(std::io::Error::other(format!(
                "cannot listen on {at}: {error} (pick another with {flag} 127.0.0.1:PORT)"
            )));
        }
    }
    let me = std::env::current_exe()?;
    let speech_token = folder.join("speech.token");
    let listen_token = folder.join("listen.token");
    let _ = std::fs::remove_file(&speech_token);
    let _ = std::fs::remove_file(&listen_token);
    let folder_arg = folder.display().to_string();
    let mut children: Vec<(&'static str, Child)> = Vec::new();
    let mut start = |name: &'static str, rest: &[&str]| -> std::io::Result<()> {
        let child = watched(me.clone())
            .args([name, &folder_arg])
            .args(rest)
            .process_group(0)
            .spawn()?;
        children.push((name, child));
        Ok(())
    };
    let started = start("speech", &["--listen", &speech_at])
        .and_then(|()| start("listen", &["--listen", &listen_at, "--whisper", &whisper]));
    let stop = |children: &mut Vec<(&'static str, Child)>| {
        for (_, child) in children.iter_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    };
    if let Err(error) = started {
        stop(&mut children);
        return Err(error);
    }
    // A token is 32 hex digits and a line end (`new_token`).
    let whole = |path: &Path| std::fs::metadata(path).is_ok_and(|info| info.len() == 33);
    let since = std::time::Instant::now();
    loop {
        if whole(&speech_token) && whole(&listen_token) {
            break;
        }
        if let Some(at) = children
            .iter_mut()
            .position(|(_, child)| matches!(child.try_wait(), Ok(Some(_))))
        {
            let name = children[at].0;
            stop(&mut children);
            return Err(std::io::Error::other(format!(
                "lamp-sim {name} ended before it was ready (its line is above)"
            )));
        }
        if since.elapsed() > LOCAL_SERVICE_WAIT {
            stop(&mut children);
            return Err(std::io::Error::other(format!(
                "the host's speech and listening were not ready in {} s",
                LOCAL_SERVICE_WAIT.as_secs()
            )));
        }
        thread::sleep(Duration::from_millis(20));
    }
    Ok(LocalStarted {
        speech_url: format!("http://{speech_at}"),
        listen_url: format!("http://{listen_at}"),
        speech_token,
        listen_token,
        children,
    })
}

/// A thread with a name, so that the lamp can say which one ended.
fn spawn_named(name: &str, work: impl FnOnce() + Send + 'static) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name(name.to_owned())
        .spawn(work)
        .expect("a thread")
}

/// A part of the lamp (`lamp-body`, `lamp-control`) under a shell that stops it
/// when lamp-sim ends, even killed: the parts never outlive their lamp.
fn watched(program: PathBuf) -> Command {
    let mut command = lamp_sim::keys::command("sh");
    command
        .args([
            "-c",
            "\"$@\" & F=$!; \
             while kill -0 \"$OWNER\" 2>/dev/null && kill -0 $F 2>/dev/null; do sleep 1; done; \
             kill $F 2>/dev/null; wait $F 2>/dev/null",
            "lamp-part",
        ])
        .arg(program)
        .env("OWNER", std::process::id().to_string());
    command
}

/// A pairing code, from the running lamp-control's admin socket.
fn code(dir: &Path, args: &[String]) -> std::io::Result<()> {
    let bin = option(args, "--bin-dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(Path::to_path_buf))
                .unwrap_or_default()
        });
    let grants = option(args, "--grants").unwrap_or_else(|| ALL_GRANTS.to_owned());
    let status = watched(bin.join("lamp-control"))
        .args([
            "code",
            "--grants",
            &grants,
            "--admin-socket",
            &dir.join("a.sock").display().to_string(),
        ])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(
            "lamp-control did not give a code (is `lamp-sim run` running for this DIR?)",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn each_folder_names_its_own_lamp() {
        let (a, b) = (Path::new("/tmp/lamp-sim-a"), Path::new("/tmp/lamp-sim-b"));
        assert_ne!(lamp_name(a), lamp_name(b));
        assert_eq!(lamp_name(a), lamp_name(a));
        assert!(lamp_name(a).starts_with("virtual-") && lamp_name(a).len() == 14);
    }

    #[test]
    fn a_folder_others_can_write_to_is_named_as_the_lock_folders_trouble() {
        let base = std::env::temp_dir().join(format!("lsm-anc-{}", std::process::id()));
        let inner = base.join("a").join("b");
        std::fs::create_dir_all(&inner).unwrap();
        for folder in [&base, &base.join("a"), &inner] {
            std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        // Private all the way up to the temp folder itself (usually 1777): the
        // first offender is above `base`, never `inner`.
        let found = writable_ancestor(&inner);
        assert!(
            found
                .as_ref()
                .is_none_or(|(folder, _, _)| !folder.starts_with(&base)),
            "{found:?}"
        );
        std::fs::set_permissions(base.join("a"), std::fs::Permissions::from_mode(0o775)).unwrap();
        let (folder, mode, why) = writable_ancestor(&inner).unwrap();
        assert_eq!(why, "writable by group or others");
        assert_eq!(folder, std::fs::canonicalize(base.join("a")).unwrap());
        assert_eq!(mode, 0o775);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The same table as lamp-body's `lock_directory_policy_rejects_shared_or_untrusted_ancestors`.
    #[test]
    fn the_ancestor_rule_is_lamp_bodys() {
        for mode in [0o777, 0o1777, 0o775, 0o702] {
            assert!(ancestor_trouble(mode, 0, 1000).is_some(), "{mode:o}");
        }
        assert!(ancestor_trouble(0o755, 1001, 1000).is_some());
        assert!(ancestor_trouble(0o755, 0, 1000).is_none());
        assert!(ancestor_trouble(0o700, 1000, 1000).is_none());
        // The final folder's mode is checked by lamp-body alone, on a folder lamp-sim made 0700.
        assert!(ancestor_trouble(0o750, 1000, 1000).is_none());
    }

    #[test]
    fn the_body_log_gives_the_stop_else_its_last_line() {
        assert_eq!(why_in(""), "no exit report");
        assert_eq!(
            why_in("{\"body\":{\"stop\":\"a hand held it\"}}\n"),
            "a hand held it"
        );
        // A body that refused to start: a report with no stop, or plain text.
        let refused = "{\"error\":\"lock directory ancestor is group/world writable\"}\n";
        assert_eq!(why_in(refused), refused.trim());
        assert_eq!(
            why_in("first\nlamp-body: bad flag\n\n"),
            "lamp-body: bad flag"
        );
        let long = "x".repeat(500);
        assert!(why_in(&long).ends_with("...") && why_in(&long).len() < 410);
    }
}
