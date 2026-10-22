//! A skill's short line for the person's phone, which no text asked for
//! (`messages/out`, `docs/messages.md`, "Unasked texts"): guard mode's
//! "Someone was at your desk at 14:31, for about 2 minutes." With
//! `LAMP_MESSAGES=on`, `lamp-cloud` takes each piece off the bus and sends
//! it up as V1's `chat.event` with no run, a `task_news` event, as the
//! voice's unasked task news goes (`crate::chat`): the skill's text only,
//! nothing added. The backend keeps it for the owner and pushes it to their
//! phones.
//!
//! Only `lamp-skills`' lines are taken ([`SKILLS`], its publisher name):
//! a line another publisher puts on the topic, a test tool, a `lamp-bus`
//! publish from a shell or a part that writes it later, never reaches the
//! phone, and the journal names that publisher once.
//!
//! Bounded on the lamp's side as well as the skill's: at most [`PER_HOUR`]
//! pieces are taken in any hour, the backend's own push limit, and a newer
//! one past it is dropped. Pieces keep their order, and one heard again
//! (the same publisher, boot and `seq`) is dropped. While the broker is away at
//! most [`KEPT_OFFLINE`] wait in memory, the oldest dropped first, and go
//! on reconnect; a restart of `lamp-cloud` loses them.
//!
//! A named text (guard mode with `LAMP_GUARD_NAMES=on`, #2562) is sent
//! named only when it can go at once, with the broker there, and the eyes'
//! `tellable` file, read just then, still has a yes from each one it names
//! (`lamp_msgs::known::still_tellable`, [`Outbox::due_checked`]): a text
//! that waited on the bus while a yes was taken back goes unnamed. Held
//! instead, because the broker is away or the send failed or was lost
//! before its ack, it holds its `unnamed` words in place of its text: a held
//! or retried text never names anyone. The journal says so once each time
//! ([`HELD_UNNAMED`], [`NOT_CONFIRMED`]).
use lamp_msgs::MessageOut;
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};

/// `lamp-skills`' publisher name (`lamp-skills`' `publisher_declared(..,
/// "skills")`): anything else on `messages/out` is not a skill's.
pub const SKILLS: &str = "skills";
/// The most other publishers the journal names, once each.
const REFUSED_NAMED: usize = 8;

/// The most pieces taken in any hour: the backend pushes at most 6 a lamp
/// an hour (#1722), and guard mode sends at most one every 10 minutes.
pub const PER_HOUR: usize = 6;
/// The most pieces kept while the broker is away.
pub const KEPT_OFFLINE: usize = 4;
/// An hour, in ms.
const HOUR_MS: u64 = 3_600_000;
/// The journal's line when a named text is held as its unnamed words.
pub const HELD_UNNAMED: &str = "a named text held as unnamed: the broker was away";
/// The journal's line when a named text goes as its unnamed words because
/// a publish of lamp-cloud's own was still not acknowledged as it was
/// handed to the client: the client may hold it behind that one (a broker's
/// receive maximum) past the eyes' word, so it never waits there named
/// (#2562, the reviewer's correction read on #2769).
pub const UNACKED_AHEAD: &str =
    "a named text sent unnamed: an earlier send was still unacknowledged";
/// The journal's line when a named text goes as its unnamed words because
/// the eyes no longer say, or can't be read to say, each one named said yes.
pub const NOT_CONFIRMED: &str =
    "a named text sent as unnamed: the eyes did not confirm each yes as it went";

/// One piece to send: the skill it came from, for the journal, and the
/// `chat.event`'s `data`, made once when it was taken, so a piece sent
/// again after a reconnect keeps its event's `id` and `time`, which the
/// backend reads as the news' own id.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub skill: String,
    pub data: Value,
    /// A named text's words with no name (`MessageOut::unnamed`): put in
    /// `data` in place of the text when the piece is held (#2562).
    pub unnamed: Option<String>,
    /// Who it names, as person ids (`MessageOut::named`), checked against
    /// the eyes' `tellable` as it goes.
    pub named: Vec<String>,
}

impl Piece {
    /// Held: a named piece's text becomes its unnamed words, its event's
    /// `id` and `time` kept. Whether it named anyone. Also as it is handed
    /// to the client behind a publish of lamp-cloud's not yet acknowledged
    /// ([`UNACKED_AHEAD`]).
    pub fn hold(&mut self) -> bool {
        let Some(unnamed) = self.unnamed.take() else {
            return false;
        };
        self.named.clear();
        crate::chat::retell(&mut self.data, &unnamed);
        true
    }
}

/// What was heard on `messages/out`: the pieces to send now, and the
/// journal's lines about those not sent.
#[derive(Debug, Default)]
pub struct Heard {
    pub due: Vec<Piece>,
    pub lines: Vec<String>,
}

/// What became of a piece heard on `messages/out`.
#[derive(Debug, PartialEq, Eq)]
pub enum Taken {
    Kept,
    /// Not `lamp-skills`' ([`SKILLS`]): the publisher's name the first
    /// time it is refused, for the journal, and `None` after.
    NotSkills(Option<String>),
    /// Heard before: the same publisher and boot, a `seq` not past its
    /// last.
    Again,
    /// Past [`PER_HOUR`].
    OverTheCap,
    /// Not a skill's short line.
    Invalid,
}

/// The pieces waiting to go, and what bounds them.
#[derive(Debug, Default)]
pub struct Outbox {
    waiting: VecDeque<Piece>,
    /// When each piece in the last hour was taken, in ms.
    taken_ms: VecDeque<u64>,
    /// Each publisher's last `seq`, by its name and boot.
    last: HashMap<(String, String), u64>,
    /// Other publishers refused and named in the journal, at most
    /// [`REFUSED_NAMED`].
    refused: HashSet<String>,
    /// Pieces the broker has acknowledged since the start.
    sent: u64,
    /// A named piece was held as unnamed since the journal last said so.
    held_named: bool,
    /// A named piece went unnamed, a yes not confirmed, since last said.
    not_confirmed: bool,
}

impl Outbox {
    /// Takes `out`, from `source`'s `boot` as `seq`, at `now_ms`, its
    /// event made by `event`.
    pub fn take(
        &mut self,
        (source, boot, seq): (&str, &str, u64),
        out: &MessageOut,
        now_ms: u64,
        event: impl FnOnce(&str) -> Value,
    ) -> Taken {
        if source != SKILLS {
            let first =
                self.refused.len() < REFUSED_NAMED && self.refused.insert(source.to_owned());
            return Taken::NotSkills(first.then(|| name_of(source)));
        }
        if out.check().is_err() {
            return Taken::Invalid;
        }
        let key = (source.to_owned(), boot.to_owned());
        if self.last.get(&key).is_some_and(|last| seq <= *last) {
            return Taken::Again;
        }
        self.last.insert(key, seq);
        while self
            .taken_ms
            .front()
            .is_some_and(|at| now_ms.saturating_sub(*at) >= HOUR_MS)
        {
            self.taken_ms.pop_front();
        }
        if self.taken_ms.len() >= PER_HOUR {
            return Taken::OverTheCap;
        }
        self.taken_ms.push_back(now_ms);
        self.waiting.push_back(Piece {
            skill: skill_of(&out.from),
            data: event(&out.text),
            unnamed: out.unnamed.clone(),
            named: out.named.clone(),
        });
        self.trim();
        Taken::Kept
    }

    /// The pieces to send now, oldest first, none named (#2562): with
    /// nobody to confirm a yes, a named piece goes as its unnamed words.
    pub fn due(&mut self) -> Vec<Piece> {
        self.due_checked(|_| false)
    }

    /// The pieces to send now, oldest first: a named one as it is only if
    /// `still` says each one it names still said yes, asked now, as it goes
    /// (`lamp_msgs::known::still_tellable`); else as its unnamed words.
    pub fn due_checked(&mut self, still: impl Fn(&[String]) -> bool) -> Vec<Piece> {
        let mut due: Vec<Piece> = self.waiting.drain(..).collect();
        for piece in &mut due {
            if piece.unnamed.is_some() && !still(&piece.named) {
                self.not_confirmed |= piece.hold();
            }
        }
        due
    }

    /// Pieces not sent, or lost before the broker's ack, waiting again
    /// before any newer, in their order: held, so with no name (#2562).
    pub fn owe(&mut self, pieces: Vec<Piece>) {
        for mut piece in pieces.into_iter().rev() {
            self.held_named |= piece.hold();
            self.waiting.push_front(piece);
        }
        self.trim();
    }

    /// The broker is away: every piece waiting is held, a named one as its
    /// unnamed words (#2562).
    pub fn hold(&mut self) {
        for piece in &mut self.waiting {
            self.held_named |= piece.hold();
        }
    }

    /// [`HELD_UNNAMED`] and [`NOT_CONFIRMED`], once each, if a named piece
    /// was held, or went unnamed, since last asked.
    pub fn held_lines(&mut self) -> Vec<&'static str> {
        let held = std::mem::take(&mut self.held_named).then_some(HELD_UNNAMED);
        let unconfirmed = std::mem::take(&mut self.not_confirmed).then_some(NOT_CONFIRMED);
        held.into_iter().chain(unconfirmed).collect()
    }

    /// A piece the broker acknowledged: how many have been since the start.
    pub fn sent(&mut self) -> u64 {
        self.sent += 1;
        self.sent
    }

    /// The pieces waiting.
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    fn trim(&mut self) {
        while self.waiting.len() > KEPT_OFFLINE {
            self.waiting.pop_front();
        }
    }
}

/// The skill a piece's `from` names, for the journal: `skill:` taken off,
/// and only a name's letters, digits, `-` and `_`, at most 64.
pub fn skill_of(from: &str) -> String {
    name_of(from.strip_prefix("skill:").unwrap_or(from))
}

/// Only a name's letters, digits, `-` and `_`, at most 64.
fn name_of(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(64)
        .collect()
}

/// The journal's line for a piece sent: the skill and the count, never its
/// text.
pub fn sent_line(skill: &str, count: u64) -> String {
    format!("a text from the skill {skill} went to the phone ({count} since the start)")
}

/// The journal's line for a piece not sent, and why: never its text.
pub fn dropped_line(skill: &str, taken: &Taken) -> Option<String> {
    let why = match taken {
        Taken::Kept | Taken::Again | Taken::NotSkills(None) => return None,
        Taken::NotSkills(Some(source)) => {
            return Some(format!(
                "a text on messages/out from the publisher {source} wasn't sent to the phone: \
                 only lamp-skills' are, and later ones from it aren't journalled"
            ));
        }
        Taken::OverTheCap => format!("more than {PER_HOUR} in an hour"),
        Taken::Invalid => "not a short line".to_owned(),
    };
    Some(format!(
        "a text from the skill {skill} wasn't sent to the phone: {why}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn guard(text: &str) -> MessageOut {
        MessageOut {
            from: "skill:guard-mode".into(),
            text: text.into(),
            unnamed: None,
            named: Vec::new(),
        }
    }

    fn event(text: &str) -> Value {
        json!({"event": {"type": "task_news", "id": "evt-1", "summary": text, "detail": {"message": text}}})
    }

    const NAMED: &str = "Minh came in at 18:10 and was here for about 2 minutes.";
    const UNNAMED: &str =
        "Someone you've introduced me to came in at 18:10 and was here for about 2 minutes.";

    fn named() -> MessageOut {
        MessageOut {
            unnamed: Some(UNNAMED.into()),
            named: vec![lamp_msgs::known::person_id("Minh")],
            ..guard(NAMED)
        }
    }

    fn message(piece: &Piece) -> (&Value, &Value) {
        (
            &piece.data["event"]["summary"],
            &piece.data["event"]["detail"]["message"],
        )
    }

    /// #2562: a named text taken with the broker there goes at once, named,
    /// only while each yes is confirmed as it goes; unconfirmed, or held
    /// while the broker is away, or owed after a failed or lost send, it is
    /// its unnamed words, its event's id kept, and the journal says so once.
    #[test]
    fn a_named_text_goes_named_only_at_once_and_is_held_unnamed() {
        let mut outbox = Outbox::default();
        assert_eq!(
            outbox.take(("skills", "b1", 1), &named(), 0, event),
            Taken::Kept
        );
        let due = outbox.due_checked(|ids| ids == [lamp_msgs::known::person_id("Minh")]);
        assert_eq!(message(&due[0]), (&json!(NAMED), &json!(NAMED)));
        assert!(outbox.held_lines().is_empty());
        // Lost before its ack: again, with no name.
        outbox.owe(due);
        assert_eq!(outbox.held_lines(), [HELD_UNNAMED]);
        assert!(outbox.held_lines().is_empty(), "once");
        let again = outbox.due_checked(|_| true);
        assert_eq!(message(&again[0]), (&json!(UNNAMED), &json!(UNNAMED)));
        assert_eq!(again[0].data["event"]["id"], "evt-1");
        assert_eq!((&again[0].unnamed, again[0].named.len()), (&None, 0));
        // Taken while the broker is away: held unnamed, sent so later.
        assert_eq!(
            outbox.take(("skills", "b1", 2), &named(), 1, event),
            Taken::Kept
        );
        assert_eq!(
            outbox.take(("skills", "b1", 3), &guard("Someone was here."), 2, event),
            Taken::Kept
        );
        outbox.hold();
        assert_eq!(outbox.held_lines(), [HELD_UNNAMED]);
        let later = outbox.due_checked(|_| true);
        assert_eq!(message(&later[0]), (&json!(UNNAMED), &json!(UNNAMED)));
        assert_eq!(
            later[1].data["event"]["detail"]["message"],
            "Someone was here."
        );
        // A yes no longer confirmed as it goes: unnamed, said once; and
        // `due` alone never names.
        for checked in [true, false] {
            outbox.take(
                ("skills", "b1", 4 + u64::from(!checked)),
                &named(),
                3,
                event,
            );
            let due = if checked {
                outbox.due_checked(|_| false)
            } else {
                outbox.due()
            };
            assert_eq!(message(&due[0]), (&json!(UNNAMED), &json!(UNNAMED)));
            assert_eq!(outbox.held_lines(), [NOT_CONFIRMED]);
        }
        // Nothing named held: no line.
        outbox.take(("skills", "b1", 9), &guard("Someone was here."), 3, event);
        outbox.hold();
        assert!(outbox.held_lines().is_empty());
    }

    fn take(outbox: &mut Outbox, seq: u64, text: &str, now_ms: u64) -> Taken {
        outbox.take(("skills", "b1", seq), &guard(text), now_ms, event)
    }

    #[test]
    fn pieces_keep_their_order_and_one_heard_again_is_dropped() {
        let mut outbox = Outbox::default();
        assert_eq!(take(&mut outbox, 1, "one", 0), Taken::Kept);
        assert_eq!(take(&mut outbox, 2, "two", 1), Taken::Kept);
        assert_eq!(take(&mut outbox, 2, "two", 2), Taken::Again);
        assert_eq!(take(&mut outbox, 1, "one", 3), Taken::Again);
        // A publisher begun again counts from 1 again.
        assert_eq!(
            outbox.take(("skills", "b2", 1), &guard("three"), 4, event),
            Taken::Kept
        );
        let due = outbox.due();
        let texts: Vec<_> = due
            .iter()
            .map(|piece| piece.data["event"]["detail"]["message"].clone())
            .collect();
        assert_eq!(texts, ["one", "two", "three"]);
        assert!(due.iter().all(|piece| piece.skill == "guard-mode"));
        assert!(outbox.due().is_empty());
        assert_eq!(
            take(&mut outbox, 9, "", 5),
            Taken::Invalid,
            "not a short line"
        );
    }

    #[test]
    fn only_lamp_skills_lines_are_taken_and_another_publisher_is_named_once() {
        let mut outbox = Outbox::default();
        let shell = |outbox: &mut Outbox, seq| {
            outbox.take(("lamp_bus_cli", "b9", seq), &guard("hello"), 0, event)
        };
        let first = shell(&mut outbox, 1);
        assert_eq!(first, Taken::NotSkills(Some("lamp_bus_cli".into())));
        assert_eq!(shell(&mut outbox, 2), Taken::NotSkills(None));
        let line = dropped_line("guard-mode", &first).unwrap();
        assert!(
            line.contains("lamp_bus_cli") && !line.contains("hello"),
            "{line}"
        );
        assert_eq!(dropped_line("guard-mode", &Taken::NotSkills(None)), None);
        // Only so many publishers are named.
        for n in 0..20 {
            let name = format!("tool_{n}");
            let taken = outbox.take((&name, "b1", 1), &guard("hello"), 0, event);
            assert_eq!(
                matches!(taken, Taken::NotSkills(Some(_))),
                n < REFUSED_NAMED - 1,
                "{name}"
            );
        }
        assert_eq!(outbox.waiting(), 0);
        assert_eq!(take(&mut outbox, 1, "one", 0), Taken::Kept);
    }

    #[test]
    fn at_most_six_are_taken_in_any_hour() {
        let mut outbox = Outbox::default();
        let minute = 60_000;
        for n in 0..PER_HOUR as u64 {
            assert_eq!(take(&mut outbox, n + 1, "visit", n * minute), Taken::Kept);
        }
        assert_eq!(
            take(&mut outbox, 7, "visit", 10 * minute),
            Taken::OverTheCap
        );
        assert_eq!(
            take(&mut outbox, 8, "visit", HOUR_MS - 1),
            Taken::OverTheCap
        );
        // An hour after the first, one more.
        assert_eq!(take(&mut outbox, 9, "visit", HOUR_MS), Taken::Kept);
        assert_eq!(
            take(&mut outbox, 10, "visit", HOUR_MS + 1),
            Taken::OverTheCap
        );
    }

    #[test]
    fn while_offline_the_newest_few_wait_in_order_and_owed_go_first() {
        let mut outbox = Outbox::default();
        for n in 1..=5 {
            take(&mut outbox, n, &format!("visit {n}"), n * 1_000);
        }
        assert_eq!(outbox.waiting(), KEPT_OFFLINE);
        let due = outbox.due();
        assert_eq!(due[0].data["event"]["detail"]["message"], "visit 2");
        // Lost before the ack: back before a newer one, as they were.
        take(&mut outbox, 6, "visit 6", 6_000);
        outbox.owe(due.clone());
        let again = outbox.due();
        assert_eq!(&again[..KEPT_OFFLINE - 1], &due[1..]);
        assert_eq!(
            again.last().unwrap().data["event"]["detail"]["message"],
            "visit 6"
        );
    }

    #[test]
    fn the_journals_lines_name_the_skill_and_never_the_text() {
        assert_eq!(
            sent_line("guard-mode", 3),
            "a text from the skill guard-mode went to the phone (3 since the start)"
        );
        let over = dropped_line("guard-mode", &Taken::OverTheCap).unwrap();
        assert!(over.contains("guard-mode") && over.contains("6 in an hour"));
        assert_eq!(dropped_line("guard-mode", &Taken::Kept), None);
        assert_eq!(dropped_line("guard-mode", &Taken::Again), None);
    }
}
