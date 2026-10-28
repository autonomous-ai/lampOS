//! Explicit live bookkeeping plus replaceable observation storage. Removing all
//! metadata cannot cancel a turn, authorize output or erase a receipt obligation.
use crate::{
    fixture_provider::CueSink,
    receipt_history::ReceiptHistory,
    service_trace::{self, ServiceTrace},
};
use lamp_audio::{duplex::Heard, evidence::EchoEvidence};
use lamp_turn_owner::TurnOwner;
use serde_json::{Value, json};
use std::{
    io,
    ops::{Deref, DerefMut},
    path::Path,
};

pub(crate) const MAX_FINITE_EVENTS: usize = 20_000;
const MAX_BATCH_EVENTS: usize = 256;
/// Coordinator withholding evidence only: no admission/output authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct YieldInterval {
    pub owner: Option<TurnOwner>,
    pub started_at_us: u64,
    pub first_frame_sequence: u64,
    pub first_read_completed_at_us: u64,
    pub last_frame_sequence: u64,
    pub missing_evidence_since: Option<u64>,
}
/// Blocks of hearing written as one event: a second.
const TRAIL_BLOCKS: usize = 100;
/// What duplex hearing made of each capture block while the lamp had
/// something to yield, one character per block. Metadata only: it is how a
/// yield that came late, or never, is explained afterwards.
#[derive(Default)]
struct HearingTrail {
    first_frame_sequence: u64,
    heard: String,
    speech: String,
    explained: String,
    floor: String,
    /// Levels before and after cancellation, a letter a block, and the share
    /// of the cancelled signal the render explains, in tenths.
    capture: String,
    residual: String,
    residual_explained: String,
    /// The render at the matched alignment over the block (10 ms), a letter
    /// a block like the levels; how far it is under its own peak over the
    /// last 150 ms, a letter for every 3 dB; and the alignment's age, in
    /// tens of blocks. #4035: logged for the tail guard; not used in
    /// Hearing::heard until 52e6 trails prove it.
    render: String,
    render_drop: String,
    alignment_age: String,
    /// Loudest block before cancellation since the trail began, in tenths
    /// of a decibel: with the lamp speaking alone, its own voice at its
    /// microphone.
    loudest: Option<i16>,
}
/// A level as one letter: `a` for 0 to -3 dBFS, `b` for -3 to -6, and so on
/// down to `z` for -75 dBFS and under.
fn level_letter(tenths_of_db: i16) -> char {
    char::from(b'a' + (i32::from(-tenths_of_db.min(0)) / 30).min(25) as u8)
}
/// A share or a score in tenths, as one digit.
fn tenth(value: Option<f32>) -> char {
    match value {
        Some(value) if value.is_finite() => {
            char::from(b'0' + (value.clamp(0.0, 0.99) * 10.0) as u8)
        }
        _ => '-',
    }
}
#[derive(Default)]
pub(crate) struct SessionState {
    #[cfg(test)]
    pub(crate) onboarding_test_polls: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
    #[cfg(test)]
    pub(crate) onboarding_test_file: Option<std::path::PathBuf>,
    #[cfg(test)]
    pub(crate) onboarding_test_capture_accepted:
        Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
    /// A failed privacy-step invalidation disables only onboarding proof for
    /// this service invocation; capture authority remains with its own gate.
    pub(crate) onboarding_privacy_disabled: bool,
    /// Recovery needs an acknowledged storage epoch newer than this failure.
    pub(crate) onboarding_storage_failed_epoch: Option<u64>,
    /// The sole onboarding filesystem executor, retained across voice cycles.
    pub(crate) onboarding_storage: Option<crate::onboarding::storage::ServiceStorage>,
    /// Checked once before audio opens, retained across mute/reopen cycles.
    pub(crate) onboarding_definition: Option<std::sync::Arc<crate::onboarding::skill::Definition>>,
    events: Vec<Value>,
    pub(crate) receipts: ReceiptHistory,
    pub(crate) yield_interval: Option<YieldInterval>,
    hearing: HearingTrail,
    continuous: bool,
    writer: Option<ServiceTrace>,
    dropped: u64,
    cue_history_lost: bool,
    sequence: u64,
    /// The voice mic was waited for at the latest capture start (#1178): the
    /// voice's own, or after a mute or a sleep. Said on the voice's health
    /// until the next capture start.
    pub(crate) mic_at_start: Option<crate::wire::MicWaited>,
}
impl SessionState {
    pub(crate) fn finite() -> Self {
        Self {
            events: Vec::with_capacity(MAX_FINITE_EVENTS),
            ..Self::default()
        }
    }
    pub(crate) fn service(output: &Path) -> Self {
        Self::service_writer(ServiceTrace::start(output))
    }
    pub(crate) fn service_with_reader(
        output: &Path,
        reader: io::Result<crate::session_reader::Reader>,
    ) -> Self {
        Self::service_writer(ServiceTrace::start_with_reader(output, reader))
    }
    fn service_writer(writer: io::Result<ServiceTrace>) -> Self {
        if writer.is_err() {
            eprintln!(
                "lamp-voice serve: metadata unavailable at startup; recording health degraded"
            );
        }
        Self {
            events: Vec::with_capacity(MAX_BATCH_EVENTS),
            continuous: true,
            writer: writer.ok(),
            ..Self::default()
        }
    }
    pub(crate) fn continuous(&self) -> bool {
        self.continuous
    }
    pub(crate) fn record(&mut self, mut event: Value) -> io::Result<()> {
        if !self.continuous {
            if self.events.len() >= MAX_FINITE_EVENTS - 1 {
                return Err(io::Error::other("trace capacity reached"));
            }
            self.events.push(event);
            return Ok(());
        }
        let sequence = self.sequence.checked_add(1);
        if let Some(sequence) = sequence {
            self.sequence = sequence;
            event["service_sequence"] = json!(sequence);
        }
        let oversized = !service_trace::event_fits(&event);
        if sequence.is_none() || oversized || self.events.len() >= MAX_BATCH_EVENTS {
            self.dropped = self.dropped.saturating_add(1);
            self.cue_history_lost = true;
            if let Some(writer) = &self.writer {
                writer.lost(oversized);
            }
        } else {
            self.events.push(event);
        }
        Ok(())
    }
    /// One capture block as duplex hearing read it. Kept only in serve mode
    /// and only while the lamp has something to yield; written a second at a
    /// time and when that ends.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn heard(
        &mut self,
        sequence: u64,
        speaking: bool,
        heard: Option<Heard>,
        probability: f32,
        evidence: Option<&EchoEvidence>,
        levels_db: (Option<f32>, Option<f32>),
        at_us: u64,
    ) -> io::Result<()> {
        if !self.continuous {
            return Ok(());
        }
        if speaking {
            if self.hearing.heard.is_empty() {
                self.hearing.first_frame_sequence = sequence;
            }
            let echo = evidence.and_then(|evidence| evidence.echo);
            self.hearing.heard.push(match heard {
                Some(Heard::Still) => '0',
                Some(Heard::Clear) => '1',
                Some(Heard::Under) => '2',
                Some(Heard::Own) => '3',
                Some(Heard::Over) => '4',
                Some(Heard::Unknown) => '5',
                None => '-',
            });
            self.hearing.speech.push(tenth(Some(probability)));
            self.hearing.explained.push(tenth(
                echo.map(|echo| f32::from(echo.capture_explained) / 100.0),
            ));
            self.hearing.floor.push(tenth(
                echo.map(|echo| f32::from(echo.residual_playback_floor) / 100.0),
            ));
            match evidence {
                Some(evidence) => {
                    let level = evidence.capture_level.tenths_of_db();
                    self.hearing.loudest =
                        Some(self.hearing.loudest.map_or(level, |was| was.max(level)));
                    self.hearing.capture.push(level_letter(level));
                    self.hearing
                        .residual
                        .push(level_letter(evidence.residual_level.tenths_of_db()));
                }
                None => {
                    self.hearing.capture.push('-');
                    self.hearing.residual.push('-');
                }
            }
            self.hearing.residual_explained.push(tenth(
                echo.map(|echo| f32::from(echo.residual_explained) / 100.0),
            ));
            self.hearing.render.push(echo.map_or('-', |echo| {
                level_letter(echo.render_block_level.tenths_of_db())
            }));
            self.hearing.render_drop.push(echo.map_or('-', |echo| {
                level_letter(-(echo.render_drop.min(i16::MAX as u16) as i16))
            }));
            self.hearing.alignment_age.push(echo.map_or('-', |echo| {
                char::from(b'0' + (echo.alignment_age_blocks / 10).min(9) as u8)
            }));
        }
        if self.hearing.heard.len() >= TRAIL_BLOCKS || (!speaking && !self.hearing.heard.is_empty())
        {
            let trail = std::mem::take(&mut self.hearing);
            self.record(json!({"kind":"duplex_hearing","at_us":at_us,
                "first_frame_sequence":trail.first_frame_sequence,
                "heard":trail.heard,"speech":trail.speech,
                "explained":trail.explained,"floor":trail.floor,
                "capture_level":trail.capture,"residual_level":trail.residual,
                "residual_explained":trail.residual_explained,
                "render_level":trail.render,"render_drop":trail.render_drop,
                "alignment_age":trail.alignment_age,
                "loudest_capture_db":trail.loudest.map(|tenths| f32::from(tenths) / 10.0),
                "person_db":levels_db.0,"own_db":levels_db.1,
                "boundary":"one character per 10 ms capture block while a reply had something to yield; heard: 0 still, 1 clear, 2 under, 3 own, 4 over, 5 unknown; the rest in tenths; - where not judged; capture_level and residual_level are the level before and after cancellation, a letter for every 3 dB down from full scale (a is 0 to -3 dBFS); render_level is the lamp's render at the matched alignment over the block, lettered the same; render_drop is how far it is under its own peak over the last 150 ms, a letter for every 3 dB (c and on is 6 dB or more); alignment_age is the blocks since that alignment last held, in tens (9 for 90 or more); levels in dBFS before cancellation: the loudest block of this trail, and the slow averages of a person heard in the clear and of the lamp's own voice let through"}))?;
        }
        Ok(())
    }
    /// Metadata only, called after control/capture service. Queue pressure never
    /// becomes a Result error and cannot block an audio/privacy tick.
    pub(crate) fn flush(&mut self, cues: &mut Option<CueSink>, at_us: u64) {
        let error = if let Some(cues) = cues.as_mut() {
            if self.continuous {
                if self.cue_history_lost {
                    cues.invalidate("service_cue_history_lost");
                }
                cues.scan_batch(&self.events, at_us)
            } else {
                cues.scan(&self.events, at_us)
            }
        } else {
            None
        };
        if let Some(error) = error {
            let _ = self.record(json!({"kind":"fixture_cue_invalid","at_us":at_us,"reason":error}));
        }
        if self.continuous {
            if let Some(writer) = self.writer.as_mut() {
                for event in &self.events {
                    writer.submit(event);
                }
            } else {
                self.dropped = self.dropped.saturating_add(self.events.len() as u64);
            }
            self.events.clear();
        }
    }
    #[cfg(test)]
    pub(crate) fn writer_for_test(&self) -> &ServiceTrace {
        self.writer
            .as_ref()
            .expect("test service writer initialized")
    }
    pub(crate) fn finish_logging(&mut self) -> Option<service_trace::Report> {
        if let Some(writer) = self.writer.as_mut() {
            Some(writer.finish(service_trace::FINISH_WAIT))
        } else if self.continuous {
            Some(service_trace::Report {
                acknowledged: false,
                finish_attempted: true,
                degraded: true,
                writer_failed: true,
                initialization_failed: true,
                dropped_events: self.dropped,
                oversized_events: 0,
                written_events: 0,
                unconfirmed_events: 0,
                rotations: 0,
            })
        } else {
            None
        }
    }
}
// The slice view is observation-only. Mutable access is used by the existing
// finite ring summary and final row; neither path runs in serve mode.
impl Deref for SessionState {
    type Target = Vec<Value>;
    fn deref(&self) -> &Self::Target {
        &self.events
    }
}
impl DerefMut for SessionState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.events
    }
}
#[cfg(test)]
impl Clone for SessionState {
    fn clone(&self) -> Self {
        assert!(!self.continuous);
        Self {
            events: self.events.clone(),
            receipts: self.receipts.clone(),
            yield_interval: self.yield_interval,
            ..Self::default()
        }
    }
}
#[cfg(test)]
impl std::fmt::Debug for SessionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionState")
            .field("events", &self.events)
            .field("receipts", &self.receipts)
            .finish()
    }
}
#[cfg(test)]
impl PartialEq for SessionState {
    fn eq(&self, other: &Self) -> bool {
        self.events == other.events
            && self.receipts == other.receipts
            && self.yield_interval == other.yield_interval
    }
}
#[cfg(test)]
impl From<Vec<Value>> for SessionState {
    fn from(events: Vec<Value>) -> Self {
        Self {
            events,
            ..Self::default()
        }
    }
}

#[cfg(test)]
impl SessionState {
    /// How many events its trace writer has written so far (0 with none).
    pub(crate) fn written_events(&self) -> u64 {
        self.writer
            .as_ref()
            .map_or(0, |writer| writer.health().written_events)
    }
    pub(crate) fn with_writer(writer: ServiceTrace) -> Self {
        Self {
            continuous: true,
            writer: Some(writer),
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_audio::evidence::{EchoMatch, Level};

    fn evidence(explained: u8, floor: u8) -> EchoEvidence {
        let level = Level::from_tenths_of_db(-200);
        EchoEvidence {
            blocks: 1_000,
            render_active_blocks: 50,
            render_quiet_blocks: 0,
            capture_level: level,
            residual_level: level,
            echo: Some(EchoMatch {
                render_level: level,
                capture_explained: explained,
                residual_explained: 10,
                residual_playback_floor: floor,
                alignment_age_blocks: 12,
                render_block_level: level,
                render_drop: 70,
            }),
            silent_at_echo: None,
        }
    }

    fn service() -> SessionState {
        SessionState {
            continuous: true,
            ..SessionState::default()
        }
    }

    #[test]
    fn hearing_is_written_a_second_at_a_time_and_when_the_lamp_falls_silent() {
        let mut state = service();
        let own = evidence(90, 85);
        for block in 0..TRAIL_BLOCKS as u64 + 3 {
            state
                .heard(
                    500 + block,
                    true,
                    Some(Heard::Own),
                    0.62,
                    Some(&own),
                    (Some(-31.5), Some(-12.0)),
                    block,
                )
                .unwrap();
        }
        assert_eq!(state.len(), 1);
        assert_eq!(state[0]["kind"], "duplex_hearing");
        assert_eq!(state[0]["first_frame_sequence"], 500);
        assert_eq!(state[0]["heard"], "3".repeat(TRAIL_BLOCKS));
        assert_eq!(state[0]["speech"], "6".repeat(TRAIL_BLOCKS));
        assert_eq!(state[0]["explained"], "9".repeat(TRAIL_BLOCKS));
        assert_eq!(state[0]["floor"], "8".repeat(TRAIL_BLOCKS));
        // -20 dBFS before and after cancellation in this evidence: the
        // seventh letter. A tenth of the cancelled signal explained.
        assert_eq!(state[0]["capture_level"], "g".repeat(TRAIL_BLOCKS));
        assert_eq!(state[0]["residual_level"], "g".repeat(TRAIL_BLOCKS));
        assert_eq!(state[0]["residual_explained"], "1".repeat(TRAIL_BLOCKS));
        // The render at -20 dBFS, 7 dB under its peak (the third letter),
        // and an alignment twelve blocks old (one ten).
        assert_eq!(state[0]["render_level"], "g".repeat(TRAIL_BLOCKS));
        assert_eq!(state[0]["render_drop"], "c".repeat(TRAIL_BLOCKS));
        assert_eq!(state[0]["alignment_age"], "1".repeat(TRAIL_BLOCKS));
        // The levels as they stood when the second was written.
        assert_eq!(state[0]["loudest_capture_db"], -20.0);
        assert_eq!(
            (&state[0]["person_db"], &state[0]["own_db"]),
            (&json!(-31.5), &json!(-12.0))
        );
        // Nothing left to yield: the three blocks still held are written,
        // and the silent block itself is not.
        state
            .heard(603, false, Some(Heard::Still), 0.0, None, (None, None), 700)
            .unwrap();
        assert_eq!(state.len(), 2);
        assert_eq!(state[1]["first_frame_sequence"], 600);
        assert_eq!(state[1]["heard"], "333");
        state
            .heard(604, false, Some(Heard::Still), 0.0, None, (None, None), 701)
            .unwrap();
        assert_eq!(state.len(), 2);
    }

    #[test]
    fn hearing_marks_what_was_not_judged_and_stays_out_of_a_finite_trace() {
        let mut state = service();
        state
            .heard(1, true, None, f32::NAN, None, (None, None), 0)
            .unwrap();
        state
            .heard(
                2,
                true,
                Some(Heard::Over),
                1.0,
                Some(&evidence(40, 0)),
                (None, None),
                1,
            )
            .unwrap();
        state
            .heard(3, false, None, 0.0, None, (None, None), 2)
            .unwrap();
        assert_eq!(state[0]["heard"], "-4");
        assert_eq!(state[0]["speech"], "-9");
        assert_eq!(state[0]["explained"], "-4");
        assert_eq!(state[0]["floor"], "-0");
        assert_eq!(state[0]["capture_level"], "-g");
        assert_eq!(state[0]["residual_explained"], "-1");
        assert_eq!(level_letter(0), 'a');
        assert_eq!(level_letter(-29), 'a');
        assert_eq!(level_letter(-30), 'b');
        assert_eq!(level_letter(-960), 'z');
        assert_eq!(level_letter(15), 'a');
        let mut finite = SessionState::finite();
        finite
            .heard(1, true, Some(Heard::Over), 1.0, None, (None, None), 0)
            .unwrap();
        finite
            .heard(2, false, None, 0.0, None, (None, None), 1)
            .unwrap();
        assert!(finite.is_empty());
    }
}
