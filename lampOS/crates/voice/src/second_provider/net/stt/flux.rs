//! Deepgram's Flux (`flux-general-en` on `/v2/listen`), on trial against
//! Nova-3 behind `LAMP_SECOND_FLUX=on`, off unless set (the hearing half of
//! #1712's quality plan, the first builder at 13:09). Flux tells turns
//! itself: each `TurnInfo` carries one turn's words so far, and its
//! `EndOfTurn` says the turn is over, by Flux's own model (`trigger`
//! `model`), by `ForceEndTurn` (`manual`) or after `eot_timeout_ms` of
//! quiet (`timeout`). The coordinator still ends the line, as with Nova-3:
//! the trial measures how soon the words are there at the line's end, and
//! how far ahead of it Flux's own end of turn came.
//!
//! At the end of the line, as with Nova-3's `Finalize` (#779): words whose
//! turn has ended, with Flux's windows covering all the audio sent, are the
//! answer at once. Otherwise `ForceEndTurn` asks for the turn to end with
//! what was heard, waited for as long as `Finalize` is ([`FINALIZE`]); a
//! `Warning` that no turn is in progress answers it as well. Then
//! `CloseStream`, which ends no turn: each turn's latest words are the
//! answer.
use super::{COVERED, Early, Error, FINALIZE, KEEP_ALIVE, Outgoing, Replies, Result};
use crate::second_config::{FLUX_ENV, SecondConfig};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    sync::{Arc, Once, OnceLock},
    time::Duration,
};
use tokio::{sync::mpsc, time::Instant};
use tokio_tungstenite::tungstenite::Message;

pub(super) const MODEL: &str = "flux-general-en";
const DEEPGRAM: &str = "wss://api.deepgram.com/v2/listen";
/// Deepgram's advice for Flux: 80 ms of audio a message, at 16 kHz in
/// 16 bits. The route queues 20 ms frames; they go four at a time, and
/// what is left goes at once at the end of the line or a flush.
const CHUNK_BYTES: usize = 2_560;
/// Flux's reference names no `KeepAlive`: an idle socket is kept with 80 ms
/// of quiet.
const QUIET: [u8; CHUNK_BYTES] = [0; CHUNK_BYTES];
pub(super) const FORCE_END_TURN: &str = r#"{"type":"ForceEndTurn"}"#;
const CLOSE_STREAM: &str = r#"{"type":"CloseStream"}"#;

/// How sure Flux is to end a turn early (`eager_eot_threshold`): with
/// `LAMP_SECOND_PREPARED=on` it says `EagerEndOfTurn` this sure, its words go
/// to the chat at once ([`Early::Words`]), and `TurnResumed` takes them back
/// ([`Early::Resumed`]) (#1712, the first builder at 17:02: about 0.75 s down
/// to about 0.3 s at the line's end). Deepgram's range is 0.3 to 0.9, under
/// the end's own 0.7.
pub(super) const EAGER_THRESHOLD: &str = "0.5";

/// Whether Flux is asked for its eager turn ends: only with the prepared
/// chat that uses them.
pub(super) fn eager() -> bool {
    std::env::var(crate::second_provider::PREPARED_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on")
}

/// Flux's socket: the Autonomous proxy at `proxy` on its own path, which
/// passes `model=flux-general-en` on to Deepgram's `/v2/listen` (on
/// lamp-4ace on 19 Oct its handshake, `Connected` and `TurnInfo` came
/// back, with the lamp's own key), else Deepgram's. The query is 16 kHz
/// linear audio, and the lamp's name then `key_terms` as `keyterm`s, as
/// Nova-3's.
pub(super) fn url(proxy: Option<&str>, key_terms: &[String], eager: bool) -> Result<reqwest::Url> {
    let mut query = vec![
        ("model", MODEL),
        ("encoding", "linear16"),
        ("sample_rate", "16000"),
    ];
    if eager {
        query.push(("eager_eot_threshold", EAGER_THRESHOLD));
    }
    query.push(("keyterm", crate::second_provider::net::key_terms::NAME));
    query.extend(key_terms.iter().map(|term| ("keyterm", term.as_str())));
    let base = proxy.map_or_else(|| DEEPGRAM.to_owned(), super::proxied);
    super::secure(reqwest::Url::parse_with_params(&base, query).map_err(|_| Error::Url)?)
}

static NOT_USED: Once = Once::new();

/// Whether `config`'s lines go to Flux: its switch on
/// ([`SecondConfig::stt_flux`]), and in English. When the switch is on and
/// Flux is not used, why is said once.
pub(super) fn chosen(config: &SecondConfig) -> bool {
    if !config.stt_flux {
        return false;
    }
    let why_not = why_not(config.stt_language.as_deref());
    if let Some(why) = why_not {
        NOT_USED.call_once(|| {
            eprintln!("lamp-voice second: {FLUX_ENV} is on, but Nova-3 hears the lines: {why}");
        });
    }
    why_not.is_none()
}

/// Why lines in `language` can't go to Flux; none when they can.
fn why_not(language: Option<&str>) -> Option<&'static str> {
    language
        .filter(|language| !english(language))
        .map(|_| "Flux's model hears English only")
}

fn english(language: &str) -> bool {
    let language = language.to_ascii_lowercase();
    language == "en" || language.starts_with("en-")
}

/// A `TurnInfo`'s event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Event {
    StartOfTurn,
    Update,
    EagerEndOfTurn,
    TurnResumed,
    EndOfTurn,
    /// One Deepgram adds later: read as an update.
    Other,
}

/// One `TurnInfo` message.
#[derive(Debug, PartialEq)]
pub(crate) struct TurnInfo {
    pub event: Event,
    pub turn_index: u64,
    /// The turn's words so far: all of them, not since the last message.
    pub transcript: String,
    /// Where in the audio the turn's window ends, in seconds.
    pub audio_window_end: f64,
    /// For an `EndOfTurn`: `model`, `manual` or `timeout`; the set is open.
    pub trigger: Option<String>,
}

/// `None` for any other message (`Connected`, `Warning`, `Error`).
pub(crate) fn turn_info(text: &str) -> Option<TurnInfo> {
    #[derive(Deserialize)]
    struct Raw {
        #[serde(rename = "type")]
        kind: String,
        #[serde(default)]
        event: String,
        #[serde(default)]
        turn_index: u64,
        #[serde(default)]
        transcript: String,
        #[serde(default)]
        audio_window_end: f64,
        #[serde(default)]
        trigger: Option<String>,
    }
    let raw: Raw = serde_json::from_str(text).ok()?;
    if raw.kind != "TurnInfo" {
        return None;
    }
    let event = match raw.event.as_str() {
        "StartOfTurn" => Event::StartOfTurn,
        "Update" => Event::Update,
        "EagerEndOfTurn" => Event::EagerEndOfTurn,
        "TurnResumed" => Event::TurnResumed,
        "EndOfTurn" => Event::EndOfTurn,
        _ => Event::Other,
    };
    Some(TurnInfo {
        event,
        turn_index: raw.turn_index,
        transcript: raw.transcript.trim().to_owned(),
        audio_window_end: raw.audio_window_end,
        trigger: raw.trigger,
    })
}

/// The `Warning` a `ForceEndTurn` gets when no turn is in progress: nothing
/// more is coming for what was sent before it.
pub(crate) fn no_turn_to_end(text: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text).is_ok_and(|message| {
        message["type"] == "Warning" && message["code"] == "FORCE_END_TURN_NO_ACTIVE_TURN"
    })
}

/// A fatal `Error`'s code: the service closes after it.
fn refused(text: &str) -> Option<String> {
    let message = serde_json::from_str::<serde_json::Value>(text).ok()?;
    (message["type"] == "Error").then(|| message["code"].as_str().unwrap_or("none").to_owned())
}

/// Each turn's latest words, and how the latest turn stands.
#[derive(Default)]
pub(crate) struct Turns {
    /// By `turn_index`, each turn's words as last given.
    words: BTreeMap<u64, String>,
    /// The latest turn's index, and its `EndOfTurn`'s trigger once it ended.
    latest: Option<(u64, Option<String>)>,
    /// When Flux's own model last ended a turn, and which.
    model_ended: Option<(Instant, u64)>,
    /// Where in the audio the windows heard so far end, in seconds.
    heard_to: f64,
}

impl Turns {
    pub fn add(&mut self, info: TurnInfo, now: Instant) {
        let ended = (info.event == Event::EndOfTurn)
            .then(|| info.trigger.clone().unwrap_or_else(|| "unknown".to_owned()));
        if ended.as_deref() == Some("model") {
            self.model_ended = Some((now, info.turn_index));
        }
        if info.audio_window_end.is_finite() {
            self.heard_to = self.heard_to.max(info.audio_window_end);
        }
        match &mut self.latest {
            Some((index, trigger)) if *index == info.turn_index => {
                if ended.is_some() {
                    *trigger = ended;
                }
            }
            Some((index, _)) if *index > info.turn_index => {}
            _ => self.latest = Some((info.turn_index, ended)),
        }
        // An `EndOfTurn`'s words are the turn's, even none; an update with
        // none says nothing new.
        if info.event == Event::EndOfTurn || !info.transcript.is_empty() {
            self.words.insert(info.turn_index, info.transcript);
        }
    }

    /// Nothing more is coming for the `sent` seconds of audio: the latest
    /// turn has ended, and Flux's windows cover all of it.
    pub fn settled(&self, sent: f64) -> bool {
        self.latest
            .as_ref()
            .is_some_and(|(_, ended)| ended.is_some())
            && self.heard_to + COVERED >= sent
    }

    /// The journal's account of the end of the line, without words: turns,
    /// how the latest stands, and how far ahead of the end (`now`) Flux's
    /// own model last ended a turn.
    fn at_end(&self, sent: f64, now: Instant) -> String {
        let ms = |seconds: f64| (seconds * 1000.0).round() as i64;
        let latest = match &self.latest {
            None => "no turn".to_owned(),
            Some((_, None)) => "the latest turn open".to_owned(),
            Some((_, Some(trigger))) => format!("the latest turn ended by {trigger}"),
        };
        let model = match self.model_ended {
            None => "the model ended no turn".to_owned(),
            Some((at, index)) => format!(
                "the model ended turn {} {} ms before End",
                index + 1,
                now.saturating_duration_since(at).as_millis()
            ),
        };
        format!(
            "flux: turns={}, {latest}, {model}, settled={}, heard to {} of {} ms sent",
            self.latest.as_ref().map_or(0, |(index, _)| index + 1),
            self.settled(sent),
            ms(self.heard_to),
            ms(sent)
        )
    }

    pub fn text(&self) -> String {
        self.words
            .values()
            .filter(|words| !words.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// The stream's task on Flux: [`super::run`]'s part, in Flux's messages.
pub(super) async fn run<S>(
    socket: tokio_tungstenite::WebSocketStream<S>,
    mut queued: mpsc::Receiver<Outgoing>,
    words_at: Arc<OnceLock<Instant>>,
    (said, early): (mpsc::UnboundedSender<String>, mpsc::UnboundedSender<Early>),
    replies: Arc<Replies>,
) -> Result<(String, Option<String>)>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (mut write, mut read) = socket.split();
    let mut turns = Turns::default();
    let mut chunk: Vec<u8> = Vec::with_capacity(CHUNK_BYTES);
    let mut last_sent = Instant::now();
    let mut samples_sent = 0usize;
    // Every `ForceEndTurn` gets one answer, an `EndOfTurn` it ended or the
    // warning that there was none: a flush's may still be on its way at
    // the end of the line.
    let mut forces = 0usize;
    // Set at the end of the line, when its `ForceEndTurn` went: until when
    // its answer is waited for.
    let mut force_by: Option<Instant> = None;
    let mut force_went: Option<Instant> = None;
    let mut closing = false;
    let mut at_end = String::new();
    let told = |at_end: &str, answered: Option<(Duration, &str)>, from: &str| {
        let answer = match answered {
            Some((after, how)) => {
                format!(", force_end answered in {} ms ({how})", after.as_millis())
            }
            None => ", force_end unanswered".to_owned(),
        };
        Some(format!("{at_end}{answer}, words from={from}"))
    };
    let failed = |_| Error::Transport("speech-to-text send");
    loop {
        let sent = samples_sent as f64 / 16_000.0;
        tokio::select! {
            frame = queued.recv(), if force_by.is_none() && !closing => {
                match frame {
                    Some(Outgoing::Audio(bytes)) => {
                        samples_sent += bytes.len() / 2;
                        chunk.extend_from_slice(&bytes);
                        if chunk.len() >= CHUNK_BYTES {
                            write.send(Message::binary(std::mem::take(&mut chunk))).await.map_err(failed)?;
                            last_sent = Instant::now();
                        }
                        continue;
                    }
                    // Its answer is read as any other turn's words; the
                    // stream goes on.
                    Some(Outgoing::Flush) => {
                        if !chunk.is_empty() {
                            write.send(Message::binary(std::mem::take(&mut chunk))).await.map_err(failed)?;
                        }
                        write.send(Message::text(FORCE_END_TURN)).await.map_err(failed)?;
                        forces += 1;
                        last_sent = Instant::now();
                        continue;
                    }
                    None => {}
                }
                // The end of the line.
                if !chunk.is_empty() {
                    write.send(Message::binary(std::mem::take(&mut chunk))).await.map_err(failed)?;
                }
                at_end = turns.at_end(sent, Instant::now());
                if forces == 0 && turns.settled(sent) {
                    let _ = write.send(Message::text(CLOSE_STREAM)).await;
                    return Ok((turns.text(), Some(format!("{at_end}, words from=settled"))));
                }
                write.send(Message::text(FORCE_END_TURN)).await.map_err(failed)?;
                forces += 1;
                force_went = Some(Instant::now());
                force_by = Some(Instant::now() + FINALIZE);
            }
            _ = tokio::time::sleep_until(last_sent + KEEP_ALIVE), if force_by.is_none() && !closing => {
                // What is waiting goes, or else quiet, which is audio sent.
                let audio = if chunk.is_empty() {
                    samples_sent += CHUNK_BYTES / 2;
                    QUIET.to_vec()
                } else {
                    std::mem::take(&mut chunk)
                };
                write.send(Message::binary(audio)).await.map_err(failed)?;
                last_sent = Instant::now();
            }
            _ = tokio::time::sleep_until(force_by.unwrap_or(last_sent)), if force_by.is_some() && !closing => {
                // `ForceEndTurn` went unanswered: `CloseStream` ends the
                // socket, and each turn's latest words are the answer.
                closing = true;
                write.send(Message::text(CLOSE_STREAM)).await.map_err(failed)?;
            }
            message = read.next() => match message {
                Some(Ok(Message::Text(text))) => {
                    let answer = if let Some(info) = turn_info(&text) {
                        replies.count(if info.transcript.is_empty() {
                            &replies.without_words
                        } else {
                            &replies.with_words
                        });
                        if info.event == Event::StartOfTurn {
                            replies.count(&replies.speech_started);
                        }
                        if !info.transcript.is_empty() {
                            let _ = words_at.set(Instant::now());
                            let _ = said.send(info.transcript.clone());
                        }
                        let forced = info.event == Event::EndOfTurn
                            && info.trigger.as_deref() == Some("manual");
                        let event = info.event;
                        turns.add(info, Instant::now());
                        // The eager end's words, for the chat begun on them,
                        // and its taking back.
                        match event {
                            Event::EagerEndOfTurn => {
                                let _ = early.send(Early::Words(turns.text()));
                            }
                            Event::TurnResumed => {
                                let _ = early.send(Early::Resumed);
                            }
                            _ => {}
                        }
                        forced.then_some("end of turn")
                    } else if no_turn_to_end(&text) {
                        replies.count(&replies.other);
                        Some("no turn")
                    } else {
                        if let Some(code) = refused(&text) {
                            replies.end(format!("was refused by the service, code {code}"));
                        }
                        replies.count(&replies.other);
                        None
                    };
                    if let Some(how) = answer {
                        forces = forces.saturating_sub(1);
                        if let Some(went) = force_went
                            && forces == 0
                            && !closing
                        {
                            // Closes the socket; its close is not waited for.
                            let _ = write.send(Message::text(CLOSE_STREAM)).await;
                            return Ok((turns.text(), told(&at_end, Some((went.elapsed(), how)), "force_end")));
                        }
                    }
                }
                Some(Ok(Message::Close(frame))) => {
                    replies.end(frame.map_or_else(
                        || "was closed by the service, with no code".to_owned(),
                        |frame| {
                            format!(
                                "was closed by the service, code {} {:?}",
                                u16::from(frame.code),
                                frame.reason.as_str()
                            )
                        },
                    ));
                    return if force_by.is_some() {
                        let from = closed_by_service(&mut at_end, &turns, sent, closing, force_went);
                        Ok((turns.text(), told(&at_end, None, from)))
                    } else {
                        Err(Error::Transport("speech-to-text stream"))
                    };
                }
                None => {
                    replies.end("ended with no close".to_owned());
                    return if force_by.is_some() {
                        let from = closed_by_service(&mut at_end, &turns, sent, closing, force_went);
                        Ok((turns.text(), told(&at_end, None, from)))
                    } else {
                        Err(Error::Transport("speech-to-text stream"))
                    };
                }
                Some(Ok(Message::Binary(_))) => replies.count(&replies.other),
                Some(Ok(_)) => {}
                Some(Err(error)) => {
                    replies.end(format!("failed: {error}"));
                    return Err(Error::Transport("speech-to-text stream"));
                }
            }
        }
    }
}

/// Where a line's words came from when the service ended the stream after
/// `ForceEndTurn`: `close_stream` when the lamp had asked it to close;
/// otherwise `closed_early`, with when, how far it heard and how many
/// words, never the words, as the Nova route tells it (`stt.rs`
/// `closed_by_service`; the first builder's agent on #2055).
fn closed_by_service(
    at_end: &mut String,
    turns: &Turns,
    sent: f64,
    closing: bool,
    force_went: Option<Instant>,
) -> &'static str {
    if closing {
        return "close_stream";
    }
    let after = force_went.map_or(0, |went| went.elapsed().as_millis());
    let words = turns.text().split_whitespace().count();
    at_end.push_str(&format!(
        "; closed by the service {after} ms after force_end, before its answer: {}, {words} words",
        turns.at_end(sent, Instant::now())
    ));
    "closed_early"
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_tungstenite::{WebSocketStream, tungstenite::protocol::Role};

    const QUERY: &str = "model=flux-general-en&encoding=linear16&sample_rate=16000\
        &keyterm=Lamp&keyterm=Hanoi&keyterm=Ho+Chi+Minh+City";

    #[test]
    fn the_url_is_fluxs_with_the_lamps_name_then_the_units_words() {
        let terms = ["Hanoi".to_owned(), "Ho Chi Minh City".to_owned()];
        assert_eq!(
            url(None, &terms, false).unwrap().as_str(),
            format!("wss://api.deepgram.com/v2/listen?{QUERY}")
        );
    }

    /// The first builder on lamp-4ace, 15:54: the proxy's own path takes
    /// Flux's model and passes it on; its `/v2/listen` paths are 404s.
    #[test]
    fn through_the_proxy_flux_is_asked_on_its_own_path() {
        let terms = ["Hanoi".to_owned(), "Ho Chi Minh City".to_owned()];
        assert_eq!(
            url(
                Some("https://campaign-api.autonomous.ai/api/v1/ai/"),
                &terms,
                false
            )
            .unwrap()
            .as_str(),
            format!(
                "wss://campaign-api.autonomous.ai/api/v1/ai/v1/ws/audio/transcriptions?{QUERY}"
            )
        );
        assert_eq!(
            url(Some("ftp://example.com"), &[], false).unwrap_err(),
            Error::Url
        );
    }

    /// #1712: with the prepared chat, Flux is asked for its eager turn
    /// ends; without it, not.
    #[test]
    fn eager_turn_ends_are_asked_for_only_with_the_prepared_chat() {
        let eager = url(None, &[], true).unwrap();
        assert!(
            eager
                .query_pairs()
                .any(|(key, value)| key == "eager_eot_threshold" && value == EAGER_THRESHOLD),
            "{eager}"
        );
        let plain = url(None, &[], false).unwrap();
        assert!(
            !plain
                .query_pairs()
                .any(|(key, _)| key == "eager_eot_threshold")
        );
    }

    /// #1712: an eager end's words go to the turn at once as early words,
    /// and `TurnResumed` takes them back; the line's words are still the
    /// end's.
    #[tokio::test(start_paused = true)]
    async fn an_eager_end_gives_its_words_early_and_a_resumed_turn_takes_them_back() {
        const EAGER: &str = r#"{"type":"TurnInfo","event":"EagerEndOfTurn","turn_index":0,"audio_window_end":0.6,"transcript":"What time"}"#;
        const RESUMED: &str = r#"{"type":"TurnInfo","event":"TurnResumed","turn_index":0,"audio_window_end":0.7,"transcript":"What time"}"#;
        const EAGER_AGAIN: &str = r#"{"type":"TurnInfo","event":"EagerEndOfTurn","turn_index":0,"audio_window_end":0.9,"transcript":"What time is it?"}"#;
        let (words, at_end, _, _) = finish_flux(
            &[EAGER, RESUMED, EAGER_AGAIN, FORCED],
            &[],
            Duration::ZERO,
            0,
            0,
        )
        .await;
        assert_eq!(words, "Hello, Lamp.");
        assert!(at_end.contains("words from="), "{at_end}");
        let (near, far) = tokio::io::duplex(1 << 16);
        let server = tokio::spawn(async move {
            let mut socket = WebSocketStream::from_raw_socket(far, Role::Server, None).await;
            if let Some(Ok(Message::Binary(_))) = socket.next().await {
                for message in [EAGER, RESUMED, EAGER_AGAIN] {
                    socket.send(Message::text(message)).await.unwrap();
                }
            }
            std::future::pending::<()>().await;
        });
        let mut stream = super::super::SttStream::start_flux(
            WebSocketStream::from_raw_socket(near, Role::Client, None).await,
        );
        let mut early = stream.take_early().unwrap();
        for _ in 0..4 {
            stream.send(&[0; 320]).unwrap();
        }
        assert_eq!(
            early.recv().await,
            Some(Early::Words("What time".to_owned()))
        );
        assert_eq!(early.recv().await, Some(Early::Resumed));
        assert_eq!(
            early.recv().await,
            Some(Early::Words("What time is it?".to_owned()))
        );
        server.abort();
    }

    #[test]
    fn flux_hears_lines_in_english_only() {
        for language in [None, Some("en"), Some("en-US"), Some("EN-gb")] {
            assert_eq!(why_not(language), None, "{language:?}");
        }
        assert!(why_not(Some("vi")).is_some_and(|why| why.contains("English")));
        assert!(why_not(Some("english")).is_some());
    }

    fn info(event: &str, turn: u64, words: &str, end: f64, trigger: Option<&str>) -> TurnInfo {
        let trigger = trigger.map_or(String::new(), |trigger| {
            format!(r#","trigger":"{trigger}""#)
        });
        turn_info(&format!(
            r#"{{"type":"TurnInfo","request_id":"x","sequence_id":1,"event":"{event}","turn_index":{turn},"audio_window_start":0.0,"audio_window_end":{end},"transcript":"{words}","words":[],"end_of_turn_confidence":0.9{trigger}}}"#
        ))
        .expect("a TurnInfo")
    }

    #[test]
    fn turn_info_is_read_and_other_messages_are_not() {
        assert_eq!(
            info("EndOfTurn", 3, " Set a timer. ", 2.5, Some("manual")),
            TurnInfo {
                event: Event::EndOfTurn,
                turn_index: 3,
                transcript: "Set a timer.".to_owned(),
                audio_window_end: 2.5,
                trigger: Some("manual".to_owned()),
            }
        );
        assert_eq!(info("Shrug", 0, "", 0.0, None).event, Event::Other);
        for other in [
            r#"{"type":"Connected","request_id":"x","sequence_id":0}"#,
            r#"{"type":"Results","is_final":true}"#,
            "not json",
        ] {
            assert_eq!(turn_info(other), None, "{other}");
        }
        assert!(no_turn_to_end(
            r#"{"type":"Warning","code":"FORCE_END_TURN_NO_ACTIVE_TURN","description":"no turn"}"#
        ));
        assert!(!no_turn_to_end(r#"{"type":"Warning","code":"OTHER"}"#));
        assert_eq!(
            refused(r#"{"type":"Error","code":"INVALID_ENCODING","description":"x"}"#).as_deref(),
            Some("INVALID_ENCODING")
        );
    }

    #[test]
    fn each_turns_latest_words_are_kept_in_order_and_settle_when_the_turn_has_ended_over_all_the_audio()
     {
        let now = Instant::now();
        let mut turns = Turns::default();
        assert!(!turns.settled(0.0), "no turn yet");
        turns.add(info("StartOfTurn", 0, "", 0.2, None), now);
        turns.add(info("Update", 0, "set a", 0.6, None), now);
        turns.add(info("Update", 0, "", 0.7, None), now);
        assert_eq!(
            turns.text(),
            "set a",
            "an update with no words says nothing new"
        );
        turns.add(
            info("EndOfTurn", 0, "Set a timer.", 1.0, Some("model")),
            now,
        );
        assert!(turns.settled(1.0));
        assert!(!turns.settled(1.2), "0.2 s not covered");
        turns.add(info("StartOfTurn", 1, "", 1.3, None), now);
        turns.add(info("Update", 1, "for twenty", 1.8, None), now);
        assert!(!turns.settled(1.8), "the second turn is open");
        assert_eq!(turns.text(), "Set a timer. for twenty");
        // A late message for the first turn changes neither the latest.
        turns.add(info("Update", 0, "Set a timer.", 1.0, None), now);
        assert!(!turns.settled(1.8));
        turns.add(
            info("EndOfTurn", 1, "for twenty minutes.", 2.2, Some("manual")),
            now,
        );
        assert!(turns.settled(2.2));
        assert_eq!(turns.text(), "Set a timer. for twenty minutes.");
    }

    #[tokio::test(start_paused = true)]
    async fn the_end_of_the_line_is_told_in_ms_without_words() {
        let mut turns = Turns::default();
        let began = Instant::now();
        assert_eq!(
            turns.at_end(1.5, began),
            "flux: turns=0, no turn, the model ended no turn, settled=false, heard to 0 of 1500 ms sent"
        );
        turns.add(
            info("EndOfTurn", 0, "secret words", 1.23, Some("model")),
            began,
        );
        turns.add(info("StartOfTurn", 1, "", 1.3, None), began);
        tokio::time::sleep(Duration::from_millis(340)).await;
        let told = turns.at_end(1.5, Instant::now());
        assert_eq!(
            told,
            "flux: turns=2, the latest turn open, the model ended turn 1 340 ms before End, \
             settled=false, heard to 1300 of 1500 ms sent"
        );
        assert!(!told.contains("secret"));
    }

    /// Flux in memory, on the paused clock: the server reads audio, sends
    /// `before` once some arrived, answers each `ForceEndTurn` in turn with
    /// one of `forced` 100 ms later (none left: ignores it), and closes
    /// `round_trip` after `CloseStream`. The client sends one second of
    /// audio in 20 ms frames, waits for its words, flushes `flushes` times
    /// and finishes. Returns the words, the journal's account, how long
    /// `finish` took, and the size of each audio message the server read.
    async fn finish_flux(
        before: &'static [&'static str],
        forced: &'static [&'static str],
        round_trip: Duration,
        flushes: usize,
        pauses: usize,
    ) -> (String, String, Duration, Vec<usize>) {
        let (near, far) = tokio::io::duplex(1 << 16);
        let server = tokio::spawn(async move {
            let mut socket = WebSocketStream::from_raw_socket(far, Role::Server, None).await;
            let mut audio = Vec::new();
            let mut forced = forced.iter();
            while let Some(Ok(message)) = socket.next().await {
                match message {
                    Message::Binary(bytes) => {
                        if audio.is_empty() {
                            for message in before {
                                socket.send(Message::text(*message)).await.unwrap();
                            }
                        }
                        audio.push(bytes.len());
                    }
                    Message::Text(text) if text.contains("ForceEndTurn") => {
                        if let Some(answer) = forced.next() {
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            socket.send(Message::text(*answer)).await.unwrap();
                        }
                    }
                    Message::Text(text) if text.contains("CloseStream") => break,
                    _ => {}
                }
            }
            tokio::time::sleep(round_trip).await;
            let _ = socket.close(None).await;
            audio
        });
        let stream = super::super::SttStream::start_flux(
            WebSocketStream::from_raw_socket(near, Role::Client, None).await,
        );
        for _ in 0..50 {
            stream.send(&[0; 320]).unwrap();
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
        for _ in 0..flushes {
            stream.flush().unwrap();
        }
        for _ in 0..pauses {
            stream.paused().unwrap();
        }
        let began = Instant::now();
        let (words, at_end) = stream.finish_told(true).await.unwrap();
        let took = began.elapsed();
        let audio = server.await.unwrap();
        (words, at_end.unwrap(), took, audio)
    }

    const HELLO: &str = r#"{"type":"TurnInfo","event":"Update","turn_index":0,"audio_window_end":0.6,"transcript":"hello"}"#;
    const ENDED: &str = r#"{"type":"TurnInfo","event":"EndOfTurn","turn_index":0,"audio_window_end":1.0,"transcript":"Hello, Lamp.","trigger":"model"}"#;
    const FORCED: &str = r#"{"type":"TurnInfo","event":"EndOfTurn","turn_index":0,"audio_window_end":1.0,"transcript":"Hello, Lamp.","trigger":"manual"}"#;
    const NO_TURN: &str = r#"{"type":"Warning","code":"FORCE_END_TURN_NO_ACTIVE_TURN","description":"no active turn"}"#;

    #[tokio::test(start_paused = true)]
    async fn a_turn_ended_over_all_the_audio_sent_is_the_answer_at_once_in_80_ms_messages() {
        let (words, at_end, took, audio) =
            finish_flux(&[HELLO, ENDED], &[], Duration::from_millis(800), 0, 0).await;
        assert_eq!(words, "Hello, Lamp.");
        assert_eq!(took, Duration::ZERO);
        assert!(at_end.starts_with("flux: turns=1, the latest turn ended by model, the model ended turn 1 300 ms before End, settled=true"), "{at_end}");
        assert!(at_end.ends_with(", words from=settled"), "{at_end}");
        // Fifty 20 ms frames: twelve of 80 ms, and the last 40 ms at the end.
        assert_eq!(audio.len(), 13);
        assert!(audio[..12].iter().all(|&bytes| bytes == CHUNK_BYTES));
        assert_eq!(audio[12], 1_280);
    }

    /// Jarvis on #2102: a pause (`LAMP_SECOND_EARLY_FINALIZE`) asks nothing
    /// of Flux, whose flush would end its turn (`ForceEndTurn`): three of
    /// them leave the turn the model ended settled, as with none.
    #[tokio::test(start_paused = true)]
    async fn a_pause_asks_nothing_of_flux() {
        let (words, at_end, took, _) =
            finish_flux(&[HELLO, ENDED], &[], Duration::from_millis(800), 0, 3).await;
        assert_eq!(words, "Hello, Lamp.");
        assert_eq!(took, Duration::ZERO);
        assert!(at_end.ends_with(", words from=settled"), "{at_end}");
    }

    #[tokio::test(start_paused = true)]
    async fn an_open_turn_is_ended_by_force_end_turn_without_waiting_for_the_close() {
        let (words, at_end, took, _) =
            finish_flux(&[HELLO], &[FORCED], Duration::from_millis(800), 0, 0).await;
        assert_eq!(words, "Hello, Lamp.");
        assert_eq!(took, Duration::from_millis(100));
        assert!(
            at_end.ends_with(
                "the latest turn open, the model ended no turn, settled=false, heard to 600 of \
                 1000 ms sent, force_end answered in 100 ms (end of turn), words from=force_end"
            ),
            "{at_end}"
        );
        assert!(!at_end.contains("ello"));
    }

    /// The model ended the turn, but its window is short of the audio
    /// sent: `ForceEndTurn` is asked, and its warning that no turn is open
    /// says nothing more is coming.
    #[tokio::test(start_paused = true)]
    async fn no_turn_to_end_answers_force_end_turn() {
        const SHORT: &str = r#"{"type":"TurnInfo","event":"EndOfTurn","turn_index":0,"audio_window_end":0.7,"transcript":"Hello, Lamp.","trigger":"model"}"#;
        let (words, at_end, took, _) =
            finish_flux(&[SHORT], &[NO_TURN], Duration::from_millis(800), 0, 0).await;
        assert_eq!(words, "Hello, Lamp.");
        assert_eq!(took, Duration::from_millis(100));
        assert!(
            at_end.ends_with("force_end answered in 100 ms (no turn), words from=force_end"),
            "{at_end}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn force_end_turn_unanswered_falls_back_to_close_stream_and_the_latest_words() {
        let (words, at_end, took, _) =
            finish_flux(&[HELLO], &[], Duration::from_millis(800), 0, 0).await;
        assert_eq!(words, "hello");
        assert_eq!(took, FINALIZE + Duration::from_millis(800));
        assert!(
            at_end.ends_with(", force_end unanswered, words from=close_stream"),
            "{at_end}"
        );
    }

    /// The first builder's agent on #2055: Flux closing the stream just
    /// after `ForceEndTurn`, before its answer and with no `CloseStream`
    /// asked, keeps the words so far, told as closed early with a count,
    /// never the words, as on the Nova route.
    #[tokio::test(start_paused = true)]
    async fn a_close_before_force_end_turns_answer_is_told_as_closed_early() {
        let (near, far) = tokio::io::duplex(1 << 16);
        let server = tokio::spawn(async move {
            let mut socket = WebSocketStream::from_raw_socket(far, Role::Server, None).await;
            let mut told = false;
            while let Some(Ok(message)) = socket.next().await {
                match message {
                    Message::Binary(_) if !told => {
                        told = true;
                        socket.send(Message::text(HELLO)).await.unwrap();
                    }
                    Message::Text(text) if text.contains("ForceEndTurn") => {
                        tokio::time::sleep(Duration::from_millis(40)).await;
                        let _ = socket.close(None).await;
                        break;
                    }
                    _ => {}
                }
            }
        });
        let stream = super::super::SttStream::start_flux(
            WebSocketStream::from_raw_socket(near, Role::Client, None).await,
        );
        for _ in 0..50 {
            stream.send(&[0; 320]).unwrap();
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
        let (words, at_end) = stream.finish_told(true).await.unwrap();
        server.abort();
        let at_end = at_end.unwrap();
        assert_eq!(words, "hello");
        assert!(
            at_end.ends_with(", force_end unanswered, words from=closed_early"),
            "{at_end}"
        );
        assert!(
            at_end.contains("; closed by the service 40 ms after force_end, before its answer: flux: turns=1, the latest turn open"),
            "{at_end}"
        );
        assert!(
            at_end.contains("heard to 600 of 1000 ms sent, 1 words"),
            "{at_end}"
        );
        assert!(!at_end.contains("ello"), "{at_end}");
    }

    /// A flush's `ForceEndTurn` still unanswered at the end of the line:
    /// its answer is not taken for the line's.
    #[tokio::test(start_paused = true)]
    async fn a_flushs_late_answer_is_not_the_lines() {
        let (words, at_end, took, _) = finish_flux(
            &[HELLO],
            &[FORCED, NO_TURN],
            Duration::from_millis(800),
            1,
            0,
        )
        .await;
        assert_eq!(words, "Hello, Lamp.");
        assert_eq!(took, Duration::from_millis(200));
        assert!(
            at_end.ends_with("force_end answered in 200 ms (no turn), words from=force_end"),
            "{at_end}"
        );
    }

    /// For stop words (#1047): a flush ends the turn with what was heard,
    /// and the stream goes on; each turn's words come as they are given.
    #[tokio::test(start_paused = true)]
    async fn a_flush_ends_the_turn_and_the_stream_goes_on() {
        const STOP: &str = r#"{"type":"TurnInfo","event":"EndOfTurn","turn_index":0,"audio_window_end":0.08,"transcript":"Stop.","trigger":"manual"}"#;
        let (near, far) = tokio::io::duplex(1 << 16);
        let server = tokio::spawn(async move {
            let mut socket = WebSocketStream::from_raw_socket(far, Role::Server, None).await;
            let mut seen = Vec::new();
            while let Some(Ok(message)) = socket.next().await {
                match message {
                    Message::Binary(_) => seen.push("audio"),
                    Message::Text(text) if text.contains("ForceEndTurn") => {
                        seen.push("force_end_turn");
                        socket.send(Message::text(STOP)).await.unwrap();
                    }
                    _ => {}
                }
                if seen.len() == 3 {
                    break;
                }
            }
            seen
        });
        let mut stream = super::super::SttStream::start_flux(
            WebSocketStream::from_raw_socket(near, Role::Client, None).await,
        );
        let mut words = stream.take_words().unwrap();
        stream.send(&[0; 320]).unwrap();
        stream.flush().unwrap();
        for _ in 0..4 {
            stream.send(&[0; 320]).unwrap();
        }
        assert_eq!(words.recv().await.as_deref(), Some("Stop."));
        assert_eq!(server.await.unwrap(), ["audio", "force_end_turn", "audio"]);
    }

    /// A local plain websocket server stands in for Deepgram, reached as
    /// `connect` reaches Flux: signed with the key, audio out, the turn
    /// ended by `ForceEndTurn` and its words back.
    // The large error is tungstenite's own handshake callback signature.
    #[allow(clippy::result_large_err)]
    #[tokio::test]
    async fn audio_goes_out_on_a_signed_socket_and_the_turns_words_come_back() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_hdr_async(
                stream,
                |request: &tokio_tungstenite::tungstenite::handshake::server::Request, response| {
                    assert_eq!(request.headers()["authorization"], "Token secret");
                    assert_eq!(request.uri().path(), "/v2/listen");
                    Ok(response)
                },
            )
            .await
            .unwrap();
            let mut bytes = 0;
            while let Some(Ok(message)) = socket.next().await {
                match message {
                    Message::Binary(audio) => bytes += audio.len(),
                    Message::Text(text) if text.contains("ForceEndTurn") => {
                        socket.send(Message::text(FORCED)).await.unwrap();
                    }
                    Message::Text(text) if text.contains("CloseStream") => break,
                    _ => {}
                }
            }
            bytes
        });
        let url = reqwest::Url::parse(&format!("ws://{address}/v2/listen?model={MODEL}")).unwrap();
        let stream =
            super::super::SttStream::start_flux(super::super::socket(url, "secret").await.unwrap());
        stream.send(&[1, -1, 3]).unwrap();
        stream.send(&[7]).unwrap();
        assert_eq!(stream.finish(1).await.unwrap(), "Hello, Lamp.");
        assert_eq!(server.await.unwrap(), 8);
    }

    /// A socket idle for [`KEEP_ALIVE`] is kept with 80 ms of quiet, which
    /// counts as audio sent.
    #[tokio::test(start_paused = true)]
    async fn an_idle_socket_is_kept_with_quiet() {
        let (near, far) = tokio::io::duplex(1 << 16);
        let server = tokio::spawn(async move {
            let mut socket = WebSocketStream::from_raw_socket(far, Role::Server, None).await;
            match socket.next().await {
                Some(Ok(Message::Binary(bytes))) => bytes.to_vec(),
                other => panic!("{other:?}"),
            }
        });
        let stream = super::super::SttStream::start_flux(
            WebSocketStream::from_raw_socket(near, Role::Client, None).await,
        );
        tokio::time::sleep(KEEP_ALIVE + Duration::from_millis(10)).await;
        assert_eq!(server.await.unwrap(), QUIET.to_vec());
        drop(stream);
    }
}
