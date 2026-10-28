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

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::net::UnixStream;

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
pub async fn hold(path: PathBuf, shared: Arc<Shared>, follower: Follower) {
    let mut holding = Holding::default();
    let mut writer: Option<tokio::net::unix::OwnedWriteHalf> = None;
    let current = Arc::new(AtomicU64::new(0));
    let mut ended = Arc::new(AtomicBool::new(false));
    let mut told_unreachable = false;
    let mut check = tokio::time::interval(CHECK_EVERY);
    check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        check.tick().await;
        if writer.is_some() && ended.load(Ordering::Acquire) {
            writer = None;
            holding.gone();
        }
        let holder = shared.body_holder();
        let wanted = holder.is_some();
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
                    let generation = current.fetch_add(1, Ordering::AcqRel) + 1;
                    ended = Arc::new(AtomicBool::new(false));
                    let (shared, current, ended) = (shared.clone(), current.clone(), ended.clone());
                    tokio::spawn(async move {
                        let mut lines = BufReader::new(read).lines();
                        while let Ok(Some(line)) = lines.next_line().await {
                            if current.load(Ordering::Acquire) != generation {
                                break;
                            }
                            if line.contains("\"refused\"") {
                                eprintln!("lamp-control: the body refused its lease: {line}");
                            }
                            if let Some(out) = heard(&line) {
                                shared.deliver(vec![out]);
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
                    continue;
                }
            }
        }
        let Some(write) = writer.as_mut() else {
            continue;
        };
        let sent = write.write_all(line_of(&say).as_bytes()).await;
        if sent.is_err() || say == Say::LetGo {
            // Closed: the body ends its lease at once, and anything its
            // reader still hears is from a connection gone.
            writer = None;
            current.fetch_add(1, Ordering::AcqRel);
            holding.gone();
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
