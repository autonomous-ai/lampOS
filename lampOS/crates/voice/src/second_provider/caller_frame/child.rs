//! Model process for the measurement switch. Pipes and deadlines live on a
//! separate task. A reset interrupts a blocked write, load or response.
use super::{Duration, Pair, ResultRow, queue, watch};
use std::{
    io,
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    time::timeout,
};

struct Model {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Model {
    fn start(program: &Path, model: &Path) -> io::Result<Self> {
        let mut child = Command::new(program)
            .arg("frame-serve")
            .arg(model)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("model stdin"))?;
        let output = BufReader::new(
            child
                .stdout
                .take()
                .ok_or_else(|| io::Error::other("model stdout"))?,
        );
        Ok(Self {
            child,
            input,
            output,
        })
    }

    // Prevent a broken model from allocating an unbounded stdout line.
    async fn line(&mut self) -> io::Result<String> {
        let mut bytes = Vec::new();
        (&mut self.output)
            .take(128)
            .read_until(b'\n', &mut bytes)
            .await?;
        if bytes.pop() != Some(b'\n') {
            return Err(io::Error::other("invalid model response"));
        }
        String::from_utf8(bytes).map_err(io::Error::other)
    }

    async fn ask(&mut self, job: &Pair) -> io::Result<(&'static str, Option<f32>)> {
        let o = job.observation;
        let header = format!(
            "pair {} {} {} {}\n",
            o.caller
                .ok_or_else(|| io::Error::other("missing spoken caller"))?,
            o.request,
            job.reference.len(),
            job.line.len()
        );
        self.input.write_all(header.as_bytes()).await?;
        let mut bytes = Vec::with_capacity(8_192);
        for audio in [&*job.reference, job.line.as_slice()] {
            for chunk in audio.chunks(4_096) {
                bytes.clear();
                bytes.extend(chunk.iter().flat_map(|sample| sample.to_le_bytes()));
                self.input.write_all(&bytes).await?;
            }
        }
        self.input.flush().await?;
        let response = self.line().await?;
        parse(&response).ok_or_else(|| io::Error::other("invalid model prediction"))
    }
}

fn parse(line: &str) -> Option<(&'static str, Option<f32>)> {
    if line == "unknown" {
        return Some(("unknown", None));
    }
    let (class, score) = line.split_once(' ')?;
    let score = score
        .parse::<f32>()
        .ok()
        .filter(|n| (0.0..=1.0).contains(n))?;
    match class {
        "same" => Some(("same", Some(score))),
        "different" => Some(("different", Some(score))),
        _ => None,
    }
}

async fn stop(model: &mut Option<Model>) {
    if let Some(mut model) = model.take() {
        let _ = model.child.kill().await;
    }
}

pub(super) async fn run(
    program: PathBuf,
    model: PathBuf,
    mut jobs: queue::Receiver<Pair>,
    results: queue::Sender<ResultRow>,
    mut changes: watch::Receiver<u64>,
    mut warming: watch::Receiver<Option<u64>>,
) {
    let mut running: Option<Model> = None;
    loop {
        let (generation, job) = tokio::select! {
            biased;
            changed = changes.changed() => {
                stop(&mut running).await;
                if changed.is_err() { return }
                continue;
            }
            job = jobs.recv() => match job {
                Some(job) => (job.observation.generation, Some(job)),
                None => { stop(&mut running).await; return; }
            },
            changed = warming.changed() => {
                if changed.is_err() { stop(&mut running).await; return; }
                let Some(generation) = *warming.borrow_and_update() else { continue };
                if running.is_some() { continue; }
                (generation, None)
            }
        };
        if generation != *changes.borrow() {
            continue;
        }
        let calculate = async {
            if running.is_none() {
                // Store the child before waiting so timeout/reset can kill and reap it.
                let new = Model::start(&program, &model)?;
                let new = running.insert(new);
                if new.line().await? != "ready" {
                    return Err(io::Error::other("model startup handshake"));
                }
            }
            match &job {
                Some(job) => running.as_mut().unwrap().ask(job).await.map(Some),
                None => Ok(None),
            }
        };
        let prediction = tokio::select! {
            biased;
            changed = changes.changed() => {
                stop(&mut running).await;
                if changed.is_err() { return }
                continue;
            }
            result = timeout(Duration::from_secs(10), calculate) => result,
        };
        let ready = lamp_ipc::monotonic_us();
        let (outcome, score, ready_us) = match prediction {
            Ok(Ok(Some((outcome, score)))) => (outcome, score, Some(ready)),
            Ok(Ok(None)) => continue, // We loaded weights without sending a pair.
            _ => {
                stop(&mut running).await;
                ("unknown", None, None)
            }
        };
        if let Some(job) = job {
            let _ = results.try_send(ResultRow {
                observation: job.observation,
                outcome,
                score,
                ready_us,
                verdict_us: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_are_numeric_and_bounded() {
        assert_eq!(parse("same 0.94"), Some(("same", Some(0.94))));
        assert_eq!(parse("different 0.02"), Some(("different", Some(0.02))));
        assert_eq!(parse("unknown"), Some(("unknown", None)));
        for bad in [
            "same NaN",
            "same -0.03",
            "different 1.1",
            "same",
            "same 0.3 4",
            "oops",
        ] {
            assert_eq!(parse(bad), None);
        }
    }
}
