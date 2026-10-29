//! How many speaker underruns are recovered before they are a fault (#3697).
//!
//! An underrun is ALSA's EPIPE outside a declared drain: the speaker worker
//! was held up past its 40 ms queue. Once, it costs a re-prime and never the
//! voice; on lamp-52e6 it came 0.39 s into a start, as the models loaded.
//! Repeated, the device or the scheduling is failing, and it is a fault as
//! any other discontinuity, so the restart guard and the journal see it.

/// Underruns recovered within `UNDERRUN_WINDOW_US`; one more is a fault.
/// Each costs one 40 ms gap and a 20 ms prime through the barrier, so three
/// are at most a fifth of a second of a ten-second stretch: still a held-up
/// worker, not a speaker that has stopped keeping time.
pub const MAX_UNDERRUNS: usize = 3;

/// The window `MAX_UNDERRUNS` is counted in: long enough to span one start's
/// model load and a busy turn after it, short enough that a single hold-up
/// an hour apart never adds to the next.
pub const UNDERRUN_WINDOW_US: u64 = 10_000_000;

/// The times of the most recent recovered underruns. Fixed size: nothing is
/// allocated on the speaker's tick.
#[derive(Debug, Default)]
pub struct UnderrunBudget {
    recent: [Option<u64>; MAX_UNDERRUNS],
}

impl UnderrunBudget {
    /// Whether an underrun at `at_us` may be recovered, recording it if so.
    /// One past `MAX_UNDERRUNS` within the window is refused and not recorded.
    pub fn admit(&mut self, at_us: u64) -> bool {
        for slot in &mut self.recent {
            if slot.is_some_and(|then| at_us.saturating_sub(then) >= UNDERRUN_WINDOW_US) {
                *slot = None;
            }
        }
        match self.recent.iter_mut().find(|slot| slot.is_none()) {
            Some(slot) => {
                *slot = Some(at_us);
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_underruns_in_the_window_are_recovered_and_a_fourth_is_a_fault() {
        let mut budget = UnderrunBudget::default();
        let start = 5_000_000;
        for n in 0..MAX_UNDERRUNS as u64 {
            assert!(budget.admit(start + n * 1_000_000), "underrun {n}");
        }
        assert!(!budget.admit(start + 3_000_000));
    }

    #[test]
    fn underruns_spaced_wider_than_the_window_are_recovered_each_time() {
        let mut budget = UnderrunBudget::default();
        for n in 0..10 {
            assert!(budget.admit(1 + n * UNDERRUN_WINDOW_US), "underrun {n}");
        }
    }

    #[test]
    fn a_refused_underrun_is_not_counted_and_the_window_moves_on() {
        let mut budget = UnderrunBudget::default();
        for n in 0..MAX_UNDERRUNS as u64 {
            assert!(budget.admit(n));
        }
        assert!(!budget.admit(MAX_UNDERRUNS as u64));
        // The first fell out of the window; the refused one never entered it.
        assert!(budget.admit(UNDERRUN_WINDOW_US));
        assert!(!budget.admit(UNDERRUN_WINDOW_US));
    }
}
