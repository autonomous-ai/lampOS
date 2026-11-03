//! Shared text handling for the optional ranked index and its offline
//! evaluation. The journal keeps the original text; only its rebuildable
//! search representation is normalized here.

use rusqlite::{Connection, params};
use unicode_normalization::UnicodeNormalization;

const QUERY_WORDS: usize = 16;

/// Normalize only the search representation. Vietnamese d-with-stroke is
/// not a diacritic in SQLite's tokenizer, so fold it explicitly. FTS5's
/// `unicode61 remove_diacritics 2` handles the other Vietnamese accents.
pub fn text(value: &str) -> String {
    value
        .nfc()
        .flat_map(char::to_lowercase)
        .map(|character| if character == 'đ' { 'd' } else { character })
        .nfc()
        .collect()
}

/// A literal OR of the first sixteen distinct words of at least two
/// characters. A question never supplies FTS operators, columns, prefixes,
/// or phrases. Bind the returned value as a SQL parameter, not SQL text.
pub fn query(value: &str) -> Option<String> {
    query_with(value, false)
}

/// Words nearly every question has, which on their own match nearly every
/// exchange: "what is my drink order" found one on "my" alone (#2176). In
/// [`text`]'s form, accents kept: "chó" (a dog) is not "cho" (for), nor
/// "đỏ" (red) "đó" (that). No Vietnamese given name: "an" is "An".
const COMMON: &[&str] = &[
    "the", "and", "or", "but", "of", "to", "in", "on", "at", "for", "with", "from", "by", "about",
    "as", "is", "are", "was", "were", "be", "been", "am", "it", "its", "my", "me", "mine", "you",
    "your", "yours", "we", "our", "us", "he", "him", "his", "she", "her", "they", "them", "their",
    "what", "which", "who", "when", "where", "why", "how", "that", "this", "these", "those", "do",
    "does", "did", "have", "has", "had", "can", "could", "would", "should", "will", "any", "some",
    "not", "no", "so", "if", "there", "tell", "told", "remember", "please", "của", "là", "tôi",
    "và", "có", "không", "này", "đó", "thì", "mà", "những", "các", "một", "cho", "với", "được",
    "gì", "nào",
];

/// Whether this word, in [`text`]'s form, is one of [`COMMON`]. Each is
/// put in that form once, not again for every word asked about: a forget
/// checks every word of what it reads (#3871).
fn common(word: &str) -> bool {
    static FORMS: std::sync::OnceLock<std::collections::HashSet<String>> =
        std::sync::OnceLock::new();
    FORMS
        .get_or_init(|| COMMON.iter().map(|known| text(known)).collect())
        .contains(word)
}

/// A fact's distinctive words, in [`text`]'s form, each once: three
/// characters or more, none of [`COMMON`], a final `s` off, and
/// "allergic", "allergy" and "allergies" one word (lamp-06c1, 26 Oct:
/// "forget my peanut allergy" against a kept "allergic to peanuts"). No
/// sixteen-word limit. Only for which rows carry a forgotten fact
/// (`days::selective_descendants`): a lexical hint, not proof of use.
pub(crate) fn content_words(value: &str) -> Vec<String> {
    let normalized = text(value);
    let mut words = Vec::new();
    for word in normalized.split(|character: char| !character.is_alphanumeric()) {
        if word.chars().count() < 3 || common(word) {
            continue;
        }
        let word = if matches!(word, "allergic" | "allergy" | "allergies") {
            "allerg"
        } else {
            word.strip_suffix('s').unwrap_or(word)
        };
        if !words.iter().any(|known| known == word) {
            words.push(word.to_owned());
        }
    }
    words
}

/// Phrases that mark a person's line as a correction of something said
/// before ("Actually, my tea is now peppermint, no longer jasmine"), each a
/// run of whole words, put in [`text`]'s form when compared. English and
/// Vietnamese only. A bare "now" or "giờ" is none: "now I'm brewing jasmine
/// tea for mom" corrects nothing, and "7 giờ" is "7 o'clock" (the
/// Reviewer-lead's HOLD on #5049). "not … anymore" is [`NOT_ANYMORE`].
const CORRECTION_CUES: &[&[&str]] = &[
    &["actually"],
    &["no", "longer"],
    &["changed", "to"],
    &["moved", "to"],
    &["now", "it", "s"],
    &["now", "it", "is"],
    &["instead"],
    &["thật", "ra"],
    &["không", "còn"],
    &["đổi", "sang"],
    &["chuyển", "sang"],
    &["bây", "giờ", "là"],
    &["thay", "vì"],
];

/// "not … anymore" (also "n't", "any more"): a "not" or the "t" of "n't",
/// then "anymore" later in the same line.
fn not_anymore(words: &[&str]) -> bool {
    words
        .iter()
        .position(|word| matches!(*word, "not" | "t"))
        .is_some_and(|not| {
            let rest = &words[not + 1..];
            rest.contains(&"anymore") || rest.windows(2).any(|run| run == ["any", "more"])
        })
}

/// Whether `value` carries one of [`CORRECTION_CUES`] as whole words, or
/// "not … anymore".
fn corrects(value: &str) -> bool {
    static CUES: std::sync::OnceLock<Vec<Vec<String>>> = std::sync::OnceLock::new();
    let cues = CUES.get_or_init(|| {
        CORRECTION_CUES
            .iter()
            .map(|cue| cue.iter().map(|word| text(word)).collect())
            .collect()
    });
    let normalized = text(value);
    let words: Vec<&str> = normalized
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    not_anymore(&words)
        || cues
            .iter()
            .any(|cue| words.windows(cue.len()).any(|run| run == cue.as_slice()))
}

/// [`content_words`] without the words a correction says it with (the
/// cues', "now", "giờ", "anymore"), so two lines that both say "actually"
/// or "now" share nothing by it.
fn topic_words(value: &str) -> Vec<String> {
    static SAYING: std::sync::OnceLock<std::collections::HashSet<String>> =
        std::sync::OnceLock::new();
    let saying = SAYING.get_or_init(|| {
        CORRECTION_CUES
            .iter()
            .flat_map(|cue| cue.iter())
            .chain(&["now", "giờ", "anymore"])
            .map(|word| text(word))
            .collect()
    });
    content_words(value)
        .into_iter()
        .filter(|word| !saying.contains(word))
        .collect()
}

/// The voice's `LAMP_MEMORY_NEWER_FIRST` (`newer_first`): for each of
/// `lines` (`(at_unix_ms, id, the person's own words)`, all one person's),
/// the index of the newest later line that corrects it. A later line
/// corrects an earlier one when it carries a [`CORRECTION_CUES`] phrase and
/// is about the same fact: it shares at least two of the earlier line's
/// [`topic_words`], and every one of them but at most one, the value that
/// changed ("it is jasmine tea", then "Actually, my tea is now peppermint,
/// no longer jasmine"). Two shared words alone are cheap, a Vietnamese
/// syllable each: "Tôi uống trà nhài mỗi sáng" is not corrected by "thật
/// ra 7 giờ tôi uống trà nhài với mẹ" (two left out). Later is a larger
/// `at_unix_ms`, or the same and a larger id. Only the person's words
/// count, never the lamp's, and never the room's lines, whose speakers
/// differ ([`crate::days::Days`] asks for no room order). Adapted from
/// memory-tiers' "newer owner evidence only" supersession (Tiers,
/// `tiers/recall-adapter`, `memory-tiers.md`), rewritten as a read-time
/// order: nothing is retired.
pub(crate) fn corrected_by(lines: &[(u64, u64, &str)]) -> Vec<Option<usize>> {
    let topics: Vec<Vec<String>> = lines
        .iter()
        .map(|(_, _, words)| topic_words(words))
        .collect();
    let cued: Vec<bool> = lines.iter().map(|(_, _, words)| corrects(words)).collect();
    let same_fact = |older: usize, newer: usize| {
        let shared = topics[older]
            .iter()
            .filter(|word| topics[newer].contains(word))
            .count();
        shared >= 2 && shared + 1 >= topics[older].len()
    };
    (0..lines.len())
        .map(|older| {
            (0..lines.len())
                .filter(|&newer| {
                    cued[newer]
                        && (lines[newer].0, lines[newer].1) > (lines[older].0, lines[older].1)
                        && same_fact(older, newer)
                })
                .max_by_key(|&newer| (lines[newer].0, lines[newer].1))
        })
        .collect()
}

/// The order `newer_first` gives lines ranked as `corrected_by` came:
/// as ranked, but each line's correction (and its own, in turn) just
/// before it when it was ranked below it. A correction is strictly later
/// than what it corrects, so the walk ends.
pub(crate) fn newer_first(corrected_by: &[Option<usize>]) -> Vec<usize> {
    fn emit(
        line: usize,
        corrected_by: &[Option<usize>],
        done: &mut [bool],
        order: &mut Vec<usize>,
    ) {
        if done[line] {
            return;
        }
        if let Some(newer) = corrected_by[line] {
            emit(newer, corrected_by, done, order);
        }
        done[line] = true;
        order.push(line);
    }
    let mut done = vec![false; corrected_by.len()];
    let mut order = Vec::with_capacity(corrected_by.len());
    for line in 0..corrected_by.len() {
        emit(line, corrected_by, &mut done, &mut order);
    }
    order
}

/// Whether `value`, in [`text`]'s form, may have a character the search
/// index keeps inside a word where [`content_words`] ends one: a combining
/// mark that is no letter ("İ" lower-cased is "i" and a dot above, so the
/// index holds "istanbul" where the words say "stanbul": Fourteen on
/// #3902), or a private-use character. The index may not find such a
/// row's words, so a forget reads it whole. Asked of each character of
/// `value` on its own, each kind once: what [`text`] makes of a whole
/// line only joins marks, never adds one no character has. Plain ASCII
/// never has one.
pub(crate) fn splits_unlike_index(value: &str) -> bool {
    thread_local! {
        static SEEN: std::cell::RefCell<std::collections::HashMap<char, bool>> =
            std::cell::RefCell::default();
    }
    let unlike = |character: char| {
        !character.is_alphanumeric()
            && (unicode_normalization::char::is_combining_mark(character)
                || matches!(u32::from(character), 0xe000..=0xf8ff | 0xf0000..=0x10ffff))
    };
    !value.is_ascii()
        && SEEN.with_borrow_mut(|seen| {
            value.chars().filter(|c| !c.is_ascii()).any(|character| {
                *seen.entry(character).or_insert_with(|| {
                    unlike(character)
                        || text(character.encode_utf8(&mut [0; 4])).chars().any(unlike)
                })
            })
        })
}

/// [`query`], or with `skip_common` the same OR without [`COMMON`] words,
/// the first sixteen of the rest: a question of only those searches
/// nothing.
pub fn query_with(value: &str, skip_common: bool) -> Option<String> {
    let normalized = text(value);
    let words = collect_words_with(&normalized, skip_common);
    if words.is_empty() {
        return None;
    }
    Some(
        words
            .iter()
            .map(|word| format!("\"{word}\""))
            .collect::<Vec<_>>()
            .join(" OR "),
    )
}

/// The legacy query tokenizer, deliberately without Unicode normalization
/// or accent folding. Keep this separate so the baseline stays unchanged.
pub fn words(value: &str) -> Vec<String> {
    collect_words(&value.to_lowercase())
}

fn collect_words(value: &str) -> Vec<String> {
    collect_words_with(value, false)
}

fn collect_words_with(value: &str, skip_common: bool) -> Vec<String> {
    let mut words = Vec::new();
    for word in value.split(|character: char| !character.is_alphanumeric()) {
        if word.chars().count() >= 2
            && !words.iter().any(|known| known == word)
            && !(skip_common && common(word))
        {
            words.push(word.to_owned());
            if words.len() == QUERY_WORDS {
                break;
            }
        }
    }
    words
}

/// The legacy score: how many distinct query words occur in any supplied
/// text, irrespective of term frequency. `words` supplies at most sixteen
/// words; ignore extras even if another caller supplies a longer slice.
pub fn matched(query_words: &[String], texts: &[&str]) -> u32 {
    let mut seen = 0_u32;
    for text in texts {
        for word in text
            .to_lowercase()
            .split(|character: char| !character.is_alphanumeric())
        {
            if let Some(position) = query_words
                .iter()
                .take(QUERY_WORDS)
                .position(|candidate| candidate == word)
            {
                seen |= 1 << position;
            }
        }
    }
    seen.count_ones()
}

/// A disposable, in-memory index used by the offline evaluator. It has no
/// persistence or access to memory's files, and receives bodies without
/// benchmark labels. The live store owns its own index lifecycle.
pub struct SearchIndex {
    connection: Connection,
}

impl SearchIndex {
    pub fn new() -> rusqlite::Result<Self> {
        let connection = Connection::open_in_memory()?;
        connection.execute_batch(
            "PRAGMA temp_store = MEMORY;
             CREATE VIRTUAL TABLE search USING fts5(
                 body, tokenize='unicode61 remove_diacritics 2'
             );",
        )?;
        Ok(Self { connection })
    }

    pub fn add(&self, id: i64, body: &str) -> rusqlite::Result<()> {
        self.connection.execute(
            "INSERT INTO search(rowid, body) VALUES (?1, ?2)",
            params![id, text(body)],
        )?;
        Ok(())
    }

    /// Best matches first; a larger id breaks equal scores. The evaluator
    /// assigns increasing ids in chronological order, as the legacy search
    /// prefers newer entries on a tie. An empty query returns no results.
    pub fn rank(&self, value: &str, limit: usize) -> rusqlite::Result<Vec<i64>> {
        let Some(query) = query(value) else {
            return Ok(Vec::new());
        };
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut statement = self.connection.prepare_cached(
            "SELECT rowid FROM search WHERE search MATCH ?1
             ORDER BY rank, rowid DESC LIMIT ?2",
        )?;
        statement
            .query_map(params![query, limit], |row| row.get(0))?
            .collect()
    }
}

#[cfg(test)]
mod newer_first_tests {
    use super::{corrected_by, corrects, newer_first};

    #[test]
    fn cues_are_whole_words() {
        assert!(corrects("Actually, my tea is now peppermint"));
        assert!(corrects("I no longer drink jasmine"));
        assert!(corrects("my tea changed to peppermint"));
        assert!(corrects("the meeting moved to Monday"));
        assert!(corrects("now it's peppermint"));
        assert!(corrects("peppermint instead"));
        assert!(corrects("I don't drink jasmine anymore"));
        assert!(corrects("I do not drink jasmine any more"));
        assert!(corrects("Thật ra tôi uống trà bạc hà"));
        assert!(corrects("tôi không còn uống trà nhài"));
        assert!(corrects("tôi đổi sang trà bạc hà"));
        assert!(corrects("tôi chuyển sang trà bạc hà"));
        assert!(corrects("trà của tôi bây giờ là bạc hà"));
        assert!(corrects("trà bạc hà thay vì trà nhài"));
        assert!(!corrects("I know my tea is jasmine"));
        assert!(!corrects("a longer walk, then home"));
        assert!(!corrects("an unchanged order"));
        // A bare "now" or "giờ" is no cue (the HOLD on #5049).
        assert!(!corrects("now I'm brewing jasmine tea for mom"));
        assert!(!corrects("7 giờ tôi uống trà nhài với mẹ"));
        assert!(!corrects("Giờ tôi uống trà bạc hà"));
        assert!(!corrects("I changed my mind"));
        assert!(!corrects("anymore, not now"));
    }

    /// The Reviewer-lead's two scenarios on #5049: neither earlier line is
    /// marked, by the cue alone; nor with a real cue put in, by the fact.
    #[test]
    fn bare_now_or_gio_marks_nothing() {
        for lines in [
            [
                (10, 1, "I drink jasmine tea"),
                (20, 2, "now I'm brewing jasmine tea for mom"),
            ],
            [
                (10, 1, "Tôi uống trà nhài mỗi sáng"),
                (20, 2, "7 giờ tôi uống trà nhài với mẹ"),
            ],
            [
                (10, 1, "Tôi uống trà nhài mỗi sáng"),
                (20, 2, "thật ra 7 giờ tôi uống trà nhài với mẹ"),
            ],
        ] {
            assert_eq!(corrected_by(&lines), [None, None], "{lines:?}");
        }
    }

    #[test]
    fn a_vietnamese_correction_is_one() {
        let lines = [
            (10, 1, "Tôi uống trà nhài"),
            (20, 2, "Thật ra tôi uống trà bạc hà, không còn trà nhài"),
        ];
        assert_eq!(corrected_by(&lines), [Some(1), None]);
    }

    #[test]
    fn a_correction_goes_before_what_it_corrects() {
        let lines = [
            (10, 1, "it is jasmine tea"),
            (
                20,
                2,
                "Actually, my tea is now peppermint, no longer jasmine",
            ),
            (15, 3, "the dentist is on Friday"),
        ];
        let corrected = corrected_by(&lines);
        assert_eq!(corrected, [Some(1), None, None]);
        assert_eq!(newer_first(&corrected), [1, 0, 2]);
    }

    #[test]
    fn one_word_shared_or_an_older_cue_changes_nothing() {
        // One topic word only ("tea"), and a cue on the older line.
        let lines = [(10, 1, "Actually my jasmine tea"), (20, 2, "my tea now")];
        assert_eq!(corrected_by(&lines), [None, None]);
        assert_eq!(newer_first(&[None, None]), [0, 1]);
    }

    #[test]
    fn a_chain_goes_newest_first() {
        let lines = [
            (10, 1, "my bike is a red bicycle"),
            (20, 2, "actually my bike is now a blue bicycle"),
            (30, 3, "actually my bike is now a green bicycle"),
        ];
        let corrected = corrected_by(&lines);
        assert_eq!(corrected, [Some(2), Some(2), None]);
        assert_eq!(newer_first(&corrected), [2, 0, 1]);
    }
}
