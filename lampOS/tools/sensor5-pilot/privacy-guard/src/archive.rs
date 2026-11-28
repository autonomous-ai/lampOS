//! An archival child has no source sockets and no permission to acquire.
//! It writes only packets admitted by its parent. Disk stalls cannot hold the
//! reader's privacy loop. On failure all existing and partial files remain.
use crate::{
    MAX_FRAME_BYTES,
    consent::{create_private, new_directory, sha256},
    invalid,
};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    fs,
    io::{self, Read, Write},
    os::unix::{fs::OpenOptionsExt, process::CommandExt},
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    time::{Duration, Instant},
};

const MAX_HEADER: usize = 48 * 1024;
const MAX_QUEUED: usize = 2 * MAX_FRAME_BYTES + MAX_HEADER;
const MAX_PACKETS: usize = 64;
const WRITE_SLICE: usize = 64 * 1024;
pub const STOP_BUDGET: Duration = Duration::from_millis(500);

pub struct Packet {
    pub header: Value,
    pub bytes: Vec<u8>,
}

impl Packet {
    pub fn encode(self) -> io::Result<Vec<u8>> {
        let header = serde_json::to_vec(&self.header)?;
        if header.len() > MAX_HEADER || self.bytes.len() > MAX_FRAME_BYTES {
            return Err(invalid("archive packet exceeds bound"));
        }
        let mut out = Vec::with_capacity(8 + header.len() + self.bytes.len());
        out.extend_from_slice(&(header.len() as u32).to_le_bytes());
        out.extend_from_slice(&(self.bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&header);
        out.extend_from_slice(&self.bytes);
        Ok(out)
    }
}

/// Encoding happens before the caller's final admission check. Enqueue is
/// nonblocking, bounded and makes no filesystem call.
pub struct Archive {
    child: Child,
    input: Option<ChildStdin>,
    output: ChildStdout,
    queue: VecDeque<Vec<u8>>,
    offset: usize,
    queued: usize,
    replies: Vec<u8>,
    pub ready: bool,
    pub acknowledged: u64,
    pub done: bool,
    stopped: bool,
}

impl Archive {
    pub fn spawn(executable: &Path, output: &Path) -> io::Result<Self> {
        let mut child = Command::new(executable)
            .arg("__archive-child")
            .arg(output)
            .arg(std::process::id().to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .process_group(0)
            .spawn()?;
        let input = child.stdin.take().expect("pipe requested");
        let stdout = child.stdout.take().expect("pipe requested");
        let result = Self {
            child,
            input: Some(input),
            output: stdout,
            queue: VecDeque::with_capacity(MAX_PACKETS),
            offset: 0,
            queued: 0,
            replies: Vec::new(),
            ready: false,
            acknowledged: 0,
            done: false,
            stopped: false,
        };
        rustix::io::ioctl_fionbio(result.input.as_ref().expect("pipe set"), true)?;
        rustix::io::ioctl_fionbio(&result.output, true)?;
        Ok(result)
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn enqueue(&mut self, encoded: Vec<u8>) -> io::Result<()> {
        if self.input.is_none()
            || self.queue.len() >= MAX_PACKETS
            || encoded.len() > MAX_QUEUED.saturating_sub(self.queued)
        {
            return Err(invalid("bounded archive queue full or closed"));
        }
        self.queued += encoded.len();
        self.queue.push_back(encoded);
        Ok(())
    }

    /// At most one bounded write slice and one bounded reply read per turn.
    pub fn poll(&mut self) -> io::Result<()> {
        if let Some(packet) = self.queue.front() {
            let end = (self.offset + WRITE_SLICE).min(packet.len());
            match self
                .input
                .as_mut()
                .ok_or_else(|| invalid("archive input closed"))?
                .write(&packet[self.offset..end])
            {
                Ok(0) => return Err(invalid("archive pipe closed")),
                Ok(count) => {
                    self.offset += count;
                    self.queued -= count;
                    if self.offset == packet.len() {
                        self.queue.pop_front();
                        self.offset = 0;
                    }
                }
                Err(error)
                    if error.kind() == io::ErrorKind::WouldBlock
                        || error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        let mut bytes = [0u8; 1024];
        match self.output.read(&mut bytes) {
            Ok(count) => self.replies.extend_from_slice(&bytes[..count]),
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
        if self.replies.len() > 4096 {
            return Err(invalid("archive reply exceeds bound"));
        }
        while let Some(end) = self.replies.iter().position(|b| *b == b'\n') {
            let line = self.replies.drain(..=end).collect::<Vec<_>>();
            let line = std::str::from_utf8(&line)
                .map_err(|_| invalid("invalid archive reply"))?
                .trim();
            if line == "READY" {
                self.ready = true;
            } else if line == "DONE" {
                self.done = true;
            } else if let Some(value) = line.strip_prefix("ACK ") {
                let sequence: u64 = value.parse().map_err(|_| invalid("invalid archive ACK"))?;
                if sequence != self.acknowledged + 1 {
                    return Err(invalid("archive ACK order changed"));
                }
                self.acknowledged = sequence;
            } else {
                return Err(invalid("unknown archive reply"));
            }
        }
        if let Some(status) = self.child.try_wait()?
            && (!status.success() || !self.done)
        {
            return Err(invalid(format!(
                "archive child exited before completion: {status}"
            )));
        }
        Ok(())
    }

    /// Admitted bytes may finish writing after source admission closes. No
    /// new source is read here. Kill/reap has a fixed total 500 ms budget;
    /// an uninterruptible kernel I/O reports unreaped, never success.
    pub fn stop(&mut self, result: Value) -> io::Result<()> {
        let deadline = Instant::now() + STOP_BUDGET;
        let flush_until = Instant::now() + STOP_BUDGET / 2;
        let _ = self.enqueue(
            Packet {
                header: json!({"kind":"finish", "result":result}),
                bytes: Vec::new(),
            }
            .encode()?,
        );
        while Instant::now() < flush_until {
            if self.poll().is_err() {
                break;
            }
            if self.done {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        self.input.take();
        if !self.done {
            self.kill();
        }
        while Instant::now() < deadline {
            if let Some(status) = self.child.try_wait()? {
                self.stopped = true;
                return if self.done && status.success() {
                    Ok(())
                } else {
                    Err(invalid(
                        "archive incomplete; existing and partial files retained",
                    ))
                };
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        self.kill();
        self.stopped = true;
        Err(invalid(
            "archive child was killed but not observed reaped within 500 ms; retention incomplete",
        ))
    }

    fn kill(&mut self) {
        if let Some(pid) = rustix::process::Pid::from_raw(self.child.id() as i32) {
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        }
        let _ = self.child.kill();
    }
}

impl Drop for Archive {
    fn drop(&mut self) {
        if !self.stopped {
            self.input.take();
            self.kill();
            let deadline = Instant::now() + STOP_BUDGET / 2;
            while Instant::now() < deadline {
                if self.child.try_wait().ok().flatten().is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }
}

fn read_packet(input: &mut impl Read) -> io::Result<Option<Packet>> {
    let mut lengths = [0u8; 8];
    match input.read(&mut lengths[..1])? {
        0 => return Ok(None),
        1 => input.read_exact(&mut lengths[1..])?,
        _ => unreachable!(),
    }
    let header_len = u32::from_le_bytes(lengths[..4].try_into().expect("4 bytes")) as usize;
    let payload_len = u32::from_le_bytes(lengths[4..].try_into().expect("4 bytes")) as usize;
    if header_len == 0 || header_len > MAX_HEADER || payload_len > MAX_FRAME_BYTES {
        return Err(invalid("archive input exceeds bound"));
    }
    let mut header = vec![0; header_len];
    let mut bytes = vec![0; payload_len];
    input.read_exact(&mut header)?;
    input.read_exact(&mut bytes)?;
    Ok(Some(Packet {
        header: serde_json::from_slice(&header)?,
        bytes,
    }))
}

fn retained(path: &Path, name: &str, bytes: &[u8], inventory: &mut fs::File) -> io::Result<()> {
    // Complete OS writes are acknowledged; a barrier for every 20 ms PCM
    // block would throttle capture on slow storage. This is not a power-loss
    // durability claim. Inventory and terminal marker are synced at finish.
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path.join(name))?;
    file.write_all(bytes)?;
    serde_json::to_writer(
        &mut *inventory,
        &json!({"path":name,"sha256":sha256(bytes),"bytes":bytes.len()}),
    )?;
    inventory.write_all(b"\n")?;
    Ok(())
}

/// Private internal argv, not a recording interface. It can only consume a
/// pipe. Parent death ends it even if its main thread is blocked in disk I/O.
pub fn child(output: &Path, parent: u32) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    rustix::process::set_parent_process_death_signal(Some(rustix::process::Signal::KILL))?;
    let still_parent = move || {
        rustix::process::getppid().is_some_and(|pid| pid.as_raw_nonzero().get() as u32 == parent)
    };
    if !still_parent() {
        return Err(invalid("archive parent already gone"));
    }
    std::thread::Builder::new()
        .name("archive-parent-watch".into())
        .spawn(move || {
            loop {
                if !still_parent() {
                    std::process::exit(74);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        })?;
    new_directory(output)?;
    let result = child_inner(output);
    if let Err(error) = &result {
        let _ = create_private(&output.join("INCOMPLETE.json"), &serde_json::to_vec(&json!({"schema_version":1,"reason":error.to_string(),"complete":false,"all_existing_files_retained":true})).unwrap_or_default());
    }
    result
}

fn child_inner(output: &Path) -> io::Result<()> {
    let mut input = io::stdin().lock();
    let mut reply = io::stdout().lock();
    let first = read_packet(&mut input)?.ok_or_else(|| invalid("archive has no session"))?;
    if first.header["kind"] != "start" || !first.bytes.is_empty() {
        return Err(invalid("archive needs session first"));
    }
    let mut inventory = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(output.join("inventory.jsonl"))?;
    new_directory(&output.join("records"))?;
    retained(output,"README.txt", b"Private consented sensor-5 raw archive. Never upload. Native JPEG bytes and PCM blocks are unchanged. A missing result.json or complete:false means incomplete capture; retain every partial file. inventory.jsonl hashes completed files only; a partial/unlisted file still needs a recovery hash before analysis. Camera cadence is source-native; missing frames/PCM are never filled. Audio at_us is host read completion, not ADC time. Physical A/V sync remains unvalidated.\n", &mut inventory)?;
    retained(
        output,
        "session.json",
        &serde_json::to_vec_pretty(&first.header["session"])?,
        &mut inventory,
    )?;
    for (field, name) in [
        ("consent_record", "consent.json"),
        ("consent_text", "consent.txt"),
    ] {
        let text = first.header[field]
            .as_str()
            .ok_or_else(|| invalid("session misses consent source"))?;
        retained(output, name, text.as_bytes(), &mut inventory)?;
    }
    reply.write_all(b"READY\n")?;
    reply.flush()?;
    let mut count = 0u64;
    let (mut frames, mut audio) = (0u64, 0u64);
    loop {
        let mut packet = read_packet(&mut input)?
            .ok_or_else(|| invalid("archive input closed without a terminal record"))?;
        if packet.header["kind"] == "finish" {
            let result = json!({"schema_version":1,"records":count,"frames":frames,"audio_blocks":audio,
                "result":packet.header["result"],"complete":packet.header["result"]["reason"]=="duration_elapsed"
                    && frames > 0 && audio > 0
                    && packet.header["result"]["frames_admitted"].as_u64()==Some(frames)
                    && packet.header["result"]["audio_blocks_admitted"].as_u64()==Some(audio)});
            retained(
                output,
                "result.json",
                &serde_json::to_vec_pretty(&result)?,
                &mut inventory,
            )?;
            inventory.sync_all()?;
            fs::File::open(output.join("result.json"))?.sync_all()?;
            reply.write_all(b"DONE\n")?;
            reply.flush()?;
            return Ok(());
        }
        let kind = packet.header["kind"]
            .as_str()
            .ok_or_else(|| invalid("archive record kind missing"))?;
        let extension = match kind {
            "frame" => {
                frames += 1;
                "frame.jpg"
            }
            "audio" => {
                audio += 1;
                "audio.pcm"
            }
            "privacy" => "privacy.json",
            _ => return Err(invalid("unknown archive record kind")),
        };
        count = count
            .checked_add(1)
            .ok_or_else(|| invalid("archive count exhausted"))?;
        let name = format!("records/{count:08}.{extension}");
        packet.header["schema_version"] = json!(1);
        packet.header["record_index"] = json!(count);
        packet.header["payload_file"] = json!(name);
        packet.header["sha256"] = json!(sha256(&packet.bytes));
        retained(output, &name, &packet.bytes, &mut inventory)?;
        retained(
            output,
            &format!("{name}.json"),
            &serde_json::to_vec_pretty(&packet.header)?,
            &mut inventory,
        )?;
        reply.write_all(format!("ACK {count}\n").as_bytes())?;
        reply.flush()?;
    }
}
