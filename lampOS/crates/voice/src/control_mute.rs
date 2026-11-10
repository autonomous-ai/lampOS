//! A paired computer's mute (`docs/control.md`, "Turns and listening"):
//! lamp-control's `listen.set false`, asked over the say socket as
//! `{"skill":"control","listen":false}` and held on that connection
//! ([`crate::say`]).
//!
//! - **Held by its connection.** Each mute is one connection's, kept open
//!   while it holds. It ends when `{"listen":true}` comes on that same
//!   connection, when the connection closes (the door closes it when the
//!   computer disconnects or is revoked), or when the person's hand on the
//!   head lifts it, as it lifts the restart guard's mute. So a computer gone
//!   never leaves the lamp deaf.
//! - **Its own.** A `listen: true` lifts only its own connection's mute:
//!   never another computer's, nor the person's switch, the button's mute,
//!   sleep or the restart guard's. While several hold one, the lamp stays
//!   muted until every one is lifted.
//! - **The voice's software mute.** The hand controls take it at each poll
//!   as they take the button's ([`crate::hand_controls`]): the microphones
//!   close as for any software mute, a conversation going on ends, and
//!   `voice/mute` says it (`control`).
//! - **Bounded.** At most [`MOST_HELD`] at once; past that, refused. With no
//!   software mute on this lamp (no mute record), refused at once.
//!
//! The journal says when each starts and when it lifts, and how.
use std::{
    sync::{
        Arc, Mutex, MutexGuard, OnceLock, PoisonError,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

/// The most computers' mutes held at once.
pub const MOST_HELD: usize = 8;
/// How long a held mute waits for the voice to take it before it is given
/// up and refused: the voice's loop takes it at its next poll, in
/// milliseconds, but a cycle's start may hold that poll for a while.
pub const TAKE_WITHIN: Duration = Duration::from_secs(5);

/// How a computer's mute ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lift {
    /// `{"listen":true}` on its own connection.
    Asked,
    /// Its connection closed, or failed.
    Closed,
    /// The person's hand on the head.
    Hand,
}

impl Lift {
    /// The words the answer and the journal give for it.
    pub fn why(self) -> &'static str {
        match self {
            Self::Asked => "asked",
            Self::Closed => "its connection closed",
            Self::Hand => "a hand on the head",
        }
    }
}

/// What the voice tells a held mute's connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Told {
    /// The voice took it: the microphones are closed for it.
    Taken,
    /// It was lifted by the voice (a hand on the head), not by its
    /// connection.
    Lifted(Lift),
}

struct Holding {
    id: u64,
    taken: bool,
    told: mpsc::Sender<Told>,
}

#[derive(Default)]
struct Held {
    holding: Vec<Holding>,
    next: u64,
}

#[derive(Default)]
struct Shared {
    /// This lamp has a software mute to hold: the hand controls run with a
    /// mute record.
    served: AtomicBool,
    held: Mutex<Held>,
}

/// The computers' mutes held now, shared by the say socket's connections
/// and the hand controls.
#[derive(Clone, Default)]
pub struct ControlMutes(Arc<Shared>);

// Process ownership: written and read in serve/coordinator and its say socket threads; this
// static is not IPC.
static PROCESS: OnceLock<ControlMutes> = OnceLock::new();

impl ControlMutes {
    /// The voice's own: the say socket's and the hand controls'.
    pub fn process() -> Self {
        PROCESS.get_or_init(Self::default).clone()
    }

    fn held(&self) -> MutexGuard<'_, Held> {
        self.0.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether this lamp has a software mute a computer may hold: said by
    /// the hand controls when they start.
    pub fn serve(&self, served: bool) {
        self.0.served.store(served, Ordering::Release);
    }

    /// A new mute for one connection, or why not.
    pub fn hold(&self) -> Result<Hold, String> {
        if !self.0.served.load(Ordering::Acquire) {
            return Err("the lamp has no software mute to hold".into());
        }
        let mut held = self.held();
        if held.holding.len() >= MOST_HELD {
            return Err(format!("{MOST_HELD} computers' mutes are held already"));
        }
        held.next += 1;
        let id = held.next;
        let (told, hears) = mpsc::channel();
        held.holding.push(Holding {
            id,
            taken: false,
            told,
        });
        Ok(Hold {
            mutes: self.clone(),
            id,
            told: hears,
            ended: false,
        })
    }

    /// Whether any computer's mute holds, for the hand controls' poll: each
    /// not yet taken is taken now, told so, and journaled as started.
    pub fn take(&self) -> bool {
        let mut held = self.held();
        let count = held.holding.len();
        for holding in held.holding.iter_mut().filter(|holding| !holding.taken) {
            holding.taken = true;
            let _ = holding.told.send(Told::Taken);
            eprintln!(
                "lamp-voice: a computer's mute starts (lamp-control's listen.set false); {count} held"
            );
        }
        count > 0
    }

    /// The person's hand on the head lifts every computer's mute, as it
    /// lifts the restart guard's: each connection is told. How many it
    /// lifted.
    pub fn lift_by_hand(&self) -> usize {
        let lifted = std::mem::take(&mut self.held().holding);
        for holding in &lifted {
            let _ = holding.told.send(Told::Lifted(Lift::Hand));
        }
        if !lifted.is_empty() {
            eprintln!(
                "lamp-voice: {} computer's mute(s) lift: {}; 0 held",
                lifted.len(),
                Lift::Hand.why()
            );
        }
        lifted.len()
    }

    /// How many are held now.
    #[cfg(test)]
    pub(crate) fn count(&self) -> usize {
        self.held().holding.len()
    }

    /// Lifts the mute `id`, if it still holds, journaled with `how`.
    fn lift(&self, id: u64, how: Lift) -> bool {
        let mut held = self.held();
        let Some(index) = held.holding.iter().position(|holding| holding.id == id) else {
            return false;
        };
        let holding = held.holding.remove(index);
        let left = held.holding.len();
        drop(held);
        if holding.taken {
            eprintln!(
                "lamp-voice: a computer's mute lifts: {}; {left} held",
                how.why()
            );
        }
        true
    }

    /// Gives up the mute `id` if the voice has not taken it yet.
    fn give_up(&self, id: u64) -> bool {
        let mut held = self.held();
        match held.holding.iter().position(|holding| holding.id == id) {
            Some(index) if !held.holding[index].taken => {
                held.holding.remove(index);
                true
            }
            _ => false,
        }
    }
}

/// One connection's mute: lifted when dropped, as its connection closed.
pub struct Hold {
    mutes: ControlMutes,
    id: u64,
    told: mpsc::Receiver<Told>,
    /// Lifted by its connection's ask: nothing for the drop to lift.
    ended: bool,
}

impl Hold {
    /// What the voice has told this mute since last asked, if anything.
    pub fn told(&self) -> Option<Told> {
        self.told.try_recv().ok()
    }

    /// Lifted as its connection asked.
    pub fn lift(mut self, how: Lift) {
        self.mutes.lift(self.id, how);
        self.ended = true;
    }

    /// Given up, if the voice has not taken it yet: whether it was.
    pub fn give_up(&self) -> bool {
        self.mutes.give_up(self.id)
    }
}

impl Drop for Hold {
    fn drop(&mut self) {
        if !self.ended {
            self.mutes.lift(self.id, Lift::Closed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn served() -> ControlMutes {
        let mutes = ControlMutes::default();
        mutes.serve(true);
        mutes
    }

    #[test]
    fn a_lamp_with_no_software_mute_refuses_one() {
        let mutes = ControlMutes::default();
        assert_eq!(
            mutes.hold().err(),
            Some("the lamp has no software mute to hold".into())
        );
        assert!(!mutes.take());
    }

    #[test]
    fn each_mute_is_taken_once_and_lifted_only_by_its_own_hold() {
        let mutes = served();
        let first = mutes.hold().unwrap();
        assert_eq!(first.told(), None, "not taken before the voice's poll");
        assert!(mutes.take());
        assert_eq!(first.told(), Some(Told::Taken));
        let second = mutes.hold().unwrap();
        assert!(mutes.take());
        assert_eq!((first.told(), second.told()), (None, Some(Told::Taken)));
        // The second's lift leaves the first's: still muted.
        second.lift(Lift::Asked);
        assert!(mutes.take() && mutes.count() == 1);
        // Its connection closing lifts the first.
        drop(first);
        assert!(!mutes.take());
    }

    #[test]
    fn a_hand_lifts_every_one_and_tells_each() {
        let mutes = served();
        let holds: Vec<_> = (0..3).map(|_| mutes.hold().unwrap()).collect();
        assert!(mutes.take());
        assert_eq!(mutes.lift_by_hand(), 3);
        assert!(!mutes.take());
        for hold in &holds {
            assert_eq!(hold.told(), Some(Told::Taken));
            assert_eq!(hold.told(), Some(Told::Lifted(Lift::Hand)));
        }
        // Their connections then close: nothing more to lift.
        drop(holds);
        assert_eq!(mutes.count(), 0);
    }

    #[test]
    fn at_most_eight_are_held_and_one_not_taken_can_be_given_up() {
        let mutes = served();
        let holds: Vec<_> = (0..MOST_HELD).map(|_| mutes.hold().unwrap()).collect();
        assert_eq!(
            mutes.hold().err(),
            Some(format!("{MOST_HELD} computers' mutes are held already"))
        );
        assert!(holds[0].give_up(), "not taken yet");
        assert!(mutes.hold().is_ok_and(|late| {
            mutes.take();
            !late.give_up()
        }));
    }
}
