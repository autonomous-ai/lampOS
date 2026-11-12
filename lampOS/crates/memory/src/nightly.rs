//! Nightly pass B (the memory owner's extraction plan C, step B, 3 Nov): a
//! model pass, run by the voice once a night for one known person, that
//! fills Days with episodes, facts, profile lines, patterns, what is ahead
//! and feelings, from that person's own records of one local day.
//!
//! The kinds and their rules are Tiers' (`docs/memory-design.md`, "How
//! memory changes", and `docs/memory-tiers.md` on `tiers/recall-adapter`):
//! the model only points at record ids and code copies the quotes, only the
//! person's own records count, a pattern needs two days, supersession keeps
//! history, a plan expires 14 days after its latest source, an appointment
//! closes after its day, the profile is at most 60 lines and 2 KiB, and one
//! night changes at most a quarter of it. What differs is the commit unit:
//! Tiers applied a night whole or not at all, so one bad item threw the
//! night away; here each item is checked alone, a bad one is dropped and
//! counted by reason, and the good ones commit (`nightly_apply`).
//!
//! Items are `entries` rows (`kind='observation'`, a category of
//! [`KINDS`]), so search, provenance, replacements, forgetting and secure
//! deletion are the observations'; their own fields are in
//! `nightly_items`. A job keeps only ids, never text, so a forget during a
//! pass drops only the items resting on what it forgot. No model is called
//! here and nothing is spoken or shown. The contract is
//! `docs/memory-days.md`, "Nightly pass B".
use crate::{
    days::{Days, Error, Result, ensure_person, generation, next_id},
    search,
    store::{Day, Person},
    tidy,
};
use rusqlite::{OptionalExtension, Transaction, params};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use unicode_normalization::UnicodeNormalization;

/// The item kinds, as their `entries.category`, in the order they apply.
pub const KINDS: [&str; 6] = ["episode", "fact", "profile", "pattern", "ahead", "feeling"];
/// What the model returns each kind as.
const ARRAYS: [&str; 6] = [
    "episodes", "facts", "profile", "patterns", "ahead", "feelings",
];
/// Records of one prepare, as Tiers' `prepare_sleep`.
pub const MAX_RECORDS: usize = 128;
/// Bytes of one prepare's encoded input, JSON framing and escaping counted.
pub const MAX_INPUT_BYTES: usize = 64 * 1024;
/// Newest facts, episodes and ahead items given beside the profile lines.
pub const MAX_EXISTING: usize = 32;
/// Items read of each kind; the rest are `too_many`.
pub const MAX_ITEMS_PER_KIND: usize = 32;
pub const MAX_CITES: usize = 8;
pub const PROFILE_LINES: usize = 60;
pub const PROFILE_BYTES: usize = 2048;
/// New profile lines one night may add.
pub const PROFILE_ADDS_A_NIGHT: u64 = 15;
/// A plan lives this many days after its latest source.
pub const PLAN_DAYS: u64 = 14;
/// An end day is at most this many days after its newest source's day.
pub const WITHIN_DAYS: u32 = 60;
/// Live nightly items of one person: superseded, closed or expired ones
/// are history and not counted.
pub const MAX_ITEMS: u64 = 512;
/// Pending nightly jobs of one person.
pub const MAX_PENDING: u64 = 8;
/// The kinds that fade: an episode, a feeling or a pattern is a day's
/// detail, not a lasting fact, so it ages out unused and gives way at
/// [`MAX_ITEMS`] (the Reviewer-lead's P2 on #5097). Facts, profile lines
/// and ahead items keep their own lifetimes.
pub const FADING: [&str; 3] = ["episode", "feeling", "pattern"];
/// A fading item lives this many days after its last use: written,
/// returned by a search or recall, or, for a pattern, seen again. Tiers'
/// 90 days without a recall before an episode's detail fades
/// (`docs/memory-tiers.md`).
pub const FADE_DAYS: u64 = 90;
/// A fading item that may age out or give way, as a condition on
/// `nightly_items n JOIN entries e ON e.rid=n.rid`: live, cited by no
/// live entry (a profile line resting on it keeps it), and resting on no
/// kept thing, directly or through an item (the person asked for that).
const FADES: &str = "n.kind IN ('episode','feeling','pattern') AND e.active=1 AND NOT EXISTS (SELECT 1 FROM sources s JOIN entries c ON c.rid=s.child WHERE s.parent=e.rid AND c.active=1) AND NOT EXISTS (SELECT 1 FROM sources s JOIN entries p ON p.rid=s.parent WHERE s.child=e.rid AND (p.kind='kept' OR EXISTS (SELECT 1 FROM sources s2 JOIN entries q ON q.rid=s2.parent WHERE s2.child=p.rid AND q.kind='kept')))";
pub const PROFILE_KINDS: [&str; 7] = [
    "identity",
    "relationship",
    "taste",
    "work",
    "value",
    "goal",
    "behaviour",
];
/// A `relationship` line is a name and a relation, no private detail.
const RELATIONSHIP_WORDS: usize = 8;
const GIST_BYTES: usize = 300;
const TOPIC_BYTES: usize = 64;
const TAG_BYTES: usize = 64;
const LINE_BYTES: usize = 200;
const LABEL_BYTES: usize = 32;
pub(crate) const DAY_MS: u64 = 86_400_000;
pub(crate) const JOB_MS: u64 = DAY_MS;
/// How many days back a day's pass may still run: the voice's
/// `lamp_voice::nightly::DAYS_BACK`, mirrored (lamp-voice never links this
/// crate) and pinned equal by `tests/nightly_days_back.rs`. A forget marks
/// today and these days before it ([`Days::forget_mark`]).
pub const DAYS_BACK: u32 = 3;
/// After a forget mark, how long the person's lines are still passed over:
/// the forget request is that day's exchange too ("forget what I said
/// about my sister" names the topic again), stamped by the voice at its
/// turn, so a minute takes it whichever end of the turn it was stamped at
/// (Heron's point on #5333). A line said later is read.
pub const FORGET_TURN_MS: u64 = 60_000;

pub(crate) fn bad(message: &'static str) -> Error {
    Error::new("bad_request", message)
}

/// Only a known person's own scope has a nightly pass: never the room, a
/// rules scope or an unknown speaker.
pub(crate) fn known(person: &Person) -> Result<()> {
    if lamp_msgs::known::is_person_id(person.as_str()) {
        Ok(())
    } else {
        Err(Error::new(
            "bad_person",
            "a nightly pass is only for a known person's own memory",
        ))
    }
}

/// A day `days` after the local day of `at`, as `YYYY-MM-DD`.
fn day_after(at: u64, days: u64, local: &crate::store::LocalTime) -> Option<String> {
    Day::at(at.saturating_add(days * DAY_MS), local).map(|day| day.to_string())
}

/// Words and phrases of health, medicine and money, English and
/// Vietnamese: the code backstop of the privacy rule (the product's
/// `OBSERVE_PROMPT` and memory.md: never keep health, money or another
/// person's private matters unless the person asked the lamp to remember
/// them). The model is told the rule; this catches what it lets through.
/// It is a word check, not an understanding: a paraphrase passes it.
const PRIVATE_PREFIXES: &[&str] = &[
    "health",
    "medic",
    "doctor",
    "clinic",
    "diagnos",
    "illness",
    "disease",
    "symptom",
    "therap",
    "pregnan",
    "prescri",
    "cancer",
    "chemo",
    "diabet",
    "depress",
    "anxiet",
    "allerg",
    "injur",
    "psychiatr",
    "psycholog",
    "dentist",
    "insulin",
    "money",
    "salar",
    "debt",
    "loan",
    "mortgage",
    "bank",
    "rent",
    "income",
    "wage",
    "credit",
    "saving",
    "invest",
    "financ",
    "insur",
    "paycheck",
    "dollar",
    "bitcoin",
    "crypto",
];
const PRIVATE_WORDS: &[&str] = &[
    "sick",
    "ill",
    "flu",
    "covid",
    "hiv",
    "pain",
    "pill",
    "pills",
    "nurse",
    "nurses",
    "hospital",
    "hospitals",
    "hospitalized",
    "hospitalised",
    "surgery",
    "surgeon",
    "surgical",
    "rehab",
    "mental",
    "tax",
    "taxes",
    "pay",
    "paid",
    "owe",
    "owes",
    "owed",
    "usd",
    "vnd",
    "stock",
    "stocks",
    "budget",
    "broke",
];
/// Vietnamese, accents kept, matched as whole words or word runs.
const PRIVATE_VI: &[&str] = &[
    "bệnh",
    "thuốc",
    "bác sĩ",
    "ốm",
    "đau",
    "sức khỏe",
    "sức khoẻ",
    "khám",
    "mang thai",
    "có bầu",
    "trầm cảm",
    "lo âu",
    "tiểu đường",
    "ung thư",
    "huyết áp",
    "phẫu thuật",
    "nằm viện",
    "tiền",
    "lương",
    "nợ",
    "vay",
    "ngân hàng",
    "thuế",
    "bảo hiểm",
    "đầu tư",
    "chứng khoán",
    "thu nhập",
];

/// Whether `text` names health, medicine or money ([`PRIVATE_PREFIXES`],
/// [`PRIVATE_WORDS`], [`PRIVATE_VI`], or a currency sign).
pub fn private(text: &str) -> bool {
    if text
        .chars()
        .any(|c| matches!(c, '$' | '€' | '£' | '₫' | '¥'))
    {
        return true;
    }
    let folded: String = text.nfc().collect::<String>().to_lowercase();
    let words: Vec<&str> = folded
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    if words.iter().any(|word| {
        PRIVATE_WORDS.contains(word) || PRIVATE_PREFIXES.iter().any(|p| word.starts_with(p))
    }) {
        return true;
    }
    let joined = format!(" {} ", words.join(" "));
    PRIVATE_VI.iter().any(|phrase| {
        let phrase: String = phrase.nfc().collect();
        joined.contains(&format!(" {phrase} "))
    })
}

/// A record of the job, as it is now.
struct Source {
    rid: i64,
    kind: String,
    at: u64,
    day: String,
    heard: String,
    said: String,
    text: String,
    /// A kept thing, or an item resting on one: the person asked the lamp
    /// to remember it.
    explicit: bool,
    /// An existing item's nightly kind and fields.
    item: Option<(String, Value)>,
}

/// One checked item, ready to write.
struct Item {
    kind: &'static str,
    text: String,
    /// Words beside the text the search index takes (an episode's topic).
    also: String,
    cites: Vec<u64>,
    at: u64,
    supersedes: Option<u64>,
    expires: Option<String>,
    detail: Value,
}

/// Why an item was dropped: the counted keys of `nightly_apply`'s answer.
type Reason = &'static str;

fn text(value: Option<&Value>, cap: usize) -> std::result::Result<String, Reason> {
    let text = value.and_then(Value::as_str).ok_or("shape")?;
    if text.trim().is_empty() || text.len() > cap || text.chars().any(char::is_control) {
        return Err("shape");
    }
    Ok(text.to_owned())
}

fn optional_text(value: Option<&Value>, cap: usize) -> std::result::Result<Option<String>, Reason> {
    match value {
        None | Some(Value::Null) => Ok(None),
        some => text(some, cap).map(Some),
    }
}

fn id(value: Option<&Value>) -> std::result::Result<Option<u64>, Reason> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value.as_u64().filter(|id| *id > 0).map(Some).ok_or("shape"),
    }
}

fn cites(value: Option<&Value>) -> std::result::Result<Vec<u64>, Reason> {
    let list = value.and_then(Value::as_array).ok_or("shape")?;
    let ids: Vec<u64> = list
        .iter()
        .map(|id| id.as_u64().filter(|id| *id > 0))
        .collect::<Option<_>>()
        .ok_or("shape")?;
    if ids.is_empty()
        || ids.len() > MAX_CITES
        || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
    {
        return Err("shape");
    }
    Ok(ids)
}

/// A job's state while it applies.
struct Apply<'a> {
    person: &'a Person,
    local: &'a crate::store::LocalTime,
    /// The ids the job gave, as records and as existing items.
    batch_ids: Vec<u64>,
    existing_ids: Vec<u64>,
    /// What of them is still there.
    sources: BTreeMap<u64, Source>,
    kept: Vec<String>,
    ranges: Vec<(u64, u64)>,
    superseded: BTreeSet<u64>,
    night: String,
    patterns: bool,
    feelings: bool,
}

impl Apply<'_> {
    /// Each cite is the job's and still there; `records_only` refuses an
    /// existing item.
    fn resolve(&self, ids: &[u64], records_only: bool) -> std::result::Result<(), Reason> {
        for id in ids {
            let given =
                self.batch_ids.contains(id) || (!records_only && self.existing_ids.contains(id));
            if !given {
                return Err("unknown_source");
            }
            if !self.sources.contains_key(id) {
                return Err("forgotten");
            }
        }
        Ok(())
    }

    fn newest(&self, ids: &[u64]) -> u64 {
        ids.iter()
            .filter_map(|id| self.sources.get(id))
            .map(|source| source.at)
            .max()
            .unwrap_or(0)
    }

    /// Every cited source is a kept thing the person asked for (or rests
    /// on one); one such source among exchanges is not enough.
    fn explicit(&self, ids: &[u64]) -> bool {
        !ids.is_empty()
            && ids
                .iter()
                .all(|id| self.sources.get(id).is_some_and(|source| source.explicit))
    }

    /// The cited records as a tidy input, existing items as kept evidence,
    /// for the tidy checks of whose words they are.
    fn evidence(&self, ids: &[u64]) -> tidy::Input {
        let records = ids
            .iter()
            .filter_map(|id| self.sources.get(id).map(|source| (id, source)))
            .map(|(id, source)| {
                let exchange = source.kind == "exchange";
                tidy::Evidence {
                    id: *id,
                    at_unix_ms: source.at,
                    kind: if exchange { "exchange" } else { "kept" }.into(),
                    heard: if exchange {
                        source.heard.clone()
                    } else {
                        String::new()
                    },
                    said: if exchange {
                        source.said.clone()
                    } else {
                        String::new()
                    },
                    text: if exchange {
                        String::new()
                    } else {
                        source.text.clone()
                    },
                }
            })
            .collect();
        tidy::Input {
            job: "nightly".into(),
            person: self.person.as_str().into(),
            generation: 0,
            source_ids: ids.to_vec(),
            records,
            previous: vec![],
        }
    }

    /// The tidy checks: the person's own words, no question kept as a
    /// fact, nothing the scope keeps already.
    fn theirs(
        &self,
        text: &str,
        ids: &[u64],
        question: bool,
        supersedes: Option<u64>,
    ) -> std::result::Result<(), Reason> {
        let input = self.evidence(ids);
        let proposal = tidy::Proposal {
            text: text.into(),
            category: tidy::Category::Preference,
            source_ids: ids.to_vec(),
            supersedes,
            on: None,
            replaces: None,
        };
        if !tidy::said_by_the_person(&input, &proposal) {
            return Err("lamp_words");
        }
        if question && tidy::only_asked(&input, &proposal) {
            return Err("question");
        }
        if tidy::restates(&proposal, &self.kept) {
            return Err("restates");
        }
        Ok(())
    }

    fn privacy(&self, texts: &[&str], ids: &[u64]) -> std::result::Result<(), Reason> {
        if !self.explicit(ids) && texts.iter().any(|text| private(text)) {
            return Err("private_kind");
        }
        Ok(())
    }

    /// `until` as an end day: a real day from the newest source's day to
    /// [`WITHIN_DAYS`] after it.
    fn end_day(&self, until: &str, at: u64) -> std::result::Result<String, Reason> {
        let until = Day::parse(until).ok_or("date")?;
        let newest = Day::at(at, self.local).ok_or("date")?;
        let far = until.before(WITHIN_DAYS).is_some_and(|day| day > newest);
        if until < newest || far {
            return Err("date");
        }
        Ok(until.to_string())
    }

    /// The current item `id` of `kind`, older than `at`, to supersede.
    fn current_of(&self, tx: &Transaction<'_>, id: u64, kind: &str, at: u64) -> Result<bool> {
        if self.superseded.contains(&id) {
            return Ok(false);
        }
        Ok(tx
            .query_row(
                "SELECT e.at_unix_ms FROM entries e JOIN nightly_items n ON n.rid=e.rid WHERE e.person=?1 AND e.id=?2 AND e.active=1 AND n.kind=?3",
                params![self.person.as_str(), id, kind],
                |r| r.get::<_, u64>(0),
            )
            .optional()?
            .is_some_and(|older| older <= at))
    }

    fn episode(
        &self,
        tx: &Transaction<'_>,
        item: &Value,
    ) -> Result<std::result::Result<Item, Reason>> {
        let checked = (|| {
            let first = id(item.get("first"))?.ok_or("shape")?;
            let last = id(item.get("last"))?.ok_or("shape")?;
            let topic = text(item.get("topic"), TOPIC_BYTES)?;
            let gist = text(item.get("gist"), GIST_BYTES)?;
            let importance = match item.get("importance") {
                None | Some(Value::Null) => 0.5,
                Some(value) => value
                    .as_f64()
                    .filter(|n| (0.0..=1.0).contains(n))
                    .ok_or("shape")?,
            };
            let feeling = optional_text(item.get("feeling"), TAG_BYTES)?;
            self.resolve(&[first, last], true)?;
            if first > last {
                return Err("range");
            }
            let range: Vec<u64> = self
                .batch_ids
                .iter()
                .copied()
                .filter(|id| (first..=last).contains(id))
                .collect();
            self.resolve(&range, true)?;
            if self
                .ranges
                .iter()
                .any(|(from, to)| first <= *to && *from <= last)
            {
                return Err("range");
            }
            let mut texts = vec![topic.as_str(), gist.as_str()];
            if let Some(feeling) = &feeling {
                texts.push(feeling);
            }
            self.privacy(&texts, &range)?;
            let mut detail =
                json!({"first":first,"last":last,"topic":topic,"importance":importance});
            if self.feelings
                && let Some(feeling) = feeling
            {
                detail["feeling"] = json!(feeling);
            }
            Ok(Item {
                kind: "episode",
                text: gist,
                also: topic,
                at: self.newest(&range),
                cites: range,
                supersedes: None,
                expires: None,
                detail,
            })
        })();
        // An existing episode of this person's overlapping it.
        Ok(match checked {
            Ok(item)
                if overlaps_existing(
                    tx,
                    self.person,
                    item.detail["first"].as_u64(),
                    item.detail["last"].as_u64(),
                )? =>
            {
                Err("range")
            }
            other => other,
        })
    }

    fn fact(
        &self,
        tx: &Transaction<'_>,
        item: &Value,
    ) -> Result<std::result::Result<Item, Reason>> {
        let checked = (|| {
            let label = text(item.get("kind"), LABEL_BYTES)?;
            if !label.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
                return Err("shape");
            }
            let claim = text(item.get("claim"), GIST_BYTES)?;
            let ids = cites(item.get("cites"))?;
            let mut strength = match item.get("strength").map(Value::as_str) {
                None | Some(Some("said")) => 0,
                Some(Some("plan")) => 1,
                Some(Some("maybe")) => 2,
                _ => return Err("shape"),
            };
            let valid_until = optional_text(item.get("valid_until"), 10)?;
            let supersedes = id(item.get("supersedes"))?;
            self.resolve(&ids, false)?;
            let at = self.newest(&ids);
            // Never stronger than its sources: `maybe` over `plan` over `said`.
            for source in ids.iter().filter_map(|id| self.sources.get(id)) {
                if let Some((kind, detail)) = &source.item
                    && kind == "fact"
                {
                    strength = strength.max(match detail["strength"].as_str() {
                        Some("maybe") => 2,
                        Some("plan") => 1,
                        _ => 0,
                    });
                }
            }
            let mut expires = valid_until
                .as_deref()
                .map(|until| self.end_day(until, at))
                .transpose()?;
            if strength == 1 {
                let plan = day_after(at, PLAN_DAYS, self.local).ok_or("date")?;
                expires = Some(expires.map_or(plan.clone(), |until| until.min(plan)));
            }
            self.privacy(&[&claim], &ids)?;
            self.theirs(&claim, &ids, true, supersedes)?;
            let strength = ["said", "plan", "maybe"][strength];
            Ok(Item {
                kind: "fact",
                text: claim,
                also: String::new(),
                cites: ids.clone(),
                at,
                supersedes,
                expires,
                detail: json!({"kind":label,"strength":strength,"quotes":self.quotes(&ids)}),
            })
        })();
        self.superseding(tx, checked)
    }

    fn profile(
        &self,
        tx: &Transaction<'_>,
        item: &Value,
    ) -> Result<std::result::Result<Item, Reason>> {
        let checked = (|| {
            let kind = item.get("kind").and_then(Value::as_str).ok_or("shape")?;
            let kind = PROFILE_KINDS
                .iter()
                .find(|known| **known == kind)
                .ok_or("shape")?;
            let line = text(item.get("text"), LINE_BYTES)?;
            let ids = cites(item.get("cites"))?;
            let supersedes = id(item.get("supersedes"))?;
            self.resolve(&ids, false)?;
            if *kind == "relationship" && line.split_whitespace().count() > RELATIONSHIP_WORDS {
                return Err("private_kind");
            }
            self.privacy(&[&line], &ids)?;
            self.theirs(&line, &ids, true, supersedes)?;
            Ok(Item {
                kind: "profile",
                text: line,
                also: String::new(),
                at: self.newest(&ids),
                cites: ids,
                supersedes,
                expires: None,
                detail: json!({"kind":kind}),
            })
        })();
        self.superseding(tx, checked)
    }

    fn pattern(&self, item: &Value) -> std::result::Result<Item, Reason> {
        if !self.patterns {
            return Err("off");
        }
        let line = text(item.get("text"), GIST_BYTES)?;
        let ids = cites(item.get("cites"))?;
        self.resolve(&ids, false)?;
        let days: BTreeSet<&str> = ids
            .iter()
            .filter_map(|id| self.sources.get(id))
            .map(|source| source.day.as_str())
            .collect();
        if ids.len() < 2 || days.len() < 2 {
            return Err("days");
        }
        self.privacy(&[&line], &ids)?;
        self.theirs(&line, &ids, false, None)?;
        let days: Vec<&str> = days.into_iter().collect();
        Ok(Item {
            kind: "pattern",
            text: line,
            also: String::new(),
            at: self.newest(&ids),
            detail: json!({"days":days,"quotes":self.quotes(&ids)}),
            cites: ids,
            supersedes: None,
            expires: None,
        })
    }

    fn ahead(&self, item: &Value) -> std::result::Result<Item, Reason> {
        let what = match item.get("what").and_then(Value::as_str) {
            Some("plan") => "plan",
            Some("event") => "event",
            _ => return Err("shape"),
        };
        let line = text(item.get("text"), GIST_BYTES)?;
        let until = optional_text(item.get("until"), 10)?;
        let ids = cites(item.get("cites"))?;
        self.resolve(&ids, false)?;
        let at = self.newest(&ids);
        let expires = match until.as_deref() {
            Some(until) => self.end_day(until, at)?,
            None => day_after(at, PLAN_DAYS, self.local).ok_or("date")?,
        };
        self.privacy(&[&line], &ids)?;
        self.theirs(&line, &ids, false, None)?;
        Ok(Item {
            kind: "ahead",
            text: line,
            also: String::new(),
            cites: ids,
            at,
            supersedes: None,
            detail: json!({"what":what,"until":until}),
            expires: Some(expires),
        })
    }

    fn feeling(&self, item: &Value) -> std::result::Result<Item, Reason> {
        if !self.feelings {
            return Err("off");
        }
        let line = text(item.get("text"), LINE_BYTES)?;
        let ids = cites(item.get("cites"))?;
        self.resolve(&ids, true)?;
        self.privacy(&[&line], &ids)?;
        self.theirs(&line, &ids, true, None)?;
        Ok(Item {
            kind: "feeling",
            text: line,
            also: String::new(),
            at: self.newest(&ids),
            detail: json!({"quotes":self.quotes(&ids)}),
            cites: ids,
            supersedes: None,
            expires: None,
        })
    }

    /// The cited records' own words, copied by code: the model never
    /// writes a quote.
    fn quotes(&self, ids: &[u64]) -> Vec<String> {
        ids.iter()
            .filter_map(|id| self.sources.get(id))
            .filter(|source| source.item.is_none())
            .map(|source| {
                if source.kind == "exchange" {
                    source.heard.clone()
                } else {
                    source.text.clone()
                }
            })
            .collect()
    }

    /// A checked item's `supersedes`: a current item of its kind, older
    /// than its evidence, not already superseded in this apply.
    fn superseding(
        &self,
        tx: &Transaction<'_>,
        checked: std::result::Result<Item, Reason>,
    ) -> Result<std::result::Result<Item, Reason>> {
        let item = match checked {
            Ok(item) => item,
            Err(reason) => return Ok(Err(reason)),
        };
        if let Some(old) = item.supersedes
            && !self.current_of(tx, old, item.kind, item.at)?
        {
            return Ok(Err("supersede"));
        }
        Ok(Ok(item))
    }
}

fn overlaps_existing(
    tx: &Transaction<'_>,
    person: &Person,
    first: Option<u64>,
    last: Option<u64>,
) -> Result<bool> {
    let (Some(first), Some(last)) = (first, last) else {
        return Ok(false);
    };
    Ok(tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM nightly_items n JOIN entries e ON e.rid=n.rid WHERE n.person=?1 AND n.kind='episode' AND e.active=1 AND json_extract(n.detail,'$.first')<=?3 AND json_extract(n.detail,'$.last')>=?2)",
        params![person.as_str(), first, last],
        |r| r.get(0),
    )?)
}

/// The person's current profile lines and newest facts, episodes and
/// ahead items, for a prepare.
fn existing(db: &rusqlite::Connection, person: &Person) -> Result<Vec<Value>> {
    let mut statement = db.prepare(
        "SELECT e.id,n.kind,e.day,e.text,n.detail FROM entries e JOIN nightly_items n ON n.rid=e.rid WHERE e.person=?1 AND e.active=1 AND ((n.kind='profile') OR (n.kind IN ('fact','episode','ahead') AND e.rid IN (SELECT e2.rid FROM entries e2 JOIN nightly_items n2 ON n2.rid=e2.rid WHERE e2.person=?1 AND e2.active=1 AND n2.kind IN ('fact','episode','ahead') ORDER BY e2.at_unix_ms DESC,e2.id DESC LIMIT ?2))) ORDER BY CASE n.kind WHEN 'profile' THEN 0 ELSE 1 END,e.id",
    )?;
    let rows = statement
        .query_map(params![person.as_str(), MAX_EXISTING as u64], |r| {
            Ok((
                r.get::<_, u64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows
        .into_iter()
        .map(|(id, kind, day, text, detail)| told(id, &kind, &day, &text, &detail))
        .collect())
}

/// An item as a prepare or a search tells it: its fields beside id, kind,
/// day and text, without its copied quotes.
pub(crate) fn told(id: u64, kind: &str, day: &str, text: &str, detail: &str) -> Value {
    let mut item = json!({"id":id,"kind":kind,"day":day,"text":text});
    if let Ok(Value::Object(fields)) = serde_json::from_str::<Value>(detail) {
        for (name, value) in fields {
            if name == "quotes" {
                continue;
            }
            let name = match (kind, name.as_str()) {
                ("profile", "kind") => "profile_kind".to_owned(),
                ("fact", "kind") => "fact_kind".to_owned(),
                _ => name,
            };
            item[name] = value;
        }
    }
    item
}

/// The night's profile allowance: (lines now, changes left, adds left).
fn allowance(tx: &rusqlite::Connection, person: &Person, night: &str) -> Result<(u64, u64, u64)> {
    let lines = profile_lines(tx, person)?;
    let row: Option<(u64, u64, u64)> = tx
        .query_row(
            "SELECT base,changed,added FROM nightly_nights WHERE person=? AND day=?",
            params![person.as_str(), night],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let (base, changed, added) = row.unwrap_or((lines, 0, 0));
    Ok((
        lines,
        (base / 4).saturating_sub(changed),
        PROFILE_ADDS_A_NIGHT.saturating_sub(added),
    ))
}

fn profile_lines(db: &rusqlite::Connection, person: &Person) -> Result<u64> {
    Ok(db.query_row(
        "SELECT count(*) FROM nightly_items n JOIN entries e ON e.rid=n.rid WHERE n.person=? AND n.kind='profile' AND e.active=1",
        [person.as_str()],
        |r| r.get(0),
    )?)
}

fn job_number(job: &str) -> Result<u64> {
    job.strip_prefix('n')
        .and_then(|n| n.parse::<u64>().ok())
        .filter(|n| *n > 0 && *n < i64::MAX as u64)
        .ok_or_else(|| bad("invalid nightly job ID"))
}

impl Days {
    /// A forget asked by `person` now (`forget`, `forget_last`, the voice's
    /// `forget_mark` for a `forget {about}`; El's option 2 on #5333): the
    /// latest such time of today and of the [`DAYS_BACK`] days before it,
    /// kept so the night passes over their
    /// exchanges of those days said before it, and the forget's own turn
    /// ([`FORGET_TURN_MS`]), as `nightly_prepare` and
    /// `nightly_apply` read it. A topic forget matches kept things and
    /// observations only, so a line said and never kept had nothing to
    /// match and stayed; this keeps the night from keeping it. Only a known
    /// person's own scope has a night: any other scope marks nothing.
    pub fn forget_mark(&mut self, person: &Person) -> Result<()> {
        if !lamp_msgs::known::is_person_id(person.as_str()) {
            return Ok(());
        }
        let Some(today) = Day::at(self.now, &self.local) else {
            return Ok(());
        };
        // Today and each earlier day whose pass may still be pending
        // ([`DAYS_BACK`]), all at the same time: a forget after midnight,
        // or before a late pass, reaches the day the words were said on
        // (Thirteen's P2 on #5345).
        let tx = self.db.transaction()?;
        for back in 0..=DAYS_BACK {
            let Some(day) = today.before(back) else {
                continue;
            };
            tx.execute(
                "INSERT INTO nightly_forgets(person,day,at_unix_ms) VALUES(?1,?2,?3) ON CONFLICT(person,day) DO UPDATE SET at_unix_ms=max(at_unix_ms,?3)",
                params![person.as_str(), day.to_string(), self.now],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// A bounded input of `person`'s eligible records of the local `day`
    /// after `after_id` (`nightly_prepare`), or `None` when there is none.
    pub fn nightly_prepare(
        &mut self,
        person: &Person,
        day: &str,
        after_id: u64,
    ) -> Result<Option<Value>> {
        known(person)?;
        let bad_day = || Error::new("bad_day", "day is YYYY-MM-DD, not after today");
        let asked = Day::parse(day).ok_or_else(bad_day)?;
        let today = Day::at(self.now, &self.local).ok_or_else(bad_day)?;
        if asked > today || after_id >= i64::MAX as u64 {
            return Err(bad_day());
        }
        let day = asked.to_string();
        ensure_person(&self.db, person)?;
        let tx = self.db.transaction()?;
        let state: Option<String> = tx
            .query_row(
                "SELECT state FROM nightly_jobs WHERE person=? AND day=? AND after_id=?",
                params![person.as_str(), day, after_id],
                |r| r.get(0),
            )
            .optional()?;
        if state.as_deref() == Some("done") {
            return Err(Error::new("done", "this night's batch was already applied"));
        }
        let pending: u64 = tx.query_row(
            "SELECT count(*) FROM nightly_jobs WHERE person=? AND state='pending' AND NOT (day=? AND after_id=?)",
            params![person.as_str(), day, after_id],
            |r| r.get(0),
        )?;
        if state.is_none() && pending >= MAX_PENDING {
            return Err(Error::new("busy", "too many nightly jobs are pending"));
        }
        let existing = existing(&tx, person)?;
        let (lines, changes, adds) = allowance(&tx, person, &day)?;
        let generation = generation(&tx, person)?;
        let rows = tx
            .prepare("SELECT id,at_unix_ms,kind,heard,said,text FROM entries WHERE person=?1 AND day=?2 AND id>?3 AND active=1 AND ((kind='exchange' AND trim(heard)!='' AND at_unix_ms>coalesce((SELECT f.at_unix_ms+?4 FROM nightly_forgets f WHERE f.person=?1 AND f.day=?2),-1)) OR kind='kept') ORDER BY id")?
            .query_map(params![person.as_str(), day, after_id, FORGET_TURN_MS], |r| {
                Ok(json!({"id":r.get::<_,u64>(0)?,"at_unix_ms":r.get::<_,u64>(1)?,"kind":r.get::<_,String>(2)?,"heard":r.get::<_,String>(3)?,"said":r.get::<_,String>(4)?,"text":r.get::<_,String>(5)?}))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut input = json!({"job":"n0000000000000000000","person":person.as_str(),"day":day,"generation":generation,"records":[],"existing":existing,"profile_lines":lines,"profile_changes_left":changes,"profile_adds_left":adds,"more":false});
        let mut bytes = serde_json::to_vec(&input)
            .map_err(|_| bad("encoding"))?
            .len();
        let mut records = vec![];
        let mut more = false;
        for row in rows {
            let size = serde_json::to_vec(&row).map_err(|_| bad("encoding"))?.len() + 1;
            if records.len() == MAX_RECORDS || bytes + size > MAX_INPUT_BYTES {
                more = true;
                break;
            }
            bytes += size;
            records.push(row);
        }
        if records.is_empty() {
            return Ok(None);
        }
        let ids: Vec<u64> = records.iter().filter_map(|r| r["id"].as_u64()).collect();
        let existing_ids: Vec<u64> = input["existing"]
            .as_array()
            .map(|items| items.iter().filter_map(|i| i["id"].as_u64()).collect())
            .unwrap_or_default();
        let ids_text = serde_json::to_string(&ids).map_err(|_| bad("encoding"))?;
        let existing_text = serde_json::to_string(&existing_ids).map_err(|_| bad("encoding"))?;
        tx.execute(
            "INSERT INTO nightly_jobs(person,day,after_id,generation,at_unix_ms,ids,existing,state) VALUES(?1,?2,?3,?4,?5,?6,?7,'pending') ON CONFLICT(person,day,after_id) DO UPDATE SET generation=?4,at_unix_ms=?5,ids=?6,existing=?7",
            params![person.as_str(), day, after_id, generation, self.now, ids_text, existing_text],
        )?;
        let job: u64 = tx.query_row(
            "SELECT job FROM nightly_jobs WHERE person=? AND day=? AND after_id=?",
            params![person.as_str(), day, after_id],
            |r| r.get(0),
        )?;
        tx.commit()?;
        input["job"] = json!(format!("n{job}"));
        input["records"] = Value::Array(records);
        input["more"] = json!(more);
        Ok(Some(input))
    }

    /// `nightly_apply`: each item of `proposal` checked alone, the good
    /// ones kept, the bad ones dropped and counted by reason.
    pub fn nightly_apply(
        &mut self,
        person: &Person,
        job: &str,
        proposal: &Value,
        patterns: bool,
        feelings: bool,
    ) -> Result<Value> {
        known(person)?;
        let job = job_number(job)?;
        let now = self.now;
        let tx = self.db.transaction()?;
        let saved: Option<(String, String, String, String, String)> = tx
            .query_row(
                "SELECT state,ids,existing,result,day FROM nightly_jobs WHERE job=? AND person=?",
                params![job, person.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?;
        let Some((state, ids, existing_ids, result, night)) = saved else {
            return Err(Error::new(
                "stale",
                "the nightly job is gone or belongs to another person",
            ));
        };
        if state == "done" {
            return serde_json::from_str(&result)
                .map_err(|_| bad("stored nightly result is invalid"));
        }
        let batch_ids: Vec<u64> =
            serde_json::from_str(&ids).map_err(|_| bad("stored nightly job is invalid"))?;
        let existing_ids: Vec<u64> = serde_json::from_str(&existing_ids)
            .map_err(|_| bad("stored nightly job is invalid"))?;
        // What the job gave, as it is now: a forget since leaves it out.
        let mut sources = BTreeMap::new();
        for id in batch_ids.iter().chain(&existing_ids) {
            let found = tx
                .query_row(
                    "SELECT e.rid,e.kind,e.at_unix_ms,e.day,e.heard,e.said,e.text,n.kind,n.detail,(e.kind='kept' OR EXISTS (SELECT 1 FROM sources s JOIN entries p ON p.rid=s.parent WHERE s.child=e.rid AND p.kind='kept')) FROM entries e LEFT JOIN nightly_items n ON n.rid=e.rid WHERE e.person=?1 AND e.id=?2 AND e.active=1",
                    params![person.as_str(), id],
                    |r| {
                        let kind: Option<String> = r.get(7)?;
                        let detail: Option<String> = r.get(8)?;
                        Ok(Source {
                            rid: r.get(0)?,
                            kind: r.get(1)?,
                            at: r.get(2)?,
                            day: r.get(3)?,
                            heard: r.get(4)?,
                            said: r.get(5)?,
                            text: r.get(6)?,
                            explicit: r.get(9)?,
                            item: kind.map(|kind| {
                                (
                                    kind,
                                    detail
                                        .and_then(|d| serde_json::from_str(&d).ok())
                                        .unwrap_or(Value::Null),
                                )
                            }),
                        })
                    },
                )
                .optional()?;
            // A record given as a record must still be one of the person's
            // own eligible kinds; an existing item, still a nightly item. An
            // exchange said before a forget of its day, one asked during
            // the pass too, is left out as a forgotten one is.
            if let Some(source) = found {
                let fits = if batch_ids.contains(id) {
                    source.item.is_none()
                        && match source.kind.as_str() {
                            "kept" => true,
                            "exchange" => !marked(&tx, person, &source.day, source.at)?,
                            _ => false,
                        }
                } else {
                    source.item.is_some()
                };
                if fits {
                    sources.insert(*id, source);
                }
            }
        }
        let kept: Vec<String> = tx
            .prepare("SELECT text FROM entries WHERE person=?1 AND active=1 AND kind IN ('kept','observation') AND coalesce(category,'')!='talk'")?
            .query_map([person.as_str()], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        tx.execute(
            "INSERT OR IGNORE INTO nightly_nights(person,day,base) VALUES(?1,?2,?3)",
            params![person.as_str(), night, profile_lines(&tx, person)?],
        )?;
        let mut apply = Apply {
            person,
            local: &self.local,
            batch_ids,
            existing_ids,
            sources,
            kept,
            ranges: vec![],
            superseded: BTreeSet::new(),
            night: night.clone(),
            patterns,
            feelings,
        };
        let empty = Map::new();
        let proposal = proposal.as_object().unwrap_or(&empty);
        let mut accepted: BTreeMap<&str, u64> = KINDS.iter().map(|k| (*k, 0)).collect();
        let mut dropped = accepted.clone();
        let mut reasons: BTreeMap<&str, u64> = BTreeMap::new();
        let mut ids = vec![];
        for (kind, array) in KINDS.iter().zip(ARRAYS) {
            let items = proposal
                .get(array)
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default();
            for (n, item) in items.iter().enumerate() {
                let checked = if n >= MAX_ITEMS_PER_KIND {
                    Err("too_many")
                } else if !item.is_object() {
                    Err("shape")
                } else {
                    match *kind {
                        "episode" => apply.episode(&tx, item)?,
                        "fact" => apply.fact(&tx, item)?,
                        "profile" => apply.profile(&tx, item)?,
                        "pattern" => apply.pattern(item),
                        "ahead" => apply.ahead(item),
                        _ => apply.feeling(item),
                    }
                };
                let written = match checked {
                    Ok(item) => write(&tx, &mut apply, item, now)?,
                    Err(reason) => {
                        // A pattern seen again, on new days, is re-confirmed:
                        // the live one it restates is used now.
                        if *kind == "pattern"
                            && reason == "restates"
                            && let Some(text) = item.get("text").and_then(Value::as_str)
                        {
                            seen_again(&tx, person, text, now)?;
                        }
                        Err(reason)
                    }
                };
                match written {
                    Ok(id) => {
                        *accepted.entry(kind).or_default() += 1;
                        ids.push(id);
                    }
                    Err(reason) => {
                        *dropped.entry(kind).or_default() += 1;
                        *reasons.entry(reason).or_default() += 1;
                    }
                }
            }
        }
        let answer =
            json!({"ok":true,"accepted":accepted,"dropped":dropped,"reasons":reasons,"ids":ids});
        tx.execute(
            "UPDATE nightly_jobs SET state='done',ids='[]',existing='[]',result=?2 WHERE job=?1",
            params![job, answer.to_string()],
        )?;
        tx.commit()?;
        let total_dropped: u64 = dropped.values().sum();
        eprintln!(
            "lamp-memory: nightly_apply kept {} item(s), dropped {total_dropped}",
            ids.len()
        );
        Ok(answer)
    }

    /// The person's current nightly items matching `query` (FTS, the
    /// search's query rules), or the newest without one, within the days
    /// `from` to `to` by the item's day: at most `limit` and `max_bytes`
    /// of text, for `search`/`recall` with `nightly:true`.
    pub fn nightly_find(
        &mut self,
        person: &Person,
        query: Option<&str>,
        second: bool,
        days: Option<(String, String)>,
        limit: usize,
        max_bytes: usize,
    ) -> Result<Vec<Value>> {
        let expression = query
            .filter(|q| !q.trim().is_empty())
            .map(|q| search::query_with(q, second));
        if let Some(None) = expression {
            return Ok(vec![]);
        }
        let expression = expression.flatten();
        let (from, to) = days.unzip();
        let sql = if expression.is_some() {
            "SELECT e.id,n.kind,e.day,e.text,n.detail FROM search JOIN entries e ON e.rid=search.rowid JOIN nightly_items n ON n.rid=e.rid WHERE search MATCH ?1 AND e.person=?2 AND e.active=1 AND (?3 IS NULL OR e.day BETWEEN ?3 AND ?4) ORDER BY search.rank,e.at_unix_ms DESC,e.id DESC LIMIT ?5"
        } else {
            "SELECT e.id,n.kind,e.day,e.text,n.detail FROM entries e JOIN nightly_items n ON n.rid=e.rid WHERE ?1 IS NULL AND e.person=?2 AND e.active=1 AND (?3 IS NULL OR e.day BETWEEN ?3 AND ?4) ORDER BY e.at_unix_ms DESC,e.id DESC LIMIT ?5"
        };
        let rows = self
            .db
            .prepare(sql)?
            .query_map(
                params![expression, person.as_str(), from, to, limit.min(50) as u64],
                |r| {
                    Ok((
                        r.get::<_, u64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                    ))
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut room = max_bytes.min(16_384);
        let mut found = vec![];
        for (id, kind, day, text, detail) in rows {
            let item = told(id, &kind, &day, &text, &detail);
            let size = item.to_string().len();
            if size > room {
                break;
            }
            room -= size;
            found.push(item);
        }
        // What a search or recall returns is used: it does not fade.
        let tx = self.db.transaction()?;
        for item in &found {
            tx.execute(
                "UPDATE entries SET used_unix_ms=max(used_unix_ms,?3) WHERE person=?1 AND id=?2",
                params![person.as_str(), item["id"].as_u64(), self.now],
            )?;
        }
        tx.commit()?;
        Ok(found)
    }

    /// A nightly item's kind and day, for an entry a plain search lists.
    pub fn nightly_kind(&self, person: &Person, id: u64) -> Result<Option<(String, String)>> {
        Ok(self
            .db
            .query_row(
                "SELECT n.kind,e.day FROM entries e JOIN nightly_items n ON n.rid=e.rid WHERE e.person=? AND e.id=?",
                params![person.as_str(), id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }
}

/// Writes a checked item, or the reason it cannot be kept (a cap).
fn write(
    tx: &Transaction<'_>,
    apply: &mut Apply<'_>,
    item: Item,
    now: u64,
) -> Result<std::result::Result<u64, Reason>> {
    let person = apply.person;
    let today = Day::at(now, apply.local)
        .ok_or_else(|| Error::new("bad_time", "the timestamp is outside 1970 through 9999"))?
        .to_string();
    // At a cap, the least used fading item of an earlier night gives way
    // ([`FADES`]), so a new item is never starved; with none, `full`.
    loop {
        let items: u64 = tx.query_row(
            "SELECT count(*) FROM nightly_items n JOIN entries e ON e.rid=n.rid WHERE n.person=?1 AND e.active=1 AND (n.expires IS NULL OR n.expires>=?2)",
            params![person.as_str(), today],
            |r| r.get(0),
        )?;
        let observations: u64 = tx.query_row(
            "SELECT count(*) FROM entries WHERE person=? AND kind='observation' AND coalesce(category,'')!='talk'",
            [person.as_str()],
            |r| r.get(0),
        )?;
        if items < MAX_ITEMS && observations < crate::days::MAX_OBSERVATIONS {
            break;
        }
        if !give_way(tx, apply)? {
            return Ok(Err("full"));
        }
    }
    if item.kind == "profile" {
        let (lines, changes, adds) = allowance(tx, person, &apply.night)?;
        let bytes: u64 = tx.query_row(
            "SELECT coalesce(sum(length(CAST(e.text AS BLOB))),0) FROM nightly_items n JOIN entries e ON e.rid=n.rid WHERE n.person=?1 AND n.kind='profile' AND e.active=1",
            [person.as_str()],
            |r| r.get(0),
        )?;
        let old: u64 = match item.supersedes {
            Some(old) => tx.query_row(
                "SELECT length(CAST(text AS BLOB)) FROM entries WHERE person=? AND id=?",
                params![person.as_str(), old],
                |r| r.get(0),
            )?,
            None => 0,
        };
        let within = if item.supersedes.is_some() {
            changes > 0
        } else {
            adds > 0 && lines < PROFILE_LINES as u64
        };
        if !within || bytes - old + item.text.len() as u64 > PROFILE_BYTES as u64 {
            return Ok(Err("profile_cap"));
        }
        tx.execute(
            if item.supersedes.is_some() {
                "UPDATE nightly_nights SET changed=changed+1 WHERE person=? AND day=?"
            } else {
                "UPDATE nightly_nights SET added=added+1 WHERE person=? AND day=?"
            },
            params![person.as_str(), apply.night],
        )?;
    }
    let id = match next_id(tx, person) {
        Ok(id) => id,
        Err(error) if error.code == "full" => return Ok(Err("full")),
        Err(error) => return Err(error),
    };
    let day = Day::at(item.at, apply.local)
        .ok_or_else(|| Error::new("bad_time", "the timestamp is outside 1970 through 9999"))?
        .to_string();
    tx.execute(
        "INSERT INTO entries(person,id,kind,at_unix_ms,day,heard,said,text,category,used_unix_ms,active) VALUES(?1,?2,'observation',?3,?4,'','',?5,?6,?7,1)",
        params![person.as_str(), id, item.at, day, item.text, item.kind, now],
    )?;
    let rid = tx.last_insert_rowid();
    tx.execute(
        "INSERT INTO search(rowid,body) VALUES(?,?)",
        params![rid, search::text(&format!("{} {}", item.also, item.text))],
    )?;
    for cite in &item.cites {
        if let Some(source) = apply.sources.get(cite) {
            tx.execute(
                "INSERT OR IGNORE INTO sources(child,parent) VALUES(?1,?2)",
                params![rid, source.rid],
            )?;
        }
    }
    if let Some(old) = item.supersedes {
        tx.execute(
            "INSERT INTO replacements(newer,older) SELECT ?1,rid FROM entries WHERE person=?2 AND id=?3",
            params![rid, person.as_str(), old],
        )?;
        tx.execute(
            "UPDATE entries SET active=0 WHERE person=? AND id=?",
            params![person.as_str(), old],
        )?;
        apply.superseded.insert(old);
    }
    tx.execute(
        "INSERT INTO nightly_items(rid,person,kind,night,expires,detail) VALUES(?1,?2,?3,?4,?5,?6)",
        params![
            rid,
            person.as_str(),
            item.kind,
            apply.night,
            item.expires,
            item.detail.to_string()
        ],
    )?;
    if item.kind == "episode"
        && let (Some(first), Some(last)) =
            (item.detail["first"].as_u64(), item.detail["last"].as_u64())
    {
        apply.ranges.push((first, last));
    }
    apply.kept.push(item.text);
    Ok(Ok(id))
}

/// At a cap: erases the person's least used fading item ([`FADES`]) of an
/// earlier night that this job did not give, so nothing written tonight or
/// cited by tonight's items goes. False when there is none.
fn give_way(tx: &Transaction<'_>, apply: &mut Apply<'_>) -> Result<bool> {
    let given = serde_json::to_string(&apply.existing_ids).unwrap_or_else(|_| "[]".into());
    let found: Option<(i64, String)> = tx
        .query_row(
            &format!(
                "SELECT e.rid,e.text FROM nightly_items n JOIN entries e ON e.rid=n.rid WHERE n.person=?1 AND n.night!=?2 AND e.id NOT IN (SELECT value FROM json_each(?3)) AND {FADES} ORDER BY e.used_unix_ms,e.at_unix_ms,e.id LIMIT 1"
            ),
            params![apply.person.as_str(), apply.night, given],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((rid, text)) = found else {
        return Ok(false);
    };
    tx.execute("DELETE FROM doomed", [])?;
    tx.execute("INSERT INTO doomed(rid) VALUES(?)", [rid])?;
    crate::days::erase(tx)?;
    tx.execute("DELETE FROM doomed", [])?;
    if let Some(at) = apply.kept.iter().position(|kept| *kept == text) {
        apply.kept.remove(at);
    }
    Ok(true)
}

/// A pattern proposed again that restates a live one of the person's: the
/// live one is re-confirmed, used `now`, so it does not fade.
fn seen_again(tx: &Transaction<'_>, person: &Person, text: &str, now: u64) -> Result<()> {
    let proposal = tidy::Proposal {
        text: text.into(),
        category: tidy::Category::Preference,
        source_ids: vec![],
        supersedes: None,
        on: None,
        replaces: None,
    };
    let live: Vec<(u64, String)> = tx
        .prepare("SELECT e.id,e.text FROM nightly_items n JOIN entries e ON e.rid=n.rid WHERE n.person=?1 AND n.kind='pattern' AND e.active=1")?
        .query_map([person.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, pattern) in live {
        if tidy::restates(&proposal, &[pattern]) {
            tx.execute(
                "UPDATE entries SET used_unix_ms=max(used_unix_ms,?3) WHERE person=?1 AND id=?2",
                params![person.as_str(), id, now],
            )?;
        }
    }
    Ok(())
}

/// A forget's nightly items: every item citing anything going, again for
/// what that takes (a profile line citing a forgotten fact), with its
/// versions; and, past their exchanges' expiry, an item sharing a source
/// exchange with a kept thing or observation going (`origins`, as a talk
/// note). In the caller's transaction after the forget's own walk and
/// before `erase`; never on expiry.
pub(crate) fn with_doomed(tx: &Transaction<'_>) -> Result<()> {
    loop {
        let mut added = tx.execute(
            "INSERT OR IGNORE INTO doomed SELECT DISTINCT s.child FROM sources s JOIN nightly_items n ON n.rid=s.child WHERE s.parent IN (SELECT rid FROM doomed) AND s.child NOT IN (SELECT rid FROM doomed)",
            [],
        )?;
        added += tx.execute(
            "INSERT OR IGNORE INTO doomed SELECT DISTINCT t.rid FROM nightly_items t JOIN origins o ON o.rid=t.rid WHERE t.rid NOT IN (SELECT rid FROM doomed) AND EXISTS (SELECT 1 FROM entries d WHERE d.rid IN (SELECT rid FROM doomed) AND d.person=t.person AND coalesce(d.category,'')!='talk' AND NOT EXISTS (SELECT 1 FROM nightly_items nd WHERE nd.rid=d.rid) AND (d.id=o.origin OR EXISTS (SELECT 1 FROM origins od WHERE od.rid=d.rid AND od.origin=o.origin)))",
            [],
        )?;
        added += tx.execute(
            "INSERT OR IGNORE INTO doomed SELECT r.older FROM replacements r JOIN nightly_items n ON n.rid=r.older WHERE r.newer IN (SELECT rid FROM doomed) AND r.older NOT IN (SELECT rid FROM doomed)",
            [],
        )?;
        added += tx.execute(
            "INSERT OR IGNORE INTO doomed SELECT r.newer FROM replacements r JOIN nightly_items n ON n.rid=r.newer WHERE r.older IN (SELECT rid FROM doomed) AND r.newer NOT IN (SELECT rid FROM doomed)",
            [],
        )?;
        if added == 0 {
            return Ok(());
        }
    }
}

/// Whether a line `person` said at `at` on `day` is passed over: a forget
/// of theirs that day after it, or the forget's own turn
/// ([`Days::forget_mark`], [`FORGET_TURN_MS`]).
fn marked(tx: &rusqlite::Connection, person: &Person, day: &str, at: u64) -> Result<bool> {
    Ok(tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM nightly_forgets WHERE person=?1 AND day=?2 AND at_unix_ms+?4>=?3)",
        params![person.as_str(), day, at, FORGET_TURN_MS],
        |r| r.get(0),
    )?)
}

/// Expiry (`maintain`): an item whose end day is before `today`, or a
/// fading one unused for [`FADE_DAYS`], joins the doomed rows, with the
/// items citing it; nightly jobs older than a day, and night allowances of
/// days long gone, go; and forget marks of days before `oldest`, the first
/// day whose exchanges are still kept.
pub(crate) fn expire(tx: &Transaction<'_>, today: &str, oldest: &str, now: u64) -> Result<()> {
    tx.execute(
        "INSERT OR IGNORE INTO doomed SELECT rid FROM nightly_items WHERE expires IS NOT NULL AND expires<?",
        [today],
    )?;
    // A fading item unused for [`FADE_DAYS`] ages out ([`FADES`]).
    tx.execute(
        &format!("INSERT OR IGNORE INTO doomed SELECT n.rid FROM nightly_items n JOIN entries e ON e.rid=n.rid WHERE e.used_unix_ms<?1 AND {FADES}"),
        [now.saturating_sub(FADE_DAYS * DAY_MS)],
    )?;
    // What cites an item that closes goes with it (a profile line citing a
    // plan fact), as Tiers drops a line whose fact is gone; an exchange that
    // ages out takes none. A weekly chapter stays to its own day: a source's
    // expiry is no correction (a forget still takes it, [`with_doomed`]).
    while tx.execute(
        "INSERT OR IGNORE INTO doomed SELECT DISTINCT s.child FROM sources s JOIN nightly_items n ON n.rid=s.child JOIN nightly_items p ON p.rid=s.parent WHERE s.parent IN (SELECT rid FROM doomed) AND s.child NOT IN (SELECT rid FROM doomed) AND n.kind!='chapter'",
        [],
    )? > 0
    {}
    tx.execute(
        "DELETE FROM nightly_jobs WHERE at_unix_ms<?",
        [now.saturating_sub(JOB_MS)],
    )?;
    let old = Day::parse(today)
        .and_then(|day| day.before(7))
        .map(|day| day.to_string());
    if let Some(old) = old {
        tx.execute("DELETE FROM nightly_nights WHERE day<?", [old])?;
    }
    tx.execute("DELETE FROM nightly_forgets WHERE day<?", [oldest])?;
    Ok(())
}

/// A person's pending nightly jobs, allowances and forget marks, on
/// `forget_all` and `forget_person` (every person for `None`): their
/// exchanges are all gone, so no mark is needed.
pub(crate) fn empty(tx: &Transaction<'_>, person: Option<&Person>) -> Result<()> {
    tx.execute(
        "DELETE FROM nightly_jobs WHERE ?1 IS NULL OR person=?1",
        [person.map(Person::as_str)],
    )?;
    tx.execute(
        "DELETE FROM nightly_nights WHERE ?1 IS NULL OR person=?1",
        [person.map(Person::as_str)],
    )?;
    tx.execute(
        "DELETE FROM nightly_forgets WHERE ?1 IS NULL OR person=?1",
        [person.map(Person::as_str)],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{days_protocol::answer, store::Options};
    use std::{fs, path::PathBuf};

    const NOW: u64 = 1_791_892_800_000;
    const MAI: &str = "p-0123456789abcdef";

    struct Scratch(PathBuf, Days);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("lamp-nightly-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            let store = Days::open(
                Options {
                    dir: dir.clone(),
                    keep_days: 30,
                    local: crate::store::LocalTime::Offset(0),
                },
                NOW,
            )
            .unwrap();
            Self(dir, store)
        }

        fn ask(&mut self, request: Value, now: u64) -> Value {
            serde_json::from_str(&answer(&mut self.1, request.to_string().as_bytes(), now).line)
                .unwrap()
        }

        /// One of Mai's exchanges at `at`, its id.
        fn say(&mut self, at: u64, heard: &str, said: &str) -> u64 {
            self.1
                .append(&Person::new(MAI).unwrap(), at, heard, said, &[])
                .unwrap()
        }

        fn prepare(&mut self, at: u64, now: u64) -> Value {
            let day = Day::at(at, &crate::store::LocalTime::Offset(0))
                .unwrap()
                .to_string();
            self.ask(json!({"op":"nightly_prepare","person":MAI,"day":day}), now)
        }

        fn apply(&mut self, job: &Value, proposal: Value, switches: bool, now: u64) -> Value {
            self.ask(
                json!({"op":"nightly_apply","person":MAI,"job":job,"proposal":proposal,
                    "patterns":switches,"feelings":switches}),
                now,
            )
        }

        fn active(&self, id: u64) -> Option<bool> {
            self.1
                .db
                .query_row(
                    "SELECT active FROM entries WHERE person=? AND id=?",
                    params![MAI, id],
                    |r| r.get(0),
                )
                .optional()
                .unwrap()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A day of Mai's: work, a trip, how she felt; the lamp's own idea.
    fn day(scratch: &mut Scratch, at: u64) -> Vec<u64> {
        vec![
            scratch.say(at, "I work at the city library now.", "Nice."),
            scratch.say(at + 1, "My sister Lan visits on the eighth.", "Lovely."),
            scratch.say(at + 2, "I feel excited about the trip.", "Great."),
            scratch.say(at + 3, "Yes.", "Green tea is more my sort of thing."),
            scratch.say(at + 4, "What is my favourite colour?", "Blue."),
        ]
    }

    #[test]
    fn each_kind_is_accepted_alone_and_a_bad_item_costs_only_itself() {
        let mut s = Scratch::new("kinds");
        let earlier = day(&mut s, NOW - DAY_MS);
        let first = s.prepare(NOW - DAY_MS, NOW);
        let job = first["input"]["job"].clone();
        let made = s.apply(
            &job,
            json!({"episodes":[{"first":earlier[0],"last":earlier[1],"topic":"work","gist":"Mai talked about her library job.","importance":0.6}]}),
            false,
            NOW,
        );
        assert_eq!(made["accepted"]["episode"], 1, "{made}");
        let old_episode = made["ids"][0].as_u64().unwrap();

        let ids = day(&mut s, NOW);
        let input = s.prepare(NOW, NOW + 10);
        assert_eq!(input["input"]["records"].as_array().unwrap().len(), 5);
        assert!(
            input["input"]["existing"]
                .as_array()
                .unwrap()
                .iter()
                .any(|i| i["id"] == old_episode)
        );
        let job = input["input"]["job"].clone();
        let proposal = json!({
            "episodes":[{"first":ids[0],"last":ids[2],"topic":"week","gist":"Work at the library, a sister's visit, a trip.","importance":0.8,"feeling":"excited","extra":"ignored"}],
            "facts":[
                {"kind":"work","claim":"Works at the city library.","cites":[ids[0]],"strength":"said"},
                {"kind":"taste","claim":"Likes green tea.","cites":[ids[3]]}
            ],
            "profile":[{"kind":"relationship","text":"Sister: Lan.","cites":[ids[1]]}],
            "patterns":[{"text":"Talks about the library job often.","cites":[ids[0], old_episode]}],
            "ahead":[{"what":"event","text":"Sister Lan visits.","until":Day::at(NOW + 5 * DAY_MS, &crate::store::LocalTime::Offset(0)).unwrap().to_string(),"cites":[ids[1]]}],
            "feelings":[{"text":"Excited about the trip.","cites":[ids[2]]}],
            "unknown_kind":[{"x":1}]
        });
        let applied = s.apply(&job, proposal.clone(), true, NOW + 20);
        assert_eq!(
            applied["accepted"],
            json!({"ahead":1,"episode":1,"fact":1,"feeling":1,"pattern":1,"profile":1}),
            "{applied}"
        );
        assert_eq!(applied["dropped"]["fact"], 1);
        assert_eq!(applied["reasons"], json!({"lamp_words":1}));
        // The same apply again is the same answer, nothing written twice.
        assert_eq!(s.apply(&job, proposal, true, NOW + 30), applied);
        let fact = applied["ids"][1].as_u64().unwrap();
        let detail: String = s
            .1
            .db
            .query_row(
                "SELECT n.detail FROM nightly_items n JOIN entries e ON e.rid=n.rid WHERE e.person=? AND e.id=?",
                params![MAI, fact],
                |r| r.get(0),
            )
            .unwrap();
        // The quote is the record's own words, copied by code.
        assert!(
            detail.contains("I work at the city library now."),
            "{detail}"
        );
    }

    #[test]
    fn every_invalid_item_is_dropped_with_its_reason() {
        let far = Day::at(NOW + 90 * DAY_MS, &crate::store::LocalTime::Offset(0))
            .unwrap()
            .to_string();
        // Mai's day has ids 1 to 5 ([`day`]) in every fresh store.
        let cases = [
            (
                "shape",
                false,
                json!({"facts":[{"kind":"work","claim":"","cites":[1]}]}),
            ),
            (
                "shape",
                false,
                json!({"facts":[{"kind":"work","claim":"Works.","cites":[]}]}),
            ),
            (
                "shape",
                false,
                json!({"facts":[{"kind":"Work!","claim":"Works at the city library.","cites":[1]}]}),
            ),
            ("shape", false, json!({"facts":["a string"]})),
            (
                "shape",
                false,
                json!({"profile":[{"kind":"worry","text":"Library job.","cites":[1]}]}),
            ),
            (
                "shape",
                false,
                json!({"episodes":[{"first":1,"last":2,"topic":"t","gist":"g","importance":2.0}]}),
            ),
            (
                "unknown_source",
                false,
                json!({"facts":[{"kind":"work","claim":"Works at the city library.","cites":[999]}]}),
            ),
            (
                "lamp_words",
                false,
                json!({"facts":[{"kind":"taste","claim":"Likes green tea.","cites":[4]}]}),
            ),
            (
                "question",
                false,
                json!({"facts":[{"kind":"taste","claim":"Asked what their favourite colour is.","cites":[5]}]}),
            ),
            (
                "private_kind",
                false,
                json!({"facts":[{"kind":"work","claim":"The library salary is low.","cites":[1]}]}),
            ),
            (
                "private_kind",
                false,
                json!({"profile":[{"kind":"relationship","text":"Sister Lan, who visits on the eighth with news of her week.","cites":[2]}]}),
            ),
            (
                "range",
                false,
                json!({"episodes":[{"first":3,"last":1,"topic":"t","gist":"Library."}]}),
            ),
            (
                "range",
                false,
                json!({"episodes":[{"first":1,"last":2,"topic":"t","gist":"Library."},{"first":2,"last":3,"topic":"t","gist":"Library again."}]}),
            ),
            (
                "days",
                true,
                json!({"patterns":[{"text":"Talks about the library.","cites":[1,2]}]}),
            ),
            (
                "date",
                false,
                json!({"ahead":[{"what":"event","text":"Sister Lan visits.","until":far,"cites":[2]}]}),
            ),
            (
                "date",
                false,
                json!({"ahead":[{"what":"event","text":"Sister Lan visits.","until":"2020-01-01","cites":[2]}]}),
            ),
            (
                "supersede",
                false,
                json!({"facts":[{"kind":"work","claim":"Works at the city library.","cites":[1],"supersedes":2}]}),
            ),
            (
                "restates",
                false,
                json!({"facts":[{"kind":"work","claim":"Works at the city library.","cites":[1]},{"kind":"work","claim":"Works at city library.","cites":[1]}]}),
            ),
            (
                "off",
                false,
                json!({"patterns":[{"text":"Talks about the library.","cites":[1,2]}]}),
            ),
            (
                "off",
                false,
                json!({"feelings":[{"text":"Excited about the trip.","cites":[3]}]}),
            ),
        ];
        for (n, (reason, switches, proposal)) in cases.into_iter().enumerate() {
            let mut s = Scratch::new(&format!("reason-{n}"));
            day(&mut s, NOW);
            let input = s.prepare(NOW, NOW + 10);
            let got = s.apply(&input["input"]["job"], proposal, switches, NOW + 20);
            assert_eq!(got["ok"], true, "case {n}: {got}");
            assert!(
                got["reasons"].get(reason).is_some(),
                "case {n} ({reason}): {got}"
            );
        }
        // Past the 32nd item of a kind.
        let mut s = Scratch::new("too-many");
        day(&mut s, NOW);
        let input = s.prepare(NOW, NOW + 10);
        let many: Vec<Value> = (0..33)
            .map(|n| json!({"kind":"work","claim":format!("Works at the city library, desk {n}."),"cites":[1]}))
            .collect();
        let got = s.apply(
            &input["input"]["job"],
            json!({"facts":many}),
            false,
            NOW + 20,
        );
        assert_eq!(got["accepted"]["fact"], 32, "{got}");
        assert_eq!(got["reasons"], json!({"too_many":1}));
        // A proposal that is no object keeps nothing, and the job is done.
        let mut s = Scratch::new("not-object");
        day(&mut s, NOW);
        let input = s.prepare(NOW, NOW + 10);
        let got = s.apply(&input["input"]["job"], json!("nonsense"), false, NOW + 20);
        assert_eq!(got["ids"], json!([]));
        assert_eq!(
            s.ask(
                json!({"op":"nightly_prepare","person":MAI,"day":input["input"]["day"]}),
                NOW + 30
            )["error"],
            "done"
        );
    }

    #[test]
    fn health_or_money_is_kept_only_from_an_explicit_keep() {
        for (heard, vi) in [
            ("My doctor is Dr Hoa.", false),
            ("Bác sĩ của tôi là bà Hoa.", true),
        ] {
            let mut s = Scratch::new(if vi { "private-vi" } else { "private-en" });
            let person = Person::new(MAI).unwrap();
            let said = s.say(NOW, heard, "Okay.");
            let kept = s.1.remember(&person, NOW + 1, heard).unwrap();
            let input = s.prepare(NOW, NOW + 10);
            let claim = if vi {
                "Bác sĩ là bà Hoa."
            } else {
                "Doctor is Dr Hoa."
            };
            let got = s.apply(
                &input["input"]["job"],
                json!({"facts":[
                    {"kind":"person","claim":claim,"cites":[said]},
                    {"kind":"person","claim":format!("{claim} (kept)"),"cites":[kept]}
                ]}),
                false,
                NOW + 20,
            );
            assert_eq!(got["accepted"]["fact"], 1, "{got}");
            assert_eq!(got["reasons"], json!({"private_kind":1}), "{got}");
        }
        for text in [
            "Pays rent monthly",
            "Owes $40",
            "Có nợ ngân hàng",
            "Takes pills daily",
        ] {
            assert!(private(text), "{text}");
        }
        for text in [
            "Works at a library",
            "Likes painting",
            "Thích trà xanh",
            "Takes a taxi",
        ] {
            assert!(!private(text), "{text}");
        }
    }

    #[test]
    fn one_kept_thing_does_not_clear_an_exchange_beside_it() {
        let mut s = Scratch::new("private-mixed");
        let person = Person::new(MAI).unwrap();
        let kept =
            s.1.remember(&person, NOW, "remember my locker code is 4412")
                .unwrap();
        let chemo = s.say(NOW + 1, "My chemo is Tuesday.", "Okay.");
        let input = s.prepare(NOW, NOW + 10);
        let got = s.apply(
            &input["input"]["job"],
            json!({"episodes":[{"first":kept,"last":chemo,"topic":"week","gist":"Mai has chemo on Tuesday.","importance":0.5}]}),
            false,
            NOW + 20,
        );
        assert_eq!(got["accepted"]["episode"], 0, "{got}");
        assert_eq!(got["reasons"], json!({"private_kind":1}), "{got}");
    }

    /// `n` live nightly facts of Mai's, written straight to the store.
    fn fill(s: &mut Scratch, n: u64) -> Vec<u64> {
        let day = Day::at(NOW, &crate::store::LocalTime::Offset(0))
            .unwrap()
            .to_string();
        (0..n)
            .map(|k| {
                let id = 1_000_000 + k;
                s.1.db
                    .execute(
                        "INSERT INTO entries(person,id,kind,at_unix_ms,day,heard,said,text,category,used_unix_ms,active) VALUES(?1,?2,'observation',?3,?4,'','',?5,'fact',?3,1)",
                        params![MAI, id, NOW, day, format!("Filler {k}.")],
                    )
                    .unwrap();
                let rid = s.1.db.last_insert_rowid();
                s.1.db
                    .execute(
                        "INSERT INTO nightly_items(rid,person,kind,night,expires,detail) VALUES(?1,?2,'fact',?3,NULL,'{}')",
                        params![rid, MAI, day],
                    )
                    .unwrap();
                id
            })
            .collect()
    }

    #[test]
    fn the_item_cap_counts_only_live_items() {
        let mut s = Scratch::new("cap");
        let said = s.say(NOW, "I work at the city library now.", "Nice.");
        let input = s.prepare(NOW, NOW + 10);
        let filler = fill(&mut s, MAX_ITEMS);
        let fact =
            json!({"facts":[{"kind":"work","claim":"Works at the city library.","cites":[said]}]});
        let got = s.apply(&input["input"]["job"], fact.clone(), false, NOW + 20);
        assert_eq!(got["reasons"], json!({"full":1}), "512 live items: {got}");
        // Superseded items are history: they no longer count.
        s.1.db
            .execute(
                "UPDATE entries SET active=0 WHERE person=? AND id>=?",
                params![MAI, filler[0]],
            )
            .unwrap();
        let again = s.say(NOW + DAY_MS, "I still work at the city library.", "Nice.");
        let input = s.prepare(NOW + DAY_MS, NOW + DAY_MS + 30);
        let got = s.apply(
            &input["input"]["job"],
            json!({"facts":[{"kind":"work","claim":"Still works at the city library.","cites":[again]}]}),
            false,
            NOW + DAY_MS + 40,
        );
        assert_eq!(got["accepted"]["fact"], 1, "{got}");
        assert_eq!(got["reasons"], json!({}), "{got}");
    }

    /// A live nightly item of Mai's of `kind`, written and last used at
    /// `at`, on that night: its rid.
    fn put(s: &mut Scratch, kind: &str, id: u64, at: u64) -> i64 {
        let day = Day::at(at, &crate::store::LocalTime::Offset(0))
            .unwrap()
            .to_string();
        s.1.db
            .execute(
                "INSERT INTO entries(person,id,kind,at_unix_ms,day,heard,said,text,category,used_unix_ms,active) VALUES(?1,?2,'observation',?3,?4,'','',?5,?6,?3,1)",
                params![MAI, id, at, day, format!("Old {kind} {id}."), kind],
            )
            .unwrap();
        let rid = s.1.db.last_insert_rowid();
        s.1.db
            .execute(
                "INSERT INTO nightly_items(rid,person,kind,night,expires,detail) VALUES(?1,?2,?3,?4,NULL,'{}')",
                params![rid, MAI, kind, day],
            )
            .unwrap();
        rid
    }

    fn live(s: &Scratch) -> u64 {
        s.1.db
            .query_row(
                "SELECT count(*) FROM nightly_items n JOIN entries e ON e.rid=n.rid WHERE e.active=1",
                [],
                |r| r.get(0),
            )
            .unwrap()
    }

    #[test]
    fn at_the_cap_a_new_item_gets_in_and_the_least_used_fading_one_goes() {
        let mut s = Scratch::new("give-way");
        let said = s.say(NOW, "I work at the city library now.", "Nice.");
        let input = s.prepare(NOW, NOW + 10);
        put(&mut s, "episode", 2_000_000, NOW - 50 * DAY_MS);
        put(&mut s, "feeling", 2_000_001, NOW - 10 * DAY_MS);
        let filler = fill(&mut s, MAX_ITEMS - 2);
        assert_eq!(live(&s), MAX_ITEMS);
        let got = s.apply(
            &input["input"]["job"],
            json!({"facts":[{"kind":"work","claim":"Works at the city library.","cites":[said]}]}),
            false,
            NOW + 20,
        );
        assert_eq!(got["accepted"]["fact"], 1, "{got}");
        assert_eq!(got["reasons"], json!({}), "{got}");
        assert_eq!(s.active(2_000_000), None, "the least used episode gave way");
        assert_eq!(s.active(2_000_001), Some(true));
        assert!(filler.iter().all(|id| s.active(*id) == Some(true)));
        assert_eq!(live(&s), MAX_ITEMS);
    }

    #[test]
    fn a_kept_thing_and_what_rests_on_it_never_give_way_or_fade() {
        let mut s = Scratch::new("give-way-kept");
        let person = Person::new(MAI).unwrap();
        let kept =
            s.1.remember(
                &person,
                NOW - 60 * DAY_MS,
                "remember my locker code is 4412",
            )
            .unwrap();
        let kept_rid: i64 =
            s.1.db
                .query_row(
                    "SELECT rid FROM entries WHERE person=? AND id=?",
                    params![MAI, kept],
                    |r| r.get(0),
                )
                .unwrap();
        let resting = put(&mut s, "episode", 2_000_000, NOW - 60 * DAY_MS);
        s.1.db
            .execute(
                "INSERT INTO sources(child,parent) VALUES(?1,?2)",
                params![resting, kept_rid],
            )
            .unwrap();
        put(&mut s, "episode", 2_000_001, NOW - 10 * DAY_MS);
        fill(&mut s, MAX_ITEMS - 2);
        let said = s.say(NOW, "I work at the city library now.", "Nice.");
        let input = s.prepare(NOW, NOW + 10);
        let got = s.apply(
            &input["input"]["job"],
            json!({"facts":[{"kind":"work","claim":"Works at the city library.","cites":[said]}]}),
            false,
            NOW + 20,
        );
        assert_eq!(got["accepted"]["fact"], 1, "{got}");
        assert_eq!(s.active(2_000_001), None, "the unkept episode gave way");
        assert_eq!(s.active(2_000_000), Some(true), "never the kept one's");
        // Nothing else may give way: the next is `full`.
        let more = s.say(NOW + DAY_MS, "I bike to the library.", "Nice.");
        let input = s.prepare(NOW + DAY_MS, NOW + DAY_MS + 10);
        let got = s.apply(
            &input["input"]["job"],
            json!({"facts":[{"kind":"travel","claim":"Bikes to the library.","cites":[more]}]}),
            false,
            NOW + DAY_MS + 20,
        );
        assert_eq!(got["reasons"], json!({"full":1}), "{got}");
        s.1.maintain(NOW + 400 * DAY_MS).unwrap();
        assert_eq!(s.active(2_000_000), Some(true), "nor does it fade");
        assert_eq!(s.active(kept), Some(true));
    }

    #[test]
    fn an_unused_episode_fades_and_a_recalled_one_stays() {
        let mut s = Scratch::new("fade");
        let ids = day(&mut s, NOW);
        let input = s.prepare(NOW, NOW + 10);
        let got = s.apply(
            &input["input"]["job"],
            json!({"episodes":[
                {"first":ids[0],"last":ids[1],"topic":"library","gist":"Mai's new library job and Lan's visit."},
                {"first":ids[2],"last":ids[2],"topic":"trip","gist":"Mai is excited about the trip."}
            ]}),
            false,
            NOW + 20,
        );
        assert_eq!(got["accepted"]["episode"], 2, "{got}");
        let (recalled, unused) = (
            got["ids"][0].as_u64().unwrap(),
            got["ids"][1].as_u64().unwrap(),
        );
        let found = s.ask(
            json!({"op":"recall","person":MAI,"query":"library","nightly":true}),
            NOW + 60 * DAY_MS,
        );
        assert!(
            found["nightly"]
                .as_array()
                .unwrap()
                .iter()
                .any(|i| i["id"] == recalled),
            "{found}"
        );
        s.1.maintain(NOW + (FADE_DAYS - 1) * DAY_MS).unwrap();
        assert_eq!(s.active(unused), Some(true), "not yet {FADE_DAYS} days");
        s.1.maintain(NOW + (FADE_DAYS + 1) * DAY_MS).unwrap();
        assert_eq!(s.active(unused), None, "unused for {FADE_DAYS} days");
        assert_eq!(s.active(recalled), Some(true), "recalled on day 60");
        s.1.maintain(NOW + (60 + FADE_DAYS + 1) * DAY_MS).unwrap();
        assert_eq!(s.active(recalled), None, "and {FADE_DAYS} days after");
    }

    #[test]
    fn a_pattern_seen_again_stays_and_one_not_seen_fades() {
        let mut s = Scratch::new("fade-pattern");
        let first = s.say(NOW - DAY_MS, "I went running this morning.", "Nice.");
        let input = s.prepare(NOW - DAY_MS, NOW);
        let fact = s.apply(
            &input["input"]["job"],
            json!({"facts":[{"kind":"habit","claim":"Went running in the morning.","cites":[first]}]}),
            false,
            NOW,
        )["ids"][0]
            .as_u64()
            .unwrap();
        let pattern = json!({"text":"Goes running most mornings."});
        let propose = |s: &mut Scratch, at: u64| {
            let run = s.say(at, "I went running again this morning.", "Nice.");
            let input = s.prepare(at, at + 10);
            let mut item = pattern.clone();
            item["cites"] = json!([run, fact]);
            s.apply(
                &input["input"]["job"],
                json!({"patterns":[item]}),
                true,
                at + 20,
            )
        };
        let got = propose(&mut s, NOW);
        assert_eq!(got["accepted"]["pattern"], 1, "{got}");
        let kept = got["ids"][0].as_u64().unwrap();
        let again = propose(&mut s, NOW + 60 * DAY_MS);
        assert_eq!(again["reasons"], json!({"restates":1}), "{again}");
        s.1.maintain(NOW + (FADE_DAYS + 1) * DAY_MS).unwrap();
        assert_eq!(s.active(kept), Some(true), "re-confirmed on day 60");
        s.1.maintain(NOW + (60 + FADE_DAYS + 1) * DAY_MS).unwrap();
        assert_eq!(s.active(kept), None, "not seen again since");
    }

    #[test]
    fn with_the_switch_off_nothing_fades_or_gives_way() {
        // No pass ran: the store's own rules hold as before, and no nightly
        // row is made by maintain or a search.
        let mut s = Scratch::new("fade-off");
        let person = Person::new(MAI).unwrap();
        let kept =
            s.1.remember(&person, NOW, "remember my locker code is 4412")
                .unwrap();
        let said = s.say(NOW, "I work at the city library now.", "Nice.");
        let plain = s.ask(
            json!({"op":"search","person":MAI,"query":"locker"}),
            NOW + 1,
        );
        s.1.maintain(NOW + (FADE_DAYS + 1) * DAY_MS).unwrap();
        assert_eq!(s.active(kept), Some(true));
        assert_eq!(s.active(said), None, "an exchange still goes at 30 days");
        let after = s.ask(
            json!({"op":"search","person":MAI,"query":"locker"}),
            NOW + (FADE_DAYS + 1) * DAY_MS,
        );
        assert_eq!(
            plain["entries"].as_array().map(Vec::len),
            after["entries"].as_array().map(Vec::len)
        );
        assert_eq!(live(&s), 0);
    }

    #[test]
    fn only_a_known_persons_own_records_have_a_pass() {
        let mut s = Scratch::new("people");
        let room = Person::everyone();
        s.1.append(&room, NOW, "I work at the library.", "Nice.", &[])
            .unwrap();
        s.say(NOW, "I work at the city library now.", "Nice.");
        let other = Person::new("p-fedcba9876543210").unwrap();
        s.1.append(&other, NOW, "I love jazz.", "Nice.", &[])
            .unwrap();
        let day = Day::at(NOW, &crate::store::LocalTime::Offset(0))
            .unwrap()
            .to_string();
        for person in [json!(null), json!("everyone"), json!("r-0123456789abcdef")] {
            let mut request = json!({"op":"nightly_prepare","day":day});
            if !person.is_null() {
                request["person"] = person;
            }
            assert_eq!(s.ask(request, NOW + 10)["error"], "bad_person");
        }
        let input = s.prepare(NOW, NOW + 10);
        let records = input["input"]["records"].as_array().unwrap();
        assert_eq!(records.len(), 1, "{input}");
        assert!(!input.to_string().contains("jazz"));
        // Another person's id is unknown to Mai's job.
        let got = s.apply(
            &input["input"]["job"],
            json!({"facts":[{"kind":"taste","claim":"Loves jazz.","cites":[1, 2]}]}),
            false,
            NOW + 20,
        );
        assert_eq!(got["reasons"], json!({"unknown_source":1}), "{got}");
        // Someone else's job is stale to Mai.
        let theirs = s.ask(
            json!({"op":"nightly_apply","person":"p-fedcba9876543210","job":input["input"]["job"],"proposal":{}}),
            NOW + 30,
        );
        assert_eq!(theirs["error"], "stale");
    }

    #[test]
    fn a_forget_takes_every_item_resting_on_it() {
        let mut s = Scratch::new("cascade");
        let person = Person::new(MAI).unwrap();
        let ids = day(&mut s, NOW - DAY_MS);
        let kept =
            s.1.remember(&person, NOW - DAY_MS + 9, "I play the cello on Sundays.")
                .unwrap();
        let input = s.prepare(NOW - DAY_MS, NOW);
        let got = s.apply(
            &input["input"]["job"],
            json!({"facts":[
                {"kind":"work","claim":"Works at the city library.","cites":[ids[0]]},
                {"kind":"habit","claim":"Plays cello every Sunday.","cites":[kept]}
            ]}),
            false,
            NOW,
        );
        let (work, cello) = (
            got["ids"][0].as_u64().unwrap(),
            got["ids"][1].as_u64().unwrap(),
        );
        // The next night, a profile line cites the fact.
        let later = s.say(NOW, "The library opens at nine.", "Okay.");
        let input = s.prepare(NOW, NOW + 10);
        assert!(
            input["input"]["existing"]
                .as_array()
                .unwrap()
                .iter()
                .any(|i| i["id"] == work)
        );
        let got = s.apply(
            &input["input"]["job"],
            json!({"profile":[{"kind":"work","text":"Librarian at the city library.","cites":[work, later]}]}),
            false,
            NOW + 20,
        );
        let line = got["ids"][0].as_u64().expect("the profile line");
        // Forgetting the exchange takes the fact, and the line citing it.
        s.ask(json!({"op":"forget","person":MAI,"id":ids[0]}), NOW + 30);
        assert_eq!(s.active(work), None);
        assert_eq!(s.active(line), None);
        assert_eq!(s.active(cello), Some(true));
        // A kept thing forgotten selectively takes the fact made from it.
        s.ask(
            json!({"op":"forget","person":MAI,"id":kept,"selective":true}),
            NOW + 40,
        );
        assert_eq!(s.active(cello), None);
        // forget_all leaves no item and no job. Said past the last
        // forget's turn, so the night reads it ([`FORGET_TURN_MS`]).
        s.say(
            NOW + 40 + FORGET_TURN_MS + 10,
            "I work at the city library now.",
            "Nice.",
        );
        let day = Day::at(NOW, &crate::store::LocalTime::Offset(0))
            .unwrap()
            .to_string();
        // That night's first batch is done; what came after it is a new one.
        let input = s.ask(
            json!({"op":"nightly_prepare","person":MAI,"day":day,"after_id":later}),
            NOW + FORGET_TURN_MS + 60,
        );
        assert_eq!(input["ok"], true, "{input}");
        assert!(input["input"]["job"].is_string(), "{input}");
        s.ask(
            json!({"op":"forget_all","person":MAI}),
            NOW + FORGET_TURN_MS + 70,
        );
        let left: u64 = s
            .1
            .db
            .query_row(
                "SELECT (SELECT count(*) FROM nightly_items)+(SELECT count(*) FROM nightly_jobs)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn a_forget_during_a_pass_drops_what_rests_on_it_and_the_days_earlier_lines() {
        let mut s = Scratch::new("mid-pass");
        let ids = day(&mut s, NOW);
        let input = s.prepare(NOW, NOW + 10);
        // The job keeps ids, never words.
        let job: String =
            s.1.db
                .query_row("SELECT ids||existing FROM nightly_jobs", [], |r| r.get(0))
                .unwrap();
        assert!(!job.contains("library"));
        s.ask(json!({"op":"forget","person":MAI,"id":ids[0]}), NOW + 15);
        let got = s.apply(
            &input["input"]["job"],
            json!({
                "episodes":[{"first":ids[0],"last":ids[2],"topic":"week","gist":"Library and a visit."}],
                "facts":[
                    {"kind":"work","claim":"Works at the city library.","cites":[ids[0]]},
                    {"kind":"person","claim":"Sister Lan visits on the eighth.","cites":[ids[1]]}
                ]
            }),
            false,
            NOW + 20,
        );
        // The forgotten line's episode and fact; and the sister's fact too,
        // its line said that day before the forget (El's option 2 on #5333).
        assert_eq!(got["accepted"]["fact"], 0, "{got}");
        assert_eq!(got["reasons"], json!({"forgotten":3}), "{got}");
    }

    #[test]
    fn a_night_changes_at_most_a_quarter_of_the_profile() {
        const FRUITS: [&str; 16] = [
            "apples", "pears", "plums", "figs", "dates", "limes", "mangoes", "kiwis", "grapes",
            "melons", "cherries", "lychees", "papayas", "guavas", "peaches", "apricots",
        ];
        let mut s = Scratch::new("quarter");
        let ids: Vec<u64> = FRUITS
            .iter()
            .enumerate()
            .map(|(n, fruit)| {
                s.say(
                    NOW - DAY_MS + n as u64,
                    &format!("I like {fruit}."),
                    "Okay.",
                )
            })
            .collect();
        let input = s.prepare(NOW - DAY_MS, NOW);
        let lines: Vec<Value> = FRUITS
            .iter()
            .zip(&ids)
            .map(
                |(fruit, id)| json!({"kind":"taste","text":format!("Likes {fruit}."),"cites":[id]}),
            )
            .collect();
        let got = s.apply(&input["input"]["job"], json!({"profile":lines}), false, NOW);
        // At most 15 new lines a night.
        assert_eq!(got["accepted"]["profile"], 15, "{got}");
        assert_eq!(got["reasons"], json!({"profile_cap":1}));
        let lines: Vec<u64> = got["ids"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_u64)
            .collect();
        // The next night: 15 lines, so 3 may change; the 4th may not.
        let new: Vec<u64> = ["mangosteens", "durians", "rambutans", "starfruit"]
            .iter()
            .enumerate()
            .map(|(n, fruit)| {
                s.say(
                    NOW + n as u64,
                    &format!("I now like {fruit} best."),
                    "Okay.",
                )
            })
            .collect();
        let input = s.prepare(NOW, NOW + 10);
        assert_eq!(input["input"]["profile_changes_left"], 3);
        let changes: Vec<Value> = ["mangosteens", "durians", "rambutans", "starfruit"]
            .iter()
            .zip(&new)
            .zip(&lines)
            .map(|((fruit, id), old)| json!({"kind":"taste","text":format!("Likes {fruit} best."),"cites":[id],"supersedes":old}))
            .collect();
        let got = s.apply(
            &input["input"]["job"],
            json!({"profile":changes}),
            false,
            NOW + 20,
        );
        assert_eq!(got["accepted"]["profile"], 3, "{got}");
        assert_eq!(got["reasons"], json!({"profile_cap":1}));
        // History stays: the changed lines are inactive, the 4th current.
        assert_eq!(s.active(lines[0]), Some(false));
        assert_eq!(s.active(lines[3]), Some(true));
    }

    #[test]
    fn a_plan_expires_after_fourteen_days_and_an_event_after_its_day() {
        let mut s = Scratch::new("expiry");
        let ids = day(&mut s, NOW);
        let input = s.prepare(NOW, NOW + 10);
        let local = crate::store::LocalTime::Offset(0);
        let until = Day::at(NOW + 2 * DAY_MS, &local).unwrap().to_string();
        let got = s.apply(
            &input["input"]["job"],
            json!({
                "facts":[
                    {"kind":"plan","claim":"Plans to apply at the city library archive.","cites":[ids[0]],"strength":"plan"},
                    {"kind":"work","claim":"Works at the city library.","cites":[ids[0]]}
                ],
                "profile":[{"kind":"relationship","text":"Sister: Lan.","cites":[ids[1]]}],
                "ahead":[{"what":"event","text":"Sister Lan visits.","until":until,"cites":[ids[1]]}]
            }),
            false,
            NOW + 20,
        );
        assert_eq!(got["reasons"], json!({}), "{got}");
        let [plan, said, line, event] = [0, 1, 2, 3].map(|n| got["ids"][n].as_u64().unwrap());
        s.ask(json!({"op":"status","person":MAI}), NOW + 3 * DAY_MS);
        assert_eq!(s.active(event), None, "an appointment closes after its day");
        assert_eq!(s.active(plan), Some(true));
        s.ask(json!({"op":"status","person":MAI}), NOW + 15 * DAY_MS);
        assert_eq!(
            s.active(plan),
            None,
            "a plan expires 14 days after its source"
        );
        // Not by age, nor with their exchanges (30 days here).
        s.ask(json!({"op":"status","person":MAI}), NOW + 45 * DAY_MS);
        assert_eq!(s.active(ids[0]), None);
        assert_eq!(s.active(said), Some(true));
        assert_eq!(s.active(line), Some(true));
    }

    #[test]
    fn a_superseded_fact_is_history_and_items_are_found_by_search_and_recall() {
        let mut s = Scratch::new("surface");
        let ids = day(&mut s, NOW - DAY_MS);
        let input = s.prepare(NOW - DAY_MS, NOW);
        let got = s.apply(
            &input["input"]["job"],
            json!({
                "episodes":[{"first":ids[0],"last":ids[1],"topic":"library","gist":"Mai's new library job and Lan's visit."}],
                "facts":[{"kind":"work","claim":"Works at the city library.","cites":[ids[0]]}],
                "profile":[{"kind":"relationship","text":"Sister: Lan.","cites":[ids[1]]}]
            }),
            false,
            NOW,
        );
        let (episode, fact, line) = (
            got["ids"][0].as_u64().unwrap(),
            got["ids"][1].as_u64().unwrap(),
            got["ids"][2].as_u64().unwrap(),
        );
        let moved = s.say(NOW, "I moved to the university library.", "Nice.");
        let input = s.prepare(NOW, NOW + 10);
        let got = s.apply(
            &input["input"]["job"],
            json!({"facts":[{"kind":"work","claim":"Works at the university library.","cites":[moved],"supersedes":fact}]}),
            false,
            NOW + 20,
        );
        let newer = got["ids"][0].as_u64().expect("the newer fact");
        let day = Day::at(NOW - DAY_MS, &crate::store::LocalTime::Offset(0))
            .unwrap()
            .to_string();
        let found = s.ask(
            json!({"op":"search","person":MAI,"query":"library","nightly":true}),
            NOW + 30,
        );
        let nightly = found["nightly"].as_array().unwrap();
        let of = |id: u64| nightly.iter().find(|i| i["id"] == id).cloned();
        assert_eq!(of(episode).unwrap()["kind"], "episode");
        assert_eq!(of(episode).unwrap()["day"], json!(day));
        assert_eq!(of(newer).unwrap()["fact_kind"], "work");
        assert!(of(fact).is_none(), "a superseded fact is history");
        assert!(
            found["entries"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["id"] == newer && e["nightly"]["kind"] == "fact")
        );
        let history = s.ask(
            json!({"op":"search","person":MAI,"query":"city library","history":true}),
            NOW + 31,
        );
        assert!(
            history["entries"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["id"] == fact && e["active"] == false),
            "{history}"
        );
        let recalled = s.ask(
            json!({"op":"recall","person":MAI,"query":"library","nightly":true}),
            NOW + 32,
        );
        assert!(
            recalled["nightly"]
                .as_array()
                .unwrap()
                .iter()
                .any(|i| i["id"] == episode),
            "{recalled}"
        );
        let spanned = s.ask(
            json!({"op":"recall","person":MAI,"day_from":day,"day_to":day,"nightly":true}),
            NOW + 33,
        );
        assert!(
            spanned["nightly"]
                .as_array()
                .unwrap()
                .iter()
                .all(|i| i["day"] == json!(day))
        );
        // The note has the profile line, never the episode or the facts.
        let opened = s.ask(
            json!({"op":"session_open","person":MAI,"session":"s1"}),
            NOW + 34,
        );
        let note = opened["snapshot"]["note"].as_str().unwrap();
        assert!(note.contains("Sister: Lan."), "{note}");
        assert!(
            !note.contains("university") && !note.contains("Lan's visit"),
            "{note}"
        );
        let _ = line;
    }

    #[test]
    fn a_pattern_needs_two_days_and_without_the_switches_none_is_kept() {
        let mut s = Scratch::new("pattern");
        let first = s.say(NOW - DAY_MS, "I went running this morning.", "Nice.");
        let input = s.prepare(NOW - DAY_MS, NOW);
        let got = s.apply(
            &input["input"]["job"],
            json!({"facts":[{"kind":"habit","claim":"Went running in the morning.","cites":[first]}]}),
            false,
            NOW,
        );
        let fact = got["ids"][0].as_u64().unwrap();
        let second = s.say(NOW, "I went running again this morning.", "Nice.");
        let other = s.say(NOW + 1, "Running clears my head.", "Nice.");
        let input = s.prepare(NOW, NOW + 10);
        let proposal = json!({
            "patterns":[
                {"text":"Goes running most mornings.","cites":[second, fact]},
                {"text":"Running clears their head.","cites":[second, other]}
            ],
            "episodes":[{"first":second,"last":other,"topic":"running","gist":"Running again.","feeling":"calm"}],
            "feelings":[{"text":"Running clears their head.","cites":[other]}]
        });
        let off = s.apply(&input["input"]["job"], proposal.clone(), false, NOW + 20);
        assert_eq!(off["accepted"]["pattern"], 0);
        assert_eq!(off["accepted"]["feeling"], 0);
        assert_eq!(off["reasons"], json!({"off":3}), "{off}");
        let detail: String =
            s.1.db
                .query_row(
                    "SELECT detail FROM nightly_items WHERE kind='episode'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
        assert!(!detail.contains("calm"), "{detail}");
        // With the switches, the two-day pattern is kept, the one-day not.
        let mut s2 = Scratch::new("pattern-on");
        let first = s2.say(NOW - DAY_MS, "I went running this morning.", "Nice.");
        let input = s2.prepare(NOW - DAY_MS, NOW);
        let fact = s2.apply(
            &input["input"]["job"],
            json!({"facts":[{"kind":"habit","claim":"Went running in the morning.","cites":[first]}]}),
            false,
            NOW,
        )["ids"][0]
            .as_u64()
            .unwrap();
        assert_eq!(fact, 2);
        s2.say(NOW, "I went running again this morning.", "Nice.");
        s2.say(NOW + 1, "Running clears my head.", "Nice.");
        let input = s2.prepare(NOW, NOW + 10);
        let on = s2.apply(&input["input"]["job"], proposal, true, NOW + 20);
        assert_eq!(on["accepted"]["pattern"], 1, "{on}");
        assert_eq!(on["accepted"]["feeling"], 1, "{on}");
        assert_eq!(on["reasons"], json!({"days":1}), "{on}");
    }

    #[test]
    fn without_the_nightly_fields_answers_are_as_before() {
        let mut s = Scratch::new("off");
        day(&mut s, NOW);
        let query = |nightly: Option<bool>, op: &str| {
            let mut request = json!({"op":op,"person":MAI,"query":"library"});
            if let Some(nightly) = nightly {
                request["nightly"] = json!(nightly);
            }
            request
        };
        for op in ["search", "recall"] {
            let plain = s.ask(query(None, op), NOW + 1);
            assert!(plain.get("nightly").is_none());
            let off = s.ask(query(Some(false), op), NOW + 1);
            assert_eq!(plain, off, "{op}");
        }
        let tables: u64 = s
            .1
            .db
            .query_row("SELECT (SELECT count(*) FROM nightly_items)+(SELECT count(*) FROM nightly_jobs)+(SELECT count(*) FROM nightly_nights)", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tables, 0);
    }

    #[test]
    fn a_prepare_is_bounded_to_128_records_and_64_kib() {
        let mut s = Scratch::new("bounds");
        for n in 0..130 {
            s.say(NOW + n, &format!("Line {n} about the library."), "Okay.");
        }
        let input = s.prepare(NOW, NOW + DAY_MS / 4);
        assert_eq!(
            input["input"]["records"].as_array().unwrap().len(),
            MAX_RECORDS
        );
        assert_eq!(input["input"]["more"], true);
        let day = input["input"]["day"].clone();
        let rest = s.ask(
            json!({"op":"nightly_prepare","person":MAI,"day":day,"after_id":128}),
            NOW + DAY_MS / 4,
        );
        assert_eq!(rest["input"]["records"].as_array().unwrap().len(), 2);
        assert_eq!(rest["input"]["more"], false);
        let none = s.ask(
            json!({"op":"nightly_prepare","person":MAI,"day":day,"after_id":130}),
            NOW + DAY_MS / 4,
        );
        assert_eq!(none, json!({"ok":true,"reason":"nothing"}));
        let tomorrow = Day::at(NOW + 2 * DAY_MS, &crate::store::LocalTime::Offset(0))
            .unwrap()
            .to_string();
        assert_eq!(
            s.ask(
                json!({"op":"nightly_prepare","person":MAI,"day":tomorrow}),
                NOW + DAY_MS / 4
            )["error"],
            "bad_day"
        );

        let mut s = Scratch::new("bytes");
        for n in 0..60 {
            s.say(NOW + n, &"h".repeat(600), &"\"".repeat(400));
        }
        let input = s.prepare(NOW, NOW + DAY_MS / 4);
        let records = input["input"]["records"].as_array().unwrap().len();
        assert!(records < 60, "{records}");
        assert_eq!(input["input"]["more"], true);
        assert!(input["input"].to_string().len() <= MAX_INPUT_BYTES);
    }

    /// Hour `h` of the day of `NOW` (noon UTC), at `Offset(0)`.
    fn hour(h: u64) -> u64 {
        NOW - DAY_MS / 2 + h * 3_600_000
    }

    fn heard(input: &Value) -> Vec<String> {
        input["input"]["records"]
            .as_array()
            .map(|records| {
                records
                    .iter()
                    .map(|r| r["heard"].as_str().unwrap().to_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// El's scene on #5333: said at 15:00 and not kept, a topic forget at
    /// 16:00 that matched nothing, and the night passes over the 15:00 line
    /// and the forget's own turn, but reads a line said after it, another
    /// person's and another day's.
    #[test]
    fn a_forget_mark_keeps_the_night_off_that_days_earlier_lines() {
        let mut s = Scratch::new("mark");
        const BINH: &str = "p-fedcba9876543210";
        let binh = Person::new(BINH).unwrap();
        // Yesterday, the same words: a day whose pass may still be pending,
        // so it is marked too. Four days back, past the voice's pass: not.
        s.say(
            hour(15) - DAY_MS,
            "My sister Lan lives in Hue.",
            "Lovely city.",
        );
        let four_back = hour(15) - (u64::from(DAYS_BACK) + 1) * DAY_MS;
        s.say(four_back, "My sister Lan lived in Hanoi.", "I see.");
        s.say(
            hour(15),
            "My sister Lan just moved to Da Nang.",
            "How exciting.",
        );
        // A kept thing is the person's own ask, and a forget of it is by
        // its id: a mark leaves it to the night.
        s.1.remember(
            &Person::new(MAI).unwrap(),
            hour(15) + 1,
            "I play the cello on Sundays.",
        )
        .unwrap();
        s.1.append(&binh, hour(15), "My brother lives in Hanoi.", "Nice.", &[])
            .unwrap();
        // The voice's mark for a `forget {about}` that matched nothing.
        assert_eq!(
            s.ask(json!({"op":"forget_mark","person":MAI}), hour(16)),
            json!({"ok":true})
        );
        // The forget request itself, stamped in its turn.
        s.say(
            hour(16) + 5_000,
            "Forget what I said about my sister.",
            "I couldn't find anything about your sister.",
        );
        // Said well after it: read.
        s.say(
            hour(16) + 5 * 60_000,
            "I have a dentist visit on Friday.",
            "Noted.",
        );
        let tonight = NOW + DAY_MS / 2 + 3 * 3_600_000;
        let input = s.prepare(hour(16), tonight);
        assert_eq!(heard(&input), ["", "I have a dentist visit on Friday."]);
        assert_eq!(input["input"]["records"][0]["kind"], "kept");
        // Yesterday's earlier line is passed over too; a day past the
        // voice's pass is untouched.
        assert_eq!(
            s.prepare(hour(15) - DAY_MS, tonight),
            json!({"ok":true,"reason":"nothing"})
        );
        assert_eq!(
            heard(&s.prepare(four_back, tonight)),
            ["My sister Lan lived in Hanoi."]
        );
        // Another person's lines are untouched.
        let day = Day::at(hour(15), &crate::store::LocalTime::Offset(0))
            .unwrap()
            .to_string();
        let theirs = s.ask(
            json!({"op":"nightly_prepare","person":BINH,"day":day}),
            tonight,
        );
        assert_eq!(heard(&theirs), ["My brother lives in Hanoi."]);
        // The room and a rules scope mark nothing: Mai's today and the
        // days back are all.
        s.ask(json!({"op":"forget_mark"}), hour(17));
        let marks: u64 =
            s.1.db
                .query_row("SELECT count(*) FROM nightly_forgets", [], |r| r.get(0))
                .unwrap();
        assert_eq!(marks, u64::from(DAYS_BACK) + 1);
    }

    /// A forget asked while a pass runs: the lines said before it are left
    /// out of the apply as a forgotten one is.
    #[test]
    fn a_forget_mark_during_a_pass_drops_what_rests_on_earlier_lines() {
        let mut s = Scratch::new("mark-mid-pass");
        let sister = s.say(
            hour(15),
            "My sister Lan just moved to Da Nang.",
            "How exciting.",
        );
        let input = s.prepare(hour(15), hour(20));
        assert_eq!(heard(&input).len(), 1);
        s.ask(json!({"op":"forget_mark","person":MAI}), hour(21));
        let got = s.apply(
            &input["input"]["job"],
            json!({"facts":[{"kind":"person","claim":"Sister Lan lives in Da Nang.","cites":[sister]}]}),
            false,
            hour(22),
        );
        assert_eq!(got["accepted"]["fact"], 0, "{got}");
        assert_eq!(got["reasons"], json!({"forgotten":1}), "{got}");
    }

    /// `forget` and `forget_last` mark the day, found or not; `forget_all`
    /// and `forget_person` take the person's marks with everything else.
    #[test]
    fn every_forget_marks_and_forgetting_everything_takes_the_marks() {
        let mut s = Scratch::new("mark-ops");
        let marks = |s: &Scratch| -> u64 {
            s.1.db
                .query_row(
                    "SELECT count(*) FROM nightly_forgets WHERE person=?",
                    [MAI],
                    |r| r.get(0),
                )
                .unwrap()
        };
        let at = |s: &Scratch| -> u64 {
            s.1.db
                .query_row(
                    "SELECT min(at_unix_ms) FROM nightly_forgets WHERE person=?",
                    [MAI],
                    |r| r.get(0),
                )
                .unwrap()
        };
        s.say(
            hour(13),
            "My sister Lan just moved to Da Nang.",
            "How exciting.",
        );
        s.ask(json!({"op":"forget","person":MAI,"id":999}), hour(14));
        let window = u64::from(DAYS_BACK) + 1;
        assert_eq!((marks(&s), at(&s)), (window, hour(14)));
        s.ask(
            json!({"op":"forget_last","person":MAI,"kind":"kept"}),
            hour(15),
        );
        assert_eq!((marks(&s), at(&s)), (window, hour(15)));
        // The next day adds its own mark, and moves the rest on.
        s.ask(json!({"op":"forget_mark","person":MAI}), hour(15) + DAY_MS);
        assert_eq!((marks(&s), at(&s)), (window + 1, hour(15)));
        s.ask(json!({"op":"forget_all","person":MAI}), hour(16) + DAY_MS);
        assert_eq!(marks(&s), 0);
        s.ask(json!({"op":"forget_mark","person":MAI}), hour(17) + DAY_MS);
        s.ask(
            json!({"op":"forget_person","person":MAI}),
            hour(18) + DAY_MS,
        );
        assert_eq!(marks(&s), 0);
    }

    /// Thirteen's P2 on #5345: said at 23:40, forgotten at 00:10 the next
    /// day, and that night's 03:00 pass of the first day passes over it. A
    /// line said on the new day after the forget minute is read.
    #[test]
    fn a_forget_after_midnight_reaches_the_day_before() {
        let mut s = Scratch::new("mark-midnight");
        let minute = 60_000;
        s.say(
            hour(23) + 40 * minute,
            "My sister Lan just moved to Da Nang.",
            "How exciting.",
        );
        s.ask(
            json!({"op":"forget","person":MAI,"id":999}),
            hour(24) + 10 * minute,
        );
        s.say(
            hour(24) + 20 * minute,
            "I have a dentist visit on Friday.",
            "Noted.",
        );
        let night = hour(27);
        assert_eq!(
            s.prepare(hour(23), night),
            json!({"ok":true,"reason":"nothing"})
        );
        assert_eq!(
            heard(&s.prepare(hour(24) + 20 * minute, night)),
            ["I have a dentist visit on Friday."]
        );
    }

    /// El's second scene on #5345: a night put off, so a day two days back
    /// is still pending, and a forget today passes over its lines.
    #[test]
    fn a_forget_today_reaches_a_day_still_pending_two_days_back() {
        let mut s = Scratch::new("mark-pending");
        let two_back = hour(15) - 2 * DAY_MS;
        s.say(
            two_back,
            "My sister Lan just moved to Da Nang.",
            "How exciting.",
        );
        s.ask(json!({"op":"forget_mark","person":MAI}), hour(13));
        s.say(hour(14), "I have a dentist visit on Friday.", "Noted.");
        assert_eq!(
            s.prepare(two_back, hour(27)),
            json!({"ok":true,"reason":"nothing"})
        );
        assert_eq!(
            heard(&s.prepare(hour(14), hour(27))),
            ["I have a dentist visit on Friday."]
        );
    }

    /// A forget four days later marks no day past the voice's pass: that
    /// day's lines are as they were (its pass ran, or never will).
    #[test]
    fn a_forget_four_days_later_leaves_the_day_alone() {
        let mut s = Scratch::new("mark-four-days");
        s.say(
            hour(15),
            "My sister Lan just moved to Da Nang.",
            "How exciting.",
        );
        let later = hour(15) + (u64::from(DAYS_BACK) + 1) * DAY_MS;
        s.ask(
            json!({"op":"forget_last","person":MAI,"kind":"kept"}),
            later,
        );
        assert_eq!(
            heard(&s.prepare(hour(15), later + 60_000)),
            ["My sister Lan just moved to Da Nang."]
        );
    }

    /// `forget_last` marks the days even when nothing is kept to forget
    /// (the mutation that drops its mark fails here).
    #[test]
    fn a_forget_last_with_nothing_kept_still_marks() {
        let mut s = Scratch::new("mark-forget-last");
        s.say(
            hour(15),
            "My sister Lan just moved to Da Nang.",
            "How exciting.",
        );
        s.ask(
            json!({"op":"forget_last","person":MAI,"kind":"kept"}),
            hour(16),
        );
        assert_eq!(
            s.prepare(hour(15), hour(27)),
            json!({"ok":true,"reason":"nothing"})
        );
    }
}
