//! A live figure said with no lookup behind it is never said as it is
//! (`LAMP_LIVE_FACT_CHECKED`, #7605).
//!
//! On lamp-9adf (18 Nov 01:04–01:17, build 13929) "Lamp, what is the gold
//! price today?" was answered "around $4,050 an ounce" with `calls=none` and
//! no search; "what is the weather like in Da Nang right now?" and "And what
//! about Da Nang tomorrow?" were answered from Ho Chi Minh City's weather of
//! earlier turns, also with `calls=none`. The action-claim check
//! (`crate::action_claim`) holds actions only; a fact said as if looked up
//! had no check.
//!
//! Strict first (the lesson of #3477): a sentence of the reply is judged
//! by its own words alone, never the person's (the reviewer-lead's HOLD on
//! #7719: no word list over the person's line). It is held only when both
//! hold.
//! - **The sentence states a live figure** ([`states_figure`]): a price
//!   (digits with a currency sign or word, or a gold unit: "$4,050 an
//!   ounce", "4.5 million dong a tael"); degrees or wind beside a weather
//!   word, a live marker (today, now, current, currently, tonight,
//!   tomorrow, latest, this morning, this afternoon, this evening, at the
//!   moment) or a weather opening ("It's", "Expect", "Highs"); a percent or
//!   points beside a market's word (up, down, rate, index, …) or a live
//!   marker; digits written as a score ("2-1") beside a result's word (won,
//!   lost, leads, …; never "beat", a recipe's word) or a live marker; or a weather condition with a place
//!   named ("It's sunny in Da Nang."). Number words count with their unit
//!   ("thirty degrees", "sixty-seven dollars"). A time, a date, a count, a
//!   recipe's weight or oven heat, and "100% sure" are none.
//! - **No lookup in the turn found it** ([`Hold::ran`], [`found`]): a
//!   `web_search` or `fetch_page` whose answer has results, a page's words
//!   or a price backs any figure; a `weather` answer with the weather backs
//!   only the weather's figures, never a price. An error or an empty search
//!   (`{"results": []}`) backs nothing, nor results whose every item has a
//!   blank title and text, nor a page read with a status of 400 or more
//!   or an error page's title ("404 Not Found"). The chat's own web search
//!   (OpenRouter's plugin, `ChatEvent::Searched`) backs any. An early search counts only
//!   once the model's own `web_search` takes it. A cut answer's carried
//!   lookup counts only when its answer found something. Another turn's
//!   lookup is not this turn's: Ho Chi Minh City's weather fetched earlier
//!   never backs Da Nang's now.
//!
//! The first such sentence and the rest of its round are not said, nor kept
//! as the lamp's words. With no call in the round, the model is asked once
//! more ([`note`]), with the lookup required (`tool_choice`, [`force`]):
//! `weather` when the held words speak of weather and it is declared, else
//! `web_search`. A second such reply, a lookup not declared,
//! or one in the last round, is said as [`COULD_NOT_LINE`]. Beside other
//! calls those run and the next round is told the same note. A sentence
//! said in the round that calls a lookup ran ahead of it: not said, no
//! note, and the next round speaks from what it found. Only English is
//! matched, as the action claims are. Until a search found something, a
//! reply's first words wait for their sentence's end (no clause or
//! first-words cut) only on a line the person asked for a live fact
//! ([`asks_live`]: a live marker, a price, rate or score word or a weather
//! word; the first builder on #8224, rule 7), so a figure
//! is judged whole; any other line's are cut as with the switch off, even
//! when they hold a number ("Your timer is set for 5 minutes"). A piece cut off
//! before its sentence's end leads the rest of it when that is judged.
//! A search or a page read backs every figure in the turn, and so does
//! the chat's own search (`ChatEvent::Searched`), on no topic of its own.
//! On a line that asked no live fact, once some of the turn's words were
//! said, a figure with no number in it (a condition with a place) is said
//! as it is, with no ask again (#8615, the lead's choice (d')): a story's
//! "sunny in Da Nang" is no answer to look up, and its ask again left
//! seconds of silence inside the reply. A figure with a number stays held.
//! Off unless the
//! environment says exactly `on`, and off the turn is as before.

use serde_json::Value;

/// Where the second worker reads the switch. `on` only.
pub const ENVIRONMENT: &str = "LAMP_LIVE_FACT_CHECKED";

/// Whether the switch is on in this process's environment.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// The functions that look a live fact up.
pub const LOOKUPS: [&str; 3] = [
    crate::second_provider::net::search::NAME,
    crate::second_provider::net::search::FETCH,
    crate::second_provider::net::weather::NAME,
];

/// Whether `name` looks a live fact up.
pub fn is_lookup(name: &str) -> bool {
    LOOKUPS.contains(&name)
}

/// How the note to ask again opens: [`force`] knows it by these words.
pub const NOTE_OPENS: &str = "(For you to know: your last reply gave a live figure";

/// What the model is told when its figure was held: call `tool` now.
pub fn note(tool: &str) -> String {
    format!(
        "{NOTE_OPENS} (a price, a rate, a score or the weather) that nothing in this turn looked up, so it was not said. Call {tool} now to look it up, and answer only from what it finds. An earlier turn's lookup, of another place or day, is not this one. If it finds nothing, say plainly that you couldn't look it up, and never give a figure you didn't find.)"
    )
}

/// [`note`], told the words of the held round already said (`said`), so the
/// reply asked again goes on from them and never says them again (#8224's
/// lamp row: "In Hanoi right now" was said, and the reply asked again began
/// with it once more).
pub fn note_after(tool: &str, said: &str) -> String {
    let said = said.trim();
    let note = note(tool);
    if said.is_empty() {
        return note;
    }
    format!(
        "{} You already said \"{said}\" out loud: go on from those words, and never say them again.)",
        note.trim_end_matches(')')
    )
}

/// The lamp's own words that end a piece it said before a hold when the
/// reply asked again doesn't go on from it: "Silver's around … let me
/// check. Silver was around $45 an ounce." Once a turn, never naming a
/// tool (the first builder's c7066116904 on #8224, after the 06c1 S1 leg
/// said "Silver's around 52 to Silver was around $45"). None when nothing
/// was said before the hold, when what was said ended a sentence, or when
/// the reply goes on from it (its words are dropped instead).
pub const BRIDGE: &str = "\u{2026} let me check.";

/// What the lamp says when a live figure could not be looked up.
pub const COULD_NOT_LINE: &str = "Sorry, I couldn't look that up just now.";

/// Which lookup a held figure asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lookup {
    Weather,
    Search,
}

impl Lookup {
    /// The function's name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Weather => crate::second_provider::net::weather::NAME,
            Self::Search => crate::second_provider::net::search::NAME,
        }
    }
}

/// Live markers, one word, in the reply's own sentence.
const LIVE_WORDS: [&str; 10] = [
    "today",
    "today's",
    "now",
    "current",
    "currently",
    "tonight",
    "tonight's",
    "tomorrow",
    "tomorrow's",
    "latest",
];

/// Live markers, two words, in the reply's own sentence.
const LIVE_PHRASES: [&str; 4] = [
    "this morning",
    "this afternoon",
    "this evening",
    "the moment",
];

/// Words that make a percent or points a market's figure ("Bitcoin is up
/// 3%", "the index fell 12 points").
const MARKET_WORDS: [&str; 22] = [
    "up",
    "down",
    "rose",
    "risen",
    "fell",
    "fallen",
    "gained",
    "lost",
    "dropped",
    "climbed",
    "rate",
    "rates",
    "inflation",
    "interest",
    "index",
    "stock",
    "stocks",
    "shares",
    "price",
    "prices",
    "market",
    "markets",
];

/// Words that make digits like "2-1" a match's score, not a range.
/// "Beat" is no result's word: "Beat 2-3 eggs" is a recipe's (Twelve's P3
/// on #7719).
const RESULT_WORDS: [&str; 10] = [
    "won", "win", "wins", "lost", "drew", "draw", "leads", "leading", "score", "scored",
];

/// How a sentence that gives the weather opens ("It's about thirty
/// degrees.", "Expect highs of 30 degrees").
const WEATHER_OPENS: [&str; 7] = ["it's", "it", "expect", "highs", "lows", "high", "low"];

/// Words of the weather, in a line or a reply.
const WEATHER_WORDS: [&str; 23] = [
    "weather",
    "temperature",
    "temperatures",
    "forecast",
    "rain",
    "rainy",
    "raining",
    "showers",
    "drizzle",
    "snow",
    "snowing",
    "humid",
    "humidity",
    "sunny",
    "cloudy",
    "overcast",
    "foggy",
    "thunderstorm",
    "thunderstorms",
    "storm",
    "storms",
    "hot",
    "degrees",
];

/// Weather conditions, which with a place named are a figure.
const CONDITIONS: [&str; 18] = [
    "sunny",
    "cloudy",
    "overcast",
    "rain",
    "rainy",
    "raining",
    "showers",
    "drizzle",
    "thunderstorm",
    "thunderstorms",
    "storm",
    "storms",
    "humid",
    "fog",
    "foggy",
    "snow",
    "snowing",
    "windy",
];

/// Words a figure's unit may follow ("4,050 an ounce", "4.5 million dong").
const BEFORE_UNIT: [&str; 7] = [
    "a", "an", "per", "thousand", "million", "billion", "trillion",
];

/// What a figure's unit makes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// A price or a rate: a figure in itself.
    Money,
    /// Degrees or wind: a figure beside a weather word, a live marker, or
    /// an opening that gives the weather.
    Weather,
    /// A percent or points: a figure beside a market's word or a live
    /// marker.
    Market,
    /// Digits like "2-1": a figure beside a result's word or a live marker.
    Score,
}

/// Units and currency words said after a number, and what they make it.
/// A weight alone ("200 grams") is a recipe's, not a price's: only the
/// gold units, which come with a price, are here.
const UNITS: [(&str, Kind); 36] = [
    ("percent", Kind::Market),
    ("per cent", Kind::Market),
    ("points", Kind::Market),
    ("degrees", Kind::Weather),
    ("degree", Kind::Weather),
    ("°c", Kind::Weather),
    ("°f", Kind::Weather),
    ("celsius", Kind::Weather),
    ("fahrenheit", Kind::Weather),
    ("mph", Kind::Weather),
    ("km/h", Kind::Weather),
    ("dollars", Kind::Money),
    ("dollar", Kind::Money),
    ("usd", Kind::Money),
    ("euros", Kind::Money),
    ("euro", Kind::Money),
    ("eur", Kind::Money),
    ("pounds", Kind::Money),
    ("gbp", Kind::Money),
    ("yen", Kind::Money),
    ("jpy", Kind::Money),
    ("yuan", Kind::Money),
    ("cny", Kind::Money),
    ("baht", Kind::Money),
    ("dong", Kind::Money),
    ("đồng", Kind::Money),
    ("vnd", Kind::Money),
    ("rupees", Kind::Money),
    ("cents", Kind::Money),
    ("ounce", Kind::Money),
    ("ounces", Kind::Money),
    ("tael", Kind::Money),
    ("taels", Kind::Money),
    ("ringgit", Kind::Money),
    ("rupiah", Kind::Money),
    ("pesos", Kind::Money),
];

/// Currency words said before a number ("USD 4,050").
const CURRENCY_BEFORE: [&str; 5] = ["usd", "vnd", "eur", "us$", "$"];

/// Currency signs within a number's word.
const CURRENCY_SIGNS: [char; 9] = ['$', '€', '£', '¥', '₫', '₹', '₩', '฿', '₽'];

/// Number words that, with a unit, are a figure ("thirty degrees",
/// "sixty-seven dollars").
const NUMBER_WORDS: [&str; 33] = [
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
    "twenty",
    "thirty",
    "forty",
    "fifty",
    "sixty",
    "seventy",
    "eighty",
    "ninety",
    "hundred",
    "thousand",
    "million",
    "billion",
    "half",
];

/// Capitalized words that name no place.
const NOT_PLACES: [&str; 26] = [
    "I",
    "I'm",
    "I'll",
    "I've",
    "I'd",
    "Lamp",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
    "OK",
];

/// `text` after its first `n` words as [`words`] counts them, cut at a word's
/// end, with the space and marks that follow it.
fn after_words(text: &str, n: usize) -> &str {
    let is_word = |c: char| c.is_alphanumeric() || matches!(c, '\'' | '\u{2019}' | '\u{2018}');
    let mut count = 0;
    let mut in_word = false;
    for (at, c) in text.char_indices() {
        if is_word(c) {
            in_word = true;
        } else if std::mem::replace(&mut in_word, false) {
            count += 1;
            if count == n {
                return text[at..].trim_start_matches(|c: char| {
                    c.is_whitespace()
                        || matches!(c, ',' | ';' | ':' | '-' | '\u{2013}' | '\u{2014}')
                });
            }
        }
    }
    if n == 0 { text } else { "" }
}

/// Whether `text` ends a sentence: `.`, `!`, `?` or `…`, past closing
/// quotes and brackets.
fn ends_sentence(text: &str) -> bool {
    text.trim_end()
        .trim_end_matches(['"', '\'', '\u{201d}', '\u{2019}', ')'])
        .ends_with(['.', '!', '?', '\u{2026}'])
}

/// The words of `text`, lower case, curly apostrophes made straight.
fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .replace(['\u{2019}', '\u{2018}'], "'")
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Whether held words speak of the weather.
pub fn speaks_of_weather(text: &str) -> bool {
    text.contains('°')
        || words(text)
            .iter()
            .any(|word| WEATHER_WORDS.contains(&word.as_str()))
}

/// A word of `sentence`, its edge marks taken off.
fn bare(word: &str) -> &str {
    word.trim_matches(|c: char| {
        matches!(
            c,
            '.' | ',' | ';' | ':' | '!' | '?' | '(' | ')' | '"' | '\u{201c}' | '\u{201d}'
        )
    })
}

/// Whether `word` is digits written as a score ("2-1", "3–0").
fn is_score(word: &str) -> bool {
    let mut sides = word.split(['-', '\u{2013}']);
    let (Some(left), Some(right), None) = (sides.next(), sides.next(), sides.next()) else {
        return false;
    };
    let short = |side: &str| {
        !side.is_empty() && side.len() <= 2 && side.chars().all(|c| c.is_ascii_digit())
    };
    short(left) && short(right)
}

/// The unit that follows at `from` in `lower`, past at most two of
/// [`BEFORE_UNIT`].
fn unit_at(lower: &[String], from: usize) -> Option<Kind> {
    for at in (from..).take(3) {
        let word = lower.get(at)?;
        let two = lower
            .get(at + 1)
            .map(|next| format!("{word} {next}"))
            .unwrap_or_default();
        if let Some((_, kind)) = UNITS
            .iter()
            .find(|(unit, _)| *unit == word.as_str() || *unit == two.as_str())
        {
            return Some(*kind);
        }
        if !BEFORE_UNIT.contains(&word.as_str()) {
            return None;
        }
    }
    None
}

/// What the word at `at` of `lower` is as a figure, if one.
fn figure_at(lower: &[String], at: usize) -> Option<Kind> {
    let word = &lower[at];
    if word.chars().any(|c| c.is_ascii_digit()) {
        if word.chars().any(|c| CURRENCY_SIGNS.contains(&c))
            || at
                .checked_sub(1)
                .is_some_and(|before| CURRENCY_BEFORE.contains(&lower[before].as_str()))
        {
            return Some(Kind::Money);
        }
        if word.ends_with('%') {
            return Some(Kind::Market);
        }
        if word.contains('°') {
            return Some(Kind::Weather);
        }
        if is_score(word) {
            return Some(Kind::Score);
        }
        return unit_at(lower, at + 1);
    }
    // "thirty", "sixty-seven": every part a number word.
    word.split('-')
        .all(|part| NUMBER_WORDS.contains(&part))
        .then(|| unit_at(lower, at + 1))
        .flatten()
}

/// Whether a sentence of a reply states a live figure, judged by the
/// sentence alone, never by the person's words (the reviewer-lead's HOLD
/// on #7719). A price is one; so are degrees or wind beside a weather
/// word, a live marker or a weather opening; a percent or points beside a
/// market's word or a live marker; a score beside a result's word or a
/// live marker; and a weather condition with a place named.
pub fn states_figure(sentence: &str) -> bool {
    let raw: Vec<&str> = sentence
        .split_whitespace()
        .map(bare)
        .filter(|word| !word.is_empty())
        .collect();
    let lower: Vec<String> = raw
        .iter()
        .map(|word| word.to_lowercase().replace(['\u{2019}', '\u{2018}'], "'"))
        .collect();
    let has = |list: &[&str]| {
        lower
            .iter()
            .any(|word| list.contains(&word.trim_end_matches("'s")))
    };
    let joined = format!(" {} ", lower.join(" "));
    let live = has(&LIVE_WORDS)
        || LIVE_PHRASES
            .iter()
            .any(|phrase| joined.contains(&format!(" {phrase} ")));
    let weather = live
        || lower
            .iter()
            .any(|word| word != "degrees" && WEATHER_WORDS.contains(&word.as_str()))
        || lower
            .first()
            .is_some_and(|first| WEATHER_OPENS.contains(&first.as_str()));
    let figured = (0..lower.len()).any(|at| match figure_at(&lower, at) {
        Some(Kind::Money) => true,
        Some(Kind::Weather) => weather,
        Some(Kind::Market) => live || has(&MARKET_WORDS),
        Some(Kind::Score) => live || has(&RESULT_WORDS),
        None => false,
    });
    if figured {
        return true;
    }
    let condition = lower
        .iter()
        .any(|word| CONDITIONS.contains(&word.trim_end_matches("'s")));
    let place = raw.iter().skip(1).any(|word| {
        word.chars().next().is_some_and(char::is_uppercase) && !NOT_PLACES.contains(word)
    });
    condition && place
}

/// Words of the person's line that ask for a live fact: a price, a rate, a
/// score or a market's figure. With [`LIVE_WORDS`], [`LIVE_PHRASES`] and
/// [`WEATHER_WORDS`] they make a line one whose answer's first words wait
/// ([`asks_live`]).
const ASKING_WORDS: [&str; 14] = [
    "price", "prices", "rate", "rates", "score", "scores", "stock", "stocks", "market", "markets",
    "index", "exchange", "bitcoin", "gold",
];

/// Whether the person's line asks for a live fact, as #7605's done-when
/// says, strict first (the first builder on #8224, rule 7): a live marker
/// (today, now, current, latest, tomorrow, …), a price, rate or score
/// word, or a weather word. A place alone counts for nothing (the first
/// builder, c7063146174): "Hey, Lamp. Set a timer for 5 minutes.", "Ask
/// Mom what she wants." and "Play Spotify." never wait, and a place-only
/// line's figure is still caught by the sentence's own check. Only the
/// first-words wait reads it ([`Hold::holds_back`]): every sentence of the
/// reply is still judged by its own words alone, whatever the line.
///
/// A word list over the person's line, named as an allowed exception to
/// the owner's rule against word lists, as `action_claim::WORDED` is: it
/// decides only when the first words may be cut, never what is said, and
/// it fails safe, since a line it misses is still judged sentence by
/// sentence and its first piece is cut before any number
/// ([`is_number`]).
pub fn asks_live(line: &str) -> bool {
    asks_live_by(line, true)
}

/// A question's opening words, folded as [`words`] folds them: a weather
/// condition after one, or in a line that opens with one, is asked
/// (V2's ruling on #8615, 26 Nov 18:35).
const QUESTION_OPENERS: [&str; 8] = [
    "will it", "is it", "does it", "how's", "how is", "what's", "what is", "should i",
];

/// What a line may open with before its question: the call.
const CALLS: [&str; 4] = ["hey", "hi", "ok", "lamp"];

/// Whether the person's line asked a live fact, for [`Hold::lets_through`]
/// (#8615's (d')): as [`asks_live`], but a weather condition alone counts
/// only in a question: the line has a '?', opens with one of
/// [`QUESTION_OPENERS`] (past the lamp's call), or has the condition right
/// after one ("tell me, will it be sunny"), "be" between them allowed.
/// Speech to text drops the '?' too often to read it alone. kit-8646's
/// "tell me a short story about a cat who goes to the beach in Da Nang
/// when it's sunny." names the story's setting and asks nothing live
/// (lamp-52e6, 26 Nov, build 16293: its "sunny" figure was held after the
/// first piece's audio began, and the story asked again). Known limit: a
/// question put another way with no '?' ("I wonder if it's sunny in
/// Hanoi") asks none, so its numberless condition is said once words are
/// out.
pub fn asks_live_fact(line: &str) -> bool {
    asks_live_by(line, line.contains('?') || asks_a_question(line))
}

/// Whether `line` opens with a question's words, or a weather condition
/// sits right after them ([`QUESTION_OPENERS`]).
fn asks_a_question(line: &str) -> bool {
    let lower = words(line);
    let start = lower
        .iter()
        .take_while(|word| CALLS.contains(&word.as_str()))
        .count();
    let opening = format!("{} ", lower[start..].join(" "));
    let joined = format!(" {} ", lower.join(" "));
    QUESTION_OPENERS.iter().any(|opener| {
        opening.starts_with(&format!("{opener} "))
            || CONDITIONS.iter().any(|condition| {
                joined.contains(&format!(" {opener} {condition} "))
                    || joined.contains(&format!(" {opener} be {condition} "))
            })
    })
}

/// [`asks_live`]'s words, a weather condition ([`CONDITIONS`], some of them
/// [`WEATHER_WORDS`] too) counted only with `conditions`.
fn asks_live_by(line: &str, conditions: bool) -> bool {
    let lower = words(line);
    let joined = format!(" {} ", lower.join(" "));
    lower.iter().any(|word| {
        let word = word.trim_end_matches("'s");
        let condition = CONDITIONS.contains(&word);
        LIVE_WORDS.contains(&word)
            || WEATHER_WORDS.contains(&word) && !condition
            || conditions && condition
            || ASKING_WORDS.contains(&word)
    }) || LIVE_PHRASES
        .iter()
        .any(|phrase| joined.contains(&format!(" {phrase} ")))
}

/// Whether `word` is a bare number or a number word ("31", "$4,050",
/// "thirty-one"): while the switch watches, a reply's first piece is cut
/// before the first one, never with it, so "Silver's around 52 to" is said
/// as "Silver's around" and the rest is judged whole at its sentence's end
/// (the 06c1 S1 leg on #8224; Twelve's HOLD there).
pub fn is_number(word: &str) -> bool {
    let word = bare(word).to_lowercase();
    !word.is_empty()
        && (word
            .chars()
            .any(|c| c.is_ascii_digit() || CURRENCY_SIGNS.contains(&c))
            || word.split('-').all(|part| NUMBER_WORDS.contains(&part)))
}

/// Whether a lookup's answer found something: no error and no failed
/// status, and results, a page's words, the weather or a price in it. An
/// empty search (`{"results": []}`) backs nothing (Twelve's P2 on #7719),
/// nor results whose every item has a blank title and text, nor a page
/// read with a failed status or whose title is an error page's ("404 Not
/// Found", "Access Denied"; Twelve's P3s on #7719).
pub fn found(told: &Value) -> bool {
    if told.get("error").is_some() || failed_status(told) || error_page(told) {
        return false;
    }
    ["results", "content", "weather", "price"]
        .iter()
        .any(|key| match told.get(*key) {
            None | Some(Value::Null) => false,
            Some(Value::Array(items)) if *key == "results" => items.iter().any(filled),
            Some(Value::Array(items)) => !items.is_empty(),
            Some(Value::String(text)) => !text.trim().is_empty(),
            Some(Value::Object(fields)) => !fields.is_empty(),
            Some(_) => true,
        })
}

/// Whether a search's result has words: a title, text or summary not blank.
fn filled(item: &Value) -> bool {
    match item {
        Value::Object(fields) => ["title", "text", "summary", "content"].iter().any(|key| {
            fields
                .get(*key)
                .and_then(Value::as_str)
                .is_some_and(|text| !text.trim().is_empty())
        }),
        Value::String(text) => !text.trim().is_empty(),
        Value::Null => false,
        _ => true,
    }
}

/// Whether an answer carries an HTTP status of 400 or more, as a number or
/// as its digits (`status`, `status_code`).
fn failed_status(told: &Value) -> bool {
    ["status", "status_code", "statusCode"]
        .iter()
        .filter_map(|key| told.get(*key))
        .any(|status| {
            status
                .as_u64()
                .or_else(|| status.as_str().and_then(|text| text.trim().parse().ok()))
                .is_some_and(|code| code >= 400)
        })
}

/// Titles that make a page read an error page.
const ERROR_TITLES: [&str; 8] = [
    "not found",
    "forbidden",
    "access denied",
    "bad gateway",
    "bad request",
    "service unavailable",
    "too many requests",
    "internal server error",
];

/// Whether a page read is an error page by its title: one of
/// [`ERROR_TITLES`], or a short title (four words at most) with a 4xx or
/// 5xx code in it ("404", "Error 503").
fn error_page(told: &Value) -> bool {
    if told.get("content").is_none() {
        return false;
    }
    let title = told["title"].as_str().unwrap_or("").to_lowercase();
    let title_words: Vec<&str> = title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    ERROR_TITLES.iter().any(|error| title.contains(error))
        || (title_words.len() <= 4
            && title_words.iter().any(|word| {
                word.len() == 3
                    && word
                        .parse::<u16>()
                        .is_ok_and(|code| (400..600).contains(&code))
            }))
}

/// A turn's live-figure hold ([`crate::live_fact`]).
#[derive(Debug, Default)]
pub(crate) struct Hold {
    /// The switch on, on a turn that is judged (not a quoted line, nor a
    /// reminder's).
    on: bool,
    /// Whether a search or a page read found something in the turn: it
    /// backs any figure.
    searched: bool,
    /// Whether the weather was found in the turn: it backs the weather's
    /// figures only, never a price.
    weathered: bool,
    /// Whether `start_agent_task` answered in the turn (#8556): the rounds
    /// after it read the person's own task back, no claim about the world,
    /// so none is judged.
    task_answered: bool,
    /// The sentences held this round, from the first with a figure.
    held: Vec<String>,
    /// What was said of the sentence in hand, when its first words were cut
    /// off before its end: the rest is judged with them, so "The
    /// temperature in Hanoi" said keeps "is 30 degrees." a weather figure.
    lead: String,
    /// Whether the turn was asked again once already.
    pub(crate) asked_again: bool,
    /// When it was asked again: the extra round's time is journalled once
    /// its first words pass, or when it ends in [`COULD_NOT_LINE`].
    asked_at: Option<std::time::Instant>,
    /// Whether the person's line [`asks_live`]: only then do the first
    /// words wait ([`Self::holds_back`]).
    asked_live: bool,
    /// Whether the person's line [`asks_live_fact`]: only then is a
    /// numberless figure after words out still held ([`Self::lets_through`]).
    asked_fact: bool,
    /// What this round said, with the switch on: a round held keeps it as
    /// [`Self::echo`] for the round asked again.
    round_said: Vec<String>,
    /// The held round's said words, as [`words`] folds them: the reply
    /// asked again drops them where it begins with them all, whole words
    /// only, so they are never said twice (#8224's lamp row).
    echo: Vec<String>,
    /// [`Self::echo`] as it was said, for the note and as the lead of what
    /// follows it.
    echo_said: String,
    /// How many of [`Self::echo`]'s words the reply asked again has
    /// repeated so far, its pieces kept in [`Self::echo_kept`] until it is
    /// known whether they repeat them all.
    echo_at: usize,
    echo_kept: Vec<String>,
    /// Where each said piece of [`Self::echo`] begins.
    echo_starts: Vec<usize>,
    /// The held round's said words end mid-sentence: the next words said
    /// that don't go on from them take [`BRIDGE`] first.
    dangling: bool,
    /// [`BRIDGE`] was said this turn: never again.
    bridged: bool,
    /// Some of the turn's words were said (#8615): on a line that asked no
    /// live fact, a figure after them is said as it is.
    words_out: bool,
    /// [`Self::words_out`]'s line was journalled: once a turn.
    let_through: bool,
    /// The reply asked again goes on from the dangling words (their
    /// repeat dropped): no bridge.
    continuing: bool,
    request: u64,
}

impl Hold {
    /// A turn's hold, `on` with the switch on a turn that is judged;
    /// `carried` the finished rounds of a cut answer carried into the turn,
    /// whose lookups are the turn's own when their answers found something.
    pub(crate) fn new(
        on: bool,
        request: u64,
        carried: &[crate::second_provider::net::Message],
    ) -> Self {
        let mut hold = Self {
            on,
            request,
            ..Self::default()
        };
        for call in carried.iter().flat_map(|message| &message.tool_calls) {
            let (Some(id), Some(name)) = (call["id"].as_str(), call["function"]["name"].as_str())
            else {
                continue;
            };
            let told = carried
                .iter()
                .find(|answer| answer.tool_call_id.as_deref() == Some(id))
                .and_then(|answer| answer.words())
                .and_then(|words| serde_json::from_str(words).ok())
                .unwrap_or(Value::Null);
            hold.ran(name, &told);
        }
        hold
    }

    /// Whether sentences are judged before they are said: the switch is
    /// on, and no search found anything yet. While it is, on a line asking
    /// a live fact, a reply's first words wait for their sentence's end
    /// ([`Self::holds_back`]).
    pub(crate) fn watching(&self) -> bool {
        self.on && !self.searched && !self.task_answered
    }

    /// Whether `sentence`'s figure, which no lookup backs, is said as it
    /// is: on a line that asked no live fact ([`asks_live_fact`]), once some of
    /// the turn's words were said, and only when the figure has no number
    /// in it ([`is_number`]: a weather condition with a place). The lead's
    /// choice (d') on #8615, 25 Nov 17:50: kit-10824's story about a cat
    /// held "sunny in Da Nang" after its first piece and asked again with
    /// the weather required, 3.3 s of silence inside the story. A price,
    /// degrees, a score or any figure with a number stays held, as on a
    /// live line `asks_live` misses ("How much is an ounce of silver?",
    /// Twelve's HOLD on #8224); so does any figure on a line that asked
    /// one, or before anything was said.
    fn lets_through(&mut self, sentence: &str) -> bool {
        if self.asked_fact || !self.words_out || sentence.split_whitespace().any(is_number) {
            return false;
        }
        if !std::mem::replace(&mut self.let_through, true) {
            eprintln!(
                "lamp-voice second: request {}: a live figure in a line that asked none, words already out: said as it is (LAMP_LIVE_FACT_CHECKED)",
                self.request
            );
        }
        true
    }

    /// Whether the reply's first words wait for their sentence's end:
    /// while [`Self::watching`], and only on a line that [`asks_live`]
    /// (the first builder on #8224, rule 7). Any other line's are cut as
    /// with the switch off, whatever number they hold.
    pub(crate) fn holds_back(&self) -> bool {
        self.watching() && self.asked_live
    }

    /// The person's line, `line`: whether it [`asks_live`].
    #[cfg(test)]
    pub(crate) fn asked(self, line: &str) -> Self {
        self.asking_live(asks_live(line), asks_live_fact(line))
    }

    /// Whether the person's line [`asks_live`] and [`asks_live_fact`],
    /// read before it was moved.
    pub(crate) fn asking_live(mut self, live: bool, fact: bool) -> Self {
        self.asked_live = live;
        self.asked_fact = fact;
        self
    }

    /// Whether this turn's lookups back `sentence`'s figure.
    fn backs(&self, sentence: &str) -> bool {
        self.searched || self.task_answered || (self.weathered && speaks_of_weather(sentence))
    }

    /// A sentence as the hold lets it go: none once a figure is held this
    /// round.
    pub(crate) fn pass(&mut self, sentence: String) -> Option<String> {
        if !self.on {
            return Some(sentence);
        }
        let sentence = self.unecho(sentence)?;
        let judged = if self.lead.is_empty() {
            sentence.clone()
        } else {
            format!("{} {}", self.lead, sentence.trim_start())
        };
        if self.held.is_empty()
            && states_figure(&judged)
            && !self.backs(&judged)
            && self.lets_through(&judged)
        {
            // Said as it is, below.
        } else if !self.held.is_empty() || (states_figure(&judged) && !self.backs(&judged)) {
            self.lead.clear();
            self.held.push(judged);
            return None;
        }
        // A piece cut off before its sentence's end leads the rest.
        self.lead = if ends_sentence(&sentence) {
            String::new()
        } else {
            judged
        };
        if let Some(at) = self.asked_at.take()
            && sentence.chars().any(char::is_alphanumeric)
        {
            eprintln!(
                "lamp-voice second: request {}: the live figure's ask again said its first words {} ms after it was asked (LAMP_LIVE_FACT_CHECKED)",
                self.request,
                at.elapsed().as_millis()
            );
        }
        // The bridge goes before it; spent only once it is said
        // ([`Self::said`]).
        if self.owes_bridge() {
            return Some(format!("{BRIDGE} {}", sentence.trim_start()));
        }
        Some(sentence)
    }

    /// `sentence` was said, past every hold: kept as the round's, so a
    /// reply asked again is told it and never says it again. Words said
    /// after a dangling piece end its dangling; a bridge said is spent.
    pub(crate) fn said(&mut self, sentence: &str) {
        if !self.on {
            return;
        }
        let words = match sentence.trim().strip_prefix(BRIDGE) {
            Some(rest) => {
                self.spend_bridge();
                rest.trim()
            }
            None => sentence.trim(),
        };
        if words.chars().any(char::is_alphanumeric) {
            self.words_out = true;
            self.dangling = false;
            self.continuing = false;
            self.round_said.push(words.to_owned());
        }
    }

    /// Whether the next words said take [`BRIDGE`] first: a held round's
    /// said words dangle, the reply doesn't go on from them, and none was
    /// said this turn.
    fn owes_bridge(&self) -> bool {
        self.dangling && !self.continuing && !self.bridged
    }

    fn spend_bridge(&mut self) {
        self.bridged = true;
        self.dangling = false;
        eprintln!(
            "lamp-voice second: request {}: the reply asked again doesn't go on from the words said before its hold: said the bridge (LAMP_LIVE_FACT_CHECKED)",
            self.request
        );
    }

    /// `sentence` with the held round's said words taken off its start,
    /// once the reply asked again has repeated them all, compared as
    /// [`words`] folds them (case and marks), whole words only; a piece
    /// that is all a part of them waits for the next to say whether they
    /// repeat. Any other start is said as it came, with what waited.
    fn unecho(&mut self, sentence: String) -> Option<String> {
        if self.echo.is_empty() {
            return Some(sentence);
        }
        let said = words(&sentence);
        if self.echo_at == 0
            && let Some(first) = said.first()
            && let Some(start) = self
                .echo_starts
                .iter()
                .copied()
                .find(|start| self.echo.get(*start) == Some(first))
        {
            // The reply asked again repeats from this said piece on.
            self.echo_at = start;
        }
        let left = &self.echo[self.echo_at..];
        let matched = said
            .iter()
            .zip(left)
            .take_while(|(word, echo)| word == echo)
            .count();
        if matched == said.len() && matched < left.len() {
            self.echo_at += matched;
            self.echo_kept.push(sentence);
            return None;
        }
        if matched == left.len() {
            let repeated = self
                .echo_kept
                .iter()
                .map(|kept| words(kept).len())
                .sum::<usize>()
                + matched;
            let lead = std::mem::take(&mut self.echo_said);
            self.end_echo();
            // It goes on from the said words: no bridge.
            self.continuing = true;
            eprintln!(
                "lamp-voice second: request {}: the reply asked again began with the {repeated} word(s) already said: not said again (LAMP_LIVE_FACT_CHECKED)",
                self.request
            );
            let rest = after_words(&sentence, matched);
            if !rest.chars().any(char::is_alphanumeric) {
                return None;
            }
            // What was said leads the rest, when that is judged.
            self.lead = lead;
            return Some(rest.to_owned());
        }
        let mut came = std::mem::take(&mut self.echo_kept);
        came.push(sentence);
        self.end_echo();
        Some(came.join(" "))
    }

    /// `next`, a line pushed as said with no hold after it (the could-not
    /// line), with [`BRIDGE`] first when one is owed ([`Self::owes_bridge`]),
    /// spent at once.
    pub(crate) fn bridge(&mut self, next: String) -> String {
        if !self.owes_bridge() {
            return next;
        }
        self.spend_bridge();
        format!("{BRIDGE} {next}")
    }

    fn end_echo(&mut self) {
        self.echo.clear();
        self.echo_starts.clear();
        self.echo_said.clear();
        self.echo_at = 0;
        self.echo_kept.clear();
    }

    /// The note to ask again with ([`note_after`]): `tool`, and the held
    /// round's words already said.
    pub(crate) fn note(&self, tool: &str) -> String {
        note_after(tool, &self.echo_said)
    }

    /// A call answered: a lookup that found something backs the turn's
    /// figures; the weather's backs only the weather. A `start_agent_task`
    /// answered, whatever it said, ends the judging for the turn (#8556):
    /// lamp-06c1, 25 Nov, build 15903, "…a short poem about rain" was held
    /// as the weather and the model's second start prepared a second task.
    pub(crate) fn ran(&mut self, name: &str, told: &Value) {
        if name == crate::agent_tasks::START {
            if self.on && !self.task_answered {
                eprintln!(
                    "lamp-voice second: request {}: start_agent_task answered: the rounds after it are not judged (LAMP_LIVE_FACT_CHECKED, #8556)",
                    self.request
                );
            }
            self.task_answered = true;
            return;
        }
        if !is_lookup(name) || !found(told) {
            return;
        }
        if name == crate::second_provider::net::weather::NAME {
            self.weathered = true;
        } else {
            self.searched = true;
        }
    }

    /// The chat searched the web itself (OpenRouter's plugin).
    pub(crate) fn searched(&mut self) {
        self.searched = true;
    }

    /// The round's held sentences, taken: the lookup their words ask for.
    pub(crate) fn take(&mut self) -> Option<(Lookup, usize)> {
        if self.held.is_empty() {
            return None;
        }
        let held = std::mem::take(&mut self.held);
        // What the round said before its hold: the reply asked again must
        // not say it again. A round that said nothing keeps what an earlier
        // held round left: its said words still dangle until something is
        // said after them (Thirteen's HOLD on #8224: round 3 held whole
        // after a failed lookup lost the bridge before the could-not line).
        if !self.round_said.is_empty() {
            self.end_echo();
            self.echo_said = self.round_said.join(" ");
            self.dangling = !ends_sentence(&self.echo_said);
            // Where each said piece's words begin: the reply asked again
            // may repeat from any of them, so an opening it doesn't repeat
            // ("Sure!") drops nothing else.
            for piece in &self.round_said {
                self.echo_starts.push(self.echo.len());
                self.echo.extend(words(piece));
            }
        }
        let lookup = if held.iter().any(|sentence| speaks_of_weather(sentence)) {
            Lookup::Weather
        } else {
            Lookup::Search
        };
        Some((lookup, held.len()))
    }

    /// A new round: nothing held from the last.
    pub(crate) fn begin_round(&mut self) {
        self.held.clear();
        self.lead.clear();
        self.round_said.clear();
    }

    /// Asked again now.
    pub(crate) fn asking_again(&mut self) {
        self.asked_again = true;
        self.asked_at = Some(std::time::Instant::now());
    }

    /// The turn ends in [`COULD_NOT_LINE`]: the ask again's extra time, if
    /// it was asked again, is journalled (Twelve's P3 on #7719).
    pub(crate) fn could_not(&mut self) {
        if let Some(at) = self.asked_at.take() {
            eprintln!(
                "lamp-voice second: request {}: the live figure's ask again ended in the line that it couldn't look it up {} ms after it was asked (LAMP_LIVE_FACT_CHECKED)",
                self.request,
                at.elapsed().as_millis()
            );
        }
    }
}

/// The force the second worker gives the chat
/// (`second_provider::net::Chat::forcing`): with the switch `on`, a request
/// whose last message is [`note`] must call the lookup it names; off, none.
pub(crate) fn force_when(on: bool) -> Option<crate::second_provider::net::Force> {
    on.then(force)
}

/// [`force_when`]'s force.
pub(crate) fn force() -> crate::second_provider::net::Force {
    std::sync::Arc::new(|messages: &[crate::second_provider::net::Message]| {
        let last = messages.last().filter(|message| message.role == "system")?;
        let words = last.words()?;
        if !words.starts_with(NOTE_OPENS) {
            return None;
        }
        let name = [Lookup::Weather, Lookup::Search]
            .into_iter()
            .map(Lookup::name)
            .find(|name| words.contains(&format!("Call {name} now")))?;
        Some(crate::second_provider::net::Forced {
            name,
            only: None,
            fields: None,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::second_provider::net::Message;

    /// A figure is judged by the reply's own sentence: a price, the
    /// weather's degrees or a place's weather, a market's percent, a match's
    /// score. Tonight's lamp-9adf answers are all figures.
    #[test]
    fn a_figure_is_a_price_the_weather_a_market_or_a_score() {
        for figure in [
            "Gold is around $4,050 an ounce today.",
            "It's about 4,050 dollars an ounce.",
            "Gold is 4.5 million dong a tael.",
            "Gold is 4,050 an ounce.",
            "The rate is USD 1 to 25,400 dong.",
            "That's sixty-seven dollars.",
            "Bitcoin is up 3% today.",
            "The index fell 12 points.",
            "It's 31°C in Da Nang right now.",
            "Expect highs of 30 degrees tomorrow.",
            "It's about thirty degrees.",
            "It's thirty-one degrees.",
            "Da Nang is 27 degrees with light rain.",
            "Winds are 20 mph with showers.",
            "Arsenal won 2-1.",
            "It's sunny in Ho Chi Minh City.",
            "Da Nang will be cloudy with some showers.",
        ] {
            assert!(states_figure(figure), "{figure}");
        }
    }

    /// Talk, and figures that are no live fact, are none: a time, a date, a
    /// count, a recipe's weight or oven heat, a boiling point, "100% sure",
    /// a range, and words with no place (Twelve's false positives on #7719).
    #[test]
    fn talk_and_figures_of_no_live_fact_are_none() {
        for talk in [
            "It's 3:40 pm.",
            "Today is the 18th of November.",
            "Gold has long been seen as a safe store of value.",
            "Let me check that for you.",
            "I have 3 skills.",
            "It might rain, so take an umbrella.",
            "I can't look up live prices right now.",
            "Sunny days are lovely.",
            "I'm 100% sure you'll laugh.",
            "Use 200 grams of chicken.",
            "Bake it at 180°C for 20 minutes.",
            "Water boils at 100 degrees Celsius.",
            "Beat 2-3 eggs into the flour.",
            "I set a timer for ten minutes.",
            // Words that name no place.
            "On Monday it may be sunny.",
            "I'm sure May is rainy.",
        ] {
            assert!(!states_figure(talk), "{talk}");
        }
    }

    /// A lookup that failed, or a search that found nothing, backs nothing;
    /// one that found backs the turn; the weather backs only the weather;
    /// another call never backs.
    #[test]
    fn only_a_lookup_that_found_backs_the_turn() {
        let search = crate::second_provider::net::search::NAME;
        let weather = crate::second_provider::net::weather::NAME;
        let mut hold = Hold::new(true, 1, &[]);
        assert!(hold.watching());
        hold.ran("light_on", &serde_json::json!({"ok": true}));
        hold.ran(
            search,
            &serde_json::json!({"error": "the search did not answer in time"}),
        );
        // Twelve's P2 on #7719: an empty search finds nothing.
        hold.ran(search, &serde_json::json!({"results": []}));
        hold.ran(
            crate::second_provider::net::search::FETCH,
            &serde_json::json!({"title": "", "content": ""}),
        );
        assert!(hold.watching());
        assert_eq!(hold.pass("Sure.".into()), Some("Sure.".into()));
        assert_eq!(hold.pass("It's $4,050 an ounce.".into()), None);
        assert_eq!(hold.pass("Prices change.".into()), None);
        assert_eq!(hold.take(), Some((Lookup::Search, 2)));
        // The weather found backs the weather, never a price.
        hold.ran(
            weather,
            &serde_json::json!({"place": "Da Nang", "weather": "27 degrees, light rain"}),
        );
        assert!(hold.watching());
        let rain = "Da Nang is 27 degrees with light rain.";
        assert_eq!(hold.pass(rain.into()), Some(rain.into()));
        assert_eq!(hold.pass("Gold is $4,050 an ounce.".into()), None);
        assert_eq!(hold.take(), Some((Lookup::Search, 1)));
        hold.ran(
            search,
            &serde_json::json!({"results": [{"title": "Gold", "text": "$4,061"}]}),
        );
        assert!(!hold.watching());
        assert_eq!(
            hold.pass("It's $4,061 an ounce.".into()),
            Some("It's $4,061 an ounce.".into())
        );
        // Off, or on a turn not judged: nothing is held.
        let mut off = Hold::new(false, 1, &[]);
        assert!(!off.watching());
        assert_eq!(
            off.pass("It's $4,050 an ounce.".into()),
            Some("It's $4,050 an ounce.".into())
        );
        assert_eq!(off.take(), None);
    }

    /// #8556 (lamp-06c1, 25 Nov, build 15903): the read-back of a task
    /// `start_agent_task` answered restates the person's own ask ("…a
    /// short poem about rain"), no claim about the world, so no round after
    /// it in the turn is judged; before it, the same words are.
    #[test]
    fn a_task_read_back_after_its_start_answered_is_never_held() {
        let read_back = "Claude Code will write a short poem about rain.";
        assert!(states_figure(read_back));
        let mut hold = Hold::new(true, 1, &[]).asked("Ask Claude Code to write a poem about rain.");
        assert!(hold.holds_back());
        assert_eq!(hold.pass(read_back.into()), None);
        assert_eq!(hold.take(), Some((Lookup::Weather, 1)));
        hold.begin_round();
        hold.ran(
            crate::agent_tasks::START,
            &serde_json::json!({"prepared": true, "task": "Write a short poem about rain"}),
        );
        assert!(!hold.watching());
        assert!(!hold.holds_back());
        assert_eq!(hold.pass(read_back.into()), Some(read_back.into()));
        assert_eq!(hold.take(), None);
    }

    /// #8615's (d'), V2's ruling 26 Nov 18:35: a weather condition in the
    /// person's line asks a live fact only in a question, by its '?' or by
    /// a question's words (speech to text drops the '?' too often); a
    /// story's setting asks none (kit-8646's s1).
    #[test]
    fn a_condition_asks_a_live_fact_only_in_a_question() {
        for line in [
            "will it be sunny in Hanoi",
            "is it raining in Hue",
            "Hey Lamp, is it raining in Hue",
            "Lamp, tell me, will it be sunny in Hanoi",
            "Will it be sunny in Hanoi?",
        ] {
            assert!(asks_live_fact(line), "{line}");
        }
        for line in [
            "the beach in Da Nang when it's sunny",
            "Hey Lamp, tell me a short story about a cat who goes to the beach in Da Nang when it's sunny.",
            "a cat who loved rainy days",
            // Known limit: a question put another way, with no '?'.
            "I wonder if it's sunny in Hanoi",
        ] {
            assert!(!asks_live_fact(line), "{line}");
        }
    }

    /// The first words wait only on a line that asks for a live fact
    /// (the first builder on #8224, rule 7), never for a number in the
    /// reply: a timer or a count asked plainly never waits.
    #[test]
    fn the_first_words_wait_only_on_a_line_asking_a_live_fact() {
        for line in [
            "Lamp, what is the gold price today?",
            "What's the weather like?",
            "Is it going to rain?",
            "What about Da Nang tomorrow?",
            "How much is Bitcoin?",
            "What's the latest score?",
            "What's the exchange rate right now?",
            "Will it be sunny in Hanoi?",
        ] {
            assert!(asks_live(line), "{line}");
        }
        for line in [
            "Lamp, set a timer for 5 minutes.",
            // Speech to text writes the call as its own sentence (Twelve's
            // HOLD on #8224): no capital counts as a place.
            "Hey, Lamp. Set a timer for 5 minutes.",
            "Lamp. What's 12 times 3?",
            "I\u{2019}m tired.",
            "Ask Mom what she wants.",
            "Play Spotify.",
            "How many sessions did I do this week?",
            "Tell me a joke.",
            "Turn the light up.",
            "I'm tired on Monday.",
            "Where did my lamp come from?",
            // Missed, and fails safe: the piece never ends on "31".
            "How much is an ounce of silver?",
        ] {
            assert!(!asks_live(line), "{line}");
        }
        for number in ["31", "$4,050", "4.5", "thirty-one", "Twenty", "3%"] {
            assert!(is_number(number), "{number}");
        }
        for word in ["about", "a", "one's", "Spotify", "dollars"] {
            assert!(!is_number(word), "{word}");
        }
        let gold = "What's the gold price today?";
        assert!(Hold::new(true, 1, &[]).asked(gold).holds_back());
        assert!(
            !Hold::new(true, 1, &[])
                .asked("Set a timer for 5 minutes.")
                .holds_back()
        );
        let mut found = Hold::new(true, 1, &[]).asked(gold);
        found.ran(
            crate::second_provider::net::search::NAME,
            &serde_json::json!({"results": [{"title": "Gold", "text": "$4,061"}]}),
        );
        assert!(!found.holds_back());
        assert!(!Hold::new(false, 1, &[]).asked(gold).holds_back());
    }

    /// A piece cut off before its sentence's end leads the rest when that
    /// is judged: "The temperature in Hanoi" said keeps "is 30 degrees." a
    /// weather figure, and a whole sentence leads nothing.
    #[test]
    fn a_cut_piece_leads_the_rest_of_its_sentence() {
        let mut hold = Hold::new(true, 1, &[]);
        assert_eq!(
            hold.pass("The temperature in Hanoi".into()),
            Some("The temperature in Hanoi".into())
        );
        assert_eq!(hold.pass("is 30 degrees.".into()), None);
        assert_eq!(hold.take(), Some((Lookup::Weather, 1)));
        hold.begin_round();
        assert_eq!(hold.pass("Sure.".into()), Some("Sure.".into()));
        assert_eq!(hold.pass("It is 30.".into()), Some("It is 30.".into()));
    }

    /// The held round's said words are taken off the reply asked again
    /// once it repeats them all, across its pieces, case and marks folded;
    /// a start that differs, or only begins a word, keeps every word.
    #[test]
    fn the_reply_asked_again_drops_only_the_words_said_whole() {
        let held = |said: &[&str]| {
            let mut hold = Hold::new(true, 1, &[]);
            for said in said {
                assert_eq!(hold.pass((*said).into()), Some((*said).into()));
                hold.said(said);
            }
            assert_eq!(hold.pass("it's 20°C.".into()), None);
            assert!(hold.take().is_some());
            hold.begin_round();
            hold.ran(
                crate::second_provider::net::weather::NAME,
                &serde_json::json!({"place": "Hanoi", "weather": "20 degrees"}),
            );
            hold
        };
        let mut hold = held(&["In Hanoi right now"]);
        assert_eq!(
            hold.note("weather"),
            note_after("weather", "In Hanoi right now")
        );
        assert_eq!(hold.pass("in hanoi,".into()), None);
        assert_eq!(
            hold.pass("Right now, it's 20°C.".into()),
            Some("it's 20°C.".into())
        );
        // Once dropped, later words are said as they come.
        assert_eq!(hold.pass("In Hanoi.".into()), Some("In Hanoi.".into()));
        // An opening the reply asked again doesn't repeat ("Sure!") keeps
        // nothing else from dropping (Twelve's P3 on #8224).
        let mut hold = held(&["Sure!", "In Hanoi right now"]);
        assert_eq!(
            hold.pass("In Hanoi right now, it's 20°C.".into()),
            Some("it's 20°C.".into())
        );
        let mut hold = held(&["Sure!", "In Hanoi right now"]);
        assert_eq!(
            hold.pass("Sure! In Hanoi right now it's 20°C.".into()),
            Some("it's 20°C.".into())
        );
        // Words passed but never said (a later hold kept them) are not
        // counted as said.
        let mut unsaid = Hold::new(true, 1, &[]);
        assert!(unsaid.pass("In Hanoi right now".into()).is_some());
        assert_eq!(unsaid.pass("it's 20°C.".into()), None);
        assert!(unsaid.take().is_some());
        assert_eq!(unsaid.note("weather"), note("weather"));
        let mut hold = held(&["In Hanoi right now"]);
        assert_eq!(hold.pass("In Hanoi".into()), None);
        assert_eq!(
            hold.pass("right nowadays it's 20°C.".into()),
            Some(format!("{BRIDGE} In Hanoi right nowadays it's 20°C."))
        );
        assert_eq!(note_after("weather", ""), note("weather"));
        assert_eq!(
            after_words("In Hanoi right now, it's 20°C.", 4),
            "it's 20°C."
        );
        assert_eq!(after_words("In Hanoi right now", 4), "");
    }

    /// The bridge ends a said piece the reply asked again doesn't go on
    /// from, once; a reply that goes on from it, a said full sentence, or
    /// nothing said, has none (the first builder's c7066116904 on #8224).
    #[test]
    fn the_bridge_ends_only_a_dangling_piece_not_gone_on_from() {
        let held = |said: &str, figure: &str| {
            let mut hold = Hold::new(true, 1, &[]);
            if !said.is_empty() {
                assert!(hold.pass(said.into()).is_some());
                hold.said(said);
            }
            assert_eq!(hold.pass(figure.into()), None);
            assert!(hold.take().is_some());
            hold.begin_round();
            hold.ran(
                crate::second_provider::net::search::NAME,
                &serde_json::json!({"results": [{"title": "Silver", "text": "$45"}]}),
            );
            hold
        };
        let mut hold = held("Silver's around", "52 to 55 dollars an ounce.");
        let bridged = format!("{BRIDGE} Silver was around $45 an ounce.");
        assert_eq!(
            hold.pass("Silver was around $45 an ounce.".into()),
            Some(bridged.clone())
        );
        // Spent only once said.
        hold.said(&bridged);
        assert_eq!(hold.pass("It moves.".into()), Some("It moves.".into()));
        // Once a turn.
        assert_eq!(hold.bridge("Again.".into()), "Again.");
        let mut hold = held("In Hanoi right now", "it's 20°C.");
        assert_eq!(
            hold.pass("In Hanoi right now it's 20°C.".into()),
            Some("it's 20°C.".into())
        );
        let full = "An ounce of soap is a weight, not a price.";
        let mut hold = held(full, "Silver is $45 an ounce.");
        assert_eq!(
            hold.pass("Silver is $46 an ounce.".into()),
            Some("Silver is $46 an ounce.".into())
        );
        let mut hold = held("", "Silver is $45 an ounce.");
        assert_eq!(
            hold.pass("Silver is $46 an ounce.".into()),
            Some("Silver is $46 an ounce.".into())
        );
        assert_eq!(hold.bridge(COULD_NOT_LINE.into()), COULD_NOT_LINE);
    }

    /// Results whose every item is blank, and a page with a failed status
    /// or an error page's title, found nothing (Twelve's P3s on #7719).
    #[test]
    fn blank_results_and_error_pages_find_nothing() {
        use serde_json::json;
        for nothing in [
            json!({"results": [{"title": "", "text": " ", "published": null}]}),
            json!({"results": [{"title": "  "}, {"text": ""}]}),
            json!({"title": "Gold", "content": "Gone.", "status": 404}),
            json!({"title": "Gold", "content": "Gone.", "status_code": "503"}),
            json!({"title": "404 Not Found", "content": "nginx"}),
            json!({"title": "Page not found | Kitco", "content": "Sorry."}),
            json!({"title": "Access Denied", "content": "You don't have permission."}),
            json!({"title": "Error 500", "content": "Try later."}),
        ] {
            assert!(!found(&nothing), "{nothing}");
        }
        for something in [
            json!({"results": [{"title": "", "text": "$4,061 an ounce"}]}),
            json!({"results": [{"title": "Gold price today", "text": ""}]}),
            json!({"title": "Gold price today", "content": "$4,061 an ounce."}),
            json!({"title": "Top 500 companies by revenue in 2026", "content": "Walmart."}),
        ] {
            assert!(found(&something), "{something}");
        }
    }

    /// A cut answer's carried lookup backs the turn only when its answer
    /// found something (Twelve's P3 on #7719: not by its name alone).
    #[test]
    fn a_carried_lookup_backs_only_when_it_found() {
        let carried = |told: serde_json::Value| {
            vec![
                Message::assistant_calls(
                    None,
                    &[crate::second_provider::net::ToolCall {
                        id: "c1".into(),
                        name: crate::second_provider::net::search::NAME.into(),
                        args: serde_json::json!({}),
                        extra: None,
                    }],
                ),
                Message::tool("c1", told.to_string()),
            ]
        };
        let found = carried(serde_json::json!({"results": [{"title": "Gold", "text": "$4,061"}]}));
        assert!(!Hold::new(true, 1, &found).watching());
        for empty in [
            serde_json::json!({"results": []}),
            serde_json::json!({"error": "the search did not answer in time"}),
        ] {
            assert!(Hold::new(true, 1, &carried(empty)).watching());
        }
    }

    /// The ask again's note forces the lookup it names, only while it is
    /// the last message; nothing else does, and with the switch off no
    /// force is given at all (the worker's wiring).
    #[test]
    fn the_note_forces_the_lookup_it_names_only_with_the_switch() {
        assert!(force_when(false).is_none());
        let force = force_when(true).expect("on");
        for lookup in [Lookup::Weather, Lookup::Search] {
            let messages = [
                Message::user("What's the weather in Da Nang now?"),
                Message::system(note(lookup.name())),
            ];
            assert_eq!(
                force(&messages).map(|forced| forced.name),
                Some(lookup.name())
            );
        }
        assert_eq!(
            force(&[Message::user("What's the gold price today?")]),
            None
        );
        assert_eq!(force(&[Message::system(crate::action_claim::NOTE)]), None);
        // A note told before another is not forced.
        assert_eq!(
            force(&[
                Message::system(note(Lookup::Search.name())),
                Message::system(crate::make_skill_agent::BEGUN_AGAIN_NOTE),
            ]),
            None
        );
    }
}
