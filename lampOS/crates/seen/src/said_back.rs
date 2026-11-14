//! The opt-in said back, once each way (phase 0's seventh line, #6543, with
//! the lead's addition on it): [`ON_LINE`] when the opt-in turns on, and
//! [`OFF_LINE`] when it turns off, through the voice's say service
//! (docs/speaking-first.md, "the say service"), the path other parts use to
//! have the lamp say something.
//!
//! - The on line is said when the opt-in goes from off to on: a kept yes
//!   this part takes ([`crate::opt_in::OptIn::on`]) that was not said back.
//! - The off line is said when it goes from on to off: the last thing said
//!   back was the on line, and the opt-in is now off. That includes a
//!   forget (`memory/forgot` turns the opt-in off), so the person hears the
//!   lamp stopped looking. A lamp whose on line was never said says no off
//!   line.
//! - Once the voice has begun a line (`{"event":"begun"}`) it counts as
//!   said, and what it was (on, for which yes, or off) is kept in
//!   `lamp-seen`'s own state folder ([`Store::set_said`]), never beside the
//!   yes in the consent folder, so a restart says neither again. A person
//!   talking over it doesn't bring it back. A record left over after the yes
//!   was removed means nothing: the gate reads the yes.
//! - Not begun (no say service, refused, expired, dropped before it
//!   began): asked again after [`RETRY_US`], while it is still the line
//!   owed.
//! - An ask still out when the line owed changes (a forget before the on
//!   line began, or another yes) is withdrawn: the say service's cancel
//!   ([`CANCEL_LINE`], the voice's `say::CANCEL_LINE`) on that ask's own
//!   connection, since a connection closed cancels nothing. Once its answer
//!   comes the line now owed is asked for. Withdrawn before it began, the on
//!   line was never said, so no off line is owed after it.
//! - The gate opens no earlier than the on line for the kept yes has begun
//!   ([`SaidBack::said_on`]), so the ring's camera mark never comes before
//!   the lamp said it would look. With no say service it never opens.
//! - **Posture's line.** With `LAMP_SEEN_POSTURE` on
//!   ([`SaidBack::with_posture`]), the on line owed is [`POSTURE_ON_LINE`],
//!   which names posture too, and the record keeps that it did. A yes whose
//!   plain [`ON_LINE`] was said before posture was switched on is owed the
//!   posture line once. Posture sends no frame until it has begun for the
//!   kept yes ([`SaidBack::said_posture`]); activity needs either line. A
//!   forget then a new yes owes it again, as any new yes does.
use crate::opt_in::{Said, Store};
use lamp_msgs::consent::Kept;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Said word for word when the opt-in turns on (the first builder's
/// wording on #6543).
pub const ON_LINE: &str = "I'll look now and then to see if you're eating, drinking or slouching. I keep none of the pictures.";
/// Said instead of [`ON_LINE`] when posture is switched on
/// (`LAMP_SEEN_POSTURE`): the same line with posture named. Awaiting the
/// PM's wording (docs/seen.md); this constant is the one place to change it.
pub const POSTURE_ON_LINE: &str = "I'll look now and then to see if you're eating, drinking or slouching, and now and then how you're sitting, so I can remind you to sit back. I keep none of the pictures.";
/// Said word for word when the opt-in turns off, a forget's included.
pub const OFF_LINE: &str = "Okay, I've stopped looking at the desk.";
/// The say service's `skill`: logged and traced only.
pub const SKILL: &str = "seen";
/// Above a skill's 5: an answer to the person's own setting.
pub const PRIORITY: u8 = 6;
/// Not begun within this, it is `expired`.
pub const EXPIRES_IN_MS: u64 = 120_000;
/// A line not begun is asked again this long after.
pub const RETRY_US: u64 = 120_000_000;
/// The say service's cancel, written on the ask's own connection: the
/// voice's `say::CANCEL_LINE` (docs/speaking-first.md, "the asker's
/// cancel"). Waiting, the ask leaves the queue; begun, the turn is cut.
pub const CANCEL_LINE: &str = r#"{"cancel":true}"#;

/// The line that says `said`.
pub fn line(said: Said) -> &'static str {
    match said {
        Said::On { posture: false, .. } => ON_LINE,
        Said::On { posture: true, .. } => POSTURE_ON_LINE,
        Said::Off => OFF_LINE,
    }
}

/// What became of an ask.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Heard {
    /// The voice began the line.
    Begun,
    /// It never began: why.
    NotBegun(String),
}

/// The voice, as this part asks it to say a line.
pub trait Mouth {
    /// Ask for `line`, not waiting for it.
    fn ask(&mut self, line: &str);
    /// What became of the last ask, once known.
    fn heard(&mut self) -> Option<Heard>;
    /// Withdraw the last ask, not waiting: its answer still comes through
    /// [`Mouth::heard`].
    fn cancel(&mut self);
}

/// What one step did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Idle,
    /// A line was asked for: on or off.
    Asked(Said),
    /// An ask is out.
    Waiting,
    /// An ask out is no longer the line owed: its cancel was sent, and its
    /// answer is awaited.
    Withdrawn(Said),
    /// The voice began it; `Err` when keeping that failed (it is still not
    /// said again in this run).
    Said(Said, Result<(), String>),
    /// It never began: asked again after [`RETRY_US`].
    NotSaid(Said, String),
}

/// The said-back's state across steps.
#[derive(Clone, Debug, Default)]
pub struct SaidBack {
    /// What an ask out is saying.
    asking: Option<Said>,
    /// What was said back in this run, kept or not.
    said_here: Option<Said>,
    /// A line not begun, and when it may be asked again.
    retry: Option<(Said, u64)>,
    /// The ask out was withdrawn.
    withdrawn: bool,
    /// Posture is switched on: the on line owed names it.
    posture: bool,
}

impl SaidBack {
    /// With posture switched on or not: on, the on line owed is
    /// [`POSTURE_ON_LINE`].
    pub fn with_posture(posture: bool) -> Self {
        Self {
            posture,
            ..Self::default()
        }
    }

    /// One step at `now_us`: `opted_in` as the gate takes it, `kept` as the
    /// store read it.
    pub fn step(
        &mut self,
        opted_in: bool,
        kept: &Kept,
        store: &impl Store,
        mouth: &mut impl Mouth,
        now_us: u64,
    ) -> Step {
        let owed = self.owed(opted_in, kept, store);
        if let Some(asked) = self.asking {
            return match mouth.heard() {
                None if !self.withdrawn && owed != Some(asked) => {
                    mouth.cancel();
                    self.withdrawn = true;
                    Step::Withdrawn(asked)
                }
                None => Step::Waiting,
                // Begun before the cancel reached it: it was said, if cut.
                Some(Heard::Begun) => {
                    self.asking = None;
                    self.withdrawn = false;
                    self.retry = None;
                    self.said_here = Some(asked);
                    Step::Said(
                        asked,
                        store.set_said(asked).map_err(|error| error.to_string()),
                    )
                }
                Some(Heard::NotBegun(why)) => {
                    self.asking = None;
                    // Withdrawn, the line now owed is asked at once.
                    self.retry = (!std::mem::take(&mut self.withdrawn))
                        .then(|| (asked, now_us.saturating_add(RETRY_US)));
                    Step::NotSaid(asked, why)
                }
            };
        }
        let Some(owed) = owed else {
            return Step::Idle;
        };
        if self
            .retry
            .is_some_and(|(line, at)| line == owed && now_us < at)
        {
            return Step::Idle;
        }
        mouth.ask(line(owed));
        self.asking = Some(owed);
        Step::Asked(owed)
    }

    /// The last line said back: in this run, else as kept.
    fn last(&self, store: &impl Store) -> Option<Said> {
        self.said_here.or_else(|| store.said())
    }

    /// The line owed now, if any: on for a kept yes not said back (with
    /// posture on, not said back naming posture), off only after an on
    /// line was said.
    fn owed(&self, opted_in: bool, kept: &Kept, store: &impl Store) -> Option<Said> {
        let last = self.last(store);
        if opted_in {
            let said = last.is_some_and(|last| {
                if self.posture {
                    last.names_posture_for(kept)
                } else {
                    last.is_on_for(kept)
                }
            });
            (!said).then(|| Said::on_for(kept, self.posture))
        } else {
            matches!(last, Some(Said::On { .. })).then_some(Said::Off)
        }
    }

    /// Whether the on line for `kept` has begun (in this run, or kept from
    /// an earlier one): the gate asks for no frame before it.
    pub fn said_on(&self, kept: &Kept, store: &impl Store) -> bool {
        self.last(store).is_some_and(|last| last.is_on_for(kept))
    }

    /// Whether an on line naming posture has begun for `kept`: posture
    /// sends no frame before it.
    pub fn said_posture(&self, kept: &Kept, store: &impl Store) -> bool {
        self.last(store)
            .is_some_and(|last| last.names_posture_for(kept))
    }
}

/// The voice's say service on its socket (`LAMP_SAY_SOCKET`,
/// `/run/lampos-say/say.sock` if unset). Each ask runs on a thread of its
/// own, so the gate and the frames never wait for it.
pub struct SayService {
    pub socket: PathBuf,
    answer: Option<Receiver<Heard>>,
    /// The last ask's connection, to write its cancel on.
    asking: Option<Arc<Mutex<Asking>>>,
}

/// One ask's connection as its cancel sees it.
#[derive(Default)]
struct Asking {
    /// Withdrawn before it connected: it is not asked at all.
    cancelled: bool,
    /// Once asked, a handle on its connection.
    stream: Option<UnixStream>,
}

impl SayService {
    pub const DEFAULT_SOCKET: &str = "/run/lampos-say/say.sock";

    pub fn new(socket: PathBuf) -> Self {
        Self {
            socket,
            answer: None,
            asking: None,
        }
    }
}

/// The ask's one JSON line.
pub fn ask_line(line: &str) -> String {
    let ask = serde_json::json!({
        "skill": SKILL,
        "text": line,
        "priority": PRIORITY,
        "expires_in_ms": EXPIRES_IN_MS,
    });
    format!("{ask}\n")
}

/// What one answer line says: `Some` once it is known.
pub fn heard_from(answer: &str) -> Option<Heard> {
    let value: serde_json::Value = serde_json::from_str(answer).ok()?;
    if value.get("event").and_then(serde_json::Value::as_str) == Some("begun") {
        return Some(Heard::Begun);
    }
    let outcome = value.get("outcome").and_then(serde_json::Value::as_str)?;
    let why = value.get("why").and_then(serde_json::Value::as_str);
    Some(Heard::NotBegun(match why {
        Some(why) => format!("{outcome}: {why}"),
        None => outcome.to_owned(),
    }))
}

fn ask_through(socket: &std::path::Path, line: &str, asking: &Mutex<Asking>) -> Heard {
    let mut stream = match UnixStream::connect(socket) {
        Ok(stream) => stream,
        Err(error) => return Heard::NotBegun(format!("no say service: {error}")),
    };
    let wait = Duration::from_millis(EXPIRES_IN_MS) + Duration::from_secs(10);
    {
        // The ask is written and its handle left under one lock, so a
        // cancel never comes before the ask on the connection.
        let mut asking = asking
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if asking.cancelled {
            return Heard::NotBegun("withdrawn before it was asked".into());
        }
        if let Err(error) = stream
            .set_read_timeout(Some(wait))
            .and_then(|()| stream.write_all(ask_line(line).as_bytes()))
        {
            return Heard::NotBegun(format!("the say service: {error}"));
        }
        asking.stream = stream.try_clone().ok();
    }
    let mut reader = BufReader::new(stream);
    let mut answer = String::new();
    loop {
        answer.clear();
        match reader.read_line(&mut answer) {
            Ok(0) => return Heard::NotBegun("the say service closed".into()),
            // Begun is enough: the connection closed after it cancels
            // nothing (docs/speaking-first.md).
            Ok(_) => {
                if let Some(heard) = heard_from(answer.trim()) {
                    return heard;
                }
            }
            Err(error) => return Heard::NotBegun(format!("the say service: {error}")),
        }
    }
}

impl Mouth for SayService {
    fn ask(&mut self, line: &str) {
        let (tell, answer) = mpsc::channel();
        let socket = self.socket.clone();
        let line = line.to_owned();
        let asking = Arc::new(Mutex::new(Asking::default()));
        let its = Arc::clone(&asking);
        std::thread::spawn(move || {
            let _ = tell.send(ask_through(&socket, &line, &its));
        });
        self.answer = Some(answer);
        self.asking = Some(asking);
    }

    fn cancel(&mut self) {
        let Some(asking) = self.asking.as_ref() else {
            return;
        };
        let mut asking = asking
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        asking.cancelled = true;
        if let Some(stream) = asking.stream.as_mut() {
            // A failed write leaves the ask to its own answer or expiry.
            let _ = stream.write_all(format!("{CANCEL_LINE}\n").as_bytes());
        }
    }

    fn heard(&mut self) -> Option<Heard> {
        let answer = self.answer.as_ref()?;
        let heard = match answer.try_recv() {
            Ok(heard) => heard,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Heard::NotBegun("the ask was lost".into()),
        };
        self.answer = None;
        self.asking = None;
        Some(heard)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::io;

    /// The said-back record, in memory.
    #[derive(Default)]
    struct Memory {
        said: RefCell<Option<Said>>,
        keeps_nothing: bool,
    }

    impl Store for Memory {
        fn said(&self) -> Option<Said> {
            *self.said.borrow()
        }
        fn set_said(&self, said: Said) -> io::Result<()> {
            if self.keeps_nothing {
                return Err(io::Error::other("read-only"));
            }
            *self.said.borrow_mut() = Some(said);
            Ok(())
        }
    }

    /// The voice, as told.
    #[derive(Default)]
    struct Voice {
        asked: Vec<String>,
        next: Option<Heard>,
        cancels: usize,
    }

    impl Mouth for Voice {
        fn ask(&mut self, line: &str) {
            self.asked.push(line.to_owned());
        }
        fn heard(&mut self) -> Option<Heard> {
            self.next.take()
        }
        fn cancel(&mut self) {
            self.cancels += 1;
        }
    }

    fn yes(at: u64) -> Kept {
        Kept {
            yes: true,
            by: Some("app".into()),
            at_unix_s: Some(at),
            rev: None,
        }
    }

    fn on(at: u64) -> Said {
        Said::On {
            for_at_unix_s: Some(at),
            posture: false,
        }
    }

    fn on_posture(at: u64) -> Said {
        Said::On {
            for_at_unix_s: Some(at),
            posture: true,
        }
    }

    #[test]
    fn posture_on_with_only_the_plain_line_said_owes_the_posture_line_once() {
        let store = Memory::default();
        let mut voice = Voice::default();
        // The plain line said while posture was off.
        say(
            &mut SaidBack::default(),
            true,
            &yes(7),
            &store,
            &mut voice,
            0,
            on(7),
        );
        assert_eq!(voice.asked, [ON_LINE]);
        // Posture switched on (a restart with LAMP_SEEN_POSTURE=on).
        let mut back = SaidBack::with_posture(true);
        assert!(back.said_on(&yes(7), &store), "activity's gate stays open");
        assert!(!back.said_posture(&yes(7), &store), "no posture frame yet");
        say(
            &mut back,
            true,
            &yes(7),
            &store,
            &mut voice,
            10,
            on_posture(7),
        );
        assert_eq!(voice.asked, [ON_LINE, POSTURE_ON_LINE]);
        assert!(back.said_posture(&yes(7), &store));
        assert_eq!(*store.said.borrow(), Some(on_posture(7)), "kept");
        // Said once: not again, here or after a restart.
        assert_eq!(back.step(true, &yes(7), &store, &mut voice, 20), Step::Idle);
        assert_eq!(
            SaidBack::with_posture(true).step(true, &yes(7), &store, &mut voice, 21),
            Step::Idle
        );
        // Posture switched off again: the posture line still covers it.
        assert_eq!(
            SaidBack::default().step(true, &yes(7), &store, &mut voice, 22),
            Step::Idle
        );
        assert_eq!(voice.asked.len(), 2);
    }

    #[test]
    fn a_forget_then_a_new_yes_needs_the_posture_line_again() {
        let store = Memory::default();
        let mut voice = Voice::default();
        let mut back = SaidBack::with_posture(true);
        say(
            &mut back,
            true,
            &yes(7),
            &store,
            &mut voice,
            0,
            on_posture(7),
        );
        assert!(back.said_posture(&yes(7), &store));
        // The forget: off, said back.
        say(
            &mut back,
            false,
            &Kept::default(),
            &store,
            &mut voice,
            10,
            Said::Off,
        );
        assert!(!back.said_posture(&yes(7), &store));
        // A new yes: no posture frame until its posture line began.
        assert!(!back.said_posture(&yes(30), &store));
        say(
            &mut back,
            true,
            &yes(30),
            &store,
            &mut voice,
            20,
            on_posture(30),
        );
        assert!(back.said_posture(&yes(30), &store));
        assert_eq!(voice.asked, [POSTURE_ON_LINE, OFF_LINE, POSTURE_ON_LINE]);
    }

    /// One step that asks for `said` and has it begun.
    fn say(
        back: &mut SaidBack,
        opted_in: bool,
        kept: &Kept,
        store: &Memory,
        voice: &mut Voice,
        now: u64,
        said: Said,
    ) {
        assert_eq!(
            back.step(opted_in, kept, store, voice, now),
            Step::Asked(said)
        );
        voice.next = Some(Heard::Begun);
        assert!(matches!(
            back.step(opted_in, kept, store, voice, now + 1),
            Step::Said(begun, _) if begun == said
        ));
    }

    #[test]
    fn said_once_when_turned_on_and_not_again() {
        let store = Memory::default();
        let mut voice = Voice::default();
        let mut back = SaidBack::default();
        assert_eq!(
            back.step(false, &Kept::default(), &store, &mut voice, 0),
            Step::Idle
        );
        assert!(voice.asked.is_empty(), "never on: neither line");
        assert_eq!(
            back.step(true, &yes(7), &store, &mut voice, 1),
            Step::Asked(on(7))
        );
        assert_eq!(voice.asked, [ON_LINE]);
        assert_eq!(
            back.step(true, &yes(7), &store, &mut voice, 2),
            Step::Waiting
        );
        voice.next = Some(Heard::Begun);
        assert_eq!(
            back.step(true, &yes(7), &store, &mut voice, 3),
            Step::Said(on(7), Ok(()))
        );
        assert_eq!(store.said(), Some(on(7)));
        for now in 4..100 {
            assert_eq!(
                back.step(true, &yes(7), &store, &mut voice, now),
                Step::Idle
            );
        }
        assert_eq!(voice.asked.len(), 1, "said once");
    }

    #[test]
    fn a_restart_while_on_says_nothing_again() {
        let store = Memory::default();
        *store.said.borrow_mut() = Some(on(7));
        let mut voice = Voice::default();
        let mut restarted = SaidBack::default();
        for now in 0..10 {
            assert_eq!(
                restarted.step(true, &yes(7), &store, &mut voice, now),
                Step::Idle
            );
        }
        assert!(voice.asked.is_empty());
    }

    #[test]
    fn not_begun_is_asked_again_later_only() {
        let store = Memory::default();
        let mut voice = Voice::default();
        let mut back = SaidBack::default();
        back.step(true, &yes(7), &store, &mut voice, 0);
        voice.next = Some(Heard::NotBegun("expired".into()));
        assert_eq!(
            back.step(true, &yes(7), &store, &mut voice, 1),
            Step::NotSaid(on(7), "expired".into())
        );
        assert_eq!(back.step(true, &yes(7), &store, &mut voice, 2), Step::Idle);
        assert_eq!(
            back.step(true, &yes(7), &store, &mut voice, 1 + RETRY_US),
            Step::Asked(on(7))
        );
        assert_eq!(voice.asked.len(), 2);
        assert_eq!(store.said(), None);
    }

    #[test]
    fn a_said_back_not_kept_is_still_not_said_again_in_this_run() {
        let store = Memory {
            keeps_nothing: true,
            ..Memory::default()
        };
        let mut voice = Voice::default();
        let mut back = SaidBack::default();
        back.step(true, &yes(7), &store, &mut voice, 0);
        voice.next = Some(Heard::Begun);
        assert!(matches!(
            back.step(true, &yes(7), &store, &mut voice, 1),
            Step::Said(_, Err(_))
        ));
        for now in 2..50 {
            back.step(true, &yes(7), &store, &mut voice, now);
        }
        assert_eq!(voice.asked.len(), 1);
        // Nor the off line twice.
        say(
            &mut back,
            false,
            &Kept::default(),
            &store,
            &mut voice,
            50,
            Said::Off,
        );
        for now in 60..100 {
            back.step(false, &Kept::default(), &store, &mut voice, now);
        }
        assert_eq!(voice.asked, [ON_LINE, OFF_LINE]);
    }

    #[test]
    fn turned_off_it_says_so_once_and_not_again_on_restart() {
        let store = Memory::default();
        let mut voice = Voice::default();
        let mut back = SaidBack::default();
        say(&mut back, true, &yes(7), &store, &mut voice, 0, on(7));
        // The yes taken back in the app.
        say(
            &mut back,
            false,
            &Kept::default(),
            &store,
            &mut voice,
            10,
            Said::Off,
        );
        assert_eq!(voice.asked, [ON_LINE, OFF_LINE]);
        assert_eq!(store.said(), Some(Said::Off));
        for now in 20..100 {
            assert_eq!(
                back.step(false, &Kept::default(), &store, &mut voice, now),
                Step::Idle
            );
        }
        // A restart while off.
        let mut restarted = SaidBack::default();
        for now in 0..10 {
            assert_eq!(
                restarted.step(false, &Kept::default(), &store, &mut voice, now),
                Step::Idle
            );
        }
        assert_eq!(voice.asked.len(), 2, "said once each way");
        // On again later: the on line again, then off again.
        say(&mut restarted, true, &yes(9), &store, &mut voice, 20, on(9));
        say(
            &mut restarted,
            false,
            &Kept::default(),
            &store,
            &mut voice,
            30,
            Said::Off,
        );
        assert_eq!(voice.asked, [ON_LINE, OFF_LINE, ON_LINE, OFF_LINE]);
    }

    #[test]
    fn a_forget_says_the_off_line() {
        let store = Memory::default();
        let mut voice = Voice::default();
        let mut back = SaidBack::default();
        say(&mut back, true, &yes(7), &store, &mut voice, 0, on(7));
        // A forget: the gate's opt-in is off, whether or not the voice has
        // removed the kept yes yet.
        say(&mut back, false, &yes(7), &store, &mut voice, 10, Said::Off);
        assert_eq!(voice.asked, [ON_LINE, OFF_LINE]);
        // A yes given again after it is said back again.
        assert_eq!(
            back.step(true, &yes(9), &store, &mut voice, 20),
            Step::Asked(on(9))
        );
    }

    #[test]
    fn no_off_line_when_the_on_line_was_never_said() {
        let store = Memory::default();
        let mut voice = Voice::default();
        let mut back = SaidBack::default();
        back.step(true, &yes(7), &store, &mut voice, 0);
        voice.next = Some(Heard::NotBegun("no say service".into()));
        back.step(true, &yes(7), &store, &mut voice, 1);
        for now in [2, 3, 2 + RETRY_US] {
            assert_eq!(
                back.step(false, &Kept::default(), &store, &mut voice, now),
                Step::Idle
            );
        }
        assert_eq!(voice.asked, [ON_LINE]);
    }

    #[test]
    fn an_off_line_not_begun_is_asked_again_while_still_off() {
        let store = Memory::default();
        *store.said.borrow_mut() = Some(on(7));
        let mut voice = Voice::default();
        let mut back = SaidBack::default();
        let off = Kept::default();
        assert_eq!(
            back.step(false, &off, &store, &mut voice, 0),
            Step::Asked(Said::Off)
        );
        voice.next = Some(Heard::NotBegun("refused".into()));
        back.step(false, &off, &store, &mut voice, 1);
        assert_eq!(back.step(false, &off, &store, &mut voice, 2), Step::Idle);
        // On again before the retry: no off line owed, and the yes kept
        // since is the one said back, so nothing.
        assert_eq!(back.step(true, &yes(7), &store, &mut voice, 3), Step::Idle);
        assert_eq!(
            back.step(false, &off, &store, &mut voice, 1 + RETRY_US),
            Step::Asked(Said::Off)
        );
    }

    #[test]
    fn a_forget_before_the_on_line_begins_withdraws_it_and_says_no_off_line() {
        let store = Memory::default();
        let mut voice = Voice::default();
        let mut back = SaidBack::default();
        assert_eq!(
            back.step(true, &yes(7), &store, &mut voice, 0),
            Step::Asked(on(7))
        );
        // The voice busy: a forget comes while the on line is still queued.
        assert_eq!(
            back.step(false, &Kept::default(), &store, &mut voice, 1),
            Step::Withdrawn(on(7))
        );
        assert_eq!(voice.cancels, 1);
        // Cancelled once, then its answer awaited.
        assert_eq!(
            back.step(false, &Kept::default(), &store, &mut voice, 2),
            Step::Waiting
        );
        assert_eq!(voice.cancels, 1);
        voice.next = Some(Heard::NotBegun("dropped: stopped by its asker".into()));
        assert_eq!(
            back.step(false, &Kept::default(), &store, &mut voice, 3),
            Step::NotSaid(on(7), "dropped: stopped by its asker".into())
        );
        // The on line never began: no off line is owed, now or later.
        for now in [4, 5, 4 + RETRY_US] {
            assert_eq!(
                back.step(false, &Kept::default(), &store, &mut voice, now),
                Step::Idle
            );
        }
        assert_eq!(voice.asked, [ON_LINE]);
        assert_eq!(store.said(), None);
        assert!(!back.said_on(&yes(7), &store));
    }

    #[test]
    fn a_withdrawn_on_line_that_had_begun_is_said_and_the_off_line_follows() {
        let store = Memory::default();
        let mut voice = Voice::default();
        let mut back = SaidBack::default();
        back.step(true, &yes(7), &store, &mut voice, 0);
        assert_eq!(
            back.step(false, &Kept::default(), &store, &mut voice, 1),
            Step::Withdrawn(on(7))
        );
        // It began before the cancel reached it: said, if cut.
        voice.next = Some(Heard::Begun);
        assert_eq!(
            back.step(false, &Kept::default(), &store, &mut voice, 2),
            Step::Said(on(7), Ok(()))
        );
        assert_eq!(
            back.step(false, &Kept::default(), &store, &mut voice, 3),
            Step::Asked(Said::Off)
        );
        assert_eq!(voice.asked, [ON_LINE, OFF_LINE]);
    }

    #[test]
    fn another_yes_while_the_on_line_is_queued_asks_for_the_new_one_at_once() {
        let store = Memory::default();
        let mut voice = Voice::default();
        let mut back = SaidBack::default();
        back.step(true, &yes(7), &store, &mut voice, 0);
        assert_eq!(
            back.step(true, &yes(8), &store, &mut voice, 1),
            Step::Withdrawn(on(7))
        );
        voice.next = Some(Heard::NotBegun("dropped: stopped by its asker".into()));
        back.step(true, &yes(8), &store, &mut voice, 2);
        // A withdrawn line waits no retry.
        assert_eq!(
            back.step(true, &yes(8), &store, &mut voice, 3),
            Step::Asked(on(8))
        );
        assert_eq!(voice.cancels, 1);
    }

    #[test]
    fn said_on_only_once_the_on_line_for_this_yes_began() {
        let store = Memory::default();
        let mut voice = Voice::default();
        let mut back = SaidBack::default();
        assert!(!back.said_on(&yes(7), &store));
        back.step(true, &yes(7), &store, &mut voice, 0);
        assert!(!back.said_on(&yes(7), &store), "asked, not begun");
        voice.next = Some(Heard::Begun);
        back.step(true, &yes(7), &store, &mut voice, 1);
        assert!(back.said_on(&yes(7), &store));
        assert!(!back.said_on(&yes(8), &store), "another yes");
        assert!(!back.said_on(&Kept::default(), &store), "no yes");
        // Kept from an earlier run.
        assert!(SaidBack::default().said_on(&yes(7), &store));
        say(
            &mut back,
            false,
            &Kept::default(),
            &store,
            &mut voice,
            10,
            Said::Off,
        );
        assert!(!back.said_on(&yes(7), &store), "said off since");
    }

    #[test]
    fn another_yes_is_said_back() {
        let store = Memory::default();
        *store.said.borrow_mut() = Some(on(7));
        let mut voice = Voice::default();
        let mut back = SaidBack::default();
        assert_eq!(
            back.step(true, &yes(8), &store, &mut voice, 0),
            Step::Asked(on(8))
        );
    }

    #[test]
    fn the_ask_and_its_answers() {
        assert_eq!(line(Said::Off), OFF_LINE);
        assert_eq!(line(on(1)), ON_LINE);
        assert_eq!(line(on_posture(1)), POSTURE_ON_LINE);
        assert!(POSTURE_ON_LINE.len() <= 900);
        assert!(OFF_LINE.len() <= 900);
        let ask: serde_json::Value = serde_json::from_str(ask_line(ON_LINE).trim()).unwrap();
        assert_eq!(ask["skill"], SKILL);
        assert_eq!(ask["text"], ON_LINE);
        assert_eq!(ask["priority"], PRIORITY);
        assert_eq!(ask["expires_in_ms"], EXPIRES_IN_MS);
        assert!(ask.get("note").is_none(), "word for word, not a note");
        assert!(ask_line(ON_LINE).ends_with('\n'));
        assert!(ON_LINE.len() <= 900);
        assert_eq!(
            heard_from(r#"{"event":"begun","at_us":5}"#),
            Some(Heard::Begun)
        );
        assert_eq!(
            heard_from(r#"{"outcome":"refused","why":"16 already waiting"}"#),
            Some(Heard::NotBegun("refused: 16 already waiting".into()))
        );
        assert_eq!(
            heard_from(r#"{"outcome":"expired"}"#),
            Some(Heard::NotBegun("expired".into()))
        );
        assert_eq!(heard_from("{}"), None);
        assert_eq!(heard_from("not json"), None);
    }

    /// Through a socket that answers as the say service does.
    #[test]
    fn through_a_say_socket() {
        use std::os::unix::net::UnixListener;
        let dir = PathBuf::from(format!("/tmp/lss-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        let socket = dir.join("say.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut ask = String::new();
            reader.read_line(&mut ask).unwrap();
            let mut stream = stream;
            stream
                .write_all(b"{\"event\":\"begun\",\"at_us\":1}\n")
                .unwrap();
            ask
        });
        let mut say = SayService::new(socket.clone());
        say.ask(ON_LINE);
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut heard = None;
        while heard.is_none() && std::time::Instant::now() < deadline {
            heard = say.heard();
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(heard, Some(Heard::Begun));
        assert_eq!(server.join().unwrap(), ask_line(ON_LINE));
        // No service: not begun, said why.
        let mut none = SayService::new(dir.join("missing.sock"));
        none.ask(ON_LINE);
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut heard = None;
        while heard.is_none() && std::time::Instant::now() < deadline {
            heard = none.heard();
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(matches!(heard, Some(Heard::NotBegun(_))));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A withdrawn ask writes the cancel on its own connection.
    #[test]
    fn a_cancel_goes_on_the_asks_own_connection() {
        use std::os::unix::net::UnixListener;
        let dir = PathBuf::from(format!("/tmp/lsc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        let socket = dir.join("say.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let (asked_tx, asked_rx) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut ask = String::new();
            reader.read_line(&mut ask).unwrap();
            asked_tx.send(()).unwrap();
            let mut cancel = String::new();
            reader.read_line(&mut cancel).unwrap();
            let mut stream = stream;
            stream
                .write_all(b"{\"outcome\":\"dropped\",\"why\":\"stopped by its asker\"}\n")
                .unwrap();
            (ask, cancel)
        });
        let mut say = SayService::new(socket.clone());
        say.ask(ON_LINE);
        asked_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        say.cancel();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut heard = None;
        while heard.is_none() && std::time::Instant::now() < deadline {
            heard = say.heard();
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(
            heard,
            Some(Heard::NotBegun("dropped: stopped by its asker".into()))
        );
        let (ask, cancel) = server.join().unwrap();
        assert_eq!(ask, ask_line(ON_LINE));
        assert_eq!(cancel, format!("{CANCEL_LINE}\n"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
