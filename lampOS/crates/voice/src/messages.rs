//! Texting the lamp from the Autonomous app, the voice's side (#1673,
//! `docs/messages.md`): each text on `chat/in` is answered in text on
//! `chat/out`, on the second route's text model, beside the spoken
//! conversation and never in it: the room's Live session and its speaker
//! are not touched. The same character, notes and memory as a spoken turn
//! (the second route's system message), and the same functions, with a
//! note that the conversation is typed. Each chat (the app's `session_id`)
//! keeps its own few exchanges. News of work handed over that the person
//! hasn't been told is given with the answer to their next text, once
//! ([`Untold`]). Off unless `LAMP_MESSAGES=on`, and only where the second
//! route's worker runs, which needs its config (`LAMP_VOICE_SECOND_CONFIG`).
use crate::second_provider::net::{Chat, ChatEvent, Message, ToolCall};
use crate::second_provider::{System, ToolRunner};
use futures_util::future::BoxFuture;
use lamp_msgs::{ChatIn, ChatOut, ChatPart};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Where the coordinator reads the switch.
pub const ENVIRONMENT: &str = "LAMP_MESSAGES";
/// The provider workers' flag for it, after `--misheard-hum` and
/// `--holding-answer`.
pub const FLAG: &str = "--messages";
/// What the model is told beside the lamp's own system message.
pub const NOTE: &str = "(For you to know, not to answer: this conversation is typed, in the Autonomous app on the person's phone, not spoken at the lamp. They may be in another room or away. Answer in text, briefly and as yourself; you can't see or hear them now, and nothing you write is said aloud.)";
/// Given with a text's answer when news waits: the notes after it were
/// written for a turn the lamp begins aloud ([`typed`]).
pub const NEWS: &str = "(News you have not told the person yet, from work you handed over for them. Answer what they wrote first, then tell them the news in the same reply, in a sentence or two of plain text. The notes below were written for a turn you begin aloud; here it is typed.)";

/// The news of work handed over that the person hasn't been told, for the
/// answers to texts (#1673; `task_news::ForTexts` on the lamp).
pub trait Untold: Send + Sync {
    /// The news waiting now. Never waits.
    fn untold(&self) -> Vec<crate::task_news::News>;
    /// The news `keys`, given in a whole reply, so it is not given or told
    /// again.
    fn given(&self, keys: &[String]);
}

impl Untold for crate::task_news::ForTexts {
    fn untold(&self) -> Vec<crate::task_news::News> {
        crate::task_news::ForTexts::untold(self)
    }
    fn given(&self, keys: &[String]) {
        crate::task_news::ForTexts::given(self, keys);
    }
}

/// No tasks part: no news.
pub struct NoNews;

impl Untold for NoNews {
    fn untold(&self) -> Vec<crate::task_news::News> {
        Vec::new()
    }
    fn given(&self, _: &[String]) {}
}

/// A note written for a turn the lamp begins aloud, as a typed answer is
/// given it: without its "Nobody has spoken." opening, which a text just
/// written would make untrue.
pub fn typed(note: &str) -> String {
    note.strip_prefix("(Nobody has spoken. ")
        .map(|rest| format!("({rest}"))
        .unwrap_or_else(|| note.to_owned())
}

/// How long news of work asked for by text waits untold before it goes to
/// the phone unasked (#1673, the first builder, 20 Oct 21:14). In that time
/// the voice's own reader tells it aloud if someone is at the lamp, and news
/// told aloud is kept as told, so it is never sent too.
pub const UNASKED_AFTER: Duration = Duration::from_secs(60);
/// What the model is told for news no text asked about.
pub const UNASKED: &str = "(News you have not told the person yet, from work you handed over for them. Nobody wrote just now, and nobody at the lamp heard it: tell them the news in a sentence or two of plain text, for their phone. Don't ask them anything else. The notes below were written for a turn you begin aloud; here it is typed.)";
/// The tries one piece of unasked news gets, a minute apart, before it
/// waits for the person's next text instead.
pub const UNASKED_TRIES: u8 = 3;
/// How often the news waiting is looked at for the phone.
pub const UNASKED_LOOK_EVERY: Duration = Duration::from_secs(1);

/// The news waiting for the phone: when each was first seen, and its tries.
pub struct Unasked {
    seen: HashMap<String, (Instant, u8)>,
    /// [`UNASKED_AFTER`], but for the tests.
    after: Duration,
}

impl Default for Unasked {
    fn default() -> Self {
        Self::after(UNASKED_AFTER)
    }
}

impl Unasked {
    /// News is due once it has waited `after`.
    pub fn after(after: Duration) -> Self {
        Self {
            seen: HashMap::new(),
            after,
        }
    }

    /// The news in `waiting` due at `now`: untold for [`UNASKED_AFTER`]
    /// since it was first seen, or since its last try, and not out of tries.
    fn due(
        &mut self,
        waiting: &[crate::task_news::News],
        now: Instant,
    ) -> Vec<crate::task_news::News> {
        self.seen
            .retain(|key, _| waiting.iter().any(|news| &news.key == key));
        for news in waiting {
            self.seen.entry(news.key.clone()).or_insert((now, 0));
        }
        waiting
            .iter()
            .filter(|news| {
                self.seen.get(&news.key).is_some_and(|(at, tries)| {
                    *tries < UNASKED_TRIES && now.saturating_duration_since(*at) >= self.after
                })
            })
            .cloned()
            .collect()
    }
}

/// News of work asked for by text that nobody was told, sent to the phone
/// unasked: one `task_news` piece with no run, whose text is only the
/// model's words for it, cut to one message ([`fitted`]). The model is given
/// no functions' results and none are run: nobody asked for anything. Once
/// sent, the news is kept as told, so it is sent once and never told aloud
/// after. A fault, or no words, sends nothing, and it is tried again a
/// minute on, at most [`UNASKED_TRIES`] times. Nothing is tried while
/// nothing takes `chat/out` (`listened`: `lamp-cloud` stopped), so news
/// isn't kept as told that no phone could get (the reviewer on #2311), and
/// nothing is sent if any of it was told aloud during the round. `say`
/// answers whether the piece reached `lamp-cloud`: one that didn't is not
/// kept as told, and counts as a try (the first builder, 20 Oct 04:08).
pub async fn tell_unasked(
    model: &impl Model,
    system: &System,
    news: &dyn Untold,
    unasked: &mut Unasked,
    now: Instant,
    listened: bool,
    mut say: impl FnMut(ChatOut) -> bool + Send,
) {
    let due = unasked.due(&news.untold(), now);
    if due.is_empty() || !listened {
        return;
    }
    let told = system();
    let mut note = UNASKED.to_owned();
    for item in &due {
        note.push('\n');
        note.push_str(&typed(&item.note));
    }
    let messages = vec![
        Message::system(format!("{}\n\n{NOTE}", told.text)),
        Message::user(note),
    ];
    let mut said = String::new();
    let result = model
        .round(&messages, &mut |words: &str| said.push_str(words))
        .await;
    let text = fitted(said.trim());
    if let Err(error) = &result {
        eprintln!("lamp-voice second: news for the phone wasn't sent: {error}");
    }
    let tried = |unasked: &mut Unasked| {
        for item in &due {
            if let Some((at, tries)) = unasked.seen.get_mut(&item.key) {
                *at = now;
                *tries += 1;
            }
        }
    };
    if result.is_err() || text.is_empty() {
        tried(unasked);
        return;
    }
    // Told aloud, or given with a text, while the model wrote: not twice.
    let still = news.untold();
    if !due
        .iter()
        .all(|item| still.iter().any(|news| news.key == item.key))
    {
        return;
    }
    let delivered = say(ChatOut {
        run_id: None,
        session_id: None,
        part: ChatPart::TaskNews,
        text,
    });
    if !delivered {
        eprintln!("lamp-voice second: news for the phone reached nothing; it is tried again");
        tried(unasked);
        return;
    }
    let keys: Vec<String> = due.into_iter().map(|item| item.key).collect();
    news.given(&keys);
    for key in &keys {
        unasked.seen.remove(key);
    }
}

/// What the person reads when the model could not be reached.
pub const NOT_NOW: &str = "Sorry, I can't answer just now. Please try again in a moment.";
/// The most exchanges a chat keeps, as user and assistant messages.
pub const KEPT: usize = 12;
/// A chat quiet this long starts again with nothing kept.
pub const FORGET_AFTER: Duration = Duration::from_secs(30 * 60);
/// The most rounds of function calls one text may make.
pub const ROUNDS: usize = 4;
/// Texts' request numbers, apart from the spoken ones in the same process.
pub const REQUESTS_FROM: u64 = 1 << 48;
/// Each chat's texts are numbered in a block of their own, this many bits
/// long, so a "yes" typed in one chat confirms only a task asked for in it
/// (`agent_tasks::may_confirm`, [`same_chat`]).
pub const CHAT_BITS: u32 = 24;

/// Whether `request` is a text's, not a spoken one.
pub fn is_text(request: u64) -> bool {
    request >= REQUESTS_FROM
}

/// Whether requests `a` and `b` are both texts of the same chat.
pub fn same_chat(a: u64, b: u64) -> bool {
    a >= REQUESTS_FROM
        && b >= REQUESTS_FROM
        && (a - REQUESTS_FROM) >> CHAT_BITS == (b - REQUESTS_FROM) >> CHAT_BITS
}

/// Whether the coordinator's environment turns texts on.
pub fn on_from_environment() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// The worker's flags for the switch, from the coordinator's environment.
pub fn arguments_from_environment() -> Vec<String> {
    arguments(std::env::var(ENVIRONMENT).ok().as_deref())
}

fn arguments(value: Option<&str>) -> Vec<String> {
    if value == Some("on") {
        vec![FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

/// Take a leading [`FLAG`] off a worker's flags.
pub fn take(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// One round of a text's answer: the model's words, each passed to `text`
/// as it comes, and the calls it asked for, if any.
pub trait Model {
    fn round<'a>(
        &'a self,
        messages: &'a [Message],
        text: &'a mut (dyn FnMut(&str) + Send),
    ) -> BoxFuture<'a, Result<Vec<ToolCall>, String>>;
}

impl Model for Chat {
    fn round<'a>(
        &'a self,
        messages: &'a [Message],
        text: &'a mut (dyn FnMut(&str) + Send),
    ) -> BoxFuture<'a, Result<Vec<ToolCall>, String>> {
        Box::pin(async move {
            let mut stream = self
                .send(messages)
                .await
                .map_err(|error| error.to_string())?;
            let mut calls = Vec::new();
            while let Some(event) = stream.next().await.map_err(|error| error.to_string())? {
                match event {
                    ChatEvent::Text(piece) => text(&piece),
                    ChatEvent::ToolCalls(asked) => calls = asked,
                    // Texts are asked of the configured chat, which neither
                    // reasons aloud nor searches on its own (#1712's
                    // OpenRouter and Gemini clients do, for spoken turns).
                    ChatEvent::Reasoning(_) | ChatEvent::Searched => {}
                    ChatEvent::Done => break,
                }
            }
            Ok(calls)
        })
    }
}

/// The chats' kept exchanges, by the app's session, and each chat's block
/// of request numbers and how many of its texts there have been.
#[derive(Debug, Default)]
pub struct Chats {
    kept: HashMap<String, (Vec<Message>, Instant)>,
    numbers: HashMap<String, (u64, u64)>,
}

impl Chats {
    /// The next text of `session`'s request number, in that chat's block.
    pub fn next_request(&mut self, session: &str) -> u64 {
        let next = self.numbers.len() as u64;
        let (chat, texts) = self.numbers.entry(session.to_owned()).or_insert((next, 0));
        *texts += 1;
        REQUESTS_FROM + (*chat << CHAT_BITS) + *texts
    }

    /// What `session` keeps, as of `now`: nothing once it has been quiet for
    /// [`FORGET_AFTER`].
    pub fn kept(&mut self, session: &str, now: Instant) -> Vec<Message> {
        match self.kept.get(session) {
            Some((kept, at)) if now.duration_since(*at) < FORGET_AFTER => kept.clone(),
            Some(_) => {
                self.kept.remove(session);
                Vec::new()
            }
            None => Vec::new(),
        }
    }

    /// `session`'s person wrote `asked` and was answered `answer`.
    pub fn remember(&mut self, session: &str, asked: &str, answer: &str, now: Instant) {
        let mut kept = self.kept(session, now);
        kept.push(Message::user(asked));
        kept.push(Message::assistant(answer));
        let over = kept.len().saturating_sub(KEPT);
        kept.drain(..over);
        self.kept.insert(session.to_owned(), (kept, now));
    }
}

/// How often the texts on `chat/in` are looked for.
pub const LOOK_EVERY: Duration = Duration::from_millis(50);

/// The second route's worker answering texts until it ends: each text on
/// `chat/in` in turn, its pieces on `chat/out`. A bus that can't be used is
/// said once, and no text is taken: `lamp-cloud` then refuses them.
pub async fn serve(
    model: impl Model + Send + Sync + 'static,
    system: System,
    tools: ToolRunner,
    news: Box<dyn Untold>,
    bus_root: std::path::PathBuf,
) {
    serve_with(model, system, tools, news, bus_root, Unasked::default()).await;
}

/// [`serve`], with the news for the phone due after `unasked`'s wait.
pub(crate) async fn serve_with(
    model: impl Model + Send + Sync + 'static,
    system: System,
    tools: ToolRunner,
    news: Box<dyn Untold>,
    bus_root: std::path::PathBuf,
    mut unasked: Unasked,
) {
    // Each timed on the second route's thread with `LAMP_STALLS=on` (#4332).
    let polls = crate::stalls::Polls::current();
    let bus = polls.time("texts.bus", || lamp_bus::Bus::new(&bus_root));
    let ends = bus.and_then(|bus| {
        Ok((
            polls.time("texts.subscribe", || {
                bus.subscribe_typed(ChatIn::TOPIC, "texts", ChatIn::TYPE)
            })?,
            polls.time("texts.publisher", || {
                bus.publisher_declared(ChatOut::TOPIC, &ChatOut::declaration(), "voice")
            })?,
            bus,
        ))
    });
    let (mut input, mut output, bus) = match ends {
        Ok(ends) => ends,
        Err(error) => {
            eprintln!("lamp-voice second: texts are not taken: the bus can't be used ({error})");
            return;
        }
    };
    eprintln!(
        "lamp-voice second: texts from the Autonomous app are answered here (LAMP_MESSAGES=on)"
    );
    let mut chats = Chats::default();
    let mut unasked_looked = Instant::now();
    // A text that came while the news for the phone was being written.
    let mut pending = None;
    let mut look = tokio::time::interval(LOOK_EVERY);
    look.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        look.tick().await;
        while let Some(message) = pending
            .take()
            .or_else(|| input.recv(Duration::ZERO).ok().flatten())
        {
            let Ok(text) = message.parse::<ChatIn>() else {
                continue;
            };
            if text.check().is_err() {
                continue;
            }
            let request = chats.next_request(text.session_id.as_deref().unwrap_or_default());
            answer(
                &model,
                &system,
                &tools,
                news.as_ref(),
                &mut chats,
                &text,
                request,
                |piece| {
                    let _ = output.publish_json(lamp_ipc::monotonic_us(), &piece);
                },
            )
            .await;
        }
        // Between texts, the news nobody was told goes to the phone. A text
        // that comes while it is written ends that round, untried, and is
        // answered at once, with the news (the reviewer on #2311): a text
        // never waits on it.
        if unasked_looked.elapsed() >= UNASKED_LOOK_EVERY {
            unasked_looked = Instant::now();
            let listened = bus
                .subscribers(ChatOut::TOPIC)
                .is_ok_and(|listeners| listeners.iter().any(|listener| listener.listening));
            let round = tell_unasked(
                &model,
                &system,
                news.as_ref(),
                &mut unasked,
                Instant::now(),
                listened,
                |piece| {
                    output
                        .publish_json(lamp_ipc::monotonic_us(), &piece)
                        .is_ok_and(|published| published.delivered > 0)
                },
            );
            tokio::pin!(round);
            loop {
                tokio::select! {
                    () = &mut round => break,
                    () = tokio::time::sleep(LOOK_EVERY) => {
                        if let Ok(Some(message)) = input.recv(Duration::ZERO) {
                            pending = Some(message);
                            break;
                        }
                    }
                }
            }
        }
    }
}

/// The answer to `text`, numbered `request` for the functions: its pieces
/// passed to `say` as they come (words, then the whole reply, or a fault).
/// The news waiting in `news` is given with it, and kept as given once the
/// whole reply has gone, never after a fault.
#[allow(clippy::too_many_arguments)]
pub async fn answer(
    model: &impl Model,
    system: &System,
    tools: &ToolRunner,
    news: &dyn Untold,
    chats: &mut Chats,
    text: &ChatIn,
    request: u64,
    mut say: impl FnMut(ChatOut) + Send,
) {
    let session = text.session_id.clone().unwrap_or_default();
    let piece = |part, words: &str| ChatOut {
        run_id: Some(text.run_id.clone()),
        session_id: text.session_id.clone(),
        part,
        text: words.to_owned(),
    };
    let told = system();
    let waiting = news.untold();
    let mut note = format!("{}\n\n{NOTE}", told.text);
    if !waiting.is_empty() {
        note.push_str("\n\n");
        note.push_str(NEWS);
        for item in &waiting {
            note.push('\n');
            note.push_str(&typed(&item.note));
        }
    }
    let mut messages = vec![Message::system(note)];
    messages.extend(chats.kept(&session, Instant::now()));
    messages.push(Message::user(&text.text));
    let mut reply = String::new();
    // Words go to the app only while they fit in one message, as the whole
    // reply is cut to ([`fitted`]): past that, a `final` `lamp-cloud` can't
    // carry would leave the app with words and no end (the reviewer on
    // #1713).
    let mut sent = 0;
    let mut full = false;
    for round in 0..ROUNDS {
        let mut said = String::new();
        let calls = {
            let mut text = |words: &str| {
                said.push_str(words);
                if full || sent + words.len() > lamp_msgs::CHAT_TEXT_BYTES {
                    full = true;
                    return;
                }
                sent += words.len();
                say(piece(ChatPart::Delta, words));
            };
            model.round(&messages, &mut text).await
        };
        reply.push_str(&said);
        let calls = match calls {
            Ok(calls) => calls,
            Err(error) => {
                eprintln!("lamp-voice second: a text got no answer: {error}");
                say(piece(ChatPart::Error, NOT_NOW));
                return;
            }
        };
        if calls.is_empty() || round + 1 == ROUNDS {
            break;
        }
        messages.push(Message::assistant_calls(Some(said), &calls));
        for call in calls {
            // Typed, not heard: a task asked for here is started only by a
            // "yes" typed in the same chat (`agent_tasks::may_confirm`).
            // A forget runs only when the typed text asks for it, as a
            // spoken turn's words must (the reviewer's P3 on #2317).
            let result = if crate::remember::is_forget(&call.name)
                && !crate::remember::forget_unguarded()
                && !crate::remember::asks_to_forget(&text.text)
            {
                serde_json::json!({"error": crate::remember::NOT_ASKED_TO_FORGET})
            } else {
                // A forget the text names a thing for is that thing's
                // (#3113), never the last kept thing's.
                let args = crate::remember::named_forget_typed(&call.name, &call.args, &text.text)
                    .unwrap_or_else(|| call.args.clone());
                tools(request, false, call.name.clone(), args).await
            };
            messages.push(Message::tool(call.id, result.to_string()));
        }
    }
    let reply = fitted(reply.trim());
    if reply.is_empty() {
        say(piece(ChatPart::Error, NOT_NOW));
        return;
    }
    say(piece(ChatPart::Final, &reply));
    if !waiting.is_empty() {
        let keys: Vec<String> = waiting.into_iter().map(|item| item.key).collect();
        news.given(&keys);
    }
    chats.remember(&session, &text.text, &reply, Instant::now());
}

/// `reply` as one message carries it ([`lamp_msgs::CHAT_TEXT_BYTES`]):
/// cut at a character's edge, with "…" to say so, when it is longer.
pub fn fitted(reply: &str) -> String {
    if reply.len() <= lamp_msgs::CHAT_TEXT_BYTES {
        return reply.to_owned();
    }
    let mut end = lamp_msgs::CHAT_TEXT_BYTES - '…'.len_utf8();
    while !reply.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &reply[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::sync::{Arc, Mutex};

    /// A model of the test's own: each round's words and calls, in order,
    /// and what it was sent each round.
    struct Scripted {
        rounds: Mutex<Vec<(Vec<&'static str>, Vec<ToolCall>)>>,
        sent: Mutex<Vec<Vec<Message>>>,
    }

    impl Model for Scripted {
        fn round<'a>(
            &'a self,
            messages: &'a [Message],
            text: &'a mut (dyn FnMut(&str) + Send),
        ) -> BoxFuture<'a, Result<Vec<ToolCall>, String>> {
            Box::pin(async move {
                self.sent.lock().unwrap().push(messages.to_vec());
                let mut rounds = self.rounds.lock().unwrap();
                if rounds.is_empty() {
                    return Err("no more rounds".into());
                }
                let (words, calls) = rounds.remove(0);
                for word in words {
                    text(word);
                }
                Ok(calls)
            })
        }
    }

    fn scripted(rounds: Vec<(Vec<&'static str>, Vec<ToolCall>)>) -> Scripted {
        Scripted {
            rounds: Mutex::new(rounds),
            sent: Mutex::new(Vec::new()),
        }
    }

    fn system() -> System {
        Arc::new(|| crate::second_provider::Told::from("You are Lamp.".to_owned()))
    }

    fn no_tools() -> ToolRunner {
        Arc::new(|_, _, _, _| Box::pin(async { json!({"result": "none"}) }))
    }

    fn text(words: &str, session: Option<&str>) -> ChatIn {
        ChatIn {
            run_id: "run-1".into(),
            session_id: session.map(str::to_owned),
            text: words.into(),
            speak: false,
        }
    }

    async fn pieces(
        model: &Scripted,
        chats: &mut Chats,
        tools: &ToolRunner,
        said: &ChatIn,
    ) -> Vec<ChatOut> {
        let mut out = Vec::new();
        answer(
            model,
            &system(),
            tools,
            &NoNews,
            chats,
            said,
            REQUESTS_FROM + 1,
            |piece| out.push(piece),
        )
        .await;
        out
    }

    /// News for the tests: what waits, and what was given.
    #[derive(Default)]
    struct Waiting {
        news: Mutex<Vec<crate::task_news::News>>,
        given: Mutex<Vec<String>>,
    }

    impl Untold for Waiting {
        fn untold(&self) -> Vec<crate::task_news::News> {
            self.news.lock().unwrap().clone()
        }
        fn given(&self, keys: &[String]) {
            self.news
                .lock()
                .unwrap()
                .retain(|news| !keys.contains(&news.key));
            self.given.lock().unwrap().extend(keys.iter().cloned());
        }
    }

    fn news(key: &str, note: &str) -> crate::task_news::News {
        crate::task_news::News {
            key: key.into(),
            note: note.into(),
            until: None,
            progress: None,
            finished: None,
            also: Vec::new(),
        }
    }

    /// #1673 (the first builder, 20 Oct 21:14): news of work asked for by
    /// text that nobody was told in a minute goes to the phone unasked, as
    /// exactly one `task_news` piece with no run, whose text is the model's
    /// words and nothing else; once sent it is kept as told and never sent
    /// again. Before the minute, nothing.
    #[tokio::test]
    async fn news_nobody_was_told_goes_to_the_phone_once_in_the_models_words() {
        let waiting = Waiting::default();
        waiting.news.lock().unwrap().push(news(
            "s1:1:1",
            "(Nobody has spoken. News from work you handed over for the person. Claude Code just finished the names: \"Quilla, Brembo and Oskit.\")",
        ));
        let words = "Claude Code finished the names: Quilla, Brembo and Oskit.";
        let model = scripted(vec![(
            vec![
                "Claude Code finished ",
                "the names: Quilla, Brembo and Oskit.",
            ],
            Vec::new(),
        )]);
        let mut unasked = Unasked::default();
        let start = Instant::now();
        let mut out = Vec::new();
        for after in [0, 59, 60, 61, 120, 600] {
            tell_unasked(
                &model,
                &system(),
                &waiting,
                &mut unasked,
                start + Duration::from_secs(after),
                true,
                |piece| {
                    out.push((after, piece));
                    true
                },
            )
            .await;
        }
        assert_eq!(
            out,
            [(
                60,
                ChatOut {
                    run_id: None,
                    session_id: None,
                    part: ChatPart::TaskNews,
                    text: words.into(),
                }
            )]
        );
        assert!(out[0].1.check().is_ok());
        assert_eq!(*waiting.given.lock().unwrap(), ["s1:1:1"]);
        let sent = model.sent.lock().unwrap();
        assert_eq!(sent.len(), 1, "the model is asked once");
        let asked = format!("{:?}", sent[0].last().unwrap());
        assert!(asked.contains("Quilla, Brembo and Oskit"), "{asked}");
        assert!(!asked.contains("Nobody has spoken."), "{asked}");
    }

    /// A model that can't be reached, or says no words, sends nothing: the
    /// news is not kept as told, and is tried again a minute on, at most
    /// three times, then waits for the person's next text.
    #[tokio::test]
    async fn news_for_the_phone_waits_on_a_fault_and_stops_after_its_tries() {
        let waiting = Waiting::default();
        waiting
            .news
            .lock()
            .unwrap()
            .push(news("s1:2:1", "(Nobody has spoken. Codex stopped.)"));
        // No words, only a call: nothing to send, and the call isn't run.
        let call = ToolCall {
            id: "c1".into(),
            name: "set_volume".into(),
            args: json!({"volume": 10}),
            extra: None,
        };
        let model = scripted(vec![(Vec::new(), vec![call])]);
        let mut unasked = Unasked::default();
        let start = Instant::now();
        let mut out = Vec::new();
        let mut asked = Vec::new();
        for after in [0, 60, 61, 120, 180, 240, 300, 900] {
            tell_unasked(
                &model,
                &system(),
                &waiting,
                &mut unasked,
                start + Duration::from_secs(after),
                true,
                |piece| {
                    out.push(piece);
                    true
                },
            )
            .await;
            asked.push(model.sent.lock().unwrap().len());
        }
        assert_eq!(asked, [0, 1, 1, 2, 3, 3, 3, 3], "a minute between tries");
        assert!(out.is_empty(), "{out:?}");
        assert!(waiting.given.lock().unwrap().is_empty());
        assert_eq!(
            waiting.news.lock().unwrap().len(),
            1,
            "it waits for the next text"
        );
        assert_eq!(
            model.sent.lock().unwrap().len(),
            3,
            "tried at 60, 120 and 180 s only"
        );
    }

    /// Nothing is tried while nothing takes `chat/out` (`lamp-cloud`
    /// stopped), so news no phone could get isn't kept as told; and news
    /// told aloud while the model wrote is not sent too (the reviewer on
    /// #2311).
    #[tokio::test]
    async fn news_for_the_phone_waits_for_a_listener_and_isnt_sent_once_told_aloud() {
        let waiting = Waiting::default();
        waiting
            .news
            .lock()
            .unwrap()
            .push(news("s1:3:1", "(Nobody has spoken. Codex finished.)"));
        let model = scripted(vec![(vec!["Codex finished."], Vec::new())]);
        let mut unasked = Unasked::default();
        let start = Instant::now();
        let mut out = Vec::new();
        for after in [0, 60, 120] {
            let at = start + Duration::from_secs(after);
            tell_unasked(
                &model,
                &system(),
                &waiting,
                &mut unasked,
                at,
                false,
                |piece| {
                    out.push(piece);
                    true
                },
            )
            .await;
        }
        assert!(out.is_empty());
        assert!(model.sent.lock().unwrap().is_empty(), "not tried");
        // Told aloud while the model writes: nothing is sent.
        struct AloudDuring<'w>(&'w Waiting);
        impl Model for AloudDuring<'_> {
            fn round<'a>(
                &'a self,
                _: &'a [Message],
                text: &'a mut (dyn FnMut(&str) + Send),
            ) -> BoxFuture<'a, Result<Vec<ToolCall>, String>> {
                Box::pin(async move {
                    self.0.news.lock().unwrap().clear();
                    text("Codex finished.");
                    Ok(Vec::new())
                })
            }
        }
        let at = start + Duration::from_secs(180);
        tell_unasked(
            &AloudDuring(&waiting),
            &system(),
            &waiting,
            &mut unasked,
            at,
            true,
            |piece| {
                out.push(piece);
                true
            },
        )
        .await;
        assert!(out.is_empty(), "{out:?}");
        assert!(waiting.given.lock().unwrap().is_empty());
    }

    /// A piece that reached nothing (its publish delivered to no one) is
    /// not kept as told: it is sent again a minute on, and kept only once it
    /// is delivered (the first builder, 20 Oct 04:08).
    #[tokio::test]
    async fn news_for_the_phone_lost_on_the_way_is_sent_again() {
        let waiting = Waiting::default();
        waiting
            .news
            .lock()
            .unwrap()
            .push(news("s1:5:1", "(Nobody has spoken. Codex finished.)"));
        let model = scripted(vec![
            (vec!["Codex finished."], Vec::new()),
            (vec!["Codex finished."], Vec::new()),
        ]);
        let mut unasked = Unasked::default();
        let start = Instant::now();
        let mut sent = Vec::new();
        for (after, reaches) in [(0, true), (60, false), (61, true), (120, true), (600, true)] {
            tell_unasked(
                &model,
                &system(),
                &waiting,
                &mut unasked,
                start + Duration::from_secs(after),
                true,
                |_| {
                    sent.push((after, reaches));
                    reaches
                },
            )
            .await;
        }
        assert_eq!(sent, [(60, false), (120, true)]);
        assert_eq!(*waiting.given.lock().unwrap(), ["s1:5:1"]);
    }

    /// A text that comes while the news for the phone is being written is
    /// answered at once, not after that round, and gives the news itself,
    /// so the round is let go and nothing more is sent (the reviewer on
    /// #2311).
    #[tokio::test]
    async fn a_text_never_waits_on_the_news_for_the_phone() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, AtomicU64, Ordering},
        };

        struct PhoneRound(Option<Arc<AtomicBool>>);
        impl Drop for PhoneRound {
            fn drop(&mut self) {
                if let Some(cancelled) = self.0.as_ref() {
                    cancelled.store(true, Ordering::Release);
                }
            }
        }
        struct Fixture {
            root: std::path::PathBuf,
            worker: Option<tokio::task::JoinHandle<()>>,
        }
        impl Drop for Fixture {
            fn drop(&mut self) {
                if let Some(worker) = self.worker.take() {
                    worker.abort();
                }
                if let Err(error) = std::fs::remove_dir_all(&self.root)
                    && error.kind() != std::io::ErrorKind::NotFound
                {
                    eprintln!("phone fixture cleanup {}: {error}", self.root.display());
                }
            }
        }
        /// Slow for the news for the phone, at once for a text.
        struct SlowForThePhone {
            began: Arc<tokio::sync::Notify>,
            cancelled: Arc<AtomicBool>,
        }
        impl Model for SlowForThePhone {
            fn round<'a>(
                &'a self,
                messages: &'a [Message],
                text: &'a mut (dyn FnMut(&str) + Send),
            ) -> BoxFuture<'a, Result<Vec<ToolCall>, String>> {
                Box::pin(async move {
                    if format!("{:?}", messages.last()).contains("Nobody wrote just now") {
                        let mut round = PhoneRound(Some(self.cancelled.clone()));
                        self.began.notify_one();
                        tokio::time::sleep(Duration::from_secs(30)).await;
                        round.0 = None;
                        text("Codex finished, says the phone.");
                    } else {
                        text("Hi. Codex finished.");
                    }
                    Ok(Vec::new())
                })
            }
        }
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "lvm-phone-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let mut fixture = Fixture {
            root: root.clone(),
            worker: None,
        };
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut replies = bus
            .subscribe_typed(ChatOut::TOPIC, "test", ChatOut::TYPE)
            .unwrap();
        let waiting = Waiting::default();
        waiting
            .news
            .lock()
            .unwrap()
            .push(news("s1:4:1", "(Nobody has spoken. Codex finished.)"));
        let began = Arc::new(tokio::sync::Notify::new());
        let cancelled = Arc::new(AtomicBool::new(false));
        fixture.worker = Some(tokio::spawn(serve_with(
            SlowForThePhone {
                began: began.clone(),
                cancelled: cancelled.clone(),
            },
            system(),
            no_tools(),
            Box::new(waiting),
            root.clone(),
            Unasked::after(Duration::ZERO),
        )));
        let mut sends = bus
            .publisher_declared(ChatIn::TOPIC, &ChatIn::declaration(), "cloud")
            .unwrap();
        // The slow phone round must be polled before the text is timed.
        tokio::time::timeout(Duration::from_secs(5), began.notified())
            .await
            .expect("phone-news round did not begin");
        let sent_at = Instant::now();
        let until = sent_at + Duration::from_secs(5);
        while sends
            .publish_json(lamp_ipc::monotonic_us(), &text("Hi lamp", Some("s1")))
            .unwrap()
            .delivered
            == 0
        {
            assert!(Instant::now() < until, "the worker never listened");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let mut got = Vec::new();
        while got.last().map(|out: &ChatOut| out.part) != Some(ChatPart::Final) {
            assert!(Instant::now() < until, "the text waited: {got:?}");
            if let Some(message) = replies.recv(Duration::ZERO).unwrap() {
                got.push(message.parse::<ChatOut>().unwrap());
            } else {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
        assert!(
            sent_at.elapsed() < Duration::from_secs(2),
            "{:?}",
            sent_at.elapsed()
        );
        // The 30-second future must have been dropped, not left running
        // beyond the observation window below.
        assert!(
            cancelled.load(Ordering::Acquire),
            "the pending phone round was not cancelled"
        );
        // The text gave the news: nothing more goes to the phone.
        tokio::time::sleep(UNASKED_LOOK_EVERY * 2).await;
        while let Some(message) = replies.recv(Duration::ZERO).unwrap() {
            got.push(message.parse::<ChatOut>().unwrap());
        }
        let worker = fixture.worker.take().unwrap();
        worker.abort();
        assert!(worker.await.is_err_and(|error| error.is_cancelled()));
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            got.iter().all(|out| out.part != ChatPart::TaskNews),
            "{got:?}"
        );
        assert_eq!(
            got.last().map(|out| out.text.as_str()),
            Some("Hi. Codex finished.")
        );
    }

    /// #1673: news not told aloud is given with the answer to the next text,
    /// typed, once: the next text has none, and a fault gives none.
    #[tokio::test]
    async fn news_waiting_is_given_with_the_next_texts_answer_once() {
        let waiting = Waiting::default();
        waiting.news.lock().unwrap().push(news(
            "s1:1:1",
            "(Nobody has spoken. News from work you handed over for the person. Claude Code just finished the names: \"Lumi, Sunny and Archie.\")",
        ));
        let model = scripted(vec![
            (
                vec!["It's six. Claude Code finished the names."],
                Vec::new(),
            ),
            (vec!["Yes."], Vec::new()),
        ]);
        let mut chats = Chats::default();
        for words in ["What time is it?", "Are you there?"] {
            let request = chats.next_request("s1");
            answer(
                &model,
                &system(),
                &no_tools(),
                &waiting,
                &mut chats,
                &text(words, Some("s1")),
                request,
                |_| {},
            )
            .await;
        }
        let (first, second) = {
            let sent = model.sent.lock().unwrap();
            (
                serde_json::to_value(&sent[0][0]).unwrap(),
                serde_json::to_value(&sent[1][0]).unwrap(),
            )
        };
        let first = first["content"].as_str().unwrap();
        assert!(first.contains(NEWS), "{first}");
        assert!(first.contains("Lumi, Sunny and Archie."), "{first}");
        assert!(!first.contains("Nobody has spoken"), "{first}");
        assert!(!second["content"].as_str().unwrap().contains(NEWS));
        assert_eq!(*waiting.given.lock().unwrap(), ["s1:1:1"]);

        // A model that can't answer gives nothing: the news still waits.
        let waiting = Waiting::default();
        waiting.news.lock().unwrap().push(news("s1:2:1", "(News.)"));
        let failing = scripted(Vec::new());
        answer(
            &failing,
            &system(),
            &no_tools(),
            &waiting,
            &mut Chats::default(),
            &text("Hi", None),
            REQUESTS_FROM + 1,
            |_| {},
        )
        .await;
        assert!(waiting.given.lock().unwrap().is_empty());
        assert_eq!(waiting.untold().len(), 1);
    }

    #[test]
    fn the_switch_is_on_only_for_on_and_its_flag_is_taken_first() {
        assert_eq!(arguments(Some("on")), [FLAG]);
        for off in [None, Some(""), Some("off"), Some("ON")] {
            assert!(arguments(off).is_empty(), "{off:?}");
        }
        let flags = [FLAG.to_owned(), "x".to_owned()];
        assert_eq!(take(&flags), (true, &flags[1..]));
        assert_eq!(take(&flags[1..]), (false, &flags[1..]));
    }

    #[tokio::test]
    async fn a_text_is_answered_in_pieces_then_whole_and_told_it_is_typed() {
        let model = scripted(vec![(vec!["It's ", "six."], Vec::new())]);
        let mut chats = Chats::default();
        let out = pieces(
            &model,
            &mut chats,
            &no_tools(),
            &text("What time is it?", Some("s1")),
        )
        .await;
        let parts: Vec<(ChatPart, &str)> = out.iter().map(|o| (o.part, o.text.as_str())).collect();
        assert_eq!(
            parts,
            [
                (ChatPart::Delta, "It's "),
                (ChatPart::Delta, "six."),
                (ChatPart::Final, "It's six.")
            ]
        );
        assert!(out.iter().all(|o| o.run_id.as_deref() == Some("run-1")
            && o.session_id.as_deref() == Some("s1")
            && o.check().is_ok()));
        let sent = model.sent.lock().unwrap();
        let system = serde_json::to_value(&sent[0][0]).unwrap();
        assert!(
            system["content"].as_str().unwrap().contains(NOTE),
            "{system}"
        );
        assert!(
            system["content"]
                .as_str()
                .unwrap()
                .starts_with("You are Lamp.")
        );
        let last = serde_json::to_value(sent[0].last().unwrap()).unwrap();
        assert_eq!(last["content"], "What time is it?");
    }

    /// The reviewer on #1713: a reply longer than a message carries is cut
    /// to fit, with "…", and the words sent before it never pass that
    /// either, so every piece is one `lamp-cloud` can carry.
    #[tokio::test]
    async fn a_reply_longer_than_a_message_is_cut_to_fit() {
        // Two-byte characters, 7,000 bytes a piece: 21,000 in all.
        let piece: &'static str = Box::leak("é".repeat(3_500).into_boxed_str());
        let model = scripted(vec![(vec![piece, piece, piece], Vec::new())]);
        let out = pieces(
            &model,
            &mut Chats::default(),
            &no_tools(),
            &text("Tell me a long story.", None),
        )
        .await;
        assert!(out.iter().all(|o| o.check().is_ok()));
        let words: usize = out
            .iter()
            .filter(|o| o.part == ChatPart::Delta)
            .map(|o| o.text.len())
            .sum();
        assert_eq!(words, 14_000);
        let last = out.last().unwrap();
        assert_eq!(last.part, ChatPart::Final);
        assert!(last.text.len() <= lamp_msgs::CHAT_TEXT_BYTES);
        assert!(
            last.text.ends_with("é…"),
            "{}",
            &last.text[last.text.len() - 8..]
        );
        assert_eq!(fitted("short"), "short");
    }

    #[tokio::test]
    async fn a_chat_keeps_its_own_exchanges_and_another_chat_none() {
        let model = scripted(vec![
            (vec!["Hello."], Vec::new()),
            (vec!["Yes, still here."], Vec::new()),
            (vec!["Hi."], Vec::new()),
        ]);
        let mut chats = Chats::default();
        pieces(
            &model,
            &mut chats,
            &no_tools(),
            &text("Hi lamp", Some("s1")),
        )
        .await;
        pieces(
            &model,
            &mut chats,
            &no_tools(),
            &text("Are you there?", Some("s1")),
        )
        .await;
        pieces(&model, &mut chats, &no_tools(), &text("Hey", Some("s2"))).await;
        let sent = model.sent.lock().unwrap();
        // The second text of s1 carries the first exchange; s2 carries none.
        let contents = |round: usize| -> Vec<Value> {
            sent[round]
                .iter()
                .map(|m| serde_json::to_value(m).unwrap()["content"].clone())
                .collect()
        };
        assert_eq!(
            contents(1)[1..],
            [json!("Hi lamp"), json!("Hello."), json!("Are you there?")]
        );
        assert_eq!(contents(2)[1..], [json!("Hey")]);
    }

    #[tokio::test]
    async fn a_function_the_model_calls_is_run_typed_and_its_result_given_back() {
        let call = ToolCall {
            id: "c1".into(),
            name: "set_light".into(),
            args: json!({"colour": "blue"}),
            extra: None,
        };
        let model = scripted(vec![
            (vec!["One moment. "], vec![call]),
            (vec!["It's blue now."], Vec::new()),
        ]);
        let ran = Arc::new(Mutex::new(Vec::new()));
        let seen = ran.clone();
        let tools: ToolRunner = Arc::new(move |request, heard, name, args| {
            seen.lock().unwrap().push((request, heard, name, args));
            Box::pin(async { json!({"done": true}) })
        });
        let mut chats = Chats::default();
        let out = pieces(
            &model,
            &mut chats,
            &tools,
            &text("Make the light blue", None),
        )
        .await;
        assert_eq!(out.last().unwrap().part, ChatPart::Final);
        assert_eq!(out.last().unwrap().text, "One moment. It's blue now.");
        let ran = ran.lock().unwrap();
        assert_eq!(ran.len(), 1);
        assert_eq!(ran[0].0, REQUESTS_FROM + 1);
        assert!(!ran[0].1, "a text is not heard");
        assert_eq!(ran[0].2, "set_light");
        let sent = model.sent.lock().unwrap();
        let result = serde_json::to_value(sent[1].last().unwrap()).unwrap();
        assert_eq!(result["role"], "tool");
        assert_eq!(result["tool_call_id"], "c1");
    }

    #[tokio::test]
    async fn a_model_that_cannot_answer_is_a_fault_the_person_can_read() {
        let model = scripted(Vec::new());
        let mut chats = Chats::default();
        let out = pieces(&model, &mut chats, &no_tools(), &text("hi", None)).await;
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].part, ChatPart::Error);
        assert_eq!(out[0].text, NOT_NOW);
    }

    /// Through the bus, as the second route's worker runs it: a text on
    /// `chat/in` is answered in pieces on `chat/out`, the whole reply last.
    #[tokio::test]
    async fn the_worker_answers_a_text_from_the_bus_on_the_bus() {
        let root = std::path::PathBuf::from(format!("/tmp/lvm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut replies = bus
            .subscribe_typed(ChatOut::TOPIC, "test", ChatOut::TYPE)
            .unwrap();
        let model = scripted(vec![(vec!["Hello ", "there."], Vec::new())]);
        let worker = tokio::spawn(serve(
            model,
            system(),
            no_tools(),
            Box::new(NoNews),
            root.clone(),
        ));
        let mut sends = bus
            .publisher_declared(ChatIn::TOPIC, &ChatIn::declaration(), "cloud")
            .unwrap();
        // The worker listens once it has started: send until it has heard.
        let until = Instant::now() + Duration::from_secs(5);
        while sends
            .publish_json(lamp_ipc::monotonic_us(), &text("Hi lamp", Some("s1")))
            .unwrap()
            .delivered
            == 0
        {
            assert!(Instant::now() < until, "the worker never listened");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let mut got = Vec::new();
        while got.last().map(|out: &ChatOut| out.part) != Some(ChatPart::Final) {
            assert!(Instant::now() < until, "{got:?}");
            if let Some(message) = replies.recv(Duration::ZERO).unwrap() {
                got.push(message.parse::<ChatOut>().unwrap());
            } else {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
        worker.abort();
        let _ = std::fs::remove_dir_all(&root);
        let parts: Vec<(ChatPart, &str)> = got.iter().map(|o| (o.part, o.text.as_str())).collect();
        assert_eq!(
            parts,
            [
                (ChatPart::Delta, "Hello "),
                (ChatPart::Delta, "there."),
                (ChatPart::Final, "Hello there.")
            ]
        );
        assert!(got.iter().all(|o| o.run_id.as_deref() == Some("run-1")));
    }

    /// A model that takes `wait` over each answer, and answers "Done."
    struct Slow {
        wait: Duration,
    }

    impl Model for Slow {
        fn round<'a>(
            &'a self,
            _: &'a [Message],
            text: &'a mut (dyn FnMut(&str) + Send),
        ) -> BoxFuture<'a, Result<Vec<ToolCall>, String>> {
            Box::pin(async move {
                tokio::time::sleep(self.wait).await;
                text("Done.");
                Ok(Vec::new())
            })
        }
    }

    /// The reviewer on #1713: a text still waiting on `chat/in` when its
    /// time is up, here behind a slow answer, is passed over: never
    /// answered, its functions never run. The next text is answered.
    #[tokio::test]
    async fn a_text_whose_time_is_up_behind_a_slow_one_is_passed_over() {
        let root = std::path::PathBuf::from(format!("/tmp/lvs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut replies = bus
            .subscribe_typed(ChatOut::TOPIC, "test", ChatOut::TYPE)
            .unwrap();
        let model = Slow {
            wait: Duration::from_millis(400),
        };
        let worker = tokio::spawn(serve(
            model,
            system(),
            no_tools(),
            Box::new(NoNews),
            root.clone(),
        ));
        let mut sends = bus
            .publisher_declared(ChatIn::TOPIC, &ChatIn::declaration(), "cloud")
            .unwrap();
        let run = |id: &str| ChatIn {
            run_id: id.into(),
            ..text("Hi lamp", Some("s1"))
        };
        let until = Instant::now() + Duration::from_secs(10);
        while sends
            .publish_json(lamp_ipc::monotonic_us(), &run("run-1"))
            .unwrap()
            .delivered
            == 0
        {
            assert!(Instant::now() < until, "the worker never listened");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        // While the first is answered: one that lasts 50 ms, then one as
        // the app sends them.
        tokio::time::sleep(Duration::from_millis(100)).await;
        sends.expires_after(Some(Duration::from_millis(50)));
        sends
            .publish_json(lamp_ipc::monotonic_us(), &run("run-2"))
            .unwrap();
        sends.expires_after(ChatIn::declaration().lifespan());
        sends
            .publish_json(lamp_ipc::monotonic_us(), &run("run-3"))
            .unwrap();
        let mut finals = Vec::new();
        while finals.len() < 2 {
            assert!(Instant::now() < until, "{finals:?}");
            if let Some(message) = replies.recv(Duration::ZERO).unwrap() {
                let out = message.parse::<ChatOut>().unwrap();
                if out.part == ChatPart::Final {
                    finals.push(out.run_id.unwrap_or_default());
                }
            } else {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
        worker.abort();
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(finals, ["run-1", "run-3"]);
    }

    #[test]
    fn each_chat_numbers_its_texts_in_a_block_of_its_own() {
        let mut chats = Chats::default();
        let one = chats.next_request("s1");
        let two = chats.next_request("s1");
        let other = chats.next_request("s2");
        assert!(two > one && one >= REQUESTS_FROM);
        assert!(same_chat(one, two));
        assert!(!same_chat(one, other));
        assert!(!same_chat(5, 6), "spoken requests are no chat's");
        assert!(!same_chat(5, one));
    }

    #[test]
    fn a_chat_keeps_at_most_twelve_messages_and_forgets_after_half_an_hour() {
        let mut chats = Chats::default();
        let now = Instant::now();
        for n in 0..10 {
            chats.remember("s", &format!("q{n}"), &format!("a{n}"), now);
        }
        assert_eq!(chats.kept("s", now).len(), KEPT);
        assert!(chats.kept("s", now + FORGET_AFTER).is_empty());
    }
}
