//! A web search the second route's model may call, as functions of its own
//! beside the lamp's, behind `LAMP_SECOND_SEARCH` (off unless set):
//!
//! - `deepseek`: DeepSeek's Search & Fetch API (`POST <base>/v1/tools/search`
//!   and `/v1/tools/fetch`, the key as a bearer token; read from DeepSeek's
//!   docs, api-docs.deepseek.com/api/search, on 19 Oct). `web_search` asks
//!   for three results and gives the model each one's title, its summary for
//!   models and its date; `fetch_page` gives a page's title and its text as
//!   Markdown, cut short. DeepSeek bills $0.75 a thousand successful calls of
//!   each from 1 Nov 2026, and allows 5 a second an account.
//! - `exa`: the search service `second.json` names (`search_url`,
//!   `search_credential_file`, `search_extra`), asked in Exa's `/search`
//!   shape with the key in `x-api-key`; ready for Exa, whose key the lamp
//!   does not have.
//!
//! OpenRouter's web plugin (`LAMP_SECOND_SEARCH=openrouter`) is not here: it
//! rides on OpenRouter's chat request (`Chat::openrouter`). None of these
//! was run against its service.
use super::{Error, Result, chunk, http_client, post};
use serde_json::{Value, json};
use std::time::Duration;

/// The search function's name, as the model calls it.
pub const NAME: &str = "web_search";
/// The fetch function's name (DeepSeek's only).
pub const FETCH: &str = "fetch_page";
/// A turn waits this long for a search or a fetch, then goes on without it.
pub const WITHIN: Duration = Duration::from_secs(3);
/// Results told to the model, and how much of each.
const RESULTS: usize = 3;
const TEXT_CHARS: usize = 500;
/// How much of a fetched page the model is told.
const PAGE_CHARS: usize = 2_000;
/// The most of an answer read.
const MOST_BYTES: usize = 512 * 1024;

enum Backend {
    DeepSeek {
        base: String,
    },
    Exa {
        url: String,
        extra: serde_json::Map<String, Value>,
    },
}

pub struct Search {
    client: reqwest::Client,
    key: String,
    backend: Backend,
}

/// What the model is told of the search. The month and year go in a query
/// for news, results and prices (#1712: on lamp-4ace on 19 Oct "Who won the
/// last Formula One race?" found last season's; the first builder: "try a
/// dated query").
pub(crate) const DESCRIPTION: &str = "Look something up on the web that you cannot know yourself: today's news, a score, opening hours, a price. One short query; for news, results and prices, put the month and year you were told in it, so it finds this month's, not last year's. Say what you found in a sentence or two. Never search a place you guessed: if you aren't sure you heard a place right, or they said near them and you don't know where, ask which area first. If you searched near a place they didn't name, say which place you used.";

impl Search {
    /// DeepSeek's Search & Fetch API.
    pub fn deepseek(key: &str) -> Result<Self> {
        Self::deepseek_at(crate::second_config::DEEPSEEK_BASE_URL, key)
    }

    pub(crate) fn deepseek_at(base: &str, key: &str) -> Result<Self> {
        Ok(Self {
            client: http_client()?,
            key: key.to_owned(),
            backend: Backend::DeepSeek {
                base: base.trim_end_matches('/').to_owned(),
            },
        })
    }

    /// The search service `second.json` names, in Exa's shape.
    pub fn exa(url: &str, key: &str, extra: serde_json::Map<String, Value>) -> Result<Self> {
        Ok(Self {
            client: http_client()?,
            key: key.to_owned(),
            backend: Backend::Exa {
                url: url.to_owned(),
                extra,
            },
        })
    }

    /// The functions as the model is told them ([`DESCRIPTION`]): the search, then for
    /// DeepSeek the fetch.
    pub fn declarations(&self) -> Vec<lamp_gemini::Tool> {
        let mut tools = vec![lamp_gemini::Tool {
            name: NAME.into(),
            description: DESCRIPTION.into(),
            parameters: Some(json!({
                "type": "object",
                "properties": {"query": {"type": "string", "description": "What to search for, in a few words."}},
                "required": ["query"],
            })),
            silent: false,
        }];
        if matches!(self.backend, Backend::DeepSeek { .. }) {
            tools.push(lamp_gemini::Tool {
                name: FETCH.into(),
                description: "Read one web page a search found, when its summary is not \
                    enough. Give its address."
                    .into(),
                parameters: Some(json!({
                    "type": "object",
                    "properties": {"url": {"type": "string", "description": "The page's https address."}},
                    "required": ["url"],
                })),
                silent: false,
            });
        }
        tools
    }

    /// Whether `name` is one of these functions.
    pub fn answers(&self, name: &str) -> bool {
        self.declarations().iter().any(|tool| tool.name == name)
    }

    /// The request for a search of `query`: where, and what.
    pub(crate) fn search_request(&self, query: &str) -> (String, &'static str, Value) {
        match &self.backend {
            Backend::DeepSeek { base } => (
                format!("{base}/v1/tools/search"),
                "authorization",
                json!({"query": query, "count": RESULTS}),
            ),
            Backend::Exa { url, extra } => {
                let mut body = json!({
                    "query": query,
                    "numResults": RESULTS,
                    "contents": {"text": {"maxCharacters": TEXT_CHARS}},
                });
                for (field, value) in extra {
                    if field != "query" {
                        body[field] = value.clone();
                    }
                }
                (url.clone(), "x-api-key", body)
            }
        }
    }

    /// The function's answer for the model's `args`: the results or the
    /// page, or what went wrong in a few words (never the service's own).
    pub async fn answer(&self, name: &str, args: &Value) -> Value {
        if name == FETCH {
            return self.fetched(args).await;
        }
        let Some(query) = args["query"]
            .as_str()
            .map(str::trim)
            .filter(|query| !query.is_empty() && query.chars().count() <= 128)
        else {
            return json!({"error": "a search needs a short query"});
        };
        let (url, header, body) = self.search_request(query);
        match tokio::time::timeout(WITHIN, self.ask(&url, header, &body)).await {
            Ok(Ok(answer)) => results(&answer),
            Ok(Err(error)) => json!({"error": format!("the search failed: {error}")}),
            Err(_) => json!({"error": "the search did not answer in time"}),
        }
    }

    async fn fetched(&self, args: &Value) -> Value {
        let Backend::DeepSeek { base } = &self.backend else {
            return json!({"error": "no page can be read here"});
        };
        let Some(page) = args["url"]
            .as_str()
            .map(str::trim)
            .filter(|url| fetchable(url))
        else {
            return json!({"error": "a page is read by its http or https address"});
        };
        let url = format!("{base}/v1/tools/fetch");
        match tokio::time::timeout(
            WITHIN,
            self.ask(&url, "authorization", &json!({"url": page})),
        )
        .await
        {
            Ok(Ok(answer)) => {
                let cut = |text: &str, chars: usize| text.chars().take(chars).collect::<String>();
                json!({
                    "title": cut(answer["title"].as_str().unwrap_or(""), 200),
                    "content": cut(answer["content"].as_str().unwrap_or(""), PAGE_CHARS),
                })
            }
            Ok(Err(error)) => json!({"error": format!("the page could not be read: {error}")}),
            Err(_) => json!({"error": "the page did not come in time"}),
        }
    }

    /// One call, its whole JSON answer.
    pub(crate) async fn ask(&self, url: &str, header: &'static str, body: &Value) -> Result<Value> {
        let value = if header == "authorization" {
            format!("Bearer {}", self.key)
        } else {
            self.key.clone()
        };
        let mut response = post(&self.client, url, (header, &value), body).await?;
        let mut bytes = Vec::new();
        while let Some(piece) = chunk(&mut response).await? {
            bytes.extend(piece);
            if bytes.len() > MOST_BYTES {
                return Err(Error::Protocol("search answer length"));
            }
        }
        serde_json::from_slice(&bytes).map_err(|_| Error::Protocol("search answer"))
    }
}

/// An address DeepSeek's fetch takes: http or https with a host, no user
/// or password, at most 8,192 characters.
fn fetchable(url: &str) -> bool {
    url.len() <= 8_192
        && reqwest::Url::parse(url).is_ok_and(|url| {
            matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some_and(|host| !host.is_empty())
                && url.username().is_empty()
                && url.password().is_none()
        })
}

/// The first results' titles and text, cut short, for the model: DeepSeek's
/// `summary` (written for models) or Exa's `text`, and the date either gives.
pub(crate) fn results(answer: &Value) -> Value {
    let cut = |text: &str, chars: usize| text.chars().take(chars).collect::<String>();
    let found: Vec<Value> = answer["results"]
        .as_array()
        .into_iter()
        .flatten()
        .take(RESULTS)
        .map(|result| {
            let text = result["summary"]
                .as_str()
                .or_else(|| result["text"].as_str())
                .unwrap_or("");
            json!({
                "title": cut(result["title"].as_str().unwrap_or(""), 200),
                "text": cut(text, TEXT_CHARS),
                "published": result["date"].as_str().or_else(|| result["publishedDate"].as_str()),
            })
        })
        .collect();
    json!({"results": found})
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #1712: news, results and prices are searched with the month and year.
    #[test]
    fn news_results_and_prices_are_searched_with_the_month_and_year() {
        assert!(DESCRIPTION.contains("put the month and year you were told in it"));
        assert!(DESCRIPTION.starts_with("Look something up on the web"));
        // #4337: a guessed place is never searched or said as fact.
        assert!(DESCRIPTION.contains("Never search a place you guessed"));
        assert!(DESCRIPTION.contains("say which place you used"));
    }
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn the_functions_are_valid_and_results_are_cut_short() {
        let deepseek = Search::deepseek("k").unwrap();
        let names: Vec<String> = deepseek
            .declarations()
            .into_iter()
            .map(|tool| {
                tool.validate().unwrap();
                tool.name
            })
            .collect();
        assert_eq!(names, [NAME, FETCH]);
        assert!(deepseek.answers(FETCH));
        let exa = Search::exa("https://search.invalid/search", "k", Default::default()).unwrap();
        assert_eq!(exa.declarations().len(), 1);
        assert!(!exa.answers(FETCH));
        let long = "x".repeat(900);
        let answer = json!({"results": [
            {"title": "One", "summary": long, "snippet": "short", "url": "https://a.invalid", "date": "2026-10-19T00:00:00Z"},
            {"title": "Two", "text": "b", "publishedDate": "2026-10-18"}, {"title": "Three"}, {"title": "Four"},
        ]});
        let told = results(&answer);
        assert_eq!(told["results"].as_array().unwrap().len(), 3);
        assert_eq!(told["results"][0]["text"].as_str().unwrap().len(), 500);
        assert_eq!(told["results"][0]["published"], "2026-10-19T00:00:00Z");
        assert!(told["results"][0].get("url").is_none());
        assert_eq!(told["results"][1]["text"], "b");
        assert_eq!(told["results"][1]["published"], "2026-10-18");
        assert_eq!(results(&json!({})), json!({"results": []}));
        assert!(fetchable("https://example.invalid/page"));
        for unfit in [
            "ftp://example.invalid/",
            "https://user:pw@example.invalid/",
            "not a url",
        ] {
            assert!(!fetchable(unfit), "{unfit}");
        }
    }

    /// One answer from a local server, and the request it got.
    async fn served(body: &'static str) -> (String, tokio::task::JoinHandle<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let n = socket.read(&mut buffer).await.unwrap();
                request.extend_from_slice(&buffer[..n]);
                let text = String::from_utf8_lossy(&request);
                if let Some((head, rest)) = text.split_once("\r\n\r\n") {
                    let length = head
                        .lines()
                        .find_map(|line| {
                            line.to_lowercase()
                                .strip_prefix("content-length: ")
                                .map(str::to_owned)
                        })
                        .and_then(|n| n.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if rest.len() >= length {
                        break;
                    }
                }
            }
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            String::from_utf8(request).unwrap()
        });
        (format!("http://{address}"), task)
    }

    fn body_of(request: &str) -> Value {
        serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap()
    }

    /// `LAMP_SECOND_SEARCH=deepseek`: DeepSeek's search asked for three
    /// results with the key as a bearer token, and its page fetch.
    #[tokio::test]
    async fn deepseek_searches_and_fetches_with_its_request_shapes() {
        let (base, server) = served(
            r#"{"results":[{"rerank_score":2.1,"title":"Saigon weather","url":"https://w.invalid","snippet":"Rain.","summary":"Rain later today.","content":"","site_name":null,"site_icon":null,"date":null,"authority":2}]}"#,
        )
        .await;
        let search = Search::deepseek_at(&base, "ds-key").unwrap();
        let told = search
            .answer(NAME, &json!({"query": "weather in Saigon"}))
            .await;
        assert_eq!(
            told,
            json!({"results": [{"title": "Saigon weather", "text": "Rain later today.", "published": null}]})
        );
        let request = server.await.unwrap();
        let lower = request.to_lowercase();
        assert!(
            lower.starts_with("post /v1/tools/search http/1.1"),
            "{request}"
        );
        assert!(lower.contains("authorization: bearer ds-key"));
        assert_eq!(
            body_of(&request),
            json!({"query": "weather in Saigon", "count": 3})
        );

        let (base, server) =
            served(r##"{"title":"Example","content":"# Example\n\nText."}"##).await;
        let search = Search::deepseek_at(&base, "ds-key").unwrap();
        let page = search
            .answer(FETCH, &json!({"url": "https://example.invalid/a"}))
            .await;
        assert_eq!(
            page,
            json!({"title": "Example", "content": "# Example\n\nText."})
        );
        let request = server.await.unwrap();
        assert!(
            request
                .to_lowercase()
                .starts_with("post /v1/tools/fetch http/1.1")
        );
        assert_eq!(
            body_of(&request),
            json!({"url": "https://example.invalid/a"})
        );
        assert_eq!(
            search
                .answer(NAME, &json!({"query": "x".repeat(129)}))
                .await,
            json!({"error": "a search needs a short query"})
        );
    }

    /// A search that doesn't answer is given up after [`WITHIN`] (3 s), and
    /// the turn goes on, told so: never waited for longer, and never
    /// before. Once the server has the request, Tokio's clock is paused and
    /// stepped to either side of the 3 s, so they aren't waited for (#1806).
    #[tokio::test]
    async fn a_search_that_does_not_answer_is_given_up_after_three_seconds() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        // Takes the request and never answers it; says once it has it.
        let (has, request_in) = tokio::sync::oneshot::channel();
        let _held = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut first = [0u8; 1];
            socket.read_exact(&mut first).await.unwrap();
            let _ = has.send(());
            tokio::time::sleep(Duration::from_secs(60)).await;
            drop(socket);
        });
        let search = Search::deepseek_at(&base, "ds-key").unwrap();
        let began = tokio::time::Instant::now();
        let asked =
            tokio::spawn(async move { search.answer(NAME, &json!({"query": "weather"})).await });
        request_in.await.unwrap();
        tokio::time::pause();
        // The 3 s itself, not `WITHIN`, so a changed bound fails here.
        let bound = Duration::from_secs(3);
        // Saturating: a request that took nearly 3 s of real time to reach
        // the server can't underflow it (the reviewer's P3 on #2121).
        let short_of_it = bound
            .saturating_sub(began.elapsed())
            .saturating_sub(Duration::from_millis(10));
        tokio::time::advance(short_of_it).await;
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        assert!(!asked.is_finished(), "given up before {bound:?}");
        tokio::time::advance(Duration::from_millis(20)).await;
        let told = asked.await.unwrap();
        assert_eq!(told, json!({"error": "the search did not answer in time"}));
        assert!(began.elapsed() < bound + Duration::from_millis(100));
    }

    /// `LAMP_SECOND_SEARCH=exa`: Exa's request shape, its own fields from
    /// `search_extra`, the key in its header.
    #[tokio::test]
    async fn exa_is_asked_in_its_shape() {
        let (base, server) =
            served(r#"{"results":[{"title":"Saigon weather","text":"Rain later."}]}"#).await;
        let search = Search::exa(
            &format!("{base}/search"),
            "s-key",
            serde_json::from_value(json!({"type": "instant", "query": "not this"})).unwrap(),
        )
        .unwrap();
        let told = search
            .answer(NAME, &json!({"query": "weather in Saigon"}))
            .await;
        assert_eq!(
            told,
            json!({"results": [{"title": "Saigon weather", "text": "Rain later.", "published": null}]})
        );
        let request = server.await.unwrap();
        let lower = request.to_lowercase();
        assert!(lower.starts_with("post /search http/1.1"), "{request}");
        assert!(lower.contains("x-api-key: s-key"));
        let body = body_of(&request);
        assert_eq!(body["query"], "weather in Saigon");
        assert_eq!(body["type"], "instant");
        assert_eq!(body["numResults"], 3);
    }
}
