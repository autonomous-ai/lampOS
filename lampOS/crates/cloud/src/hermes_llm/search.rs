//! The web search for the agents on the lamp (OpenClaw, Hermes), served by
//! the forwarder at `GET /v1/search/<search token>/search?q=…&format=json`
//! and answered in SearXNG's JSON shape, so each engine's own SearXNG
//! client uses it with the base `http://127.0.0.1:7741/v1/search/<search
//! token>`. Those clients send no header, so the token is in the path; it
//! is the [`super::SEARCH_TOKEN_FILE`]'s, never the chat's, so the chat's
//! token is never in a URL. A client that sends a header may instead ask
//! `GET /v1/search/search?q=…` with `Authorization: Bearer <search token>`.
//!
//! The search token is made to be safe to leak (Hermes shows a tool's
//! endpoint to its model on an error): it opens only the search (the chat
//! and the models refuse it, 401), only from loopback, and only within the
//! search's own bounds below, never the chat's budget or the relay's key.
//!
//! Where a search goes:
//! - DeepSeek's search (`POST <relay DeepSeek>/v1/tools/search`, the key
//!   lamp-cloud already holds as the bearer) first: the same search the
//!   voice uses (`lamp-voice`'s `second_provider::net::search`), on the
//!   relay, with no key asked of the person. DeepSeek allows 5 calls a second
//!   an account, so each lamp keeps to [`PER_MINUTE`] a minute, at least
//!   [`SPACED`] apart, a budget of its own beside the chat's.
//! - DuckDuckGo's HTML page ([`DUCKDUCKGO`]) when DeepSeek fails (a
//!   timeout, 429, 5xx, no answer) or the lamp's budget is used: free and
//!   needs no key, but unofficial and rate-limited, so only a best-effort
//!   fallback, kept to its own budget.
//! - Both failing: 502, no results, both named in `unresponsive_engines`.
//!
//! A query is at most [`MOST_QUERY_CHARS`]; each engine has a deadline and
//! a most answer read. The query, the token and the key are never logged:
//! a search's journal line holds the engine, a count and the reasons only.
use super::{Refusal, Started, finish, plain, same, send};
use serde_json::{Value, json};
use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// The search's paths start with this, then the search token.
pub const PREFIX: &str = "/v1/search/";
/// The key-free fallback: DuckDuckGo's page without scripts.
pub const DUCKDUCKGO: &str = "https://html.duckduckgo.com/html/";
/// Results asked of DeepSeek and given from either engine.
pub const COUNT: usize = 5;
/// The longest query asked; a longer one is cut to this many characters.
pub const MOST_QUERY_CHARS: usize = 256;
/// Searches answered at the same time; past them, 429.
pub const AT_ONCE: usize = 2;
/// Calls to each engine started in any minute, per lamp.
pub const PER_MINUTE: usize = 30;
/// Calls to each engine are at least this far apart; one more may wait
/// for its turn, past that the engine is passed over.
pub const SPACED: Duration = Duration::from_secs(1);
/// Each engine's deadline.
const DEEPSEEK_WITHIN: Duration = Duration::from_secs(6);
const FALLBACK_WITHIN: Duration = Duration::from_secs(6);
/// The most of an engine's answer read.
const MOST_ANSWER: usize = 512 * 1024;
const MOST_PAGE: usize = 1024 * 1024;
/// How much of each result's title and text is given.
const TITLE_CHARS: usize = 300;
const CONTENT_CHARS: usize = 1_000;
/// DuckDuckGo turns away a request with no browser's name.
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux aarch64; rv:128.0) Gecko/20100101 Firefox/128.0";

/// Why an engine gave nothing, as `unresponsive_engines` says it.
type Why = &'static str;
const LIMITED: Why = "rate limited on the lamp";
const TIMEOUT: Why = "timeout";
const TOO_MANY: Why = "too many requests";
const HTTP_ERROR: Why = "HTTP error";
const UNREACHABLE: Why = "unreachable";
const UNREADABLE: Why = "unreadable answer";

/// A search as asked and let through, with its place among the
/// [`AT_ONCE`].
pub(super) struct Ask {
    query: String,
    _running: OwnedSemaphorePermit,
}

/// Where a search's line goes: the journal, or a test's own list.
type Note = Arc<dyn Fn(&str) + Send + Sync>;

pub(super) struct Search {
    deepseek: String,
    fallback: String,
    key: String,
    token: String,
    client: reqwest::Client,
    running: Arc<Semaphore>,
    deepseek_rate: Mutex<Started>,
    fallback_rate: Mutex<Started>,
    within: (Duration, Duration),
    note: Note,
}

impl Search {
    /// DeepSeek's search under the relay's DeepSeek `base`, with `key`;
    /// `fallback` the key-free page; searches must carry `token`.
    pub(super) fn new(base: &str, key: &str, token: &str, fallback: &str) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .tls_backend_preconfigured(crate::beat::tls()?)
            .https_only(!cfg!(test))
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(DEEPSEEK_WITHIN)
            .timeout(DEEPSEEK_WITHIN.max(FALLBACK_WITHIN))
            .build()
            .map_err(|_| "the web search's HTTPS client".to_owned())?;
        Ok(Self {
            deepseek: format!("{}/v1/tools/search", base.trim_end_matches('/')),
            fallback: fallback.to_owned(),
            key: key.to_owned(),
            token: token.to_owned(),
            client,
            running: Arc::new(Semaphore::new(AT_ONCE)),
            deepseek_rate: Mutex::new(Started::new(PER_MINUTE, SPACED)),
            fallback_rate: Mutex::new(Started::new(PER_MINUTE, SPACED)),
            within: (DEEPSEEK_WITHIN, FALLBACK_WITHIN),
            note: Arc::new(|line: &str| eprintln!("lamp-cloud: the Hermes forwarder: {line}")),
        })
    }

    /// A search request's head read: `rest` is its path past [`PREFIX`],
    /// `target` the whole request target, `bearer` its header's token. The
    /// token first, so a wrong one learns nothing of the rest.
    pub(super) fn ask(
        &self,
        method: &str,
        target: &str,
        rest: &str,
        bearer: Option<&str>,
    ) -> Result<Ask, Refusal> {
        let ours = |given: &str| plain(given) && same(given.as_bytes(), self.token.as_bytes());
        let under = match rest.split_once('/') {
            Some((given, under)) if ours(given) => under,
            None if ours(rest) => "",
            // `/v1/search/search`, the token as the bearer.
            _ if matches!(rest, "search" | "search/") && bearer.is_some_and(ours) => rest,
            _ => return Err(Refusal::Unauthorized),
        };
        if !matches!(under, "search" | "search/") {
            return Err(Refusal::NotFound);
        }
        if method != "GET" {
            return Err(Refusal::NotAllowed);
        }
        let url =
            reqwest::Url::parse(&format!("http://127.0.0.1{target}")).map_err(|_| Refusal::Bad)?;
        let (mut query, mut format) = (None, None);
        for (name, value) in url.query_pairs() {
            match name.as_ref() {
                "q" => query = Some(value.into_owned()),
                "format" => format = Some(value.into_owned()),
                // `pageno`, `language` and the rest: taken, not used; each
                // search is the first page.
                _ => {}
            }
        }
        if format.is_some_and(|format| format != "json") {
            return Err(Refusal::NotJson);
        }
        let query: String = query
            .as_deref()
            .map(str::trim)
            .filter(|query| !query.is_empty())
            .ok_or(Refusal::NoQuery)?
            .chars()
            .take(MOST_QUERY_CHARS)
            .collect();
        let running = self
            .running
            .clone()
            .try_acquire_owned()
            .map_err(|_| Refusal::TooMany)?;
        Ok(Ask {
            query: query.trim().to_owned(),
            _running: running,
        })
    }

    /// The search's answer written whole, then the stream closed.
    pub(super) async fn answer(&self, stream: &mut TcpStream, ask: Ask) -> io::Result<()> {
        let (code, body) = self.find(&ask.query).await;
        let body = body.to_string();
        let reason = if code == 200 { "OK" } else { "Bad Gateway" };
        let head = format!(
            "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\n\
             Cache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        send(stream, head.as_bytes()).await?;
        send(stream, body.as_bytes()).await?;
        finish(stream).await
    }

    /// DeepSeek, then DuckDuckGo: the status and the SearXNG answer.
    async fn find(&self, query: &str) -> (u16, Value) {
        let mut down = Vec::new();
        let found = match self.deepseek(query).await {
            Ok(results) => Some(("deepseek", results)),
            Err(why) => {
                down.push(json!(["deepseek", why]));
                match self.duckduckgo(query).await {
                    Ok(results) => Some(("duckduckgo", results)),
                    Err(why) => {
                        down.push(json!(["duckduckgo", why]));
                        None
                    }
                }
            }
        };
        let failed = down
            .iter()
            .filter_map(|engine| Some(format!("{} ({})", engine[0].as_str()?, engine[1].as_str()?)))
            .collect::<Vec<_>>()
            .join(", ");
        let (code, results) = match found {
            Some((engine, results)) => {
                (self.note)(&format!(
                    "a search: {} results from {engine}{}",
                    results.len(),
                    if failed.is_empty() {
                        String::new()
                    } else {
                        format!(", after {failed}")
                    }
                ));
                (200, results)
            }
            None => {
                (self.note)(&format!("a search: no results, {failed}"));
                (502, Vec::new())
            }
        };
        let answer = json!({
            "query": query,
            "number_of_results": results.len(),
            "results": results,
            "answers": [],
            "suggestions": [],
            "infoboxes": [],
            "unresponsive_engines": down,
        });
        (code, answer)
    }

    /// The wait for `rate`'s next turn, or [`LIMITED`].
    async fn turn(rate: &Mutex<Started>) -> Result<(), Why> {
        let wait = rate
            .lock()
            .ok()
            .and_then(|mut rate| rate.take(Instant::now()))
            .ok_or(LIMITED)?;
        tokio::time::sleep(wait).await;
        Ok(())
    }

    async fn deepseek(&self, query: &str) -> Result<Vec<Value>, Why> {
        Self::turn(&self.deepseek_rate).await?;
        let call = async {
            let answer = self
                .client
                .post(&self.deepseek)
                .bearer_auth(&self.key)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(json!({"query": query, "count": COUNT}).to_string())
                .send()
                .await
                .map_err(failed)?;
            let bytes = read(answer, MOST_ANSWER).await?;
            let answer: Value = serde_json::from_slice(&bytes).map_err(|_| UNREADABLE)?;
            from_deepseek(&answer).ok_or(UNREADABLE)
        };
        tokio::time::timeout(self.within.0, call)
            .await
            .map_err(|_| TIMEOUT)?
    }

    async fn duckduckgo(&self, query: &str) -> Result<Vec<Value>, Why> {
        Self::turn(&self.fallback_rate).await?;
        let call = async {
            let url = reqwest::Url::parse_with_params(&self.fallback, [("q", query)])
                .map_err(|_| UNREACHABLE)?;
            let answer = self
                .client
                .get(url)
                .header(reqwest::header::USER_AGENT, USER_AGENT)
                .send()
                .await
                .map_err(failed)?;
            // DuckDuckGo answers 202 with a challenge when it limits.
            if answer.status() == reqwest::StatusCode::ACCEPTED {
                return Err(TOO_MANY);
            }
            let page = read(answer, MOST_PAGE).await?;
            let page = String::from_utf8_lossy(&page);
            let results = from_duckduckgo(&page);
            if results.is_empty() && page.contains("anomaly") {
                return Err(TOO_MANY);
            }
            Ok(results)
        };
        tokio::time::timeout(self.within.1, call)
            .await
            .map_err(|_| TIMEOUT)?
    }
}

/// A failed call's reason, without its error (which may hold the URL).
fn failed(error: reqwest::Error) -> Why {
    if error.is_timeout() {
        TIMEOUT
    } else {
        UNREACHABLE
    }
}

/// A successful answer's body, at most `most` bytes.
async fn read(mut answer: reqwest::Response, most: usize) -> Result<Vec<u8>, Why> {
    let status = answer.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(TOO_MANY);
    }
    if !status.is_success() {
        return Err(HTTP_ERROR);
    }
    let mut bytes = Vec::new();
    while let Some(piece) = answer.chunk().await.map_err(failed)? {
        bytes.extend_from_slice(&piece);
        if bytes.len() > most {
            return Err(UNREADABLE);
        }
    }
    Ok(bytes)
}

fn cut(text: &str, chars: usize) -> String {
    text.chars().take(chars).collect()
}

/// An `http(s)` address with a host, as a result's `url` must be.
fn web_address(url: &str) -> Option<reqwest::Url> {
    reqwest::Url::parse(url).ok().filter(|url| {
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some_and(|host| !host.is_empty())
    })
}

/// DeepSeek's `results[]` (`title`, `url`, `summary`, `date`) as
/// SearXNG's: the summary as `content`, the date as `publishedDate`.
/// None when the answer has no list of results.
fn from_deepseek(answer: &Value) -> Option<Vec<Value>> {
    let results = answer.get("results")?.as_array()?;
    Some(
        results
            .iter()
            .filter_map(|result| {
                let url = result["url"]
                    .as_str()
                    .filter(|url| web_address(url).is_some())?;
                let content = result["summary"]
                    .as_str()
                    .or_else(|| result["text"].as_str())
                    .unwrap_or("");
                let mut told = json!({
                    "title": cut(result["title"].as_str().unwrap_or(url), TITLE_CHARS),
                    "url": url,
                    "content": cut(content, CONTENT_CHARS),
                    "engine": "deepseek",
                });
                if let Some(date) = result["date"].as_str().filter(|date| !date.is_empty()) {
                    told["publishedDate"] = json!(date);
                }
                Some(told)
            })
            .take(COUNT)
            .collect(),
    )
}

/// The results on DuckDuckGo's HTML page: each `result__a` link's title
/// and address (out of DuckDuckGo's `/l/?uddg=` redirect), and the
/// `result__snippet` after it. Ads (DuckDuckGo's own addresses) are left
/// out. Best effort: the page is not a published interface.
fn from_duckduckgo(page: &str) -> Vec<Value> {
    let starts = class_at(page, "result__a");
    let mut results = Vec::new();
    for (index, &at) in starts.iter().enumerate() {
        let end = starts.get(index + 1).copied().unwrap_or(page.len());
        let Some(open) = page[..at].rfind('<') else {
            continue;
        };
        let Some(close) = page[at..end].find('>').map(|close| at + close) else {
            continue;
        };
        let tag = &page[open..close];
        let Some(url) = attribute(tag, "href").and_then(|href| real_address(&href)) else {
            continue;
        };
        let title = text(until_closed(&page[close + 1..end]));
        let content = class_at(&page[close..end], "result__snippet")
            .first()
            .and_then(|snippet| {
                let from = close + snippet;
                page[from..end]
                    .find('>')
                    .map(|open| text(until_closed(&page[from + open + 1..end])))
            })
            .unwrap_or_default();
        if title.is_empty() {
            continue;
        }
        results.push(json!({
            "title": cut(&title, TITLE_CHARS),
            "url": url,
            "content": cut(&content, CONTENT_CHARS),
            "engine": "duckduckgo",
        }));
        if results.len() == COUNT {
            break;
        }
    }
    results
}

/// Where `class` is one of an element's classes in `page`.
fn class_at(page: &str, class: &str) -> Vec<usize> {
    page.match_indices(class)
        .filter(|(at, _)| {
            let before = page[..*at].chars().next_back();
            let after = page[at + class.len()..].chars().next();
            matches!(before, Some('"' | ' ')) && matches!(after, Some('"' | ' '))
        })
        .map(|(at, _)| at)
        .collect()
}

/// `name`'s value in a tag, entities decoded.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let at = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    let length = tag[at..].find('"')?;
    Some(entities(&tag[at..at + length]))
}

/// The text up to the element's end: the first closing `a`, `td` or `div`.
fn until_closed(rest: &str) -> &str {
    let end = ["</a>", "</td>", "</div>"]
        .iter()
        .filter_map(|closing| rest.find(closing))
        .min()
        .unwrap_or(rest.len());
    &rest[..end]
}

/// A result link's real address: DuckDuckGo's redirect undone, never one
/// of DuckDuckGo's own (an ad's).
fn real_address(href: &str) -> Option<String> {
    let href = if href.starts_with("//") {
        format!("https:{href}")
    } else {
        href.to_owned()
    };
    let mut url = web_address(&href)?;
    if url
        .host_str()
        .is_some_and(|host| host == "duckduckgo.com" || host.ends_with(".duckduckgo.com"))
    {
        let real = url
            .query_pairs()
            .find(|(name, _)| name == "uddg")
            .map(|(_, value)| value.into_owned())?;
        url = web_address(&real)?;
        if url
            .host_str()
            .is_some_and(|host| host.ends_with("duckduckgo.com"))
        {
            return None;
        }
    }
    Some(url.to_string())
}

/// Markup's text: tags dropped, entities decoded, spaces folded.
fn text(markup: &str) -> String {
    let mut plain = String::with_capacity(markup.len());
    let mut in_tag = false;
    for character in markup.chars() {
        match character {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => plain.push(character),
            _ => {}
        }
    }
    entities(&plain)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// HTML's entities decoded: the named few a search page uses, and numbers.
fn entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest
            .char_indices()
            .take(12)
            .find(|(_, character)| *character == ';')
            .map(|(end, _)| end)
        else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..end];
        let decoded = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ => name
                .strip_prefix("#x")
                .or_else(|| name.strip_prefix("#X"))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| {
                    name.strip_prefix('#')
                        .and_then(|number| number.parse().ok())
                })
                .and_then(char::from_u32),
        };
        match decoded {
            Some(character) => {
                out.push(character);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests;
