//! Bounded recovery of the capture-clocked reference.
//!
//! Without it, any doubt about which reference belongs to the capture in
//! hand is a fault and the session ends. That is right when the doubt cannot
//! be resolved and wasteful when it can. This module tells three cases
//! apart and treats each as what it is.
//!
//! **The capture loop was held up and nothing is missing.** The microphone
//! kept recording into its buffer and the speaker kept sending. Every packet
//! is there, in order, with the speaker's own timestamps a block apart. The
//! reference is paired with the capture by count, so it is exactly as known
//! as before; only its arrival is late. It is used, and counted as late.
//! Nothing is degraded. What bounds it is room: the ring, and how much the
//! microphone may have waiting.
//!
//! **The count can no longer be believed.** A packet is missing, the next
//! one starts somewhere else, the ring ran dry or over, or the speaker's own
//! timestamps show it accepted nothing for longer than its queue can cover,
//! so that what it played and what was counted have parted. The reference is
//! then **untrusted**: the canceller is not run, the capture goes on as the
//! microphone recorded it, no echo evidence is given, and a status says so
//! with the cause. No silence is put in the reference's place and no old
//! reference is passed off as current.
//!
//! **Fresh reference returns.** Once a run of packets that arrived fresh and
//! follow one another without a gap fills the ring to its working level, the
//! canceller is reset onto it: a **reseed**, with a new DSP epoch. The
//! frames the canceller never saw are counted as skipped under their own
//! cursors; no counter is rewound and nothing is renumbered. The canceller
//! starts cold, so its evidence is withheld until it has processed three
//! seconds of the lamp's own sound, as V1 waits three seconds into playback
//! before it listens: **warming**.
//!
//! Recovery is bounded twice: a reference may stay untrusted for a second,
//! and three reseeds are allowed in a minute. Past either the worker ends
//! with a fault that says which. Privacy, a malformed or regressing packet
//! and a capture clock that runs backwards are never recovered from.
//!
//! # The fixed margin
//!
//! The fixed-margin reference gets one thing from this module, and it is
//! the thing that ends its sessions on a lamp. Its margin is a count of
//! render blocks against capture blocks, and the two devices' clocks differ:
//! on one lamp by 157 ppm, which moves the count a block a minute and used
//! the margin up after 378.6 s of an idle conversation. Nothing was late and
//! nothing was lost. With recovery, a margin used up while both sides are
//! alive and on time is **re-centred**: the canceller is reset, given the
//! newest accepted blocks again as its lead, and the count the margin is
//! measured against is moved by exactly what the status says. It warms as
//! after any reseed and spends the same budget. A cold canceller is best
//! had when nobody needs it, so the re-centre is made two blocks before the
//! bound if the room has been still and the lamp silent for half a second,
//! and at the bound regardless. A capture loop that fell
//! behind, a speaker that stopped, a lost packet: in the fixed mode those
//! are still the faults they were. The capture-clocked reference never
//! needs a re-centre, because its count does not drift.
use super::{
    Kind, LEAD_FRAMES, MAX_AHEAD_BLOCKS, MAX_REFERENCE_AGE_US, ReferenceAssembly, Result, fresh,
};
use crate::wire::{
    RecoveryCounts, ReferenceCause as Cause, ReferenceStatus, ReferenceTrust as Trust,
    RenderPayload, RenderReference,
};
use lamp_audio::{
    CAPTURE_RATE, EchoProcessor, RENDER_RATE, RENDER_SAMPLES, RenderBlock, drift::TARGET_FRAMES,
};
use std::collections::VecDeque;

/// The bounds of recovery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Policy {
    /// Fresh, gapless reference that must be waiting before a reseed.
    pub anchor_frames: usize,
    /// The longest the reference may stay untrusted.
    pub untrusted_deadline_us: u64,
    /// Reseeds allowed within `window_us`.
    pub reseeds: usize,
    pub window_us: u64,
    /// Render blocks carrying sound that a reseeded canceller must have
    /// processed before its evidence is given out.
    pub warm_blocks: u32,
    /// The longest the speaker's own timestamps may show it accepting
    /// nothing while the count is still believed: its queue is never deeper.
    pub sender_pause_us: u64,
    /// Microphone frames that may be waiting when a block is read.
    pub capture_backlog_frames: i64,
}

impl Policy {
    /// 50 ms of fresh reference to reseed; untrusted for a second at most;
    /// three reseeds a minute; three seconds of own sound to warm, which is
    /// V1's wait; a sender pause of 40 ms, the speaker's whole buffer; and
    /// 40 ms of microphone waiting, which is all its buffer holds on a lamp
    /// before it overruns.
    pub const BOUNDED: Self = Self {
        anchor_frames: TARGET_FRAMES,
        untrusted_deadline_us: 1_000_000,
        reseeds: 3,
        window_us: 60_000_000,
        warm_blocks: 300,
        sender_pause_us: 40_000,
        capture_backlog_frames: 640,
    };
}

/// Status changes waiting to be sent. A tick makes two at most.
const MAX_EVENTS: usize = 8;
/// Microphone frames that may be waiting when a pairing is established: a
/// block more than the most a lamp has shown with its loop on time.
const FRESH_CAPTURE_FRAMES: i64 = 208;
/// Fixed margin: accepted blocks kept so that a re-centred canceller can be
/// given its lead again.
const HISTORY_BLOCKS: usize = 8;
/// Fixed margin: from this many render blocks ahead a re-centre is made as
/// soon as the room is idle. The bound itself is eight.
const EARLY_AHEAD_BLOCKS: u64 = 6;
/// Fixed margin: render blocks beyond the bound that may be taken in while
/// the capture block that settles it is awaited. Four is all that can arrive
/// inside the 40 ms a reference stays fresh.
const AWAITED_BLOCKS: u64 = 4;
/// How recent the other side's last sign of life must be for a margin at
/// its bound to be the clocks' doing: two blocks. A lamp's capture reads
/// were never more than 12.6 ms apart and its packets never more than
/// 20.9 ms. Anything later is a hold-up, and a hold-up is not drift.
const ON_TIME_US: u64 = 20_000;
/// Capture blocks without speech, and render blocks of silence, that make
/// the room idle: half a second.
const IDLE_BLOCKS: u32 = 50;
/// A detector score below which a capture block is not speech: the
/// endpoint's own low threshold.
const QUIET_SCORE: f32 = 0.35;

/// Which bound the fixed margin reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Side {
    /// More render blocks than the bound: the speaker's clock is the faster.
    Ahead,
    /// No render block for a capture block: the speaker's is the slower.
    Behind,
}

impl Side {
    /// Render blocks the re-centred canceller is given as its lead. The
    /// margin then has the whole range to cross again in the direction it
    /// was going: two of eight when it was rising, six when it was falling.
    fn lead(self) -> usize {
        match self {
            Self::Ahead => 2,
            Self::Behind => 6,
        }
    }
}

pub(super) struct State {
    policy: Policy,
    trust: Trust,
    cause: Option<Cause>,
    untrusted_since_us: u64,
    /// Whether a fresh, gapless run is collecting in the ring.
    run: bool,
    warm_remaining: u32,
    reseeded_at_us: VecDeque<u64>,
    reseed_pending: bool,
    capture_backlog_frames: i64,
    counts: RecoveryCounts,
    events: VecDeque<ReferenceStatus>,
    /// Fixed margin only: the newest accepted blocks, oldest first.
    history: VecDeque<RenderBlock>,
    /// Consecutive capture blocks the detector scored as no speech.
    still_blocks: u32,
    /// Consecutive render blocks of silence.
    silent_blocks: u32,
}

impl State {
    pub(super) fn new(policy: Policy) -> Self {
        Self {
            policy,
            trust: Trust::Trusted,
            cause: None,
            untrusted_since_us: 0,
            run: false,
            warm_remaining: 0,
            reseeded_at_us: VecDeque::with_capacity(policy.reseeds + 1),
            reseed_pending: false,
            capture_backlog_frames: 0,
            counts: RecoveryCounts::default(),
            events: VecDeque::with_capacity(MAX_EVENTS),
            history: VecDeque::with_capacity(HISTORY_BLOCKS),
            still_blocks: 0,
            silent_blocks: 0,
        }
    }
}

impl ReferenceAssembly {
    /// The capture-clocked reference with bounded recovery. Drive it with
    /// [`Self::accept`] and [`Self::capture_step`].
    pub fn recovering(policy: Policy) -> Self {
        Self {
            recovery: Some(Box::new(State::new(policy))),
            ..Self::capture_clocked()
        }
    }

    /// The fixed-margin reference with its margin re-centred when the clocks
    /// use it up. Every other doubt is the fault it was.
    pub fn recentring(policy: Policy) -> Self {
        Self {
            recovery: Some(Box::new(State::new(policy))),
            ..Self::default()
        }
    }

    /// `Trusted` always, without recovery: there a doubt is a fault.
    pub fn trust(&self) -> Trust {
        self.recovery
            .as_ref()
            .map_or(Trust::Trusted, |state| state.trust)
    }

    /// Whether the canceller's evidence may be given out for the block just
    /// processed.
    pub fn evidence_trusted(&self) -> bool {
        self.trust() == Trust::Trusted
    }

    pub fn recovery_counts(&self) -> Option<RecoveryCounts> {
        self.recovery.as_ref().map(|state| RecoveryCounts {
            trust: state.trust,
            ..state.counts
        })
    }

    /// How old a start notice's stamps may be when it is taken. Capture-
    /// clocked with recovery, the reference is paired with the capture by
    /// count through the ring the prime filled, and the start's stamps feed
    /// only the delay hint; a notice the worker took late, held up by load,
    /// pairs as an on-time one would. There it may be as old as the
    /// reference may stay untrusted, and no older. With the fixed margin
    /// the start sets the count's origin, and without recovery any doubt is
    /// a fault: there it is the 40 ms every reference has.
    pub(super) fn start_age_bound_us(&self) -> u64 {
        match (&self.recovery, &self.clock) {
            (Some(state), Some(_)) => state.policy.untrusted_deadline_us,
            _ => MAX_REFERENCE_AGE_US,
        }
    }

    /// A start notice taken older than 40 ms, and used.
    pub(super) fn note_late_start(&mut self, late_us: u64) {
        if let Some(state) = self.recovery.as_mut() {
            state.counts.late_starts = state.counts.late_starts.saturating_add(1);
        }
        eprintln!(
            "lamp-voice: reference: the speaker's clock start was taken {} ms late and is used: \
             the capture-clocked count does not read it",
            late_us / 1_000
        );
    }

    /// The next change of trust to report, oldest first.
    pub fn take_status(&mut self) -> Option<ReferenceStatus> {
        self.recovery.as_mut()?.events.pop_front()
    }

    /// The detector's score for the capture block just processed. Only used
    /// to choose a quiet moment for a re-centre that is coming anyway.
    pub fn note_score(&mut self, probability: f32) {
        if let Some(state) = self.recovery.as_mut() {
            state.still_blocks = if probability.is_finite() && probability < QUIET_SCORE {
                state.still_blocks.saturating_add(1)
            } else {
                0
            };
        }
    }

    /// True once after each reseed: the canceller was reset and whatever
    /// keeps state beside it must start again under a new DSP epoch.
    pub fn take_reseed(&mut self) -> bool {
        self.recovery
            .as_mut()
            .is_some_and(|state| std::mem::take(&mut state.reseed_pending))
    }

    /// One capture block. `Some(delay)` when the canceller has this block's
    /// reference and is to be given the capture with that hint; `None` when
    /// it has not and the capture is to go on uncancelled.
    ///
    /// Without recovery this is `before_capture`, `render_for_capture` and
    /// `captured`, and never `None`.
    pub fn capture_step(
        &mut self,
        capture_delayed_frames: i64,
        read_completed_at_us: u64,
        now: u64,
        echo: &mut EchoProcessor,
    ) -> Result<Option<u16>> {
        // Starting a stream is not recovered: before the clock has started
        // every rule is the strict one. Only a start notice taken late is
        // let through, capture-clocked, by `started` (`start_age_bound_us`).
        if self.recovery.is_none() || self.timing.clock_started_at_us.is_none() {
            let delay = self.before_capture(capture_delayed_frames, now)?;
            self.render_for_capture(now, echo)?;
            self.captured(read_completed_at_us, now)?;
            return Ok(Some(delay));
        }
        if self.clock.is_none() {
            // Fixed margin. A capture block with no render block ahead of it,
            // while the speaker is alive and on time, is the margin walked
            // down to nothing.
            self.state().capture_backlog_frames = capture_delayed_frames;
            let rendered = self.timing.analysed_through / RENDER_SAMPLES as u64;
            let running = self.running_capture_blocks();
            let ahead = rendered.saturating_sub(running);
            let idle = {
                let state = self.state();
                state.still_blocks >= IDLE_BLOCKS && state.silent_blocks >= IDLE_BLOCKS
            };
            // Only this block's own status says whether this loop is on
            // time. A margin at its bound with the loop on time is the
            // clocks. With the loop late it is a backlog, and stays the
            // fault it was.
            let on_time = self.capture_on_time(capture_delayed_frames, now);
            if ahead > MAX_AHEAD_BLOCKS {
                if !on_time {
                    return Err(self.fault(Kind::ReferenceBacklog, now).into());
                }
                self.recentre(Side::Ahead, now, echo)?;
            } else if rendered <= running && fresh(self.timing.accepted_at_us, now, ON_TIME_US) {
                // No render block ahead of this capture block, and a speaker
                // that is on time: the margin walked down to nothing. A
                // speaker that is late is not that, and `before_capture`
                // says so below.
                self.recentre(Side::Behind, now, echo)?;
            } else if ahead >= EARLY_AHEAD_BLOCKS && idle && on_time {
                // Coming within two blocks anyway: better now, in silence.
                self.recentre(Side::Ahead, now, echo)?;
            }
            let delay = self.before_capture(capture_delayed_frames, now)?;
            self.captured(read_completed_at_us, now)?;
            let sounding = self
                .state()
                .history
                .back()
                .is_some_and(|block| block.iter().any(|sample| *sample != 0));
            self.warm(sounding, now)?;
            return Ok(Some(delay));
        }
        // A capture device reporting a negative queue is broken, not late.
        if capture_delayed_frames < 0 {
            return Err(self.fault(Kind::QueueTiming, now).into());
        }
        let policy = self.policy();
        self.state().capture_backlog_frames = capture_delayed_frames;
        // A fresh pairing needs both sides fresh: a run of fresh reference
        // waiting, and a capture loop that is on time. One still working
        // through a backlog reads blocks faster than the speaker supplies
        // them, and a ring filled then would be drained at once.
        // The bound on being untrusted is absolute. Fresh reference that
        // arrives at it does not buy another interval.
        if self.trust() == Trust::Untrusted
            && now.saturating_sub(self.state().untrusted_since_us) >= policy.untrusted_deadline_us
        {
            return Err(self.fault(Kind::RecoveryTimedOut, now).into());
        }
        if self.trust() == Trust::Untrusted
            && self.state().run
            && capture_delayed_frames <= FRESH_CAPTURE_FRAMES
            && self
                .clock
                .as_ref()
                .is_some_and(|clock| clock.resampler.unread() >= policy.anchor_frames)
        {
            self.reseed(now, echo)?;
        }
        if self.trust() != Trust::Untrusted {
            let ready = self
                .clock
                .as_ref()
                .is_some_and(|clock| clock.resampler.ready());
            if capture_delayed_frames > policy.capture_backlog_frames {
                self.degrade(Cause::CaptureBacklog, now, None)?;
            } else if !ready {
                self.degrade(Cause::Absent, now, None)?;
            } else {
                let delay = self.render_counted(capture_delayed_frames, now, echo)?;
                self.captured(read_completed_at_us, now)?;
                return Ok(Some(delay));
            }
        }
        let since = self.state().untrusted_since_us;
        if now.saturating_sub(since) >= policy.untrusted_deadline_us {
            return Err(self.fault(Kind::RecoveryTimedOut, now).into());
        }
        // The capture is still counted, and its clock still may not run back.
        self.captured(read_completed_at_us, now)?;
        self.state().counts.bypassed_capture_blocks += 1;
        Ok(None)
    }

    /// Read this block's reference from the ring, give it to the canceller
    /// and return the delay hint. The ring is known to be ready.
    fn render_counted(
        &mut self,
        capture_delayed_frames: i64,
        now: u64,
        echo: &mut EchoProcessor,
    ) -> Result<u16> {
        let origin = self.ring_origin;
        let queue_at = self
            .timing
            .queue_observed_at_us
            .max(self.timing.clock_start_requested_at_us.unwrap_or(0));
        let accepted_at = self.timing.accepted_at_us;
        let queued = self.timing.queued_frames;
        let clock = self.clock.as_mut().expect("recovery is capture-clocked");
        let mut block = [0; RENDER_SAMPLES];
        if clock.resampler.take(&mut block).is_err() {
            return Err(self.fault(Kind::ReferenceLate, now).into());
        }
        echo.render(&block)?;
        let since = now.saturating_sub(accepted_at).min(20_000);
        let owed = since as f64 * f64::from(RENDER_RATE) / 1e6;
        let ppm = clock
            .controller
            .observe(clock.resampler.unread() as f64 + owed);
        clock.resampler.set_ppm(ppm);
        let unread = clock.resampler.unread();
        let consumed = clock.resampler.consumed();
        self.timing.analysed_through = origin.saturating_add_unsigned(consumed).max(0) as u64;
        self.timing.analysed_at_us = now;
        // The hint is the same nominal estimate as ever. A queue observation
        // too old to interpolate leaves it at the capture's own share, and
        // the canceller's estimator does the rest; it is never a fault here.
        let elapsed = now.saturating_sub(queue_at);
        let played = (u128::from(elapsed) * u128::from(RENDER_RATE) / 1_000_000)
            .min(usize::MAX as u128) as usize;
        let render_frames = queued.saturating_sub(played).saturating_sub(unread);
        let delay = render_frames as u64 * 1_000 / u64::from(RENDER_RATE)
            + capture_delayed_frames as u64 * 1_000 / u64::from(CAPTURE_RATE);
        self.warm(block.iter().any(|sample| *sample != 0), now)?;
        Ok(delay.min(500) as u16)
    }

    /// One more block through a reseeded canceller. Only blocks in which the
    /// lamp was sounding teach it anything.
    fn warm(&mut self, sounding: bool, now: u64) -> Result<()> {
        let state = self.state();
        if state.trust == Trust::Warming && sounding {
            state.warm_remaining = state.warm_remaining.saturating_sub(1);
            if state.warm_remaining == 0 {
                state.trust = Trust::Trusted;
                state.cause = None;
                self.report(now, None, None)?;
            }
        }
        Ok(())
    }

    /// Fixed margin: keep the block just given to the canceller.
    pub(super) fn remember_rendered(&mut self) {
        let block = self.samples;
        if let Some(state) = self.recovery.as_mut() {
            if state.history.len() == HISTORY_BLOCKS {
                state.history.pop_front();
            }
            state.silent_blocks = if block.iter().all(|sample| *sample == 0) {
                state.silent_blocks.saturating_add(1)
            } else {
                0
            };
            state.history.push_back(block);
        }
    }

    /// Fixed margin: whether a packet that takes the count past its bound
    /// may be taken in while the capture block that settles it is awaited.
    pub(super) fn awaits_capture(&self, ahead: u64) -> bool {
        self.recovery.is_some()
            && self.clock.is_none()
            && self.timing.clock_started_at_us.is_some()
            && ahead <= MAX_AHEAD_BLOCKS + AWAITED_BLOCKS
    }

    /// Whether the capture block in hand finds this loop on time: the
    /// microphone has no more waiting than a lamp shows with its loop
    /// running, and the block before it was read no more than two blocks
    /// ago. Judged on this block's own status, never an earlier one's.
    fn capture_on_time(&self, capture_delayed_frames: i64, now: u64) -> bool {
        (0..=FRESH_CAPTURE_FRAMES).contains(&capture_delayed_frames)
            && self
                .last_capture_read_at_us
                .is_some_and(|read| fresh(read, now, ON_TIME_US))
    }

    /// Fixed margin: reset the canceller, give it its lead again from the
    /// newest accepted blocks and move the count by what that takes.
    pub(super) fn recentre(
        &mut self,
        side: Side,
        now: u64,
        echo: &mut EchoProcessor,
    ) -> Result<()> {
        self.spend_reseed(now)?;
        let lead = side.lead().min(self.state().history.len());
        if lead == 0 {
            // Nothing accepted to lead with: not a drift, and not recovered.
            return Err(self
                .fault(
                    match side {
                        Side::Ahead => Kind::ReferenceBacklog,
                        Side::Behind => Kind::ReferenceLate,
                    },
                    now,
                )
                .into());
        }
        echo.reset();
        let state = self.state();
        let first = state.history.len() - lead;
        for index in first..state.history.len() {
            echo.render(&state.history[index])?;
        }
        let margin = (self.timing.analysed_through / RENDER_SAMPLES as u64) as i64
            - self.running_capture_blocks() as i64;
        self.margin_rebase += margin - lead as i64;
        let warm = self.policy().warm_blocks;
        let state = self.state();
        state.counts.degradations += 1;
        state.counts.reseeds += 1;
        state.cause = Some(Cause::MarginExhausted);
        state.reseed_pending = true;
        state.warm_remaining = warm;
        state.trust = if warm == 0 {
            Trust::Trusted
        } else {
            Trust::Warming
        };
        self.report(now, None, Some(0))
    }

    /// One reseed from the budget, or the fault that says it is spent.
    fn spend_reseed(&mut self, now: u64) -> Result<()> {
        let policy = self.policy();
        let state = self.state();
        while state
            .reseeded_at_us
            .front()
            .is_some_and(|at| now.saturating_sub(*at) >= policy.window_us)
        {
            state.reseeded_at_us.pop_front();
        }
        if state.reseeded_at_us.len() >= policy.reseeds {
            return Err(self.fault(Kind::RecoveryExhausted, now).into());
        }
        state.reseeded_at_us.push_back(now);
        Ok(())
    }

    /// A packet, with recovery in force and the clock started. `Ok(false)`
    /// for one that was not used.
    pub(super) fn accept_recovering(
        &mut self,
        reference: RenderReference,
        now: u64,
        privacy: u64,
    ) -> Result<bool> {
        let count = reference.payload.frames();
        // What no stall explains stays a fault: another stream, another
        // privacy generation, a cursor or a clock that runs backwards.
        let fatal = if reference.playback_epoch != self.timing.playback_epoch
            || count == 0
            || count > RENDER_SAMPLES
        {
            Some(Kind::InvalidReference)
        } else if reference.privacy_generation != self.privacy_generation
            || privacy != self.privacy_generation
        {
            Some(Kind::PrivacyMismatch)
        } else if reference.queued_frames > 960
            || reference.accepted_at_us > reference.queue_observed_at_us
            || reference.accepted_at_us > now
            || reference.queue_observed_at_us > now
            || reference.queue_observed_at_us < self.timing.queue_observed_at_us
        {
            // A queue observation from the future, or from before the last
            // one, is not late. It is wrong, and its timing is not taken.
            Some(Kind::QueueTiming)
        } else {
            None
        };
        if let Some(reason) = fatal {
            let mut fault = self.fault(reason, now);
            fault.received_sequence = Some(reference.sequence);
            fault.received_first_sample = Some(reference.first_sample);
            return Err(fault.into());
        }
        // A packet at or before one already seen is not used and is counted.
        // Nothing it carries can displace what followed it.
        if reference.sequence <= self.sequence {
            self.state().counts.ignored_packets += 1;
            return Ok(false);
        }
        let regressed = if reference.first_sample < self.timing.accepted_through {
            Some(Kind::CursorGap)
        } else if reference.accepted_at_us < self.timing.accepted_at_us {
            Some(Kind::QueueTiming)
        } else {
            None
        };
        if let Some(reason) = regressed {
            let mut fault = self.fault(reason, now);
            fault.received_sequence = Some(reference.sequence);
            fault.received_first_sample = Some(reference.first_sample);
            return Err(fault.into());
        }
        let policy = self.policy();
        let next = self.sequence.checked_add(1) == Some(reference.sequence);
        let joined = reference.first_sample == self.timing.accepted_through;
        // The first packet of an epoch follows a prime, not a packet.
        let paused = self.sequence > 0
            && reference
                .accepted_at_us
                .saturating_sub(self.timing.accepted_at_us)
                > policy.sender_pause_us;
        let arrived_fresh = fresh(reference.accepted_at_us, now, MAX_REFERENCE_AGE_US)
            && fresh(reference.queue_observed_at_us, now, MAX_REFERENCE_AGE_US);
        let zeros = [0; RENDER_SAMPLES];
        let samples = match &reference.payload {
            RenderPayload::Silence { .. } => &zeros[..count],
            RenderPayload::Speech { samples, .. } => samples.as_slice(),
        };
        if self.trust() != Trust::Untrusted {
            let cause = if !next {
                Some(Cause::SequenceGap)
            } else if !joined {
                Some(Cause::CursorGap)
            } else if paused {
                Some(Cause::SenderPaused)
            } else {
                None
            };
            match cause {
                Some(cause) => self.degrade(cause, now, Some(reference.sequence))?,
                None => {
                    let clock = self.clock.as_mut().expect("recovery is capture-clocked");
                    if clock.resampler.push(samples).is_ok() {
                        if !arrived_fresh {
                            // Complete and in order: late to arrive, not unknown.
                            self.state().counts.late_packets += 1;
                        }
                        self.note(&reference, count);
                        return Ok(true);
                    }
                    self.degrade(Cause::Overflow, now, Some(reference.sequence))?;
                }
            }
        }
        // Untrusted. Only a run of packets that arrived fresh and follow one
        // another without a gap or a pause can be reseeded from. Anything
        // else is history from the stall and is counted as skipped.
        let continues = self.state().run && next && joined && !paused && arrived_fresh;
        let clock = self.clock.as_mut().expect("recovery is capture-clocked");
        if !continues || clock.resampler.push(samples).is_err() {
            clock.resampler.reset();
            self.ring_origin = reference.first_sample as i64;
            let clock = self.clock.as_mut().expect("recovery is capture-clocked");
            let started = arrived_fresh && clock.resampler.push(samples).is_ok();
            self.state().run = started;
        }
        // The reseed itself waits for the next capture block, which is
        // where the capture side's own lateness is known.
        self.note(&reference, count);
        Ok(true)
    }

    /// The speaker restarted its clock and primed a new epoch. Its canceller
    /// is as cold as after any reseed.
    pub(super) fn primed_recovering(&mut self, previous_epoch: u64, now: u64) -> Result<()> {
        self.ring_origin = -(LEAD_FRAMES as i64);
        if previous_epoch == 0 {
            return Ok(());
        }
        let warm = self.policy().warm_blocks;
        let state = self.state();
        let untrusted_us =
            (state.trust == Trust::Untrusted).then(|| now.saturating_sub(state.untrusted_since_us));
        state.run = false;
        state.history.clear();
        state.cause = Some(Cause::ClockRestart);
        state.trust = if warm == 0 {
            Trust::Trusted
        } else {
            Trust::Warming
        };
        state.warm_remaining = warm;
        self.report(now, None, untrusted_us)
    }

    fn degrade(&mut self, cause: Cause, now: u64, received: Option<u64>) -> Result<()> {
        // What waits in the ring was accepted and will not be analysed.
        let clock = self.clock.as_mut().expect("recovery is capture-clocked");
        clock.resampler.reset();
        let state = self.state();
        state.trust = Trust::Untrusted;
        state.cause = Some(cause);
        state.untrusted_since_us = now;
        state.run = false;
        state.counts.degradations += 1;
        self.report(now, received, None)
    }

    fn reseed(&mut self, now: u64, echo: &mut EchoProcessor) -> Result<()> {
        let policy = self.policy();
        self.spend_reseed(now)?;
        let state = self.state();
        let untrusted_us = now.saturating_sub(state.untrusted_since_us);
        // Cold: nothing the canceller learned on the old pairing is kept.
        echo.reset();
        let first = self.ring_origin.max(0) as u64;
        let skipped = first.saturating_sub(self.timing.analysed_through);
        self.timing.analysed_through = self.timing.analysed_through.max(first);
        self.timing.analysed_at_us = now;
        let clock = self.clock.as_mut().expect("recovery is capture-clocked");
        // The crystals are the ones they were: the rate learned is kept and
        // only the smoothed fill starts again.
        clock.controller.hold();
        let state = self.state();
        state.counts.skipped_frames += skipped;
        state.counts.reseeds += 1;
        state.run = false;
        state.reseed_pending = true;
        state.warm_remaining = policy.warm_blocks;
        state.trust = if policy.warm_blocks == 0 {
            Trust::Trusted
        } else {
            Trust::Warming
        };
        self.report(now, None, Some(untrusted_us))
    }

    fn note(&mut self, reference: &RenderReference, count: usize) {
        self.sequence = reference.sequence;
        self.timing.accepted_through = reference.first_sample + count as u64;
        self.timing.accepted_at_us = reference.accepted_at_us;
        self.timing.queue_observed_at_us = reference.queue_observed_at_us;
        self.timing.queued_frames = reference.queued_frames;
    }

    fn report(&mut self, now: u64, received: Option<u64>, untrusted_us: Option<u64>) -> Result<()> {
        let status = ReferenceStatus {
            trust: self.trust(),
            cause: self.state().cause,
            at_us: now,
            playback_epoch: self.timing.playback_epoch,
            capture_blocks_processed: self.timing.capture_blocks_processed,
            expected_sequence: self.sequence.saturating_add(1),
            received_sequence: received,
            accepted_through: self.timing.accepted_through,
            analysed_through: self.timing.analysed_through,
            untrusted_us,
            counts: self.recovery_counts().unwrap_or_default(),
        };
        let state = self.state();
        if state.events.len() == MAX_EVENTS {
            // A status that cannot be reported is not swallowed.
            return Err(self.fault(Kind::RecoveryExhausted, now).into());
        }
        state.events.push_back(status);
        Ok(())
    }

    fn policy(&self) -> Policy {
        self.recovery.as_ref().expect("recovery state").policy
    }

    fn state(&mut self) -> &mut State {
        self.recovery.as_mut().expect("recovery state")
    }
}

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;
