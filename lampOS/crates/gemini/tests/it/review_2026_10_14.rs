//! Review of what changed on the night of 2026-10-14
//! (`docs/voice-review-2026-10-14.md`): tests only.
//!
//! The note the voice sends beside what is being said
//! (`Supervisor::try_note`, used by `crates/voice/src/called.rs`), through
//! the supervisor and a real session, against the scripted service of
//! `lamp_gemini::testing`. No credential, socket or cloud request.
//!
//! The tests whose names begin `review_` show a fault and fail today. The
//! ones that begin `review_sound_` are edges that were tried and held.
use lamp_gemini::{
    BarrierWait, Credential, Error, Event, GOOGLE_ENDPOINT, Notice, RecoveryPolicy, RequestId,
    Resumption, SessionConfig, SuccessorStart, Supervisor,
    testing::{Dial, FakeConnector, connector, next_dial, sent_audio},
};
use serde_json::Value;
use std::time::{Duration, Instant};
use tokio::{sync::mpsc::UnboundedReceiver, time::timeout};

const WAIT: Duration = Duration::from_secs(3);
/// What the voice sends, in substance (`crates/voice/src/called.rs`).
const NOTE: &str = "(For you to know, not to answer: your own ears caught your name, Lamp, just now. If what is being said is something a lamp on a desk could answer or chat about, it is said to you.)";

type Rig = (Supervisor<FakeConnector>, UnboundedReceiver<Dial>);

fn request(id: u64) -> RequestId {
    RequestId::new(id).unwrap()
}

fn plain() -> SessionConfig {
    SessionConfig::new(GOOGLE_ENDPOINT, Credential::api_key("test-only").unwrap())
        .unwrap()
        .resumption(Resumption::Retain)
}

/// As the lamp runs it (`--provider-continuation kept-start` in
/// `deploy/lamp-4ace/lamp-voice.service`; `Continuation::KeptStart`).
fn kept_start() -> SessionConfig {
    plain()
        .barrier_wait(BarrierWait::AtCommit)
        .barrier_reports(true)
        .successor_start(SuccessorStart::AfterFirstOutput {
            patience: Duration::from_millis(2_500),
        })
        .unwrap()
}

/// A supervisor whose first connection the scripted service has accepted.
async fn connected(config: SessionConfig) -> (Rig, Dial) {
    let policy = RecoveryPolicy {
        first_backoff: Duration::from_millis(20),
        max_backoff: Duration::from_millis(40),
        outage_budget: Duration::from_secs(2),
        ..RecoveryPolicy::default()
    };
    let (dialer, mut dials) = connector(config);
    let mut supervisor = Supervisor::new(dialer, policy).unwrap();
    let serve = async {
        let mut dial = next_dial(&mut dials).await;
        dial.service.accept().await;
        dial
    };
    let (ready, dial) = tokio::join!(
        async {
            timeout(WAIT, supervisor.next())
                .await
                .expect("ready in time")
        },
        serve
    );
    assert!(matches!(ready, Ok(Notice::Ready { .. })), "{ready:?}");
    ((supervisor, dials), dial)
}

/// Notices until `request` is started; a lost link fails the test.
async fn started(supervisor: &mut Supervisor<FakeConnector>, request: RequestId) {
    loop {
        match timeout(WAIT, supervisor.next())
            .await
            .expect("notice in time")
        {
            Ok(Notice::Event(Event::InputStarted { lineage, .. }))
                if lineage.request == request =>
            {
                return;
            }
            Ok(Notice::Unavailable { error }) => panic!("the link was lost: {error:?}"),
            Ok(_) => {}
            Err(failure) => panic!("the link ended: {failure:?}"),
        }
    }
}

/// Every client message the service has, until `quiet` passes with none.
async fn everything(dial: &mut Dial, quiet: Duration) -> Vec<Value> {
    let mut all = Vec::new();
    while let Some(message) = dial.service.try_receive(quiet).await {
        all.push(message);
    }
    all
}

fn is(message: &Value, field: &str) -> bool {
    message["realtimeInput"].get(field).is_some()
}

/// What the service received, one word a message: the field, the first
/// sample of a block of sound, `note` for the note.
fn shape(sent: &[Value]) -> Vec<String> {
    sent.iter()
        .map(|message| {
            if let Some(pcm) = sent_audio(message) {
                format!("audio of {}", pcm[0])
            } else if message["realtimeInput"]["text"] == NOTE {
                "note".to_owned()
            } else {
                ["activityStart", "activityEnd"]
                    .into_iter()
                    .find(|field| is(message, field))
                    .unwrap_or("other")
                    .to_owned()
            }
        })
        .collect()
}

/// The note was offered for request 1 while it was being heard and went
/// out, inside its activity. Request 1 was then given up before its end
/// (the voice throws out an utterance it judges a sound, "Hey Lamp" said
/// alone for one, or the microphone is closed): the worker abandons it and
/// closes its input, and no activityEnd is written, so the activity stays
/// open. Whatever is said next, by anyone and however much later, continues
/// that activity, and the model is given it with the note in front: "your
/// own ears caught your name, Lamp, just now ... it is said to you". The
/// note was about request 1 only. Both ways the session can be set up.
#[tokio::test]
async fn review_a_note_for_a_request_given_up_is_not_carried_into_the_next_request() {
    let mut faults = Vec::new();
    for (how, config) in [("at once", plain()), ("kept start", kept_start())] {
        let ((mut supervisor, _dials), mut dial) = connected(config).await;
        let one = request(1);
        supervisor.try_start(one).unwrap();
        started(&mut supervisor, one).await;
        supervisor
            .try_audio(one, 0, Instant::now(), &[1; 160])
            .unwrap();
        supervisor.try_note(one, NOTE).unwrap();
        // The note is offered well before the request is given up: on the
        // lamp the name is heard in its first second, and an utterance is
        // judged a sound only after 600 ms of quiet at its end. (As first
        // written this test waited here until the note was on the wire,
        // which it was. The mend is that it is no longer written until the
        // request is committed, so there is nothing to wait for.)
        let mut sent: Vec<Value> = everything(&mut dial, Duration::from_millis(150)).await;
        // Given up, as `ProviderIntake::synchronize` does for an abandoned
        // request: discard, then close the input it still holds open.
        supervisor.discard(one);
        supervisor.try_end(one).unwrap();
        // Later, someone else says something.
        let two = request(2);
        supervisor.try_start(two).unwrap();
        started(&mut supervisor, two).await;
        supervisor
            .try_audio(two, 0, Instant::now(), &[2; 160])
            .unwrap();
        supervisor.try_end(two).unwrap();
        sent.extend(everything(&mut dial, Duration::from_millis(300)).await);
        let shape = shape(&sent);
        let first_of_two = shape
            .iter()
            .position(|word| word == "audio of 2")
            .expect("request 2's sound reached the service");
        let opened = shape[..first_of_two]
            .iter()
            .rposition(|word| word == "activityStart")
            .expect("request 2's sound is inside an activity");
        let closed = first_of_two
            + shape[first_of_two..]
                .iter()
                .position(|word| word == "activityEnd")
                .expect("request 2 was committed");
        if shape[opened..=closed].iter().any(|word| word == "note") {
            faults.push(format!(
                "{how}: request 2 is committed in the activity that carries request 1's note: {shape:?}"
            ));
        }
        assert!(supervisor.is_connected());
    }
    assert!(faults.is_empty(), "{faults:#?}");
}

/// A note offered at a moment it does not belong (no request, a request
/// that is not the one submitted, a request whose input has ended, one that
/// was given up) is refused before anything is written, and the link goes
/// on: a refused note never reaches the session, where it would end the
/// connection. A note offered at the right moment then goes out in order.
#[tokio::test]
async fn review_sound_a_note_at_a_wrong_moment_is_refused_here_and_the_link_stays() {
    for config in [plain(), kept_start()] {
        let ((mut supervisor, _dials), mut dial) = connected(config).await;
        // Nothing submitted yet.
        assert_eq!(
            supervisor.try_note(request(1), NOTE),
            Err(Error::StaleRequest)
        );
        let one = request(1);
        supervisor.try_start(one).unwrap();
        started(&mut supervisor, one).await;
        supervisor
            .try_audio(one, 0, Instant::now(), &[1; 160])
            .unwrap();
        // Not the request whose sound is being sent.
        assert_eq!(
            supervisor.try_note(request(2), NOTE),
            Err(Error::StaleRequest)
        );
        supervisor.try_end(one).unwrap();
        // Its input has ended: the note would be outside its activity.
        assert_eq!(supervisor.try_note(one, NOTE), Err(Error::StaleRequest));
        let sent = everything(&mut dial, Duration::from_millis(200)).await;
        assert_eq!(shape(&sent), ["activityStart", "audio of 1", "activityEnd"]);
        // The service declines to answer; nothing more is owed.
        dial.service
            .send(serde_json::json!({"serverContent":{"turnComplete":true,"interactionStatus":"IDLE"}}))
            .await;
        // A request given up while its note was due.
        let two = request(2);
        supervisor.try_start(two).unwrap();
        started(&mut supervisor, two).await;
        supervisor.discard(two);
        assert_eq!(supervisor.try_note(two, NOTE), Err(Error::StaleRequest));
        supervisor.try_end(two).unwrap();
        // And at the right moment, the note goes inside the activity: after
        // the request's sound and just before its end, since the mend of
        // the fault above (it went in order with the sound when this was
        // first written).
        let three = request(3);
        supervisor.try_start(three).unwrap();
        started(&mut supervisor, three).await;
        supervisor
            .try_audio(three, 0, Instant::now(), &[3; 160])
            .unwrap();
        supervisor.try_note(three, NOTE).unwrap();
        supervisor
            .try_audio(three, 1, Instant::now(), &[3; 160])
            .unwrap();
        supervisor.try_end(three).unwrap();
        let sent = everything(&mut dial, Duration::from_millis(200)).await;
        let shape = shape(&sent);
        let at = shape.iter().position(|word| word == "note").expect("noted");
        assert_eq!(
            shape[at - 2..],
            ["audio of 3", "audio of 3", "note", "activityEnd"]
        );
        assert!(supervisor.is_connected());
    }
}
