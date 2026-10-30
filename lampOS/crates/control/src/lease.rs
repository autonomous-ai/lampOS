//! Who holds which part, and for how long (docs/control.md, "Who wins").
//!
//! A client takes a lease on the parts it wants to drive for at most
//! [`LEASE_MAX_MS`] and renews it before it lapses; renewed in time, it is
//! held as long as its holder keeps renewing (no ceiling: the owner's "no
//! limits for developers", 30 Oct). One client holds a part at a time, and
//! every loss below still ends it.
//!
//! The lamp takes its body back without asking: someone talking to it
//! *suspends* every lease (held again when the talk ends, if not lapsed by
//! then); a hand, "stop", the privacy switch, a pause, sleep, the owner's
//! app or a revoke *loses* it. Each change is an event for its holder. The
//! clock is the caller's (monotonic microseconds), so tests step it.

use crate::protocol::{LEASE_MAX_MS, LeaseState, LeaseWhy, Part};

/// A lease's change, for its holder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub client: String,
    pub lease: String,
    pub state: LeaseState,
    pub why: Option<LeaseWhy>,
}

/// Why a take, renew, release or act was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Another client holds the part (or this one, by another lease).
    Busy { part: Part, holder: String },
    /// No lease of this client's by that id.
    NoLease,
    /// It held one, but it was lost: the why.
    Lost(LeaseWhy),
    /// Held, but suspended while someone talks to the lamp.
    Suspended,
    /// Not a hold the protocol allows ([`LEASE_MAX_MS`]).
    BadHold,
}

#[derive(Clone, Debug)]
struct Lease {
    id: String,
    client: String,
    parts: Vec<Part>,
    until_us: u64,
    suspended: bool,
}

/// What a take or renew gives: the lease's id and when it lapses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Held {
    pub lease: String,
    pub until_us: u64,
}

/// Every lease the lamp has given and not yet ended, and the last way each
/// recent one ended (so an act on it says why, not just "no lease").
#[derive(Debug, Default)]
pub struct Leases {
    held: Vec<Lease>,
    /// Each client's last ended leases, [`ENDED_KEPT`] each, so one
    /// client's churn never pushes out another's why.
    ended: std::collections::HashMap<String, Vec<(String, LeaseWhy)>>,
    /// Lapses found by a take, renew, release or act, for the next
    /// [`Leases::lapse`] to tell.
    pending: Vec<Change>,
    next: u64,
}

/// How many ended leases are remembered for their why.
const ENDED_KEPT: usize = 16;

impl Leases {
    pub fn new() -> Self {
        Self::default()
    }

    /// `client` takes `parts` for `for_ms`. Refused if any part is held,
    /// by anyone, or the hold is out of bounds.
    pub fn take(
        &mut self,
        client: &str,
        parts: &[Part],
        for_ms: u32,
        now_us: u64,
    ) -> Result<Held, Refusal> {
        if !(1..=LEASE_MAX_MS).contains(&for_ms) || parts.is_empty() {
            return Err(Refusal::BadHold);
        }
        self.expire(now_us);
        for part in parts {
            if let Some(lease) = self.held.iter().find(|lease| lease.parts.contains(part)) {
                return Err(Refusal::Busy {
                    part: *part,
                    holder: lease.client.clone(),
                });
            }
        }
        self.next += 1;
        let id = format!("l_{}", self.next);
        let mut parts = parts.to_vec();
        parts.sort();
        parts.dedup();
        let until_us = now_us + u64::from(for_ms) * 1_000;
        self.held.push(Lease {
            id: id.clone(),
            client: client.to_owned(),
            parts,
            until_us,
            suspended: false,
        });
        Ok(Held {
            lease: id,
            until_us,
        })
    }

    /// Renews `client`'s lease for `for_ms` more from now. A lease renewed
    /// before it lapses has no ceiling.
    pub fn renew(
        &mut self,
        client: &str,
        lease: &str,
        for_ms: u32,
        now_us: u64,
    ) -> Result<Held, Refusal> {
        if !(1..=LEASE_MAX_MS).contains(&for_ms) {
            return Err(Refusal::BadHold);
        }
        self.expire(now_us);
        let index = self.find(client, lease)?;
        let held = &mut self.held[index];
        held.until_us = now_us + u64::from(for_ms) * 1_000;
        Ok(Held {
            lease: held.id.clone(),
            until_us: held.until_us,
        })
    }

    /// Ends `client`'s lease.
    pub fn release(&mut self, client: &str, lease: &str, now_us: u64) -> Result<(), Refusal> {
        self.expire(now_us);
        let index = self.find(client, lease)?;
        let lease = self.held.remove(index);
        self.remember(lease, LeaseWhy::Released);
        Ok(())
    }

    /// Whether `client` may act on `part` under `lease` now: held, its, on
    /// that part, and not suspended.
    pub fn may_act(
        &mut self,
        client: &str,
        lease: &str,
        part: Part,
        now_us: u64,
    ) -> Result<(), Refusal> {
        self.expire(now_us);
        let index = self.find(client, lease)?;
        let held = &self.held[index];
        if !held.parts.contains(&part) {
            return Err(Refusal::NoLease);
        }
        if held.suspended {
            return Err(Refusal::Suspended);
        }
        Ok(())
    }

    /// Whether `client` holds `part` now, by any lease, not suspended.
    pub fn holds(&self, client: &str, part: Part, now_us: u64) -> bool {
        self.held.iter().any(|lease| {
            lease.client == client
                && lease.parts.contains(&part)
                && !lease.suspended
                && lease.until_us > now_us
        })
    }

    /// The id of the lease that holds `part` now, if any.
    pub fn holding(&self, part: Part, now_us: u64) -> Option<&str> {
        self.held
            .iter()
            .find(|lease| lease.parts.contains(&part) && lease.until_us > now_us)
            .map(|lease| lease.id.as_str())
    }

    /// Who holds `part` now, if anyone.
    pub fn holder(&self, part: Part, now_us: u64) -> Option<&str> {
        self.held
            .iter()
            .find(|lease| lease.parts.contains(&part) && lease.until_us > now_us)
            .map(|lease| lease.client.as_str())
    }

    /// The leases that lapsed by `now_us`, ended, with any an earlier call
    /// found: one change each, each told once.
    pub fn lapse(&mut self, now_us: u64) -> Vec<Change> {
        self.expire(now_us);
        std::mem::take(&mut self.pending)
    }

    /// Ends the leases lapsed by `now_us`, their changes kept for
    /// [`Leases::lapse`].
    fn expire(&mut self, now_us: u64) {
        let (gone, kept): (Vec<Lease>, Vec<Lease>) = self
            .held
            .drain(..)
            .partition(|lease| lease.until_us <= now_us);
        self.held = kept;
        for lease in gone {
            let change = self.ended_with(lease, LeaseWhy::Expired);
            self.pending.push(change);
        }
    }

    /// Someone began talking to the lamp: every lease holding `part`
    /// (any, for `None`) is suspended. Its clock goes on.
    pub fn suspend(&mut self, part: Option<Part>, why: LeaseWhy, now_us: u64) -> Vec<Change> {
        let mut changes = self.lapse(now_us);
        for lease in self
            .held
            .iter_mut()
            .filter(|lease| !lease.suspended && part.is_none_or(|part| lease.parts.contains(&part)))
        {
            lease.suspended = true;
            changes.push(Change {
                client: lease.client.clone(),
                lease: lease.id.clone(),
                state: LeaseState::Suspended,
                why: Some(why),
            });
        }
        changes
    }

    /// The talk ended: every suspended lease not lapsed is held again.
    pub fn resume(&mut self, now_us: u64) -> Vec<Change> {
        let mut changes = self.lapse(now_us);
        for lease in self.held.iter_mut().filter(|lease| lease.suspended) {
            lease.suspended = false;
            changes.push(Change {
                client: lease.client.clone(),
                lease: lease.id.clone(),
                state: LeaseState::Held,
                why: None,
            });
        }
        changes
    }

    /// A hand, "stop", privacy, pause, sleep or the owner's app: every
    /// lease holding `part` (any, for `None`) is lost, whole: a lease taken
    /// as body and light loses the light too when a hand takes the body, so
    /// a client never drives half of what it asked for (the Reviewer-lead's
    /// P3 on #3715).
    pub fn lose(&mut self, part: Option<Part>, why: LeaseWhy, now_us: u64) -> Vec<Change> {
        let mut changes = self.lapse(now_us);
        let (gone, kept): (Vec<Lease>, Vec<Lease>) = self
            .held
            .drain(..)
            .partition(|lease| part.is_none_or(|part| lease.parts.contains(&part)));
        self.held = kept;
        changes.extend(gone.into_iter().map(|lease| self.ended_with(lease, why)));
        changes
    }

    /// A client revoked, or its last connection closed: its leases end
    /// (`Revoked` is told; a closed connection has nobody to tell).
    pub fn client_gone(&mut self, client: &str, why: LeaseWhy, now_us: u64) -> Vec<Change> {
        let mut changes = self.lapse(now_us);
        let (gone, kept): (Vec<Lease>, Vec<Lease>) = self
            .held
            .drain(..)
            .partition(|lease| lease.client == client);
        self.held = kept;
        changes.extend(gone.into_iter().map(|lease| self.ended_with(lease, why)));
        changes
    }

    fn find(&self, client: &str, lease: &str) -> Result<usize, Refusal> {
        match self
            .held
            .iter()
            .position(|held| held.id == lease && held.client == client)
        {
            Some(index) => Ok(index),
            None => match self
                .ended
                .get(client)
                .and_then(|ended| ended.iter().rev().find(|(id, _)| id == lease))
            {
                Some((_, why)) => Err(Refusal::Lost(*why)),
                None => Err(Refusal::NoLease),
            },
        }
    }

    fn ended_with(&mut self, lease: Lease, why: LeaseWhy) -> Change {
        let change = Change {
            client: lease.client.clone(),
            lease: lease.id.clone(),
            state: LeaseState::Lost,
            why: Some(why),
        };
        self.remember(lease, why);
        change
    }

    fn remember(&mut self, lease: Lease, why: LeaseWhy) {
        let ended = self.ended.entry(lease.client).or_default();
        ended.push((lease.id, why));
        if ended.len() > ENDED_KEPT {
            ended.remove(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000;

    /// The Reviewer-lead's P3 on #4023: let go and taken again is another
    /// lease, by id, even for the same client.
    #[test]
    fn the_lease_holding_a_part_is_named_by_its_own_id() {
        let mut leases = Leases::new();
        assert_eq!(leases.holding(Part::Body, 0), None);
        let first = leases.take("c_1", &[Part::Body], 30_000, 0).unwrap().lease;
        assert_eq!(leases.holding(Part::Body, S), Some(first.as_str()));
        leases.release("c_1", &first, S).unwrap();
        let again = leases.take("c_1", &[Part::Body], 30_000, S).unwrap().lease;
        assert_ne!(first, again);
        assert_eq!(leases.holding(Part::Body, S), Some(again.as_str()));
        assert_eq!(leases.holding(Part::Light, S), None);
    }

    #[test]
    fn one_client_holds_a_part_at_a_time_and_a_free_part_is_given() {
        let mut leases = Leases::new();
        let held = leases
            .take("c_1", &[Part::Body, Part::Light], 30_000, 0)
            .unwrap();
        assert_eq!(
            held,
            Held {
                lease: "l_1".to_owned(),
                until_us: 30 * S
            }
        );
        assert_eq!(
            leases.take("c_2", &[Part::Light], 1_000, S),
            Err(Refusal::Busy {
                part: Part::Light,
                holder: "c_1".to_owned()
            })
        );
        assert_eq!(
            leases.take("c_1", &[Part::Body], 1_000, S),
            Err(Refusal::Busy {
                part: Part::Body,
                holder: "c_1".to_owned()
            })
        );
        assert_eq!(
            leases.take("c_2", &[Part::Voice], 1_000, S).unwrap().lease,
            "l_2"
        );
        assert_eq!(leases.holder(Part::Body, S), Some("c_1"));
        assert_eq!(leases.holder(Part::Voice, S), Some("c_2"));
        assert!(leases.may_act("c_1", "l_1", Part::Body, S).is_ok());
        assert_eq!(
            leases.may_act("c_2", "l_1", Part::Body, S),
            Err(Refusal::NoLease)
        );
        assert_eq!(
            leases.may_act("c_2", "l_2", Part::Body, S),
            Err(Refusal::NoLease)
        );
    }

    #[test]
    fn a_lease_lapses_unless_renewed_and_is_held_while_renewed_in_time() {
        let mut leases = Leases::new();
        leases.take("c_1", &[Part::Body], 30_000, 0).unwrap();
        assert_eq!(
            leases.renew("c_1", "l_1", 30_000, 20 * S).unwrap().until_us,
            50 * S
        );
        assert!(leases.may_act("c_1", "l_1", Part::Body, 49 * S).is_ok());
        let lapsed = leases.lapse(50 * S);
        assert_eq!(
            lapsed,
            [Change {
                client: "c_1".to_owned(),
                lease: "l_1".to_owned(),
                state: LeaseState::Lost,
                why: Some(LeaseWhy::Expired)
            }]
        );
        assert_eq!(
            leases.may_act("c_1", "l_1", Part::Body, 50 * S),
            Err(Refusal::Lost(LeaseWhy::Expired))
        );
        assert_eq!(
            leases.renew("c_1", "l_1", 1_000, 50 * S),
            Err(Refusal::Lost(LeaseWhy::Expired))
        );
        // Renewed every 20 s, it is still held an hour after its take (the
        // old ten-minute ceiling is gone), and lapses once renewals stop.
        leases.take("c_1", &[Part::Body], 30_000, 100 * S).unwrap();
        let mut now = 100 * S;
        while now < 100 * S + 3_600 * S {
            now += 20 * S;
            let held = leases.renew("c_1", "l_2", 30_000, now).unwrap();
            assert_eq!(held.until_us, now + 30 * S);
        }
        assert!(leases.lapse(now + 29 * S).is_empty());
        assert_eq!(leases.holder(Part::Body, now + 29 * S), Some("c_1"));
        assert_eq!(leases.lapse(now + 30 * S).len(), 1);
        assert_eq!(leases.holder(Part::Body, now + 30 * S), None);
        // A take or renew may ask for up to ten minutes at once.
        let later = now + 100 * S;
        leases.take("c_1", &[Part::Body], 600_000, later).unwrap();
        assert_eq!(
            leases
                .renew("c_1", "l_3", 600_000, later + 500 * S)
                .unwrap()
                .until_us,
            later + 1_100 * S
        );
        assert_eq!(
            leases.take("c_1", &[Part::Body], 0, 0),
            Err(Refusal::BadHold)
        );
        assert_eq!(
            leases.take("c_1", &[Part::Body], 600_001, 0),
            Err(Refusal::BadHold)
        );
        assert_eq!(leases.take("c_1", &[], 1_000, 0), Err(Refusal::BadHold));
    }

    #[test]
    fn a_conversation_suspends_and_its_end_holds_again_while_a_hand_loses_it() {
        let mut leases = Leases::new();
        leases.take("c_1", &[Part::Body], 30_000, 0).unwrap();
        leases.take("c_2", &[Part::Light], 30_000, 0).unwrap();
        let suspended = leases.suspend(Some(Part::Body), LeaseWhy::Conversation, S);
        assert_eq!(suspended.len(), 1);
        assert_eq!(suspended[0].state, LeaseState::Suspended);
        assert_eq!(suspended[0].client, "c_1");
        assert_eq!(
            leases.may_act("c_1", "l_1", Part::Body, S),
            Err(Refusal::Suspended)
        );
        assert!(!leases.holds("c_1", Part::Body, S));
        // Still its: nobody else may take the body meanwhile.
        assert!(matches!(
            leases.take("c_3", &[Part::Body], 1_000, S),
            Err(Refusal::Busy { .. })
        ));
        // Suspending again tells nothing new.
        assert!(
            leases
                .suspend(None, LeaseWhy::Conversation, S)
                .iter()
                .all(|c| c.client != "c_1")
        );
        let resumed = leases.resume(2 * S);
        assert!(
            resumed
                .iter()
                .any(|c| c.client == "c_1" && c.state == LeaseState::Held)
        );
        assert!(leases.may_act("c_1", "l_1", Part::Body, 2 * S).is_ok());
        let lost = leases.lose(Some(Part::Body), LeaseWhy::Hand, 3 * S);
        assert_eq!(
            lost,
            [Change {
                client: "c_1".to_owned(),
                lease: "l_1".to_owned(),
                state: LeaseState::Lost,
                why: Some(LeaseWhy::Hand)
            }]
        );
        assert_eq!(
            leases.may_act("c_1", "l_1", Part::Body, 3 * S),
            Err(Refusal::Lost(LeaseWhy::Hand))
        );
        // The light's holder kept its lease.
        assert!(leases.may_act("c_2", "l_2", Part::Light, 3 * S).is_ok());
        assert_eq!(leases.lose(None, LeaseWhy::Private, 4 * S).len(), 1);
        assert_eq!(leases.holder(Part::Light, 4 * S), None);
    }

    #[test]
    fn a_hand_on_the_body_loses_a_lease_of_body_and_light_whole() {
        let mut leases = Leases::new();
        leases
            .take("c_1", &[Part::Body, Part::Light], 30_000, 0)
            .unwrap();
        let lost = leases.lose(Some(Part::Body), LeaseWhy::Hand, S);
        assert_eq!(lost.len(), 1);
        assert_eq!(leases.holder(Part::Light, S), None);
        assert_eq!(
            leases.may_act("c_1", "l_1", Part::Light, S),
            Err(Refusal::Lost(LeaseWhy::Hand))
        );
    }

    #[test]
    fn one_clients_churn_never_hides_anothers_why() {
        let mut leases = Leases::new();
        leases.take("c_1", &[Part::Body], 30_000, 0).unwrap();
        leases.lose(None, LeaseWhy::Hand, S);
        for n in 0..100u64 {
            leases
                .take("c_2", &[Part::Voice], 1_000, 2 * S + n)
                .unwrap();
            leases.lose(None, LeaseWhy::Paused, 2 * S + n);
        }
        assert_eq!(
            leases.may_act("c_1", "l_1", Part::Body, 3 * S),
            Err(Refusal::Lost(LeaseWhy::Hand))
        );
    }

    #[test]
    fn a_lapse_found_by_an_act_or_a_take_is_still_told_once() {
        let mut leases = Leases::new();
        leases.take("c_1", &[Part::Body], 1_000, 0).unwrap();
        assert_eq!(
            leases.may_act("c_1", "l_1", Part::Body, 2 * S),
            Err(Refusal::Lost(LeaseWhy::Expired))
        );
        leases.take("c_2", &[Part::Body], 1_000, 2 * S).unwrap();
        let told = leases.lapse(2 * S);
        assert_eq!(told.len(), 1);
        assert_eq!(told[0].client, "c_1");
        assert!(leases.lapse(2 * S).is_empty());
    }

    #[test]
    fn a_suspended_lease_that_lapses_is_lost_not_held_again() {
        let mut leases = Leases::new();
        leases.take("c_1", &[Part::Body], 5_000, 0).unwrap();
        leases.suspend(None, LeaseWhy::Conversation, S);
        let resumed = leases.resume(10 * S);
        assert_eq!(resumed.len(), 1);
        assert_eq!(resumed[0].state, LeaseState::Lost);
        assert_eq!(resumed[0].why, Some(LeaseWhy::Expired));
    }

    #[test]
    fn release_and_a_client_gone_end_its_leases_only() {
        let mut leases = Leases::new();
        leases.take("c_1", &[Part::Body], 30_000, 0).unwrap();
        leases.take("c_1", &[Part::Voice], 30_000, 0).unwrap();
        leases.take("c_2", &[Part::Light], 30_000, 0).unwrap();
        assert_eq!(leases.release("c_2", "l_1", S), Err(Refusal::NoLease));
        assert_eq!(leases.release("c_1", "l_1", S), Ok(()));
        assert_eq!(
            leases.may_act("c_1", "l_1", Part::Body, S),
            Err(Refusal::Lost(LeaseWhy::Released))
        );
        let gone = leases.client_gone("c_1", LeaseWhy::Revoked, 2 * S);
        assert_eq!(gone.len(), 1);
        assert_eq!(gone[0].lease, "l_2");
        assert_eq!(leases.holder(Part::Voice, 2 * S), None);
        assert_eq!(leases.holder(Part::Light, 2 * S), Some("c_2"));
    }
}
