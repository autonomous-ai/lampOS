//! Ephemeral constraints from held lines in one caller's conversation.
//! No inference, wait, time window, admission or persistence is owned here.
use super::CARRY_NEARER_BY;

const MAX_HELD: usize = 8;

#[derive(Default)]
pub(super) struct HeldColleagues {
    first_request: Option<u64>,
    prints: Vec<(u64, Option<Vec<f32>>)>,
    exhausted: bool,
}

impl HeldColleagues {
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    pub(super) fn begin(&mut self, request: u64) {
        self.clear();
        self.first_request = Some(request);
    }

    pub(super) fn hold(&mut self, request: u64, print: Option<Vec<f32>>) {
        if self.first_request.is_none_or(|first| request < first) {
            return;
        }
        if let Some((_, kept)) = self.prints.iter_mut().find(|(of, _)| *of == request) {
            if kept.is_none() {
                *kept = print;
            }
            return;
        }
        if self.prints.len() == MAX_HELD {
            // Evicting a constraint would let an older visitor pass. Keep
            // the bound and require a face until a fresh call instead.
            self.exhausted = true;
            return;
        }
        self.prints.push((request, print));
    }

    pub(super) fn nearer(
        &mut self,
        request: u64,
        own: Option<f32>,
        mut cached_print: impl FnMut(u64) -> Option<Vec<f32>>,
    ) -> bool {
        if self.first_request.is_none_or(|first| request < first) || self.exhausted {
            return false;
        }
        if self.prints.is_empty() {
            return true;
        }
        let Some(own) = own.filter(|own| own.is_finite()) else {
            return false;
        };
        // Fill only already requested, cached replies. A missing or evicted
        // reply stays a constraint requiring a face, never another's print.
        for (of, print) in &mut self.prints {
            if print.is_none() {
                *print = cached_print(*of);
            }
        }
        let Some(current) = cached_print(request) else {
            return false;
        };
        self.prints.iter().all(|(_, held)| {
            held.as_ref()
                .and_then(|held| crate::same_voice::cosine(&current, held))
                .is_some_and(|other| own >= other + CARRY_NEARER_BY)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_held_print_constrains_the_voice_without_eviction() {
        let mut held = HeldColleagues::default();
        held.begin(1);
        held.hold(2, Some(vec![0.75, 0.661_4]));
        held.hold(4, Some(vec![-1.0, 0.0]));
        assert!(!held.nearer(6, Some(0.75), |_| Some(vec![0.75, 0.661_4])));
        assert!(held.nearer(7, Some(1.0), |_| Some(vec![1.0, 0.0])));
        assert_eq!(held.prints.len(), 2);
    }

    #[test]
    fn a_missing_held_print_needs_its_own_cached_reply() {
        let mut held = HeldColleagues::default();
        held.begin(1);
        held.hold(2, None);
        held.hold(3, Some(vec![-1.0, 0.0]));
        assert!(!held.nearer(4, Some(1.0), |of| (of == 4).then(|| vec![1.0, 0.0])));
        let mut reads = Vec::new();
        assert!(held.nearer(4, Some(1.0), |of| {
            reads.push(of);
            Some(if of == 2 {
                vec![0.0, 1.0]
            } else {
                vec![1.0, 0.0]
            })
        }));
        assert_eq!(reads, [2, 4]);
        assert!(held.nearer(5, Some(1.0), |_| Some(vec![1.0, 0.0])));
    }

    #[test]
    fn duplicates_fill_missing_evidence_without_replacing_known_evidence() {
        let mut held = HeldColleagues::default();
        held.begin(1);
        held.hold(2, None);
        held.hold(2, Some(vec![1.0, 0.0]));
        held.hold(2, Some(vec![-1.0, 0.0]));
        held.hold(2, None);
        assert_eq!(held.prints.len(), 1);
        assert!(!held.nearer(3, Some(1.0), |_| Some(vec![1.0, 0.0])));
    }

    #[test]
    fn capacity_exhaustion_stays_face_only_until_a_fresh_call() {
        let mut held = HeldColleagues::default();
        held.begin(1);
        for request in 2..=MAX_HELD as u64 + 1 {
            held.hold(request, Some(vec![-1.0, 0.0]));
        }
        assert!(held.nearer(20, Some(1.0), |_| Some(vec![1.0, 0.0])));
        held.hold(2, None);
        assert!(!held.exhausted);
        held.hold(20, Some(vec![-1.0, 0.0]));
        assert_eq!(held.prints.len(), MAX_HELD);
        assert!(!held.nearer(21, Some(1.0), |_| panic!("exhausted: no cache read")));
        held.begin(30);
        assert!(held.nearer(31, Some(1.0), |_| panic!("no held line: no cache read")));
    }

    #[test]
    fn closed_scopes_and_old_callbacks_cannot_restore_constraints() {
        let mut held = HeldColleagues::default();
        held.hold(2, Some(vec![1.0, 0.0]));
        assert!(held.prints.is_empty());
        held.begin(10);
        held.hold(9, Some(vec![1.0, 0.0]));
        assert!(held.prints.is_empty());
        held.hold(11, Some(vec![1.0, 0.0]));
        held.clear();
        held.hold(11, Some(vec![1.0, 0.0]));
        assert!(held.prints.is_empty());
        assert!(!held.nearer(12, Some(1.0), |_| panic!("closed: no cache read")));
        held.begin(20);
        held.hold(11, Some(vec![1.0, 0.0]));
        assert!(held.nearer(21, Some(1.0), |_| None));
        assert!(!held.nearer(19, Some(1.0), |_| None));
    }
}
