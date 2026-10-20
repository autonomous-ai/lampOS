use lamp_mood::runtime::{Config, serve};
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

/// The product's switch: `on` is as `--enable`; anything else is off.
const ENVIRONMENT: &str = "LAMP_MOOD";

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("serve") {
        return Err("usage: lamp-mood serve [--enable] [--zone IANA|system] [--bus-root ABS] [--clock-id TEST_ID] [--run-for-ms N] (LAMP_MOOD=on is --enable)".into());
    }
    let mut config = Config::default();
    let mut seen = std::collections::BTreeSet::new();
    while let Some(arg) = args.next() {
        if !seen.insert(arg.clone()) {
            return Err("duplicate option".into());
        }
        match arg.as_str() {
            "--enable" => config.enabled = true,
            "--zone" => config.zone = Some(args.next().ok_or("--zone needs an IANA name")?),
            "--bus-root" => {
                config.bus_root =
                    PathBuf::from(args.next().ok_or("--bus-root needs an absolute path")?)
            }
            "--clock-id" => {
                config.clock_id = Some(args.next().ok_or("--clock-id needs a test identity")?)
            }
            "--run-for-ms" => {
                let ms: u64 = args
                    .next()
                    .ok_or("--run-for-ms needs milliseconds")?
                    .parse()?;
                if !(1..=86_400_000).contains(&ms) {
                    return Err("--run-for-ms must be 1..86400000".into());
                }
                config.run_for = Some(Duration::from_millis(ms));
            }
            _ => return Err("unknown option".into()),
        }
    }
    // The product unit is off unless a drop-in sets `LAMP_MOOD=on` (#1670),
    // as `--enable` is for a run by hand.
    if std::env::var(ENVIRONMENT).as_deref() == Ok("on") {
        config.enabled = true;
    }
    // How the near face seems, as an input (#1671): off unless set.
    config.seems = std::env::var(lamp_mood::input::SEEMS_ENVIRONMENT).as_deref() == Ok("on");
    // Disabled invocation does not read the clock, resolve a zone, open files,
    // install signal handlers, start threads, subscribe or publish.
    if !config.enabled {
        println!("{{\"enabled\":false,\"message\":\"Mood is off; supply --enable to start it.\"}}");
        return Ok(());
    }
    let stop = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, stop.clone())?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, stop.clone())?;
    let report = serve(config, stop)?;
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("mood: {error}");
        std::process::exit(1);
    }
}
