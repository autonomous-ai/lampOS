//! One held colleague line may extend a carry from its actual audio end.
//! No timer, I/O, identity decision or admission is owned here.
use super::SAME_SPEAKER_CARRIES_FOR;

const MAX_CARRY_US: u64 = 40_000_000;
const RECENT_ENDS: usize = 4;

#[derive(Clone, Default)]
pub(super) struct CarryWindow {
    first_request: Option<u64>,
    answer: Option<(u64, u64)>,
    used: bool,
    held_end: Option<u64>,
    ends: std::collections::VecDeque<(u64, u64)>,
}

impl CarryWindow {
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    /// Whether nothing is kept: no conversation carried.
    #[cfg(test)]
    pub(super) fn is_clear(&self) -> bool {
        self.first_request.is_none()
            && self.answer.is_none()
            && !self.used
            && self.held_end.is_none()
            && self.ends.is_empty()
    }

    /// The request the window was begun, or last answered, for: a window
    /// begun since for a new caller has another.
    pub(super) fn first_request(&self) -> Option<u64> {
        self.first_request
    }

    pub(super) fn begin(&mut self, request: u64) {
        self.clear();
        self.first_request = Some(request);
    }

    /// Only a new actual answer resets the allowance. Later audio chunks
    /// of that answer advance its end without giving another allowance.
    pub(super) fn answered(&mut self, request: u64, end: u64) {
        if end == 0 || self.first_request.is_none_or(|first| request < first) {
            return;
        }
        match self.answer {
            Some((old, _)) if request < old => return,
            Some((old, old_end)) if request == old => {
                self.answer = Some((request, old_end.max(end)));
                return;
            }
            _ => {}
        }
        self.first_request = Some(request);
        self.answer = Some((request, end));
        self.used = false;
        self.held_end = None;
        self.ends.clear();
    }

    /// Keep at most four eligible audio ends for a delayed classification.
    /// Future, stale and duplicate timestamps cannot move a deadline.
    pub(super) fn remember(&mut self, request: u64, end: u64, now: u64) {
        if end > now
            || self.answer.is_none_or(|(answered, _)| request <= answered)
            || self.ends.iter().any(|(old, _)| *old == request)
        {
            return;
        }
        if self.ends.len() == RECENT_ENDS {
            self.ends.pop_front();
        }
        self.ends.push_back((request, end));
    }

    pub(super) fn held(&mut self, request: u64) {
        if self.used || self.answer.is_none_or(|(answered, _)| request <= answered) {
            return;
        }
        self.used = true;
        self.held_end = self
            .ends
            .iter()
            .find(|(old, _)| *old == request)
            .map(|(_, end)| *end);
    }

    /// The ordinary twenty-second rule remains at the caller. This only
    /// adds time when a held line actually ended after the lamp's words.
    pub(super) fn extra_allows(&self, began: u64) -> bool {
        let (Some((_, answer)), Some(held)) = (self.answer, self.held_end) else {
            return false;
        };
        if held <= answer || began < answer {
            return false;
        }
        let deadline = held
            .saturating_add(SAME_SPEAKER_CARRIES_FOR.as_micros() as u64)
            .min(answer.saturating_add(MAX_CARRY_US));
        began <= deadline
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const S: u64 = 1_000_000;

    fn answered() -> CarryWindow {
        let mut window = CarryWindow::default();
        window.begin(1);
        window.answered(1, 60 * S);
        window
    }

    #[test]
    fn actual_end_sets_an_inclusive_deadline_and_forty_second_cap() {
        for (end, deadline) in [(65 * S, 85 * S), (95 * S, 100 * S)] {
            let mut window = answered();
            window.remember(2, end, 200 * S);
            window.held(2);
            assert!(window.extra_allows(deadline));
            assert!(!window.extra_allows(deadline + 1));
            assert!(!window.extra_allows(59 * S));
        }
    }

    #[test]
    fn repeated_or_late_classification_cannot_renew_the_allowance() {
        let mut window = answered();
        window.remember(2, 65 * S, 200 * S);
        window.held(2);
        window.remember(2, 90 * S, 200 * S);
        window.remember(3, 90 * S, 200 * S);
        window.held(3);
        window.answered(1, 61 * S); // another chunk, not a new answer
        assert!(window.extra_allows(85 * S));
        assert!(!window.extra_allows(85 * S + 1));
        window.answered(3, 90 * S);
        window.remember(4, 95 * S, 200 * S);
        window.held(4);
        assert!(window.extra_allows(115 * S));
        assert!(!window.extra_allows(115 * S + 1));
    }

    #[test]
    fn missing_future_and_stale_ends_do_not_create_an_extension() {
        let mut window = answered();
        window.remember(2, 90 * S, 80 * S);
        window.held(2);
        window.remember(2, 90 * S, 100 * S);
        window.held(2);
        assert!(!window.extra_allows(90 * S));
        window.begin(5);
        window.answered(1, 95 * S);
        window.remember(6, 96 * S, 100 * S);
        window.held(6);
        assert!(!window.extra_allows(100 * S));
    }

    #[test]
    fn clearing_cannot_be_undone_by_an_old_answer_or_print() {
        let mut window = answered();
        window.remember(2, 65 * S, 100 * S);
        window.held(2);
        window.clear();
        window.answered(1, 90 * S);
        window.held(2);
        assert!(!window.extra_allows(85 * S));
        assert!(window.ends.is_empty());
    }

    #[test]
    fn delayed_evidence_storage_is_bounded_and_older_ends_expire() {
        let mut window = answered();
        for request in 2..=10 {
            window.remember(request, 65 * S, 100 * S);
        }
        assert_eq!(window.ends.len(), RECENT_ENDS);
        window.held(2);
        assert!(!window.extra_allows(85 * S));
        window.held(10); // the missing first end cannot buy a second try
        assert!(!window.extra_allows(85 * S));
    }
}
