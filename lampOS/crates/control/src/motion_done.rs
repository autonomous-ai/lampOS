//! A computer's motion told back (#4209): each `gesture`, `move` and
//! feeling's move the door hands the body gets a `done` once the body says
//! on `body/gesture` what became of it, so an SDK's `wait` ends when the
//! motion has, and a motion the body drops (the person using the lamp, a
//! move it can't play) is told to its computer, never only in the journal.
//!
//! The product body says `{"name", "state"}` with no asker (`--arbitration`
//! is off), so a motion is matched by its name, oldest first, within
//! [`BEGIN_WITHIN`] of being sent: `nod` as asked, `control` for a move, the
//! scaled feeling's name for an express. A skill's same gesture begun in
//! that window could be taken for the computer's; the body plays one at a
//! time, so at most one of the two is told wrong.

use std::collections::VecDeque;

use serde_json::{Value, json};

/// How long a motion handed to the body may wait for it to begin: the body
/// stands a folded lamp up first, which took more than 4 s on lamp-75c9
/// (9 Nov, run3: a nod asked 11 s after lamp-body's start was still waiting
/// at 4 s), so past a stand-up with room to spare.
pub const BEGIN_WITHIN_US: u64 = 10_000_000;
/// The same, for the docs.
pub const BEGIN_WITHIN: &str = "10 s";
/// The why of a playing motion the body ended with no why of its own while
/// a newer motion of that name waits: the door's own `replace`. The
/// product body names no why for it on `body/gesture` (lamp-75c9, 9 Nov).
pub const REPLACED: &str = "replaced by a newer motion";
/// The why of a playing motion the body ended with no why and nothing
/// newer waiting.
pub const ENDED: &str = "the body ended it";
/// How long a begun motion may play with no word of its end: past every
/// clip and move (a move is 10 s at most), and its `done` says it was not
/// heard to end.
pub const END_WITHIN_US: u64 = 60_000_000;
/// The field a `done` for one connection carries inside the door, taken
/// off by the server before it is sent (`Shared::send_to`).
pub const CONNECTION: &str = "_connection";
/// A computer's motions waiting for their `done`, across every computer;
/// past this the oldest is told as not heard.
pub const MOST_PENDING: usize = 32;

/// One motion handed to the body and not yet told.
#[derive(Clone, Debug, PartialEq)]
struct Pending {
    client: String,
    /// The connection that asked: its `done` goes there alone.
    connection: Option<u64>,
    re: u64,
    name: String,
    sent_us: u64,
    began_us: Option<u64>,
}

/// The door's motions waiting for the body's word.
#[derive(Debug, Default)]
pub struct Motions {
    pending: VecDeque<Pending>,
}

/// A `done` for one computer.
pub type Told = (String, Value);

fn done(connection: Option<u64>, re: u64, how: &str, why: Option<String>) -> Value {
    let mut done = json!({"ev": "done", "re": re, "how": how});
    if let Some(why) = why {
        done["why"] = json!(why);
    }
    if let Some(connection) = connection {
        done[CONNECTION] = json!(connection);
    }
    done
}

impl Motions {
    /// `client`'s request `re`, a motion named `name` the body took at
    /// `now_us`. Past [`MOST_PENDING`], the oldest is told now.
    pub fn sent(
        &mut self,
        client: &str,
        connection: Option<u64>,
        re: u64,
        name: &str,
        now_us: u64,
    ) -> Vec<Told> {
        let mut told = Vec::new();
        while self.pending.len() >= MOST_PENDING {
            if let Some(old) = self.pending.pop_front() {
                told.push(Self::unheard(old));
            }
        }
        self.pending.push_back(Pending {
            client: client.to_owned(),
            connection,
            re,
            name: name.to_owned(),
            sent_us: now_us,
            began_us: None,
        });
        told
    }

    /// What `body/gesture` said (`{"name", "state", "why"?}`) at `now_us`:
    /// the `done`s it ends.
    pub fn said(&mut self, message: &Value, now_us: u64) -> Vec<Told> {
        let mut told = self.lapse(now_us);
        let (Some(name), Some(state)) = (
            message.get("name").and_then(Value::as_str),
            message.get("state").and_then(Value::as_str),
        ) else {
            return told;
        };
        let why = message
            .get("why")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let waiting = |p: &Pending| p.name == name && p.began_us.is_none();
        let playing = |p: &Pending| p.name == name && p.began_us.is_some();
        match state {
            "began" => {
                if let Some(motion) = self.pending.iter_mut().find(|p| waiting(p)) {
                    motion.began_us = Some(now_us);
                }
            }
            "over" => {
                if let Some(at) = self.pending.iter().position(playing) {
                    let motion = self.pending.remove(at).expect("found");
                    told.push((
                        motion.client,
                        done(motion.connection, motion.re, "finished", None),
                    ));
                }
            }
            "dropped" => {
                // One playing that is ended (replaced, or for the person)
                // was cut short; one still waiting was dropped. The body's
                // why says which: "ended: …" for one playing, "dropped: …"
                // for one that never began; with none, the playing one.
                let ended = why.as_deref().is_none_or(|why| why.starts_with("ended"));
                let playing_at = self.pending.iter().position(playing);
                let waiting_at = self.pending.iter().position(waiting);
                if let (false, Some(at)) = (ended, waiting_at) {
                    let motion = self.pending.remove(at).expect("found");
                    let why = why.unwrap_or_else(|| "the body dropped it".to_owned());
                    told.push((
                        motion.client,
                        done(motion.connection, motion.re, "dropped", Some(why)),
                    ));
                } else if let Some(at) = playing_at {
                    let motion = self.pending.remove(at).expect("found");
                    // A newer one of the same name waiting: the door's own
                    // replace, which the body says with no why.
                    let replaced = waiting_at.is_some();
                    let why =
                        why.unwrap_or_else(|| if replaced { REPLACED } else { ENDED }.to_owned());
                    told.push((
                        motion.client,
                        done(motion.connection, motion.re, "cut_short", Some(why)),
                    ));
                } else if let Some(at) = waiting_at {
                    let motion = self.pending.remove(at).expect("found");
                    let why = why.unwrap_or_else(|| "the body dropped it".to_owned());
                    told.push((
                        motion.client,
                        done(motion.connection, motion.re, "dropped", Some(why)),
                    ));
                }
            }
            _ => {}
        }
        told
    }

    /// The motions the body never began, or never said the end of, by
    /// `now_us`.
    pub fn lapse(&mut self, now_us: u64) -> Vec<Told> {
        let mut told = Vec::new();
        let mut kept = VecDeque::with_capacity(self.pending.len());
        for motion in self.pending.drain(..) {
            let late = match motion.began_us {
                None => now_us.saturating_sub(motion.sent_us) >= BEGIN_WITHIN_US,
                Some(began) => now_us.saturating_sub(began) >= END_WITHIN_US,
            };
            if late {
                told.push(Self::unheard(motion));
            } else {
                kept.push_back(motion);
            }
        }
        self.pending = kept;
        told
    }

    /// Takes back `client`'s request `re`, never sent: the body did not
    /// take it (registered first, so the body's word can't come before).
    pub fn withdraw(&mut self, client: &str, re: u64) {
        if let Some(at) = self
            .pending
            .iter()
            .rposition(|motion| motion.client == client && motion.re == re)
        {
            self.pending.remove(at);
        }
    }

    /// Forgets `client`'s motions: it went, nobody to tell.
    pub fn gone(&mut self, client: &str) {
        self.pending.retain(|motion| motion.client != client);
    }

    /// Whether any motion waits for the body's word.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    fn unheard(motion: Pending) -> Told {
        let (how, why) = match motion.began_us {
            None => (
                "dropped",
                format!("the body did not begin it within {BEGIN_WITHIN}"),
            ),
            Some(_) => (
                "cut_short",
                "the body did not say it ended within 60 s".to_owned(),
            ),
        };
        (
            motion.client,
            done(motion.connection, motion.re, how, Some(why)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000;

    fn said(name: &str, state: &str) -> Value {
        json!({"name": name, "state": state})
    }

    #[test]
    fn a_gesture_played_out_is_finished_at_its_over() {
        let mut motions = Motions::default();
        assert!(motions.sent("c_1", None, 4, "nod", 0).is_empty());
        assert!(motions.said(&said("nod", "began"), S / 10).is_empty());
        assert!(motions.said(&said("idle", "over"), S / 10).is_empty());
        assert_eq!(
            motions.said(&said("nod", "over"), 3 * S),
            [(
                "c_1".to_owned(),
                json!({"ev": "done", "re": 4, "how": "finished"})
            )]
        );
        assert!(motions.is_empty());
    }

    #[test]
    fn a_motion_replaced_by_a_newer_one_is_cut_short_and_the_newer_finishes() {
        let mut motions = Motions::default();
        motions.sent("c_1", None, 1, "control", 0);
        motions.said(&said("control", "began"), S / 10);
        motions.sent("c_1", None, 2, "control", S);
        let cut = json!({"name": "control", "state": "dropped",
            "why": "ended: replaced by a newer motion"});
        assert_eq!(
            motions.said(&cut, S + S / 10),
            [(
                "c_1".to_owned(),
                json!({"ev": "done", "re": 1, "how": "cut_short",
                    "why": "ended: replaced by a newer motion"})
            )]
        );
        motions.said(&said("control", "began"), S + S / 10);
        assert_eq!(
            motions.said(&said("control", "over"), 3 * S),
            [(
                "c_1".to_owned(),
                json!({"ev": "done", "re": 2, "how": "finished"})
            )]
        );
    }

    /// #4209: the body's drop reaches the computer with the body's why.
    #[test]
    fn a_motion_the_body_drops_is_told_dropped_with_its_why() {
        let mut motions = Motions::default();
        motions.sent("c_2", None, 7, "control", 0);
        let drop = json!({"name": "control", "state": "dropped",
            "why": "dropped: the person is using the lamp"});
        assert_eq!(
            motions.said(&drop, S / 5),
            [(
                "c_2".to_owned(),
                json!({"ev": "done", "re": 7, "how": "dropped",
                    "why": "dropped: the person is using the lamp"})
            )]
        );
    }

    /// lamp-75c9, 9 Nov: the product body ends a replaced motion on
    /// `body/gesture` with no why; the door names it as its replace, and an
    /// end with nothing newer waiting as the body's.
    #[test]
    fn an_end_with_no_why_is_the_replace_when_a_newer_one_waits() {
        let mut motions = Motions::default();
        motions.sent("c_1", None, 5, "control", 0);
        motions.said(&said("control", "began"), S / 10);
        motions.sent("c_1", None, 6, "control", S);
        let told = motions.said(&said("control", "dropped"), S + S / 10);
        assert_eq!(
            told[0].1,
            json!({"ev": "done", "re": 5, "how": "cut_short", "why": REPLACED})
        );
        motions.said(&said("control", "began"), S + S / 5);
        let told = motions.said(&said("control", "dropped"), 2 * S);
        assert_eq!(
            told[0].1,
            json!({"ev": "done", "re": 6, "how": "cut_short", "why": ENDED})
        );
    }

    /// Ten's second case on #4209: one asked while its own name plays is
    /// dropped before it began, and the one playing goes on to finish.
    #[test]
    fn a_drop_of_one_waiting_leaves_the_one_playing_of_that_name() {
        let mut motions = Motions::default();
        motions.sent("c_1", None, 1, "nod", 0);
        motions.said(&said("nod", "began"), S / 10);
        motions.sent("c_1", None, 2, "nod", S);
        let drop = json!({"name": "nod", "state": "dropped",
            "why": "dropped: another gesture plays"});
        let told = motions.said(&drop, S + S / 10);
        assert_eq!(
            (&told[0].1["re"], &told[0].1["how"]),
            (&json!(2), &json!("dropped"))
        );
        let told = motions.said(&said("nod", "over"), 3 * S);
        assert_eq!(
            (&told[0].1["re"], &told[0].1["how"]),
            (&json!(1), &json!("finished"))
        );
    }

    #[test]
    fn a_motion_never_begun_is_dropped_after_ten_seconds() {
        let mut motions = Motions::default();
        motions.sent("c_1", None, 3, "shy", 0);
        assert!(motions.lapse(BEGIN_WITHIN_US - 1).is_empty());
        let told = motions.lapse(BEGIN_WITHIN_US);
        assert_eq!(told[0].1["how"], "dropped");
        assert_eq!(told[0].1["why"], "the body did not begin it within 10 s");
        assert!(motions.is_empty());
    }

    #[test]
    fn a_begun_motion_with_no_end_is_cut_short_after_sixty_seconds() {
        let mut motions = Motions::default();
        motions.sent("c_1", None, 3, "dance_hype", 0);
        motions.said(&said("dance_hype", "began"), S);
        assert!(motions.lapse(S + END_WITHIN_US - 1).is_empty());
        let told = motions.lapse(S + END_WITHIN_US);
        assert_eq!(told[0].1["how"], "cut_short");
    }

    #[test]
    fn the_oldest_waiting_of_a_name_is_matched_and_each_computer_told_its_own() {
        let mut motions = Motions::default();
        motions.sent("c_1", None, 1, "nod", 0);
        motions.sent("c_2", None, 9, "nod", S / 10);
        motions.said(&said("nod", "began"), S / 5);
        let first = motions.said(&said("nod", "over"), 2 * S);
        assert_eq!(first[0].0, "c_1");
        assert_eq!(first[0].1["re"], 1);
        motions.said(&said("nod", "began"), 2 * S);
        let second = motions.said(&said("nod", "over"), 4 * S);
        assert_eq!(
            (second[0].0.as_str(), &second[0].1["re"]),
            ("c_2", &json!(9))
        );
    }

    #[test]
    fn a_gone_computer_is_told_nothing_and_a_full_list_tells_its_oldest() {
        let mut motions = Motions::default();
        motions.sent("c_1", None, 1, "nod", 0);
        motions.gone("c_1");
        assert!(motions.is_empty());
        for re in 0..MOST_PENDING as u64 {
            assert!(motions.sent("c_3", None, re, "nod", 0).is_empty());
        }
        let told = motions.sent("c_3", None, 99, "nod", 0);
        assert_eq!(told.len(), 1);
        assert_eq!(told[0].1["re"], 0);
    }
}
