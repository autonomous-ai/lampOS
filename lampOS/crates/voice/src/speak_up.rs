//! Speaking first: the lamp says hello to someone who comes and sits down.
//!
//! Everything else the lamp says is an answer. This is the one thing it
//! says unasked, and it is meant to be as small as what a person at the
//! next desk does when a colleague comes back: look up, a word or two, and
//! nothing more. It does not announce itself to a room, greet people
//! walking past, or speak into a conversation.
//!
//! The eyes (`lamp-eyes --presence-file`) keep a small file fresh while a
//! face is near; its text is how large the face is in the picture. From
//! that, [`Arrival`] decides. The lamp speaks when all of these hold:
//!
//! - someone has been there for three seconds without a break;
//! - before that nobody was, for five minutes or more (a face lost for
//!   under twenty seconds is someone who looked away, not someone who left);
//! - nothing has been said to the lamp or by it for half a minute;
//! - it has not greeted anyone in the last twenty minutes.
//!
//! What it then says is the model's: the turn is a written note in place of
//! speech (`ProviderInput::Written`), not words chosen here.
//!
//! With [`ASK_ENVIRONMENT`] `on` (#752) a due hello asks the policy first:
//! it is said on `voice/arrival` and held up to [`ASK_FOR_US`] for the
//! policy's decision about it. `say_now` says it, `wait` keeps it, and a
//! decision in shadow or none in time leaves it to the rule above
//! ([`Hello`]). Without it nothing here touches the bus.
use lamp_bus::{Bus, Publisher, Subscriber};
use lamp_msgs::{Decision, VoiceArrival};
use serde_json::{Value, json};
use std::{
    io,
    path::PathBuf,
    time::{Duration, SystemTime},
};

/// Where the coordinator reads the path of the eyes' presence file. Unset:
/// the lamp never speaks first and nothing here runs.
pub const ENVIRONMENT: &str = "LAMP_EYES_PRESENCE_FILE";
/// `on` for the lamp to say its hello. Anything else, with the file set:
/// the moments it would have are recorded and nothing is said, which is how
/// the rule is watched in a room before it is given a voice.
pub const SPEAK_ENVIRONMENT: &str = "LAMP_SPEAK_FIRST";
/// `on` for a due hello to ask the policy first (`lamp-policy --hello`).
/// Read at start; anything else, and the hello is as it was.
pub const ASK_ENVIRONMENT: &str = "LAMP_HELLO_ASKS_POLICY";
/// How long a due hello is held for the policy's decision.
pub const ASK_FOR_US: u64 = 2_000_000;
/// An answer that could not be acted on for this long (the lamp was busy)
/// is old: the policy is asked again.
const ANSWER_LASTS_US: u64 = 10_000_000;
/// No more decisions than this are read in one tick.
const MOST_READ: usize = 64;

/// What to do with a due hello now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hello {
    /// Nothing yet: not due and ready, or the policy is being asked.
    NotYet,
    /// As the rule says: said with [`SPEAK_ENVIRONMENT`] `on`, else
    /// recorded. Without asking; or the policy only watches (its decision
    /// is in shadow); or it did not answer in [`ASK_FOR_US`].
    ByRule,
    /// The policy, acting, says say it (with [`SPEAK_ENVIRONMENT`] `on`;
    /// that stays the switch for the lamp to speak first at all).
    SayNow,
    /// The policy, acting, says not now: recorded, not said.
    Wait,
}

/// A hello said on `voice/arrival` and not yet acted on.
#[derive(Clone, Debug)]
struct Pending {
    number: u64,
    asked_us: u64,
    /// The answer, once known, and what the record calls it.
    answer: Option<(Hello, &'static str)>,
    answered_us: u64,
}

/// The bus, when a due hello asks the policy.
struct Asking {
    out: Publisher,
    decisions: Subscriber,
    pending: Option<Pending>,
    /// What to write in the session record about the last answer acted on.
    told: Option<Value>,
}

impl Asking {
    fn on(bus: &Bus) -> io::Result<Self> {
        Ok(Self {
            // Subscribed before anything is asked: no answer is missed.
            decisions: bus.subscribe_typed(Decision::TOPIC, "voice", Decision::TYPE)?,
            out: bus.publisher_declared(
                VoiceArrival::TOPIC,
                &VoiceArrival::declaration(),
                crate::bus_out::SOURCE,
            )?,
            pending: None,
            told: None,
        })
    }

    /// Read what the policy has decided, keeping the answer about the
    /// hello held. Everything else on the topic is passed over: it is read
    /// so that the socket's queue never fills.
    fn read(&mut self, now_us: u64) {
        for _ in 0..MOST_READ {
            let Ok(Some(message)) = self.decisions.recv(Duration::ZERO) else {
                break;
            };
            let Some(pending) = self.pending.as_mut() else {
                continue;
            };
            let Ok(decision) = message.parse::<Decision>() else {
                continue;
            };
            if decision.about != "hello"
                || decision.arrival != Some(pending.number)
                || pending.answer.is_some()
            {
                continue;
            }
            pending.answer = match (decision.shadow, decision.choice.as_str()) {
                (true, _) => Some((Hello::ByRule, "shadow")),
                (false, "say_now") => Some((Hello::SayNow, "say_now")),
                (false, "wait") => Some((Hello::Wait, "wait")),
                _ => None,
            };
            pending.answered_us = now_us;
        }
        if let Some(pending) = self.pending.as_mut()
            && pending.answer.is_none()
            && now_us.saturating_sub(pending.asked_us) >= ASK_FOR_US
        {
            pending.answer = Some((Hello::ByRule, "none"));
            pending.answered_us = now_us;
        }
    }
}

/// What the model is given in place of something heard.
pub const NOTE: &str = "(Nobody has spoken. Someone has just come and sat down in front of you after being away for a while. Say hello the way a friend at the next desk would: a few warm words and no question they have to answer.)";

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
const AWAY_FOR_US: u64 = 300_000_000;
const LOOKED_AWAY_US: u64 = 20_000_000;
const QUIET_FOR_US: u64 = 30_000_000;
const AT_MOST_EVERY_US: u64 = 1_200_000_000;
/// The file is read this often, and counts while it is this fresh.
const READ_EVERY_US: u64 = 250_000;
const FRESH: Duration = Duration::from_millis(1_500);
/// A face smaller than this share of the picture's height is across the
/// room. People at the lamp were 0.12 to 0.27 on lamp-4ace.
const NEAR: f32 = 0.14;

pub struct Arrival {
    speaks: bool,
    /// The bus, when a due hello asks the policy first.
    asking: Option<Asking>,
    path: Option<PathBuf>,
    read_us: u64,
    near: bool,
    /// Since when someone has been there without a break.
    there_since: Option<u64>,
    /// When a face was last seen.
    seen_us: Option<u64>,
    /// Since when nobody has been there; none while unknown or while someone is.
    gone_since: Option<u64>,
    /// How long nobody had been there when the person now there arrived.
    away_us: u64,
    greeted_us: Option<u64>,
    greeted_this_arrival: bool,
}

impl Arrival {
    pub fn from_environment(now_us: u64) -> Option<Self> {
        let path = std::env::var_os(ENVIRONMENT).filter(|path| !path.is_empty())?;
        let path = PathBuf::from(path);
        let speaks = std::env::var(SPEAK_ENVIRONMENT).is_ok_and(|value| value == "on");
        if !path.is_absolute() {
            return None;
        }
        let arrival = Self {
            speaks,
            ..Self::new(Some(path), now_us)
        };
        if std::env::var(ASK_ENVIRONMENT).as_deref() != Ok("on") {
            return Some(arrival);
        }
        match Bus::from_environment().and_then(|bus| Asking::on(&bus)) {
            Ok(asking) => Some(Self {
                asking: Some(asking),
                ..arrival
            }),
            Err(error) => {
                eprintln!(
                    "lamp-voice: a hello is to ask the policy and the bus cannot be used ({error}); the rule decides"
                );
                Some(arrival)
            }
        }
    }

    /// The same, watching a presence file the caller names; for a test or
    /// a measurement, as `from_environment` with `speaks`.
    pub fn watching(path: Option<PathBuf>, speaks: bool, now_us: u64) -> Self {
        Self {
            speaks,
            ..Self::new(path, now_us)
        }
    }

    /// A due hello asks the policy on `bus` first ([`ASK_ENVIRONMENT`]).
    pub fn asking(mut self, bus: &Bus) -> io::Result<Self> {
        self.asking = Some(Asking::on(bus)?);
        Ok(self)
    }

    /// What the eyes say, given directly instead of read from their file:
    /// for a test or a measurement.
    pub fn seen(&mut self, near: bool, now_us: u64) {
        self.note(near, now_us);
    }

    /// What to do with the hello now. `ready` is whether it is due and the
    /// lamp could begin it (no turn, a provider ready); `quiet_us` is how
    /// long nothing has been said. Call every tick. Without asking, a ready
    /// hello is [`Hello::ByRule`] at once, as it always was. Asking, the
    /// first ready tick says it on `voice/arrival`, and it is
    /// [`Hello::NotYet`] until the policy's answer or [`ASK_FOR_US`].
    pub fn hello(&mut self, ready: bool, quiet_us: u64, now_us: u64) -> Hello {
        let since_hello_min = self
            .greeted_us
            .map(|at| u32::try_from(now_us.saturating_sub(at) / 60_000_000).unwrap_or(u32::MAX));
        let away_min = u32::try_from(self.away_us / 60_000_000).unwrap_or(u32::MAX);
        let there_s = self.there_since.map_or(0, |since| {
            u32::try_from(now_us.saturating_sub(since) / 1_000_000).unwrap_or(u32::MAX)
        });
        let Some(asking) = self.asking.as_mut() else {
            return if ready { Hello::ByRule } else { Hello::NotYet };
        };
        asking.read(now_us);
        // An answer that waited too long for the lamp to be ready is old.
        if asking.pending.as_ref().is_some_and(|pending| {
            pending.answer.is_some() && now_us.saturating_sub(pending.answered_us) > ANSWER_LASTS_US
        }) {
            asking.pending = None;
        }
        if !ready {
            return Hello::NotYet;
        }
        let Some(pending) = asking.pending.as_ref() else {
            let number = now_us.max(1);
            let said = VoiceArrival {
                arrival: number,
                away_min,
                there_s,
                quiet_s: u32::try_from(quiet_us / 1_000_000).unwrap_or(u32::MAX),
                since_hello_min,
            };
            if asking.out.publish_json(now_us, &said).is_err() {
                return Hello::ByRule;
            }
            eprintln!("lamp-voice: a hello is due; asking the policy (arrival {number})");
            asking.pending = Some(Pending {
                number,
                asked_us: now_us,
                answer: None,
                answered_us: 0,
            });
            return Hello::NotYet;
        };
        let Some((hello, said)) = pending.answer else {
            return Hello::NotYet;
        };
        let waited_ms = pending.answered_us.saturating_sub(pending.asked_us) / 1_000;
        asking.told = Some(json!({"kind":"hello_policy","at_us":now_us,
            "arrival":pending.number,"answer":said,"waited_ms":waited_ms}));
        eprintln!("lamp-voice: the policy's answer to the hello: {said} after {waited_ms} ms");
        asking.pending = None;
        hello
    }

    /// The session record's line about the last answer acted on, once.
    pub fn take_told(&mut self) -> Option<Value> {
        self.asking.as_mut().and_then(|asking| asking.told.take())
    }

    /// Whether a hello that is due is said, or only recorded.
    pub fn speaks(&self) -> bool {
        self.speaks
    }

    fn new(path: Option<PathBuf>, now_us: u64) -> Self {
        Self {
            speaks: false,
            asking: None,
            path,
            read_us: 0,
            near: false,
            there_since: None,
            seen_us: None,
            // Nobody is known to be there when the service starts; whoever
            // is turns up in the first read and was never away.
            gone_since: Some(now_us),
            away_us: 0,
            greeted_us: None,
            greeted_this_arrival: false,
        }
    }

    /// Whether the eyes have a near face now, from their file.
    fn near_now(&mut self, now_us: u64) -> bool {
        if now_us.saturating_sub(self.read_us) < READ_EVERY_US {
            return self.near;
        }
        self.read_us = now_us;
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
            if self.there_since.is_none() {
                // An arrival, unless it is the same person looking back.
                if let Some(gone) = self.gone_since.take() {
                    self.away_us = now_us.saturating_sub(gone);
                    self.greeted_this_arrival = false;
                    // What was asked for someone before is not about them.
                    if let Some(asking) = self.asking.as_mut() {
                        asking.pending = None;
                    }
                }
                self.there_since = Some(now_us);
            }
            self.seen_us = Some(now_us);
        } else {
            self.there_since = None;
            if self.gone_since.is_none()
                && self
                    .seen_us
                    .is_none_or(|seen| now_us.saturating_sub(seen) >= LOOKED_AWAY_US)
            {
                // Gone since they were last seen, not since this was noticed.
                self.gone_since = Some(self.seen_us.unwrap_or(now_us));
            }
        }
    }

    /// Whether the lamp should say hello now. `quiet_us` is how long it has
    /// been since anything was said to the lamp or by it. Nothing changes
    /// until [`Self::said`]: a hello that could not be begun is still due.
    pub fn due(&self, quiet_us: u64, now_us: u64) -> bool {
        !self.greeted_this_arrival
            && self
                .there_since
                .is_some_and(|since| now_us.saturating_sub(since) >= THERE_FOR_US)
            && self.away_us >= AWAY_FOR_US
            && quiet_us >= QUIET_FOR_US
            && self
                .greeted_us
                .is_none_or(|at| now_us.saturating_sub(at) >= AT_MOST_EVERY_US)
    }

    /// Whether someone has been at the lamp for a moment now.
    pub fn present(&self, now_us: u64) -> bool {
        self.there_since
            .is_some_and(|since| now_us.saturating_sub(since) >= THERE_FOR_US)
    }

    /// The hello has been begun.
    pub fn said(&mut self, now_us: u64) {
        self.greeted_this_arrival = true;
        self.greeted_us = Some(now_us);
    }

    #[cfg(test)]
    fn decide(&mut self, near: bool, quiet_us: u64, now_us: u64) -> bool {
        self.note(near, now_us);
        let due = self.due(quiet_us, now_us);
        if due {
            self.said(now_us);
        }
        due
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000;
    const QUIET: u64 = 60 * S;

    /// Run `seconds` of ticks with a face there or not; whether it greeted.
    fn pass(arrival: &mut Arrival, now: &mut u64, seconds: u64, near: bool, quiet: u64) -> u32 {
        let mut greetings = 0;
        for _ in 0..seconds * 10 {
            *now += S / 10;
            greetings += u32::from(arrival.decide(near, quiet, *now));
        }
        greetings
    }

    #[test]
    fn it_says_hello_once_to_someone_who_comes_back_after_five_minutes() {
        let mut now = 1_000 * S;
        let mut arrival = Arrival::new(None, now);
        // An empty desk for six minutes, then someone sits down.
        assert_eq!(pass(&mut arrival, &mut now, 360, false, QUIET), 0);
        assert_eq!(
            pass(&mut arrival, &mut now, 2, true, QUIET),
            0,
            "not yet three seconds"
        );
        assert_eq!(pass(&mut arrival, &mut now, 2, true, QUIET), 1);
        // They stay for an hour, looking away now and then: nothing more.
        for _ in 0..60 {
            assert_eq!(pass(&mut arrival, &mut now, 50, true, QUIET), 0);
            assert_eq!(pass(&mut arrival, &mut now, 10, false, QUIET), 0);
        }
    }

    #[test]
    fn it_keeps_quiet_for_everything_else() {
        // Someone already there when the service starts was never away.
        let mut now = 1_000 * S;
        let mut arrival = Arrival::new(None, now);
        assert_eq!(pass(&mut arrival, &mut now, 600, true, QUIET), 0);
        // Away for two minutes only.
        assert_eq!(pass(&mut arrival, &mut now, 120, false, QUIET), 0);
        assert_eq!(pass(&mut arrival, &mut now, 60, true, QUIET), 0);
        // Someone walking past: a face for two seconds after a long absence.
        assert_eq!(pass(&mut arrival, &mut now, 600, false, QUIET), 0);
        assert_eq!(pass(&mut arrival, &mut now, 2, true, QUIET), 0);
        assert_eq!(pass(&mut arrival, &mut now, 600, false, QUIET), 0);
        // A room in conversation: they arrive, but it is not quiet. Once it
        // has been quiet for half a minute with them still there, it does.
        assert_eq!(pass(&mut arrival, &mut now, 30, true, 5 * S), 0);
        assert_eq!(pass(&mut arrival, &mut now, 1, true, QUIET), 1);
        // A hello that could not be begun stays due until it is.
        let mut waiting = Arrival::new(None, now);
        let mut later = now;
        for _ in 0..4_000 {
            later += S / 10;
            waiting.note(later > now + 310 * S, later);
        }
        assert!(waiting.due(QUIET, later) && waiting.due(QUIET, later + S));
        waiting.said(later + S);
        assert!(!waiting.due(QUIET, later + 2 * S));
        // And then not again for twenty minutes, whoever comes.
        assert_eq!(pass(&mut arrival, &mut now, 400, false, QUIET), 0);
        assert_eq!(pass(&mut arrival, &mut now, 30, true, QUIET), 0);
        assert_eq!(pass(&mut arrival, &mut now, 800, false, QUIET), 0);
        assert_eq!(pass(&mut arrival, &mut now, 10, true, QUIET), 1);
    }

    /// An arrival after six minutes away, there three seconds: due.
    fn arrived(asking: Option<&Bus>) -> (Arrival, u64) {
        let mut now = 1_000 * S;
        let mut arrival = Arrival::watching(None, true, now);
        if let Some(bus) = asking {
            arrival = arrival.asking(bus).unwrap();
        }
        now += 360 * S;
        arrival.seen(true, now);
        now += 3 * S;
        arrival.seen(true, now);
        assert!(arrival.due(QUIET, now));
        (arrival, now)
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

    /// The policy, played by the test: it reads the arrival and answers.
    fn decide(bus: &Bus, number: u64, choice: &str, shadow: bool) {
        let mut out = bus
            .publisher_declared(Decision::TOPIC, &Decision::declaration(), "policy")
            .unwrap();
        out.publish_json(
            0,
            &Decision {
                about: "hello".into(),
                choice: choice.into(),
                probability: 0.7,
                answers: Default::default(),
                by: "stand-in".into(),
                took_ms: 1,
                shadow,
                request: None,
                task: None,
                arrival: Some(number),
                ask: None,
            },
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(30));
    }

    #[test]
    fn without_asking_a_ready_hello_is_the_rules_at_once() {
        let (mut arrival, now) = arrived(None);
        assert_eq!(arrival.hello(false, QUIET, now), Hello::NotYet);
        assert_eq!(arrival.hello(true, QUIET, now), Hello::ByRule);
        assert!(arrival.take_told().is_none());
    }

    #[test]
    fn asking_holds_the_hello_for_the_policys_answer() {
        let (bus, root) = bus("answers");
        let mut heard = bus
            .subscribe_typed(VoiceArrival::TOPIC, "test", VoiceArrival::TYPE)
            .unwrap();
        for (choice, shadow, expected, said) in [
            ("say_now", false, Hello::SayNow, "say_now"),
            ("wait", false, Hello::Wait, "wait"),
            // The policy only watches: the rule says, at once.
            ("say_now", true, Hello::ByRule, "shadow"),
        ] {
            let (mut arrival, now) = arrived(Some(&bus));
            assert_eq!(arrival.hello(true, QUIET, now), Hello::NotYet);
            std::thread::sleep(Duration::from_millis(30));
            let asked: VoiceArrival = heard
                .recv(Duration::from_secs(2))
                .unwrap()
                .expect("the arrival on the bus")
                .parse()
                .unwrap();
            assert_eq!(
                asked,
                VoiceArrival {
                    arrival: now,
                    away_min: 6,
                    there_s: 3,
                    quiet_s: 60,
                    since_hello_min: None,
                }
            );
            // Another arrival's answer is not this one's.
            decide(&bus, now + 1, "wait", false);
            assert_eq!(arrival.hello(true, QUIET, now + 1_000), Hello::NotYet);
            decide(&bus, now, choice, shadow);
            // Not ready when it comes: kept until the lamp is.
            assert_eq!(arrival.hello(false, QUIET, now + 50_000), Hello::NotYet);
            assert_eq!(arrival.hello(true, QUIET, now + 60_000), expected);
            let told = arrival.take_told().unwrap();
            assert_eq!(told["kind"], "hello_policy");
            assert_eq!(
                (told["arrival"].as_u64(), told["answer"].as_str()),
                (Some(now), Some(said))
            );
            assert_eq!(told["waited_ms"], 50);
            assert!(arrival.take_told().is_none());
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn no_answer_within_two_seconds_leaves_it_to_the_rule() {
        let (bus, root) = bus("silent");
        let (mut arrival, now) = arrived(Some(&bus));
        assert_eq!(arrival.hello(true, QUIET, now), Hello::NotYet);
        assert_eq!(
            arrival.hello(true, QUIET, now + ASK_FOR_US - 1),
            Hello::NotYet
        );
        assert_eq!(arrival.hello(true, QUIET, now + ASK_FOR_US), Hello::ByRule);
        assert_eq!(arrival.take_told().unwrap()["answer"], "none");
        // An answer that could not be acted on for long is asked again.
        let (mut late, now) = arrived(Some(&bus));
        assert_eq!(late.hello(true, QUIET, now), Hello::NotYet);
        decide(&bus, now, "say_now", false);
        assert_eq!(late.hello(false, QUIET, now + 100_000), Hello::NotYet);
        assert_eq!(
            late.hello(false, QUIET, now + 100_000 + ANSWER_LASTS_US + 1),
            Hello::NotYet
        );
        assert_eq!(
            late.hello(true, QUIET, now + 200_000 + ANSWER_LASTS_US),
            Hello::NotYet
        );
        std::fs::remove_dir_all(root).unwrap();
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
        let mut arrival = Arrival::new(Some(path.clone()), 0);
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
