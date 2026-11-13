//! Where the frames go: activity's bursts ([`Activity`]) and posture's
//! samples ([`Pose`]), each taking a frame only when it wants one; with
//! neither, each copy is dropped (phase 1's [`Discard`]).
//!
//! [`Outlet`] sits inside the ring's mark ([`crate::mark::Marked`]), so one
//! mark covers one frame whoever takes it: the mark is asked once when
//! either wants a frame, and the frame passed is handed to each, which
//! takes it only if it is itself ready. Neither gets a frame twice.
use crate::activity::Activity;
use crate::frames::{Discard, Sink};
use crate::pose::Pose;
use lamp_camera::frames::FrameMeta;

/// Activity's bursts and posture's samples, either or both.
#[derive(Default)]
pub struct Outlet {
    pub activity: Option<Box<Activity>>,
    pub pose: Option<Box<Pose>>,
}

impl Sink for Outlet {
    fn ready(&mut self, now_us: u64) -> bool {
        if self.activity.is_none() && self.pose.is_none() {
            return Discard.ready(now_us);
        }
        let activity = self
            .activity
            .as_mut()
            .is_some_and(|activity| activity.ready(now_us));
        let pose = self.pose.as_mut().is_some_and(|pose| pose.ready(now_us));
        activity || pose
    }
    fn frame(&mut self, now_us: u64, meta: &FrameMeta, jpeg: &[u8]) {
        // Each checks for itself that it wants this frame.
        if let Some(activity) = self.activity.as_mut() {
            activity.frame(now_us, meta, jpeg);
        }
        if let Some(pose) = self.pose.as_mut() {
            pose.frame(now_us, meta, jpeg);
        }
    }
    fn stopped(&mut self) {
        if let Some(activity) = self.activity.as_mut() {
            activity.stopped();
        }
        if let Some(pose) = self.pose.as_mut() {
            pose.stopped();
        }
    }
}

#[cfg(test)]
mod tests;
