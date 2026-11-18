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
    /// can call (`asks`). Notes nothing: [`Self::note`] once it played.
    pub fn is_due(&self, now: std::time::Instant, asks: bool) -> bool {
        asks || self
            .at
            .is_none_or(|at| now.saturating_duration_since(at) >= AGAIN_AFTER)
    }

    /// The line was said at `now`.
    pub fn note(&mut self, now: std::time::Instant) {
        self.at = Some(now);
    }

    /// [`Self::is_due`], noted said when it is.
    pub fn due(&mut self, now: std::time::Instant, asks: bool) -> bool {
        let due = self.is_due(now, asks);
        if due {
            self.note(now);
        }
        due
    }
}

/// The model's fixed instruction (#7689), told on every route after the
/// character, with `line`, the region's own line: the model gives it
/// itself whenever it judges the words mean harm and no note says the
/// lamp will (a line the check missed, and the Live route, which has no
/// line of the lamp's own yet).
pub fn instruction(line: &str) -> String {
    format!(
        "If someone says they might harm themselves, or that they don't want to live, directly, indirectly or in passing: answer warmly and steadily, keep listening, and stay with them. Never argue, lecture or diagnose; never say you are calling or telling anyone; never promise or mention secrecy; never end the conversation; no jokes, offers or other topics in that answer. Give them this line, word for word, in that answer: \"{line}\" Give no other phone number. When a note says the lamp says that line itself, don't say it: the lamp does, right after your answer."
    )
}

/// The note beside a line [`said_harm`] caught, for the model.
pub const NOTE: &str = "These words may mean the person might harm themselves or not want to live. Answer with care, as your instructions say. The lamp says the line with a place to turn itself, right after your answer: don't say it or any number.";

/// The coordinator's written turn after a care turn was cut or stopped and
/// nothing followed, on the second route: the lamp's line follows it.
pub const STILL_HERE: &str = "(The person said something a moment ago that may mean they might harm themselves, and your answer was cut off. In one or two gentle sentences, say you're still here with them. The lamp says the line with a place to turn itself, right after: don't say it or any number.)";

/// [`STILL_HERE`] on the Live route, which has no line of the lamp's own:
/// the model gives it, as its instructions say.
pub const STILL_HERE_LIVE: &str = "(The person said something a moment ago that may mean they might harm themselves, and your answer was cut off. In one or two gentle sentences, say you're still here with them, and give them the line from your instructions, word for word.)";

/// The most of the care line played or kept: longer than the other fixed
/// lines' 8 s, so the Chinese line's 120 at its end is never cut.
pub const MOST_SAMPLES: usize = 20 * 24_000;

/// Sets the region by hand, two letters (`US`, `VN`, `CN`, …): before
/// anything else (the lamp's set time zone may not say where it is).
pub const REGION_ENVIRONMENT: &str = "LAMP_CARE_REGION";

/// A copy of the lines to read in place of the shipped ones, kept by the
/// safety reader: a line corrected without a release. Missing or broken:
/// the shipped lines.
pub const FILE_ENVIRONMENT: &str = "LAMP_CARE_LINES_FILE";

/// Where [`FILE_ENVIRONMENT`] points when unset.
pub const FILE: &str = "/etc/lampOS/voice/care-lines.json";

impl CareLines {
    /// The lines the lamp says: its installed copy ([`FILE_ENVIRONMENT`])
    /// if it is a good one, else the shipped ones.
    pub fn installed() -> Self {
        let path =
            std::env::var_os(FILE_ENVIRONMENT).unwrap_or_else(|| std::ffi::OsString::from(FILE));
        Self::or_shipped(std::fs::read_to_string(path).ok().as_deref())
    }
}

/// The country of a lamp whose clock is set to `zone` (an IANA name, as
/// setup's time zone sets it), for the regions with a line of their own:
/// none for any other zone.
pub fn country_of_zone(zone: &str) -> Option<&'static str> {
    const US: &[&str] = &[
        "America/New_York",
        "America/Chicago",
        "America/Denver",
        "America/Los_Angeles",
        "America/Phoenix",
        "America/Anchorage",
        "America/Adak",
        "America/Juneau",
        "America/Sitka",
        "America/Metlakatla",
        "America/Nome",
        "America/Yakutat",
        "America/Boise",
        "America/Detroit",
        "America/Menominee",
        "Pacific/Honolulu",
    ];
    const US_PREFIXES: &[&str] = &[
        "America/Indiana/",
        "America/Kentucky/",
        "America/North_Dakota/",
        "US/",
    ];
    let zone = zone.trim();
    if US.contains(&zone) || US_PREFIXES.iter().any(|prefix| zone.starts_with(prefix)) {
        Some("US")
    } else if ["Asia/Ho_Chi_Minh", "Asia/Saigon"].contains(&zone) {
        Some("VN")
    } else if [
        "Asia/Shanghai",
        "Asia/Urumqi",
        "Asia/Chongqing",
        "Asia/Chungking",
        "Asia/Harbin",
        "PRC",
    ]
    .contains(&zone)
    {
        Some("CN")
    } else {
        None
    }
}

/// Where the lamp is, for its line: the hand-set region
/// ([`REGION_ENVIRONMENT`]), else lamp-cloud's placement when it says
/// `CN`, else the country of the lamp's set time zone (`zone`). None when
/// none says: the default line.
pub fn region_from(
    set: Option<&str>,
    placement: Option<&str>,
    zone: Option<&str>,
) -> Option<String> {
    let two = |value: &str| {
        let value = value.trim().to_ascii_uppercase();
        (value.len() == 2 && value.chars().all(|c| c.is_ascii_uppercase())).then_some(value)
    };
    set.and_then(two)
        .or_else(|| placement.and_then(two).filter(|region| region == "CN"))
        .or_else(|| zone.and_then(country_of_zone).map(str::to_owned))
}

/// [`region_from`] as the lamp has it now.
pub fn region_now() -> Option<String> {
    region_from(
        std::env::var(REGION_ENVIRONMENT).ok().as_deref(),
        crate::second_provider::region_from_file().as_deref(),
        crate::local_time::zone_here().as_deref(),
    )
}

/// The line for the lamp as it is now: its region and the owner's language.
pub fn line_now() -> String {
    let language =
        crate::language::from_environment().map_or("en", crate::language::Language::code);
    CareLines::installed()
        .line(region_now().as_deref(), language)
        .to_owned()
}

/// The requests of care turns, whose exchanges never reach the running
/// summary or a talk note (`Thread::held_for`, the safety read's P1 on
/// #7752): the latest few, by request id only.
static HELD_FROM_SUMMARY: std::sync::Mutex<std::collections::VecDeque<u64>> =
    std::sync::Mutex::new(std::collections::VecDeque::new());

/// `request` is a care turn's: its exchange is held from the summary.
pub fn hold_from_summary(request: u64) {
    if let Ok(mut held) = HELD_FROM_SUMMARY.lock()
        && !held.contains(&request)
    {
        if held.len() == 16 {
            held.pop_front();
        }
        held.push_back(request);
    }
}

/// Whether `request`'s exchange is held from the summary ([`hold_from_summary`]).
pub fn held_from_summary(request: u64) -> bool {
    HELD_FROM_SUMMARY
        .lock()
        .is_ok_and(|held| held.contains(&request))
}

/// A care line owed: said after the next answer, whatever ended the turn
/// that owes it (the safety read's and the first builder's P1s on #7752).
#[derive(Clone, Copy, Debug)]
struct Owed {
    at: std::time::Instant,
    asks: bool,
}

/// Where the region comes from: fixed (tests), or as the lamp has it at
/// each line ([`region_now`]).
enum RegionFrom {
    Fixed(Option<String>),
    Now,
}

/// The care of one voice route: the lines, where the lamp is and in what
/// language, the line owed, when it was last said, and its clip rendered
/// ahead. Nothing here keeps a word heard.
pub struct Care {
    lines: CareLines,
    region: RegionFrom,
    language: String,
    owed: std::sync::Mutex<Option<Owed>>,
    said: std::sync::Mutex<Said>,
    last: std::sync::Mutex<Option<std::time::Instant>>,
    /// The line said last, with when, so its own words heard back are not
    /// caught as the person's.
    said_line: std::sync::Mutex<Option<(String, std::time::Instant)>>,
    clip: std::sync::Mutex<Option<(String, Vec<i16>)>>,
}

impl Care {
    /// For a lamp in `region` (fixed), speaking `language`.
    pub fn new(lines: CareLines, region: Option<String>, language: &str) -> Self {
        Self::with(lines, RegionFrom::Fixed(region), language)
    }

    /// For the lamp as it is at each line ([`region_now`]).
    pub fn reading(lines: CareLines, language: &str) -> Self {
        Self::with(lines, RegionFrom::Now, language)
    }

    fn with(lines: CareLines, region: RegionFrom, language: &str) -> Self {
        Self {
            lines,
            region,
            language: language.to_owned(),
            owed: Default::default(),
            said: Default::default(),
            last: Default::default(),
            said_line: Default::default(),
            clip: Default::default(),
        }
    }

    /// The line for the region and language now.
    pub fn line(&self) -> String {
        let region = match &self.region {
            RegionFrom::Fixed(region) => region.clone(),
            RegionFrom::Now => region_now(),
        };
        self.lines
            .line(region.as_deref(), &self.language)
            .to_owned()
    }

    /// The region the line is for now, for the log.
    pub fn region(&self) -> Option<String> {
        match &self.region {
            RegionFrom::Fixed(region) => region.clone(),
            RegionFrom::Now => region_now(),
        }
    }

    /// A care turn at `now` (`asks`: one asking who they can call): the
    /// line is owed, and what nobody asked for is held from now.
    pub fn mark(&self, now: std::time::Instant, asks: bool) {
        if let Ok(mut owed) = self.owed.lock() {
            let asks = asks || owed.is_some_and(|owed| owed.asks);
            *owed = Some(Owed { at: now, asks });
        }
        if let Ok(mut last) = self.last.lock() {
            *last = Some(now);
        }
    }

    /// Whether a line is owed at `now`.
    pub fn owes(&self, now: std::time::Instant) -> bool {
        self.owed.lock().is_ok_and(|owed| {
            owed.is_some_and(|owed| now.saturating_duration_since(owed.at) < AGAIN_AFTER)
        })
    }

    /// At any answer's end at `now`: the line owed, if it is due
    /// ([`Said::is_due`]). One owed longer than [`AGAIN_AFTER`], or not
    /// due, is let go. Still owed until [`Self::said`].
    pub fn owed_line(&self, now: std::time::Instant) -> Option<String> {
        let mut owed = self.owed.lock().ok()?;
        let due = owed.is_some_and(|owing| {
            now.saturating_duration_since(owing.at) < AGAIN_AFTER
                && self
                    .said
                    .lock()
                    .is_ok_and(|said| said.is_due(now, owing.asks))
        });
        if !due {
            *owed = None;
            return None;
        }
        Some(self.line())
    }

    /// `line` was played at `now`: nothing is owed, and it is not said
    /// again within [`AGAIN_AFTER`] unless asked.
    pub fn said(&self, line: &str, now: std::time::Instant) {
        if let Ok(mut owed) = self.owed.lock() {
            *owed = None;
        }
        if let Ok(mut said) = self.said.lock() {
            said.note(now);
        }
        if let Ok(mut said_line) = self.said_line.lock() {
            *said_line = Some((line.to_owned(), now));
        }
    }

    /// Whether `heard` at `now` is the lamp's own line heard back: part of
    /// the line it said in the last 30 seconds (the US line names the
    /// lifeline, which [`said_harm`] would catch).
    pub fn own_line_heard(&self, heard: &str, now: std::time::Instant) -> bool {
        let heard = words(heard);
        !heard.is_empty()
            && self.said_line.lock().is_ok_and(|said_line| {
                said_line.as_ref().is_some_and(|(line, at)| {
                    now.saturating_duration_since(*at) < std::time::Duration::from_secs(30)
                        && format!(" {} ", words(line)).contains(&format!(" {heard} "))
                })
            })
    }

    /// Whether what nobody asked for is still held at `now`: within
    /// [`HOLD`] of the last care turn.
    pub fn holds(&self, now: std::time::Instant) -> bool {
        self.last
            .lock()
            .is_ok_and(|last| last.is_some_and(|at| now.saturating_duration_since(at) < HOLD))
    }

    /// The clip of `line`, if rendered ahead or kept from its last time.
    pub fn clip(&self, line: &str) -> Option<Vec<i16>> {
        self.clip.lock().ok().and_then(|clip| {
            clip.as_ref()
                .filter(|(of, _)| of == line)
                .map(|(_, clip)| clip.clone())
        })
    }

    /// `line`'s clip, said whole: kept for the next time.
    pub fn keep_clip(&self, line: &str, clip: Vec<i16>) {
        if !clip.is_empty()
            && let Ok(mut kept) = self.clip.lock()
        {
            *kept = Some((line.to_owned(), clip));
        }
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
    /// Pieces are joined with a space (the Live route's come with none).
    pub fn heard(&mut self, piece: &str, finished: bool, now: std::time::Instant) -> bool {
        if self.line.len() + piece.len() < 4_096 {
            self.line.push(' ');
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

/// The coordinator's side of a line owed after a cut (the first builder's
/// P1 on #7752): words of self-harm heard, the answer they were in cut or
/// stopped, and nothing after: once the lamp is quiet a few seconds, one
/// written turn ([`STILL_HERE`]) says it's still here, and the line follows.
/// An answer to a later line played whole lets it go (the second route's
/// line follows that answer).
#[derive(Debug, Default)]
pub struct StillHere {
    /// The turn in hand when the words were caught (none: before its reply).
    caught_in: Option<Option<u64>>,
    /// When the answer was cut or stopped.
    cut_at_us: Option<u64>,
}

/// How long after the cut the still-here turn waits, at least.
pub const STILL_HERE_AFTER_US: u64 = 3_000_000;
/// How long after the cut it may still begin: past this, the moment is gone.
pub const STILL_HERE_WITHIN_US: u64 = 120_000_000;

impl StillHere {
    /// Words of self-harm heard, with `turn` the turn in hand.
    pub fn caught(&mut self, turn: Option<u64>) {
        self.caught_in = Some(turn);
        self.cut_at_us = None;
    }

    /// `turn`'s reply ended at `at_us`, `whole` when played to its end.
    pub fn settled(&mut self, turn: u64, whole: bool, at_us: u64) {
        let Some(caught_in) = self.caught_in else {
            return;
        };
        if caught_in.is_some_and(|caught| turn < caught) {
            return;
        }
        if whole {
            *self = Self::default();
        } else {
            self.cut_at_us = Some(at_us);
        }
    }

    /// Muted, asleep or paused: nothing is said, and nothing waits.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Whether the still-here turn begins at `at_us`, with the lamp quiet
    /// and free (`ready`). Once: taken when it begins, let go past
    /// [`STILL_HERE_WITHIN_US`].
    pub fn take(&mut self, at_us: u64, ready: bool) -> bool {
        let Some(cut) = self.cut_at_us else {
            return false;
        };
        let since = at_us.saturating_sub(cut);
        if since >= STILL_HERE_WITHIN_US {
            self.clear();
            return false;
        }
        if ready && since >= STILL_HERE_AFTER_US {
            self.clear();
            return true;
        }
        false
    }
}

/// Phrases, as [`words`] gives them, that ask who they can call or talk
/// to: the line is said again at once.
const ASKS: &[&str] = &[
    "who can i call",
    "who can i talk to",
    "who should i call",
    "who do i call",
    "who do i talk to",
    "what number",
    "whats the number",
    "what is the number",
    "is there a number",
    "hotline",
    "helpline",
    "số nào",
    "gọi cho ai",
    "nên gọi ai",
    "gọi ai bây giờ",
    "nói chuyện với ai",
    "đường dây nóng",
    "goi cho ai",
    "nen goi ai",
    "so nao",
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
    // Direct ("kill myself" and "killing myself" are in [`GUARDED`]).
    "kill my self",
    "end my life",
    "ending my life",
    "end my own life",
    "ending my own life",
    "take my own life",
    "taking my own life",
    "take my life",
    "taking my life",
    "suicide",
    "suicidal",
    "hurt myself",
    "hurting myself",
    "harm myself",
    "harming myself",
    "self harm",
    "cutting myself",
    "cut myself again",
    "cut myself on purpose",
    "hang myself",
    "hanging myself",
    "shoot myself",
    "slit my wrists",
    "want to die",
    "wanna die",
    "want to be dead",
    "going to die tonight",
    "end it all",
    "ending it all",
    "going to end it",
    "gonna end it",
    "want to end it",
    "wanna end it",
    "i will end it",
    "end things",
    "overdose",
    // Indirect.
    "dont want to live",
    "dont wanna live",
    "do not want to live",
    "dont want to be alive",
    "do not want to be alive",
    "dont want to be here anymore",
    "dont want to be here any more",
    "do not want to be here anymore",
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
    "thà chết",
    "chết quách",
    "chết đi cho xong",
    "kết thúc cuộc đời",
    "kết thúc cuộc sống",
    "kết liễu",
    "không muốn sống",
    "chẳng muốn sống",
    "không còn muốn sống",
    "chẳng còn muốn sống",
    "chán sống",
    "không thiết sống",
    "không muốn tồn tại",
    "biến mất mãi mãi",
    "không còn lý do để sống",
    "không có tôi sẽ tốt hơn",
    "không có mình sẽ tốt hơn",
    "không muốn thức dậy nữa",
    "rạch tay",
    "rạch cổ tay",
    "treo cổ",
    "nhảy lầu",
    "muốn nhảy cầu",
    // Vietnamese, the marks dropped. Not "tu tu": unmarked, it is also
    // "từ từ" ("slowly"), said all day; nor "treo co", also "treo cờ".
    "tu sat",
    "khong muon song",
    "khong con muon song",
    "chan song",
    "rach tay",
    "nhay lau",
];

/// Phrases caught unless the word just before or after says otherwise
/// (the first builder's and the safety read's false hits on #7752): the
/// phrase, the words that may not come before it, and the words that may
/// not come after it.
const GUARDED: &[(&str, &[&str], &[&str])] = &[
    // "Trời nóng muốn chết", "đói muốn chết": a hot or hungry day.
    (
        "muốn chết",
        &["nóng", "lạnh", "đói", "no", "khát", "mệt", "cười", "ngủ"],
        &[],
    ),
    (
        "muon chet",
        &["nong", "lanh", "doi", "khat", "cuoi", "ngu"],
        &[],
    ),
    // "I don't want to wake up late".
    (
        "dont want to wake up",
        &[],
        &["late", "early", "at", "before", "so", "too", "yet", "for"],
    ),
    (
        "do not want to wake up",
        &[],
        &["late", "early", "at", "before", "so", "too", "yet", "for"],
    ),
    // "I can't go on Friday", "can't go on the trip".
    ("cant go on", &[], NOT_GOING_ON),
    ("can not go on", &[], NOT_GOING_ON),
    ("cannot go on", &[], NOT_GOING_ON),
    // "I'm killing myself laughing".
    ("killing myself", &[], &["laughing", "lol"]),
    ("kill myself", &[], &["laughing", "lol"]),
];

/// What may not follow "can't go on": a day, a trip, a stage.
const NOT_GOING_ON: &[&str] = &[
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
    "the",
    "a",
    "an",
    "that",
    "this",
    "stage",
    "holiday",
    "vacation",
    "trip",
    "tour",
    "today",
    "tomorrow",
    "time",
    "air",
    "for",
    "at",
    "in",
];

/// `text` as its words: NFC (a transcript may give Vietnamese decomposed),
/// lower-case, letters and digits, apostrophes taken out ("don't" as
/// "dont"), one space between.
fn words(text: &str) -> String {
    use unicode_normalization::UnicodeNormalization as _;
    text.nfc()
        .collect::<String>()
        .to_lowercase()
        .replace(['\'', '’'], "")
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether `phrase` stands in `line` (spaced [`words`]) with neither a
/// word of `not_before` just before it nor one of `not_after` just after.
fn stands(line: &str, phrase: &str, not_before: &[&str], not_after: &[&str]) -> bool {
    let needle = format!(" {phrase} ");
    line.match_indices(&needle).any(|(at, _)| {
        let before = line[..at].rsplit(' ').next().unwrap_or_default();
        let after = line[at + needle.len()..]
            .split(' ')
            .next()
            .unwrap_or_default();
        !not_before.contains(&before) && !not_after.contains(&after)
    })
}

/// Whether the heard line says the person might harm themselves or doesn't
/// want to live, by one of [`PHRASES`] or [`GUARDED`] on word boundaries.
pub fn said_harm(heard: &str) -> bool {
    let line = format!(" {} ", words(heard));
    PHRASES
        .iter()
        .any(|phrase| line.contains(&format!(" {phrase} ")))
        || GUARDED
            .iter()
            .any(|(phrase, before, after)| stands(&line, phrase, before, after))
}

#[cfg(test)]
mod tests;
