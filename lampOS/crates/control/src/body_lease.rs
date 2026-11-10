//! The body's lease (`lamp-body --lease-socket`, docs/body.md, "Held by
//! lamp-control", #3791): while a computer holds the protocol's `body`
//! lease, the door holds the body, so that the lamp's own life (the rest
//! glance, Expression's offsets, a stand timing out) stands back while the
//! computer moves it. Moves themselves still go on the mood socket.
//!
//! - **Held** with `{"hold":{"for_ms":30000}}`, renewed every
//!   [`RENEW_EVERY`], so a door that hangs gives the lamp its own life back
//!   within 30 s.
//! - **Following the hand** in view while the computer's `look` at `hand`
//!   stands: each hold says `"follow":"hand"` then, and is said again at
//!   once when that starts or stops; a hold without it follows nothing. The
//!   body does the fast loop on the lamp, from `humans/hands/tracked`; the
//!   door never sees the picture. A body that doesn't know `follow`
//!   refuses the hold, which ends the computer's lease as `released`.
//! - **Let go** when no computer holds `body` any more: the door says
//!   `{"let_go":true}` and closes, and the body ends its lease at once.
//! - **The body's own end** of it, `lost` with why, ends the protocol's
//!   `body` lease with the same why: `hand`, `private`, `paused`, `asleep`,
//!   `slumped`, and `lapsed` as the protocol's `expired`. Its
//!   `suspended`/`held` for a conversation are already the door's own from
//!   the bus, so they are not told twice. A `refused` is written to the
//!   journal, and the door asks again at its next renewal.
//! - **A computer's `look`** goes on the same connection, so it never
//!   outlives the lease it came under (Nine on #3997): `{"look":…}`, each
//!   answered once by the body, in order, and told to the computer as its
//!   `done`: `finished` with the angles the head reached and whether they
//!   were cut to its reach, or `cut_short` or `dropped` with why. Looks still
//!   owed when the connection goes are `cut_short`, why `reconnected`, or
//!   `released` when the door let go.
//! - **A computer's `joints`** for the head's roll and the arm go the same
//!   way as the body's holder pose (lamp-body's `Posing`, d83b9dda68):
//!   `{"pose":{"roll":R,"shoulder":S,"elbow":E}}`, answered once
//!   `there`, `cut_short` or `refused`. Looks and poses are owed apart, each
//!   in the order sent, since a pose may be answered before a later look. A
//!   `joints` naming the head's yaw or pitch and an arm joint sends both
//!   lines under one request ([`Join`]): its `done` comes once both are
//!   answered, `finished` only when both are.
//! - **Each answer to its own line.** A body refuses a new line at once while
//!   an older one is still owed, so its answers don't always come in the
//!   order the lines went (Second's P2 on #5855). A body that says so on its
//!   lease answer (`"tags":true`) gets each line with the door's own tag,
//!   `"tag":N`, and says it back on every answer to that line; the door
//!   matches by it. A body that doesn't gets one pose at a time: a second
//!   while one is owed is `dropped` ([`ONE_POSE_AT_A_TIME`]), never sent, so
//!   no refusal can overtake.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::net::UnixStream;

use crate::parts::Events;
use crate::protocol::{LeaseWhy, Part};
use crate::server::Shared;
use crate::watch::Out;

/// How long one hold asks for: the body's longest.
pub const HOLD_MS: u64 = 30_000;
/// How often a held body is asked again, well inside [`HOLD_MS`].
pub const RENEW_EVERY: Duration = Duration::from_secs(10);
/// How often the door checks whether `body` is held.
pub const CHECK_EVERY: Duration = Duration::from_millis(250);

/// The client whose `look` at `hand` stands, if any: the body follows the
/// hand only while that client holds `body` ([`follows`]).
pub type Follower = Arc<std::sync::Mutex<Option<String>>>;

/// Whether the body follows the hand this check: only for the follower
/// while it holds `body`. A follower that no longer holds it is cleared,
/// so a next holder, even one that takes the body before any check sees it
/// free, never starts out following (the local leader's P3).
pub fn follows(follower: &mut Option<String>, holder: Option<&str>) -> bool {
    if follower.is_some() && follower.as_deref() != holder {
        *follower = None;
    }
    follower.is_some()
}

/// A computer's `look` or pose, for the body: who asked, its request, the
/// line, and, when one `joints` sent two lines, the [`Join`] they share.
#[derive(Debug)]
pub struct Look {
    pub client: String,
    pub re: u64,
    pub line: String,
    pub join: Option<Join>,
}

/// The two answers one `joints` waits for (a look and a pose): the first
/// is kept here, and the second makes the one `done`.
pub type Join = Arc<Mutex<Option<Value>>>;

/// Whether a line for the body is a pose (else a look).
fn is_pose(line: &str) -> bool {
    serde_json::from_str::<Value>(line).is_ok_and(|said| said.get("pose").is_some())
}

/// Two answers to one `joints` as its one `done`: `finished` only when both
/// are, with both parts' angles and `clamped` if either was; else the first
/// not finished, with its why.
fn joined(first: Value, second: Value) -> Value {
    let finished = |done: &Value| done["how"] == "finished";
    match (finished(&first), finished(&second)) {
        (true, true) => {
            let mut done = first.clone();
            for (field, value) in second.as_object().into_iter().flatten() {
                if field != "clamped" {
                    done[field.as_str()] = value.clone();
                }
            }
            done["clamped"] = Value::Bool(first["clamped"] == true || second["clamped"] == true);
            done
        }
        (false, _) => first,
        (true, false) => second,
    }
}

/// Tells a `done` to its asker, or keeps it until the other half of its
/// `joints` is answered.
fn tell(events: &Events, client: &str, join: Option<&Join>, done: Value) {
    let done = match join {
        None => done,
        Some(join) => {
            let mut kept = join.lock().expect("join");
            match kept.take() {
                None => {
                    *kept = Some(done);
                    return;
                }
                Some(first) => joined(first, done),
            }
        }
    };
    events(client, done);
}

/// Where `parts.rs` sends a `look`.
pub type Looks = tokio::sync::mpsc::UnboundedSender<Look>;

/// One look or pose sent and not yet answered: who asked, the request, and
/// its [`Join`].
/// Who asked, the request, its [`Join`], and the door's own tag for the
/// line, which a body that echoes it says back with its answer.
type Asker = (String, u64, Option<Join>, u64);

/// The looks and the poses sent and not yet answered, each oldest first.
#[derive(Default)]
struct Waiting {
    looks: VecDeque<Asker>,
    poses: VecDeque<Asker>,
    /// Whether the body says back each line's tag (it said so on its lease
    /// answer, `"tags":true`); until then its answers carry none.
    tagged: bool,
}

/// Why a pose isn't sent to a body that doesn't say tags back while another
/// is still owed: its refusal could overtake the owed one's answer, and the
/// door couldn't tell them apart (Second, #5855).
pub const ONE_POSE_AT_A_TIME: &str = "another pose is still owed, and this body can't tell its answers apart; ask again once it is answered";

impl Waiting {
    fn push(&mut self, look: &Look, tag: u64) {
        let asker = (look.client.clone(), look.re, look.join.clone(), tag);
        if is_pose(&look.line) {
            self.poses.push_back(asker);
        } else {
            self.looks.push_back(asker);
        }
    }

    /// The line to send for `look`, owed as `tag`: with its tag when the body
    /// says tags back; else the line as it is, but never a second pose while
    /// one is owed. `Err` with why when it can't go.
    fn admit(&mut self, look: &Look, tag: u64) -> Result<String, &'static str> {
        let pose = is_pose(&look.line);
        let line = if self.tagged {
            let mut line: Value =
                serde_json::from_str(&look.line).map_err(|_| "a line the door can't read")?;
            line["tag"] = Value::from(tag);
            line.to_string()
        } else if pose && !self.poses.is_empty() {
            return Err(ONE_POSE_AT_A_TIME);
        } else {
            look.line.clone()
        };
        self.push(look, tag);
        Ok(line)
    }

    /// The asker an answer is for: the one whose line carried `tag` when the
    /// body says it back, else the oldest of its kind. A body refuses a new
    /// line at once while an older one is still owed, so its answers don't
    /// always come in the order the lines went (Second's P2 on #5855); only
    /// the tag tells them apart. A tag nobody is owed takes nothing.
    fn take(&mut self, pose: bool, tag: Option<u64>) -> Option<Asker> {
        let queue = if pose {
            &mut self.poses
        } else {
            &mut self.looks
        };
        match tag {
            Some(tag) => {
                let at = queue.iter().position(|asker| asker.3 == tag)?;
                queue.remove(at)
            }
            None => queue.pop_front(),
        }
    }
}

/// Whether a lease answer says the body says each line's tag back.
fn says_tags(line: &str) -> bool {
    serde_json::from_str::<Value>(line)
        .is_ok_and(|said| said.get("lease").is_some() && said["tags"] == true)
}

/// The door's tag a body said back with its answer, if it did.
fn tag_of(line: &str) -> Option<u64> {
    serde_json::from_str::<Value>(line).ok()?["tag"].as_u64()
}

type Owed = Arc<Mutex<Waiting>>;

/// The body's answer to a look or a pose, as the computer's `done`.
pub fn done_of(line: &str, re: u64) -> Option<Value> {
    let said: Value = serde_json::from_str(line).ok()?;
    let (answer, fields): (&str, &[&str]) = match (said["look"].as_str(), said["pose"].as_str()) {
        (Some(answer), None) => (answer, &["yaw", "pitch", "clamped"]),
        (None, Some(answer)) => (answer, &["roll", "shoulder", "elbow", "clamped"]),
        _ => return None,
    };
    let how = match answer {
        "there" => "finished",
        "cut_short" => "cut_short",
        "refused" => "dropped",
        _ => return None,
    };
    let mut done = serde_json::json!({"ev": "done", "re": re, "how": how});
    if how == "finished" {
        for field in fields {
            done[*field] = said[*field].clone();
        }
    } else {
        done["why"] = said["why"].clone();
    }
    Some(done)
}

/// A look or pose that never reached the body.
fn dropped(events: &Events, look: &Look, why: &str) {
    tell(
        events,
        &look.client,
        look.join.as_ref(),
        serde_json::json!({"ev": "done", "re": look.re, "how": "dropped", "why": why}),
    );
}

/// Every look and pose still owed ends: the connection it went on is gone.
fn end_owed(owed: &Owed, events: &Events, why: &str) {
    let ended: Vec<Asker> = {
        let mut waiting = owed.lock().expect("owed");
        let looks: Vec<_> = waiting.looks.drain(..).collect();
        looks.into_iter().chain(waiting.poses.drain(..)).collect()
    };
    for (client, re, join, _) in ended {
        tell(
            events,
            &client,
            join.as_ref(),
            serde_json::json!({"ev": "done", "re": re, "how": "cut_short", "why": why}),
        );
    }
}

/// What the door says to the body this check.
#[derive(Debug, PartialEq, Eq)]
pub enum Say {
    /// Held, following the hand or not.
    Hold {
        follow: bool,
    },
    LetGo,
}

/// Whether the door holds the body, and since when it last asked.
#[derive(Debug, Default)]
pub struct Holding {
    asked_us: Option<u64>,
    /// Whether the last hold said to follow the hand.
    following: bool,
}

impl Holding {
    /// `wanted`: a computer holds the protocol's `body` lease now;
    /// `follow`: its `look` at `hand` stands.
    pub fn step(&mut self, wanted: bool, follow: bool, now_us: u64) -> Option<Say> {
        let renew_us = u64::try_from(RENEW_EVERY.as_micros()).unwrap_or(u64::MAX);
        match (wanted, self.asked_us) {
            (true, None) => {
                self.asked_us = Some(now_us);
                self.following = follow;
                Some(Say::Hold { follow })
            }
            (true, Some(at))
                if now_us.saturating_sub(at) >= renew_us || follow != self.following =>
            {
                self.asked_us = Some(now_us);
                self.following = follow;
                Some(Say::Hold { follow })
            }
            (false, Some(_)) => {
                self.asked_us = None;
                Some(Say::LetGo)
            }
            _ => None,
        }
    }

    /// The connection went: the next hold is a new one.
    pub fn gone(&mut self) {
        self.asked_us = None;
    }
}

/// A line from the body, as what the door does with it. A `lost` ends the
/// computer's `body` lease with the body's why; one the door doesn't know
/// ends it too, as `released`, the safe direction (Fourteen's P3 on #3900).
/// A `refused` ends it as `released` as well: the door can't hold the body,
/// so the computer must not think the lamp's own life stands back (Sixth's
/// P3 on #3900).
pub fn heard(line: &str) -> Option<Out> {
    let said: Value = serde_json::from_str(line).ok()?;
    let why = match said["lease"].as_str()? {
        "lost" => said["why"]
            .as_str()
            .and_then(why_of)
            .unwrap_or(LeaseWhy::Released),
        "refused" => LeaseWhy::Released,
        _ => return None,
    };
    Some(Out::Lose(Some(Part::Body), why))
}

/// The body's why in the protocol's words.
fn why_of(why: &str) -> Option<LeaseWhy> {
    Some(match why {
        "hand" => LeaseWhy::Hand,
        "private" => LeaseWhy::Private,
        "paused" => LeaseWhy::Paused,
        "asleep" => LeaseWhy::Asleep,
        "lapsed" => LeaseWhy::Expired,
        "slumped" => LeaseWhy::Slumped,
        "conversation" => LeaseWhy::Conversation,
        _ => return None,
    })
}

fn line_of(say: &Say) -> String {
    match say {
        Say::Hold { follow: false } => format!("{{\"hold\":{{\"for_ms\":{HOLD_MS}}}}}\n"),
        Say::Hold { follow: true } => {
            format!("{{\"hold\":{{\"for_ms\":{HOLD_MS},\"follow\":\"hand\"}}}}\n")
        }
        Say::LetGo => "{\"let_go\":true}\n".to_owned(),
    }
}

/// Holds the body at `path` while a computer holds `body`, and hands its
/// ends to the door's leases. Each connection's reader is told apart by a
/// generation, so a late line from one the door has dropped never ends a
/// newer holder's lease; and its end (the body restarted) is seen at the
/// next check, so the door asks again within [`CHECK_EVERY`], not at the
/// next renewal (Sixth's and Fourteen's P3s on #3900).
///
/// `follower` is set by a `look` at `hand` (`crate::parts::Lamp::following`)
/// and cleared here once its client no longer holds `body` ([`follows`]).
pub async fn hold(
    path: PathBuf,
    shared: Arc<Shared>,
    follower: Follower,
    events: Events,
    mut looks: tokio::sync::mpsc::UnboundedReceiver<Look>,
) {
    let owed: Owed = Arc::default();
    // The door's own tag for each line it sends, so a body that says it back
    // ties each answer to its line.
    let mut tags: u64 = 0;
    // Looks asked before the body's hold was taken, sent once it is.
    let mut waiting: Vec<(String, Look)> = Vec::new();
    let mut holding = Holding::default();
    let mut writer: Option<tokio::net::unix::OwnedWriteHalf> = None;
    let current = Arc::new(AtomicU64::new(0));
    let mut ended = Arc::new(AtomicBool::new(false));
    let mut told_unreachable = false;
    let mut check = tokio::time::interval(CHECK_EVERY);
    check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = check.tick() => {}
            Some(look) = looks.recv() => {
                let sent = match writer.as_mut() {
                    Some(write) => {
                        tags += 1;
                        let admitted = owed.lock().expect("owed").admit(&look, tags);
                        match admitted {
                            Ok(line) => write.write_all(format!("{line}\n").as_bytes()).await.is_ok(),
                            Err(why) => {
                                dropped(&events, &look, why);
                                true
                            }
                        }
                    }
                    // The computer holds `body`, and the door takes the body's
                    // hold at its next check: the look waits for it, at most
                    // one check (Nine's P3 on #4023).
                    None => match shared.body_lease() {
                        Some(lease) => {
                            waiting.push((lease, look));
                            true
                        }
                        None => {
                            dropped(&events, &look, "the body is not held");
                            true
                        }
                    },
                };
                if !sent {
                    writer = None;
                    current.fetch_add(1, Ordering::AcqRel);
                    holding.gone();
                    end_owed(&owed, &events, "reconnected");
                }
                continue;
            }
        }
        if writer.is_some() && ended.load(Ordering::Acquire) {
            writer = None;
            holding.gone();
            end_owed(&owed, &events, "reconnected");
        }
        let holder = shared.body_holder();
        let wanted = holder.is_some();
        // A look still waiting when nobody holds `body` any more is the
        // old asker's, never a later holder's (the Reviewer-lead's P2 on
        // #4023).
        if !wanted {
            for (_, look) in waiting.drain(..) {
                dropped(&events, &look, "the body lease ended");
            }
        }
        let following = follows(&mut follower.lock().expect("follower"), holder.as_deref());
        let Some(say) = holding.step(wanted, following, lamp_ipc::monotonic_us()) else {
            continue;
        };
        if writer.is_none() && matches!(say, Say::Hold { .. }) {
            match UnixStream::connect(&path).await {
                Ok(stream) => {
                    told_unreachable = false;
                    let (read, write) = stream.into_split();
                    writer = Some(write);
                    // A new connection may be another build of the body: untagged
                    // until it says otherwise.
                    owed.lock().expect("owed").tagged = false;
                    let generation = current.fetch_add(1, Ordering::AcqRel) + 1;
                    ended = Arc::new(AtomicBool::new(false));
                    let (shared, current, ended) = (shared.clone(), current.clone(), ended.clone());
                    let (owed, events) = (owed.clone(), events.clone());
                    tokio::spawn(async move {
                        let mut lines = BufReader::new(read).lines();
                        while let Ok(Some(line)) = lines.next_line().await {
                            if current.load(Ordering::Acquire) != generation {
                                break;
                            }
                            if line.contains("\"lease\":\"refused\"") {
                                eprintln!("lamp-control: the body refused its lease: {line}");
                            }
                            if says_tags(&line) {
                                owed.lock().expect("owed").tagged = true;
                            }
                            if let Some(out) = heard(&line) {
                                shared.deliver(vec![out]);
                            }
                            let pose = line.contains("\"pose\"");
                            if pose || line.contains("\"look\"") {
                                let asker = owed.lock().expect("owed").take(pose, tag_of(&line));
                                if let Some((client, re, join, _)) = asker
                                    && let Some(done) = done_of(&line, re)
                                {
                                    tell(&events, &client, join.as_ref(), done);
                                }
                            }
                        }
                        ended.store(true, Ordering::Release);
                    });
                }
                Err(error) => {
                    if !told_unreachable {
                        eprintln!(
                            "lamp-control: the body's lease socket {}: {error}; the lamp's own life goes on under a body lease",
                            path.display()
                        );
                        told_unreachable = true;
                    }
                    holding.gone();
                    for (_, look) in waiting.drain(..) {
                        dropped(&events, &look, "the body's lease socket can't be reached");
                    }
                    continue;
                }
            }
        }
        let Some(write) = writer.as_mut() else {
            continue;
        };
        let mut sent = write.write_all(line_of(&say).as_bytes()).await;
        if sent.is_ok() && matches!(say, Say::Hold { .. }) {
            // Only those asked under the lease that holds the body now: one
            // let go and taken again inside a check is another lease.
            let now_held = shared.body_lease();
            let (ours, theirs): (Vec<_>, Vec<_>) = std::mem::take(&mut waiting)
                .into_iter()
                .partition(|(lease, _)| Some(lease) == now_held.as_ref());
            for (_, look) in theirs {
                dropped(&events, &look, "the body lease ended");
            }
            let mut looks = ours.into_iter().map(|(_, look)| look);
            for look in looks.by_ref() {
                tags += 1;
                let admitted = owed.lock().expect("owed").admit(&look, tags);
                sent = match admitted {
                    Ok(line) => write.write_all(format!("{line}\n").as_bytes()).await,
                    Err(why) => {
                        dropped(&events, &look, why);
                        Ok(())
                    }
                };
                if sent.is_err() {
                    break;
                }
            }
            // The connection failed partway: those not sent never reached
            // the body (the sent ones end with `owed`, `reconnected`).
            for look in looks {
                dropped(&events, &look, "the body's lease socket can't be reached");
            }
        }
        if sent.is_err() || say == Say::LetGo {
            // Closed: the body ends its lease at once, and anything its
            // reader still hears is from a connection gone.
            writer = None;
            current.fetch_add(1, Ordering::AcqRel);
            holding.gone();
            let why = if sent.is_err() {
                "reconnected"
            } else {
                "released"
            };
            end_owed(&owed, &events, why);
            for (_, look) in waiting.drain(..) {
                dropped(&events, &look, "the body is not held");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000;

    #[test]
    fn held_while_wanted_renewed_and_let_go() {
        let hold = Some(Say::Hold { follow: false });
        let mut holding = Holding::default();
        assert_eq!(holding.step(false, false, 0), None);
        assert_eq!(holding.step(true, false, S), hold);
        assert_eq!(holding.step(true, false, 5 * S), None);
        assert_eq!(holding.step(true, false, 11 * S), hold);
        assert_eq!(holding.step(false, false, 12 * S), Some(Say::LetGo));
        assert_eq!(holding.step(false, false, 13 * S), None);
        // A connection gone: held again at once when still wanted.
        assert_eq!(holding.step(true, false, 14 * S), hold);
        holding.gone();
        assert_eq!(holding.step(true, false, 15 * S), hold);
        assert_eq!(
            line_of(&Say::Hold { follow: false }),
            "{\"hold\":{\"for_ms\":30000}}\n".to_owned()
        );
        assert_eq!(line_of(&Say::LetGo), "{\"let_go\":true}\n".to_owned());
    }

    /// The local leader's P3: A follows, A's lease ends and B takes the body
    /// before a check sees it free; B's holds follow nothing.
    #[test]
    fn only_the_follower_holding_the_body_follows() {
        let mut follower = Some("c_a".to_owned());
        assert!(follows(&mut follower, Some("c_a")));
        assert!(!follows(&mut follower, Some("c_b")));
        assert_eq!(follower, None, "cleared, not kept for A's return");
        assert!(!follows(&mut follower, Some("c_a")));
        let mut follower = Some("c_a".to_owned());
        assert!(!follows(&mut follower, None));
        assert_eq!(follower, None);
        assert!(!follows(&mut None, Some("c_a")));
    }

    /// A follow started or stopped is said at once, not at the next renewal,
    /// and each renewal says it again.
    #[test]
    fn a_follow_is_said_at_once_and_with_every_hold() {
        let follow = Some(Say::Hold { follow: true });
        let mut holding = Holding::default();
        assert_eq!(
            holding.step(true, false, S),
            Some(Say::Hold { follow: false })
        );
        assert_eq!(holding.step(true, true, S + 250_000), follow);
        assert_eq!(holding.step(true, true, 2 * S), None);
        assert_eq!(holding.step(true, true, 12 * S), follow);
        assert_eq!(
            holding.step(true, false, 12 * S + 250_000),
            Some(Say::Hold { follow: false })
        );
        // A new connection while following: its first hold follows.
        holding.step(true, true, 13 * S);
        holding.gone();
        assert_eq!(holding.step(true, true, 13 * S + 250_000), follow);
        assert_eq!(
            line_of(&Say::Hold { follow: true }),
            "{\"hold\":{\"for_ms\":30000,\"follow\":\"hand\"}}\n".to_owned()
        );
    }

    #[test]
    fn the_bodys_answer_to_a_look_is_the_computers_done() {
        assert_eq!(
            done_of(
                r#"{"look":"there","yaw":-10.0,"pitch":5.0,"clamped":true}"#,
                7
            ),
            Some(
                serde_json::json!({"ev": "done", "re": 7, "how": "finished", "yaw": -10.0, "pitch": 5.0, "clamped": true})
            )
        );
        assert_eq!(
            done_of(r#"{"look":"cut_short","why":"hand"}"#, 8),
            Some(serde_json::json!({"ev": "done", "re": 8, "how": "cut_short", "why": "hand"}))
        );
        assert_eq!(
            done_of(r#"{"look":"refused","why":"the body is not held"}"#, 9).unwrap()["how"],
            "dropped"
        );
        assert_eq!(done_of(r#"{"lease":"held"}"#, 1), None);
        let owed: Owed = Arc::default();
        owed.lock().unwrap().looks.extend([
            ("c_1".to_owned(), 3, None, 1),
            ("c_2".to_owned(), 4, None, 2),
        ]);
        let (told, events) = recorder();
        end_owed(&owed, &events, "reconnected");
        let told = told.lock().unwrap();
        assert_eq!(told.len(), 2);
        assert_eq!(told[1].0, "c_2");
        assert_eq!(told[1].1["why"], "reconnected");
        let waiting = owed.lock().unwrap();
        assert!(waiting.looks.is_empty() && waiting.poses.is_empty());
    }

    type Told = Arc<Mutex<Vec<(String, Value)>>>;

    fn recorder() -> (Told, Events) {
        let told: Told = Arc::default();
        let sink = told.clone();
        let events: Events =
            Arc::new(move |client, event| sink.lock().unwrap().push((client.to_owned(), event)));
        (told, events)
    }

    /// lamp-body's answers to a holder's pose (`Posing`) are the
    /// computer's `done` as a look's are, with the arm's angles.
    #[test]
    fn the_bodys_answer_to_a_pose_is_the_computers_done() {
        assert_eq!(
            done_of(
                r#"{"pose":"there","roll":3.0,"shoulder":10.1,"elbow":0.0,"clamped":false}"#,
                5
            ),
            Some(
                serde_json::json!({"ev": "done", "re": 5, "how": "finished", "roll": 3.0, "shoulder": 10.1, "elbow": 0.0, "clamped": false})
            )
        );
        assert_eq!(
            done_of(
                r#"{"pose":"refused","why":"the lamp is not standing still at its full height"}"#,
                6
            ),
            Some(
                serde_json::json!({"ev": "done", "re": 6, "how": "dropped", "why": "the lamp is not standing still at its full height"})
            )
        );
        assert_eq!(
            done_of(r#"{"pose":"cut_short","why":"another pose"}"#, 7).unwrap()["how"],
            "cut_short"
        );
        assert!(is_pose(r#"{"pose":{"roll":3}}"#) && !is_pose(r#"{"look":"ahead"}"#));
    }

    /// One `joints` with a look and a pose is one `done`: held until both
    /// are answered, finished only when both are, else the first that
    /// wasn't, whichever answer came first.
    #[test]
    fn a_look_and_a_pose_under_one_joints_are_one_done() {
        let (told, events) = recorder();
        let look = r#"{"look":"there","yaw":2.0,"pitch":1.0,"clamped":false}"#;
        let pose = r#"{"pose":"there","roll":0.0,"shoulder":0.0,"elbow":-4.2,"clamped":true}"#;
        let join = Join::default();
        tell(&events, "c_1", Some(&join), done_of(pose, 9).unwrap());
        assert!(told.lock().unwrap().is_empty(), "held for the look");
        tell(&events, "c_1", Some(&join), done_of(look, 9).unwrap());
        assert_eq!(
            told.lock().unwrap().pop().unwrap().1,
            serde_json::json!({"ev": "done", "re": 9, "how": "finished", "yaw": 2.0, "pitch": 1.0, "roll": 0.0, "shoulder": 0.0, "elbow": -4.2, "clamped": true})
        );
        // A pose refused (not standing) and the look there: dropped, with why.
        let join = Join::default();
        tell(&events, "c_1", Some(&join), done_of(look, 10).unwrap());
        tell(
            &events,
            "c_1",
            Some(&join),
            done_of(
                r#"{"pose":"refused","why":"the lamp is not standing still at its full height"}"#,
                10,
            )
            .unwrap(),
        );
        let done = told.lock().unwrap().pop().unwrap().1;
        assert_eq!(
            (done["how"].as_str(), done["why"].as_str()),
            (
                Some("dropped"),
                Some("the lamp is not standing still at its full height")
            )
        );
        // The connection gone with both owed: one cut_short, not two.
        let owed: Owed = Arc::default();
        let join = Join::default();
        {
            let mut waiting = owed.lock().unwrap();
            waiting
                .looks
                .push_back(("c_2".to_owned(), 11, Some(join.clone()), 1));
            waiting
                .poses
                .push_back(("c_2".to_owned(), 11, Some(join), 2));
        }
        end_owed(&owed, &events, "released");
        let told = told.lock().unwrap();
        assert_eq!(told.len(), 1);
        assert_eq!(
            (told[0].1["re"].as_u64(), told[0].1["how"].as_str()),
            (Some(11), Some("cut_short"))
        );
    }

    /// Second's P2 on #5855: a body refuses a new pose at once while an
    /// older one is still owed, and cuts the older one short after, so the
    /// answers come out of the order the lines went. With the tag said back,
    /// each answer goes to its own line; a joint join gets its own part's.
    #[test]
    fn a_refusal_that_overtakes_an_owed_pose_goes_to_its_own_line() {
        let mut waiting = Waiting::default();
        let line = |line: &str, re| Look {
            client: "c_1".into(),
            re,
            line: line.into(),
            join: None,
        };
        waiting.push(&line(r#"{"pose":{"roll":2}}"#, 41), 1);
        waiting.push(&line(r#"{"pose":{"roll":3}}"#, 42), 2);
        let refused = r#"{"pose":"refused","why":"a gesture plays","tag":2}"#;
        let cut = r#"{"pose":"cut_short","why":"a gesture plays","tag":1}"#;
        let first = waiting.take(true, tag_of(refused)).unwrap();
        assert_eq!(
            (first.1, done_of(refused, first.1).unwrap()["how"].clone()),
            (42, serde_json::json!("dropped"))
        );
        let second = waiting.take(true, tag_of(cut)).unwrap();
        assert_eq!(
            (second.1, done_of(cut, second.1).unwrap()["how"].clone()),
            (41, serde_json::json!("cut_short"))
        );
        assert!(waiting.poses.is_empty());
        // An older body that says no tag: the oldest, as before.
        waiting.push(&line(r#"{"pose":{"roll":1}}"#, 50), 3);
        assert_eq!(waiting.take(true, None).map(|asker| asker.1), Some(50));
        // A tag nobody is owed takes nothing.
        assert!(waiting.take(true, Some(99)).is_none());
        // A joint join under it: the look and the pose each get their own answer.
        let (told, events) = recorder();
        let join = Join::default();
        let joint = |kind: &str| Look {
            client: "c_1".into(),
            re: 60,
            line: kind.into(),
            join: Some(join.clone()),
        };
        waiting.push(&joint(r#"{"look":{"yaw":1,"pitch":0}}"#), 4);
        waiting.push(&joint(r#"{"pose":{"elbow":3}}"#), 5);
        waiting.push(&line(r#"{"pose":{"elbow":4}}"#, 61), 6);
        let refused = r#"{"pose":"refused","why":"a gesture plays","tag":6}"#;
        let asker = waiting.take(true, tag_of(refused)).unwrap();
        assert_eq!(asker.1, 61, "the refusal is the new pose's, not the join's");
        tell(
            &events,
            &asker.0,
            asker.2.as_ref(),
            done_of(refused, asker.1).unwrap(),
        );
        let there =
            r#"{"pose":"there","roll":0.0,"shoulder":0.0,"elbow":3.0,"clamped":false,"tag":5}"#;
        let asker = waiting.take(true, tag_of(there)).unwrap();
        tell(
            &events,
            &asker.0,
            asker.2.as_ref(),
            done_of(there, asker.1).unwrap(),
        );
        let look = r#"{"look":"there","yaw":1.0,"pitch":0.0,"clamped":false,"tag":4}"#;
        let asker = waiting.take(false, tag_of(look)).unwrap();
        tell(
            &events,
            &asker.0,
            asker.2.as_ref(),
            done_of(look, asker.1).unwrap(),
        );
        let told = told.lock().unwrap();
        assert_eq!(told.len(), 2, "{told:?}");
        assert_eq!(
            (told[0].1["re"].as_u64(), told[0].1["how"].as_str()),
            (Some(61), Some("dropped"))
        );
        assert_eq!(
            (told[1].1["re"].as_u64(), told[1].1["how"].as_str()),
            (Some(60), Some("finished"))
        );
    }

    /// Second on #5855: a body that doesn't say tags back gets one pose at a
    /// time, so no refusal can overtake an owed pose's answer; one that does
    /// gets each line with its tag. A look is never held back.
    #[test]
    fn an_untagged_body_gets_one_pose_at_a_time_and_a_tagged_one_its_tags() {
        let line = |line: &str, re| Look {
            client: "c_1".into(),
            re,
            line: line.into(),
            join: None,
        };
        let mut waiting = Waiting::default();
        assert_eq!(
            waiting.admit(&line(r#"{"pose":{"roll":2}}"#, 1), 1),
            Ok(r#"{"pose":{"roll":2}}"#.to_owned())
        );
        assert_eq!(
            waiting.admit(&line(r#"{"pose":{"roll":3}}"#, 2), 2),
            Err(ONE_POSE_AT_A_TIME)
        );
        assert!(waiting.admit(&line(r#"{"look":"ahead"}"#, 3), 3).is_ok());
        assert_eq!(waiting.poses.len(), 1);
        waiting.take(true, None);
        assert!(waiting.admit(&line(r#"{"pose":{"roll":3}}"#, 4), 4).is_ok());
        // A body that says tags back: every line carries its tag, any number of poses.
        assert!(says_tags(r#"{"lease":"held","tags":true}"#) && !says_tags(r#"{"lease":"held"}"#));
        let mut tagged = Waiting {
            tagged: true,
            ..Waiting::default()
        };
        let sent = tagged
            .admit(&line(r#"{"pose":{"elbow":3}}"#, 5), 7)
            .unwrap();
        assert_eq!(serde_json::from_str::<Value>(&sent).unwrap()["tag"], 7);
        assert!(tagged.admit(&line(r#"{"pose":{"elbow":4}}"#, 6), 8).is_ok());
        assert_eq!(tagged.poses.len(), 2);
    }

    /// Looks and poses are owed apart: a pose answered before an earlier
    /// look still goes to the pose's asker.
    #[test]
    fn looks_and_poses_are_owed_apart() {
        let mut waiting = Waiting::default();
        let line = |line: &str, re| Look {
            client: "c_1".into(),
            re,
            line: line.into(),
            join: None,
        };
        waiting.push(&line(r#"{"look":"ahead"}"#, 1), 1);
        waiting.push(&line(r#"{"pose":{"shoulder":5}}"#, 2), 2);
        waiting.push(&line(r#"{"look":{"yaw":3,"pitch":0}}"#, 3), 3);
        assert_eq!(waiting.poses.pop_front().map(|asker| asker.1), Some(2));
        assert_eq!(waiting.looks.pop_front().map(|asker| asker.1), Some(1));
        assert_eq!(waiting.looks.pop_front().map(|asker| asker.1), Some(3));
    }

    #[test]
    fn the_bodys_end_is_the_body_leases_end_in_the_protocols_words() {
        for (line, why) in [
            (r#"{"lease":"lost","why":"hand"}"#, LeaseWhy::Hand),
            (r#"{"lease":"lost","why":"private"}"#, LeaseWhy::Private),
            (r#"{"lease":"lost","why":"paused"}"#, LeaseWhy::Paused),
            (r#"{"lease":"lost","why":"asleep"}"#, LeaseWhy::Asleep),
            (r#"{"lease":"lost","why":"lapsed"}"#, LeaseWhy::Expired),
            (r#"{"lease":"lost","why":"slumped"}"#, LeaseWhy::Slumped),
        ] {
            assert!(
                matches!(heard(line), Some(Out::Lose(Some(Part::Body), w)) if w == why),
                "{line}"
            );
        }
        // A why the door doesn't know, none at all, or a refused hold: the
        // computer's lease ends, as released (Fourteen's and Sixth's P3s on
        // #3900).
        for line in [
            r#"{"lease":"lost","why":"something new"}"#,
            r#"{"lease":"lost"}"#,
            r#"{"lease":"refused","why":"one of hold or let_go"}"#,
        ] {
            assert!(
                matches!(
                    heard(line),
                    Some(Out::Lose(Some(Part::Body), LeaseWhy::Released))
                ),
                "{line}"
            );
        }
        // Told already from the bus, or nothing to do.
        for line in [
            r#"{"lease":"held"}"#,
            r#"{"lease":"suspended","why":"conversation"}"#,
            r#"{"lease":"free"}"#,
            "not json",
        ] {
            assert!(heard(line).is_none(), "{line}");
        }
        assert_eq!(serde_json::to_value(LeaseWhy::Slumped).unwrap(), "slumped");
    }
}
