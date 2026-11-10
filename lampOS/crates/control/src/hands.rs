//! Whether a hand is wanted (`humans/hands/wanted`, `lamp_msgs::HandsWanted`):
//! true while a paired computer's `look` at `hand` stands (the
//! [`Follower`](crate::body_lease::Follower) holds its client), false when it
//! ends. The eyes look for a hand only while it is true, so with nobody
//! wanting one nothing runs and nothing is published; the door never sees a
//! picture, only this one word going out.
//!
//! Told at start (false, so a latecomer reads the truth), at every change,
//! and, while true, again every [`REPEAT`], so a restarted eyes that missed
//! the change still learns it. Looked at every [`LOOK_EVERY`]: a hand wanted
//! starts the eyes' search within that, never on a clock of the eyes' own.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::body_lease::Follower;

/// How often the follower is looked at.
pub const LOOK_EVERY: Duration = Duration::from_millis(100);
/// How often a standing "wanted" is told again.
pub const REPEAT: Duration = Duration::from_secs(10);

/// What was told last, and when.
#[derive(Debug, Default)]
pub struct Wanted {
    told: Option<(bool, Instant)>,
}

impl Wanted {
    /// What to tell now, if anything: the first look, a change, or a
    /// standing "wanted" past [`REPEAT`].
    pub fn step(&mut self, wanted: bool, now: Instant) -> Option<bool> {
        let due = match self.told {
            None => true,
            Some((before, at)) => {
                before != wanted || (wanted && now.saturating_duration_since(at) >= REPEAT)
            }
        };
        due.then(|| {
            self.told = Some((wanted, now));
            wanted
        })
    }
}

/// Looks at `follower` until `stop`, telling each word `tell` is due.
pub fn run(follower: Follower, mut tell: impl FnMut(bool), stop: Arc<AtomicBool>, every: Duration) {
    let mut wanted = Wanted::default();
    while !stop.load(Ordering::SeqCst) {
        let now = follower
            .lock()
            .map(|client| client.is_some())
            .unwrap_or(false);
        if let Some(word) = wanted.step(now, Instant::now()) {
            tell(word);
        }
        std::thread::sleep(every);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Told at the first look, at each change, and again only while wanted
    /// and past `REPEAT`.
    #[test]
    fn a_hand_wanted_is_told_at_start_at_each_change_and_again_while_it_stands() {
        let start = Instant::now();
        let at = |seconds: u64| start + Duration::from_secs(seconds);
        let mut wanted = Wanted::default();
        assert_eq!(wanted.step(false, at(0)), Some(false), "the first word");
        assert_eq!(wanted.step(false, at(1)), None);
        assert_eq!(wanted.step(false, at(100)), None, "not wanted: told once");
        assert_eq!(wanted.step(true, at(101)), Some(true));
        assert_eq!(wanted.step(true, at(105)), None);
        assert_eq!(wanted.step(true, at(111)), Some(true), "again after REPEAT");
        assert_eq!(wanted.step(true, at(115)), None);
        assert_eq!(wanted.step(false, at(116)), Some(false), "ended at once");
        assert_eq!(wanted.step(false, at(300)), None);
    }

    /// The loop follows the follower: false, then true when a client's look
    /// at a hand stands, then false when it ends; and it stops when told.
    #[test]
    fn the_loop_tells_what_the_follower_holds_and_stops() {
        let follower = Follower::default();
        let told: Arc<Mutex<Vec<bool>>> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let (sink, halt, held) = (told.clone(), stop.clone(), follower.clone());
        let worker = std::thread::spawn(move || {
            run(
                held,
                move |word| sink.lock().unwrap().push(word),
                halt,
                Duration::from_millis(5),
            )
        });
        let wait = |want: Vec<bool>| {
            for _ in 0..200 {
                if *told.lock().unwrap() == want {
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            panic!("told {:?}, wanted {want:?}", told.lock().unwrap());
        };
        wait(vec![false]);
        *follower.lock().unwrap() = Some("c_1".to_owned());
        wait(vec![false, true]);
        *follower.lock().unwrap() = None;
        wait(vec![false, true, false]);
        stop.store(true, Ordering::SeqCst);
        worker.join().unwrap();
    }
}
