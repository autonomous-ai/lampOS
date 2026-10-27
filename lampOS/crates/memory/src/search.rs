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
