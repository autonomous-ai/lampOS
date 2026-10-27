//! A bounded model pass after a conversation, outside the memory socket loop.
//!
//! An adapter is given a frozen selection of source records and proposes dated
//! observations. It cannot edit the store. The owner must recheck the job,
//! person, generation and source identities before committing the whole result
//! in one transaction. In particular, a result that arrives after forgetting
//! must not restore what was forgotten. Previous observations are comparison
//! context, not evidence for a new fact unless their underlying records are
//! included too. All text remains untrusted historical data, never instructions.
//!
//! This keeps the useful V1 safeguards recorded in docs/v1-verdicts.md,
//! HAL-1.18/1.19 and GO-1.3: retain the raw journal, preserve exchanges arriving
//! during a pass, reject an empty model response, and do not turn remembered
//! prose into standing orders. V1's regression tests live in
//! hal/test/test_realtime_memory_window.py, test_realtime_summarizer_health.py
//! and system/agent/migrate_persona/memory_guard_test.go in the parent source
//! tree; they are reading material, not runtime dependencies.
//!
//! There are no threads, locks, network calls or store writes here. A caller
//! drives [`Pass`] with an injected clock. Every [`Model`] method must return
//! promptly without waiting for inference, transport or worker shutdown. A
//! blocking arbitrary callback cannot be timed out by this state machine: a
//! production adapter must provide its own genuinely cancellable or killable
//! worker boundary, and must discard late results from cancelled jobs.

use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, error::Error, fmt, task::Poll};

pub const MAX_SOURCES: usize = 64;
pub const MAX_OBSERVATIONS: usize = 8;
/// Bytes of the serialized [`Input`], including JSON framing and escaping.
pub const MAX_INPUT_BYTES: usize = 32 * 1024;
/// UTF-8 bytes of a kept fact, previous observation or proposed observation.
pub const MAX_FACT_BYTES: usize = 300;
pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;
pub const MAX_TIMEOUT_MS: u64 = 30_000;

const MAX_JOB_BYTES: usize = 128;
const MAX_HEARD_BYTES: usize = 600;
const MAX_SAID_BYTES: usize = 800;
const LAST_UNIX_MS: u64 = 253_402_300_799_999;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Promise,
    Thread,
    Preference,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub id: u64,
    pub at_unix_ms: u64,
    /// `exchange` has heard/said text; `kept` has text only.
    pub kind: String,
    pub heard: String,
    pub said: String,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Previous {
    pub id: u64,
    pub at_unix_ms: u64,
    pub text: String,
    pub category: Category,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub job: String,
    pub person: String,
    pub generation: u64,
    pub source_ids: Vec<u64>,
    pub records: Vec<Evidence>,
    pub previous: Vec<Previous>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub text: String,
    pub category: Category,
    pub source_ids: Vec<u64>,
    pub supersedes: Option<u64>,
    /// #1693: the local day, `YYYY-MM-DD`, a `thread` or `promise` is
    /// about (a wedding on Saturday), from its evidence's day to 60 days
    /// after it. Checked and kept by `Days::tidy_apply`; absent, as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Output {
    pub observations: Vec<Proposal>,
}

/// Deliberately contains no provider body, source text or model output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelError {
    Busy,
    InvalidTimeout,
    InvalidInput(&'static str),
    InvalidOutput(&'static str),
    Empty,
    Failed,
    Cancelled,
    TimedOut,
}

impl fmt::Display for ModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy => formatter.write_str("a memory pass is already active"),
            Self::InvalidTimeout => {
                formatter.write_str("the memory pass timeout is outside 1..=30000 ms")
            }
            Self::InvalidInput(why) => write!(formatter, "invalid memory pass input: {why}"),
            Self::InvalidOutput(why) => write!(formatter, "invalid memory pass output: {why}"),
            Self::Empty => formatter.write_str("the memory model returned no observations"),
            Self::Failed => formatter.write_str("the memory model failed"),
            Self::Cancelled => formatter.write_str("the memory model was cancelled"),
            Self::TimedOut => formatter.write_str("the memory model did not finish in time"),
        }
    }
}

impl Error for ModelError {}

fn person_is_valid(person: &str) -> bool {
    (1..=32).contains(&person.len())
        && person.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
}

fn text_is_valid(text: &str, max_bytes: usize) -> bool {
    text.len() <= max_bytes && !text.trim().is_empty() && !text.chars().any(char::is_control)
}

fn ids_are_valid(ids: &[u64]) -> bool {
    !ids.is_empty()
        && ids.len() <= MAX_SOURCES
        && !ids.contains(&0)
        && ids.iter().collect::<BTreeSet<_>>().len() == ids.len()
}

/// Check bounded shape and references, without claiming the records are true
/// or that their IDs still name the store's current records.
pub fn validate_input(input: &Input) -> Result<(), ModelError> {
    let bad = ModelError::InvalidInput;
    if input.job.is_empty()
        || input.job.len() > MAX_JOB_BYTES
        || input
            .job
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(bad("job must be a bounded nonblank identifier"));
    }
    if !person_is_valid(&input.person) {
        return Err(bad("person must be a valid store person ID"));
    }
    if !ids_are_valid(&input.source_ids) {
        return Err(bad("source IDs must be 1 to 64 distinct positive IDs"));
    }
    if input.records.len() != input.source_ids.len() || input.previous.len() > MAX_SOURCES {
        return Err(bad("record or previous-observation count is invalid"));
    }
    let mut records = BTreeSet::new();
    for record in &input.records {
        if !input.source_ids.contains(&record.id) || !records.insert(record.id) {
            return Err(bad("records must match source IDs exactly once"));
        }
        if record.at_unix_ms > LAST_UNIX_MS {
            return Err(bad("evidence time is outside 1970 through 9999"));
        }
        let valid = match record.kind.as_str() {
            "exchange" => {
                record.text.is_empty()
                    && (record.heard.is_empty() || text_is_valid(&record.heard, MAX_HEARD_BYTES))
                    && (record.said.is_empty() || text_is_valid(&record.said, MAX_SAID_BYTES))
                    && (!record.heard.is_empty() || !record.said.is_empty())
            }
            "kept" => {
                record.heard.is_empty()
                    && record.said.is_empty()
                    && text_is_valid(&record.text, MAX_FACT_BYTES)
            }
            _ => false,
        };
        if !valid {
            return Err(bad("evidence kind, fields or text is invalid"));
        }
    }
    let mut previous = BTreeSet::new();
    for observation in &input.previous {
        if observation.id == 0
            || records.contains(&observation.id)
            || !previous.insert(observation.id)
        {
            return Err(bad(
                "previous IDs must be distinct from each other and the source IDs",
            ));
        }
        if observation.at_unix_ms > LAST_UNIX_MS
            || !text_is_valid(&observation.text, MAX_FACT_BYTES)
        {
            return Err(bad("previous observation time or text is invalid"));
        }
    }
    // Counts and every variable-sized field have been bounded first, so even
    // this temporary encoding cannot grow with an unchecked caller value.
    let bytes = serde_json::to_vec(input).map_err(|_| bad("input could not be encoded"))?;
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(bad("encoded input exceeds 32 KiB"));
    }
    Ok(())
}

/// Validate a complete proposal before any of it is persisted. The store must
/// still verify scope, source existence, job generation and supersession under
/// its own transaction. This cannot determine whether a model's claim follows
/// from the supplied words.
pub fn validate_output(input: &Input, output: &Output) -> Result<(), ModelError> {
    validate_input(input)?;
    if output.observations.is_empty() {
        return Err(ModelError::Empty);
    }
    let bad = ModelError::InvalidOutput;
    if output.observations.len() > MAX_OBSERVATIONS {
        return Err(bad("more than eight observations"));
    }
    let mut replaced = BTreeSet::new();
    for observation in &output.observations {
        if !text_is_valid(&observation.text, MAX_FACT_BYTES) {
            return Err(bad(
                "observation text must be nonblank and at most 300 UTF-8 bytes",
            ));
        }
        if !ids_are_valid(&observation.source_ids)
            || !observation
                .source_ids
                .iter()
                .all(|id| input.source_ids.contains(id))
        {
            return Err(bad(
                "observation sources must name distinct supplied evidence",
            ));
        }
        if let Some(id) = observation.supersedes {
            let previous = input.previous.iter().find(|previous| previous.id == id);
            if previous.is_none_or(|previous| previous.category != observation.category)
                || !replaced.insert(id)
            {
                return Err(bad(
                    "supersession must name one supplied observation of the same category",
                ));
            }
        }
    }
    Ok(())
}

/// Words that make what follows "asked" a question asked ("Asked the lamp
/// what time it is"), or what follows "do you know" one ("Do you know
/// where my keys are?"), for [`only_asked`].
const ASKED_ABOUT: &[&str] = &[
    "what", "what's", "whats", "when", "where", "who", "whom", "whose", "which", "how", "why",
    "whether", "if", "about",
];

/// English words a question opens with, for [`only_asked`].
const ASKS_EN: &[&str] = &[
    "what", "what's", "whats", "when", "where", "who", "whose", "which", "how", "why", "is", "are",
    "do", "does", "did", "can", "could",
];

/// Vietnamese words that make a line a question, for [`only_asked`]: one
/// it opens with ("sao", "ai", "khi nào", "tại sao", "vì sao", "ở đâu"), or
/// one it ends with ("gì", "đâu", "ai", "sao", "nào", "không" after "có").
const ASKS_VI_FIRST: &[&str] = &["sao", "ai", "tại", "vì", "khi", "ở", "bao"];
const ASKS_VI_LAST: &[&str] = &["gì", "đâu", "ai", "sao", "nào", "chưa"];

/// Whether one sentence, its end mark given, asks rather than states.
/// Ending in "?" it asks, unless only a short tag after a comma does and
/// what comes before it opens with no question word ("I love tea, you
/// know?"), and is more than a name ("Mom, where is it?" asks). Ending in "." or "!" it states. With no mark it asks when it
/// opens with a question word, or in Vietnamese ends with one.
fn asks(sentence: &str, mark: Option<char>) -> bool {
    let words = |text: &str| -> Vec<String> {
        text.split_whitespace()
            .map(|word| {
                word.replace('\u{2019}', "'")
                    .trim_matches(|c: char| !(c.is_alphanumeric() || c == '\''))
                    .to_lowercase()
            })
            .filter(|word| !word.is_empty())
            .collect()
    };
    let opens_asking = |words: &[String]| {
        words.first().is_some_and(|first| {
            ASKS_EN.contains(&first.as_str())
                || (ASKS_VI_FIRST.contains(&first.as_str())
                    && match first.as_str() {
                        "tại" | "vì" => words.get(1).is_some_and(|w| w == "sao"),
                        "khi" | "bao" => words.get(1).is_some_and(|w| w == "nào" || w == "giờ"),
                        "ở" => words.get(1).is_some_and(|w| w == "đâu"),
                        _ => true,
                    })
        })
    };
    let ends_asking_vi = |words: &[String]| {
        words.last().is_some_and(|last| {
            ASKS_VI_LAST.contains(&last.as_str())
                || (last == "không" && words[..words.len() - 1].iter().any(|w| w == "có"))
        })
    };
    // "Do you know I like tea?": a statement told as a question (the
    // reviewer-lead's P3 on #3852), what follows the opener is theirs.
    // "Do you know where my keys are?" still asks (the reviewer-lead's P3
    // at 263793ad9): only a known thing that is no question word states.
    let all = words(sentence);
    let told = match all.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["do" | "did", "you", "know", "that", next, ..]
        | ["do" | "did", "you", "know", next, ..]
        | ["you", "know", "that", next, ..]
        | ["you", "know", next, ..] => !ASKED_ABOUT.contains(&next),
        _ => false,
    };
    if told {
        return false;
    }
    match mark {
        Some('?') => match sentence.rsplit_once(',') {
            Some((head, tag)) if words(tag).len() <= 3 && !words(head).is_empty() => {
                let head = words(head);
                // "Mom, where is it?": a name or greeting before the question.
                opens_asking(&head) || (head.len() <= 2 && opens_asking(&words(tag)))
            }
            _ => true,
        },
        Some(_) => false,
        None => opens_asking(&all) || ends_asking_vi(&all),
    }
}

/// Whether what was heard only asks something: its last sentence asks
/// ([`asks`]) and any before it is a greeting or a name of three words or
/// fewer ("Hey, mom. What is my favorite dream?"). A line that also says
/// something of its own ("I work at a library now. Where is it?") states.
fn heard_only_asks(heard: &str) -> bool {
    let mut sentences = vec![];
    let mut start = 0;
    for (at, character) in heard.char_indices() {
        if matches!(character, '.' | '!' | '?') {
            sentences.push((&heard[start..at], Some(character)));
            start = at + character.len_utf8();
        }
    }
    if !heard[start..].trim().is_empty() {
        sentences.push((&heard[start..], None));
    }
    sentences.retain(|(text, _)| text.chars().any(char::is_alphanumeric));
    let Some(((last, mark), before)) = sentences.split_last() else {
        return false;
    };
    asks(last, *mark)
        && before
            .iter()
            .all(|(text, _)| text.split_whitespace().count() <= 3)
}

/// Whether `proposal` keeps a question as a fact, which is left out
/// (finding N10 on #3504: lamp-06c1, 27 Oct, a misheard "What is my
/// favorite dream?" kept as "Asked what their favorite dream is."): its
/// text starts "Asked" and a question word ("Asked what …", "Asked
/// whether …", "Asked about …"; "Asked the lamp to remind them …" is a
/// promise, kept: the reviewer-lead's P3 on #3852), or every cited exchange's heard line only asks
/// ([`heard_only_asks`]) and it cites no kept thing. A statement with a
/// question tag ("I love tea, you know?") states, and is kept.
pub fn only_asked(input: &Input, proposal: &Proposal) -> bool {
    // "Asked" and, within its next four words and before any "to", a
    // question word: "Asked what …", "Asked the lamp whether …" (the
    // reviewer-lead's P3 at 263793ad9); "Asked the lamp to remind them …"
    // is a promise.
    let opening: Vec<String> = proposal
        .text
        .split_whitespace()
        .take(5)
        .map(|word| {
            word.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'')
                .to_lowercase()
        })
        .collect();
    if opening.first().is_some_and(|first| first == "asked")
        && opening[1..]
            .iter()
            .take_while(|word| *word != "to")
            .any(|word| ASKED_ABOUT.contains(&word.as_str()))
    {
        return true;
    }
    let cited: Vec<&Evidence> = input
        .records
        .iter()
        .filter(|record| proposal.source_ids.contains(&record.id))
        .collect();
    !cited.is_empty()
        && cited
            .iter()
            .all(|record| record.kind == "exchange" && heard_only_asks(&record.heard))
}

/// Words that say nothing of what a proposal is about, for
/// [`said_by_the_person`].
const PLAIN: &[&str] = &[
    "the",
    "and",
    "but",
    "for",
    "with",
    "from",
    "about",
    "are",
    "was",
    "were",
    "been",
    "they",
    "them",
    "their",
    "theirs",
    "she",
    "her",
    "his",
    "him",
    "you",
    "your",
    "yours",
    "this",
    "that",
    "these",
    "those",
    "what",
    "which",
    "who",
    "when",
    "where",
    "how",
    "have",
    "has",
    "had",
    "does",
    "did",
    "will",
    "would",
    "should",
    "could",
    "can",
    "like",
    "likes",
    "liked",
    "prefers",
    "prefer",
    "preferred",
    "enjoys",
    "enjoy",
    "wants",
    "want",
    "person",
    "user",
    "they're",
    "is",
    "not",
    "very",
    "more",
    "most",
    "some",
    "any",
    "one",
    "thing",
    "things",
    "sort",
    "kind",
    "của",
    "là",
    "và",
    "có",
    "không",
    "này",
    "đó",
    "thì",
    "mà",
    "những",
    "các",
    "một",
    "cho",
    "với",
    "được",
    "thích",
    "favorite",
    "favourite",
    "favorites",
    "favourites",
    // Assent and small talk that carry nothing, the person's ("yes",
    // "ừ", "vâng") or the lamp's ("nice", "noted", "ồ, hay quá").
    "yes",
    "yeah",
    "yep",
    "sure",
    "okay",
    "fine",
    "right",
    "nice",
    "noted",
    "great",
    "cool",
    "good",
    "lovely",
    "got",
    "thanks",
    "ừ",
    "ừm",
    "vâng",
    "dạ",
    "ồ",
    "hay",
    "quá",
    "nhé",
    "vậy",
    "nhỉ",
];

/// A text's distinctive words: [`crate::search::content_words`] (as a
/// targeted forget picks a fact's words: three characters or more, no
/// common word, a final `s` off, `đ` folded), without the generic ones of
/// [`PLAIN`] ("likes", "prefers", "favourite"…), folded the same way.
pub(crate) fn about_words(text: &str) -> BTreeSet<String> {
    let plain: BTreeSet<String> = PLAIN
        .iter()
        .flat_map(|word| crate::search::content_words(word))
        .collect();
    crate::search::content_words(text)
        .into_iter()
        .filter(|word| !plain.contains(word))
        .collect()
}

/// Whether `proposal` rests on the person's own words. Left out: one with
/// a distinctive word ([`about_words`]) the lamp said in its cited
/// exchanges and nobody heard (finding M on #3504: the lamp's own "green
/// tea's more my sort of thing", and on lamp-06c1, 28 Oct, a recall's echo,
/// "What is my favorite drink?" answered "Hibiscus tea", kept as "Favorite
/// drink is hibiscus tea"). Kept: one sharing a word with what was heard;
/// or, in other words or another language ("phone their mother" from "call
/// mom", "likes green tea" from "tôi thích trà xanh"), one whose heard
/// lines say something themselves (not only "yes", "ừ", "sure") while the
/// lamp's lines say nothing of their own. Otherwise left out: "ừ" to the
/// lamp's "Mình thì thích trà xanh hơn" proposed as "likes green tea", or
/// "sure" to "chamomile could help you sleep" as "drinks herbal tea at
/// night" (the reviewer-lead's HOLD on #3649). Other cited evidence than
/// an exchange counts as heard.
pub fn said_by_the_person(input: &Input, proposal: &Proposal) -> bool {
    let about = about_words(&proposal.text);
    let cited = || {
        input
            .records
            .iter()
            .filter(|record| proposal.source_ids.contains(&record.id))
    };
    let heard: BTreeSet<String> = cited()
        .flat_map(|record| {
            about_words(match record.kind.as_str() {
                "exchange" => &record.heard,
                _ => &record.text,
            })
        })
        .collect();
    let lamps: BTreeSet<String> = cited()
        .filter(|record| record.kind == "exchange")
        .flat_map(|record| about_words(&record.said))
        .collect();
    // A distinctive word only the lamp said: theirs it is not.
    if about
        .iter()
        .any(|word| lamps.contains(word) && !heard.contains(word))
    {
        return false;
    }
    // In their own words, or in other words or another language from
    // what they said, when the lamp said nothing of its own to take them
    // from (the reviewer-lead's HOLD on #3649: "ừ" to the lamp's "Mình
    // thì thích trà xanh hơn" is not "likes green tea"; "tôi thích trà
    // xanh" is).
    !heard.is_disjoint(&about) || (!heard.is_empty() && lamps.is_empty())
}

/// A text's words for [`restates`]: every word, short ones and numbers
/// too, but for [`PLAIN`]'s and a few that only join ("is", "a", "my"…),
/// a final `s` off: "Favorite drink is hibiscus tea" and "Favourite drink:
/// hibiscus tea" are the same; "Orchid species 1" and "Orchid species 2"
/// are not.
fn restate_words(text: &str) -> BTreeSet<String> {
    const JOINING: &[&str] = &[
        "is", "a", "an", "to", "of", "my", "i", "me", "it", "in", "on", "at",
    ];
    let plain: BTreeSet<String> = PLAIN.iter().map(|word| crate::search::text(word)).collect();
    crate::search::text(text)
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty() && !plain.contains(*word) && !JOINING.contains(word))
        .map(|word| word.strip_suffix('s').unwrap_or(word).to_owned())
        .collect()
}

/// Whether `proposal` says again what one of `kept` (the scope's current
/// kept things and observations) says, in the same words but for joining
/// and generic ones ([`restate_words`]): a recall's echo kept again as an
/// observation (lamp-06c1, 28 Oct). A proposal that supersedes an
/// observation is a change, and a dated one may carry its day anew: never
/// a restatement.
pub fn restates(proposal: &Proposal, kept: &[String]) -> bool {
    let words = restate_words(&proposal.text);
    proposal.supersedes.is_none()
        && proposal.on.is_none()
        && !words.is_empty()
        && kept.iter().any(|text| restate_words(text) == words)
}

/// A nonblocking adapter, driven outside the memory server's socket loop.
///
/// `start` may schedule work, `poll` may collect a finished result, and `cancel`
/// must invalidate the current job immediately without waiting for a worker.
/// All three return promptly. The adapter bounds its own resources, makes
/// cancellation idempotent, and never returns a cancelled job's result from a
/// later start. Starting new work must not leave an unbounded pile of cancelled
/// workers behind. These are adapter obligations; [`Pass`] cannot enforce them
/// against a blocking or misbehaving implementation.
pub trait Model {
    fn start(&mut self, input: Input, now_ms: u64) -> Result<(), ModelError>;
    fn poll(&mut self, now_ms: u64) -> Poll<Result<Output, ModelError>>;
    fn cancel(&mut self);
}

struct Active {
    input: Input,
    deadline_ms: u64,
}

/// One active, bounded pass; no automatic retries and no store access.
///
/// Use a monotonic clock. Backwards samples are clamped to the latest sample,
/// so rollback neither underflows nor restores spent time. No state machine
/// can infer elapsed real time from a clock that stops or remains backwards.
pub struct Pass<M: Model> {
    model: M,
    active: Option<Active>,
    timeout_ms: u64,
    latest_ms: u64,
}

impl<M: Model> Pass<M> {
    pub fn new(model: M) -> Self {
        Self {
            model,
            active: None,
            timeout_ms: DEFAULT_TIMEOUT_MS,
            latest_ms: 0,
        }
    }

    pub fn with_timeout(model: M, timeout_ms: u64) -> Result<Self, ModelError> {
        if !(1..=MAX_TIMEOUT_MS).contains(&timeout_ms) {
            return Err(ModelError::InvalidTimeout);
        }
        let mut pass = Self::new(model);
        pass.timeout_ms = timeout_ms;
        Ok(pass)
    }

    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// The frozen identity and evidence of the active pass, for its owner.
    pub fn active_input(&self) -> Option<&Input> {
        self.active.as_ref().map(|active| &active.input)
    }

    pub fn start(&mut self, input: Input, now_ms: u64) -> Result<(), ModelError> {
        if self.active.is_some() {
            return Err(ModelError::Busy);
        }
        validate_input(&input)?;
        self.latest_ms = self.latest_ms.max(now_ms);
        if let Err(error) = self.model.start(input.clone(), self.latest_ms) {
            // A failed start may have partly scheduled work. The adapter must
            // invalidate it without waiting, just as it would a timed-out job.
            self.model.cancel();
            return Err(error);
        }
        self.active = Some(Active {
            input,
            deadline_ms: self.latest_ms.saturating_add(self.timeout_ms),
        });
        Ok(())
    }

    /// Return each terminal result once. Idle, cancelled and already completed
    /// passes are pending and do not poll the adapter. At the deadline timeout
    /// wins even when a late result is ready; no old result becomes a success.
    pub fn poll(&mut self, now_ms: u64) -> Poll<Result<Output, ModelError>> {
        let Some(active) = self.active.as_ref() else {
            return Poll::Pending;
        };
        self.latest_ms = self.latest_ms.max(now_ms);
        if self.latest_ms >= active.deadline_ms {
            self.cancel();
            return Poll::Ready(Err(ModelError::TimedOut));
        }
        match self.model.poll(self.latest_ms) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(result) => {
                let result = result.and_then(|output| {
                    validate_output(&active.input, &output)?;
                    Ok(output)
                });
                self.cancel();
                Poll::Ready(result)
            }
        }
    }

    /// Invalidate the active pass. No completion is subsequently emitted for
    /// it; the caller already knows it cancelled. Returns whether one existed.
    pub fn cancel(&mut self) -> bool {
        if self.active.take().is_some() {
            self.model.cancel();
            true
        } else {
            false
        }
    }
}

impl<M: Model> Drop for Pass<M> {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #1505: a tidy job's input as a later build stores it in `jobs.input`,
    /// with a field this build doesn't know. The same input without it reads;
    /// with it, `days.rs` reads it with this same `from_str` and fails that
    /// request ("stored tidy input is invalid"). No fix yet: whoever makes it
    /// read one release forward flips this to a plain test (Tests).
    #[test]
    #[should_panic(expected = "unknown field")]
    fn an_input_from_a_later_build_is_read() {
        let input = input();
        let text = serde_json::to_string(&input).unwrap();
        assert_eq!(serde_json::from_str::<Input>(&text).unwrap(), input);
        let mut later: serde_json::Value = serde_json::from_str(&text).unwrap();
        later["a_field_from_a_later_build"] = serde_json::json!(true);
        assert_eq!(
            serde_json::from_str::<Input>(&later.to_string()).unwrap(),
            input
        );
    }

    fn input() -> Input {
        Input {
            job: "room-day-1".into(),
            person: "everyone".into(),
            generation: 7,
            source_ids: vec![2],
            records: vec![Evidence {
                id: 2,
                at_unix_ms: 1_791_892_800_000,
                kind: "exchange".into(),
                heard: "I now take my coffee black.".into(),
                said: "Black coffee.".into(),
                text: String::new(),
            }],
            previous: vec![Previous {
                id: 1,
                at_unix_ms: 1_791_806_400_000,
                text: "Someone in this room takes milk in their coffee.".into(),
                category: Category::Preference,
            }],
        }
    }

    fn output() -> Output {
        Output {
            observations: vec![Proposal {
                text: "Someone in this room takes their coffee black.".into(),
                category: Category::Preference,
                source_ids: vec![2],
                supersedes: Some(1),
                on: None,
            }],
        }
    }

    struct Scripted {
        ready_at: Option<u64>,
        result: Result<Output, ModelError>,
        start_error: Option<ModelError>,
        starts: Vec<Input>,
        polls: Vec<u64>,
        cancellations: usize,
    }

    impl Scripted {
        fn ready_at(ready_at: Option<u64>) -> Self {
            Self {
                ready_at,
                result: Ok(output()),
                start_error: None,
                starts: Vec::new(),
                polls: Vec::new(),
                cancellations: 0,
            }
        }
    }

    impl Model for Scripted {
        fn start(&mut self, input: Input, _: u64) -> Result<(), ModelError> {
            self.starts.push(input);
            match self.start_error {
                Some(error) => Err(error),
                None => Ok(()),
            }
        }

        fn poll(&mut self, now_ms: u64) -> Poll<Result<Output, ModelError>> {
            self.polls.push(now_ms);
            if self.ready_at.is_some_and(|ready_at| now_ms >= ready_at) {
                Poll::Ready(self.result.clone())
            } else {
                Poll::Pending
            }
        }

        fn cancel(&mut self) {
            self.cancellations += 1;
            // Keep the scripted result available to prove the Pass itself
            // never polls a cancelled job or emits its completion twice.
        }
    }

    #[test]
    fn categories_and_records_have_strict_round_tripping_json_shapes() {
        assert_eq!(
            serde_json::to_string(&Category::Promise).unwrap(),
            "\"promise\""
        );
        assert_eq!(
            serde_json::to_string(&Category::Thread).unwrap(),
            "\"thread\""
        );
        assert_eq!(
            serde_json::to_string(&Category::Preference).unwrap(),
            "\"preference\""
        );
        assert_eq!(
            serde_json::from_str::<Input>(&serde_json::to_string(&input()).unwrap()).unwrap(),
            input()
        );
        assert_eq!(
            serde_json::from_str::<Output>(&serde_json::to_string(&output()).unwrap()).unwrap(),
            output()
        );
        assert!(serde_json::from_str::<Category>("\"command\"").is_err());
        let mut encoded = serde_json::to_value(output()).unwrap();
        encoded["observations"][0]["instruction"] = serde_json::json!("an extra field");
        assert!(serde_json::from_value::<Output>(encoded).is_err());
    }

    #[test]
    fn ready_results_are_given_once_and_leave_no_active_pass() {
        let mut pass = Pass::new(Scripted::ready_at(Some(10)));
        assert_eq!(pass.poll(0), Poll::Pending);
        pass.start(input(), 10).unwrap();
        assert_eq!(pass.active_input(), Some(&input()));
        assert_eq!(pass.poll(10), Poll::Ready(Ok(output())));
        assert!(!pass.is_active());
        assert_eq!(pass.poll(11), Poll::Pending);
        assert_eq!(pass.poll(12), Poll::Pending);
        assert_eq!(pass.model.polls, [10]);
        assert_eq!(pass.model.cancellations, 1);
    }

    #[test]
    fn delayed_work_is_pending_and_a_second_start_is_refused_without_touching_it() {
        let mut pass = Pass::new(Scripted::ready_at(Some(80)));
        pass.start(input(), 10).unwrap();
        let mut other = input();
        other.job = "room-day-2".into();
        assert_eq!(pass.start(other, 20), Err(ModelError::Busy));
        assert_eq!(pass.model.starts, [input()]);
        assert_eq!(pass.poll(30), Poll::Pending);
        assert_eq!(pass.poll(79), Poll::Pending);
        assert_eq!(pass.poll(80), Poll::Ready(Ok(output())));
    }

    #[test]
    fn stalled_work_is_cancelled_at_the_deadline_without_repeated_cancellation() {
        let mut pass = Pass::new(Scripted::ready_at(None));
        pass.start(input(), 50).unwrap();
        assert_eq!(pass.poll(30_049), Poll::Pending);
        assert_eq!(pass.poll(30_050), Poll::Ready(Err(ModelError::TimedOut)));
        assert_eq!(pass.poll(50_000), Poll::Pending);
        assert_eq!(pass.model.polls, [30_049]);
        assert_eq!(pass.model.cancellations, 1);
    }

    #[test]
    fn a_result_ready_at_or_after_the_deadline_is_never_accepted() {
        for ready_at in [30_000, 30_001] {
            let mut pass = Pass::new(Scripted::ready_at(Some(ready_at)));
            pass.start(input(), 0).unwrap();
            assert_eq!(pass.poll(ready_at), Poll::Ready(Err(ModelError::TimedOut)));
            assert_eq!(pass.poll(ready_at + 1), Poll::Pending);
            assert!(pass.model.polls.is_empty());
        }
    }

    #[test]
    fn explicit_cancellation_ignores_late_completion_and_is_idempotent() {
        let mut pass = Pass::new(Scripted::ready_at(Some(50)));
        pass.start(input(), 0).unwrap();
        assert_eq!(pass.poll(20), Poll::Pending);
        assert!(pass.cancel());
        assert!(!pass.cancel());
        assert_eq!(pass.poll(50), Poll::Pending);
        assert_eq!(pass.poll(100), Poll::Pending);
        assert_eq!(pass.model.polls, [20]);
        assert_eq!(pass.model.cancellations, 1);
    }

    #[test]
    fn a_conforming_adapter_does_not_deliver_an_old_job_to_a_new_pass() {
        struct DelayedJobs {
            current: Option<String>,
            replies: Vec<(u64, String, Output)>,
        }

        impl Model for DelayedJobs {
            fn start(&mut self, input: Input, now_ms: u64) -> Result<(), ModelError> {
                self.current = Some(input.job.clone());
                let mut reply = output();
                reply.observations[0].text = format!("Observation from {}", input.job);
                self.replies.push((now_ms + 50, input.job, reply));
                Ok(())
            }

            fn poll(&mut self, now_ms: u64) -> Poll<Result<Output, ModelError>> {
                let Some(current) = self.current.as_deref() else {
                    return Poll::Pending;
                };
                // An adapter must keep the job identity alongside an external
                // result. The trait's Output alone does not carry identity.
                self.replies.retain(|(_, job, _)| job == current);
                let ready = self.replies.iter().position(|(at, _, _)| *at <= now_ms);
                match ready {
                    Some(at) => Poll::Ready(Ok(self.replies.remove(at).2)),
                    None => Poll::Pending,
                }
            }

            fn cancel(&mut self) {
                self.current = None;
            }
        }

        let mut pass = Pass::new(DelayedJobs {
            current: None,
            replies: Vec::new(),
        });
        pass.start(input(), 0).unwrap();
        assert!(pass.cancel());
        let mut next = input();
        next.job = "room-day-2".into();
        pass.start(next, 20).unwrap();
        assert_eq!(pass.poll(50), Poll::Pending);
        let Poll::Ready(Ok(result)) = pass.poll(70) else {
            panic!("the second job should be ready");
        };
        assert_eq!(result.observations[0].text, "Observation from room-day-2");
    }

    #[test]
    fn dropping_an_active_pass_cancels_it_once() {
        use std::{cell::Cell, rc::Rc};

        struct Stalled(Rc<Cell<usize>>);

        impl Model for Stalled {
            fn start(&mut self, _: Input, _: u64) -> Result<(), ModelError> {
                Ok(())
            }

            fn poll(&mut self, _: u64) -> Poll<Result<Output, ModelError>> {
                Poll::Pending
            }

            fn cancel(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }

        let cancellations = Rc::new(Cell::new(0));
        let mut pass = Pass::new(Stalled(Rc::clone(&cancellations)));
        pass.start(input(), 0).unwrap();
        drop(pass);
        assert_eq!(cancellations.get(), 1);
    }

    #[test]
    fn start_and_poll_failures_end_the_pass_without_retrying() {
        let mut model = Scripted::ready_at(Some(0));
        model.start_error = Some(ModelError::Failed);
        let mut pass = Pass::new(model);
        assert_eq!(pass.start(input(), 0), Err(ModelError::Failed));
        assert_eq!(pass.poll(0), Poll::Pending);
        assert!(!pass.is_active());
        assert_eq!(pass.model.starts.len(), 1);
        assert_eq!(pass.model.cancellations, 1);

        pass.model.start_error = None;
        pass.model.result = Err(ModelError::Failed);
        pass.start(input(), 1).unwrap();
        assert_eq!(pass.poll(1), Poll::Ready(Err(ModelError::Failed)));
        assert_eq!(pass.poll(2), Poll::Pending);
        assert_eq!(pass.model.starts.len(), 2);
        assert_eq!(pass.model.cancellations, 2);
    }

    #[test]
    fn empty_and_malformed_results_end_the_pass_without_becoming_success() {
        let mut model = Scripted::ready_at(Some(0));
        model.result = Ok(Output {
            observations: vec![],
        });
        let mut pass = Pass::new(model);
        pass.start(input(), 0).unwrap();
        assert_eq!(pass.poll(0), Poll::Ready(Err(ModelError::Empty)));
        assert!(!pass.is_active());

        let mut malformed = output();
        malformed.observations[0].source_ids = vec![99];
        pass.model.result = Ok(malformed);
        pass.start(input(), 1).unwrap();
        assert!(matches!(
            pass.poll(1),
            Poll::Ready(Err(ModelError::InvalidOutput(_)))
        ));
        assert!(!pass.is_active());
    }

    #[test]
    fn clock_rollback_does_not_underflow_restore_spent_time_or_reach_the_adapter() {
        let mut pass = Pass::with_timeout(Scripted::ready_at(None), 100).unwrap();
        pass.start(input(), 1_000).unwrap();
        assert_eq!(pass.poll(1_060), Poll::Pending);
        assert_eq!(pass.poll(20), Poll::Pending);
        assert_eq!(pass.poll(1_099), Poll::Pending);
        assert_eq!(pass.poll(1_100), Poll::Ready(Err(ModelError::TimedOut)));
        assert_eq!(pass.model.polls, [1_060, 1_060, 1_099]);
    }

    #[test]
    fn deadline_arithmetic_saturates_and_timeout_settings_are_bounded() {
        for timeout in [0, MAX_TIMEOUT_MS + 1, u64::MAX] {
            assert!(matches!(
                Pass::with_timeout(Scripted::ready_at(None), timeout),
                Err(ModelError::InvalidTimeout)
            ));
        }
        assert!(Pass::with_timeout(Scripted::ready_at(None), 1).is_ok());
        let mut pass = Pass::new(Scripted::ready_at(None));
        pass.start(input(), u64::MAX - 1).unwrap();
        assert_eq!(pass.poll(u64::MAX - 1), Poll::Pending);
        assert_eq!(pass.poll(u64::MAX), Poll::Ready(Err(ModelError::TimedOut)));
    }

    #[test]
    fn bad_input_is_rejected_before_the_model_is_started() {
        let mut pass = Pass::new(Scripted::ready_at(Some(0)));
        let mut bad = input();
        bad.records.clear();
        assert!(matches!(
            pass.start(bad, 0),
            Err(ModelError::InvalidInput(_))
        ));
        assert!(pass.model.starts.is_empty());
        assert_eq!(pass.model.cancellations, 0);
        assert!(!pass.is_active());
    }

    #[test]
    fn input_requires_exact_bounded_source_records_and_separate_previous_ids() {
        assert_eq!(validate_input(&input()), Ok(()));
        let bad_inputs: Vec<Input> = vec![
            Input {
                person: "../other".into(),
                ..input()
            },
            Input {
                person: "Other".into(),
                ..input()
            },
            Input {
                job: "job with spaces".into(),
                ..input()
            },
            Input {
                job: "x".repeat(MAX_JOB_BYTES + 1),
                ..input()
            },
            Input {
                source_ids: vec![],
                records: vec![],
                ..input()
            },
            Input {
                source_ids: vec![0],
                ..input()
            },
            Input {
                source_ids: vec![2, 2],
                records: vec![input().records[0].clone(); 2],
                ..input()
            },
            Input {
                source_ids: vec![3],
                ..input()
            },
            Input {
                records: vec![],
                ..input()
            },
            Input {
                previous: vec![input().previous[0].clone(); 2],
                ..input()
            },
            Input {
                previous: vec![Previous {
                    id: 2,
                    ..input().previous[0].clone()
                }],
                ..input()
            },
            Input {
                previous: vec![Previous {
                    id: 0,
                    ..input().previous[0].clone()
                }],
                ..input()
            },
        ];
        for bad in bad_inputs {
            assert!(matches!(
                validate_input(&bad),
                Err(ModelError::InvalidInput(_))
            ));
        }
        let mut many = input();
        many.source_ids = (1..=MAX_SOURCES as u64 + 1).collect();
        assert!(validate_input(&many).is_err());
        many = input();
        many.previous = (0..=MAX_SOURCES)
            .map(|offset| Previous {
                id: 100 + offset as u64,
                ..input().previous[0].clone()
            })
            .collect();
        assert!(validate_input(&many).is_err());
    }

    #[test]
    fn evidence_shape_text_and_dates_are_checked_before_model_work() {
        let mut good = input();
        good.records[0] = Evidence {
            id: 2,
            at_unix_ms: 0,
            kind: "kept".into(),
            heard: String::new(),
            said: String::new(),
            text: "Cà phê đen".into(),
        };
        assert_eq!(validate_input(&good), Ok(()));
        for record in [
            Evidence {
                kind: "observation".into(),
                ..good.records[0].clone()
            },
            Evidence {
                at_unix_ms: u64::MAX,
                ..good.records[0].clone()
            },
            Evidence {
                heard: "mixed shapes".into(),
                ..good.records[0].clone()
            },
            Evidence {
                text: " ".into(),
                ..good.records[0].clone()
            },
            Evidence {
                text: "one\ntwo".into(),
                ..good.records[0].clone()
            },
            Evidence {
                text: "é".repeat(151),
                ..good.records[0].clone()
            },
            Evidence {
                text: String::new(),
                ..good.records[0].clone()
            },
            Evidence {
                heard: "h".repeat(MAX_HEARD_BYTES + 1),
                ..input().records[0].clone()
            },
            Evidence {
                said: "s".repeat(MAX_SAID_BYTES + 1),
                ..input().records[0].clone()
            },
            Evidence {
                heard: String::new(),
                said: String::new(),
                ..input().records[0].clone()
            },
        ] {
            good.records[0] = record;
            assert!(validate_input(&good).is_err());
        }
    }

    #[test]
    fn the_input_byte_limit_counts_json_escaping_not_just_plain_text() {
        let mut large = input();
        large.previous.clear();
        large.source_ids = (1..=32).collect();
        large.records = large
            .source_ids
            .iter()
            .map(|id| Evidence {
                id: *id,
                at_unix_ms: 0,
                kind: "exchange".into(),
                heard: "h".repeat(400),
                said: "s".repeat(400),
                text: String::new(),
            })
            .collect();
        assert_eq!(validate_input(&large), Ok(()));
        for record in &mut large.records {
            record.heard = "\"".repeat(400);
            record.said = "\\".repeat(400);
        }
        assert!(matches!(
            validate_input(&large),
            Err(ModelError::InvalidInput("encoded input exceeds 32 KiB"))
        ));
    }

    #[test]
    fn proposals_require_known_sources_valid_replacements_and_utf8_byte_caps() {
        assert_eq!(validate_output(&input(), &output()), Ok(()));
        let mut good = output();
        good.observations[0].text = "é".repeat(150);
        assert_eq!(validate_output(&input(), &good), Ok(()));
        for proposal in [
            Proposal {
                text: "é".repeat(151),
                ..output().observations[0].clone()
            },
            Proposal {
                text: " \t ".into(),
                ..output().observations[0].clone()
            },
            Proposal {
                text: "one\ntwo".into(),
                ..output().observations[0].clone()
            },
            Proposal {
                source_ids: vec![],
                ..output().observations[0].clone()
            },
            Proposal {
                source_ids: vec![2, 2],
                ..output().observations[0].clone()
            },
            Proposal {
                source_ids: vec![0],
                ..output().observations[0].clone()
            },
            Proposal {
                source_ids: vec![1],
                ..output().observations[0].clone()
            },
            Proposal {
                supersedes: Some(2),
                ..output().observations[0].clone()
            },
            Proposal {
                supersedes: Some(99),
                ..output().observations[0].clone()
            },
            Proposal {
                category: Category::Promise,
                ..output().observations[0].clone()
            },
        ] {
            assert!(matches!(
                validate_output(
                    &input(),
                    &Output {
                        observations: vec![proposal]
                    }
                ),
                Err(ModelError::InvalidOutput(_))
            ));
        }
        let mut too_many = output();
        too_many.observations = vec![
            Proposal {
                supersedes: None,
                ..output().observations[0].clone()
            };
            MAX_OBSERVATIONS + 1
        ];
        assert!(validate_output(&input(), &too_many).is_err());
        let fork = Output {
            observations: vec![output().observations[0].clone(); 2],
        };
        assert!(validate_output(&input(), &fork).is_err());
    }

    #[test]
    fn errors_never_contain_the_invalid_text() {
        let secret = "private sentence that must not be logged";
        let mut bad = output();
        bad.observations[0].text = secret.repeat(20);
        let error = validate_output(&input(), &bad).unwrap_err();
        assert!(!error.to_string().contains(secret));
        assert!(!format!("{error:?}").contains(secret));
    }

    /// Finding N10 on #3504: a question is never kept as a fact; a
    /// statement, with a question tag or not, is.
    #[test]
    fn a_question_is_only_asked_and_a_statement_is_not() {
        for heard in [
            "What is my favorite dream?",
            "Hey, mom. What is my favorite drink?",
            "Mom, where is it?",
            "where do I work",
            "Can you tell me the time?",
            "Is the shop open on Sunday?",
            "Do you like tea, Tom?",
            "Bạn tên gì",
            "Khi nào cửa hàng mở cửa",
            "Chìa khoá ở đâu",
            "Bạn có thích trà không",
            // The reviewer-lead's P3 at 263793ad9.
            "Do you know where my keys are?",
            "Do you know what my favorite drink is?",
        ] {
            assert!(heard_only_asks(heard), "{heard}");
        }
        for heard in [
            "I love tea, you know?",
            "I work at a library now, right?",
            "I work at a library now. Where is it?",
            "When I was young, I lived in Hue.",
            "My sister's wedding is in June.",
            "I can't stand coffee",
            "Tôi thích cà phê sữa",
            "",
            // The reviewer-lead's P3 on #3852.
            "Do you know I like tea?",
            "Did you know my sister lives in Hanoi?",
            "Do you know that I moved?",
        ] {
            assert!(!heard_only_asks(heard), "{heard}");
        }
        let mut input = input();
        input.records[0].heard = "What is my favorite dream?".into();
        let mut proposal = output().observations.remove(0);
        proposal.supersedes = None;
        assert!(only_asked(&input, &proposal));
        input.records[0].heard = "I love tea, you know?".into();
        assert!(!only_asked(&input, &proposal));
        proposal.text = "Asked what their favorite dream is.".into();
        assert!(only_asked(&input, &proposal));
        for asked in [
            "Asked whether the shop opens on Sunday",
            "Asked the lamp what time it is.",
            "Asked the lamp whether it would rain.",
        ] {
            proposal.text = asked.into();
            assert!(only_asked(&input, &proposal), "{asked}");
        }
        // A promise they asked for is kept (the reviewer-lead's P3 on #3852).
        proposal.text = "Asked the lamp to remind them to call mom on Saturday.".into();
        assert!(!only_asked(&input, &proposal));
    }
}
