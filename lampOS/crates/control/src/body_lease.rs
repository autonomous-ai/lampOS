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

use std::path::PathBuf;
use std::sync::Arc;
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

/// A line from the body, as what the door does with it.
pub fn heard(line: &str) -> Option<Out> {
    let said: Value = serde_json::from_str(line).ok()?;
    match said["lease"].as_str()? {
        "lost" => Some(Out::Lose(Some(Part::Body), why_of(said["why"].as_str()?)?)),
        _ => None,
    }
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
/// ends to the door's leases.
pub async fn hold(path: PathBuf, shared: Arc<Shared>) {
    let mut holding = Holding::default();
    let mut writer: Option<tokio::net::unix::OwnedWriteHalf> = None;
    let mut told_unreachable = false;
    let mut check = tokio::time::interval(CHECK_EVERY);
    check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        check.tick().await;
        let wanted = shared.body_held();
        let Some(say) = holding.step(wanted, lamp_ipc::monotonic_us()) else {
            continue;
        };
        if writer.is_none() && say == Say::Hold {
            match UnixStream::connect(&path).await {
                Ok(stream) => {
                    told_unreachable = false;
                    let (read, write) = stream.into_split();
                    writer = Some(write);
                    let shared = shared.clone();
                    tokio::spawn(async move {
                        let mut lines = BufReader::new(read).lines();
                        while let Ok(Some(line)) = lines.next_line().await {
                            if line.contains("\"refused\"") {
                                eprintln!("lamp-control: the body refused its lease: {line}");
                            }
                            if let Some(out) = heard(&line) {
                                shared.deliver(vec![out]);
                            }
                        }
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
            // Closed: the body ends its lease at once.
            writer = None;
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
        // Told already from the bus, or nothing to do.
        for line in [
            r#"{"lease":"held"}"#,
            r#"{"lease":"suspended","why":"conversation"}"#,
            r#"{"lease":"free"}"#,
            r#"{"lease":"refused","why":"one holder"}"#,
            r#"{"lease":"lost","why":"something new"}"#,
            "not json",
        ] {
            assert!(heard(line).is_none(), "{line}");
        }
        assert_eq!(serde_json::to_value(LeaseWhy::Slumped).unwrap(), "slumped");
    }
}
