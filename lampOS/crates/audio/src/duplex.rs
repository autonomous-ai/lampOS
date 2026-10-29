//! Hearing a person while the lamp itself is speaking.
//!
//! A speech detector fed the cancelled microphone signal fires on the lamp's
//! own voice whenever the canceller lets some through, which on the lamps
//! measured is every two to three seconds of a reply. The level of what it
//! fires on is no help either: leaked playback is as loud as a person. What
//! does tell them apart is the render. [`Hearing`] reads one capture block's
//! detector score together with the [`EchoEvidence`] for the same block and
//! says which of six things the block is. It needs both: a score alone or a
//! level alone decides nothing here.
//!
//! [`Interruption`] turns a run of such blocks into at most three moments. A
//! person heard over the lamp makes it **yield**: the caller stops handing
//! reply audio to the speaker, which costs nothing that cannot be taken back.
//! Once the lamp's own sound has died away, speech that is still there is a
//! person beyond doubt, and the lamp **commits** to the interruption. If there
//! is none, it **resumes**. A brief acknowledgment, or a mistake by the first
//! step, therefore costs a short pause and never the reply.
//!
//! What this cannot do is hear a person the canceller has already removed.
//! When the lamp's own voice reaches its microphone far louder than the
//! person does, the canceller suppresses both, and nothing downstream of it
//! sees the person until the lamp stops. [`Margin`] measures how far apart the
//! two are so that this limit is reported and not guessed at.
//!
//! No clock, no allocation, no I/O. Thresholds are [`Hearing::EVALUATED`] and
//! [`Confirmation::EVALUATED`]: chosen on development recordings, not a
//! device default, and never chosen on the lamp's own voice alone.
use crate::evidence::EchoEvidence;

/// What one 10 ms capture block holds, as far as it can be told.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heard {
    /// Nothing speech-like, and nothing of the lamp's still sounding.
    Still,
    /// Speech, and nothing of the lamp's still sounding: not the lamp.
    Clear,
    /// Nothing speech-like while the lamp is sounding.
    Under,
    /// Speech-like while the lamp is sounding, and the render accounts for
    /// it: the lamp's own voice, let through by the canceller.
    Own,
    /// Speech-like while the lamp is sounding, and the render does not
    /// account for it: someone is speaking over the lamp.
    Over,
    /// Speech-like while the lamp is sounding, with no alignment between
    /// render and microphone to judge by. Not evidence of anything.
    Unknown,
}

impl Heard {
    /// Whether the block counts towards a person speaking.
    pub fn person(self) -> bool {
        matches!(self, Self::Clear | Self::Over)
    }
}

/// How a block is read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hearing {
    /// Detector score from which a block is speech-like.
    pub speech: f32,
    /// A block the render explains more of than this, in percent of the
    /// microphone's energy before cancellation, is the lamp's own voice.
    pub explained_percent: u8,
    /// So is a block whose cancelled signal must be playback by more than
    /// this, in percent, by energy accounting.
    pub floor_percent: u8,
    /// Render blocks of silence after which nothing of the lamp's is still
    /// arriving at the microphone.
    pub clear_after_blocks: u8,
    /// A block of a person over the lamp of which the render explains no
    /// more than this, in percent, holds a person plainly: the lamp's own
    /// voice is next to nothing in it.
    pub plain_percent: u8,
    /// Blocks since the render last explained half of the microphone at the
    /// alignment in use, beyond which a block says nothing about who is
    /// speaking. When the lamp starts to speak its sound has not reached the
    /// microphone yet and the alignment is the one its last reply left: for
    /// 0.2 s to 0.45 s on lamp-4ace the render explained none of the block,
    /// and the lamp's own first word, let through by a canceller that had
    /// not converged either, was heard as a person, plainly.
    pub fresh_blocks: u16,
    /// Speech where the render at the echo's alignment is silent
    /// ([`EchoEvidence::silent_at_echo`]), with that alignment no older
    /// than [`Self::fresh_blocks`], is a person over the lamp: nothing of the
    /// lamp's is in the window its echo would come from, only the room's
    /// tail, which the canceller has. Without it such a block says nothing
    /// ([`Heard::Unknown`]) until the render has been quiet
    /// [`Self::clear_after_blocks`], so a pause in the lamp's speech under
    /// 300 ms gave a yield nothing to hear (#828, `LAMP_LISTEN_GAPS=on`).
    pub silence_heard: bool,
}

impl Hearing {
    /// Chosen on the development half of two recorded sessions and their
    /// synthetic overlaps; see `docs/duplex-listening.md`. The 300 ms is the
    /// longest the microphone took to fall back to its floor after a reply,
    /// 270 ms over sixteen recorded stops, with one block more.
    pub const EVALUATED: Self = Self {
        speech: 0.5,
        explained_percent: 75,
        floor_percent: 80,
        clear_after_blocks: 30,
        plain_percent: 20,
        fresh_blocks: 40,
        silence_heard: false,
    };

    pub fn heard(&self, probability: f32, evidence: &EchoEvidence) -> Heard {
        let speech = probability.is_finite() && probability >= self.speech;
        if evidence.render_quiet_blocks >= self.clear_after_blocks {
            return if speech { Heard::Clear } else { Heard::Still };
        }
        if !speech {
            return Heard::Under;
        }
        if self.silence_heard
            && evidence.echo.is_none()
            && evidence
                .silent_at_echo
                .is_some_and(|age| age <= self.fresh_blocks)
        {
            return Heard::Over;
        }
        match evidence.echo {
            None => Heard::Unknown,
            // Nothing recent says where the lamp's own sound lies in this
            // block. A block the render does explain is never stale: it
            // sets the age to zero.
            Some(echo) if echo.alignment_age_blocks > self.fresh_blocks => Heard::Unknown,
            Some(echo)
                if echo.capture_explained <= self.explained_percent
                    && echo.residual_playback_floor <= self.floor_percent =>
            {
                Heard::Over
            }
            Some(_) => Heard::Own,
        }
    }

    /// Whether a block heard as a person over the lamp holds that person
    /// plainly. On lamp-4ace the lamp's own voice, misjudged as a person,
    /// was still 30% to 75% explained by the render; a played "Stop." was
    /// under 10% for twenty blocks.
    pub fn plain(&self, heard: Heard, evidence: &EchoEvidence) -> bool {
        heard == Heard::Over
            && evidence
                .echo
                .is_some_and(|echo| echo.capture_explained <= self.plain_percent)
    }

    /// The score to give a detector that starts a turn on consecutive speech
    /// blocks, so that it starts on a person and not on the lamp. The lamp's
    /// own voice and what cannot be judged count as no speech. A person over
    /// the lamp counts as speech, whatever the cancelled signal scored: the
    /// canceller has usually taken some of the person with the echo.
    /// Everything heard with the lamp silent keeps its own score.
    pub fn detector_score(heard: Heard, probability: f32) -> f32 {
        match heard {
            Heard::Over => 1.0,
            Heard::Own | Heard::Unknown => 0.0,
            Heard::Still | Heard::Clear | Heard::Under => probability,
        }
    }
}

/// When a yield becomes an interruption, and when it is given up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Confirmation {
    /// Consecutive [`Heard::Over`] blocks that make the lamp yield.
    pub onset_blocks: u8,
    /// [`Heard::Clear`] blocks, after the lamp's own sound has gone, that
    /// make the yield an interruption.
    pub speech_blocks: u8,
    /// Consecutive [`Heard::Still`] blocks that end the yield with the reply
    /// resumed.
    pub silence_blocks: u8,
    /// Blocks after which an undecided yield is given up.
    pub deadline_blocks: u16,
    /// Plain blocks, in the run that made the lamp yield and after it, with
    /// which a yield that ends in stillness is an interruption all the same.
    pub plain_blocks: u8,
    /// Consecutive [`Heard::Over`] blocks that make the lamp yield while it
    /// has only just had something to say, and for how many blocks that is.
    /// A canceller takes its first second of a reply to settle, and what it
    /// lets through of the lamp then is misjudged for longer than later on.
    pub early_onset_blocks: u8,
    pub early_blocks: u16,
}

impl Confirmation {
    /// 70 ms to yield, 60 ms of speech in the clear to commit, 400 ms of
    /// stillness or 1.1 s in all to resume. Someone who has been spoken over
    /// waits a moment before going on; with 150 ms the lamp resumed in the
    /// breath between a person's first word and their second. The yield took
    /// 60 ms until 2026-10-13. On lamp-4ace seven blocks stopped for every
    /// played cut-in that six did, and ten gave up four of twenty; see
    /// `docs/duplex-listening.md`.
    pub const EVALUATED: Self = Self {
        onset_blocks: 7,
        speech_blocks: 6,
        silence_blocks: 40,
        deadline_blocks: 110,
        plain_blocks: 10,
        // 1.2 s from the answer's first audio arriving: its first second
        // of sound and the quarter second it is held before it plays.
        early_onset_blocks: 10,
        early_blocks: 120,
    };
}

/// What the caller should do after a block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Nothing changes.
    Carry,
    /// Stop handing reply audio to the speaker. Keep everything.
    Yield,
    /// The person went on speaking once the lamp was silent, or said
    /// something short that was heard plainly: an interruption.
    Commit,
    /// Nobody went on speaking: hand reply audio to the speaker again.
    Resume,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Listening {
        over: u8,
        plain: u8,
    },
    Yielded {
        blocks: u16,
        speech: u8,
        still: u8,
        plain: u8,
    },
}

const LISTENING: State = State::Listening { over: 0, plain: 0 };

/// One reply's worth of yielding. Counts are kept for the trace.
#[derive(Clone, Copy, Debug)]
pub struct Interruption {
    rule: Confirmation,
    state: State,
    /// Blocks in a row for which there has been something to yield, since
    /// there last was nothing or since the reply was last resumed.
    spoken: u16,
    yields: u32,
    commits: u32,
    resumes: u32,
    /// The yield in force waits for the words said in it before stillness
    /// may resume it ([`Self::await_words`], #828).
    words_pending: bool,
    /// They were a stop word: the yield commits at its next block.
    stop_heard: bool,
    /// A stop word heard by a listener of its own (`stop/heard`, #828):
    /// the next block with something to yield begins a yield, whatever is
    /// heard in it ([`Self::force_yield`]).
    forced: bool,
    /// The yield in force, or the last one, began so.
    forced_yield: bool,
    /// `LAMP_BACKCHANNEL_HOLDS=on` (#2939): see [`Self::hold_backchannels`],
    /// with the still blocks after which a yield's words are asked for.
    holds_backchannels: bool,
    words_asked_after_still: u8,
    /// Whether the words of the yield in force, or the last one, were a
    /// backchannel; `None` until they came.
    backchannel: Option<bool>,
    /// Speech in the clear in the yield in force when its words came, or
    /// when they were asked for if that was sooner: what follows is speech
    /// they did not hear (#2939).
    speech_at_words: u8,
    /// Speech in the clear in the yield in force when its listener first
    /// asked for its words ([`Self::words_asked`]): what was said after is
    /// not in the words that ask brings (reviewer-two's P2s on #3295).
    speech_at_ask: Option<u8>,
    /// Blocks the commit of the yield in force, or the last one, was held
    /// for its words.
    held: u16,
    /// The speech in the clear of the yield in force is talk well under the
    /// person the lamp is answering ([`Self::quiet_talk`], #3967).
    quiet_talk: bool,
}

/// How long a yield's commit is held for its words once they have been
/// asked for (#2939): 500 ms, for the speech-to-text's answer, about
/// 250-350 ms through the relay. The hold is tied to the ask, which comes
/// only once the person has been still for a while, not to when the commit
/// fell due: a 300 ms "Yeah." on the speech rule is due 60 ms into it, and
/// its words come some 0.7 s later.
pub const BACKCHANNEL_WORDS_WAIT_BLOCKS: u8 = 50;
/// Speech in the clear in a yield that commits it even when its words are
/// a backchannel, and that no hold waits past: 700 ms, a sentence. A
/// spoken backchannel runs past 400 ms: on lamp-52e6 on 23 Oct a played
/// "Yeah." and "Uh-huh.", about 0.5 s of sound each, reached 400 ms in the
/// clear and committed, their words a backchannel or not yet come (#2939).
pub const BACKCHANNEL_SPEECH_BLOCKS: u8 = 70;

impl Interruption {
    pub const fn new(rule: Confirmation) -> Self {
        Self {
            rule,
            state: LISTENING,
            spoken: 0,
            yields: 0,
            commits: 0,
            resumes: 0,
            words_pending: false,
            stop_heard: false,
            forced: false,
            forced_yield: false,
            holds_backchannels: false,
            words_asked_after_still: 0,
            backchannel: None,
            speech_at_words: 0,
            speech_at_ask: None,
            held: 0,
            quiet_talk: false,
        }
    }

    /// `LAMP_BACKCHANNEL_HOLDS=on` (#2939): while a yield's words are
    /// awaited ([`Self::await_words`]), its commit is held for them: until
    /// [`BACKCHANNEL_WORDS_WAIT_BLOCKS`] after they are asked for, at
    /// `words_asked_after_still` still blocks, never past the yield's
    /// deadline, and only while its speech in the clear is under
    /// [`BACKCHANNEL_SPEECH_BLOCKS`]. Words that are a backchannel
    /// let stillness resume it, unless that speech reaches a sentence or
    /// speech goes on after they came and reaches the speech rule; any
    /// other words end the hold, and a due commit commits. No words by the
    /// hold's end resume the reply: a wordless yield is room sound or the
    /// lamp's own echo, and ends no answer. A stop word and a forced yield
    /// are never held.
    pub fn hold_backchannels(&mut self, words_asked_after_still: u8) {
        self.holds_backchannels = true;
        self.words_asked_after_still = words_asked_after_still;
    }

    /// Whether backchannels are held ([`Self::hold_backchannels`]).
    pub fn holds_backchannels(&self) -> bool {
        self.holds_backchannels
    }

    /// Whether the words of the yield in force, or the last one, were a
    /// backchannel; `None` until they came.
    pub fn backchannel(&self) -> Option<bool> {
        self.backchannel
    }

    /// The yield in force, or the last one, was held as a backchannel: its
    /// words were one and backchannels are held.
    pub fn yield_was_backchannel(&self) -> bool {
        self.holds_backchannels && self.backchannel == Some(true)
    }

    /// Blocks the commit of the yield in force, or the last one, was held.
    pub fn held_blocks(&self) -> u16 {
        self.held
    }

    /// A stop word heard by a listener of its own (`stop/heard`, #828):
    /// the next block with something to yield begins a yield whatever is
    /// heard in it, and from there the yield commits on a person and
    /// resumes on stillness or its deadline, as any yield. A false hearing
    /// heard plainly, so quiet after it commits the yield: the lamp stops.
    /// Nothing while a yield is in force, and nothing kept past a block with
    /// nothing to yield. Whether it was taken.
    pub fn force_yield(&mut self) -> bool {
        let taken = matches!(self.state, State::Listening { .. });
        self.forced |= taken;
        taken
    }

    /// A forced yield waits for the next block.
    pub fn force_pending(&self) -> bool {
        self.forced
    }

    /// The yield in force, or the last one, was forced.
    pub fn yield_was_forced(&self) -> bool {
        self.forced_yield
    }

    /// The yield in force waits for the words said in it: stillness does
    /// not resume it until they come ([`Self::words`]), or until its
    /// deadline does, which still resumes it whatever was heard (#828).
    /// Nothing without a yield in force.
    pub fn await_words(&mut self) {
        if self.yielded() {
            self.words_pending = true;
        }
    }

    /// The listener asked for the words said in the yield in force: its
    /// first ask is where they end, whatever stillness this rule has seen
    /// (`heard` asks at a still block, `heard-always` also after the
    /// detector's own pause, before this rule counts one). Nothing without
    /// a yield in force.
    pub fn words_asked(&mut self) {
        if let State::Yielded { speech, .. } = self.state {
            self.speech_at_ask.get_or_insert(speech);
        }
    }

    /// The words said in the yield in force came: a stop word commits it at
    /// its next block, and anything else lets stillness resume it as before.
    /// Whether they were a backchannel is kept for the record, and with
    /// [`Self::hold_backchannels`] decides the commit held for them.
    pub fn words(&mut self, stop: bool, backchannel: bool) {
        if let State::Yielded { speech, .. } = self.state {
            self.speech_at_words = self.speech_at_ask.map_or(speech, |asked| asked.min(speech));
            self.words_pending = false;
            self.stop_heard = stop;
            self.backchannel = Some(backchannel);
        }
    }

    /// `LAMP_CUT_IN_NEAR_ASKER=on` (#3967): the speech heard in the clear in
    /// the yield in force is talk well under the person the lamp is
    /// answering, the room behind them: it commits nothing, and the yield
    /// resumes on stillness or at its deadline as one that came to nothing.
    /// A stop word and a forced yield still commit. Set before each block;
    /// a new yield starts without it.
    pub fn quiet_talk(&mut self, quiet: bool) {
        self.quiet_talk = quiet && self.yielded();
    }

    /// Blocks of stillness in a row in the yield in force, while one is.
    pub fn still_blocks(&self) -> Option<u8> {
        match self.state {
            State::Yielded { still, .. } => Some(still),
            State::Listening { .. } => None,
        }
    }

    /// One capture block that is not known to hold a person plainly. See
    /// [`Self::observe`].
    pub fn block(&mut self, heard: Heard, speaking: bool) -> Step {
        self.observe(heard, false, speaking)
    }

    /// One capture block. `speaking` is whether the lamp has reply audio it
    /// would otherwise be handing to the speaker; without it there is nothing
    /// to yield and a person is an ordinary new turn. `plain` is
    /// [`Hearing::plain`] for the block.
    ///
    /// A yield ends in one of three ways. Speech once the lamp is silent
    /// commits it. Stillness resumes the reply, unless what made the lamp
    /// yield was heard plainly for long enough: a short word that the lamp's
    /// own voice cannot account for is a person telling it something, "stop"
    /// as likely as anything, and the lamp does not talk over that. The
    /// deadline resumes it whatever was heard.
    pub fn observe(&mut self, heard: Heard, plain: bool, speaking: bool) -> Step {
        self.spoken = if speaking {
            self.spoken.saturating_add(1)
        } else {
            0
        };
        let needed = self.onset_needed();
        let forced = std::mem::take(&mut self.forced) && speaking;
        match &mut self.state {
            State::Listening { over, plain: seen } => {
                if speaking && heard == Heard::Over {
                    *over = over.saturating_add(1);
                    *seen = seen.saturating_add(u8::from(plain));
                } else {
                    *over = 0;
                    *seen = 0;
                }
                if *over < needed && !forced {
                    return Step::Carry;
                }
                self.state = State::Yielded {
                    blocks: 0,
                    speech: 0,
                    still: 0,
                    // A stop heard by its own listener counts as heard plainly:
                    // quiet after it commits the yield, as after a short word
                    // the lamp's own voice cannot explain.
                    plain: if forced {
                        self.rule.plain_blocks
                    } else {
                        *seen
                    },
                };
                self.yields += 1;
                self.forced_yield = forced;
                self.quiet_talk = false;
                self.backchannel = None;
                self.speech_at_words = 0;
                self.speech_at_ask = None;
                self.held = 0;
                Step::Yield
            }
            State::Yielded {
                blocks,
                speech,
                still,
                plain: seen,
            } => {
                *blocks = blocks.saturating_add(1);
                match heard {
                    Heard::Clear => {
                        *speech = speech.saturating_add(1);
                        *still = 0;
                    }
                    Heard::Still => *still = still.saturating_add(1),
                    // The person going on over the lamp's dying sound.
                    Heard::Over => *seen = seen.saturating_add(u8::from(plain)),
                    // The lamp's own sound is still dying away: no evidence.
                    Heard::Under | Heard::Own | Heard::Unknown => {}
                }
                let quiet = *still >= self.rule.silence_blocks.max(1);
                let sentence = *speech >= BACKCHANNEL_SPEECH_BLOCKS;
                // A backchannel commits only as a sentence would (#2939),
                // or as any speech does once it goes on after its words
                // were asked for: they were asked for at a pause and heard
                // nothing said after it, however late they came (lamp-52e6 on 24 Oct, a "Stop, stop." whose
                // first word came as "yeah" over the lamp and whose second,
                // 250 ms in the clear, was held as that backchannel).
                let after_words = speech.saturating_sub(self.speech_at_words);
                // Talk well under the asker commits nothing (#3967): the
                // room behind them, not someone cutting in.
                let room_behind = self.quiet_talk && !self.forced_yield;
                let due = self.stop_heard
                    || (!room_behind
                        && if self.holds_backchannels && self.backchannel == Some(true) {
                            sentence || after_words >= self.rule.speech_blocks.max(1)
                        } else {
                            *speech >= self.rule.speech_blocks.max(1)
                                || (quiet && *seen >= self.rule.plain_blocks.max(1))
                        });
                // Awaiting words that may yet be a backchannel: a due
                // commit waits for them, and one whose words never come
                // resumes the reply rather than end it (lamp-52e6 on #3200:
                // a wordless yield, room sound or the lamp's own echo, cut a
                // story). A sentence still commits without them, and so does
                // speech that goes on after they were asked for: the ask
                // heard only what came before its pause, and a "Wait" whose
                // question follows 300 ms on came back as no words
                // (lamp-52e6 on 24 Oct, 2 of 3 cut-ins held to the deadline).
                let after_ask = self
                    .speech_at_ask
                    .map_or(0, |asked| speech.saturating_sub(asked));
                let wordless = self.holds_backchannels
                    && self.words_pending
                    && !self.stop_heard
                    && !self.forced_yield
                    && !sentence
                    && after_ask < self.rule.speech_blocks.max(1);
                // Held for its words, never past the deadline: the request
                // kept for the commit is kept no longer than a yield lasts.
                let held = due
                    && wordless
                    && *blocks < self.rule.deadline_blocks
                    && u16::from(*still)
                        < u16::from(self.words_asked_after_still)
                            + u16::from(BACKCHANNEL_WORDS_WAIT_BLOCKS);
                if held {
                    self.held += 1;
                    Step::Carry
                } else if due && !wordless {
                    self.state = LISTENING;
                    self.words_pending = false;
                    self.stop_heard = false;
                    self.commits += 1;
                    Step::Commit
                } else if due
                    || (quiet && !self.words_pending)
                    || *blocks >= self.rule.deadline_blocks
                {
                    self.state = LISTENING;
                    self.words_pending = false;
                    self.stop_heard = false;
                    self.resumes += 1;
                    // Going on after a pause is a beginning again.
                    self.spoken = 0;
                    Step::Resume
                } else {
                    Step::Carry
                }
            }
        }
    }

    /// Whether reply audio is being withheld from the speaker.
    pub fn yielded(&self) -> bool {
        matches!(self.state, State::Yielded { .. })
    }

    /// Consecutive [`Heard::Over`] blocks that make the lamp yield as things
    /// stand: more while it has only just begun to speak.
    pub fn onset_needed(&self) -> u8 {
        let rule = &self.rule;
        if self.spoken <= rule.early_blocks {
            rule.early_onset_blocks.max(rule.onset_blocks)
        } else {
            rule.onset_blocks
        }
        .max(1)
    }

    /// Consecutive [`Heard::Over`] blocks counted towards a yield that has
    /// not happened: zero once it has, and while there is nothing to yield.
    pub fn onset_run(&self) -> u8 {
        match self.state {
            State::Listening { over, .. } => over,
            State::Yielded { .. } => 0,
        }
    }

    /// Blocks since the yield began, while one is in force.
    pub fn yielded_blocks(&self) -> Option<u16> {
        match self.state {
            State::Yielded { blocks, .. } => Some(blocks),
            State::Listening { .. } => None,
        }
    }

    /// Yields, commits and resumes since construction.
    pub fn counts(&self) -> (u32, u32, u32) {
        (self.yields, self.commits, self.resumes)
    }

    /// Forget a yield in force, for a reply that ended or was cancelled
    /// another way. The counts stay.
    pub fn clear(&mut self) {
        self.state = LISTENING;
        self.words_pending = false;
        self.stop_heard = false;
    }
}

/// How loud a person is at the microphone against the lamp's own voice.
///
/// Both are slow averages of the level before cancellation, in decibels: the
/// person's over blocks heard in the clear, the lamp's over blocks that were
/// its own voice. Their difference is the margin. At about -6 dB and above a
/// person over the lamp was heard in most synthetic trials. Below about
/// -12 dB the canceller removed the person with the echo and none of the
/// evidence here could see them. A margin is not available until both have
/// been heard.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Margin {
    person_db: Option<f32>,
    own_db: Option<f32>,
}

impl Margin {
    /// Blocks each average takes to move most of the way: two seconds.
    const BLOCKS: f32 = 200.0;

    pub fn observe(&mut self, heard: Heard, evidence: &EchoEvidence) {
        let level = f32::from(evidence.capture_level.tenths_of_db()) / 10.0;
        let slot = match heard {
            Heard::Clear => &mut self.person_db,
            Heard::Own => &mut self.own_db,
            _ => return,
        };
        let average = slot.get_or_insert(level);
        *average += (level - *average) / Self::BLOCKS;
    }

    pub fn person_db(&self) -> Option<f32> {
        self.person_db
    }

    pub fn own_db(&self) -> Option<f32> {
        self.own_db
    }

    /// The person's level less the lamp's own, in decibels.
    pub fn db(&self) -> Option<f32> {
        Some(self.person_db? - self.own_db?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::{EchoMatch, Level};

    fn level(db: f32) -> Level {
        Level::from_tenths_of_db((db * 10.0) as i16)
    }

    /// Evidence for a block: render blocks since the lamp last sounded, and
    /// the two shares the observer reports when it has an alignment.
    fn evidence(quiet: u8, shares: Option<(u8, u8)>) -> EchoEvidence {
        EchoEvidence {
            blocks: 1_000,
            render_active_blocks: if quiet == 0 { 50 } else { 0 },
            render_quiet_blocks: quiet,
            capture_level: level(-20.0),
            residual_level: level(-30.0),
            echo: shares.map(|(capture_explained, residual_playback_floor)| EchoMatch {
                render_level: level(-18.0),
                capture_explained,
                residual_explained: 10,
                residual_playback_floor,
                alignment_age_blocks: 0,
                render_block_level: Level::FLOOR,
                render_drop: 0,
            }),
            silent_at_echo: None,
        }
    }

    const HEARING: Hearing = Hearing::EVALUATED;

    /// #828: speech while the render at the echo's alignment is silent,
    /// the alignment `age` blocks old.
    fn silent(quiet: u8, age: u16) -> EchoEvidence {
        EchoEvidence {
            silent_at_echo: Some(age),
            ..evidence(quiet, None)
        }
    }

    /// #828, listening gaps: in the lamp's own silence a person is heard
    /// over it, with the switch and a fresh alignment. Without the switch,
    /// or with no alignment at all, it says nothing, as before; and quiet
    /// long enough is the clear, as before.
    #[test]
    fn speech_in_the_lamps_own_silence_is_over_it_only_with_the_switch() {
        let listening = Hearing {
            silence_heard: true,
            ..Hearing::EVALUATED
        };
        assert_eq!(listening.heard(0.9, &silent(5, 12)), Heard::Over);
        assert_eq!(HEARING.heard(0.9, &silent(5, 12)), Heard::Unknown);
        // No speech is no person; a stale alignment says nothing.
        assert_eq!(listening.heard(0.1, &silent(5, 12)), Heard::Under);
        assert_eq!(listening.heard(0.9, &silent(5, 41)), Heard::Unknown);
        // No alignment at all is still not evidence of anything.
        assert_eq!(listening.heard(0.9, &evidence(5, None)), Heard::Unknown);
        // Quiet long enough is the clear, as before.
        assert_eq!(listening.heard(0.9, &silent(30, 12)), Heard::Clear);
        // A render the alignment explains is still the lamp.
        assert_eq!(
            listening.heard(0.9, &evidence(0, Some((97, 10)))),
            Heard::Own
        );
    }

    #[test]
    fn a_score_alone_decides_nothing_while_the_lamp_is_sounding() {
        // The same high score, four different blocks.
        let own = HEARING.heard(0.99, &evidence(0, Some((97, 10))));
        let own_by_accounting = HEARING.heard(0.99, &evidence(0, Some((60, 92))));
        let over = HEARING.heard(0.99, &evidence(0, Some((40, 0))));
        let unknown = HEARING.heard(0.99, &evidence(0, None));
        assert_eq!(
            (own, own_by_accounting, over, unknown),
            (Heard::Own, Heard::Own, Heard::Over, Heard::Unknown)
        );
        // And a level alone decides nothing either: the same block, louder.
        let mut loud = evidence(0, Some((97, 10)));
        loud.residual_level = level(-8.0);
        assert_eq!(HEARING.heard(0.99, &loud), Heard::Own);
    }

    #[test]
    fn a_block_with_no_recent_match_says_nothing_about_who_is_speaking() {
        // The lamp begins a reply: the render is sounding, the alignment is
        // the one its last reply left, and none of the block is explained.
        let mut starting = evidence(0, Some((0, 0)));
        let stale = |evidence: &mut EchoEvidence, age: u16| {
            evidence.echo.as_mut().unwrap().alignment_age_blocks = age;
        };
        stale(&mut starting, 300);
        assert_eq!(HEARING.heard(0.99, &starting), Heard::Unknown);
        assert!(!HEARING.plain(HEARING.heard(0.99, &starting), &starting));
        // The same block a moment after the render last explained the
        // microphone is a person, and plainly one.
        stale(&mut starting, HEARING.fresh_blocks);
        assert_eq!(HEARING.heard(0.99, &starting), Heard::Over);
        assert!(HEARING.plain(Heard::Over, &starting));
        stale(&mut starting, HEARING.fresh_blocks + 1);
        assert_eq!(HEARING.heard(0.99, &starting), Heard::Unknown);
        // Without speech it is the lamp sounding and nothing else, as ever.
        assert_eq!(HEARING.heard(0.1, &starting), Heard::Under);
        // So the lamp's own first word never stops it, however long.
        let mut interruption = Interruption::new(Confirmation::EVALUATED);
        for _ in 0..60 {
            let heard = HEARING.heard(0.99, &starting);
            let plain = HEARING.plain(heard, &starting);
            assert_eq!(interruption.observe(heard, plain, true), Step::Carry);
        }
    }

    #[test]
    fn the_thresholds_are_the_boundaries_they_say() {
        assert_eq!(
            HEARING.heard(0.5, &evidence(0, Some((75, 80)))),
            Heard::Over
        );
        assert_eq!(HEARING.heard(0.5, &evidence(0, Some((76, 80)))), Heard::Own);
        assert_eq!(HEARING.heard(0.5, &evidence(0, Some((75, 81)))), Heard::Own);
        assert_eq!(
            HEARING.heard(0.49, &evidence(0, Some((10, 0)))),
            Heard::Under
        );
        assert_eq!(
            HEARING.heard(f32::NAN, &evidence(0, Some((10, 0)))),
            Heard::Under
        );
        // 290 ms after the render stopped the lamp may still be sounding.
        assert_eq!(HEARING.heard(0.9, &evidence(29, None)), Heard::Unknown);
        assert_eq!(HEARING.heard(0.9, &evidence(30, None)), Heard::Clear);
        assert_eq!(HEARING.heard(0.1, &evidence(30, None)), Heard::Still);
        assert_eq!(
            HEARING.heard(0.9, &evidence(64, Some((99, 99)))),
            Heard::Clear
        );
    }

    #[test]
    fn the_detector_is_told_of_a_person_and_not_of_the_lamp() {
        assert_eq!(Hearing::detector_score(Heard::Over, 0.55), 1.0);
        assert_eq!(Hearing::detector_score(Heard::Own, 0.99), 0.0);
        assert_eq!(Hearing::detector_score(Heard::Unknown, 0.99), 0.0);
        for heard in [Heard::Still, Heard::Clear, Heard::Under] {
            assert_eq!(Hearing::detector_score(heard, 0.62), 0.62);
        }
        assert!(Heard::Over.person() && Heard::Clear.person());
        assert!(!Heard::Own.person() && !Heard::Unknown.person() && !Heard::Under.person());
    }

    /// Blocks over the lamp that make it yield in the first second it has
    /// something to say, which is where every run below takes place unless
    /// it says otherwise.
    const ONSET: usize = Confirmation::EVALUATED.early_onset_blocks as usize;
    /// And later in a reply.
    const LATE: usize = Confirmation::EVALUATED.onset_blocks as usize;

    fn run(interruption: &mut Interruption, blocks: &[(Heard, usize)]) -> Vec<(usize, Step)> {
        let mut steps = Vec::new();
        let mut index = 0;
        for &(heard, count) in blocks {
            for _ in 0..count {
                match interruption.block(heard, true) {
                    Step::Carry => {}
                    step => steps.push((index, step)),
                }
                index += 1;
            }
        }
        steps
    }

    #[test]
    fn a_person_who_goes_on_speaking_is_yielded_to_and_then_committed_to() {
        let mut interruption = Interruption::new(Confirmation::EVALUATED);
        // Own voice, then a person over it. The lamp yields on the seventh
        // block. Its own sound takes 320 ms to go; the person is then heard
        // in the clear and the sixth such block commits.
        let steps = run(
            &mut interruption,
            &[
                (Heard::Own, 40),
                (Heard::Over, ONSET),
                (Heard::Over, 16 - ONSET),
                (Heard::Unknown, 23),
                (Heard::Clear, 6),
            ],
        );
        assert_eq!(steps, [(39 + ONSET, Step::Yield), (84, Step::Commit)]);
        assert!(!interruption.yielded());
        assert_eq!(interruption.counts(), (1, 1, 0));
    }

    /// #3967 (`LAMP_CUT_IN_NEAR_ASKER`): talk in the clear well under the
    /// person being answered, the room behind them, commits nothing; the
    /// yield resumes at its deadline as one that came to nothing. A stop
    /// word still commits it, and a new yield starts without the mark.
    #[test]
    fn talk_well_under_the_asker_resumes_the_reply_and_a_stop_word_still_ends_it() {
        let yielded = |interruption: &mut Interruption| {
            let steps = run(
                interruption,
                &[(Heard::Own, 40), (Heard::Over, 16), (Heard::Unknown, 23)],
            );
            assert_eq!(steps, [(39 + ONSET, Step::Yield)]);
        };
        let mut room = Interruption::new(Confirmation::EVALUATED);
        yielded(&mut room);
        let mut steps = Vec::new();
        for index in 0..100 {
            room.quiet_talk(true);
            match room.block(Heard::Clear, true) {
                Step::Carry => {}
                step => steps.push((index, step)),
            }
        }
        // The yield's deadline, 110 blocks from it, resumes it.
        let before = 16 - ONSET + 23;
        assert_eq!(steps, [(110 - before - 1, Step::Resume)]);
        assert_eq!(room.counts(), (1, 0, 1));
        // A stop word still ends the reply.
        let mut stop = Interruption::new(Confirmation::EVALUATED);
        yielded(&mut stop);
        stop.quiet_talk(true);
        stop.words(true, false);
        assert_eq!(run(&mut stop, &[(Heard::Clear, 1)]), [(0, Step::Commit)]);
        // The mark is for the yield in force only.
        room.quiet_talk(true);
        yielded(&mut room);
        assert_eq!(run(&mut room, &[(Heard::Clear, 6)]), [(5, Step::Commit)]);
    }

    /// #828, step 2: a yield that waits for the words said in it. A short
    /// word over the lamp, then stillness: while its words are awaited,
    /// stillness does not resume it. A stop word commits it at its next
    /// block; other words let stillness resume it; and with no words the
    /// deadline still resumes it. Without waiting, as before.
    #[test]
    fn a_yield_waiting_for_its_words_commits_on_a_stop_word_and_otherwise_resumes() {
        // A word over the lamp: the yield, then its own sound dying away.
        let short_word = |interruption: &mut Interruption| {
            let steps = run(interruption, &[(Heard::Over, 25), (Heard::Under, 14)]);
            assert_eq!(steps, [(ONSET - 1, Step::Yield)]);
        };
        // A stop word.
        let mut stop = Interruption::new(Confirmation::EVALUATED);
        short_word(&mut stop);
        stop.await_words();
        assert_eq!(run(&mut stop, &[(Heard::Still, 40)]), []);
        assert_eq!(stop.still_blocks(), Some(40));
        stop.words(true, false);
        assert_eq!(run(&mut stop, &[(Heard::Still, 1)]), [(0, Step::Commit)]);
        assert_eq!(stop.counts(), (1, 1, 0));
        // Other words: stillness resumes it as it would have.
        let mut other = Interruption::new(Confirmation::EVALUATED);
        short_word(&mut other);
        other.await_words();
        assert_eq!(run(&mut other, &[(Heard::Still, 40)]), []);
        other.words(false, false);
        assert_eq!(run(&mut other, &[(Heard::Still, 1)]), [(0, Step::Resume)]);
        // No words: the deadline, 110 blocks from the yield, resumes it.
        let mut none = Interruption::new(Confirmation::EVALUATED);
        short_word(&mut none);
        none.await_words();
        assert_eq!(run(&mut none, &[(Heard::Still, 80)]), []);
        assert_eq!(run(&mut none, &[(Heard::Still, 1)]), [(0, Step::Resume)]);
        // Without waiting, stillness resumes it at its fortieth block.
        let mut plain = Interruption::new(Confirmation::EVALUATED);
        short_word(&mut plain);
        assert_eq!(run(&mut plain, &[(Heard::Still, 40)]), [(39, Step::Resume)]);
        // Nothing without a yield in force, and nothing left behind by one.
        plain.await_words();
        plain.words(true, false);
        assert_eq!(plain.still_blocks(), None);
        assert_eq!(run(&mut plain, &[(Heard::Own, 50)]), []);
        stop.await_words();
        assert!(!stop.yielded());
    }

    #[test]
    fn an_acknowledgment_costs_a_pause_and_not_the_reply() {
        let mut interruption = Interruption::new(Confirmation::EVALUATED);
        // 250 ms of "mm-hm" over the lamp, then nobody.
        let steps = run(
            &mut interruption,
            &[
                (Heard::Over, 25),
                (Heard::Under, 14),
                (Heard::Still, 40),
                (Heard::Own, 50),
            ],
        );
        assert_eq!(steps, [(ONSET - 1, Step::Yield), (78, Step::Resume)]);
        assert_eq!(interruption.counts(), (1, 0, 1));
        // The pause was 720 ms: from the yield to the resume.
        assert_eq!(steps[1].0 - steps[0].0, 79 - ONSET);
    }

    #[test]
    fn a_short_word_heard_plainly_is_an_interruption_though_nothing_follows_it() {
        let rule = Confirmation::EVALUATED;
        let plain = usize::from(rule.plain_blocks);
        let play = |blocks: &[(Heard, bool, usize)]| {
            let mut interruption = Interruption::new(rule);
            let mut steps = Vec::new();
            let mut index = 0;
            for &(heard, plainly, count) in blocks {
                for _ in 0..count {
                    match interruption.observe(heard, plainly, true) {
                        Step::Carry => {}
                        step => steps.push((index, step)),
                    }
                    index += 1;
                }
            }
            (steps, interruption.counts())
        };
        // "Stop.": 200 ms that the render cannot account for, then nobody.
        let (steps, counts) = play(&[
            (Heard::Over, true, 20),
            (Heard::Under, false, 30),
            (Heard::Still, false, 40),
        ]);
        assert_eq!(steps, [(ONSET - 1, Step::Yield), (89, Step::Commit)]);
        assert_eq!(counts, (1, 1, 0));
        // The same length with the lamp's own voice half of every block: a
        // pause, as for any acknowledgment.
        let (steps, counts) = play(&[
            (Heard::Over, false, 20),
            (Heard::Under, false, 30),
            (Heard::Still, false, 40),
        ]);
        assert_eq!(steps, [(ONSET - 1, Step::Yield), (89, Step::Resume)]);
        assert_eq!(counts, (1, 0, 1));
        // Plain blocks count from the first of the run to the last heard
        // over the lamp's dying sound, and one short of enough is not enough.
        for (after, step) in [(1, Step::Commit), (0, Step::Resume)] {
            let (steps, _) = play(&[
                (Heard::Over, false, ONSET - (plain - 1)),
                (Heard::Over, true, plain - 1),
                (Heard::Own, false, 5),
                (Heard::Over, true, after),
                (Heard::Under, false, 30),
                (Heard::Still, false, 40),
            ]);
            assert_eq!(steps.last().unwrap().1, step, "{after} after the yield");
        }
        // A run that broke before the yield lends the next one nothing.
        let (steps, _) = play(&[
            (Heard::Over, true, ONSET - 1),
            (Heard::Own, false, 1),
            (Heard::Over, false, ONSET),
            (Heard::Over, true, plain - 1),
            (Heard::Under, false, 30),
            (Heard::Still, false, 40),
        ]);
        assert_eq!(steps.last().unwrap().1, Step::Resume);
        // And the deadline resumes whatever was heard.
        let (steps, _) = play(&[(Heard::Over, true, 30), (Heard::Unknown, false, 200)]);
        assert_eq!(steps.last().unwrap().1, Step::Resume);
    }

    #[test]
    fn a_lamp_that_has_only_just_begun_to_speak_takes_longer_to_stop() {
        let rule = Confirmation::EVALUATED;
        let (early, late) = (
            usize::from(rule.early_onset_blocks),
            usize::from(rule.onset_blocks),
        );
        assert!(late < early);
        let beginning = usize::from(rule.early_blocks);
        // A second and more into a reply: the shorter run is enough.
        let mut interruption = Interruption::new(rule);
        let steps = run(
            &mut interruption,
            &[(Heard::Own, beginning), (Heard::Over, late)],
        );
        assert_eq!(steps, [(beginning + late - 1, Step::Yield)]);
        // The same run in its first second is not, and the longer one is.
        let mut interruption = Interruption::new(rule);
        let steps = run(
            &mut interruption,
            &[
                (Heard::Own, 30),
                (Heard::Over, late),
                (Heard::Own, 1),
                (Heard::Over, early),
            ],
        );
        assert_eq!(steps, [(30 + late + early, Step::Yield)]);
        // A reply that goes on after a pause has begun again.
        let mut interruption = Interruption::new(rule);
        let steps = run(
            &mut interruption,
            &[
                (Heard::Own, beginning),
                (Heard::Over, late),
                (Heard::Under, 30),
                (Heard::Still, 40),
                (Heard::Own, 20),
                (Heard::Over, late),
                (Heard::Own, 1),
                (Heard::Over, early),
            ],
        );
        let resumed = beginning + late + 70 - 1;
        assert_eq!(
            steps,
            [
                (beginning + late - 1, Step::Yield),
                (resumed, Step::Resume),
                (resumed + 20 + late + 1 + early, Step::Yield)
            ]
        );
        // With nothing to yield the count of what has been spoken starts over.
        let mut interruption = Interruption::new(rule);
        for _ in 0..beginning + 50 {
            interruption.observe(Heard::Own, false, true);
        }
        interruption.observe(Heard::Still, false, false);
        assert_eq!(usize::from(interruption.onset_needed()), early);
    }

    #[test]
    fn the_lamps_own_voice_never_yields_however_long_or_loud() {
        let mut interruption = Interruption::new(Confirmation::EVALUATED);
        let steps = run(
            &mut interruption,
            &[
                (Heard::Own, 500),
                (Heard::Unknown, 200),
                (Heard::Under, 100),
                // One block short of a yield and one of own voice, repeated.
                (Heard::Over, LATE - 1),
                (Heard::Own, 1),
                (Heard::Over, LATE - 1),
                (Heard::Own, 1),
            ],
        );
        assert_eq!(steps, []);
        assert_eq!(interruption.counts(), (0, 0, 0));
    }

    #[test]
    fn a_yield_that_nothing_settles_is_given_up_at_its_deadline() {
        let mut interruption = Interruption::new(Confirmation::EVALUATED);
        // After the yield nothing is ever clear: the render never goes quiet.
        let steps = run(
            &mut interruption,
            &[(Heard::Over, ONSET), (Heard::Unknown, 200)],
        );
        assert_eq!(
            steps,
            [(ONSET - 1, Step::Yield), (ONSET + 109, Step::Resume)]
        );
        // And speech in the clear that stops short of 60 ms does not commit.
        let mut interruption = Interruption::new(Confirmation::EVALUATED);
        let steps = run(
            &mut interruption,
            &[
                (Heard::Over, ONSET),
                (Heard::Unknown, 30),
                (Heard::Clear, 5),
                (Heard::Still, 40),
            ],
        );
        assert_eq!(
            steps,
            [(ONSET - 1, Step::Yield), (ONSET + 74, Step::Resume)]
        );
    }

    #[test]
    fn with_nothing_to_yield_a_person_is_only_a_person() {
        let mut interruption = Interruption::new(Confirmation::EVALUATED);
        for _ in 0..100 {
            assert_eq!(interruption.block(Heard::Over, false), Step::Carry);
        }
        // A reply begins: the count starts from there.
        for run in 1..Confirmation::EVALUATED.early_onset_blocks {
            assert_eq!(interruption.block(Heard::Over, true), Step::Carry);
            assert_eq!(interruption.onset_run(), run);
        }
        assert_eq!(interruption.block(Heard::Over, true), Step::Yield);
        assert_eq!(interruption.onset_run(), 0);
        assert_eq!(interruption.yielded_blocks(), Some(0));
        interruption.clear();
        assert!(!interruption.yielded());
        assert_eq!(interruption.counts(), (1, 0, 0));
    }

    #[test]
    fn the_margin_is_the_person_against_the_lamp_once_both_were_heard() {
        let mut margin = Margin::default();
        assert_eq!(margin.db(), None);
        let mut own = evidence(0, Some((97, 10)));
        own.capture_level = level(-11.0);
        for _ in 0..50 {
            margin.observe(Heard::Own, &own);
        }
        assert_eq!((margin.own_db(), margin.db()), (Some(-11.0), None));
        let mut person = evidence(64, None);
        person.capture_level = level(-31.0);
        for _ in 0..50 {
            margin.observe(Heard::Clear, &person);
        }
        // As on the lamp where nothing was heard over its own voice.
        assert_eq!(margin.db(), Some(-20.0));
        // Other blocks say nothing about either level.
        for heard in [Heard::Over, Heard::Unknown, Heard::Under, Heard::Still] {
            margin.observe(heard, &evidence(0, None));
        }
        assert_eq!(margin.db(), Some(-20.0));
    }

    /// #828, `stop/heard`: a forced yield begins at the next block with
    /// something to yield, whatever is heard, and counts as heard plainly:
    /// quiet after it commits it (the lamp stops). It is dropped with
    /// nothing to yield, and not taken while a yield is in force.
    #[test]
    fn a_forced_yield_begins_at_the_next_block_and_quiet_commits_it() {
        let mut interruption = Interruption::new(Confirmation::EVALUATED);
        assert_eq!(interruption.observe(Heard::Own, false, true), Step::Carry);
        assert!(interruption.force_yield());
        assert_eq!(interruption.observe(Heard::Own, false, true), Step::Yield);
        assert!(interruption.yielded() && interruption.yield_was_forced());
        assert!(!interruption.force_yield(), "a yield is in force");
        let mut steps = Vec::new();
        for _ in 0..200 {
            steps.push(interruption.observe(Heard::Still, false, true));
            if !interruption.yielded() {
                break;
            }
        }
        assert_eq!(steps.last(), Some(&Step::Commit), "{steps:?}");
        // Asked with nothing to yield: dropped, not kept for later.
        let mut later = Interruption::new(Confirmation::EVALUATED);
        assert!(later.force_yield());
        assert_eq!(later.observe(Heard::Own, false, false), Step::Carry);
        assert!(!later.force_pending());
        assert_eq!(later.observe(Heard::Own, false, true), Step::Carry);
    }
}
