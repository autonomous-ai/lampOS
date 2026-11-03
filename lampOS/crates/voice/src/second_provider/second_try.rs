//! `LAMP_SECOND_CHAT_SECOND_TRY=on` (#4710): a turn's first chat round with no
//! response head [`SECOND_TRY_AFTER`] after the words goes once more on a new
//! connection, and whichever try has the first event answers. A head with no
//! event yet is a live connection and a slow answer, which is no case for a
//! second request. The first stays open, so a slow answer still arrives; the
//! other is dropped at the first event, so no word of it is read. Nothing else
//! changes: the wait's sound, its limit and the slow line are the turn's own,
//! and a round after a tool, or a look's, never gets one, since only before
//! anything has run can nothing happen twice. Nor does one go with too little
//! of the wait left to answer in ([`SECOND_TRY_ROOM`]), or when the first went
//! only at its time.
//!
//! It is also the test of what a silent chat is. A second try that answers
//! quickly says the first's kept connection was dead; one that can't open says
//! the lamp's network was down; one that opens and stays silent says the wait
//! is further along. The journal reads those three apart. Its lines keep clear
//! of the words `lamp-voice-eval` counts rounds by (`chat round`).
use super::net::{self, ChatEvent, Opening, Reach, Watch};
use futures_util::{
    StreamExt,
    stream::{self, BoxStream},
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::time::Instant;

/// Where the worker reads whether a first round goes twice: `on`, and nothing
/// else.
pub const CHAT_SECOND_TRY_ENVIRONMENT: &str = "LAMP_SECOND_CHAT_SECOND_TRY";
/// How long after the words a first round with no response head goes again: on
/// four lamps on 3 Nov, 5 of 143 first rounds passed 2 s, 1 passed 3 s, the
/// slowest 3.8 s (Backend's 12:0x on #4710).
pub const SECOND_TRY_AFTER: Duration = Duration::from_millis(2_500);
/// How much of the round's wait must be left for a second try to go: a new
/// connection and a first word need about this, or it only costs a request.
/// With the wait's sound on the wait is 4.5 s, which leaves 2 s; with it off
/// the wait is 3 s, which leaves 0.5 s, and none goes.
pub const SECOND_TRY_ROOM: Duration = Duration::from_millis(1_500);

/// Whether `value` turns [`CHAT_SECOND_TRY_ENVIRONMENT`] on.
pub fn second_try_on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// Whether a turn's first round may go twice: with the switch on, not a look,
/// and only while nothing of the turn has run, as only then can nothing happen
/// twice.
pub(super) fn may_go_twice(on: bool, look: bool, ran: bool) -> bool {
    on && !look && !ran
}

type Events = BoxStream<'static, net::Result<ChatEvent>>;

/// How a round's chat is told its functions, for
/// [`super::Services::chat_watched`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatTold {
    Every,
    Only(&'static [&'static str]),
    Without(&'static [&'static str]),
}

/// One request's second-try journal lines.
#[derive(Clone)]
pub(super) struct Say {
    request: u64,
    #[cfg(test)]
    journal: Option<Arc<Mutex<Vec<String>>>>,
}

impl Say {
    pub(super) fn new(request: u64) -> Self {
        Self {
            request,
            #[cfg(test)]
            journal: None,
        }
    }

    #[cfg(test)]
    pub(super) fn keeping(mut self, journal: Option<Arc<Mutex<Vec<String>>>>) -> Self {
        self.journal = journal;
        self
    }

    fn line(&self, words: &str) {
        let line = format!(
            "lamp-voice second: request {} second try{words}",
            self.request
        );
        #[cfg(test)]
        if let Some(journal) = &self.journal {
            journal.lock().unwrap().push(line.clone());
        }
        eprintln!("{line}");
    }

    /// How the second try's new connection went. One still opening when the
    /// turn ends is told by the end's line, as reaching nothing.
    fn opening(&self, opening: Opening) {
        match opening {
            Opening::Opened(took) => self.line(&format!(
                ": its new connection opened in {} ms",
                took.as_millis()
            )),
            Opening::Failed(took) => self.line(&format!(
                ": its new connection could not open, after {} ms",
                took.as_millis()
            )),
            Opening::Going => {}
        }
    }
}

/// Why no second try goes at its time, if none should: the first went only then
/// (`first_late`), so the two would go out together; the first's response head
/// came, so its connection is alive; or `left` of the round's wait is too little
/// to answer in. A first that has only been sent is the case the try is for.
fn why_none(first: &Reach, first_late: bool, left: Duration) -> Option<String> {
    if first_late {
        return Some("the first went only then".to_owned());
    }
    if first.answer_has_begun() {
        return Some("the first's answer has begun".to_owned());
    }
    (left < SECOND_TRY_ROOM).then(|| format!("only {} ms of the wait left", left.as_millis()))
}

/// A round's two tries, and the end's line. Dropped with the turn, however it
/// ends: a second try that went is always told an end.
pub(super) struct Tries {
    say: Say,
    from: Instant,
    first: Arc<Reach>,
    second: Mutex<Option<Arc<Reach>>>,
    /// Why a try failed, while the other went on waiting.
    failed: Mutex<[Option<String>; 2]>,
    over: AtomicBool,
}

impl Tries {
    /// Whether the second try went: the turn has no other, nor an ask again
    /// after a line lost on the network.
    pub(super) fn went(&self) -> bool {
        self.second.lock().is_ok_and(|second| second.is_some())
    }

    fn ms(&self) -> u128 {
        self.from.elapsed().as_millis()
    }

    /// None goes, and `why`. The turn's own lines tell the rest.
    fn none_goes(&self, why: &str) {
        self.say.line(&format!(
            ": none goes, {} ms after the words: {why}",
            self.ms()
        ));
    }

    fn going(&self, second: Arc<Reach>) {
        if let Ok(mut slot) = self.second.lock() {
            *slot = Some(second);
        }
        self.say.line(&format!(
            " goes {} ms after the words, on a new connection; the first stays open, having reached {} ({})",
            self.ms(),
            self.first.told(),
            CHAT_SECOND_TRY_ENVIRONMENT
        ));
    }

    /// What `which` (0 the first, 1 the second) reached, with why it failed.
    fn reached(&self, which: usize) -> String {
        let reach = match which {
            0 => self.first.told(),
            _ => self
                .second
                .lock()
                .ok()
                .and_then(|second| second.as_ref().map(|reach| reach.told()))
                .unwrap_or_else(|| "nothing".to_owned()),
        };
        let failed = self
            .failed
            .lock()
            .ok()
            .and_then(|failed| failed[which].clone());
        match failed {
            Some(why) => format!("{reach}, then failed: {why}"),
            None => reach,
        }
    }

    /// `which` answered: its first event came now.
    fn answered(&self, which: usize) {
        if self.over.swap(true, Ordering::SeqCst) {
            return;
        }
        let (name, other) = if which == 0 {
            ("first", "second")
        } else {
            ("second", "first")
        };
        self.say.line(&format!(
            ": the {name} answered, its first event {} ms after the words; the {other} was dropped, having reached {}",
            self.ms(),
            self.reached(1 - which)
        ));
    }

    fn failed(&self, which: usize, why: Option<&net::Error>) {
        let why = why.map_or(
            "it ended with no word or call".to_owned(),
            ToString::to_string,
        );
        let (name, other) = if which == 0 {
            ("first", "second")
        } else {
            ("second", "first")
        };
        self.say.line(&format!(
            ": the {name} ended {} ms after the words ({why}); the {other} still waits",
            self.ms()
        ));
        if let Ok(mut failed) = self.failed.lock() {
            failed[which] = Some(why);
        }
    }

    /// Neither answered, if a second try went and no end was told. Once.
    pub(super) fn neither(&self) {
        if !self.went() || self.over.swap(true, Ordering::SeqCst) {
            return;
        }
        self.say.line(&format!(
            ": neither answered in {} ms; the first reached {}, the second reached {}",
            self.ms(),
            self.reached(0),
            self.reached(1)
        ));
    }
}

impl Drop for Tries {
    fn drop(&mut self) {
        self.neither();
    }
}

/// `first`'s events, with one try more by `again` at `at` if it has no event and
/// no response head by then, and enough of the wait is left before `ends`
/// ([`why_none`]). Whichever has the first event is the stream's; the other is
/// dropped there and then. An error before any event is the stream's own while
/// it is the only try; with two it waits for the other, and comes only if that
/// fails too.
pub(super) fn race(
    first: Events,
    first_reach: Arc<Reach>,
    at: Instant,
    ends: Instant,
    from: Instant,
    say: Say,
    again: impl FnOnce(Watch) -> Option<Events> + Send + 'static,
) -> (Events, Arc<Tries>) {
    let tries = Arc::new(Tries {
        say,
        from,
        first: first_reach,
        second: Mutex::new(None),
        failed: Mutex::new([None, None]),
        over: AtomicBool::new(false),
    });
    let racing = stream::once(racing(first, at, ends, again, tries.clone()))
        .flat_map(|(head, rest)| stream::iter(head).chain(rest))
        .boxed();
    (racing, tries)
}

async fn racing(
    mut first: Events,
    at: Instant,
    ends: Instant,
    again: impl FnOnce(Watch) -> Option<Events>,
    tries: Arc<Tries>,
) -> (Option<net::Result<ChatEvent>>, Events) {
    let first_late = Instant::now() >= at;
    if !first_late {
        tokio::select! {
            biased;
            head = first.next() => return (head, first),
            () = tokio::time::sleep_until(at) => {}
        }
    }
    // Read now, not from `at`: the wait's sound can hold this stream past its
    // time, and it is what is left now that the try has to answer in.
    let left = ends.saturating_duration_since(Instant::now());
    if let Some(why) = why_none(&tries.first, first_late, left) {
        tries.none_goes(&why);
        return (first.next().await, first);
    }
    let say = tries.say.clone();
    let watch = Watch::second_to(&tries.first, Arc::new(move |opening| say.opening(opening)));
    let reach = watch.reach();
    let Some(mut second) = again(watch) else {
        return (first.next().await, first);
    };
    tries.going(reach);
    let (mut first_open, mut second_open) = (true, true);
    let mut errors: [Option<net::Error>; 2] = [None, None];
    while first_open || second_open {
        let (which, head) = tokio::select! {
            biased;
            head = first.next(), if first_open => (0, head),
            head = second.next(), if second_open => (1, head),
        };
        match head {
            Some(Ok(event)) => {
                tries.answered(which);
                return (Some(Ok(event)), if which == 0 { first } else { second });
            }
            other => {
                let error = other.and_then(Result::err);
                if which == 0 {
                    first_open = false;
                } else {
                    second_open = false;
                }
                if first_open || second_open {
                    tries.failed(which, error.as_ref());
                } else if let Some(error) = &error
                    && let Ok(mut failed) = tries.failed.lock()
                {
                    failed[which] = Some(error.to_string());
                }
                errors[which] = error;
            }
        }
    }
    tries.neither();
    let [first_error, second_error] = errors;
    (
        which_error(first_error, second_error).map(Err),
        stream::empty().boxed(),
    )
}

/// The turn's error when both tries failed: a service's answer over a broken
/// connection, as it decides what the lamp says; else the first's.
fn which_error(first: Option<net::Error>, second: Option<net::Error>) -> Option<net::Error> {
    match (first, second) {
        (Some(first @ net::Error::Status(_)), _) => Some(first),
        (_, Some(second @ net::Error::Status(_))) => Some(second),
        (first, second) => first.or(second),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_on_turns_it_on() {
        assert!(second_try_on(Some("on")));
        for off in [
            None,
            Some(""),
            Some("off"),
            Some("On"),
            Some("1"),
            Some("on "),
        ] {
            assert!(!second_try_on(off), "{off:?}");
        }
    }

    #[test]
    fn only_a_turn_with_nothing_run_and_no_look_goes_twice() {
        assert!(may_go_twice(true, false, false));
        assert!(!may_go_twice(false, false, false), "off");
        assert!(!may_go_twice(true, true, false), "a look");
        assert!(!may_go_twice(true, false, true), "something ran");
    }

    #[test]
    fn a_try_goes_only_while_the_first_has_no_head_and_the_wait_has_room() {
        let first = Reach::default();
        assert_eq!(why_none(&first, false, SECOND_TRY_ROOM), None);
        assert_eq!(why_none(&first, false, Duration::from_secs(2)), None);
        assert_eq!(
            why_none(&first, false, Duration::from_millis(500)).as_deref(),
            Some("only 500 ms of the wait left")
        );
        assert_eq!(
            why_none(&first, true, Duration::from_secs(2)).as_deref(),
            Some("the first went only then")
        );
    }

    /// Setup took the whole of the time: the first goes only now, and a second
    /// beside it would say nothing of a silent chat.
    #[tokio::test(start_paused = true)]
    async fn a_first_going_after_its_time_has_no_second_try() {
        async fn run(setup: Duration) -> (bool, Vec<String>) {
            let from = Instant::now();
            tokio::time::advance(setup).await;
            let journal: Arc<Mutex<Vec<String>>> = Arc::default();
            let asked = Arc::new(AtomicBool::new(false));
            let asking = asked.clone();
            let (mut racing, _tries) = race(
                stream::pending().boxed(),
                Arc::default(),
                from + SECOND_TRY_AFTER,
                from + Duration::from_millis(4_500),
                from,
                Say::new(7).keeping(Some(journal.clone())),
                move |_| {
                    asking.store(true, Ordering::SeqCst);
                    None
                },
            );
            assert!(
                tokio::time::timeout(Duration::from_secs(5), racing.next())
                    .await
                    .is_err()
            );
            let lines = journal.lock().unwrap().clone();
            (asked.load(Ordering::SeqCst), lines)
        }
        let (asked, lines) = run(Duration::from_millis(2_700)).await;
        assert!(!asked);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0].contains(
                "request 7 second try: none goes, 2700 ms after the words: the first went only then"
            ),
            "{lines:?}"
        );
        let (asked, lines) = run(Duration::from_millis(1_000)).await;
        assert!(asked, "a first out in time still gets its try");
        assert!(lines.is_empty(), "{lines:?}");
    }

    /// A service's answer decides what the lamp says, whichever try had it.
    #[test]
    fn a_service_answer_outranks_a_broken_connection() {
        let broken = || Some(net::Error::Transport("request"));
        let refused = || Some(net::Error::Status(429));
        assert_eq!(which_error(broken(), refused()), refused());
        assert_eq!(which_error(refused(), broken()), refused());
        assert_eq!(which_error(broken(), None), broken());
        assert_eq!(which_error(None, broken()), broken());
        assert_eq!(which_error(None, None), None);
    }
}
