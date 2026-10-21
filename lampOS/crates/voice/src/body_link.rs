//! Tells the lamp's body what the lamp is doing: one small datagram, never
//! waited for. A body that is absent, slow or stopped changes nothing here,
//! and nothing comes back.
//!
//! The body is a separate process (`lamp-body` in `lamp-body`) that owns the
//! servo port. This side only names a mood and a voice level; what moves, how
//! far and whether at all is decided there.
use crate::ring_wire::RingPhase;
use std::{io, os::unix::net::UnixDatagram, path::PathBuf};

/// The body's socket, when the service is to tell one. Unset: nothing is sent.
pub const ENVIRONMENT: &str = "LAMP_BODY_MOOD_SOCKET";
/// A change of mood is told at once; an unchanged one this often, so that
/// the voice level arrives and the body knows this side is alive.
const REPEAT_US: u64 = 40_000;

/// Turns in a row the lamp did not answer after which its head stops
/// lifting for what it hears: the talk around it is not for it.
const SETTLE_AFTER: u8 = 2;
/// Turns in a row the lamp did not answer after which the room counts as
/// busy with other people's talk ([`Attention::busy`]).
const BUSY_AFTER: u8 = 3;
/// Quiet after which that is forgotten and the next voice gets its head again.
const FORGET_US: u64 = 30_000_000;

/// Whether what the lamp has been hearing lately was for it. A lamp beside
/// two people talking to each other looks up at the first sentences and then
/// settles; it does not lift its head and look away at every one. The light
/// still shows that it hears, and it still answers when it is spoken to: the
/// answer brings the head back and clears this.
#[derive(Default)]
pub struct Attention {
    /// The turn in progress and whether the lamp has had anything to say in it.
    turn: Option<(u64, bool)>,
    unanswered: u8,
    ended_us: u64,
    /// A turn let through only for the model to judge (a short sound in a
    /// quiet room, [`crate::not_speech::sent_anyway`]): left unanswered it is
    /// not someone else's talk, and does not count toward a busy room.
    uncounted: Option<u64>,
}

impl Attention {
    /// Call every tick with the turn that owns the lamp, if any, and whether
    /// an answer to it has arrived.
    pub fn observe(&mut self, turn: Option<(u64, bool)>, now_us: u64) {
        if self.turn.map(|(id, _)| id) != turn.map(|(id, _)| id)
            && let Some((id, answered)) = self.turn
        {
            self.unanswered = if answered {
                0
            } else if self.uncounted == Some(id) {
                self.unanswered
            } else {
                self.unanswered.saturating_add(1)
            };
            self.ended_us = now_us;
        }
        self.turn = turn;
        if turn.is_some_and(|(_, answered)| answered)
            || (turn.is_none() && now_us.saturating_sub(self.ended_us) >= FORGET_US)
        {
            self.unanswered = 0;
        }
    }

    /// The turn `turn` was let through only for the model to judge: if it
    /// ends unanswered it is not counted.
    pub fn not_counted(&mut self, turn: u64) {
        self.uncounted = Some(turn);
    }

    /// The head stays down for listening and thinking.
    pub fn settled(&self) -> bool {
        self.unanswered >= SETTLE_AFTER
    }

    /// People are talking to each other around the lamp: the last three
    /// things it heard went unanswered. Someone who then does speak to it
    /// is not to lose their answer to the next voice in the room.
    pub fn busy(&self) -> bool {
        self.unanswered >= BUSY_AFTER
    }
}

pub struct BodyLink {
    socket: UnixDatagram,
    path: PathBuf,
    last: Option<(&'static str, u64)>,
}

impl BodyLink {
    pub fn from_environment() -> Option<Self> {
        let path = std::env::var_os(ENVIRONMENT).filter(|path| !path.is_empty())?;
        Self::to(path.into()).ok()
    }

    pub fn to(path: PathBuf) -> io::Result<Self> {
        let socket = UnixDatagram::unbound()?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            path,
            last: None,
        })
    }

    /// `phase` is the phase of the turn that owns the lamp, or none between
    /// turns. `level` is the voice in play, as for the ring's face.
    pub fn tell(&mut self, phase: Option<RingPhase>, level: u8, now_us: u64) {
        let mood = match phase {
            None => "resting",
            Some(RingPhase::Listening) => "listening",
            Some(RingPhase::Waiting) => "thinking",
            Some(RingPhase::Speaking) => "speaking",
        };
        if self
            .last
            .is_some_and(|(told, at)| told == mood && now_us.saturating_sub(at) < REPEAT_US)
        {
            return;
        }
        self.last = Some((mood, now_us));
        let message = format!(r#"{{"mood":"{mood}","level":{level}}}"#);
        // Nobody listening, a full queue, a missing path: all the same here.
        let _ = self.socket.send_to(message.as_bytes(), &self.path);
    }

    /// Ask the body for one gesture by name. Whether it makes it is the
    /// body's to decide: a lamp that is not standing makes none.
    pub fn gesture(&mut self, name: &str) {
        let message = format!(r#"{{"gesture":"{name}"}}"#);
        let _ = self.socket.send_to(message.as_bytes(), &self.path);
    }
}

/// The gesture that goes with how an answer begins, as a person's head does:
/// a nod with "yes", a shake with "no", a greeting with "hello", in English
/// or Vietnamese. Most answers begin with none of these and get none.
pub fn cue_for(opening: &str) -> Option<&'static str> {
    let plain: String = opening
        .chars()
        .flat_map(|c| match c {
            '\'' | '\u{2019}' => vec!['\''],
            c if c.is_alphabetic() => c.to_lowercase().collect(),
            _ => vec![' '],
        })
        .collect();
    let words: Vec<&str> = plain.split_whitespace().collect();
    let begins = |phrases: &[&str]| {
        phrases.iter().any(|phrase| {
            let wanted: Vec<&str> = phrase.split(' ').collect();
            words.len() >= wanted.len() && words[..wanted.len()] == wanted[..]
        })
    };
    // The moves kept from V1 on 2026-10-16 (#940, docs/skills.md), each for
    // the way an answer begins. A body without the clip ignores the name,
    // but an opening these now take ("No way!" was a shake) loses its old
    // gesture even there.
    // Surprise first: "no way" is surprise, not a refusal.
    if begins(&["wow", "whoa", "oh wow", "no way", "trời ơi"]) {
        return Some("surprised");
    }
    if begins(&[
        "oh no",
        "i'm sorry",
        "i am sorry",
        "sorry to hear",
        "oh dear",
        "that sounds hard",
        "that sounds tough",
        "tiếc quá",
        "thương quá",
    ]) {
        return Some("caring");
    }
    // A bare "Sorry?", "Pardon?" or "Huh?": the lamp did not catch it (#926).
    if words.len() == 1 && ["sorry", "pardon", "huh"].contains(&words[0]) {
        return Some("confused");
    }
    if begins(&["hmm", "hm", "ooh", "interesting", "tell me more"]) {
        return Some("curious");
    }
    if begins(&[
        "got it",
        "on it",
        "will do",
        "done",
        "all done",
        "được rồi",
        "xong rồi",
    ]) {
        return Some("acknowledge");
    }
    if begins(&[
        "yay",
        "woohoo",
        "hooray",
        "congratulations",
        "congrats",
        "awesome",
        "that's great",
        "that's fantastic",
        "that's wonderful",
        "that's amazing",
        "that is amazing",
        "that is incredible",
        "how wonderful",
        "fantastic",
        "wonderful",
        "how fun",
        "tuyệt",
        "hay quá",
    ]) {
        return Some("happy");
    }
    if begins(&["good night", "goodnight", "sleep well", "chúc ngủ ngon"]) {
        return Some("sleepy");
    }
    // Said in agreement, though they begin with "no".
    if begins(&[
        "no problem",
        "no worries",
        "no doubt",
        "không sao",
        "không có gì",
        "không có chi",
    ]) {
        return None;
    }
    if begins(&[
        "hi",
        "hello",
        "hey",
        "hiya",
        "howdy",
        "good morning",
        "good afternoon",
        "good evening",
        "morning",
        "chào",
        "xin chào",
        "a lô",
        "alo",
    ]) {
        Some("greeting")
    } else if words.first().is_some_and(|word| {
        ["haha", "hehe", "hihi"]
            .iter()
            .any(|laugh| word.starts_with(laugh))
            || *word == "ha"
    }) {
        Some("laugh")
    } else if begins(&[
        "yes",
        "yeah",
        "yep",
        "yup",
        "sure",
        "absolutely",
        "definitely",
        "exactly",
        "certainly",
        "correct",
        "indeed",
        "okay",
        "ok",
        "of course",
        "that's right",
        "you bet",
        "you got it",
        "ừ",
        "ừm",
        "vâng",
        "dạ",
        "đúng rồi",
        "đúng vậy",
        "chắc chắn",
        "tất nhiên",
        "có chứ",
        "được chứ",
        "được thôi",
    ]) {
        Some("nod")
    } else if begins(&[
        "no",
        "nope",
        "nah",
        "not really",
        "i don't think",
        "i'm afraid not",
        "i'm not",
        "không",
        "chưa",
        "đâu có",
    ]) {
        Some("headshake")
    } else if begins(&[
        "thank you",
        "thanks",
        "aw",
        "aww",
        "oh stop",
        "cảm ơn",
        "cám ơn",
    ]) {
        Some("shy")
    } else {
        None
    }
}

/// Decides once per answer, from its first words as they arrive.
#[derive(Default)]
pub struct Cue {
    turn: Option<u64>,
    opening: String,
    decided: bool,
}

impl Cue {
    /// Call with each piece of the lamp's own words for a turn. Returns the
    /// gesture for that answer the first time enough of it is known: three
    /// words, or its end.
    pub fn heard(&mut self, turn: u64, text: &str, finished: bool) -> Option<&'static str> {
        if self.turn != Some(turn) {
            self.turn = Some(turn);
            self.opening.clear();
            self.decided = false;
        }
        if self.decided {
            return None;
        }
        if self.opening.len() < 96 {
            self.opening.push_str(text);
        }
        if !finished && self.opening.split_whitespace().count() < 3 && self.opening.len() < 24 {
            return None;
        }
        self.decided = true;
        cue_for(&self.opening)
    }
}

/// The switch for the onset cue (#2669): `on` exactly. Off, the body hears
/// `listening` only once an input is admitted, as before.
pub const ONSET_ENVIRONMENT: &str = "LAMP_BODY_ONSET_LISTENING";
/// Speech heard this long, on a resting lamp, tells the body `listening`
/// (the first builder on #2669): a clap is over long before it.
pub const ONSET_SPEECH_US: u64 = 300_000;
/// A capture block (`lamp_audio::BLOCK_MS`).
const ONSET_BLOCK_US: u64 = lamp_audio::BLOCK_MS as u64 * 1_000;
/// A dip in the detector's score no longer than this, between syllables,
/// does not break the speech: shorter than the gap between two hand claps.
const ONSET_GAP_US: u64 = 80_000;
/// After the last speech, the cue holds this long for the decision to
/// admit it; an input admitted takes the mood from the turn instead. Past
/// the 0.6 s an utterance's end waits for its silence, and the admission's
/// 200 ms after it, so an admitted line does not drop to `resting` first.
pub const ONSET_HOLDS_US: u64 = 1_500_000;

/// The body told `listening` at a person's first words rather than at the
/// decision to take them (#2669). A cue to the body only: it opens no turn
/// and changes nothing of what is admitted or answered.
#[derive(Default)]
pub struct Onset {
    /// The speech run in progress: its first and latest speech blocks.
    run: Option<(u64, u64)>,
    /// Once cued: the latest speech block since.
    cued: Option<u64>,
}

impl Onset {
    pub fn from_environment() -> Option<Self> {
        (std::env::var(ONSET_ENVIRONMENT).ok().as_deref() == Some("on")).then(Self::default)
    }

    /// One capture block, ending at `at_us`. `resting`: the lamp is in no
    /// turn of its own (none, or only the one this speech opened, still
    /// hearing it with nothing of the lamp's in it), not speaking or just
    /// done speaking, and the talk around it has not been for others. Speech is the detector's own (score 0.5 or more,
    /// as `last_speech_us` takes it) and, with echo evidence, heard with the
    /// lamp's render quiet ([`lamp_audio::duplex::Heard::Clear`]): never the
    /// lamp's own voice. Returns how long the speech has lasted, in
    /// milliseconds, the once a run reaches [`ONSET_SPEECH_US`].
    pub fn block(
        &mut self,
        at_us: u64,
        probability: f32,
        echo: Option<&lamp_audio::evidence::EchoEvidence>,
        resting: bool,
    ) -> Option<u64> {
        if !resting {
            *self = Self::default();
            return None;
        }
        let speech = match echo {
            Some(evidence) => {
                lamp_audio::duplex::Hearing::EVALUATED.heard(probability, evidence)
                    == lamp_audio::duplex::Heard::Clear
            }
            None => probability.is_finite() && probability >= 0.5,
        };
        if !speech {
            if self
                .run
                .is_some_and(|(_, last)| at_us.saturating_sub(last) > ONSET_GAP_US)
            {
                self.run = None;
            }
            return None;
        }
        let first = match self.run {
            Some((first, last)) if at_us.saturating_sub(last) <= ONSET_GAP_US + ONSET_BLOCK_US => {
                first
            }
            _ => at_us,
        };
        self.run = Some((first, at_us));
        let lasted = at_us.saturating_sub(first) + ONSET_BLOCK_US;
        let first_cue = self.cued.is_none() && lasted >= ONSET_SPEECH_US;
        if first_cue || self.cued.is_some() {
            self.cued = Some(at_us);
        }
        first_cue.then_some(lasted / 1_000)
    }

    /// Whether the body is to hear `listening` now for the onset.
    pub fn listening(&mut self, now_us: u64) -> bool {
        if self
            .cued
            .is_some_and(|last| now_us.saturating_sub(last) >= ONSET_HOLDS_US)
        {
            self.cued = None;
        }
        self.cued.is_some()
    }
}

/// The eyes' socket, when the service is to let a camera look. Unset:
/// nothing is sent and the eyes stay shut.
pub const EYES_ENVIRONMENT: &str = "LAMP_EYES_PERMIT_SOCKET";
/// Permission is repeated this often. The eyes close by themselves when it
/// has not come for a second and a half.
const PERMIT_REPEAT_US: u64 = 500_000;

/// Tells the lamp's eyes (`lamp-eyes`, a separate process that owns the
/// camera) whether they may be open: they may exactly while the microphone
/// is permitted. Yes is repeated; no is said once, at once. A voice cycle
/// that ends (a mute, a fault, the service's stop) says no as it goes
/// ([`EyesLink::close`], also on drop), so the camera does not wait out the
/// eyes' 1.5 s on a privacy close (#1802). A service killed outright says
/// nothing more, and the eyes close on that 1.5 s.
/// What a test's eyes link said, and when (`EyesLink::logging`).
#[cfg(test)]
pub(crate) type SaidLog = std::sync::Arc<std::sync::Mutex<Vec<(bool, u64)>>>;

pub struct EyesLink {
    socket: UnixDatagram,
    path: PathBuf,
    last: Option<(bool, u64)>,
    /// What a test hears it said, and when (`EyesLink::logging`).
    #[cfg(test)]
    said: Option<SaidLog>,
}

impl EyesLink {
    pub fn from_environment() -> Option<Self> {
        let path = std::env::var_os(EYES_ENVIRONMENT).filter(|path| !path.is_empty())?;
        Self::to(path.into()).ok()
    }

    pub fn to(path: PathBuf) -> io::Result<Self> {
        let socket = UnixDatagram::unbound()?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            path,
            last: None,
            #[cfg(test)]
            said: None,
        })
    }

    /// A test's link that also notes each yes or no it says, with the
    /// time it says it on the loop test's clock.
    #[cfg(test)]
    pub(crate) fn logging(mut self, said: SaidLog) -> Self {
        self.said = Some(said);
        self
    }

    pub fn permit(&mut self, allowed: bool, now_us: u64) {
        if self.last.is_some_and(|(told, at)| {
            told == allowed && (!allowed || now_us.saturating_sub(at) < PERMIT_REPEAT_US)
        }) {
            return;
        }
        self.last = Some((allowed, now_us));
        let message: &[u8] = if allowed {
            br#"{"camera":true}"#
        } else {
            br#"{"camera":false}"#
        };
        // Nobody listening, a full queue, a missing path: all the same here.
        let _ = self.socket.send_to(message, &self.path);
        #[cfg(test)]
        if let Some(said) = &self.said {
            said.lock()
                .unwrap()
                .push((allowed, crate::scenario_clock::monotonic_us()));
        }
    }

    /// No, now, whatever was said last and when, unless no was the last
    /// thing said: whether it was said now. One datagram, never waited on
    /// (#1802).
    pub fn close(&mut self) -> bool {
        if self.last.is_some_and(|(told, _)| !told) {
            return false;
        }
        self.permit(false, 0);
        true
    }

    /// After [`EyesLink::close`]: ask the eyes to say on `eyes/camera_closed`
    /// that the camera is shut for this close, once it is (#1802).
    pub fn ask_close_receipt(&mut self, close: u64) {
        let message = format!(r#"{{"close_receipt":{close}}}"#);
        let _ = self.socket.send_to(message.as_bytes(), &self.path);
    }

    /// Ask the eyes for one full look (#836), whose still comes marked with
    /// `id`, at its own longer side when one is given (#941). Eyes started
    /// without `--full-look` take it for nothing.
    pub fn look_full(&mut self, id: u64, side: Option<u32>) {
        let message = match side {
            None => format!(r#"{{"look_full":{id}}}"#),
            Some(side) => format!(r#"{{"look_full":{id},"side":{side}}}"#),
        };
        let _ = self.socket.send_to(message.as_bytes(), &self.path);
    }
}

impl Drop for EyesLink {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every datagram the eyes have been sent so far.
    fn told(eyes: &UnixDatagram) -> Vec<String> {
        let mut datagram = [0u8; 64];
        let mut told = Vec::new();
        while let Ok(length) = eyes.recv(&mut datagram) {
            told.push(String::from_utf8_lossy(&datagram[..length]).into_owned());
        }
        told
    }

    #[test]
    fn a_closing_cycle_tells_the_eyes_no_at_once_and_only_once() {
        let path = std::env::temp_dir().join(format!("lamp-eyes-close-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let eyes = UnixDatagram::bind(&path).unwrap();
        eyes.set_nonblocking(true).unwrap();
        let mut link = EyesLink::to(path.clone()).unwrap();
        link.permit(true, 0);
        let started = std::time::Instant::now();
        // Just after a yes: a close does not wait for a repeat to fall due.
        assert!(link.close());
        assert!(started.elapsed() < std::time::Duration::from_millis(50));
        assert_eq!(told(&eyes), [r#"{"camera":true}"#, r#"{"camera":false}"#]);
        // No is said once: a second close and a drop after it add nothing.
        assert!(!link.close());
        drop(link);
        assert!(told(&eyes).is_empty());
        // A cycle that ends while permitted (a mute's early return, a fault)
        // says no as it goes.
        let mut link = EyesLink::to(path.clone()).unwrap();
        link.permit(true, 0);
        drop(link);
        assert_eq!(told(&eyes), [r#"{"camera":true}"#, r#"{"camera":false}"#]);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_close_with_no_eyes_listening_neither_waits_nor_fails() {
        let path = std::env::temp_dir().join(format!("lamp-eyes-absent-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut link = EyesLink::to(path).unwrap();
        link.permit(true, 0);
        let started = std::time::Instant::now();
        assert!(link.close());
        drop(link);
        assert!(started.elapsed() < std::time::Duration::from_millis(50));
    }

    #[test]
    fn a_close_receipt_is_asked_for_after_the_no_in_one_exact_datagram() {
        let path = std::env::temp_dir().join(format!("lamp-eyes-receipt-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let eyes = UnixDatagram::bind(&path).unwrap();
        eyes.set_nonblocking(true).unwrap();
        let mut link = EyesLink::to(path.clone()).unwrap();
        link.permit(true, 0);
        assert!(link.close());
        link.ask_close_receipt(42);
        assert_eq!(
            told(&eyes),
            [
                r#"{"camera":true}"#,
                r#"{"camera":false}"#,
                r#"{"close_receipt":42}"#
            ]
        );
        drop(link);
        assert!(told(&eyes).is_empty());
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn the_eyes_are_asked_for_a_full_look_in_one_exact_datagram() {
        let directory =
            std::env::temp_dir().join(format!("lamp-voice-eyes-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("permit.sock");
        let _ = std::fs::remove_file(&path);
        let eyes = UnixDatagram::bind(&path).unwrap();
        let mut link = EyesLink::to(path.clone()).unwrap();
        link.look_full(1_760_000_000_042, None);
        let mut got = [0u8; 64];
        let length = eyes.recv(&mut got).unwrap();
        assert_eq!(&got[..length], br#"{"look_full":1760000000042}"#);
        link.look_full(7, Some(3_264));
        let length = eyes.recv(&mut got).unwrap();
        assert_eq!(&got[..length], br#"{"look_full":7,"side":3264}"#);
        // The largest id and side fit the eyes' 64-byte read.
        link.look_full(u64::MAX, Some(4_096));
        let length = eyes.recv(&mut got).unwrap();
        assert!(length < 64);
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_turn_let_through_only_to_be_judged_does_not_make_the_room_busy() {
        let mut room = Attention::default();
        let mut now = 0;
        let mut turn = |room: &mut Attention, id: u64, answered: bool, judged_only: bool| {
            room.observe(Some((id, false)), now);
            if judged_only {
                room.not_counted(id);
            }
            room.observe(Some((id, answered)), now + 1);
            now += 1_000_000;
            room.observe(None, now);
        };
        // Short sounds, each left alone by the model: never busy.
        for id in 1..=5 {
            turn(&mut room, id, false, true);
        }
        assert!(!room.busy());
        // Three things said and not answered are someone else's talk.
        for id in 6..=8 {
            turn(&mut room, id, false, false);
        }
        assert!(room.busy());
        // A short sound in between neither adds to it nor clears it.
        turn(&mut room, 9, false, true);
        assert!(room.busy());
        // An answer clears it.
        turn(&mut room, 10, true, false);
        assert!(!room.busy());
    }

    #[test]
    fn a_gesture_goes_with_how_an_answer_begins_and_most_answers_get_none() {
        for (opening, gesture) in [
            ("Yes, it is.", Some("nod")),
            ("Yeah! That one.", Some("nod")),
            ("Of course I can.", Some("nod")),
            ("Okay.", Some("nod")),
            ("That\u{2019}s right, seven.", Some("nod")),
            ("No, not today.", Some("headshake")),
            ("Nope.", Some("headshake")),
            ("I don't think so.", Some("headshake")),
            ("I'm not a toaster, I'm a lamp!", Some("headshake")),
            ("Hello there!", Some("greeting")),
            ("Good morning! How are you?", Some("greeting")),
            ("Haha, good one.", Some("laugh")),
            ("Thank you, that's kind.", Some("shy")),
            ("No problem at all.", None),
            ("Now that is a question.", None),
            ("Nothing much.", None),
            ("His name is Tom.", None),
            ("Seven days in a week.", None),
            ("Sorry, I missed that.", None),
            ("Chào bạn! Hôm nay thế nào?", Some("greeting")),
            ("Xin chào!", Some("greeting")),
            ("Ừ, đúng đó.", Some("nod")),
            ("Dạ, mình nghe đây.", Some("nod")),
            ("Đúng rồi, bảy ngày.", Some("nod")),
            ("Có chứ!", Some("nod")),
            ("Không, hôm nay thì không.", Some("headshake")),
            ("Chưa, mình chưa biết.", Some("headshake")),
            ("Không sao đâu.", None),
            ("Không có gì!", None),
            ("Có lẽ là thứ Ba.", None),
            ("Hihi, vui ghê.", Some("laugh")),
            ("Cảm ơn bạn nha.", Some("shy")),
            ("Mình khỏe, còn bạn?", None),
            ("Wow, that's a big one.", Some("surprised")),
            ("No way! Really?", Some("surprised")),
            ("Oh no, I'm sorry to hear that.", Some("caring")),
            ("I'm sorry, that sounds hard.", Some("caring")),
            ("Sorry?", Some("confused")),
            ("Huh?", Some("confused")),
            ("Hmm, let me think.", Some("curious")),
            ("Got it, two minutes.", Some("acknowledge")),
            ("Done! The light is warmer.", Some("acknowledge")),
            ("Yay, that's wonderful!", Some("happy")),
            ("Congratulations on the new job!", Some("happy")),
            ("That's fantastic! I'm so happy for you.", Some("happy")),
            ("That is incredible, well done.", Some("happy")),
            ("That's a hard one.", None),
            ("Good night, sleep well.", Some("sleepy")),
            ("Trời ơi, thật hả?", Some("surprised")),
            ("Được rồi, hai phút nhé.", Some("acknowledge")),
            ("Chúc ngủ ngon nha.", Some("sleepy")),
            ("Doneness matters for steak.", None),
            ("", None),
        ] {
            assert_eq!(cue_for(opening), gesture, "{opening:?}");
        }
    }

    #[test]
    fn the_cue_is_decided_once_an_answer_from_its_first_words() {
        let mut cue = Cue::default();
        // Words arrive in pieces; one word is not yet enough to know.
        assert_eq!(cue.heard(4, "Yes", false), None);
        assert_eq!(cue.heard(4, ", it", false), None);
        assert_eq!(cue.heard(4, " is the", false), Some("nod"));
        assert_eq!(cue.heard(4, " seventh. No, wait.", true), None);
        // A short answer is known at its end.
        assert_eq!(cue.heard(5, "Nope.", true), Some("headshake"));
        // The next answer starts afresh, and most get nothing.
        assert_eq!(cue.heard(6, "Seven days in a week,", false), None);
        assert_eq!(cue.heard(6, " yes.", true), None);
        let mut cue = Cue::default();
        assert_eq!(
            cue.heard(1, "Hello there, friend!", false),
            Some("greeting")
        );
    }

    #[test]
    fn a_gesture_is_asked_of_the_body_in_exactly_this_text() {
        let path = std::env::temp_dir().join(format!("lamp-body-gesture-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let body = UnixDatagram::bind(&path).unwrap();
        body.set_nonblocking(true).unwrap();
        let mut link = BodyLink::to(path.clone()).unwrap();
        link.gesture("nod");
        let mut datagram = [0u8; 64];
        let length = body.recv(&mut datagram).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(&datagram[..length], br#"{"gesture":"nod"}"#);
    }

    #[test]
    fn the_eyes_are_told_yes_twice_a_second_and_no_once_at_once() {
        let path = std::env::temp_dir().join(format!("lamp-eyes-permit-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let eyes = UnixDatagram::bind(&path).unwrap();
        eyes.set_nonblocking(true).unwrap();
        let mut link = EyesLink::to(path.clone()).unwrap();
        // Every 10 ms for a second and a quarter while permitted, then not.
        for step in 0..125u64 {
            link.permit(true, step * 10_000);
        }
        for step in 125..200u64 {
            link.permit(false, step * 10_000);
        }
        link.permit(true, 2_000_000);
        let mut told = Vec::new();
        let mut datagram = [0u8; 64];
        while let Ok(length) = eyes.recv(&mut datagram) {
            told.push(String::from_utf8_lossy(&datagram[..length]).into_owned());
        }
        std::fs::remove_file(&path).unwrap();
        let (yes, no) = (r#"{"camera":true}"#, r#"{"camera":false}"#);
        assert_eq!(told, vec![yes, yes, yes, no, yes]);
    }

    /// One turn: heard, thought about, perhaps answered, then over.
    fn turn(attention: &mut Attention, now: &mut u64, id: u64, answered: bool) {
        for step in 0..3 {
            *now += 500_000;
            attention.observe(Some((id, answered && step == 2)), *now);
        }
        *now += 500_000;
        attention.observe(None, *now);
    }

    #[test]
    fn the_head_settles_after_two_turns_it_did_not_answer_and_comes_back_for_one_it_does() {
        let mut attention = Attention::default();
        let mut now = 0;
        assert!(!attention.settled());
        turn(&mut attention, &mut now, 1, false);
        assert!(!attention.settled(), "one sentence is not a conversation");
        turn(&mut attention, &mut now, 2, false);
        assert!(attention.settled());
        // Spoken to: the answer arrives and the head is free again at once,
        // while that turn is still in progress.
        attention.observe(Some((3, false)), now + 1);
        assert!(attention.settled());
        attention.observe(Some((3, true)), now + 2);
        assert!(!attention.settled());
        turn(&mut attention, &mut now, 3, true);
        assert!(!attention.settled());
        // One turn replacing another before any answer counts each of them.
        attention.observe(Some((4, false)), now + 3);
        attention.observe(Some((5, false)), now + 4);
        attention.observe(None, now + 5);
        assert!(attention.settled());
    }

    #[test]
    fn a_room_is_busy_after_three_things_unanswered_until_the_lamp_answers_or_it_goes_quiet() {
        let mut room = Attention::default();
        let mut now = 0;
        turn(&mut room, &mut now, 1, false);
        turn(&mut room, &mut now, 2, false);
        assert!(!room.busy(), "a person who paused twice is not a busy room");
        turn(&mut room, &mut now, 3, false);
        assert!(room.busy());
        // The question put to the lamp in that room: busy while it waits
        // for its own answer, and not once the answer has begun.
        room.observe(Some((4, false)), now + 1);
        assert!(room.busy());
        room.observe(Some((4, true)), now + 2);
        assert!(!room.busy());
        turn(&mut room, &mut now, 4, true);
        turn(&mut room, &mut now, 5, false);
        assert!(!room.busy());
        // Left alone for half a minute, it is no longer busy either.
        turn(&mut room, &mut now, 6, false);
        turn(&mut room, &mut now, 7, false);
        assert!(room.busy());
        room.observe(None, now + FORGET_US);
        assert!(!room.busy());
    }

    #[test]
    fn half_a_minute_of_quiet_gives_the_next_voice_the_head_again() {
        let mut attention = Attention::default();
        for id in 1..=3 {
            attention.observe(Some((id, false)), id * 1_000_000);
            attention.observe(None, id * 1_000_000 + 500_000);
        }
        assert!(attention.settled());
        attention.observe(None, 3_500_000 + FORGET_US - 1);
        assert!(attention.settled());
        attention.observe(None, 3_500_000 + FORGET_US);
        assert!(!attention.settled());
        // A long turn is not quiet.
        let mut attention = Attention::default();
        attention.observe(Some((1, false)), 0);
        attention.observe(None, 1);
        attention.observe(Some((2, false)), 2);
        attention.observe(Some((2, false)), 2 + 2 * FORGET_US);
        attention.observe(None, 3 + 2 * FORGET_US);
        assert!(attention.settled());
    }
    use std::sync::atomic::{AtomicU32, Ordering};

    fn listener() -> (UnixDatagram, PathBuf) {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "lamp-body-{}-{}.sock",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_file(&path);
        let socket = UnixDatagram::bind(&path).unwrap();
        socket.set_nonblocking(true).unwrap();
        (socket, path)
    }

    fn received(socket: &UnixDatagram) -> Vec<String> {
        let mut messages = Vec::new();
        let mut bytes = [0u8; 128];
        while let Ok(length) = socket.recv(&mut bytes) {
            messages.push(String::from_utf8(bytes[..length].to_vec()).unwrap());
        }
        messages
    }

    #[test]
    fn each_mood_is_told_at_once_and_an_unchanged_one_only_now_and_then() {
        let (socket, path) = listener();
        let mut link = BodyLink::to(path.clone()).unwrap();
        link.tell(None, 0, 1_000);
        link.tell(Some(RingPhase::Listening), 90, 1_001);
        // The same mood again within the interval: not repeated.
        link.tell(Some(RingPhase::Listening), 200, 20_000);
        link.tell(Some(RingPhase::Listening), 210, 41_001);
        link.tell(Some(RingPhase::Waiting), 0, 41_002);
        link.tell(Some(RingPhase::Speaking), 255, 41_003);
        assert_eq!(
            received(&socket),
            [
                r#"{"mood":"resting","level":0}"#,
                r#"{"mood":"listening","level":90}"#,
                r#"{"mood":"listening","level":210}"#,
                r#"{"mood":"thinking","level":0}"#,
                r#"{"mood":"speaking","level":255}"#,
            ]
        );
        std::fs::remove_file(path).unwrap();
    }

    /// One capture block's echo evidence: the lamp's render quiet for half
    /// a second, or playing and explaining `explained` percent of the block.
    fn echo(playing: Option<u8>) -> lamp_audio::evidence::EchoEvidence {
        use lamp_audio::evidence::{EchoEvidence, EchoMatch, Level};
        EchoEvidence {
            blocks: 1_000,
            render_active_blocks: if playing.is_some() { 50 } else { 0 },
            render_quiet_blocks: if playing.is_some() { 0 } else { 50 },
            capture_level: Level::from_tenths_of_db(-130),
            residual_level: Level::from_tenths_of_db(-570),
            echo: playing.map(|capture_explained| EchoMatch {
                render_level: Level::from_tenths_of_db(-180),
                capture_explained,
                residual_explained: 10,
                residual_playback_floor: 0,
                alignment_age_blocks: 0,
            }),
            silent_at_echo: None,
        }
    }

    /// Plays 10 ms blocks from `from_us` through the onset and tells the
    /// body as the service does between turns (no phase): the score of each
    /// block by its time, and whether the lamp rests. The cue's times, and
    /// when the body first heard `listening`.
    fn play(
        onset: &mut Onset,
        link: &mut BodyLink,
        from_us: u64,
        until_us: u64,
        echo_of: impl Fn(u64) -> Option<lamp_audio::evidence::EchoEvidence>,
        score: impl Fn(u64) -> f32,
        resting: impl Fn(u64) -> bool,
    ) -> Vec<(u64, u64)> {
        let mut cues = Vec::new();
        let mut at = from_us;
        while at <= until_us {
            let evidence = echo_of(at);
            if let Some(speech_ms) = onset.block(at, score(at), evidence.as_ref(), resting(at)) {
                cues.push((at, speech_ms));
            }
            let phase = onset.listening(at).then_some(RingPhase::Listening);
            link.tell(phase, 0, at);
            at += 10_000;
        }
        cues
    }

    const RESTING: &str = r#"{"mood":"resting","level":0}"#;
    const LISTENING: &str = r#"{"mood":"listening","level":0}"#;

    #[test]
    fn the_body_hears_listening_at_300_ms_of_speech_and_rests_again_if_nothing_is_admitted() {
        let (socket, path) = listener();
        let mut link = BodyLink::to(path.clone()).unwrap();
        let mut onset = Onset::default();
        let quiet = |_| Some(echo(None));
        // Silence for 200 ms, then speech from the block ending at 210 ms.
        let speech = |at: u64| if at > 200_000 { 0.9 } else { 0.1 };
        let cues = play(&mut onset, &mut link, 0, 500_000, quiet, speech, |_| true);
        // 30 blocks of speech, 200-500 ms: told at the 300 ms mark, once.
        assert_eq!(cues, [(500_000, 300)]);
        let told = received(&socket);
        let first = told.iter().position(|said| said == LISTENING).unwrap();
        // Resting every 40 ms until then: 0, 40, ... 480 ms is 13 messages.
        assert_eq!(first, 13);
        assert!(told[..first].iter().all(|said| said == RESTING));
        // The person stops at 500 ms and no input is admitted: the cue
        // holds for ONSET_HOLDS_US after the last speech, then rests.
        let mut at = 510_000;
        while at < 500_000 + ONSET_HOLDS_US {
            assert_eq!(onset.block(at, 0.1, Some(&echo(None)), true), None);
            assert!(onset.listening(at), "{at}");
            at += 10_000;
        }
        assert!(!onset.listening(500_000 + ONSET_HOLDS_US));
        link.tell(None, 0, 500_000 + ONSET_HOLDS_US);
        assert_eq!(received(&socket), [RESTING]);
        // An admitted input takes the lamp: the onset lets go of the mood
        // at once and the turn's own phase is told from then.
        let mut onset = Onset::default();
        play(&mut onset, &mut link, 0, 400_000, quiet, |_| 0.9, |_| true);
        assert!(onset.listening(400_000));
        assert_eq!(onset.block(410_000, 0.9, None, false), None);
        assert!(!onset.listening(410_000));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn claps_never_tell_the_body_listening() {
        let (socket, path) = listener();
        let mut link = BodyLink::to(path.clone()).unwrap();
        let mut onset = Onset::default();
        // Even scored as speech: 50 ms claps, four a second, for 3 s.
        let clap = |at: u64| if at % 250_000 < 50_000 { 0.95 } else { 0.05 };
        let cues = play(
            &mut onset,
            &mut link,
            0,
            3_000_000,
            |_| Some(echo(None)),
            clap,
            |_| true,
        );
        assert!(cues.is_empty());
        // One 290 ms burst falls short too.
        let burst = |at: u64| {
            if (4_000_000..4_290_000).contains(&at) {
                0.95
            } else {
                0.05
            }
        };
        let cues = play(
            &mut onset,
            &mut link,
            3_990_000,
            5_000_000,
            |_| None,
            burst,
            |_| true,
        );
        assert!(cues.is_empty());
        assert!(received(&socket).iter().all(|said| said == RESTING));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn music_or_a_loud_tv_the_detector_does_not_call_speech_never_does() {
        let (socket, path) = listener();
        let mut link = BodyLink::to(path.clone()).unwrap();
        let mut onset = Onset::default();
        // Ten seconds of steady sound scored under the detector's 0.5, with
        // a block or two over it now and then.
        let music = |at: u64| if at % 700_000 < 20_000 { 0.6 } else { 0.3 };
        let cues = play(
            &mut onset,
            &mut link,
            0,
            10_000_000,
            |_| Some(echo(None)),
            music,
            |_| true,
        );
        assert!(cues.is_empty());
        assert!(received(&socket).iter().all(|said| said == RESTING));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn the_lamps_own_voice_never_tells_the_body_listening() {
        let (socket, path) = listener();
        let mut link = BodyLink::to(path.clone()).unwrap();
        let mut onset = Onset::default();
        // While it speaks (in a turn, or just done): not resting.
        let cues = play(
            &mut onset,
            &mut link,
            0,
            2_000_000,
            |_| Some(echo(Some(90))),
            |_| 0.9,
            |_| false,
        );
        assert!(cues.is_empty());
        // Between turns with its render still playing (a sound of its own):
        // the render explains the block, or a person is heard over it; or
        // nothing yet says whose it is. Only a quiet render counts.
        for playing in [Some(echo(Some(90))), Some(echo(Some(0)))] {
            let cues = play(
                &mut onset,
                &mut link,
                2_010_000,
                4_000_000,
                |_| playing,
                |_| 0.9,
                |_| true,
            );
            assert!(cues.is_empty());
        }
        let mut alignment_unknown = echo(Some(0));
        alignment_unknown.echo = None;
        let cues = play(
            &mut onset,
            &mut link,
            4_010_000,
            6_000_000,
            |_| Some(alignment_unknown),
            |_| 0.9,
            |_| true,
        );
        assert!(cues.is_empty());
        assert!(received(&socket).iter().all(|said| said == RESTING));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_body_that_is_not_there_changes_nothing() {
        let (socket, path) = listener();
        drop(socket);
        std::fs::remove_file(&path).unwrap();
        let mut link = BodyLink::to(path).unwrap();
        for at in 0..100 {
            link.tell(Some(RingPhase::Speaking), 255, 1 + at * 50_000);
        }
    }
}
