//! The executive as a process: `lamp-actions --dir ABSOLUTE_DIR ...`.
//!
//! It runs until it is killed. The standard library cannot catch a signal
//! and this crate takes in nothing that can, so SIGTERM ends it where it
//! stands, between calls or in one; a call it was in is not answered and
//! the voice tells its model so. The directory is the unit's runtime
//! directory, which systemd removes, and with it the list of what the lamp
//! can do: the voice then offers its model none of it.

use lamp_actions::{
    Executive,
    light::BusLight,
    sound::{Amixer, Volume},
};
use std::{path::PathBuf, process::ExitCode, sync::atomic::AtomicBool};

const USAGE: &str = "usage: lamp-actions --dir ABSOLUTE_DIR [--mixer-card NAME --mixer-control NAME [--volume-ceiling 1..100]] [--bus-root ABSOLUTE_DIR] [--caps-dir ABSOLUTE_PERSISTENT_DIR]";
const HELP: &str = "The lamp's executive: carries out what the voice's model asks the lamp to do to itself.\n\
  --dir             where tools.json, call.sock and ready are kept (the unit's runtime directory)\n\
  --mixer-card      the ALSA card of the speaker's volume control; with --mixer-control, the volume functions are offered\n\
  --mixer-control   the control's name, as `amixer scontrols` prints it\n\
  --volume-ceiling  the loudest the speaker is allowed, in points of 100 (default 100)\n\
  --bus-root        the bus's directory; with it the light functions are offered while the ring's owner says what it shows\n\
  --caps-dir        private persistent storage for output caps; required with LAMP_OUTPUT_CAPS=on and --bus-root, never the runtime directory. Ignored when off; relative or dot-component paths are always refused\n\
  LAMP_OUTPUT_CAPS_DIR is the same folder, as the product unit passes it; --caps-dir wins when both are given.\n\
  LAMP_OUTPUT_CAPS=on enables private caps_put/caps_remove/caps_get operations. Every other value leaves storage, hardware and bus behavior unchanged. No cap operations are model tools.\n\
  LAMP_SCHEDULE_TOOLS=on enables six reminder tools; LAMP_SCHEDULE_SOCKET is an absolute socket path (default /run/lampos-schedule/call.sock). Other switch values leave the old tool list unchanged.\n\
  LAMP_SHORT_DONE=on tells the model to say only a word or two after a light or volume change it asked for.\n\
  LAMP_REQUESTED_AUDIO=on publishes speaker mute generations and volume observations; requires --bus-root and the Linux host boot clock. Other values add no mixer polling or speaker topic.";

#[derive(Debug, Default, Eq, PartialEq)]
struct Arguments {
    dir: PathBuf,
    mixer: Option<(String, String)>,
    ceiling: u8,
    bus: Option<PathBuf>,
    caps_dir: Option<PathBuf>,
}

fn parse(arguments: impl Iterator<Item = String>) -> Result<Option<Arguments>, String> {
    let mut parsed = Arguments {
        ceiling: 100,
        ..Arguments::default()
    };
    let (mut dir, mut card, mut control) = (None, None, None);
    let mut arguments = arguments;
    while let Some(option) = arguments.next() {
        if option == "--help" || option == "-h" {
            return Ok(None);
        }
        let value = arguments
            .next()
            .ok_or_else(|| format!("{option} needs a value"))?;
        let absolute = |value: &str| -> Result<PathBuf, String> {
            let path = PathBuf::from(value);
            path.is_absolute()
                .then_some(path)
                .ok_or_else(|| format!("{option} must be an absolute path"))
        };
        match option.as_str() {
            "--dir" => dir = Some(absolute(&value)?),
            "--bus-root" => parsed.bus = Some(absolute(&value)?),
            "--caps-dir" => {
                let path = PathBuf::from(value);
                if !lamp_actions::caps_service::safe_path(&path) {
                    return Err("--caps-dir must be absolute without dot components".into());
                }
                parsed.caps_dir = Some(path);
            }
            "--mixer-card" => card = Some(value),
            "--mixer-control" => control = Some(value),
            "--volume-ceiling" => {
                parsed.ceiling = value
                    .parse()
                    .ok()
                    .filter(|ceiling| (1..=100).contains(ceiling))
                    .ok_or("--volume-ceiling is 1 to 100")?;
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    parsed.dir = dir.ok_or("--dir is required")?;
    parsed.mixer = match (card, control) {
        (Some(card), Some(control)) => Some((card, control)),
        (None, None) => None,
        _ => return Err("--mixer-card and --mixer-control go together".into()),
    };
    Ok(Some(parsed))
}

/// `LAMP_OUTPUT_CAPS_DIR`, held to `--caps-dir`'s rule; unset or empty is none.
fn caps_dir_from(value: Option<&std::ffi::OsStr>) -> Result<Option<PathBuf>, String> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let path = PathBuf::from(value);
    if !lamp_actions::caps_service::safe_path(&path) {
        return Err("LAMP_OUTPUT_CAPS_DIR must be absolute without dot components".into());
    }
    Ok(Some(path))
}

fn main() -> ExitCode {
    // Its log lines never block it or end it, and its last ones reach the
    // journal (`lamp_ipc::log_relay`).
    let _log = lamp_ipc::log_relay::guard();
    let mut arguments = match parse(std::env::args().skip(1)) {
        Ok(Some(arguments)) => arguments,
        Ok(None) => {
            println!("{USAGE}\n\n{HELP}");
            return ExitCode::SUCCESS;
        }
        Err(why) => {
            eprintln!("lamp-actions: {why}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    if arguments.caps_dir.is_none() {
        match caps_dir_from(
            std::env::var_os(lamp_actions::caps_service::DIR_ENVIRONMENT).as_deref(),
        ) {
            Ok(directory) => arguments.caps_dir = directory,
            Err(why) => {
                eprintln!("lamp-actions: {why}");
                return ExitCode::from(2);
            }
        }
    }
    let schedule = match lamp_actions::schedule::Adapter::from_environment() {
        Ok(adapter) => adapter,
        Err(error) => {
            eprintln!("lamp-actions: invalid Schedule configuration: {error}");
            return ExitCode::from(2);
        }
    };
    let caps = match lamp_actions::caps_service::Config::configured(
        std::env::var_os(lamp_actions::caps_service::ENVIRONMENT).as_deref(),
        arguments.caps_dir.as_deref(),
        arguments.bus.as_deref(),
    ) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("lamp-actions: invalid output cap configuration: {error}");
            return ExitCode::from(2);
        }
    };
    let sound = arguments
        .mixer
        .map(|(card, control)| Volume::new(Amixer { card, control }, arguments.ceiling));
    let speaker =
        match lamp_actions::speaker_state::Service::from_environment(arguments.bus.as_deref()) {
            Ok(speaker) => speaker,
            Err(error) => {
                eprintln!("lamp-actions: invalid speaker observation configuration: {error}");
                return ExitCode::from(2);
            }
        };
    // The speaker's volume said on the bus after each change, for every
    // reader (lamp-control's `volume.changed`): with a bus and a mixer.
    let volume_out = match (arguments.bus.as_ref(), sound.as_ref()) {
        (Some(root), Some(_)) => lamp_bus::Bus::new(root)
            .and_then(|bus| lamp_actions::volume_out::VolumeOut::on(&bus))
            .inspect_err(|error| eprintln!("lamp-actions: the volume is not said: {error}"))
            .ok(),
        _ => None,
    };
    // The lamp's health hears from this part on the same bus.
    let health = arguments.bus.as_ref().and_then(|root| {
        lamp_bus::Bus::new(root)
            .and_then(|bus| {
                bus.publisher_declared(
                    lamp_msgs::Diagnostic::TOPIC,
                    &lamp_msgs::Diagnostic::declaration(),
                    "actions",
                )
            })
            .inspect_err(|error| eprintln!("lamp-actions: its health is not said: {error}"))
            .ok()
    });
    let light = match arguments.bus.map(lamp_bus::Bus::new) {
        Some(Ok(bus)) => match BusLight::new(bus) {
            Ok(light) => Some(light),
            Err(error) => {
                eprintln!("lamp-actions: no light: {error}");
                None
            }
        },
        Some(Err(error)) => {
            eprintln!("lamp-actions: no light: the bus root cannot be used: {error}");
            None
        }
        None => None,
    };
    let short =
        std::env::var_os(lamp_actions::SHORT_ENVIRONMENT).is_some_and(|value| value == "on");
    if short {
        eprintln!(
            "lamp-actions: the light's and the volume's calls are answered short (LAMP_SHORT_DONE)"
        );
    }
    let mut executive = Executive {
        sound,
        light,
        short,
    };
    match lamp_actions::server::serve_reporting_with_volume(
        &arguments.dir,
        &mut executive,
        &AtomicBool::new(false),
        health,
        schedule.as_ref(),
        caps.as_ref(),
        speaker,
        volume_out,
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lamp-actions: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(text: &str) -> Result<Option<Arguments>, String> {
        parse(text.split_whitespace().map(str::to_owned))
    }

    #[test]
    fn the_options_are_read_and_anything_else_is_refused_with_the_reason() {
        let all = of("--dir /run/lampos-actions --mixer-card allwinnerhdmi --mixer-control Speaker --volume-ceiling 80 --bus-root /run/lampos-bus")
            .unwrap()
            .unwrap();
        assert_eq!(
            all,
            Arguments {
                dir: "/run/lampos-actions".into(),
                mixer: Some(("allwinnerhdmi".into(), "Speaker".into())),
                ceiling: 80,
                bus: Some("/run/lampos-bus".into()),
                caps_dir: None,
            }
        );
        let least = of("--dir /run/x").unwrap().unwrap();
        assert_eq!((least.mixer, least.ceiling, least.bus), (None, 100, None));
        assert_eq!(of("--help").unwrap(), None);
        for refused in [
            "",
            "--dir relative",
            "--dir /run/x --bus-root relative",
            "--dir /run/x --mixer-card only",
            "--dir /run/x --volume-ceiling 0",
            "--dir /run/x --volume-ceiling 101",
            "--dir /run/x --caps-dir relative",
            "--dir /run/x --caps-dir /var/lib/../caps",
            "--dir /run/x --caps-dir /var/lib/./caps",
            "--dir /run/x --loud yes",
            "--dir",
        ] {
            assert!(of(refused).is_err(), "{refused:?}");
        }
    }

    #[test]
    fn cap_storage_is_an_explicit_absolute_option() {
        let parsed = of("--dir /run/x --caps-dir /var/lib/lampOS/output-caps")
            .unwrap()
            .unwrap();
        assert_eq!(
            parsed.caps_dir.as_deref(),
            Some(std::path::Path::new("/var/lib/lampOS/output-caps"))
        );
    }

    #[test]
    fn cap_storage_also_comes_by_environment_held_to_the_same_rule() {
        use std::{ffi::OsStr, path::Path};
        assert_eq!(caps_dir_from(None), Ok(None));
        assert_eq!(caps_dir_from(Some(OsStr::new(""))), Ok(None));
        assert_eq!(
            caps_dir_from(Some(OsStr::new("/var/lib/lampos-output-caps"))),
            Ok(Some(Path::new("/var/lib/lampos-output-caps").into()))
        );
        for refused in ["relative", "/var/lib/../caps", "/var/lib/./caps"] {
            assert!(
                caps_dir_from(Some(OsStr::new(refused))).is_err(),
                "{refused}"
            );
        }
    }
}
