mod lamp_bus {pub const EXPIRY_GRACE_MS:u64=50;}
const FACES_KEPT_MS: u64 = 2_500;
/// And at most this many, whatever the eyes' pace.
const FACES_LOOKS_KEPT: usize = 16;
/// A look over this old when taken is refused; a gap between two looks
/// over this, a look dated before the one kept last or ahead of now, or a
/// verdict this long after the newest look, is a break: what was kept is
/// dropped (reviewer-two's HOLDs on #3355). The eyes' slowest pace is
/// 600 ms.
const LOOK_GAP_MS: u64 = 1_000;
/// How many kept looks must see two faces or more for the room to be busy.
const TWO_FACES_LOOKS: usize = 3;
/// How long, from the first such look to the last, two faces must stay.
const TWO_FACES_FOR_MS: u64 = 1_500;

/// One look the eyes published, as [`FacesHeld`] keeps it.
#[derive(Clone, Copy)]
struct FacesLook {
    /// When the eyes looked (`header.at_us`): their `lamp_ipc::monotonic_us`,
    /// the system-wide clock the voice reads too (reviewer-two on #3355), so
    /// every age, expiry and span is on one clock no wall step can move.
    at_us: u64,
    faces: usize,
    /// The header's `expires_ms`, from when the eyes looked.
    expires_ms: Option<u64>,
}

/// Every look the eyes published (`humans/faces/tracked`), read on a
/// thread of its own as they come (`LAMP_TWO_FACES_HELD=on`): two faces
/// make the room busy only when the newest fresh look sees two and at
/// least [`TWO_FACES_LOOKS`] of those in the last [`FACES_KEPT_MS`] did,
/// spanning [`TWO_FACES_FOR_MS`] or more, so a face passing by, a misread
/// or a photo frame seen once does not. Sampling once per line instead
/// made it 3 lines of 4 (the coordinator's finding on #3355). A privacy
/// change drops what was kept and refuses every look dated at or before
/// it, and every look the reader took across it (`epoch`), so looks
/// queued before it never come back.
#[derive(Default)]
struct FacesHeld {
    looks: std::collections::VecDeque<FacesLook>,
    epoch: u64,
    cutoff_us: Option<u64>,
}

impl FacesHeld {
    /// Keep a look the reader took at `now_us`, `epoch` read before it
    /// began to wait for it.
    fn look(&mut self, look: FacesLook, epoch: u64, now_us: u64) {
        if epoch != self.epoch || self.cutoff_us.is_some_and(|cutoff| look.at_us <= cutoff) {
            return;
        }
        let ahead = look.at_us > now_us + lamp_bus::EXPIRY_GRACE_MS * 1_000;
        if ahead
            || self.looks.back().is_some_and(|last| {
                look.at_us < last.at_us || look.at_us - last.at_us > LOOK_GAP_MS * 1_000
            })
        {
            self.looks.clear();
        }
        if ahead || Self::stale(&look, now_us) {
            return;
        }
        if self.looks.len() == FACES_LOOKS_KEPT {
            self.looks.pop_front();
        }
        self.looks.push_back(look);
    }

    /// Whether a look is over [`LOOK_GAP_MS`] old at `now_us`, or expired
    /// as [`lamp_bus::Header::expired_at`] has it.
    fn stale(look: &FacesLook, now_us: u64) -> bool {
        let age_ms = now_us.saturating_sub(look.at_us) / 1_000;
        age_ms > LOOK_GAP_MS
            || look
                .expires_ms
                .is_some_and(|lasts| age_ms > lasts + lamp_bus::EXPIRY_GRACE_MS)
    }

    /// Privacy closed or reopened at `now_us`: nothing before it counts.
    fn privacy_changed(&mut self, now_us: u64) {
        self.looks.clear();
        self.epoch += 1;
        self.cutoff_us = Some(self.cutoff_us.map_or(now_us, |cutoff| cutoff.max(now_us)));
    }

    /// Whether two faces have stayed at `now_us`. A missing or expired
    /// newest look ends what was kept.
    fn two_stayed(&mut self, now_us: u64) -> bool {
        while self
            .looks
            .front()
            .is_some_and(|look| now_us.saturating_sub(look.at_us) > FACES_KEPT_MS * 1_000)
        {
            self.looks.pop_front();
        }
        let Some(newest) = self.looks.back().copied() else {
            return false;
        };
        if Self::stale(&newest, now_us) {
            self.looks.clear();
            return false;
        }
        if newest.faces < 2 {
            return false;
        }
        let mut two = self.looks.iter().filter(|look| look.faces >= 2);
        let count = two.clone().count();
        let first = two.next().map_or(0, |look| look.at_us);
        let last = two.next_back().map_or(first, |look| look.at_us);
        count >= TWO_FACES_LOOKS && last.saturating_sub(first) >= TWO_FACES_FOR_MS * 1_000
    }
}


#[cfg(test)] mod controls {
 use super::*;
 fn look(h:&mut FacesHeld,at_ms:u64,taken_ms:u64,expires:Option<u64>){let epoch=h.epoch;h.look(FacesLook{at_us:at_ms*1000,faces:2,expires_ms:expires},epoch,taken_ms*1000);}
 fn positive()->FacesHeld{let mut h=FacesHeld::default();for at in [100000,100750,101500]{look(&mut h,at,at,Some(2000))}h}
 #[test] fn sustained_positive(){assert!(positive().two_stayed(101600000));}
 #[test] fn ordinary_short_span_is_not_busy(){let mut h=FacesHeld::default();for at in [100000,100250,100500]{look(&mut h,at,at,Some(2000))}assert!(!h.two_stayed(100500000));}
 #[test] fn original_forward_wall_step_trace_is_not_busy(){let mut h=FacesHeld::default();let trace=[(100000,100000),(100750,100250),(101000,100500),(101250,100750),(101500,101000)];for (_,mono) in trace{look(&mut h,mono,mono,Some(2000))}assert_eq!(trace[4].0-trace[0].0,1500);assert_eq!(trace[4].1-trace[0].1,1000);assert!(!h.two_stayed(101000000));}
 #[test] fn late_admission_is_refused(){let mut h=FacesHeld::default();for (at,taken) in [(100000,101900),(100750,102000),(101500,103400)]{look(&mut h,at,taken,Some(2000))}assert!(h.looks.is_empty());assert!(!h.two_stayed(104300000));}
 #[test] fn fresh_delayed_looks_keep_original_expiry(){let mut h=FacesHeld::default();for at in [100000,100750,101500]{look(&mut h,at,at+400,Some(2000))}assert!(h.two_stayed(102000000));assert!(!h.two_stayed(102600000));}
 #[test] fn privacy_refuses_queued_and_cross_epoch_looks(){let mut h=positive();let old=h.epoch;h.privacy_changed(101550000);h.privacy_changed(101550000);h.look(FacesLook{at_us:101600000,faces:2,expires_ms:Some(2000)},old,101650000);for at in [100000,100750,101500]{look(&mut h,at,101600,Some(2000))}assert!(h.looks.is_empty());for at in [101700,102450,103200]{look(&mut h,at,at,Some(2000))}assert!(h.two_stayed(103250000));}
 #[test] fn no_stream_expires_and_short_ttl_does_not_restart(){assert!(!positive().two_stayed(102501000));let mut h=FacesHeld::default();for at in [100000,100750,101500]{look(&mut h,at,at,Some(500))}assert!(h.two_stayed(102000000));assert!(!h.two_stayed(102051000));let mut stale=FacesHeld::default();look(&mut stale,100000,100551,Some(500));assert!(stale.looks.is_empty());}
 #[test] fn future_and_reordered_messages_break_history(){let mut h=positive();look(&mut h,101800,101600,Some(2000));assert!(h.looks.is_empty());let mut h=positive();look(&mut h,101000,101600,Some(2000));assert!(!h.two_stayed(101600000));}
}
