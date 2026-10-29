use lamp_schedule::runtime::{Config, SystemClock, serve};
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("serve") {
        return Err("usage: lamp-schedule serve --dir ABS --socket ABS --zone IANA|system [--bus-root ABS] [--boot-id ID] [--trust-clock] [--kernel-clock] [--run-for-ms N] [--enable]".into());
    }
    let mut dir = None;
    let mut socket = None;
    let mut zone = None;
    let mut bus_root = PathBuf::from(lamp_bus::DEFAULT_ROOT);
    let mut boot_id = None;
    let mut trust_clock = false;
    let mut kernel_clock = false;
    let mut enabled = false;
    let mut run_for = None;
    let mut seen = std::collections::BTreeSet::new();
    while let Some(arg) = args.next() {
        if !seen.insert(arg.clone()) {
            return Err("duplicate option".into());
        }
        match arg.as_str() {
            "--enable" => enabled = true,
            "--trust-clock" => trust_clock = true,
            "--kernel-clock" => kernel_clock = true,
            "--dir" => dir = Some(PathBuf::from(args.next().ok_or("--dir needs a path")?)),
            "--socket" => socket = Some(PathBuf::from(args.next().ok_or("--socket needs a path")?)),
            "--zone" => zone = Some(args.next().ok_or("--zone needs an IANA name")?),
            "--bus-root" => bus_root = PathBuf::from(args.next().ok_or("--bus-root needs a path")?),
            "--boot-id" => boot_id = Some(args.next().ok_or("--boot-id needs an identity")?),
            "--run-for-ms" => {
                let ms = args
                    .next()
                    .ok_or("--run-for-ms needs milliseconds")?
                    .parse::<u64>()?;
                if !(1..=86_400_000).contains(&ms) {
                    return Err("--run-for-ms must be 1..86400000".into());
                }
                run_for = Some(Duration::from_millis(ms));
            }
            _ => return Err("unknown option".into()),
        }
    }
    if !enabled {
        println!(
            "{{\"enabled\":false,\"message\":\"Schedule is off; supply --enable to start it.\"}}"
        );
        return Ok(());
    }
    let config = Config {
        enabled,
        dir: dir.ok_or("--dir is required")?,
        socket: socket.ok_or("--socket is required")?,
        bus_root,
        zone: zone.ok_or("--zone is required")?,
        // zone.env's other line, in the unit's environment (#1680).
        utc_offset_minutes: std::env::var("LAMP_UTC_OFFSET_MINUTES")
            .ok()
            .and_then(|minutes| minutes.trim().parse().ok()),
        run_for,
        report_every: lamp_schedule::runtime::REPORT_EVERY,
        request_deadline: lamp_schedule::runtime::REQUEST_DEADLINE,
    };
    let clock = SystemClock::new(boot_id.as_deref(), trust_clock)?.with_kernel_clock(kernel_clock);
    let stop = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, stop.clone())?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, stop.clone())?;
    serve(config, clock, stop)?;
    Ok(())
}

fn main() {
    // Its log lines never block it or end it, and its last ones reach the
    // journal (`lamp_ipc::log_relay`).
    let _log = lamp_ipc::log_relay::guard();
    if let Err(error) = run() {
        eprintln!("schedule: {error}");
        // `exit` runs no destructor, so the guard would not pass the line on.
        lamp_ipc::log_relay::finish(std::time::Duration::from_millis(500));
        std::process::exit(1);
    }
}
