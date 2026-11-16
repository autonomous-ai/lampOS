//! The character's energy level worked out by the model from the owner's
//! words (#7314), off unless `LAMP_ENERGY_FROM_MODEL=on`. One short call to
//! the voice's DeepSeek on the company relay ([`crate::hermes_llm`]'s base,
//! model and key, which lamp-cloud already holds), only when the kept words
//! are new: once per save, never per turn. A level the owner picked in the
//! app (`chosen`) is never asked about. Any failure, a timeout
//! ([`WITHIN`]) or an answer that is not one digit from 1 to 5 is level 1,
//! as the lamp always moved. No word of the owner's is ever logged.
use serde_json::{Value, json};
use std::time::Duration;

/// The switch.
pub const SWITCH: &str = "LAMP_ENERGY_FROM_MODEL";
/// The longest one rating is waited for, its one retry included.
pub const WITHIN: Duration = Duration::from_secs(10);

/// Whether the switch is exactly `on`.
pub fn on_from(value: Option<&str>) -> bool {
    value == Some("on")
}

/// What the model is asked, the words quoted after the question.
pub fn prompt(words: &str) -> String {
    format!(
        "Rate this character's energy from 1 (calm) to 5 (very lively). Answer with one digit only.\nCharacter: \"{words}\""
    )
}

/// The level in the model's whole answer: one digit from 1 to 5, a full
/// stop after it allowed; anything else is none.
pub fn level_in(answer: &str) -> Option<u8> {
    let answer = answer.trim().trim_end_matches('.');
    match answer.as_bytes() {
        [digit @ b'1'..=b'5'] => Some(digit - b'0'),
        _ => None,
    }
}

/// The text of a streamed chat answer (Server-Sent Events), or of a whole
/// one, put together.
fn text_of(body: &str) -> String {
    let streamed: String = body
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .filter_map(|data| serde_json::from_str::<Value>(data.trim()).ok())
        .filter_map(|event| {
            event["choices"][0]["delta"]["content"]
                .as_str()
                .map(str::to_owned)
        })
        .collect();
    if !streamed.is_empty() {
        return streamed;
    }
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|whole| {
            whole["choices"][0]["message"]["content"]
                .as_str()
                .map(str::to_owned)
        })
        .unwrap_or_default()
}

/// Asks the model, and remembers the last words it rated and their level.
pub struct Rater {
    base: String,
    key: String,
    client: reqwest::Client,
    // ponytail: in memory only, so a restart of lamp-cloud asks once more
    // for the same words; keep it in the state folder if that matters.
    last: Option<(String, u8)>,
}

impl Rater {
    /// The relay's DeepSeek at `base` (`hermes_llm::relay_base`), asked
    /// with `key`.
    pub fn new(base: &str, key: &str) -> Result<Self, String> {
        if key.trim().is_empty() {
            return Err("the energy rating needs the lamp's key".into());
        }
        let client = reqwest::Client::builder()
            .tls_backend_preconfigured(crate::beat::tls()?)
            .https_only(!cfg!(test))
            .redirect(reqwest::redirect::Policy::none())
            .timeout(WITHIN)
            .build()
            .map_err(|_| "the energy rating's HTTPS client".to_owned())?;
        Ok(Self {
            base: base.trim().trim_end_matches('/').to_owned(),
            key: key.trim().to_owned(),
            client,
            last: None,
        })
    }

    /// The level for `words`: the one rated last when they are the same
    /// words, else asked once (one retry, the relay's 404s come and go),
    /// 1 on any failure.
    pub async fn level(&mut self, words: &str) -> u8 {
        if let Some((rated, level)) = &self.last
            && rated == words
        {
            return *level;
        }
        let level = tokio::time::timeout(WITHIN, async {
            for _ in 0..2 {
                if let Some(level) = self.ask(words).await {
                    return Some(level);
                }
            }
            None
        })
        .await
        .ok()
        .flatten()
        .unwrap_or(1);
        self.last = Some((words.to_owned(), level));
        level
    }

    async fn ask(&self, words: &str) -> Option<u8> {
        let body = json!({
            "model": crate::hermes_llm::MODEL,
            "messages": [{"role": "user", "content": prompt(words)}],
            "max_tokens": 4,
            "temperature": 0,
            "stream": true,
        });
        let answer = self
            .client
            .post(format!("{}/chat/completions", self.base))
            .bearer_auth(&self.key)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.to_string())
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?;
        level_in(&text_of(&answer.text().await.ok()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn only_one_digit_from_one_to_five_is_a_level() {
        assert_eq!(level_in("5"), Some(5));
        assert_eq!(level_in(" 3.\n"), Some(3));
        for none in ["", "0", "6", "35", "level 4", "four"] {
            assert_eq!(level_in(none), None, "{none:?}");
        }
        assert!(prompt("quick to laugh").ends_with("\"quick to laugh\""));
        assert!(!on_from(None) && !on_from(Some("ON")) && on_from(Some("on")));
    }

    #[test]
    fn a_streamed_or_whole_answer_is_read() {
        let streamed = "data: {\"choices\":[{\"delta\":{\"content\":\"4\"}}]}\n\ndata: [DONE]\n";
        assert_eq!(text_of(streamed), "4");
        assert_eq!(text_of(r#"{"choices":[{"message":{"content":"2"}}]}"#), "2");
    }

    /// A stand-in relay on loopback answering each request with `answer`;
    /// it counts the requests.
    async fn relay(
        answer: &'static str,
    ) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let asked = std::sync::Arc::new(AtomicUsize::new(0));
        let counted = asked.clone();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                counted.fetch_add(1, Ordering::SeqCst);
                let mut chunk = [0u8; 8192];
                let _ = stream.read(&mut chunk).await;
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                    answer.len()
                );
                let _ = stream.write_all(reply.as_bytes()).await;
            }
        });
        (format!("http://{address}/v1/deepseek"), asked)
    }

    /// #7314: the model is asked once for the same words, again for new
    /// ones; a relay that can't be reached is level 1.
    #[tokio::test]
    async fn the_model_is_asked_once_per_saved_words_and_a_failure_is_level_one() {
        use std::sync::atomic::Ordering;
        let (base, asked) =
            relay("data: {\"choices\":[{\"delta\":{\"content\":\"5\"}}]}\n\ndata: [DONE]\n").await;
        let mut rater = Rater::new(&base, "key").unwrap();
        assert_eq!(rater.level("quick to laugh").await, 5);
        assert_eq!(rater.level("quick to laugh").await, 5);
        assert_eq!(asked.load(Ordering::SeqCst), 1);
        assert_eq!(rater.level("silly").await, 5);
        assert_eq!(asked.load(Ordering::SeqCst), 2);

        let mut gone = Rater::new("http://127.0.0.1:9/v1/deepseek", "key").unwrap();
        assert_eq!(gone.level("joyful").await, 1);
        assert!(Rater::new(&base, " ").is_err());
    }
}
