//! Texting the lamp from the Autonomous app, the voice's side (#1673,
//! `docs/messages.md`): each text on `chat/in` is answered in text on
//! `chat/out`, on the second route's text model, beside the spoken
//! conversation and never in it: the room's Live session and its speaker
//! are not touched. The same character, notes and memory as a spoken turn
//! (the second route's system message), and the same functions, with a
//! note that the conversation is typed. Each chat (the app's `session_id`)
//! keeps its own few exchanges. Off unless `LAMP_MESSAGES=on`, and only
//! where the second route's worker runs, which needs its config
//! (`LAMP_VOICE_SECOND_CONFIG`).
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
    bus_root: std::path::PathBuf,
) {
    let bus = lamp_bus::Bus::new(&bus_root);
    let ends = bus.and_then(|bus| {
        Ok((
            bus.subscribe_typed(ChatIn::TOPIC, "texts", ChatIn::TYPE)?,
            bus.publisher_declared(ChatOut::TOPIC, &ChatOut::declaration(), "voice")?,
        ))
    });
    let (mut input, mut output) = match ends {
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
    let mut look = tokio::time::interval(LOOK_EVERY);
    look.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        look.tick().await;
        while let Ok(Some(message)) = input.recv(Duration::ZERO) {
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
                &mut chats,
                &text,
                request,
                |piece| {
                    let _ = output.publish_json(lamp_ipc::monotonic_us(), &piece);
                },
            )
            .await;
        }
    }
}

/// The answer to `text`, numbered `request` for the functions: its pieces
/// passed to `say` as they come (words, then the whole reply, or a fault).
pub async fn answer(
    model: &impl Model,
    system: &System,
    tools: &ToolRunner,
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
    let mut messages = vec![Message::system(format!("{}\n\n{NOTE}", told.text))];
    messages.extend(chats.kept(&session, Instant::now()));
    messages.push(Message::user(&text.text));
    let mut reply = String::new();
    for round in 0..ROUNDS {
        let mut said = String::new();
        let calls = {
            let mut text = |words: &str| {
                said.push_str(words);
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
            let result = tools(request, false, call.name.clone(), call.args.clone()).await;
            messages.push(Message::tool(call.id, result.to_string()));
        }
    }
    let reply = reply.trim().to_owned();
    if reply.is_empty() {
        say(piece(ChatPart::Error, NOT_NOW));
        return;
    }
    say(piece(ChatPart::Final, &reply));
    chats.remember(&session, &text.text, &reply, Instant::now());
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
            chats,
            said,
            REQUESTS_FROM + 1,
            |piece| out.push(piece),
        )
        .await;
        out
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
        let worker = tokio::spawn(serve(model, system(), no_tools(), root.clone()));
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
