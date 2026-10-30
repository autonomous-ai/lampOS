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
/// Autonomous's own site, whose product pages carry their price as
/// schema.org data the lamp reads itself ([`own_price`]). On lamp-29e7 on 30
/// Oct a searched $399 was given as the ErgoChair Pro's price, and argued
/// for, while its page said $499 (Migrate's finding 5 on #3997).
const OWN_SITE: &str = "www.autonomous.ai";
/// The most of an own product page read: the ErgoChair Pro's was 410 KB on
/// 30 Oct.
const OWN_PAGE_MOST_BYTES: usize = 2 * 1024 * 1024;
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
pub(crate) const DESCRIPTION: &str = "Look something up on the web that you cannot know yourself: today's news, a score, opening hours, a price. One short query; for news, results and prices, put the month and year you were told in it, so it finds this month's, not last year's. A price you find may be out of date: say so. For Autonomous's own products, give only the price autonomous.ai's own product page shows; if someone looking at the page sees another, believe them and never argue. Say what you found in a sentence or two. Never search a place you guessed: if you aren't sure you heard a place right, or they said near them and you don't know where, ask which area first. If you searched near a place they didn't name, say which place you used.";

/// Told with a search's results when they or the query speak of a price.
pub(crate) const SEARCHED_PRICE_NOTE: &str = "A price found by a search may be out of date: if you give one, say it may have changed. Never give one for Autonomous's own products from a search or another site, only the one autonomous.ai's own product page shows. If they are looking at the page and see another price, believe them and never argue.";

/// Told with an Autonomous product page and the price the lamp read on it.
pub(crate) const OWN_PRICE_NOTE: &str = "This is Autonomous's own product page, read just now: give this price, not one found elsewhere. If they are looking at the page and see another, believe them and never argue.";

/// Told with an Autonomous page whose price could not be read.
pub(crate) const OWN_PRICE_UNREAD_NOTE: &str = "No price could be read from Autonomous's own page just now: give none from a search or another site; they can see it on autonomous.ai.";

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
            Ok(Ok(answer)) => results(&answer, query),
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
        let body = json!({"url": page});
        let fetch = tokio::time::timeout(WITHIN, self.ask(&url, "authorization", &body));
        // An own product page is also read here, at once beside the fetch,
        // for the price it carries (`OWN_SITE`).
        let Some(own) = own_page(page) else {
            return fetch_told(fetch.await);
        };
        let (fetched, price) = tokio::join!(
            fetch,
            tokio::time::timeout(WITHIN, own_price(&self.client, &own))
        );
        let mut told = fetch_told(fetched);
        match price {
            Ok(Some(price)) => {
                if told.get("error").is_some() {
                    told = json!({});
                }
                told["price"] = price;
                told["note"] = OWN_PRICE_NOTE.into();
            }
            _ => told["note"] = OWN_PRICE_UNREAD_NOTE.into(),
        }
        told
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

/// DeepSeek's fetch answer as the model is told it: the page's title and
/// text cut short, or what went wrong.
fn fetch_told(fetched: std::result::Result<Result<Value>, tokio::time::error::Elapsed>) -> Value {
    match fetched {
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

/// The address the lamp reads an Autonomous page at itself: https on
/// `OWN_SITE` (the bare `autonomous.ai` moved there, as it redirects),
/// with its path and query; `None` for any other site, a relay's included.
fn own_page(url: &str) -> Option<String> {
    let url = reqwest::Url::parse(url).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !matches!(url.host_str(), Some("autonomous.ai" | OWN_SITE))
        || url.port().is_some()
    {
        return None;
    }
    let query = url
        .query()
        .map(|query| format!("?{query}"))
        .unwrap_or_default();
    Some(format!("https://{OWN_SITE}{}{query}", url.path()))
}

/// The price an Autonomous product page at `url` shows, read from its own
/// schema.org data ([`price_of`]); `None` when it can't be read or has none.
async fn own_price(client: &reqwest::Client, url: &str) -> Option<Value> {
    let mut response = client.get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let mut bytes = Vec::new();
    while let Some(piece) = chunk(&mut response).await.ok()? {
        bytes.extend(piece);
        if bytes.len() > OWN_PAGE_MOST_BYTES {
            return None;
        }
    }
    price_of(&String::from_utf8_lossy(&bytes))
}

/// The price a product page's schema.org data (`application/ld+json`) gives
/// for its product, the first `Product` or `ProductGroup` in it: its name,
/// its currency, and the lowest and highest of its offers (each colour of
/// the ErgoChair Pro's page has one), never a related product's; `None` with
/// no product or offer, or offers in more than one currency.
fn price_of(html: &str) -> Option<Value> {
    let mut rest = html;
    let product = loop {
        let at = rest.find("application/ld+json")?;
        rest = &rest[at..];
        let open = rest.find('>')?;
        rest = &rest[open + 1..];
        let close = rest.find("</script>")?;
        let data = serde_json::from_str::<Value>(&rest[..close]).ok();
        rest = &rest[close..];
        if let Some(product) = data.as_ref().and_then(first_product) {
            break product.clone();
        }
    };
    let (mut prices, mut currencies) = (Vec::new(), Vec::new());
    offers(&product, &mut prices, &mut currencies);
    let [currency] = currencies.as_slice() else {
        return None;
    };
    let lowest = prices.iter().copied().reduce(f64::min)?;
    let highest = prices.iter().copied().reduce(f64::max)?;
    // A whole amount is told as one: 499, not 499.0.
    let amount = |price: f64| {
        if price.fract() == 0.0 && price < 1e15 {
            json!(price as u64)
        } else {
            json!(price)
        }
    };
    let mut price = json!({"currency": currency});
    if lowest < highest {
        price["from"] = amount(lowest);
        price["to"] = amount(highest);
    } else {
        price["price"] = amount(lowest);
    }
    if let Some(name) = product["name"].as_str() {
        price["product"] = json!(name);
    }
    Some(price)
}

/// The first `Product` or `ProductGroup` under `data`, in order.
fn first_product(data: &Value) -> Option<&Value> {
    match data {
        Value::Array(items) => items.iter().find_map(first_product),
        Value::Object(fields) => {
            if matches!(
                fields.get("@type").and_then(Value::as_str),
                Some("Product" | "ProductGroup")
            ) {
                return Some(data);
            }
            fields.values().find_map(first_product)
        }
        _ => None,
    }
}

/// Every offer's prices and currency under a product: an `Offer`'s `price`,
/// an `AggregateOffer`'s `lowPrice` and `highPrice`, never a product it
/// links to.
fn offers(data: &Value, prices: &mut Vec<f64>, currencies: &mut Vec<String>) {
    const LINKED: [&str; 5] = [
        "isRelatedTo",
        "isSimilarTo",
        "isAccessoryOrSparePartFor",
        "isConsumableFor",
        "isVariantOf",
    ];
    let amount = |value: Option<&Value>| match value {
        Some(Value::Number(price)) => price.as_f64(),
        Some(Value::String(price)) => price.trim().parse().ok(),
        _ => None,
    };
    match data {
        Value::Array(items) => {
            for item in items {
                offers(item, prices, currencies);
            }
        }
        Value::Object(fields) => {
            let kind = fields.get("@type").and_then(Value::as_str);
            let found: Vec<f64> = match kind {
                Some("Offer") => amount(fields.get("price")).into_iter().collect(),
                Some("AggregateOffer") => [fields.get("lowPrice"), fields.get("highPrice")]
                    .into_iter()
                    .filter_map(amount)
                    .collect(),
                _ => Vec::new(),
            };
            let found: Vec<f64> = found.into_iter().filter(|price| *price > 0.0).collect();
            if let (false, Some(currency)) = (
                found.is_empty(),
                fields.get("priceCurrency").and_then(Value::as_str),
            ) {
                prices.extend(found);
                if !currencies.iter().any(|known| known == currency) {
                    currencies.push(currency.to_owned());
                }
            }
            for (key, value) in fields {
                if !LINKED.contains(&key.as_str()) {
                    offers(value, prices, currencies);
                }
            }
        }
        _ => {}
    }
}

/// Whether `text` speaks of a price: the word, or an amount with a currency
/// sign or code beside it.
fn speaks_of_price(text: &str) -> bool {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    if words.iter().any(|word| {
        matches!(
            *word,
            "price" | "prices" | "priced" | "cost" | "costs" | "usd" | "vnd" | "dollars"
        )
    }) || lower.contains("how much")
    {
        return true;
    }
    let chars: Vec<char> = lower.chars().collect();
    chars.windows(2).any(|pair| {
        matches!(pair[0], '$' | '€' | '£' | '¥' | '₫' | '₹') && pair[1].is_ascii_digit()
            || pair[0].is_ascii_digit() && matches!(pair[1], '₫' | '€')
    })
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
/// An Autonomous page's address goes too, so `fetch_page` can read its
/// price. When the `query` or a result speaks of a price,
/// [`SEARCHED_PRICE_NOTE`] goes with them.
pub(crate) fn results(answer: &Value, query: &str) -> Value {
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
            let mut told = json!({
                "title": cut(result["title"].as_str().unwrap_or(""), 200),
                "text": cut(text, TEXT_CHARS),
                "published": result["date"].as_str().or_else(|| result["publishedDate"].as_str()),
            });
            if let Some(url) = result["url"].as_str().filter(|url| own_page(url).is_some()) {
                told["url"] = json!(url);
            }
            told
        })
        .collect();
    let priced = speaks_of_price(query)
        || found.iter().any(|result| {
            speaks_of_price(result["title"].as_str().unwrap_or(""))
                || speaks_of_price(result["text"].as_str().unwrap_or(""))
        });
    let mut told = json!({"results": found});
    if priced {
        told["note"] = SEARCHED_PRICE_NOTE.into();
    }
    told
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
        // Migrate's finding 5 on #3997 (lamp-29e7, 30 Oct): a searched price
        // is said as maybe old, our own products' only from our own page,
        // and the person looking at the page is believed.
        assert!(DESCRIPTION.contains("A price you find may be out of date: say so."));
        assert!(DESCRIPTION.contains("only the price autonomous.ai's own product page shows"));
        assert!(DESCRIPTION.contains("believe them and never argue"));
    }

    /// Only Autonomous's own site is read for its price, at `OWN_SITE` over
    /// https; a relay on a subdomain, a look-alike or a port is not.
    #[test]
    fn only_autonomous_own_pages_are_read_for_their_price() {
        for (url, own) in [
            (
                "https://www.autonomous.ai/office-chairs/ergonomic-chair",
                Some("https://www.autonomous.ai/office-chairs/ergonomic-chair"),
            ),
            (
                "http://autonomous.ai/standing-desks/smartdesk-pro?color=white",
                Some("https://www.autonomous.ai/standing-desks/smartdesk-pro?color=white"),
            ),
            ("https://campaign-api.autonomous.ai/api/v1/ai", None),
            ("https://autonomous.ai.example.invalid/chair", None),
            ("https://notautonomous.ai/chair", None),
            ("https://www.autonomous.ai:8443/chair", None),
            ("ftp://www.autonomous.ai/chair", None),
            ("not a url", None),
        ] {
            assert_eq!(own_page(url).as_deref(), own, "{url}");
        }
    }

    /// The trimmed shape of the ErgoChair Pro's page on 30 Oct: a product
    /// group whose variants each carry an offer, then a page of questions.
    const OWN_PAGE: &str = r#"<html><head><script type="application/ld+json">{"@context":"https://schema.org","@graph":[{"@type":"ProductGroup","name":"ErgoChair Pro","hasVariant":[{"@type":"Product","name":"Black","offers":{"@type":"Offer","priceCurrency":"USD","price":499}},{"@type":"Product","name":"Grey","offers":{"@type":"Offer","priceCurrency":"USD","price":"549.00"}}]}]}</script><script type="application/ld+json">{"@graph":[{"@type":"FAQPage","mainEntity":[]}]}</script></head><body>Was $399</body></html>"#;

    /// The price is read from the page's own schema.org data, never its
    /// text: the "$399" in the body is not it.
    #[test]
    fn a_product_page_price_is_read_from_its_own_data() {
        assert_eq!(
            price_of(OWN_PAGE),
            Some(json!({"product": "ErgoChair Pro", "currency": "USD", "from": 499, "to": 549}))
        );
        let one = r#"<script type="application/ld+json">{"@type":"Product","name":"Desk","offers":[{"@type":"Offer","priceCurrency":"USD","price":"399"}]}</script>"#;
        assert_eq!(
            price_of(one),
            Some(json!({"product": "Desk", "currency": "USD", "price": 399}))
        );
        // A related product's offer is not this one's; an aggregate's range
        // is.
        let related = r#"<script type="application/ld+json">{"@type":"Product","name":"Desk","offers":{"@type":"AggregateOffer","priceCurrency":"USD","lowPrice":"399","highPrice":599},"isRelatedTo":{"@type":"Product","name":"Mat","offers":{"@type":"Offer","priceCurrency":"USD","price":49}}}</script>"#;
        assert_eq!(
            price_of(related),
            Some(json!({"product": "Desk", "currency": "USD", "from": 399, "to": 599}))
        );
        let cents = r#"<script type="application/ld+json">{"@type":"Product","name":"Mat","offers":{"@type":"Offer","priceCurrency":"USD","price":"49.99"}}</script>"#;
        assert_eq!(
            price_of(cents),
            Some(json!({"product": "Mat", "currency": "USD", "price": 49.99}))
        );
        for none in [
            "<html><body>Only $399 today</body></html>",
            r#"<script type="application/ld+json">{"@type":"FAQPage"}</script>"#,
            r#"<script type="application/ld+json">not json</script>"#,
            r#"<script type="application/ld+json">{"@type":"Offer","price":5,"priceCurrency":"USD"}</script>"#,
            r#"<script type="application/ld+json">{"@type":"Product","offers":{"@type":"Offer","price":0,"priceCurrency":"USD"}}</script>"#,
            r#"<script type="application/ld+json">{"@type":"Product","offers":[{"@type":"Offer","price":1,"priceCurrency":"USD"},{"@type":"Offer","price":2,"priceCurrency":"VND"}]}</script>"#,
            r#"<script type="application/ld+json">{"@type":"Product","offers":{"@type":"Offer","price":1"#,
        ] {
            assert_eq!(price_of(none), None, "{none}");
        }
    }

    #[test]
    fn a_price_is_heard_in_words_signs_and_codes() {
        for priced in [
            "ErgoChair Pro price October 2026",
            "How much is the SmartDesk",
            "It is $399 on the US site",
            "Giá 12.990.000₫",
            "499€ in Germany",
            "about 500 USD",
        ] {
            assert!(speaks_of_price(priced), "{priced}");
        }
        for plain in [
            "weather in Saigon",
            "Costa Rica news",
            "Dong Khoi street opening hours",
            "who won the race",
        ] {
            assert!(!speaks_of_price(plain), "{plain}");
        }
    }

    /// A search speaking of a price carries the note that it may be old,
    /// and an Autonomous result its address, so `fetch_page` can read it.
    #[test]
    fn searched_prices_are_told_as_maybe_old() {
        let answer = json!({"results": [
            {"title": "ErgoChair Pro", "summary": "Now $399.", "url": "https://www.autonomous.ai/office-chairs/ergonomic-chair"},
            {"title": "Review", "summary": "A good chair.", "url": "https://reviews.invalid/chair"},
        ]});
        let told = results(&answer, "ErgoChair Pro");
        assert_eq!(told["note"], SEARCHED_PRICE_NOTE);
        assert_eq!(
            told["results"][0]["url"],
            "https://www.autonomous.ai/office-chairs/ergonomic-chair"
        );
        assert!(told["results"][1].get("url").is_none());
        let plain = json!({"results": [{"title": "Saigon weather", "summary": "Rain later."}]});
        assert!(results(&plain, "weather in Saigon").get("note").is_none());
        assert_eq!(
            results(&plain, "how much is a SmartDesk")["note"],
            SEARCHED_PRICE_NOTE
        );
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
        let told = results(&answer, "anything");
        assert_eq!(told["results"].as_array().unwrap().len(), 3);
        assert_eq!(told["results"][0]["text"].as_str().unwrap().len(), 500);
        assert_eq!(told["results"][0]["published"], "2026-10-19T00:00:00Z");
        assert!(told["results"][0].get("url").is_none());
        assert_eq!(told["results"][1]["text"], "b");
        assert_eq!(told["results"][1]["published"], "2026-10-18");
        assert_eq!(results(&json!({}), "anything"), json!({"results": []}));
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

    /// An own page is read with a plain GET and its price taken from its
    /// data.
    #[tokio::test]
    async fn an_own_page_is_read_for_its_price() {
        let (base, server) = served(OWN_PAGE).await;
        let client = http_client().unwrap();
        let price = own_price(&client, &format!("{base}/office-chairs/ergonomic-chair")).await;
        assert_eq!(
            price,
            Some(json!({"product": "ErgoChair Pro", "currency": "USD", "from": 499, "to": 549}))
        );
        let request = server.await.unwrap();
        assert!(
            request
                .to_lowercase()
                .starts_with("get /office-chairs/ergonomic-chair http/1.1"),
            "{request}"
        );
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
