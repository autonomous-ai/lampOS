//! `lamp-control`: the lamp's local programming door (docs/control.md, #3504).
//!
//! ```text
//! lamp-control serve [--state-dir DIR] [--listen ADDR] [--admin-socket PATH] [--name NAME]
//!                    [--mood-socket PATH] [--say-socket PATH] [--bus-root DIR] [--boot-id ID]
//! lamp-control code --grants body,light,speak [--admin-socket PATH]
//! lamp-control revoke CLIENT [--admin-socket PATH]
//! lamp-control clients [--admin-socket PATH]
//! ```
//!
//! `serve` runs only with `LAMP_CONTROL=on` in its environment; otherwise it
//! says so and exits 0. It keeps its certificate and paired computers in
//! `--state-dir` (`/var/lib/lampos-control`), listens on `--listen`
//! (`0.0.0.0:7731`) and answers the owner's side on `--admin-socket`
//! (`/run/lampos-control/admin.sock`, 0600). Actions go to the body's
//! `--mood-socket`, the voice's `--say-socket` and the ring's light requests
//! on the bus at `--bus-root` (docs/control.md, "Routing"); a part not given
//! is refused with a why.

use std::path::PathBuf;

use lamp_control::{cert, pairing::Clients, parts, server, session};
use lamp_msgs::LightRequest;
use std::sync::{Arc, Mutex};

const ENVIRONMENT: &str = "LAMP_CONTROL";
const STATE_DIR: &str = "/var/lib/lampos-control";
const LISTEN: &str = "0.0.0.0:7731";
const ADMIN: &str = "/run/lampos-control/admin.sock";

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let code = match run(&arguments) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("lamp-control: {error}");
            2
        }
    };
    std::process::exit(code);
}

fn option(arguments: &[String], name: &str) -> Option<String> {
    arguments
        .iter()
        .position(|a| a == name)
        .and_then(|at| arguments.get(at + 1))
        .cloned()
}

fn run(arguments: &[String]) -> Result<(), String> {
    let admin =
        PathBuf::from(option(arguments, "--admin-socket").unwrap_or_else(|| ADMIN.to_owned()));
    match arguments.first().map(String::as_str) {
        Some("serve") => serve(arguments, admin),
        Some("code") => {
            let grants: Vec<String> = option(arguments, "--grants")
                .unwrap_or_default()
                .split(',')
                .filter(|grant| !grant.is_empty())
                .map(str::to_owned)
                .collect();
            ask(&admin, &serde_json::json!({"code": {"grants": grants}}))
        }
        Some("revoke") => {
            let client = arguments.get(1).ok_or("revoke CLIENT")?;
            ask(&admin, &serde_json::json!({"revoke": client}))
        }
        Some("clients") => ask(&admin, &serde_json::json!({"clients": true})),
        _ => Err("serve, code, revoke or clients (see docs/control.md)".to_owned()),
    }
}

fn ask(admin: &PathBuf, asked: &serde_json::Value) -> Result<(), String> {
    use std::io::{BufRead as _, Write as _};
    let mut stream = std::os::unix::net::UnixStream::connect(admin)
        .map_err(|e| format!("{}: {e}", admin.display()))?;
    writeln!(stream, "{asked}").map_err(|e| e.to_string())?;
    let mut line = String::new();
    std::io::BufReader::new(stream)
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    print!("{line}");
    Ok(())
}

fn serve(arguments: &[String], admin: PathBuf) -> Result<(), String> {
    if std::env::var(ENVIRONMENT).ok().as_deref() != Some("on") {
        eprintln!("lamp-control: off ({ENVIRONMENT} is not on): nothing listens");
        return Ok(());
    }
    let state =
        PathBuf::from(option(arguments, "--state-dir").unwrap_or_else(|| STATE_DIR.to_owned()));
    let listen = option(arguments, "--listen").unwrap_or_else(|| LISTEN.to_owned());
    let host = option(arguments, "--name")
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|h| h.trim().to_owned())
        })
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "lamp".to_owned());
    std::fs::create_dir_all(&state).map_err(|e| format!("{}: {e}", state.display()))?;
    let identity = cert::load_or_make(&state, &host).map_err(|e| format!("certificate: {e}"))?;
    let clients_path = state.join("clients.json");
    let clients =
        Clients::load(&clients_path).map_err(|e| format!("{}: {e}", clients_path.display()))?;
    eprintln!(
        "lamp-control: on, {listen}, {} paired, certificate pin {}",
        clients.clients.len(),
        identity.pin
    );
    // The quiet hours (#3917): the lamp's zone, `--zone`, `system` by
    // default. One that can't be read keeps every computer's say and sound
    // refused as quiet, never a silent UTC.
    let zone = option(arguments, "--zone").unwrap_or_else(|| "system".to_owned());
    let local_time = match lamp_local_time::LocalTime::zone(&zone) {
        Ok(local_time) => Some(Arc::new(local_time)),
        Err(error) => {
            eprintln!("lamp-control: {error}; say and sound are refused as quiet hours");
            None
        }
    };
    let door = session::Door::new(
        &host,
        &host,
        identity.pin_bytes(),
        clients,
        Some(clients_path),
    )
    .with_quiet_hours(session::Quiet::new(local_time));
    let body = option(arguments, "--mood-socket").map(PathBuf::from);
    let say = option(arguments, "--say-socket").map(PathBuf::from);
    let bus = match option(arguments, "--bus-root") {
        Some(root) => Some(lamp_bus::Bus::new(root).map_err(|e| format!("bus: {e}"))?),
        None => None,
    };
    let watch = Arc::new(Mutex::new(lamp_control::watch::Watch::new()));
    let light: Option<Box<dyn parts::LightOut>> = match &bus {
        Some(bus) => {
            let out = bus
                .publisher_declared(LightRequest::TOPIC, &LightRequest::declaration(), "control")
                .map_err(|e| format!("light/request: {e}"))?;
            Some(Box::new(out))
        }
        None => None,
    };
    let clock_id = option(arguments, "--boot-id")
        .or_else(|| {
            std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
                .ok()
                .map(|id| id.trim().to_owned())
        })
        .unwrap_or_default();
    let watching = watch.clone();
    let shared = server::Shared::new_with(door, identity.pin.clone(), move |events| {
        Box::new(parts::Lamp::new(body, light, say, clock_id, events).watching(watching))
    });
    let tls = server::acceptor(&identity).map_err(|e| format!("tls: {e}"))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(&listen)
            .await
            .map_err(|e| format!("{listen}: {e}"))?;
        let _ = std::fs::remove_file(&admin);
        // The owner's side: its folder 0700 before the socket is bound, so
        // nobody else can reach it even before its own 0600 (the
        // Reviewer-lead's P3 on #3778).
        if let Some(parent) = admin.parent() {
            use std::os::unix::fs::DirBuilderExt as _;
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)
                .map_err(|e| format!("{}: {e}", parent.display()))?;
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let owner = tokio::net::UnixListener::bind(&admin)
            .map_err(|e| format!("{}: {e}", admin.display()))?;
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&admin, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
        tokio::spawn(server::admin(owner, shared.clone()));
        if let Some(bus) = &bus {
            server::watch_bus(bus, watch, shared.clone())
                .map_err(|e| format!("bus topics: {e}"))?;
        }
        // The body's own lease, so the lamp's own life stands back while a
        // computer holds `body` (lamp-body --lease-socket).
        if let Some(path) = option(arguments, "--lease-socket") {
            tokio::spawn(lamp_control::body_lease::hold(
                PathBuf::from(path),
                shared.clone(),
            ));
        }
        tokio::select! {
            () = server::serve(listener, tls, shared) => {}
            _ = tokio::signal::ctrl_c() => {}
        }
        Ok::<(), String>(())
    })
}
