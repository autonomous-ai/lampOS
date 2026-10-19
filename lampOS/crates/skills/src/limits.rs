//! One bounded background client for declarative skill output ceilings.
//! Actions is the durable authority. Its accepted snapshot is read after a
//! restart, so exact tokens are removed and a timed restriction is never
//! confused with a previous person's mixer setting.
use crate::{Skill, daily::Moment, routine::Limits};
use lamp_local_time::LocalTime;
use lamp_msgs::OutputCap;
use serde_json::{Value, json};
use std::{
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, mpsc},
    time::{Duration, Instant, SystemTime},
};

const MOST: usize = 8;
const INTERVAL: Duration = Duration::from_secs(1);
const TIMEOUT: Duration = Duration::from_secs(3);
#[derive(Clone, Debug, PartialEq, Eq)]
struct Definition {
    source: String,
    limits: Limits,
    // Captured with the accepted definition; checked again at caps_put.
    output_limits: bool,
}
type Definitions = Vec<Definition>;

#[derive(Default)]
struct Latest {
    generation: Arc<()>,
    pending: Option<Definitions>,
}

struct Batch {
    definitions: Definitions,
    fence: Fence,
}

struct Fence {
    shared: Arc<Mutex<Latest>>,
    generation: Arc<()>,
}

impl Fence {
    fn acquire(&self) -> io::Result<MutexGuard<'_, Latest>> {
        let latest = self
            .shared
            .lock()
            .map_err(|_| io::Error::other("output limit definitions are unavailable"))?;
        if !Arc::ptr_eq(&latest.generation, &self.generation) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "output limit definitions changed",
            ));
        }
        Ok(latest)
    }
}

pub fn source(skill: &str) -> Result<String, String> {
    let name = format!("skill-{skill}");
    if name.len() > 64
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-".contains(&b))
    {
        return Err("a skill with limits needs a lowercase ASCII name of at most 58 bytes".into());
    }
    Ok(name)
}

/// Latest definitions replace earlier ones; there is no unbounded reload
/// queue. Socket/mixer delays stay off the Skills observation loop.
#[derive(Clone)]
pub struct Client(Arc<Mutex<Latest>>);

impl Client {
    pub fn start(path: PathBuf, local: Arc<LocalTime>) -> io::Result<Self> {
        let client = Self(Arc::default());
        let worker = client.clone();
        std::thread::Builder::new()
            .name("skill-limits".into())
            .spawn(move || {
                let (_send, wait) = mpsc::channel::<()>();
                let mut batch = None;
                let mut last_error = None;
                loop {
                    if let Some(new) = worker.take() {
                        batch = Some(new);
                    }
                    if let Some(batch) = &batch {
                        let now = SystemTime::now()
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .map_or(0, |time| time.as_millis() as u64);
                        let result = reconcile(&path, batch, &local, now);
                        let error = result.err().map(|error| error.to_string());
                        if error != last_error
                            && let Some(error) = &error
                        {
                            eprintln!("lamp-skills: output limits: {error}");
                        }
                        last_error = error;
                    }
                    let _ = wait.recv_timeout(INTERVAL);
                }
            })?;
        Ok(client)
    }

    fn take(&self) -> Option<Batch> {
        let mut latest = self.0.lock().ok()?;
        Some(Batch {
            definitions: latest.pending.take()?,
            fence: Fence {
                shared: Arc::clone(&self.0),
                generation: Arc::clone(&latest.generation),
            },
        })
    }

    pub fn replace(&self, skills: &[Skill]) -> Result<(), String> {
        let definitions = skills
            .iter()
            .filter(|skill| skill.enabled)
            .filter_map(|skill| {
                skill.limits.as_ref().map(|limits| {
                    if let Some(why) = skill.permissions.refuses_output_limits() {
                        return Err(why);
                    }
                    Ok(Definition {
                        source: source(&skill.name)?,
                        limits: limits.clone(),
                        output_limits: skill.permissions.output_limits,
                    })
                })
            })
            .collect::<Result<Definitions, _>>()?;
        if definitions.len() > MOST {
            return Err("at most eight skills may own output limits".into());
        }
        let mut latest = self
            .0
            .lock()
            .map_err(|_| "output limit definitions are unavailable")?;
        latest.generation = Arc::new(());
        latest.pending = Some(definitions);
        Ok(())
    }
}

/// End at the first real minute outside this occurrence. Sampling the zone
/// at each candidate keeps a repeated/skipped hour from inventing a second
/// window. Work is bounded to 1,440 samples and a maximum 24-hour lease.
fn end(limits: &Limits, local: &LocalTime, now: u64) -> Option<u64> {
    let moment = Moment::at(local, now).ok()?;
    let day = limits.during.occurrence_day(moment.day, moment.minute)?;
    let next_minute = now.checked_sub(now % 60_000)?.checked_add(60_000)?;
    for minutes in 0..1440_u64 {
        let at = next_minute.checked_add(minutes * 60_000)?;
        let next = Moment::at(local, at).ok()?;
        if limits.during.occurrence_day(next.day, next.minute) != Some(day) {
            return Some(at);
        }
    }
    None
}

fn reconcile(path: &Path, batch: &Batch, local: &LocalTime, now: u64) -> io::Result<()> {
    let deadline = Instant::now() + TIMEOUT;
    let answer = exchange(path, &json!({"op":"caps_get"}), deadline)?;
    // A reload can complete while caps_get is waiting. Only the accepted
    // generation may act on its answer, including exact-token removals.
    drop(batch.fence.acquire()?);
    let definitions = &batch.definitions;
    let active: Vec<OutputCap> =
        serde_json::from_value(answer["snapshot"]["active"].clone()).map_err(io::Error::other)?;
    if active.len() > MOST || active.iter().any(|cap| cap.check().is_err()) {
        return Err(io::Error::other("invalid accepted cap snapshot"));
    }
    let moment = Moment::at(local, now).ok();
    // One mutation per iteration bounds time even with eight sources. A
    // restart sees the exact accepted token; a lost reply needs no guess.
    for cap in active.iter().filter(|cap| cap.source.starts_with("skill-")) {
        let definition = definitions
            .iter()
            .find(|definition| definition.output_limits && definition.source == cap.source);
        let remove = definition.is_none()
            || definition.is_some_and(|definition| {
                now >= cap.issued_unix_ms
                    && moment
                        .is_some_and(|moment| !definition.limits.during.contains(moment.minute))
            });
        if remove {
            mutate(
                path,
                &json!({"op":"caps_remove","source":cap.source,"token":cap.token}),
                deadline,
                &batch.fence,
            )?;
            return Ok(());
        }
    }
    for definition in definitions {
        let Definition {
            source,
            limits,
            output_limits,
        } = definition;
        if moment.is_none_or(|moment| !limits.during.contains(moment.minute)) {
            continue;
        }
        if !*output_limits {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "caps_put requires output_limits permission",
            ));
        }
        // The durable end is sufficient while this same limit remains
        // active. Recompute only on entry/replacement, not 1,440 zone
        // samples every second. A changed local window still removes the
        // cap above as soon as it ends; expiry permits a new one if needed.
        if active.iter().any(|cap| {
            cap.source == *source
                && cap.valid_until_unix_ms > now
                && cap.light_channel_max == limits.light_channel_max
                && cap.volume_raw_max == limits.volume_raw_max
        }) {
            continue;
        }
        let Some(until) = end(limits, local, now) else {
            continue;
        };
        let cap = OutputCap {
            source: source.clone(),
            token: format!("window-{now}"),
            issued_unix_ms: now,
            valid_until_unix_ms: until,
            light_channel_max: limits.light_channel_max,
            volume_raw_max: limits.volume_raw_max,
        };
        cap.check().map_err(io::Error::other)?;
        mutate(
            path,
            &json!({"op":"caps_put","cap":cap}),
            deadline,
            &batch.fence,
        )?;
        return Ok(());
    }
    Ok(())
}

/// Absolute deadline and bounded bytes, including a peer that trickles a
/// line. No model text, public tool name or presentation permission enters.
fn exchange(path: &Path, request: &Value, deadline: Instant) -> io::Result<Value> {
    let answer =
        lamp_schedule::client::exchange(path, request, deadline).map_err(io::Error::other)?;
    accepted(answer)
}

fn mutate(path: &Path, request: &Value, deadline: Instant, fence: &Fence) -> io::Result<Value> {
    let answer =
        lamp_schedule::client::exchange_guarded(path, request, deadline, || fence.acquire().ok())
            .map_err(io::Error::other)?;
    accepted(answer)
}

fn accepted(answer: Value) -> io::Result<Value> {
    if answer["ok"] != true {
        return Err(io::Error::other("Actions did not accept the cap operation"));
    }
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daily::Window;
    fn limits(from: &str, to: &str) -> Limits {
        Limits {
            during: Window::parse(from, to).unwrap(),
            light_channel_max: Some(24),
            volume_raw_max: Some(35),
        }
    }

    fn definition(source: &str) -> Definition {
        Definition {
            source: source.into(),
            limits: limits("22:00", "06:00"),
            output_limits: true,
        }
    }

    fn capped_skill() -> Skill {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("routine");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(
            folder.join("SKILL.md"),
            "---\nname: routine\ndescription: A bounded cap fixture.\n---\n",
        )
        .unwrap();
        std::fs::write(
            folder.join("skill.toml"),
            "[trigger]\ntopic = \"voice/state\"\nfield = \"state\"\nequals = \"resting\"\n[ask]\npolicy = \"nudge\"\n[permissions]\ntopics = [\"voice/state\"]\nbody = true\noutput_limits = true\n[limits]\nduring = { from = \"22:00\", to = \"06:00\" }\nlight_channel_max = 24\nvolume_raw_max = 35\n[[do]]\nrest = { op = \"cancel\" }\n",
        )
        .unwrap();
        let mut skill = crate::load(&folder).unwrap();
        // This client installs only caps: it does not borrow the separate
        // presentation grants that validated the fixture's Rest step.
        skill.permissions = crate::Permissions {
            output_limits: true,
            ..crate::Permissions::default()
        };
        skill
    }

    #[test]
    fn rejected_batches_preserve_the_accepted_definitions_and_generation() {
        let client = Client(Arc::default());
        let mut skill = capped_skill();
        client.replace(&[skill.clone()]).unwrap();
        let accepted = client.0.lock().unwrap().pending.clone();
        let generation = Arc::clone(&client.0.lock().unwrap().generation);
        assert_eq!(accepted, Some(vec![definition("skill-routine")]));
        let mut refused = skill.clone();
        refused.name = "other".into();
        refused.permissions.output_limits = false;
        assert!(
            client
                .replace(&[skill.clone(), refused])
                .unwrap_err()
                .contains("output_limits = true")
        );
        let too_many = (0..=MOST)
            .map(|index| {
                let mut skill = skill.clone();
                skill.name = format!("cap-{index}");
                skill
            })
            .collect::<Vec<_>>();
        assert!(client.replace(&too_many).unwrap_err().contains("eight"));
        {
            let latest = client.0.lock().unwrap();
            assert_eq!(latest.pending, accepted);
            assert!(Arc::ptr_eq(&latest.generation, &generation));
        }
        let batch = client.take().unwrap();
        assert!(batch.fence.acquire().is_ok());
        // Explicit disable removes the accepted definition even when its
        // old grant has been revoked; the worker can clean up its token.
        skill.enabled = false;
        skill.permissions.output_limits = false;
        client.replace(&[skill]).unwrap();
        assert_eq!(client.0.lock().unwrap().pending, Some(vec![]));
        assert_eq!(
            batch.fence.acquire().err().unwrap().kind(),
            io::ErrorKind::PermissionDenied
        );
    }
    #[test]
    fn end_is_exclusive_and_unknown_time_creates_no_cap() {
        let local = LocalTime::Offset(0);
        let midnight = 1_767_225_600_000;
        let limit = limits("22:00", "06:00");
        assert_eq!(end(&limit, &local, 0), None);
        assert_eq!(end(&limit, &local, midnight + 21 * 3_600_000), None);
        assert_eq!(
            end(&limit, &local, midnight + 23 * 3_600_000),
            Some(midnight + 30 * 3_600_000)
        );
        assert_eq!(end(&limit, &local, midnight + 6 * 3_600_000), None);
    }
    #[test]
    fn spring_and_autumn_windows_end_in_local_time() {
        let local = LocalTime::zone("America/New_York").unwrap();
        let limit = limits("00:00", "04:00");
        // 2026-03-08 00:00 EST; 04:00 EDT is three real hours later.
        let spring = 1_772_946_000_000;
        assert_eq!(end(&limit, &local, spring), Some(spring + 3 * 3_600_000));
        // 2026-11-01 00:00 EDT; 04:00 EST is five real hours later.
        let autumn = 1_793_505_600_000;
        assert_eq!(end(&limit, &local, autumn), Some(autumn + 5 * 3_600_000));
    }

    fn round(
        active: Vec<OutputCap>,
        definitions: Definitions,
        now: u64,
        mutations: usize,
    ) -> Vec<Value> {
        let (requests, result) = round_result(active, definitions, now, mutations);
        result.unwrap();
        requests
    }

    fn round_result(
        active: Vec<OutputCap>,
        definitions: Definitions,
        now: u64,
        mutations: usize,
    ) -> (Vec<Value>, io::Result<()>) {
        use std::{
            io::{BufRead, BufReader, Read, Write},
            os::unix::net::UnixListener,
        };
        let root = tempfile::Builder::new()
            .prefix("caps-")
            .tempdir_in("/tmp")
            .unwrap();
        let path = root.path().join("call.sock");
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            let deadline = Instant::now() + Duration::from_secs(4);
            while requests.len() <= mutations {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_read_timeout(Some(Duration::from_secs(1)))
                            .unwrap();
                        let mut line = String::new();
                        BufReader::new(&stream)
                            .take(8193)
                            .read_line(&mut line)
                            .unwrap();
                        let request: Value = serde_json::from_str(&line).unwrap();
                        if request["op"] == "caps_get" {
                            writeln!(
                                stream,
                                "{}",
                                json!({"ok":true,"snapshot":{"active":active}})
                            )
                            .unwrap();
                        } else {
                            writeln!(stream, "{{\"ok\":true,\"accepted\":true}}").unwrap();
                        }
                        requests.push(request);
                    }
                    Err(error)
                        if error.kind() == io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_millis(1))
                    }
                    Err(error) => panic!("cap fixture did not receive request: {error}"),
                }
            }
            requests
        });
        let client = Client(Arc::new(Mutex::new(Latest {
            pending: Some(definitions),
            ..Latest::default()
        })));
        let result = reconcile(&path, &client.take().unwrap(), &LocalTime::Offset(0), now);
        (server.join().unwrap(), result)
    }

    fn restored() -> OutputCap {
        OutputCap {
            source: "skill-routine".into(),
            token: "kept-token".into(),
            issued_unix_ms: 1_767_225_600_000 + 23 * 3_600_000,
            valid_until_unix_ms: 1_767_225_600_000 + 30 * 3_600_000,
            light_channel_max: Some(24),
            volume_raw_max: Some(35),
        }
    }

    #[test]
    fn restart_recovers_the_exact_token_and_unknown_or_older_time_keeps_it() {
        let cap = restored();
        let definitions = vec![definition(&cap.source)];
        for now in [
            0,
            cap.issued_unix_ms - 4 * 3_600_000,
            cap.issued_unix_ms + 1,
        ] {
            assert_eq!(
                round(vec![cap.clone()], definitions.clone(), now, 0).len(),
                1
            );
        }
        let request = round(vec![cap.clone()], vec![], 0, 1).pop().unwrap();
        assert_eq!(
            request,
            json!({"op":"caps_remove","source":cap.source,"token":cap.token}),
            "explicit disable works with unknown clock"
        );
    }

    #[test]
    fn window_end_removes_only_its_owned_token_and_entry_uses_private_put() {
        let cap = restored();
        let definitions = vec![definition(&cap.source)];
        let request = round(
            vec![cap.clone()],
            definitions.clone(),
            cap.valid_until_unix_ms,
            1,
        )
        .pop()
        .unwrap();
        assert_eq!(request["op"], "caps_remove");
        assert_eq!(request["token"], cap.token);
        let request = round(vec![], definitions, cap.issued_unix_ms, 1)
            .pop()
            .unwrap();
        assert_eq!(request["op"], "caps_put");
        assert_eq!(
            request["cap"]["valid_until_unix_ms"],
            cap.valid_until_unix_ms
        );
        assert_eq!(request["cap"]["volume_raw_max"], 35);
    }

    #[test]
    fn the_final_cap_boundary_refuses_a_revoked_put_but_keeps_exact_token_cleanup() {
        let cap = restored();
        let mut revoked = definition(&cap.source);
        revoked.output_limits = false;
        let (requests, result) = round_result(vec![], vec![revoked.clone()], cap.issued_unix_ms, 0);
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(requests, [json!({"op":"caps_get"})]);
        let cleanup = round(vec![cap.clone()], vec![revoked], 0, 1);
        assert_eq!(
            cleanup.last().unwrap(),
            &json!({"op":"caps_remove","source":cap.source,"token":cap.token})
        );
    }

    #[test]
    fn reload_while_caps_get_is_held_fences_both_put_and_remove() {
        use std::{
            io::{BufRead, BufReader, Write},
            os::unix::net::UnixListener,
        };

        let skill = capped_skill();
        let cap = restored();
        for (before, after, active) in [
            (vec![skill.clone()], vec![], vec![]),
            (vec![], vec![skill.clone()], vec![cap.clone()]),
        ] {
            let root = tempfile::Builder::new()
                .prefix("caps-")
                .tempdir_in("/tmp")
                .unwrap();
            let path = root.path().join("call.sock");
            let listener = UnixListener::bind(&path).unwrap();
            let client = Client(Arc::default());
            client.replace(&before).unwrap();
            let batch = client.take().unwrap();
            let now = cap.issued_unix_ms;
            let worker =
                std::thread::spawn(move || reconcile(&path, &batch, &LocalTime::Offset(0), now));
            let (mut peer, _) = listener.accept().unwrap();
            peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
            let mut line = String::new();
            BufReader::new(&peer).read_line(&mut line).unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&line).unwrap(),
                json!({"op":"caps_get"})
            );

            // Reload must finish while the old get is still waiting. A
            // lock held across the exchange fails this bound.
            let (send, receive) = mpsc::sync_channel(1);
            let reload = client.clone();
            let reloader = std::thread::spawn(move || {
                send.send(reload.replace(&after)).unwrap();
            });
            receive
                .recv_timeout(Duration::from_secs(1))
                .expect("reload waited for the held socket")
                .unwrap();
            reloader.join().unwrap();
            writeln!(peer, "{}", json!({"ok":true,"snapshot":{"active":active}})).unwrap();
            let error = worker.join().unwrap().unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
            listener.set_nonblocking(true).unwrap();
            assert_eq!(
                listener.accept().unwrap_err().kind(),
                io::ErrorKind::WouldBlock
            );
            assert!(client.take().unwrap().fence.acquire().is_ok());
        }
    }
}
