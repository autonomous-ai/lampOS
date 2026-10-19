//! Review of what changed on the night of 2026-10-14
//! (`docs/voice-review-2026-10-14.md`): tests only.
//!
//! The lamp's name heard in the room's sound (`name/heard`), taken by the
//! provider worker's own loop for the rule that decides whether an answer
//! is played (`crate::attention`), against the scripted service. The
//! hearings are put on a bus of the test's own, as `lamp-name` would.
//!
//! The tests whose names begin `review_` show a fault and fail today. The
//! ones that begin `review_sound_` are edges that were tried and held.
//!
//! Two things about the way these run, so that they cannot disturb any
//! other test in this binary:
//!
//! - The worker finds the bus through `BUS_ROOT`, which is set once for
//!   the whole test binary. Every worker started after it is set listens
//!   to the same `name/heard`. So each hearing here is said only once it
//!   is [`AGO_US`] old, dated back by its `ago_ms` as `lamp-name` dates
//!   one. A worker that is not one of these tests reads it as a name that
//!   began four seconds ago, which counts for none of its requests unless
//!   one of them waits more than two and a half seconds for its first
//!   answer. The one other test here that applies the rule waits a few
//!   milliseconds.
//! - The tests here take turns ([`one_at_a_time`]), so that one's hearing
//!   is never read by the other's worker.
//!
//! They run on a logical clock of their own (#823): a paused runtime and
//! `logical_clock()`, so the four seconds pass in the rig's ticks, not on
//! the host. A worker dates a hearing from its own clock when it reads it
//! (`now - ago_ms`, `crate::bus_out`), so another test's worker, on the host
//! clock, still reads it as four seconds old.
use super::*;
use lamp_msgs::{NameHeard, TurnVerdict, VoiceTurn};
use std::sync::atomic::AtomicBool;

/// How old a hearing is when it is said: see the module's notes.
const AGO_US: u64 = 4_000_000;

static TAKEN: AtomicBool = AtomicBool::new(false);
struct Turn;
impl Drop for Turn {
    fn drop(&mut self) {
        TAKEN.store(false, std::sync::atomic::Ordering::Release);
    }
}
async fn one_at_a_time() -> Turn {
    while TAKEN
        .compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::AcqRel,
            std::sync::atomic::Ordering::Acquire,
        )
        .is_err()
    {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Turn
}

/// The bus the worker takes the name from, removed when the test is over.
/// Made before the worker starts, since it looks for the bus once, as it
/// begins. A root short enough for a socket's path, which the system's own
/// temporary directory on a Mac is not.
struct Root(std::path::PathBuf);
impl Root {
    fn new() -> Self {
        let root = BUS_ROOT
            .get_or_init(|| format!("/tmp/lvr-{}", std::process::id()).into())
            .clone();
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `lamp-name` says the name began to be heard at `began_us`, by the
/// worker's clock. Heartbeats go on meanwhile, and on for long enough
/// after for the worker, which looks every 20 ms, to have read it.
async fn name_heard(rig: &mut LiveRig, root: &Path, began_us: u64) {
    while monotonic_us() < began_us + AGO_US {
        rig.publish();
        rig.tick(Duration::from_millis(5)).await;
    }
    let mut says = lamp_bus::Bus::new(root)
        .unwrap()
        .publisher_declared(NameHeard::TOPIC, &NameHeard::declaration(), "name")
        .unwrap();
    let now = monotonic_us();
    let ago_ms = u32::try_from((now - began_us) / 1_000).unwrap();
    let published = says
        .publish_json(now, &NameHeard { score: 1.0, ago_ms })
        .unwrap();
    assert!(
        published.delivered >= 1,
        "no worker is listening for the name"
    );
    rig.pump(Duration::from_millis(200)).await;
}

fn busy() -> (crate::room_told::Room, bool) {
    (crate::room_told::Room::Busy, false)
}

fn in_a_busy_room() -> Options {
    Options {
        reporting: Reporting::Typed,
        holding_answer: false,
        tools: crate::tools::Settings::default(),
        attention: Some(busy),
        name_signal: false,
        sounds_checked: false,
        ask_unclear: false,
        name_in_passing_ignored: false,
        remarks: false,
        going_on_asks: None,
        cut_in_told: false,
        misheard_hum: false,
        policy_holds: None,
        follows_run: false,
        wait_for_room: false,
        staff_note: false,
        spoken_to: false,
        voices: None,
        catalog: None,
    }
}

fn heard(text: &str) -> serde_json::Value {
    json!({"serverContent":{"inputTranscription":{"text":text}}})
}

/// A new session in a busy room hears two people: "How was your weekend?"
/// is the first thing it hears, and the rule would hold its answer. A
/// second after that request's sound has ended (the lamp's own end of
/// speech, 600 ms of quiet, is past), someone else begins a line with a
/// name like the lamp's ("Hey Liam, the meeting moved.", "Hey man, ...":
/// the name part reports such lines in about half of cases,
/// `docs/name.md`). The room is busy, so that line does not take the
/// turn while the first answer is awaited (`floor_held`). The hearing
/// reaches the worker before the model's first sound for the first
/// request, and that answer, to other people's talk, is let out.
///
/// The rule counts a hearing "at any time while [the request] is in hand"
/// (`docs/attention.md`); its reason is that the name is said at the start
/// of what is said, or in the middle of it. A name that began after the
/// request's sound ended was not said in it.
#[tokio::test(start_paused = true)]
async fn review_a_name_heard_after_a_requests_sound_ended_does_not_let_out_its_answer() {
    let _clock = logical_clock();
    let _turn = one_at_a_time().await;
    let root = Root::new();
    let mut rig = LiveRig::start_with(quick_policy(), in_a_busy_room());
    let mut dial = rig.ready().await;
    let request = rig.speak(&mut dial, false, true).await;
    let ended = monotonic_us();
    dial.service.send(heard("How was your weekend?")).await;
    assert!(matches!(
        rig.output().await,
        ProviderOutput::Transcript { request: None, .. }
    ));
    // Someone else's line begins a second after this one ended.
    name_heard(&mut rig, &root.0, ended + 1_000_000).await;
    dial.service.send(provider_audio(9, 480)).await;
    dial.service.send(idle_complete()).await;
    let first = rig.output().await;
    assert!(
        matches!(first, ProviderOutput::TurnComplete { request: done, idle: true } if done == request),
        "the answer to other people's talk was let out by a name said after it; first out: {}",
        what(&first)
    );
    assert!(rig.take().is_none());
    rig.stop().await;
}

/// One output, short, for a failure's message.
fn what(output: &ProviderOutput) -> String {
    match output {
        ProviderOutput::Audio {
            request, samples, ..
        } => format!(
            "the answer's sound for request {request}, {} samples",
            samples.len()
        ),
        other => format!("{other:?}"),
    }
}

/// The change itself, through the worker's own loop: the same request,
/// the same busy room and new session, but the name heard as the request
/// began. Its answer is played where the rule alone would hold it.
#[tokio::test(start_paused = true)]
async fn review_sound_a_name_heard_as_a_request_began_lets_out_the_answer_the_rule_would_hold() {
    let _clock = logical_clock();
    let _turn = one_at_a_time().await;
    let root = Root::new();
    let mut rig = LiveRig::start_with(quick_policy(), in_a_busy_room());
    let mut dial = rig.ready().await;
    let began = monotonic_us();
    // The request's sound goes on while the name is heard, as a person's
    // does: the rig's own requests are over in a few milliseconds, and a
    // name that begins after a request's sound has ended is not counted
    // for it (the test above).
    let request = rig.speak(&mut dial, false, false).await;
    dial.service
        .send(heard("What do you think we should have for lunch?"))
        .await;
    assert!(matches!(
        rig.output().await,
        ProviderOutput::Transcript { request: None, .. }
    ));
    name_heard(&mut rig, &root.0, began + 200_000).await;
    let owner = rig.snapshot.owner().unwrap();
    rig.owner.input_ended(real_time(), owner).unwrap();
    rig.publish();
    rig.parent
        .data
        .send(ProviderInput::End {
            request,
            privacy_generation: rig.snapshot.microphone_generation(),
            sound: false,
            cut_in: false,
        })
        .unwrap();
    let message = rig.service_message(&mut dial).await;
    assert!(
        message["realtimeInput"].get("activityEnd").is_some(),
        "expected the end of the request's sound, service received {message}"
    );
    dial.service.send(provider_audio(9, 480)).await;
    let first = rig.output().await;
    assert!(
        matches!(first, ProviderOutput::Audio { request: played, .. } if played == request),
        "a call heard by name was held; first out: {}",
        what(&first)
    );
    dial.service.send(idle_complete()).await;
    assert!(matches!(
        rig.output().await,
        ProviderOutput::TurnComplete { .. }
    ));
    rig.stop().await;
}

/// The name note on (`NAME_NOTE`, the worker's `--name-note`) for one
/// test, and off again when dropped, a failed assertion included.
struct NameNoteOn;
impl NameNoteOn {
    fn set() -> Self {
        NAME_NOTE.store(true, std::sync::atomic::Ordering::Relaxed);
        Self
    }
}
impl Drop for NameNoteOn {
    fn drop(&mut self) {
        NAME_NOTE.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

/// #828 (the first builder, 03:52 on 19 October): with `LAMP_CUT_IN_TOLD`,
/// a cut-in over the lamp's answer that also names the lamp is told both:
/// a request sends only its newest note, so the name's note, given after
/// the cut-in's, goes out with it (`add_note`), never in its place.
#[tokio::test(start_paused = true)]
async fn review_sound_a_cut_in_that_names_the_lamp_is_told_both_notes_together() {
    let _clock = logical_clock();
    let _turn = one_at_a_time().await;
    let root = Root::new();
    let _note = NameNoteOn::set();
    let mut options = in_a_busy_room();
    options.cut_in_told = true;
    let mut rig = LiveRig::start_with(quick_policy(), options);
    let mut dial = rig.ready().await;
    let began = monotonic_us();
    let request = rig
        .speak_as(
            &mut dial,
            false,
            false,
            StartSays {
                over_the_lamp: true,
                over_the_lamps_voice: true,
                ..StartSays::default()
            },
        )
        .await;
    dial.service
        .send(heard("Lamp, what is two plus two?"))
        .await;
    assert!(matches!(
        rig.output().await,
        ProviderOutput::Transcript { request: None, .. }
    ));
    name_heard(&mut rig, &root.0, began + 200_000).await;
    let owner = rig.snapshot.owner().unwrap();
    rig.owner.input_ended(real_time(), owner).unwrap();
    rig.publish();
    rig.parent
        .data
        .send(ProviderInput::End {
            request,
            privacy_generation: rig.snapshot.microphone_generation(),
            sound: false,
            cut_in: false,
        })
        .unwrap();
    let mut told = Vec::new();
    loop {
        let message = rig.service_message(&mut dial).await;
        if let Some(text) = message["realtimeInput"]["text"].as_str() {
            told.push(text.to_owned());
        }
        if message["realtimeInput"].get("activityEnd").is_some() {
            break;
        }
    }
    let last = told.last().expect("a note beside the request");
    assert!(
        last.contains(crate::cut_in_told::NOTE) && last.contains(crate::called::NOTE),
        "{told:?}"
    );
    assert!(
        told.iter()
            .all(|text| !text.contains(crate::called::NOTE)
                || text.contains(crate::cut_in_told::NOTE)),
        "the name's note went out without the cut-in's: {told:?}"
    );
    rig.stop().await;
}

/// The heard-as line on (`NAME_NOTE_HEARD_AS`, the worker's
/// `--name-note-heard-as`) for one test, and off again when dropped.
struct HeardAsOn;
impl HeardAsOn {
    fn set() -> Self {
        NAME_NOTE_HEARD_AS.store(true, std::sync::atomic::Ordering::Relaxed);
        Self
    }
}
impl Drop for HeardAsOn {
    fn drop(&mut self) {
        NAME_NOTE_HEARD_AS.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

/// A request heard as "Hey Mom, …" whose name lamp-name heard as it began:
/// what the service is given as text beside it, up to its end. Its caller
/// takes its turn ([`one_at_a_time`]) before it sets the name's switches,
/// which every test here reads (Sixth on #705: set before the turn, they
/// reached the tests then running).
async fn told_beside_hey_mom() -> Vec<String> {
    let _clock = logical_clock();
    let root = Root::new();
    let mut rig = LiveRig::start_with(quick_policy(), in_a_busy_room());
    let mut dial = rig.ready().await;
    let began = monotonic_us();
    let request = rig.speak(&mut dial, false, false).await;
    dial.service
        .send(heard(
            "Hey Mom, tell me a story about a brave little robot.",
        ))
        .await;
    assert!(matches!(
        rig.output().await,
        ProviderOutput::Transcript { request: None, .. }
    ));
    name_heard(&mut rig, &root.0, began + 200_000).await;
    let owner = rig.snapshot.owner().unwrap();
    rig.owner.input_ended(real_time(), owner).unwrap();
    rig.publish();
    rig.parent
        .data
        .send(ProviderInput::End {
            request,
            privacy_generation: rig.snapshot.microphone_generation(),
            sound: false,
            cut_in: false,
        })
        .unwrap();
    let mut told = Vec::new();
    loop {
        let message = rig.service_message(&mut dial).await;
        if let Some(text) = message["realtimeInput"]["text"].as_str() {
            told.push(text.to_owned());
        }
        if message["realtimeInput"].get("activityEnd").is_some() {
            break;
        }
    }
    rig.stop().await;
    told
}

/// #829 (the first builder at 05:22 on 19 October): the model's ears heard
/// "Hey Lamp" as "Hey Mom", "Hey man" or "Hey ma'am" in 7 of 10 takes, while
/// lamp-name heard the name. With `LAMP_NAME_NOTE_HEARD_AS`, the name's note
/// says the name may have sounded like another word; without it, the note
/// is as before.
#[tokio::test(start_paused = true)]
async fn review_sound_with_the_heard_as_line_the_name_note_says_it_may_sound_like_another_word() {
    let _turn = one_at_a_time().await;
    let _note = NameNoteOn::set();
    {
        let _heard_as = HeardAsOn::set();
        let told = told_beside_hey_mom().await;
        assert!(
            told.iter()
                .any(|text| text.contains(crate::called::NOTE_HEARD_AS)),
            "{told:?}"
        );
        assert!(
            told.iter().all(|text| !text.contains(crate::called::NOTE)),
            "{told:?}"
        );
    }
    let told = told_beside_hey_mom().await;
    assert!(
        told.iter().any(|text| text.contains(crate::called::NOTE)),
        "{told:?}"
    );
    assert!(
        told.iter()
            .all(|text| !text.contains(crate::called::NOTE_HEARD_AS)),
        "{told:?}"
    );
}

/// Listens on the test's bus for each turn's verdict (`voice/turn`,
/// #1155), from before the worker starts, as the Policy does.
fn turns(root: &Root) -> lamp_bus::Subscriber {
    lamp_bus::Bus::new(&root.0)
        .unwrap()
        .subscribe(VoiceTurn::TOPIC, "test")
        .unwrap()
}

/// Waits up to `within` on the host for `request` to be told `verdict`.
/// Other tests' workers that apply the rule say theirs on the same bus,
/// numbered by their own rigs, so a verdict for another request, or for
/// this number with the other verdict, is passed over until this one
/// comes.
fn told(turns: &mut lamp_bus::Subscriber, request: u64, verdict: TurnVerdict, within: Duration) {
    let until = std::time::Instant::now() + within;
    let mut heard = Vec::new();
    while let Some(left) = until.checked_duration_since(std::time::Instant::now()) {
        let Some(message) = turns.recv(left).unwrap() else {
            break;
        };
        let turn: VoiceTurn = message.parse().unwrap();
        if (turn.request, turn.verdict) == (request, verdict) {
            return;
        }
        heard.push((turn.request, turn.verdict));
    }
    panic!("request {request} was not told {verdict:?}; heard {heard:?}");
}

/// #1155: a turn whose answer the rule holds, in a busy room's new
/// session, is told held back on the bus, as the journal says it, so the
/// Policy's hello does not count it toward its quiet.
#[tokio::test(start_paused = true)]
async fn an_answer_the_rule_holds_is_told_held_back() {
    let _clock = logical_clock();
    let _turn = one_at_a_time().await;
    let root = Root::new();
    let mut turns = turns(&root);
    let mut rig = LiveRig::start_with(quick_policy(), in_a_busy_room());
    let mut dial = rig.ready().await;
    let request = rig.speak(&mut dial, false, true).await;
    dial.service.send(heard("How was your weekend?")).await;
    assert!(matches!(
        rig.output().await,
        ProviderOutput::Transcript { request: None, .. }
    ));
    dial.service.send(provider_audio(9, 480)).await;
    dial.service.send(idle_complete()).await;
    let first = rig.output().await;
    assert!(
        matches!(first, ProviderOutput::TurnComplete { request: done, idle: true } if done == request),
        "{}",
        what(&first)
    );
    told(
        &mut turns,
        request,
        TurnVerdict::HeldBack,
        Duration::from_secs(2),
    );
    rig.stop().await;
}

/// #1155: a turn the rule lets out, in a quiet room, is told answered.
#[tokio::test(start_paused = true)]
async fn an_answer_the_rule_lets_out_is_told_answered() {
    let _clock = logical_clock();
    let _turn = one_at_a_time().await;
    let root = Root::new();
    let mut turns = turns(&root);
    let mut options = in_a_busy_room();
    options.attention = Some(|| (crate::room_told::Room::Quiet, false));
    let mut rig = LiveRig::start_with(quick_policy(), options);
    let mut dial = rig.ready().await;
    let request = rig.speak(&mut dial, false, true).await;
    dial.service.send(heard("What time is it?")).await;
    assert!(matches!(
        rig.output().await,
        ProviderOutput::Transcript { request: None, .. }
    ));
    dial.service.send(provider_audio(9, 480)).await;
    let first = rig.output().await;
    assert!(
        matches!(first, ProviderOutput::Audio { request: played, .. } if played == request),
        "{}",
        what(&first)
    );
    dial.service.send(idle_complete()).await;
    assert!(matches!(
        rig.output().await,
        ProviderOutput::TurnComplete { .. }
    ));
    told(
        &mut turns,
        request,
        TurnVerdict::Answered,
        Duration::from_secs(2),
    );
    rig.stop().await;
}

/// The first builder on #1817: a line the model itself declines
/// (`not_for_me`) is told held back on the bus, as a line the rule holds
/// is: here with no rule at all.
#[tokio::test(start_paused = true)]
async fn a_line_the_model_declines_is_told_held_back() {
    let _clock = logical_clock();
    let _turn = one_at_a_time().await;
    let root = Root::new();
    let mut turns = turns(&root);
    let mut options = in_a_busy_room();
    options.attention = None;
    options.tools = crate::tools::Settings {
        decline: true,
        exec: None,
        memory: None,
        clock: false,
        ..Default::default()
    };
    let mut rig = LiveRig::start_with(quick_policy(), options);
    let mut dial = rig.ready().await;
    let request = rig.speak(&mut dial, false, true).await;
    // With no rule here, a verdict for this request can only be the
    // decline's: what other tests' workers said before it is let go, and
    // the decline's is looked for in the half second after it.
    while turns.recv(Duration::ZERO).unwrap().is_some() {}
    dial.service
        .send(function_call("c1", crate::tools::NOT_FOR_ME))
        .await;
    let answered = rig.service_message(&mut dial).await;
    assert!(answered.get("toolResponse").is_some(), "{answered}");
    dial.service.send(idle_complete()).await;
    let first = rig.output().await;
    assert!(
        matches!(first, ProviderOutput::TurnComplete { request: done, .. } if done == request),
        "{}",
        what(&first)
    );
    told(
        &mut turns,
        request,
        TurnVerdict::HeldBack,
        Duration::from_millis(500),
    );
    rig.stop().await;
}

/// The real Skills socket protocol, with a valid private offer if it is
/// reached. Negative cases must avoid the dispatch itself, not merely hide
/// its reply. The thread is joined even when a regression assertion fails.
struct DeclineMusic {
    home: crate::process::SessionDirectory,
    asked: std::sync::mpsc::Receiver<serde_json::Value>,
    stop: std::sync::Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl DeclineMusic {
    fn new() -> Self {
        use std::io::{BufRead, Read, Write};
        let home = crate::process::SessionDirectory::create().unwrap();
        std::fs::write(
            home.path.join(crate::tools::CATALOG_FILE),
            r#"{"tools":[{"name":"play_relaxing_music","description":"A requested recording."}]}"#,
        )
        .unwrap();
        let listener =
            std::os::unix::net::UnixListener::bind(home.path.join(crate::tools::CALL_SOCKET))
                .unwrap();
        let (sent, asked) = std::sync::mpsc::channel();
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let ending = stop.clone();
        let worker = std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                if ending.load(std::sync::atomic::Ordering::Acquire) {
                    return;
                }
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut line = String::new();
                std::io::BufReader::new((&stream).take(16_384))
                    .read_line(&mut line)
                    .unwrap();
                let call: serde_json::Value = serde_json::from_str(&line).unwrap();
                assert_eq!(call["name"], "play_relaxing_music");
                let context: lamp_msgs::RequestedAudioContext =
                    serde_json::from_value(call["requested_audio_context"].clone()).unwrap();
                let offer = lamp_msgs::RequestedAudioOffer {
                    skills_boot: "skills-test".into(),
                    definition_generation: 2,
                    offer_id: 3,
                    skill: "relaxing-music".into(),
                    asset: "/local/relaxing.wav".into(),
                    sha256: "a".repeat(64),
                    frames: 24_000,
                    issued_us: context.issued_us,
                    start_before_us: context.issued_us + lamp_msgs::REQUESTED_AUDIO_START_US,
                    finish_before_us: context.issued_us
                        + lamp_msgs::REQUESTED_AUDIO_START_US
                        + 1_000_000,
                    context,
                };
                offer.check().unwrap();
                sent.send(call).unwrap();
                // A cancelled request may close the socket before this
                // response; receipt of the request still proves dispatch.
                let _ = writeln!(
                    stream,
                    "{}",
                    json!({"ok":true,"result":{
                        "next":"Confirm.","requested_audio":offer,
                    }})
                );
            }
        });
        Self {
            home,
            asked,
            stop,
            worker: Some(worker),
        }
    }

    fn options(&self, wrapped: bool) -> Options {
        typed_with(crate::tools::Settings {
            decline: true,
            skills: Some(self.home.path.clone()),
            requested_audio: true,
            use_skill: wrapped,
            ..Default::default()
        })
    }
}

impl Drop for DeclineMusic {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        let _ =
            std::os::unix::net::UnixStream::connect(self.home.path.join(crate::tools::CALL_SOCKET));
        if let Some(worker) = self.worker.take() {
            let result = worker.join();
            if !std::thread::panicking() {
                result.unwrap();
            }
        }
        let _ = std::fs::remove_dir_all(&self.home.path);
    }
}

fn music_function(wrapped: bool) -> serde_json::Value {
    json!({
        "id":"music-original",
        "name":if wrapped { crate::use_skill::NAME } else { "play_relaxing_music" },
        "args":if wrapped { json!({"skill":"play_relaxing_music"}) } else { json!({}) },
    })
}

fn decline_function() -> serde_json::Value {
    json!({"id":"decline","name":crate::tools::NOT_FOR_ME,"args":{}})
}

fn response_id(message: &serde_json::Value) -> &str {
    let responses = message["toolResponse"]["functionResponses"]
        .as_array()
        .expect("the original tool call must still be answered");
    assert_eq!(responses.len(), 1, "{message}");
    assert!(
        responses[0]["response"].get("requested_audio").is_none(),
        "private offer leaked into the model response: {message}"
    );
    responses[0]["id"].as_str().unwrap()
}

async fn outputs_through_completion(rig: &mut LiveRig, request: u64) -> Vec<ProviderOutput> {
    let mut outputs = Vec::new();
    loop {
        let output = rig.output().await;
        let done =
            matches!(output, ProviderOutput::TurnComplete { request: of, .. } if of == request);
        outputs.push(output);
        assert!(outputs.len() <= 16, "unexpected output loop: {outputs:?}");
        if done {
            return outputs;
        }
    }
}

/// The verdict is a positive boundary, not a delay assumed to mean that the
/// worker has handled the decline. Keep the original authority fresh while
/// waiting for its next tick to publish it.
async fn wait_for_decline(rig: &mut LiveRig, turns: &mut lamp_bus::Subscriber, request: u64) {
    let until = Instant::now() + Duration::from_millis(500);
    loop {
        while let Some(message) = turns.recv(Duration::ZERO).unwrap() {
            let turn: VoiceTurn = message.parse().unwrap();
            if turn.request == request && turn.verdict == TurnVerdict::HeldBack {
                return;
            }
        }
        assert!(
            Instant::now() < until,
            "the accepted decline was not published"
        );
        rig.publish();
        rig.tick(Duration::from_millis(2)).await;
    }
}

fn no_music_dispatched(skills: &DeclineMusic, outputs: &[ProviderOutput]) {
    let asked: Vec<_> = skills.asked.try_iter().collect();
    assert!(
        asked.is_empty()
            && !outputs
                .iter()
                .any(|output| matches!(output, ProviderOutput::RequestedAudio { .. })),
        "an accepted unacted decline still dispatched music or handed on its offer: asked={asked:?}; outputs={outputs:?}"
    );
    assert!(
        !outputs
            .iter()
            .any(|output| matches!(output, ProviderOutput::Audio { .. })),
        "audio after the accepted decline escaped: {outputs:?}"
    );
}

/// A preamble is already outside the worker when the model declines. Neither
/// a later tool nor the next call in the same service message may turn that
/// preamble into authority for a recording.
#[tokio::test]
async fn an_accepted_decline_blocks_later_and_mixed_music_calls() {
    let _turn = one_at_a_time().await;
    let root = Root::new();
    for mixed in [false, true] {
        for wrapped in [false, true] {
            let skills = DeclineMusic::new();
            let mut turns = turns(&root);
            let mut rig = LiveRig::start_with(quick_policy(), skills.options(wrapped));
            let mut dial = rig.ready().await;
            let request = rig.speak(&mut dial, false, true).await;
            dial.service.send(provider_audio(17, PACKET_SAMPLES)).await;
            assert!(
                matches!(rig.output().await, ProviderOutput::Audio { request: of, .. } if of == request)
            );
            while turns.recv(Duration::ZERO).unwrap().is_some() {}
            if mixed {
                dial.service
                    .send(json!({"toolCall":{"functionCalls":[decline_function(),music_function(wrapped)]}}))
                    .await;
            } else {
                dial.service
                    .send(json!({"toolCall":{"functionCalls":[decline_function()]}}))
                    .await;
            }
            let declined = rig.service_message(&mut dial).await;
            assert_eq!(response_id(&declined), "decline");
            wait_for_decline(&mut rig, &mut turns, request).await;
            if !mixed {
                dial.service
                    .send(json!({"toolCall":{"functionCalls":[music_function(wrapped)]}}))
                    .await;
            }
            // As for any non-silent function, the transport consumes the
            // service's tool round completion before its reply round ends.
            dial.service.send(idle_complete()).await;
            let answered = rig.service_message(&mut dial).await;
            assert_eq!(response_id(&answered), "music-original");
            dial.service.send(provider_audio(29, PACKET_SAMPLES)).await;
            dial.service.send(idle_complete()).await;
            let outputs = outputs_through_completion(&mut rig, request).await;
            rig.stop().await;
            no_music_dispatched(&skills, &outputs);
        }
    }
}

/// The Skills call is parked first. The following decline is acknowledged
/// before an explicit Policy `answer` arrives, so release must not revive it.
#[tokio::test]
async fn an_accepted_decline_does_not_release_a_parked_music_call() {
    let _turn = one_at_a_time().await;
    let root = Root::new();
    for wrapped in [false, true] {
        let skills = DeclineMusic::new();
        let policy_root = skills.home.path.join("policy");
        let mut decisions = lamp_bus::Bus::new(&policy_root)
            .unwrap()
            .publisher_declared(
                lamp_msgs::Decision::TOPIC,
                &lamp_msgs::Decision::declaration(),
                "policy",
            )
            .unwrap();
        let mut options = skills.options(wrapped);
        options.policy_holds = Some(policy_root);
        let mut turns = turns(&root);
        let mut rig = LiveRig::start_with(quick_policy(), options);
        let mut dial = rig.ready().await;
        let request = rig.speak(&mut dial, false, true).await;
        dial.service.send(provider_audio(17, PACKET_SAMPLES)).await;
        assert!(
            matches!(rig.output().await, ProviderOutput::Audio { request: of, .. } if of == request)
        );
        while turns.recv(Duration::ZERO).unwrap().is_some() {}
        dial.service
            .send(
                json!({"toolCall":{"functionCalls":[music_function(wrapped),decline_function()]}}),
            )
            .await;
        dial.service.send(idle_complete()).await;
        let declined = rig.service_message(&mut dial).await;
        assert_eq!(response_id(&declined), "decline");
        // Ordered callbacks in the same provider message establish that
        // the preceding music call reached the desk and was not dispatched.
        assert!(skills.asked.try_recv().is_err(), "the call was not parked");
        wait_for_decline(&mut rig, &mut turns, request).await;
        let delivered = decisions
            .publish_json(
                monotonic_us(),
                &json!({"about":"attention","choice":"answer","probability":0.9,
                    "answers":{},"by":"test/policy","took_ms":1,"shadow":false,"request":request}),
            )
            .unwrap();
        assert!(
            delivered.delivered >= 1,
            "the worker missed the Policy decision"
        );
        let answered = rig.service_message(&mut dial).await;
        assert_eq!(response_id(&answered), "music-original");
        dial.service.send(provider_audio(29, PACKET_SAMPLES)).await;
        dial.service.send(idle_complete()).await;
        let outputs = outputs_through_completion(&mut rig, request).await;
        rig.stop().await;
        no_music_dispatched(&skills, &outputs);
    }
}

/// The opposite ordering is intentional: an action already dispatched owns
/// its answer. A later decline does not retract its one offer or confirmation.
#[tokio::test]
async fn a_music_action_before_decline_keeps_its_single_offer_and_confirmation() {
    let _turn = one_at_a_time().await;
    let root = Root::new();
    for wrapped in [false, true] {
        let skills = DeclineMusic::new();
        let mut turns = turns(&root);
        let mut rig = LiveRig::start_with(quick_policy(), skills.options(wrapped));
        let mut dial = rig.ready().await;
        let request = rig.speak(&mut dial, false, true).await;
        while turns.recv(Duration::ZERO).unwrap().is_some() {}
        dial.service
            .send(json!({"toolCall":{"functionCalls":[music_function(wrapped)]}}))
            .await;
        dial.service.send(idle_complete()).await;
        let answered = rig.service_message(&mut dial).await;
        assert_eq!(response_id(&answered), "music-original");
        assert_eq!(
            answered["toolResponse"]["functionResponses"][0]["response"],
            json!({"next":"Confirm."})
        );
        let first = rig.output().await;
        let ProviderOutput::RequestedAudio {
            request: of,
            session,
            offer,
        } = &first
        else {
            panic!("the accepted Skills reply did not hand on its offer: {first:?}");
        };
        assert_eq!((*of, *session), (request, dial.session.get()));
        assert_eq!(offer.context.call_id, "music-original");
        dial.service
            .send(json!({"toolCall":{"functionCalls":[decline_function()]}}))
            .await;
        let declined = rig.service_message(&mut dial).await;
        assert_eq!(response_id(&declined), "decline");
        dial.service.send(provider_audio(21, PACKET_SAMPLES)).await;
        dial.service.send(idle_complete()).await;
        let mut outputs = vec![first];
        outputs.extend(outputs_through_completion(&mut rig, request).await);
        // Declines are published on the worker's 20 ms tick. Drive two
        // ticks with heartbeats before checking the absence of a verdict.
        rig.pump(Duration::from_millis(40)).await;
        let mut verdicts = Vec::new();
        while let Some(message) = turns.recv(Duration::ZERO).unwrap() {
            let turn: VoiceTurn = message.parse().unwrap();
            if turn.request == request {
                verdicts.push(turn.verdict);
            }
        }
        rig.stop().await;
        assert!(verdicts.is_empty(), "an action was retracted: {verdicts:?}");
        assert_eq!(skills.asked.try_iter().count(), 1, "the skill ran twice");
        assert_eq!(
            outputs
                .iter()
                .filter(|output| matches!(output, ProviderOutput::RequestedAudio { .. }))
                .count(),
            1,
            "{outputs:?}"
        );
        assert_eq!(
            outputs
                .iter()
                .filter(|output| matches!(output, ProviderOutput::Audio { .. }))
                .count(),
            1,
            "confirmation audio was duplicated: {outputs:?}"
        );
        assert!(
            outputs.iter().any(
                |output| matches!(output, ProviderOutput::Audio { request: of, samples, .. }
                if *of == request && *samples == vec![21; PACKET_SAMPLES])
            ),
            "the single confirmation was suppressed: {outputs:?}"
        );
    }
}

/// Declining a line closes its speech and action authority, but a later
/// withdrawal of report consent still reaches the reports owner once.
#[tokio::test]
async fn an_accepted_decline_allows_reports_withdrawal_without_reviving_speech() {
    use lamp_msgs::{ReportsRequest, ReportsState};

    // ReportsStop reads LAMP_BUS_ROOT rather than the worker's BUS_ROOT test
    // hook. Set it only on a child test process: changing the environment in
    // this threaded test binary could redirect another test's bus traffic.
    const CHILD: &str = "LAMP_TEST_DECLINED_REPORTS_CHILD";
    const COMPLETED: &str = "declined ReportsStop completed both Policy cases";
    if std::env::var_os(CHILD).is_none() {
        let home = crate::process::SessionDirectory::create().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "provider_worker::tests::review_2026_10_14::an_accepted_decline_allows_reports_withdrawal_without_reviving_speech",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD, &home.path)
            .env(lamp_bus::ROOT_VARIABLE, &home.path)
            .output()
            .unwrap();
        let _ = std::fs::remove_dir_all(&home.path);
        assert!(
            output.status.success(),
            "isolated reports regression failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(COMPLETED),
            "the exact child test did not complete both cases"
        );
        return;
    }

    assert_eq!(
        std::env::var_os(lamp_bus::ROOT_VARIABLE),
        std::env::var_os(CHILD),
        "the ReportsStop route must use only this test's private bus"
    );
    let _turn = one_at_a_time().await;
    let root = Root::new();
    let bus = lamp_bus::Bus::from_environment().unwrap();
    // The fake owner has no uploader, settings writer or network client. It
    // receives the actual production bus request and publishes only a state.
    let mut requests = bus
        .subscribe_typed(ReportsRequest::TOPIC, "cloud", ReportsRequest::TYPE)
        .unwrap();
    let mut states = bus
        .publisher_declared(ReportsState::TOPIC, &ReportsState::declaration(), "cloud")
        .unwrap();
    for policy in [false, true] {
        assert!(requests.recv(Duration::ZERO).unwrap().is_none());
        states
            .publish_json(
                monotonic_us(),
                &ReportsState {
                    share: true,
                    queued: 2,
                },
            )
            .unwrap();
        let mut options = typed_with(crate::tools::Settings {
            decline: true,
            reports_stop: true,
            requested_audio: true,
            ..Default::default()
        });
        // ReportsStop is excluded from Policy's parked-action set. Keep that
        // real entry path: no fabricated Parked value or stay_quiet decision.
        if policy {
            options.policy_holds = Some(root.0.join("policy"));
        }
        let mut turns = turns(&root);
        let mut rig = LiveRig::start_with(quick_policy(), options);
        let mut dial = rig.ready().await;
        let request = rig.speak(&mut dial, false, true).await;
        dial.service.send(provider_audio(17, PACKET_SAMPLES)).await;
        assert!(matches!(
            rig.output().await,
            ProviderOutput::Audio { request: of, .. } if of == request
        ));
        while turns.recv(Duration::ZERO).unwrap().is_some() {}
        dial.service
            .send(json!({"toolCall":{"functionCalls":[decline_function()]}}))
            .await;
        let declined = rig.service_message(&mut dial).await;
        assert_eq!(response_id(&declined), "decline");
        wait_for_decline(&mut rig, &mut turns, request).await;

        dial.service
            .send(json!({"toolCall":{"functionCalls":[{
                "id":"withdraw-reports",
                "name":crate::reports_stop::NAME,
                "args":{},
            }]}}))
            .await;
        dial.service.send(idle_complete()).await;
        let until = Instant::now() + Duration::from_secs(1);
        let asked = loop {
            if let Some(message) = requests.recv(Duration::ZERO).unwrap() {
                break message;
            }
            assert!(
                Instant::now() < until,
                "report consent withdrawal never reached the fake owner (policy={policy})"
            );
            rig.publish();
            rig.tick(Duration::from_millis(2)).await;
        };
        assert_eq!(asked.header.source, "voice");
        assert_eq!(
            asked.parse::<ReportsRequest>().unwrap(),
            ReportsRequest { share: false }
        );
        states
            .publish_json(
                monotonic_us(),
                &ReportsState {
                    share: false,
                    queued: 0,
                },
            )
            .unwrap();
        let answered = rig.service_message(&mut dial).await;
        assert_eq!(response_id(&answered), "withdraw-reports");
        assert_eq!(
            answered["toolResponse"]["functionResponses"][0]["response"],
            json!({"ok":true,"say":crate::reports_stop::STOPPED})
        );
        dial.service
            .send(json!({"serverContent":{"outputTranscription":{
                "text":crate::reports_stop::STOPPED,
                "finished":true,
            }}}))
            .await;
        dial.service.send(provider_audio(29, PACKET_SAMPLES)).await;
        dial.service.send(idle_complete()).await;
        let outputs = outputs_through_completion(&mut rig, request).await;
        rig.stop().await;
        assert!(
            requests.recv(Duration::ZERO).unwrap().is_none(),
            "consent withdrawal was dispatched more than once (policy={policy})"
        );
        assert!(
            !outputs.iter().any(|event| matches!(
                event,
                ProviderOutput::Audio { .. }
                    | ProviderOutput::Transcript { .. }
                    | ProviderOutput::RequestedAudio { .. }
            )),
            "withdrawal revived a declined answer (policy={policy}): {outputs:?}"
        );
    }
    eprintln!("{COMPLETED}");
}
