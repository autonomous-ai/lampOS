//! The lamp's own written notes for a look and an ask again, and whether
//! someone is at the lamp.
//!
//! The lamp's hello to someone who comes back to the desk is the Welcome
//! back skill's alone (`lampOS/skills/welcome-back`, said through the say
//! service): the voice's own hello, built here once, is gone (#2825), so
//! one arrival can never get two. What stays here is [`Presence`]: the eyes
//! (`lamp-eyes --presence-file`) keep a small file fresh while a face is
//! near, and from it the voice knows someone is at the lamp, for a reminder
//! or news that waits for a person, and how familiar Mood says that person
//! is (#1670), told with a skill's note ([`crate::person::at_the_lamp`]).
use lamp_bus::{Bus, Message};
use lamp_msgs::{FamiliarLevel, MOOD_STATE_TTL_US, MoodState};
use std::{
    path::PathBuf,
    time::{Duration, SystemTime},
};

/// Where the coordinator reads the path of the eyes' presence file. Unset:
/// nobody is ever known to be at the lamp from the eyes.
pub const ENVIRONMENT: &str = "LAMP_EYES_PRESENCE_FILE";

/// What the model is given with the picture when the lamp takes a look,
/// and without one when its eyes are shut.
pub const LOOK_NOTE: &str = "(You are looking now, and this is what your eye sees. Answer what they just asked you in a sentence or two, the way you would tell a friend what you see. It is not a picture or a camera to you: it is what you see.)";
/// The same with a full look (`LAMP_LOOK_FULL=on`, #836): what the lamp
/// cannot make out it says, rather than guess.
pub const LOOK_FULL_NOTE: &str = "(You are looking now, and this is what your eye sees. Answer what they just asked you in a sentence or two, the way you would tell a friend what you see. It is not a picture or a camera to you: it is what you see. If you cannot make out what they are showing you, say so plainly and ask them to hold it a bit closer to you; never guess what it might be.)";
/// The written turn that asks again when what was heard in a conversation
/// made no sense in it (#926): another script, or a number alone. On
/// lamp-4ace (09:42) the model took a softer note as its cue to go on with
/// the story it had been cut off in, so this one says that is over.
pub const ASK_AGAIN_NOTE: &str = "(The person just cut in and you could not make out what they said: it was misheard. What you were saying before is over: do not go on with it, do not finish it, do not sum it up, and do not answer what was misheard. Say only one word, in the language of the conversation, as a question: \"Sorry?\" Then stop and listen.)";
pub const NO_LOOK_NOTE: &str = "(You tried to look, but your eye is shut just now and you saw nothing: you could not see it. Tell them plainly, in a few light words, that you could not see it; never guess what it is or answer as if you had seen it.)";

/// Whether an answer that opens like this is the lamp saying it will look:
/// its own words are how it asks for its eyes. Only "let me look" and its
/// two longer forms; "let me see" is something people say before any answer.
/// The phrase must close where it ends, with `.`, `!`, `…`, `,`, `;`, `:` or
/// a dash, as the character says it ("say exactly \"Let me look.\""): a word
/// after it is something else. On lamp-4ace on 19 Oct the second route's
/// "Let me look that up" before a web search took two pictures, and the
/// model described the desk unasked (#1712). Until the closing mark comes,
/// more words may follow, so it is not yet a look.
pub fn asks_to_look(opening: &str) -> bool {
    // Each word, lowercased, with where it ends in `opening`.
    let mut words: Vec<(String, usize)> = Vec::new();
    let mut began = None;
    for (at, c) in opening.char_indices() {
        if c.is_alphabetic() {
            began.get_or_insert(at);
        } else if let Some(from) = began.take() {
            words.push((opening[from..at].to_lowercase(), at));
        }
    }
    if let Some(from) = began {
        words.push((opening[from..].to_lowercase(), opening.len()));
    }
    let closes = |rest: &str| {
        rest.trim_start()
            .chars()
            .next()
            .is_some_and(|c| matches!(c, '.' | '!' | '…' | ',' | ';' | ':' | '—' | '–' | '-'))
    };
    [
        &["let", "me", "look"][..],
        &["let", "me", "take", "a", "look"][..],
        &["let", "me", "have", "a", "look"][..],
    ]
    .iter()
    .any(|phrase| {
        words.len() >= phrase.len()
            && words
                .iter()
                .zip(phrase.iter())
                .all(|((word, _), said)| word == said)
            && closes(&opening[words[phrase.len() - 1].1..])
    })
}

const THERE_FOR_US: u64 = 3_000_000;
/// The file is read this often, and counts while it is this fresh.
const READ_EVERY_US: u64 = 250_000;
const FRESH: Duration = Duration::from_millis(1_500);
/// A face smaller than this share of the picture's height is across the
/// room. People at the lamp were 0.12 to 0.27 on lamp-4ace.
const NEAR: f32 = 0.14;

/// Whether someone is at the lamp, from the eyes' presence file.
pub struct Presence {
    path: Option<PathBuf>,
    read_us: u64,
    near: bool,
    /// Since when someone has been there without a break.
    there_since: Option<u64>,
    /// The bus Mood's state is read from, and how familiar the remembered
    /// person at the lamp is by it (#1670): none without a fresh level.
    mood: Option<Bus>,
    familiar: Option<FamiliarLevel>,
}

/// How familiar Mood says the remembered person at the lamp is (#1670): only
/// from source `mood`, unexpired, checked, stamped no later than now on the
/// voice's monotonic clock and within its 2.5 s; else `None`.
pub fn familiar_in(message: Option<Message>, now_us: u64) -> Option<FamiliarLevel> {
    let message = message.filter(|message| !message.expired && message.header.source == "mood")?;
    let state = message.parse::<MoodState>().ok()?;
    state.check().ok()?;
    let since = now_us.checked_sub(state.stamp.at_us)?;
    (since < MOOD_STATE_TTL_US).then_some(state.familiar)?
}

impl Presence {
    pub fn from_environment() -> Option<Self> {
        let path = std::env::var_os(ENVIRONMENT).filter(|path| !path.is_empty())?;
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return None;
        }
        Some(Self {
            // How familiar the person at the lamp is, where Mood says it
            // (#1670); without a bus, nothing changes.
            mood: Bus::from_environment().ok(),
            ..Self::new(Some(path))
        })
    }

    fn new(path: Option<PathBuf>) -> Self {
        Self {
            path,
            read_us: 0,
            near: false,
            there_since: None,
            mood: None,
            familiar: None,
        }
    }

    /// How familiar the remembered person at the lamp is, as Mood last said
    /// (#1670), for a skill's note while someone is here.
    pub fn familiar(&self) -> Option<FamiliarLevel> {
        self.familiar
    }

    /// Whether the eyes have a near face now, from their file.
    fn near_now(&mut self, now_us: u64) -> bool {
        if now_us.saturating_sub(self.read_us) < READ_EVERY_US {
            return self.near;
        }
        self.read_us = now_us;
        if let Some(bus) = self.mood.as_ref() {
            self.familiar = familiar_in(bus.latest(MoodState::TOPIC).ok().flatten(), now_us);
        }
        self.near = self.path.as_ref().is_some_and(|path| {
            let fresh = std::fs::metadata(path)
                .and_then(|about| about.modified())
                .ok()
                .and_then(|at| SystemTime::now().duration_since(at).ok())
                .is_some_and(|age| age < FRESH);
            fresh
                && std::fs::read_to_string(path)
                    .ok()
                    .and_then(|text| text.trim().parse::<f32>().ok())
                    .is_some_and(|size| size.is_finite() && size >= NEAR)
        });
        self.near
    }

    /// Call every tick: takes in whether someone is there.
    pub fn watch(&mut self, now_us: u64) {
        let near = self.near_now(now_us);
        self.note(near, now_us);
    }

    fn note(&mut self, near: bool, now_us: u64) {
        if near {
            self.there_since.get_or_insert(now_us);
        } else {
            self.there_since = None;
        }
    }

    /// Whether someone has been at the lamp for a moment now.
    pub fn present(&self, now_us: u64) -> bool {
        self.there_since
            .is_some_and(|since| now_us.saturating_sub(since) >= THERE_FOR_US)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000;

    /// #2825: the hello on an arrival is Welcome back's alone. Fails if the
    /// voice grows a hello of its own again, which could speak to the same
    /// arrival as the skill, or if the skill stops greeting.
    #[test]
    fn only_welcome_back_says_hello_on_an_arrival() {
        let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        // Built from pieces, so this test's own text is not what it finds.
        let own_hello = [
            ["lamp_speaks", "_first"].concat(),
            ["has just come ", "and sat down"].concat(),
            ["LAMP_SPEAK", "_FIRST"].concat(),
            ["LAMP_HELLO", "_ASKS_POLICY"].concat(),
        ];
        let mut folders = vec![crate_dir.join("src")];
        let mut read = 0;
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(&folder).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    folders.push(path);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    read += 1;
                    for needle in &own_hello {
                        assert!(
                            !text.contains(needle.as_str()),
                            "{} has a hello of the voice's own ({needle})",
                            path.display()
                        );
                    }
                }
            }
        }
        assert!(read > 100, "the voice's sources were read: {read}");
        // And the skill greets: a face appearing after five minutes away,
        // the Policy asked, said through the voice by a note.
        let skill = std::fs::read_to_string(crate_dir.join("../../skills/welcome-back/skill.toml"))
            .unwrap();
        for line in [
            "topic = \"humans/faces/tracked\"",
            "when = \"appears\"",
            "absent_for = \"5m\"",
            "cooldown = \"20m\"",
            "who = true",
            "policy = \"hello\"",
            "say = ",
        ] {
            assert!(skill.contains(line), "welcome-back lacks {line}");
        }
    }

    #[test]
    fn someone_is_present_after_three_seconds_and_gone_at_once() {
        let mut presence = Presence::new(None);
        presence.note(true, S);
        assert!(!presence.present(3 * S));
        assert!(presence.present(4 * S));
        presence.note(true, 4 * S);
        assert!(presence.present(4 * S), "still the same sitting");
        presence.note(false, 5 * S);
        assert!(!presence.present(5 * S));
    }

    /// #1670: Mood's familiar level is read from source `mood` only, fresh,
    /// valid and within its 2.5 s.
    #[test]
    fn the_familiar_level_is_read_from_moods_fresh_state_only() {
        let (bus, root) = bus("familiar");
        let latest = || bus.latest(MoodState::TOPIC).unwrap();
        assert_eq!(familiar_in(latest(), 5_000_000), None);
        let state = |familiar| MoodState {
            stamp: lamp_msgs::ClockStamp {
                clock_id: "boot".into(),
                at_us: 5_000_000,
                sequence: 1,
            },
            session_id: "s".into(),
            awake: 0.7,
            energy: 0.3,
            warmth: 0.6,
            asleep: false,
            attention: None,
            familiarity: lamp_msgs::Familiarity::Unknown,
            familiar,
            felt: None,
            breath: None,
        };
        let mut mood = bus
            .publisher_declared(MoodState::TOPIC, &MoodState::declaration(), "mood")
            .unwrap();
        mood.publish_json(1, &state(Some(FamiliarLevel::Close)))
            .unwrap();
        assert_eq!(familiar_in(latest(), 5_000_000), Some(FamiliarLevel::Close));
        assert_eq!(familiar_in(latest(), 4_999_999), None);
        assert_eq!(familiar_in(latest(), 5_000_000 + MOOD_STATE_TTL_US), None);
        bus.publisher_declared(MoodState::TOPIC, &MoodState::declaration(), "replay")
            .unwrap()
            .publish_json(2, &state(Some(FamiliarLevel::Close)))
            .unwrap();
        assert_eq!(familiar_in(latest(), 5_000_000), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn bus(name: &str) -> (Bus, PathBuf) {
        let root = PathBuf::from(format!("/tmp/lvh-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        (
            Bus::new(&root)
                .unwrap()
                .rescan_every(Duration::from_millis(20)),
            root,
        )
    }

    #[test]
    fn only_saying_it_will_look_asks_for_a_look() {
        for (opening, asks) in [
            ("Let me look.", true),
            ("Let me look... okay.", true),
            ("let me take a look.", true),
            ("Let me have a look!", true),
            ("Let me look, one moment.", true),
            ("Let me look — okay.", true),
            // Not closed yet: more words may follow.
            ("Let me look", false),
            ("let me take a look", false),
            // #1712: a word after it is something else, never a look.
            ("Let me look that up.", false),
            ("Let me look that up for you.", false),
            ("Let me look into it.", false),
            ("Let me look it up.", false),
            ("Let me look up the weather.", false),
            ("Let me look for that.", false),
            ("Let me take a look at the forecast.", false),
            ("Let me see... that's four!", false),
            ("Let me think.", false),
            ("Let", false),
            ("Let me", false),
            ("I'll let me look.", false),
            ("Look, it's simple.", false),
            ("", false),
        ] {
            assert_eq!(asks_to_look(opening), asks, "{opening:?}");
        }
    }

    #[test]
    fn the_eyes_file_counts_only_fresh_and_only_for_a_near_face() {
        let directory = std::env::temp_dir().join(format!(
            "lamp-voice-arrival-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("present");
        let mut arrival = Presence::new(Some(path.clone()));
        assert!(!arrival.near_now(S));
        std::fs::write(&path, "0.21\n").unwrap();
        assert!(!arrival.near_now(S + 1), "read at most four times a second");
        assert!(arrival.near_now(2 * S));
        std::fs::write(&path, "0.09\n").unwrap();
        assert!(!arrival.near_now(3 * S), "a face across the room");
        std::fs::write(&path, "not a number").unwrap();
        assert!(!arrival.near_now(4 * S));
        std::fs::write(&path, "0.30").unwrap();
        let old = SystemTime::now() - Duration::from_secs(5);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert!(!arrival.near_now(5 * S), "the eyes stopped writing");
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
