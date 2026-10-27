use lamp_addons::machine;
use lamp_addons::runtime::{Config, ENVIRONMENT, serve};
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

const HELP: &str = "usage: lamp-addons machine [--enable] [--stream] [--bus-root ABS] [--yes-file PATH] [--harness-dir PATH] [--port N] [--run-for-ms N]
LAMP_MACHINE_ADDONS=on is --enable: proposals from the person's own computers, through this lamp's Harness.
LAMP_MACHINE_ADDONS_STREAM=on is --stream: the bus's changes to those computers too, with the person's yes for it.
usage: lamp-addons serve [--enable] [--bus-root ABS] [--clock-id TEST_ID] [--run-for-ms N] [--say-quiet-ms N] [--move-quiet-ms N] [--body-socket PATH] [--mute-grace-ms N]
LAMP_ADDONS=on is --enable.
--say-quiet-ms N  Tests only: wait 0..15000 ms of quiet before a say (default 15000 ms).
--move-quiet-ms N  Tests only: wait 0..60000 ms of quiet before a move (default 60000 ms).
--mute-grace-ms N  Tests only: wait 0..3000 ms for a voice/mute never said (default 3000 ms).";

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--help" | "-h") => {
            println!("{HELP}");
            return Ok(());
        }
        Some("serve") => {}
        Some("machine") => return machine(args),
        _ => return Err(HELP.into()),
    }
    let mut config = Config::default();
    let mut enabled = std::env::var(ENVIRONMENT).as_deref() == Ok("on");
    let mut seen = std::collections::BTreeSet::new();
    while let Some(arg) = args.next() {
        if !seen.insert(arg.clone()) {
            return Err("duplicate option".into());
        }
        match arg.as_str() {
            "--help" | "-h" => {
                println!("{HELP}");
                return Ok(());
            }
            "--enable" => enabled = true,
            "--bus-root" => {
                config.bus_root = Some(PathBuf::from(
                    args.next().ok_or("--bus-root needs an absolute path")?,
                ))
            }
            "--body-socket" => {
                config.body_socket = Some(PathBuf::from(
                    args.next()
                        .ok_or("--body-socket needs the body's mood socket")?,
                ))
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
            // For tests only: the quiet before a say, shorter than the
            // product's 15 s, which is kept without it.
            "--say-quiet-ms" => {
                let ms: u64 = args
                    .next()
                    .ok_or("--say-quiet-ms needs milliseconds")?
                    .parse()?;
                if ms > 15_000 {
                    return Err("--say-quiet-ms is for tests, at most 15000".into());
                }
                config.say_quiet = Some(Duration::from_millis(ms));
            }
            "--move-quiet-ms" => {
                let ms: u64 = args
                    .next()
                    .ok_or("--move-quiet-ms needs milliseconds")?
                    .parse()?;
                if ms > 60_000 {
                    return Err("--move-quiet-ms is for tests, at most 60000".into());
                }
                config.move_quiet = Some(Duration::from_millis(ms));
            }
            "--mute-grace-ms" => {
                let ms: u64 = args
                    .next()
                    .ok_or("--mute-grace-ms needs milliseconds")?
                    .parse()?;
                if ms > 3_000 {
                    return Err("--mute-grace-ms must be 0..3000 (tests only)".into());
                }
                config.mute_grace_us = ms * 1_000;
            }
            _ => return Err("unknown option".into()),
        }
    }
    // Off unless set: without the switch it opens nothing and publishes
    // nothing, as lamp-skills loads no add-on then.
    if !enabled {
        println!(
            "{{\"enabled\":false,\"message\":\"Add-ons are off; LAMP_ADDONS=on or --enable starts the judge.\"}}"
        );
        return Ok(());
    }
    let stop = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, stop.clone())?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, stop.clone())?;
    let report = serve(config, stop)?;
    println!(
        "{{\"kept\":{},\"dropped\":{},\"lights_asked\":{},\"moves_asked\":{}}}",
        report.kept, report.dropped, report.lights_asked, report.moves_asked
    );
    Ok(())
}

/// `lamp-addons machine`: off unless set, as `serve` is.
fn machine(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = machine::Config::from_environment();
    let mut enabled = std::env::var(machine::ENVIRONMENT).as_deref() == Ok("on");
    let mut seen = std::collections::BTreeSet::new();
    while let Some(arg) = args.next() {
        if !seen.insert(arg.clone()) {
            return Err("duplicate option".into());
        }
        let mut value = |what: &str| args.next().ok_or_else(|| format!("{arg} needs {what}"));
        match arg.as_str() {
            "--help" | "-h" => {
                println!("{HELP}");
                return Ok(());
            }
            "--enable" => enabled = true,
            "--stream" => config.stream = true,
            "--bus-root" => config.bus_root = Some(PathBuf::from(value("an absolute path")?)),
            "--yes-file" => config.yes_file = PathBuf::from(value("a path")?),
            "--harness-dir" => config.data_dir = PathBuf::from(value("Harness's data folder")?),
            "--port" => {
                config.port = value("a port")?.parse()?;
                if config.port == 0 {
                    return Err("--port must be 1..65535".into());
                }
            }
            "--run-for-ms" => {
                let ms: u64 = value("milliseconds")?.parse()?;
                if !(1..=86_400_000).contains(&ms) {
                    return Err("--run-for-ms must be 1..86400000".into());
                }
                config.run_for = Some(Duration::from_millis(ms));
            }
            _ => return Err("unknown option".into()),
        }
    }
    // Off unless set: it reads no credential, opens nothing, writes nothing.
    if !enabled {
        println!(
            "{{\"enabled\":false,\"message\":\"Own-computer add-ons are off; LAMP_MACHINE_ADDONS=on or --enable starts them.\"}}"
        );
        return Ok(());
    }
    let harness = Arc::new(machine::Loopback::new(&config.data_dir, config.port)?);
    let stop = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, stop.clone())?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, stop.clone())?;
    let report = machine::serve(config, stop, harness)?;
    println!(
        "{{\"proposed\":{},\"refused\":{},\"decided\":{},\"published\":{},\"lines\":{}}}",
        report.proposed, report.refused, report.decided, report.published, report.lines
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("lamp-addons: {error}");
        std::process::exit(1);
    }
}
