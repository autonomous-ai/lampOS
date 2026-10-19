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
use lamp_msgs::NameHeard;
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
/// what the service is given as text beside it, up to its end. The caller
/// holds the clock and the turn, so that the switches it sets are its own
/// for all it asks (#1569 row 29).
async fn told_beside_hey_mom() -> Vec<String> {
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
    // The turn first, for the whole test: `NAME_NOTE` and the heard-as line
    // are the process's, and another test of this module sets or clears
    // them in its turn. Set before it, or across two turns, they were turned
    // on under the test holding the turn, or off under this one: in parallel
    // this test failed 6 of 6 runs and the module 20 of 20 (#1569 row 29).
    let _clock = logical_clock();
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
