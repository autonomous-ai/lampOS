//! The engine against scripted services: no network.
use super::*;

mod held_consent;
use futures_util::stream::{self, BoxStream};
use net::ToolCall;
use serde_json::json;
use std::sync::atomic::AtomicUsize;

/// Scripted services. Speech of a sentence is one 1,500-sample packet
/// whose every sample is the sentence's length, so order can be read
/// back; a sentence containing "FAIL" cannot be spoken.
#[derive(Default)]
struct Script {
    listen_error: Option<net::Error>,
    heard: Mutex<VecDeque<net::Result<String>>>,
    samples_heard: Arc<AtomicUsize>,
    chats: Mutex<VecDeque<Vec<net::Result<ChatEvent>>>>,
    asked: Mutex<Vec<Vec<Message>>>,
    /// A chat stream that, after its script, never ends.
    hold_chat: bool,
    /// A chat's first event waits this long (on the test's clock).
    first_event_after: Option<Duration>,
    /// Speech of this sentence starts late.
    slow: Option<&'static str>,
    /// Speech of this sentence never ends.
    endless: Option<&'static str>,
    /// Held by every live stream: dropped streams let go of it.
    token: Arc<()>,
    /// How many times the voice was asked to make ready ([`Services::warm`]).
    warmed: Arc<AtomicUsize>,
    /// Each listener's early words, given as its line ends.
    early: Mutex<VecDeque<Vec<net::Early>>>,
    /// The final words come this long after the line's end, as `Finalize`'s
    /// answer does.
    finish_after: Option<Duration>,
    /// The functions each chat told only some was told ([`Services::chat_only`]).
    told_only: Mutex<Vec<&'static [&'static str]>>,
    /// How many listeners were asked for, and whether those made say their
    /// socket has closed ([`Listener::open`]).
    listened: AtomicUsize,
    closed: Arc<AtomicBool>,
    /// Each listener's stream drops after this many samples sent, as a
    /// connection lost mid-line does (#1844's D); `None`, never.
    drops_after: Mutex<VecDeque<Option<usize>>>,
    /// The requests a speech socket was asked for ([`Services::open_speech_ahead`]).
    sockets_asked: Mutex<Vec<u64>>,
    /// How many times a listener was asked for its words so far, at a pause
    /// ([`Listener::flush`], `LAMP_SECOND_EARLY_FINALIZE`).
    flushed: Arc<AtomicUsize>,
}

struct ScriptListener {
    heard: net::Result<String>,
    samples: Arc<AtomicUsize>,
    early: Vec<net::Early>,
    finish_after: Option<Duration>,
    closed: Arc<AtomicBool>,
    drops_after: Option<usize>,
    sent: AtomicUsize,
    flushed: Arc<AtomicUsize>,
}
impl Listener for ScriptListener {
    fn flush(&self) -> net::Result<()> {
        self.flushed.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    fn open(&self) -> bool {
        !self.closed.load(Ordering::Relaxed)
    }
    fn send(&self, samples: &[i16]) -> net::Result<()> {
        let sent = self.sent.fetch_add(samples.len(), Ordering::Relaxed) + samples.len();
        if self.drops_after.is_some_and(|after| sent > after) {
            return Err(net::Error::Transport("speech-to-text stream"));
        }
        self.samples.fetch_add(samples.len(), Ordering::Relaxed);
        Ok(())
    }
    fn finish(self: Box<Self>, _request: u64) -> BoxFuture<'static, net::Result<String>> {
        Box::pin(async move {
            if let Some(after) = self.finish_after {
                tokio::time::sleep(after).await;
            }
            self.heard
        })
    }
    fn early(&mut self) -> Option<mpsc::UnboundedReceiver<net::Early>> {
        let (said, early) = mpsc::unbounded_channel();
        for words in std::mem::take(&mut self.early) {
            let _ = said.send(words);
        }
        Some(early)
    }
}

impl Services for Script {
    fn listen(&self) -> BoxFuture<'static, net::Result<Box<dyn Listener>>> {
        self.listened.fetch_add(1, Ordering::Relaxed);
        let result = match &self.listen_error {
            Some(error) => Err(error.clone()),
            // A listener kept for a line the script never plays hears none.
            None => Ok(Box::new(ScriptListener {
                heard: self
                    .heard
                    .lock()
                    .unwrap()
                    .pop_front()
                    .unwrap_or_else(|| Ok(String::new())),
                samples: self.samples_heard.clone(),
                early: self.early.lock().unwrap().pop_front().unwrap_or_default(),
                finish_after: self.finish_after,
                closed: self.closed.clone(),
                drops_after: self.drops_after.lock().unwrap().pop_front().flatten(),
                sent: AtomicUsize::new(0),
                flushed: self.flushed.clone(),
            }) as Box<dyn Listener>),
        };
        Box::pin(async move { result })
    }

    fn chat(&self, messages: Vec<Message>) -> BoxStream<'static, net::Result<ChatEvent>> {
        self.asked.lock().unwrap().push(messages);
        let events = self.chats.lock().unwrap().pop_front().unwrap();
        let token = self.token.clone();
        let delay = self.first_event_after;
        let scripted = stream::once(async move {
            if let Some(delay) = delay {
                tokio::time::sleep(delay).await;
            }
        })
        .filter_map(|()| async { None })
        .chain(stream::iter(events));
        if self.hold_chat {
            scripted
                .chain(stream::pending().map(move |never: ()| {
                    let _ = &token;
                    Ok(ChatEvent::Text(format!("{never:?}")))
                }))
                .boxed()
        } else {
            scripted.boxed()
        }
    }

    fn chat_only(
        &self,
        messages: Vec<Message>,
        only: &'static [&'static str],
    ) -> BoxStream<'static, net::Result<ChatEvent>> {
        self.told_only.lock().unwrap().push(only);
        self.chat(messages)
    }

    fn speak(&self, text: String) -> BoxStream<'static, net::Result<Vec<i16>>> {
        let token = self.token.clone();
        if self.endless == Some(text.as_str()) {
            return stream::repeat_with(move || {
                let _ = &token;
                Ok(vec![7; PACKET_SAMPLES])
            })
            .then(|packet| async move {
                tokio::time::sleep(Duration::from_millis(1)).await;
                packet
            })
            .boxed();
        }
        let delay = if self.slow == Some(text.as_str()) {
            30
        } else {
            0
        };
        stream::once(async move {
            tokio::time::sleep(Duration::from_millis(delay)).await;
            if text.contains("FAIL") {
                Err(net::Error::Status(503))
            } else {
                Ok(vec![text.len() as i16; 1_500])
            }
        })
        .boxed()
    }

    fn warm(&self) -> BoxFuture<'static, ()> {
        self.warmed.fetch_add(1, Ordering::Relaxed);
        Box::pin(async {})
    }

    fn open_speech_ahead(&self, request: u64) {
        self.sockets_asked.lock().unwrap().push(request);
    }
}

fn text(piece: &str) -> net::Result<ChatEvent> {
    Ok(ChatEvent::Text(piece.to_owned()))
}

fn call(id: &str, name: &str, args: Value) -> ToolCall {
    ToolCall {
        extra: None,
        id: id.into(),
        name: name.into(),
        args,
    }
}

/// Each tool call: its name, its arguments, and whether it was heard.
type Calls = Arc<Mutex<Vec<(String, Value, bool)>>>;

/// An engine on `script` whose tools answer `{"ok": true}`, with a clock
/// the test moves.
fn scripted(script: Arc<Script>) -> (Engine, Calls, Arc<Mutex<Instant>>) {
    let calls: Calls = Arc::default();
    let seen = calls.clone();
    let tools: ToolRunner = Arc::new(move |_, heard, name, args| {
        seen.lock().unwrap().push((name, args, heard));
        Box::pin(async { json!({"ok": true}) })
    });
    let now = Arc::new(Mutex::new(Instant::now()));
    let clock = now.clone();
    let engine = Engine::with_clock(
        script,
        Arc::new(|| "SYSTEM".to_owned().into()),
        tools,
        Arc::new(move || *clock.lock().unwrap()),
    );
    (engine, calls, now)
}

fn start(request: u64) -> ProviderInput {
    ProviderInput::Start {
        request,
        session: 1,
        privacy_generation: 1,
        continues: false,
        continues_cut_off: false,
        over_the_lamp: false,
        after_stop: false,
        follows_on: false,
        over_the_lamps_voice: false,
        heard_of_cut: None,
    }
}
fn audio(request: u64, sequence: u64) -> ProviderInput {
    ProviderInput::Audio {
        request,
        privacy_generation: 1,
        sequence,
        read_completed_at_us: 1,
        samples: vec![0; lamp_audio::CAPTURE_SAMPLES],
    }
}
fn end(request: u64) -> ProviderInput {
    ProviderInput::End {
        request,
        privacy_generation: 1,
        sound: false,
        cut_in: false,
    }
}
fn paused(request: u64) -> ProviderInput {
    ProviderInput::Paused {
        request,
        privacy_generation: 1,
    }
}
fn written(request: u64, words: &str) -> ProviderInput {
    ProviderInput::Written {
        request,
        session: 1,
        privacy_generation: 1,
        text: words.into(),
        look: false,
    }
}

/// Every output of the turn, through its last event, as short labels; a
/// run of audio packets is one `audio:<sample>` label. Packets are checked
/// against the coordinator's bound on the way.
async fn turn(engine: &mut Engine) -> Vec<String> {
    turn_within(engine, Duration::from_secs(2)).await
}

/// [`turn`], each event awaited at most `wait`.
async fn turn_within(engine: &mut Engine, wait: Duration) -> Vec<String> {
    let mut labels: Vec<String> = Vec::new();
    loop {
        let event = tokio::time::timeout(wait, engine.next())
            .await
            .expect("the turn ends");
        let (label, last) = match event {
            ProviderOutput::Started { request, .. } => (format!("started:{request}"), false),
            ProviderOutput::Transcript {
                request: None,
                text,
                finished,
            } => {
                assert!(finished);
                (format!("heard:{text}"), false)
            }
            // The words' end (`finished`) and the request's times, each
            // checked by its own test.
            ProviderOutput::Transcript {
                request: Some(_),
                text,
                finished: true,
            } if text.is_empty() => continue,
            ProviderOutput::Stages { .. } => continue,
            ProviderOutput::Transcript {
                request: Some(_),
                text,
                ..
            } => (format!("said:{text}"), false),
            ProviderOutput::Audio {
                samples,
                source_frames,
                source_offset,
                ..
            } => {
                assert!(!samples.is_empty() && samples.len() <= PACKET_SAMPLES);
                assert_eq!((source_frames, source_offset), (samples.len(), 0));
                (format!("audio:{}", samples[0]), false)
            }
            ProviderOutput::GenerationComplete { .. } => ("generated".into(), false),
            ProviderOutput::TurnComplete { idle, .. } => {
                assert!(idle);
                ("complete".into(), true)
            }
            ProviderOutput::TurnFailed { stage, reason, .. } => {
                (format!("failed:{stage:?}:{reason:?}"), true)
            }
            ProviderOutput::ReminderDeferred { reason, .. } => {
                (format!("deferred:{reason:?}"), true)
            }
            ProviderOutput::Unclear {
                request,
                why,
                cut_in,
            } => (format!("unclear:{request}:{why}:{cut_in}"), false),
            ProviderOutput::HoldingAnswer { request, session } => {
                (format!("holding:{request}:{session}"), false)
            }
            ProviderOutput::NewsAnswered { request, key } => {
                (format!("news:{request}:{key}"), false)
            }
            other => panic!("the second route never sends {other:?}"),
        };
        if labels.last() != Some(&label) || !label.starts_with("audio:") {
            labels.push(label);
        }
        if last {
            return labels;
        }
    }
}

const SAID: usize = ANNOUNCEMENT.len();

#[tokio::test]
async fn a_spoken_turn_is_heard_then_said_sentence_by_sentence_in_order() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("What time is it?".to_owned())].into()),
        chats: Mutex::new(
            [vec![
                text("It is "),
                text("noon. Enjoy "),
                text("lunch!"),
                Ok(ChatEvent::Done),
            ]]
            .into(),
        ),
        // The first sentence's speech comes after the second's: still first.
        slow: Some("It is noon."),
        ..Script::default()
    });
    let (mut engine, _, _) = scripted(script.clone());
    engine.submit(start(1));
    for sequence in 1..=3 {
        engine.submit(audio(1, sequence));
    }
    engine.submit(end(1));
    assert_eq!(
        turn(&mut engine).await,
        [
            "started:1".to_owned(),
            "heard:What time is it?".into(),
            format!("audio:{SAID}"),
            "said:It is noon.".into(),
            "audio:11".into(),
            "said: Enjoy lunch!".into(),
            "audio:12".into(),
            "generated".into(),
            "complete".into(),
        ]
    );
    assert_eq!(
        script.samples_heard.load(Ordering::Relaxed),
        3 * lamp_audio::CAPTURE_SAMPLES
    );
    assert_eq!(
        script.asked.lock().unwrap()[0],
        [Message::system("SYSTEM"), Message::user("What time is it?")]
    );
}

#[tokio::test]
async fn a_written_turn_is_answered_without_listening() {
    let script = Arc::new(Script {
        chats: Mutex::new([vec![text("Here you are."), Ok(ChatEvent::Done)]].into()),
        ..Script::default()
    });
    let (mut engine, _, _) = scripted(script.clone());
    engine.submit(written(4, "Say something."));
    assert_eq!(
        turn(&mut engine).await,
        [
            "started:4".to_owned(),
            format!("audio:{SAID}"),
            "said:Here you are.".into(),
            "audio:13".into(),
            "generated".into(),
            "complete".into(),
        ]
    );
    assert_eq!(
        script.asked.lock().unwrap()[0][1],
        Message::user("Say something.")
    );
}

#[tokio::test]
async fn a_tool_call_is_run_and_its_result_answered() {
    let asked = call("c1", "set_timer", json!({"minutes": 5}));
    let script = Arc::new(Script {
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![asked.clone()])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("Timer set."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (mut engine, calls, _) = scripted(script.clone());
    engine.submit(written(1, "Five minute timer."));
    let labels = turn(&mut engine).await;
    assert_eq!(
        labels[labels.len() - 4..],
        ["said:Timer set.", "audio:10", "generated", "complete"]
    );
    assert_eq!(
        *calls.lock().unwrap(),
        [("set_timer".to_owned(), json!({"minutes": 5}), false)]
    );
    let asked_again = &script.asked.lock().unwrap()[1];
    assert_eq!(
        asked_again[2..],
        [
            Message::assistant_calls(None, &[asked]),
            Message::tool("c1", r#"{"ok":true}"#),
        ]
    );
}

/// lamp-4ace, 20 Oct 21:31: "Lamp, what's my name?" was answered with
/// `recall`, then `forget_my_face` and `forget`. A forget of any kind the
/// person's spoken words don't ask for is not run, and the model is told
/// so; one they ask for runs as before. On a turn the lamp wrote itself, no
/// forget runs, whatever its text says (the reviewer's P3 on #2317).
#[tokio::test]
async fn a_forget_nobody_asked_for_is_not_run() {
    let refused = Message::tool(
        "c1",
        json!({"error": crate::remember::NOT_ASKED_TO_FORGET}).to_string(),
    );
    for (name, said, spoken, runs) in [
        (crate::remember::FORGET, "What's my name?", true, false),
        (
            crate::known_faces::FORGET,
            "Lamp, what's my name?",
            true,
            false,
        ),
        (crate::remember::FORGET_EVERYTHING, "Who am I?", true, false),
        (
            crate::remember::FORGET,
            "Please forget my name.",
            true,
            true,
        ),
        (
            crate::known_faces::FORGET,
            "Delete my face, please.",
            true,
            true,
        ),
        (
            crate::remember::FORGET,
            "Clear my inbox when you can.",
            false,
            false,
        ),
    ] {
        let asked = call("c1", name, json!({}));
        let script = Arc::new(Script {
            heard: Mutex::new([Ok(said.to_owned())].into()),
            chats: Mutex::new(
                [
                    vec![
                        Ok(ChatEvent::ToolCalls(vec![asked.clone()])),
                        Ok(ChatEvent::Done),
                    ],
                    vec![text("Okay."), Ok(ChatEvent::Done)],
                ]
                .into(),
            ),
            ..Script::default()
        });
        let (mut engine, calls, _) = scripted(script.clone());
        if spoken {
            engine.submit(start(1));
            engine.submit(audio(1, 1));
            engine.submit(end(1));
        } else {
            engine.submit(written(1, said));
        }
        turn(&mut engine).await;
        assert_eq!(
            calls.lock().unwrap().len(),
            usize::from(runs),
            "{name}: {said}"
        );
        let told = script.asked.lock().unwrap()[1].last().cloned().unwrap();
        assert_eq!(told == refused, !runs, "{name}: {said}");
    }
}

/// Jarvis's P2 on #2331: a yes keyed on the lamp's question's words keeps
/// leaking, since the model writes the question. So for now a yes alone
/// forgets nothing, whatever the lamp asked, and the model is told to ask
/// them to say it; a yes that says it ("Yes, forget everything.") runs.
#[tokio::test]
async fn a_yes_alone_forgets_nothing_and_one_that_says_it_does() {
    for (then, runs) in [("Yes.", false), ("Yes, forget everything.", true)] {
        let everything = call("c1", crate::remember::FORGET_EVERYTHING, json!({}));
        let script = Arc::new(Script {
            heard: Mutex::new(
                [
                    Ok("Forget everything about me.".to_owned()),
                    Ok(then.to_owned()),
                ]
                .into(),
            ),
            chats: Mutex::new(
                [
                    vec![text("Everything, really?"), Ok(ChatEvent::Done)],
                    vec![
                        Ok(ChatEvent::ToolCalls(vec![everything.clone()])),
                        Ok(ChatEvent::Done),
                    ],
                    vec![text("Done."), Ok(ChatEvent::Done)],
                ]
                .into(),
            ),
            ..Script::default()
        });
        let (mut engine, calls, _) = scripted(script);
        engine.submit(start(1));
        engine.submit(audio(1, 1));
        engine.submit(end(1));
        turn(&mut engine).await;
        engine.retire_through(1);
        engine.submit(start(2));
        engine.submit(audio(2, 1));
        engine.submit(end(2));
        turn(&mut engine).await;
        assert_eq!(calls.lock().unwrap().len(), usize::from(runs), "{then}");
    }
}

#[test]
fn only_words_that_ask_to_forget_let_a_forget_run() {
    for asks in [
        "Please forget my name.",
        "Forget it.",
        "Can you delete my face?",
        "Erase that.",
        "stop remembering that",
        "Quên tên tôi đi.",
        "Que\u{302}n tên tôi đi.",
        "Xóa nó.",
        "Scratch that.",
        "Get rid of that, please.",
        "Don't keep that.",
        "Take that off your list.",
    ] {
        assert!(crate::remember::asks_to_forget(asks), "{asks}");
    }
    for not in [
        "What's my name?",
        "Remember my face.",
        "I don't remember.",
        "Forgetful me.",
        "Rất vui được làm quen.",
        "Xoa đầu nó đi.",
        "I keep forgetting my keys.",
        "She forgets everything.",
        "",
    ] {
        assert!(!crate::remember::asks_to_forget(not), "{not}");
    }
}

/// The route A/B of 19 Oct: three hand-overs asked of the second route
/// alone, none handed over. A spoken request naming Claude Code is asked
/// with the Live route's staff note beside its words (`LAMP_STAFF_NOTE=on`,
/// `worker::staff_noting`), `start_agent_task` is among the functions this
/// route declares, and the call the model makes is run through this
/// route's own runner (`worker::tool_runner`, as `serve` builds it) to the
/// tasks part, which prepares the work; the model is told so and asks, and
/// the lamp says it.
#[test]
fn a_hand_over_asked_by_voice_reaches_the_tasks_part_through_this_route() {
    let _one = crate::agent_tasks::tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let directory = std::env::temp_dir().join(format!("second-tasks-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).unwrap();
    let seen = crate::agent_tasks::tests::tasks_part(
        &directory,
        vec![
            (
                "request.sock",
                json!({"ok":true,"store_id":"s1","revision":0,"summary":{"session_id":"sess"}}),
            ),
            (
                "request.sock",
                json!({"ok":true,"id":7,"prepared_digest":"abc","revision":1}),
            ),
        ],
    );
    let place = crate::agent_tasks::Place {
        directory: directory.clone(),
        target: crate::agent_tasks::DEFAULT_TARGET.into(),
        who: false,
        lead_in: false,
        asked: false,
        needs: None,
        staff_note: true,
        accounts: false,
        machine_names: false,
        make_skill: false,
        named_only: false,
    };
    let settings = crate::tools::Settings {
        tasks: Some(place.clone()),
        ..crate::tools::Settings::default()
    };
    assert!(
        crate::tools::declared(&settings)
            .iter()
            .any(|tool| tool.name == crate::agent_tasks::START)
    );
    let task = "Write a two-line poem about the rain";
    let asked = call("c1", crate::agent_tasks::START, json!({ "task": task }));
    let heard = "Hey Lamp, ask Claude Code to write a two-line poem about the rain.";
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![asked.clone()])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("Shall I start it?"), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        worker::tool_runner(settings, None, None, false),
        Arc::new(Instant::now),
    )
    .announcing(false)
    .noting_staff(worker::staff_noting(place));
    let labels = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            engine.submit(start(1));
            engine.submit(audio(1, 1));
            engine.submit(end(1));
            turn(&mut engine).await
        });
    let _ = std::fs::remove_dir_all(&directory);
    assert_eq!(
        labels[labels.len() - 4..],
        [
            "said:Shall I start it?",
            "audio:17",
            "generated",
            "complete"
        ]
    );
    let asked_of = script.asked.lock().unwrap();
    assert_eq!(
        asked_of[0],
        [
            Message::system("SYSTEM"),
            Message::system(crate::staff_note::note(&["Claude Code"])),
            Message::user(heard),
        ]
    );
    let ops: Vec<Value> = seen.try_iter().collect();
    assert_eq!(ops.len(), 2, "{ops:?}");
    assert_eq!(ops[0]["op"], "status");
    assert_eq!(
        (&ops[1]["op"], &ops[1]["text"], &ops[1]["target"]),
        (
            &json!("prepare"),
            &json!(task),
            &json!(crate::agent_tasks::DEFAULT_TARGET)
        )
    );
    let told = asked_of[1]
        .iter()
        .find(|message| message.role == "tool")
        .and_then(|message| message.content.as_ref()?.as_str())
        .and_then(|content| serde_json::from_str::<Value>(content).ok())
        .expect("the model is told what the call did");
    assert_eq!(
        (&told["prepared"], &told["task"]),
        (&json!(true), &json!(task))
    );
}

/// #1390 on the second route (the first builder's 20:15 on #1722): news the
/// person asks about (`check_agent_task`) is put on the turn it was
/// answered in, as `NewsAnswered`, and its key is never the model's. A key
/// in another function's answer is not news: only the tasks part's is.
#[tokio::test]
async fn news_asked_about_rides_on_its_turn_and_its_key_is_never_the_models() {
    let checked = call("c1", crate::agent_tasks::CHECK, json!({}));
    let timer = call("c2", "set_timer", json!({"minutes": 5}));
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("How is that task going?".to_owned())].into()),
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![checked.clone(), timer.clone()])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("Claude Code has finished it."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let tools: ToolRunner = Arc::new(|_, _, name, _| {
        Box::pin(async move {
            if name == crate::agent_tasks::CHECK {
                json!({"answer": "done", crate::agent_tasks::NEWS_KEY: "s1:2:4"})
            } else {
                json!({"ok": true, crate::agent_tasks::NEWS_KEY: "not-news"})
            }
        })
    });
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        tools,
        Arc::new(Instant::now),
    )
    .announcing(false);
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    let labels = turn(&mut engine).await;
    assert_eq!(
        labels
            .iter()
            .filter(|label| label.starts_with("news:"))
            .collect::<Vec<_>>(),
        ["news:1:s1:2:4"],
        "{labels:?}"
    );
    assert_eq!(
        labels[labels.len() - 4..],
        [
            "said:Claude Code has finished it.",
            "audio:28",
            "generated",
            "complete"
        ]
    );
    let asked_again = &script.asked.lock().unwrap()[1];
    assert_eq!(
        asked_again[asked_again.len() - 2..],
        [
            Message::tool("c1", r#"{"answer":"done"}"#),
            Message::tool("c2", r#"{"news_key":"not-news","ok":true}"#),
        ]
    );
}

/// The same through this route's own runner (`worker::tool_runner`, as
/// `serve` builds it) to the tasks part: a finished task's news asked about
/// by voice is answered to the model without its key and put on the turn.
#[test]
fn news_asked_by_voice_through_this_route_is_put_on_the_turn() {
    let _one = crate::agent_tasks::tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let directory = std::env::temp_dir().join(format!("second-news-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).unwrap();
    let (place, _seen) =
        crate::agent_tasks::tests::serving(&directory, "idle", "Lumi, Sunny and Archie.", false);
    let settings = crate::tools::Settings {
        tasks: Some(place),
        ..crate::tools::Settings::default()
    };
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("How is that story going?".to_owned())].into()),
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c1",
                        crate::agent_tasks::CHECK,
                        json!({}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("It's done."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        worker::tool_runner(settings, None, None, false),
        Arc::new(Instant::now),
    )
    .announcing(false);
    let labels = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            engine.submit(start(1));
            engine.submit(audio(1, 1));
            engine.submit(end(1));
            turn(&mut engine).await
        });
    let _ = std::fs::remove_dir_all(&directory);
    assert!(labels.contains(&"news:1:s1:2:4".to_owned()), "{labels:?}");
    let told = script.asked.lock().unwrap()[1]
        .iter()
        .find(|message| message.role == "tool")
        .and_then(|message| message.content.as_ref()?.as_str())
        .and_then(|content| serde_json::from_str::<Value>(content).ok())
        .expect("the model is told the news");
    assert!(told.get(crate::agent_tasks::NEWS_KEY).is_none(), "{told}");
    assert_eq!(told["answer"], "Lumi, Sunny and Archie.");
}

/// `LAMP_SECOND_SAID_ONCE` (the first builder at 06:56 on #1722): on
/// lamp-4ace "Set a timer for one minute." got "A minute, starting now. One
/// minute, counting from now". With the switch, a round that said words
/// before its call tells the next round, after the call's result, that they
/// were heard; one that said nothing before it, or without the switch,
/// tells nothing.
#[test]
fn words_said_before_a_call_are_told_to_the_next_round_with_the_switch() {
    for (on, before, noted) in [
        (true, Some("A minute, starting now."), true),
        (false, Some("A minute, starting now."), false),
        (true, None, false),
    ] {
        let mut first: Vec<net::Result<ChatEvent>> = before.map(text).into_iter().collect();
        first.push(Ok(ChatEvent::ToolCalls(vec![call(
            "c1",
            "set_timer",
            json!({"seconds": 60}),
        )])));
        first.push(Ok(ChatEvent::Done));
        let script = Arc::new(Script {
            heard: Mutex::new([Ok("Set a timer for one minute.".to_owned())].into()),
            chats: Mutex::new(
                [
                    first,
                    vec![text("It goes off at six forty-five."), Ok(ChatEvent::Done)],
                ]
                .into(),
            ),
            ..Script::default()
        });
        let (engine, calls, _) = scripted(script.clone());
        let mut engine = engine.announcing(false).saying_once(on);
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                engine.submit(start(1));
                engine.submit(audio(1, 1));
                engine.submit(end(1));
                turn(&mut engine).await
            });
        assert_eq!(calls.lock().unwrap().len(), 1, "{on} {before:?}");
        let asked = script.asked.lock().unwrap();
        assert_eq!(asked.len(), 2, "{on} {before:?}");
        let second = &asked[1];
        let note = before.map(super::said_once_note);
        // After the call's result, last before the round is asked.
        let result_at = second.len() - 1 - usize::from(noted);
        assert_eq!(second[result_at].role, "tool", "{on} {before:?}");
        let last = second.last().unwrap();
        assert_eq!(
            note.is_some_and(|note| *last == Message::system(note)),
            noted,
            "{on} {before:?}"
        );
        assert_eq!(
            second
                .iter()
                .filter(|message| message
                    .content
                    .as_ref()
                    .and_then(|content| content.as_str())
                    .is_some_and(|content| content.contains("already said aloud")))
                .count(),
            usize::from(noted),
            "{on} {before:?}"
        );
    }
}

#[tokio::test]
async fn not_for_me_ends_the_turn_in_silence() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("No, the other one".to_owned())].into()),
        chats: Mutex::new(
            [vec![
                text("Sure"),
                Ok(ChatEvent::ToolCalls(vec![call(
                    "c1",
                    NOT_FOR_ME,
                    json!({}),
                )])),
                Ok(ChatEvent::Done),
            ]]
            .into(),
        ),
        ..Script::default()
    });
    let (mut engine, calls, _) = scripted(script);
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    assert_eq!(
        turn(&mut engine).await,
        ["started:1", "heard:No, the other one", "complete"]
    );
    assert!(calls.lock().unwrap().is_empty());
}

/// #2191, `LAMP_NAMED_ALWAYS=on`: in a turn the hook says began with the
/// lamp's name, said to a face turned to the lamp, the model's `not_for_me`
/// is not taken: it is asked once more, told it was called, and its answer is
/// said. A second decline ends the turn in silence; without the hook, or
/// for a turn it does not name, the first does, as before.
#[tokio::test]
async fn a_decline_of_a_line_named_to_a_face_is_not_taken_once() {
    let decline = || {
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "c1",
                NOT_FOR_ME,
                json!({}),
            )])),
            Ok(ChatEvent::Done),
        ]
    };
    let answer = || vec![text("It is noon."), Ok(ChatEvent::Done)];
    let cases = [
        ("named", Some(true), vec![decline(), answer()], true, 2),
        (
            "named twice",
            Some(true),
            vec![decline(), decline()],
            false,
            2,
        ),
        (
            "not named",
            Some(false),
            vec![decline(), answer()],
            false,
            1,
        ),
        ("no switch", None, vec![decline(), answer()], false, 1),
    ];
    for (case, named, rounds, said, asked) in cases {
        let script = Arc::new(Script {
            heard: Mutex::new([Ok("Lamp, what time is it?".to_owned())].into()),
            chats: Mutex::new(rounds.into()),
            ..Script::default()
        });
        let (engine, calls, _) = scripted(script.clone());
        let mut engine = match named {
            Some(named) => engine.answering_named_to_a_face(Arc::new(move |_| named)),
            None => engine,
        };
        engine.submit(start(1));
        engine.submit(audio(1, 1));
        engine.submit(end(1));
        let out = turn(&mut engine).await;
        assert_eq!(out.last().map(String::as_str), Some("complete"), "{case}");
        assert_eq!(
            out.iter().any(|o| o == "said:It is noon."),
            said,
            "{case}: {out:?}"
        );
        assert!(calls.lock().unwrap().is_empty(), "{case}");
        let asked_rounds = script.asked.lock().unwrap();
        assert_eq!(asked_rounds.len(), asked, "{case}");
        if asked == 2 {
            assert!(
                asked_rounds[1].contains(&Message::tool(
                    "c1".to_owned(),
                    NAMED_TO_A_FACE_ANSWER.to_owned()
                )),
                "{case}"
            );
        }
    }
}

/// Words the heed judges not said to the lamp end the turn after them,
/// with no chat asked; it is given the microphone's times of the line's
/// first and last packets (#1014).
/// The first builder's 22:54 on #1722, the holding answers on the second
/// route (`LAMP_HOLDING_ANSWER=on`): a spoken turn answered only "One
/// moment." sends `HoldingAnswer` for its request and session before its
/// idle `TurnComplete`, as the Live worker does, so the coordinator asks it
/// to go on. Not with more words, not after any call, not for the lamp's own
/// written turn, and never without the switch.
#[tokio::test]
async fn a_holding_only_answer_is_told_so_the_route_is_asked_to_go_on() {
    let timer = call("c1", "set_timer", json!({"minutes": 5}));
    /// Each chat round's events.
    type Rounds = Vec<Vec<net::Result<ChatEvent>>>;
    let cases: [(&str, bool, Rounds, bool); 5] = [
        (
            "holding",
            true,
            vec![vec![text("One moment."), Ok(ChatEvent::Done)]],
            true,
        ),
        (
            "more words",
            true,
            vec![vec![text("One moment. It's sunny."), Ok(ChatEvent::Done)]],
            false,
        ),
        (
            "after a call",
            true,
            vec![
                vec![
                    Ok(ChatEvent::ToolCalls(vec![timer.clone()])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("Just a moment."), Ok(ChatEvent::Done)],
            ],
            false,
        ),
        (
            "written",
            true,
            vec![vec![text("One moment."), Ok(ChatEvent::Done)]],
            false,
        ),
        (
            "switch off",
            false,
            vec![vec![text("One moment."), Ok(ChatEvent::Done)]],
            false,
        ),
    ];
    for (case, on, chats, holds) in cases {
        let script = Arc::new(Script {
            heard: Mutex::new([Ok("What's the weather like?".to_owned())].into()),
            chats: Mutex::new(chats.into()),
            ..Script::default()
        });
        let (engine, _, _) = scripted(script);
        let mut engine = engine.announcing(false).holding_answers(on);
        if case == "written" {
            engine.submit(written(1, "Tell them the weather."));
        } else {
            engine.submit(start(1));
            engine.submit(audio(1, 1));
            engine.submit(end(1));
        }
        let labels = turn(&mut engine).await;
        let held: Vec<&String> = labels
            .iter()
            .filter(|label| label.starts_with("holding:"))
            .collect();
        if holds {
            assert_eq!(held, ["holding:1:1"], "{case}: {labels:?}");
            assert_eq!(
                labels[labels.len() - 2..],
                ["holding:1:1", "complete"],
                "{case}: just before its completion"
            );
        } else {
            assert!(held.is_empty(), "{case}: {labels:?}");
        }
    }

    // "One moment." and a look by its call (the peer read's P2 on #1986): the
    // look is a call, so the coordinator takes the look and is never also
    // asked to go on; two answers to one question otherwise.
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Lamp, what do you see?".to_owned())].into()),
        chats: Mutex::new(
            [vec![
                text("One moment."),
                Ok(ChatEvent::ToolCalls(vec![call("c1", LOOK_CALL, json!({}))])),
                Ok(ChatEvent::Done),
            ]]
            .into(),
        ),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script);
    let mut engine = engine
        .announcing(false)
        .holding_answers(true)
        .looking_by_call();
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    let (mut looked, mut holding) = (false, false);
    loop {
        match tokio::time::timeout(Duration::from_secs(2), engine.next())
            .await
            .expect("the turn ends")
        {
            ProviderOutput::LookAsked { dropped: None, .. } => looked = true,
            ProviderOutput::HoldingAnswer { .. } => holding = true,
            ProviderOutput::TurnComplete { .. } => break,
            _ => {}
        }
    }
    assert!(looked, "the look is asked");
    assert!(
        !holding,
        "a look by its call is a call: no HoldingAnswer beside it"
    );
}

/// A link whose hosts answer again `after` from its first ask, on the
/// runtime's clock, and how many times it was asked.
fn link_back_after(after: Duration) -> (LinkBack, Arc<AtomicU64>) {
    let first: Arc<Mutex<Option<tokio::time::Instant>>> = Arc::default();
    let asks = Arc::new(AtomicU64::new(0));
    let counted = asks.clone();
    let back: LinkBack = Arc::new(move || {
        counted.fetch_add(1, Ordering::Relaxed);
        let at = *first
            .lock()
            .unwrap()
            .get_or_insert_with(tokio::time::Instant::now);
        Box::pin(async move { at.elapsed() >= after })
    });
    (back, asks)
}

/// `LAMP_RESEND_LOST_LINE` on the second route (the first builder's 00:02
/// on #1722), on the paused clock: a spoken line whose chat was lost on the
/// network before any word is asked once more when the link is back at
/// 2 s, and answered; back only at 6 s, past `resend::RESEND_WITHIN_US`, it
/// fails as before. Never twice: lost again after its ask again, it fails.
#[tokio::test(start_paused = true)]
async fn a_line_lost_on_the_network_is_asked_once_more_when_the_link_is_back() {
    let lost = || vec![Err(net::Error::Transport("chat connect"))];
    let answered = || vec![text("It's sunny."), Ok(ChatEvent::Done)];
    let cases = [
        (
            "back at 2 s",
            Duration::from_secs(2),
            vec![lost(), answered()],
            true,
            2,
        ),
        (
            "back at 6 s",
            Duration::from_secs(6),
            vec![lost(), answered()],
            false,
            1,
        ),
        (
            "lost twice",
            Duration::from_secs(1),
            vec![lost(), lost(), answered()],
            false,
            2,
        ),
    ];
    for (case, after, chats, answers, asked) in cases {
        let script = Arc::new(Script {
            heard: Mutex::new([Ok("What's the weather like?".to_owned())].into()),
            chats: Mutex::new(chats.into()),
            ..Script::default()
        });
        let (engine, _, _) = scripted(script.clone());
        let (back, _) = link_back_after(after);
        let mut engine = engine.announcing(false).resending(back);
        let began = tokio::time::Instant::now();
        engine.submit(start(1));
        engine.submit(audio(1, 1));
        engine.submit(end(1));
        let labels = turn_within(&mut engine, Duration::from_secs(10)).await;
        let last = labels.last().unwrap().as_str();
        if answers {
            assert!(
                labels.contains(&"said:It's sunny.".to_owned()),
                "{case}: {labels:?}"
            );
            assert_eq!(last, "complete", "{case}");
            assert!(began.elapsed() >= after, "{case}: asked again once back");
        } else {
            assert_eq!(
                last, "failed:AwaitingResponse:Transport",
                "{case}: {labels:?}"
            );
            // Nothing said while it waited and gave up: no second sorry
            // (the peer read's P3 on #2001).
            assert!(
                !labels
                    .iter()
                    .any(|label| label.starts_with("said:") || label.starts_with("audio:")),
                "{case}: {labels:?}"
            );
            assert!(
                began.elapsed()
                    <= Duration::from_micros(crate::resend::RESEND_WITHIN_US)
                        + Duration::from_secs(1),
                "{case}: given up by 5 s"
            );
        }
        assert_eq!(script.asked.lock().unwrap().len(), asked, "{case}");
    }
}

/// Not asked again: after any word was said, after a call ran (it may have
/// done something), for a refusal (a status is no network loss), for the
/// lamp's own written turn, without the switch, or once a newer line began.
#[tokio::test(start_paused = true)]
async fn a_line_is_not_asked_again_after_words_a_call_a_refusal_or_a_newer_line() {
    /// Each chat round's events.
    type Rounds = Vec<Vec<net::Result<ChatEvent>>>;
    let timer = call("c1", "set_timer", json!({"minutes": 5}));
    let lost = || Err(net::Error::Transport("chat read"));
    let cases: [(&str, Rounds, bool, bool, &str); 5] = [
        (
            "a word said",
            vec![vec![text("Well, "), lost()]],
            true,
            false,
            "failed:AwaitingResponse:Transport",
        ),
        (
            "a call ran",
            vec![
                vec![
                    Ok(ChatEvent::ToolCalls(vec![timer.clone()])),
                    Ok(ChatEvent::Done),
                ],
                vec![lost()],
            ],
            true,
            false,
            "failed:AwaitingResponse:Transport",
        ),
        (
            "a refusal",
            vec![vec![Err(net::Error::Status(429))]],
            true,
            false,
            "failed:AwaitingResponse:RateLimited",
        ),
        (
            "written",
            vec![vec![lost()]],
            true,
            true,
            "failed:AwaitingResponse:Transport",
        ),
        (
            "switch off",
            vec![vec![lost()]],
            false,
            false,
            "failed:AwaitingResponse:Transport",
        ),
    ];
    for (case, chats, on, written_turn, ends) in cases {
        let rounds = chats.len();
        let script = Arc::new(Script {
            heard: Mutex::new([Ok("Set a timer, please.".to_owned())].into()),
            chats: Mutex::new(chats.into()),
            ..Script::default()
        });
        let (engine, _, _) = scripted(script.clone());
        let (back, asks) = link_back_after(Duration::ZERO);
        let mut engine = engine.announcing(false);
        if on {
            engine = engine.resending(back);
        }
        if written_turn {
            engine.submit(written(1, "Tell them the time."));
        } else {
            engine.submit(start(1));
            engine.submit(audio(1, 1));
            engine.submit(end(1));
        }
        let labels = turn(&mut engine).await;
        assert_eq!(labels.last().unwrap(), ends, "{case}: {labels:?}");
        assert_eq!(
            script.asked.lock().unwrap().len(),
            rounds,
            "{case}: no ask again"
        );
        assert_eq!(
            asks.load(Ordering::Relaxed),
            0,
            "{case}: the link not even asked"
        );
    }

    // A newer line begun while the lost one waits for the link: it replaces
    // the lost one's turn (`Engine::begin`), which is never asked again on
    // its own; the newer line is answered at once, not once the link is back.
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok("What's the weather like?".to_owned()),
                Ok("Never mind.".to_owned()),
            ]
            .into(),
        ),
        chats: Mutex::new(
            [
                vec![Err(net::Error::Transport("chat connect"))],
                vec![text("Okay."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script.clone());
    let (back, _) = link_back_after(Duration::from_secs(3));
    let mut engine = engine.announcing(false).resending(back);
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    let started = tokio::time::Instant::now();
    tokio::time::sleep(Duration::from_secs(1)).await;
    engine.submit(start(2));
    engine.submit(audio(2, 1));
    engine.submit(end(2));
    let mut seen = Vec::new();
    loop {
        let event = tokio::time::timeout(Duration::from_secs(10), engine.next())
            .await
            .expect("the newer line is answered");
        match event {
            ProviderOutput::TurnComplete { request: 2, .. } => break,
            ProviderOutput::TurnComplete { request, .. }
            | ProviderOutput::TurnFailed { request, .. } => seen.push(request),
            _ => {}
        }
    }
    assert!(seen.is_empty(), "request 1 ended on its own: {seen:?}");
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "answered as it came, not once the link was back"
    );
    assert_eq!(
        script.asked.lock().unwrap().len(),
        2,
        "the lost line and the newer one, nothing asked again"
    );

    // A mute (a privacy close) 1 s into the wait: the coordinator retires the
    // turn, as for any mute (`retire_through`), which ends its task with the
    // wait in it; the link back at 2 s asks nothing (the peer read's P3 on
    // #2001; Live's resend holds this by the privacy generation).
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("What's the weather like?".to_owned())].into()),
        chats: Mutex::new(
            [
                vec![Err(net::Error::Transport("chat connect"))],
                vec![text("It's sunny."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script.clone());
    let (back, _) = link_back_after(Duration::from_secs(2));
    let mut engine = engine.announcing(false).resending(back);
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    let started = tokio::time::Instant::now();
    while started.elapsed() < Duration::from_secs(1) {
        let _ = tokio::time::timeout(Duration::from_millis(100), engine.next()).await;
    }
    engine.retire_through(1);
    while started.elapsed() < Duration::from_secs(4) {
        if let Ok(event) = tokio::time::timeout(Duration::from_millis(100), engine.next()).await {
            assert!(
                !matches!(
                    event,
                    ProviderOutput::Audio { .. } | ProviderOutput::TurnComplete { .. }
                ),
                "nothing of the retired turn: {event:?}"
            );
        }
    }
    assert_eq!(
        script.asked.lock().unwrap().len(),
        1,
        "the link came back after the mute, and nothing was asked again"
    );
}

#[tokio::test]
async fn words_the_heed_holds_end_the_turn_unanswered() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Me.".to_owned())].into()),
        ..Script::default()
    });
    let (engine, calls, _) = scripted(script.clone());
    type Judged = Vec<(u64, (u64, u64), String)>;
    let judged: Arc<Mutex<Judged>> = Arc::default();
    let seen = judged.clone();
    let mut engine = engine.heeding(Arc::new(move |request, said, heard, _| {
        seen.lock().unwrap().push((request, said, heard.to_owned()));
        Some("a busy room")
    }));
    engine.submit(start(1));
    for (sequence, at_us) in [(1, 7_000), (2, 7_010), (3, 7_020)] {
        let ProviderInput::Audio { samples, .. } = audio(1, sequence) else {
            unreachable!()
        };
        engine.submit(ProviderInput::Audio {
            request: 1,
            privacy_generation: 1,
            sequence,
            read_completed_at_us: at_us,
            samples,
        });
    }
    engine.submit(end(1));
    assert_eq!(
        turn(&mut engine).await,
        ["started:1", "heard:Me.", "complete"]
    );
    assert_eq!(
        *judged.lock().unwrap(),
        [(1, (7_000, 7_020), "Me.".to_owned())]
    );
    assert!(script.asked.lock().unwrap().is_empty());
    assert!(calls.lock().unwrap().is_empty());
}

/// Each judged turn's verdict, as the engine tells it (#1155).
type TurnsTold = Arc<Mutex<Vec<(u64, lamp_msgs::TurnVerdict)>>>;

fn telling(engine: Engine) -> (Engine, TurnsTold) {
    let told: TurnsTold = Arc::default();
    let seen = told.clone();
    let engine = engine.telling_verdicts(Arc::new(move |request, verdict| {
        seen.lock().unwrap().push((request, verdict));
    }));
    (engine, told)
}

/// #1155: a turn the heed holds is told held back, for the Policy's hello,
/// as the journal says it.
#[tokio::test]
async fn a_turn_the_heed_holds_is_told_held_back() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Me.".to_owned())].into()),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script);
    let (mut engine, told) = telling(engine.heeding(Arc::new(|_, _, _, _| Some("a busy room"))));
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    assert_eq!(
        turn(&mut engine).await,
        ["started:1", "heard:Me.", "complete"]
    );
    assert_eq!(
        *told.lock().unwrap(),
        [(1, lamp_msgs::TurnVerdict::HeldBack)]
    );
}

/// The first builder on #1817: a line the model itself declines
/// (`not_for_me`, called or written as words) is told held back, with no
/// heed too, as a line the rule holds is.
#[tokio::test]
async fn a_line_the_model_declines_is_told_held_back() {
    for declined in [
        Ok(ChatEvent::ToolCalls(vec![call(
            "c1",
            NOT_FOR_ME,
            json!({}),
        )])),
        text("not_for_me{}"),
    ] {
        let script = Arc::new(Script {
            heard: Mutex::new([Ok("No, the other one".to_owned())].into()),
            chats: Mutex::new([vec![declined, Ok(ChatEvent::Done)]].into()),
            ..Script::default()
        });
        let (engine, _, _) = scripted(script);
        let (mut engine, told) = telling(engine);
        engine.submit(start(1));
        engine.submit(audio(1, 1));
        engine.submit(end(1));
        let labels = turn(&mut engine).await;
        assert_eq!(
            labels.last().map(String::as_str),
            Some("complete"),
            "{labels:?}"
        );
        assert_eq!(
            *told.lock().unwrap(),
            [(1, lamp_msgs::TurnVerdict::HeldBack)]
        );
    }
}

/// A line declined after a tool ran for it is the lamp's: nothing is told.
#[tokio::test]
async fn a_line_declined_after_a_tool_ran_is_not_told_held_back() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Set a timer for five minutes.".to_owned())].into()),
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c1",
                        "set_timer",
                        json!({"minutes": 5}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c2",
                        NOT_FOR_ME,
                        json!({}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (engine, calls, _) = scripted(script);
    let (mut engine, told) = telling(engine);
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    let labels = turn(&mut engine).await;
    assert_eq!(
        labels.last().map(String::as_str),
        Some("complete"),
        "{labels:?}"
    );
    assert_eq!(calls.lock().unwrap().len(), 1);
    assert!(
        told.lock().unwrap().is_empty(),
        "{:?}",
        told.lock().unwrap()
    );
}

/// Second on #1782: the verdict is told before the turn's words leave the
/// engine. When it is told, the turn's start, its words and its end are all
/// still waiting to be taken, so the policy can hear the verdict before the
/// words, and keeps it for them (`lamp_policy::hello::Hello::verdict`).
#[tokio::test]
async fn a_held_verdict_is_told_before_its_words_are_taken() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Me.".to_owned())].into()),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script);
    let (said, told) = tokio::sync::oneshot::channel();
    let said = Mutex::new(Some(said));
    let mut engine = engine
        .heeding(Arc::new(|_, _, _, _| Some("a busy room")))
        .telling_verdicts(Arc::new(move |request, verdict| {
            if let Some(said) = said.lock().unwrap().take() {
                let _ = said.send((request, verdict));
            }
        }));
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    let verdict = tokio::time::timeout(Duration::from_secs(2), told)
        .await
        .expect("the verdict is told with nothing taken")
        .unwrap();
    assert_eq!(verdict, (1, lamp_msgs::TurnVerdict::HeldBack));
    assert_eq!(
        turn(&mut engine).await,
        ["started:1", "heard:Me.", "complete"]
    );
}

/// #1155: a turn the heed lets out is told answered. With nothing that
/// judges turns, as on a route with the rule off, no verdict is told, nor
/// for a turn with nothing heard: the hello then counts the turn, as it
/// did before.
#[tokio::test]
async fn a_turn_the_heed_lets_out_is_told_answered_and_one_not_judged_is_not_told() {
    for heed in [true, false] {
        let script = Arc::new(Script {
            heard: Mutex::new([Ok("What time is it?".to_owned()), Ok(String::new())].into()),
            chats: Mutex::new([vec![text("Noon."), Ok(ChatEvent::Done)]].into()),
            ..Script::default()
        });
        let (engine, _, _) = scripted(script);
        let engine = if heed {
            engine.heeding(Arc::new(|_, _, _, _| None))
        } else {
            engine
        };
        let (mut engine, told) = telling(engine);
        for request in [1, 2] {
            engine.submit(start(request));
            engine.submit(audio(request, 1));
            engine.submit(end(request));
            let labels = turn(&mut engine).await;
            assert_eq!(
                labels.last().map(String::as_str),
                Some("complete"),
                "{labels:?}"
            );
        }
        let answered = [(1, lamp_msgs::TurnVerdict::Answered)];
        let expected: &[_] = if heed { &answered } else { &[] };
        assert_eq!(*told.lock().unwrap(), expected, "with a heed: {heed}");
    }
}

/// The first builder's 21:51 on #1722, the hums on the second route: a line
/// the rule held as unclear is told to the coordinator as `Unclear`, before
/// the turn completes and with no chat asked, as the Live worker tells it;
/// a cut-in is told to the rule before it judges. Hummed for as a misheard
/// cut-in, the next request is asked with the note that the lamp did not
/// catch it (`misheard_hum::NOTE`), once.
#[tokio::test]
async fn a_line_held_as_unclear_is_told_and_a_misheard_cut_in_is_noted_next() {
    use crate::not_speech::CUT_IN_FRAGMENT;
    let misheard = "Venga, nos vemos luego.";
    let again = "Can you say that again?";
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok(misheard.to_owned()),
                Ok(again.to_owned()),
                Ok(again.to_owned()),
            ]
            .into(),
        ),
        chats: Mutex::new(
            [
                vec![text("Of course."), Ok(ChatEvent::Done)],
                vec![text("Sure."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script.clone());
    let cut_ins: Arc<Mutex<Vec<u64>>> = Arc::default();
    let told = cut_ins.clone();
    let mut engine = engine
        .announcing(false)
        .heeding(Arc::new(|request, _, _, _| {
            (request == 1).then_some(CUT_IN_FRAGMENT)
        }))
        .asking_again(
            Arc::new(move |request| told.lock().unwrap().push(request)),
            Arc::new(|request| (request == 1).then_some((CUT_IN_FRAGMENT, true))),
        );
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(ProviderInput::End {
        request: 1,
        privacy_generation: 1,
        sound: false,
        cut_in: true,
    });
    assert_eq!(
        turn(&mut engine).await,
        [
            "started:1".to_owned(),
            format!("heard:{misheard}"),
            format!("unclear:1:{CUT_IN_FRAGMENT}:true"),
            "complete".to_owned()
        ]
    );
    assert_eq!(*cut_ins.lock().unwrap(), [1]);
    assert!(script.asked.lock().unwrap().is_empty(), "no chat for it");
    for request in [2, 3] {
        engine.submit(start(request));
        engine.submit(audio(request, 1));
        engine.submit(end(request));
        let labels = turn(&mut engine).await;
        assert!(
            !labels.iter().any(|label| label.starts_with("unclear:")),
            "{labels:?}"
        );
    }
    let asked = script.asked.lock().unwrap();
    let note = Message::system(crate::misheard_hum::NOTE);
    assert_eq!(
        asked[0][asked[0].len() - 2..],
        [note.clone(), Message::user(again)],
        "the next request is told"
    );
    assert!(!asked[1].contains(&note), "once");
    assert_eq!(*cut_ins.lock().unwrap(), [1]);
}

/// A chat that has not begun within `BEGIN_WITHIN` of the words is given
/// up: the turn ends unanswered, nothing said, not even the announcement.
#[tokio::test(start_paused = true)]
async fn a_chat_not_begun_within_three_seconds_ends_the_turn_unanswered() {
    let script = Arc::new(Script {
        chats: Mutex::new([vec![]].into()),
        hold_chat: true,
        ..Script::default()
    });
    let (mut engine, calls, _) = scripted(script.clone());
    engine.submit(written(1, "What time is it?"));
    let begun = tokio::time::Instant::now();
    let mut seen = Vec::new();
    loop {
        match engine.next().await {
            ProviderOutput::Started { .. } => seen.push("started"),
            // Its times: nothing reached.
            ProviderOutput::Stages {
                chat_ms: None,
                audio_ms: None,
                ..
            } => seen.push("stages"),
            ProviderOutput::TurnComplete { .. } => {
                seen.push("complete");
                break;
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(seen, ["started", "stages", "complete"]);
    assert!(begun.elapsed() >= BEGIN_WITHIN, "{:?}", begun.elapsed());
    assert!(calls.lock().unwrap().is_empty());
}

/// #1712, the first builder at 16:04: a spoken line whose chat the route
/// gave up, not begun within `BEGIN_WITHIN`, is no talk the lamp left
/// alone. Two of them don't end the conversation, and the person's next
/// line without the name is still carried in a busy room. Two lines the
/// model leaves alone (`not_for_me`) still end it.
#[tokio::test(start_paused = true)]
async fn lines_the_route_gave_up_are_no_talk_past_the_lamp() {
    let silent = || {
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "c1",
                NOT_FOR_ME,
                json!({}),
            )])),
            Ok(ChatEvent::Done),
        ]
    };
    for (given_up, past) in [(true, false), (false, true)] {
        crate::room_told::room()
            .lock()
            .unwrap()
            .count_only_talk(true);
        let script = Arc::new(Script {
            heard: Mutex::new(
                [
                    Ok("Hey Lamp, what's the weather in Hanoi today?".to_owned()),
                    Ok("And what's the price of gold?".to_owned()),
                ]
                .into(),
            ),
            chats: Mutex::new(if given_up {
                [vec![], vec![]].into()
            } else {
                [silent(), silent()].into()
            }),
            hold_chat: given_up,
            ..Script::default()
        });
        let (mut engine, _, _) = scripted(script);
        for request in 1..=2 {
            engine.submit(start(request));
            engine.submit(audio(request, 1));
            engine.submit(end(request));
            // Past `BEGIN_WITHIN`, which `turn`'s own wait would not see.
            loop {
                let event = tokio::time::timeout(BEGIN_WITHIN * 2, engine.next())
                    .await
                    .expect("the turn ends");
                assert!(
                    !matches!(event, ProviderOutput::Audio { .. }),
                    "nothing is said"
                );
                if matches!(event, ProviderOutput::TurnComplete { .. }) {
                    break;
                }
            }
        }
        let room = crate::room_told::room();
        let mut room = room.lock().unwrap();
        assert_eq!(room.talked_past_the_lamp(), past, "given up: {given_up}");
        assert_eq!(room.left_alone_since_spoke(), if past { 2 } else { 0 });
        *room = crate::room_told::RoomTold::new();
    }
}

/// #1712, the first builder at 16:43: the owner's ten lines at the desk on
/// lamp-4ace (15:56), as heard, five with the lamp's name and five without,
/// each said before the lamp's answer to the one before was out. In a busy
/// room every one is let out: the five without the name as the
/// conversation going on from the owner's last call, though the lamp never
/// got a word out. On the lamp, four of the five were held.
#[tokio::test(start_paused = true)]
async fn the_owners_ten_lines_are_let_out_though_each_answer_was_cut_by_the_next() {
    let lines = [
        "Lamp, what time is this?",
        "Hey, Lamp. Watch the weather in Hanoi today.",
        "Set a timer for five minutes.",
        "What's the price of gold?",
        "Lamp, make the lights a bit warmer.",
        "Tell me a short joke.",
        "Lamp, who won the last Formula one race.",
        "Remind me to call the bank as 4.",
        "Lamp. How are you today?",
        "Thank you. Lynn.",
    ];
    *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
    {
        let mut room = crate::room_told::room().lock().unwrap();
        *room = crate::room_told::RoomTold::new();
        room.count_only_talk(true);
        // Three lines of the room's talk left alone a moment ago: busy.
        for request in 201..=203 {
            room.unanswered(request, lamp_ipc::monotonic_us());
        }
        assert_eq!(
            room.at(lamp_ipc::monotonic_us()),
            crate::room_told::Room::Busy
        );
    }
    let script = Arc::new(Script {
        heard: Mutex::new(lines.iter().map(|line| Ok(line.to_string())).collect()),
        // Every answer still to come when the next line begins.
        chats: Mutex::new(lines.iter().map(|_| vec![]).collect()),
        hold_chat: true,
        ..Script::default()
    });
    let (engine, _, _) = scripted(script);
    let heeding = Arc::new(Mutex::new(super::worker::Heeding::new(
        true, true, None, None,
    )));
    heeding
        .lock()
        .unwrap()
        .as_the_only_voice(false, true, false);
    type Verdicts = Arc<Mutex<Vec<(u64, Option<&'static str>)>>>;
    let verdicts: Verdicts = Arc::default();
    let (judge, kept) = (heeding.clone(), verdicts.clone());
    let mut engine = engine.heeding(Arc::new(move |request, said, heard, from| {
        let verdict = judge
            .lock()
            .ok()
            .and_then(|mut heeding| heeding.heed(request, said, heard, from));
        kept.lock().unwrap().push((request, verdict));
        verdict
    }));
    for request in 207..207 + lines.len() as u64 {
        engine.submit(start(request));
        engine.submit(audio(request, 1));
        engine.submit(end(request));
        // A second on: its words are judged, and its answer has not begun.
        while let Ok(event) = tokio::time::timeout(Duration::from_secs(1), engine.next()).await {
            super::worker::keep(&event, None);
            assert!(
                !matches!(event, ProviderOutput::Audio { .. }),
                "nothing is said"
            );
        }
    }
    let verdicts = verdicts.lock().unwrap();
    assert_eq!(verdicts.len(), lines.len());
    for (request, verdict) in verdicts.iter() {
        assert_eq!(*verdict, None, "request {request} was held");
    }
    *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
}

/// #1712 (`LAMP_SECOND_PREPARED`, the first builder at 17:02): with the
/// switch, a spoken turn's chat is begun on the line's early words, so its
/// first words come as the final words do, not a chat's wait after them.
/// Final words that are the early ones keep it, asked once; other words drop
/// it and ask again; without the switch nothing is begun early.
#[tokio::test(start_paused = true)]
async fn a_chat_begun_on_the_early_words_is_kept_when_they_are_the_lines() {
    for (prepare, early, final_words, asked, first_audio_ms) in [
        // The chat's 600 ms run beside `Finalize`'s 400 ms.
        (true, "what time is it", "What time is it?", 1, 600),
        (true, "what time", "What time is it?", 2, 1_000),
        (false, "what time is it", "What time is it?", 1, 1_000),
    ] {
        let script = Arc::new(Script {
            heard: Mutex::new([Ok(final_words.to_owned())].into()),
            early: Mutex::new([vec![net::Early::Words(early.to_owned())]].into()),
            finish_after: Some(Duration::from_millis(400)),
            first_event_after: Some(Duration::from_millis(600)),
            chats: Mutex::new(
                [
                    vec![text("It's noon."), Ok(ChatEvent::Done)],
                    vec![text("It's noon."), Ok(ChatEvent::Done)],
                ]
                .into(),
            ),
            ..Script::default()
        });
        let (engine, _, _) = scripted(script.clone());
        let mut engine = engine.preparing(prepare);
        engine.submit(start(1));
        engine.submit(audio(1, 1));
        let ended = tokio::time::Instant::now();
        engine.submit(end(1));
        let mut first_audio = None;
        loop {
            let event = tokio::time::timeout(Duration::from_secs(5), engine.next())
                .await
                .expect("the turn ends");
            if matches!(event, ProviderOutput::Audio { .. }) && first_audio.is_none() {
                first_audio = Some(ended.elapsed());
            }
            if matches!(event, ProviderOutput::TurnComplete { .. }) {
                break;
            }
        }
        let asked_count = script.asked.lock().unwrap().len();
        assert_eq!(
            asked_count, asked,
            "{early:?} then {final_words:?}, prepare {prepare}"
        );
        let first_audio = first_audio.expect("the answer is said");
        assert!(
            first_audio >= Duration::from_millis(first_audio_ms)
                && first_audio < Duration::from_millis(first_audio_ms + 50),
            "{early:?}, prepare {prepare}: {first_audio:?}"
        );
        // Round 1 was asked with words the line had.
        let words = &script.asked.lock().unwrap()[asked - 1];
        assert_eq!(
            words.last(),
            Some(&Message::user(if asked == 1 && prepare {
                early
            } else {
                final_words
            }))
        );
    }
}

/// #1712: Flux's `TurnResumed` (`net::Early::Resumed`) takes the early words
/// back: the chat begun on them is dropped, and round 1 is asked on the
/// line's final words.
#[tokio::test(start_paused = true)]
async fn a_resumed_turn_drops_the_chat_begun_on_its_early_words() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("What time is it?".to_owned())].into()),
        early: Mutex::new(
            [vec![
                net::Early::Words("what time is it".to_owned()),
                net::Early::Resumed,
            ]]
            .into(),
        ),
        chats: Mutex::new(
            [
                vec![text("It's noon."), Ok(ChatEvent::Done)],
                vec![text("It's noon."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script.clone());
    let mut engine = engine.preparing(true);
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    assert!(
        turn(&mut engine)
            .await
            .contains(&"said:It's noon.".to_owned())
    );
    let asked = script.asked.lock().unwrap();
    assert_eq!(asked.len(), 2, "begun early, dropped, asked again");
    assert_eq!(
        asked[1].last(),
        Some(&Message::user("What time is it?".to_owned()))
    );
}

/// Fourth's P2 on #2229: with a chat begun on the early words, memory is
/// asked once, on those words, and its note goes with the prepared round;
/// when the final words are the same, the round stands and memory isn't
/// asked again.
#[tokio::test(start_paused = true)]
async fn memory_told_on_the_early_words_keeps_the_prepared_round() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("What's my name?".to_owned())].into()),
        early: Mutex::new([vec![net::Early::Words("what's my name".to_owned())]].into()),
        chats: Mutex::new([vec![text("You're Ossa."), Ok(ChatEvent::Done)]].into()),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script.clone());
    let asked_memory = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = asked_memory.clone();
    let mut engine = engine
        .preparing(true)
        .remembering(Arc::new(move |_, words: &str| {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            words.contains("name").then(|| "MEMORY NOTE".to_owned())
        }));
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    assert!(
        turn(&mut engine)
            .await
            .contains(&"said:You're Ossa.".to_owned())
    );
    let asked = script.asked.lock().unwrap();
    assert_eq!(asked.len(), 1, "the prepared round stands");
    assert!(
        asked[0].contains(&Message::system("MEMORY NOTE".to_owned())),
        "{:?}",
        asked[0]
    );
    assert_eq!(
        asked_memory.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "asked once, early"
    );
}

/// #1712, the first builder at 18:26: the first piece waits for 4 words
/// with the configured voice and 6 with Gemini's, unless
/// `LAMP_SECOND_FIRST_WORDS` says otherwise (0: whole clauses only).
#[test]
fn the_first_piece_waits_for_four_words_with_the_configured_voice() {
    assert_eq!(first_words_for(None, false), Some(CONFIGURED_FIRST_WORDS));
    assert_eq!(first_words_for(None, true), Some(FIRST_WORDS));
    assert_eq!(first_words_for(Some("3"), false), Some(3));
    assert_eq!(first_words_for(Some("8"), true), Some(8));
    assert_eq!(first_words_for(Some("0"), false), None);
    for unfit in ["", "x", "-1", "21"] {
        assert_eq!(first_words_for(Some(unfit), false), Some(4), "{unfit:?}");
    }
}

/// When the early words stand for round 1 (`Prepared::misfit`): the same
/// words whatever the case and punctuation, the same conversation, and no
/// note but the name note on words that name the lamp.
#[test]
fn early_words_stand_only_for_the_same_line_asked_the_same_way() {
    let (early, done) = mpsc::unbounded_channel();
    drop(early);
    let prepared = Prepared {
        words: "lamp what time is it".to_owned(),
        notes: Vec::new(),
        remembered: None,
        messages: vec![
            Message::system("SYSTEM".to_owned()),
            Message::user("lamp what time is it".to_owned()),
        ],
        events: done,
        at: Instant::now(),
        task: None,
    };
    let base = [Message::system("SYSTEM".to_owned())];
    assert_eq!(prepared.misfit(&base, &[], "Lamp, what time is it?"), None);
    let name = crate::called::NOTE.to_owned();
    assert_eq!(
        prepared.misfit(&base, std::slice::from_ref(&name), "Lamp, what time is it?"),
        None
    );
    assert!(
        prepared
            .misfit(&base, &[], "Lamp, what time is it now?")
            .is_some()
    );
    assert!(
        prepared
            .misfit(
                &[Message::system("OTHER".to_owned())],
                &[],
                "Lamp, what time is it?"
            )
            .is_some()
    );
    assert!(
        prepared
            .misfit(&base, &["(A note.)".to_owned()], "Lamp, what time is it?")
            .is_some()
    );
    assert_eq!(plain_words("  Lamp,  what's IT? "), "lamp what s it");
}

/// A round after a tool ran is not given up: the timer is set, so the
/// turn is not ended in silence, and the tool runs once (Second on #1049).
#[tokio::test(start_paused = true)]
async fn a_round_after_a_tool_ran_is_not_given_up() {
    let script = Arc::new(Script {
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c1",
                        "set_timer",
                        json!({"minutes": 5}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![],
            ]
            .into(),
        ),
        hold_chat: true,
        ..Script::default()
    });
    let (mut engine, calls, _) = scripted(script.clone());
    engine.submit(written(1, "Set a timer for five minutes."));
    let mut seen = Vec::new();
    while let Ok(event) = tokio::time::timeout(BEGIN_WITHIN * 4, engine.next()).await {
        seen.push(format!("{event:?}"));
    }
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert!(seen[0].starts_with("Started"), "{seen:?}");
    assert_eq!(calls.lock().unwrap().len(), 1);
    assert_eq!(script.asked.lock().unwrap().len(), 2);
}

/// What a turn sent, through its last event: for each audio packet its
/// first sample, words said as `said`, and how it ended.
async fn sent(engine: &mut Engine) -> Vec<String> {
    let mut sent = Vec::new();
    loop {
        match engine.next().await {
            ProviderOutput::Audio { samples, .. } => sent.push(format!("audio:{}", samples[0])),
            ProviderOutput::Transcript {
                request: Some(_), ..
            } => sent.push("said".into()),
            ProviderOutput::TurnComplete { .. } => {
                sent.push("complete".into());
                return sent;
            }
            ProviderOutput::TurnFailed { .. } => {
                sent.push("failed".into());
                return sent;
            }
            _ => {}
        }
    }
}

/// Audio packets sent before the first words, the announcement's left
/// out: the wait's sound.
fn before_words(sent: &[String]) -> usize {
    let announcement = format!("audio:{}", ANNOUNCEMENT.len());
    sent.iter()
        .take_while(|event| event.starts_with("audio:"))
        .filter(|event| **event != announcement)
        .count()
}

fn waiting(first_event_after: Option<Duration>, chats: Vec<Vec<net::Result<ChatEvent>>>) -> Engine {
    let hold = chats.iter().all(Vec::is_empty);
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("What time is it?".to_owned())].into()),
        chats: Mutex::new(chats.into()),
        first_event_after,
        hold_chat: hold,
        ..Script::default()
    });
    let (engine, _, _) = scripted(script);
    engine.waiting_with_a_sound(true)
}

/// `LAMP_SECOND_EARLY_FINALIZE` (the first builder on #1722, 07:20): a
/// pause the coordinator tells asks the line's listener for its words so
/// far, once, and the line goes on; one told for another request asks
/// nothing; a line with no pause asks nothing.
#[tokio::test]
async fn a_pause_asks_the_lines_listener_for_its_words_so_far() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("What time is it?".to_owned()), Ok("Hello.".to_owned())].into()),
        chats: Mutex::new(
            [
                vec![text("Noon."), Ok(ChatEvent::Done)],
                vec![text("Hi."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (mut engine, _, _) = scripted(script.clone());
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(paused(2));
    engine.submit(paused(1));
    engine.submit(audio(1, 2));
    engine.submit(end(1));
    turn(&mut engine).await;
    assert_eq!(script.flushed.load(Ordering::Relaxed), 1);
    assert_eq!(
        script.samples_heard.load(Ordering::Relaxed),
        2 * lamp_audio::CAPTURE_SAMPLES,
        "the sound after the pause still went"
    );
    spoken_turn_as(&mut engine, 3);
    turn(&mut engine).await;
    assert_eq!(
        script.flushed.load(Ordering::Relaxed),
        1,
        "no pause, no ask"
    );
}

fn spoken_turn_as(engine: &mut Engine, request: u64) {
    engine.submit(start(request));
    engine.submit(audio(request, 1));
    engine.submit(end(request));
}

fn spoken_turn(engine: &mut Engine) {
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
}

/// sixth-P3: a first round begun within 1.5 s makes no sound; one begun at
/// 2 s makes one soft sound first, then the answer; one never begun makes
/// the sound and ends silently at 4.5 s, not 3 s. A written turn never
/// makes it.
#[tokio::test(start_paused = true)]
async fn a_slow_first_round_makes_one_soft_sound_and_waits_longer() {
    let answer = || vec![vec![text("It is noon."), Ok(ChatEvent::Done)]];
    let mut quick = waiting(Some(Duration::from_millis(1_000)), answer());
    spoken_turn(&mut quick);
    assert_eq!(before_words(&sent(&mut quick).await), 0);

    let mut slow = waiting(Some(Duration::from_millis(2_000)), answer());
    spoken_turn(&mut slow);
    let heard = sent(&mut slow).await;
    let sound = &heard[..before_words(&heard)];
    assert!(!sound.is_empty(), "{heard:?}");
    // The sound is one of the three, short and never louder than said.
    assert!(
        sound.iter().all(|packet| packet.as_str() <= "audio:4"),
        "{sound:?}"
    );
    assert!(sound.len() * PACKET_SAMPLES <= 24_000 * 6 / 10);
    assert_eq!(heard.last().map(String::as_str), Some("complete"));
    assert!(heard.contains(&"said".to_owned()));

    let mut never = waiting(None, vec![vec![]]);
    let begun = tokio::time::Instant::now();
    spoken_turn(&mut never);
    let heard = sent(&mut never).await;
    assert!(before_words(&heard) > 0, "{heard:?}");
    assert!(!heard.contains(&"said".to_owned()));
    assert!(
        begun.elapsed() >= BEGIN_WITHIN_AFTER_CUE,
        "{:?}",
        begun.elapsed()
    );

    let mut written = waiting(Some(Duration::from_millis(2_000)), answer());
    written.submit(written_turn(2, "What time is it?"));
    assert_eq!(before_words(&sent(&mut written).await), 0);
}

fn written_turn(request: u64, words: &str) -> ProviderInput {
    written(request, words)
}

/// The first builder, 19 Oct: an A/B is scored on each request's first
/// answer audio, never its wait cue. Every request sends its times before
/// its end: one that waited with a sound has its words, its cue, its chat's
/// first event and its answer's first audio; one whose chat never began
/// has its words and its cue and nothing after; a written one no words.
#[tokio::test(start_paused = true)]
async fn each_request_sends_its_times_before_its_end() {
    async fn times_of(engine: &mut Engine) -> (Option<[bool; 4]>, bool) {
        let mut times = None;
        loop {
            match tokio::time::timeout(Duration::from_secs(30), engine.next())
                .await
                .expect("the turn ends")
            {
                ProviderOutput::Stages {
                    words_ms,
                    cue_ms,
                    chat_ms,
                    audio_ms,
                    ..
                } => {
                    times = Some([
                        words_ms.is_some(),
                        cue_ms.is_some(),
                        chat_ms.is_some(),
                        audio_ms.is_some(),
                    ])
                }
                ProviderOutput::TurnComplete { .. } | ProviderOutput::TurnFailed { .. } => {
                    return (times, true);
                }
                _ => {}
            }
        }
    }
    let answer = || vec![vec![text("It is noon."), Ok(ChatEvent::Done)]];
    let mut slow = waiting(Some(Duration::from_millis(2_000)), answer());
    spoken_turn(&mut slow);
    assert_eq!(
        times_of(&mut slow).await,
        (Some([true, true, true, true]), true)
    );
    let mut never = waiting(None, vec![vec![]]);
    spoken_turn(&mut never);
    assert_eq!(
        times_of(&mut never).await,
        (Some([true, true, false, false]), true)
    );
    let mut written = waiting(Some(Duration::from_millis(100)), answer());
    written.submit(written_turn(2, "What time is it?"));
    assert_eq!(
        times_of(&mut written).await,
        (Some([false, false, true, true]), true)
    );
}

/// The next sound is never the one played last.
#[test]
fn the_wait_sounds_take_turns() {
    let cues = Cues::default();
    assert!(cues.next().is_none());
    *cues.clips.lock().unwrap() = vec![vec![1], vec![2], vec![3]];
    let played: Vec<i16> = (0..6).map(|_| cues.next().unwrap()[0]).collect();
    assert!(
        played.windows(2).all(|pair| pair[0] != pair[1]),
        "{played:?}"
    );
    // At most 0.6 s, and faded to quiet at its end.
    let clip = soft(vec![1_000; 24_000]);
    assert_eq!(clip.len(), 24_000 * 6 / 10);
    assert_eq!(clip[0], 500, "half the reply's level");
    assert!(clip.last().unwrap().abs() < 50);
}

/// `not_for_me{}` written as words ends the turn in silence, as the call
/// does: nothing said, no announcement (lamp-4ace, 2026-10-16 17:12).
#[tokio::test]
async fn not_for_me_written_as_words_ends_the_turn_in_silence() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("No, the other one".to_owned())].into()),
        chats: Mutex::new([vec![text("not_for_me{}"), Ok(ChatEvent::Done)]].into()),
        ..Script::default()
    });
    let (mut engine, _, _) = scripted(script);
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    assert_eq!(
        turn(&mut engine).await,
        ["started:1", "heard:No, the other one", "complete"]
    );
}

/// A look on this route, which has no picture to give, tells the model it
/// saw nothing, never the words that asked it to describe one (#1044).
#[tokio::test]
async fn a_look_without_a_picture_tells_the_model_it_saw_nothing() {
    let script = Arc::new(Script {
        chats: Mutex::new([vec![text("My eye is shut."), Ok(ChatEvent::Done)]].into()),
        ..Script::default()
    });
    let (mut engine, _, _) = scripted(script.clone());
    engine.submit(ProviderInput::Written {
        request: 1,
        session: 1,
        privacy_generation: 1,
        text: crate::speak_up::LOOK_FULL_NOTE.into(),
        look: true,
    });
    turn(&mut engine).await;
    let asked = script.asked.lock().unwrap();
    assert_eq!(
        asked[0].last(),
        Some(&Message::user(crate::speak_up::NO_LOOK_NOTE))
    );
}

/// A look with a fresh picture sends it with the words as an image part;
/// one whose eye has none tells the model it saw nothing (#1044).
#[tokio::test]
async fn a_look_sends_the_eyes_picture_or_says_there_is_none() {
    let script = Arc::new(Script {
        chats: Mutex::new(
            [
                vec![text("A blue mug."), Ok(ChatEvent::Done)],
                vec![text("My eye is shut."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (engine, _, now) = scripted(script.clone());
    let shut = Arc::new(AtomicBool::new(false));
    let eye = shut.clone();
    let mut engine = engine.looking(Arc::new(move || {
        if eye.load(Ordering::Relaxed) {
            Err("the eyes' picture is old")
        } else {
            Ok(vec![0xFF, 0xD8, 0xFF])
        }
    }));
    let look = |request| ProviderInput::Written {
        request,
        session: 1,
        privacy_generation: 1,
        text: crate::speak_up::LOOK_FULL_NOTE.into(),
        look: true,
    };
    engine.submit(look(1));
    turn(&mut engine).await;
    shut.store(true, Ordering::Relaxed);
    *now.lock().unwrap() += ANNOUNCE_AFTER;
    engine.submit(look(2));
    turn(&mut engine).await;
    let asked = script.asked.lock().unwrap();
    assert_eq!(
        asked[0].last(),
        Some(&Message::user_seeing(
            crate::speak_up::LOOK_FULL_NOTE,
            &[0xFF, 0xD8, 0xFF]
        ))
    );
    let sent = serde_json::to_value(asked[0].last().unwrap()).unwrap();
    assert_eq!(
        sent["content"][1]["image_url"]["url"],
        "data:image/jpeg;base64,/9j/"
    );
    assert_eq!(
        asked[1].last(),
        Some(&Message::user(crate::speak_up::NO_LOOK_NOTE))
    );
}

/// The reviewer on #1075: a refused picture asks once more with the
/// no-look note; a tool round after a look is asked without the picture; a
/// plain or spoken turn on an engine with an eye carries none.
#[tokio::test]
async fn a_look_s_picture_goes_once_and_never_otherwise() {
    let eye = || -> Look { Arc::new(|| Ok(vec![0xFF, 0xD8, 0xFF])) };
    let look = |request| ProviderInput::Written {
        request,
        session: 1,
        privacy_generation: 1,
        text: crate::speak_up::LOOK_FULL_NOTE.into(),
        look: true,
    };
    // Refused: asked again with the note, and answered.
    let script = Arc::new(Script {
        chats: Mutex::new(
            [
                vec![Err(net::Error::Status(400))],
                vec![text("My eye is shut."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script.clone());
    let mut engine = engine.looking(eye());
    engine.submit(look(1));
    assert_eq!(
        turn(&mut engine).await.last().map(String::as_str),
        Some("complete")
    );
    let asked = script.asked.lock().unwrap().clone();
    assert_eq!(
        asked[1].last(),
        Some(&Message::user(crate::speak_up::NO_LOOK_NOTE))
    );
    // A tool round after a look: the words alone.
    let script = Arc::new(Script {
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c1",
                        "set_timer",
                        json!({}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("Done."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script.clone());
    let mut engine = engine.looking(eye());
    engine.submit(look(1));
    turn(&mut engine).await;
    let asked = script.asked.lock().unwrap().clone();
    assert_eq!(
        asked[1].iter().find(|message| message.role == "user"),
        Some(&Message::user(crate::speak_up::LOOK_FULL_NOTE))
    );
    // Never otherwise: a plain written turn and a spoken one.
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Hi.".to_owned())].into()),
        chats: Mutex::new(
            [
                vec![text("Hello."), Ok(ChatEvent::Done)],
                vec![text("Hello."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script.clone());
    let mut engine = engine.looking(eye());
    engine.submit(written(1, "Hello?"));
    turn(&mut engine).await;
    engine.submit(start(2));
    engine.submit(audio(2, 1));
    engine.submit(end(2));
    turn(&mut engine).await;
    let asked = script.asked.lock().unwrap().clone();
    assert_eq!(asked[0].last(), Some(&Message::user("Hello?")));
    assert_eq!(asked[1].last(), Some(&Message::user("Hi.")));
}

#[tokio::test]
async fn nothing_heard_is_a_quiet_complete_turn() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(String::new())].into()),
        ..Script::default()
    });
    let (mut engine, _, _) = scripted(script.clone());
    engine.submit(start(1));
    engine.submit(end(1));
    assert_eq!(turn(&mut engine).await, ["started:1", "heard:", "complete"]);
    assert!(script.asked.lock().unwrap().is_empty());
}

#[tokio::test]
async fn the_announcement_is_said_once_per_quiet_spell() {
    let script = Arc::new(Script {
        chats: Mutex::new(
            (0..3)
                .map(|_| vec![text("Yes."), Ok(ChatEvent::Done)])
                .collect(),
        ),
        ..Script::default()
    });
    let (mut engine, _, now) = scripted(script);
    // Heard, not written: its audio, of its own length.
    let announced = |labels: &[String]| labels.contains(&format!("audio:{SAID}"));
    engine.submit(written(1, "One?"));
    assert!(announced(&turn(&mut engine).await));
    *now.lock().unwrap() += Duration::from_secs(119);
    engine.submit(written(2, "Two?"));
    assert!(!announced(&turn(&mut engine).await));
    *now.lock().unwrap() += ANNOUNCE_AFTER;
    engine.submit(written(3, "Three?"));
    assert!(announced(&turn(&mut engine).await));
}

/// The lamp's words end as the Live route's do: after the last sentence's
/// audio, one empty piece, finished; a turn that said nothing has none.
#[tokio::test]
async fn the_lamps_words_end_with_a_finished_piece_as_the_live_routes_do() {
    let script = Arc::new(Script {
        chats: Mutex::new(
            [
                vec![text("It is noon. Enjoy lunch!"), Ok(ChatEvent::Done)],
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c1",
                        NOT_FOR_ME,
                        json!({}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script);
    let mut engine = engine.announcing(false);
    let pieces = |events: &[ProviderOutput]| -> Vec<(String, bool)> {
        events
            .iter()
            .filter_map(|event| match event {
                ProviderOutput::Transcript {
                    request: Some(_),
                    text,
                    finished,
                } => Some((text.clone(), *finished)),
                _ => None,
            })
            .collect()
    };
    async fn turn_of(engine: &mut Engine) -> Vec<ProviderOutput> {
        let mut events = Vec::new();
        loop {
            let event = tokio::time::timeout(Duration::from_secs(2), engine.next())
                .await
                .expect("the turn ends");
            let done = matches!(event, ProviderOutput::TurnComplete { .. });
            events.push(event);
            if done {
                return events;
            }
        }
    }
    engine.submit(written(1, "Time?"));
    let events = turn_of(&mut engine).await;
    assert_eq!(
        pieces(&events),
        [
            ("It is noon.".to_owned(), false),
            (" Enjoy lunch!".to_owned(), false),
            (String::new(), true),
        ]
    );
    // After every packet of audio, before the turn's end.
    let finished_at = events
        .iter()
        .position(|event| matches!(event, ProviderOutput::Transcript { finished: true, .. }))
        .unwrap();
    assert!(
        events[finished_at..]
            .iter()
            .all(|event| !matches!(event, ProviderOutput::Audio { .. }))
    );
    engine.submit(written(2, "Not you."));
    let events = turn_of(&mut engine).await;
    assert!(pieces(&events).is_empty(), "{:?}", pieces(&events));
}

/// The lamp's only voice (`LAMP_VOICE_ROUTE=second`, `--only-voice`) never
/// says its voice sounds different: the first answer after a quiet spell
/// is the model's own first words.
#[tokio::test]
async fn the_only_voice_never_says_it_sounds_different() {
    let script = Arc::new(Script {
        chats: Mutex::new(
            (0..2)
                .map(|_| vec![text("Yes."), Ok(ChatEvent::Done)])
                .collect(),
        ),
        ..Script::default()
    });
    let (engine, _, now) = scripted(script);
    let mut engine = engine.announcing(false);
    let expected = |request: u64| {
        [
            format!("started:{request}"),
            "said:Yes.".into(),
            "audio:4".into(),
            "generated".into(),
            "complete".into(),
        ]
    };
    engine.submit(written(1, "One?"));
    assert_eq!(turn(&mut engine).await, expected(1));
    *now.lock().unwrap() += ANNOUNCE_AFTER;
    engine.submit(written(2, "Two?"));
    assert_eq!(turn(&mut engine).await, expected(2));
}

#[tokio::test]
async fn a_turn_spoken_over_stops_its_chat_and_speech_and_sends_nothing_more() {
    let script = Arc::new(Script {
        chats: Mutex::new([vec![text("Long story. ")]].into()),
        hold_chat: true,
        endless: Some("Long story."),
        ..Script::default()
    });
    let (mut engine, _, now) = scripted(script.clone());
    // Within the spell: no announcement, the story is the first speech.
    *engine.shared.answered_at.lock().unwrap() = Some(*now.lock().unwrap());
    engine.submit(written(1, "Tell me a story."));
    loop {
        if let ProviderOutput::Audio { request: 1, .. } = engine.next().await {
            break;
        }
    }
    engine.retire_through(1);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(engine.try_output().is_none());
    // The chat stream and the speech download were both dropped.
    assert_eq!(Arc::strong_count(&script.token), 1);
}

#[tokio::test]
async fn each_failing_service_fails_the_turn_at_its_stage() {
    // Speech-to-text cannot be opened while the person is talking.
    let script = Arc::new(Script {
        listen_error: Some(net::Error::Transport("speech-to-text connect")),
        ..Script::default()
    });
    let (mut engine, _, _) = scripted(script);
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    assert_eq!(
        turn(&mut engine).await,
        ["started:1", "failed:InputOpen:Transport"]
    );

    // No final transcript after the end.
    let script = Arc::new(Script {
        heard: Mutex::new([Err(net::Error::Timeout("speech-to-text final transcript"))].into()),
        ..Script::default()
    });
    let (mut engine, _, _) = scripted(script);
    engine.submit(start(1));
    engine.submit(end(1));
    assert_eq!(
        turn(&mut engine).await,
        ["started:1", "failed:AwaitingResponse:ResponseTimeout"]
    );

    // The chat refuses: the announcement already downloading is not said.
    let script = Arc::new(Script {
        chats: Mutex::new([vec![Err(net::Error::Status(429))]].into()),
        ..Script::default()
    });
    let (mut engine, _, _) = scripted(script);
    engine.submit(written(1, "Hello?"));
    assert_eq!(
        turn(&mut engine).await,
        ["started:1", "failed:AwaitingResponse:RateLimited"]
    );

    // Speech fails after the announcement was heard: part of the answer
    // was delivered.
    let script = Arc::new(Script {
        chats: Mutex::new([vec![text("This will FAIL."), Ok(ChatEvent::Done)]].into()),
        ..Script::default()
    });
    let (mut engine, _, _) = scripted(script);
    engine.submit(written(1, "Hello?"));
    assert_eq!(
        turn(&mut engine).await,
        [
            "started:1".to_owned(),
            format!("audio:{SAID}"),
            "said:This will FAIL.".into(),
            "failed:Responding:ServerUnavailable".into(),
        ]
    );
}

/// #1844's D: speech to text's stream lost mid-line fails that turn where
/// it was (`InputOpen`, `Transport`, which #1942's link check takes up), and
/// the next line opens a stream of its own and is answered: nothing of the
/// lost one is kept.
#[tokio::test]
async fn a_speech_to_text_stream_lost_mid_line_fails_that_turn_and_the_next_is_answered() {
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok("never heard whole".to_owned()),
                Ok("What time is it?".to_owned()),
            ]
            .into(),
        ),
        chats: Mutex::new([vec![text("It is noon."), Ok(ChatEvent::Done)]].into()),
        drops_after: Mutex::new([Some(lamp_audio::CAPTURE_SAMPLES), None].into()),
        ..Script::default()
    });
    let (mut engine, _, _) = scripted(script.clone());
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(audio(1, 2));
    engine.submit(audio(1, 3));
    assert_eq!(
        turn(&mut engine).await,
        ["started:1", "failed:InputOpen:Transport"]
    );
    engine.submit(start(2));
    engine.submit(audio(2, 1));
    engine.submit(end(2));
    let next = turn(&mut engine).await;
    assert!(
        next.contains(&"heard:What time is it?".to_owned()),
        "{next:?}"
    );
    assert!(next.contains(&"said:It is noon.".to_owned()), "{next:?}");
    assert!(
        !next.iter().any(|label| label.starts_with("failed")),
        "{next:?}"
    );
    assert!(
        !script
            .asked
            .lock()
            .unwrap()
            .iter()
            .flatten()
            .any(|message| format!("{message:?}").contains("never heard whole")),
        "nothing of the lost line is asked"
    );
}

#[tokio::test]
async fn a_reminder_is_deferred_as_the_fixture_does() {
    let (mut engine, _, _) = scripted(Arc::default());
    engine.submit(ProviderInput::Reminder {
        request: 1,
        session: 1,
        privacy_generation: 1,
        text: "Tea".into(),
        late: false,
        late_by_ms: None,
    });
    assert_eq!(turn(&mut engine).await, ["deferred:Disabled"]);
}

fn reminder(request: u64, text: &str) -> ProviderInput {
    ProviderInput::Reminder {
        request,
        session: 1,
        privacy_generation: 1,
        text: text.into(),
        late: false,
        late_by_ms: None,
    }
}

/// The only voice (`LAMP_VOICE_ROUTE=second`, with the lamp saying its
/// reminders) says a due reminder as the Live route does: the chat is
/// given the Live route's own note for it, the item's words as data, and
/// what it says is said. A call it makes is not run: a reminder is only
/// said. An item the Live route would refuse is refused the same way.
#[tokio::test]
async fn the_only_voice_says_a_due_reminder_with_the_live_routes_note_and_no_tools() {
    let script = Arc::new(Script {
        chats: Mutex::new(
            [
                vec![text("Time to stretch."), Ok(ChatEvent::Done)],
                vec![
                    text("Stretch now."),
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c1",
                        "set_timer",
                        json!({}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (engine, calls, _) = scripted(script.clone());
    let mut engine = engine.announcing(false).saying_reminders(true);
    engine.submit(reminder(1, "stretch"));
    assert_eq!(
        turn(&mut engine).await,
        [
            "started:1".to_owned(),
            "said:Time to stretch.".into(),
            "audio:16".into(),
            "generated".into(),
            "complete".into(),
        ]
    );
    let note = crate::provider_worker::reminder_note("stretch", false, None).unwrap();
    assert_eq!(script.asked.lock().unwrap()[0][1], Message::user(note));
    engine.submit(reminder(2, "stretch"));
    assert_eq!(
        turn(&mut engine).await,
        [
            "started:2".to_owned(),
            "said:Stretch now.".into(),
            "audio:12".into(),
            "generated".into(),
            "complete".into(),
        ]
    );
    assert!(calls.lock().unwrap().is_empty(), "no tool ran");
    assert_eq!(
        script.asked.lock().unwrap().len(),
        2,
        "no round after the call"
    );
    engine.submit(reminder(3, "  "));
    assert_eq!(turn(&mut engine).await, ["deferred:Invalid"]);
}

#[tokio::test]
async fn the_chat_is_given_the_exchanges_it_is_told_between_the_system_and_the_question() {
    let script = Arc::new(Script {
        chats: Mutex::new([vec![text("About two million."), Ok(ChatEvent::Done)]].into()),
        ..Script::default()
    });
    let now = Instant::now();
    let tools: ToolRunner = Arc::new(|_, _, _, _| Box::pin(async { json!({"ok": true}) }));
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| Told {
            text: "SYSTEM".into(),
            after: None,
            history: vec![
                Message::user("Capital of France?"),
                Message::assistant("Paris."),
            ],
        }),
        tools,
        Arc::new(move || now),
    );
    engine.submit(written(1, "How many live there?"));
    turn(&mut engine).await;
    assert_eq!(
        script.asked.lock().unwrap()[0],
        [
            Message::system("SYSTEM"),
            Message::user("Capital of France?"),
            Message::assistant("Paris."),
            Message::user("How many live there?"),
        ]
    );
}

/// #1712, the first builder at 18:56: with what changes told apart
/// ([`Told::after`]), it goes after the exchanges and before the turn's
/// notes and words, so the request begins with what stays.
#[tokio::test]
async fn what_changes_is_told_after_the_exchanges_and_before_the_notes_and_words() {
    let script = Arc::new(Script {
        chats: Mutex::new([vec![text("About two million."), Ok(ChatEvent::Done)]].into()),
        ..Script::default()
    });
    let now = Instant::now();
    let tools: ToolRunner = Arc::new(|_, _, _, _| Box::pin(async { json!({"ok": true}) }));
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| Told {
            text: "CHARACTER".into(),
            after: Some("IT IS NOON".into()),
            history: vec![
                Message::user("Capital of France?"),
                Message::assistant("Paris."),
            ],
        }),
        tools,
        Arc::new(move || now),
    );
    engine.submit(written(1, "How many live there?"));
    turn(&mut engine).await;
    assert_eq!(
        script.asked.lock().unwrap()[0],
        [
            Message::system("CHARACTER"),
            Message::user("Capital of France?"),
            Message::assistant("Paris."),
            Message::system("IT IS NOON"),
            Message::user("How many live there?"),
        ]
    );
}

/// First builder's reviewer on #778: a tool call in a turn the lamp wrote
/// itself is not heard, so it cannot confirm a person's prepared work
/// (`agent_tasks::may_confirm`); one in a spoken turn is.
#[tokio::test]
async fn a_tool_call_is_heard_only_in_a_spoken_turn() {
    let confirm = || call("c1", "confirm_agent_task", json!({}));
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Yes, go ahead.".to_owned())].into()),
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![confirm()])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("Not without a yes."), Ok(ChatEvent::Done)],
                vec![
                    Ok(ChatEvent::ToolCalls(vec![confirm()])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("Handing it over."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (mut engine, calls, _) = scripted(script);
    engine.submit(written(8, "Ask whether to start the task."));
    turn(&mut engine).await;
    engine.retire_through(8);
    engine.submit(start(9));
    engine.submit(audio(9, 1));
    engine.submit(end(9));
    turn(&mut engine).await;
    let heard: Vec<bool> = calls.lock().unwrap().iter().map(|c| c.2).collect();
    assert_eq!(heard, [false, true]);
}

/// A spoken turn whose end is marked as a short sound let through, heard as
/// `heard`, with sounds `checked` or not: its outputs, and how many times
/// the chat was asked.
async fn sound_turn(checked: bool, marked: bool, heard: &str) -> (Vec<String>, usize) {
    sound_turn_asked(checked, marked, heard, None).await
}

/// [`sound_turn`], the engine told whether the lamp has just asked
/// something (`answering_the_lamp`), if it is.
async fn sound_turn_asked(
    checked: bool,
    marked: bool,
    heard: &str,
    asked: Option<bool>,
) -> (Vec<String>, usize) {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new([vec![text("Yes?"), Ok(ChatEvent::Done)]].into()),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script.clone());
    let mut engine = engine.checking_sounds(checked);
    if let Some(asked) = asked {
        engine = engine.answering_the_lamp(Arc::new(move |request| {
            assert_eq!(request, 1);
            asked
        }));
    }
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(ProviderInput::End {
        request: 1,
        privacy_generation: 1,
        sound: marked,
        cut_in: false,
    });
    let labels = turn(&mut engine).await;
    let asked = script.asked.lock().unwrap().len();
    (labels, asked)
}

/// #800 on this route (the reviewer on #818, 20:11): with sounds checked, a
/// short sound heard as "¿Qué?" ends quietly after its words and the chat
/// is never asked; heard as "Yes." it is answered; and with sounds not
/// checked, or an end not marked as a sound, "¿Qué?" is answered as before.
#[tokio::test]
async fn a_short_sound_not_heard_as_a_short_word_is_held_when_sounds_are_checked() {
    let (held, asked) = sound_turn(true, true, "¿Qué?").await;
    assert_eq!(held, ["started:1", "heard:¿Qué?", "complete"]);
    assert_eq!(asked, 0);
    for (checked, marked, heard) in [
        (true, true, "Yes."),
        (false, true, "¿Qué?"),
        (true, false, "¿Qué?"),
    ] {
        let (labels, asked) = sound_turn(checked, marked, heard).await;
        assert!(
            labels.iter().any(|label| label.starts_with("audio:")),
            "{checked} {marked} {heard}: {labels:?}"
        );
        assert_eq!(asked, 1, "{checked} {marked} {heard}");
    }
}

/// The Live route's case of 17 October on this route: "Yes. Please start
/// it." to the lamp's "Shall I start it?" ended after "Yes." as a short
/// sound heard as "bien". Checked, with the lamp having just asked, it is
/// answered; not having asked, or with no word heard, it is held as before.
#[tokio::test]
async fn a_checked_sound_with_a_word_right_after_the_lamps_question_is_answered() {
    let (labels, asked) = sound_turn_asked(true, true, "bien", Some(true)).await;
    assert!(
        labels.iter().any(|label| label.starts_with("audio:")),
        "{labels:?}"
    );
    assert_eq!(asked, 1);
    for (heard, lamp_asked) in [("bien", Some(false)), ("bien", None), ("¿?", Some(true))] {
        let (labels, asked) = sound_turn_asked(true, true, heard, lamp_asked).await;
        assert_eq!(
            labels,
            ["started:1", &format!("heard:{heard}"), "complete"],
            "{heard} {lamp_asked:?}"
        );
        assert_eq!(asked, 0, "{heard} {lamp_asked:?}");
    }
}

/// The first builder, 21:25: a look's first round may take 5 s, not 3 s;
/// past it the lamp says it could not look rather than nothing. A refused
/// picture's retry stays inside the same 5 s (Second on #1154).
#[tokio::test(start_paused = true)]
async fn a_look_waits_five_seconds_and_then_says_it_could_not_look() {
    async fn said(
        first_event_after: Duration,
        chats: Vec<Vec<net::Result<ChatEvent>>>,
    ) -> (Vec<String>, Duration) {
        let script = Arc::new(Script {
            chats: Mutex::new(chats.into()),
            first_event_after: Some(first_event_after),
            ..Script::default()
        });
        let (engine, _, _) = scripted(script);
        let mut engine = engine.looking(Arc::new(|| Ok(vec![0xFF, 0xD8, 0xFF])));
        engine.submit(ProviderInput::Written {
            request: 1,
            session: 1,
            privacy_generation: 1,
            text: crate::speak_up::LOOK_FULL_NOTE.into(),
            look: true,
        });
        let begun = tokio::time::Instant::now();
        let mut said = Vec::new();
        loop {
            match engine.next().await {
                // The words' end (`finished`) adds none.
                ProviderOutput::Transcript {
                    request: Some(_),
                    text,
                    ..
                } if !text.is_empty() => said.push(text),
                ProviderOutput::TurnComplete { .. } => return (said, begun.elapsed()),
                ProviderOutput::TurnFailed { .. } => panic!("failed"),
                _ => {}
            }
        }
    }
    let answer = || vec![vec![text("A blue mug."), Ok(ChatEvent::Done)]];
    let (slow, _) = said(Duration::from_secs(4), answer()).await;
    assert_eq!(slow, ["A blue mug."]);
    let (never, after) = said(Duration::from_secs(60), answer()).await;
    assert_eq!(never, [COULD_NOT_LOOK]);
    assert!(after >= LOOK_BEGIN_WITHIN, "{after:?}");
    // Refused at 4 s; the retry, 4 s more, is past the 5 s: the line at 5 s.
    let refused = vec![vec![Err(net::Error::Status(400))], answer().remove(0)];
    let (retried, after) = said(Duration::from_secs(4), refused).await;
    assert_eq!(retried, [COULD_NOT_LOOK]);
    assert!(
        after < LOOK_BEGIN_WITHIN + Duration::from_millis(100),
        "{after:?}"
    );
}

/// A full look whose still never came arrives as the no-look note without
/// `look`: it is a look all the same, with 5 s and the line, never a silent
/// end at 3 s (the reviewer on #1154).
#[tokio::test(start_paused = true)]
async fn a_full_look_without_its_still_still_says_it_could_not_look() {
    let script = Arc::new(Script {
        chats: Mutex::new([vec![text("My eye was shut."), Ok(ChatEvent::Done)]].into()),
        first_event_after: Some(Duration::from_secs(60)),
        ..Script::default()
    });
    let (mut engine, _, _) = scripted(script.clone());
    engine.submit(ProviderInput::Written {
        request: 1,
        session: 1,
        privacy_generation: 1,
        text: crate::speak_up::NO_LOOK_NOTE.into(),
        look: false,
    });
    let begun = tokio::time::Instant::now();
    let mut said = Vec::new();
    loop {
        match engine.next().await {
            // The words' end (`finished`) adds none.
            ProviderOutput::Transcript {
                request: Some(_),
                text,
                ..
            } if !text.is_empty() => said.push(text),
            ProviderOutput::TurnComplete { .. } => break,
            ProviderOutput::TurnFailed { .. } => panic!("failed"),
            _ => {}
        }
    }
    assert_eq!(said, [COULD_NOT_LOOK]);
    assert!(
        begun.elapsed() >= LOOK_BEGIN_WITHIN,
        "{:?}",
        begun.elapsed()
    );
    assert_eq!(
        script.asked.lock().unwrap()[0].last(),
        Some(&Message::user(crate::speak_up::NO_LOOK_NOTE))
    );
}

/// A request that goes on from one given up after an early end is asked
/// with that one's words before its own, as Gemini, which heard both, is
/// told; what is told as heard is its own words only. A request that does
/// not go on from one starts afresh.
#[tokio::test]
async fn a_request_going_on_from_one_given_up_begins_with_its_words() {
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok("What time".to_owned()),
                Ok("is it in Paris".to_owned()),
                Ok("Thanks".to_owned()),
            ]
            .into(),
        ),
        chats: Mutex::new(
            [
                vec![text("Hm."), Ok(ChatEvent::Done)],
                vec![text("Nine."), Ok(ChatEvent::Done)],
                vec![text("Sure."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (mut engine, _, _) = scripted(script.clone());
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    turn(&mut engine).await;
    engine.submit(ProviderInput::Start {
        request: 2,
        session: 1,
        privacy_generation: 1,
        continues: false,
        continues_cut_off: true,
        over_the_lamp: false,
        after_stop: false,
        follows_on: false,
        over_the_lamps_voice: false,
        heard_of_cut: None,
    });
    engine.submit(audio(2, 1));
    engine.submit(end(2));
    let heard = turn(&mut engine).await;
    assert!(
        heard.contains(&"heard:is it in Paris".to_owned()),
        "{heard:?}"
    );
    engine.submit(start(3));
    engine.submit(audio(3, 1));
    engine.submit(end(3));
    turn(&mut engine).await;
    let asked = script.asked.lock().unwrap();
    assert_eq!(
        asked[1].last(),
        Some(&Message::user("What time is it in Paris"))
    );
    assert_eq!(asked[2].last(), Some(&Message::user("Thanks")));
}

/// The reviewer's probe on #1168: a request given up before its words came
/// leaves the one going on from it to be asked alone, never with an earlier
/// request's words; nor is another session's taken.
#[tokio::test]
async fn a_request_going_on_takes_only_the_words_of_the_one_just_before_it_in_its_session() {
    let begin = |request, session, continues_cut_off| ProviderInput::Start {
        request,
        session,
        privacy_generation: 1,
        continues: false,
        continues_cut_off,
        over_the_lamp: false,
        after_stop: false,
        follows_on: false,
        over_the_lamps_voice: false,
        heard_of_cut: None,
    };
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok("my code is four four one two".to_owned()),
                Ok("is it in Paris".to_owned()),
                Ok("and in Rome".to_owned()),
            ]
            .into(),
        ),
        chats: Mutex::new(
            [
                vec![text("Noted."), Ok(ChatEvent::Done)],
                vec![text("Nine."), Ok(ChatEvent::Done)],
                vec![text("Nine."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (mut engine, _, _) = scripted(script.clone());
    engine.submit(begin(1, 1, false));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    turn(&mut engine).await;
    // Request 2, another session, is given up before any of its words.
    engine.submit(begin(2, 2, false));
    engine.submit(begin(3, 2, true));
    engine.submit(audio(3, 1));
    engine.submit(end(3));
    turn(&mut engine).await;
    // Request 4 goes on from request 3, but in another session.
    engine.submit(begin(4, 3, true));
    engine.submit(audio(4, 1));
    engine.submit(end(4));
    turn(&mut engine).await;
    let asked = script.asked.lock().unwrap();
    assert_eq!(asked[1].last(), Some(&Message::user("is it in Paris")));
    assert_eq!(asked[2].last(), Some(&Message::user("and in Rome")));
}

/// The first request after a hand or the head stopped the lamp is asked with
/// the stopped note beside it, as Gemini is told (#1148): a system message
/// just before the person's words, never their words. The next is not.
#[tokio::test]
async fn the_first_request_after_a_stop_is_told_of_it_beside_its_words() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("What now".to_owned()), Ok("Thanks".to_owned())].into()),
        chats: Mutex::new(
            [
                vec![text("Sure."), Ok(ChatEvent::Done)],
                vec![text("Any time."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (mut engine, _, _) = scripted(script.clone());
    engine.submit(ProviderInput::Start {
        request: 1,
        session: 1,
        privacy_generation: 1,
        continues: false,
        continues_cut_off: false,
        over_the_lamp: false,
        after_stop: true,
        follows_on: false,
        over_the_lamps_voice: false,
        heard_of_cut: None,
    });
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    turn(&mut engine).await;
    engine.submit(start(2));
    engine.submit(audio(2, 1));
    engine.submit(end(2));
    turn(&mut engine).await;
    let asked = script.asked.lock().unwrap();
    let first = &asked[0];
    assert_eq!(first.last(), Some(&Message::user("What now")));
    assert_eq!(
        first[first.len() - 2],
        Message::system(crate::provider_worker::STOPPED_NOTE)
    );
    assert!(
        !asked[1].contains(&Message::system(crate::provider_worker::STOPPED_NOTE)),
        "{:?}",
        asked[1]
    );
}

/// The reviewer's probe on #1175: a stop's request that is never asked (it
/// heard nothing) leaves the note to the next request in its session, which
/// is told; in another session it is not.
#[tokio::test]
async fn a_stop_whose_request_is_never_asked_is_told_with_the_next_one() {
    let begin = |request, session, after_stop| ProviderInput::Start {
        request,
        session,
        privacy_generation: 1,
        continues: false,
        continues_cut_off: false,
        over_the_lamp: false,
        after_stop,
        follows_on: false,
        over_the_lamps_voice: false,
        heard_of_cut: None,
    };
    let stopped = Message::system(crate::provider_worker::STOPPED_NOTE);
    for (next_session, told) in [(1, true), (2, false)] {
        let script = Arc::new(Script {
            heard: Mutex::new([Ok(String::new()), Ok("Tell me another".to_owned())].into()),
            chats: Mutex::new([vec![text("Sure."), Ok(ChatEvent::Done)]].into()),
            ..Script::default()
        });
        let (mut engine, _, _) = scripted(script.clone());
        engine.submit(begin(1, 1, true));
        engine.submit(audio(1, 1));
        engine.submit(end(1));
        turn(&mut engine).await;
        engine.submit(begin(2, next_session, false));
        engine.submit(audio(2, 1));
        engine.submit(end(2));
        turn(&mut engine).await;
        let asked = script.asked.lock().unwrap();
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0].last(), Some(&Message::user("Tell me another")));
        assert_eq!(asked[0].contains(&stopped), told, "session {next_session}");
    }
}

/// The reviewer's 03:01 on #1175: a request going on from the stop's
/// request (`continues_cut_off`) is told of the stop even though that one
/// was asked with it, as it was given up; an ordinary next request is not;
/// and a stop is never told past the coordinator's 2 min.
#[tokio::test(start_paused = true)]
async fn a_stop_goes_with_a_request_going_on_and_never_past_two_minutes() {
    let begin = |request, after_stop, continues_cut_off| ProviderInput::Start {
        request,
        session: 1,
        privacy_generation: 1,
        continues: false,
        continues_cut_off,
        over_the_lamp: false,
        after_stop,
        follows_on: false,
        over_the_lamps_voice: false,
        heard_of_cut: None,
    };
    let stopped = Message::system(crate::provider_worker::STOPPED_NOTE);
    let answered = || vec![text("Sure."), Ok(ChatEvent::Done)];
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok("What time".to_owned()),
                Ok("is it in Paris".to_owned()),
                Ok("Thanks".to_owned()),
                Ok(String::new()),
                Ok("Tell me another".to_owned()),
            ]
            .into(),
        ),
        chats: Mutex::new([answered(), answered(), answered(), answered()].into()),
        ..Script::default()
    });
    let (mut engine, _, _) = scripted(script.clone());
    async fn ask(engine: &mut Engine, inputs: [ProviderInput; 3]) {
        for input in inputs {
            engine.submit(input);
        }
        sent(engine).await;
    }
    ask(&mut engine, [begin(1, true, false), audio(1, 1), end(1)]).await;
    ask(&mut engine, [begin(2, false, true), audio(2, 1), end(2)]).await;
    ask(&mut engine, [begin(3, false, false), audio(3, 1), end(3)]).await;
    // A stop left by a request that heard nothing, ten minutes before.
    ask(&mut engine, [begin(4, true, false), audio(4, 1), end(4)]).await;
    tokio::time::sleep(Duration::from_secs(600)).await;
    ask(&mut engine, [begin(5, false, false), audio(5, 1), end(5)]).await;
    let asked = script.asked.lock().unwrap();
    assert_eq!(asked.len(), 4);
    assert!(asked[0].contains(&stopped));
    assert_eq!(
        asked[1].last(),
        Some(&Message::user("What time is it in Paris"))
    );
    assert!(asked[1].contains(&stopped), "{:?}", asked[1]);
    assert!(!asked[2].contains(&stopped), "{:?}", asked[2]);
    assert_eq!(asked[3].last(), Some(&Message::user("Tell me another")));
    assert!(!asked[3].contains(&stopped), "{:?}", asked[3]);
}

/// The first builder, 2026-10-17 02:48: with the down line on, a spoken
/// turn whose chat never began says "I can't think right now; give me a
/// minute." as audio only, not as words, at most once a minute; a written
/// turn (the lamp's own) never says it; with it off, nothing.
#[tokio::test(start_paused = true)]
async fn a_chat_that_never_begins_says_the_down_line_at_most_once_a_minute() {
    async fn turns(on: bool) -> Vec<Vec<String>> {
        let script = Arc::new(Script {
            heard: Mutex::new(
                std::iter::repeat_with(|| Ok("What time is it?".to_owned()))
                    .take(5)
                    .collect(),
            ),
            chats: Mutex::new(
                [
                    vec![text("It is noon."), Ok(ChatEvent::Done)],
                    vec![],
                    vec![],
                    vec![],
                    vec![],
                ]
                .into(),
            ),
            hold_chat: true,
            ..Script::default()
        });
        let (engine, _, _) = scripted(script);
        let mut engine = engine.saying_when_down(on);
        let mut heard = Vec::new();
        // The first, answered, renders the line after it is out. The
        // second, written, is the lamp's own and stays silent.
        for (request, wait, spoken) in [
            (1, 0, true),
            (2, 1, false),
            (3, 1, true),
            (4, 1, true),
            (5, 61, true),
        ] {
            tokio::time::sleep(Duration::from_secs(wait)).await;
            if spoken {
                engine.submit(start(request));
                engine.submit(audio(request, 1));
                engine.submit(end(request));
            } else {
                engine.submit(written(request, "Say hello."));
            }
            heard.push(sent(&mut engine).await);
        }
        heard
    }
    let line = format!("audio:{}", DOWN_LINE.len());
    let on = turns(true).await;
    assert!(!on[0].contains(&line), "{on:?}");
    assert_eq!(on[1], ["complete"], "a written turn: {on:?}");
    assert!(on[2].contains(&line), "{on:?}");
    assert!(!on[2].iter().any(|event| event == "said"), "{on:?}");
    assert_eq!(on[3], ["complete"]);
    assert!(on[4].contains(&line), "{on:?}");
    let off = turns(false).await;
    assert!(off[1..].iter().all(|turn| turn == &["complete"]), "{off:?}");
}

/// #1844's B: a chat that refuses before any word (429, 401, a 5xx) says the
/// down line as a chat that never begins does, at most once a minute, then
/// fails the turn as before; a written turn never says it; with it off,
/// nothing.
#[tokio::test(start_paused = true)]
async fn a_chat_refused_before_any_word_says_the_down_line_at_most_once_a_minute() {
    async fn turns(on: bool) -> Vec<Vec<String>> {
        let refused = |status| vec![Err(net::Error::Status(status))];
        let script = Arc::new(Script {
            heard: Mutex::new(
                std::iter::repeat_with(|| Ok("What time is it?".to_owned()))
                    .take(5)
                    .collect(),
            ),
            chats: Mutex::new(
                [
                    vec![text("It is noon."), Ok(ChatEvent::Done)],
                    refused(429),
                    refused(401),
                    refused(401),
                    refused(503),
                ]
                .into(),
            ),
            ..Script::default()
        });
        let (engine, _, _) = scripted(script);
        let mut engine = engine.saying_when_down(on);
        let mut heard = Vec::new();
        for (request, wait, spoken) in [
            (1, 0, true),
            (2, 1, true),
            (3, 1, false),
            (4, 1, true),
            (5, 61, true),
        ] {
            tokio::time::sleep(Duration::from_secs(wait)).await;
            if spoken {
                engine.submit(start(request));
                engine.submit(audio(request, 1));
                engine.submit(end(request));
            } else {
                engine.submit(written(request, "Say hello."));
            }
            heard.push(sent(&mut engine).await);
        }
        heard
    }
    let line = format!("audio:{}", DOWN_LINE.len());
    let on = turns(true).await;
    assert!(!on[0].contains(&line), "{on:?}");
    assert!(on[1].contains(&line), "the first refusal: {on:?}");
    assert!(
        on[1].iter().any(|event| event == "failed"),
        "still a failed turn: {on:?}"
    );
    assert!(!on[2].contains(&line), "a written turn: {on:?}");
    assert!(!on[3].contains(&line), "within the minute: {on:?}");
    assert!(on[4].contains(&line), "past the minute: {on:?}");
    let off = turns(false).await;
    assert!(off.iter().all(|turn| !turn.contains(&line)), "{off:?}");
}

/// The rule is told which request a turn goes on from (`continues`): the one
/// begun just before it, in its session; in another session, none (the
/// reviewer on #1226).
#[tokio::test]
async fn a_request_going_on_names_the_one_begun_before_it_in_its_session() {
    let begin = |request, session, continues| ProviderInput::Start {
        request,
        session,
        privacy_generation: 1,
        continues,
        continues_cut_off: false,
        over_the_lamp: false,
        after_stop: false,
        follows_on: false,
        over_the_lamps_voice: false,
        heard_of_cut: None,
    };
    let script = Arc::new(Script {
        heard: Mutex::new(
            std::iter::repeat_with(|| Ok("What time is it?".to_owned()))
                .take(3)
                .collect(),
        ),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script);
    let froms: Arc<Mutex<Vec<Option<u64>>>> = Arc::default();
    let seen = froms.clone();
    let mut engine = engine.heeding(Arc::new(move |_, _, _, from| {
        seen.lock().unwrap().push(from);
        Some("held for the test")
    }));
    for (request, session, continues) in [(1, 1, false), (2, 1, true), (3, 2, true)] {
        engine.submit(begin(request, session, continues));
        engine.submit(audio(request, 1));
        engine.submit(end(request));
        turn(&mut engine).await;
    }
    assert_eq!(*froms.lock().unwrap(), [None, Some(1), None]);
}

/// #1712: with the second route as the lamp's only voice, nothing publishes
/// `room/state`, so it keeps the room itself, as the Live worker does. Three
/// spoken lines the model leaves alone within a minute make the room busy;
/// the lamp answering a line counts as speaking, not as talk.
#[tokio::test]
async fn lines_left_unanswered_make_the_routes_room_busy_and_an_answer_is_speaking() {
    let silent = || {
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "c1",
                NOT_FOR_ME,
                json!({}),
            )])),
            Ok(ChatEvent::Done),
        ]
    };
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok("Did you see the game?".to_owned()),
                Ok("Yes, last night.".to_owned()),
                Ok("What a finish.".to_owned()),
                Ok("Lamp, what time is it?".to_owned()),
            ]
            .into(),
        ),
        chats: Mutex::new(
            [
                silent(),
                silent(),
                silent(),
                vec![text("It's noon."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (mut engine, _, _) = scripted(script);
    let room_now = || {
        crate::room_told::room()
            .lock()
            .unwrap()
            .at(lamp_ipc::monotonic_us())
    };
    assert_eq!(room_now(), crate::room_told::Room::Unknown);
    for request in 1..=3 {
        engine.submit(start(request));
        engine.submit(audio(request, 1));
        engine.submit(end(request));
        assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
    }
    assert_eq!(room_now(), crate::room_told::Room::Busy);
    engine.submit(start(4));
    engine.submit(audio(4, 1));
    engine.submit(end(4));
    assert!(
        turn(&mut engine)
            .await
            .contains(&"said:It's noon.".to_owned())
    );
    // The answer cleared the run of lines left alone: two more are no busy
    // room's run of three.
    assert!(
        !crate::room_told::room()
            .lock()
            .unwrap()
            .talked_past_the_lamp()
    );
}

/// #1712: a spoken line the lamp's name began is asked with the Live route's
/// note beside the words (`LAMP_NAME_NOTE=on`, `worker::Heeding`); a line
/// without it, with none.
#[tokio::test]
async fn a_line_the_name_began_is_asked_with_the_name_note() {
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok("Lamp, what time is it?".to_owned()),
                Ok("What time is it?".to_owned()),
            ]
            .into(),
        ),
        chats: Mutex::new(
            [
                vec![text("It's noon."), Ok(ChatEvent::Done)],
                vec![text("Noon."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script.clone());
    let mut engine = engine.noting_the_name(Arc::new(|request| {
        (request == 1).then_some(crate::called::note(false))
    }));
    for request in 1..=2 {
        engine.submit(start(request));
        engine.submit(audio(request, 1));
        engine.submit(end(request));
        assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
    }
    let asked = script.asked.lock().unwrap();
    assert_eq!(
        asked[0],
        [
            Message::system("SYSTEM"),
            Message::system(crate::called::note(false)),
            Message::user("Lamp, what time is it?"),
        ]
    );
    assert_eq!(
        asked[1],
        [Message::system("SYSTEM"), Message::user("What time is it?")]
    );
}

/// #1712, lamp-4ace's A/B on 19 Oct: a hand-over prepared on this route,
/// then the person's yes. This route's history keeps words only, so the yes
/// is asked with the note that a task waits for it, and the model's
/// `confirm_agent_task` starts it: the tasks part is asked to confirm, not
/// to prepare again.
#[test]
fn a_yes_after_a_hand_over_on_this_route_starts_it() {
    let _one = crate::agent_tasks::tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let directory = std::env::temp_dir().join(format!("second-yes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).unwrap();
    let status = |revision: u64| json!({"ok":true,"store_id":"s1","revision":revision,"summary":{"session_id":"sess"}});
    let seen = crate::agent_tasks::tests::tasks_part(
        &directory,
        vec![
            ("request.sock", status(0)),
            (
                "request.sock",
                json!({"ok":true,"id":7,"prepared_digest":"abc","revision":1}),
            ),
            ("request.sock", status(1)),
            (
                "voice.sock",
                json!({"ok":true,"id":7,"queued":true,"revision":2}),
            ),
        ],
    );
    let place = crate::agent_tasks::Place {
        directory: directory.clone(),
        target: crate::agent_tasks::DEFAULT_TARGET.into(),
        who: false,
        lead_in: false,
        asked: false,
        needs: None,
        staff_note: false,
        accounts: false,
        machine_names: false,
        make_skill: false,
        named_only: false,
    };
    let settings = crate::tools::Settings {
        tasks: Some(place),
        ..crate::tools::Settings::default()
    };
    let task = "Write a short story";
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok("Ask Claude Code to write a short story.".to_owned()),
                Ok("Yes. Please start it.".to_owned()),
            ]
            .into(),
        ),
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c1",
                        crate::agent_tasks::START,
                        json!({ "task": task }),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("Shall I start it?"), Ok(ChatEvent::Done)],
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c2",
                        crate::agent_tasks::CONFIRM,
                        json!({}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![
                    text("Okay, giving that to Claude Code."),
                    Ok(ChatEvent::Done),
                ],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        worker::tool_runner(settings, None, None, false),
        Arc::new(Instant::now),
    )
    .announcing(false)
    .noting_a_task_waiting(Arc::new(|request| {
        crate::agent_tasks::pending_note(request, std::time::Instant::now())
    }));
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            for request in 1..=2 {
                engine.submit(start(request));
                engine.submit(audio(request, 1));
                engine.submit(end(request));
                assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
            }
        });
    let _ = std::fs::remove_dir_all(&directory);
    let asked = script.asked.lock().unwrap();
    // The yes, round 1: the note that a task waits for it, beside its words.
    assert!(
        asked[2].iter().any(|message| message.role == "system"
            && message
                .content
                .as_ref()
                .and_then(Value::as_str)
                .is_some_and(|note| note.contains(crate::agent_tasks::CONFIRM))),
        "{:?}",
        asked[2]
    );
    let ops: Vec<Value> = seen.try_iter().collect();
    let ops: Vec<&str> = ops.iter().filter_map(|op| op["op"].as_str()).collect();
    assert_eq!(ops, ["status", "prepare", "status", "confirm"]);
}

/// #1712's map, item 6 (`LAMP_AIR_FACTS` on the second route): a spoken
/// request is asked with the air's note beside its words when the note is
/// due; a written turn (the lamp's own) never asks for it.
#[tokio::test]
async fn a_spoken_request_is_told_the_air_and_a_written_turn_is_not() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("What should I do?".to_owned())].into()),
        chats: Mutex::new(
            [
                vec![text("Maybe open a window."), Ok(ChatEvent::Done)],
                vec![text("Hello."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let asked_for = Arc::new(Mutex::new(Vec::new()));
    let (engine, _, _) = scripted(script.clone());
    let noted = asked_for.clone();
    let heard_for = Arc::new(Mutex::new(Vec::new()));
    let heard = heard_for.clone();
    let mut engine = engine.noting_the_air(
        Arc::new(move |request| {
            noted.lock().unwrap().push(request);
            Some("AIR NOTE".to_owned())
        }),
        Arc::new(move |request| heard.lock().unwrap().push(request)),
    );
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
    engine.submit(written(2, "Say hello."));
    assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
    let told = |messages: &Vec<Message>| {
        messages.iter().any(|message| {
            message.role == "system"
                && message
                    .content
                    .as_ref()
                    .and_then(Value::as_str)
                    .is_some_and(|note| note == "AIR NOTE")
        })
    };
    let asked = script.asked.lock().unwrap();
    assert!(told(&asked[0]), "{:?}", asked[0]);
    assert!(!told(&asked[1]), "{:?}", asked[1]);
    assert_eq!(
        *asked_for.lock().unwrap(),
        [1],
        "asked of the spoken one only"
    );
    // Each answer, once its first audio goes, is told as heard: the spoken
    // one's spends its note (the reviewer's P3 on #1960).
    assert!(
        heard_for.lock().unwrap().contains(&1),
        "{:?}",
        heard_for.lock().unwrap()
    );
}

/// #1712: what the lamp did is told to the rule, as the Live worker tells its
/// own: a task, timer or reminder asked of its parts is its work, and its
/// own written turn heard is news someone may reply to. A lookup is
/// neither.
#[tokio::test]
async fn the_lamps_work_and_its_own_turns_are_told_to_the_rule() {
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok("Set a timer for one minute.".to_owned()),
                Ok("What time is it?".to_owned()),
            ]
            .into(),
        ),
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c1",
                        "set_timer",
                        json!({"text": "timer", "seconds": 60}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("Done."), Ok(ChatEvent::Done)],
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c2",
                        "the_time",
                        json!({}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("Noon."), Ok(ChatEvent::Done)],
                vec![text("Claude Code is done."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script);
    let did: Arc<Mutex<Vec<&'static str>>> = Arc::default();
    let seen = did.clone();
    let mut engine = engine.telling_what_it_did(Arc::new(move |did| {
        seen.lock().unwrap().push(match did {
            Did::Worked(_) => "worked",
            Did::Spoke(_) => "spoke",
            // The words' end, told with each packet: tested on its own.
            Did::Said(..) => return,
        })
    }));
    for request in 1..=2 {
        engine.submit(start(request));
        engine.submit(audio(request, 1));
        engine.submit(end(request));
        assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
    }
    assert_eq!(
        *did.lock().unwrap(),
        ["worked"],
        "the timer only, not the time"
    );
    engine.submit(written(3, "Tell the person their task is done."));
    assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
    assert_eq!(*did.lock().unwrap(), ["worked", "spoke"]);
}

/// #1712, the first builder at 19:21: the engine tells when the lamp's
/// words sent will have been played, each packet after the one before, so
/// a conversation goes on from their end, not from when the answer began.
#[tokio::test]
async fn the_end_of_the_lamps_words_is_told_as_they_are_sent() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("What time is it?".to_owned())].into()),
        chats: Mutex::new([vec![text("Noon. It is sunny."), Ok(ChatEvent::Done)]].into()),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script);
    let said: Arc<Mutex<Vec<u64>>> = Arc::default();
    let seen = said.clone();
    let mut engine = engine.telling_what_it_did(Arc::new(move |did| {
        if let Did::Said(_, until_us) = did {
            seen.lock().unwrap().push(until_us);
        }
    }));
    let before = lamp_ipc::monotonic_us();
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
    let after = lamp_ipc::monotonic_us();
    let said = said.lock().unwrap().clone();
    // Each of the script's packets is 1,500 samples: 62.5 ms at 24 kHz,
    // played after the one before, though all are sent at once.
    assert!(!said.is_empty());
    for pair in said.windows(2) {
        assert!(pair[1] >= pair[0] + 62_500, "{said:?}");
    }
    let last = *said.last().unwrap();
    assert!(last >= before + 62_500 * said.len() as u64, "{said:?}");
    assert!(last <= after + 62_500 * said.len() as u64, "{said:?}");
}

/// #1712: a line the rule let out only as the conversation going on is
/// asked with the note that it may be people's talk, beside its words; the
/// next line, not let out so, with none.
#[tokio::test]
async fn a_line_let_out_only_as_the_conversation_is_asked_with_its_note() {
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok("I saved a hundred dollars on the flight.".to_owned()),
                Ok("What time is it?".to_owned()),
            ]
            .into(),
        ),
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c1",
                        NOT_FOR_ME,
                        json!({}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("Noon."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script.clone());
    let mut engine = engine.noting_the_conversation_alone(Arc::new(|request| {
        (request == 1).then_some(worker::CONVERSATION_ALONE)
    }));
    for request in 1..=2 {
        engine.submit(start(request));
        engine.submit(audio(request, 1));
        engine.submit(end(request));
        assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
    }
    let asked = script.asked.lock().unwrap();
    assert_eq!(
        asked[0],
        [
            Message::system("SYSTEM"),
            Message::system(worker::CONVERSATION_ALONE),
            Message::user("I saved a hundred dollars on the flight."),
        ]
    );
    assert_eq!(
        asked[1],
        [Message::system("SYSTEM"), Message::user("What time is it?")]
    );
}

/// #1712: the voice is asked to make ready as each spoken line begins, while
/// the person speaks, not when the answer's first sentence is cut.
#[tokio::test]
async fn the_voice_makes_ready_as_a_line_begins() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("What time is it?".to_owned())].into()),
        chats: Mutex::new([vec![text("Noon."), Ok(ChatEvent::Done)]].into()),
        ..Script::default()
    });
    let (mut engine, _, _) = scripted(script.clone());
    engine.submit(start(1));
    tokio::task::yield_now().await;
    assert_eq!(script.warmed.load(Ordering::Relaxed), 1);
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
    assert_eq!(script.warmed.load(Ordering::Relaxed), 1);
}

/// #1712: a call the hold names is not run; the model is told it was not
/// done, as the Live route tells it, and says nothing more of it.
#[tokio::test]
async fn a_held_call_is_not_run_and_the_model_is_told_so() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Yes, give me five minutes.".to_owned())].into()),
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c1",
                        "set_timer",
                        json!({"text": "timer", "seconds": 300}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let calls: Calls = Arc::default();
    let seen = calls.clone();
    let runner: ToolRunner = Arc::new(move |_, heard, name, args| {
        seen.lock().unwrap().push((name, args, heard));
        Box::pin(async { json!({"ok": true}) })
    });
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        runner,
        Arc::new(Instant::now),
    )
    .announcing(false)
    .holding_calls(Arc::new(|request, name| {
        request == 1 && name == "set_timer"
    }));
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
    assert!(calls.lock().unwrap().is_empty(), "the timer was not set");
    let asked = script.asked.lock().unwrap();
    assert_eq!(
        asked[1].last().unwrap(),
        &Message::tool("c1".to_owned(), crate::policy_holds::held().to_string())
    );
}

/// #1712: with the first words cut, the answer's first piece is said once
/// that many words have come, before its sentence ends; the rest of the
/// sentence follows. Without it, the whole sentence.
#[tokio::test]
async fn the_first_words_are_said_before_the_first_sentence_ends() {
    let pieces = || {
        vec![
            text("It is warm "),
            text("and sunny in "),
            text("Hanoi today."),
            Ok(ChatEvent::Done),
        ]
    };
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok("What's the weather?".to_owned()),
                Ok("What's the weather?".to_owned()),
            ]
            .into(),
        ),
        chats: Mutex::new([pieces(), pieces()].into()),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script.clone());
    let mut engine = engine.cutting_first_words(4);
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    let said: Vec<String> = turn(&mut engine)
        .await
        .into_iter()
        .filter(|label| label.starts_with("said:"))
        .collect();
    assert_eq!(said, ["said:It is warm and", "said: sunny in Hanoi today."]);
    let (mut whole, _, _) = scripted(script);
    whole.submit(start(2));
    whole.submit(audio(2, 1));
    whole.submit(end(2));
    let said: Vec<String> = turn(&mut whole)
        .await
        .into_iter()
        .filter(|label| label.starts_with("said:"))
        .collect();
    assert_eq!(said, ["said:It is warm and sunny in Hanoi today."]);
}

/// #1712: a spoken request's words are told once heard, before its chat
/// begins, so a search can start beside the model's first round.
#[tokio::test]
async fn the_words_heard_are_told_before_the_chat_begins() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Who won the race?".to_owned())].into()),
        chats: Mutex::new([vec![text("Max did."), Ok(ChatEvent::Done)]].into()),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script.clone());
    let told: Arc<Mutex<Vec<(u64, String, usize)>>> = Arc::default();
    let seen = told.clone();
    let asked = script.clone();
    let mut engine = engine.hearing_words(Arc::new(move |request, heard| {
        let chats = asked.asked.lock().unwrap().len();
        seen.lock()
            .unwrap()
            .push((request, heard.to_owned(), chats));
    }));
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
    assert_eq!(
        *told.lock().unwrap(),
        [(1, "Who won the race?".to_owned(), 0)],
        "told once, before any chat"
    );
}

/// #1712: a look the model asks for by its call goes to the coordinator for
/// the turn's full look; the lamp says it will look, and the turn ends with
/// no further round. In a turn the lamp wrote itself, the look is dropped.
#[tokio::test]
async fn a_look_asked_by_its_call_goes_to_the_coordinator_and_ends_the_turn() {
    let looking = || {
        vec![
            Ok(ChatEvent::ToolCalls(vec![call("c1", LOOK_CALL, json!({}))])),
            Ok(ChatEvent::Done),
        ]
    };
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("What do you see?".to_owned())].into()),
        chats: Mutex::new([looking(), looking()].into()),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script.clone());
    let mut engine = engine.looking_by_call();
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    let mut asked = None;
    let mut said = Vec::new();
    loop {
        let event = tokio::time::timeout(Duration::from_secs(2), engine.next())
            .await
            .expect("the turn ends");
        match event {
            ProviderOutput::LookAsked {
                request, dropped, ..
            } => asked = Some((request, dropped)),
            ProviderOutput::Transcript {
                request: Some(_),
                text,
                ..
            } if !text.is_empty() => said.push(text),
            ProviderOutput::TurnComplete { .. } => break,
            _ => {}
        }
    }
    assert_eq!(asked, Some((1, None)));
    assert_eq!(said, [LOOKING]);
    assert_eq!(
        script.asked.lock().unwrap().len(),
        1,
        "no round after the look"
    );
    engine.submit(written(2, "Tell the person what you see."));
    let mut dropped = None;
    loop {
        let event = tokio::time::timeout(Duration::from_secs(2), engine.next())
            .await
            .expect("the turn ends");
        match event {
            ProviderOutput::LookAsked { dropped: why, .. } => dropped = why,
            ProviderOutput::TurnComplete { .. } => break,
            _ => {}
        }
    }
    assert_eq!(
        dropped.as_deref(),
        Some("the turn is one the lamp wrote itself")
    );
}

/// `LAMP_SECOND_SKILL_LOOK` (#941 on the second route): a skill's call
/// (`look_for`, `skills/look-for-things`) answered with `"look": true` asks
/// the coordinator for the turn's full look, as on the Live route, and the
/// turn ends with the lamp saying it will look, with no round after it. An
/// answer without the look, a call that is not a skill's, and the switch
/// off ask for none, and the model is asked again with the answer as before.
#[tokio::test]
async fn a_skill_answer_with_a_look_asks_the_coordinator_for_the_turns_look() {
    let looking = r#"{"next":"Say only \"Let me take a look.\" now.","look":true}"#;
    let plain = r#"{"next":"Done."}"#;
    let cases = [
        ("skill, look, switch on", "look_for", looking, true, true),
        ("skill, no look, switch on", "look_for", plain, true, false),
        (
            "not a skill, look, switch on",
            "light_on",
            looking,
            true,
            false,
        ),
        ("skill, look, switch off", "look_for", looking, false, false),
    ];
    for (case, name, answer, on, looks) in cases {
        let script = Arc::new(Script {
            heard: Mutex::new([Ok("Where are my keys?".to_owned())].into()),
            chats: Mutex::new(
                [
                    vec![
                        Ok(ChatEvent::ToolCalls(vec![call(
                            "c1",
                            name,
                            json!({"thing": "keys"}),
                        )])),
                        Ok(ChatEvent::Done),
                    ],
                    vec![text("All done."), Ok(ChatEvent::Done)],
                ]
                .into(),
            ),
            ..Script::default()
        });
        let answer: Value = serde_json::from_str(answer).unwrap();
        let tools: ToolRunner = Arc::new(move |_, _, _, _| {
            let answer = answer.clone();
            Box::pin(async move { answer })
        });
        let mut engine = Engine::with_clock(
            script.clone(),
            Arc::new(|| "SYSTEM".to_owned().into()),
            tools,
            Arc::new(Instant::now),
        )
        .looking_by_call();
        if on {
            engine = engine.looking_for_skills(Arc::new(|name| name == "look_for"));
        }
        engine.submit(start(1));
        engine.submit(audio(1, 1));
        engine.submit(end(1));
        let mut asked = None;
        let mut said = Vec::new();
        loop {
            let event = tokio::time::timeout(Duration::from_secs(2), engine.next())
                .await
                .expect("the turn ends");
            match event {
                ProviderOutput::LookAsked {
                    request,
                    session,
                    dropped,
                } => asked = Some((request, session, dropped)),
                ProviderOutput::Transcript {
                    request: Some(_),
                    text,
                    ..
                } if !text.is_empty() => said.push(text),
                ProviderOutput::TurnComplete { .. } => break,
                _ => {}
            }
        }
        let rounds = script.asked.lock().unwrap().len();
        if looks {
            // On the session that began the turn, for the coordinator's look.
            assert_eq!(asked, Some((1, 1, None)), "{case}");
            assert_eq!(said, [LOOKING], "{case}");
            assert_eq!(rounds, 1, "{case}: no round after the look");
        } else {
            assert_eq!(asked, None, "{case}");
            assert_eq!(said, ["All done."], "{case}");
            assert_eq!(rounds, 2, "{case}");
        }
    }
}

/// An engine whose every tool answers `answer`, looking by its call and
/// for `look_for`'s skill, with an eye that always has a picture.
fn skill_looking(script: Arc<Script>, answer: &str) -> Engine {
    let answer: Value = serde_json::from_str(answer).unwrap();
    let tools: ToolRunner = Arc::new(move |_, _, _, _| {
        let answer = answer.clone();
        Box::pin(async move { answer })
    });
    Engine::with_clock(
        script,
        Arc::new(|| "SYSTEM".to_owned().into()),
        tools,
        Arc::new(Instant::now),
    )
    .looking_by_call()
    .looking_for_skills(Arc::new(|name| name == "look_for"))
    .looking(Arc::new(|| Ok(vec![0xFF, 0xD8, 0xFF])))
}

/// The turn's `LookAsked` and words said, through its last event.
async fn look_and_words(engine: &mut Engine) -> (Option<(u64, u64, Option<String>)>, Vec<String>) {
    let mut asked = None;
    let mut said = Vec::new();
    loop {
        let event = tokio::time::timeout(Duration::from_secs(2), engine.next())
            .await
            .expect("the turn ends");
        match event {
            ProviderOutput::LookAsked {
                request,
                session,
                dropped,
            } => asked = Some((request, session, dropped)),
            ProviderOutput::Transcript {
                request: Some(_),
                text,
                ..
            } if !text.is_empty() => said.push(text),
            ProviderOutput::TurnComplete { .. } => break,
            _ => {}
        }
    }
    (asked, said)
}

/// The reviewer on #2218 (P2 1): a skill's say for its picture (`told`'s
/// "When the picture comes: …") reaches the look turn's model, as it stays
/// in Gemini's session on Live: after the look turn's question, in the same
/// user message as the picture, once. The look turn after it has none, nor
/// a look turn after another turn began.
#[tokio::test]
async fn a_skills_note_for_its_picture_goes_once_with_its_look_turn() {
    const NOTE: &str = "say where it is by something near it.";
    let looked = || {
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "c1",
                "look_for",
                json!({"thing": "keys"}),
            )])),
            Ok(ChatEvent::Done),
        ]
    };
    let said = |words: &str| vec![text(words), Ok(ChatEvent::Done)];
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok("Where are my keys?".to_owned()),
                Ok("Where is my phone?".to_owned()),
                Ok("Never mind.".to_owned()),
            ]
            .into(),
        ),
        chats: Mutex::new(
            [
                looked(),
                said("By the mug."),
                said("A desk."),
                looked(),
                said("All right."),
                said("Nothing there."),
            ]
            .into(),
        ),
        ..Script::default()
    });
    let answer = format!(
        r#"{{"next":"Say only \"Let me take a look.\" now, in those words. When the picture comes: {NOTE}","look":true}}"#
    );
    let mut engine = skill_looking(script.clone(), &answer);
    let look = |request| ProviderInput::Written {
        request,
        session: 1,
        privacy_generation: 1,
        text: crate::speak_up::LOOK_FULL_NOTE.into(),
        look: true,
    };
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    assert_eq!(look_and_words(&mut engine).await.0, Some((1, 1, None)));
    engine.submit(look(2));
    look_and_words(&mut engine).await;
    engine.submit(look(3));
    look_and_words(&mut engine).await;
    // Another line begins between the look and its turn: the note goes.
    engine.submit(start(4));
    engine.submit(audio(4, 1));
    engine.submit(end(4));
    assert_eq!(look_and_words(&mut engine).await.0, Some((4, 1, None)));
    engine.submit(start(5));
    engine.submit(audio(5, 1));
    engine.submit(end(5));
    look_and_words(&mut engine).await;
    engine.submit(look(6));
    look_and_words(&mut engine).await;
    let asked = script.asked.lock().unwrap();
    assert_eq!(asked.len(), 6);
    let jpeg = [0xFF, 0xD8, 0xFF];
    assert_eq!(
        asked[1].last(),
        Some(&Message::user_seeing(
            format!(
                "{}\n\n(The skill you called for this look said what to do with the picture: {NOTE})",
                crate::speak_up::LOOK_FULL_NOTE
            ),
            &jpeg
        ))
    );
    let plain = Message::user_seeing(crate::speak_up::LOOK_FULL_NOTE, &jpeg);
    assert_eq!(asked[2].last(), Some(&plain), "once");
    assert_eq!(
        asked[5].last(),
        Some(&plain),
        "let go as another turn began"
    );
}

/// A skill's answer's note for its picture: the words after "When the
/// picture comes:", bounded and cut at a character's end; none without them.
#[test]
fn a_skills_note_for_its_picture_is_read_from_its_answer() {
    let next = |next: &str| json!({ "next": next, "look": true });
    assert_eq!(
        picture_note(&next(
            "Say only \"Let me take a look.\" now. When the picture comes:  say what you see. "
        )),
        Some("say what you see.".to_owned())
    );
    assert_eq!(
        picture_note(&next(
            "Say only \"Let me take a look.\" now. The picture comes after it."
        )),
        None
    );
    assert_eq!(picture_note(&next("When the picture comes:   ")), None);
    assert_eq!(picture_note(&json!({ "look": true })), None);
    assert_eq!(picture_note(&json!({ "next": 3 })), None);
    let long = picture_note(&next(&format!(
        "When the picture comes: {}",
        "é".repeat(800)
    )))
    .unwrap();
    assert!(long.len() <= PICTURE_NOTE_MAX);
    assert_eq!(long, "é".repeat(PICTURE_NOTE_MAX / 2));
}

/// The reviewer on #2218 (P2 2): in a turn the lamp wrote itself, a
/// skill's look is dropped with its why, and the turn goes on: the next
/// round's words are said, as with the switch off.
#[tokio::test]
async fn a_skills_look_dropped_in_a_written_turn_goes_on_to_the_next_round() {
    let script = Arc::new(Script {
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c1",
                        "look_for",
                        json!({"thing": "keys"}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("There, by the mug."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let mut engine = skill_looking(
        script.clone(),
        r#"{"next":"Say only \"Let me take a look.\" now. When the picture comes: say where it is.","look":true}"#,
    );
    engine.submit(written(1, "(Tell them where the keys are.)"));
    let (asked, said) = look_and_words(&mut engine).await;
    assert_eq!(
        asked,
        Some((
            1,
            1,
            Some("the turn is one the lamp wrote itself".to_owned())
        ))
    );
    assert_eq!(said, ["There, by the mug."]);
    assert_eq!(script.asked.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn memory_scope_is_bound_before_answer_and_revoked_with_the_turn() {
    use crate::memory_scope::{FaceCache, ScopeLease};
    let path = std::env::temp_dir().join(format!(
        "sixth-scope-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&path, "A").unwrap();
    let cache = Arc::new(FaceCache::default());
    cache.replace(
        cache.begin_refresh(),
        crate::known_faces::FaceObservation::read(&path),
    );
    let (engine, _, _) = scripted(Arc::new(Script::default()));
    let runner: ScopedToolRunner =
        Arc::new(|_, _, _, _, _: ScopeLease| Box::pin(async { json!({"ok":true}) }));
    let mut engine = engine.with_memory_scope(
        lamp_turn_owner::BootId::new([9; 16]).unwrap(),
        cache.clone(),
        runner,
        Some(Arc::new(|_: &crate::memory_scope::ScopeLease| {
            Told::from("SYSTEM".to_owned())
        })),
    );
    engine.memory_camera(true, 1);
    cache.replace(
        cache.begin_refresh(),
        crate::known_faces::FaceObservation::read(&path),
    );
    engine.submit(start(1));
    let lease = engine
        .turn
        .as_ref()
        .unwrap()
        .memory_scope
        .as_ref()
        .unwrap()
        .lease();
    assert_eq!(
        lease.scope().unwrap().person(),
        Some(crate::remember::person_id("A").as_str())
    );
    // No answer or tool has run yet. A later observation cannot rename this turn.
    std::fs::write(&path, "B").unwrap();
    cache.replace(
        cache.begin_refresh(),
        crate::known_faces::FaceObservation::read(&path),
    );
    engine.submit(end(1));
    assert!(lease.is_current(), "End closes input, not tool authority");
    assert_eq!(
        lease.scope().unwrap().person(),
        Some(crate::remember::person_id("A").as_str())
    );
    engine.submit(start(2));
    assert!(
        !lease.is_current(),
        "replacement revokes detached tool leases"
    );
    let next = engine
        .turn
        .as_ref()
        .unwrap()
        .memory_scope
        .as_ref()
        .unwrap()
        .lease();
    assert_eq!(
        next.scope().unwrap().person(),
        Some(crate::remember::person_id("B").as_str())
    );
    engine.memory_camera(true, 3);
    assert!(
        !next.is_current(),
        "coalesced camera close/reopen revokes identity"
    );
    assert_eq!(cache.snapshot(std::time::Instant::now()).person(), None);
    engine.retire_through(2);
    assert!(!next.is_current());
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn dropping_engine_revokes_memory_scope_even_when_task_holds_a_lease() {
    let (engine, _, _) = scripted(Arc::new(Script::default()));
    let runner: ScopedToolRunner = Arc::new(|_, _, _, _, _| Box::pin(async { json!({"ok":true}) }));
    let mut engine = engine.with_memory_scope(
        lamp_turn_owner::BootId::new([10; 16]).unwrap(),
        Arc::default(),
        runner,
        Some(Arc::new(|_: &crate::memory_scope::ScopeLease| {
            Told::from("SYSTEM".to_owned())
        })),
    );
    engine.submit(start(1));
    let lease = engine
        .turn
        .as_ref()
        .unwrap()
        .memory_scope
        .as_ref()
        .unwrap()
        .lease();
    assert_eq!(lease.scope().unwrap().person(), None);
    drop(engine);
    assert!(!lease.is_current());
}

#[tokio::test]
async fn revoked_tool_result_is_not_sent_to_another_chat_round() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Recall my note".to_owned())].into()),
        chats: Mutex::new(
            [vec![
                Ok(ChatEvent::ToolCalls(vec![call(
                    "private",
                    "recall",
                    json!({}),
                )])),
                Ok(ChatEvent::Done),
            ]]
            .into(),
        ),
        ..Script::default()
    });
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let runner: ScopedToolRunner = {
        let entered = entered.clone();
        let release = release.clone();
        Arc::new(move |_, _, _, _, _| {
            let entered = entered.clone();
            let release = release.clone();
            Box::pin(async move {
                entered.notify_one();
                release.notified().await;
                json!({"private":"must never reach next chat"})
            })
        })
    };
    let (engine, _, _) = scripted(script.clone());
    let mut engine = engine
        .with_memory_scope(
            lamp_turn_owner::BootId::new([11; 16]).unwrap(),
            Arc::default(),
            runner,
            Some(Arc::new(|_: &crate::memory_scope::ScopeLease| {
                Told::from("SYSTEM".to_owned())
            })),
        )
        .announcing(false);
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    tokio::time::timeout(Duration::from_secs(2), entered.notified())
        .await
        .unwrap();
    engine
        .turn
        .as_ref()
        .unwrap()
        .memory_scope
        .as_ref()
        .unwrap()
        .revoke();
    release.notify_one();
    tokio::time::timeout(
        Duration::from_secs(2),
        &mut engine.turn.as_mut().unwrap().task,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(script.asked.lock().unwrap().len(), 1);
}

/// #1712, lamp-4ace 19 Oct 15:56: "what's the weather" called a search, and
/// the next line, 4 s later, was held as not for the lamp; it had ended the
/// turn still searching, so the weather was never said. Now the words cut
/// short are answered in the held line's place; a line let out for the lamp
/// lets them go.
#[tokio::test]
async fn words_cut_short_by_a_held_line_are_answered_in_its_place() {
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok("What's the weather?".to_owned()),
                Ok("Yes, give me five minutes.".to_owned()),
            ]
            .into(),
        ),
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c1",
                        "web_search",
                        json!({"query": "weather"}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("It is 31 degrees and sunny."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (searching, searched) = std::sync::mpsc::channel();
    let runner: ToolRunner = Arc::new(move |_, _, _, _| {
        let searching = searching.clone();
        Box::pin(async move {
            let _ = searching.send(());
            tokio::time::sleep(Duration::from_secs(30)).await;
            json!({"results": []})
        })
    });
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        runner,
        Arc::new(Instant::now),
    )
    .announcing(false)
    .heeding(Arc::new(|request, _, _, _| {
        (request == 2).then_some("not said to the lamp")
    }));
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    // Until the search is under way.
    loop {
        if searched.try_recv().is_ok() {
            break;
        }
        let _ = tokio::time::timeout(Duration::from_millis(20), engine.next()).await;
    }
    // The next line begins, as the coordinator says, and is held.
    engine.submit(start(2));
    engine.retire_through(1);
    engine.submit(audio(2, 1));
    engine.submit(end(2));
    let mut said = Vec::new();
    loop {
        let event = tokio::time::timeout(Duration::from_secs(2), engine.next())
            .await
            .expect("the held line's turn ends");
        match event {
            ProviderOutput::Transcript {
                request: Some(2),
                text,
                ..
            } if !text.is_empty() => said.push(text),
            ProviderOutput::TurnComplete { request: 2, .. } => break,
            _ => {}
        }
    }
    assert_eq!(said, ["It is 31 degrees and sunny."]);
    let asked = script.asked.lock().unwrap();
    assert_eq!(
        asked[1].last().unwrap(),
        &Message::user("What's the weather?"),
        "the words cut short are asked again"
    );
}

/// #1712: a line let out for the lamp is answered itself, and the words it
/// cut short are let go: the person has moved on.
#[tokio::test]
async fn words_cut_short_by_a_line_let_out_are_let_go() {
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok("What's the weather?".to_owned()),
                Ok("Lamp, what time is it?".to_owned()),
            ]
            .into(),
        ),
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c1",
                        "web_search",
                        json!({"query": "weather"}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("It's noon."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (searching, searched) = std::sync::mpsc::channel();
    let runner: ToolRunner = Arc::new(move |_, _, _, _| {
        let searching = searching.clone();
        Box::pin(async move {
            let _ = searching.send(());
            tokio::time::sleep(Duration::from_secs(30)).await;
            json!({"results": []})
        })
    });
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        runner,
        Arc::new(Instant::now),
    )
    .announcing(false);
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    loop {
        if searched.try_recv().is_ok() {
            break;
        }
        let _ = tokio::time::timeout(Duration::from_millis(20), engine.next()).await;
    }
    engine.submit(start(2));
    engine.retire_through(1);
    engine.submit(audio(2, 1));
    engine.submit(end(2));
    let mut said = Vec::new();
    loop {
        let event = tokio::time::timeout(Duration::from_secs(2), engine.next())
            .await
            .expect("the turn ends");
        match event {
            ProviderOutput::Transcript {
                request: Some(2),
                text,
                ..
            } if !text.is_empty() => said.push(text),
            ProviderOutput::TurnComplete { request: 2, .. } => break,
            _ => {}
        }
    }
    assert_eq!(said, ["It's noon."]);
    assert_eq!(
        script.asked.lock().unwrap()[1].last().unwrap(),
        &Message::user("Lamp, what time is it?")
    );
}

/// A frame of the line's sound read at `at_us`.
fn audio_at(request: u64, sequence: u64, at_us: u64) -> ProviderInput {
    ProviderInput::Audio {
        request,
        privacy_generation: 1,
        sequence,
        read_completed_at_us: at_us,
        samples: vec![0; lamp_audio::CAPTURE_SAMPLES],
    }
}

/// #1712, the owner on lamp-4ace at 16:37: "Lamp, … what's the weather?"
/// came as two lines of about 1.3 s, each its own speech-to-text stream,
/// and the one with the name got no words. A line begun right after one
/// not answered, in the same session, has that line's sound sent first,
/// and its window begins where that line's did. Not after 2 s, and not
/// after a line that was answered.
#[tokio::test]
async fn a_line_going_on_from_one_unanswered_hears_its_sound_first() {
    let frame = lamp_audio::CAPTURE_SAMPLES;
    let run = |first: &'static str, gap: Duration| async move {
        let script = Arc::new(Script {
            heard: Mutex::new([Ok(first.to_owned()), Ok("what's the weather?".to_owned())].into()),
            chats: Mutex::new(
                [
                    vec![text("Noon."), Ok(ChatEvent::Done)],
                    vec![text("Sunny."), Ok(ChatEvent::Done)],
                ]
                .into(),
            ),
            ..Script::default()
        });
        let (engine, _, now) = scripted(script.clone());
        let windows: Arc<Mutex<Vec<(u64, u64)>>> = Arc::default();
        let seen = windows.clone();
        let mut engine = engine.heeding(Arc::new(move |request, window, _, _| {
            seen.lock().unwrap().push((request, window.0));
            None
        }));
        engine.submit(start(1));
        engine.submit(audio_at(1, 1, 1_000));
        engine.submit(end(1));
        assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
        *now.lock().unwrap() += gap;
        engine.submit(start(2));
        engine.submit(audio_at(2, 1, 9_000));
        engine.submit(end(2));
        assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
        let heard = script.samples_heard.load(Ordering::Relaxed);
        let window = windows
            .lock()
            .unwrap()
            .iter()
            .find(|(of, _)| *of == 2)
            .map(|(_, from)| *from);
        (heard, window)
    };
    // No words in the first line: the name's piece, unanswered.
    assert_eq!(
        run("", Duration::from_millis(500)).await,
        (3 * frame, Some(1_000)),
        "its sound, then this line's, and its window from the first piece"
    );
    assert_eq!(
        run("", Duration::from_secs(3)).await,
        (2 * frame, Some(9_000)),
        "too late"
    );
    assert_eq!(
        run("What time is it?", Duration::from_millis(500)).await,
        (2 * frame, Some(9_000)),
        "the first line was answered"
    );
}

/// #1712: each spoken line is asked with the note the hook gives, beside
/// its words: answer it when said by name, else it may be declined.
#[tokio::test]
async fn each_spoken_line_is_asked_with_its_call_note() {
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok("Lamp, what time is it?".to_owned()),
                Ok("Okay yeah sure.".to_owned()),
            ]
            .into(),
        ),
        chats: Mutex::new(
            [
                vec![text("Noon."), Ok(ChatEvent::Done)],
                vec![Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script.clone());
    let mut engine = engine.noting_whether_called(Arc::new(|_, words| {
        Some(worker::call_note(crate::attention::called_by_name_on(
            words, true,
        )))
    }));
    for request in 1..=2 {
        engine.submit(start(request));
        engine.submit(audio(request, 1));
        engine.submit(end(request));
        assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
    }
    let asked = script.asked.lock().unwrap();
    assert_eq!(
        asked[0],
        [
            Message::system("SYSTEM"),
            Message::system(worker::BY_NAME_NOTE),
            Message::user("Lamp, what time is it?"),
        ]
    );
    assert!(asked[1].contains(&Message::system(worker::DECLINE_NOTE)));
    assert!(!asked[1].contains(&Message::system(worker::BY_NAME_NOTE)));
}

/// #1712, the first builder at 18:56: only a piece that came back with no
/// words carries its sound, and at most its last 1.5 s. A piece whose words
/// came back, even held, carries nothing; a long wordless piece carries its
/// last 1.5 s alone.
#[tokio::test]
async fn only_a_wordless_piece_carries_and_only_its_last_moments() {
    let frame = lamp_audio::CAPTURE_SAMPLES;
    let run = |first: &'static str, frames: u64| async move {
        let script = Arc::new(Script {
            heard: Mutex::new([Ok(first.to_owned()), Ok("what's the weather?".to_owned())].into()),
            chats: Mutex::new([vec![text("Sunny."), Ok(ChatEvent::Done)]].into()),
            ..Script::default()
        });
        let (engine, _, _) = scripted(script.clone());
        // The first piece is held, so it is never answered.
        let mut engine = engine.heeding(Arc::new(|request, _, _, _| {
            (request == 1).then_some("not said to the lamp")
        }));
        engine.submit(start(1));
        for sequence in 1..=frames {
            engine.submit(audio_at(1, sequence, 1_000));
        }
        engine.submit(end(1));
        assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
        engine.submit(start(2));
        engine.submit(audio_at(2, 1, 9_000));
        engine.submit(end(2));
        assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
        script.samples_heard.load(Ordering::Relaxed) - frames as usize * frame
    };
    // The second line's own stream: with a wordless first piece, its sound
    // first, then its own frame.
    assert_eq!(run("", 1).await, 2 * frame, "a wordless piece carries");
    assert_eq!(
        run("Okay yeah sure.", 1).await,
        frame,
        "words came back: nothing carried"
    );
    let long = 200; // 2 s of sound
    assert_eq!(
        run("", long).await,
        KEPT_SAMPLES + frame,
        "its last 1.5 s, then its own"
    );
}

/// #2002's P1 (the first builder at 01:23): with the next line's socket
/// kept ready, "Lamp, [600 ms] what time is it?" came back as "Lamp", held
/// as a short sound, and "what time is it?", held for no name. The name's
/// words now begin the next line, which is judged and asked whole and
/// answered; the piece's sound isn't sent again, and its time is the
/// line's. A piece with other words still carries nothing.
#[tokio::test]
async fn the_name_cut_off_by_a_pause_begins_the_next_line_with_the_listener_kept_ready() {
    let frame = lamp_audio::CAPTURE_SAMPLES;
    let run = |first: &'static str| async move {
        let script = Arc::new(Script {
            heard: Mutex::new([Ok(first.to_owned()), Ok("what time is it?".to_owned())].into()),
            chats: Mutex::new(
                [
                    vec![text("Noon."), Ok(ChatEvent::Done)],
                    vec![text("Noon."), Ok(ChatEvent::Done)],
                ]
                .into(),
            ),
            ..Script::default()
        });
        let (engine, _, now) = scripted(script.clone());
        let judged: Arc<Mutex<Vec<(u64, u64, String)>>> = Arc::default();
        let seen = judged.clone();
        // The name piece is held, as a short sound was on lamp-4ace; any
        // other line by its words, as the rule holds a line with no name.
        let mut engine = engine
            .heeding(Arc::new(move |request, window, words, _| {
                seen.lock()
                    .unwrap()
                    .push((request, window.0, words.to_owned()));
                if request == 1 {
                    Some("a short sound")
                } else {
                    (!crate::attention::called_by_name(words)).then_some("no name")
                }
            }))
            .keeping_a_listener_ready(true);
        engine.submit(start(1));
        engine.submit(audio_at(1, 1, 1_000));
        engine.submit(end(1));
        assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
        *now.lock().unwrap() += Duration::from_millis(600);
        engine.submit(start(2));
        engine.submit(audio_at(2, 1, 9_000));
        engine.submit(end(2));
        let second = turn(&mut engine).await;
        let judged = judged.lock().unwrap().clone();
        let asked = script.asked.lock().unwrap().last().cloned();
        (
            second,
            judged,
            asked,
            script.samples_heard.load(Ordering::Relaxed),
        )
    };
    let (second, judged, asked, heard) = run("Lamp.").await;
    assert!(second.contains(&"said:Noon.".to_owned()), "{second:?}");
    assert_eq!(judged[1], (2, 1_000, "Lamp. what time is it?".to_owned()));
    assert_eq!(
        asked.unwrap().last(),
        Some(&Message::user("Lamp. what time is it?"))
    );
    assert_eq!(heard, 2 * frame, "each piece's own sound, once");
    // Other words came back: nothing carried, and the line is held.
    let (second, judged, _, heard) = run("Okay.").await;
    assert!(
        !second.iter().any(|label| label.starts_with("said:")),
        "{second:?}"
    );
    assert_eq!(judged[1], (2, 9_000, "what time is it?".to_owned()));
    assert_eq!(heard, 2 * frame);
}

/// #1712, the first builder at 19:21: a wordless piece whose name lamp-name
/// heard gives the next line its time, so the name counts, but not its
/// sound, which held the words back about 2 s. Without the name heard, the
/// sound goes again for speech to text to find it.
#[tokio::test]
async fn a_wordless_piece_with_the_name_heard_lends_its_time_not_its_sound() {
    let frame = lamp_audio::CAPTURE_SAMPLES;
    let run = |name_heard: bool| async move {
        let script = Arc::new(Script {
            heard: Mutex::new([Ok(String::new()), Ok("what's the weather?".to_owned())].into()),
            chats: Mutex::new([vec![text("Sunny."), Ok(ChatEvent::Done)]].into()),
            ..Script::default()
        });
        let (engine, _, _) = scripted(script.clone());
        let windows: Arc<Mutex<Vec<(u64, u64)>>> = Arc::default();
        let seen = windows.clone();
        let asked_from: Arc<Mutex<Vec<u64>>> = Arc::default();
        let asked = asked_from.clone();
        let mut engine = engine
            .heeding(Arc::new(move |request, window, _, _| {
                seen.lock().unwrap().push((request, window.0));
                None
            }))
            .knowing_the_name_heard(Arc::new(move |from_us| {
                asked.lock().unwrap().push(from_us);
                name_heard
            }));
        engine.submit(start(1));
        engine.submit(audio_at(1, 1, 1_000));
        engine.submit(end(1));
        assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
        engine.submit(start(2));
        engine.submit(audio_at(2, 1, 9_000));
        engine.submit(end(2));
        assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
        let window = windows
            .lock()
            .unwrap()
            .iter()
            .find(|(of, _)| *of == 2)
            .map(|(_, from)| *from);
        let asked = asked_from.lock().unwrap().clone();
        (script.samples_heard.load(Ordering::Relaxed), window, asked)
    };
    assert_eq!(
        run(true).await,
        (2 * frame, Some(1_000), vec![1_000]),
        "the name heard: its time, not its sound"
    );
    assert_eq!(
        run(false).await,
        (3 * frame, Some(1_000), vec![1_000]),
        "no name heard: its sound goes again"
    );
}

/// #1675: a scoped turn is told for its admitted person, not for whoever the
/// eyes see when its chat begins; a written turn is told as the room's.
#[tokio::test]
async fn scoped_system_is_built_for_the_admitted_scope_not_the_face_in_view() {
    use crate::memory_scope::{FaceCache, ScopeLease};
    let path = std::env::temp_dir().join(format!(
        "sixth-scoped-system-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let observe = |cache: &FaceCache, name: &str| {
        std::fs::write(&path, name).unwrap();
        cache.replace(
            cache.begin_refresh(),
            crate::known_faces::FaceObservation::read(&path),
        );
    };
    let cache = Arc::new(FaceCache::default());
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("What is on my list?".to_owned())].into()),
        chats: Mutex::new(
            [
                vec![text("Tea."), Ok(ChatEvent::Done)],
                vec![text("Noted."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script.clone());
    let runner: ScopedToolRunner =
        Arc::new(|_, _, _, _, _: ScopeLease| Box::pin(async { json!({"ok":true}) }));
    let mut engine = engine
        .with_memory_scope(
            lamp_turn_owner::BootId::new([12; 16]).unwrap(),
            cache.clone(),
            runner,
            Some(Arc::new(|lease: &ScopeLease| {
                Told::from(format!("for {:?}", lease.scope().unwrap().person()))
            })),
        )
        .announcing(false);
    engine.memory_camera(true, 1);
    observe(&cache, "A");
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    // B steps in front of the lamp before the chat is asked.
    observe(&cache, "B");
    engine.submit(end(1));
    turn(&mut engine).await;
    engine.submit(written(2, "Remind me later."));
    turn(&mut engine).await;
    let asked = script.asked.lock().unwrap();
    let a = crate::remember::person_id("A");
    assert_eq!(
        asked[0][0],
        Message::system(format!("for {:?}", Some(a.as_str())))
    );
    assert_eq!(asked[1][0], Message::system("for None"));
    std::fs::remove_file(path).unwrap();
}

/// A Start marked as the coordinator marks it: begun over the lamp's
/// audible answer, the first after a stop, soon after the lamp spoke.
fn start_marked(
    request: u64,
    over_the_voice: bool,
    after_stop: bool,
    follows_on: bool,
) -> ProviderInput {
    ProviderInput::Start {
        request,
        session: 1,
        privacy_generation: 1,
        continues: false,
        continues_cut_off: false,
        over_the_lamp: over_the_voice,
        after_stop,
        follows_on,
        over_the_lamps_voice: over_the_voice,
        heard_of_cut: None,
    }
}

/// One spoken line heard as `words` and its chat's messages, with `marks`
/// sent as the coordinator sends them, before its audio and End.
async fn asked_with(
    words: &str,
    engine: impl FnOnce(Engine) -> Engine,
    start: ProviderInput,
    called: bool,
    sound: bool,
) -> Vec<Vec<Message>> {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(words.to_owned())].into()),
        chats: Mutex::new([vec![text("Yes?"), Ok(ChatEvent::Done)]].into()),
        ..Script::default()
    });
    let (built, _, _) = scripted(script.clone());
    let mut engine = engine(built);
    engine.submit(start);
    engine.submit(audio(1, 1));
    if called {
        engine.submit(ProviderInput::Called {
            request: 1,
            privacy_generation: 1,
        });
    }
    engine.submit(ProviderInput::End {
        request: 1,
        privacy_generation: 1,
        sound,
        cut_in: false,
    });
    turn(&mut engine).await;
    script.asked.lock().unwrap().clone()
}

/// #1712, item 4 of the first builder's 20:15 (`LAMP_CUT_IN_TOLD`): a line
/// begun over the lamp's audible answer is told it cut in, beside its
/// words, as the Live route tells it; not without the switch, and the
/// first after a stop is told the stop instead.
#[tokio::test]
async fn a_line_begun_over_the_lamps_answer_is_told_it_cut_in() {
    let cut_in = Message::system(crate::cut_in_told::NOTE);
    let stopped = Message::system(crate::provider_worker::STOPPED_NOTE);
    let told = asked_with(
        "Okay, that is enough",
        |engine| engine.telling_cut_ins(true),
        start_marked(1, true, false, false),
        false,
        false,
    )
    .await;
    assert_eq!(told[0][told[0].len() - 2], cut_in, "{:?}", told[0]);
    assert_eq!(told[0].last(), Some(&Message::user("Okay, that is enough")));
    let off = asked_with(
        "Okay, that is enough",
        |engine| engine,
        start_marked(1, true, false, false),
        false,
        false,
    )
    .await;
    assert!(!off[0].contains(&cut_in), "{:?}", off[0]);
    let quiet = asked_with(
        "Okay, that is enough",
        |engine| engine.telling_cut_ins(true),
        start_marked(1, false, false, false),
        false,
        false,
    )
    .await;
    assert!(!quiet[0].contains(&cut_in), "{:?}", quiet[0]);
    let after_stop = asked_with(
        "Okay, that is enough",
        |engine| engine.telling_cut_ins(true),
        start_marked(1, true, true, false),
        false,
        false,
    )
    .await;
    assert!(!after_stop[0].contains(&cut_in), "{:?}", after_stop[0]);
    assert!(after_stop[0].contains(&stopped), "{:?}", after_stop[0]);
}

/// #1712, item 4 (`LAMP_FOLLOWS_ON`): a line begun soon after the lamp's
/// words finished is told it may follow on from them, as the Live route
/// tells it; not when its End marks a sound, nor when it was judged in a
/// busy room.
#[tokio::test]
async fn a_line_soon_after_the_lamp_spoke_is_told_it_may_follow_on() {
    let follows = Message::system(crate::follows_on::NOTE);
    let told = asked_with(
        "And what do you like about it",
        |engine| engine,
        start_marked(1, false, false, true),
        false,
        false,
    )
    .await;
    assert_eq!(told[0][told[0].len() - 2], follows, "{:?}", told[0]);
    let unmarked = asked_with(
        "And what do you like about it",
        |engine| engine,
        start(1),
        false,
        false,
    )
    .await;
    assert!(!unmarked[0].contains(&follows), "{:?}", unmarked[0]);
    let sound = asked_with(
        "And what do you like about it",
        |engine| engine,
        start_marked(1, false, false, true),
        false,
        true,
    )
    .await;
    assert!(!sound[0].contains(&follows), "{:?}", sound[0]);
    let busy = asked_with(
        "And what do you like about it",
        |engine| engine.judging_busy(Arc::new(|request| request == 1)),
        start_marked(1, false, false, true),
        false,
        false,
    )
    .await;
    assert!(!busy[0].contains(&follows), "{:?}", busy[0]);
    let quiet = asked_with(
        "And what do you like about it",
        |engine| engine.judging_busy(Arc::new(|_| false)),
        start_marked(1, false, false, true),
        false,
        false,
    )
    .await;
    assert!(quiet[0].contains(&follows), "{:?}", quiet[0]);
}

/// #1712, item 4 (`ProviderInput::Called`, which this route dropped): a line
/// that was only the lamp's name, its call's wait run out, is told so, as
/// Gemini's model is; with no words of it back, as a lone name often has,
/// it is asked as the name, not left as a sound. Without `Called`, no words
/// are still no line.
#[tokio::test]
async fn only_the_lamps_name_is_told_so_and_asked_as_the_name_with_no_words() {
    let called = Message::system(crate::provider_worker::CALLED_NOTE);
    let told = asked_with("Lamp", |engine| engine, start(1), true, false).await;
    assert_eq!(told[0][told[0].len() - 2], called, "{:?}", told[0]);
    assert_eq!(told[0].last(), Some(&Message::user("Lamp")));
    let wordless = asked_with("", |engine| engine, start(1), true, false).await;
    assert_eq!(
        wordless[0][wordless[0].len() - 2],
        called,
        "{:?}",
        wordless[0]
    );
    assert_eq!(
        wordless[0].last(),
        Some(&Message::user(super::CALLED_WORDS))
    );
    let not_called = asked_with("Lamp", |engine| engine, start(1), false, false).await;
    assert!(!not_called[0].contains(&called), "{:?}", not_called[0]);
    assert!(
        asked_with("", |engine| engine, start(1), false, false)
            .await
            .is_empty()
    );
}

/// #1712, item 4, with `LAMP_SECOND_PREPARED`: a chat begun on a cut-in's
/// early words is asked with its note, so it is kept, not dropped as "a
/// note goes beside the words"; a note known only at the End (`Called`)
/// still drops it.
#[tokio::test(start_paused = true)]
async fn a_chat_begun_on_a_cut_ins_early_words_carries_its_note() {
    for (called, asked) in [(false, 1), (true, 2)] {
        let script = Arc::new(Script {
            heard: Mutex::new([Ok("Okay, that is enough".to_owned())].into()),
            early: Mutex::new([vec![net::Early::Words("okay that is enough".to_owned())]].into()),
            finish_after: Some(Duration::from_millis(400)),
            first_event_after: Some(Duration::from_millis(600)),
            chats: Mutex::new(
                [
                    vec![text("Of course."), Ok(ChatEvent::Done)],
                    vec![text("Of course."), Ok(ChatEvent::Done)],
                ]
                .into(),
            ),
            ..Script::default()
        });
        let (engine, _, _) = scripted(script.clone());
        let mut engine = engine.preparing(true).telling_cut_ins(true);
        engine.submit(start_marked(1, true, false, false));
        engine.submit(audio(1, 1));
        if called {
            engine.submit(ProviderInput::Called {
                request: 1,
                privacy_generation: 1,
            });
        }
        engine.submit(end(1));
        loop {
            let event = tokio::time::timeout(Duration::from_secs(5), engine.next())
                .await
                .expect("the turn ends");
            if matches!(event, ProviderOutput::TurnComplete { .. }) {
                break;
            }
        }
        let chats = script.asked.lock().unwrap();
        assert_eq!(chats.len(), asked, "called {called}: {chats:?}");
        let cut_in = Message::system(crate::cut_in_told::NOTE);
        assert!(chats.iter().all(|chat| chat.contains(&cut_in)), "{chats:?}");
    }
}

/// #1895 on the second route (the first builder, 19:49): B's first line,
/// said just after A's while the eyes' count would still say A, reaches
/// the memory as B's. Its memory call runs with B's admitted scope and its
/// chat is told for B, never for A. Deepgram and DeepSeek are the script.
#[tokio::test]
async fn b_first_line_after_a_reaches_the_memory_as_bs() {
    use crate::memory_scope::{FaceCache, ScopeLease};
    let path = std::env::temp_dir().join(format!(
        "sixth-b-first-line-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let observe = |cache: &FaceCache, name: &str| {
        std::fs::write(&path, name).unwrap();
        cache.replace(
            cache.begin_refresh(),
            crate::known_faces::FaceObservation::read(&path),
        );
    };
    let remember = || {
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "m1",
                "remember",
                json!({"text":"my test code is 2580"}),
            )])),
            Ok(ChatEvent::Done),
        ]
    };
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok("Lamp, remember that my test code is 417.".to_owned()),
                Ok("Lamp, remember that my test code is 2580.".to_owned()),
            ]
            .into(),
        ),
        chats: Mutex::new(
            [
                remember(),
                vec![text("Got it."), Ok(ChatEvent::Done)],
                remember(),
                vec![text("Got it."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script.clone());
    let cache = Arc::new(FaceCache::default());
    let scopes: Arc<Mutex<Vec<Option<String>>>> = Arc::default();
    let seen = scopes.clone();
    let runner: ScopedToolRunner = Arc::new(move |_, _, name, _, lease: ScopeLease| {
        assert_eq!(name, "remember");
        seen.lock()
            .unwrap()
            .push(lease.scope().unwrap().person().map(str::to_owned));
        Box::pin(async { json!({"ok":true}) })
    });
    let mut engine = engine
        .with_memory_scope(
            lamp_turn_owner::BootId::new([13; 16]).unwrap(),
            cache.clone(),
            runner,
            Some(Arc::new(|lease: &ScopeLease| {
                Told::from(format!("for {:?}", lease.scope().unwrap().person()))
            })),
        )
        .announcing(false);
    engine.memory_camera(true, 1);
    observe(&cache, "A");
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    turn(&mut engine).await;
    // B steps in; A's line is just over.
    observe(&cache, "B");
    engine.submit(start(2));
    engine.submit(audio(2, 1));
    engine.submit(end(2));
    turn(&mut engine).await;
    let a = crate::remember::person_id("A");
    let b = crate::remember::person_id("B");
    assert_eq!(*scopes.lock().unwrap(), [Some(a.clone()), Some(b.clone())]);
    let asked = script.asked.lock().unwrap();
    let b_told = Message::system(format!("for {:?}", Some(b.as_str())));
    let a_told = Message::system(format!("for {:?}", Some(a.as_str())));
    assert_eq!(asked[2][0], b_told, "B's first round is told for B");
    assert_eq!(asked[3][0], b_told, "and so is its round after the call");
    assert!(asked[2..].iter().all(|messages| messages[0] != a_told));
    std::fs::remove_file(path).unwrap();
}

/// Seeing someone speak (`LAMP_SECOND_SPEAKING_FACE=on`, the first builder
/// on #1722, 00:08 and 00:23): A is known at the lamp with face `f1`. A line
/// that `f2` is seen speaking through is told as the room's, and its memory
/// call runs with the room's scope; a line A's own face speaks stays A's;
/// one both faces speak through is the room's (Third on #2008).
/// Deepgram and DeepSeek are the script; the eyes are a feeder thread.
#[tokio::test]
async fn a_line_another_face_spoke_is_the_rooms_and_the_known_faces_stays_theirs() {
    use crate::memory_scope::{FaceCache, ScopeLease};
    use crate::speaking_faces::{SpeakingFaces, known_face_of};
    use std::sync::atomic::{AtomicBool, Ordering};
    let path = std::env::temp_dir().join(format!(
        "ten-speaking-face-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&path, "A").unwrap();
    std::fs::write(known_face_of(&path), "f1\n").unwrap();
    let remember = || {
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "m1",
                "remember",
                json!({"text":"my test code is 417"}),
            )])),
            Ok(ChatEvent::Done),
        ]
    };
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok("Lamp, remember that my test code is 417.".to_owned()),
                Ok("Lamp, remember that my test code is 417.".to_owned()),
                Ok("Lamp, remember that my test code is 417.".to_owned()),
            ]
            .into(),
        ),
        chats: Mutex::new(
            [
                remember(),
                vec![text("Got it."), Ok(ChatEvent::Done)],
                remember(),
                vec![text("Got it."), Ok(ChatEvent::Done)],
                remember(),
                vec![text("Got it."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script.clone());
    let cache = Arc::new(FaceCache::default());
    let scopes: Arc<Mutex<Vec<Option<String>>>> = Arc::default();
    let seen = scopes.clone();
    let runner: ScopedToolRunner = Arc::new(move |_, _, _, _, lease: ScopeLease| {
        seen.lock()
            .unwrap()
            .push(lease.scope().unwrap().person().map(str::to_owned));
        Box::pin(async { json!({"ok":true}) })
    });
    let eyes = Arc::new(SpeakingFaces::default());
    let mut engine = engine
        .with_memory_scope(
            lamp_turn_owner::BootId::new([14; 16]).unwrap(),
            cache.clone(),
            runner,
            Some(Arc::new(|lease: &ScopeLease| {
                Told::from(format!("for {:?}", lease.scope().unwrap().person()))
            })),
        )
        .seeing_who_speaks(eyes.clone(), path.clone())
        .announcing(false);
    engine.memory_camera(true, 1);
    cache.replace(
        cache.begin_refresh(),
        crate::known_faces::FaceObservation::read(&path),
    );
    for (line, speaking) in [(1, &["f2"][..]), (2, &["f1"]), (3, &["f1", "f2"])] {
        // The eyes, while the line is said: one face speaking, one not.
        let done = Arc::new(AtomicBool::new(false));
        let feeder = {
            let (eyes, done) = (eyes.clone(), done.clone());
            std::thread::spawn(move || {
                let mut frame = 0u64;
                while !done.load(Ordering::Acquire) {
                    let faces = lamp_msgs::Faces {
                        faces: ["f1", "f2"]
                            .map(|id| lamp_msgs::Face {
                                id: id.to_owned(),
                                xmin: 0.1,
                                ymin: 0.1,
                                xmax: 0.3,
                                ymax: 0.4,
                                confidence: 1.0,
                                yaw: None,
                                facing: None,
                                speaking: Some(lamp_msgs::Speaking {
                                    state: if speaking.contains(&id) {
                                        lamp_msgs::SpeakingState::Yes
                                    } else {
                                        lamp_msgs::SpeakingState::No
                                    },
                                    score: 0.9,
                                    from: None,
                                }),
                            })
                            .into(),
                    };
                    frame += 1;
                    eyes.look(Instant::now(), Some((("eyes", frame), &faces)));
                    std::thread::sleep(Duration::from_millis(2));
                }
            })
        };
        engine.submit(start(line));
        engine.submit(audio(line, 1));
        tokio::time::sleep(Duration::from_millis(40)).await;
        engine.submit(end(line));
        turn(&mut engine).await;
        done.store(true, Ordering::Release);
        feeder.join().unwrap();
    }
    let a = crate::remember::person_id("A");
    assert_eq!(*scopes.lock().unwrap(), [None, Some(a.clone()), None]);
    let asked = script.asked.lock().unwrap();
    let room = Message::system("for None");
    let a_told = Message::system(format!("for {:?}", Some(a.as_str())));
    assert_eq!(asked[0][0], room, "f2's line is told as the room's");
    assert_eq!(asked[1][0], room, "and so is its round after the call");
    assert_eq!(asked[2][0], a_told, "A's own face's line stays A's");
    assert_eq!(asked[4][0], room, "both faces' line is the room's");
    std::fs::remove_file(known_face_of(&path)).unwrap();
    std::fs::remove_file(path).unwrap();
}

/// The reviewer on #2008: the control for
/// `a_line_another_face_spoke_is_the_rooms_and_the_known_faces_stays_theirs`.
/// The same `f2` line, with the eyes seeing it, but without
/// `LAMP_SECOND_SPEAKING_FACE` (no `seeing_who_speaks`), stays A's, as
/// before: told for A, its memory call in A's scope.
#[tokio::test]
async fn without_the_switch_a_line_another_face_spoke_stays_the_known_persons() {
    use crate::memory_scope::{FaceCache, ScopeLease};
    use crate::speaking_faces::{SpeakingFaces, known_face_of};
    let path = std::env::temp_dir().join(format!(
        "ten-speaking-face-off-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&path, "A").unwrap();
    std::fs::write(known_face_of(&path), "f1\n").unwrap();
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Lamp, remember that my test code is 417.".to_owned())].into()),
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "m1",
                        "remember",
                        json!({"text":"my test code is 417"}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("Got it."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script.clone());
    let cache = Arc::new(FaceCache::default());
    let scopes: Arc<Mutex<Vec<Option<String>>>> = Arc::default();
    let seen = scopes.clone();
    let runner: ScopedToolRunner = Arc::new(move |_, _, _, _, lease: ScopeLease| {
        seen.lock()
            .unwrap()
            .push(lease.scope().unwrap().person().map(str::to_owned));
        Box::pin(async { json!({"ok":true}) })
    });
    let mut engine = engine
        .with_memory_scope(
            lamp_turn_owner::BootId::new([17; 16]).unwrap(),
            cache.clone(),
            runner,
            Some(Arc::new(|lease: &ScopeLease| {
                Told::from(format!("for {:?}", lease.scope().unwrap().person()))
            })),
        )
        .announcing(false);
    engine.memory_camera(true, 1);
    cache.replace(
        cache.begin_refresh(),
        crate::known_faces::FaceObservation::read(&path),
    );
    // The eyes see f2 speaking all through the line; nothing reads them.
    let eyes = SpeakingFaces::default();
    let speaking = lamp_msgs::Faces {
        faces: vec![lamp_msgs::Face {
            id: "f2".into(),
            xmin: 0.1,
            ymin: 0.1,
            xmax: 0.3,
            ymax: 0.4,
            confidence: 1.0,
            yaw: None,
            facing: None,
            speaking: Some(lamp_msgs::Speaking {
                state: lamp_msgs::SpeakingState::Yes,
                score: 0.9,
                from: None,
            }),
        }],
    };
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    for frame in 0..10u64 {
        eyes.look(Instant::now(), Some((("eyes", frame), &speaking)));
        tokio::time::sleep(Duration::from_millis(4)).await;
    }
    engine.submit(end(1));
    turn(&mut engine).await;
    let a = crate::remember::person_id("A");
    assert_eq!(*scopes.lock().unwrap(), [Some(a.clone())]);
    let asked = script.asked.lock().unwrap();
    assert_eq!(
        asked[0][0],
        Message::system(format!("for {:?}", Some(a.as_str())))
    );
    std::fs::remove_file(known_face_of(&path)).unwrap();
    std::fs::remove_file(path).unwrap();
}

/// Second on #2008: without a scoped builder (`LAMP_SECOND_SCOPED_CONTEXT`
/// off) a turn is told for the sticky person whatever its lease says, so
/// seeing who speaks is refused there and taken with one.
#[test]
fn seeing_who_speaks_needs_the_scoped_context() {
    use crate::memory_scope::{FaceCache, ScopeLease};
    let runner = || -> ScopedToolRunner {
        Arc::new(|_, _, _, _, _: ScopeLease| Box::pin(async { json!({"ok":true}) }))
    };
    let eyes = Arc::new(crate::speaking_faces::SpeakingFaces::default());
    let boot = lamp_turn_owner::BootId::new([15; 16]).unwrap();
    let (engine, _, _) = scripted(Arc::new(Script::default()));
    let engine = engine
        .with_memory_scope(boot, Arc::new(FaceCache::default()), runner(), None)
        .seeing_who_speaks(eyes.clone(), "/run/lampos-eyes/known".into());
    assert!(engine.shared.speaking.is_none());
    let (engine, _, _) = scripted(Arc::new(Script::default()));
    let scoped: ScopedSystem = Arc::new(|_: &ScopeLease| Told::from("scoped".to_owned()));
    let engine = engine
        .with_memory_scope(boot, Arc::new(FaceCache::default()), runner(), Some(scoped))
        .seeing_who_speaks(eyes, "/run/lampos-eyes/known".into());
    assert!(engine.shared.speaking.is_some());
}

/// #1712, item 4 (`LAMP_GOING_ON_ASKS`, #1522): a line the rule says waits
/// for the Policy is asked once the Policy's hook decides it is played, and
/// not at all when held; a hook that never decides lets it be played after
/// the bound, as with no decision.
#[tokio::test(start_paused = true)]
async fn a_line_that_waits_for_the_policy_goes_as_it_decides() {
    for (decides_after, says, asked) in [
        (Some(3), None, 1),
        (Some(3), Some("held"), 0),
        (None, None, 1),
    ] {
        let script = Arc::new(Script {
            heard: Mutex::new([Ok("And what about tomorrow".to_owned())].into()),
            chats: Mutex::new([vec![text("Rain."), Ok(ChatEvent::Done)]].into()),
            ..Script::default()
        });
        let (engine, _, _) = scripted(script.clone());
        let polled = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = polled.clone();
        let mut engine = engine
            .heeding(Arc::new(|_, _, _, _| Some(super::WAITS_FOR_POLICY)))
            .asking_the_policy(Arc::new(move |request| {
                assert_eq!(request, 1);
                let polls = counted.fetch_add(1, Ordering::SeqCst) + 1;
                decides_after.filter(|after| polls >= *after).map(|_| says)
            }));
        engine.submit(start(1));
        engine.submit(audio(1, 1));
        engine.submit(end(1));
        loop {
            let event = tokio::time::timeout(Duration::from_secs(10), engine.next())
                .await
                .expect("the turn ends");
            if matches!(event, ProviderOutput::TurnComplete { .. }) {
                break;
            }
        }
        assert_eq!(
            script.asked.lock().unwrap().len(),
            asked,
            "decides after {decides_after:?}, says {says:?}"
        );
        let polls = polled.load(Ordering::SeqCst);
        match decides_after {
            Some(after) => assert_eq!(polls, after),
            // Up to the bound, `policy_holds::WAIT` and four polls more.
            None => assert!((40..=46).contains(&polls), "{polls} polls"),
        }
    }
}

/// The Policy holding the lamp's actions on this route (#800, #1712).
mod policy;

/// #1712 (`LAMP_SECOND_STT_WARM`, the first builder at 00:25): with the
/// switch, each line's listener opens the next line's, so a line's sound
/// goes to a socket already open; one found closed since is not used, and
/// the line opens its own. Without it, one listener a line, as before.
#[tokio::test]
async fn the_next_lines_listener_is_kept_ready_and_one_closed_since_is_not_used() {
    let script = |lines: &[&str]| {
        Arc::new(Script {
            heard: Mutex::new(lines.iter().map(|line| Ok((*line).to_owned())).collect()),
            chats: Mutex::new(
                lines
                    .iter()
                    .map(|_| vec![text("Sure."), Ok(ChatEvent::Done)])
                    .collect(),
            ),
            ..Script::default()
        })
    };
    let lines = ["What time is it?", "And tomorrow?", "Thanks."];
    let kept = script(&lines);
    let (engine, _, _) = scripted(kept.clone());
    let mut engine = engine.keeping_a_listener_ready(true);
    let listened = |script: &Script| script.listened.load(Ordering::Relaxed);
    for (request, expected) in [(1, 2), (2, 3)] {
        engine.submit(start(request));
        engine.submit(audio(request, 1));
        engine.submit(end(request));
        assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
        assert_eq!(listened(&kept), expected, "request {request}");
    }
    // The kept listener's socket closed since: the line opens its own, and
    // the next line's is opened after it.
    kept.closed.store(true, Ordering::Relaxed);
    engine.submit(start(3));
    engine.submit(audio(3, 1));
    engine.submit(end(3));
    assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
    assert_eq!(listened(&kept), 5);
    // Each line was heard as its own words, in order.
    let firsts: Vec<Option<Message>> = kept
        .asked
        .lock()
        .unwrap()
        .iter()
        .map(|asked| asked.last().cloned())
        .collect();
    assert_eq!(firsts[0], Some(Message::user("What time is it?")));
    assert_eq!(firsts[1], Some(Message::user("And tomorrow?")));
    // Off: one listener a line.
    let plain = script(&lines[..2]);
    let (mut engine, _, _) = scripted(plain.clone());
    for request in 1..=2 {
        engine.submit(start(request));
        engine.submit(audio(request, 1));
        engine.submit(end(request));
        assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
    }
    assert_eq!(listened(&plain), 2);
}

/// #1712, the first builder at 00:31: a kept listener lasts at most
/// [`KEPT_FOR`], so a lamp nobody talks to keeps nothing open; and it goes
/// at once when listening stops (the privacy switch, a mute) or the privacy
/// generation changes. Either way the next line opens its own.
#[tokio::test(start_paused = true)]
async fn a_kept_listener_goes_after_its_time_or_when_listening_stops() {
    let script = || {
        Arc::new(Script {
            heard: Mutex::new([Ok("Hi.".to_owned()), Ok("Hello.".to_owned())].into()),
            chats: Mutex::new(
                [
                    vec![text("Hi."), Ok(ChatEvent::Done)],
                    vec![text("Hello."), Ok(ChatEvent::Done)],
                ]
                .into(),
            ),
            ..Script::default()
        })
    };
    let listened = |script: &Script| script.listened.load(Ordering::Relaxed);
    // Each way the kept one goes, then the next line: 4 listeners (line
    // 1's, its kept one, line 2's own, and line 2's kept one), not 3.
    for gone in [
        "its time",
        "listening stopped",
        "another generation",
        "kept",
    ] {
        let played = script();
        let (engine, _, _) = scripted(played.clone());
        let mut engine = engine.keeping_a_listener_ready(true);
        engine.submit(start(1));
        engine.submit(audio(1, 1));
        engine.submit(end(1));
        assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
        assert_eq!(listened(&played), 2);
        match gone {
            "its time" => tokio::time::sleep(KEPT_FOR + Duration::from_secs(1)).await,
            "listening stopped" => engine.listening(false, 1),
            "another generation" => engine.listening(true, 2),
            _ => engine.listening(true, 1),
        }
        tokio::task::yield_now().await;
        engine.submit(start(2));
        engine.submit(audio(2, 1));
        engine.submit(end(2));
        assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
        let expected = if gone == "kept" { 3 } else { 4 };
        assert_eq!(listened(&played), expected, "{gone}");
    }
}

/// #1712 (the first builder at 03:48): the journal counts a line's words in
/// the line it cut short and in the lamp's own answer, as plain words, any
/// case and punctuation, never the words.
#[test]
fn a_lines_words_are_counted_against_the_line_before_and_the_lamps_answer() {
    assert_eq!(
        super::shared_words("Hey Lamp, what time is it?", "hey lamp what time is it"),
        6
    );
    assert_eq!(
        super::shared_words("Four in the morning.", "Hey Lamp, what time is it?"),
        0
    );
    assert_eq!(
        super::shared_words("it's four in the morning", " It's four in the morning."),
        5
    );
    assert_eq!(super::shared_words("", "anything"), 0);
}

/// The reviewer's P3 on #2043: the journal's line as it is said, both
/// parts, each alone, and nothing with no words or nothing to compare.
#[test]
fn the_whose_words_line_says_its_counts_in_its_own_words() {
    let cut = || Some((4, "Hey Lamp, what time is it".to_owned()));
    let said = || Some((3, "It's four in the morning.".to_owned()));
    assert_eq!(
        super::whose_words(5, "Hey Lamp, what time is it?", cut(), said()).as_deref(),
        Some(
            "lamp-voice second: request 5: of its 6 words, 6 in request 4's words it cut short, 0 in the lamp's answer to request 3 so far"
        )
    );
    assert_eq!(
        super::whose_words(5, "it's four in the morning", None, said()).as_deref(),
        Some(
            "lamp-voice second: request 5: of its 5 words, 5 in the lamp's answer to request 3 so far"
        )
    );
    assert_eq!(
        super::whose_words(5, "what time", cut(), None).as_deref(),
        Some("lamp-voice second: request 5: of its 2 words, 2 in request 4's words it cut short")
    );
    assert_eq!(super::whose_words(5, "what time", None, None), None);
    assert_eq!(super::whose_words(5, "  ", cut(), said()), None);
}

/// The words of the latest answer sent to the voice are kept with its
/// request, for that count; a newer answer replaces them.
#[tokio::test]
async fn the_latest_answers_words_are_kept_for_the_count() {
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok("What time is it".to_owned()),
                Ok("And tomorrow".to_owned()),
            ]
            .into(),
        ),
        chats: Mutex::new(
            [
                vec![text("It's four."), Ok(ChatEvent::Done)],
                vec![text("Rain."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (mut engine, _, _) = scripted(script.clone());
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    turn(&mut engine).await;
    let said = engine.shared.lamp_said.lock().unwrap().clone();
    assert_eq!(said.0, 1);
    assert!(said.1.contains("It's four."), "{said:?}");
    engine.submit(start(2));
    engine.submit(audio(2, 1));
    engine.submit(end(2));
    turn(&mut engine).await;
    let said = engine.shared.lamp_said.lock().unwrap().clone();
    assert_eq!(said.0, 2);
    assert!(
        said.1.contains("Rain.") && !said.1.contains("four"),
        "{said:?}"
    );
}

/// `LAMP_SAID_AS_HEARD` (the first builder, 09:38 on #1722): a story cut in
/// its second sentence is in the next request's history as what was played
/// of it, its first sentence and the second's first words with the dash, and
/// never its third; and the cut-in note still tells the model it was cut
/// there. The thread is kept from the engine's output as the worker keeps it.
#[tokio::test]
async fn a_line_that_cuts_the_story_is_asked_with_the_story_as_heard() {
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok("Tell me a story.".to_owned()),
                Ok("Wait, who was he?".to_owned()),
            ]
            .into(),
        ),
        chats: Mutex::new(
            [
                vec![
                    text("Once a keeper lived by the sea. "),
                    text("Every night he climbed the stairs to the light. "),
                    text("One winter a ship came."),
                    Ok(ChatEvent::Done),
                ],
                vec![text("A keeper named Tom."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let thread = Arc::new(Mutex::new(crate::thread::Thread::new()));
    let told = thread.clone();
    let tools: ToolRunner = Arc::new(|_, _, _, _| Box::pin(async { json!({"ok": true}) }));
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(move || Told {
            text: "SYSTEM".to_owned(),
            after: None,
            history: told
                .lock()
                .unwrap()
                .recent(Instant::now(), Duration::from_secs(300), 6)
                .into_iter()
                .flat_map(|exchange| {
                    [
                        Message::user(exchange.person),
                        Message::assistant(exchange.lamp),
                    ]
                })
                .collect(),
        }),
        tools,
        Arc::new(Instant::now),
    )
    .telling_cut_ins(true)
    .announcing(false);
    // A turn to its end, its words kept in the thread as the worker keeps
    // them: each piece of the lamp's, and the person's words.
    async fn kept_turn(engine: &mut Engine, thread: &Mutex<crate::thread::Thread>) {
        loop {
            let event = tokio::time::timeout(Duration::from_secs(5), engine.next())
                .await
                .expect("the turn ends");
            let mut thread = thread.lock().unwrap();
            match event {
                ProviderOutput::Transcript {
                    request: None,
                    text,
                    finished,
                } => thread.person(&text, finished, Instant::now()),
                ProviderOutput::Transcript {
                    request: Some(request),
                    text,
                    ..
                } if !text.is_empty() => {
                    thread.lamp(request, &format!("{} ", text.trim_start()), Instant::now());
                }
                ProviderOutput::TurnComplete { .. } => return,
                _ => {}
            }
        }
    }
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    kept_turn(&mut engine, &thread).await;
    let sent = thread
        .lock()
        .unwrap()
        .recent(Instant::now(), Duration::from_secs(300), 6);
    assert!(sent[0].lamp.contains("ship"), "as sent: {:?}", sent[0].lamp);
    // The line cuts it halfway through its second piece of sound: each
    // piece's speech is 1,500 samples here.
    let mut cutting = start_marked(2, true, false, false);
    if let ProviderInput::Start { heard_of_cut, .. } = &mut cutting {
        *heard_of_cut = Some(crate::said_as_heard::HeardOfCut {
            request: 1,
            samples: 1_500 + 750,
        });
    }
    super::worker::keep_heard(&mut thread.lock().unwrap(), &engine, &cutting);
    engine.submit(cutting);
    engine.submit(audio(2, 1));
    engine.submit(end(2));
    kept_turn(&mut engine, &thread).await;
    let asked = script.asked.lock().unwrap();
    assert_eq!(asked.len(), 2, "{asked:?}");
    let story = asked[1]
        .iter()
        .find(|message| message.role == "assistant")
        .and_then(|message| message.content.as_ref()?.as_str().map(str::to_owned))
        .expect("the story in its history");
    assert!(
        story.starts_with("Once a keeper lived by the sea. Every") && story.ends_with('—'),
        "{story:?}"
    );
    assert!(!story.contains("ship"), "{story:?}");
    assert!(
        asked[1].contains(&Message::system(crate::cut_in_told::NOTE)),
        "{:?}",
        asked[1]
    );
}

/// The reviewer's P3 on #2145: a call written as words mid-answer is left
/// out of what was heard, as the thread leaves it out, and its sound still
/// counts. Played whole, what was heard is what the thread keeps.
#[tokio::test]
async fn a_call_written_in_the_answer_is_left_out_of_what_was_heard() {
    use crate::said_as_heard::HeardOfCut;
    let (engine, _, _) = scripted(Arc::new(Script::default()));
    let pieces = [
        ("Sure.", 1_000),
        (" set_timer{", 500),
        ("\"minutes\": 5}", 500),
        (" It is set.", 1_000),
    ];
    for (words, samples) in pieces {
        engine.shared.said_piece(4, Some(words));
        engine.shared.said_samples(4, samples);
    }
    let heard = |samples| {
        engine
            .said_as_heard(HeardOfCut {
                request: 4,
                samples,
            })
            .unwrap()
            .0
    };
    assert_eq!(heard(3_000), "Sure. It is set.");
    assert_eq!(heard(2_500), "Sure. It—");
    assert_eq!(heard(1_500), "Sure.", "within the call's sound");
    let mut thread = crate::thread::Thread::new();
    let now = Instant::now();
    thread.person("Set a timer.", true, now);
    for (words, _) in pieces {
        thread.lamp(4, &format!("{} ", words.trim_start()), now);
    }
    let kept = thread.recent(now, Duration::from_secs(300), 6);
    assert_eq!(kept[0].lamp, heard(3_000));
}

/// `LAMP_SECOND_SPEECH_SOCKET` (#2135's row): the speech socket is asked for
/// as each answer begins, while its chat thinks, once a request.
#[tokio::test]
async fn each_answer_asks_for_its_speech_socket_as_it_begins() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("What time is it?".to_owned())].into()),
        chats: Mutex::new(
            [
                vec![text("It is noon."), Ok(ChatEvent::Done)],
                vec![text("ORCHID."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (mut engine, _, _) = scripted(script.clone());
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    turn(&mut engine).await;
    engine.submit(written(2, "Say ORCHID."));
    turn(&mut engine).await;
    assert_eq!(*script.sockets_asked.lock().unwrap(), [1, 2]);
}
