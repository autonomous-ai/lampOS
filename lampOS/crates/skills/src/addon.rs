//! An add-on's proposals as the program sends them (`docs/addons.md`): each
//! numbered as it leaves, and held to the add-on limits, so that a partner's
//! skill cannot flood the judge however its triggers are written. The judge
//! keeps or drops what passes; nothing here is an output.
use lamp_msgs::AddonProposal;
use std::collections::{HashMap, VecDeque};

/// One proposal an add-on every this long at most.
pub const EVERY_US: u64 = 10_000_000;
/// And this many an hour at most, an add-on.
pub const AN_HOUR: usize = 20;
/// `say` proposals an hour, between all the add-ons: the lamp speaking
/// unprompted is rare, whoever suggests it.
pub const SAYS_AN_HOUR: usize = 4;
const HOUR_US: u64 = 3_600_000_000;

/// What the add-ons have proposed lately, by add-on.
#[derive(Debug, Default)]
pub struct Limits {
    sent: HashMap<String, VecDeque<u64>>,
    says: VecDeque<u64>,
    last: u64,
}

impl Limits {
    /// The proposal numbered and counted, or why it is not sent. `now_us`
    /// is the monotonic clock; the number is it, or one past the last
    /// number when the clock has not moved, so numbers only grow.
    pub fn admit(
        &mut self,
        proposal: &AddonProposal,
        now_us: u64,
    ) -> Result<AddonProposal, String> {
        let sent = self.sent.entry(proposal.addon.clone()).or_default();
        sent.retain(|at| now_us.saturating_sub(*at) < HOUR_US);
        self.says.retain(|at| now_us.saturating_sub(*at) < HOUR_US);
        if sent
            .back()
            .is_some_and(|last| now_us.saturating_sub(*last) < EVERY_US)
        {
            return Err(format!(
                "not proposed: one proposal every {} s an add-on",
                EVERY_US / 1_000_000
            ));
        }
        if sent.len() >= AN_HOUR {
            return Err(format!(
                "not proposed: {AN_HOUR} proposals an hour an add-on"
            ));
        }
        if proposal.act == "say" && self.says.len() >= SAYS_AN_HOUR {
            return Err(format!(
                "not proposed: {SAYS_AN_HOUR} say proposals an hour between all add-ons"
            ));
        }
        let number = now_us.max(self.last + 1);
        let numbered = AddonProposal {
            proposal: number,
            ..proposal.clone()
        };
        numbered
            .check()
            .map_err(|lamp_msgs::Invalid(why)| format!("not proposed: {why}"))?;
        self.last = number;
        sent.push_back(now_us);
        if proposal.act == "say" {
            self.says.push_back(now_us);
        }
        Ok(numbered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mood(addon: &str) -> AddonProposal {
        AddonProposal {
            proposal: 0,
            addon: addon.into(),
            version: "1.0.0".into(),
            act: "mood".into(),
            why: "music".into(),
            sure: 0.7,
            holds_ms: 30_000,
            light: None,
            gesture: None,
            mood: Some(lamp_msgs::AddonMood {
                energy: Some(0.1),
                warmth: None,
                for_ms: 600_000,
            }),
            note: None,
        }
    }

    fn say(addon: &str) -> AddonProposal {
        AddonProposal {
            act: "say".into(),
            mood: None,
            note: Some("Music is playing.".into()),
            ..mood(addon)
        }
    }

    #[test]
    fn proposals_are_numbered_in_order_and_spaced_per_add_on() {
        let mut limits = Limits::default();
        let first = limits.admit(&mood("a"), 1_000).unwrap();
        assert_eq!(first.proposal, 1_000);
        // Another add-on at the same instant: its own number, one past.
        assert_eq!(limits.admit(&mood("b"), 1_000).unwrap().proposal, 1_001);
        assert!(limits.admit(&mood("a"), 1_000 + EVERY_US - 1).is_err());
        assert_eq!(
            limits.admit(&mood("a"), 1_000 + EVERY_US).unwrap().proposal,
            1_000 + EVERY_US
        );
    }

    #[test]
    fn an_add_on_has_twenty_an_hour_and_all_have_four_says() {
        let mut limits = Limits::default();
        for n in 0..AN_HOUR as u64 {
            limits.admit(&mood("a"), 1 + n * EVERY_US).unwrap();
        }
        let next = 1 + AN_HOUR as u64 * EVERY_US;
        assert!(
            limits
                .admit(&mood("a"), next)
                .unwrap_err()
                .contains("an hour")
        );
        // An hour after the first, its place is free again.
        assert!(limits.admit(&mood("a"), 1 + HOUR_US).is_ok());

        let mut limits = Limits::default();
        for (n, addon) in ["a", "b", "c", "d"].iter().enumerate() {
            limits.admit(&say(addon), 1 + n as u64).unwrap();
        }
        assert!(limits.admit(&say("e"), 10).unwrap_err().contains("say"));
        // A mood is not a say.
        assert!(limits.admit(&mood("e"), 10).is_ok());
    }

    #[test]
    fn a_proposal_that_fails_its_check_is_not_sent_or_counted() {
        let mut limits = Limits::default();
        let wrong = AddonProposal {
            sure: 2.0,
            ..mood("a")
        };
        assert!(limits.admit(&wrong, 1).is_err());
        assert!(limits.admit(&mood("a"), 2).is_ok());
    }
}
