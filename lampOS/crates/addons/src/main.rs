use lamp_addons::runtime::{Config, ENVIRONMENT, serve};
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("serve") {
        return Err("usage: lamp-addons serve [--enable] [--bus-root ABS] [--clock-id TEST_ID] [--run-for-ms N] [--say-quiet-ms N, for tests: below 15000] [--move-quiet-ms N, for tests: below 60000] [--body-socket PATH] (LAMP_ADDONS=on is --enable)".into());
    }
    let mut config = Config::default();
    let mut enabled = std::env::var(ENVIRONMENT).as_deref() == Ok("on");
    let mut seen = std::collections::BTreeSet::new();
    while let Some(arg) = args.next() {
        if !seen.insert(arg.clone()) {
            return Err("duplicate option".into());
        }
        match arg.as_str() {
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

fn main() {
    if let Err(error) = run() {
        eprintln!("lamp-addons: {error}");
        std::process::exit(1);
    }
}
