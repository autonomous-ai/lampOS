//! The say service's turn (#939): a say becomes the voice's written turn
//! through the real coordinator path, and its asker is told `spoken` only
//! when the turn completed with its audio played whole through the real
//! speaker control; a turn cut short, or one with no audio, is `dropped`
//! with what became of it. The provider PCM and speaker receipts are
//! authored, not acoustic evidence.
use super::*;
use crate::say::{Answer, Outcome};
use lamp_audio::ownership::PlaybackCursor;
use std::sync::mpsc;

struct Speaker {
    channels: WorkerChannels,
}
impl SpeakerControlIo for Speaker {
    fn next_speaker_event(&mut self) -> io::Result<Option<WorkerEvent>> {
        self.channels.control.receive()
    }
    fn publish_speaker_snapshot(&mut self, snapshot: Snapshot) -> io::Result<()> {
        self.channels.control.send(Control::Authority { snapshot })
    }
}

struct Test {
    _directory: SessionDirectory,
    speaker: Speaker,
    peer: WorkerChannels,
    owner: Controller,
    reply: Option<Reply>,
    pending: Option<PendingSpeaker>,
    snapshot: Snapshot,
    mask: PlaybackMask,
    trace: SessionState,
    cloud: CloudAvailability,
    sequence: u64,
    cursor: u64,
    answered: mpsc::Receiver<Answer>,
    /// The news the last turn ended with as heard (#1390).
    heard: Vec<String>,
}

impl Test {
    /// A say taken by the voice and begun: its written turn submitted to
    /// the provider, which has started it.
    fn new() -> Self {
        let directory = SessionDirectory::create().unwrap();
        let (channels, peer) = pair(&directory, "speaker");
        let (mut provider, mut provider_peer) = pair(&directory, "provider");
        let mut speaker = Speaker { channels };
        let mut owner = Controller::new(new_boot().unwrap(), now());
        owner
            .set_microphone_permission(now(), Permission::Allowed)
            .unwrap();
        let until = MonoTime::from_micros(monotonic_us() + 1_000_000);
        owner
            .set_capture(now(), CaptureState::RetainingUntil(until))
            .unwrap();
        owner
            .set_admission(now(), AdmissionState::OpenUntil(until))
            .unwrap();
        let (ticket, answered) = crate::say::ticket_for_test("reminders");
        let mut reply = None;
        let mut queue = VecDeque::new();
        let mut trace = SessionState::default();
        let snapshot = commit_written_turn(
            &mut owner,
            &mut reply,
            &mut queue,
            "Remind them to call the bank.",
            false,
            "skill_says",
            1,
            PlayoutStart::default(),
            &mut trace,
            |state| speaker.publish_speaker_snapshot(state),
        )
        .unwrap();
        reply.as_mut().unwrap().say = Some(ticket);
        let turn = reply.as_ref().unwrap().owner;
        assert!(matches!(queue.front(), Some(ProviderInput::Written {
            request, session: 1, text, look: false, ..
        }) if *request == turn.turn() && text == "Remind them to call the bank."));
        let mut cloud = CloudAvailability::default();
        cloud.observe(ready_link(1, 1), monotonic_us()).unwrap();
        flush_provider_input(&cloud, &mut reply, &mut queue, &mut provider.data).unwrap();
        assert!(matches!(
            provider_peer.data.receive::<ProviderInput>().unwrap(),
            Some(ProviderInput::Written { request, .. }) if request == turn.turn()
        ));
        observe_provider_output(
            &mut cloud,
            &mut reply,
            &ProviderOutput::Started {
                request: turn.turn(),
                session: 1,
                waiting_for_barrier: false,
            },
        )
        .unwrap();
        Self {
            _directory: directory,
            speaker,
            peer,
            owner,
            reply,
            pending: None,
            snapshot,
            mask: PlaybackMask::new(TurnTaking::VadOnly),
            trace,
            cloud,
            sequence: 0,
            cursor: 0,
            answered,
            heard: Vec::new(),
        }
    }

    fn renew_input_evidence(&mut self) {
        let until = MonoTime::from_micros(monotonic_us() + 1_000_000);
        self.owner
            .set_capture(now(), CaptureState::RetainingUntil(until))
            .unwrap();
        self.owner
            .set_admission(now(), AdmissionState::OpenUntil(until))
            .unwrap();
    }

    fn event(&mut self, event: WorkerEvent) {
        self.peer.control.send(event).unwrap();
        service_speaker_control(
            &mut self.speaker,
            &mut self.owner,
            &mut self.reply,
            &mut self.pending,
            &mut self.mask,
            &mut self.snapshot,
            &mut 0,
            &mut 16,
            &mut self.trace,
            &mut None,
        )
        .unwrap();
    }

    /// One generation of the model's audio, handed to the speaker.
    fn generation(&mut self, value: i16) -> SpeakerChunk {
        self.renew_input_evidence();
        let request = self.reply.as_ref().unwrap().owner.turn();
        let received_at = monotonic_us();
        let samples = vec![value; 240];
        observe_provider_output(
            &mut self.cloud,
            &mut self.reply,
            &ProviderOutput::Audio {
                request,
                sequence: self.sequence + 1,
                provider_event_at_us: received_at,
                source_frames: samples.len(),
                source_offset: 0,
                samples: samples.clone(),
            },
        )
        .unwrap();
        let current = self.reply.as_mut().unwrap();
        current.append_provider_audio(samples, received_at);
        current.had_audio = true;
        observe_provider_output(
            &mut self.cloud,
            &mut self.reply,
            &ProviderOutput::GenerationComplete { request },
        )
        .unwrap();
        dispatch_speaker_chunk(
            &mut self.owner,
            &mut self.reply,
            &mut self.speaker.channels.data,
            &mut self.pending,
            &mut self.sequence,
            &mut self.snapshot,
            &mut self.mask,
        )
        .unwrap();
        let chunk = self.peer.data.receive::<SpeakerChunk>().unwrap().unwrap();
        self.trace
            .receipts
            .dispatched(chunk.permit.owner(), chunk.chunk_sequence)
            .unwrap();
        chunk
    }

    fn accept(&mut self, chunk: &SpeakerChunk, frames: usize) {
        let at = monotonic_us();
        self.event(WorkerEvent::Playback {
            owner: chunk.permit.owner(),
            turn: chunk.permit.owner().turn(),
            sequence: chunk.chunk_sequence,
            first_sample: PlaybackCursor {
                epoch: 1,
                frame: self.cursor,
            },
            end_sample: PlaybackCursor {
                epoch: 1,
                frame: self.cursor + frames as u64,
            },
            queue_observed_at_us: at,
            queued_frames: 0,
            accepted_frames: frames,
            observed_at_us: at,
        });
        self.cursor += frames as u64;
    }

    fn retire(&mut self, chunk: &SpeakerChunk) {
        self.event(WorkerEvent::SpeechRetired {
            owner: chunk.permit.owner(),
            sequence: chunk.chunk_sequence,
            final_sample: PlaybackCursor {
                epoch: 1,
                frame: self.cursor,
            },
            observed_at_us: monotonic_us(),
        });
    }

    fn provider_complete(&mut self) {
        let request = self.reply.as_ref().unwrap().owner.turn();
        observe_provider_output(
            &mut self.cloud,
            &mut self.reply,
            &ProviderOutput::TurnComplete {
                request,
                idle: true,
            },
        )
        .unwrap();
    }

    /// As the voice's loop ends a settled turn: completed, published, and
    /// the say answered.
    fn finish_if_settled(&mut self) -> bool {
        let current = self.reply.as_mut().unwrap();
        if !current.settled(&self.pending) {
            return false;
        }
        self.owner.complete_turn(now(), current.owner).unwrap();
        self.snapshot = self.owner.snapshot(now()).unwrap();
        self.speaker
            .publish_speaker_snapshot(self.snapshot)
            .unwrap();
        current.answer_say(&self.pending, self.snapshot);
        self.heard = current.heard_news(&self.pending, self.snapshot);
        self.reply = None;
        true
    }
}

/// Two generations, each accepted and retired, then the provider's
/// completion: `spoken`, and not a moment before the turn settles.
#[test]
fn a_say_whose_turn_completes_with_its_audio_played_is_spoken() {
    let mut test = Test::new();
    let first = test.generation(17);
    test.accept(&first, 240);
    test.retire(&first);
    assert!(!test.finish_if_settled(), "one generation is not the turn");
    assert!(test.answered.try_recv().is_err());
    let second = test.generation(23);
    test.provider_complete();
    test.accept(&second, 240);
    assert!(!test.finish_if_settled(), "accepted is not retired");
    assert!(test.answered.try_recv().is_err());
    test.retire(&second);
    assert!(test.finish_if_settled());
    assert_eq!(test.answered.try_recv(), Ok(Answer::Done(Outcome::Spoken)));
}

/// A person speaks over it: the voice cancels the turn, and the asker is
/// told it was dropped, and why.
#[test]
fn a_say_cut_short_is_dropped_with_why() {
    let mut test = Test::new();
    let first = test.generation(17);
    test.accept(&first, 120);
    cancel_reply(
        &mut test.owner,
        &mut test.reply,
        &mut test.trace,
        "user_interrupted",
    )
    .unwrap();
    assert_eq!(
        test.answered.try_recv(),
        Ok(Answer::Done(Outcome::Dropped("user_interrupted".into())))
    );
}

/// The model said nothing: the turn settles with no audio, and the say is
/// dropped as `no_audio_answer`.
#[test]
fn a_say_the_model_left_silent_is_dropped() {
    let mut test = Test::new();
    test.provider_complete();
    assert!(test.finish_if_settled());
    assert_eq!(
        test.answered.try_recv(),
        Ok(Answer::Done(Outcome::Dropped("no_audio_answer".into())))
    );
}

/// The news a turn told: two tasks told together, as `tell_together` gives
/// them, put on the turn by the news branch's own `news_on` (the reviewer
/// on #1425: with that a no-op, no harness would ever close).
const NEWS: [&str; 2] = ["s1:147:67", "s1:148:70"];
fn telling_news(test: &mut Test) {
    let told = crate::task_news::News {
        key: NEWS[0].into(),
        note: "(News from work you handed over.)".into(),
        until: None,
        progress: None,
        finished: None,
        also: vec![NEWS[1].into()],
    };
    news_on(told, &mut test.reply);
    assert_eq!(test.reply.as_ref().unwrap().news, NEWS);
}

/// #1390, the reviewer on #1425: the news a turn told reaches the tasks
/// part (so that its harness may be closed) only as the turn ends heard
/// whole, on the proof a say's `spoken` takes; every key told together
/// with it too, and not a moment before.
#[test]
fn news_heard_whole_is_handed_on_as_the_turn_ends() {
    let mut test = Test::new();
    telling_news(&mut test);
    let chunk = test.generation(17);
    test.provider_complete();
    test.accept(&chunk, 240);
    assert!(!test.finish_if_settled(), "accepted is not retired");
    test.retire(&chunk);
    assert!(test.finish_if_settled());
    assert_eq!(test.heard, NEWS);
}

/// #1390, the follow-up to #1425: news the person asked about
/// (`check_agent_task`) comes from the provider worker as `NewsAnswered`
/// for the request it was answered in. On that turn it is handed on as the
/// turn ends heard whole, like news told unasked; for another request, a
/// turn already over, it is let go.
#[test]
fn news_answered_to_a_question_rides_on_its_own_turn() {
    let mut test = Test::new();
    let turn = test.reply.as_ref().unwrap().owner.turn();
    for (request, key) in [(turn, "s1:2:4"), (turn + 1, "s1:3:5"), (turn, "s1:2:4")] {
        observe_provider_output(
            &mut test.cloud,
            &mut test.reply,
            &ProviderOutput::NewsAnswered {
                request,
                key: key.into(),
            },
        )
        .unwrap();
    }
    let chunk = test.generation(17);
    test.provider_complete();
    test.accept(&chunk, 240);
    test.retire(&chunk);
    assert!(test.finish_if_settled());
    assert_eq!(test.heard, ["s1:2:4"]);
}

/// A Stop or the person speaking over it cancels the turn: the news goes
/// with it, never handed on, and its harness stays open.
#[test]
fn news_cut_short_is_never_handed_on() {
    let mut test = Test::new();
    telling_news(&mut test);
    let chunk = test.generation(17);
    test.accept(&chunk, 120);
    cancel_reply(
        &mut test.owner,
        &mut test.reply,
        &mut test.trace,
        "user_interrupted",
    )
    .unwrap();
    assert!(test.reply.is_none());
    assert!(test.heard.is_empty());
}

/// A turn that ends with no audio, or with a gap in its playback, is no news
/// heard. (Muting the mic takes the turn from its owner: it is cut short.)
#[test]
fn news_not_heard_whole_is_never_handed_on() {
    let mut silent = Test::new();
    telling_news(&mut silent);
    silent.provider_complete();
    assert!(silent.finish_if_settled());
    assert!(silent.heard.is_empty());

    let mut gap = Test::new();
    telling_news(&mut gap);
    let chunk = gap.generation(17);
    gap.accept(&chunk, 240);
    gap.retire(&chunk);
    gap.reply.as_mut().unwrap().playback_gaps = 1;
    gap.provider_complete();
    assert!(gap.finish_if_settled());
    assert!(gap.heard.is_empty());
}

/// #939, (c): with the switch, a due reminder or news waits for a skill's
/// ask, at most `SKILL_WAITS_US` from when it was first held: taken with
/// the ask's ticket when one is there, said by the voice once it has
/// waited, and at once without the switch. Another one due begins its own
/// wait; an ask for the other source is not taken.
#[test]
fn a_due_reminder_or_news_waits_for_a_skill_at_most_its_time() {
    use crate::say::{SKILL_WAITS_US, Says, Source};
    let mut held = None;
    let mut idle = Says::idle_for_test();
    // Without the switch and nobody asking: said at once, as always.
    assert!(matches!(
        for_skill(
            Some(&mut idle),
            Source::Schedule,
            false,
            "s/1/1",
            None,
            &mut held,
            0
        ),
        Some(None)
    ));
    // With no say service nothing can ask: said at once, switch or not
    // (Fourth on #1038).
    assert!(matches!(
        for_skill(None, Source::Schedule, true, "s/1/1", None, &mut held, 0),
        Some(None)
    ));
    assert_eq!(held, None);
    // With it and nobody asking: held, and said by the voice at 10 s.
    let mut wait = |key: &str, at: u64, held: &mut Option<(String, u64)>| {
        for_skill(Some(&mut idle), Source::Schedule, true, key, None, held, at)
    };
    assert!(wait("s/1/1", 1_000, &mut held).is_none());
    assert!(wait("s/1/1", SKILL_WAITS_US, &mut held).is_none());
    // Another reminder due begins its own wait.
    assert!(wait("s/2/1", SKILL_WAITS_US + 5, &mut held).is_none());
    assert!(matches!(
        wait("s/2/1", 2 * SKILL_WAITS_US + 5, &mut held),
        Some(None)
    ));
    assert_eq!(held, None);
    // Without the switch, a skill that asked is still told: its ask is
    // taken, and it moves with the words (Fourth on #1038).
    let (mut says, answered) = Says::asking_for_test(Source::Schedule, "reminders", 60_000_000);
    let ticket = for_skill(
        Some(&mut says),
        Source::Schedule,
        false,
        "s/3/1",
        None,
        &mut held,
        2_000,
    )
    .flatten()
    .expect("the skill's ticket, switch off");
    ticket.answer(Outcome::Spoken);
    assert_eq!(answered.try_recv(), Ok(Answer::Done(Outcome::Spoken)));
    // A skill asked for news: a reminder does not take it; news does.
    let (mut says, answered) = Says::asking_for_test(Source::Tasks, "task-news", 60_000_000);
    assert!(
        for_skill(
            Some(&mut says),
            Source::Schedule,
            true,
            "s/4/1",
            None,
            &mut held,
            2_000
        )
        .is_none()
    );
    let taken = for_skill(
        Some(&mut says),
        Source::Tasks,
        true,
        "t/7",
        None,
        &mut held,
        2_000,
    );
    let ticket = taken.flatten().expect("the skill's ticket");
    assert_eq!(ticket.skill(), "task-news");
    assert_eq!(held, None);
    ticket.answer(Outcome::Spoken);
    assert_eq!(answered.try_recv(), Ok(Answer::Done(Outcome::Spoken)));
    assert_eq!(says.waiting(), 0);
}

/// The reviewer on #1042: news of task 9 is first in line, and the Task
/// news skill asks for task 10. With `LAMP_TASK_NEWS=skill` 9's news does
/// not take 10's ask: it waits its 10 s for an ask of its own, then the
/// voice says it, and 10's ask is still there for 10's news.
#[test]
fn news_takes_only_an_ask_for_its_own_task() {
    use crate::say::{SKILL_WAITS_US, Says, Source};
    let (mut says, answered) =
        Says::asking_for_task_test(Source::Tasks, "task-news", Some(10), 60_000_000);
    let mut held = None;
    let mut nine = |now_us| {
        for_skill(
            Some(&mut says),
            Source::Tasks,
            true,
            "s:9:1",
            Some(9),
            &mut held,
            now_us,
        )
    };
    assert!(nine(1_000).is_none(), "9 waits for its own ask");
    assert!(matches!(nine(1_000 + SKILL_WAITS_US), Some(None)));
    let ticket = for_skill(
        Some(&mut says),
        Source::Tasks,
        true,
        "s:10:1",
        Some(10),
        &mut held,
        2_000 + SKILL_WAITS_US,
    )
    .flatten()
    .expect("10's ask, for 10's news");
    ticket.answer(Outcome::Spoken);
    assert_eq!(answered.try_recv(), Ok(Answer::Done(Outcome::Spoken)));
}

/// #938: an owner with nothing under way and listening ready, as at rest.
fn resting_owner() -> Controller {
    let mut owner = Controller::new(new_boot().unwrap(), now());
    owner
        .set_microphone_permission(now(), Permission::Allowed)
        .unwrap();
    let until = MonoTime::from_micros(monotonic_us() + 1_000_000);
    owner
        .set_capture(now(), CaptureState::RetainingUntil(until))
        .unwrap();
    owner
        .set_admission(now(), AdmissionState::OpenUntil(until))
        .unwrap();
    owner
}

/// The say service's gates at rest, but for what `reply` and `admission`
/// say: nothing queued or pending, quiet heard, 15 s since the last turn.
fn sound_now(
    owner: &mut Controller,
    reply: &Option<Reply>,
    admission: &InputAdmission,
    says: &crate::say::Says,
) -> crate::say::SoundNow {
    let snapshot = owner.snapshot(now()).unwrap();
    skill_sound_now(
        SoundGates {
            at_us: monotonic_us(),
            paused: false,
            snapshot: &snapshot,
            reply,
            provider_queue: &VecDeque::new(),
            speaker_pending: &None,
            playback_mask: &PlaybackMask::new(TurnTaking::VadOnly),
            admission,
            submission_draining: false,
            heard_quiet: true,
            reply_owed: false,
            reminder_waiting: false,
            since_turn_us: crate::say::QUIET_US,
        },
        says,
    )
}

/// A skill's sound taken from `says` and played, or refused.
fn take_sound(
    says: &mut crate::say::Says,
    now_is: &crate::say::SoundNow,
    owner: &mut Controller,
    reply: &mut Option<Reply>,
    trace: &mut SessionState,
) -> bool {
    play_skill_sound(
        says,
        now_is,
        monotonic_us(),
        owner,
        reply,
        PlayoutStart::default(),
        trace,
        |_| Ok(()),
    )
    .unwrap()
    .is_some()
}

/// The speaker's side of a sound: its chunks dispatched and each accepted.
struct Played {
    _directory: SessionDirectory,
    speaker: Speaker,
    peer: WorkerChannels,
    last: SpeakerChunk,
    cursor: u64,
    snapshot: Snapshot,
    mask: PlaybackMask,
}

fn play_sound(
    owner: &mut Controller,
    reply: &mut Option<Reply>,
    pending: &mut Option<PendingSpeaker>,
    trace: &mut SessionState,
    chunks: usize,
) -> Played {
    let directory = SessionDirectory::create().unwrap();
    let (channels, mut peer) = pair(&directory, "speaker");
    let mut speaker = Speaker { channels };
    let mut snapshot = owner.snapshot(now()).unwrap();
    let mut mask = PlaybackMask::new(TurnTaking::VadOnly);
    let (mut sequence, mut cursor, mut last) = (0, 0, None);
    for _ in 0..chunks {
        dispatch_speaker_chunk(
            owner,
            reply,
            &mut speaker.channels.data,
            pending,
            &mut sequence,
            &mut snapshot,
            &mut mask,
        )
        .unwrap();
        let chunk = peer.data.receive::<SpeakerChunk>().unwrap().unwrap();
        trace
            .receipts
            .dispatched(chunk.permit.owner(), chunk.chunk_sequence)
            .unwrap();
        let at = monotonic_us();
        peer.control
            .send(WorkerEvent::Playback {
                owner: chunk.permit.owner(),
                turn: chunk.permit.owner().turn(),
                sequence: chunk.chunk_sequence,
                first_sample: PlaybackCursor {
                    epoch: 1,
                    frame: cursor,
                },
                end_sample: PlaybackCursor {
                    epoch: 1,
                    frame: cursor + 240,
                },
                queue_observed_at_us: at,
                queued_frames: 0,
                accepted_frames: 240,
                observed_at_us: at,
            })
            .unwrap();
        service_speaker_control(
            &mut speaker,
            owner,
            reply,
            pending,
            &mut mask,
            &mut snapshot,
            &mut 0,
            &mut 16,
            trace,
            &mut None,
        )
        .unwrap();
        cursor += 240;
        last = Some(chunk);
    }
    Played {
        _directory: directory,
        speaker,
        peer,
        last: last.unwrap(),
        cursor,
        snapshot,
        mask,
    }
}

/// #938: a skill's sound at rest is played as the lamp's own turn, and
/// its asker told `begun`, then `spoken` once it played to its end; and
/// another within 30 s is refused.
#[test]
fn a_skill_sound_played_to_its_end_is_spoken() {
    let (mut says, answered) = crate::say::Says::sound_for_test("chime", vec![500; 480]);
    let mut owner = resting_owner();
    let (mut reply, mut pending, mut trace) = (None, None, SessionState::default());
    let admission = InputAdmission::default();
    let now_is = sound_now(&mut owner, &reply, &admission, &says);
    assert_eq!(crate::say::may_play_sound(&now_is), Ok(()));
    assert!(take_sound(
        &mut says, &now_is, &mut owner, &mut reply, &mut trace
    ));
    assert_eq!(
        reply.as_ref().unwrap().notice.origin(),
        SpeechOrigin::SkillSound
    );
    assert!(matches!(answered.try_recv(), Ok(Answer::Begun(_))));
    let mut played = play_sound(&mut owner, &mut reply, &mut pending, &mut trace, 2);
    assert!(played.last.end_of_speech);
    assert!(
        !reply.as_ref().unwrap().settled(&pending),
        "accepted is not retired"
    );
    played
        .peer
        .control
        .send(WorkerEvent::SpeechRetired {
            owner: played.last.permit.owner(),
            sequence: played.last.chunk_sequence,
            final_sample: PlaybackCursor {
                epoch: 1,
                frame: played.cursor,
            },
            observed_at_us: monotonic_us(),
        })
        .unwrap();
    service_speaker_control(
        &mut played.speaker,
        &mut owner,
        &mut reply,
        &mut pending,
        &mut played.mask,
        &mut played.snapshot,
        &mut 0,
        &mut 16,
        &mut trace,
        &mut None,
    )
    .unwrap();
    let current = reply.as_mut().unwrap();
    assert!(current.settled(&pending));
    owner.complete_turn(now(), current.owner).unwrap();
    let snapshot = owner.snapshot(now()).unwrap();
    current.answer_say(&pending, snapshot);
    reply = None;
    assert_eq!(answered.try_recv(), Ok(Answer::Done(Outcome::Spoken)));
    // Another sound within 30 s of it: refused.
    let (mut again, answered) = crate::say::Says::sound_for_test("chime", vec![500; 480]);
    again.sound_played(monotonic_us());
    let now_is = sound_now(&mut owner, &reply, &admission, &again);
    assert!(!take_sound(
        &mut again, &now_is, &mut owner, &mut reply, &mut trace
    ));
    assert!(reply.is_none());
    assert_eq!(
        answered.try_recv(),
        Ok(Answer::Done(Outcome::Refused(
            "a sound played less than 30 s ago".into()
        )))
    );
}

/// #938: a skill's sound cut before its end, as by a person speaking over
/// it, is `dropped` with why, never `spoken`.
#[test]
fn a_skill_sound_cut_short_is_dropped() {
    let (mut says, answered) = crate::say::Says::sound_for_test("chime", vec![500; 480]);
    let mut owner = resting_owner();
    let (mut reply, mut pending, mut trace) = (None, None, SessionState::default());
    let admission = InputAdmission::default();
    let now_is = sound_now(&mut owner, &reply, &admission, &says);
    assert!(take_sound(
        &mut says, &now_is, &mut owner, &mut reply, &mut trace
    ));
    assert!(matches!(answered.try_recv(), Ok(Answer::Begun(_))));
    let _played = play_sound(&mut owner, &mut reply, &mut pending, &mut trace, 1);
    cancel_reply(&mut owner, &mut reply, &mut trace, "user_interrupted").unwrap();
    assert_eq!(
        answered.try_recv(),
        Ok(Answer::Done(Outcome::Dropped("user_interrupted".into())))
    );
}

/// #938: with a reply open, or a candidate of someone's speech not yet a
/// turn, a skill's sound is refused at once and never played later.
#[test]
fn a_skill_sound_is_refused_while_a_reply_or_a_candidate_is_open() {
    let mut owner = resting_owner();
    let mut trace = SessionState::default();
    // A reply open: a say's written turn.
    let (mut says, answered) = crate::say::Says::sound_for_test("chime", vec![500; 480]);
    let mut reply = None;
    let mut queue = VecDeque::new();
    commit_written_turn(
        &mut owner,
        &mut reply,
        &mut queue,
        "Remind them to call the bank.",
        false,
        "skill_says",
        1,
        PlayoutStart::default(),
        &mut trace,
        |_| Ok(()),
    )
    .unwrap();
    let turn = reply.as_ref().unwrap().owner;
    let admission = InputAdmission::default();
    let now_is = sound_now(&mut owner, &reply, &admission, &says);
    assert!(!take_sound(
        &mut says, &now_is, &mut owner, &mut reply, &mut trace
    ));
    assert_eq!(reply.as_ref().unwrap().owner, turn, "the reply goes on");
    assert_eq!(
        answered.try_recv(),
        Ok(Answer::Done(Outcome::Refused(
            "the lamp is speaking".into()
        )))
    );
    assert!(says.take_sound(monotonic_us()).is_none(), "never later");
    // A candidate open: speech heard, not yet a turn.
    let mut owner = resting_owner();
    let (mut says, answered) = crate::say::Says::sound_for_test("chime", vec![500; 480]);
    let mut admission = InputAdmission::default();
    // The authority issued at the very moment the speech is observed: one
    // issued a microsecond later is not yet current at `at`, and the
    // candidate is refused as unavailable (failed about 1 run in 5).
    let at = monotonic_us();
    let authority = owner.snapshot(MonoTime::from_micros(at)).unwrap();
    let update = admission
        .observe(
            Activity::Start(vec![ObservedAudio {
                sequence: 1,
                captured_at_us: at,
                samples: [7; lamp_audio::CAPTURE_SAMPLES],
            }]),
            AdmissionContext {
                capture: CaptureLineage {
                    worker: new_boot().unwrap(),
                    epoch: 1,
                    dsp_epoch: 2,
                    privacy_generation: authority.microphone_generation(),
                },
                authority,
            },
            at,
        )
        .unwrap();
    assert!(matches!(update.step, AdmissionStep::Candidate(_)));
    assert!(!admission.owns_stream(), "a candidate, not a stream");
    let mut reply = None;
    let now_is = sound_now(&mut owner, &reply, &admission, &says);
    assert!(!take_sound(
        &mut says, &now_is, &mut owner, &mut reply, &mut trace
    ));
    assert!(reply.is_none());
    assert_eq!(
        answered.try_recv(),
        Ok(Answer::Done(Outcome::Refused(
            "the lamp is listening".into()
        )))
    );
}

/// #1144: the ring asks for no frame while a skill's sound plays; the loop
/// tells it so (`tell_ring_of_turn`), and only the head tap's "hm?" is shown.
#[test]
fn the_ring_prepares_no_frame_for_a_skill_sound() {
    // Without telling the ring, a skill's sound turn gets a frame: the
    // control.
    let (mut says, _answered) = crate::say::Says::sound_for_test("chime", vec![500; 480]);
    let mut owner = resting_owner();
    let (mut reply, mut trace) = (None, SessionState::default());
    let now_is = sound_now(&mut owner, &reply, &InputAdmission::default(), &says);
    assert!(take_sound(
        &mut says, &now_is, &mut owner, &mut reply, &mut trace
    ));
    let mut ring = crate::choreography::RingChoreographer::new(40).unwrap();
    assert!(
        ring.prepare(&mut owner, now()).unwrap().is_some(),
        "the control shows the frame the sound must not have"
    );
    // Told as the loop tells it: nothing, in that pass and after.
    let (mut says, _answered) = crate::say::Says::sound_for_test("chime", vec![500; 480]);
    let mut owner = resting_owner();
    let (mut reply, mut trace) = (None, SessionState::default());
    let now_is = sound_now(&mut owner, &reply, &InputAdmission::default(), &says);
    assert!(take_sound(
        &mut says, &now_is, &mut owner, &mut reply, &mut trace
    ));
    let origin = reply.as_ref().unwrap().notice.origin();
    assert_eq!(origin, SpeechOrigin::SkillSound);
    let mut ring = crate::choreography::RingChoreographer::new(40).unwrap();
    tell_ring_of_turn(&mut ring, Some(origin));
    assert!(ring.prepare(&mut owner, now()).unwrap().is_none());
    assert!(ring.prepare(&mut owner, now()).unwrap().is_none());
}
