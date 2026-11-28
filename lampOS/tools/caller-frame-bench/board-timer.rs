//! Measure the actual caller-voice child protocol on already-loaded synthetic PCM.
//! This standalone program uses the Rust standard library only.
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const DEADLINE: Duration = Duration::from_secs(60);
const COLD_PROCESSES: usize = 3;
const WARM_ROUNDS: usize = 3;

struct Probe {
    id: String,
    words: usize,
    condition: String,
    wake: Arc<Vec<u8>>,
    line: Arc<Vec<u8>>,
}

fn local_file(root: &Path, name: &str) -> Result<PathBuf> {
    let path = Path::new(name);
    if !path.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err("expected a relative fixture path".into());
    }
    let file = root.join(path).canonicalize()?;
    if !file.starts_with(root) {
        return Err("fixture escapes kit".into());
    }
    Ok(file)
}

fn pcm(path: &Path, max_samples: usize) -> Result<Arc<Vec<u8>>> {
    let wav = fs::read(path)?;
    if wav.get(..4) != Some(b"RIFF") || wav.get(8..12) != Some(b"WAVE") {
        return Err("expected a RIFF/WAVE file".into());
    }
    let mut pos = 12;
    let mut format = false;
    let mut data = None;
    while let Some(header) = wav.get(pos..pos + 8) {
        let size = u32::from_le_bytes(header[4..8].try_into()?) as usize;
        pos += 8;
        let chunk = wav.get(pos..pos + size).ok_or("truncated WAV chunk")?;
        match &header[..4] {
            b"fmt " => {
                if size < 16
                    || &chunk[..4] != b"\x01\x00\x01\x00"
                    || u32::from_le_bytes(chunk[4..8].try_into()?) != 16_000
                    || &chunk[12..16] != b"\x02\x00\x10\x00"
                {
                    return Err("expected 16 kHz, mono, PCM16".into());
                }
                format = true;
            }
            b"data" if data.is_none() => data = Some(chunk),
            b"data" => return Err("multiple PCM data chunks".into()),
            _ => {}
        }
        pos += size + (size % 2);
    }
    let bytes = data.ok_or("missing PCM")?;
    if !format || bytes.is_empty() || bytes.len() % 2 != 0 || bytes.len() / 2 > max_samples {
        return Err("invalid PCM format, duration or word alignment".into());
    }
    Ok(Arc::new(bytes.to_vec()))
}

fn probes(root: &Path) -> Result<Vec<Probe>> {
    let manifest = fs::read_to_string(root.join("cases.tsv"))?;
    let mut rows = manifest.lines();
    if rows.next() != Some("case_id\treference\tfollowup\tlabel\twords\tcondition") {
        return Err("unexpected cases.tsv header".into());
    }
    let mut probes = Vec::new();
    for row in rows {
        let fields: Vec<_> = row.split('\t').collect();
        let [id, wake, line, label, words, condition] = fields.as_slice() else {
            return Err("invalid case row".into());
        };
        let words = words.parse::<usize>()?;
        if id.is_empty()
            || words == 0
            || probes.iter().any(|p: &Probe| p.id == *id)
            || !matches!(*condition, "quiet" | "noisy")
            || !matches!(*label, "same" | "different")
        {
            return Err("invalid or repeated case".into());
        }
        probes.push(Probe {
            id: id.to_string(),
            words,
            condition: condition.to_string(),
            wake: pcm(&local_file(root, wake)?, 48_000)?,
            line: pcm(&local_file(root, line)?, 192_000)?,
        });
    }
    if probes.is_empty() {
        return Err("no synthetic probes".into());
    }
    Ok(probes)
}

fn wire_line(reader: &mut impl BufRead) -> io::Result<(String, Instant)> {
    let mut bytes = Vec::new();
    std::io::Read::take(reader, 128).read_until(b'\n', &mut bytes)?;
    let completed = Instant::now();
    if bytes.pop() != Some(b'\n') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing or overlong child line",
        ));
    }
    Ok((
        String::from_utf8(bytes).map_err(io::Error::other)?,
        completed,
    ))
}

struct Request {
    header: Vec<u8>,
    wake: Arc<Vec<u8>>,
    line: Arc<Vec<u8>>,
}

struct Response {
    line: String,
    elapsed: Duration,
    end: Instant,
}

struct Session {
    child: Child,
    commands: Option<Sender<Request>>,
    replies: Receiver<io::Result<Response>>,
    start: Instant,
    ready: Duration,
}

impl Session {
    fn launch(root: &Path, stderr: &File) -> Result<Self> {
        let mut command = Command::new(root.join("bin/lamp-same-voice"));
        command
            .arg("frame-serve")
            .arg(root.join("model"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(stderr.try_clone()?);
        let start = Instant::now();
        let mut child = command.spawn()?;
        let mut input = child.stdin.take().ok_or("no child stdin")?;
        let mut output = BufReader::new(child.stdout.take().ok_or("no child stdout")?);
        let (commands, queue) = mpsc::channel::<Request>();
        let (send, replies) = mpsc::channel();
        thread::spawn(move || {
            let ready = wire_line(&mut output).map(|(line, end)| Response {
                line,
                elapsed: Duration::ZERO,
                end,
            });
            if send.send(ready).is_err() {
                return;
            }
            for req in queue {
                let measured = (|| {
                    let begin = Instant::now();
                    input.write_all(&req.header)?;
                    input.write_all(&req.wake)?;
                    input.write_all(&req.line)?;
                    input.flush()?;
                    let (line, end) = wire_line(&mut output)?;
                    Ok(Response {
                        line,
                        elapsed: end.duration_since(begin),
                        end,
                    })
                })();
                let failed = measured.is_err();
                if send.send(measured).is_err() || failed {
                    break;
                }
            }
        });
        let mut s = Self {
            child,
            commands: Some(commands),
            replies,
            start,
            ready: Duration::ZERO,
        };
        let hello = s.replies.recv_timeout(DEADLINE)??;
        if hello.line != "ready" {
            return Err("child did not answer ready".into());
        }
        s.ready = hello.end.duration_since(start);
        Ok(s)
    }

    fn measure(&self, probe: &Probe, seq: usize) -> Result<Response> {
        let request = Request {
            header: format!(
                "pair {} {} {} {}\n",
                seq * 2 + 1,
                seq * 2 + 2,
                probe.wake.len() / 2,
                probe.line.len() / 2
            )
            .into_bytes(),
            wake: Arc::clone(&probe.wake),
            line: Arc::clone(&probe.line),
        };
        self.commands
            .as_ref()
            .ok_or("child closed")?
            .send(request)?;
        let response = self.replies.recv_timeout(DEADLINE)??;
        if response.line != "unknown" {
            let (class, score) = response
                .line
                .split_once(' ')
                .ok_or("invalid child response")?;
            if !matches!(class, "same" | "different") || !score.parse::<f64>()?.is_finite() {
                return Err("invalid child class or score".into());
            }
        }
        Ok(response)
    }

    fn finish(&mut self) -> Result<()> {
        drop(self.commands.take());
        let stop = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait()? {
                return if status.success() {
                    Ok(())
                } else {
                    Err(format!("child exit {status}").into())
                };
            }
            if Instant::now() >= stop {
                return Err("child failed to exit on EOF".into());
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        drop(self.commands.take());
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn row(
    out: &mut File,
    phase: &str,
    process: usize,
    seq: usize,
    p: &Probe,
    response: &Response,
    s: &Session,
) -> io::Result<()> {
    let (class, score) = response
        .line
        .split_once(' ')
        .unwrap_or((&response.line, ""));
    let (ready, total) = if phase == "first" {
        (
            s.ready.as_micros().to_string(),
            response.end.duration_since(s.start).as_micros().to_string(),
        )
    } else {
        (String::new(), String::new())
    };
    writeln!(
        out,
        "{phase}\t{process}\t{seq}\t{}\t{}\t{}\t{}\t{}\t{class}\t{score}\t{ready}\t{}\t{total}",
        p.id,
        p.words,
        p.condition,
        p.wake.len() / 2,
        p.line.len() / 2,
        response.elapsed.as_micros()
    )?;
    out.flush()
}

fn summary(out: &mut File, metric: &str, mut values: Vec<u128>) -> io::Result<()> {
    if values.is_empty() {
        return Ok(());
    }
    values.sort_unstable();
    let n = values.len();
    writeln!(
        out,
        "# summary\t{metric}\tn={n}\tp50_us={}\tp95_us={}\tmax_us={}",
        values[(n * 50).div_ceil(100) - 1],
        values[(n * 95).div_ceil(100) - 1],
        values[n - 1]
    )
}

fn execute(out: &mut File, stderr: &File, root: &Path, cases: &[Probe]) -> Result<()> {
    writeln!(
        out,
        "# sensor6-board-v2\tplatform={}/{}\tclock=monotonic\tunits=us\tpercentile=nearest_rank",
        env::consts::OS,
        env::consts::ARCH
    )?;
    writeln!(
        out,
        "phase\tprocess\tsequence\tcase_id\twords\tcondition\twake_samples\tfollowup_samples\toutput\tscore\tspawn_to_ready_us\trequest_to_reply_us\tspawn_to_reply_us"
    )?;
    let mut starts = Vec::new();
    let mut first = Vec::new();
    let mut totals = Vec::new();
    let mut warm = Vec::new();
    let mut short = Vec::new();
    let mut long = Vec::new();
    let mut seq = 0;
    for process in 1..=COLD_PROCESSES {
        let mut s = Session::launch(root, stderr)?;
        starts.push(s.ready.as_micros());
        let p = &cases[0];
        let r = s.measure(p, seq)?;
        first.push(r.elapsed.as_micros());
        totals.push(r.end.duration_since(s.start).as_micros());
        row(out, "first", process, seq, p, &r, &s)?;
        seq += 1;
        if process == COLD_PROCESSES {
            for round in 0..WARM_ROUNDS {
                for offset in 0..cases.len() {
                    let p = &cases[(offset + round) % cases.len()];
                    let r = s.measure(p, seq)?;
                    let us = r.elapsed.as_micros();
                    warm.push(us);
                    if p.words <= 3 {
                        short.push(us);
                    } else {
                        long.push(us);
                    }
                    row(out, "warm", process, seq, p, &r, &s)?;
                    seq += 1;
                }
            }
        }
        s.finish()?;
    }
    for (name, v) in [
        ("spawn_to_ready", starts),
        ("first_request_to_reply", first),
        ("spawn_to_first_reply", totals),
        ("warm_request_to_reply", warm),
        ("warm_short_request_to_reply", short),
        ("warm_long_request_to_reply", long),
    ] {
        summary(out, name, v)?;
    }
    writeln!(out, "# COMPLETE\t{seq} replies")?;
    out.sync_all()?;
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let [kit, path] = args.as_slice() else {
        return Err("usage: board-timer KIT_DIR NEW_RESULT.tsv".into());
    };
    let root = Path::new(kit).canonicalize()?;
    let cases = probes(&root)?;
    let mut out = OpenOptions::new().write(true).create_new(true).open(path)?;
    let mut child_path = path.clone();
    child_path.push(".child.log");
    let stderr = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(child_path)?;
    if let Err(e) = execute(&mut out, &stderr, &root, &cases) {
        writeln!(out, "# INCOMPLETE\t{e}")?;
        out.sync_all()?;
        return Err(e);
    }
    Ok(())
}
