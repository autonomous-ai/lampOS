//! The body's lease (`lamp-body --lease-socket`, docs/body.md, "Held by
//! lamp-control", #3791): while a computer holds the protocol's `body`
//! lease, the door holds the body, so that the lamp's own life (the rest
//! glance, Expression's offsets, a stand timing out) stands back while the
//! computer moves it. Moves themselves still go on the mood socket.
//!
//! - **Held** with `{"hold":{"for_ms":30000}}`, renewed every
//!   [`RENEW_EVERY`], so a door that hangs gives the lamp its own life back
//!   within 30 s.
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

/// A computer's `look`, for the body: who asked, its request, and the line.
#[derive(Debug)]
pub struct Look {
    pub client: String,
    pub re: u64,
    pub line: String,
}

/// Where `parts.rs` sends a `look`.
pub type Looks = tokio::sync::mpsc::UnboundedSender<Look>;

/// The looks sent and not yet answered, oldest first.
type Owed = Arc<Mutex<VecDeque<(String, u64)>>>;

/// The body's answer to a look, as the computer's `done`.
pub fn done_of(line: &str, re: u64) -> Option<Value> {
    let said: Value = serde_json::from_str(line).ok()?;
    let how = match said["look"].as_str()? {
        "there" => "finished",
        "cut_short" => "cut_short",
        "refused" => "dropped",
        _ => return None,
    };
    let mut done = serde_json::json!({"ev": "done", "re": re, "how": how});
    if how == "finished" {
        for field in ["yaw", "pitch", "clamped"] {
            done[field] = said[field].clone();
        }
    } else {
        done["why"] = said["why"].clone();
    }
    Some(done)
}

/// A look that never reached the body.
fn dropped(events: &Events, look: &Look, why: &str) {
    events(
        &look.client,
        serde_json::json!({"ev": "done", "re": look.re, "how": "dropped", "why": why}),
    );
}

/// Every look still owed ends: the connection it went on is gone.
fn end_owed(owed: &Owed, events: &Events, why: &str) {
    let ended: Vec<_> = owed.lock().expect("owed").drain(..).collect();
    for (client, re) in ended {
        events(
            &client,
            serde_json::json!({"ev": "done", "re": re, "how": "cut_short", "why": why}),
        );
    }
}

/// What the door says to the body this check.
#[derive(Debug, PartialEq, Eq)]
pub enum Say {
    Hold,
    LetGo,
}

/// Whether the door holds the body, and since when it last asked.
#[derive(Debug, Default)]
pub struct Holding {
    asked_us: Option<u64>,
}

impl Holding {
    /// `wanted`: a computer holds the protocol's `body` lease now.
    pub fn step(&mut self, wanted: bool, now_us: u64) -> Option<Say> {
        let renew_us = u64::try_from(RENEW_EVERY.as_micros()).unwrap_or(u64::MAX);
        match (wanted, self.asked_us) {
            (true, None) => {
                self.asked_us = Some(now_us);
                Some(Say::Hold)
            }
            (true, Some(at)) if now_us.saturating_sub(at) >= renew_us => {
                self.asked_us = Some(now_us);
                Some(Say::Hold)
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
        Say::Hold => format!("{{\"hold\":{{\"for_ms\":{HOLD_MS}}}}}\n"),
        Say::LetGo => "{\"let_go\":true}\n".to_owned(),
    }
}

/// Holds the body at `path` while a computer holds `body`, and hands its
/// ends to the door's leases. Each connection's reader is told apart by a
/// generation, so a late line from one the door has dropped never ends a
/// newer holder's lease; and its end (the body restarted) is seen at the
/// next check, so the door asks again within [`CHECK_EVERY`], not at the
/// next renewal (Sixth's and Fourteen's P3s on #3900).
pub async fn hold(
    path: PathBuf,
    shared: Arc<Shared>,
    events: Events,
    mut looks: tokio::sync::mpsc::UnboundedReceiver<Look>,
) {
    let owed: Owed = Arc::default();
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
                        owed.lock().expect("owed").push_back((look.client.clone(), look.re));
                        write.write_all(format!("{}\n", look.line).as_bytes()).await.is_ok()
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
        let wanted = shared.body_held();
        // A look still waiting when nobody holds `body` any more is the
        // old asker's, never a later holder's (the Reviewer-lead's P2 on
        // #4023).
        if !wanted {
            for (_, look) in waiting.drain(..) {
                dropped(&events, &look, "the body lease ended");
            }
        }
        let Some(say) = holding.step(wanted, lamp_ipc::monotonic_us()) else {
            continue;
        };
        if writer.is_none() && say == Say::Hold {
            match UnixStream::connect(&path).await {
                Ok(stream) => {
                    told_unreachable = false;
                    let (read, write) = stream.into_split();
                    writer = Some(write);
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
                            if let Some(out) = heard(&line) {
                                shared.deliver(vec![out]);
                            }
                            if line.contains("\"look\"") {
                                let asker = owed.lock().expect("owed").pop_front();
                                if let Some((client, re)) = asker
                                    && let Some(done) = done_of(&line, re)
                                {
                                    events(&client, done);
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
        if sent.is_ok() && say == Say::Hold {
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
                owed.lock()
                    .expect("owed")
                    .push_back((look.client.clone(), look.re));
                sent = write.write_all(format!("{}\n", look.line).as_bytes()).await;
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
        let mut holding = Holding::default();
        assert_eq!(holding.step(false, 0), None);
        assert_eq!(holding.step(true, S), Some(Say::Hold));
        assert_eq!(holding.step(true, 5 * S), None);
        assert_eq!(holding.step(true, 11 * S), Some(Say::Hold));
        assert_eq!(holding.step(false, 12 * S), Some(Say::LetGo));
        assert_eq!(holding.step(false, 13 * S), None);
        // A connection gone: held again at once when still wanted.
        assert_eq!(holding.step(true, 14 * S), Some(Say::Hold));
        holding.gone();
        assert_eq!(holding.step(true, 15 * S), Some(Say::Hold));
        assert_eq!(
            line_of(&Say::Hold),
            "{\"hold\":{\"for_ms\":30000}}\n".to_owned()
        );
        assert_eq!(line_of(&Say::LetGo), "{\"let_go\":true}\n".to_owned());
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
        owed.lock()
            .unwrap()
            .extend([("c_1".to_owned(), 3), ("c_2".to_owned(), 4)]);
        let told: Arc<Mutex<Vec<(String, Value)>>> = Arc::default();
        let sink = told.clone();
        let events: Events =
            Arc::new(move |client, event| sink.lock().unwrap().push((client.to_owned(), event)));
        end_owed(&owed, &events, "reconnected");
        let told = told.lock().unwrap();
        assert_eq!(told.len(), 2);
        assert_eq!(told[1].0, "c_2");
        assert_eq!(told[1].1["why"], "reconnected");
        assert!(owed.lock().unwrap().is_empty());
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
