//! `lamp-play serve`: the unit that plays a URL for `lamp-control`. Off
//! unless `LAMP_PLAY_URL=on`; off it says so and exits 0 with nothing
//! listening. It listens on one unix socket (`LAMP_PLAY_SOCKET`, default
//! `/run/lamp-play/play.sock`) and takes one play at a time from `lamp-control`
//! alone, known by its unit's cgroup (not by its user, which 22 units share). It writes to the journal the play's id, how it
//! ended, its seconds and its bytes, and never a URL or a host.
use lamp_play::accept::handle;
use lamp_play::decode::Limits;
use lamp_play::fetch::SystemNet;
use lamp_play::peer::{ASKER, peer_in_unit};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::process::ExitCode;

const SWITCH: &str = "LAMP_PLAY_URL";
const SOCKET: &str = "LAMP_PLAY_SOCKET";
const DEFAULT_SOCKET: &str = "/run/lamp-play/play.sock";

fn main() -> ExitCode {
    if std::env::var(SWITCH).as_deref() != Ok("on") {
        eprintln!("lamp-play: off ({SWITCH} is not on), nothing listening");
        return ExitCode::SUCCESS;
    }
    let path = std::env::var(SOCKET).unwrap_or_else(|_| DEFAULT_SOCKET.to_owned());
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("lamp-play: cannot listen: {}", error.kind());
            return ExitCode::FAILURE;
        }
    };
    // Anyone may connect, and every connection is checked: only a peer in
    // lamp-control.service's cgroup is read from; the rest are closed.
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666));
    eprintln!("lamp-play: listening for {ASKER} only");
    let mut net = SystemNet::default();
    let admit = |stream: &std::os::unix::net::UnixStream| peer_in_unit(stream, ASKER);
    for connection in listener.incoming() {
        let Ok(stream) = connection else { continue };
        let Some(ended) = handle(stream, &admit, &mut net, Limits::default()) else {
            continue;
        };
        eprintln!(
            "lamp-play: play {}: {}, {} s, {} bytes",
            ended.id, ended.outcome, ended.seconds, ended.bytes
        );
    }
    ExitCode::SUCCESS
}
