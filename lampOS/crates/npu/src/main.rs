//! `lamp-npu serve`: the lamp's NPU service (docs/npu.md). Off unless
//! `LAMP_NPU=on`: off, it idles until stopped and touches nothing (no
//! socket, no file, no NPU, no network).
//!
//! `lamp-npu pack`: the Store's side, the three files a model is served as,
//! signed with the Store's key (as `lamp-skills pack --key-file`).
//!
//! `lamp-npu pose`: asks a running service for one JPEG's pose, and
//! `lamp-npu status` for what it has: for a person at the lamp, through
//! the same socket lamp-control uses.
use lamp_npu::{
    BOARD, SOCKET, STATE, SWITCH,
    model::{self, Source, Store},
    on, serve,
    service::{Config, Service},
    vip::{self, Vip},
};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener, net::UnixStream},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

const USAGE: &str = "usage: lamp-npu serve [--socket PATH] [--state DIR] [--bus-root DIR] [--board TARGET]\n\
     \x20                     [--models-url https://HOST/PATH --models-host HOST] [--skills DIR] [--store-key FILE]\n\
     \x20                     [--apps DIR] [--library PATH] [--device PATH] [--idle-s N]\n\
     \x20      lamp-npu pack --key-file PRIVATE --model FILE --id ID --head HEAD --licence SPDX [--board TARGET] [--op pose] --out DIR\n\
     \x20      lamp-npu pose [--socket PATH] JPEG\n\
     \x20      lamp-npu status [--socket PATH]";

/// How often idle models are let go and installed apps looked at.
const TICK: Duration = Duration::from_secs(10);
/// How often the installed apps are looked at for one that uses `pose`.
const APPS_EVERY: Duration = Duration::from_secs(60);
/// How often the watchdog looks at the NPU's run under way.
const WATCH_EVERY: Duration = Duration::from_secs(1);
/// The exit status when the NPU's run hung (`EX_SOFTWARE`).
const HUNG_EXIT: i32 = 70;

fn main() {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_default();
    let rest: Vec<String> = args.collect();
    let result = match command.as_str() {
        "serve" => serve_command(&rest),
        "pack" => pack_command(&rest),
        "pose" | "status" => ask_command(&command, &rest),
        _ => Err(USAGE.to_owned()),
    };
    if let Err(why) = result {
        eprintln!("lamp-npu: {why}");
        std::process::exit(2);
    }
}

/// `--flag value` pairs, each flag at most once, from `allowed`; and the
/// words that are not flags.
fn flags(
    args: &[String],
    allowed: &[&str],
) -> Result<(std::collections::BTreeMap<String, String>, Vec<String>), String> {
    let mut found = std::collections::BTreeMap::new();
    let mut words = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if let Some(flag) = arg.strip_prefix("--") {
            if !allowed.contains(&flag) {
                return Err(format!("--{flag} is not a flag here\n{USAGE}"));
            }
            let value = args.next().ok_or(format!("--{flag} needs a value"))?;
            if found.insert(flag.to_owned(), value.clone()).is_some() {
                return Err(format!("--{flag} twice"));
            }
        } else {
            words.push(arg.clone());
        }
    }
    Ok((found, words))
}

fn serve_command(args: &[String]) -> Result<(), String> {
    let (flags, words) = flags(
        args,
        &[
            "socket",
            "state",
            "bus-root",
            "board",
            "models-url",
            "models-host",
            "skills",
            "store-key",
            "apps",
            "library",
            "device",
            "idle-s",
        ],
    )?;
    if !words.is_empty() {
        return Err(USAGE.to_owned());
    }
    if !on(std::env::var(SWITCH).ok().as_deref()) {
        eprintln!("[lamp-npu] {SWITCH} is not on: idle");
        loop {
            std::thread::park();
        }
    }
    let get = |name: &str, default: &str| -> PathBuf {
        flags.get(name).map_or(default.into(), PathBuf::from)
    };
    let socket = get("socket", SOCKET);
    let state = get("state", STATE);
    let bus_root = get("bus-root", lamp_bus::DEFAULT_ROOT);
    let skills = get("skills", "/var/lib/lampOS/skills");
    let store_key = get("store-key", lamp_skills::package::STORE_KEY);
    let apps_root = get("apps", "/opt/lampOS/apps");
    let board = flags.get("board").map_or(BOARD, String::as_str);
    let source = match (flags.get("models-url"), flags.get("models-host")) {
        (Some(url), Some(host)) => Some(Source::new(url, host)?),
        (None, None) => None,
        _ => return Err("--models-url and --models-host go together".to_owned()),
    };
    let mut config = Config::new(board, source.clone());
    if let Some(idle) = flags.get("idle-s") {
        config.idle = Duration::from_secs(
            idle.parse()
                .map_err(|_| "--idle-s: whole seconds".to_owned())?,
        );
    }
    let backend = Vip {
        library: get("library", vip::LIBRARY),
        device: get("device", vip::DEVICE),
    };
    let keys: lamp_npu::service::Keys =
        Box::new(move || lamp_skills::package::store_keys(&skills, &store_key));
    let channels: lamp_npu::service::Channels = Box::new(|| {
        lamp_update::fetch::Web::within(
            Duration::from_secs(10),
            Duration::from_secs(30),
            Duration::from_secs(300),
        )
        .map(|web| Box::new(web) as Box<dyn lamp_update::fetch::Channel + Send>)
        .map_err(|error| format!("no HTTPS client: {error}"))
    });
    let spawn: lamp_npu::service::Spawn = Box::new(|job| {
        std::thread::spawn(job);
    });
    let service = Service::new(
        config,
        Box::new(backend),
        Store::new(state.join("models")),
        keys,
        channels,
        spawn,
    );
    let _ = std::fs::remove_file(&socket);
    let listener =
        UnixListener::bind(&socket).map_err(|error| format!("{}: {error}", socket.display()))?;
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("{}: {error}", socket.display()))?;
    eprintln!(
        "[lamp-npu] serving {} (board {board}, models {})",
        socket.display(),
        source
            .as_ref()
            .map_or("not set", |source| source.base.as_str())
    );
    let ticking = service.clone();
    std::thread::spawn(move || {
        let mut apps_at: Option<Instant> = None;
        loop {
            let now = Instant::now();
            ticking.tick(now);
            // A newer model the Store offers, at most each 6 h.
            ticking.refresh(now);
            if apps_at.is_none_or(|at| now >= at + APPS_EVERY) {
                apps_at = Some(now);
                serve::apps_tick(&ticking, &apps_root, now);
            }
            std::thread::sleep(TICK);
        }
    });
    // The watchdog: a run of the NPU past its deadline has hung, holding
    // the net's lock and every ask behind it, and only a new process lets
    // the driver go. It exits, and systemd (`Restart=on-failure`) starts
    // the service again.
    let watched = service.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(WATCH_EVERY);
            if let Some(taken) = watched.hung(Instant::now()) {
                eprintln!(
                    "[lamp-npu] the NPU's run has taken {} ms, past its deadline: exiting for a restart",
                    taken.as_millis()
                );
                std::process::exit(HUNG_EXIT);
            }
        }
    });
    serve::serve(
        listener,
        service,
        Arc::new(move || serve::conditions(&bus_root)),
    );
    Ok(())
}

fn pack_command(args: &[String]) -> Result<(), String> {
    let (flags, words) = flags(
        args,
        &[
            "key-file", "model", "id", "head", "licence", "board", "op", "out",
        ],
    )?;
    let need = |name: &str| {
        flags
            .get(name)
            .cloned()
            .ok_or(format!("--{name} is needed\n{USAGE}"))
    };
    if !words.is_empty() {
        return Err(USAGE.to_owned());
    }
    let key = std::fs::read_to_string(need("key-file")?).map_err(|error| error.to_string())?;
    let path = PathBuf::from(need("model")?);
    let file = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("--model: a file's name")?;
    let op = flags.get("op").map_or("pose", String::as_str);
    let board = flags.get("board").map_or(BOARD, String::as_str);
    let manifest = model::manifest_for(
        name,
        &file,
        &need("id")?,
        op,
        &need("head")?,
        board,
        &need("licence")?,
    );
    manifest.check(op, board)?;
    let (manifest_bytes, signature) = model::pack(&key, &manifest)?;
    let out = PathBuf::from(need("out")?);
    std::fs::create_dir_all(&out).map_err(|error| error.to_string())?;
    for (file_name, bytes) in [
        (model::MANIFEST, manifest_bytes.as_slice()),
        (model::SIGNATURE, signature.as_slice()),
        (name, file.as_slice()),
    ] {
        std::fs::write(out.join(file_name), bytes).map_err(|error| error.to_string())?;
    }
    println!(
        "{}: {} {} bytes sha256 {}",
        out.display(),
        manifest.id,
        manifest.size,
        manifest.sha256
    );
    Ok(())
}

fn ask_command(command: &str, args: &[String]) -> Result<(), String> {
    let (flags, words) = flags(args, &["socket"])?;
    let socket = flags.get("socket").map_or(SOCKET.into(), PathBuf::from);
    let line = if command == "status" {
        serde_json::json!({"op": "status"})
    } else {
        let [jpeg] = words.as_slice() else {
            return Err(USAGE.to_owned());
        };
        let bytes = std::fs::read(jpeg).map_err(|error| format!("{jpeg}: {error}"))?;
        use base64::Engine;
        serde_json::json!({"op": "pose", "client": "lamp-npu",
            "jpeg": base64::engine::general_purpose::STANDARD.encode(bytes)})
    };
    let mut stream =
        UnixStream::connect(&socket).map_err(|error| format!("{}: {error}", socket.display()))?;
    writeln!(stream, "{line}").map_err(|error| error.to_string())?;
    let mut answer = String::new();
    BufReader::new(stream)
        .read_line(&mut answer)
        .map_err(|error| error.to_string())?;
    print!("{answer}");
    Ok(())
}
