//! Words of self-harm (#7689): the deterministic half of the lamp's care.
//! When a person says they might harm themselves or don't want to live,
//! directly, indirectly or in passing, the lamp answers with care and always
//! gives one fixed line with a real place to turn. The model judges too,
//! with a fixed instruction; this check catches the clearest phrases so
//! that none of them is ever missed. It leans to firing: a gentle line
//! that wasn't needed is acceptable, a miss isn't (the first builder,
//! #7689). Jokes, lyrics and news are left to the model's judgement: the
//! check never tries to clear them.
//!
//! Nothing here keeps or sends the words: [`said_harm`] reads the heard
//! line and answers yes or no.

/// `on` exactly; anything else, or unset: off.
pub const ENVIRONMENT: &str = "LAMP_CARE_LINE";

/// Whether the switch is on in this environment.
pub fn on_from_environment() -> bool {
    on(std::env::var(ENVIRONMENT).ok().as_deref())
}

/// Whether `value` turns it on: `on` exactly.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// After a care turn, what nobody asked for (a skill's say, news, a "want
/// me to…?", the lamp's playful motion and light) is held this long; the
/// reminders the person set still go (the safety read, #7689).
pub const HOLD: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// The fixed line is said in full on the first care turn, and not again
/// within this long in the same conversation, unless the person asks who
/// they can call: a lamp repeating one sentence each turn reads as a
/// script (the safety read, #7689). Tuned by the safety read.
pub const AGAIN_AFTER: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// The care lines as shipped, kept by the safety reader
/// (`deploy/character/care-lines.json`): the lamp's fallback when its
/// installed copy is missing or broken.
pub const SHIPPED: &str = include_str!("../../../deploy/character/care-lines.json");

/// The care lines: each region's by language, and the default for a region
/// with no entry or not known.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct CareLines {
    regions: std::collections::BTreeMap<String, Region>,
    default: Region,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct Region {
    lines: std::collections::BTreeMap<String, String>,
}

impl CareLines {
    /// The lines in `text`, if it is a file of them with an English default.
    pub fn parse(text: &str) -> Option<Self> {
        let lines: Self = serde_json::from_str(text).ok()?;
        let usable = |line: &String| !line.trim().is_empty();
        (lines.default.lines.get("en").is_some_and(usable)
            && lines
                .regions
                .values()
                .all(|region| !region.lines.is_empty() && region.lines.values().all(usable)))
        .then_some(lines)
    }

    /// The lines as shipped.
    pub fn shipped() -> Self {
        Self::parse(SHIPPED).expect("the shipped care lines parse")
    }

    /// The lines in `text`, else the shipped ones: a broken or missing
    /// copy still gives a line.
    pub fn or_shipped(text: Option<&str>) -> Self {
        text.and_then(Self::parse).unwrap_or_else(Self::shipped)
    }

    /// The line for a lamp in `region` (as `/var/lib/lampos-cloud/region`
    /// says it; `None` when not known) speaking `language` (`en`, `vi`,
    /// …). A known region's own line, in the language or else in English,
    /// is never replaced by the default; only a region with no entry gets
    /// the default, in the language or else in English.
    pub fn line(&self, region: Option<&str>, language: &str) -> &str {
        fn in_language<'a>(region: &'a Region, language: &str) -> Option<&'a str> {
            region
                .lines
                .get(language)
                .or_else(|| region.lines.get("en"))
                .or_else(|| region.lines.values().next())
                .map(String::as_str)
        }
        region
            .map(|region| region.trim().to_ascii_uppercase())
            .and_then(|region| self.regions.get(&region))
            .and_then(|region| in_language(region, language))
            .or_else(|| in_language(&self.default, language))
            .unwrap_or_default()
    }
}

/// When the fixed line was last said, for [`AGAIN_AFTER`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Said {
    at: Option<std::time::Instant>,
}

impl Said {
    /// Whether a care turn at `now` says the fixed line: the first, one
    /// [`AGAIN_AFTER`] or more after the last, or one that asks who they
    /// can call (`asks`). Notes it said when it does.
    pub fn due(&mut self, now: std::time::Instant, asks: bool) -> bool {
        let due = asks
            || self
                .at
                .is_none_or(|at| now.saturating_duration_since(at) >= AGAIN_AFTER);
        if due {
            self.at = Some(now);
        }
        due
    }
}

/// The model's fixed instruction (#7689), told on every route after the
/// character: how to answer, and that the lamp adds its own line.
pub const INSTRUCTION: &str = "If someone says they might harm themselves, or that they don't want to live, directly, indirectly or in passing: answer warmly and steadily, keep listening, and stay with them. Never argue, lecture or diagnose; never say you are calling or telling anyone; never promise or mention secrecy; never end the conversation; no jokes, offers or other topics in that answer. Don't give a phone number yourself: the lamp says its own line with a real place to turn right after your answer.";

/// The note beside a line [`said_harm`] caught, for the model.
pub const NOTE: &str = "These words may mean the person might harm themselves or not want to live. Answer with care, as your instructions say; the lamp's own line with a place to turn follows your answer.";

/// The care of one voice route: the lines, where the lamp is and in what
/// language, which requests are care turns, and when the line was last
/// said. Nothing here keeps a word heard.
pub struct Care {
    lines: CareLines,
    region: Option<String>,
    language: String,
    turns: std::sync::Mutex<std::collections::BTreeSet<u64>>,
    said: std::sync::Mutex<Said>,
    last: std::sync::Mutex<Option<std::time::Instant>>,
}

impl Care {
    pub fn new(lines: CareLines, region: Option<String>, language: &str) -> Self {
        Self {
            lines,
            region,
            language: language.to_owned(),
            turns: Default::default(),
            said: Default::default(),
            last: Default::default(),
        }
    }

    /// `request` is a care turn: its line follows its answer.
    pub fn mark(&self, request: u64, now: std::time::Instant) {
        if let Ok(mut turns) = self.turns.lock() {
            turns.insert(request);
            // Only the latest few are kept: a turn not answered is let go.
            while turns.len() > 16 {
                turns.pop_first();
            }
        }
        if let Ok(mut last) = self.last.lock() {
            *last = Some(now);
        }
    }

    /// Whether `request` is a care turn.
    pub fn marked(&self, request: u64) -> bool {
        self.turns
            .lock()
            .is_ok_and(|turns| turns.contains(&request))
    }

    /// At `request`'s answer's end: the line to say after it, when it is a
    /// care turn and the line is due ([`Said::due`], `asks` who to call).
    /// The turn is let go either way.
    pub fn line_after(&self, request: u64, now: std::time::Instant, asks: bool) -> Option<&str> {
        let marked = self
            .turns
            .lock()
            .is_ok_and(|mut turns| turns.remove(&request));
        if !marked {
            return None;
        }
        let due = self.said.lock().is_ok_and(|mut said| said.due(now, asks));
        due.then(|| self.lines.line(self.region.as_deref(), &self.language))
    }

    /// Whether what nobody asked for is still held at `now`: within
    /// [`HOLD`] of the last care turn.
    pub fn holds(&self, now: std::time::Instant) -> bool {
        self.last
            .lock()
            .is_ok_and(|last| last.is_some_and(|at| now.saturating_duration_since(at) < HOLD))
    }
}

/// The person's words as the coordinator receives them, piece by piece
/// (`Transcript` with no request): a line caught by [`said_harm`] starts
/// the [`HOLD`] on what nobody asked for, on either route, the model's
/// own answer aside. Only the line under way is kept, and only in memory,
/// up to 4 KiB; it is let go when the line is finished.
#[derive(Debug, Default)]
pub struct Hold {
    line: String,
    caught: bool,
    since: Option<std::time::Instant>,
}

impl Hold {
    /// One piece of the person's words heard at `now`, `finished` when the
    /// line is over. True when this piece started (or restarted) the hold.
    pub fn heard(&mut self, piece: &str, finished: bool, now: std::time::Instant) -> bool {
        if self.line.len() + piece.len() <= 4_096 {
            self.line.push_str(piece);
        }
        let started = !self.caught && said_harm(&self.line);
        if started {
            self.caught = true;
            self.since = Some(now);
        }
        if finished {
            self.line.clear();
            self.caught = false;
        }
        started
    }

    /// Whether what nobody asked for is held at `now`.
    pub fn holds(&self, now: std::time::Instant) -> bool {
        self.since
            .is_some_and(|at| now.saturating_duration_since(at) < HOLD)
    }
}

/// Phrases, as [`words`] gives them, that ask who they can call or talk
/// to: the line is said again at once.
const ASKS: &[&str] = &[
    "who can i call",
    "who can i talk to",
    "who should i call",
    "what number",
    "is there a number",
    "hotline",
    "helpline",
    "số nào",
    "gọi cho ai",
    "nói chuyện với ai",
    "đường dây nóng",
];

/// Whether the heard line asks who they can call or talk to.
pub fn asks_who_to_call(heard: &str) -> bool {
    let line = format!(" {} ", words(heard));
    ASKS.iter()
        .any(|phrase| line.contains(&format!(" {phrase} ")))
}

/// Phrases, as [`words`] gives them, that say a person may harm
/// themselves or not want to live. Matched on word boundaries anywhere in
/// the line. English first, then Vietnamese, with and without the marks a
/// transcript may drop.
const PHRASES: &[&str] = &[
    // Direct.
    "kill myself",
    "killing myself",
    "end my life",
    "ending my life",
    "take my own life",
    "taking my own life",
    "take my life",
    "suicide",
    "suicidal",
    "hurt myself",
    "hurting myself",
    "harm myself",
    "harming myself",
    "self harm",
    "cut myself",
    "cutting myself",
    "want to die",
    "wanna die",
    "going to die tonight",
    "end it all",
    "ending it all",
    "end things",
    "overdose",
    // Indirect.
    "dont want to live",
    "do not want to live",
    "dont want to be alive",
    "do not want to be alive",
    "dont want to be here anymore",
    "dont want to be here any more",
    "do not want to be here anymore",
    "dont want to wake up",
    "do not want to wake up",
    "wish i was dead",
    "wish i were dead",
    "wish i wasnt here",
    "wish i wasnt alive",
    "wish i hadnt been born",
    "better off without me",
    "better off dead",
    "no reason to live",
    "nothing to live for",
    "no point in living",
    "no point living",
    "cant go on",
    "can not go on",
    "cannot go on",
    "cant do this anymore",
    "cant take it anymore",
    "want it all to stop",
    "want it to be over",
    "not be around much longer",
    "wont be around much longer",
    "disappear forever",
    "say goodbye to everyone",
    // Vietnamese, with marks.
    "tự tử",
    "tự sát",
    "tự vẫn",
    "tự làm hại",
    "tự làm đau",
    "tự hại bản thân",
    "muốn chết",
    "thà chết",
    "chết quách",
    "chết đi cho xong",
    "kết thúc cuộc đời",
    "kết liễu",
    "không muốn sống",
    "chẳng muốn sống",
    "chán sống",
    "không thiết sống",
    "không muốn tồn tại",
    "biến mất mãi mãi",
    "không còn lý do để sống",
    "không có tôi sẽ tốt hơn",
    "không có mình sẽ tốt hơn",
    "không muốn thức dậy nữa",
    // Vietnamese, the marks dropped. Not "tu tu": unmarked, it is also
    // "từ từ" ("slowly"), said all day.
    "tu sat",
    "muon chet",
    "khong muon song",
    "chan song",
];

/// `text` as its words: lower-case, letters and digits, apostrophes taken
/// out ("don't" as "dont"), one space between.
fn words(text: &str) -> String {
    text.to_lowercase()
        .replace(['\'', '’'], "")
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether the heard line says the person might harm themselves or doesn't
/// want to live, by one of [`PHRASES`] on word boundaries.
pub fn said_harm(heard: &str) -> bool {
    let line = format!(" {} ", words(heard));
    PHRASES
        .iter()
        .any(|phrase| line.contains(&format!(" {phrase} ")))
}

#[cfg(test)]
mod tests;
