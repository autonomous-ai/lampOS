//! Each action routed to the part of the lamp that already does it
//! (docs/control.md, "Routing"), held to that part's own limits:
//!
//! - `gesture` and `move` → the body's mood socket, as lamp-skills sends
//!   them (`lamp_skills::body_ask`), at `skill` priority with the source
//!   `control:<client>`. The body plays them with its own move check, reach,
//!   speed and anti-tip limits, or drops them with why in its journal.
//!   The body plays one at a time, so the door remembers when the last
//!   motion it sent ends, whoever asked, and refuses a `gesture`, `move` or
//!   `express` move asked before then as `busy`, with `retry_ms` (#4209):
//!   the drop the body would make, said to the computer. A length the door
//!   can't know (a move folder's gesture) holds nothing.
//! - `light` → a `light/request` on the bus at `skill` priority, lapsing at
//!   its `for_ms` unless asked again, the ring's own night caps on top.
//!   `stop` gives the light back at once.
//! - `say` → the voice's say socket as written words (`text`), through its
//!   quiet, privacy, pause and busy gates. Its outcome comes later as a
//!   `done` event: spoken, dropped, expired or refused, with why. The
//!   client's `stop` cancels each of its own says on the connection that
//!   asked it ([`SAY_CANCEL`]): waiting, the voice lets it go; playing, the
//!   voice cuts that turn alone. Its `done` is then dropped, "stopped by its
//!   asker". Another client's say, the person's turn and the voice's own
//!   words are never reached.
//! - `express` → the `feelings` skill's own move for it, sent whole on the
//!   mood socket as `move` is, and its light as `light` is
//!   (`crate::feelings`). With no light to show (off, not known, or at its
//!   dimmest with no warmth to add) the move goes alone, and the answer says
//!   why.
//!
//! The priority is this part's, never the caller's: a client cannot claim
//! more than `skill`. With the parts' own caller check (#3720, Ten's caps)
//! the body and the ring hold `lamp-control` to `skill` as well.
//!
//! - `look` at `ahead` or `{yaw, pitch}` → the body's lease socket, under
//!   the lease the door holds for the computer (`crate::body_lease`); its
//!   end comes later as a `done` event, with the angles the head reached.
//!
//! `look` at `face` and at `name` are not in this release, and refused as
//! that. Not routed yet, each refused with what it waits for: `joints` (a
//! goal stream on the lease socket), `play` and `sound` (a client's package,
//! with the Store's packaging). State and events from the
//! bus come in their own piece.

use std::collections::HashMap;
use std::io::{BufReader, Write as _};
use std::os::unix::net::{UnixDatagram, UnixStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use lamp_msgs::{Claim, Light, LightPattern, LightRequest, PatternKind, Priority};
use serde_json::{Value, json};

use crate::feelings;
use crate::protocol::{Angles, Error, ErrorCode, LookAt, LookTarget, Op, Pattern, StateKind};
use crate::session::Parts;

/// How long a `say` may wait to begin before the voice lets it go.
pub const SAY_EXPIRES_MS: u64 = 30_000;
/// How long the say socket's answer may take, its turn included.
pub const SAY_ANSWER_WITHIN: Duration = Duration::from_secs(90);
/// The line that cancels a say on the connection that asked it: the
/// voice's `lamp_voice::say::CANCEL_LINE`.
pub const SAY_CANCEL: &str = r#"{"cancel":true}"#;
/// Why a say its client stopped is dropped: the voice's own words for it.
pub const STOPPED_BY_ASKER: &str = "stopped by its asker";
/// The longest the door's lock waits to write a cancel to the voice.
const SAY_CANCEL_WITHIN: Duration = Duration::from_millis(100);

/// The says each client still waits on, by client and then by the say's
/// own number: the connection to the voice once it is open. A say no
/// longer listed was stopped.
type Says = Arc<std::sync::Mutex<HashMap<String, HashMap<u64, Option<UnixStream>>>>>;

/// Where the events of actions that end later go: the client, and the
/// event (`done`).
pub type Events = Arc<dyn Fn(&str, Value) + Send + Sync>;

/// Something a light request can be published on: the bus, or a test's
/// list.
pub trait LightOut: Send {
    /// Publishes it: how many subscribers took it.
    fn publish(&mut self, request: &LightRequest) -> std::io::Result<usize>;
}

impl LightOut for lamp_bus::Publisher {
    fn publish(&mut self, request: &LightRequest) -> std::io::Result<usize> {
        Ok(self
            .publish_json(lamp_ipc::monotonic_us(), request)?
            .delivered)
    }
}

/// `look` at `face` and at `name`: out of this release.
pub const NOT_IN_THIS_RELEASE: &str = "not in this release: look at a face or a name";

/// The lamp's parts, as `lamp-control` reaches them.
pub struct Lamp {
    pub body: Option<PathBuf>,
    pub light: Option<Box<dyn LightOut>>,
    pub say: Option<PathBuf>,
    /// The boot's id, which a light request's clock names.
    pub clock_id: String,
    pub events: Events,
    /// Where a `look` goes: the body's lease socket, through the door's
    /// hold of it (`--lease-socket`).
    looks: Option<crate::body_lease::Looks>,
    /// Each client's light still asked for, to give back at its `stop`.
    lights: HashMap<String, (Light, u64)>,
    /// What the bus says, for `get` (docs/control.md, "Events").
    watch: Option<Arc<std::sync::Mutex<crate::watch::Watch>>>,
    datagram: Option<UnixDatagram>,
    /// The boot's monotonic clock (`CLOCK_MONOTONIC`), the one a claim's
    /// `until_us` is read against: not the door's own, which starts with
    /// lamp-control (the Reviewer-lead's P2 on #3805).
    clock: fn() -> u64,
    /// Whether the body follows the hand in view while the lease holds,
    /// told to it on the lease socket (`crate::body_lease`); none without
    /// one.
    follow: Option<crate::body_lease::Follower>,
    /// When the last motion sent to the body ends, on the door's clock.
    /// One for the lamp, since the body plays one at a time whoever asked
    /// (#4209). A `stop` doesn't free it: the door never stops the body,
    /// which plays the motion to its end.
    motion: Option<u64>,
    /// Each client's says still waiting on the voice, for its `stop`.
    says: Says,
    /// The number the next say is kept under.
    next_say: u64,
}

impl Lamp {
    pub fn new(
        body: Option<PathBuf>,
        light: Option<Box<dyn LightOut>>,
        say: Option<PathBuf>,
        clock_id: String,
        events: Events,
    ) -> Self {
        Self {
            body,
            light,
            say,
            clock_id,
            events,
            lights: HashMap::new(),
            looks: None,
            watch: None,
            // Never blocking under the door's lock: a full mood queue is
            // the body busy, said (the Reviewer-lead's P3 on #3805).
            datagram: UnixDatagram::unbound()
                .and_then(|socket| socket.set_nonblocking(true).map(|()| socket))
                .ok(),
            clock: lamp_ipc::monotonic_us,
            follow: None,
            motion: None,
            says: Says::default(),
            next_say: 0,
        }
    }

    /// Another clock for claims, for tests.
    pub fn with_clock(mut self, clock: fn() -> u64) -> Self {
        self.clock = clock;
        self
    }

    /// `look` at `hand` follows it, through the body's lease socket.
    pub fn following(mut self, follow: crate::body_lease::Follower) -> Self {
        self.follow = Some(follow);
        self
    }

    /// Sends a `look` to the body's lease socket through the door's hold.
    pub fn looking(mut self, looks: crate::body_lease::Looks) -> Self {
        self.looks = Some(looks);
        self
    }

    /// Answers `get` from what `watch` keeps of the bus.
    pub fn watching(mut self, watch: Arc<std::sync::Mutex<crate::watch::Watch>>) -> Self {
        self.watch = Some(watch);
        self
    }

    fn to_body(&self, mut ask: Value, client: &str) -> Result<Value, Error> {
        let (Some(path), Some(socket)) = (&self.body, &self.datagram) else {
            return Err(not_here("the body"));
        };
        ask["source"] = json!(source(client));
        ask["priority"] = json!(Priority::Skill);
        socket
            .send_to(ask.to_string().as_bytes(), path)
            .map_err(|error| {
                let why = if error.kind() == std::io::ErrorKind::WouldBlock {
                    "the body is busy: its queue is full".to_owned()
                } else {
                    format!("the body did not take it: {error}")
                };
                Error::new(ErrorCode::Refused, why)
            })?;
        Ok(json!({"to": "body"}))
    }

    fn light(
        &mut self,
        client: &str,
        colour: &str,
        level: u8,
        pattern: Option<Pattern>,
        for_ms: u32,
    ) -> Result<Value, Error> {
        let pattern = pattern.map(|pattern| {
            let kind = match pattern {
                Pattern::Breathe => PatternKind::Breathe,
                Pattern::Candle => PatternKind::Candle,
                Pattern::Pulse => PatternKind::Pulse,
            };
            LightPattern {
                kind,
                period_ms: LightPattern::default_ms(kind),
                low: 1,
            }
        });
        if let Some(pattern) = &pattern {
            pattern
                .check(level)
                .map_err(|invalid| Error::new(ErrorCode::Refused, invalid.0))?;
        }
        let light = Light {
            on: true,
            colour: colour.to_owned(),
            level,
        };
        let until_us = (self.clock)() + u64::from(for_ms) * 1_000;
        self.ask_ring(client, light.clone(), pattern, until_us)?;
        self.lights.insert(client.to_owned(), (light, until_us));
        Ok(json!({"to": "ring"}))
    }

    /// Refused as `busy` while the last motion sent still plays, with when
    /// it ends (#4209).
    fn free_for_motion(&self, now_us: u64) -> Result<(), Error> {
        match &self.motion {
            Some(until_us) if *until_us > now_us => {
                let left_ms = (until_us - now_us).div_ceil(1_000);
                let mut error = Error::new(
                    ErrorCode::Busy,
                    format!("busy: a motion plays for {left_ms} ms more"),
                );
                error.retry_ms = Some(left_ms);
                Err(error)
            }
            _ => Ok(()),
        }
    }

    /// Sends a motion to the body when none still plays, and holds the
    /// slot for its length, if known; a refused one moves nothing.
    fn motion(
        &mut self,
        ask: Value,
        played_us: Option<u64>,
        client: &str,
        now_us: u64,
    ) -> Result<Value, Error> {
        self.free_for_motion(now_us)?;
        let sent = self.to_body(ask, client)?;
        self.motion = played_us.map(|played_us| now_us + played_us);
        Ok(sent)
    }

    fn express(
        &mut self,
        client: &str,
        feeling: &str,
        strength: f32,
        now_us: u64,
    ) -> Result<Value, Error> {
        let feeling =
            feelings::named(feeling).map_err(|why| Error::new(ErrorCode::BadRequest, why))?;
        let mut answer = json!({"to": "body"});
        match feeling
            .scaled(strength)
            .map_err(|why| Error::new(ErrorCode::Refused, why))?
        {
            Some(scaled) => {
                answer["move"] = json!(scaled.name);
                let played_us = played_us(scaled.step_ms.into(), scaled.frames.len());
                self.motion(json!({"move": scaled}), Some(played_us), client, now_us)?;
            }
            None => answer["move"] = Value::Null,
        }
        let now = self.state(StateKind::Light);
        match feeling.light(&now) {
            Ok((light, pattern)) => {
                let until_us = (self.clock)() + u64::from(feeling.lasting_ms) * 1_000;
                match self.ask_ring(client, light.clone(), pattern, until_us) {
                    Ok(()) => {
                        self.lights.insert(client.to_owned(), (light, until_us));
                        answer["light"] = json!(true);
                    }
                    Err(error) => answer["light"] = json!(error.why),
                }
            }
            Err(why) => answer["light"] = json!(why),
        }
        Ok(answer)
    }

    fn look(&self, client: &str, re: u64, at: &LookAt) -> Result<Value, Error> {
        // A look at `hand` starts the body following it (said on each hold);
        // any other look ends that once it is taken, never a refused one
        // (Nine's P3 on #4375: the SDKs end a following only on a taken look).
        let look = match at {
            LookAt::Target(LookTarget::Hand) => {
                let Some(follow) = &self.follow else {
                    return Err(waits_for("the body's lease socket"));
                };
                *follow.lock().expect("follower") = Some(client.to_owned());
                return Ok(json!({"to": "body", "follow": "hand"}));
            }
            LookAt::Angles(Angles { yaw, pitch }) => json!({"yaw": yaw, "pitch": pitch}),
            LookAt::Target(LookTarget::Ahead) => json!("ahead"),
            // Out of this release, said as that (the local leader, 29 Oct).
            LookAt::Target(LookTarget::Face | LookTarget::Name) => {
                return Err(Error::new(ErrorCode::Refused, NOT_IN_THIS_RELEASE));
            }
        };
        let Some(looks) = &self.looks else {
            return Err(Error::new(
                ErrorCode::Refused,
                "the body's lease socket is not on here",
            ));
        };
        looks
            .send(crate::body_lease::Look {
                client: client.to_owned(),
                re,
                line: json!({ "look": look }).to_string(),
            })
            .map_err(|_| not_here("the body's lease"))?;
        if let Some(follow) = &self.follow {
            *follow.lock().expect("follower") = None;
        }
        Ok(json!({"to": "body"}))
    }

    fn ask_ring(
        &mut self,
        client: &str,
        light: Light,
        pattern: Option<LightPattern>,
        until_us: u64,
    ) -> Result<(), Error> {
        let Some(out) = self.light.as_mut() else {
            return Err(not_here("the ring"));
        };
        let request = LightRequest {
            claim: Claim {
                source: source(client),
                priority: Priority::Skill,
                clock_id: self.clock_id.clone(),
                until_us,
            },
            light,
            pattern,
            fade_ms: None,
        };
        match out.publish(&request) {
            Ok(0) => Err(Error::new(
                ErrorCode::Refused,
                "the ring took no light request (LAMP_LIGHT_REQUESTS off?)",
            )),
            Ok(_) => Ok(()),
            Err(error) => Err(Error::new(
                ErrorCode::Refused,
                format!("the ring did not take it: {error}"),
            )),
        }
    }

    fn say(&mut self, client: &str, re: u64, text: &str) -> Result<Value, Error> {
        let Some(path) = self.say.clone() else {
            return Err(not_here("the voice"));
        };
        let line = json!({"skill": "control", "text": text, "expires_in_ms": SAY_EXPIRES_MS});
        let events = self.events.clone();
        let client = client.to_owned();
        // Kept before the thread starts, so that a `stop` right after the
        // reply still reaches it.
        let number = self.next_say;
        self.next_say += 1;
        let says = Arc::clone(&self.says);
        says.lock()
            .expect("says")
            .entry(client.clone())
            .or_default()
            .insert(number, None);
        // Off the door's lock, connecting included (the Reviewer-lead's P3
        // on #3805: a voice whose backlog is full must not hold every
        // connection). The outcome comes when the turn ends, as `done`; a
        // voice that can't be reached is a `done` dropped, with why.
        std::thread::spawn(move || {
            let (how, why) = match ask_voice(&path, &line, &says, &client, number) {
                Ok(Some(stream)) => {
                    let _ = stream.set_read_timeout(Some(SAY_ANSWER_WITHIN));
                    outcome(BufReader::new(stream))
                }
                Ok(None) => ("dropped", Some(format!("dropped: {STOPPED_BY_ASKER}"))),
                Err(error) => (
                    "dropped",
                    Some(format!("the voice did not take it: {error}")),
                ),
            };
            forget_say(&says, &client, number);
            let mut done = json!({"ev": "done", "re": re, "how": how});
            if let Some(why) = why {
                done["why"] = json!(why);
            }
            events(&client, done);
        });
        Ok(json!({"to": "voice"}))
    }
}

/// Asks the voice for say `number` of `client`, unless its `stop` came
/// first (`None`): the connection the outcome comes on, kept for a `stop`;
/// one that came while connecting is sent on at once.
fn ask_voice(
    path: &std::path::Path,
    line: &Value,
    says: &Says,
    client: &str,
    number: u64,
) -> std::io::Result<Option<UnixStream>> {
    if !is_kept(says, client, number) {
        return Ok(None);
    }
    let mut stream = UnixStream::connect(path)?;
    writeln!(stream, "{line}")?;
    let _ = stream.set_write_timeout(Some(SAY_CANCEL_WITHIN));
    let kept = stream.try_clone()?;
    let mut says = says.lock().expect("says");
    match says
        .get_mut(client)
        .and_then(|asked| asked.get_mut(&number))
    {
        Some(slot) => *slot = Some(kept),
        None => {
            let _ = writeln!(&kept, "{SAY_CANCEL}");
        }
    }
    Ok(Some(stream))
}

fn is_kept(says: &Says, client: &str, number: u64) -> bool {
    says.lock()
        .expect("says")
        .get(client)
        .is_some_and(|asked| asked.contains_key(&number))
}

/// Say `number` of `client` is answered: no `stop` reaches it now.
fn forget_say(says: &Says, client: &str, number: u64) {
    let mut says = says.lock().expect("says");
    if let Some(asked) = says.get_mut(client) {
        asked.remove(&number);
        if asked.is_empty() {
            says.remove(client);
        }
    }
}

/// The say socket's outcome, past the `begun` event, in the protocol's
/// words (`done.how`: `finished`, `cut_short`, `dropped`; the Reviewer-lead's
/// and harness-reviewer's P2 on #3805): `spoken` is `finished`; `dropped`,
/// `expired`, `refused` and no outcome at all are `dropped`, the voice's
/// own word and why in `why`.
fn outcome(reader: impl std::io::BufRead) -> (&'static str, Option<String>) {
    for line in reader.lines() {
        let Ok(line) = line else { break };
        let Ok(said) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if said.get("event").is_some() {
            continue;
        }
        let word = said["outcome"].as_str().unwrap_or("failed");
        if word == "spoken" {
            return ("finished", None);
        }
        let why = match said["why"].as_str() {
            Some(why) => format!("{word}: {why}"),
            None => word.to_owned(),
        };
        return ("dropped", Some(why));
    }
    ("dropped", Some("the voice gave no outcome".to_owned()))
}

/// How long the body plays a move of these frames: from the first frame
/// to the last, as its own clips count it (`lamp_body::clips::Clip::duration_us`).
fn played_us(step_ms: u64, frames: usize) -> u64 {
    (frames.saturating_sub(1) as u64) * step_ms * 1_000
}

fn source(client: &str) -> String {
    format!("control:{client}")
}

fn not_here(part: &str) -> Error {
    Error::new(
        ErrorCode::Refused,
        format!("{part} is not reachable from lamp-control here"),
    )
}

fn waits_for(what: &str) -> Error {
    Error::new(
        ErrorCode::Refused,
        format!("not routed yet: waits for {what}"),
    )
}

impl Parts for Lamp {
    fn act(&mut self, client: &str, re: u64, op: &Op, now_us: u64) -> Result<Value, Error> {
        match op {
            // A built-in clip's own length; a move folder's gesture, which
            // the door can't see, holds nothing.
            Op::Gesture { name } => self.motion(
                json!({"gesture": name}),
                lamp_body::clips::named(name).map(lamp_body::clips::Clip::duration_us),
                client,
                now_us,
            ),
            Op::Move { step_ms, frames } => self.motion(
                json!({"move": {"name": "control", "step_ms": step_ms, "frames": frames}}),
                Some(played_us((*step_ms).into(), frames.len())),
                client,
                now_us,
            ),
            Op::Light {
                colour,
                level,
                pattern,
                for_ms,
            } => self.light(client, colour, *level, *pattern, *for_ms),
            Op::Say { text } => self.say(client, re, text),
            Op::Look { at } => self.look(client, re, at),
            Op::Joints { .. } => Err(waits_for("a goal stream on the body's lease socket")),
            Op::Express { feeling, strength } => self.express(client, feeling, *strength, now_us),
            Op::Play { .. } | Op::Sound { .. } => Err(waits_for("client packages")),
            _ => Err(Error::new(ErrorCode::BadRequest, "not an action")),
        }
    }

    fn state(&self, kind: StateKind) -> Value {
        self.watch
            .as_ref()
            .and_then(|watch| watch.lock().ok().map(|watch| watch.state(kind)))
            .unwrap_or(Value::Null)
    }

    fn stop(&mut self, client: &str, _: u64) {
        if let Some(follow) = &self.follow {
            let mut follower = follow.lock().expect("follower");
            if follower.as_deref() == Some(client) {
                *follower = None;
            }
        }
        // The light given back now: the same light, lapsing at once.
        let now_us = (self.clock)();
        if let Some((light, until_us)) = self.lights.remove(client)
            && until_us > now_us
        {
            let _ = self.ask_ring(client, light, None, now_us);
        }
        // Its own says cancelled, each on the connection that asked it, and
        // no other client's: one still connecting cancels itself when it
        // finds it is no longer kept (`ask_voice`).
        let stopped = self.says.lock().expect("says").remove(client);
        for stream in stopped.into_iter().flat_map(HashMap::into_values).flatten() {
            let _ = writeln!(&stream, "{SAY_CANCEL}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead as _;
    use std::sync::Mutex;

    #[derive(Clone, Default)]
    struct Ring(Arc<Mutex<Vec<LightRequest>>>);

    impl LightOut for Ring {
        fn publish(&mut self, request: &LightRequest) -> std::io::Result<usize> {
            self.0.lock().unwrap().push(request.clone());
            Ok(1)
        }
    }

    fn dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lamp-control-parts-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn op(frame: &str) -> Op {
        crate::protocol::parse(frame).unwrap().op
    }

    fn quiet() -> Events {
        Arc::new(|_, _| {})
    }

    #[test]
    fn a_gesture_and_a_move_go_to_the_body_at_skill_priority_as_this_part() {
        let dir = dir("body");
        let path = dir.join("mood.sock");
        let body = UnixDatagram::bind(&path).unwrap();
        let mut lamp = Lamp::new(Some(path), None, None, "boot".into(), quiet());
        lamp.act("c_1", 1, &op(r#"{"id":1,"op":"gesture","name":"nod"}"#), 0)
            .unwrap();
        let mut buffer = [0u8; 4096];
        let length = body.recv(&mut buffer).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&buffer[..length]).unwrap(),
            json!({"gesture": "nod", "source": "control:c_1", "priority": "skill"})
        );
        lamp.act("c_1", 2, &op(r#"{"id":2,"op":"move","step_ms":500,"frames":[[0,0,0,0,0],[20,0,30,0,0],[20,0,30,0,0],[0,0,0,0,0]]}"#), 10_000_000).unwrap();
        let length = body.recv(&mut buffer).unwrap();
        let sent: Value = serde_json::from_slice(&buffer[..length]).unwrap();
        assert_eq!(sent["move"]["frames"][1], json!([20, 0, 30, 0, 0]));
        assert_eq!(sent["priority"], "skill");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `express` is the feelings skill's own move on the mood socket and
    /// its light on the ring: glad's lift, breathing from the person's warm
    /// 3 down to 2 for 12 s; smaller below strength 0.5; with no light
    /// known, the move alone, said.
    #[test]
    fn express_is_the_feelings_skills_move_and_light() {
        let dir = dir("express");
        let path = dir.join("mood.sock");
        let body = UnixDatagram::bind(&path).unwrap();
        let ring = Ring::default();
        let watch = Arc::new(Mutex::new(crate::watch::Watch::new()));
        let mut lamp = Lamp::new(
            Some(path),
            Some(Box::new(ring.clone())),
            None,
            "boot".into(),
            quiet(),
        )
        .with_clock(booted_61s_ago)
        .watching(watch.clone());
        let glad = r#"{"id":1,"op":"express","feeling":"glad"}"#;
        // No light state yet: the move goes alone, and the answer says why.
        assert_eq!(
            lamp.act("c_1", 1, &op(glad), 0).unwrap(),
            json!({"to": "body", "move": "glad-lift", "light": "the light shown is not known"})
        );
        let mut buffer = [0u8; 16 * 1024];
        let length = body.recv(&mut buffer).unwrap();
        let sent: Value = serde_json::from_slice(&buffer[..length]).unwrap();
        assert_eq!(
            sent["move"],
            json!(crate::feelings::named("glad").unwrap().written())
        );
        assert_eq!(
            (&sent["source"], &sent["priority"]),
            (&json!("control:c_1"), &json!("skill"))
        );
        assert!(ring.0.lock().unwrap().is_empty());
        watch.lock().unwrap().take(
            "light/state",
            &json!({"on": true, "colour": "warm", "level": 3}),
            0,
        );
        assert_eq!(
            lamp.act(
                "c_1",
                2,
                &op(r#"{"id":2,"op":"express","feeling":"glad","strength":0.25}"#),
                60_000_000
            )
            .unwrap(),
            json!({"to": "body", "move": "glad-lift-50", "light": true})
        );
        let length = body.recv(&mut buffer).unwrap();
        let sent: Value = serde_json::from_slice(&buffer[..length]).unwrap();
        let written = crate::feelings::named("glad").unwrap().written();
        let peak = |frames: &Value| {
            frames
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|frame| {
                    frame
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|count| count.as_i64().unwrap().abs())
                })
                .max()
                .unwrap()
        };
        assert!(peak(&sent["move"]["frames"]) < peak(&json!(written.frames)));
        let asked = ring.0.lock().unwrap();
        assert_eq!(asked[0].claim.until_us, 61_000_000 + 12_000_000);
        assert_eq!(
            asked[0].light,
            Light {
                on: true,
                colour: "warm".into(),
                level: 3
            }
        );
        assert_eq!(
            asked[0].pattern,
            Some(LightPattern {
                kind: PatternKind::Breathe,
                period_ms: 6_000,
                low: 2
            })
        );
        drop(asked);
        // A feeling the skill doesn't have.
        assert_eq!(
            lamp.act(
                "c_1",
                3,
                &op(r#"{"id":3,"op":"express","feeling":"cross"}"#),
                0
            )
            .unwrap_err()
            .code,
            ErrorCode::BadRequest
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A lamp with a mood socket the test reads, and the socket.
    fn with_body(name: &str) -> (Lamp, UnixDatagram, PathBuf) {
        let dir = dir(name);
        let path = dir.join("mood.sock");
        let body = UnixDatagram::bind(&path).unwrap();
        body.set_nonblocking(true).unwrap();
        (
            Lamp::new(Some(path), None, None, "boot".into(), quiet()),
            body,
            dir,
        )
    }

    fn sent(body: &UnixDatagram) -> usize {
        let mut buffer = [0u8; 16 * 1024];
        std::iter::from_fn(|| body.recv(&mut buffer).ok()).count()
    }

    const NOD: &str = r#"{"id":1,"op":"gesture","name":"nod"}"#;

    /// #4209: a motion asked while the last one sent still plays is refused
    /// `busy` with when to try again, whoever asks, never sent, and moves
    /// nothing; once it has played, the next is taken.
    #[test]
    fn a_motion_asked_while_the_last_still_plays_is_busy_until_it_ends() {
        let (mut lamp, body, dir) = with_body("busy");
        let nod_us = lamp_body::clips::NOD.duration_us();
        lamp.act("c_1", 1, &op(NOD), 1_000_000).unwrap();
        assert_eq!(sent(&body), 1);
        let error = lamp.act("c_2", 2, &op(NOD), 1_100_000).unwrap_err();
        assert_eq!(error.code, ErrorCode::Busy);
        let left_ms = (nod_us - 100_000) / 1_000;
        assert!(left_ms > 0);
        assert_eq!(error.retry_ms, Some(left_ms));
        assert_eq!(
            error.why,
            format!("busy: a motion plays for {left_ms} ms more")
        );
        assert_eq!(sent(&body), 0, "a refused motion isn't sent");
        // Refused again just before its end: still the first's end, not
        // pushed on by the refusals.
        let error = lamp
            .act("c_1", 3, &op(NOD), 1_000_000 + nod_us - 1_000)
            .unwrap_err();
        assert_eq!(error.retry_ms, Some(1));
        lamp.act("c_2", 4, &op(NOD), 1_000_000 + nod_us).unwrap();
        assert_eq!(sent(&body), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A move holds the slot for its frames at its step, as the body plays
    /// it: from the first frame to the last; `express` for its own move.
    #[test]
    fn a_moves_length_is_its_frames_at_its_step() {
        let (mut lamp, body, dir) = with_body("busy-move");
        let walk = op(
            r#"{"id":1,"op":"move","step_ms":500,"frames":[[0,0,0,0,0],[20,0,30,0,0],[20,0,30,0,0],[0,0,0,0,0]]}"#,
        );
        lamp.act("c_1", 1, &walk, 0).unwrap();
        let error = lamp.act("c_1", 2, &walk, 1_499_000).unwrap_err();
        assert_eq!(error.retry_ms, Some(1));
        lamp.act("c_1", 3, &walk, 1_500_000).unwrap();
        assert_eq!(sent(&body), 2);
        let glad = op(r#"{"id":4,"op":"express","feeling":"glad"}"#);
        lamp.act("c_1", 4, &glad, 3_000_000).unwrap();
        let written = crate::feelings::named("glad").unwrap().written();
        let glad_us = (written.frames.len() as u64 - 1) * u64::from(written.step_ms) * 1_000;
        let error = lamp.act("c_1", 5, &walk, 3_000_000).unwrap_err();
        assert_eq!(error.retry_ms, Some(glad_us / 1_000));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A `stop`, the asker's own or another's, leaves the slot: the door
    /// never stops the body, which plays the motion to its end.
    #[test]
    fn a_stop_leaves_the_motion_slot() {
        let (mut lamp, body, dir) = with_body("busy-stop");
        lamp.act("c_1", 1, &op(NOD), 0).unwrap();
        for client in ["c_2", "c_1"] {
            lamp.stop(client, 100_000);
            assert_eq!(
                lamp.act(client, 2, &op(NOD), 100_000).unwrap_err().code,
                ErrorCode::Busy
            );
        }
        assert_eq!(sent(&body), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 61 s since boot.
    fn booted_61s_ago() -> u64 {
        61_000_000
    }

    /// The Reviewer-lead's P2 on #3805: the door started a minute after
    /// boot, so its own clock reads 1 s; the claim is on the boot's clock,
    /// the one the ring reads.
    #[test]
    fn a_light_is_a_request_that_lapses_and_stop_gives_it_back() {
        let ring = Ring::default();
        let mut lamp = Lamp::new(
            None,
            Some(Box::new(ring.clone())),
            None,
            "boot-1".into(),
            quiet(),
        )
        .with_clock(booted_61s_ago);
        lamp.act("c_1", 1, &op(r#"{"id":1,"op":"light","colour":"warm","level":3,"pattern":"breathe","for_ms":10000}"#), 1_000_000).unwrap();
        {
            let asked = ring.0.lock().unwrap();
            assert_eq!(
                asked[0].claim,
                Claim {
                    source: "control:c_1".into(),
                    priority: Priority::Skill,
                    clock_id: "boot-1".into(),
                    until_us: 71_000_000
                }
            );
            assert_eq!(
                asked[0].light,
                Light {
                    on: true,
                    colour: "warm".into(),
                    level: 3
                }
            );
            assert_eq!(
                asked[0].pattern.map(|p| (p.kind, p.period_ms)),
                Some((PatternKind::Breathe, 6_000))
            );
        }
        // A candle at level 2 is the ring's own refusal, said.
        let error = lamp.act("c_1", 2, &op(r#"{"id":2,"op":"light","colour":"warm","level":2,"pattern":"candle","for_ms":1000}"#), 0).unwrap_err();
        assert_eq!(error.code, ErrorCode::Refused);
        assert!(error.why.contains("level 3"), "{}", error.why);
        lamp.stop("c_1", 2_000_000);
        let asked = ring.0.lock().unwrap();
        assert_eq!(asked.len(), 2);
        assert_eq!(asked[1].claim.until_us, 61_000_000);
    }

    #[test]
    fn a_say_is_written_words_and_its_outcome_comes_as_done() {
        let dir = dir("say");
        let path = dir.join("say.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let told: Arc<Mutex<Vec<(String, Value)>>> = Arc::default();
        let sink = told.clone();
        let events: Events =
            Arc::new(move |client, event| sink.lock().unwrap().push((client.to_owned(), event)));
        let voice = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            let mut out = &stream;
            writeln!(out, r#"{{"event":"begun","at_us":5}}"#).unwrap();
            writeln!(out, r#"{{"outcome":"spoken"}}"#).unwrap();
            serde_json::from_str::<Value>(&line).unwrap()
        });
        let mut lamp = Lamp::new(None, None, Some(path), "boot".into(), events);
        assert_eq!(
            lamp.act(
                "c_1",
                14,
                &op(r#"{"id":14,"op":"say","text":"Build passed."}"#),
                0
            )
            .unwrap(),
            json!({"to": "voice"})
        );
        let asked = voice.join().unwrap();
        assert_eq!(
            asked,
            json!({"skill": "control", "text": "Build passed.", "expires_in_ms": 30000})
        );
        for _ in 0..200 {
            if !told.lock().unwrap().is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            told.lock().unwrap()[0],
            (
                "c_1".to_owned(),
                json!({"ev": "done", "re": 14, "how": "finished"})
            )
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A client's `stop` cancels its own say on the connection that asked
    /// it, and its `done` is dropped with the voice's why; another client's
    /// say on the same voice is untouched and still answered.
    #[test]
    fn a_stop_cancels_the_clients_own_say_and_no_other() {
        let dir = dir("say-stop");
        let path = dir.join("say.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let told: Arc<Mutex<Vec<(String, Value)>>> = Arc::default();
        let sink = told.clone();
        let events: Events =
            Arc::new(move |client, event| sink.lock().unwrap().push((client.to_owned(), event)));
        let mut lamp = Lamp::new(None, None, Some(path), "boot".into(), events);
        lamp.act("c_1", 1, &op(r#"{"id":1,"op":"say","text":"Mine."}"#), 0)
            .unwrap();
        lamp.act("c_2", 2, &op(r#"{"id":2,"op":"say","text":"Theirs."}"#), 0)
            .unwrap();
        // The voice's side: each connection by its words.
        let mut by_text = HashMap::new();
        for _ in 0..2 {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let asked: Value = serde_json::from_str(&line).unwrap();
            by_text.insert(asked["text"].as_str().unwrap().to_owned(), (stream, reader));
        }
        // Both kept with their connections before the stop.
        for _ in 0..200 {
            let kept = lamp
                .says
                .lock()
                .unwrap()
                .values()
                .flat_map(HashMap::values)
                .flatten()
                .count();
            if kept == 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        lamp.stop("c_1", 0);
        let (mine, mine_reader) = by_text.get_mut("Mine.").unwrap();
        let mut line = String::new();
        mine_reader.read_line(&mut line).unwrap();
        assert_eq!(line.trim_end(), SAY_CANCEL);
        writeln!(
            mine,
            r#"{{"outcome":"dropped","why":"{STOPPED_BY_ASKER}"}}"#
        )
        .unwrap();
        // Theirs hears nothing from c_1's stop.
        let (theirs, theirs_reader) = by_text.get_mut("Theirs.").unwrap();
        theirs
            .set_read_timeout(Some(Duration::from_millis(300)))
            .unwrap();
        let mut line = String::new();
        assert!(theirs_reader.read_line(&mut line).is_err(), "{line}");
        writeln!(theirs, r#"{{"outcome":"spoken"}}"#).unwrap();
        for _ in 0..200 {
            if told.lock().unwrap().len() == 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut told = told.lock().unwrap().clone();
        told.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            told,
            vec![
                (
                    "c_1".to_owned(),
                    json!({"ev": "done", "re": 1, "how": "dropped",
                        "why": format!("dropped: {STOPPED_BY_ASKER}")})
                ),
                (
                    "c_2".to_owned(),
                    json!({"ev": "done", "re": 2, "how": "finished"})
                ),
            ]
        );
        // Answered, neither is kept for a later stop.
        assert!(lamp.says.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `look` at `hand` follows it through the lease socket's hold; any
    /// other look taken, or its own client's `stop`, ends that; a refused
    /// look doesn't (Nine's P3 on #4375).
    #[test]
    fn a_look_at_the_hand_follows_it_until_another_look_or_a_stop() {
        let at_hand = op(r#"{"id":1,"op":"look","at":"hand"}"#);
        let mut lamp = Lamp::new(None, None, None, "boot".into(), quiet());
        let error = lamp.act("c_1", 1, &at_hand, 0).unwrap_err();
        assert!(error.why.contains("lease socket"), "{}", error.why);
        let follow = crate::body_lease::Follower::default();
        let following = || follow.lock().unwrap().clone();
        let mut lamp =
            Lamp::new(None, None, None, "boot".into(), quiet()).following(follow.clone());
        assert_eq!(
            lamp.act("c_1", 1, &at_hand, 0).unwrap(),
            json!({"to": "body", "follow": "hand"})
        );
        assert_eq!(following().as_deref(), Some("c_1"));
        // Refused looks leave it: one out of this release, and one with no
        // lease socket to take it.
        let face = op(r#"{"id":2,"op":"look","at":"face"}"#);
        assert!(lamp.act("c_1", 2, &face, 0).is_err());
        let ahead = op(r#"{"id":2,"op":"look","at":"ahead"}"#);
        assert!(lamp.act("c_1", 2, &ahead, 0).is_err());
        assert_eq!(following().as_deref(), Some("c_1"), "a refused look");
        let (looks, _looked) = tokio::sync::mpsc::unbounded_channel();
        let mut lamp = Lamp::new(None, None, None, "boot".into(), quiet())
            .following(follow.clone())
            .looking(looks);
        lamp.act("c_1", 2, &at_hand, 0).unwrap();
        assert_eq!(
            lamp.act("c_1", 2, &ahead, 0).unwrap(),
            json!({"to": "body"})
        );
        assert_eq!(following(), None, "another look taken ends it");
        lamp.act("c_1", 3, &at_hand, 0).unwrap();
        lamp.stop("c_2", 0);
        assert_eq!(following().as_deref(), Some("c_1"), "another client's stop");
        lamp.stop("c_1", 0);
        assert_eq!(following(), None);
    }

    /// `look` goes to the door's hold of the body's lease, as a line the
    /// body reads, with who asked and the request it answers.
    #[test]
    fn a_look_goes_on_the_bodys_lease_as_its_line() {
        let (looks, mut looked) = tokio::sync::mpsc::unbounded_channel();
        let mut lamp = Lamp::new(None, None, None, "boot".into(), quiet()).looking(looks);
        assert_eq!(
            lamp.act(
                "c_1",
                10,
                &op(r#"{"id":10,"op":"look","at":{"yaw":-10,"pitch":5}}"#),
                0
            )
            .unwrap(),
            json!({"to": "body"})
        );
        let look = looked.try_recv().unwrap();
        assert_eq!((look.client.as_str(), look.re), ("c_1", 10));
        assert_eq!(
            serde_json::from_str::<Value>(&look.line).unwrap(),
            json!({"look": {"yaw": -10.0, "pitch": 5.0}})
        );
        lamp.act("c_1", 11, &op(r#"{"id":11,"op":"look","at":"ahead"}"#), 0)
            .unwrap();
        assert_eq!(looked.try_recv().unwrap().line, r#"{"look":"ahead"}"#);
    }

    #[test]
    fn what_is_not_routed_yet_says_what_it_waits_for() {
        let mut lamp = Lamp::new(None, None, None, "boot".into(), quiet());
        for (frame, waits) in [
            (
                r#"{"id":1,"op":"look","at":"ahead"}"#,
                "lease socket is not on",
            ),
            (r#"{"id":1,"op":"look","at":"face"}"#, NOT_IN_THIS_RELEASE),
            (r#"{"id":1,"op":"look","at":"name"}"#, NOT_IN_THIS_RELEASE),
            (r#"{"id":1,"op":"play","name":"wave"}"#, "packages"),
        ] {
            let error = lamp.act("c_1", 1, &op(frame), 0).unwrap_err();
            assert!(error.why.contains(waits), "{frame}: {}", error.why);
        }
        let error = lamp
            .act("c_1", 1, &op(r#"{"id":1,"op":"gesture","name":"nod"}"#), 0)
            .unwrap_err();
        assert!(error.why.contains("not reachable"), "{}", error.why);
        assert_eq!(
            outcome(BufReader::new(
                &b"{\"event\":\"begun\"}\n{\"outcome\":\"dropped\",\"why\":\"someone spoke\"}\n"[..]
            )),
            ("dropped", Some("dropped: someone spoke".to_owned()))
        );
        // The voice's words in the protocol's (the P2s on #3805).
        for (line, how, why) in [
            (r#"{"outcome":"spoken"}"#, "finished", None),
            (r#"{"outcome":"expired"}"#, "dropped", Some("expired")),
            (
                r#"{"outcome":"refused","why":"private"}"#,
                "dropped",
                Some("refused: private"),
            ),
            ("", "dropped", Some("the voice gave no outcome")),
        ] {
            assert_eq!(
                outcome(BufReader::new(line.as_bytes())),
                (how, why.map(str::to_owned)),
                "{line}"
            );
        }
    }

    /// A voice that can't be reached is a `done` dropped, off the door's
    /// lock: the reply comes at once.
    #[test]
    fn a_say_the_voice_cannot_take_is_done_dropped() {
        let dir = dir("say-gone");
        let told: Arc<Mutex<Vec<(String, Value)>>> = Arc::default();
        let sink = told.clone();
        let events: Events =
            Arc::new(move |client, event| sink.lock().unwrap().push((client.to_owned(), event)));
        let mut lamp = Lamp::new(
            None,
            None,
            Some(dir.join("none.sock")),
            "boot".into(),
            events,
        );
        let said = lamp.act("c_1", 3, &op(r#"{"id":3,"op":"say","text":"Hi."}"#), 0);
        assert_eq!(said.unwrap(), json!({"to": "voice"}));
        for _ in 0..200 {
            if !told.lock().unwrap().is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let (client, done) = told.lock().unwrap()[0].clone();
        assert_eq!(client, "c_1");
        assert_eq!(done["how"], "dropped");
        assert!(
            done["why"].as_str().unwrap().contains("did not take"),
            "{done}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
