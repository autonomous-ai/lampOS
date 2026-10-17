//! Turns of the lamp's own with no provider, and the self-test's speaker
//! check in the service.
//!
//! A sound turn ([`commit_sound_turn`]) is a short sound of the lamp's own,
//! played whole through its speaker path so the echo canceller has it: the
//! head tap's "hm?" (#711).
//!
//! The speaker check (`crate::speaker_check`, #812): requests taken from
//! `selftest/speaker/request` and answered on `selftest/speaker` at once,
//! from the lamp's own last speaking turn; nothing is played for it. Off
//! unless `LAMP_SELFTEST_SPEAKER=on`. It is a check, not the conversation: a
//! request that cannot be read or an answer that cannot be written is said
//! once in the log and switches the check off for the rest of the service;
//! the voice goes on.
use super::*;
use crate::speaker_check::{self, Turns};
use lamp_audio::evidence::EchoEvidence;
use lamp_bus::{Bus, Message, Publisher, Subscriber};
use lamp_msgs::{SpeakerCheck, SpeakerCheckRequest};

/// The switch.
pub(super) const ENVIRONMENT: &str = "LAMP_SELFTEST_SPEAKER";

pub(super) struct SpeakerEcho {
    requests: Subscriber,
    answers: Publisher,
    turns: Turns,
    /// Switched off after a bus failure of its own.
    off: bool,
}

impl SpeakerEcho {
    /// The check's requests and answers, if the switch says `on` and the
    /// bus can be used.
    pub(super) fn from_environment() -> Option<Self> {
        Self::from_switch(
            std::env::var(ENVIRONMENT).ok().as_deref(),
            Bus::from_environment,
        )
    }

    /// As [`Self::from_environment`], with the switch's value given: anything
    /// but `on` is no check, and the bus is not even opened.
    pub(super) fn from_switch(
        switch: Option<&str>,
        bus: impl FnOnce() -> io::Result<Bus>,
    ) -> Option<Self> {
        if switch != Some("on") {
            return None;
        }
        match bus().and_then(|bus| Self::on(&bus)) {
            Ok(check) => Some(check),
            Err(error) => {
                eprintln!("lamp-voice: the self-test's speaker check is not offered: {error}");
                None
            }
        }
    }

    pub(super) fn on(bus: &Bus) -> io::Result<Self> {
        Ok(Self {
            requests: bus.subscribe_typed(
                SpeakerCheckRequest::TOPIC,
                "voice",
                SpeakerCheckRequest::TYPE,
            )?,
            answers: bus.publisher_declared(
                SpeakerCheck::TOPIC,
                &SpeakerCheck::declaration(),
                "voice",
            )?,
            turns: Turns::default(),
            off: false,
        })
    }

    /// Each capture block's evidence, if it has any, and when it was read:
    /// the lamp's speaking turns.
    pub(super) fn observe(&mut self, echo: Option<&EchoEvidence>, read_at_us: u64) {
        if let Some(echo) = echo {
            self.turns.observe(echo, read_at_us);
        }
    }

    /// Answers a waiting request, if any.
    pub(super) fn answer_requests(&mut self, at_us: u64) {
        if self.off {
            return;
        }
        let received = self.requests.recv(Duration::ZERO);
        self.take(received, at_us);
    }

    /// What one look at the requests gave.
    pub(super) fn take(&mut self, received: io::Result<Option<Message>>, at_us: u64) {
        let message = match received {
            Ok(Some(message)) if !message.expired => message,
            Ok(_) => return,
            Err(error) => {
                self.fail("its requests cannot be read", &error);
                return;
            }
        };
        let request = match message.parse::<SpeakerCheckRequest>() {
            Ok(request) => request,
            Err(error) => {
                eprintln!("lamp-voice: a speaker check request that is not one: {error}");
                return;
            }
        };
        if let Err(why) = request.check() {
            self.publish(
                SpeakerCheck {
                    id: request.id,
                    verdict: "not_checked".into(),
                    reason: why.0,
                    rise_db: None,
                    delay_ms: None,
                    cancelled_db: None,
                    cancelled_median_db: None,
                    cancelled_lower_bound: None,
                    turn_age_s: None,
                },
                at_us,
            );
            return;
        }
        let last = self.turns.last();
        let (verdict, reason) = speaker_check::verdict(last);
        self.publish(
            SpeakerCheck {
                id: request.id,
                verdict: verdict.into(),
                reason,
                rise_db: last.map(|m| m.rise_db),
                delay_ms: None,
                cancelled_db: last.map(|m| m.cancelled_db),
                cancelled_median_db: last.map(|m| m.cancelled_median_db),
                cancelled_lower_bound: last.map(|m| m.lower_bound),
                turn_age_s: last.map(|m| (at_us.saturating_sub(m.ended_at_us) / 1_000_000) as u32),
            },
            at_us,
        );
    }

    /// Whether a failure of its own has switched the check off.
    #[cfg(test)]
    pub(super) fn is_off(&self) -> bool {
        self.off
    }

    fn fail(&mut self, what: &str, error: &io::Error) {
        if !self.off {
            eprintln!(
                "lamp-voice: the self-test's speaker check is off for this service: {what}: {error}"
            );
        }
        self.off = true;
    }

    fn publish(&mut self, answer: SpeakerCheck, at_us: u64) {
        if let Err(error) = self.answers.publish_json(at_us, &answer) {
            self.fail("its answer cannot be written", &error);
        }
    }
}

/// Opens a turn that is only a short sound of the lamp's own (`sound`):
/// the head tap's "hm?" (#711). No provider request; ended as soon as it
/// began. Like a written turn (`commit_written_turn`) but nothing is sent
/// to the provider.
#[allow(clippy::too_many_arguments)]
pub(super) fn commit_sound_turn(
    owner: &mut Controller,
    reply: &mut Option<Reply>,
    sound: NoticeDelivery,
    basis: &'static str,
    boundary: &'static str,
    provider_session: u64,
    playout_start: PlayoutStart,
    trace: &mut SessionState,
    mut publish: impl FnMut(Snapshot) -> io::Result<()>,
) -> Result<(Snapshot, TurnOwner)> {
    let turn = owner.admit(now(), AdmittedInput::NewTurn)?;
    let plan = owner.plan_output(now(), turn, OutputKind::Speech, None)?;
    let mut snapshot = owner.snapshot(now())?;
    publish(snapshot)?;
    let admitted_at_us = monotonic_us();
    trace
        .receipts
        .admitted(turn, snapshot.issued_at().as_micros())?;
    record(
        trace,
        json!({"kind":"input_admitted","owner":turn,"turn":turn.turn(),"at_us":admitted_at_us,"authority_issued_at_us":snapshot.issued_at().as_micros(),
            "admission_basis":basis,"provider_session":null,
            "boundary":boundary}),
    )?;
    let current = Reply {
        owner: turn,
        // Complete from the start: no request was or will be made, so no
        // link change can abandon it and no failure notice can follow it.
        cloud: CloudRequest {
            session: provider_session,
            start_submitted: true,
            started: false,
            input_end_submitted: true,
            input_abandoned: false,
            outcome: Some(RequestOutcome::Complete),
            movable: false,
        },
        local_input_ended: true,
        plan,
        playback: None,
        pcm: VecDeque::new(),
        generation_done: true,
        provider_idle: true,
        input_sequence: 0,
        had_audio: false,
        voice_level: 0,
        told_stop: None,
        final_chunk: None,
        playback_gaps: 0,
        notice: sound,
        local_only: None,
        reminder: None,
        say: None,
        news: Vec::new(),
        last_provider_audio_at_us: None,
        start: playout_start.gate(),
        need: StartNeed::default(),
    };
    owner.input_ended(now(), current.owner)?;
    snapshot = owner.snapshot(now())?;
    publish(snapshot)?;
    *reply = Some(current);
    Ok((snapshot, turn))
}
