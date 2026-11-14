//! Private supervised entry point. No interactive motion commands or defaults.
use lamp_body::runtime::{config::Config, supervisor::parse_boot};
use lamp_turn_owner::BootId;
use std::{io, path::PathBuf};
struct Args {
    config: PathBuf,
    directory: PathBuf,
    parent: BootId,
    worker: BootId,
    parent_pid: u32,
}
fn parse(args: impl Iterator<Item = String>) -> io::Result<Args> {
    let values: Vec<_> = args.take(11).collect();
    if values.len() != 10
        || values[0] != "--config"
        || values[2] != "--directory"
        || values[4] != "--controller"
        || values[6] != "--worker"
        || values[8] != "--parent-pid"
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: lamp-body-worker --config PRIVATE_JSON --directory PRIVATE_IPC_DIRECTORY --controller BOOT_HEX --worker BOOT_HEX --parent-pid PID",
        ));
    }
    Ok(Args {
        config: values[1].clone().into(),
        directory: values[3].clone().into(),
        parent: parse_boot(&values[5])?,
        worker: parse_boot(&values[7])?,
        parent_pid: values[9]
            .parse()
            .map_err(|_| io::Error::other("invalid parent PID"))?,
    })
}
fn main() -> std::process::ExitCode {
    let result = parse(std::env::args().skip(1)).and_then(run);
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("motor worker: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
#[cfg(unix)]
fn run(args: Args) -> io::Result<()> {
    let config = Config::read(&args.config)?;
    let channels = lamp_body::runtime::channel::Channels::bind(
        &args.directory,
        args.parent,
        args.worker,
        true,
    )?;
    lamp_body::runtime::native::run(config, channels, args.parent, args.worker, args.parent_pid)
}
#[cfg(not(unix))]
fn run(args: Args) -> io::Result<()> {
    let _ = (
        args.config,
        args.directory,
        args.parent,
        args.worker,
        args.parent_pid,
        std::mem::size_of::<Config>(),
    );
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "motor serial worker requires a unix system; no device was opened",
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_implicit_devices_or_motion_commands() {
        assert!(parse([].into_iter()).is_err());
        assert!(parse(["--torque".into(), "off".into()].into_iter()).is_err());
        assert!(
            parse(
                [
                    "--config",
                    "/x",
                    "--directory",
                    "/y",
                    "--controller",
                    "11111111111111111111111111111111",
                    "--worker",
                    "22222222222222222222222222222222",
                    "--parent-pid",
                    "123"
                ]
                .map(String::from)
                .into_iter()
            )
            .is_ok()
        );
    }
}
