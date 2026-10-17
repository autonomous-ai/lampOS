//! The engine against scripted services: no network.
use super::*;
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
}

struct ScriptListener {
    heard: net::Result<String>,
    samples: Arc<AtomicUsize>,
}
impl Listener for ScriptListener {
    fn send(&self, samples: &[i16]) -> net::Result<()> {
        self.samples.fetch_add(samples.len(), Ordering::Relaxed);
        Ok(())
    }
    fn finish(self: Box<Self>, _request: u64) -> BoxFuture<'static, net::Result<String>> {
        Box::pin(async move { self.heard })
    }
}

impl Services for Script {
    fn listen(&self) -> BoxFuture<'static, net::Result<Box<dyn Listener>>> {
        let result = match &self.listen_error {
            Some(error) => Err(error.clone()),
            None => Ok(Box::new(ScriptListener {
                heard: self.heard.lock().unwrap().pop_front().unwrap(),
                samples: self.samples_heard.clone(),
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
}

fn text(piece: &str) -> net::Result<ChatEvent> {
    Ok(ChatEvent::Text(piece.to_owned()))
}

fn call(id: &str, name: &str, args: Value) -> ToolCall {
    ToolCall {
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
    let mut labels: Vec<String> = Vec::new();
    loop {
        let event = tokio::time::timeout(Duration::from_secs(2), engine.next())
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

/// Words the heed judges not said to the lamp end the turn after them,
/// with no chat asked; it is given the microphone's times of the line's
/// first and last packets (#1014).
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
            ProviderOutput::TurnComplete { .. } => {
                seen.push("complete");
                break;
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(seen, ["started", "complete"]);
    assert!(begun.elapsed() >= BEGIN_WITHIN, "{:?}", begun.elapsed());
    assert!(calls.lock().unwrap().is_empty());
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
                ProviderOutput::Transcript {
                    request: Some(_),
                    text,
                    ..
                } => said.push(text),
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
            ProviderOutput::Transcript {
                request: Some(_),
                text,
                ..
            } => said.push(text),
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
