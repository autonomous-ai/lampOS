//! Optional Schedule consumer. All path access, storage and Schedule calls are
//! isolated in one child. The coordinator polls only bounded local datagrams.
//! A prepared delivery is not permission to speak; a final Voice owner decides.
use crate::going_on_asks::{names_a_schedule_item, names_the_schedule};
use lamp_schedule::delivery::{PreparedToken, QualifiedCompletion, QualifiedRetirement};
use lamp_turn_owner::TurnOwner;
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, io, path::PathBuf, rc::Rc, time::Instant};

mod consumer;

/// `on` only: a reminder the person stopped over its ring (a spoken stop,
/// the head or the button), once some of it played, is held, not rung once
/// more 5 s later (Seventh's P3 on #5258). Off, such a stop is a cut like
/// any other (#5219's once more).
pub const RING_STOP_SWITCH: &str = "LAMP_RING_STOP_ENDS_IT";

/// How long after a ring was cut by a line that line's stop still holds it:
/// the consumer's pace before it is offered again (5 s), so a stop judged
/// later than that finds it offered already and changes nothing.
pub const STOP_FOR_THE_CUT_US: u64 = 5_000_000;

/// The attempt a settlement says was cut by a person's line, with some of it
/// played: a fenced `Retire` (a cut-in) or an `Abandon` with audio that
/// isn't already marked stopped.
fn cut_by_a_line(settlement: &ToWorker) -> Option<&PreparedToken> {
    match settlement {
        ToWorker::Retire { token, proof, .. }
            if matches!(
                proof.basis,
                lamp_schedule::delivery::RetirementBasis::OwnerFenced { .. }
            ) =>
        {
            Some(token)
        }
        ToWorker::Abandon {
            token,
            no_audio: false,
            stopped: false,
        } => Some(token),
        _ => None,
    }
}

/// Whether [`RING_STOP_SWITCH`] is on in this process's environment.
pub fn ring_stop_ends_it() -> bool {
    std::env::var(RING_STOP_SWITCH).ok().as_deref() == Some("on")
}
mod scheduling;
#[cfg(test)]
mod tests;
mod transport;

#[cfg(test)]
pub(crate) use consumer::roundtrip_tests::RoundtripFixture;

pub const SPEAK_VARIABLE: &str = "LAMP_SCHEDULE_SPEAK";

/// Whether Schedule speech is on: `on`, or `skill`, where a due reminder
/// also waits for a skill's `from: schedule` ask (#939, (c);
/// [`crate::say::SKILL_WAITS_US`]). The one rule for both the parent and
/// the provider worker's flag (`tools::schedule_arguments`): the worker's
/// came only for `on`, so under `skill` it refused every reminder and none
/// was said (the reviewer on #1038).
pub(crate) fn speaks(value: Option<&str>) -> bool {
    matches!(value, Some("on" | "skill"))
}

fn speaking(value: Result<&str, &std::env::VarError>) -> bool {
    speaks(value.ok())
}
/// With Schedule speech off the consumer never starts, so its start never
/// clears the reminders' clips (#8894): the voice's own start does, with
/// `value` the speech switch as read. How many files went.
pub(crate) fn clear_clips_without_speech(
    value: Option<&str>,
    clips: &crate::ring_clips::Clips,
) -> usize {
    if speaks(value) {
        return 0;
    }
    let gone = clips.clear();
    if gone > 0 {
        eprintln!(
            "lamp-voice: the offline ring clips were cleared ({gone} files): Schedule speech is off"
        );
    }
    gone
}
pub const LEDGER_VARIABLE: &str = "LAMP_SCHEDULE_DELIVERY_DIR";
/// `on` exactly (#5560): a delivery ledger fenced ahead of the store is
/// re-fenced to it at the voice's start, and while one still can't be, a
/// timer or reminder is not set and the model is told it couldn't be.
pub const REFENCE_VARIABLE: &str = "LAMP_LEDGER_REFENCE";
/// The file, in the delivery directory, the consumer keeps while its ledger
/// refuses every item, with [`REFENCE_VARIABLE`] on.
pub const CANNOT_RING_FILE: &str = "cannot-ring";
/// What the model is told for a set refused while the ledger can't ring.
pub const CANNOT_RING: &str = "Nothing was set: this lamp can't ring timers or reminders right now. Say you couldn't set it, and never that it is set.";

/// Older than this, [`CANNOT_RING_FILE`] is as absent: the consumer renews
/// it every tick (200 ms) while it holds, so one left by a consumer that
/// stopped blocks sets for at most this long.
pub const CANNOT_RING_FRESH: std::time::Duration = std::time::Duration::from_secs(2);

pub(crate) fn refence_on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// The refusal for a call that would set a ring (`set_timer`,
/// `set_reminder`, `repeat_reminder`) while [`CANNOT_RING_FILE`] is kept and
/// fresh, with [`REFENCE_VARIABLE`] on; `var` reads the environment.
pub(crate) fn cannot_ring(
    name: &str,
    var: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Option<serde_json::Value> {
    let sets = matches!(name, "set_timer" | "set_reminder" | "repeat_reminder");
    let on = refence_on(var(REFENCE_VARIABLE).as_deref().and_then(|v| v.to_str()));
    let dir = var(LEDGER_VARIABLE).map(PathBuf::from)?;
    let fresh = || {
        std::fs::metadata(dir.join(CANNOT_RING_FILE))
            .and_then(|meta| meta.modified())
            .is_ok_and(|at| at.elapsed().is_ok_and(|age| age < CANNOT_RING_FRESH))
    };
    (sets && on && fresh()).then(|| serde_json::json!({ "error": CANNOT_RING }))
}

#[cfg(test)]
thread_local! {
    /// [`lamp_actions::schedule::ASKS_HOW_LONG_SWITCH`] as a test of this
    /// thread sets it.
    static ASKS_HERE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// The switch for this test's thread alone.
#[cfg(test)]
pub(crate) fn set_asks_here(on: Option<bool>) {
    ASKS_HERE.with(|here| here.set(on));
}

#[cfg(test)]
thread_local! {
    /// [`asked_before`]'s words as a test of this thread sets them.
    static ASKED_HERE: std::cell::RefCell<Option<Option<(String, String)>>> =
        const { std::cell::RefCell::new(None) };
}

/// [`asked_before`]'s words for this test's thread alone, the person's lines
/// and the lamp's question; `None` reads the thread's.
#[cfg(test)]
pub(crate) fn set_asked_here(asked: Option<Option<(&str, &str)>>) {
    ASKED_HERE.with(|here| {
        *here.borrow_mut() = asked
            .map(|asked| asked.map(|(lines, question)| (lines.to_owned(), question.to_owned())))
    });
}

/// The person's lines the lamp's questions just before `request` answered
/// ([`crate::thread::Thread::asked_before`]), with the lamp's newest
/// question, the one the reply answers
/// ([`crate::thread::Thread::lamp_question_said`]): a reply to "What should
/// I remind you about?" is set from the line that gave the time.
fn asked_before(request: u64) -> Option<(String, String)> {
    #[cfg(test)]
    if let Some(asked) = ASKED_HERE.with(|here| here.borrow().clone()) {
        return asked;
    }
    let thread = crate::thread::THREAD.lock().ok()?;
    let lines = thread.asked_before(
        request,
        std::time::Instant::now(),
        crate::said_to_me::QUESTION_WAITS,
    )?;
    Some((lines, thread.lamp_question_said(request)?.to_owned()))
}

/// Whether [`lamp_actions::schedule::ASKS_HOW_LONG_SWITCH`] is on here.
fn asks_how_long() -> bool {
    #[cfg(test)]
    if let Some(on) = ASKS_HERE.with(std::cell::Cell::get) {
        return on;
    }
    lamp_actions::schedule::asks_how_long_from_environment()
}

/// The words that state a length or a clock time when no digit does:
/// numbers, units and a clock's names. Not a list for choosing a tool: it
/// only says whether the line gave any length at all ("a minute", "ten
/// minutes", "at nine", "at noon"); "o'clock" is read as "clock". "am"
/// and "pm" are left out: "I am" is no time, and "4 pm" has its digit.
const WHEN_WORDS: [&str; 44] = [
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
    "twenty",
    "thirty",
    "forty",
    "fifty",
    "sixty",
    "ninety",
    "hundred",
    "half",
    "quarter",
    "second",
    "seconds",
    "sec",
    "secs",
    "minute",
    "minutes",
    "min",
    "mins",
    "hour",
    "hours",
    "hr",
    "hrs",
    "noon",
    "midnight",
    "clock",
    "oclock",
];

/// Whether `heard` gives a length or a time: a digit, or a word of
/// [`WHEN_WORDS`].
fn gives_when(heard: &str) -> bool {
    heard.chars().any(|c| c.is_ascii_digit())
        || heard
            .split(|c: char| !c.is_alphabetic())
            .any(|word| WHEN_WORDS.contains(&word.to_lowercase().as_str()))
}

/// The refusal for a `set_timer` or `set_reminder` whose `request`'s own
/// heard line gives no length or time (#8182, gate 1126: speech to text
/// kept "set a timer" and lost "one minute", and the model set ten): the
/// model is told to ask once, in a few words, and the answer's line, which
/// gives one, sets it. Nor a reply to the lamp's own question whose asked
/// line gave one ([`asked_before`]: "Remind me at seven." → "What should I
/// remind you about?" → "Take my pills."), when the asked line was itself a
/// timer or reminder ask and the lamp's question asked about it: a fresh
/// ask ("Set another timer." after "It rings at 4:09. Anything else?")
/// carries its own length (Fifth's HOLD on #8274), and so does a reply
/// that names a timer, reminder, alarm or countdown itself ("Set another
/// timer." after "Want another timer?";
/// [`crate::going_on_asks::names_a_schedule_item`], Sixth's P3 on #8321),
/// while one that only says "remind" or the lamp ("Remind me to take my
/// pills.") still lends (the Reviewer-lead's P2 on #8274). With
/// [`lamp_actions::schedule::ASKS_HOW_LONG_SWITCH`]
/// on, in English only (the words are English), and only for a line a
/// person said: a typed text, or a line not known, checks nothing.
pub(crate) fn when_not_heard(name: &str, request: u64, heard: bool) -> Option<serde_json::Value> {
    let ask = match name {
        "set_timer" => "For how long?",
        "set_reminder" => "What time?",
        _ => return None,
    };
    if !asks_how_long()
        || crate::messages::is_text(request)
        || crate::language::at_start().is_some()
    {
        return None;
    }
    let words = crate::stop_scope::heard_words(request, heard)?;
    if gives_when(&words) || (!names_a_schedule_item(&words) && asked_gave_when(request)) {
        return None;
    }
    eprintln!(
        "lamp-voice: request {request}: {name} refused, its line gave no length or time; the model is told to ask"
    );
    Some(serde_json::json!({
        "error": format!("Nothing was set: the person's words gave no length or time for it, so it may have been lost in hearing. Never choose one. Ask only this, in a few words: \"{ask}\", and set it from their answer.")
    }))
}

/// Whether a line the lamp's questions just before `request` answered
/// ([`asked_before`]) was itself a timer or reminder ask that gave its
/// length or time ([`crate::going_on_asks::names_the_schedule`] and
/// [`gives_when`]), and the lamp's question, its last sentence, asked about
/// the timer or reminder too (the same word set): "What should I remind you
/// about?" asks for the asked one's label, while "Anything else?" invites a
/// new ask. Fifth's HOLD on #8274: "What's five plus five?" → "Ten.
/// Anything else?" lends "five" to nothing, and "Set a timer for ten
/// minutes." → "It rings at 4:09. Anything else?" lends "ten minutes" to
/// nothing.
fn asked_gave_when(request: u64) -> bool {
    asked_before(request).is_some_and(|(asked, question)| {
        let question = question.trim_end().trim_end_matches('?');
        let question = question.rsplit(['.', '!', '?']).next().unwrap_or(question);
        names_the_schedule(question)
            && asked
                .lines()
                .any(|line| names_the_schedule(line) && gives_when(line))
    })
}

/// Whether a call was told [`cannot_ring`]'s refusal: nothing was set.
pub(crate) fn is_cannot_ring(told: &serde_json::Value) -> bool {
    told["error"] == CANNOT_RING
}
pub const LEASE_US: u64 = 750_000;
/// Due this long ago or more, a reminder is said only when someone is at the
/// lamp, once and gently, as something asked for earlier, and is then
/// acknowledged (the first builder, 2026-10-16: lamp-4ace's "stretch",
/// twelve hours late, was never said and stayed pending).
pub const OVERDUE_MS: u64 = 60 * 60_000;

/// Without eyes that say someone is there, something said this recently
/// says it, for a reminder and a skill run's held write: two minutes, as
/// task news had it before #8783 (its own is about 15 s now,
/// `task_news::HEARD_WITHIN_US`).
pub(crate) const HEARD_WITHIN_US: u64 = 120_000_000;

/// Someone is at the lamp: the eyes say a face is there (`present`), or a
/// person said something within the last two minutes
/// ([`HEARD_WITHIN_US`]). A TV's speech counts too.
pub(crate) fn someone_here(present: bool, said_at_us: Option<u64>, now_us: u64) -> bool {
    present || said_at_us.is_some_and(|at| now_us.saturating_sub(at) < HEARD_WITHIN_US)
}

/// The overdue reminder last said to be waiting for someone, so the
/// journal says it once for each attempt.
#[derive(Default)]
pub(crate) struct OverdueWait {
    told: Option<PreparedToken>,
}
impl OverdueWait {
    /// Whether `offer` may begin now. While it may not, the journal says
    /// once for its attempt that it waits.
    pub(crate) fn may_begin(&mut self, offer: &Offer, someone_here: bool) -> bool {
        if offer.may_begin(someone_here) {
            return true;
        }
        if self.told.as_ref() != Some(&offer.token) {
            eprintln!(
                "lamp-voice: a reminder due over an hour ago, or by an unknown amount, waits for someone at the lamp"
            );
            self.told = Some(offer.token.clone());
        }
        false
    }
    #[cfg(test)]
    pub(crate) fn told(&self) -> Option<&PreparedToken> {
        self.told.as_ref()
    }
}
const POLL_TARGET_US: u64 = 2_000;
const POLL_PACKETS: usize = 4;

/// At most this many other late reminders are joined to the one offered, in
/// one line (29e7, 2 Nov: four said back to back in 34 s).
pub const MAX_JOINED: usize = 4;
/// The reminders said in one line have at most this many bytes of text in
/// all, as JSON ([`json_bytes`]), so the line's note fits one provider note.
pub const JOINED_TEXT_BYTES: usize = 240;

/// `text`'s bytes as a JSON string, quotes and escapes included.
pub fn json_bytes(text: &str) -> usize {
    serde_json::to_string(text).map_or(usize::MAX, |json| json.len())
}
/// A late reminder ([`Offer::late_line`]) waits for a pause in the talk: no
/// one's line heard for this long...
pub const TALK_PAUSE_US: u64 = 20_000_000;
/// ...but waits no longer than this for one, then is said at the next quiet
/// moment, as any due reminder is.
pub const TALK_PAUSE_WAIT_US: u64 = 3 * 60_000_000;

/// A late reminder's wait for a pause in the talk (29e7, 2 Nov: the four
/// came between the person's own lines, as the microphones opened): it may
/// begin once nobody has said a line for [`TALK_PAUSE_US`], or once it has
/// waited [`TALK_PAUSE_WAIT_US`]. One on time does not wait.
#[derive(Default)]
pub(crate) struct TalkPause {
    /// The attempt waiting, and since when.
    since: Option<(PreparedToken, u64)>,
}
impl TalkPause {
    pub(crate) fn may_begin(
        &mut self,
        offer: &Offer,
        said_at_us: Option<u64>,
        now_us: u64,
    ) -> bool {
        if !offer.late_line() {
            return true;
        }
        let talking = said_at_us.is_some_and(|at| now_us.saturating_sub(at) < TALK_PAUSE_US);
        if !talking {
            self.since = None;
            return true;
        }
        let since = match &self.since {
            Some((token, since)) if *token == offer.token => *since,
            _ => {
                eprintln!(
                    "lamp-voice: a late reminder waits for a pause in the talk, {} s at most",
                    TALK_PAUSE_WAIT_US / 1_000_000
                );
                self.since = Some((offer.token.clone(), now_us));
                now_us
            }
        };
        now_us.saturating_sub(since) >= TALK_PAUSE_WAIT_US
    }
}

/// Another late reminder said in the same line as the one offered
/// ([`Offer::also`]): its own prepared attempt, completed with it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Joined {
    pub token: PreparedToken,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Offer {
    pub token: PreparedToken,
    pub text: String,
    pub late: bool,
    pub late_by_ms: Option<u64>,
    pub observed_at_us: u64,
    pub expires_at_us: u64,
    /// Other reminders as late, said with it in one line (29e7, 2 Nov):
    /// at most [`MAX_JOINED`], each completed when it is.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also: Vec<Joined>,
    /// Offered once more after a line cut an attempt of it, or of one
    /// joined to it, with some of its audio played: the person may have
    /// heard part of it, so it is said as a repeat ("Again: …"), once.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub again: bool,
    /// A timer, by Schedule's `kind` for the item, not a reminder: at the
    /// usage limit it is rung with its own fixed line
    /// (`LAMP_LIMIT_LOCAL_ASKS`, #7642). Sent only when true.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub timer: bool,
    /// Rung already with no link from the generic line, "You have a
    /// reminder.", its own words never told (`LAMP_OFFLINE_RINGS`, #8708):
    /// not rung offline again, and said in its own words once the link is
    /// back. The consumer's ledger keeps the mark across a restart. Sent
    /// only when true.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rung_offline: bool,
}
impl Offer {
    /// Said late enough to be kept as said and to wait for a pause in the
    /// talk: late by [`lamp_schedule::engine::SAID_LATE_MS`] or more, or by
    /// an unknown amount.
    pub fn late_line(&self) -> bool {
        self.late_by_ms
            .is_none_or(|ms| self.late && ms >= lamp_schedule::engine::SAID_LATE_MS)
    }
    /// The texts of the others said with it.
    pub fn also_texts(&self) -> Vec<String> {
        self.also.iter().map(|joined| joined.text.clone()).collect()
    }
    /// Due an hour ago or more ([`OVERDUE_MS`]), or due by an unknown
    /// amount: Schedule gives no lateness while the wall clock is not
    /// trusted (the first minute after a boot), and that is never taken
    /// for on time (the reviewer on #1103).
    pub fn overdue(&self) -> bool {
        self.late_by_ms
            .is_none_or(|ms| self.late && ms >= OVERDUE_MS)
    }
    /// Whether it may begin now: an overdue one only with someone at the
    /// lamp, never into an empty room.
    pub fn may_begin(&self, someone_here: bool) -> bool {
        !self.overdue() || someone_here
    }

    fn fresh(&self, now_us: u64) -> bool {
        let text_ok = |text: &str| {
            !text.trim().is_empty() && text.len() <= 300 && !text.chars().any(char::is_control)
        };
        self.token.key.validate().is_ok()
            && self.token.attempt > 0
            && text_ok(&self.text)
            && self.also.len() <= MAX_JOINED
            && self.also.iter().all(|joined| {
                joined.token.key.validate().is_ok()
                    && joined.token.attempt > 0
                    && joined.token.key.store_id == self.token.key.store_id
                    && joined.token.key != self.token.key
                    && text_ok(&joined.text)
            })
            && self.observed_at_us <= now_us
            && self.observed_at_us < self.expires_at_us
            && self
                .observed_at_us
                .checked_add(LEASE_US)
                .is_some_and(|limit| self.expires_at_us <= limit)
            && now_us < self.expires_at_us
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ToWorker {
    Complete {
        token: PreparedToken,
        proof: QualifiedCompletion,
    },
    Retire {
        token: PreparedToken,
        proof: QualifiedRetirement,
        /// A real share of it was heard before the line that cut it in
        /// (the coordinator's `Reply::share_heard_at_cut`: half its answer
        /// and `CUT_HEARD_MIN_SAMPLES` or more handed to the speaker): the
        /// consumer offers it once more, said as a repeat
        /// ([`Offer::again`]), and holds it if such a cut comes again, as for
        /// an abandon with audio. Sent only when true, so the common packet
        /// is the same as before.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        heard_some: bool,
        /// Some of its audio came, but less than that share (an echo's or a
        /// cough's cut a few frames in, #4938, #5491): offered again as
        /// before, never counted toward the hold, but at most
        /// `LIGHT_CUT_RETRIES` times for each item. Sent only when true.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        light_cut: bool,
    },
    /// The reply ended with neither a completion nor a qualified
    /// retirement (#5219).
    Abandon {
        token: PreparedToken,
        /// No audio of the reply was ever received for the speaker
        /// (`Receipt::audio` never called): the turn gave up before a sound
        /// of it was played (lamp-9adf, build 10012: `GaveUp` when the chat
        /// had not begun in 3 s), so it was certainly not heard. Absent, as
        /// from a build before it, it may have been heard. Sent only when
        /// true, so the common packet is the same as before.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        no_audio: bool,
        /// The person stopped it over its ring, by a spoken stop, the head
        /// or the button, after some of it had played: they heard it, and a
        /// ring again 5 s later would be the lamp not listening (Seventh's P3
        /// on #5258). Only with [`RING_STOP_SWITCH`] on; sent only when true,
        /// so the common packet is the same as before, and a packet without
        /// it is taken as cut by talk.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        stopped: bool,
    },
    /// Someone said a line after a reminder was said (29e7, 2 Nov): one
    /// said late is noticed ([`lamp_schedule::protocol::Request::Noticed`]).
    Noticed {},
    /// The person's line that cut the ring `token` was a stop (the second
    /// route answered it "Okay, I'll stop there"): it is held, not rung once
    /// more (the Reviewer-lead's P2 on #5351). Only with
    /// [`RING_STOP_SWITCH`] on.
    RingStopped { token: PreparedToken },
    /// The ring `token` was rung with no link from the generic line, "You
    /// have a reminder.", played whole, and its own words were never told
    /// (`LAMP_OFFLINE_RINGS`, #8708): it is kept "rung, not told" and
    /// offered again, said in its words once the link is back.
    RungOffline { token: PreparedToken },
    /// The link is back after a ring was sent as [`Self::RungOffline`]: the
    /// rings waiting for it are offered again, said in their words (the
    /// Reviewer-lead's P2 on #8800).
    LinkBack {},
}
/// The soonest ring not yet due (`LAMP_OFFLINE_RINGS`, #8632): its Schedule
/// identity and when it is due on the machine's monotonic clock, as the
/// consumer read it. Never its words.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Upcoming {
    pub id: u64,
    pub delivery: u64,
    pub due_at_us: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum FromWorker {
    Offer {
        offer: Offer,
    },
    /// With `LAMP_OFFLINE_RINGS=on` (#8632), each time it changes: the
    /// soonest timer or reminder set and not yet due, for the link's check
    /// before it rings ([`crate::offline_rings::asks_link_check`]). None
    /// when nothing is set.
    Upcoming {
        #[serde(default)]
        soonest: Option<Upcoming>,
    },
    Withdraw {
        token: PreparedToken,
    },
    Settled {
        token: PreparedToken,
    },
    Idle {},
}

struct Progress {
    token: PreparedToken,
    /// Aggregate partial playback cannot prove every item in a joined line.
    joined: bool,
    owner: TurnOwner,
    privacy: u64,
    session: u64,
    had_audio: bool,
    qualified: bool,
    retired: u64,
    final_cursor: Option<(u64, u64, u64)>,
    live: bool,
    stopped: bool,
    /// The request whose line cut this reply in (`interrupted_after_owner_fenced`):
    /// only that line's stop may hold the ring (Thirteen's P3 on #5351).
    cut_by: Option<u64>,
    /// A real share of it was heard before a line cut it in
    /// ([`Receipt::share_heard_at_cut`]).
    heard_share: bool,
    /// Rung offline from the generic line ([`Receipt::untold`]): played
    /// whole, it is [`ToWorker::RungOffline`], not a completion.
    untold: bool,
    settlement: Option<ToWorker>,
}
impl Progress {
    /// The caller has established that this exact owner cannot produce
    /// more sound. This does not assert that none of it was heard.
    fn retire_owner_fenced(&mut self) {
        if self.settlement.is_some() {
            return;
        }
        let proof = QualifiedRetirement {
            correlation: self.token.correlation.clone(),
            retired_by_session: self.token.correlation.consumer_session.clone(),
            basis: lamp_schedule::delivery::RetirementBasis::OwnerFenced {
                owner_boot: self.owner.boot().bytes(),
                turn: self.owner.turn(),
                microphone_generation: self.privacy,
            },
        };
        self.settlement = Some(ToWorker::Retire {
            token: self.token.clone(),
            proof,
            heard_some: self.had_audio && self.heard_share,
            light_cut: self.had_audio && !self.heard_share,
        });
    }

    fn eligible_completion_cursor(&self) -> Option<(u64, u64, u64)> {
        if self.qualified && self.had_audio && self.retired > 0 && self.settlement.is_none() {
            self.final_cursor
        } else {
            None
        }
    }
}

/// Lives on the actual coordinator Reply. The owning consumer keeps only the
/// shared, bounded settlement slot, so a dropped Reply cannot silently become
/// completed speech. No destructor performs I/O or asserts cancellation.
pub(crate) struct Receipt(Rc<RefCell<Progress>>);
impl Receipt {
    pub fn invalidate(&mut self) {
        self.0.borrow_mut().qualified = false;
    }
    pub fn audio(&mut self) {
        self.0.borrow_mut().had_audio = true;
    }
    /// The person stopped this reply over its ring (a spoken stop, the head
    /// or the button). Counts only once some of its audio came; the drop
    /// then says so. The coordinator calls it only with
    /// [`RING_STOP_SWITCH`] on.
    pub fn stopped_by_the_person(&mut self) {
        let mut state = self.0.borrow_mut();
        state.stopped = state.had_audio;
    }
    pub fn retired(&mut self, sequence: u64, epoch: u64, frame: u64) {
        let mut state = self.0.borrow_mut();
        state.retired = state.retired.saturating_add(1);
        state.final_cursor = Some((sequence, epoch, frame));
    }
    /// Whether this receipt has its own completion proof. The coordinator
    /// must still perform the successful whole-reply join; this changes nothing.
    pub fn eligible_for_completion(&self) -> bool {
        self.0.borrow().eligible_completion_cursor().is_some()
    }
    /// Called only after the real successful whole-reply join, never directly
    /// from SpeechRetired (which retires only one PCM generation).
    pub fn completed(&mut self) {
        let mut state = self.0.borrow_mut();
        let Some((playback_sequence, final_epoch, final_frame)) =
            state.eligible_completion_cursor()
        else {
            return;
        };
        if state.untold {
            state.settlement = Some(ToWorker::RungOffline {
                token: state.token.clone(),
            });
            return;
        }
        let proof = QualifiedCompletion {
            correlation: state.token.correlation.clone(),
            owner_boot: state.owner.boot().bytes(),
            turn: state.owner.turn(),
            microphone_generation: state.privacy,
            provider_session: state.session,
            playback_sequence,
            final_epoch,
            final_frame,
            retired_generations: state.retired,
        };
        state.settlement = Some(ToWorker::Complete {
            token: state.token.clone(),
            proof,
        });
    }
    /// The result already recorded by `completed`, read after the whole-reply
    /// join. A skill's say must not infer success from playback after a reset
    /// invalidated the receipt. This does not claim the worker durably Acked it.
    /// This ring is the generic offline line, "You have a reminder.", not
    /// the reminder's own words (`LAMP_OFFLINE_RINGS`, #8708): played whole
    /// or mostly heard, it is settled as [`ToWorker::RungOffline`], so its
    /// words are told once the link is back.
    pub fn untold(&mut self) {
        self.0.borrow_mut().untold = true;
    }
    pub fn completion_recorded(&self) -> bool {
        matches!(self.0.borrow().settlement, Some(ToWorker::Complete { .. }))
    }
    /// A provider deferred before any PCM was received and the coordinator
    /// successfully revoked/published the exact owner. Such an attempt can be
    /// retried later. Partially spoken/otherwise uncertain attempts stay pending.
    pub fn deferred_before_audio(&mut self) {
        let mut state = self.0.borrow_mut();
        if state.had_audio {
            return;
        }
        state.retire_owner_fenced();
    }

    /// A positively admitted user interruption may be retried even after
    /// partial audio before any own retirement. Call only after cancelling
    /// this exact owner, clearing
    /// its queued output and successfully publishing its revoked/replacement
    /// authority. A reason string or a failed publication proves no fence.
    /// The coordinator retains this receipt across that operation; on any
    /// failure its ordinary Drop leaves the uncertain attempt held instead.
    /// An invalidated receipt or any qualified own retirement stays held.
    /// A generation retirement is not a whole-provider completion, but may
    /// already be the final words; conservatively do not repeat that attempt.
    /// `cut_by` is the request of the line that cut it in, when the
    /// coordinator knows it: only that line's stop may hold the ring.
    /// A line cut this reply in after most of its words had played (the
    /// coordinator's `Reply::mostly_heard_at_cut`): it was heard. It is
    /// completed on the cut's own cursor, the playback occurrence it was in
    /// and the frames handed to the speaker, the cut counting as the
    /// generation that ended it, rather than retired to be said again
    /// (06c1, 5 Nov 17:04: "Time to drink some water." was cut by a
    /// wordless cut-in after 4 of its 5 words and said again 10 s later).
    /// Only a live, qualified receipt for one item, with audio and no
    /// settlement yet; whether it was completed. A joined line has no
    /// per-item audio boundaries, so its unplayed tail may contain a whole
    /// reminder. Its partial cut keeps the existing retry-or-held handling.
    pub fn heard_before_the_cut(&mut self, playback_sequence: u64, frames: u64) -> bool {
        let mut state = self.0.borrow_mut();
        if state.joined
            || !state.live
            || !state.qualified
            || !state.had_audio
            || state.settlement.is_some()
            || playback_sequence == 0
            || frames == 0
        {
            return false;
        }
        if state.untold {
            state.settlement = Some(ToWorker::RungOffline {
                token: state.token.clone(),
            });
            return true;
        }
        let proof = QualifiedCompletion {
            correlation: state.token.correlation.clone(),
            owner_boot: state.owner.boot().bytes(),
            turn: state.owner.turn(),
            microphone_generation: state.privacy,
            provider_session: state.session,
            playback_sequence,
            final_epoch: 0,
            final_frame: frames,
            retired_generations: state.retired.saturating_add(1),
        };
        state.settlement = Some(ToWorker::Complete {
            token: state.token.clone(),
            proof,
        });
        true
    }

    /// A real share of this reply was heard before the line that is cutting
    /// it in (the coordinator's `Reply::share_heard_at_cut`): its fenced
    /// retirement then counts toward the hold ([`ToWorker::Retire`]'s
    /// `heard_some`). Without it, a cut after some audio is a light one.
    pub fn share_heard_at_cut(&mut self) {
        self.0.borrow_mut().heard_share = true;
    }

    pub fn interrupted_after_owner_fenced(&mut self, cut_by: Option<u64>) {
        let mut state = self.0.borrow_mut();
        if !state.live || !state.qualified || state.eligible_completion_cursor().is_some() {
            return;
        }
        state.cut_by = cut_by;
        state.retire_owner_fenced();
    }
}
impl Drop for Receipt {
    fn drop(&mut self) {
        let mut state = self.0.borrow_mut();
        state.live = false;
        if state.settlement.is_none() {
            state.settlement = Some(ToWorker::Abandon {
                token: state.token.clone(),
                no_audio: !state.had_audio,
                stopped: state.stopped,
            });
        }
    }
}

#[cfg(test)]
pub(crate) struct Probe(Rc<RefCell<Progress>>);
#[cfg(test)]
impl Probe {
    pub fn settlement(&self) -> Option<ToWorker> {
        self.0.borrow().settlement.clone()
    }
}
#[cfg(test)]
pub(crate) fn receipt_for_test(owner: TurnOwner, privacy: u64, session: u64) -> (Receipt, Probe) {
    let state = Rc::new(RefCell::new(Progress {
        joined: false,
        token: PreparedToken {
            key: lamp_schedule::delivery::DeliveryKey {
                store_id: "0123456789abcdef0123456789abcdef".into(),
                id: 1,
                delivery: 1,
            },
            attempt: 1,
            correlation: lamp_schedule::delivery::Correlation {
                consumer_session: "test-consumer".into(),
                sequence: 1,
            },
        },
        owner,
        privacy,
        session,
        had_audio: false,
        qualified: true,
        retired: 0,
        final_cursor: None,
        live: true,
        stopped: false,
        cut_by: None,
        heard_share: false,
        untold: false,
        settlement: None,
    }));
    (Receipt(Rc::clone(&state)), Probe(state))
}

/// One instance belongs to run_source, outside every mute/unmute audio cycle.
pub(crate) struct Reminders {
    transport: transport::Transport,
    offer: Option<Offer>,
    active: Option<Rc<RefCell<Progress>>>,
    unadmitted: Option<ToWorker>,
    sent: bool,
    failed: bool,
    /// `LAMP_SCHEDULE_SPEAK=skill`: a due reminder waits for a skill to ask.
    by_skill: bool,
    /// The attempt last said to be owed while the microphones are off
    /// ([`Self::owed_while_muted`]), so the journal says it once for each.
    owed_told: Option<PreparedToken>,
    /// When a reminder's completion was last sent, until a line is heard
    /// after it ([`Self::heard`]).
    said_at_us: Option<u64>,
    /// A line was heard after a reminder: Schedule is to be told.
    notice_owed: bool,
    /// The ring last cut by a line (sent as a fenced `Retire` or an
    /// `Abandon` with audio), and when: a stop said for it within
    /// [`STOP_FOR_THE_CUT_US`] holds it ([`Self::stop_said`]).
    cut_last: Option<(PreparedToken, u64, Option<u64>)>,
    /// A [`ToWorker::RingStopped`] still to be sent.
    stop_owed: Option<PreparedToken>,
    /// The soonest ring not yet due, as the consumer last told it
    /// (`LAMP_OFFLINE_RINGS`, #8632).
    upcoming: Option<Upcoming>,
    /// A ring was sent as [`ToWorker::RungOffline`]: the consumer is told
    /// once the link is back ([`Self::link_ready`]), and whether that is
    /// still to be sent.
    link_waits: bool,
    link_back_owed: bool,
}
impl Reminders {
    pub fn from_environment() -> io::Result<Option<Self>> {
        let value = std::env::var(SPEAK_VARIABLE);
        if !speaking(value.as_deref()) {
            return Ok(None);
        }
        let by_skill = value.as_deref() == Ok("skill");
        // Config is validated in the child. A slow/missing directory cannot
        // delay audio startup; parent startup only creates/spawns its IPC peer.
        let transport = transport::Transport::spawn(&crate::process::own_binary()?)?;
        Ok(Some(Self {
            transport,
            offer: None,
            active: None,
            unadmitted: None,
            sent: false,
            failed: false,
            by_skill,
            owed_told: None,
            said_at_us: None,
            notice_owed: false,
            cut_last: None,
            stop_owed: None,
            upcoming: None,
            link_waits: false,
            link_back_owed: false,
        }))
    }

    /// The second route answered a line as a stop over the lamp's answer
    /// (`LAMP_SPOKEN_STOP_ENDS`): if that line cut a ring within
    /// [`STOP_FOR_THE_CUT_US`], the ring is held, not rung once more
    /// (the Reviewer-lead's P2 on #5351). The caller checks
    /// [`RING_STOP_SWITCH`].
    /// `request` is the stop's own request: a ring a known line cut is held
    /// only by that line's stop, never by a stop said for another (Thirteen's
    /// P3 on #5351); one cut by talk, whose line isn't known, as before.
    pub fn stop_said(&mut self, request: u64, now_us: u64) {
        let Some((_, cut_at_us, cut_by)) = &self.cut_last else {
            return;
        };
        if now_us.saturating_sub(*cut_at_us) > STOP_FOR_THE_CUT_US {
            self.cut_last = None;
            return;
        }
        if cut_by.is_some_and(|cut_by| cut_by != request) {
            return;
        }
        if let Some((token, _, _)) = self.cut_last.take() {
            self.stop_owed = Some(token);
        }
    }

    /// Someone's line was last heard at `said_at_us`: one heard after a
    /// reminder was said is their answer to it, so a reminder said late
    /// no longer waits for one (29e7, 2 Nov).
    pub fn heard(&mut self, said_at_us: Option<u64>) {
        if let (Some(said), Some(heard)) = (self.said_at_us, said_at_us)
            && heard > said
        {
            self.said_at_us = None;
            self.notice_owed = true;
        }
    }

    /// Whether a due reminder waits for a skill's ask (`LAMP_SCHEDULE_SPEAK=skill`).
    pub fn by_skill(&self) -> bool {
        self.by_skill
    }

    pub fn poll(&mut self, now_us: u64) {
        if self.failed {
            // Retain and reap the one failed child; never manufacture a new
            // consumer merely because its termination signal was sent.
            let _ = self.transport.try_exit();
            return;
        }
        if let Err(error) = self.poll_inner(now_us) {
            // Due reminders stay pending until the voice restarts: say so.
            eprintln!(
                "lamp-voice: Schedule consumer failed ({error}); due reminders wait for a restart"
            );
            self.failed = true;
            self.offer = None;
            self.transport.stop();
        }
    }
    fn poll_inner(&mut self, now_us: u64) -> io::Result<()> {
        let started = Instant::now();
        self.transport.poll()?;
        if self.transport.try_exit()? {
            return Err(io::Error::other("schedule consumer exited"));
        }
        for _ in 0..POLL_PACKETS {
            if started.elapsed().as_micros() >= u128::from(POLL_TARGET_US) {
                break;
            }
            let Some(message) = self.transport.receive::<FromWorker>()? else {
                break;
            };
            match message {
                FromWorker::Offer { offer }
                    if self.transport.claim_held() && offer.fresh(now_us) =>
                {
                    if self.unadmitted.is_some() {
                        continue;
                    } else if let Some(active) = &self.active {
                        if active.borrow().token == offer.token {
                            self.offer = Some(offer);
                        }
                    } else {
                        self.offer = Some(offer);
                    }
                }
                FromWorker::Offer { .. } => self.offer = None,
                FromWorker::Withdraw { token } => {
                    if self
                        .offer
                        .as_ref()
                        .is_some_and(|offer| offer.token == token)
                    {
                        self.offer = None;
                    }
                    if self.active.is_none() && self.unadmitted.is_none() {
                        self.unadmitted = Some(ToWorker::Retire {
                            proof: QualifiedRetirement {
                                correlation: token.correlation.clone(),
                                retired_by_session: token.correlation.consumer_session.clone(),
                                basis: lamp_schedule::delivery::RetirementBasis::NeverAdmitted {},
                            },
                            token,
                            heard_some: false,
                            light_cut: false,
                        });
                        self.sent = false;
                    }
                }
                FromWorker::Settled { token } => {
                    if self
                        .unadmitted
                        .as_ref()
                        .is_some_and(|command| match command {
                            ToWorker::Retire { token: pending, .. }
                            | ToWorker::RungOffline { token: pending } => pending == &token,
                            _ => false,
                        })
                    {
                        self.unadmitted = None;
                        self.offer = None;
                        self.sent = false;
                    }
                    if self
                        .active
                        .as_ref()
                        .is_some_and(|state| state.borrow().token == token && !state.borrow().live)
                    {
                        self.active = None;
                        self.offer = None;
                        self.sent = false;
                    }
                }
                FromWorker::Upcoming { soonest } => self.upcoming = soonest,
                FromWorker::Idle {} => {}
            }
        }
        if !self.sent
            && let Some(active) = &self.active
        {
            let state = active.borrow();
            if !state.live
                && let Some(settlement) = &state.settlement
            {
                self.sent = self.transport.send(settlement)?;
                if self.sent && matches!(settlement, ToWorker::RungOffline { .. }) {
                    self.link_waits = true;
                }
                if self.sent && matches!(settlement, ToWorker::Complete { .. }) {
                    self.said_at_us = Some(now_us);
                }
                if self.sent
                    && let Some(token) = cut_by_a_line(settlement)
                {
                    self.cut_last = Some((token.clone(), now_us, state.cut_by));
                }
            }
        }
        if self.notice_owed && self.transport.send(&ToWorker::Noticed {})? {
            self.notice_owed = false;
        }
        if let Some(token) = &self.stop_owed
            && self.transport.send(&ToWorker::RingStopped {
                token: token.clone(),
            })?
        {
            self.stop_owed = None;
        }
        if !self.sent
            && let Some(settlement) = &self.unadmitted
        {
            self.sent = self.transport.send(settlement)?;
            if self.sent && matches!(settlement, ToWorker::RungOffline { .. }) {
                self.link_waits = true;
            }
        }
        if self.link_back_owed && self.transport.send(&ToWorker::LinkBack {})? {
            self.link_back_owed = false;
        }
        Ok(())
    }
    /// `offer`, rung offline before from the generic line
    /// ([`Offer::rung_offline`]), is due with no link (after a restart): it
    /// is sent back as rung offline, so the consumer passes it over until
    /// the link is back and later rings go on (the Reviewer-lead's P2 on
    /// #8800).
    pub fn wait_for_link(&mut self, offer: &Offer) {
        if !offer.rung_offline || self.active.is_some() || self.unadmitted.is_some() {
            return;
        }
        if self.offer.as_ref().is_some_and(|o| o.token == offer.token) {
            self.offer = None;
        }
        self.unadmitted = Some(ToWorker::RungOffline {
            token: offer.token.clone(),
        });
        self.sent = false;
    }
    /// The link is ready: if a ring was sent as rung offline, the consumer
    /// is told, once, so the rings waiting for it are offered again.
    pub fn link_ready(&mut self) {
        if std::mem::take(&mut self.link_waits) {
            self.link_back_owed = true;
        }
    }
    /// The soonest ring not yet due (`LAMP_OFFLINE_RINGS`, #8632), none
    /// once the consumer failed.
    pub fn upcoming(&self) -> Option<Upcoming> {
        self.upcoming.filter(|_| !self.failed)
    }
    pub fn offer(&self, now_us: u64) -> Option<Offer> {
        if self.failed
            || self.active.is_some()
            || self.unadmitted.is_some()
            || !self.transport.claim_held()
        {
            return None;
        }
        self.offer
            .as_ref()
            .filter(|offer| offer.fresh(now_us))
            .cloned()
    }
    /// Whether a due reminder is owed while the microphones are off, as the
    /// muted wait asks at each look (#1545): `Some(true)` the first time for
    /// its attempt, which the journal says once, `Some(false)` after, and
    /// `None` when nothing is offered. A muted lamp opens no session, so the
    /// reminder waits, still offered, and is said once they open, by the
    /// rules of any due reminder (an hour late or more, only with someone
    /// at the lamp). On lamp-cb94 on 17 October one came due 21 minutes
    /// into a mute and nothing anywhere said it was owed (the first builder,
    /// #1545).
    pub fn owed_while_muted(&mut self, now_us: u64) -> Option<bool> {
        let offer = self.offer(now_us)?;
        if self.owed_told.as_ref() == Some(&offer.token) {
            return Some(false);
        }
        eprintln!(
            "lamp-voice: a reminder is due while the microphones are off: it waits until they open"
        );
        self.owed_told = Some(offer.token);
        Some(true)
    }
    pub fn active_valid(&self, owner: TurnOwner, now_us: u64) -> bool {
        let Some(active) = &self.active else {
            return true;
        };
        let state = active.borrow();
        state.owner != owner
            || (!self.failed
                && self
                    .offer
                    .as_ref()
                    .is_some_and(|offer| offer.token == state.token && offer.fresh(now_us)))
    }
    pub fn admitted(
        &mut self,
        offer: Offer,
        owner: TurnOwner,
        privacy: u64,
        session: u64,
    ) -> Receipt {
        let state = Rc::new(RefCell::new(Progress {
            joined: !offer.also.is_empty(),
            token: offer.token,
            owner,
            privacy,
            session,
            had_audio: false,
            qualified: true,
            retired: 0,
            final_cursor: None,
            live: true,
            stopped: false,
            cut_by: None,
            heard_share: false,
            untold: false,
            settlement: None,
        }));
        self.active = Some(Rc::clone(&state));
        self.sent = false;
        Receipt(state)
    }
}

/// Internal child entry. It opens no model provider, microphone, speaker or
/// actuator. With `LAMP_OFFLINE_RINGS=on` it renders the reminders' offline
/// clips through the speech service, on a thread of its own
/// (`crate::ring_clips`, #8708): it is the one process that reads their
/// words, and its packets to the voice still carry none.
pub fn run_consumer() -> io::Result<()> {
    if !speaking(std::env::var(SPEAK_VARIABLE).as_deref()) {
        return Err(io::Error::other("Schedule speech is disabled"));
    }
    scheduling::background()?;
    let ledger = std::env::var_os(LEDGER_VARIABLE)
        .map(PathBuf::from)
        .ok_or_else(|| {
            io::Error::other("Schedule speech needs an explicit persistent delivery directory")
        })?;
    let socket = std::env::var_os("LAMP_SCHEDULE_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/run/lampos-schedule/call.sock"));
    let root = std::env::var_os(lamp_bus::ROOT_VARIABLE)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(lamp_bus::DEFAULT_ROOT));
    consumer::run(&ledger, &socket, &root)
}
