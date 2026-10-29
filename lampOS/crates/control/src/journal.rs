//! The door's journal (docs/control.md, "The journal"): one line for every
//! command and its answer, every lease the lamp takes back, and every event
//! sent, each naming the computer by its id and its paired name.
//!
//! A command's line says which op it was, what it asked in its harmless
//! fields ([`fields`]: a light's colour, level, pattern and time, a
//! gesture's name, the events subscribed to, …), how it ended and where it
//! went. A name a computer chose is written only when it is a plain short
//! word ([`word`]), so no line carries text a computer made up. What a line
//! never holds: a `say`'s words (only how many characters and the first 8
//! hex of their SHA-256), the phrases a computer listens for or heard, a
//! pairing's code, key or proof, a hello's signature, a lease's `why`, or a
//! pushed skill's bytes.
//!
//! At most [`MOST_A_SECOND`] lines a second for each computer; past that,
//! one line says how many more there were, as its next second begins: at
//! its next line, or the door's next tick ([`Journal::tick`]), whichever
//! comes first.

use std::collections::BTreeMap;

use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};

use crate::protocol::{LookAt, Op};

/// A computer's lines a second at most.
pub const MOST_A_SECOND: u32 = 10;
/// Who a line is about before a computer said hello.
pub const NOBODY: &str = "-";

/// One computer's second.
#[derive(Debug, Default)]
struct Second {
    began_us: u64,
    written: u32,
    left_out: u32,
    /// The last command's line, less its ms: the same again within the
    /// second is counted, not written ([`Journal::command`]).
    last: Option<String>,
    again: u32,
}

impl Second {
    /// "…, again N times" for the command repeated since its line, if it
    /// was; the run is over.
    fn repeated(&mut self) -> Option<String> {
        let again = std::mem::take(&mut self.again);
        let last = self.last.take()?;
        match again {
            0 => None,
            1 => Some(format!("{last}, again once")),
            again => Some(format!("{last}, again {again} times")),
        }
    }
}

/// Where the door's lines go: the journal, through stderr, or a test's
/// list.
pub struct Journal {
    seconds: BTreeMap<String, Second>,
    write: Box<dyn FnMut(&str) + Send>,
    /// Made at start and never written: a say's fingerprint is an HMAC
    /// under it, so two lines can be told apart while a guessed sentence
    /// can't be checked against the journal (harness-reviewer's P3 on the
    /// first head).
    key: hmac::Key,
}

impl Default for Journal {
    fn default() -> Self {
        Self::to(|line| eprintln!("{line}"))
    }
}

impl std::fmt::Debug for Journal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Journal").finish_non_exhaustive()
    }
}

impl Journal {
    /// A journal that hands each line to `write`.
    pub fn to(write: impl FnMut(&str) + Send + 'static) -> Self {
        let mut key = [0u8; 32];
        SystemRandom::new()
            .fill(&mut key)
            .expect("the system's random source");
        Self {
            seconds: BTreeMap::new(),
            write: Box::new(write),
            key: hmac::Key::new(hmac::HMAC_SHA256, &key),
        }
    }

    /// `text` about `client` (its id, or [`NOBODY`]), within its second's
    /// bound. A command repeated just before it is said first.
    pub fn line(&mut self, client: &str, now_us: u64, text: &str) {
        self.roll(client, now_us);
        if let Some(repeated) = self.second(client).repeated() {
            self.put(client, &repeated);
        }
        self.put(client, text);
    }

    /// A command's line, `head` without its time and `ms` the time it took.
    /// The same command with the same answer as the one just before, within
    /// the computer's second, is counted, not written: one line says how
    /// many when the run ends, at another line, the second's end or the
    /// door's tick (Fourth's P3 on #4245: a dashboard polling `get` wrote
    /// the whole 10 lines a second). Every command is still told.
    pub fn command(&mut self, client: &str, now_us: u64, head: &str, ms: f64) {
        self.roll(client, now_us);
        let second = self.second(client);
        if second.last.as_deref() == Some(head) {
            second.again += 1;
            return;
        }
        if let Some(repeated) = second.repeated() {
            self.put(client, &repeated);
        }
        self.put(client, &format!("{head}, {ms:.1} ms"));
        self.second(client).last = Some(head.to_owned());
    }

    fn second(&mut self, client: &str) -> &mut Second {
        self.seconds.entry(client.to_owned()).or_default()
    }

    /// Ends `client`'s second if it is over: its repeats and the lines left
    /// out are said, and the next second begins now.
    fn roll(&mut self, client: &str, now_us: u64) {
        let second = self.second(client);
        if now_us.saturating_sub(second.began_us) < 1_000_000 {
            return;
        }
        let repeated = second.repeated();
        let left_out = second.left_out;
        *second = Second {
            began_us: now_us,
            ..Second::default()
        };
        if let Some(repeated) = repeated {
            (self.write)(&format!("lamp-control: {repeated}"));
        }
        if left_out > 0 {
            (self.write)(&format!(
                "lamp-control: {client}: {left_out} more not logged"
            ));
        }
    }

    /// One line, within the second's bound.
    fn put(&mut self, client: &str, text: &str) {
        let second = self.second(client);
        if second.written >= MOST_A_SECOND {
            second.left_out += 1;
            return;
        }
        second.written += 1;
        (self.write)(&format!("lamp-control: {text}"));
    }

    /// The door's tick: each computer whose second is over with lines left
    /// out has them said now, so a burst followed by silence still says so.
    pub fn tick(&mut self, now_us: u64) {
        for (client, second) in &mut self.seconds {
            let over = now_us.saturating_sub(second.began_us) >= 1_000_000;
            if !over || (second.left_out == 0 && second.again == 0) {
                continue;
            }
            if let Some(repeated) = second.repeated() {
                (self.write)(&format!("lamp-control: {repeated}"));
            }
            if second.left_out > 0 {
                (self.write)(&format!(
                    "lamp-control: {client}: {} more not logged",
                    second.left_out
                ));
            }
            *second = Second::default();
        }
    }

    /// A computer gone: its second is forgotten.
    pub fn forget(&mut self, client: &str) {
        self.seconds.remove(client);
    }
}

/// "c_1 (lab-mac)": the id, and its paired name when that is a plain word
/// ([`word`]); a name such as "the boss's laptop" may name a person, so it is
/// written as "a name the door doesn't write" (harness-reviewer's P3 on the
/// first head). The id alone names the computer.
pub fn who(client: &str, name: Option<&str>) -> String {
    match name {
        Some(name) if !name.is_empty() => format!("{client} ({})", word(name)),
        _ => client.to_owned(),
    }
}

/// An op's name on the wire.
pub fn op_name(op: &Op) -> &'static str {
    match op {
        Op::Pair { .. } => "pair",
        Op::Hello { .. } => "hello",
        Op::Subscribe { .. } => "subscribe",
        Op::Unsubscribe { .. } => "unsubscribe",
        Op::Get { .. } => "get",
        Op::LeaseTake { .. } => "lease.take",
        Op::LeaseRenew { .. } => "lease.renew",
        Op::LeaseRelease { .. } => "lease.release",
        Op::Express { .. } => "express",
        Op::Gesture { .. } => "gesture",
        Op::Look { .. } => "look",
        Op::Move { .. } => "move",
        Op::Play { .. } => "play",
        Op::Joints { .. } => "joints",
        Op::Light { .. } => "light",
        Op::Say { .. } => "say",
        Op::Sound { .. } => "sound",
        Op::Stop {} => "stop",
        Op::SkillPush { .. } => "skill.push",
        Op::SkillList {} => "skill.list",
        Op::SkillRemove { .. } => "skill.remove",
    }
}

/// Where an op the lamp took went (crates/control/src/parts.rs).
pub fn went(op: &Op) -> &'static str {
    match op {
        // `look` goes to the body's lease socket once ten/look routes it;
        // until then it's refused, and a refusal names no place (the P3 from
        // a0ae's tour build, where it read "to the door").
        Op::Look { .. } => "the body, by lease.sock",
        Op::Light { .. } => "the ring, by light/request",
        Op::Gesture { .. } | Op::Move { .. } => "the body, by mood.sock",
        Op::Express { .. } => "the body, by mood.sock, and the ring, by light/request",
        Op::Say { .. } => "the voice, by say.sock",
        Op::SkillPush { .. } | Op::SkillList {} | Op::SkillRemove { .. } => "the skill helper",
        _ => "the door",
    }
}

impl Journal {
    /// What a line says of an op's own fields, so two requests that went
    /// to the same place can be told apart: the harmless ones only. A `say`
    /// is its length and a short HMAC under this journal's own key, never
    /// its words; a subscribe its events, never its phrases; a lease its
    /// parts and time, never its `why`; pairing, hello and a push say
    /// nothing of theirs.
    pub fn fields(&self, op: &Op) -> String {
        fields(op, |text| self.said(text))
    }

    /// "12 characters, tag 5f2b1c0a": the same words give the same tag
    /// while this door runs, and nobody can check a guess against it.
    pub fn said(&self, text: &str) -> String {
        let tag = hmac::sign(&self.key, text.as_bytes());
        let short: String = tag
            .as_ref()
            .iter()
            .take(4)
            .map(|byte| format!("{byte:02x}"))
            .collect();
        format!("{} characters, tag {short}", text.chars().count())
    }
}

fn fields(op: &Op, said: impl Fn(&str) -> String) -> String {
    let inside = match op {
        Op::Say { text } => said(text),
        Op::Gesture { name } | Op::Play { name } | Op::Sound { name } => word(name),
        Op::Move { step_ms, frames } => format!("{} frames, step_ms {step_ms}", frames.len()),
        Op::Express { feeling, strength } => format!("{}, strength {strength:.2}", word(feeling)),
        Op::Light {
            colour,
            level,
            pattern,
            for_ms,
        } => format!(
            "{}, level {level}, {}, for_ms {for_ms}",
            word(colour),
            pattern.as_ref().map_or("steady".to_owned(), named)
        ),
        Op::Look { at } => match at {
            LookAt::Target(target) => named(target),
            LookAt::Angles(angles) => format!("yaw {:.1}, pitch {:.1}", angles.yaw, angles.pitch),
        },
        Op::Get { state } => list(state.iter().map(named)),
        Op::Subscribe { events, .. } | Op::Unsubscribe { events } => list(events.iter().map(named)),
        Op::LeaseTake { parts, for_ms, .. } => {
            format!("{}, for_ms {for_ms}", list(parts.iter().map(named)))
        }
        Op::LeaseRenew { for_ms, .. } => format!("for_ms {for_ms}"),
        _ => return String::new(),
    };
    format!(" ({inside})")
}

/// A name a computer chose, as a line may hold it: a plain word of letters,
/// digits, `_` and `-`, 32 at most, or "a name the door doesn't write".
pub fn word(name: &str) -> String {
    let plain = (1..=32).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
    if plain {
        name.to_owned()
    } else {
        "a name the door doesn't write".to_owned()
    }
}

fn list(names: impl Iterator<Item = String>) -> String {
    let names: Vec<String> = names.collect();
    if names.is_empty() {
        "none".to_owned()
    } else {
        names.join(" ")
    }
}

/// The wire name of one of the protocol's own enums.
fn named<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "?".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn kept() -> (Journal, Arc<Mutex<Vec<String>>>) {
        let lines = Arc::new(Mutex::new(Vec::new()));
        let into = lines.clone();
        (
            Journal::to(move |line| into.lock().unwrap().push(line.to_owned())),
            lines,
        )
    }

    #[test]
    fn ten_lines_a_second_then_how_many_more() {
        let (mut journal, lines) = kept();
        for n in 0..14 {
            journal.line("c_1", 5_000_000 + n, &format!("line {n}"));
        }
        // Another computer has its own second.
        journal.line("c_2", 5_000_100, "theirs");
        assert_eq!(lines.lock().unwrap().len(), 11);
        journal.line("c_1", 6_000_000, "next second");
        let lines = lines.lock().unwrap();
        assert_eq!(lines[9], "lamp-control: line 9");
        assert_eq!(lines[10], "lamp-control: theirs");
        assert_eq!(lines[11], "lamp-control: c_1: 4 more not logged");
        assert_eq!(lines[12], "lamp-control: next second");
        assert_eq!(lines.len(), 13);
    }

    /// Fourth's P3 on #4245: a computer polling `get` 20 times a second
    /// wrote the whole 10 lines a second. The same command with the same
    /// answer, within the second, is one line and then how many again.
    #[test]
    fn the_same_command_again_within_a_second_is_counted_not_written() {
        let (mut journal, lines) = kept();
        let get = "c_1 (lab-mac): get (presence): ok, to the door";
        for n in 0..20 {
            journal.command("c_1", 5_000_000 + n * 50_000, get, 0.1);
        }
        assert_eq!(lines.lock().unwrap().len(), 1, "the first only, so far");
        // The second's end says the run, at the tick or the next line.
        journal.tick(6_000_000);
        {
            let lines = lines.lock().unwrap();
            assert_eq!(lines[0], format!("lamp-control: {get}, 0.1 ms"));
            assert_eq!(lines[1], format!("lamp-control: {get}, again 19 times"));
            assert_eq!(lines.len(), 2);
        }
        // Another command, or another answer, ends a run at once.
        let refused = "c_1 (lab-mac): get (presence): refused: rate";
        journal.command("c_1", 7_000_000, get, 0.1);
        journal.command("c_1", 7_010_000, get, 0.1);
        journal.command("c_1", 7_020_000, refused, 0.0);
        journal.line("c_1", 7_030_000, "c_1 (lab-mac): event touch sent");
        let lines = lines.lock().unwrap();
        assert_eq!(lines[2], format!("lamp-control: {get}, 0.1 ms"));
        assert_eq!(lines[3], format!("lamp-control: {get}, again once"));
        assert_eq!(lines[4], format!("lamp-control: {refused}, 0.0 ms"));
        assert_eq!(lines[5], "lamp-control: c_1 (lab-mac): event touch sent");
        assert_eq!(lines.len(), 6);
    }

    /// Different commands are each written, within the bound as before, and
    /// a run cut by the second's turn is said as the next second begins.
    #[test]
    fn different_commands_are_each_written_and_a_run_is_told_at_the_seconds_turn() {
        let (mut journal, lines) = kept();
        for n in 0..12 {
            journal.command(
                "c_1",
                5_000_000 + n,
                &format!("c_1: gesture (g{n}): ok"),
                0.0,
            );
        }
        assert_eq!(lines.lock().unwrap().len(), 10);
        journal.command("c_1", 5_500_000, "c_1: stop: ok, to the door", 0.0);
        journal.command("c_1", 5_600_000, "c_1: stop: ok, to the door", 0.0);
        journal.command(
            "c_1",
            6_000_000,
            "c_1: light (warm, level 3, steady, for_ms 10000): ok",
            0.2,
        );
        let lines = lines.lock().unwrap();
        // The stop's first line was past the bound; its repeat is told at
        // the turn, then the lines left out.
        assert_eq!(
            lines[10],
            "lamp-control: c_1: stop: ok, to the door, again once"
        );
        assert_eq!(lines[11], "lamp-control: c_1: 3 more not logged");
        assert_eq!(
            lines[12],
            "lamp-control: c_1: light (warm, level 3, steady, for_ms 10000): ok, 0.2 ms"
        );
        assert_eq!(lines.len(), 13);
    }

    #[test]
    fn a_say_is_its_length_and_a_short_tag_never_its_words() {
        let (journal, _) = kept();
        let text = "Your build passed, desk two.";
        let line = journal.said(text);
        assert_eq!(text.chars().count(), 28);
        assert!(line.starts_with("28 characters, tag "), "{line}");
        assert_eq!(line.len(), "28 characters, tag ".len() + 8);
        assert!(!line.contains("build") && !line.contains("desk"));
        assert!(journal.said("héllo").starts_with("5 characters, tag "));
        // The same words, the same tag, so two lines can be told apart.
        assert_eq!(journal.said(text), line);
        assert_ne!(journal.said("Your build failed, desk two."), line);
    }

    /// harness-reviewer's P3 on the first head: a plain SHA-256 let anyone
    /// with the journal check a guessed sentence. The tag is an HMAC under a
    /// key made at start: another door's tag for the same words differs,
    /// and neither is the words' SHA-256.
    #[test]
    fn a_guess_cannot_be_checked_against_a_say_tag() {
        use sha2::{Digest, Sha256};
        let text = "Your build passed, desk two.";
        let (first, _) = kept();
        let (second, _) = kept();
        let plain: String = Sha256::digest(text.as_bytes())
            .iter()
            .take(4)
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let tag = |journal: &Journal| journal.said(text).rsplit(' ').next().unwrap().to_owned();
        assert_ne!(tag(&first), plain);
        assert_ne!(tag(&first), tag(&second));
    }

    /// A burst then silence still says how many were left out, at the
    /// door's first tick past the second, once (the P3 on the first head).
    #[test]
    fn a_burst_then_silence_says_how_many_more_at_the_next_tick() {
        let (mut journal, lines) = kept();
        for n in 0..13 {
            journal.line("c_1", 5_000_000 + n, "a line");
        }
        journal.tick(5_900_000);
        assert_eq!(
            lines.lock().unwrap().len(),
            10,
            "not before its second is over"
        );
        journal.tick(6_000_000);
        journal.tick(6_250_000);
        journal.line("c_1", 6_300_000, "later");
        let lines = lines.lock().unwrap();
        assert_eq!(lines[10], "lamp-control: c_1: 3 more not logged");
        assert_eq!(lines[11], "lamp-control: later");
        assert_eq!(lines.len(), 12, "said once");
    }

    /// The light's and the gesture's own fields, and a name a computer made
    /// up kept out of the line.
    #[test]
    fn a_light_and_a_gesture_say_what_they_asked() {
        let (journal, _) = kept();
        let op = |text: &str| -> Op { serde_json::from_str(text).unwrap() };
        let light =
            op(r#"{"op":"light","colour":"red","level":4,"pattern":"pulse","for_ms":5000}"#);
        assert_eq!(
            journal.fields(&light),
            " (red, level 4, pulse, for_ms 5000)"
        );
        assert_eq!(
            journal.fields(&op(r#"{"op":"gesture","name":"nod"}"#)),
            " (nod)"
        );
        let made_up = op(r#"{"op":"gesture","name":"nod\nlamp-control: forged"}"#);
        assert_eq!(journal.fields(&made_up), " (a name the door doesn't write)");
        let lease =
            op(r#"{"op":"lease.take","parts":["body"],"for_ms":10000,"why":"their own words"}"#);
        assert_eq!(journal.fields(&lease), " (body, for_ms 10000)");
        let look = op(r#"{"op":"look","at":{"yaw":12.5,"pitch":-3}}"#);
        // Once routed, a look goes to the body's lease socket, never "the
        // door" (the P3 from a0ae's tour build, with ten/look).
        assert_eq!(went(&look), "the body, by lease.sock");
        assert_eq!(journal.fields(&look), " (yaw 12.5, pitch -3.0)");
        let subscribe = op(
            r#"{"op":"subscribe","events":["person.arrived","phrase.heard"],"phrases":["good night lamp"]}"#,
        );
        assert_eq!(journal.fields(&subscribe), " (person.arrived phrase.heard)");
        assert_eq!(journal.fields(&op(r#"{"op":"stop"}"#)), "");
    }

    #[test]
    fn a_computer_is_its_id_and_its_paired_name_only_when_plain() {
        assert_eq!(who("c_1", Some("lab-mac")), "c_1 (lab-mac)");
        assert_eq!(
            who("c_1", Some("the boss's laptop")),
            "c_1 (a name the door doesn't write)"
        );
        assert_eq!(who("c_1", Some("")), "c_1");
        assert_eq!(who("c_1", None), "c_1");
    }
}
