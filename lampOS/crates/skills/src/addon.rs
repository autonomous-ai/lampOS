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
/// unprompted is rare, whoever suggests it. One the judge drops is not
/// counted ([`Limits::dropped`]): a talking room, where every say is
/// dropped, never spends the hour before its quiet (lamp-75c9, 22 Oct
/// 07:25).
pub const SAYS_AN_HOUR: usize = 4;
/// After the judge drops one of its says, an add-on proposes nothing for
/// this long: a talking room is never asked every few seconds, and its
/// first quiet still gets one say (the first builder on #2530, 07:3x).
pub const AFTER_A_DROP_US: u64 = 300_000_000;
const HOUR_US: u64 = 3_600_000_000;

/// What the add-ons have proposed lately, by add-on.
#[derive(Debug, Default)]
pub struct Limits {
    sent: HashMap<String, VecDeque<u64>>,
    /// The says proposed within the hour: their numbers, add-ons and when.
    says: VecDeque<(u64, String, u64)>,
    /// When the judge last dropped a say of each add-on's.
    dropped: HashMap<String, u64>,
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
        self.says
            .retain(|(_, _, at)| now_us.saturating_sub(*at) < HOUR_US);
        if self
            .dropped
            .get(&proposal.addon)
            .is_some_and(|at| now_us.saturating_sub(*at) < AFTER_A_DROP_US)
        {
            return Err(format!(
                "not proposed: {} s after the judge dropped its say",
                AFTER_A_DROP_US / 1_000_000
            ));
        }
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
            self.says
                .push_back((number, proposal.addon.clone(), now_us));
        }
        Ok(numbered)
    }

    /// The judge dropped say `proposal` at `now_us`: it no longer counts
    /// toward [`SAYS_AN_HOUR`], and its add-on waits [`AFTER_A_DROP_US`]
    /// before proposing again. A number not counted here changes nothing.
    pub fn dropped(&mut self, proposal: u64, now_us: u64) {
        let Some(at) = self
            .says
            .iter()
            .position(|(number, ..)| *number == proposal)
        else {
            return;
        };
        let (_, addon, _) = self.says.remove(at).expect("found above");
        self.dropped.insert(addon, now_us);
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

    /// lamp-75c9, 22 Oct 07:25: in a talking room the judge dropped four
    /// says, and the quiet after it found the hour spent. A say the judge
    /// drops no longer counts; one it keeps, or has not yet decided, does.
    #[test]
    fn a_say_the_judge_dropped_does_not_spend_the_hours_four() {
        let mut limits = Limits::default();
        let numbers: Vec<u64> = ["a", "b", "c", "d"]
            .iter()
            .enumerate()
            .map(|(n, addon)| limits.admit(&say(addon), 1 + n as u64).unwrap().proposal)
            .collect();
        assert!(limits.admit(&say("e"), 10).is_err(), "four counted");
        // The judge drops two of them: two places free.
        limits.dropped(numbers[0], 10);
        limits.dropped(numbers[2], 10);
        // A number this part never counted changes nothing.
        limits.dropped(999_999, 10);
        assert!(limits.admit(&say("e"), 11).is_ok());
        assert!(limits.admit(&say("f"), 12).is_ok());
        assert!(limits.admit(&say("g"), 13).is_err(), "four counted again");
    }

    /// A drop holds that add-on, and only it, five minutes: not a say
    /// asked again every few seconds of a talking room.
    #[test]
    fn a_drop_holds_that_add_ons_next_proposal_five_minutes() {
        let mut limits = Limits::default();
        let number = limits.admit(&say("a"), 1).unwrap().proposal;
        limits.admit(&say("b"), 2).unwrap();
        limits.dropped(number, 1_000);
        let why = limits
            .admit(&mood("a"), 1_000 + AFTER_A_DROP_US - 1)
            .unwrap_err();
        assert!(why.contains("after the judge dropped"), "{why}");
        assert!(limits.admit(&say("b"), 1_000 + EVERY_US).is_ok());
        assert!(limits.admit(&say("a"), 1_000 + AFTER_A_DROP_US).is_ok());
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
