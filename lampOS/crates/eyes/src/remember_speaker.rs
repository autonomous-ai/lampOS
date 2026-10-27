//! Optional enrollment bound to the face seen speaking the requesting line.
//! Only already-computed recognition prints are held here, in memory, within
//! one camera permission hold. No print is written until the guarded capture
//! finishes. A passing tracking id alone is not identity: a replacement at
//! the same place can inherit it.
use crate::who::{PRINT, SAME_PERSON, TAKING_FOR_US, likeness};
use std::collections::VecDeque;

pub const ENVIRONMENT: &str = "LAMP_REMEMBER_SPEAKER";
pub const KEPT_US: u64 = 30_000_000;
pub const MOST_ANCHORS: usize = 16;
const FRESH_FRAME_US: u64 = 1_200_000;

pub fn from_environment() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// Internal Voice-to-Eyes proof; never model-selected arguments.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub name: String,
    pub face: String,
    pub began_us: u64,
    pub ended_us: u64,
    pub replace: bool,
    pub apart: bool,
}

fn face_id(face: &str) -> bool {
    face.len() <= 128
        && face.split_once('/').is_some_and(|(boot, id)| {
            !boot.is_empty()
                && boot
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
                && id
                    .strip_prefix('f')
                    .is_some_and(|id| id.parse::<u32>().is_ok())
        })
}

fn valid(print: &[f32]) -> bool {
    let norm = print.iter().map(|v| v * v).sum::<f32>();
    print.len() == PRINT && norm.is_finite() && (norm - 1.0).abs() <= 0.01
}

#[derive(Debug)]
struct Anchor {
    face: String,
    at_us: u64,
    print: Vec<f32>,
}

/// At most 16 existing prints, no older than 30 seconds, under one camera
/// permission. Refresh runs every loop, even when no recognition is due.
#[derive(Debug, Default)]
pub struct Anchors {
    hold: Option<u64>,
    kept: VecDeque<Anchor>,
    lost_through_us: Option<u64>,
    hold_observed_us: u64,
}

impl Anchors {
    pub fn clear(&mut self) {
        self.kept.clear();
        self.hold = None;
        self.lost_through_us = None;
    }

    pub fn refresh(&mut self, now_us: u64, hold: Option<u64>) {
        if hold != self.hold || hold.is_none() {
            self.clear();
            self.hold = hold;
            self.hold_observed_us = now_us;
        }
        self.kept
            .retain(|a| a.at_us <= now_us && now_us - a.at_us < KEPT_US);
    }

    /// The print must be the one near face, seen speaking in this very
    /// frame. Its timestamp is the driver's capture time, not processing
    /// time; the permission must still be the hold that read the frame.
    pub fn observe(
        &mut self,
        face: &str,
        print: &[f32],
        frame_us: u64,
        now_us: u64,
        frame_hold: Option<u64>,
        hold: Option<u64>,
    ) {
        self.refresh(now_us, hold);
        if hold.is_none()
            || frame_hold != hold
            || !face_id(face)
            || !valid(print)
            || frame_us <= self.hold_observed_us
            || frame_us == 0
            || frame_us > now_us
            || now_us - frame_us > FRESH_FRAME_US
            || self.kept.back().is_some_and(|a| frame_us <= a.at_us)
        {
            return;
        }
        self.kept.push_back(Anchor {
            face: face.into(),
            at_us: frame_us,
            print: print.to_vec(),
        });
        while self.kept.len() > MOST_ANCHORS {
            if let Some(lost) = self.kept.pop_front() {
                self.lost_through_us = Some(self.lost_through_us.unwrap_or(0).max(lost.at_us));
            }
        }
    }

    /// The same guarded collector constructed by the live socket dispatcher.
    /// Consent-capable operation admission happens before this call; all original
    /// face, interval, camera-hold and final-write fences remain in `Guard`.
    pub fn capture(
        &mut self,
        request: &Request,
        now_us: u64,
        hold: Option<u64>,
    ) -> Result<crate::who::Taking, &'static str> {
        let name = crate::who::plain_name(&request.name).ok_or("not_a_name")?;
        let guard = self.begin(request, now_us, hold)?;
        let mut taking = crate::who::Taking::new(name, now_us);
        taking.one_face = Some(request.replace);
        taking.apart = request.apart;
        taking.speaker = Some(guard);
        Ok(taking)
    }

    pub fn begin(
        &mut self,
        request: &Request,
        now_us: u64,
        hold: Option<u64>,
    ) -> Result<Guard, &'static str> {
        self.refresh(now_us, hold);
        let hold = hold.ok_or("eyes_shut")?;
        if !face_id(&request.face)
            || request.began_us == 0
            || request.began_us > request.ended_us
            || request.ended_us > now_us
            || now_us.saturating_sub(request.began_us) >= KEPT_US
        {
            return Err("speaker_expired");
        }
        if request.began_us < self.hold_observed_us
            || self
                .lost_through_us
                .is_some_and(|lost| request.began_us <= lost)
        {
            return Err("speaker_not_seen");
        }
        let mut matching = self.kept.iter().filter(|a| {
            a.face == request.face && a.at_us >= request.began_us && a.at_us <= request.ended_us
        });
        let first = matching.next().ok_or("speaker_not_seen")?;
        // A passing id occupied by two different prints during the line is
        // ambiguous. Never choose its last occupant as the requesting person.
        if matching.any(|a| likeness(&first.print, &a.print) < SAME_PERSON) {
            return Err("not_that_face");
        }
        Ok(Guard {
            face: request.face.clone(),
            print: first.print.clone(),
            hold,
            arrived_us: now_us,
            last_frame_us: None,
            failed: None,
        })
    }
}

/// A capture's immutable speaker anchor. Every accepted print matches it,
/// and caller/permission checks also run immediately before the store write.
#[derive(Debug)]
pub struct Guard {
    face: String,
    print: Vec<f32>,
    hold: u64,
    arrived_us: u64,
    last_frame_us: Option<u64>,
    failed: Option<&'static str>,
}

impl Guard {
    pub fn checked(
        &self,
        now_us: u64,
        hold: Option<u64>,
        caller_closed: bool,
    ) -> Result<(), &'static str> {
        if caller_closed {
            return Err("peer_closed");
        }
        if hold != Some(self.hold) {
            return Err("eyes_shut");
        }
        if let Some(error) = self.failed {
            return Err(error);
        }
        if now_us < self.arrived_us || now_us - self.arrived_us >= TAKING_FOR_US {
            return Err("speaker_expired");
        }
        Ok(())
    }

    /// A frame at or before arrival is buffered evidence, never a capture.
    /// A different/absent face or unprintable face terminates this capture;
    /// a later return cannot rescue it with samples of both people.
    pub fn looked(
        &mut self,
        face: Option<&str>,
        print: Option<&[f32]>,
        one_near: bool,
        frame_us: u64,
        now_us: u64,
        hold: Option<u64>,
    ) -> bool {
        if let Err(error) = self.checked(now_us, hold, false) {
            self.failed = Some(error);
            return false;
        }
        if frame_us <= self.arrived_us || self.last_frame_us.is_some_and(|last| frame_us <= last) {
            return false;
        }
        if frame_us > now_us
            || now_us - frame_us > FRESH_FRAME_US
            || !one_near
            || face != Some(self.face.as_str())
            || !print.is_some_and(|p| valid(p) && likeness(&self.print, p) >= SAME_PERSON)
        {
            self.failed = Some("not_that_face");
            return false;
        }
        self.last_frame_us = Some(frame_us);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn print(index: usize) -> Vec<f32> {
        let mut p = vec![0.0; PRINT];
        p[index] = 1.0;
        p
    }
    fn request() -> Request {
        Request {
            name: "Fixture Speaker".into(),
            face: "boot/f3".into(),
            began_us: 1_000_000,
            ended_us: 3_000_000,
            replace: false,
            apart: false,
        }
    }
    fn anchors() -> Anchors {
        let mut a = Anchors::default();
        a.refresh(1_000_000, Some(1));
        a.observe("boot/f3", &print(0), 2_000_000, 2_010_000, Some(1), Some(1));
        a
    }
    #[test]
    fn only_a_print_from_the_requesting_line_can_anchor_a_save() {
        let mut a = anchors();
        assert!(a.begin(&request(), 3_100_000, Some(1)).is_ok());
        for (begin, end) in [
            (2_100_000, 3_000_000),
            (1, 1_999_999),
            (0, 3_000_000),
            (4_000_000, 3_000_000),
            (1_000_000, 4_000_000),
        ] {
            let mut r = request();
            r.began_us = begin;
            r.ended_us = end;
            assert!(a.begin(&r, 3_100_000, Some(1)).is_err());
        }
        let mut r = request();
        r.face = "another-boot/f3".into();
        assert!(a.begin(&r, 3_100_000, Some(1)).is_err());
    }
    #[test]
    fn replacement_under_the_same_passing_id_is_not_the_speaker() {
        let mut a = anchors();
        let mut g = a.begin(&request(), 3_100_000, Some(1)).unwrap();
        assert!(g.looked(
            Some("boot/f3"),
            Some(&print(0)),
            true,
            3_200_000,
            3_210_000,
            Some(1)
        ));
        assert!(!g.looked(
            Some("boot/f3"),
            Some(&print(1)),
            true,
            3_300_000,
            3_310_000,
            Some(1)
        ));
        assert!(!g.looked(
            Some("boot/f3"),
            Some(&print(0)),
            true,
            3_400_000,
            3_410_000,
            Some(1)
        ));
        assert_eq!(g.checked(3_410_000, Some(1), false), Err("not_that_face"));
    }
    #[test]
    fn changing_occupant_during_the_line_is_ambiguous() {
        let mut a = anchors();
        a.observe("boot/f3", &print(1), 2_500_000, 2_510_000, Some(1), Some(1));
        assert_eq!(
            a.begin(&request(), 3_100_000, Some(1)).unwrap_err(),
            "not_that_face"
        );
    }
    #[test]
    fn fresh_capture_and_final_write_keep_the_same_permission_and_caller() {
        let mut a = anchors();
        let mut g = a.begin(&request(), 3_100_000, Some(1)).unwrap();
        for at in [0, 3_000_000, 3_100_000] {
            assert!(!g.looked(
                Some("boot/f3"),
                Some(&print(0)),
                true,
                at,
                3_110_000,
                Some(1)
            ));
        }
        assert!(g.looked(
            Some("boot/f3"),
            Some(&print(0)),
            true,
            3_200_000,
            3_210_000,
            Some(1)
        ));
        assert!(!g.looked(
            Some("boot/f3"),
            Some(&print(0)),
            true,
            3_200_000,
            3_220_000,
            Some(1)
        ));
        assert_eq!(g.checked(3_220_000, Some(1), true), Err("peer_closed"));
        assert_eq!(g.checked(3_220_000, Some(2), false), Err("eyes_shut"));
        assert_eq!(g.checked(3_220_000, None, false), Err("eyes_shut"));
        assert_eq!(g.checked(7_100_000, Some(1), false), Err("speaker_expired"));
    }
    #[test]
    fn absent_ambiguous_unprinted_and_stale_frames_fail_closed() {
        for (face, p, one, frame, now) in [
            (None, Some(print(0)), true, 3_200_000, 3_210_000),
            (Some("boot/f4"), Some(print(0)), true, 3_200_000, 3_210_000),
            (Some("boot/f3"), None, true, 3_200_000, 3_210_000),
            (Some("boot/f3"), Some(print(0)), false, 3_200_000, 3_210_000),
            (Some("boot/f3"), Some(print(0)), true, 3_200_000, 5_000_000),
            (Some("boot/f3"), Some(print(0)), true, 4_000_000, 3_210_000),
        ] {
            let mut g = anchors().begin(&request(), 3_100_000, Some(1)).unwrap();
            assert!(!g.looked(face, p.as_deref(), one, frame, now, Some(1)));
            assert_eq!(g.checked(now, Some(1), false), Err("not_that_face"));
        }
    }
    #[test]
    fn cache_is_bounded_and_forget_close_and_age_discard_it() {
        let mut a = anchors();
        a.refresh(3_000_000, None);
        a.refresh(3_010_000, Some(2));
        assert!(a.begin(&request(), 3_100_000, Some(2)).is_err());
        let mut a = anchors();
        a.clear();
        assert!(a.begin(&request(), 3_100_000, Some(1)).is_err());
        let mut a = anchors();
        a.refresh(32_000_000, Some(1));
        assert!(a.kept.is_empty());
        let mut a = anchors();
        for n in 1..100 {
            a.observe(
                "boot/f3",
                &print(0),
                n * 100_000,
                n * 100_000,
                Some(1),
                Some(1),
            );
        }
        assert_eq!(a.kept.len(), MOST_ANCHORS);
        a.observe(
            "boot/f3",
            &print(1),
            10_000_000,
            10_000_000,
            Some(1),
            Some(2),
        );
        assert!(a.kept.is_empty());
    }
    #[test]
    fn a_line_crossing_a_hold_reset_or_forget_cannot_choose_its_later_occupant() {
        for forget in [false, true] {
            let mut a = anchors();
            if forget {
                a.clear();
            }
            let hold = if forget { 1 } else { 2 };
            a.refresh(2_400_000, Some(hold));
            a.observe(
                "boot/f3",
                &print(1),
                2_500_000,
                2_510_000,
                Some(hold),
                Some(hold),
            );
            assert_eq!(
                a.begin(&request(), 3_100_000, Some(hold)).unwrap_err(),
                "speaker_not_seen"
            );
            let mut later = request();
            later.began_us = 2_450_000;
            assert!(a.begin(&later, 3_100_000, Some(hold)).is_ok());
        }
    }

    #[test]
    fn overflow_cannot_erase_an_earlier_occupant_of_the_same_id() {
        let mut a = Anchors::default();
        a.refresh(0, Some(1));
        a.observe("boot/f3", &print(0), 1_000_000, 1_000_000, Some(1), Some(1));
        for n in 2..=17 {
            a.observe(
                "boot/f3",
                &print(1),
                n * 1_000_000,
                n * 1_000_000,
                Some(1),
                Some(1),
            );
        }
        let mut r = request();
        r.began_us = 500_000;
        r.ended_us = 18_000_000;
        assert_eq!(
            a.begin(&r, 18_100_000, Some(1)).unwrap_err(),
            "speaker_not_seen"
        );
        // A genuinely later request is not made ambiguous by an older one.
        r.began_us = 17_000_000;
        assert!(a.begin(&r, 18_100_000, Some(1)).is_ok());
    }

    #[test]
    fn a_buffered_frame_from_before_a_close_cannot_refill_the_new_hold() {
        let mut a = anchors();
        a.refresh(3_100_000, Some(2));
        a.observe("boot/f3", &print(0), 3_000_000, 3_110_000, Some(2), Some(2));
        assert!(a.kept.is_empty());
        let mut r = request();
        r.ended_us = 3_120_000;
        assert!(a.begin(&r, 3_130_000, Some(2)).is_err());
        a.observe("boot/f3", &print(0), 3_200_000, 3_210_000, Some(2), Some(2));
        r.began_us = 3_150_000;
        r.ended_us = 3_220_000;
        assert!(a.begin(&r, 3_230_000, Some(2)).is_ok());
    }
}
