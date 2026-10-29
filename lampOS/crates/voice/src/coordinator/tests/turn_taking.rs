//! Deterministic tests of the production observation/decision/commit functions.
use super::*;
use crate::{activity::START_BLOCKS, turn_taking::TAIL_US};
use lamp_audio::ownership::PlaybackCursor;
use std::{cell::RefCell, rc::Rc};

thread_local! {
    static CLOCK: RefCell<std::rc::Weak<crate::scenario_clock::Scope>> =
        const { RefCell::new(std::rc::Weak::new()) };
}

/// All live harnesses on this test thread share their dated history. The last
/// one releases the scope, including when a test shadows an earlier harness.
fn fixture_clock() -> Rc<crate::scenario_clock::Scope> {
    CLOCK.with(|clock| {
        let mut clock = clock.borrow_mut();
        if let Some(scope) = clock.upgrade() {
            return scope;
        }
        let scope = Rc::new(
            crate::scenario_clock::Scope::enter_at(Duration::from_secs(10), 600_000_000).unwrap(),
        );
        *clock = Rc::downgrade(&scope);
        scope
    })
}

struct Harness {
    owner: Controller,
    reply: Option<Reply>,
    mask: PlaybackMask,
    detector: TurnDetector,
    admission: InputAdmission,
    trace: SessionState,
    lineage: CaptureLineage,
    base: u64,
    sequence: u64,
    /// Why each sound let go while an answer waited was let go (#1123).
    let_go: Vec<&'static str>,
    /// The microphone's and the admission's leases renewed with each block.
    renew: bool,
    /// Each block heard by the mask with echo evidence, nothing playing, as
    /// the loop hears it (`hear_capture_block`), and whose reply it heard.
    echo: bool,
    /// The blocks come without echo evidence, as before the evidence starts.
    echo_missing: bool,
    /// The speaker plays the lamp's sound, not yet matched by the echo
    /// evidence: the mask cannot hear a person over it yet.
    render_active: bool,
    heard_reply: Option<TurnOwner>,
    /// The wait's state as the loop keeps it, and the candidate that last
    /// waited (#1123).
    wait: AnswerWait,
    kept_for: Option<crate::admission::CandidateId>,
    /// The candidate decided at once when the answer began to sound.
    sounded_for: Option<crate::admission::CandidateId>,
    /// The last block heard as speech, as the loop keeps it.
    last_speech_us: u64,
    /// A waited sound's words (`LAMP_WAIT_KEEPS_ANSWER=words`), and what
    /// they did (#1123 step 2).
    words: Option<crate::wait_words::WaitWords>,
    told: Vec<WordsTold>,
    /// The answer was asked to stop by a waited sound's words.
    stopped: bool,
    _clock: Rc<crate::scenario_clock::Scope>,
}
impl Harness {
    /// The product's turn-taking and mask with `LAMP_WAIT_KEEPS_ANSWER=on`:
    /// the duplex mask hearing every block with echo evidence, with the
    /// memory the loop gives it for the wait (`keep_waited_sound`), and the
    /// duplex hold. The answer to the question that ended at the base time
    /// has not arrived: no audio of it yet. Its leases renewed with each
    /// block, as the loop renews them, and its clock `lag` back: a commit
    /// reads the real clock, which must come after the last block and
    /// within its 0.9 s lease.
    fn duplex(lag: u64) -> Self {
        Self::product(lag, true)
    }
    /// [`Self::duplex`], with the mask's usual memory or the wait's.
    fn product(lag: u64, waited_memory: bool) -> Self {
        let mut h = Self::with_lag(TurnTaking::Duplex, lag);
        h.admission = InputAdmission::with_hold(crate::admission::Hold::DUPLEX);
        h.renew = true;
        h.mask = PlaybackMask::duplex();
        if waited_memory {
            h.mask.keep_waited_sound();
        }
        h.echo = true;
        h.reply.as_mut().unwrap().pcm.clear();
        h.wait = AnswerWait {
            on: true,
            asked_until_us: Some(h.base),
            ..AnswerWait::default()
        };
        h
    }
    fn new(policy: TurnTaking) -> Self {
        Self::with_lag(policy, 500_000)
    }
    fn with_lag(policy: TurnTaking, lag: u64) -> Self {
        let clock = fixture_clock();
        let base = monotonic_us() - lag;
        let time = MonoTime::from_micros(base);
        let mut owner = Controller::new(new_boot().unwrap(), time);
        owner
            .set_microphone_permission(time, Permission::Allowed)
            .unwrap();
        let until = MonoTime::from_micros(base + 900_000);
        owner
            .set_capture(time, CaptureState::RetainingUntil(until))
            .unwrap();
        owner
            .set_admission(time, AdmissionState::OpenUntil(until))
            .unwrap();
        let turn = owner.admit(time, AdmittedInput::NewTurn).unwrap();
        let plan = owner
            .plan_output(time, turn, OutputKind::Speech, None)
            .unwrap();
        owner.input_ended(time, turn).unwrap();
        let lineage = CaptureLineage {
            worker: new_boot().unwrap(),
            epoch: 1,
            dsp_epoch: 1,
            privacy_generation: owner.snapshot(time).unwrap().microphone_generation(),
        };
        let reply = Reply {
            owner: turn,
            cloud: submitted_request(),
            local_input_ended: true,
            plan,
            playback: None,
            pcm: vec![7; 480].into(),
            generation_done: false,
            provider_idle: false,
            input_sequence: 1,
            had_audio: false,
            voice_level: 0,
            told_stop: None,
            final_chunk: None,
            playback_gaps: 0,
            notice: Default::default(),
            media: Default::default(),
            local_only: None,
            reminder: None,
            say: None,
            news: Vec::new(),
            last_provider_audio_at_us: None,
            start: lamp_audio::playout::StartGate::new(0),
            need: lamp_audio::playout::StartNeed::default(),
            handed: 0,
            sounded: false,
        };
        Self {
            owner,
            reply: Some(reply),
            mask: PlaybackMask::new(policy),
            detector: TurnDetector::default(),
            admission: InputAdmission::default(),
            trace: SessionState::default(),
            lineage,
            base,
            sequence: 0,
            let_go: Vec::new(),
            renew: false,
            echo: false,
            echo_missing: false,
            render_active: false,
            heard_reply: None,
            wait: AnswerWait::default(),
            kept_for: None,
            sounded_for: None,
            last_speech_us: 0,
            words: None,
            told: Vec::new(),
            stopped: false,
            _clock: clock,
        }
    }
    fn old(&self) -> TurnOwner {
        self.reply.as_ref().unwrap().owner
    }
    fn dispatch(&mut self, offset: u64) {
        self.mask
            .dispatched(self.old(), 1, self.base + offset, 240)
            .unwrap();
    }
    fn accepted(&mut self, written: u64, received: u64) {
        self.mask
            .accepted(
                self.old(),
                1,
                PlaybackCursor {
                    epoch: 1,
                    frame: 480,
                },
                PlaybackCursor {
                    epoch: 1,
                    frame: 720,
                },
                240,
                self.base + written,
                self.base + written,
                self.base + received,
            )
            .unwrap();
    }
    fn drain(&mut self, offset: u64) {
        self.mask
            .drained(
                self.old(),
                1,
                PlaybackCursor {
                    epoch: 1,
                    frame: 720,
                },
                self.base + offset,
                self.base + offset,
            )
            .unwrap();
    }
    fn feed(&mut self, offset: u64, score: f32) -> Result<Option<Accepted>> {
        self.feed_received(offset, offset, score)
    }
    fn feed_received(
        &mut self,
        offset: u64,
        received: u64,
        score: f32,
    ) -> Result<Option<Accepted>> {
        self.step(offset, received, score, false)
    }
    /// [`Self::feed`], with #1123's wait applied as the loop applies it.
    fn feed_waiting(&mut self, offset: u64, score: f32) -> Result<Option<Accepted>> {
        self.step(offset, offset, score, true)
    }
    fn step(
        &mut self,
        offset: u64,
        received: u64,
        score: f32,
        wait: bool,
    ) -> Result<Option<Accepted>> {
        self.sequence += 1;
        let at_us = self.base + received;
        if self.renew {
            let time = MonoTime::from_micros(at_us);
            let until = MonoTime::from_micros(at_us + 900_000);
            self.owner
                .set_capture(time, CaptureState::RetainingUntil(until))?;
            self.owner
                .set_admission(time, AdmissionState::OpenUntil(until))?;
        }
        let read_us = self.base + offset;
        if score >= 0.5 {
            self.last_speech_us = read_us;
        }
        let context = AdmissionContext {
            capture: self.lineage,
            authority: self.owner.snapshot(MonoTime::from_micros(at_us))?,
        };
        let observation = ObservedAudio {
            sequence: self.sequence,
            captured_at_us: read_us,
            samples: [self.sequence as i16; 160],
        };
        let quiet = super::duplex::evidence(if self.render_active { 0 } else { 200 }, None);
        let evidence = (!self.echo_missing).then_some(&quiet);
        let heard = if self.echo {
            let (heard, yielded) = hear_capture_block(
                &mut self.mask,
                &self.reply,
                &mut self.heard_reply,
                self.sequence,
                read_us,
                score,
                evidence,
                at_us,
                &mut self.trace,
            )?;
            if yielded {
                self.detector.reach_back();
            }
            heard
        } else {
            score
        };
        let (update, _) = observe_prospective_heard(
            &mut self.mask,
            &mut self.detector,
            &mut self.admission,
            context,
            observation,
            read_us - 10_000,
            heard,
            evidence.is_some().then_some(score),
            at_us,
        )?;
        if let Some(r) = update.rejection {
            record_input_rejection(&mut self.trace, r)?;
        }
        let candidate = match update.step {
            AdmissionStep::Candidate(c) => Some(c),
            AdmissionStep::None => self
                .admission
                .pending_evidence(at_us, AcceptanceBasis::DirectedSessionVadOnly)
                .map(|(c, _)| c),
            AdmissionStep::Rejected(r) => {
                record_input_rejection(&mut self.trace, r)?;
                None
            }
            _ => None,
        };
        if wait && let Some(words) = self.words.as_mut() {
            let still_pending = words
                .holds()
                .is_some_and(|id| self.admission.pending_audio(id).is_some());
            let awaited = words_awaited(self.reply.as_ref(), self.wait.asked_until_us, at_us);
            words.follow(awaited, still_pending);
        }
        if wait && let Some(c) = candidate.as_ref() {
            let state = AnswerWait {
                waited: self.kept_for == Some(c.id),
                sounding: reply_sounding(&self.mask, &self.reply, at_us),
                decided: self.sounded_for == Some(c.id),
                heard: !self.mask.is_duplex() || (self.echo && !self.echo_missing),
                ..self.wait
            };
            let mut waits = while_answer_waits(
                state,
                self.reply.as_ref(),
                c,
                self.admission.pending_ended(),
                self.detector.pause_blocks(),
                at_us,
            );
            if let Some(words) = self.words.as_mut() {
                let told;
                (waits, told) = with_waited_words(
                    words,
                    waits,
                    c,
                    self.admission.pending_audio(c.id).unwrap_or_default(),
                    self.admission.pending_ended(),
                    self.detector.pause_blocks(),
                    self.last_speech_us,
                    may_hold_once_decided(state),
                    at_us,
                );
                self.told.extend(told);
            }
            match waits {
                WhileAnswerWaits::Decide => {
                    if state.waited && state.sounding {
                        self.sounded_for = Some(c.id);
                    }
                }
                WhileAnswerWaits::Wait => {
                    self.kept_for = Some(c.id);
                    return Ok(None);
                }
                WhileAnswerWaits::LetGo(why) => {
                    if let Some(r) = self.admission.release_prospective(at_us)? {
                        record_input_rejection(&mut self.trace, r)?;
                    }
                    self.wait.let_go_ended_us = Some(self.last_speech_us);
                    self.detector.clear_prospective();
                    self.let_go.push(why);
                    return Ok(None);
                }
                // As the loop does: let go, and the answer stopped next pass.
                WhileAnswerWaits::Stop => {
                    if let Some(r) = self.admission.release_prospective(at_us)? {
                        record_input_rejection(&mut self.trace, r)?;
                    }
                    self.detector.clear_prospective();
                    self.let_go.push("stop_said");
                    self.stopped = true;
                    return Ok(None);
                }
            }
        }
        match candidate {
            Some(c) => decide_prospective_input(
                &self.mask,
                &mut self.detector,
                &mut self.admission,
                c,
                context,
                at_us,
                &mut self.trace,
            ),
            None => Ok(None),
        }
    }
    fn commit(&mut self, accepted: Accepted) -> VecDeque<ProviderInput> {
        self.commit_with(accepted, false)
    }
    fn commit_with(&mut self, accepted: Accepted, cut_in: bool) -> VecDeque<ProviderInput> {
        self.commit_told(accepted, cut_in, false, false)
    }
    /// Committed with how much of the reply it cuts was played
    /// (`LAMP_SAID_AS_HEARD`).
    fn commit_heard(
        &mut self,
        accepted: Accepted,
        cut: crate::said_as_heard::HeardOfCut,
    ) -> VecDeque<ProviderInput> {
        let mut queue = VecDeque::new();
        commit_admitted_input(
            &mut self.owner,
            &mut self.reply,
            &mut None,
            &mut queue,
            accepted,
            1,
            false,
            false,
            true,
            None,
            Some(cut),
            false,
            false,
            false,
            true,
            PlayoutStart::default(),
            &mut self.trace,
            |_| Ok(()),
        )
        .unwrap();
        queue
    }
    fn commit_told(
        &mut self,
        accepted: Accepted,
        cut_in: bool,
        after_stop: bool,
        follows_on: bool,
    ) -> VecDeque<ProviderInput> {
        let mut queue = VecDeque::new();
        commit_admitted_input(
            &mut self.owner,
            &mut self.reply,
            &mut None,
            &mut queue,
            accepted,
            1,
            false,
            false,
            cut_in,
            after_stop.then_some((1, 1)),
            None,
            follows_on,
            false,
            false,
            true,
            PlayoutStart::default(),
            &mut self.trace,
            |_| Ok(()),
        )
        .unwrap();
        queue
    }
}

#[test]
fn v1_baseline_full_reply_echo_never_replaces_owner_or_queues_start() {
    let mut h = Harness::new(TurnTaking::V1Baseline);
    let old = h.old();
    h.dispatch(10_000);
    h.accepted(11_000, 11_000);
    for n in 1..=80 {
        let accepted = h.feed(20_000 + n * 10_000, 0.99).unwrap();
        assert!(
            accepted.is_none(),
            "own reply became a destructive candidate"
        );
    }
    assert_eq!(h.old(), old);
    assert!(h.reply.as_ref().unwrap().pcm.len() == 480);
    assert!(!h.trace.iter().any(|e| e["kind"] == "turn_finished"));
}

#[test]
fn v1_baseline_first_write_race_waits_then_discards_original_overlapping_input() {
    let mut h = Harness::new(TurnTaking::V1Baseline);
    h.dispatch(10_000); // First control drain saw no receipt; write is independently in flight.
    for n in 1..=u64::from(START_BLOCKS) {
        assert!(
            h.feed(10_000 + n * 10_000, 0.99).unwrap().is_none(),
            "unsettled write was assumed silent"
        );
    }
    assert!(
        h.admission
            .pending_evidence(h.base + 70_000, AcceptanceBasis::DirectedSessionVadOnly)
            .is_some()
    );
    h.accepted(11_000, 71_000); // Actual source time, not arrival, covers retained onset.
    assert!(h.feed(80_000, 0.99).unwrap().is_none());
    assert!(
        h.admission
            .pending_evidence(h.base + 80_000, AcceptanceBasis::DirectedSessionVadOnly)
            .is_none()
    );
    assert!(!h.admission.owns_stream());
}

/// `LAMP_SAID_AS_HEARD`: a reply counts the samples it hands the speaker,
/// not its last chunk's padding. A line that cuts it is told how much was
/// played, on its Start and in its record. With no reply in hand, the next
/// line on a Stop's session is told what the Stop cut, once. Off, nothing.
#[test]
fn a_line_that_cuts_a_reply_is_told_how_much_of_it_was_played() {
    use crate::said_as_heard::HeardOfCut;
    let mut h = Harness::new(TurnTaking::V1Baseline);
    let reply = h.reply.as_mut().unwrap();
    reply.pcm = vec![7; 600].into();
    reply.generation_done = true;
    // 240, 240 and its last 120, padded to 240.
    for _ in 0..3 {
        assert!(reply.next_speaker_chunk().is_some());
    }
    assert!(reply.next_speaker_chunk().is_none());
    assert_eq!(reply.handed, 600);
    let request = reply.owner.turn();
    let mut stop_cut = None;
    let cut = heard_of_cut(true, h.reply.as_ref(), &mut stop_cut, 1);
    assert_eq!(
        cut,
        Some(HeardOfCut {
            request,
            samples: 600
        })
    );
    assert_eq!(
        heard_of_cut(false, h.reply.as_ref(), &mut stop_cut, 1),
        None
    );
    // A Stop that cuts the reply keeps how much was played, and its
    // session, for the next line; off, nothing.
    let session = h.reply.as_ref().unwrap().cloud.session;
    assert_eq!(
        stop_cut_of(true, h.reply.as_ref()),
        Some((cut.unwrap(), session))
    );
    assert_eq!(stop_cut_of(false, h.reply.as_ref()), None);
    assert_eq!(stop_cut_of(true, None), None);
    let stopped = HeardOfCut {
        request: 3,
        samples: 4_800,
    };
    stop_cut = Some((stopped, 1));
    assert_eq!(heard_of_cut(true, None, &mut stop_cut, 1), Some(stopped));
    assert_eq!(heard_of_cut(true, None, &mut stop_cut, 1), None, "once");
    stop_cut = Some((stopped, 1));
    assert_eq!(heard_of_cut(true, None, &mut stop_cut, 2), None);
    assert!(stop_cut.is_none(), "another session's is let go");
    stop_cut = Some((stopped, 1));
    assert_eq!(heard_of_cut(false, None, &mut stop_cut, 1), None, "off");
    // The line that cuts it carries it.
    h.dispatch(10_000);
    h.accepted(11_000, 11_000);
    h.drain(20_000);
    let mut accepted = None;
    for n in 1..=30 {
        if let Some(a) = h.feed(20_000 + TAIL_US + n * 10_000, 0.99).unwrap() {
            accepted = Some(a);
            break;
        }
    }
    let cut = cut.unwrap();
    let queue = h.commit_heard(accepted.expect("a line"), cut);
    assert!(
        queue.iter().any(|input| matches!(
            input,
            ProviderInput::Start {
                heard_of_cut: Some(told),
                ..
            } if *told == cut
        )),
        "{queue:?}"
    );
    let admitted = h
        .trace
        .iter()
        .find(|event| event["kind"] == "input_admitted")
        .expect("admitted");
    assert_eq!(
        admitted["heard_of_cut"],
        serde_json::json!({"request": request, "samples": 600})
    );
}

#[test]
fn v1_baseline_after_reply_starts_fresh_without_suppressed_prefix() {
    let mut h = Harness::new(TurnTaking::V1Baseline);
    h.dispatch(10_000);
    h.accepted(11_000, 11_000);
    h.drain(20_000);
    let until = 20_000 + TAIL_US;
    let mut accepted = None;
    for n in 1..=30 {
        let offset = 20_000 + n * 10_000;
        if let Some(a) = h.feed(offset, 0.99).unwrap() {
            accepted = Some(a);
            break;
        }
    }
    let accepted = accepted.expect("fresh post-tail onset was lost");
    assert!(
        accepted
            .audio
            .iter()
            .all(|f| f.captured_at_us - h.base > until)
    );
    assert_eq!(accepted.audio.len(), usize::from(START_BLOCKS));
    let queue = h.commit(accepted);
    assert_eq!(
        queue
            .iter()
            .filter(|e| matches!(e, ProviderInput::Start { .. }))
            .count(),
        1
    );
    assert_eq!(
        h.trace
            .iter()
            .filter(|e| e["outcome"] == "user_interrupted")
            .count(),
        1
    );
}

/// Feeds `blocks` of 10 ms at `score` from `*t`, and keeps the first input
/// admitted.
fn sound(
    h: &mut Harness,
    t: &mut u64,
    blocks: u32,
    score: f32,
    wait: bool,
    accepted: &mut Option<Accepted>,
) {
    for _ in 0..blocks {
        *t += 10_000;
        let admitted = if wait {
            h.feed_waiting(*t, score)
        } else {
            h.feed(*t, score)
        }
        .unwrap();
        if accepted.is_none() {
            *accepted = admitted;
        }
    }
}

/// Quiet before a sound: more than the 30 blocks of pre-roll, so that a
/// candidate holds its whole pre-roll, as on the lamp.
const QUIET_FIRST: u32 = 50;

/// What a fresh product harness (`LAMP_WAIT_KEEPS_ANSWER=on`) does with
/// `pieces` of 10 ms blocks at a score after [`QUIET_FIRST`], and 1.5 s of
/// quiet after: the input taken, if any, why each sound was let go, and the
/// harness's base time.
fn waited(pieces: &[(u32, f32)]) -> (Option<Accepted>, Vec<&'static str>, u64) {
    let mut h = Harness::duplex(4_000_000);
    let (mut t, mut accepted) = (0, None);
    sound(&mut h, &mut t, QUIET_FIRST, 0.0, true, &mut accepted);
    for &(blocks, score) in pieces {
        sound(&mut h, &mut t, blocks, score, true, &mut accepted);
    }
    sound(&mut h, &mut t, 150, 0.0, true, &mut accepted);
    (accepted, h.let_go, h.base)
}

/// #1123, `LAMP_WAIT_KEEPS_ANSWER=on`, on the product's mask and hold:
/// while an answer to someone is awaited and has not sounded, a short sound
/// with no words (0.4 s) does not take the turn. It waits, ends and is let
/// go, and the answer goes on; a second one 1.5 s later the same, with no
/// error. Without the wait, or with the switch off on the same path, the
/// same sound takes the turn at once (the reviewer on #1130).
#[test]
fn while_an_answer_waits_a_short_sound_is_let_go_and_the_answer_goes_on() {
    let mut h = Harness::duplex(4_000_000);
    let old = h.old();
    let (mut t, mut accepted) = (0, None);
    sound(&mut h, &mut t, QUIET_FIRST, 0.0, true, &mut accepted);
    for _ in 0..2 {
        sound(&mut h, &mut t, 40, 0.99, true, &mut accepted);
        sound(&mut h, &mut t, 150, 0.0, true, &mut accepted);
    }
    assert!(accepted.is_none(), "a short sound took the turn");
    assert_eq!(h.old(), old);
    assert_eq!(h.let_go.len(), 2, "{:?}", h.let_go);
    assert!(h.let_go.iter().all(|why| ["quiet", "ended"].contains(why)));
    for off in [false, true] {
        let mut h = Harness::duplex(4_000_000);
        h.wait.on = !off;
        let (mut t, mut accepted) = (0, None);
        sound(&mut h, &mut t, QUIET_FIRST, 0.0, off, &mut accepted);
        sound(&mut h, &mut t, 40, 0.99, off, &mut accepted);
        assert!(accepted.is_some(), "taken at once, switch off: {off}");
        assert!(h.let_go.is_empty());
    }
}

/// #1123: only `on`, or `words` (step 2), turns the wait on.
#[test]
fn the_wait_is_on_only_with_on_or_words() {
    for value in [
        None,
        Some(""),
        Some("off"),
        Some("ON"),
        Some("1"),
        Some("Words"),
    ] {
        assert!(!wait_keeps_answer(value), "{value:?}");
        assert!(!wait_hears_words(value), "{value:?}");
    }
    assert!(wait_keeps_answer(Some("on")));
    assert!(!wait_hears_words(Some("on")));
    assert!(wait_keeps_answer(Some("words")));
    assert!(wait_hears_words(Some("words")));
}

/// The seventh's P3 on #1277: a stream for a waited sound's words is kept
/// ready only while a sound may still wait. That is an answer unheard to a
/// request that ended no more than 3 s ago: past that, or with no known
/// end, a new sound is decided as before. A long agent answer keeps no idle
/// stream open to the relay.
#[test]
fn a_stream_for_words_is_kept_ready_only_while_a_sound_may_wait() {
    let h = Harness::duplex(4_000_000);
    let asked = Some(h.base);
    assert!(words_awaited(h.reply.as_ref(), asked, h.base));
    assert!(words_awaited(
        h.reply.as_ref(),
        asked,
        h.base + WAIT_KEEPS_WITHIN_US
    ));
    assert!(!words_awaited(
        h.reply.as_ref(),
        asked,
        h.base + WAIT_KEEPS_WITHIN_US + 1
    ));
    assert!(!words_awaited(h.reply.as_ref(), None, h.base));
    assert!(!words_awaited(None, asked, h.base));
    let mut heard = Harness::duplex(4_000_000);
    heard.reply.as_mut().unwrap().had_audio = true;
    assert!(!words_awaited(
        heard.reply.as_ref(),
        Some(heard.base),
        heard.base
    ));
}

/// What a product harness did with a sound and its words (#1123 step 2).
struct WithWords {
    accepted: Option<Accepted>,
    let_go: Vec<&'static str>,
    told: Vec<WordsTold>,
    /// What the ears were told, in order.
    ears: Vec<String>,
    /// Blocks from the sound's first: when its words were asked for, and
    /// when it was taken or let go.
    asked: Option<u32>,
    decided: Option<u32>,
    /// When the sound began, by the capture clock.
    began: u64,
    /// Its words asked the loop to stop the answer.
    stopped: bool,
}

/// A fresh product harness, with `LAMP_WAIT_KEEPS_ANSWER=words` on scripted
/// ears if `words`, or `on` alone: [`QUIET_FIRST`], a sound of `blocks` at
/// 0.99, then 1.5 s of quiet. `said` comes back from the ears `after`
/// blocks once its words were asked for, if ever.
fn with_words(blocks: u32, words: bool, said: Option<(&str, u32)>) -> WithWords {
    with_words_in(&[(blocks, 0.99)], words, said, None)
}

/// [`with_words`], with the sound in `pieces` of blocks at a score: speech,
/// a pause, speech again. With `answer`, the answer begins to sound at
/// that block of the sound (`answer_sounds`), with no echo evidence from
/// then on if its flag is set (`echo_missing`).
fn with_words_in(
    pieces: &[(u32, f32)],
    words: bool,
    said: Option<(&str, u32)>,
    answer: Option<(u32, bool)>,
) -> WithWords {
    let mut h = Harness::duplex(4_000_000);
    let script = words.then(|| {
        let (words, script) = crate::wait_words::tests::scripted();
        h.words = Some(words);
        script
    });
    let (mut t, mut accepted) = (0, None);
    sound(&mut h, &mut t, QUIET_FIRST, 0.0, true, &mut accepted);
    let began = h.base + t + 10_000;
    let (mut asked, mut decided) = (None, None);
    let scores: Vec<f32> = pieces
        .iter()
        .flat_map(|&(blocks, score)| std::iter::repeat_n(score, blocks as usize))
        .collect();
    for block in 0..scores.len() as u32 + 150 {
        let score = scores.get(block as usize).copied().unwrap_or(0.0);
        if let Some((at, echo_missing)) = answer
            && at == block
        {
            answer_sounds(&mut h);
            h.echo_missing = echo_missing;
        }
        sound(&mut h, &mut t, 1, score, true, &mut accepted);
        if decided.is_none() && (accepted.is_some() || !h.let_go.is_empty()) {
            decided = Some(block);
        }
        let Some(script) = script.as_ref() else {
            continue;
        };
        let mut script = script.lock().unwrap();
        if asked.is_none() && script.told.iter().any(|told| told == "finalize") {
            asked = Some(block);
        }
        if let (Some(asked), Some((text, after))) = (asked, said)
            && block == asked + after
        {
            script.words = Some(text.into());
        }
    }
    let ears = script.map_or_else(Vec::new, |script| script.lock().unwrap().told.clone());
    WithWords {
        accepted,
        let_go: h.let_go,
        told: h.told,
        ears,
        asked,
        decided,
        began,
        stopped: h.stopped,
    }
}

/// #1123 step 2: with `words`, a short "No." (0.4 s) said while an answer
/// waits, which `on` lets go, takes the turn from its start once its words
/// come back. Its audio from its first block went to the stream, its words
/// were asked for after 100 ms of quiet, and only their count is recorded.
#[test]
fn with_words_a_short_no_while_an_answer_waits_takes_the_turn_from_its_start() {
    let on = with_words(40, false, None);
    assert!(on.accepted.is_none(), "`on` lets it go");
    assert_eq!(on.let_go, ["ended"]);
    let heard = with_words(40, true, Some(("No.", 10)));
    let accepted = heard.accepted.expect("its words take the turn");
    assert!(heard.let_go.is_empty(), "{:?}", heard.let_go);
    let first = accepted.audio.first().unwrap().captured_at_us;
    assert!(first <= heard.began, "its start is kept");
    assert_eq!(heard.told.len(), 1);
    assert_eq!(heard.told[0].words, Some(1));
    assert!(heard.told[0].took);
    // Asked once it had been quiet for 100 ms, 10 blocks after its last,
    // and taken on the block after its words came back.
    assert_eq!(heard.asked, Some(40 + 10 - 1));
    assert_eq!(heard.decided, Some(40 + 10 - 1 + 10 + 1));
    assert!(heard.decided < on.decided, "before `on` lets it go");
    // Every block of it, from its pre-roll, went to the stream once.
    let sent = heard
        .ears
        .iter()
        .filter(|told| told.starts_with("hear"))
        .count();
    assert!(sent >= 40 + 10, "{sent} blocks");
    let mut values: Vec<_> = heard
        .ears
        .iter()
        .filter(|told| told.starts_with("hear"))
        .collect();
    let count = values.len();
    values.dedup();
    assert_eq!(values.len(), count, "no block twice");
    assert_eq!(heard.ears.first().map(String::as_str), Some("ready"));
    assert_eq!(
        heard.ears.iter().filter(|told| *told == "finalize").count(),
        1
    );
}

/// #1123 step 2: words that say nothing, or none back in time, leave the
/// sound to `on`. A cough's empty words, an "Um.", a laugh or the asker's
/// "Okay." (the seventh's P2) let it go when `on` does; none back lets it go when it ends, as `on` does, or at most 100 ms
/// later for one `on` lets go as quiet at its 1 s mark (0.75 s), which
/// waits for its words to just short of its deadline.
#[test]
fn with_words_a_cough_a_filler_or_no_words_in_time_are_let_go() {
    for blocks in [20, 40, 75] {
        let on = with_words(blocks, false, None);
        assert!(on.accepted.is_none(), "{blocks}");
        for said in [
            Some(("", 5)),
            Some(("Um.", 5)),
            Some(("Haha.", 5)),
            Some(("Okay.", 5)),
            None,
        ] {
            let heard = with_words(blocks, true, said);
            assert!(heard.accepted.is_none(), "{blocks} blocks, {said:?}");
            assert_eq!(heard.let_go.len(), 1, "{blocks} blocks, {said:?}");
            assert_eq!(heard.told.len(), 1, "{blocks} blocks, {said:?}");
            assert!(!heard.told[0].took);
            assert_eq!(heard.told[0].words, said.map(|_| 0), "{blocks} blocks");
            let (decided, before) = (heard.decided.unwrap(), on.decided.unwrap());
            if said.is_some() || blocks < 75 {
                assert_eq!(decided, before, "{blocks} blocks, {said:?}");
            } else {
                assert!(
                    decided > before && decided <= before + 10,
                    "{blocks} blocks: {decided} against {before}"
                );
            }
        }
    }
}

/// #1123 step 2: a sound whose words are not back when it ends is let go
/// then, never held for them: the next sound's start would find it still
/// pending, and the admission refuses that as a contradiction, which ends
/// the conversation. Here the next one begins on the block after the end,
/// and is taken at once, as one soon after a sound let go is.
#[test]
fn with_words_a_sound_is_never_held_for_them_past_its_end() {
    let mut h = Harness::duplex(4_000_000);
    let (words, _script) = crate::wait_words::tests::scripted();
    h.words = Some(words);
    let (mut t, mut accepted) = (0, None);
    sound(&mut h, &mut t, QUIET_FIRST, 0.0, true, &mut accepted);
    sound(&mut h, &mut t, 40, 0.99, true, &mut accepted);
    sound(&mut h, &mut t, 60, 0.0, true, &mut accepted);
    assert_eq!(h.let_go, ["ended"]);
    assert_eq!(h.told.len(), 1);
    assert_eq!(h.told[0].words, None);
    sound(&mut h, &mut t, 40, 0.99, true, &mut accepted);
    assert!(accepted.is_some(), "the next sound is taken");
}

/// #1123 step 2, measured on the product's mask and hold: how long words
/// may take to come back, once asked for, and still take the turn, by the
/// sound's length. A sound of 0.5 s or less ends, and is let go, 500 ms
/// after its words were asked for, so they have 490 ms; one quiet at its
/// 1 s mark waits for them to 100 ms short of its deadline: 450 ms at 0.6 s,
/// 350 ms at 0.7 s, 300 ms at 0.75 s. Words back later let it go, as `on`
/// does. A sound that goes on is taken as before, or sooner if it paused
/// and its words came back first; one with no pause never has them asked.
#[test]
fn how_long_the_words_may_take_by_length() {
    for (blocks, in_time) in [(20, 49), (40, 49), (50, 49), (60, 45), (70, 35), (75, 30)] {
        let taken = with_words(blocks, true, Some(("No.", in_time)));
        assert!(taken.accepted.is_some(), "{blocks} blocks, {in_time}");
        let late = with_words(blocks, true, Some(("No.", in_time + 1)));
        assert!(late.accepted.is_none(), "{blocks} blocks, {}", in_time + 1);
        assert_eq!(late.told[0].words, None);
    }
    let on = with_words(80, false, None);
    let heard = with_words(80, true, None);
    assert!(heard.accepted.is_some());
    assert_eq!(heard.decided, on.decided);
    let sooner = with_words(80, true, Some(("No.", 5)));
    assert!(sooner.accepted.is_some());
    assert_eq!(sooner.decided, Some(80 + 10 - 1 + 5 + 1));
    let on = with_words(140, false, None);
    let heard = with_words(140, true, Some(("No.", 5)));
    assert!(heard.accepted.is_some());
    assert_eq!(heard.decided, on.decided);
    assert_eq!(heard.asked, None);
    assert!(heard.told.is_empty());
}

/// #1123: a sound that goes on (1.4 s) takes the turn once it has gone on
/// for 1 s after its trigger, with its whole pre-roll, on the product's
/// mask, and the waiting answer is replaced as before. A follow-up with a
/// short pause in it (0.5 s, 0.2 s of quiet, 0.8 s) is taken too.
#[test]
fn while_an_answer_waits_a_sound_that_goes_on_takes_the_turn_from_its_start() {
    let mut h = Harness::duplex(2_400_000);
    let old = h.old();
    let (mut t, mut accepted) = (0, None);
    sound(&mut h, &mut t, QUIET_FIRST, 0.0, true, &mut accepted);
    let began = h.base + t;
    sound(&mut h, &mut t, 140, 0.99, true, &mut accepted);
    let accepted = accepted.expect("a sound that goes on takes the turn");
    let first = accepted.audio.first().unwrap().captured_at_us;
    assert!(
        first <= began,
        "its start is kept: {} ms before it",
        began.saturating_sub(first) / 1_000
    );
    let last = accepted.audio.last().unwrap().captured_at_us;
    assert!(
        last >= accepted.candidate.trigger_read_at_us + 1_000_000,
        "taken {} ms after its trigger",
        (last - accepted.candidate.trigger_read_at_us) / 1_000
    );
    assert!(h.let_go.is_empty());
    let queue = h.commit(accepted);
    assert_ne!(h.old(), old);
    assert!(matches!(queue.front(), Some(ProviderInput::Start { .. })));
    let (accepted, let_go, _) = waited(&[(50, 0.99), (20, 0.0), (80, 0.99)]);
    assert!(accepted.is_some(), "a follow-up with a pause was let go");
    assert!(let_go.is_empty(), "{let_go:?}");
}

/// The reviewer and the first builder on #1130: at its decision a waited
/// sound holds its pre-roll and 1 s more, about 130 blocks. With the mask's
/// usual memory (128) its oldest blocks are gone and it is rejected as
/// `retained_history_unavailable`, losing what was said; with the memory
/// the loop gives the mask for the wait (`keep_waited_sound`) it is taken.
#[test]
fn a_waited_sound_needs_the_memory_the_wait_gives_the_mask() {
    for memory in [false, true] {
        let mut h = Harness::product(4_000_000, memory);
        let (mut t, mut accepted) = (0, None);
        sound(&mut h, &mut t, QUIET_FIRST, 0.0, true, &mut accepted);
        sound(&mut h, &mut t, 140, 0.99, true, &mut accepted);
        let evicted = h
            .trace
            .iter()
            .any(|e| e["reason"] == "retained_history_unavailable");
        assert_eq!(accepted.is_some(), memory, "memory {memory}");
        assert_eq!(evicted, !memory, "memory {memory}");
    }
}

/// The reviewer on #1130, measured on the product's mask: what the wait
/// lets go and what it takes, by the sound's length. A sound of 0.75 s or
/// less is let go (it has ended, or gone quiet, by its decision); 0.8 s or
/// more is taken. Of the sixth's 0.6 to 1.4 s sounds on lamp-52e6, the
/// longer ones still take the turn.
#[test]
fn what_the_wait_lets_go_and_takes_by_length() {
    for blocks in [20, 40, 50, 60, 70, 75, 80, 90, 100, 120, 140] {
        let (accepted, let_go, _) = waited(&[(blocks, 0.99)]);
        assert_eq!(
            accepted.is_none(),
            blocks <= 75,
            "{} ms: let go {let_go:?}",
            blocks * 10
        );
        assert_eq!(
            let_go.len(),
            usize::from(blocks <= 75),
            "{} ms",
            blocks * 10
        );
    }
}

/// The reviewer on #1130: a follow-up with a pause across its 1 s mark
/// (0.7 s, 0.4 s of quiet, 0.6 s; or 0.6 s, 0.5 s, 0.5 s) has its first
/// piece let go. The next piece, begun within 1 s of that, is taken at once
/// (`AFTER_LET_GO_US`): only the first piece is lost, not the whole.
#[test]
fn a_follow_up_with_a_pause_loses_only_its_first_piece() {
    for pieces in [
        [(70, 0.99), (40, 0.0), (60, 0.99)],
        [(60, 0.99), (50, 0.0), (50, 0.99)],
    ] {
        let (accepted, let_go, base) = waited(&pieces);
        let accepted = accepted.expect("the second piece is taken");
        assert_eq!(let_go, ["quiet"], "{pieces:?}");
        // What was taken begins after the first piece: that piece is lost.
        let first_piece_ended = base + u64::from(QUIET_FIRST + pieces[0].0) * 10_000;
        let started = accepted.audio.first().unwrap().captured_at_us;
        assert!(
            started > first_piece_ended,
            "{pieces:?}: begins {} ms in",
            (started - base) / 1_000
        );
    }
}

/// The answer's audio arrives and plays: the mask hears the lamp sounding,
/// and the echo evidence has not matched it yet, so a person over it is
/// scored 0.0 (`Heard::Unknown`).
fn answer_sounds(h: &mut Harness) {
    let reply = h.reply.as_mut().unwrap();
    reply.had_audio = true;
    reply.pcm = vec![9; 480].into();
    h.render_active = true;
}

/// The first builder's re-check on #1130: once the answer sounds, the mask
/// scores a person's speech 0.0 until it has matched the lamp's sound, so a
/// waited sound's pause from then on is the mask's silence. A sound still
/// going when the answer begins to sound (1.4 s, the answer at 0.5 s) is
/// decided at once and taken, not let go as quiet; one that had stopped
/// 0.4 s before (a 0.4 s cough) is let go and the answer plays. The
/// answer's audio arriving with nothing yet to sound keeps the wait (the
/// first builder on #1130). A sound that begins after the answer has
/// sounded is decided as before.
#[test]
fn once_the_answer_sounds_a_waited_sound_is_judged_by_its_pause_then() {
    let mut h = Harness::duplex(4_000_000);
    let (mut t, mut accepted) = (0, None);
    sound(&mut h, &mut t, QUIET_FIRST, 0.0, true, &mut accepted);
    sound(&mut h, &mut t, 50, 0.99, true, &mut accepted);
    answer_sounds(&mut h);
    sound(&mut h, &mut t, 90, 0.99, true, &mut accepted);
    sound(&mut h, &mut t, 100, 0.0, true, &mut accepted);
    assert!(
        accepted.is_some(),
        "a sound running into the answer was lost: {:?}",
        h.let_go
    );
    assert!(h.let_go.is_empty(), "{:?}", h.let_go);

    let mut h = Harness::duplex(4_000_000);
    let old = h.old();
    let (mut t, mut accepted) = (0, None);
    sound(&mut h, &mut t, QUIET_FIRST, 0.0, true, &mut accepted);
    sound(&mut h, &mut t, 40, 0.99, true, &mut accepted);
    sound(&mut h, &mut t, 40, 0.0, true, &mut accepted);
    answer_sounds(&mut h);
    sound(&mut h, &mut t, 100, 0.0, true, &mut accepted);
    assert!(accepted.is_none(), "a cough over before the answer cut it");
    assert_eq!(h.old(), old);
    assert_eq!(h.let_go.len(), 1, "{:?}", h.let_go);

    let mut h = Harness::duplex(4_000_000);
    let (mut t, mut accepted) = (0, None);
    sound(&mut h, &mut t, QUIET_FIRST, 0.0, true, &mut accepted);
    sound(&mut h, &mut t, 30, 0.99, true, &mut accepted);
    h.reply.as_mut().unwrap().had_audio = true;
    sound(&mut h, &mut t, 20, 0.99, true, &mut accepted);
    sound(&mut h, &mut t, 150, 0.0, true, &mut accepted);
    assert!(
        accepted.is_none(),
        "the cough cut the answer once its audio came"
    );
    assert_eq!(h.let_go.len(), 1, "{:?}", h.let_go);

    let mut h = Harness::duplex(4_000_000);
    h.reply.as_mut().unwrap().had_audio = true;
    let (mut t, mut accepted) = (0, None);
    sound(&mut h, &mut t, QUIET_FIRST, 0.0, true, &mut accepted);
    sound(&mut h, &mut t, 10, 0.99, true, &mut accepted);
    assert!(
        accepted.is_some(),
        "an answer already sounding is cut as before"
    );
}

/// The first builder's re-check on #1130: a block with no echo evidence is
/// kept only among the mask's last reads (80, and 0.6 s), so a sound heard
/// without it does not wait: it is decided as before, and never lost.
#[test]
fn a_sound_heard_without_echo_evidence_does_not_wait() {
    let mut h = Harness::duplex(4_000_000);
    h.echo_missing = true;
    let (mut t, mut accepted) = (0, None);
    sound(&mut h, &mut t, QUIET_FIRST, 0.0, true, &mut accepted);
    sound(&mut h, &mut t, 140, 0.99, true, &mut accepted);
    assert!(accepted.is_some(), "a sound without evidence was lost");
    assert!(
        h.let_go.is_empty() && h.kept_for.is_none(),
        "{:?}",
        h.let_go
    );
}

/// The reviewer on #1130: a sound in which the lamp's name was heard, from
/// its start on, takes the turn ("Lamp, … stop" with its pause at the 1 s
/// mark); the name heard in the question, before it, does not.
#[test]
fn a_sound_with_the_lamps_name_in_it_takes_the_turn() {
    for (began, taken) in [(500_000, true), (0, false)] {
        let mut h = Harness::duplex(4_000_000);
        h.wait.name_began_us = Some(h.base + began);
        let (mut t, mut accepted) = (0, None);
        sound(&mut h, &mut t, QUIET_FIRST, 0.0, true, &mut accepted);
        sound(&mut h, &mut t, 70, 0.99, true, &mut accepted);
        sound(&mut h, &mut t, 150, 0.0, true, &mut accepted);
        assert_eq!(accepted.is_some(), taken, "name at {began}");
    }
}

/// The first builder and the reviewer on #1130: nothing waits for the
/// lamp's own turn (a hello, a look, a reminder: no input), for an answer
/// awaited more than 3 s, or on the standard hold (200 ms), where the wait
/// ends 100 ms before the deadline: a short sound is taken as before.
#[test]
fn the_wait_is_only_for_an_answer_to_someone_awaited_a_short_while() {
    let mut h = Harness::duplex(4_000_000);
    h.reply.as_mut().unwrap().input_sequence = 0;
    let (mut t, mut accepted) = (0, None);
    sound(&mut h, &mut t, QUIET_FIRST, 0.0, true, &mut accepted);
    sound(&mut h, &mut t, 40, 0.99, true, &mut accepted);
    assert!(accepted.is_some(), "the lamp's own turn waited");
    let mut h = Harness::duplex(4_000_000);
    h.wait.asked_until_us = Some(h.base - 2_800_000);
    let (mut t, mut accepted) = (0, None);
    sound(&mut h, &mut t, QUIET_FIRST, 0.0, true, &mut accepted);
    sound(&mut h, &mut t, 40, 0.99, true, &mut accepted);
    assert!(accepted.is_some(), "an answer awaited 3.3 s waited");
    let mut h = Harness::new(TurnTaking::V1Baseline);
    h.wait = AnswerWait {
        on: true,
        asked_until_us: Some(h.base),
        ..AnswerWait::default()
    };
    let (mut t, mut accepted) = (0, None);
    sound(&mut h, &mut t, 40, 0.99, true, &mut accepted);
    assert!(accepted.is_some(), "the standard hold's sound was lost");
    assert!(h.let_go.is_empty());
}

#[test]
fn v1_baseline_cloud_wait_without_any_write_still_accepts_resume() {
    let mut h = Harness::new(TurnTaking::V1Baseline);
    let old = h.old();
    let mut accepted = None;
    for n in 1..=6 {
        accepted = h.feed(n * 10_000, 0.99).unwrap().or(accepted);
    }
    let queue = h.commit(accepted.unwrap());
    assert_ne!(h.old(), old);
    assert!(matches!(queue.front(), Some(ProviderInput::Start { .. })));
}

/// #926, the reviewer: a candidate that had already ended when it was
/// admitted queues its End in the commit itself; a cut-in's says so, an
/// ordinary turn's does not.
#[test]
fn an_input_ended_at_admission_carries_whether_it_cut_in() {
    for cut_in in [true, false] {
        let mut h = Harness::new(TurnTaking::V1Baseline);
        let mut accepted = None;
        for n in 1..=6 {
            accepted = h.feed(n * 10_000, 0.99).unwrap().or(accepted);
        }
        let mut accepted = accepted.unwrap();
        accepted.ended = true;
        let queue = h.commit_with(accepted, cut_in);
        let end = queue
            .iter()
            .find(|input| matches!(input, ProviderInput::End { .. }))
            .expect("an input ended at admission queues its End");
        assert!(
            matches!(end, ProviderInput::End { cut_in: marked, sound: false, .. } if *marked == cut_in),
            "{end:?}"
        );
    }
}

/// #829, the first builder at 16:33: the first request after a hand or
/// the head stopped the lamp says so in its Start, for the model to be
/// told, and in its record; any other says nothing.
#[test]
fn the_first_request_after_a_stop_says_so_in_its_start() {
    for after_stop in [true, false] {
        let mut h = Harness::new(TurnTaking::V1Baseline);
        let mut accepted = None;
        for n in 1..=6 {
            accepted = h.feed(n * 10_000, 0.99).unwrap().or(accepted);
        }
        let queue = h.commit_told(accepted.unwrap(), false, after_stop, false);
        assert!(
            matches!(queue.front(), Some(ProviderInput::Start { after_stop: told, .. }) if *told == after_stop),
            "{queue:?}"
        );
        let admitted = h
            .trace
            .iter()
            .find(|row| row["kind"] == "input_admitted")
            .unwrap();
        assert_eq!(
            admitted.get("after_stop").is_some(),
            after_stop,
            "{admitted}"
        );
    }
}

/// A request begun soon after the lamp's words finished says so in its
/// Start, for the model to be told it may follow on, and in its record
/// (`LAMP_FOLLOWS_ON=on`, #829, the first builder at 09:01); any other
/// says nothing. Nor does the first request after a Stop, which is told the
/// lamp's words are over (the reviewer on #1278).
#[test]
fn a_request_begun_soon_after_the_lamps_words_says_so_in_its_start() {
    for (follows_on, after_stop, marked) in [
        (true, false, true),
        (false, false, false),
        (true, true, false),
    ] {
        let mut h = Harness::new(TurnTaking::V1Baseline);
        let mut accepted = None;
        for n in 1..=6 {
            accepted = h.feed(n * 10_000, 0.99).unwrap().or(accepted);
        }
        let queue = h.commit_told(accepted.unwrap(), false, after_stop, follows_on);
        assert!(
            matches!(queue.front(), Some(ProviderInput::Start { follows_on: told, .. }) if *told == marked),
            "{queue:?}"
        );
        let admitted = h
            .trace
            .iter()
            .find(|row| row["kind"] == "input_admitted")
            .unwrap();
        assert_eq!(admitted.get("follows_on").is_some(), marked, "{admitted}");
    }
}

/// #828, the reviewer on #1291: a request admitted over a reply in hand says
/// so in its Start, and says it was over the lamp's voice only when that
/// reply had sound: `(over_the_lamp, over_the_lamps_voice)` is
/// `(true, false)` over a silent reply waiting for its answer, and
/// `(true, true)` over one that had sound.
#[test]
fn a_request_over_a_reply_with_sound_says_so_in_its_start() {
    for had_audio in [false, true] {
        let mut h = Harness::new(TurnTaking::V1Baseline);
        h.reply.as_mut().unwrap().had_audio = had_audio;
        let mut accepted = None;
        for n in 1..=6 {
            accepted = h.feed(n * 10_000, 0.99).unwrap().or(accepted);
        }
        let queue = h.commit_told(accepted.unwrap(), false, false, false);
        assert!(
            matches!(
                queue.front(),
                Some(ProviderInput::Start {
                    over_the_lamp: true,
                    over_the_lamps_voice,
                    ..
                }) if *over_the_lamps_voice == had_audio
            ),
            "{queue:?}"
        );
    }
}

#[test]
fn vad_only_keeps_existing_experiment_semantics() {
    let mut h = Harness::new(TurnTaking::VadOnly);
    h.dispatch(10_000);
    h.accepted(11_000, 11_000);
    let mut accepted = None;
    for n in 1..=6 {
        accepted = h.feed(20_000 + n * 10_000, 0.99).unwrap().or(accepted);
    }
    assert!(accepted.is_some());
}

#[test]
fn v1_baseline_delayed_rejection_uses_latest_retained_evidence_without_renewing_deadline() {
    let mut h = Harness::new(TurnTaking::V1Baseline);
    h.dispatch(10_000);
    for n in 2..=18 {
        assert!(h.feed(n * 10_000, 0.99).unwrap().is_none());
    }
    let (original, _) = h
        .admission
        .pending_evidence(h.base + 180_000, AcceptanceBasis::DirectedSessionV1Baseline)
        .unwrap();
    assert_eq!(original.trigger_read_at_us, h.base + 70_000);
    h.mask.rejected(1, h.base + 181_000).unwrap();
    let accepted = h
        .feed(190_000, 0.99)
        .unwrap()
        .expect("fresh retained evidence must replace stale trigger evidence");
    assert_eq!(accepted.candidate.id, original.id);
    assert_eq!(
        accepted.candidate.decision_deadline_us,
        original.decision_deadline_us
    );
    assert_eq!(
        accepted.audio.last().unwrap().captured_at_us,
        h.base + 190_000
    );
    assert!(
        accepted.audio.last().unwrap().captured_at_us - accepted.candidate.trigger_read_at_us
            > 100_000
    );
}

#[test]
fn v1_baseline_unresolved_dispatch_expires_without_replacing_reply() {
    let mut h = Harness::new(TurnTaking::V1Baseline);
    let original = h.old();
    h.dispatch(10_000);
    for n in 2..=20 {
        assert!(h.feed(n * 10_000, 0.99).unwrap().is_none());
    }
    assert!(
        h.feed(210_000, 0.99)
            .unwrap_err()
            .to_string()
            .contains("write evidence expired")
    );
    assert_eq!(h.old(), original);
    assert!(!h.trace.iter().any(|e| e["kind"] == "input_admitted"));
}

#[test]
fn v1_baseline_does_not_suppress_previously_accepted_input_or_owned_outage_drain() {
    for outage in [false, true] {
        let mut h = Harness::new(TurnTaking::V1Baseline);
        let mut accepted = None;
        for n in 1..=6 {
            accepted = h.feed(n * 10_000, 0.99).unwrap().or(accepted);
        }
        assert!(accepted.is_some());
        if outage {
            h.admission
                .discard_stream_until_end(h.base + 60_000)
                .unwrap();
        }
        h.dispatch(61_000);
        h.accepted(62_000, 62_000);
        assert!(h.feed(70_000, 0.99).unwrap().is_none());
        assert!(h.admission.owns_stream());
        assert_eq!(
            h.mask.suppressed_blocks(),
            0,
            "accepted input cannot silently disappear"
        );
    }
}

#[test]
fn v1_baseline_long_output_gap_reopens_without_completing_reply() {
    let mut h = Harness::new(TurnTaking::V1Baseline);
    let old = h.old();
    h.dispatch(10_000);
    h.accepted(11_000, 11_000);
    h.drain(20_000);
    assert!(!h.reply.as_ref().unwrap().generation_done);
    assert!(h.reply.as_ref().unwrap().pcm.len() == 480);
    let mut accepted = None;
    for n in 1..=6 {
        accepted = h.feed(120_000 + n * 10_000, 0.99).unwrap().or(accepted);
    }
    assert_eq!(h.old(), old);
    assert!(!h.trace.iter().any(|e| e["kind"] == "turn_finished"));
    let queue = h.commit(accepted.unwrap());
    assert_ne!(h.old(), old);
    assert!(matches!(queue.front(), Some(ProviderInput::Start { .. })));
}

#[test]
fn v1_baseline_privacy_loss_and_capture_lineage_change_never_reopen_old_candidate() {
    let mut h = Harness::new(TurnTaking::V1Baseline);
    h.dispatch(10_000);
    for n in 2..=7 {
        assert!(h.feed(n * 10_000, 0.99).unwrap().is_none());
    }
    let (old, _) = h
        .admission
        .pending_evidence(h.base + 70_000, AcceptanceBasis::DirectedSessionV1Baseline)
        .unwrap();
    h.lineage.dsp_epoch += 1;
    h.detector.discard_inactive_prefix();
    h.mask.clear_read_lineage();
    assert!(h.feed(80_000, 0.99).unwrap().is_none());
    assert!(h.trace.iter().any(|e| e["reason"] == "capture_changed"));
    let context = AdmissionContext {
        capture: h.lineage,
        authority: h
            .owner
            .snapshot(MonoTime::from_micros(h.base + 80_000))
            .unwrap(),
    };
    assert!(matches!(
        h.admission
            .decide(
                Evidence {
                    candidate: old.id,
                    through_sequence: old.trigger_sequence,
                    produced_at_us: h.base + 80_000,
                    verdict: Verdict::Accept(AcceptanceBasis::DirectedSessionV1Baseline)
                },
                context,
                h.base + 80_000
            )
            .unwrap(),
        Decision::Ignored
    ));
    h.accepted(11_000, 81_000);
    h.owner
        .set_microphone_permission(MonoTime::from_micros(h.base + 82_000), Permission::Denied)
        .unwrap();
    assert!(h.feed(90_000, 0.99).is_err());
    assert!(
        !h.owner
            .snapshot(MonoTime::from_micros(h.base + 90_000))
            .unwrap()
            .listening_ready(MonoTime::from_micros(h.base + 90_000))
    );
}

struct SocketSpeaker {
    channels: WorkerChannels,
    published: Vec<Snapshot>,
}
impl SpeakerControlIo for SocketSpeaker {
    fn next_speaker_event(&mut self) -> io::Result<Option<WorkerEvent>> {
        self.channels.control.receive()
    }
    fn publish_speaker_snapshot(&mut self, snapshot: Snapshot) -> io::Result<()> {
        self.published.push(snapshot);
        Ok(())
    }
}
fn socket_pending(h: &mut Harness) -> Option<PendingSpeaker> {
    let permit = h
        .owner
        .issue_output(
            MonoTime::from_micros(h.base + 1),
            h.reply.as_ref().unwrap().plan,
            200_000,
        )
        .unwrap();
    Some(PendingSpeaker {
        origin: SpeechOrigin::Provider,
        sequence: 1,
        permit,
        accepted: 0,
        final_chunk: false,
        report_due_us: u64::MAX,
    })
}
fn write_receipt(h: &Harness, frames: usize, first: u64, completed: u64) -> WorkerEvent {
    WorkerEvent::Playback {
        owner: h.old(),
        turn: h.old().turn(),
        sequence: 1,
        accepted_frames: frames,
        queued_frames: 240,
        first_sample: PlaybackCursor {
            epoch: 1,
            frame: first,
        },
        end_sample: PlaybackCursor {
            epoch: 1,
            frame: first + frames as u64,
        },
        observed_at_us: h.base + completed,
        queue_observed_at_us: h.base + completed,
    }
}
fn service_socket(
    h: &mut Harness,
    io: &mut SocketSpeaker,
    pending: &mut Option<PendingSpeaker>,
    budget: &mut usize,
) -> bool {
    let mut snapshot = h.owner.snapshot(now()).unwrap();
    service_speaker_control(
        io,
        &mut h.owner,
        &mut h.reply,
        pending,
        &mut h.mask,
        &mut snapshot,
        &mut 0,
        budget,
        &mut h.trace,
        &mut None,
    )
    .unwrap()
}
#[test]
fn independent_speaker_socket_second_drain_catches_first_write_after_empty_check() {
    let directory = SessionDirectory::create().unwrap();
    let (channels, mut peer) = pair(&directory, "speaker");
    let mut io = SocketSpeaker {
        channels,
        published: vec![],
    };
    let mut h = Harness::with_lag(TurnTaking::V1Baseline, 100_000);
    let old = h.old();
    let mut pending = socket_pending(&mut h);
    h.dispatch(10_000);
    let mut budget = 32;
    // Use the actual channel read to prove the first priority check was empty.
    // Snapshot at the same source time here avoids moving the test controller
    // beyond the original capture observations about to be retained.
    let mut snapshot = h
        .owner
        .snapshot(MonoTime::from_micros(h.base + 10_000))
        .unwrap();
    assert!(
        service_speaker_control(
            &mut io,
            &mut h.owner,
            &mut h.reply,
            &mut pending,
            &mut h.mask,
            &mut snapshot,
            &mut 0,
            &mut budget,
            &mut h.trace,
            &mut None,
        )
        .unwrap()
    );
    for n in 2..=7 {
        assert!(h.feed(n * 10_000, 0.99).unwrap().is_none());
    }
    peer.control
        .send(write_receipt(&h, 240, 480, 11_000))
        .unwrap();
    assert!(service_socket(&mut h, &mut io, &mut pending, &mut budget));
    let context = AdmissionContext {
        capture: h.lineage,
        authority: h.owner.snapshot(now()).unwrap(),
    };
    let at = monotonic_us();
    let (candidate, _) = h
        .admission
        .pending_evidence(at, AcceptanceBasis::DirectedSessionV1Baseline)
        .unwrap();
    assert!(
        decide_prospective_input(
            &h.mask,
            &mut h.detector,
            &mut h.admission,
            candidate,
            context,
            at,
            &mut h.trace
        )
        .unwrap()
        .is_none()
    );
    assert_eq!(h.old(), old);
    assert!(pending.is_none());
    assert!(io.published.last().unwrap().playback().is_some());
    assert!(!h.trace.iter().any(|e| e["outcome"] == "user_interrupted"));
}

#[test]
fn speaker_control_budget_exhaustion_defers_pending_candidate_and_fault_keeps_priority() {
    let directory = SessionDirectory::create().unwrap();
    let (channels, mut peer) = pair(&directory, "speaker");
    let mut io = SocketSpeaker {
        channels,
        published: vec![],
    };
    let mut h = Harness::with_lag(TurnTaking::V1Baseline, 100_000);
    let old = h.old();
    let mut pending = socket_pending(&mut h);
    h.dispatch(10_000);
    for n in 2..=7 {
        assert!(h.feed(n * 10_000, 0.99).unwrap().is_none());
    }
    peer.control
        .send(write_receipt(&h, 120, 480, 11_000))
        .unwrap();
    peer.control
        .send(WorkerEvent::Fault {
            code: "test-only speaker failure".into(),
        })
        .unwrap();
    let mut budget = 1;
    assert!(!service_socket(&mut h, &mut io, &mut pending, &mut budget));
    assert_eq!(budget, 0);
    assert!(!service_socket(&mut h, &mut io, &mut pending, &mut budget));
    assert!(
        h.admission
            .pending_evidence(monotonic_us(), AcceptanceBasis::DirectedSessionV1Baseline)
            .is_some()
    );
    assert_eq!(h.old(), old);
    let mut snapshot = h.owner.snapshot(now()).unwrap();
    budget = 32;
    assert!(
        service_speaker_control(
            &mut io,
            &mut h.owner,
            &mut h.reply,
            &mut pending,
            &mut h.mask,
            &mut snapshot,
            &mut 0,
            &mut budget,
            &mut h.trace,
            &mut None,
        )
        .unwrap_err()
        .to_string()
        .contains("test-only speaker failure")
    );
    assert!(!h.trace.iter().any(|e| e["outcome"] == "user_interrupted"));
}

#[test]
fn actual_reset_receipt_preserves_old_mask_then_accepts_fresh_successor_epoch() {
    let directory = SessionDirectory::create().unwrap();
    let (channels, mut peer) = pair(&directory, "speaker");
    let mut io = SocketSpeaker {
        channels,
        published: vec![],
    };
    let mut h = Harness::new(TurnTaking::V1Baseline);
    h.dispatch(10_000);
    h.accepted(11_000, 11_000);
    let old = h.old();
    // The old nonfinal cursor already retired: no queued ownership-failure
    // record exists, but successful stop still reports its exact old range.
    peer.control
        .send(WorkerEvent::PlaybackClockReset {
            previous: lamp_audio::ownership::QueuedRange {
                epoch: 1,
                retired_through: 720,
                accepted_through: 720,
            },
            next_epoch: 2,
            observed_at_us: h.base + 330_000,
        })
        .unwrap();
    let mut budget = 32;
    let mut pending = None;
    assert!(service_socket(&mut h, &mut io, &mut pending, &mut budget));
    let received = monotonic_us() - h.base;
    assert!(h.feed_received(430_000, received, 0.99).unwrap().is_none());
    assert_eq!(h.mask.suppressed_blocks(), 1);
    let mut accepted = None;
    for n in 1..=6 {
        let received = monotonic_us() - h.base;
        accepted = h
            .feed_received(430_000 + n * 10_000, received, 0.99)
            .unwrap()
            .or(accepted);
    }
    let _queue = h.commit(accepted.expect("fresh input after reset tail was lost"));
    assert_ne!(h.old(), old);
    h.owner.input_ended(now(), h.old()).unwrap();
    h.reply.as_mut().unwrap().local_input_ended = true;
    h.reply.as_mut().unwrap().cloud = submitted_request();
    h.reply.as_mut().unwrap().pcm = vec![9; 480].into();
    let mut sequence = 1;
    let mut snapshot = h.owner.snapshot(now()).unwrap();
    dispatch_speaker_chunk(
        &mut h.owner,
        &mut h.reply,
        &mut io.channels.data,
        &mut pending,
        &mut sequence,
        &mut snapshot,
        &mut h.mask,
    )
    .unwrap();
    let sent = peer.data.receive::<SpeakerChunk>().unwrap().unwrap();
    let written = monotonic_us();
    peer.control
        .send(WorkerEvent::Playback {
            owner: sent.permit.owner(),
            turn: sent.permit.owner().turn(),
            sequence: sent.chunk_sequence,
            accepted_frames: 240,
            queued_frames: 240,
            first_sample: PlaybackCursor {
                epoch: 2,
                frame: 480,
            },
            end_sample: PlaybackCursor {
                epoch: 2,
                frame: 720,
            },
            observed_at_us: written,
            queue_observed_at_us: written,
        })
        .unwrap();
    assert!(service_socket(&mut h, &mut io, &mut pending, &mut budget));
    assert_eq!(h.old(), sent.permit.owner());
    assert!(h.reply.as_ref().unwrap().playback.is_some());
    assert!(
        !h.trace
            .iter()
            .any(|e| e["kind"] == "speech_final_sample_retired")
    );
}

#[test]
fn pre_dispatch_input_can_replace_reply_and_late_old_write_cannot_fault_successor() {
    let directory = SessionDirectory::create().unwrap();
    let (channels, mut peer) = pair(&directory, "speaker");
    let mut io = SocketSpeaker {
        channels,
        published: vec![],
    };
    let mut h = Harness::new(TurnTaking::V1Baseline);
    let old = h.old();
    h.dispatch(465_000);
    let mut accepted = None;
    for n in 1..=6 {
        accepted = h
            .feed_received(390_000 + n * 10_000, 475_000 + n, 0.99)
            .unwrap()
            .or(accepted);
    }
    let queue = h.commit(accepted.expect("strictly pre-dispatch onset was incorrectly masked"));
    assert!(matches!(queue.front(), Some(ProviderInput::Start { .. })));
    let successor = h.old();
    assert_ne!(old, successor);
    peer.control
        .send(WorkerEvent::Playback {
            owner: old,
            turn: old.turn(),
            sequence: 1,
            accepted_frames: 240,
            queued_frames: 240,
            first_sample: PlaybackCursor {
                epoch: 1,
                frame: 480,
            },
            end_sample: PlaybackCursor {
                epoch: 1,
                frame: 720,
            },
            observed_at_us: h.base + 467_000,
            queue_observed_at_us: h.base + 467_000,
        })
        .unwrap();
    let mut budget = 32;
    assert!(service_socket(&mut h, &mut io, &mut None, &mut budget));
    assert_eq!(h.old(), successor);
    assert!(h.reply.as_ref().unwrap().playback.is_none());
    assert!(h.admission.owns_stream());
    assert!(!h.trace.iter().any(|e| e["kind"] == "speaker_first_write"));
}

#[test]
fn valid_control_receipts_and_short_read_gap_never_forward_masked_preroll() {
    use crate::wire::{Envelope, ReceiveOrder, decode, encode};
    let mut h = Harness::new(TurnTaking::V1Baseline);
    h.dispatch(10_000);
    for n in 2..=11 {
        assert!(h.feed(n * 10_000, 0.0).unwrap().is_none());
    }
    let worker = new_boot().unwrap();
    let mut order = ReceiveOrder::new(worker);
    let acceptance = Envelope {
        boot: worker,
        sequence: 1,
        sent_at_us: h.base + 12_000,
        payload: write_receipt(&h, 240, 480, 11_000),
    };
    let packet: Envelope<WorkerEvent> = decode(&encode(&acceptance).unwrap()).unwrap();
    order.accept(&packet, h.base + 111_000, 100_000).unwrap();
    h.accepted(11_000, 111_000);
    let drain = Envelope {
        boot: worker,
        sequence: 2,
        sent_at_us: h.base + 21_000,
        payload: WorkerEvent::SpeechDrained {
            owner: h.old(),
            sequence: 1,
            last_sample: PlaybackCursor {
                epoch: 1,
                frame: 720,
            },
            observed_at_us: h.base + 20_000,
        },
    };
    let packet: Envelope<WorkerEvent> = decode(&encode(&drain).unwrap()).unwrap();
    order.accept(&packet, h.base + 111_000, 100_000).unwrap();
    h.mask
        .drained(
            h.old(),
            1,
            PlaybackCursor {
                epoch: 1,
                frame: 720,
            },
            h.base + 20_000,
            h.base + 111_000,
        )
        .unwrap();
    // The next actual read starts after the tail. Its 21 ms completion gap is
    // within the existing 50 ms continuity rule; no PCM sequence was dropped.
    let mut accepted = None;
    for n in 0..6 {
        accepted = h.feed(131_000 + n * 10_000, 0.99).unwrap().or(accepted);
    }
    let accepted = accepted.expect("six fresh onset blocks must remain usable");
    println!(
        "acceptance_age_us=99000 drain_age_us=90000 tail_end_offset_us=120000 next_first_read_offset_us=121000 capture_completion_gap_us=21000 accepted_prefix_offsets={:?}",
        accepted
            .audio
            .iter()
            .map(|f| f.captured_at_us - h.base)
            .collect::<Vec<_>>()
    );
    assert!(
        accepted
            .audio
            .iter()
            .all(|f| f.captured_at_us >= h.base + 131_000),
        "old masked quiet prefix was sent with a fresh user onset"
    );
    assert_eq!(accepted.audio.len(), 6);
    assert_eq!(accepted.candidate.first_read_at_us, h.base + 20_000);
    assert_eq!(accepted.candidate.trigger_read_at_us, h.base + 181_000);
    assert_eq!(accepted.candidate.decision_deadline_us, h.base + 381_000);
    assert_eq!(
        h.trace
            .iter()
            .find(|e| e["kind"] == "input_prefix_sanitized")
            .unwrap()["removed_blocks"],
        10
    );
    let queued = h.commit(accepted);
    assert!(matches!(queued.front(), Some(ProviderInput::Start { .. })));
    let fresh_samples: Vec<_> = queued
        .iter()
        .filter_map(|input| match input {
            ProviderInput::Audio { samples, .. } => Some(samples[0]),
            _ => None,
        })
        .collect();
    assert_eq!(fresh_samples, vec![11, 12, 13, 14, 15, 16]);
}

/// The reviewer's P3 on #1277: with `words`, a lone "Stop." said before the
/// answer begins is let go and never asked of the model, and asks the loop
/// to stop the answer it waited for, as a stop said over the lamp's speech
/// does. "Stop, what's the weather?" asks something more and takes the
/// turn from its start, as before. With `on` alone nothing is stopped.
#[test]
fn with_words_a_lone_stop_while_an_answer_waits_stops_it_and_is_never_asked() {
    let stop = with_words(40, true, Some(("Stop.", 10)));
    assert!(stop.accepted.is_none(), "a lone stop is never a request");
    assert_eq!(stop.let_go, ["stop_said"]);
    assert!(stop.stopped);
    assert_eq!(stop.told.len(), 1);
    assert!(stop.told[0].stop && !stop.told[0].took);
    assert_eq!(stop.told[0].words, Some(1));
    let more = with_words(40, true, Some(("Stop, what's the weather?", 10)));
    assert!(more.accepted.is_some(), "a stop with more is a request");
    assert!(!more.stopped);
    assert!(more.told[0].took && !more.told[0].stop);
    let on = with_words(40, false, None);
    assert!(on.accepted.is_none() && !on.stopped);
}

/// The reviewer's P2 on #1352: "Wait, … what about tomorrow?" with a
/// comma's pause. Its words are asked for at the pause and come back
/// "Wait.", but the person has gone on by then, so it is no stop: it takes
/// the turn from its start, as any word does, and nothing is stopped. The
/// reviewer's probe: 0.3 s of speech, 150 ms of quiet, 0.6 s more, "Wait."
/// back 150 ms after the ask. A lone "Wait." with nothing after it still
/// stops.
#[test]
fn with_words_a_stop_the_person_went_on_from_takes_the_turn_and_a_lone_one_stops() {
    let went_on = with_words_in(
        &[(30, 0.99), (15, 0.0), (60, 0.99)],
        true,
        Some(("Wait.", 15)),
        None,
    );
    assert!(!went_on.stopped, "the answer the person wanted was stopped");
    let accepted = went_on.accepted.expect("the request goes on as one line");
    assert!(accepted.audio.first().unwrap().captured_at_us <= went_on.began);
    assert!(went_on.let_go.is_empty(), "{:?}", went_on.let_go);
    assert!(went_on.told[0].took && !went_on.told[0].stop);
    let lone = with_words(30, true, Some(("Wait.", 15)));
    assert!(lone.stopped);
    assert_eq!(lone.let_go, ["stop_said"]);
    assert!(lone.accepted.is_none());
}

/// The seventh's P2 on #1352: a "Stop." that ends less than 300 ms before
/// the answer begins to sound. Once it sounds, `on` decides a waited sound
/// that hasn't been quiet for 300 ms at once, before its words, and so the
/// stop was taken as a request. Its words were asked for and it has been
/// quiet since, so now they decide it: it waits for them, stops the answer
/// and is never asked of the model. The seventh's probe: a 0.4 s "Stop.",
/// its words asked for 100 ms after it ends and back 250 ms later, the
/// answer sounding 120, 150 or 200 ms after it ends. "No." in the same
/// place is taken, as before.
#[test]
fn with_words_a_stop_just_before_the_answer_sounds_still_stops_it() {
    for answer_at in [52, 55, 60] {
        let stop = with_words_in(
            &[(40, 0.99)],
            true,
            Some(("Stop.", 25)),
            Some((answer_at, false)),
        );
        assert!(stop.accepted.is_none(), "answer at {answer_at}: taken");
        assert!(stop.stopped, "answer at {answer_at}");
        assert_eq!(stop.let_go, ["stop_said"], "answer at {answer_at}");
    }
    let no = with_words_in(&[(40, 0.99)], true, Some(("No.", 25)), Some((55, false)));
    assert!(no.accepted.is_some());
    assert!(!no.stopped);
}

/// The reviewer's P1 on #1352: a block with no echo evidence is kept only
/// among the mask's last reads (80, and 0.6 s), so `on` decides a sound
/// heard without it (`!heard`) and it must not be held for its words: held,
/// its first reads fall out and the loop's decision ends the conversation
/// ("candidate original read interval unavailable"). The reviewer's probe: a
/// 0.4 s "No.", the answer sounding at block 55 with no echo evidence from
/// then on, its words back 400 ms after the ask, or never. It goes as `on`
/// decided, as before the decided path: nothing is held, taken or stopped,
/// and the answer plays. So does a "Stop." there whose words come back
/// after the answer sounds: it isn't held for them either, and the answer
/// is not stopped, as before the decided path (a limitation, said in
/// `group-talk.md`).
#[test]
fn with_words_a_sound_decided_without_echo_evidence_is_never_held_for_its_words() {
    for said in [Some(("No.", 40)), None] {
        let no = with_words_in(&[(40, 0.99)], true, said, Some((55, true)));
        assert!(no.accepted.is_none(), "{said:?}");
        assert!(!no.stopped, "{said:?}");
    }
    let stop = with_words_in(&[(40, 0.99)], true, Some(("Stop.", 25)), Some((55, true)));
    assert!(!stop.stopped, "{:?}", stop.told);
    assert!(stop.accepted.is_none() && stop.let_go.is_empty() && stop.told.is_empty());
}

/// `LAMP_OWN_LINE_KEEPS` (#2220): while a line the lamp started on its own
/// (a skill's say, news it tells, its hello to someone arriving or a line of
/// the setup walk) hasn't sounded, a new sound waits. The lamp's name heard
/// as it began takes the turn; otherwise it's let go once the line sounds,
/// or at its own deadline. A sound begun once the line sounds is judged as
/// before (a cut-in), and so is anything with the switch off, for an
/// answer to someone, or in a block heard without echo evidence.
#[test]
fn a_line_the_lamp_starts_waits_out_the_rooms_sound_until_it_sounds() {
    let mut h = Harness::new(TurnTaking::Duplex);
    let trigger = h.base + 300_000;
    let candidate = crate::admission::Candidate {
        id: crate::admission::CandidateId::for_test(1),
        capture: h.lineage,
        displaced_owner: None,
        trigger_sequence: 6,
        trigger_read_at_us: trigger,
        first_read_at_us: trigger - 60_000,
        decision_deadline_us: trigger + 1_200_000,
    };
    let (ticket, _answered) = crate::say::ticket_for_test("welcome-back");
    {
        let reply = h.reply.as_mut().unwrap();
        reply.input_sequence = 0;
        reply.say = Some(ticket);
        assert!(!reply.had_audio && reply.playback.is_none());
    }
    let wait = |h: &Harness, waited, name, at| {
        let wait = OwnLineWait {
            on: true,
            waited,
            heard: true,
            name_began_us: name,
            ..OwnLineWait::default()
        };
        while_own_line_waits(wait, h.reply.as_ref(), &candidate, at)
    };
    assert_eq!(wait(&h, false, None, trigger), WhileOwnLineWaits::Wait);
    // A block the mask heard without echo evidence can't be held whole:
    // decided as before (the reviewer on #2242, P3 1).
    let heard = OwnLineWait {
        on: true,
        heard: true,
        ..OwnLineWait::default()
    };
    let unheard = OwnLineWait {
        heard: false,
        ..heard
    };
    assert_eq!(
        while_own_line_waits(unheard, h.reply.as_ref(), &candidate, trigger),
        WhileOwnLineWaits::AsBefore
    );
    assert_eq!(
        wait(&h, true, None, trigger + 500_000),
        WhileOwnLineWaits::Wait
    );
    assert_eq!(
        wait(&h, true, None, trigger + 1_100_000),
        WhileOwnLineWaits::LetGo("deadline")
    );
    // The lamp's name, heard as it began, takes the turn; an older one is
    // no call.
    let call = crate::activity::CALL_BEFORE_US;
    assert_eq!(
        wait(&h, false, Some(trigger - call), trigger),
        WhileOwnLineWaits::Decide
    );
    assert_eq!(
        wait(&h, false, Some(trigger - call - 1), trigger),
        WhileOwnLineWaits::Wait
    );
    // The line's own person (#2287's follow-up): a face seen speaking
    // through the sound, turned to the lamp, decides it at once; the room's
    // sound with no such face waits. The name still comes first.
    let theirs = OwnLineWait {
        theirs: true,
        ..heard
    };
    assert_eq!(
        while_own_line_waits(theirs, h.reply.as_ref(), &candidate, trigger),
        WhileOwnLineWaits::Theirs
    );
    assert_eq!(
        while_own_line_waits(
            OwnLineWait {
                name_began_us: Some(trigger),
                ..theirs
            },
            h.reply.as_ref(),
            &candidate,
            trigger
        ),
        WhileOwnLineWaits::Decide
    );
    assert_eq!(
        while_own_line_waits(heard, h.reply.as_ref(), &candidate, trigger),
        WhileOwnLineWaits::Wait
    );
    // The line sounds: one that waited is let go, one begun now is a cut-in
    // as before.
    h.reply.as_mut().unwrap().had_audio = true;
    assert_eq!(
        wait(&h, true, None, trigger + 600_000),
        WhileOwnLineWaits::LetGo("sounded")
    );
    assert_eq!(
        wait(&h, false, None, trigger + 600_000),
        WhileOwnLineWaits::AsBefore
    );
    // Off, or an answer to someone's words: as before.
    h.reply.as_mut().unwrap().had_audio = false;
    assert_eq!(
        while_own_line_waits(
            OwnLineWait::default(),
            h.reply.as_ref(),
            &candidate,
            trigger
        ),
        WhileOwnLineWaits::AsBefore
    );
    h.reply.as_mut().unwrap().input_sequence = 1;
    assert_eq!(wait(&h, false, None, trigger), WhileOwnLineWaits::AsBefore);
    // No line at all: as before.
    assert_eq!(
        while_own_line_waits(heard, None, &candidate, trigger),
        WhileOwnLineWaits::AsBefore
    );
    // A written turn with no ticket: the lamp's own only as the turn it
    // began itself (its hello to someone arriving, a line of the setup
    // walk) or with news it tells. Another written turn, a look's, is as
    // before.
    {
        let reply = h.reply.as_mut().unwrap();
        reply.input_sequence = 0;
        reply.say = None;
    }
    let turn = h.reply.as_ref().unwrap().owner.turn();
    let began = |h: &Harness, lamp_began| {
        let wait = OwnLineWait {
            lamp_began,
            ..heard
        };
        while_own_line_waits(wait, h.reply.as_ref(), &candidate, trigger)
    };
    assert_eq!(began(&h, None), WhileOwnLineWaits::AsBefore);
    assert_eq!(began(&h, Some(turn + 1)), WhileOwnLineWaits::AsBefore);
    assert_eq!(began(&h, Some(turn)), WhileOwnLineWaits::Wait);
    h.reply.as_mut().unwrap().news = vec!["task-1".to_owned()];
    assert_eq!(began(&h, None), WhileOwnLineWaits::Wait);
    // News on an answer to someone's words is no line of the lamp's own.
    h.reply.as_mut().unwrap().input_sequence = 1;
    assert_eq!(began(&h, Some(turn)), WhileOwnLineWaits::AsBefore);
}

/// `LAMP_OWN_LINE_OVER_TALK` (#2602's design): while a line the lamp
/// started on its own is sounding, talk over it in a room busy as the line
/// began is let go (`busy_room`), unless the lamp's name, a stop word or a
/// face turned to the lamp is in it. A room that wasn't busy (or the switch
/// off: `busy` is false either way), an answer to a person, and a line that
/// hasn't sounded yet are as before.
#[test]
fn talk_over_the_lamps_own_line_in_a_busy_room_is_let_go() {
    let mut h = Harness::new(TurnTaking::Duplex);
    let trigger = h.base + 300_000;
    let candidate = crate::admission::Candidate {
        id: crate::admission::CandidateId::for_test(1),
        capture: h.lineage,
        displaced_owner: None,
        trigger_sequence: 6,
        trigger_read_at_us: trigger,
        first_read_at_us: trigger - 60_000,
        decision_deadline_us: trigger + 1_200_000,
    };
    let (ticket, _answered) = crate::say::ticket_for_test("stretch");
    {
        let reply = h.reply.as_mut().unwrap();
        reply.input_sequence = 0;
        reply.say = Some(ticket);
        reply.had_audio = true;
        // One block accepted by the speaker: the line has sounded.
        reply.handed = 240;
        reply.sounded = true;
    }
    let busy = OwnLineWait {
        heard: true,
        busy: true,
        ..OwnLineWait::default()
    };
    let in_it = Some(trigger - crate::activity::CALL_BEFORE_US);
    let table = [
        (
            "busy, no name, no face",
            busy,
            WhileOwnLineWaits::LetGo("busy_room"),
        ),
        (
            "with LAMP_OWN_LINE_KEEPS too",
            OwnLineWait { on: true, ..busy },
            WhileOwnLineWaits::LetGo("busy_room"),
        ),
        (
            "the lamp's name",
            OwnLineWait {
                name_began_us: in_it,
                ..busy
            },
            WhileOwnLineWaits::AsBefore,
        ),
        (
            "a name heard before it",
            OwnLineWait {
                name_began_us: Some(trigger - crate::activity::CALL_BEFORE_US - 1),
                ..busy
            },
            WhileOwnLineWaits::LetGo("busy_room"),
        ),
        (
            "a face turned to the lamp",
            OwnLineWait {
                theirs: true,
                ..busy
            },
            WhileOwnLineWaits::AsBefore,
        ),
        (
            "a stop word",
            OwnLineWait {
                stop_began_us: in_it,
                ..busy
            },
            WhileOwnLineWaits::AsBefore,
        ),
        (
            "a room not busy, or the switch off",
            OwnLineWait {
                busy: false,
                ..busy
            },
            WhileOwnLineWaits::AsBefore,
        ),
        (
            "heard without echo evidence",
            OwnLineWait {
                heard: false,
                ..busy
            },
            WhileOwnLineWaits::AsBefore,
        ),
    ];
    // Judged once the name could have been said (see the test after this).
    let past = trigger + OWN_LINE_NAME_LAG_US;
    for (case, wait, then) in table {
        assert_eq!(
            while_own_line_waits(wait, h.reply.as_ref(), &candidate, past),
            then,
            "{case}"
        );
    }
    // Provider audio queued short of a speaker block (an unfinished
    // 240-frame tail: no playback, nothing handed) is no sounding line yet:
    // talk over it is admitted as before (reviewer-two's P2 on #2920).
    {
        let reply = h.reply.as_mut().unwrap();
        reply.handed = 0;
        reply.sounded = false;
        reply.generation_done = false;
        reply.pcm = vec![1_i16; 120].into();
        assert!(!reply.has_speaker_chunk());
        assert!(reply.playback.is_none());
    }
    assert_eq!(
        while_own_line_waits(busy, h.reply.as_ref(), &candidate, trigger),
        WhileOwnLineWaits::AsBefore
    );
    // Its first block handed to the speaker and not yet accepted (no
    // playback): not sounding yet either (reviewer-two's second P2).
    h.reply.as_mut().unwrap().handed = 240;
    assert_eq!(
        while_own_line_waits(busy, h.reply.as_ref(), &candidate, trigger),
        WhileOwnLineWaits::AsBefore
    );
    // A line that sounded and then paused (for a yield: playback ended,
    // its rest queued) is still sounding for the room.
    h.reply.as_mut().unwrap().sounded = true;
    assert_eq!(
        while_own_line_waits(busy, h.reply.as_ref(), &candidate, past),
        WhileOwnLineWaits::LetGo("busy_room")
    );
    // Not yet sounding: only `LAMP_OWN_LINE_KEEPS` waits, as before.
    h.reply.as_mut().unwrap().had_audio = false;
    assert_eq!(
        while_own_line_waits(busy, h.reply.as_ref(), &candidate, trigger),
        WhileOwnLineWaits::AsBefore
    );
    assert_eq!(
        while_own_line_waits(
            OwnLineWait { on: true, ..busy },
            h.reply.as_ref(),
            &candidate,
            trigger
        ),
        WhileOwnLineWaits::Wait
    );
    // An answer to a person's words keeps today's rules.
    {
        let reply = h.reply.as_mut().unwrap();
        reply.had_audio = true;
        reply.input_sequence = 1;
    }
    assert_eq!(
        while_own_line_waits(busy, h.reply.as_ref(), &candidate, trigger),
        WhileOwnLineWaits::AsBefore
    );
}

/// lamp-52e6 on 22 Oct (`LAMP_OWN_LINE_OVER_TALK=on`): "Lamp, wait." said
/// over a skill's say in a busy room. `lamp-name` says a name once it is
/// over (there 482 ms after it began), and the talk had been let go as
/// `busy_room` on its first blocks: the say went on `spoken`. Now talk with
/// no name yet stays pending (`ForName`), the line playing on, until
/// `OWN_LINE_NAME_LAG_US` after its trigger: a name said late in it still
/// takes it as before, and with none it is let go then. Before its decision
/// deadline in any case.
#[test]
fn a_name_said_late_in_talk_over_the_lamps_own_line_in_a_busy_room_still_stops_it() {
    let mut h = Harness::new(TurnTaking::Duplex);
    let trigger = h.base + 300_000;
    let first_read = trigger - 60_000;
    let candidate = crate::admission::Candidate {
        id: crate::admission::CandidateId::for_test(1),
        capture: h.lineage,
        displaced_owner: None,
        trigger_sequence: 6,
        trigger_read_at_us: trigger,
        first_read_at_us: first_read,
        decision_deadline_us: trigger + 1_200_000,
    };
    let (ticket, _answered) = crate::say::ticket_for_test("stretch");
    {
        let reply = h.reply.as_mut().unwrap();
        reply.input_sequence = 0;
        reply.say = Some(ticket);
        reply.had_audio = true;
        reply.handed = 240;
        reply.sounded = true;
    }
    let busy = OwnLineWait {
        heard: true,
        busy: true,
        ..OwnLineWait::default()
    };
    let judge = |wait, at_us| while_own_line_waits(wait, h.reply.as_ref(), &candidate, at_us);
    // Its first blocks: no name yet, and none could have been said.
    assert_eq!(judge(busy, trigger), WhileOwnLineWaits::ForName);
    assert_eq!(
        judge(busy, first_read + 500_000),
        WhileOwnLineWaits::ForName
    );
    // The name began with the talk and is said 600 ms after: as before.
    let named = OwnLineWait {
        name_began_us: Some(first_read),
        ..busy
    };
    assert_eq!(
        judge(named, first_read + 600_000),
        WhileOwnLineWaits::AsBefore
    );
    // A stop word said late, the same.
    let stopped = OwnLineWait {
        stop_began_us: Some(first_read),
        ..busy
    };
    assert_eq!(
        judge(stopped, first_read + 600_000),
        WhileOwnLineWaits::AsBefore
    );
    // With `LAMP_OWN_LINE_KEEPS` too.
    assert_eq!(
        judge(OwnLineWait { on: true, ..busy }, trigger),
        WhileOwnLineWaits::ForName
    );
    // No name by then: the room's.
    assert_eq!(
        judge(busy, trigger + OWN_LINE_NAME_LAG_US),
        WhileOwnLineWaits::LetGo("busy_room")
    );
    // A short hold's deadline comes first: let go before it.
    let short = crate::admission::Candidate {
        decision_deadline_us: trigger + 200_000,
        ..candidate
    };
    assert_eq!(
        while_own_line_waits(busy, h.reply.as_ref(), &short, trigger + 100_000),
        WhileOwnLineWaits::LetGo("busy_room")
    );
    // A room not busy, or the switch off: as before, at once.
    assert_eq!(
        judge(
            OwnLineWait {
                busy: false,
                ..busy
            },
            trigger
        ),
        WhileOwnLineWaits::AsBefore
    );
}

/// #2602 (lamp-06c1, 22:21:49 on 21 Oct, `LAMP_OWN_LINE_KEEPS=on`): a
/// sound let go at its deadline while the lamp's own hello waited stays
/// that sound while it goes on. The played room talk was let go at +2.144 s,
/// the hello's first audio came at +2.291 s, and the same talk rose again as
/// a new candidate at +2.343 s: it is let go too, and the hello is spoken.
/// A new sound after a gap of the wait's quiet (300 ms), speech that starts
/// after the first audio with nothing let go before it, or the lamp's name
/// in it, are a cut-in as before; and with the switch off nothing changes.
#[test]
fn a_sound_let_go_at_its_deadline_stays_that_sound_past_the_own_lines_first_audio() {
    let mut h = Harness::new(TurnTaking::Duplex);
    let (ticket, _answered) = crate::say::ticket_for_test("welcome-back");
    {
        let reply = h.reply.as_mut().unwrap();
        reply.input_sequence = 0;
        reply.say = Some(ticket);
    }
    let turn = h.reply.as_ref().unwrap().owner.turn();
    // The hello begins at `h.base`; the room's sound waited from +1.041 s.
    let at = |ms: u64| h.base + ms * 1_000;
    let first = crate::admission::Candidate {
        id: crate::admission::CandidateId::for_test(1),
        capture: h.lineage,
        displaced_owner: None,
        trigger_sequence: 6,
        trigger_read_at_us: at(1_041),
        first_read_at_us: at(981),
        decision_deadline_us: at(2_244),
    };
    let on = OwnLineWait {
        on: true,
        heard: true,
        ..OwnLineWait::default()
    };
    let waited = OwnLineWait { waited: true, ..on };
    assert_eq!(
        while_own_line_waits(on, h.reply.as_ref(), &first, at(1_041)),
        WhileOwnLineWaits::Wait
    );
    assert_eq!(
        while_own_line_waits(waited, h.reply.as_ref(), &first, at(2_144)),
        WhileOwnLineWaits::LetGo("deadline")
    );
    // Let go at +2.144 s, its last speech in the block before.
    let let_go = LetGoSound::new(turn, at(2_140));
    // The talk goes on block by block through the hello's first audio.
    let mut going_on = Some(let_go);
    for ms in (2_150..=2_343).step_by(10) {
        going_on = going_on.and_then(|sound| sound.heard(at(ms), true));
    }
    h.reply.as_mut().unwrap().had_audio = true;
    let again = crate::admission::Candidate {
        id: crate::admission::CandidateId::for_test(2),
        trigger_sequence: 30,
        trigger_read_at_us: at(2_343),
        first_read_at_us: at(2_283),
        decision_deadline_us: at(3_543),
        ..first
    };
    let goes_on = |sound: Option<LetGoSound>, candidate: &crate::admission::Candidate| {
        sound.is_some_and(|sound| sound.goes_on(turn, candidate))
    };
    assert!(goes_on(going_on, &again));
    let carried = OwnLineWait {
        let_go_goes_on: goes_on(going_on, &again),
        ..on
    };
    assert_eq!(
        while_own_line_waits(carried, h.reply.as_ref(), &again, at(2_343)),
        WhileOwnLineWaits::LetGo("goes_on")
    );
    // The same sound under another turn is no longer the line's.
    assert!(!going_on.unwrap().goes_on(turn + 1, &again));
    // The lamp's name in it is a call, judged as before.
    assert_eq!(
        while_own_line_waits(
            OwnLineWait {
                name_began_us: Some(at(2_300)),
                ..carried
            },
            h.reply.as_ref(),
            &again,
            at(2_343)
        ),
        WhileOwnLineWaits::AsBefore
    );
    // A new sound after a gap: the talk stopped at +2.140 s, and 300 ms of
    // quiet (the wait's `WAIT_QUIET_BLOCKS`) ends it. One that rises at
    // +2.5 s is a cut-in as before.
    let mut gapped = Some(let_go);
    let mut ms = 2_150;
    while ms < 2_440 {
        gapped = gapped.and_then(|sound| sound.heard(at(ms), false));
        ms += 10;
    }
    assert!(gapped.is_some(), "29 quiet blocks are still that sound");
    gapped = gapped.and_then(|sound| sound.heard(at(2_440), false));
    assert!(gapped.is_none(), "the 30th quiet block ends it");
    let after_gap = crate::admission::Candidate {
        trigger_read_at_us: at(2_560),
        first_read_at_us: at(2_500),
        ..again
    };
    assert!(!goes_on(gapped, &after_gap));
    assert_eq!(
        while_own_line_waits(on, h.reply.as_ref(), &after_gap, at(2_560)),
        WhileOwnLineWaits::AsBefore
    );
    // A sound that begins later than 300 ms after the one let go stopped is
    // no continuation even if the gap was never seen as quiet.
    assert!(!let_go.goes_on(turn, &after_gap));
    // Speech that starts after the first audio, nothing let go before it:
    // a cut-in as before.
    assert!(!goes_on(None, &again));
    assert_eq!(
        while_own_line_waits(on, h.reply.as_ref(), &again, at(2_343)),
        WhileOwnLineWaits::AsBefore
    );
    // With the switch off: as before, whatever was let go.
    assert_eq!(
        while_own_line_waits(
            OwnLineWait {
                on: false,
                ..carried
            },
            h.reply.as_ref(),
            &again,
            at(2_343)
        ),
        WhileOwnLineWaits::AsBefore
    );
}

/// #2602 (El on lamp-06c1, 08:17–09:08 on 22 Oct, build 5378 with #2705,
/// `LAMP_OWN_LINE_KEEPS=on`): room noise still cut 3 of 3 hellos. In two, the
/// waiting sound was let go as `sounded` at the hello's first audio, and the
/// same played talk came back as a new candidate 56 ms and 67 ms after
/// `provider_first_audio`, ending the hello as a cut-in. However the wait
/// ends while a sound is held (its deadline, the line sounding, or a sound
/// going on from one let go), the held sound stays that sound while it goes
/// on with no 300 ms gap. The lamp's name, a face turned to the lamp, a new
/// sound after a gap, and speech after the first audio with nothing held
/// are a cut-in as before, and with the switch off nothing changes.
#[test]
fn a_sound_held_until_the_own_line_sounds_stays_that_sound_past_its_first_audio() {
    let mut h = Harness::new(TurnTaking::Duplex);
    let (ticket, _answered) = crate::say::ticket_for_test("welcome-back");
    {
        let reply = h.reply.as_mut().unwrap();
        reply.input_sequence = 0;
        reply.say = Some(ticket);
    }
    let turn = h.reply.as_ref().unwrap().owner.turn();
    let at = |ms: u64| h.base + ms * 1_000;
    // The hello's first audio at +1.600 s; the room's talk is held from
    // +1.041 s, its deadline (+2.800 s) still ahead when the hello sounds.
    let first_audio = 1_600;
    let held = crate::admission::Candidate {
        id: crate::admission::CandidateId::for_test(1),
        capture: h.lineage,
        displaced_owner: None,
        trigger_sequence: 6,
        trigger_read_at_us: at(1_041),
        first_read_at_us: at(981),
        decision_deadline_us: at(2_800),
    };
    let on = OwnLineWait {
        on: true,
        heard: true,
        ..OwnLineWait::default()
    };
    let waited = OwnLineWait { waited: true, ..on };
    assert_eq!(
        while_own_line_waits(on, h.reply.as_ref(), &held, at(1_041)),
        WhileOwnLineWaits::Wait
    );
    assert_eq!(
        while_own_line_waits(waited, h.reply.as_ref(), &held, at(first_audio - 10)),
        WhileOwnLineWaits::Wait
    );
    h.reply.as_mut().unwrap().had_audio = true;
    let sounded = while_own_line_waits(waited, h.reply.as_ref(), &held, at(first_audio));
    assert_eq!(sounded, WhileOwnLineWaits::LetGo("sounded"));
    // Each way the wait ends while a sound is held keeps that sound; no
    // other outcome does.
    let kept = LetGoSound::let_go(sounded, turn, at(first_audio));
    assert_eq!(kept, Some(LetGoSound::new(turn, at(first_audio))));
    for why in ["deadline", "sounded", "goes_on"] {
        assert_eq!(
            LetGoSound::let_go(WhileOwnLineWaits::LetGo(why), turn, at(first_audio)),
            Some(LetGoSound::new(turn, at(first_audio))),
            "{why}"
        );
    }
    for other in [
        WhileOwnLineWaits::AsBefore,
        WhileOwnLineWaits::Decide,
        WhileOwnLineWaits::Theirs,
        WhileOwnLineWaits::Wait,
        WhileOwnLineWaits::Stop,
    ] {
        assert_eq!(LetGoSound::let_go(other, turn, at(first_audio)), None);
    }
    // The talk goes on block by block, and comes back as a new candidate
    // 56 ms and 67 ms after the first audio (its onset read 60 ms before).
    for back_ms in [56, 67] {
        let mut going_on = kept;
        for ms in (first_audio + 10..=first_audio + back_ms).step_by(10) {
            going_on = going_on.and_then(|sound| sound.heard(at(ms), true));
        }
        let again = crate::admission::Candidate {
            id: crate::admission::CandidateId::for_test(2),
            trigger_sequence: 30,
            trigger_read_at_us: at(first_audio + back_ms),
            first_read_at_us: at(first_audio + back_ms - 60),
            decision_deadline_us: at(first_audio + back_ms + 1_200),
            ..held
        };
        let carried = OwnLineWait {
            let_go_goes_on: going_on.is_some_and(|sound| sound.goes_on(turn, &again)),
            ..on
        };
        assert_eq!(
            while_own_line_waits(carried, h.reply.as_ref(), &again, at(first_audio + back_ms)),
            WhileOwnLineWaits::LetGo("goes_on"),
            "+{back_ms} ms"
        );
        // The lamp's name in it, or a face turned to the lamp speaking
        // through it: a cut-in as before.
        for person in [
            OwnLineWait {
                name_began_us: Some(at(first_audio + back_ms - 40)),
                ..carried
            },
            OwnLineWait {
                theirs: true,
                ..carried
            },
        ] {
            assert_eq!(
                while_own_line_waits(person, h.reply.as_ref(), &again, at(first_audio + back_ms)),
                WhileOwnLineWaits::AsBefore
            );
        }
        // Nothing held: speech after the first audio is a cut-in as before.
        assert_eq!(
            while_own_line_waits(on, h.reply.as_ref(), &again, at(first_audio + back_ms)),
            WhileOwnLineWaits::AsBefore
        );
        // The switch off: as before.
        assert_eq!(
            while_own_line_waits(
                OwnLineWait {
                    on: false,
                    ..carried
                },
                h.reply.as_ref(),
                &again,
                at(first_audio + back_ms)
            ),
            WhileOwnLineWaits::AsBefore
        );
    }
    // A new sound after a gap: 300 ms of quiet after the held sound's last
    // speech ends it, and one that rises after is a cut-in as before.
    let mut gapped = kept;
    for ms in (first_audio + 10..=first_audio + 300).step_by(10) {
        gapped = gapped.and_then(|sound| sound.heard(at(ms), false));
    }
    assert!(gapped.is_none());
    let after_gap = crate::admission::Candidate {
        id: crate::admission::CandidateId::for_test(3),
        trigger_read_at_us: at(first_audio + 420),
        first_read_at_us: at(first_audio + 360),
        decision_deadline_us: at(first_audio + 1_620),
        ..held
    };
    assert!(!kept.unwrap().goes_on(turn, &after_gap));
    assert_eq!(
        while_own_line_waits(on, h.reply.as_ref(), &after_gap, at(first_audio + 420)),
        WhileOwnLineWaits::AsBefore
    );
}

/// #1712's B (the first builder at 06:44, `LAMP_MOTION_KEEPS_ANSWER=on`):
/// while an answer is awaited and has not sounded, a sound that began as
/// the body moved a joint is the lamp's own motors and takes no turn, and
/// one the body hasn't yet said of waits. The lamp's name heard with it, an
/// answer that has sounded, the lamp sounding now, or no answer awaited,
/// each hears it as before, and the body is not asked.
#[test]
fn while_an_answer_waits_a_sound_begun_as_the_body_moves_is_its_own() {
    use crate::body_motion::Judged;
    let h = Harness::new(TurnTaking::Duplex);
    let trigger = h.base + 300_000;
    let candidate = crate::admission::Candidate {
        id: crate::admission::CandidateId::for_test(1),
        capture: h.lineage,
        displaced_owner: None,
        trigger_sequence: 6,
        trigger_read_at_us: trigger,
        first_read_at_us: trigger - 60_000,
        decision_deadline_us: trigger + 200_000,
    };
    let moved = Judged::Moved {
        began: trigger - 100_000,
        ended: trigger + 80_000,
    };
    for judged in [moved, Judged::Waits, Judged::Still] {
        assert_eq!(
            body_heard(|| judged, h.reply.as_ref(), false, None, &candidate),
            judged
        );
    }
    let unasked = || -> Judged { panic!("the body was asked") };
    // The name, heard as the sound began (up to `CALL_BEFORE_US` before).
    let named = Some(trigger - crate::activity::CALL_BEFORE_US);
    assert_eq!(
        body_heard(unasked, h.reply.as_ref(), false, named, &candidate),
        Judged::Still
    );
    // An older name is no call.
    let older = Some(trigger - crate::activity::CALL_BEFORE_US - 1);
    assert_eq!(
        body_heard(|| moved, h.reply.as_ref(), false, older, &candidate),
        moved
    );
    // The lamp sounding now: the duplex hears it.
    assert_eq!(
        body_heard(unasked, h.reply.as_ref(), true, None, &candidate),
        Judged::Still
    );
    // An answer that has sounded, a request still being said, or none.
    let mut sounded = Harness::new(TurnTaking::Duplex);
    sounded.reply.as_mut().unwrap().had_audio = true;
    assert_eq!(
        body_heard(unasked, sounded.reply.as_ref(), false, None, &candidate),
        Judged::Still
    );
    let mut said = Harness::new(TurnTaking::Duplex);
    said.reply.as_mut().unwrap().local_input_ended = false;
    assert_eq!(
        body_heard(unasked, said.reply.as_ref(), false, None, &candidate),
        Judged::Still
    );
    assert_eq!(
        body_heard(unasked, None, false, None, &candidate),
        Judged::Still
    );
}

/// `LAMP_MOTION_VOICE` (the first builder on #1722, 21 Oct 03:27): a sound
/// the body rule found begun as a joint moved is read against the asker of
/// the answer awaited, on its own speech blocks from its first read. The
/// asker going on as the body turns to them (lamp-4ace, 03:11) is kept with
/// `on`; motor sound well under them is let go; with no asker's level, or
/// asker-first off, it is let go as before.
#[test]
fn a_sound_begun_as_the_body_moves_is_the_askers_when_about_as_loud() {
    use crate::motion_voice::{Decision, Mode};
    let h = Harness::new(TurnTaking::Duplex);
    let turn = h.reply.as_ref().unwrap().owner.turn();
    let trigger = h.base + 300_000;
    let candidate = crate::admission::Candidate {
        id: crate::admission::CandidateId::for_test(1),
        capture: h.lineage,
        displaced_owner: None,
        trigger_sequence: 6,
        trigger_read_at_us: trigger,
        first_read_at_us: trigger - 100_000,
        decision_deadline_us: trigger + 200_000,
    };
    let asked = |level: u8| {
        let mut rule = crate::asker_first::AskerFirst::default();
        for _ in 0..50 {
            rule.asking(turn, 150);
        }
        rule.asked(turn, h.base);
        // Its own blocks, every 20 ms from its first read.
        for block in 0..8 {
            rule.heard(candidate.first_read_at_us + block * 20_000, level);
        }
        rule
    };
    let at = trigger + 50_000;
    let voice = asked(145);
    let (decision, levels) =
        motion_voice_heard(Mode::On, Some(&voice), h.reply.as_ref(), &candidate, at);
    assert_eq!(decision, Decision::Kept);
    let levels = levels.unwrap();
    assert_eq!((levels.asker, levels.upper, levels.blocks), (150, 145, 8));
    assert_eq!(
        motion_voice_heard(Mode::Record, Some(&voice), h.reply.as_ref(), &candidate, at).0,
        Decision::LetGo { would_keep: true }
    );
    let motors = asked(30);
    assert_eq!(
        motion_voice_heard(Mode::On, Some(&motors), h.reply.as_ref(), &candidate, at).0,
        Decision::LetGo { would_keep: false }
    );
    // No asker-first rule, or no answer awaited: nothing to read against.
    assert_eq!(
        motion_voice_heard(Mode::On, None, h.reply.as_ref(), &candidate, at),
        (Decision::LetGo { would_keep: false }, None)
    );
    assert_eq!(
        motion_voice_heard(Mode::On, Some(&voice), None, &candidate, at),
        (Decision::LetGo { would_keep: false }, None)
    );
}

/// #2287's follow-up, way 1: with `LAMP_WAIT_KEEPS_ANSWER=words`, a sound
/// waiting on the lamp's own line is judged by its words as the answer's
/// wait judges them. A lone "Stop." stops the line, and so does a lone
/// put-off, "Not now." (told as a stop, taking no turn), in a room not busy;
/// in a busy one a put-off is the room's, as usual between colleagues (the
/// reviewer's P3 on #2322). Other words are the room's: two colleagues
/// talking nearby still wait, told as not taken (the reviewer's P2 on
/// #2308). Nothing said leaves it waiting. An outcome that
/// isn't a wait or a let-go is passed on as it is, and nothing is asked.
#[test]
fn with_words_a_stop_said_before_the_own_line_sounds_stops_it() {
    use crate::activity::ObservedAudio;
    let base = 10_000_000;
    let candidate = crate::admission::Candidate {
        id: crate::admission::CandidateId::for_test(1),
        capture: Harness::new(TurnTaking::Duplex).lineage,
        displaced_owner: None,
        trigger_sequence: 6,
        trigger_read_at_us: base + 60_000,
        first_read_at_us: base,
        decision_deadline_us: base + 2_000_000,
    };
    let audio: Vec<ObservedAudio> = (0..30u64)
        .map(|index| ObservedAudio {
            sequence: index + 1,
            captured_at_us: base + index * 10_000,
            samples: [400; 160],
        })
        .collect();
    let spoke_until = base + 29 * 10_000;
    let quiet = crate::wait_words::ASK_AFTER_QUIET_BLOCKS;
    // (what was said, a busy room, what the line does)
    for (said, busy, then) in [
        ("Stop.", false, WhileOwnLineWaits::Stop),
        ("Stop.", true, WhileOwnLineWaits::Stop),
        ("Not now.", false, WhileOwnLineWaits::Stop),
        // In a busy room a put-off is as usual between colleagues: the
        // room's (the reviewer's P3 on #2322).
        ("Not now.", true, WhileOwnLineWaits::Wait),
        ("No thanks.", true, WhileOwnLineWaits::Wait),
        (
            "Did you send the report to the team?",
            false,
            WhileOwnLineWaits::Wait,
        ),
        ("", false, WhileOwnLineWaits::Wait),
    ] {
        let (mut words, script) = crate::wait_words::tests::scripted();
        words.follow(true, true);
        let wait = WhileOwnLineWaits::Wait;
        let (own, told) = own_line_with_words(
            &mut words,
            wait,
            &candidate,
            &audio,
            false,
            0,
            spoke_until,
            busy,
            base + 300_000,
        );
        assert_eq!(
            (own, told.is_some()),
            (wait, false),
            "{said}: heard while it speaks"
        );
        let (own, _) = own_line_with_words(
            &mut words,
            wait,
            &candidate,
            &audio,
            false,
            quiet,
            spoke_until,
            busy,
            base + 400_000,
        );
        assert_eq!(own, wait, "{said}: asked, its words not back");
        script.lock().unwrap().words = Some(said.into());
        let (own, told) = own_line_with_words(
            &mut words,
            wait,
            &candidate,
            &audio,
            false,
            quiet,
            spoke_until,
            busy,
            base + 500_000,
        );
        assert_eq!(own, then, "{said:?}, busy {busy}");
        if then == WhileOwnLineWaits::Stop {
            let told = told.expect("what its words did is told once");
            assert!(told.stop && !told.took, "{said}: {told:?}");
        } else if !said.is_empty() {
            let told = told.expect("what its words did is told once");
            assert!(!told.took && !told.stop, "{said}: the room's, {told:?}");
        }
    }
    for own in [
        WhileOwnLineWaits::AsBefore,
        WhileOwnLineWaits::Decide,
        WhileOwnLineWaits::Theirs,
    ] {
        let (mut words, script) = crate::wait_words::tests::scripted();
        words.follow(true, true);
        assert_eq!(
            own_line_with_words(
                &mut words,
                own,
                &candidate,
                &audio,
                false,
                quiet,
                spoke_until,
                false,
                base
            ),
            (own, None)
        );
        assert!(
            !script
                .lock()
                .unwrap()
                .told
                .iter()
                .any(|told| told == "finalize"),
            "nothing asked for {own:?}"
        );
    }
}
