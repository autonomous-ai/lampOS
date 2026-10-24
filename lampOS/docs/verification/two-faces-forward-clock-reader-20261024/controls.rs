mod lamp_bus { pub const EXPIRY_GRACE_MS:u64=50; }
const FACES_KEPT_MS: u64 = 2_500;
/// And at most this many, whatever the eyes' pace.
const FACES_LOOKS_KEPT: usize = 16;
/// A gap between two looks over this, by the eyes' clock, a look dated
/// before the one kept last or after the voice's now, or a verdict this
/// long after the newest look was taken, is a break: what was kept is
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
    /// When the eyes looked, by their wall clock (`at_unix_ms`): only for
    /// the gaps and the span between looks.
    at_ms: u64,
    /// When the voice took it, by its own monotonic clock.
    taken_us: u64,
    /// When the eyes looked, on the voice's monotonic clock: `taken_us`
    /// less the look's age as it was taken, set by [`FacesHeld::look`].
    /// How fresh it is and when it expires count from here, so a look
    /// delivered late gets no time back, and a wall clock set back cannot
    /// stretch it (reviewer-two's and the reviewer-lead's HOLD on #3355).
    born_us: u64,
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
    cutoff_ms: Option<u64>,
}

impl FacesHeld {
    /// Keep a look the reader took, `epoch` read before it began to wait
    /// for it and `now_ms` the voice's wall clock as it took it.
    fn look(&mut self, mut look: FacesLook, epoch: u64, now_ms: u64) {
        if epoch != self.epoch || self.cutoff_ms.is_some_and(|cutoff| look.at_ms <= cutoff) {
            return;
        }
        let future = look.at_ms > now_ms;
        // Taken already stale or expired: no look.
        let age_ms = now_ms.saturating_sub(look.at_ms);
        if !future
            && (age_ms > LOOK_GAP_MS
                || look
                    .expires_ms
                    .is_some_and(|lasts| age_ms > lasts + lamp_bus::EXPIRY_GRACE_MS))
        {
            return;
        }
        look.born_us = look.taken_us.saturating_sub(age_ms * 1_000);
        if future
            || self.looks.back().is_some_and(|last| {
                look.at_ms < last.at_ms || look.at_ms - last.at_ms > LOOK_GAP_MS
            })
        {
            self.looks.clear();
        }
        if future {
            return;
        }
        if self.looks.len() == FACES_LOOKS_KEPT {
            self.looks.pop_front();
        }
        self.looks.push_back(look);
    }

    /// Privacy closed or reopened at `now_ms` by the wall clock: nothing
    /// before it counts. A wall clock set back after it refuses looks
    /// until it passes the cutoff again, so no room is made busy then.
    fn privacy_changed(&mut self, now_ms: u64) {
        self.looks.clear();
        self.epoch += 1;
        self.cutoff_ms = Some(self.cutoff_ms.map_or(now_ms, |cutoff| cutoff.max(now_ms)));
    }

    /// Whether two faces have stayed, at `now_us` by the voice's monotonic
    /// clock. A missing or expired newest look ends what was kept, as
    /// [`lamp_bus::Header::expired_at`] has it from when it was taken.
    fn two_stayed(&mut self, now_us: u64) -> bool {
        let age_ms = |look: &FacesLook| now_us.saturating_sub(look.born_us) / 1_000;
        while self
            .looks
            .front()
            .is_some_and(|look| age_ms(look) > FACES_KEPT_MS)
        {
            self.looks.pop_front();
        }
        let Some(newest) = self.looks.back().copied() else {
            return false;
        };
        if age_ms(&newest) > LOOK_GAP_MS
            || newest
                .expires_ms
                .is_some_and(|lasts| age_ms(&newest) > lasts + lamp_bus::EXPIRY_GRACE_MS)
        {
            self.looks.clear();
            return false;
        }
        if newest.faces < 2 {
            return false;
        }
        let mut two = self.looks.iter().filter(|look| look.faces >= 2);
        let count = two.clone().count();
        let first = two.next().map_or(0, |look| look.at_ms);
        let last = two.next_back().map_or(first, |look| look.at_ms);
        count >= TWO_FACES_LOOKS && last.saturating_sub(first) >= TWO_FACES_FOR_MS
    }
}


#[cfg(test)] mod controls {
 use super::*;
 fn look(held:&mut FacesHeld,at:u64,taken:u64,wall:u64) {
  let epoch=held.epoch;
  held.look(FacesLook{at_ms:at,taken_us:taken*1000,born_us:0,faces:2,expires_ms:Some(2000)},epoch,wall);
 }
 #[test] fn normal_persistence() {let mut h=FacesHeld::default();for at in [100000,100750,101500]{look(&mut h,at,at,at)}assert!(h.two_stayed(101600000));}
 #[test] fn ordinary_short_span_is_not_busy() {let mut h=FacesHeld::default();for at in [100000,100250,100500]{look(&mut h,at,at,at)}assert!(!h.two_stayed(100500000));}
 #[test] fn delayed_delivery_keeps_original_age() {let mut h=FacesHeld::default();for at in [100000,100750,101500]{look(&mut h,at,at+400,at+400)}assert!(h.two_stayed(102000000));assert!(!h.two_stayed(102600000));}
 #[test] fn queued_before_privacy_is_refused() {let mut h=FacesHeld::default();h.privacy_changed(101550);h.privacy_changed(101550);for at in [100000,100750,101500]{look(&mut h,at,101600,101600)}assert!(h.looks.is_empty());assert!(!h.two_stayed(101650000));}
 #[test] fn rollback_without_stream_expires() {let mut h=FacesHeld::default();for at in [100000,100750,101500]{look(&mut h,at,at,at)}assert!(!h.two_stayed(102501000));}
 #[test] fn forward_wall_step_does_not_invent_half_second_of_persistence() {
  let mut h=FacesHeld::default();
  // Five looks at the actual 250 ms cadence; wall clock steps forward
  // 500 ms between the first two. Every look is delivered immediately.
  for (wall,mono) in [(100000,100000),(100750,100250),(101000,100500),(101250,100750),(101500,101000)] {look(&mut h,wall,mono,wall)}
  let first=h.looks.front().unwrap().born_us;let last=h.looks.back().unwrap().born_us;
  assert_eq!(last-first,1000000,"only one physical second has passed");
  assert!(!h.two_stayed(101000000),"two faces have stayed 1.0 s, below the required 1.5 s; wall time must not manufacture persistence");
 }
}
