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
//!   (`{"results": []}`) backs nothing. So does the chat's own web search
//!   (OpenRouter's plugin), which backs any. An early search counts only
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
//! first-words cut), so a figure is judged whole. Off unless the
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

/// Whether a lookup's answer found something: no error, and results, a
/// page's words, the weather or a price in it. An empty search
/// (`{"results": []}`) backs nothing (Twelve's P2 on #7719).
pub fn found(told: &Value) -> bool {
    if told.get("error").is_some() {
        return false;
    }
    ["results", "content", "weather", "price"]
        .iter()
        .any(|key| match told.get(*key) {
            None | Some(Value::Null) => false,
            Some(Value::Array(items)) => !items.is_empty(),
            Some(Value::String(text)) => !text.trim().is_empty(),
            Some(Value::Object(fields)) => !fields.is_empty(),
            Some(_) => true,
        })
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
    /// The sentences held this round, from the first with a figure.
    held: Vec<String>,
    /// Whether the turn was asked again once already.
    pub(crate) asked_again: bool,
    /// When it was asked again: the extra round's time is journalled once
    /// its first words pass, or when it ends in [`COULD_NOT_LINE`].
    asked_at: Option<std::time::Instant>,
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
    /// on, and no search found anything yet. While it is, a reply's first
    /// words wait for their sentence's end.
    pub(crate) fn watching(&self) -> bool {
        self.on && !self.searched
    }

    /// Whether this turn's lookups back `sentence`'s figure.
    fn backs(&self, sentence: &str) -> bool {
        self.searched || (self.weathered && speaks_of_weather(sentence))
    }

    /// A sentence as the hold lets it go: none once a figure is held this
    /// round.
    pub(crate) fn pass(&mut self, sentence: String) -> Option<String> {
        if !self.held.is_empty() || (self.on && states_figure(&sentence) && !self.backs(&sentence))
        {
            self.held.push(sentence);
            return None;
        }
        if let Some(at) = self.asked_at.take()
            && sentence.chars().any(char::is_alphanumeric)
        {
            eprintln!(
                "lamp-voice second: request {}: the live figure's ask again said its first words {} ms after it was asked (LAMP_LIVE_FACT_CHECKED)",
                self.request,
                at.elapsed().as_millis()
            );
        }
        Some(sentence)
    }

    /// A call answered: a lookup that found something backs the turn's
    /// figures; the weather's backs only the weather.
    pub(crate) fn ran(&mut self, name: &str, told: &Value) {
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
