//! What each arm of a played test cost, from the lamp's journal alone (the
//! first builder on #1722, 19 Oct 21:18: `cost-night1.py` as a subcommand
//! beside `score`). Each arm is a stretch of the journal by its time of day,
//! given as `score`'s are. Usage is measured from the voice's own lines;
//! prices come from a file (`--prices`), never from the code. Only counts
//! are read and printed: no words a person said.
//!
//! What is counted, by the line that says it:
//! - **a request:** the second route's `request N heard W words, …`, and
//!   any Live `provider: request N` line; known by its process
//!   (`lamp-voice[PID]`) and number, as in `score`;
//! - **answered:** the second route's `first answer audio` for a request,
//!   and Live's first sound for one not held back, as `score` counts them;
//! - **DeepSeek:** each `chat answer: …; P prompt tokens, H from the cache,
//!   M not; C completion tokens`;
//! - **searches:** each `web_search` in a chat round's `called …` list, and
//!   each `searched the web` (a search the chat's own plugin ran);
//! - **speech to text:** the sound sent, from Nova-3's `finals cover … of N
//!   ms sent` and Flux's `flux: … heard to … of N ms sent`, each at its own
//!   price;
//! - **the voice:** the characters in each request's summary of its pieces
//!   (`request N ElevenLabs: … C chars;`, #1935), by the voice named there.
//!   A piece whose first audio never came is not in it, so this can read a
//!   little low. An answered line with no such summary (a journal from
//!   before #1935) refuses the run, unless an estimate is asked for
//!   ([`VoiceEstimate`]), so the voice is never priced at nothing;
//! - **Live:** each `provider: usage …`: text in (`prompt` less
//!   `prompt_audio`), audio in, text out (`response` less `response_audio`)
//!   and audio out.
//!
//! The second route's arms get a row for each of DeepSeek's price tiers in
//! the file (peak, off-peak); Live's one row. A "conversation minute" is 5
//! requests at the arm's own cost a request (the research's model: 4
//! answered and 1 not, as `cost-night1.py` has it).

use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

/// One arm: its name and its stretch of the journal, by time of day in
/// seconds, `from` included and `to` not (`to` before `from` runs past
/// midnight). As `score`'s.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arm {
    pub name: String,
    pub from: u32,
    pub to: u32,
}

impl Arm {
    /// An arm from its name and two times of day, `HH:MM` or `HH:MM:SS`.
    pub fn parse(name: &str, from: &str, to: &str) -> Result<Self, String> {
        Ok(Self {
            name: name.to_owned(),
            from: clock(from).ok_or_else(|| format!("{from:?} is no HH:MM or HH:MM:SS"))?,
            to: clock(to).ok_or_else(|| format!("{to:?} is no HH:MM or HH:MM:SS"))?,
        })
    }

    fn holds(&self, at: u32) -> bool {
        if self.from <= self.to {
            (self.from..self.to).contains(&at)
        } else {
            at >= self.from || at < self.to
        }
    }

    /// The arm's length in minutes.
    fn minutes(&self) -> f64 {
        let seconds = if self.from <= self.to {
            self.to - self.from
        } else {
            86_400 - self.from + self.to
        };
        f64::from(seconds) / 60.0
    }
}

/// The prices, in dollars: the list prices until the team's own rates come.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prices {
    /// Where the prices come from, for the reader; not read.
    #[serde(default)]
    pub note: Option<String>,
    /// DeepSeek's chat by tier (`peak`, `off-peak`, …): a row each.
    pub deepseek: BTreeMap<String, Chat>,
    /// A web search, a call.
    pub search_per_call: f64,
    /// Speech to text, a minute of sound sent.
    pub speech_to_text_per_minute: SpeechToText,
    /// The voice, a character, by the name the journal gives it
    /// (`ElevenLabs`, `Gemini`, `OpenAI`).
    pub voice_per_character: BTreeMap<String, f64>,
    /// Gemini Live, a million tokens of each kind.
    pub live_per_million: Live,
}

/// A million tokens of each kind.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chat {
    pub uncached_in: f64,
    pub cached_in: f64,
    pub out: f64,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeechToText {
    #[serde(rename = "nova-3")]
    pub nova_3: f64,
    pub flux: f64,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Live {
    pub text_in: f64,
    pub audio_in: f64,
    pub text_out: f64,
    pub audio_out: f64,
}

/// For answered lines with no summary of their pieces (a journal from
/// before #1935): their voice's characters estimated as `chars_per_token`
/// a completion token, over the arm's share of such lines, as
/// `cost-night1.py` estimates them, and said as estimated.
#[derive(Clone, Debug, PartialEq)]
pub struct VoiceEstimate {
    pub voice: String,
    pub chars_per_token: f64,
}

/// A request: its route (second or Live), its process and its number.
type Request = (bool, u64, u64);

/// What one arm used.
#[derive(Debug, Default)]
struct Usage {
    requests: BTreeSet<Request>,
    answered: BTreeSet<Request>,
    sounded: BTreeSet<Request>,
    held: BTreeSet<Request>,
    prompt: u64,
    cached: u64,
    uncached: u64,
    completion: u64,
    /// Chat answers whose usage the service did not give: not priced.
    no_usage: u64,
    searches: u64,
    nova_ms: u64,
    flux_ms: u64,
    characters: BTreeMap<String, u64>,
    /// The second route's requests with a summary of their pieces.
    voiced: BTreeSet<Request>,
    /// Text in, audio in, text out, audio out.
    live: [u64; 4],
    live_generations: u64,
}

impl Usage {
    fn read(&mut self, line: &str) {
        let process = number_after(line, "lamp-voice[").unwrap_or(0);
        let second = line.contains("lamp-voice second:");
        let live = line.contains("lamp-voice provider:");
        let request = number_after(line, "request ");
        if second
            && line.contains(" words, the last ")
            && let Some(number) = request
        {
            self.requests.insert((true, process, number));
        }
        if live && let Some(number) = request {
            self.requests.insert((false, process, number));
            if line.contains(": first sound") {
                self.sounded.insert((false, process, number));
            }
            if line.contains("its answer is held back: ") {
                self.held.insert((false, process, number));
            }
        }
        if second
            && line.contains("first answer audio ")
            && let Some(number) = request
        {
            self.answered.insert((true, process, number));
        }
        if line.contains("chat answer: ")
            && let Some(tokens) = line.split("after it was asked; ").nth(1)
        {
            self.chat(tokens);
        }
        if second
            && line.contains(" chat round ")
            && let Some(names) = line.split(" called ").nth(1)
        {
            self.searches += names
                .split(", ")
                .filter(|name| name.trim_end() == "web_search")
                .count() as u64;
        }
        if second && line.contains(" searched the web in ") {
            self.searches += 1;
        }
        if let Some(sent) = sent_ms(line) {
            if line.contains("flux: ") {
                self.flux_ms += sent;
            } else if line.contains("finals cover ") {
                self.nova_ms += sent;
            }
        }
        if let Some((voice, characters)) = voice_characters(line) {
            *self.characters.entry(voice).or_default() += characters;
            if let Some(number) = request {
                self.voiced.insert((true, process, number));
            }
        }
        if live && line.contains("provider: usage total=") {
            let some = |name: &str| number_after(line, &format!(" {name}=Some(")).unwrap_or(0);
            let (prompt, prompt_audio) = (some("prompt"), some("prompt_audio"));
            let (response, response_audio) = (some("response"), some("response_audio"));
            self.live[0] += prompt.saturating_sub(prompt_audio);
            self.live[1] += prompt_audio;
            self.live[2] += response.saturating_sub(response_audio);
            self.live[3] += response_audio;
            self.live_generations += 1;
        }
    }

    /// A chat answer's tokens (`net/chat.rs`, `answer_told`): `P prompt
    /// tokens, H from the cache, M not; C completion tokens`, or with `the
    /// cache not said` all P taken as not from the cache (the dearer), or
    /// `no usage given`, counted as such.
    fn chat(&mut self, tokens: &str) {
        if tokens.starts_with("no usage given") {
            self.no_usage += 1;
            return;
        }
        let prompt = number_after(tokens, "").unwrap_or(0);
        let completion = tokens
            .split("; ")
            .nth(1)
            .and_then(|completion| number_after(completion, ""))
            .unwrap_or(0);
        let (cached, missed) = match tokens
            .split(" prompt tokens, ")
            .nth(1)
            .filter(|cache| cache.contains(" from the cache, "))
        {
            Some(cache) => {
                let cached = number_after(cache, "").unwrap_or(0);
                let missed = number_after(cache, "from the cache, ")
                    .unwrap_or(prompt.saturating_sub(cached));
                (cached, missed)
            }
            None => (0, prompt),
        };
        self.prompt += prompt;
        self.cached += cached;
        self.uncached += missed;
        self.completion += completion;
    }

    /// Requests answered: the second route's with a first answer audio,
    /// and Live's that made a first sound and were not held back.
    fn answered(&self) -> usize {
        self.answered.len() + self.sounded.difference(&self.held).count()
    }

    fn second_route(&self) -> bool {
        self.prompt > 0 || self.nova_ms > 0 || self.flux_ms > 0 || !self.characters.is_empty()
    }
}

/// The number of milliseconds sent in a speech to text line, `of N ms sent`.
fn sent_ms(line: &str) -> Option<u64> {
    let before = line.find(" ms sent")?;
    let of = line[..before].rfind(" of ")?;
    line[of + 4..before].parse().ok()
}

/// A request's summary of its voice's pieces, `request N VOICE: X of Y
/// pieces began, … C chars;`: the voice and its characters.
fn voice_characters(line: &str) -> Option<(String, u64)> {
    let summary = line.split("lamp-voice second: request ").nth(1)?;
    if !summary.contains(" pieces began, ") {
        return None;
    }
    let (head, rest) = summary.split_once(": ")?;
    let voice = head.split_once(' ')?.1.to_owned();
    let characters = rest
        .split(" chars;")
        .next()?
        .rsplit(", ")
        .next()?
        .parse()
        .ok()?;
    Some((voice, characters))
}

/// `HH:MM` or `HH:MM:SS` as seconds of the day.
fn clock(text: &str) -> Option<u32> {
    let parts: Vec<&str> = text.split(':').collect();
    let number = |part: &str, most: u32| {
        (part.len() == 2 && part.bytes().all(|b| b.is_ascii_digit()))
            .then(|| part.parse::<u32>().ok())
            .flatten()
            .filter(|value| *value <= most)
    };
    match parts.as_slice() {
        [h, m] => Some(number(h, 23)? * 3600 + number(m, 59)? * 60),
        [h, m, s] => Some(number(h, 23)? * 3600 + number(m, 59)? * 60 + number(s, 59)?),
        _ => None,
    }
}

/// The first `HH:MM:SS` in a journal line, as seconds of the day.
fn time_of_day(line: &str) -> Option<u32> {
    let bytes = line.as_bytes();
    (0..bytes.len().saturating_sub(7)).find_map(|at| {
        let window = &bytes[at..at + 8];
        let digits = [0, 1, 3, 4, 6, 7]
            .iter()
            .all(|i| window[*i].is_ascii_digit());
        (digits && window[2] == b':' && window[5] == b':')
            .then(|| clock(std::str::from_utf8(window).ok()?))
            .flatten()
    })
}

/// The number right after `before` in `text` (`""`: at its start).
fn number_after(text: &str, before: &str) -> Option<u64> {
    let rest = &text[text.find(before)? + before.len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// The table: a row for each arm and price tier, with what it is made of.
/// Every voice the journal names must have a price, and every answered
/// line its voice's characters or `estimate`; one without is said, not
/// priced at nothing.
pub fn cost(
    journal: &str,
    arms: &[Arm],
    prices: &Prices,
    estimate: Option<&VoiceEstimate>,
) -> Result<String, String> {
    let mut usage: Vec<Usage> = arms.iter().map(|_| Usage::default()).collect();
    for line in journal.lines() {
        let Some(at) = time_of_day(line) else {
            continue;
        };
        if let Some(index) = arms.iter().position(|arm| arm.holds(at)) {
            usage[index].read(line);
        }
    }
    let mut table = String::from(
        "arm | price | requests | answered | minutes | $ / request | $ / answered line | \
         $ / conversation minute (5 requests) | what it is made of\n",
    );
    for (arm, used) in arms.iter().zip(&usage) {
        let mut rows: Vec<(String, f64)> = Vec::new();
        let mut parts = Vec::new();
        let live = if used.live_generations > 0 {
            let [text_in, audio_in, text_out, audio_out] = used.live.map(|n| n as f64 / 1e6);
            let rates = prices.live_per_million;
            parts.push(format!(
                "Live: {} generations, {} text in, {} audio in, {} text out, {} audio out tokens",
                used.live_generations, used.live[0], used.live[1], used.live[2], used.live[3]
            ));
            text_in * rates.text_in
                + audio_in * rates.audio_in
                + text_out * rates.text_out
                + audio_out * rates.audio_out
        } else {
            0.0
        };
        if used.second_route() {
            let stt = prices.speech_to_text_per_minute;
            let speech_to_text = used.nova_ms as f64 / 60_000.0 * stt.nova_3
                + used.flux_ms as f64 / 60_000.0 * stt.flux;
            let mut voice = 0.0;
            for (name, characters) in &used.characters {
                let rate = prices
                    .voice_per_character
                    .get(name)
                    .ok_or_else(|| format!("no price for {name}'s characters in the prices"))?;
                voice += *characters as f64 * rate;
                parts.push(format!(
                    "{name} {characters} chars ${:.4}",
                    *characters as f64 * rate
                ));
            }
            let unvoiced = used.answered.difference(&used.voiced).count();
            if unvoiced > 0 {
                let answered = used.answered.len();
                let estimate = estimate.ok_or_else(|| {
                    format!(
                        "{}: {unvoiced} of {answered} answered lines have no summary of their \
                         voice's pieces (a journal from before #1935), so their voice can't be \
                         priced: give --voice-estimate VOICE CHARS_PER_TOKEN to estimate it",
                        arm.name
                    )
                })?;
                let rate = prices
                    .voice_per_character
                    .get(&estimate.voice)
                    .ok_or_else(|| {
                        format!("no price for {}'s characters in the prices", estimate.voice)
                    })?;
                let characters =
                    used.completion as f64 * estimate.chars_per_token * unvoiced as f64
                        / answered as f64;
                voice += characters * rate;
                parts.push(format!(
                    "{} about {characters:.0} chars estimated for {unvoiced} of {answered} answered \
                     lines with no voice line, at {} a completion token ${:.4}",
                    estimate.voice,
                    estimate.chars_per_token,
                    characters * rate
                ));
            }
            let search = used.searches as f64 * prices.search_per_call;
            let cache = (100 * used.cached).checked_div(used.prompt).unwrap_or(0);
            parts.push(format!(
                "Deepgram {:.0} s sent to Nova-3, {:.0} s to Flux ${speech_to_text:.4}; DeepSeek {} prompt tokens, \
                 {cache}% from the cache, {} out; {} searches ${search:.4}",
                used.nova_ms as f64 / 1000.0,
                used.flux_ms as f64 / 1000.0,
                used.prompt,
                used.completion,
                used.searches
            ));
            if used.no_usage > 0 {
                parts.push(format!(
                    "{} chat answers with no usage given, not priced",
                    used.no_usage
                ));
            }
            for (tier, chat) in &prices.deepseek {
                let llm = (used.uncached as f64 * chat.uncached_in
                    + used.cached as f64 * chat.cached_in
                    + used.completion as f64 * chat.out)
                    / 1e6;
                rows.push((tier.clone(), speech_to_text + llm + voice + search + live));
            }
        } else {
            rows.push(("list".into(), live));
        }
        let requests = used.requests.len();
        let answered = used.answered();
        for (tier, total) in rows {
            let per_request = total / requests.max(1) as f64;
            table.push_str(&format!(
                "{} | {tier} | {requests} | {answered} | {:.0} | ${per_request:.5} | ${:.5} | ${:.4} | {}\n",
                arm.name,
                arm.minutes(),
                total / answered.max(1) as f64,
                5.0 * per_request,
                parts.join("; ")
            ));
        }
    }
    Ok(table)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prices() -> Prices {
        serde_json::from_str(
            r#"{
                "deepseek": {
                    "peak": {"uncached_in": 0.30, "cached_in": 0.006, "out": 1.20},
                    "off-peak": {"uncached_in": 0.15, "cached_in": 0.003, "out": 0.60}
                },
                "search_per_call": 0.00075,
                "speech_to_text_per_minute": {"nova-3": 0.009, "flux": 0.0065},
                "voice_per_character": {"ElevenLabs": 0.00004},
                "live_per_million": {"text_in": 0.75, "audio_in": 3.0, "text_out": 4.5, "audio_out": 12.0}
            }"#,
        )
        .unwrap()
    }

    /// The journal's own lines, in `journalctl -u lamp-voice -o short-iso`'s
    /// shape, two requests on the second route with Nova-3 and one on Flux,
    /// and two of Live's, one held back.
    const JOURNAL: &str = "\
2026-10-19T20:00:05+0700 lamp-4ace lamp-voice[100]: lamp-voice second: request 1 heard 6 words, the last 210 ms after the line ended
2026-10-19T20:00:05+0700 lamp-4ace lamp-voice[100]: lamp-voice second: settled=true, speech_final=true, finals cover 1800 of 2400 ms sent, pending=none
2026-10-19T20:00:06+0700 lamp-4ace lamp-voice[100]: lamp-voice second: request 1 chat round 1 called web_search, the_time
2026-10-19T20:00:06+0700 lamp-4ace lamp-voice[100]: lamp-voice second: chat answer: its first words or call 812 ms and its end 1430 ms after it was asked; 8000 prompt tokens, 6000 from the cache, 2000 not; 100 completion tokens
2026-10-19T20:00:07+0700 lamp-4ace lamp-voice[100]: lamp-voice second: request 1 first answer audio 900 ms after its words
2026-10-19T20:00:08+0700 lamp-4ace lamp-voice[100]: lamp-voice second: request 1 ElevenLabs: 2 of 2 pieces began, 1 on a reused connection, 400 chars; the first piece's first audio byte 300 ms after it went, the slowest 350 ms
2026-10-19T20:01:00+0700 lamp-4ace lamp-voice[100]: lamp-voice second: request 2 heard 0 words, the last 0 ms after the line ended
2026-10-19T20:01:00+0700 lamp-4ace lamp-voice[100]: lamp-voice second: settled=false, speech_final=false, finals cover none of 1200 ms sent, pending=none
2026-10-19T20:30:00+0700 lamp-4ace lamp-voice[200]: lamp-voice second: request 1 heard 4 words, the last 150 ms after the line ended
2026-10-19T20:30:00+0700 lamp-4ace lamp-voice[200]: lamp-voice second: flux: turns=1, the latest turn ended by EndOfTurn, flux-general-en, settled=true, heard to 2900 of 3000 ms sent
2026-10-19T20:30:01+0700 lamp-4ace lamp-voice[200]: lamp-voice second: request 1 chat round 1 searched the web in 640 ms
2026-10-19T20:30:01+0700 lamp-4ace lamp-voice[200]: lamp-voice second: chat answer: its first words or call 700 ms and its end 1200 ms after it was asked; 4000 prompt tokens, 4000 from the cache, 0 not; 50 completion tokens
2026-10-19T20:30:02+0700 lamp-4ace lamp-voice[200]: lamp-voice second: request 1 first answer audio 800 ms after its words
2026-10-19T20:30:03+0700 lamp-4ace lamp-voice[200]: lamp-voice second: request 1 ElevenLabs: 1 of 1 pieces began, 0 on a reused connection, 100 chars; the first piece's first audio byte 280 ms after it went, the slowest 280 ms
2026-10-19T21:00:01+0700 lamp-4ace lamp-voice[300]: lamp-voice provider: request 1 began
2026-10-19T21:00:02+0700 lamp-4ace lamp-voice[300]: lamp-voice provider: request 1: first sound 640 ms after the line ended
2026-10-19T21:00:03+0700 lamp-4ace lamp-voice[300]: lamp-voice provider: usage total=Some(1500) prompt=Some(1000) prompt_audio=Some(400) response=Some(500) response_audio=Some(300) thoughts=None cached=None
2026-10-19T21:00:30+0700 lamp-4ace lamp-voice[300]: lamp-voice provider: request 2 began
2026-10-19T21:00:30+0700 lamp-4ace lamp-voice[300]: lamp-voice provider: request 2: first sound 500 ms after the line ended
2026-10-19T21:00:31+0700 lamp-4ace lamp-voice[300]: lamp-voice provider: request 2: its answer is held back: said to someone else
2026-10-19T21:00:31+0700 lamp-4ace lamp-voice[300]: lamp-voice provider: usage total=Some(200) prompt=Some(200) prompt_audio=None response=None response_audio=None thoughts=None cached=None
2026-10-19T22:00:00+0700 lamp-4ace lamp-voice[300]: lamp-voice provider: request 3 began, after every arm
";

    fn arms() -> Vec<Arm> {
        vec![
            Arm::parse("deepgram", "20:00", "20:15").unwrap(),
            Arm::parse("flux", "20:30", "20:45").unwrap(),
            Arm::parse("live", "21:00", "21:15").unwrap(),
        ]
    }

    fn row<'a>(table: &'a str, arm: &str, price: &str) -> Vec<&'a str> {
        table
            .lines()
            .find(|line| line.starts_with(&format!("{arm} | {price} |")))
            .unwrap_or_else(|| panic!("no {arm} {price} row in\n{table}"))
            .split(" | ")
            .collect()
    }

    #[test]
    fn each_arm_costs_what_its_journal_lines_used_at_the_files_prices() {
        let table = cost(JOURNAL, &arms(), &prices(), None).unwrap();
        // Nova-3 3.6 s, DeepSeek peak 2000 not cached, 6000 cached and 100
        // out, one search and 400 ElevenLabs characters, over two
        // requests, one answered.
        let nova = 3.6 / 60.0 * 0.009;
        let peak = (2000.0 * 0.30 + 6000.0 * 0.006 + 100.0 * 1.20) / 1e6;
        let total = nova + peak + 0.00075 + 400.0 * 0.00004;
        let deepgram = row(&table, "deepgram", "peak");
        assert_eq!(&deepgram[2..5], ["2", "1", "15"], "{table}");
        assert_eq!(deepgram[5], format!("${:.5}", total / 2.0));
        assert_eq!(deepgram[6], format!("${total:.5}"));
        assert_eq!(deepgram[7], format!("${:.4}", 5.0 * total / 2.0));
        assert!(deepgram[8].contains("ElevenLabs 400 chars"), "{table}");
        assert!(
            deepgram[8].contains("4 s sent to Nova-3, 0 s to Flux"),
            "{table}"
        );
        let off_peak = row(&table, "deepgram", "off-peak");
        let cheaper =
            nova + (2000.0 * 0.15 + 6000.0 * 0.003 + 100.0 * 0.60) / 1e6 + 0.00075 + 0.016;
        assert_eq!(off_peak[5], format!("${:.5}", cheaper / 2.0));
        // Flux's sound at Flux's price, and the plugin's search counted.
        let flux =
            3.0 / 60.0 * 0.0065 + (4000.0 * 0.006 + 50.0 * 1.20) / 1e6 + 0.00075 + 100.0 * 0.00004;
        let flux_row = row(&table, "flux", "peak");
        assert_eq!(&flux_row[2..4], ["1", "1"], "{table}");
        assert_eq!(flux_row[5], format!("${flux:.5}"));
        assert!(
            flux_row[8].contains("0 s sent to Nova-3, 3 s to Flux"),
            "{table}"
        );
        assert!(flux_row[8].contains("100% from the cache"), "{table}");
        // Live: two requests, one answered (the other held back), priced by
        // its four kinds of tokens.
        let live = (600.0 * 0.75 + 400.0 * 3.0 + 200.0 * 4.5 + 300.0 * 12.0 + 200.0 * 0.75) / 1e6;
        let live_row = row(&table, "live", "list");
        assert_eq!(&live_row[2..4], ["2", "1"], "{table}");
        assert_eq!(live_row[5], format!("${:.5}", live / 2.0));
        assert_eq!(live_row[6], format!("${live:.5}"));
        assert!(
            live_row[8]
                .contains("2 generations, 800 text in, 400 audio in, 200 text out, 300 audio out")
        );
        // A line after every arm is in none.
        assert_eq!(table.lines().count(), 1 + 2 + 2 + 1);
    }

    #[test]
    fn a_voice_with_no_price_is_said_not_priced_at_nothing() {
        let mut prices = prices();
        prices.voice_per_character.clear();
        let refused = cost(JOURNAL, &arms(), &prices, None).unwrap_err();
        assert!(refused.contains("no price for ElevenLabs"), "{refused}");
    }

    /// Tonight's journal (19 Oct) is from before #1935: answered lines with
    /// no summary of their voice's pieces. Without an estimate that refuses
    /// the run, never prices the voice at nothing; with one, those lines'
    /// share of the completion tokens is priced, and said as estimated.
    #[test]
    fn an_answered_line_with_no_voice_line_is_refused_or_estimated() {
        let journal = "\
2026-10-19T20:00:05+0700 lamp-4ace lamp-voice[100]: lamp-voice second: request 1 heard 6 words, the last 210 ms after the line ended
2026-10-19T20:00:06+0700 lamp-4ace lamp-voice[100]: lamp-voice second: chat answer: its first words or call 812 ms and its end 1430 ms after it was asked; 1000 prompt tokens, 0 from the cache, 1000 not; 100 completion tokens
2026-10-19T20:00:07+0700 lamp-4ace lamp-voice[100]: lamp-voice second: request 1 first answer audio 900 ms after its words
2026-10-19T20:01:05+0700 lamp-4ace lamp-voice[100]: lamp-voice second: request 2 heard 3 words, the last 200 ms after the line ended
2026-10-19T20:01:06+0700 lamp-4ace lamp-voice[100]: lamp-voice second: chat answer: its first words or call 700 ms and its end 1100 ms after it was asked; 1000 prompt tokens, 0 from the cache, 1000 not; 100 completion tokens
2026-10-19T20:01:07+0700 lamp-4ace lamp-voice[100]: lamp-voice second: request 2 first answer audio 850 ms after its words
";
        let arm = [Arm::parse("deepgram", "20:00", "20:15").unwrap()];
        let refused = cost(journal, &arm, &prices(), None).unwrap_err();
        assert!(
            refused.contains("2 of 2 answered lines") && refused.contains("--voice-estimate"),
            "{refused}"
        );
        let four = VoiceEstimate {
            voice: "ElevenLabs".into(),
            chars_per_token: 4.0,
        };
        let table = cost(journal, &arm, &prices(), Some(&four)).unwrap();
        let llm = (2000.0 * 0.30 + 200.0 * 1.20) / 1e6;
        let peak = row(&table, "deepgram", "peak");
        assert_eq!(peak[6], format!("${:.5}", (llm + 800.0 * 0.00004) / 2.0));
        assert!(
            peak[8].contains("ElevenLabs about 800 chars estimated for 2 of 2 answered lines"),
            "{table}"
        );
        // One line with its own summary: only the other's share is estimated.
        let half = journal.replace(
            "request 2 first answer audio 850 ms after its words\n",
            "request 2 first answer audio 850 ms after its words\n2026-10-19T20:01:08+0700 lamp-4ace lamp-voice[100]: lamp-voice second: request 2 ElevenLabs: 1 of 1 pieces began, 0 on a reused connection, 300 chars; the first piece's first audio byte 280 ms after it went, the slowest 280 ms\n",
        );
        let table = cost(&half, &arm, &prices(), Some(&four)).unwrap();
        let peak = row(&table, "deepgram", "peak");
        assert_eq!(peak[6], format!("${:.5}", (llm + 700.0 * 0.00004) / 2.0));
        assert!(
            peak[8].contains("ElevenLabs 300 chars")
                && peak[8].contains("about 400 chars estimated for 1 of 2 answered lines"),
            "{table}"
        );
        // An estimate in a voice the prices don't have is refused too.
        let unpriced = VoiceEstimate {
            voice: "Gemini".into(),
            chars_per_token: 4.0,
        };
        let refused = cost(journal, &arm, &prices(), Some(&unpriced)).unwrap_err();
        assert!(refused.contains("no price for Gemini"), "{refused}");
    }

    #[test]
    fn a_prices_file_with_a_misspelt_field_is_refused() {
        let wrong = r#"{"deepseek": {}, "search_per_call": 0, "speech_to_text_per_minute": {"nova-3": 0, "flux": 0},
            "voice_per_character": {}, "live_per_million": {"text_in": 0, "audio_in": 0, "text_out": 0, "audio_out": 0},
            "serach_per_call": 1}"#;
        assert!(serde_json::from_str::<Prices>(wrong).is_err());
    }

    /// A chat answer with the cache not said is priced as all not from the
    /// cache, the dearer; one with no usage is counted, not priced.
    #[test]
    fn a_chat_answers_tokens_with_the_cache_not_said_or_no_usage() {
        let mut used = Usage::default();
        used.chat("900 prompt tokens, the cache not said; 30 completion tokens");
        used.chat("500 prompt tokens, 400 from the cache, ? not; 20 completion tokens");
        used.chat("no usage given");
        assert_eq!(
            (
                used.prompt,
                used.cached,
                used.uncached,
                used.completion,
                used.no_usage
            ),
            (1400, 400, 1000, 50, 1)
        );
    }

    #[test]
    fn the_sound_sent_is_read_from_nova_3s_line_and_fluxs() {
        assert_eq!(
            sent_ms("finals cover 1800 of 2400 ms sent, pending=none"),
            Some(2400)
        );
        assert_eq!(sent_ms("finals cover none of 1200 ms sent"), Some(1200));
        assert_eq!(
            sent_ms("flux: turns=1, settled=true, heard to 2900 of 3000 ms sent"),
            Some(3000)
        );
        assert_eq!(sent_ms("nothing sent"), None);
    }
}
