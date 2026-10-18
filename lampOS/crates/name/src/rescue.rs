//! With `--rescue-model`, a second name model rescues a call the first one
//! half heard (#1546).
//!
//! The installed `name.onnx` hears few bare "Lamp," calls through lamp-4ace's
//! speaker and canceller, but almost never hears the name where it was not
//! said. A model trained on the lamp's own recordings hears far more calls,
//! and far more that were not calls. Together: a step's score is the
//! installed model's, unless the installed model reached the gate at this
//! step or in the [`LOOK_BACK`] - 1 steps before it (1 s at 80 ms a step),
//! and then it is the higher of the two. So the installed model decides
//! where a call may be, and the trained one only how sure it is there.
//!
//! On the second sitting's recordings (18 Oct, 99 played calls and 18.9 min
//! of held-out negatives), with the gate at 0.5: 51 calls heard at 0.98
//! against the installed model's 35, and no false hearing at 0.98 for
//! either (`training/name/lamp.py --rescue`, `docs/bare-name.md`).
use std::collections::VecDeque;

/// The steps the gate looks back over, this one included: 1 s.
pub const LOOK_BACK: u64 = 13;

pub struct Rescue {
    gate: f32,
    /// The installed model's scores that may still open the gate, by step.
    recent: VecDeque<(u64, f32)>,
}

impl Rescue {
    /// A rescue that opens where the installed model reached `gate`.
    pub fn new(gate: f32) -> Self {
        Self {
            gate,
            recent: VecDeque::with_capacity(LOOK_BACK as usize),
        }
    }

    /// Steps are counted again from a forget: what came before is not of
    /// the steps that follow.
    pub fn forget(&mut self) {
        self.recent.clear();
    }

    /// The score the hearing rule judges at `step`: `name`, or where `name`
    /// reached the gate within the look back, the higher of it and `rescue`.
    /// Steps missing between two calls (the models rested) open nothing.
    pub fn score(&mut self, step: u64, name: f32, rescue: f32) -> f32 {
        if self.recent.back().is_some_and(|(at, _)| *at >= step) {
            self.recent.clear();
        }
        while self
            .recent
            .front()
            .is_some_and(|(at, _)| at + LOOK_BACK <= step)
        {
            self.recent.pop_front();
        }
        self.recent.push_back((step, name));
        if self.recent.iter().any(|(_, score)| *score >= self.gate) {
            name.max(rescue)
        } else {
            name
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rule `lamp.py --rescue` measured (`rescued`): the gate looks
    /// back 13 steps, this one included, and opens only for the higher of
    /// the two scores.
    #[test]
    fn the_trained_score_counts_only_within_a_second_of_the_installed_one_reaching_the_gate() {
        let mut rescue = Rescue::new(0.5);
        // No gate yet: the installed score, however sure the rescue is.
        assert_eq!(rescue.score(1, 0.1, 0.99), 0.1);
        // The gate reached: the higher of the two, at once and for 12 more.
        assert_eq!(rescue.score(2, 0.6, 0.3), 0.6);
        assert_eq!(rescue.score(3, 0.2, 0.99), 0.99);
        for step in 4..=14 {
            assert_eq!(rescue.score(step, 0.0, 0.99), 0.99, "{step}");
        }
        // Step 15 is 13 after the gate: closed.
        assert_eq!(rescue.score(15, 0.0, 0.99), 0.0);
        // A lower installed score stays as it is when the rescue is lower.
        assert_eq!(rescue.score(16, 0.7, 0.2), 0.7);
    }

    /// Steps the models rested through, and a forget, close the gate: a
    /// score before them is not within a second of the one after.
    #[test]
    fn a_rest_or_a_forget_closes_the_gate() {
        let mut rescue = Rescue::new(0.5);
        assert_eq!(rescue.score(10, 0.9, 0.0), 0.9);
        // Rested from step 11 to step 40.
        assert_eq!(rescue.score(41, 0.1, 0.99), 0.1);
        assert_eq!(rescue.score(42, 0.9, 0.0), 0.9);
        rescue.forget();
        // Counted from 1 again after a forget.
        assert_eq!(rescue.score(1, 0.1, 0.99), 0.1);
        // A step counted back (a forget the caller did not pass on) opens
        // nothing from before it.
        assert_eq!(rescue.score(2, 0.1, 0.0), 0.1);
        assert_eq!(rescue.score(5, 0.9, 0.0), 0.9);
        assert_eq!(rescue.score(3, 0.1, 0.99), 0.1);
    }
}
