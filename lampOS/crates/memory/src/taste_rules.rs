//! Everyday tastes a known person states, kept as observations by code
//! rules, with no model (`lamp-memory --taste-rules`, step A of the memory
//! owner's extraction plan C, 3 Nov; docs/memory-days.md, "Tastes kept by
//! rule"). The source-linked observation that copies the person's own words
//! is the Tiers design's fact with exact quotes (docs/memory-tiers.md).
//!
//! Only a known person's own exchanges (`p-…`, [`applies_to`]) are read, and
//! only what the lamp heard them say, never its own line. A sentence that
//! asks, hedges ("maybe", "có lẽ"), speaks of someone else (a person word
//! or its "'s", a capitalised name in the object, or reported speech:
//! [`sensitive`], [`names_someone`], [`reported`]), or names health,
//! medicine or money ([`sensitive`]) is passed over: the product keeps none
//! of those unless the person asks the lamp to remember them (memory.md,
//! the observation pass's rule). There is no allergy or diet rule for that
//! reason.
//! What is kept is that one sentence, copied, linked to its exchange, so a
//! forget of the exchange or of the observation takes both.
use crate::{
    days::{self, Record},
    search,
    store::{LocalTime, Person},
    tidy,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use unicode_normalization::UnicodeNormalization;

/// The side table that names each rule-kept observation's kind and what
/// it is about. Made only when the rules are switched on, so a store
/// without them is unchanged. Its row goes with its observation on every
/// forget and on expiry.
pub const TABLE: &str = "CREATE TABLE IF NOT EXISTS tastes(\
    rid INTEGER PRIMARY KEY REFERENCES entries(rid) ON DELETE CASCADE,\
    kind TEXT NOT NULL CHECK(kind IN ('preference','dislike')),\
    about TEXT NOT NULL);\
    CREATE INDEX IF NOT EXISTS tastes_about ON tastes(about);";

/// At most this many observations from one exchange.
pub const MOST_A_LINE: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Preference,
    Dislike,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Preference => "preference",
            Kind::Dislike => "dislike",
        }
    }
}

/// One stated taste: the sentence as heard, its kind, and what it is
/// about, so a newer one about the same thing replaces it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Taste {
    /// The person's own words, copied from the line: the whole sentence.
    pub text: String,
    pub kind: Kind,
    /// "taste coffee", "favourite colour".
    pub about: String,
}

/// Whether the rules read this scope's lines: a known person's own only,
/// never the room's (`everyone`, guests and unknown speakers included) or a
/// rules scope. The voice decides the scope (`memory_scope.rs`).
pub fn applies_to(person: &Person) -> bool {
    lamp_msgs::known::is_person_id(person.as_str())
}

/// Every taste the line states, in order, at most [`MOST_A_LINE`].
pub fn tastes(heard: &str) -> Vec<Taste> {
    let mut found = Vec::new();
    for (sentence, mark) in sentences(heard) {
        if found.len() == MOST_A_LINE {
            break;
        }
        if mark == Some('?') || sentence.len() > tidy::MAX_FACT_BYTES {
            continue;
        }
        let heard_words = split(sentence, false);
        let words = split(sentence, true);
        let words: Vec<&str> = words.iter().map(String::as_str).collect();
        if hedged(&words) || sensitive(&words) {
            continue;
        }
        if let Some((kind, about, object_len)) = english(&words).or_else(|| vietnamese(&words)) {
            // The object, from its first word to the sentence's end, as
            // heard and lower-cased. The two splits differ only in case,
            // so they line up; if they ever did not, nothing is kept.
            if heard_words.len() != words.len() {
                continue;
            }
            // The sentence's own first word ("Món yêu thích …") is not a
            // name, so it is never read for one.
            let from = words.len() - object_len;
            if names_someone(&heard_words[from.max(1)..]) || reported(&words[from..]) {
                continue;
            }
            found.push(Taste {
                text: sentence.trim().to_owned(),
                kind,
                about,
            });
        }
    }
    found
}

/// The line's sentences, each with the mark that ended it, if any. A
/// sentence's text runs to and includes its mark.
fn sentences(heard: &str) -> Vec<(&str, Option<char>)> {
    let mut found = Vec::new();
    let mut start = 0;
    for (at, character) in heard.char_indices() {
        if matches!(
            character,
            '.' | '!' | '?' | ';' | '\n' | '…' | '？' | '！' | '。'
        ) {
            let end = at + character.len_utf8();
            let mark = match character {
                '?' | '？' => '?',
                other => other,
            };
            found.push((&heard[start..end], Some(mark)));
            start = end;
        }
    }
    found.push((&heard[start..], None));
    found.retain(|(text, _)| text.chars().any(char::is_alphanumeric));
    found
}

/// The sentence's words in NFC (`đ` kept: "đó" is not "dó"), lower-cased
/// when `lower`, split at hyphens too ("gluten-free"), punctuation off
/// their ends, a curly apostrophe made straight.
fn split(sentence: &str, lower: bool) -> Vec<String> {
    let text: String = if lower {
        sentence.nfc().flat_map(char::to_lowercase).nfc().collect()
    } else {
        sentence.nfc().collect()
    };
    text.replace(['\u{2019}', '\u{2018}'], "'")
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | ':' | '-' | '/'))
        .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric() && c != '\''))
        .map(|word| word.trim_matches('\''))
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Words that make a sentence less than said: a maybe is never kept as a
/// fact (the said/plan/maybe idea of the Tiers design).
const HEDGES: &[&str] = &[
    "maybe", "might", "perhaps", "probably", "possibly", "think", "guess", "suppose", "kinda",
    "sorta", "unsure", "may", "wonder", "chắc", "nghĩ", "đoán",
];
const HEDGE_PAIRS: &[(&str, &str)] = &[
    ("not", "sure"),
    ("kind", "of"),
    ("sort", "of"),
    ("có", "lẽ"),
    ("hình", "như"),
    ("có", "thể"),
    ("không", "biết"),
];

fn hedged(words: &[&str]) -> bool {
    words.iter().any(|word| HEDGES.contains(word))
        || words
            .windows(2)
            .any(|pair| HEDGE_PAIRS.contains(&(pair[0], pair[1])))
}

/// Words of health, medicine, money or another person: a sentence with one
/// is never kept by a rule ("I love my wife's cooking", "I hate my
/// medicine", "I like saving money", "tôi thích tiền").
const SENSITIVE: &[&str] = &[
    // Health and medicine.
    "allergy",
    "allergic",
    "allergies",
    "vegetarian",
    "vegan",
    "pescatarian",
    "diet",
    "dieting",
    "medicine",
    "medicines",
    "medication",
    "medications",
    "pill",
    "pills",
    "drug",
    "drugs",
    "doctor",
    "doctors",
    "hospital",
    "clinic",
    "therapy",
    "therapist",
    "sick",
    "illness",
    "disease",
    "pain",
    "diabetes",
    "cancer",
    "pregnant",
    "pregnancy",
    "depression",
    "anxiety",
    "surgery",
    "treatment",
    "health",
    "symptom",
    "symptoms",
    "injury",
    "vaccine",
    "insulin",
    "gluten",
    "lactose",
    "intolerant",
    "weight",
    "dị",
    "ứng",
    "chay",
    "thuốc",
    "bệnh",
    "viện",
    "bác",
    "sĩ",
    "ốm",
    "đau",
    "khám",
    "sức",
    "khỏe",
    "khoẻ",
    "tiểu",
    "đường",
    "ung",
    "thư",
    "bầu",
    "trị",
    "kiêng",
    // Money.
    "money",
    "salary",
    "debt",
    "debts",
    "loan",
    "loans",
    "bank",
    "mortgage",
    "rent",
    "dollars",
    "dollar",
    "income",
    "tax",
    "taxes",
    "invest",
    "investing",
    "stocks",
    "crypto",
    "bitcoin",
    "tiền",
    "lương",
    "nợ",
    "vay",
    "ngân",
    "hàng",
    "đầu",
    "tư",
    "thuế",
    "chứng",
    "khoán",
    // Another person.
    "wife",
    "husband",
    "partner",
    "girlfriend",
    "boyfriend",
    "mom",
    "mum",
    "mother",
    "dad",
    "father",
    "parents",
    "son",
    "daughter",
    "kid",
    "kids",
    "children",
    "child",
    "baby",
    "brother",
    "sister",
    "friend",
    "friends",
    "boss",
    "colleague",
    "coworker",
    "neighbor",
    "neighbour",
    "family",
    "grandma",
    "grandpa",
    "grandmother",
    "grandfather",
    "aunt",
    "uncle",
    "cousin",
    "he",
    "she",
    "him",
    "her",
    "his",
    "hers",
    "they",
    "their",
    "them",
    "name",
    "named",
    "person",
    "people",
    "guy",
    "girl",
    "man",
    "woman",
    "vợ",
    "chồng",
    "mẹ",
    "bố",
    "ba",
    "má",
    "cha",
    "con",
    "bạn",
    "sếp",
    "người",
    "yêu",
    "anh",
    "chị",
    "ông",
    "bà",
    "cô",
    "chú",
    "dì",
    "cậu",
    "họ",
    "nó",
    "gia",
    "đình",
    "tên",
];

/// Whether the sentence names health, medicine, money or another person.
/// Each word is read with a trailing "'s" off too, so a possessive of a
/// person word ("my boss's jokes", "my sister's cooking") counts as the
/// word. "yêu thích" (favourite) is not "người yêu" (a partner): a
/// favourite's own words are read without it.
fn sensitive(words: &[&str]) -> bool {
    let mut words = words.to_vec();
    if let Some(at) = words.windows(2).position(|pair| pair == ["yêu", "thích"]) {
        words.drain(at..at + 2);
    }
    words.iter().any(|word| {
        let bare = word.strip_suffix("'s").unwrap_or(word);
        SENSITIVE.contains(word) || SENSITIVE.contains(&bare)
    })
}

/// Whether the object, as heard, holds a capitalised word other than the
/// person's own "I": a name ("I can't stand Tom", "tôi ghét Lan"). The
/// caller leaves out the sentence's first word, capitalised as heard. A
/// capitalised food or brand ("I love Pepsi") is dropped as well: the rule
/// cannot tell a brand from a person, and another person is never kept.
fn names_someone(object: &[String]) -> bool {
    object.iter().any(|word| {
        let lower = word.to_lowercase();
        let own = lower == "i" || lower.starts_with("i'");
        !own && word.chars().next().is_some_and(char::is_uppercase)
    })
}

/// Words of reported speech: after the taste verb they make the sentence
/// someone else's words ("I hate broccoli, Minh says", "tôi ghét hành, mẹ
/// bảo").
const REPORTED: &[&str] = &["says", "said", "told", "tells", "nói", "bảo", "kể"];

/// Whether the object, lower-cased, reports someone else's speech.
fn reported(object: &[&str]) -> bool {
    object.iter().any(|word| REPORTED.contains(word))
}

fn skip<'a, 'b>(mut words: &'a [&'b str], these: &[&str]) -> &'a [&'b str] {
    while let Some((first, rest)) = words.split_first() {
        if !these.contains(first) {
            break;
        }
        words = rest;
    }
    words
}

const EN_OPENING: &[&str] = &[
    "oh", "well", "yeah", "yes", "yep", "and", "but", "so", "actually", "honestly", "also", "um",
    "uh", "hmm", "ok", "okay", "no", "nope", "now",
];
const EN_ADVERBS: &[&str] = &[
    "really",
    "just",
    "absolutely",
    "totally",
    "also",
    "still",
    "truly",
    "so",
    "do",
    "especially",
    "definitely",
    "actually",
    "even",
];
const EN_LIKE: &[&str] = &["love", "like", "enjoy", "prefer", "adore"];
const EN_HATE: &[&str] = &["hate", "dislike", "detest", "loathe"];
const EN_NOT: &[&str] = &["not", "don't", "dont", "never"];
/// An object that says nothing on its own: "I love it", "I like that".
const EN_EMPTY: &[&str] = &[
    "it", "that", "this", "these", "those", "them", "you", "him", "her", "us", "me", "what", "how",
    "when", "where", "why", "which", "who", "your", "his", "their", "our", "if",
];
const EN_ARTICLES: &[&str] = &["a", "an", "the", "some"];
const EN_TAIL: &[&str] = &[
    "anymore", "any", "more", "now", "a", "lot", "much", "very", "so", "too", "either", "really",
    "nowadays", "these", "days", "though",
];
/// Where an object's key stops: "coffee but not tea" is about coffee.
const EN_CUT: &[&str] = &[
    "but", "because", "since", "when", "though", "although", "if", "so", "anyway",
];

/// What an English sentence states, if a rule knows it, and how many of
/// its last words are the object (from the favourite's thing or the
/// verb's object to the end).
fn english(words: &[&str]) -> Option<(Kind, String, usize)> {
    let words = skip(words, EN_OPENING);
    // "my favourite colour is teal"
    if words.first() == Some(&"my") && matches!(words.get(1), Some(&("favourite" | "favorite"))) {
        let is = words
            .iter()
            .skip(2)
            .take(4)
            .position(|word| matches!(*word, "is" | "are"))?
            + 2;
        let thing = &words[2..is];
        object(&words[is + 1..], EN_EMPTY, EN_ARTICLES, EN_TAIL, EN_CUT)?;
        return (!thing.is_empty()).then(|| {
            (
                Kind::Preference,
                format!("favourite {}", thing.join(" ")),
                words.len() - 2,
            )
        });
    }
    let rest = match words {
        ["i", rest @ ..] => rest,
        _ => return None,
    };
    let rest = skip(rest, EN_ADVERBS);
    let (kind, thing) = match rest {
        [verb, thing @ ..] if EN_LIKE.contains(verb) => (Kind::Preference, thing),
        [verb, thing @ ..] if EN_HATE.contains(verb) => (Kind::Dislike, thing),
        ["can't" | "cannot" | "cant", "stand", thing @ ..]
        | ["can", "not", "stand", thing @ ..] => (Kind::Dislike, thing),
        _ => {
            let after = match rest {
                ["no", "longer", after @ ..] => after,
                [not, after @ ..] if EN_NOT.contains(not) => after,
                _ => return None,
            };
            match skip(after, EN_ADVERBS) {
                [verb, thing @ ..] if matches!(*verb, "like" | "love" | "enjoy") => {
                    (Kind::Dislike, thing)
                }
                _ => return None,
            }
        }
    };
    let key = object(thing, EN_EMPTY, EN_ARTICLES, EN_TAIL, EN_CUT)?;
    Some((kind, format!("taste {key}"), thing.len()))
}

const VI_OPENING: &[&str] = &[
    "à", "ừ", "ừm", "ờ", "vâng", "dạ", "thì", "nhưng", "mà", "còn", "thật", "ra", "nói", "chung",
];
const VI_SUBJECTS: &[&str] = &["tôi", "em", "mình"];
const VI_ADVERBS: &[&str] = &[
    "cũng", "rất", "vẫn", "thật", "sự", "thực", "khá", "luôn", "đặc", "biệt", "cực", "kỳ", "chỉ",
    "đang",
];
const VI_NOT: &[&str] = &["không", "chẳng", "chả"];
/// A sentence's last word that makes it a question even without its mark:
/// "em thích cà phê không".
const VI_ASKING: &[&str] = &[
    "không", "ko", "hông", "hả", "à", "nhỉ", "chứ", "chưa", "sao", "gì", "nào", "chăng", "ư",
];
/// An object that is a person or a pointing word: "em thích anh", "thích
/// cái đó".
const VI_EMPTY: &[&str] = &[
    "anh", "chị", "em", "bạn", "nó", "cậu", "ông", "bà", "cô", "chú", "mày", "họ", "gì", "điều",
    "đó", "này", "kia", "ấy",
];
const VI_TAIL: &[&str] = &["nữa", "lắm", "nhất", "quá", "rồi", "luôn", "cả", "đâu"];

/// What a Vietnamese sentence states, if a rule knows it, and how many of
/// its last words are the object, as [`english`] gives.
fn vietnamese(words: &[&str]) -> Option<(Kind, String, usize)> {
    let words = skip(words, VI_OPENING);
    if words.last().is_some_and(|last| VI_ASKING.contains(last)) {
        return None;
    }
    // "món yêu thích (nhất) của tôi là phở"
    if let Some(at) = words
        .windows(2)
        .take(4)
        .position(|pair| pair == ["yêu", "thích"])
    {
        let thing = &words[..at];
        let rest = &words[at + 2..];
        let rest = rest.strip_prefix(&["nhất"]).unwrap_or(rest);
        return match rest {
            ["của", subject, "là", object_words @ ..]
                if VI_SUBJECTS.contains(subject) && !thing.is_empty() =>
            {
                vi_object(object_words)?;
                Some((
                    Kind::Preference,
                    format!("favourite {}", thing.join(" ")),
                    words.len(),
                ))
            }
            _ => None,
        };
    }
    let rest = match words {
        [subject, rest @ ..] if VI_SUBJECTS.contains(subject) => rest,
        _ => return None,
    };
    // "em ấy thích …" is someone else.
    if rest.first() == Some(&"ấy") {
        return None;
    }
    let rest = skip(rest, VI_ADVERBS);
    let (kind, thing) = match rest {
        ["thích", thing @ ..] => (Kind::Preference, thing),
        ["ghét", thing @ ..] => (Kind::Dislike, thing),
        [not, after @ ..] if VI_NOT.contains(not) => {
            let after = after.strip_prefix(&["còn"]).unwrap_or(after);
            match skip(after, VI_ADVERBS) {
                ["thích", thing @ ..] => (Kind::Dislike, thing),
                _ => return None,
            }
        }
        _ => return None,
    };
    let key = vi_object(thing)?;
    Some((kind, format!("taste {key}"), thing.len()))
}

fn vi_object(words: &[&str]) -> Option<String> {
    if words.iter().any(|word| VI_EMPTY.contains(word)) {
        return None;
    }
    object(words, VI_EMPTY, &[], VI_TAIL, &["nhưng", "vì", "tại"])
}

/// Objects that are a turn of speech, not a taste: "nothing", "everything".
const WHOLE_EMPTY: &[&str] = &["nothing", "none", "everything", "anything"];
/// Words that point the object at the listener: "the way you said that".
const LISTENER: &[&str] = &["you", "your", "yours", "bạn"];

/// What an object is about, as a key: its words without a leading article,
/// up to a clause word, less a trailing "anymore"; none for an object that
/// says nothing on its own, starts with "to" and a verb ("I hate to bother
/// you"), speaks of the listener ("I like the way you said that") or is
/// only "nothing" / "everything".
fn object(
    words: &[&str],
    empty: &[&str],
    articles: &[&str],
    tail: &[&str],
    cut: &[&str],
) -> Option<String> {
    if words.first().is_none_or(|first| empty.contains(first))
        || (words.first() == Some(&"to") && words.len() > 1)
        || words.iter().any(|word| LISTENER.contains(word))
    {
        return None;
    }
    let words = skip(words, articles);
    let end = words
        .iter()
        .position(|word| cut.contains(word))
        .unwrap_or(words.len());
    let mut words = &words[..end];
    while let Some((last, rest)) = words.split_last() {
        if !tail.contains(last) {
            break;
        }
        words = rest;
    }
    (!words.is_empty() && !matches!(words, [only] if WHOLE_EMPTY.contains(only)))
        .then(|| words.join(" "))
}

/// Keeps the line's tastes as observations of `person`'s, each linked to
/// the exchange `id` just written in `tx`, inside its write: a failure here
/// rolls back only this, never the exchange. Gives how many were kept.
#[allow(clippy::too_many_arguments)]
pub(crate) fn observe(
    tx: &mut Transaction<'_>,
    person: &Person,
    id: u64,
    at: u64,
    heard: &str,
    local: &LocalTime,
    now: u64,
) -> usize {
    if !applies_to(person) {
        return 0;
    }
    let found = tastes(heard);
    if found.is_empty() {
        return 0;
    }
    let kept = (|| -> days::Result<usize> {
        let point = tx.savepoint()?;
        let kept = keep(&point, person, id, at, &found, local, now)?;
        point.commit()?;
        Ok(kept)
    })();
    match kept {
        Ok(kept) => kept,
        Err(_) => {
            eprintln!("lamp-memory: taste rules kept nothing: their write failed");
            0
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn keep(
    db: &Connection,
    person: &Person,
    id: u64,
    at: u64,
    found: &[Taste],
    local: &LocalTime,
    now: u64,
) -> days::Result<usize> {
    let mut kept = 0;
    for taste in found {
        let count: u64 = db.query_row(
            "SELECT count(*) FROM entries WHERE person=? AND kind='observation' AND coalesce(category,'')!='talk'",
            [person.as_str()],
            |r| r.get(0),
        )?;
        if count >= days::MAX_OBSERVATIONS {
            break;
        }
        // The current rule-kept observation about the same thing, if any,
        // by its indexed `about`.
        let same: Option<(u64, String, u64)> = db
            .query_row(
                "SELECT e.id,e.text,e.at_unix_ms FROM tastes t JOIN entries e ON e.rid=t.rid WHERE t.about=?2 AND e.person=?1 AND e.active=1 ORDER BY e.id DESC LIMIT 1",
                params![person.as_str(), taste.about],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if same
            .as_ref()
            .is_some_and(|(_, text, _)| days::same_text(text) == days::same_text(&taste.text))
        {
            continue;
        }
        // Not when the scope keeps it already in the same words: only the
        // kept things and observations the index finds with one of its
        // words are read (`search::query`), never the whole scope.
        let texts: Vec<String> = match search::query(&taste.text) {
            Some(query) => db
                .prepare("SELECT e.text FROM search s JOIN entries e ON e.rid=s.rowid WHERE search MATCH ?2 AND e.person=?1 AND e.active=1 AND e.kind IN ('kept','observation')")?
                .query_map(params![person.as_str(), query], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?,
            None => Vec::new(),
        };
        let proposal = tidy::Proposal {
            text: taste.text.clone(),
            category: tidy::Category::Preference,
            source_ids: vec![id],
            supersedes: None,
            on: None,
            replaces: None,
        };
        if tidy::restates(&proposal, &texts) {
            continue;
        }
        // A newer taste about the same thing retires the older, kept as
        // history; one dated after this line is left beside it.
        let supersedes = same
            .filter(|(_, _, old_at)| *old_at <= at)
            .map(|(old, _, _)| old);
        let new = days::next_id(db, person)?;
        days::insert_record(
            db,
            &Record {
                id: new,
                person: person.as_str().into(),
                kind: "observation".into(),
                at_unix_ms: at,
                heard: String::new(),
                said: String::new(),
                text: taste.text.clone(),
                category: Some(tidy::Category::Preference),
                source_ids: vec![id],
                supersedes,
                active: true,
            },
            local,
            now,
        )?;
        db.execute(
            "INSERT INTO tastes(rid,kind,about) SELECT rid,?3,?4 FROM entries WHERE person=?1 AND id=?2",
            params![person.as_str(), new, taste.kind.name(), taste.about],
        )?;
        kept += 1;
    }
    Ok(kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(line: &str) -> Option<(Kind, String, String)> {
        let found = tastes(line);
        assert!(found.len() <= 1, "{line}: {found:?}");
        found.into_iter().next().map(|t| (t.kind, t.about, t.text))
    }

    #[test]
    fn each_english_rule_keeps_the_words_and_its_kind() {
        for (line, kind, about) in [
            ("I love coffee.", Kind::Preference, "taste coffee"),
            ("I like jazz", Kind::Preference, "taste jazz"),
            (
                "Oh, I really like green tea!",
                Kind::Preference,
                "taste green tea",
            ),
            (
                "I enjoy hiking on weekends",
                Kind::Preference,
                "taste hiking on weekends",
            ),
            ("I prefer tea", Kind::Preference, "taste tea"),
            ("I hate cilantro", Kind::Dislike, "taste cilantro"),
            (
                "I don't like coffee anymore.",
                Kind::Dislike,
                "taste coffee",
            ),
            (
                "I do not like loud music",
                Kind::Dislike,
                "taste loud music",
            ),
            ("I can't stand traffic", Kind::Dislike, "taste traffic"),
            // The person's own "I" in the object is not a name.
            (
                "I like coffee when I wake up",
                Kind::Preference,
                "taste coffee",
            ),
            (
                "My favourite colour is teal.",
                Kind::Preference,
                "favourite colour",
            ),
            (
                "my favorite food is pho",
                Kind::Preference,
                "favourite food",
            ),
        ] {
            let (k, a, text) = one(line).unwrap_or_else(|| panic!("{line}: nothing"));
            assert_eq!((k, a.as_str()), (kind, about), "{line}");
            assert_eq!(text, line.trim(), "the words are copied: {line}");
        }
    }

    #[test]
    fn each_vietnamese_rule_keeps_the_words_and_its_kind() {
        for (line, kind, about) in [
            ("Tôi thích cà phê.", Kind::Preference, "taste cà phê"),
            ("em rất thích phở", Kind::Preference, "taste phở"),
            (
                "Mình thích nghe nhạc jazz",
                Kind::Preference,
                "taste nghe nhạc jazz",
            ),
            ("Tôi ghét sầu riêng", Kind::Dislike, "taste sầu riêng"),
            ("Tôi không thích cà phê nữa", Kind::Dislike, "taste cà phê"),
            ("em không còn thích trà sữa", Kind::Dislike, "taste trà sữa"),
            (
                "Món yêu thích của tôi là bún chả",
                Kind::Preference,
                "favourite món",
            ),
            (
                "màu yêu thích nhất của em là xanh",
                Kind::Preference,
                "favourite màu",
            ),
        ] {
            let (k, a, text) = one(line).unwrap_or_else(|| panic!("{line}: nothing"));
            assert_eq!((k, a.as_str()), (kind, about), "{line}");
            assert_eq!(text, line.trim(), "{line}");
        }
    }

    #[test]
    fn questions_others_maybes_and_empty_objects_keep_nothing() {
        for line in [
            "I like coffee?",
            "Do you like coffee?",
            "do you like coffee",
            "What do I like",
            "My wife likes coffee.",
            "My sister's name is Lan.",
            "She said I love it.",
            "I love it.",
            "I love you",
            "I like that",
            "I'd like a coffee",
            "I would love some tea",
            "I liked coffee as a kid",
            "I used to love coffee",
            "Maybe I like coffee.",
            "I might like coffee",
            "I think I like jazz",
            "I'm not allergic to peanuts",
            "I'm not vegetarian",
            // Health, diet, medicine, money and other people: never by rule.
            "I'm allergic to peanuts.",
            "I am severely allergic to shellfish",
            "I have an allergy to cats",
            "I'm vegetarian",
            "I'm vegan now.",
            "I like gluten-free bread",
            "I hate my medicine",
            "I don't like going to the doctor",
            "I love saving money",
            "I hate paying rent",
            "I love my wife's cooking",
            "I like my boss",
            "My mom's name is Lan.",
            "My favourite person is Lan",
            "My favourite drink is whatever she makes",
            "Tôi bị dị ứng với tôm.",
            "em dị ứng đậu phộng",
            "Tôi ăn chay.",
            "mình ăn chay trường",
            "Tôi ghét uống thuốc",
            "Tôi thích tiền",
            "Tôi thích nấu ăn cho vợ",
            "Em thích mẹ em nấu",
            "Tên của mẹ tôi là Lan",
            "Người yêu thích nhất của tôi là Lan",
            "I don't hate it",
            "Em có thích cà phê không",
            "Em thích cà phê không?",
            "Anh ấy thích cà phê",
            "Em ấy thích phở",
            "Có lẽ tôi thích cà phê",
            "Chắc tôi thích cà phê",
            "Tôi thích cái đó",
            "Em thích anh",
            "Tôi không ăn chay",
            "Bạn thích gì",
            // A possessive of a person word, a bare name, reported speech.
            "I hate my boss's jokes.",
            "I love my sister's cooking.",
            "I like my son's school.",
            "I hate my neighbour's dog.",
            "I love my kid's drawings.",
            "I love my boss’s office.",
            "I can't stand Tom.",
            "Tôi ghét Lan.",
            "I hate broccoli, Minh says.",
            "I love jazz, my teacher told me.",
            "I hate broccoli, minh says.",
            "Tôi ghét hành, ai cũng nói vậy.",
            // Turns of speech, not tastes.
            "I hate to bother you.",
            "I like the way you said that.",
            "I like nothing.",
            "I love everything.",
            "I don't like anything.",
            "",
            "...",
        ] {
            assert_eq!(tastes(line), vec![], "{line}");
        }
    }

    #[test]
    fn a_line_keeps_each_stated_sentence_only() {
        let found = tastes("Hello there. I love coffee! Do you like tea? I hate rain.");
        let texts: Vec<&str> = found.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, ["I love coffee!", "I hate rain."]);
        let many = tastes("I like tea. I like jazz. I like rain. I like cats. I like dogs.");
        assert_eq!(many.len(), MOST_A_LINE);
    }

    #[test]
    fn only_a_known_persons_own_scope_is_read() {
        assert!(applies_to(&Person::new("p-0123456789abcdef").unwrap()));
        assert!(applies_to(&Person::new("p-0123456789abcdef-2").unwrap()));
        for scope in ["everyone", "r-0123456789abcdef", "r-everyone", "mai", "p-2"] {
            assert!(!applies_to(&Person::new(scope).unwrap()), "{scope}");
        }
    }

    /// The rules cost little next to the write's own durable commit: 1,000
    /// lines read in well under a millisecond each, in a debug build.
    #[test]
    fn reading_a_line_takes_microseconds() {
        let lines = [
            "I love coffee and I take it black every single morning before work.",
            "Tôi không thích cà phê nữa, nhưng tôi vẫn uống trà.",
            "What's the weather like tomorrow in Hanoi, and should I bring an umbrella?",
        ];
        let began = std::time::Instant::now();
        let mut kept = 0;
        for n in 0..1000 {
            kept += tastes(lines[n % lines.len()]).len();
        }
        let each = began.elapsed() / 1000;
        assert!(kept > 0);
        assert!(
            each < std::time::Duration::from_micros(500),
            "{each:?} a line"
        );
    }
}
