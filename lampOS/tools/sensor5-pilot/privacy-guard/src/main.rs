use lamp_sensor5_recorder::{
    archive, consent, invalid,
    runtime::{self, Options},
};
use std::{collections::BTreeMap, io, path::PathBuf};

const USAGE: &str = "lamp-sensor5-recorder prepare --link-dir NEW_PRIVATE_ABSOLUTE_DIR\nlamp-sensor5-recorder record --bus-root PATH --link-dir PATH --output NEW_DIR --consent-alias P01 --operator-alias O01 --consent-record PATH --consent-text PATH --consent-sha256 HEX64 --seconds 10\nSource-only diagnostic. No device handles, grants, service installation or default-on route. Live record requires Linux and the dedicated authorized service.\n";

fn run() -> io::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let Some(command) = args.next() else {
        print!("{USAGE}");
        return Ok(());
    };
    if command == "--help" && args.next().is_none() {
        print!("{USAGE}");
        return Ok(());
    }
    if command == "__archive-child" {
        let output = args
            .next()
            .map(PathBuf::from)
            .ok_or_else(|| invalid("archive path missing"))?;
        let parent = args
            .next()
            .and_then(|v| v.to_str().and_then(|s| s.parse().ok()))
            .ok_or_else(|| invalid("archive parent missing"))?;
        if args.next().is_some() {
            return Err(invalid("extra archive argument"));
        }
        return archive::child(&output, parent);
    }
    let mut values = BTreeMap::new();
    while let Some(key) = args.next() {
        let key = key
            .into_string()
            .map_err(|_| invalid("option name must be UTF-8"))?;
        let value = args.next().ok_or_else(|| invalid("option value missing"))?;
        if values.insert(key, value).is_some() {
            return Err(invalid("duplicate option"));
        }
    }
    if command == "prepare" {
        let path = values
            .remove("--link-dir")
            .ok_or_else(|| invalid("--link-dir required"))?;
        if !values.is_empty() {
            return Err(invalid("unknown prepare option"));
        }
        consent::prepare(&PathBuf::from(path))?;
        println!("Private link prepared; no service changed and no source opened.");
        return Ok(());
    }
    if command != "record" {
        return Err(invalid("unknown command; use --help"));
    }
    let mut take = |key: &str| {
        values
            .remove(key)
            .ok_or_else(|| invalid(format!("{key} required")))
    };
    let bus_root = PathBuf::from(take("--bus-root")?);
    let link_dir = PathBuf::from(take("--link-dir")?);
    let output = PathBuf::from(take("--output")?);
    let consent_record = PathBuf::from(take("--consent-record")?);
    let consent_text = PathBuf::from(take("--consent-text")?);
    let mut text = |key: &str| {
        take(key)?
            .into_string()
            .map_err(|_| invalid("option value must be UTF-8"))
    };
    let consent_sha256 = text("--consent-sha256")?;
    let consent_alias = text("--consent-alias")?;
    let operator_alias = text("--operator-alias")?;
    let seconds = text("--seconds")?
        .parse()
        .map_err(|_| invalid("seconds must be integer"))?;
    if !values.is_empty() {
        return Err(invalid("unknown record option"));
    }
    runtime::record(Options {
        bus_root,
        link_dir,
        output,
        consent_record,
        consent_text,
        consent_sha256,
        consent_alias,
        operator_alias,
        seconds,
    })
}

fn main() {
    if let Err(error) = run() {
        eprintln!("lamp-sensor5-recorder: {error}");
        std::process::exit(1);
    }
}
