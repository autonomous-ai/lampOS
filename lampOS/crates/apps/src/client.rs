//! The real [`Yes`]: one line to lamp-control's admin socket, one line back,
//! the whole exchange within [`WITHIN`]. Whatever answers is only believed if
//! it IS lamp-control: its cgroup is read from the connection and must be the
//! unit's own. A socket that is gone, slow, garbled or answered by another
//! process is a no.
use crate::{
    peer::{CONTROL, in_one_of, peer_of},
    yes::{Yes, YesKey, parse_reply, request_line},
};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::{Duration, Instant},
};

/// lamp-control's admin socket.
pub const ADMIN: &str = "/run/lampos-control/admin.sock";
/// The longest the whole ask may take.
pub const WITHIN: Duration = Duration::from_secs(5);
/// The most bytes of an answer.
const MOST_REPLY: u64 = 4_096;

pub struct AdminYes {
    pub socket: PathBuf,
    /// The cgroups an answer is believed from: lamp-control's.
    pub units: Vec<String>,
}

impl AdminYes {
    pub fn product() -> Self {
        Self {
            socket: ADMIN.into(),
            units: vec![CONTROL.to_owned()],
        }
    }
}

impl Yes for AdminYes {
    fn take(&mut self, key: &YesKey) -> Result<(), String> {
        let started = Instant::now();
        let gone = |e: std::io::Error| format!("lamp-control's socket: {e}");
        let mut stream = UnixStream::connect(&self.socket).map_err(gone)?;
        // The answer's author, before anything is sent or believed.
        let peer = peer_of(&stream)?;
        let units: Vec<&str> = self.units.iter().map(String::as_str).collect();
        if !in_one_of(&peer.cgroup, &units) {
            return Err("the admin socket is not answered by lamp-control".into());
        }
        let left = |started: Instant| {
            WITHIN
                .checked_sub(started.elapsed())
                .filter(|left| !left.is_zero())
                .ok_or_else(|| "lamp-control did not answer in time".to_owned())
        };
        let within = left(started)?;
        stream
            .set_read_timeout(Some(within))
            .and_then(|()| stream.set_write_timeout(Some(within)))
            .map_err(gone)?;
        let mut line = request_line(key);
        line.push('\n');
        stream.write_all(line.as_bytes()).map_err(gone)?;
        let mut reply = String::new();
        BufReader::new(stream.take(MOST_REPLY))
            .read_line(&mut reply)
            .map_err(|e| format!("lamp-control's answer: {e}"))?;
        left(started)?;
        parse_reply(&reply)
    }
}
