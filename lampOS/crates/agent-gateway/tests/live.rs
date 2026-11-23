//! Live check of [`openclaw::Session`] against a real, isolated OpenClaw
//! gateway (never the product's own on 18789 unless pointed there).
//!
//! Ignored by default. Driven by environment:
//!
//! - `OCGW_LIVE_URL`: the gateway's WebSocket URL (`ws://127.0.0.1:28789`).
//! - `OCGW_LIVE_TOKEN`: its shared token (written to a 0600 temp file for
//!   the session, never logged).
//! - `OCGW_LIVE_TERM_CMD`, `OCGW_LIVE_START_CMD`: shell commands that stop
//!   the gateway with SIGTERM and start it again, for the restart case;
//!   without them that case is skipped.
//! - `OCGW_LIVE_CASES`: comma-separated subset of `a,b,c,d,e,f` (default
//!   all, in that order).
//! - `OCGW_LIVE_STUB`: `1` when the model is the trace stub, whose replies
//!   follow keywords (`tool`, `slowtool`, `slow`, `hang`). With a real model
//!   the hang case is skipped and the tool case asks in plain words.
//!
//! Run: `cargo test -p lamp-agent-gateway --test live -- --ignored --nocapture`.

use std::io::Write as _;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use lamp_agent_gateway::openclaw::{Config, Session};
use lamp_agent_gateway::{AgentSession, Error, Outcome, Phase, RunId, Turn, TurnKind};

struct Live {
    session: Session,
    stub: bool,
    term_cmd: Option<String>,
    start_cmd: Option<String>,
    t0: Instant,
    n: u32,
    // Keeps the token file alive for the session's life.
    _token: tempfile::NamedTempFile,
}

fn say(t0: Instant, line: &str) {
    eprintln!("[live {:>8.3}s] {line}", t0.elapsed().as_secs_f64());
}

impl Live {
    fn turn(&mut self, message: &str) -> Turn {
        self.n += 1;
        Turn {
            task_id: format!("live{}", self.n),
            message: message.to_owned(),
            kind: TurnKind::Spoken,
        }
    }

    fn say(&self, line: &str) {
        say(self.t0, line);
    }

    /// Sends `message` and waits for its end, polling progress every
    /// 20 ms. Returns the outcome and every distinct phase seen.
    fn run(
        &mut self,
        message: &str,
        within: Duration,
    ) -> Result<(RunId, Outcome, Vec<String>), String> {
        let turn = self.turn(message);
        let sent = Instant::now();
        let run = self.session.send(&turn).map_err(|e| format!("send: {e}"))?;
        self.say(&format!(
            "sent {:?} as {} in {} ms",
            message,
            run.0,
            sent.elapsed().as_millis()
        ));
        let deadline = Instant::now() + within;
        let mut seen: Vec<String> = Vec::new();
        let mut snap_at = Instant::now();
        loop {
            if snap_at.elapsed() >= Duration::from_secs(2) {
                snap_at = Instant::now();
                if let Some(p) = self.session.progress(&run) {
                    self.say(&format!(
                        "  snapshot +{} ms: phase {:?}, tools_done {}, last_tool {:?}, last_text {:?}",
                        sent.elapsed().as_millis(),
                        p.phase,
                        p.tools_done,
                        p.last_tool,
                        p.last_text.as_deref().map(|t| t.chars().take(80).collect::<String>())
                    ));
                }
            }
            if let Some(p) = self.session.progress(&run) {
                let tag = match &p.phase {
                    Phase::Tool => format!(
                        "Tool({})",
                        p.last_tool.as_ref().map(|t| t.name.as_str()).unwrap_or("?")
                    ),
                    other => format!("{other:?}"),
                };
                let tag = format!("{tag} tools_done={}", p.tools_done);
                if seen.last() != Some(&tag) {
                    self.say(&format!(
                        "  progress {} after {} ms",
                        tag,
                        sent.elapsed().as_millis()
                    ));
                    seen.push(tag);
                }
            }
            match self.session.wait(&run, Duration::from_millis(20)) {
                Ok(outcome) => {
                    self.say(&format!(
                        "  ended after {} ms: {}",
                        sent.elapsed().as_millis(),
                        brief(&outcome)
                    ));
                    self.say(&format!(
                        "  session_tokens {}",
                        self.session.session_tokens()
                    ));
                    return Ok((run, outcome, seen));
                }
                Err(Error::TimedOut) if Instant::now() < deadline => {}
                Err(e) => return Err(format!("wait: {e}")),
            }
        }
    }

    fn answered(&mut self, message: &str, within: Duration) -> Result<String, String> {
        match self.run(message, within)? {
            (_, Outcome::Answered { text, .. }, _) if !text.trim().is_empty() => Ok(text),
            (_, other, _) => Err(format!("not answered: {}", brief(&other))),
        }
    }
}

fn brief(o: &Outcome) -> String {
    match o {
        Outcome::Answered { text, progress } => format!(
            "Answered {:?} (tools_done={})",
            text.chars().take(120).collect::<String>(),
            progress.tools_done
        ),
        Outcome::Failed { why, .. } => format!("Failed {why:?}"),
        Outcome::Aborted { .. } => "Aborted".to_owned(),
    }
}

fn shell(cmd: &str) -> Result<String, String> {
    let out = Command::new("/bin/sh")
        .arg("-c")
        .arg(cmd)
        .output()
        .map_err(|e| format!("{cmd}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{cmd}: {} {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn case_a(l: &mut Live) -> Result<(), String> {
    let t = Instant::now();
    l.session
        .ready(Duration::from_secs(180))
        .map_err(|e| format!("ready: {e}"))?;
    l.say(&format!(
        "ready in {} ms, server {:?}, key {}",
        t.elapsed().as_millis(),
        l.session.server_version(),
        l.session.session_key()
    ));
    let t = Instant::now();
    l.session
        .ready(Duration::from_secs(1))
        .map_err(|e| format!("second ready: {e}"))?;
    l.say(&format!("second ready in {} ms", t.elapsed().as_millis()));
    Ok(())
}

fn case_b(l: &mut Live) -> Result<(), String> {
    let text = l.answered("say hi", Duration::from_secs(120))?;
    l.say(&format!("answer: {text:?}"));
    Ok(())
}

fn case_c(l: &mut Live) -> Result<(), String> {
    let ask = if l.stub {
        "zzslowtool: run the exec tool"
    } else {
        "List the files in /tmp/ocgw-trace and count them, using the shell."
    };
    let (_, outcome, seen) = l.run(ask, Duration::from_secs(180))?;
    let tool_at = seen.iter().position(|s| s.starts_with("Tool("));
    let done_at = seen.iter().position(|s| !s.ends_with("tools_done=0"));
    let Outcome::Answered { text, progress } = &outcome else {
        return Err(format!("not answered: {}", brief(&outcome)));
    };
    l.say(&format!(
        "phases {seen:?}; last_tool {:?}",
        progress.last_tool
    ));
    match (tool_at, done_at) {
        (Some(t), Some(d)) if t <= d && progress.tools_done >= 1 && !text.trim().is_empty() => {
            Ok(())
        }
        _ => Err(format!(
            "expected Tool then tools_done>=1 then an answer; saw {seen:?}, tools_done {}",
            progress.tools_done
        )),
    }
}

fn case_d(l: &mut Live) -> Result<(), String> {
    let (Some(term), Some(start)) = (l.term_cmd.clone(), l.start_cmd.clone()) else {
        return Err("SKIP: no OCGW_LIVE_TERM_CMD / OCGW_LIVE_START_CMD".to_owned());
    };
    let ask = if l.stub {
        "zzslow reply please"
    } else {
        "Count slowly from one to twenty, one number per line."
    };
    let turn = l.turn(ask);
    let run = l.session.send(&turn).map_err(|e| format!("send: {e}"))?;
    l.say(&format!("sent slow run {}", run.0));
    // TERM once the run is writing (or after 15 s at the latest).
    let until = Instant::now() + Duration::from_secs(15);
    while Instant::now() < until {
        if matches!(
            l.session.progress(&run).map(|p| p.phase),
            Some(Phase::Writing | Phase::Tool)
        ) {
            break;
        }
        let _ = l.session.wait(&run, Duration::from_millis(50));
    }
    l.say(&format!(
        "TERM the gateway; run phase {:?}",
        l.session.progress(&run).map(|p| p.phase)
    ));
    shell(&term)?;
    let mut first = None;
    match l.session.wait(&run, Duration::from_secs(60)) {
        Ok(o) => {
            l.say(&format!("run across TERM: {}", brief(&o)));
            first = Some(o);
        }
        Err(e) => l.say(&format!("run across TERM: wait said {e}")),
    }
    let pid = shell(&start)?;
    l.say(&format!("gateway started again (pid {pid})"));
    let t = Instant::now();
    let mut tries = 0;
    loop {
        tries += 1;
        match l.session.ready(Duration::from_secs(5)) {
            Ok(()) => break,
            Err(e) if t.elapsed() < Duration::from_secs(600) => {
                // `ready` answers at once while a backoff runs past its
                // deadline: pace the loop so it never spins.
                if tries % 20 == 1 {
                    l.say(&format!("  ready: {e}"));
                }
                std::thread::sleep(Duration::from_millis(500));
            }
            Err(e) => return Err(format!("never ready again: {e}")),
        }
    }
    l.say(&format!(
        "ready again {} ms after start ({tries} tries)",
        t.elapsed().as_millis()
    ));
    if first.is_none() {
        match l.session.wait(&run, Duration::from_secs(10)) {
            Ok(o) => l.say(&format!("run after reconnect: {}", brief(&o))),
            Err(e) => l.say(&format!("run after reconnect: wait said {e}")),
        }
    }
    let text = l.answered("say hi", Duration::from_secs(180))?;
    l.say(&format!("answer after restart: {text:?}"));
    Ok(())
}

fn case_e(l: &mut Live) -> Result<(), String> {
    if !l.stub {
        return Err("SKIP: needs the stub's hang mode".to_owned());
    }
    let turn = l.turn("zzhang forever please");
    let run = l.session.send(&turn).map_err(|e| format!("send: {e}"))?;
    l.say(&format!("sent hanging run {}", run.0));
    let quiet = Duration::from_secs(5);
    let t = Instant::now();
    let mut stuck = false;
    while t.elapsed() < Duration::from_secs(90) {
        let _ = l.session.wait(&run, Duration::from_secs(1));
        // lamp-tasks' rule: no event from the gateway for `quiet` is stuck.
        stuck = l
            .session
            .progress(&run)
            .and_then(|p| p.updated_at.elapsed().ok())
            .is_some_and(|since| since >= quiet);
        if stuck {
            break;
        }
    }
    let p = l.session.progress(&run);
    l.say(&format!(
        "stuck={stuck} after {} ms; phase {:?}",
        t.elapsed().as_millis(),
        p.map(|p| p.phase)
    ));
    if !stuck {
        // The gateway still holds the run in flight: abort it by hand so
        // the session can go on, and report the miss.
        l.session.abort(&run).map_err(|e| format!("abort: {e}"))?;
        l.say("not detected as stuck within 90 s; aborted by hand");
    } else {
        l.session.abort(&run).map_err(|e| format!("abort: {e}"))?;
    }
    let outcome = l.session.wait(&run, Duration::from_secs(5));
    l.say(&format!(
        "hanging run now: {:?}",
        outcome.as_ref().map(brief)
    ));
    let text = l.answered("say hi", Duration::from_secs(120))?;
    l.say(&format!("next answer: {text:?}"));
    if stuck {
        Ok(())
    } else {
        Err("a run that hung for 90 s never went quiet".to_owned())
    }
}

fn case_f(l: &mut Live) -> Result<(), String> {
    let before = l.session.session_tokens();
    l.session.reset().map_err(|e| format!("reset: {e}"))?;
    l.say(&format!(
        "reset ok (tokens {before} -> {})",
        l.session.session_tokens()
    ));
    let text = l.answered("say hi", Duration::from_secs(120))?;
    l.say(&format!("answer after reset: {text:?}"));
    Ok(())
}

#[test]
#[ignore = "needs a live OpenClaw gateway (OCGW_LIVE_URL, OCGW_LIVE_TOKEN)"]
fn live_gateway() {
    let url = std::env::var("OCGW_LIVE_URL").expect("OCGW_LIVE_URL");
    let token = std::env::var("OCGW_LIVE_TOKEN").expect("OCGW_LIVE_TOKEN");
    let mut file = tempfile::NamedTempFile::new().expect("a temp file");
    file.write_all(token.as_bytes()).expect("token written");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(file.path(), std::fs::Permissions::from_mode(0o600))
            .expect("0600");
    }
    let t0 = Instant::now();
    let cfg = Config {
        url,
        token_path: file.path().to_owned(),
        ..Config::default()
    };
    let session = Session::new(cfg).with_logger(Arc::new(move |line: &str| {
        say(t0, &format!("session: {line}"));
    }));
    let mut live = Live {
        session,
        stub: std::env::var("OCGW_LIVE_STUB").as_deref() == Ok("1"),
        term_cmd: std::env::var("OCGW_LIVE_TERM_CMD").ok(),
        start_cmd: std::env::var("OCGW_LIVE_START_CMD").ok(),
        t0,
        n: 0,
        _token: file,
    };
    let cases = std::env::var("OCGW_LIVE_CASES").unwrap_or_else(|_| "a,b,c,d,e,f".to_owned());
    type Case = fn(&mut Live) -> Result<(), String>;
    let all: [(&str, &str, Case); 6] = [
        ("a", "ready", case_a),
        ("b", "say hi answered", case_b),
        ("c", "tool progress then answer", case_c),
        ("d", "TERM mid-run, restart, answered", case_d),
        ("e", "hang detected by stuck, abort, answered", case_e),
        ("f", "reset then answered", case_f),
    ];
    let mut failed = Vec::new();
    for (id, what, case) in all {
        if !cases.split(',').any(|c| c.trim() == id) {
            continue;
        }
        if id != "a" {
            // Every case starts connected.
            let _ = live.session.ready(Duration::from_secs(180));
        }
        live.say(&format!("=== case {id}: {what}"));
        match case(&mut live) {
            Ok(()) => live.say(&format!("=== case {id}: PASS")),
            Err(why) if why.starts_with("SKIP") => live.say(&format!("=== case {id}: {why}")),
            Err(why) => {
                live.say(&format!("=== case {id}: FAIL: {why}"));
                failed.push(format!("{id}: {why}"));
            }
        }
    }
    assert!(failed.is_empty(), "live cases failed: {failed:#?}");
}
