//! The lamp's face: what its 32-pixel ring shows, as a function of what the
//! lamp is doing, how loud the voice in play is, and time. Pure arithmetic:
//! no I/O, no clock of its own and no authority. The ring worker decides
//! whether a frame may be written at all; this only says what it would show.
//!
//! Every pattern here but the eye's is symmetric around the ring or moves
//! around it, so none depends on which pixel is at the top of the head. The
//! eye's look does: on lamp-4ace on 2026-10-14 pixel 0 was at the top and
//! they counted clockwise as a person facing the lamp sees them (pixels 8,
//! 16 and 24 at three, six and nine o'clock), and every angle here is
//! counted that way.
use lamp_msgs::{LightPattern, PatternKind};
use lamp_ring::{PIXEL_COUNT, Rgb};
use std::f32::consts::{PI, TAU};

/// What the lamp is doing, as far as its face is concerned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mood {
    /// Nobody has the lamp's attention: a slow breath.
    Resting,
    /// Someone is speaking to it: open, and moving with their voice.
    Listening,
    /// They have finished and nothing has been said back yet.
    Thinking,
    /// The lamp is speaking: the glow follows its own voice.
    Speaking,
    /// Someone is speaking to it while it cannot reach the internet
    /// (#830): one slow, dim, warm breath, so they see they were heard.
    Offline,
}

/// The lamp's own light: what its face is at rest. A lasting setting,
/// asked for by name ("make your light blue") and kept until asked again.
/// The faces of listening, thinking and speaking are the lamp's
/// expression and do not change with it; when they are over the ring
/// comes back to this.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lit {
    /// Off is not dark: see [`EMBER`].
    pub on: bool,
    /// The colour, each channel as a share of the brightest.
    tint: [f32; 3],
    /// How bright, from 1 to [`LEVELS`].
    level: u8,
}

/// How many levels a light has.
pub const LEVELS: u8 = 5;
/// The glow of each level as a share of the ceiling, and how much a slow
/// breath adds to it, so that a light that is on is never a still disc.
/// The first is the low breath the lamp rested in before it had a light
/// to set; the last reaches the ceiling at the top of a breath.
const LEVEL_GLOW: [(f32, f32); LEVELS as usize] = [
    (0.07, 0.06),
    (0.24, 0.06),
    (0.44, 0.07),
    (0.68, 0.08),
    (0.90, 0.10),
];
/// A light that is off still shows an ember, in amber: the ring at rest
/// is also the sign that the lamp can hear, and that sign is not the
/// lamp's to switch off because it was asked for darkness.
///
/// An ember is one pixel in [`EMBER_EVERY`] at [`EMBER_STEPS`] of red and
/// of green, the faintest amber the ring can show, and the rest dark. It
/// is counted in steps and not as a share of the ceiling, and it cannot
/// be the whole ring glowing more faintly: under the ceiling the lamp
/// runs with, the lowest light is already two or three steps of red and
/// one or two of green, and anything fainter is less than one step of
/// each, which the ring can only show as some pixels red and others
/// green. Seen on lamp-4ace on 2026-10-14 with the whole ring at 3 to 5
/// hundredths: a patchwork of pink and green.
const EMBER_STEPS: (f32, f32) = (3.0, 1.0);
const EMBER_EVERY: usize = 4;
/// A wanted value this far over a whole step is shown as that step on
/// every pixel: less than the least a pixel's rounding can add.
const WHOLE_STEP: f32 = 0.015;
const AMBER: [f32; 3] = [1.0, 0.55, 0.0];

impl Lit {
    /// The ten colours by name, as [`lamp_msgs::Light::TINTS`] has them.
    const TINTS: [(&'static str, [f32; 3]); 10] = lamp_msgs::Light::TINTS;

    /// A light of this colour and level, or `None` for a colour there is
    /// not or a level outside 1 to [`LEVELS`].
    pub fn of(on: bool, colour: &str, level: u8) -> Option<Self> {
        let tint = Self::TINTS
            .iter()
            .find_map(|(name, tint)| (*name == colour).then_some(*tint))?;
        (1..=LEVELS)
            .contains(&level)
            .then_some(Self { on, tint, level })
    }

    /// The light a lamp has before anyone has asked for one: on, warm, at
    /// its lowest. Exactly the face it rested in before lights could be set.
    pub const fn first() -> Self {
        Self {
            on: true,
            tint: AMBER,
            level: 1,
        }
    }

    /// The colours there are, in the order of [`Self::TINTS`].
    pub fn colours() -> [&'static str; 10] {
        Self::TINTS.map(|(name, _)| name)
    }

    /// One level dimmer, as a share of this light's glow: the next level
    /// down's over this one's, or half of the lowest (#1362's one dip).
    pub fn step_down_share(&self) -> f32 {
        let index = usize::from(self.level.clamp(1, LEVELS)) - 1;
        match index {
            0 => 0.5,
            _ => LEVEL_GLOW[index - 1].0 / LEVEL_GLOW[index].0,
        }
    }
}

/// The quietest and the loudest block that move the face, in dBFS.
const LEVEL_FLOOR_DB: f32 = -48.0;
const LEVEL_FULL_DB: f32 = -14.0;

/// How loud a block of speech is on the face's scale: 0 at or under
/// -48 dBFS, 255 at or over -14 dBFS, by its root mean square.
pub fn level_of(samples: &[i16]) -> u8 {
    if samples.is_empty() {
        return 0;
    }
    let power = samples
        .iter()
        .map(|sample| {
            let value = f32::from(*sample) / 32_768.0;
            value * value
        })
        .sum::<f32>()
        / samples.len() as f32;
    if power <= 0.0 {
        return 0;
    }
    let db = 10.0 * power.log10();
    let scaled = (db - LEVEL_FLOOR_DB) / (LEVEL_FULL_DB - LEVEL_FLOOR_DB);
    (scaled.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// A step longer than this between two frames is treated as this long: the
/// face eases on from where it was instead of jumping after a pause.
const LONGEST_STEP_US: u64 = 100_000;
const FIRST_STEP_US: u64 = 20_000;
/// How fast the shown colours follow the wanted ones.
const EASE_US: f32 = 70_000.0;
/// The voice level rises at once and falls slowly, as a mouth would.
const LEVEL_RISE_US: f32 = 20_000.0;
const LEVEL_FALL_US: f32 = 170_000.0;
/// How long the face takes to close and open again after a turn it let go.
const LET_GO_US: u64 = 900_000;

/// The eye's look follows the person this fast, so that it moves as an eye
/// does and never jumps when a face comes, goes or is found again.
const GAZE_US: f32 = 250_000.0;
/// A person this near the middle of the camera's picture (as a share of
/// it, either way) is looked at straight: the whole eye is open to them.
/// The look leans to their side from here, fully at [`GAZE_FULL`].
const GAZE_STRAIGHT: f32 = 0.06;
const GAZE_FULL: f32 = 0.22;
/// Looking to one side, the far side of the eye dims by this much at most;
/// the side toward the person keeps its light.
const LOOK_DIM: f32 = 0.75;
/// The strong eye ([`Face::eye_strong`]): it leans from a person this near
/// the middle, fully at [`GAZE_FULL_STRONG`], and its far side dims by
/// [`LOOK_DIM_STRONG`]. On lamp-4ace on 2026-10-15 the owner found the eye
/// too subtle: the head turns to a face, so the face sits near the middle
/// of the picture, inside [`GAZE_STRAIGHT`], and the eye rarely leant.
const GAZE_STRAIGHT_STRONG: f32 = 0.03;
const GAZE_FULL_STRONG: f32 = 0.14;
const LOOK_DIM_STRONG: f32 = 0.9;
/// The strong eye notices a person: when a face is seen after none for this
/// long, the eye brightens at once by [`NOTICE_GAIN`] of its light, and
/// settles back over [`NOTICE_US`], as eyes widen at someone who arrives.
const NOTICE_AFTER_US: u64 = 3_000_000;
const NOTICE_GAIN: f32 = 0.35;
const NOTICE_US: f32 = 300_000.0;
/// A blink at rest: this long, this far down, and the next one this many
/// microseconds later, from the first to the first plus the second, so
/// that blinks never come as a clock's would.
const BLINK_US: u64 = 150_000;
const BLINK_LOW: f32 = 0.25;
/// The countdown of a hand kept on the head (#711): from the stroke at one
/// second to sleep at three, as Controls counts them, the face dims to this
/// share of itself, slowly enough to be seen as a countdown.
const COUNTDOWN_FROM_US: f32 = 1_000_000.0;
const COUNTDOWN_TO_US: f32 = 3_000_000.0;
const COUNTDOWN_LOW: f32 = 0.15;
/// A stir at its name in the night's sleep (`LAMP_REST_THROUGH_SOUNDS=on`,
/// #1672, [`crate::ring_stir`]): at its full the resting face is this much
/// brighter than its night level, a little, as eyes half open; the first
/// level of light goes from about 2 to 3 steps of 24 to 3 to 5. Authored,
/// to be looked at on a lamp (CLAUDE.md rule 8).
pub const STIR_LIFT: f32 = 0.6;
/// After a tap's listening face at rest (#711), the face settles back to its
/// own light this slowly: most of the way in about half a second, so that it
/// eases and does not snap (First, #711).
const SETTLE_EASE_US: f32 = 170_000.0;
const SETTLE_US: u64 = 600_000;
/// A light asked to fade in (#1677, a skill's light going out when nobody
/// is there) is eased toward with a time of its fade over this: 98% of the
/// way by the fade's end, its last visible step about three quarters of the
/// way through. The ease is exponential, so the light falls as a share of
/// itself, which the eye reads as an even dimming, like falling asleep.
const FADE_EASES: f32 = 4.0;
/// How long a tap on the head is listened for at rest: the attention rule's
/// own window for a tap ([`crate::attention::TAPPED_BEFORE_US`]), so the
/// face is shown as long as a request may begin as a call. It runs from the
/// hum's first frame, after the voice has heard the tap and decided to hum
/// (not measured on a lamp, `docs/ring-choreography.md`). Speech that begins
/// in it is a turn, whose own listening face goes on from there.
pub const TAP_LISTENS_US: u64 = crate::attention::TAPPED_BEFORE_US;
const BLINK_EVERY_US: (u64, u64) = (7_000_000, 9_000_000);

/// One face. It keeps what is on the ring now, so that a change of mood, or
/// a resting frame after a spoken one, eases from one look into the other.
#[derive(Clone, Debug)]
pub struct Face {
    /// What is shown, per pixel and channel, as a share of the ceiling.
    shown: [[f32; 3]; PIXEL_COUNT],
    last_us: Option<u64>,
    level: f32,
    mood: Option<Mood>,
    /// When the lamp last let a turn go without answering it.
    let_go_us: Option<u64>,
    /// The lamp's own light, which is its face at rest.
    lit: Lit,
    /// The eye (task 34, #712), off unless set: listening, the face looks
    /// toward the person, and at rest it blinks now and then.
    eye: bool,
    /// The strong eye, with the eye only: it leans sooner and further, and
    /// notices a person who arrives (`LAMP_RING_EYE=strong`).
    strong: bool,
    /// When a face was last seen, and when the eye last noticed one.
    seen_us: Option<u64>,
    noticed_us: Option<u64>,
    /// Where the person is, as how far right and how far up of the middle
    /// of the camera's picture they are, seen from in front of the lamp,
    /// each from -0.5 to 0.5: `None` while no fresh face is seen.
    toward: Option<[f32; 2]>,
    /// The look as shown, easing toward `toward` (or straight ahead).
    gaze: [f32; 2],
    /// When the next blink at rest begins, and how many there have been.
    next_blink_us: Option<u64>,
    blinks: u64,
    /// How long a hand has been kept on the head ([`Face::hand`]).
    held_us: Option<u64>,
    /// Until when a tap on the head is listened for at rest, and the turn
    /// of the hum that answered it ([`Face::hum_frame`]).
    tapped_until_us: Option<u64>,
    hum_turn: Option<u64>,
    /// The accent with the lamp's gesture ([`accent_code`]) and when it began.
    accent: u8,
    accent_at_us: u64,
    /// How the lamp's own light moves at rest, and since when (#940 item 3,
    /// a skill's pattern): none, and it rests in its own slow breath.
    pattern: Option<(LightPattern, u64)>,
    /// How slowly, in µs, the face rests into its own light, when the light
    /// shown was asked to fade in (#1677): none, its own quick ease.
    fade_us: Option<u64>,
    /// When a light that faded in was let go: the face then settles back
    /// into its light as after a tap ([`SETTLE_EASE_US`] for
    /// [`SETTLE_US`]), waking rather than snapping on.
    woke_us: Option<u64>,
    /// The lamp's one breath from its inner state, its phase and pace now
    /// ([`Face::breath`]): none, and it breathes on the boot clock.
    breath: Option<(f64, f32)>,
    /// Whether a skill's light held tints the Speaking face
    /// (`LAMP_SKILL_LIGHT_SPEAKING=on`), and the light held now
    /// ([`Face::skill_light`]).
    tints_speaking: bool,
    held: Option<Lit>,
    /// How far a stir at its name has lifted the face at rest, 0 to 1
    /// ([`Face::stir`]).
    stir: f32,
}

impl Default for Face {
    fn default() -> Self {
        Self::new()
    }
}

impl Face {
    pub const fn new() -> Self {
        Self {
            shown: [[0.0; 3]; PIXEL_COUNT],
            last_us: None,
            level: 0.0,
            mood: None,
            let_go_us: None,
            lit: Lit::first(),
            eye: false,
            strong: false,
            seen_us: None,
            noticed_us: None,
            toward: None,
            gaze: [0.0; 2],
            next_blink_us: None,
            blinks: 0,
            held_us: None,
            tapped_until_us: None,
            hum_turn: None,
            accent: 0,
            accent_at_us: 0,
            pattern: None,
            fade_us: None,
            woke_us: None,
            breath: None,
            tints_speaking: false,
            held: None,
            stir: 0.0,
        }
    }

    /// How far a stir at its name has come in, 0 to 1, already eased
    /// ([`crate::ring_stir`], #1672): the resting face is lifted by up to
    /// [`STIR_LIFT`] of itself. Only the resting face on a light that is on:
    /// a light the person turned off stays its ember, and listening,
    /// thinking and speaking (the answer's full wake) are as always.
    pub fn stir(&mut self, share: f32) {
        self.stir = share.clamp(0.0, 1.0);
    }

    /// How long a hand has been kept on the head, as Controls counts it, or
    /// `None` (#711, [`crate::ring_head`]): from one second to three the face
    /// dims, a countdown to sleep; a hand let go before then lets it brighten
    /// again, eased like any other change.
    pub fn hand(&mut self, held_us: Option<u64>) {
        self.held_us = held_us;
    }

    /// A frame of the head tap's "hm?" (#711), of the turn `turn`, shown at
    /// `now_us`. The first frame of each hum starts the tap's window: until
    /// it ends ([`TAP_LISTENS_US`]) the resting face is the listening face,
    /// the one a call gets while the lamp waits for the rest, then it
    /// settles back to its own light over about half a second. Only the hum
    /// starts it, so a tap the voice does not answer gets no face (First on
    /// #705, 17 Oct 07:26). A turn's own faces are unchanged.
    pub fn hum_frame(&mut self, turn: u64, now_us: u64) {
        if self.hum_turn != Some(turn) {
            self.hum_turn = Some(turn);
            self.tapped_until_us = Some(now_us.saturating_add(TAP_LISTENS_US));
        }
    }

    /// Whether a resting frame at `now_us` shows the lamp's own light: not
    /// while a tap's listening face, or its settle back, is over it, nor
    /// while the face closes and opens again after a turn it let go.
    pub fn rests_on_its_light(&self, now_us: u64) -> bool {
        self.tapped_until_us
            .is_none_or(|until| now_us >= until.saturating_add(SETTLE_US))
            && self
                .let_go_us
                .is_none_or(|at| now_us >= at.saturating_add(LET_GO_US))
    }

    /// Show the accent `code` ([`accent_code`]) from `at_us` for [`ACCENT_US`].
    pub fn accent(&mut self, code: u8, at_us: u64) {
        self.accent = code;
        self.accent_at_us = at_us;
    }

    /// The lamp's one breath, from its inner state (#1670,
    /// [`crate::ring_mood`]): its phase now, from 0 to 1, and its pace; or
    /// `None` for the plain breath on the boot clock, as before. The resting
    /// breath takes its phase, so it breathes with the head; with the eye,
    /// blinks come as much sooner or later as the pace says.
    pub fn breath(&mut self, breath: Option<(f64, f32)>) {
        self.breath = breath;
    }

    /// Switch the skill's light on the Speaking face on or off
    /// (`LAMP_SKILL_LIGHT_SPEAKING=on`): off, a held light changes no turn's
    /// face, as before.
    pub fn tints_speaking(&mut self, on: bool) {
        self.tints_speaking = on;
    }

    /// The light a skill holds now, as the ring shows it over the person's
    /// ([`crate::ring_light::RingLight::held`]), or `None`. With
    /// [`Face::tints_speaking`] on, the Speaking face takes its colour, at
    /// [`SKILL_SPEAKING_LEVEL`]'s glow, so a light a skill holds through
    /// its words (Welcome back's warm glow) comes with them, as
    /// `docs/skills.md` means it to; given back, the next frame is plain
    /// Speaking again, eased like any other change.
    pub fn skill_light(&mut self, held: Option<Lit>) {
        self.held = held;
    }

    /// Switch the eye on or off ([`Face::eye`]'s field says what it does).
    pub fn eye(&mut self, on: bool) {
        self.eye = on;
    }

    /// Switch the strong eye on or off. It changes nothing without the eye.
    pub fn eye_strong(&mut self, on: bool) {
        self.strong = on;
    }

    /// Where the person the eye looks at is, as [`toward`] gives it, or
    /// `None` when no fresh face is seen. Only the eye uses it.
    pub fn toward(&mut self, at: Option<[f32; 2]>) {
        self.toward = at;
    }

    /// How open the eye is at rest now: 1, or [`BLINK_LOW`] during a blink.
    fn blink(&mut self, mood: Mood, now_us: u64) -> f32 {
        if mood != Mood::Resting {
            self.next_blink_us = None;
            return 1.0;
        }
        let pace = self.breath.map_or(1.0, |(_, pace)| pace);
        let spacing = |count: u64| {
            let spread = (count.wrapping_add(1).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 40)
                % BLINK_EVERY_US.1;
            // At a pace, blinks come as much sooner or later.
            ((BLINK_EVERY_US.0 + spread) as f32 / pace) as u64
        };
        let next = *self
            .next_blink_us
            .get_or_insert_with(|| now_us + spacing(self.blinks));
        if now_us < next {
            1.0
        } else if now_us - next < BLINK_US {
            BLINK_LOW
        } else {
            self.blinks += 1;
            self.next_blink_us = Some(now_us + spacing(self.blinks));
            1.0
        }
    }

    /// Set the lamp's own light. The ring eases into it like any other
    /// change of face.
    pub fn light(&mut self, lit: Lit) {
        self.lit = lit;
    }

    pub fn lit(&self) -> Lit {
        self.lit
    }

    /// How the lamp's own light moves at rest, from when, or not at all
    /// ([`crate::ring_light::RingLight::pattern`]): between the pattern's
    /// low and the light's level, eased like any other change. A turn's
    /// faces, a tap's and an ember are unchanged.
    pub fn pattern(&mut self, pattern: Option<(LightPattern, u64)>) {
        self.pattern = pattern;
    }

    /// How slowly the face rests into the lamp's own light, from `now_us`
    /// (#1677, [`crate::ring_light::RingLight::fade`]): over `fade_ms` while
    /// the light shown asks it, as something falling asleep, and its own
    /// quick ease without. A turn's faces, a tap's and the face closing after
    /// a turn ease as ever. Once such a light is let go, the face settles
    /// back over about half a second, waking.
    pub fn fade(&mut self, fade_ms: Option<u32>, now_us: u64) {
        let fade_us = fade_ms.map(|ms| u64::from(ms) * 1_000);
        if fade_us.is_some() {
            self.woke_us = None;
        } else if self.fade_us.is_some() {
            self.woke_us = Some(now_us);
        }
        self.fade_us = fade_us;
    }

    /// The frame for `now_us`. `level` is the voice in play on the scale of
    /// [`level_of`]: the person's while listening, the lamp's own while
    /// speaking. No channel of the result exceeds `ceiling`.
    pub fn render(
        &mut self,
        mood: Mood,
        level: u8,
        now_us: u64,
        ceiling: u8,
    ) -> [Rgb; PIXEL_COUNT] {
        // The first frame counts as one ordinary step from dark, so that the
        // very first thing shown is already visible.
        let step_us = self.last_us.map_or(FIRST_STEP_US, |last| {
            now_us.saturating_sub(last).min(LONGEST_STEP_US)
        }) as f32;
        self.last_us = Some(now_us);
        let wanted_level = f32::from(level) / 255.0;
        let pace = if wanted_level > self.level {
            LEVEL_RISE_US
        } else {
            LEVEL_FALL_US
        };
        self.level += (wanted_level - self.level) * follow(step_us, pace);
        // At rest after a tap on the head (#711): the listening face for the
        // window, then a slow settle back to rest.
        let since_tap_window = self
            .tapped_until_us
            .filter(|_| mood == Mood::Resting)
            .map(|until| now_us.checked_sub(until));
        let mood = match since_tap_window {
            Some(None) => Mood::Listening,
            _ => mood,
        };
        let settling = matches!(since_tap_window, Some(Some(since)) if since < SETTLE_US);
        // Thinking that ends in rest, with nothing said: it was not for the
        // lamp, or it had no answer. The face closes for a moment, as eyes
        // do, before it rests again.
        if self.mood == Some(Mood::Thinking) && mood == Mood::Resting {
            self.let_go_us = Some(now_us);
        }
        self.mood = Some(mood);
        let since_let_go = self
            .let_go_us
            .map(|at| now_us.saturating_sub(at))
            .filter(|since| mood == Mood::Resting && *since < LET_GO_US);
        let open = since_let_go.map_or(1.0, |since| {
            // Closed by a fifth of the way through, open again by the end.
            let through = since as f32 / LET_GO_US as f32;
            if through < 0.2 {
                1.0 - through / 0.2
            } else {
                (through - 0.2) / 0.8
            }
        });
        // The eye: a look eased toward the person, as an angle around the
        // ring and how far it leans, and a blink at rest.
        let (look_to, blink) = if self.eye {
            let wanted = self.toward.unwrap_or([0.0; 2]);
            let pace = follow(step_us, GAZE_US);
            for (shown, wanted) in self.gaze.iter_mut().zip(wanted) {
                *shown += (wanted - *shown) * pace;
            }
            let [right, up] = self.gaze;
            let (straight, full, dim) = if self.strong {
                (GAZE_STRAIGHT_STRONG, GAZE_FULL_STRONG, LOOK_DIM_STRONG)
            } else {
                (GAZE_STRAIGHT, GAZE_FULL, LOOK_DIM)
            };
            let lean = ((right.hypot(up) - straight) / (full - straight)).clamp(0.0, 1.0);
            let look_to = (lean > 0.0).then(|| (right.atan2(up).rem_euclid(TAU), dim * lean));
            (look_to, self.blink(mood, now_us))
        } else {
            (None, 1.0)
        };
        // The strong eye notices a person who arrives, listening or at rest.
        let mut notice = 0.0;
        if self.eye && self.strong {
            if self.toward.is_some() {
                if self
                    .seen_us
                    .is_none_or(|seen| now_us.saturating_sub(seen) >= NOTICE_AFTER_US)
                {
                    self.noticed_us = Some(now_us);
                }
                self.seen_us = Some(now_us);
            }
            if matches!(mood, Mood::Listening | Mood::Resting) {
                notice = self.noticed_us.map_or(0.0, |at| {
                    (-(now_us.saturating_sub(at) as f32) / NOTICE_US).exp()
                });
            }
        }
        let stirred = if mood == Mood::Resting && self.lit.on {
            1.0 + STIR_LIFT * self.stir
        } else {
            1.0
        };
        let brighten = (1.0 + NOTICE_GAIN * notice) * stirred;
        // A hand kept on the head (#711): the countdown to sleep dims it.
        let countdown = self.held_us.map_or(0.0, |held| {
            ((held as f32 - COUNTDOWN_FROM_US) / (COUNTDOWN_TO_US - COUNTDOWN_FROM_US))
                .clamp(0.0, 1.0)
        });
        let open = open * blink * (1.0 - (1.0 - COUNTDOWN_LOW) * countdown);
        let seconds = (now_us % 3_600_000_000) as f32 / 1_000_000.0;
        // The resting breath, from 0 to 1: the lamp's one breath where its
        // inner state keeps it, else the plain one on the boot clock.
        let breath = resting_breath(self.breath, now_us);
        // At rest on its own light: slowly into a light asked to fade in
        // (#1677), and back with a tap's settle once it is let go.
        let own = mood == Mood::Resting && !settling && since_let_go.is_none();
        let waking = self
            .woke_us
            .is_some_and(|at| own && now_us.saturating_sub(at) < SETTLE_US);
        let pace = match self.fade_us.filter(|_| own) {
            Some(fade_us) => fade_us as f32 / FADE_EASES,
            None if settling || waking => SETTLE_EASE_US,
            None => EASE_US,
        };
        let ease = follow(step_us, pace);
        let accent_t = (self.accent != 0
            && now_us >= self.accent_at_us
            && now_us - self.accent_at_us < ACCENT_US)
            .then(|| (now_us - self.accent_at_us) as f32 / ACCENT_US as f32);
        // A skill's pattern: its kind, how many of its periods since it
        // began, and its low.
        let motion = self.pattern.map(|(pattern, since)| {
            let period_us = f64::from(pattern.period_ms.max(1)) * 1_000.0;
            let cycles = (now_us.saturating_sub(since) as f64 / period_us) as f32;
            (pattern.kind, cycles, pattern.low)
        });
        let mut frame = [Rgb::BLACK; PIXEL_COUNT];
        for (pixel, (shown, out)) in self.shown.iter_mut().zip(&mut frame).enumerate() {
            let angle = TAU * pixel as f32 / PIXEL_COUNT as f32;
            // A pulse that has gone around is the light as it rests, with
            // its own slow breath, not a still disc (the reviewer on #1494).
            let moves = |(kind, cycles, _): &(PatternKind, f32, u8)| {
                mood == Mood::Resting
                    && self.lit.on
                    && !(*kind == PatternKind::Pulse && *cycles >= 1.0)
            };
            let held = self.held.filter(|_| self.tints_speaking);
            let wanted = match motion.filter(moves) {
                Some(motion) => moving(self.lit, motion, pixel),
                None => look(
                    mood, self.level, seconds, breath, angle, self.lit, held, ceiling, look_to,
                ),
            };
            let scale = accent_t.map_or([1.0; 3], |t| accent_scale(self.accent, t, angle));
            for ((channel, target), scale) in shown.iter_mut().zip(wanted).zip(scale) {
                *channel += (target * brighten * open * scale - *channel) * ease;
            }
            // A share of the ceiling rarely lands on a whole step. Each pixel
            // rounds at its own fixed point, so a level between two steps is
            // shown as some pixels on the higher one: a dim breath stays
            // smooth instead of stepping, and nothing flickers in time.
            // Each colour rounds at a different pixel, so that a pixel is
            // not one step up in every colour at once.
            let step = |share: f32, shift: usize| {
                let order = pixel_order((pixel + shift) % PIXEL_COUNT);
                let dither = f32::from(order) / PIXEL_COUNT as f32;
                let value = (share.clamp(0.0, 1.0) * f32::from(ceiling) + dither).floor();
                value.min(f32::from(ceiling)) as u8
            };
            *out = Rgb::new(step(shown[0], 0), step(shown[1], 11), step(shown[2], 21));
        }
        frame
    }
}

/// How far in the resting breath is at `now_us`, from 0 (out) to 1 (in):
/// at the lamp's one breath's phase where Mood keeps it (#1670,
/// [`Face::breath`]), else the boot clock's seconds within the hour over
/// 5.2 s, as it always was. Expression's head takes the same
/// (`lamp_expression::Breath::share_at`), so the two breathe as one.
pub fn resting_breath(breath: Option<(f64, f32)>, now_us: u64) -> f32 {
    match breath {
        Some((phase, _)) => 0.5 - 0.5 * (TAU * phase as f32).cos(),
        None => wave((now_us % 3_600_000_000) as f32 / 1_000_000.0, 5.2, 0.0, 0.0),
    }
}

/// How long an accent with a gesture lasts.
pub const ACCENT_US: u64 = 1_600_000;

/// The face's accent for one of the body's gestures (#940, the V1 emotions
/// kept on 2026-10-16): 0 for a gesture without one.
pub fn accent_code(gesture: &str) -> u8 {
    match gesture {
        "happy" => 1,
        "caring" => 2,
        "surprised" => 3,
        "confused" => 4,
        "curious" => 5,
        "acknowledge" => 6,
        "sleepy" => 7,
        _ => 0,
    }
}

/// How much each colour of a pixel at `angle` (0 at the top, clockwise) is
/// scaled, `t` of the way (0 to 1) through the accent `code`. V1's
/// coloured blinks per emotion are left out: these stay in the lamp's own
/// light and only brighten, warm, dim or move it a little.
fn accent_scale(code: u8, t: f32, angle: f32) -> [f32; 3] {
    let swell = (std::f32::consts::PI * t).sin();
    match code {
        // Happy: a warm sparkle round the ring.
        1 => {
            let twinkle = 0.5 + 0.5 * (6.0 * angle + 20.0 * t).cos();
            let lift = 1.0 + 0.35 * swell * (0.6 + 0.4 * twinkle);
            [lift * 1.08, lift, lift * 0.85]
        }
        // Caring: a little dimmer and warmer, slowly.
        2 => {
            let dim = 1.0 - 0.15 * swell;
            [dim, dim * (1.0 - 0.1 * swell), dim * (1.0 - 0.3 * swell)]
        }
        // Surprised: one quick brightening that falls away.
        3 => [1.0 + 0.6 * (-6.0 * t).exp(); 3],
        // Confused: a dim spot that swings once along the lower ring.
        4 => {
            let at = std::f32::consts::PI + (TAU * t).sin() * std::f32::consts::FRAC_PI_2;
            let distance =
                (angle - at + std::f32::consts::PI).rem_euclid(TAU) - std::f32::consts::PI;
            [1.0 - 0.5 * swell * (-(distance * distance) / 0.3).exp(); 3]
        }
        // Curious: the top of the ring brightens, as if looking up.
        5 => [1.0 + 0.3 * swell * angle.cos().max(0.0); 3],
        // Acknowledge: one short brighter blink at the start.
        6 => [1.0 + 0.4 * (std::f32::consts::PI * (t * 4.0).min(1.0)).sin(); 3],
        // Sleepy: a slow dimming.
        7 => [1.0 - 0.4 * swell; 3],
        _ => [1.0; 3],
    }
}

/// How much of the way to go in one step for a given pace.
fn follow(step_us: f32, pace_us: f32) -> f32 {
    1.0 - (-step_us / pace_us).exp()
}

/// The pixel's place in a bit-reversed order of the ring, so that pixels
/// that round up first are spread evenly around it.
fn pixel_order(pixel: usize) -> u8 {
    ((pixel as u8) << 3).reverse_bits()
}

/// A wave between 0 and 1 with the given period, at an angle on the ring.
fn wave(seconds: f32, period: f32, lobes: f32, angle: f32) -> f32 {
    0.5 - 0.5 * (TAU * seconds / period - lobes * angle).cos()
}

/// Where the person is for the eye ([`Face::toward`]): the middle of a face
/// in the camera's picture, its edges as fractions of the picture with
/// (0, 0) at the top left, as how far right and up of the middle it is seen
/// from in front of the lamp. The camera sits in the middle of the ring
/// looking out, so a person on the left of its picture is on the lamp's
/// left, which someone facing the lamp sees on their right.
pub fn toward(xmin: f32, ymin: f32, xmax: f32, ymax: f32) -> [f32; 2] {
    let (across, down) = ((xmin + xmax) / 2.0, (ymin + ymax) / 2.0);
    [
        (0.5 - across).clamp(-0.5, 0.5),
        (0.5 - down).clamp(-0.5, 0.5),
    ]
}

/// A pixel of the lamp's own light moving in a skill's pattern (#940 item
/// 3): `cycles` of its period since it was first drawn, its colour never
/// shifting.
/// - Breathe: the whole ring on a sine between the glow of `low` and the
///   light's level, from its low.
/// - Candle: each pixel wanders at most a fifth under the level (V1's
///   candle, `CANDLE_FLICKER_MIN` 0.8), on two slow waves of two and three
///   lobes, so neighbours stay close and the shimmer has no beat.
/// - Pulse: at the level, one soft arc a sixth of the ring wide, up to a
///   third brighter, goes once around over the period; then the light as
///   it rests ([`look`]), drawn by [`Face::render`].
fn moving(lit: Lit, (kind, cycles, low): (PatternKind, f32, u8), pixel: usize) -> [f32; 3] {
    let glow = |level: u8| LEVEL_GLOW[usize::from(level.clamp(1, LEVELS)) - 1].0;
    let top = glow(lit.level);
    let angle = TAU * pixel as f32 / PIXEL_COUNT as f32;
    let shown = match kind {
        PatternKind::Breathe => {
            let bottom = glow(low);
            bottom + (top - bottom) * (0.5 - 0.5 * (TAU * cycles).cos())
        }
        PatternKind::Candle => {
            let wander = 0.5
                + 0.3 * (TAU * cycles + 2.0 * angle).sin()
                + 0.2 * (TAU * cycles / 1.618 - 3.0 * angle + 1.1).sin();
            top * (1.0 - CANDLE_DIP * wander.clamp(0.0, 1.0))
        }
        PatternKind::Pulse if cycles < 1.0 => {
            let apart = (angle - TAU * cycles).rem_euclid(TAU);
            let apart = apart.min(TAU - apart);
            let width = TAU / 6.0;
            let near = if apart < width {
                0.5 + 0.5 * (PI * apart / width).cos()
            } else {
                0.0
            };
            top + ((top * PULSE_RISE).min(1.0) - top) * near
        }
        PatternKind::Pulse => top,
    };
    lit.tint.map(|channel| channel * shown)
}

/// How far under its level a candle dips at most: V1's 0.8 of its colour.
const CANDLE_DIP: f32 = 0.2;
/// How much brighter than the level a pulse's arc is at its middle.
const PULSE_RISE: f32 = 1.35;

/// The Speaking face's own warm, and its least glow, with the voice quiet.
const SPEAKING_TINT: [f32; 3] = [1.0, 0.72, 0.30];
const SPEAKING_LEAST: f32 = 0.26;
/// A skill's light held over the Speaking face (`LAMP_SKILL_LIGHT_SPEAKING`):
/// this share of the face's colour is the light's, the rest Speaking's own
/// warm, so the face is still the lamp speaking, in the light's colour.
const SKILL_TINT_SHARE: f32 = 0.6;
/// The level a held light tints the Speaking face at, whatever its own. Over
/// it the voice would have only the gap to the ceiling, 0.90 to 1.0 at
/// level 5, the idle turn's own swing, and the face would read as a lit
/// disc, not a mouth (Seventh on #2253). Under it, the ring cannot
/// tell a level the skill named from one `lamp-skills` filled in from the
/// light shown (`crates/skills/src/glow.rs`, `asked`): Welcome back's warm
/// glow names none and comes at the person's level, 1 on most lamps, and
/// under Speaking's own least glow a level 1 or 2 is not seen. The middle,
/// as `glow.rs`'s `LEVEL_UNKNOWN`.
pub const SKILL_SPEAKING_LEVEL: u8 = 3;

/// The wanted colour of one pixel, each channel as a share of the ceiling.
/// `look_to`: the eye's look, as the angle around the ring toward the
/// person and how much the far side dims, from 0 to 1. `breath`: where the
/// resting breath is, from 0 to 1 ([`Face::breath`]). `held`: a skill's
/// light that tints the Speaking face ([`Face::skill_light`]), with the
/// switch on.
#[allow(clippy::too_many_arguments)]
fn look(
    mood: Mood,
    level: f32,
    seconds: f32,
    breath: f32,
    angle: f32,
    lit: Lit,
    held: Option<Lit>,
    ceiling: u8,
    look_to: Option<(f32, f32)>,
) -> [f32; 3] {
    let (tint, glow) = match mood {
        Mood::Resting => {
            // The lamp's own light, in a slow breath, with a soft brighter
            // patch that wanders around the ring so that two breaths never
            // look the same. Its lowest level is low on purpose: on this
            // ring a few steps of 255 are already a clear glow, and V1
            // rested at one. A light that is off is an ember of the same.
            let patch = wave(seconds, 23.0, 1.0, angle).powi(4);
            if lit.on {
                let (glow, breathes) = LEVEL_GLOW[usize::from(lit.level.clamp(1, LEVELS)) - 1];
                // The patch is for the dim levels: a bright ring needs
                // nothing to show that it is alive.
                let patches = if lit.level == 1 { 0.03 } else { 0.0 };
                (lit.tint, glow + breathes * breath + patches * patch)
            } else {
                let pixel = (angle * PIXEL_COUNT as f32 / TAU).round() as usize;
                if !pixel.is_multiple_of(EMBER_EVERY) {
                    return [0.0; 3];
                }
                let share =
                    |steps: f32| ((steps + WHOLE_STEP) / f32::from(ceiling.max(1))).min(1.0);
                return [share(EMBER_STEPS.0), share(EMBER_STEPS.1), 0.0];
            }
        }
        Mood::Listening => {
            // Cool and open at once, brighter with the person's voice, with
            // a slow three-lobed shimmer so it is alive when they pause.
            let shimmer = wave(seconds, 1.7, 3.0, angle);
            let glow = 0.42 + 0.40 * level + 0.07 * shimmer;
            // The eye looks toward the person: the side toward them keeps
            // its light and the far side dims, as much as the look leans.
            let looking = look_to.map_or(1.0, |(toward, dim)| {
                let near = (0.5 + 0.5 * (angle - toward).cos()).powi(2);
                1.0 - dim * (1.0 - near)
            });
            ([0.50, 0.85, 1.0], glow * looking)
        }
        Mood::Offline => {
            // One slow, dim breath in warm amber, about 2.5 s a breath and
            // never more than a third of listening's least: no red, no
            // flashing, the same all round (#830).
            let breath = wave(seconds, 2.5, 0.0, 0.0);
            ([1.0, 0.55, 0.12], 0.04 + 0.10 * breath)
        }
        Mood::Thinking => {
            // An amber point circling the ring with a fading tail behind it.
            let head = TAU * (seconds / 1.15).fract();
            let behind = (head - angle).rem_euclid(TAU);
            ([1.0, 0.52, 0.04], 0.14 + 0.80 * (-behind / 0.75).exp())
        }
        Mood::Speaking => {
            // Warm, and as bright as the lamp's own voice is loud, with a
            // slow two-lobed turn so that it is never a flat disc.
            let turn = wave(seconds, 3.1, 2.0, angle);
            match held.filter(|held| held.on) {
                // A skill's light held with the words: its colour mixed into
                // the face, and the face's least glow that of the middle
                // level whatever the light's, so the colour shows over
                // Speaking's own least and the voice still moves it over
                // most of plain Speaking's span (Seventh on #2253).
                Some(held) => {
                    let least = LEVEL_GLOW[usize::from(SKILL_SPEAKING_LEVEL) - 1]
                        .0
                        .max(SPEAKING_LEAST);
                    let tint: [f32; 3] = std::array::from_fn(|channel| {
                        SPEAKING_TINT[channel]
                            + (held.tint[channel] - SPEAKING_TINT[channel]) * SKILL_TINT_SHARE
                    });
                    (
                        tint,
                        (least + (1.0 - least) * level.powf(0.8)) * (0.90 + 0.10 * turn),
                    )
                }
                None => (
                    SPEAKING_TINT,
                    (SPEAKING_LEAST + 0.74 * level.powf(0.8)) * (0.90 + 0.10 * turn),
                ),
            }
        }
    };
    tint.map(|channel| channel * glow)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brightness(frame: &[Rgb; PIXEL_COUNT]) -> u32 {
        frame
            .iter()
            .map(|pixel| u32::from(pixel.red) + u32::from(pixel.green) + u32::from(pixel.blue))
            .sum()
    }

    /// Run a mood for a while at 50 frames a second and return the frames.
    fn play(
        face: &mut Face,
        mood: Mood,
        level: u8,
        from_us: u64,
        frames: u64,
    ) -> Vec<[Rgb; PIXEL_COUNT]> {
        (0..frames)
            .map(|frame| face.render(mood, level, from_us + frame * 20_000, 48))
            .collect()
    }

    /// #711: a hand kept on the head dims the face from one second to three,
    /// a countdown to sleep, and letting go brightens it again. Two faces with
    /// the same light, one with the hand, are compared frame by frame.
    #[test]
    fn a_hand_kept_on_the_head_dims_the_face_from_one_second_to_three() {
        let at = |seconds: f32| 4_000_000_000 + (seconds * 1e6) as u64;
        let mut lit = Face::new();
        lit.light(Lit::of(true, "warm", 5).unwrap());
        let mut held = lit.clone();
        let mut ratio = |hand: Option<u64>, from: f32| {
            held.hand(hand);
            let plain = play(&mut lit, Mood::Resting, 0, at(from), 50);
            let dimmed = play(&mut held, Mood::Resting, 0, at(from), 50);
            brightness(dimmed.last().unwrap()) as f32 / brightness(plain.last().unwrap()) as f32
        };
        // Under a second it is only a touch: nothing changes.
        assert!((ratio(Some(800_000), 0.0) - 1.0).abs() < 0.02);
        // Half way, about half way down; at three seconds, the low glow.
        let half = ratio(Some(2_000_000), 1.0);
        assert!((0.5..0.65).contains(&half), "{half}");
        let low = ratio(Some(3_000_000), 2.0);
        assert!((0.10..0.22).contains(&low), "{low}");
        assert!(low > 0.0, "never dark: the face still shows it hears");
        // Let go: back to the light.
        assert!((ratio(None, 3.0) - 1.0).abs() < 0.02);
    }

    /// #1672: a stir at its name lifts the resting face a little from its
    /// night level (the first level of light, as at night), by up to
    /// [`STIR_LIFT`] of itself, and lets it back down; never past the
    /// ceiling. Two faces, one stirred, compared at the same frames.
    #[test]
    fn a_stir_lifts_the_resting_face_a_little_and_lets_it_back() {
        let at = |seconds: f32| 4_000_000_000 + (seconds * 1e6) as u64;
        let mut plain = Face::new();
        let mut stirred = plain.clone();
        let mut ratio = |share: f32, from: f32| {
            stirred.stir(share);
            let plain = play(&mut plain, Mood::Resting, 0, at(from), 50);
            let lifted = play(&mut stirred, Mood::Resting, 0, at(from), 50);
            brightness(lifted.last().unwrap()) as f32 / brightness(plain.last().unwrap()) as f32
        };
        assert!((ratio(0.0, 0.0) - 1.0).abs() < 0.02);
        let full = ratio(1.0, 1.0);
        assert!((1.4..1.8).contains(&full), "{full}");
        let half = ratio(0.5, 2.0);
        assert!((1.15..1.45).contains(&half), "{half}");
        assert!((ratio(0.0, 3.0) - 1.0).abs() < 0.02);
        // The brightest light is held at its ceiling, never past it.
        let mut bright = Face::new();
        bright.light(Lit::of(true, "warm", 5).unwrap());
        bright.stir(1.0);
        for frame in play(&mut bright, Mood::Resting, 0, at(4.0), 300) {
            assert!(
                frame
                    .iter()
                    .all(|pixel| pixel.red.max(pixel.green).max(pixel.blue) <= 48)
            );
        }
    }

    /// #1672: a light the person turned off stays its ember through a
    /// stir, and the answer's full wake (speaking), listening and thinking
    /// are the faces they always were, frame for frame.
    #[test]
    fn a_stir_leaves_an_ember_and_every_turns_face_as_they_were() {
        let at = |seconds: f32| 4_000_000_000 + (seconds * 1e6) as u64;
        let mut off = Face::new();
        off.light(Lit::of(false, "warm", 1).unwrap());
        let mut off_stirred = off.clone();
        off_stirred.stir(1.0);
        assert_eq!(
            play(&mut off, Mood::Resting, 0, at(0.0), 100),
            play(&mut off_stirred, Mood::Resting, 0, at(0.0), 100)
        );
        for mood in [Mood::Speaking, Mood::Listening, Mood::Thinking] {
            let mut plain = Face::new();
            let mut stirred = Face::new();
            stirred.stir(1.0);
            assert_eq!(
                play(&mut plain, mood, 160, at(1.0), 100),
                play(&mut stirred, mood, 160, at(1.0), 100),
                "{mood:?}"
            );
        }
    }

    /// #711: a tap the voice answers with its hum shows the listening face
    /// at rest for the window, then settles back to rest over about half a
    /// second, never snapping. A turn's own faces are unchanged.
    #[test]
    fn a_hummed_tap_listens_at_rest_for_its_window_and_settles_back_slowly() {
        let at = |seconds: f32| 4_000_000_000 + (seconds * 1e6) as u64;
        let apart = |a: &[Rgb; PIXEL_COUNT], b: &[Rgb; PIXEL_COUNT]| -> u32 {
            a.iter()
                .zip(b)
                .map(|(a, b)| {
                    u32::from(a.red.abs_diff(b.red))
                        + u32::from(a.green.abs_diff(b.green))
                        + u32::from(a.blue.abs_diff(b.blue))
                })
                .sum()
        };
        let mut resting = Face::new();
        resting.light(Lit::of(true, "warm", 5).unwrap());
        let (mut tapped, mut listening) = (resting.clone(), resting.clone());
        for face in [&mut resting, &mut tapped, &mut listening] {
            play(face, Mood::Resting, 0, at(0.0), 50);
        }
        // The window from the hum's first frame: the resting face is the
        // listening face. More frames of the same hum don't stretch it.
        tapped.hum_frame(7, at(3.0) - TAP_LISTENS_US);
        tapped.hum_frame(7, at(2.5));
        let during = play(&mut tapped, Mood::Resting, 0, at(1.0), 100);
        let heard = play(&mut listening, Mood::Listening, 0, at(1.0), 100);
        let rest = play(&mut resting, Mood::Resting, 0, at(1.0), 100);
        assert_eq!(during.last(), heard.last());
        let wide = apart(heard.last().unwrap(), rest.last().unwrap());
        assert!(wide > 0, "the listening face is not the resting one");
        // A turn's face is its own, tapped or not.
        let mut turn = tapped.clone();
        let mut plain = listening.clone();
        assert_eq!(
            play(&mut turn, Mood::Speaking, 100, at(3.0) - 100_000, 3),
            play(&mut plain, Mood::Speaking, 100, at(3.0) - 100_000, 3)
        );
        // After it: back to rest, slowly. A tenth of a second in, well short
        // of rest; by 0.6 s, there.
        let after = play(&mut tapped, Mood::Resting, 0, at(3.0), 30);
        let rest = play(&mut resting, Mood::Resting, 0, at(3.0), 30);
        let left = |frame: usize| apart(&after[frame], &rest[frame]) as f32 / wide as f32;
        assert!(left(5) > 0.45, "{}", left(5));
        assert!(left(29) < 0.06, "{}", left(29));
        assert!((1..30).all(|frame| left(frame) <= left(frame - 1) + 0.01));
    }

    /// First on #705, 17 Oct 07:26: the face only with the hum (that a
    /// refused tap sends no hum frame is the coordinator's test,
    /// `only_the_hums_own_ring_frames_are_marked_listening`). A hum keeps
    /// the face until its window and settle are over; each hum that plays
    /// gets its own window.
    #[test]
    fn each_hum_opens_its_own_window_at_rest_until_its_settle() {
        let at = |seconds: f32| 4_000_000_000 + (seconds * 1e6) as u64;
        let mut hummed = Face::new();
        hummed.light(Lit::of(true, "warm", 5).unwrap());
        assert!(hummed.rests_on_its_light(at(2.0)));
        hummed.hum_frame(7, at(3.0));
        assert!(!hummed.rests_on_its_light(at(3.0)));
        let end = at(3.0) + TAP_LISTENS_US + SETTLE_US;
        assert!(!hummed.rests_on_its_light(end - 1));
        assert!(hummed.rests_on_its_light(end));
        // The next hum, 30 s on, is a new window.
        hummed.hum_frame(9, at(33.0));
        assert!(!hummed.rests_on_its_light(at(33.0) + TAP_LISTENS_US));
    }

    #[test]
    fn a_block_of_speech_is_scored_by_its_loudness() {
        assert_eq!(level_of(&[]), 0);
        assert_eq!(level_of(&[0; 240]), 0);
        // About -60 dBFS: under the floor.
        assert_eq!(level_of(&[33; 240]), 0);
        // Full scale: over the top.
        assert_eq!(level_of(&[i16::MAX; 240]), 255);
        // About -30 dBFS sits near the middle and louder is higher.
        let quiet = level_of(&[1_036; 240]);
        let loud = level_of(&[3_277; 240]);
        assert!((100..160).contains(&quiet), "{quiet}");
        assert!(loud > quiet);
    }

    #[test]
    fn offline_is_a_slow_dim_warm_breath_far_under_listening() {
        let mut face = Face::new();
        // Past the change of mood's easing.
        let offline = play(&mut face, Mood::Offline, 255, 1, 250);
        let mut listening_face = Face::new();
        let listening = play(&mut listening_face, Mood::Listening, 0, 1, 250);
        let brightest =
            |frames: &[[Rgb; PIXEL_COUNT]]| frames[50..].iter().map(brightness).max().unwrap();
        let dimmest =
            |frames: &[[Rgb; PIXEL_COUNT]]| frames[50..].iter().map(brightness).min().unwrap();
        // At most a third of listening's least, even with a loud voice.
        assert!(
            3 * brightest(&offline) <= dimmest(&listening) + 3 * 32,
            "{} {}",
            brightest(&offline),
            dimmest(&listening)
        );
        // Warm: red over green over blue, never blue-led; and alive.
        for frame in &offline[50..] {
            for pixel in frame {
                assert!(
                    pixel.red >= pixel.green && pixel.green >= pixel.blue,
                    "{pixel:?}"
                );
            }
        }
        assert!(brightest(&offline) > dimmest(&offline));
        // Slow: from one 20 ms frame to the next it never jumps.
        for pair in offline[50..].windows(2) {
            assert!(brightness(&pair[0]).abs_diff(brightness(&pair[1])) <= 32);
        }
    }

    /// The total light of a frame, every channel of every pixel.
    fn total(pixels: &[Rgb; PIXEL_COUNT]) -> u32 {
        pixels
            .iter()
            .map(|p| u32::from(p.red) + u32::from(p.green) + u32::from(p.blue))
            .sum()
    }

    #[test]
    fn an_accent_changes_the_speaking_face_for_its_moment_and_then_is_gone() {
        // Speaking at a steady level, the same frame with and without an
        // accent, sampled at the accent's middle and well after it.
        let speak = |accent: Option<(u8, u64)>, at_us: u64| {
            let mut face = Face::new();
            if let Some((code, from)) = accent {
                face.accent(code, from);
            }
            let mut last = [Rgb::BLACK; PIXEL_COUNT];
            for step in 0..=(at_us / 20_000) {
                last = face.render(Mood::Speaking, 160, 1 + step * 20_000, 120);
            }
            total(&last)
        };
        let from = 2_000_000;
        let middle = from + ACCENT_US / 2;
        let plain = speak(None, middle);
        assert!(speak(Some((accent_code("happy"), from)), middle) > plain * 11 / 10);
        assert!(speak(Some((accent_code("sleepy"), from)), middle) < plain * 9 / 10);
        assert!(speak(Some((accent_code("caring"), from)), middle) < plain);
        // Over: as if there had been none.
        let after = from + ACCENT_US * 3;
        let plain_after = speak(None, after);
        let happy_after = speak(Some((accent_code("happy"), from)), after);
        assert!(happy_after.abs_diff(plain_after) <= plain_after / 50);
        // A gesture without an accent changes nothing.
        assert_eq!(accent_code("nod"), 0);
        assert_eq!(speak(Some((0, from)), middle), plain);
    }

    #[test]
    fn an_accent_never_exceeds_the_ceiling() {
        for code in 1..=7 {
            let mut face = Face::new();
            face.accent(code, 0);
            for frame in 0..100u64 {
                let pixels = face.render(Mood::Speaking, 255, 1 + frame * 20_000, 48);
                assert!(
                    pixels
                        .iter()
                        .all(|pixel| pixel.red <= 48 && pixel.green <= 48 && pixel.blue <= 48)
                );
            }
        }
    }

    #[test]
    fn no_channel_ever_exceeds_the_ceiling() {
        for ceiling in [0, 1, 12, 48, 120] {
            let mut face = Face::new();
            for mood in [
                Mood::Resting,
                Mood::Listening,
                Mood::Thinking,
                Mood::Speaking,
                Mood::Offline,
            ] {
                for frame in 0..200u64 {
                    let pixels = face.render(mood, 255, 1 + frame * 20_000, ceiling);
                    assert!(pixels.iter().all(|pixel| pixel.red <= ceiling
                        && pixel.green <= ceiling
                        && pixel.blue <= ceiling));
                }
            }
        }
    }

    #[test]
    fn a_first_frame_is_already_visible_and_eases_in() {
        let mut face = Face::new();
        let frames = play(&mut face, Mood::Listening, 0, 1_000, 30);
        assert!(brightness(&frames[0]) > 32);
        assert!(brightness(&frames[0]) < brightness(&frames[29]) / 2);
        assert!(
            frames
                .windows(2)
                .take(10)
                .all(|pair| brightness(&pair[1]) >= brightness(&pair[0]))
        );
        assert!(brightness(&frames[29]) > 32 * 30);
    }

    #[test]
    fn the_resting_breath_is_dim_and_moves_in_small_steps() {
        let mut face = Face::new();
        play(&mut face, Mood::Resting, 0, 1_000, 100);
        let frames = play(&mut face, Mood::Resting, 0, 2_001_000, 600);
        let levels: Vec<u32> = frames.iter().map(brightness).collect();
        let (low, high) = (levels.iter().min().unwrap(), levels.iter().max().unwrap());
        // It breathes: a clear difference between its lowest and highest.
        assert!(*high as f32 > *low as f32 * 1.4, "{low} {high}");
        // And never bright: well under a third of the ceiling on every pixel.
        assert!(*high < 32 * 3 * 16, "{high}");
        // From one frame to the next the whole ring changes by a few steps
        // at most, of the 4,608 it has: no visible jump.
        assert!(
            levels
                .windows(2)
                .all(|pair| pair[0].abs_diff(pair[1]) <= 12)
        );
    }

    /// The whole ring's red, green and blue over a few seconds of rest.
    fn rested(face: &mut Face) -> [u32; 3] {
        play(face, Mood::Resting, 0, 1_000, 100);
        let mut sums = [0u32; 3];
        for frame in play(face, Mood::Resting, 0, 2_001_000, 300) {
            for pixel in frame {
                sums[0] += u32::from(pixel.red);
                sums[1] += u32::from(pixel.green);
                sums[2] += u32::from(pixel.blue);
            }
        }
        sums
    }

    /// A face resting in `colour` at `level`, moving in `kind` from 0 µs.
    fn moving_face(colour: &str, level: u8, kind: PatternKind, period_ms: u32, low: u8) -> Face {
        let mut face = Face::new();
        face.light(Lit::of(true, colour, level).unwrap());
        face.pattern(Some((
            LightPattern {
                kind,
                period_ms,
                low,
            },
            0,
        )));
        face
    }

    /// #940 item 3: a breath goes smoothly from the low level to the light's
    /// level and back, once a period, starting low.
    #[test]
    fn a_breathing_light_eases_between_its_low_and_its_level_once_a_period() {
        let mut face = moving_face("white", 4, PatternKind::Breathe, 6_000, 1);
        let frames: Vec<u32> = play(&mut face, Mood::Resting, 0, 0, 600)
            .iter()
            .map(brightness)
            .collect();
        let mean = |level: u8| {
            let mut steady = Face::new();
            steady.light(Lit::of(true, "white", level).unwrap());
            let frames = play(&mut steady, Mood::Resting, 0, 0, 300);
            frames[100..].iter().map(brightness).sum::<u32>() / 200
        };
        let (low, high) = (mean(1), mean(4));
        let second = &frames[300..];
        let (least, most) = (*second.iter().min().unwrap(), *second.iter().max().unwrap());
        assert!(least * 10 <= low * 13, "{least} against level 1's {low}");
        assert!(
            most * 10 >= high * 9 && most * 10 <= high * 11,
            "{most} against {high}"
        );
        // Low at the start of each period, at its level half way.
        assert!(frames[300] * 10 <= low * 13 + 10, "{}", frames[300]);
        assert!(frames[450] * 10 >= high * 9, "{}", frames[450]);
        // Smooth: no frame moves more than 3% of the way.
        let range = most - least;
        for pair in frames[20..].windows(2) {
            assert!(
                pair[0].abs_diff(pair[1]) * 100 <= range * 3,
                "{pair:?} of {range}"
            );
        }
    }

    /// A candle at the product's ceiling (24) and the ring's 25 ms frames:
    /// each pixel stays within a fifth under its level, as V1's did, its
    /// colour that of the light, its neighbours close, and it still moves
    /// (the reviewer on #1494: a swing from the lowest level broke dim
    /// pixels' colour about five times a second).
    #[test]
    fn a_candle_flickers_shallow_with_close_neighbours_and_keeps_its_colour() {
        for level in 3..=LEVELS {
            let mut face = moving_face("orange", level, PatternKind::Candle, 1_000, 1);
            let frames: Vec<[Rgb; PIXEL_COUNT]> = (0..120)
                .map(|frame| face.render(Mood::Resting, 0, frame * 25_000, 24))
                .collect();
            let top = LEVEL_GLOW[usize::from(level) - 1].0 * 24.0;
            let (floor, ceil) = ((top * 0.8).floor() - 1.0, top.ceil() + 1.0);
            let mut apart = 0.0;
            let mut pairs = 0.0;
            for frame in &frames[20..] {
                for (pixel, shown) in frame.iter().enumerate() {
                    let red = f32::from(shown.red);
                    assert!((floor..=ceil).contains(&red), "{level}: {shown:?}");
                    // Orange's green is a third of its red, within the step
                    // each of the two channels may round by.
                    let green = f32::from(shown.green);
                    assert!((green - red * 0.34).abs() <= 1.4, "{level}: {shown:?}");
                    let next = frame[(pixel + 1) % PIXEL_COUNT];
                    apart += (red - f32::from(next.red)).abs();
                    pairs += 1.0;
                }
            }
            assert!(
                apart / pairs <= 2.0,
                "{level}: neighbours {}",
                apart / pairs
            );
            let one: Vec<u8> = frames[20..].iter().map(|frame| frame[0].red).collect();
            assert!(
                one.iter().max().unwrap() > one.iter().min().unwrap(),
                "{level}: {one:?}"
            );
        }
    }

    /// A pulse: at the light's level, one soft brighter arc goes once
    /// around the ring, never under the level; then the steady light.
    #[test]
    fn a_pulse_goes_once_around_over_its_level_and_then_is_steady() {
        let mut face = moving_face("blue", 3, PatternKind::Pulse, 4_000, 1);
        let frames = play(&mut face, Mood::Resting, 0, 0, 260);
        let mut plain = Face::new();
        plain.light(Lit::of(true, "blue", 3).unwrap());
        let resting = play(&mut plain, Mood::Resting, 0, 0, 260);
        let steady = (LEVEL_GLOW[2].0 * 48.0) as u8;
        let brightest = |frame: &[Rgb; PIXEL_COUNT]| {
            (0..PIXEL_COUNT)
                .max_by_key(|&pixel| frame[pixel].blue)
                .unwrap()
        };
        // Its head moves on around the ring through the period.
        let heads: Vec<usize> = [20, 60, 100, 140, 180]
            .iter()
            .map(|&at| brightest(&frames[at]))
            .collect();
        for pair in heads.windows(2) {
            let ahead = (pair[1] + PIXEL_COUNT - pair[0]) % PIXEL_COUNT;
            assert!((1..PIXEL_COUNT / 2).contains(&ahead), "{heads:?}");
        }
        let peak = frames[20..190]
            .iter()
            .flat_map(|frame| frame.iter().map(|pixel| pixel.blue))
            .max()
            .unwrap();
        assert!(peak >= steady + 5, "{peak} over {steady}");
        for frame in &frames[20..] {
            assert!(
                frame.iter().all(|pixel| pixel.blue + 1 >= steady),
                "{frame:?}"
            );
        }
        // Done: the light as it rests with no pattern, its own slow breath
        // and all, frame for frame once the face has eased back.
        assert_eq!(frames[230..], resting[230..]);
    }

    /// A turn's faces and an ember never move with a pattern.
    #[test]
    fn a_pattern_moves_only_the_lamps_own_light_at_rest() {
        let mut plain = Face::new();
        plain.light(Lit::of(true, "white", 4).unwrap());
        let mut breathing = moving_face("white", 4, PatternKind::Breathe, 6_000, 1);
        for mood in [Mood::Listening, Mood::Thinking, Mood::Speaking] {
            assert_eq!(
                play(&mut plain, mood, 120, 0, 50),
                play(&mut breathing, mood, 120, 0, 50),
                "{mood:?}"
            );
        }
        let mut ember = Face::new();
        ember.light(Lit::of(false, "white", 4).unwrap());
        let mut off = moving_face("white", 4, PatternKind::Candle, 1_500, 1);
        off.light(Lit::of(false, "white", 4).unwrap());
        assert_eq!(
            play(&mut ember, Mood::Resting, 0, 0, 50),
            play(&mut off, Mood::Resting, 0, 0, 50)
        );
    }

    #[test]
    fn the_lamps_own_light_is_its_face_at_rest_in_the_colour_and_at_the_level_asked_for() {
        // Before anyone asks, and asked for the first light, it rests as
        // it always did: amber, with no blue at all.
        let mut plain = Face::new();
        let mut first = Face::new();
        first.light(Lit::of(true, "warm", 1).unwrap());
        assert_eq!(Lit::of(true, "warm", 1), Some(Lit::first()));
        let was = rested(&mut plain);
        assert_eq!(rested(&mut first), was);
        assert!(was[0] > was[1] && was[1] > 0 && was[2] == 0, "{was:?}");
        // Blue is blue, and each level is brighter than the one below.
        let mut before = 0;
        for level in 1..=LEVELS {
            let mut face = Face::new();
            face.light(Lit::of(true, "blue", level).unwrap());
            assert_eq!(face.lit(), Lit::of(true, "blue", level).unwrap());
            let [red, green, blue] = rested(&mut face);
            assert!(
                red == 0 && blue > green * 3,
                "{level}: {red} {green} {blue}"
            );
            assert!(blue > before, "{level}: {blue} after {before}");
            before = blue;
        }
        // The brightest reaches the ceiling and never passes it.
        let mut bright = Face::new();
        bright.light(Lit::of(true, "white", LEVELS).unwrap());
        let frames = play(&mut bright, Mood::Resting, 0, 1_000, 600);
        let peak = frames
            .iter()
            .flat_map(|frame| frame.iter().map(|pixel| pixel.red))
            .max()
            .unwrap();
        assert!((44..=48).contains(&peak), "{peak}");
        // Every colour there is has a look of its own.
        let mut looks = Vec::new();
        for colour in Lit::colours() {
            let mut face = Face::new();
            face.light(Lit::of(true, colour, 3).unwrap());
            let look = rested(&mut face);
            assert!(look.iter().sum::<u32>() > 0, "{colour}");
            assert!(!looks.contains(&look), "{colour}");
            looks.push(look);
        }
        // And they are the ten the message can name.
        let mut named = Lit::colours();
        let mut messaged = lamp_msgs::Light::COLOURS;
        named.sort_unstable();
        messaged.sort_unstable();
        assert_eq!(named, messaged);
        // A colour or a level there is not is no light.
        assert_eq!(Lit::of(true, "mauve", 3), None);
        assert_eq!(Lit::of(true, "blue", 0), None);
        assert_eq!(Lit::of(true, "blue", LEVELS + 1), None);
    }

    /// #1677: a light asked to fade in is rested into over its fade, as
    /// something falling asleep, never in one step; a turn over it is as
    /// quick as ever; let go, the face wakes back into its light in about
    /// half a second, not at once.
    #[test]
    fn a_light_asked_to_fade_in_is_rested_into_slowly_and_woken_from_in_half_a_second() {
        let (warm, out) = (
            Lit::of(true, "warm", 3).unwrap(),
            Lit::of(false, "warm", 3).unwrap(),
        );
        // Each frame beside a face resting at warm 3 and one at the ember,
        // drawn at the same times, so the breath is the same in all three.
        let mut lit = Face::new();
        lit.light(warm);
        let mut ember = Face::new();
        ember.light(out);
        let mut face = Face::new();
        face.light(warm);
        let frame = |face: &mut Face, at_us: u64| total(&face.render(Mood::Resting, 0, at_us, 48));
        for step in 0..100u64 {
            for face in [&mut lit, &mut ember, &mut face] {
                frame(face, step * 20_000);
            }
        }
        // It goes out over 30 s from 2 s.
        face.light(out);
        face.fade(Some(30_000), 2_000_000);
        let mut fell = Vec::new();
        for step in 0..1_500u64 {
            let at = 2_000_000 + step * 20_000;
            fell.push((
                frame(&mut lit, at),
                frame(&mut ember, at),
                frame(&mut face, at),
            ));
        }
        let share = |(lit, ember, face): (u32, u32, u32)| {
            (face as f32 - ember as f32) / (lit as f32 - ember as f32)
        };
        // Most of the light is there after a second, about a quarter after
        // ten, nearly none by the end: an exponential over a quarter of the
        // fade.
        assert!(share(fell[50]) > 0.8, "{:?}", fell[50]);
        assert!((0.15..0.4).contains(&share(fell[500])), "{:?}", fell[500]);
        assert!(share(fell[1_499]) < 0.05, "{:?}", fell[1_499]);
        // Never a step: no frame falls by a tenth of the light from the one
        // before. Its own quick ease takes a quarter in the first frame.
        let most = fell
            .windows(2)
            .map(|pair| share(pair[0]) - share(pair[1]))
            .fold(0.0f32, f32::max);
        assert!(most < 0.1, "{most}");
        // A turn's face over it comes as quickly as ever.
        let mut plain = Face::new();
        assert_eq!(
            play(&mut face, Mood::Listening, 120, 32_000_000, 80).last(),
            play(&mut plain, Mood::Listening, 120, 32_000_000, 80).last(),
        );
        // Back to rest, out still, and let go at 60 s: it wakes into its
        // light, more slowly than a snap, and is there within the second.
        for step in 0..1_320u64 {
            frame(&mut face, 33_600_000 + step * 20_000);
        }
        face.light(warm);
        face.fade(None, 60_000_000);
        let mut woke = Vec::new();
        for step in 0..50u64 {
            let at = 60_000_000 + step * 20_000;
            woke.push((
                frame(&mut lit, at),
                frame(&mut ember, at),
                frame(&mut face, at),
            ));
        }
        assert!(share(woke[0]) < 0.2, "{:?}", woke[0]);
        assert!(share(woke[5]) < 0.6, "{:?}", woke[5]);
        assert!(share(woke[49]) > 0.9, "{:?}", woke[49]);
        // Once woken, a change eases as quickly as ever: a quarter of the
        // way in its first frame.
        face.light(out);
        let at = 61_020_000;
        let next = (
            frame(&mut lit, at),
            frame(&mut ember, at),
            frame(&mut face, at),
        );
        assert!(share(next) < 0.85, "{next:?}");
    }

    #[test]
    fn a_light_that_is_off_is_an_ember_and_the_faces_of_a_turn_do_not_change_with_it() {
        let mut off = Face::new();
        off.light(Lit::of(false, "blue", 5).unwrap());
        let ember = rested(&mut off);
        let mut lowest = Face::new();
        let low = rested(&mut lowest);
        // Dimmer than the lowest light, amber whatever colour it would be,
        // and not dark: the ring at rest is also the sign the lamp hears.
        let total = |sums: [u32; 3]| sums.iter().sum::<u32>();
        assert!(
            total(ember) > 0 && total(ember) * 2 < total(low),
            "{ember:?} {low:?}"
        );
        assert!(ember[0] > ember[1] && ember[2] == 0, "{ember:?}");
        // A few pixels that are each amber, not a whole ring too faint to
        // have a colour: one in four is lit, with red and green both in
        // it, and the others are dark. Under the ceiling the lamp runs with.
        let mut dark = Face::new();
        dark.light(Lit::of(false, "blue", 5).unwrap());
        let settled = (0..200u64)
            .map(|frame| dark.render(Mood::Resting, 0, 1_000 + frame * 25_000, 24))
            .last()
            .unwrap();
        for (pixel, shown) in settled.iter().enumerate() {
            if pixel % 4 == 0 {
                assert_eq!(*shown, Rgb::new(3, 1, 0), "{pixel}");
            } else {
                assert_eq!(*shown, Rgb::BLACK, "{pixel}");
            }
        }
        // Listening, thinking and speaking are the lamp's expression: the
        // same whatever its light.
        for mood in [Mood::Listening, Mood::Thinking, Mood::Speaking] {
            let mut plain = Face::new();
            let mut blue = Face::new();
            blue.light(Lit::of(true, "blue", 5).unwrap());
            assert_eq!(
                play(&mut plain, mood, 120, 1_000, 80).last(),
                play(&mut blue, mood, 120, 1_000, 80).last(),
                "{mood:?}"
            );
        }
        // Coming back to rest, the ring eases into its light and does not
        // jump there.
        let mut face = Face::new();
        face.light(Lit::of(true, "blue", 5).unwrap());
        let spoke = play(&mut face, Mood::Speaking, 200, 1_000, 60);
        let rest = play(&mut face, Mood::Resting, 0, 1_201_000, 3);
        let reds = |frame: &[Rgb; PIXEL_COUNT]| frame.iter().map(|p| u32::from(p.red)).sum::<u32>();
        assert!(
            reds(&rest[0]) > reds(spoke.last().unwrap()) / 2,
            "it jumped"
        );
        let later = play(&mut face, Mood::Resting, 0, 1_261_000, 60);
        assert_eq!(reds(later.last().unwrap()), 0);
    }

    #[test]
    fn listening_and_speaking_follow_the_voice() {
        for mood in [Mood::Listening, Mood::Speaking] {
            let mut face = Face::new();
            let quiet = play(&mut face, mood, 0, 1_000, 60);
            let loud = play(&mut face, mood, 255, 1_201_000, 60);
            let after = play(&mut face, mood, 0, 2_401_000, 60);
            let (quiet, loud, after) = (
                brightness(quiet.last().unwrap()),
                brightness(loud.last().unwrap()),
                brightness(after.last().unwrap()),
            );
            assert!(loud as f32 > quiet as f32 * 1.5, "{mood:?} {quiet} {loud}");
            assert!(
                (after as f32) < loud as f32 * 0.75,
                "{mood:?} {after} {loud}"
            );
        }
    }

    #[test]
    fn the_voice_shows_within_two_frames_and_fades_over_several() {
        let mut face = Face::new();
        play(&mut face, Mood::Speaking, 0, 1_000, 60);
        let before = brightness(&face.render(Mood::Speaking, 0, 1_201_000, 48));
        face.render(Mood::Speaking, 255, 1_221_000, 48);
        let risen = brightness(&face.render(Mood::Speaking, 255, 1_241_000, 48));
        assert!(risen as f32 > before as f32 * 1.25, "{before} {risen}");
        let peak = brightness(
            play(&mut face, Mood::Speaking, 255, 1_261_000, 30)
                .last()
                .unwrap(),
        );
        let one_later = brightness(&face.render(Mood::Speaking, 0, 1_881_000, 48));
        assert!(one_later as f32 > peak as f32 * 0.8, "{peak} {one_later}");
    }

    #[test]
    fn thinking_is_a_point_that_travels_around_the_ring() {
        let mut face = Face::new();
        let frames = play(&mut face, Mood::Thinking, 0, 1_000, 120);
        let brightest = |frame: &[Rgb; PIXEL_COUNT]| {
            (0..PIXEL_COUNT)
                .max_by_key(|pixel| frame[*pixel].red)
                .unwrap()
        };
        let places: std::collections::BTreeSet<usize> =
            frames[20..].iter().map(brightest).collect();
        // Over two seconds the bright point has been on most of the ring.
        assert!(places.len() > 24, "{}", places.len());
        // At any moment most of the ring is much dimmer than the point.
        let frame = frames[60];
        let peak = frame[brightest(&frame)].red;
        let dim = frame.iter().filter(|pixel| pixel.red < peak / 2).count();
        assert!(dim >= 20, "{dim}");
    }

    #[test]
    fn a_change_of_mood_eases_instead_of_jumping() {
        let mut face = Face::new();
        let speaking = play(&mut face, Mood::Speaking, 255, 1_000, 60);
        let resting = play(&mut face, Mood::Resting, 0, 1_201_000, 60);
        let from = brightness(speaking.last().unwrap());
        let first = brightness(&resting[0]);
        let last = brightness(resting.last().unwrap());
        // The first resting frame is still close to the spoken one.
        assert!(first as f32 > from as f32 * 0.6, "{from} {first}");
        assert!(last < first / 2, "{first} {last}");
    }

    #[test]
    fn a_turn_let_go_without_an_answer_closes_the_face_for_a_moment() {
        let rest = |face: &mut Face, from_us: u64| {
            play(face, Mood::Resting, 0, from_us, 60)
                .iter()
                .map(brightness)
                .collect::<Vec<u32>>()
        };
        // Thinking, then rest with nothing said in between.
        let mut face = Face::new();
        play(&mut face, Mood::Thinking, 0, 1_000, 100);
        let let_go = rest(&mut face, 2_001_000);
        // The same moments, but it spoke before it rested: no such closing.
        let mut face = Face::new();
        play(&mut face, Mood::Thinking, 0, 1_000, 50);
        play(&mut face, Mood::Speaking, 120, 1_001_000, 50);
        let answered = rest(&mut face, 2_001_000);
        let darkest = *let_go[..25].iter().min().unwrap();
        // Nearly dark within half a second, which an answered turn never is.
        assert!(
            darkest * 4 < *answered[..25].iter().min().unwrap(),
            "{let_go:?} {answered:?}"
        );
        // And back to the ordinary resting level a second later.
        let (after, ordinary) = (let_go[59], answered[59]);
        assert!(
            after.abs_diff(ordinary) * 4 <= ordinary,
            "{after} {ordinary}"
        );
    }

    #[test]
    fn a_long_gap_between_frames_does_not_jump() {
        let mut face = Face::new();
        play(&mut face, Mood::Speaking, 255, 1_000, 60);
        let before = brightness(&face.render(Mood::Speaking, 255, 1_201_000, 48));
        // Ten seconds with no frame, then a resting one.
        let after = brightness(&face.render(Mood::Resting, 0, 11_201_000, 48));
        assert!(after as f32 > before as f32 * 0.2, "{before} {after}");
    }

    #[test]
    fn a_level_between_two_steps_lights_some_pixels_one_step_higher() {
        let orders: std::collections::BTreeSet<u8> = (0..PIXEL_COUNT).map(pixel_order).collect();
        assert_eq!(orders.len(), PIXEL_COUNT);
        assert_eq!(*orders.iter().max().unwrap() as usize, PIXEL_COUNT - 1);
        // Neighbours on the ring round at points far apart.
        assert!(
            (0..PIXEL_COUNT - 1)
                .all(|pixel| pixel_order(pixel).abs_diff(pixel_order(pixel + 1)) >= 8)
        );
    }

    fn glow(pixel: &Rgb) -> u32 {
        u32::from(pixel.red) + u32::from(pixel.green) + u32::from(pixel.blue)
    }

    /// The light on a quarter of the ring around one pixel.
    fn around(frame: &[Rgb; PIXEL_COUNT], middle: usize) -> u32 {
        (0..9)
            .map(|offset| glow(&frame[(middle + PIXEL_COUNT + offset - 4) % PIXEL_COUNT]))
            .sum()
    }

    /// A person on the left of the camera's picture, one in its middle, and
    /// one on its right, as the eyes would see them.
    const ON_THE_LEFT: [f32; 4] = [0.05, 0.40, 0.25, 0.60];
    const IN_THE_MIDDLE: [f32; 4] = [0.42, 0.40, 0.58, 0.60];
    const ON_THE_RIGHT: [f32; 4] = [0.75, 0.40, 0.95, 0.60];

    fn at(face: [f32; 4]) -> Option<[f32; 2]> {
        Some(toward(face[0], face[1], face[2], face[3]))
    }

    #[test]
    fn without_the_eye_every_frame_is_as_before_whatever_is_seen() {
        let (mut before, mut now) = (Face::new(), Face::new());
        now.toward(at(ON_THE_LEFT));
        now.eye(false);
        for (mood, from) in [
            (Mood::Resting, 1_000),
            (Mood::Listening, 21_000_000),
            (Mood::Thinking, 23_000_000),
            (Mood::Speaking, 25_000_000),
            (Mood::Resting, 27_000_000),
        ] {
            assert_eq!(
                play(&mut before, mood, 140, from, 100),
                play(&mut now, mood, 140, from, 100),
                "{mood:?}"
            );
        }
    }

    #[test]
    fn listening_the_eye_looks_toward_the_person_and_leaves_them_the_light() {
        let mut face = Face::new();
        face.eye(true);
        face.toward(at(ON_THE_LEFT));
        let frames = play(&mut face, Mood::Listening, 120, 1_000, 100);
        let last = frames.last().unwrap();
        // On the lamp's left, which is three o'clock seen from in front: pixel
        // 8. The far side, nine o'clock, is dimmer; top and bottom between.
        let (near, far) = (around(last, 8), around(last, 24));
        assert!(near > far * 2, "{near} {far}");
        assert!(around(last, 0) < near && around(last, 0) > far);
        // The side toward the person is as bright as without the eye.
        let mut plain = Face::new();
        let plain = play(&mut plain, Mood::Listening, 120, 1_000, 100);
        let plain = around(plain.last().unwrap(), 8);
        assert!(near * 10 >= plain * 9, "{near} {plain}");
        // A person on the right of the picture is looked at the other way.
        face.toward(at(ON_THE_RIGHT));
        let frames = play(&mut face, Mood::Listening, 120, 2_001_000, 100);
        let last = frames.last().unwrap();
        assert!(around(last, 24) > around(last, 8) * 2);
    }

    #[test]
    fn a_person_straight_ahead_or_none_has_the_whole_eye() {
        let mut plain = Face::new();
        let plain = play(&mut plain, Mood::Listening, 120, 1_000, 100);
        for seen in [at(IN_THE_MIDDLE), None] {
            let mut face = Face::new();
            face.eye(true);
            face.toward(seen);
            assert_eq!(play(&mut face, Mood::Listening, 120, 1_000, 100), plain);
        }
    }

    #[test]
    fn the_look_eases_from_one_side_to_the_other_and_never_jumps() {
        let mut face = Face::new();
        face.eye(true);
        face.toward(at(ON_THE_LEFT));
        play(&mut face, Mood::Listening, 120, 1_000, 100);
        face.toward(at(ON_THE_RIGHT));
        let frames = play(&mut face, Mood::Listening, 120, 2_001_000, 100);
        // A tenth of a second on, it still leans the old way.
        assert!(around(&frames[5], 8) > around(&frames[5], 24));
        // From frame to frame no pixel changes by much.
        for pair in frames.windows(2) {
            for (before, after) in pair[0].iter().zip(&pair[1]) {
                assert!(
                    glow(before).abs_diff(glow(after)) <= 12,
                    "{before:?} {after:?}"
                );
            }
        }
        assert!(around(&frames[99], 24) > around(&frames[99], 8) * 2);
    }

    #[test]
    fn at_rest_the_eye_blinks_now_and_then_and_briefly() {
        let mut face = Face::new();
        face.light(Lit::of(true, "white", 3).unwrap());
        face.eye(true);
        // Forty seconds at fifty frames a second, after the first, in which
        // the face eases in from dark.
        let frames = play(&mut face, Mood::Resting, 0, 1_000, 2_050);
        let lights: Vec<u32> = frames[50..].iter().map(brightness).collect();
        let mut sorted = lights.clone();
        sorted.sort_unstable();
        let usual = sorted[sorted.len() / 2];
        let dim: Vec<bool> = lights.iter().map(|light| *light * 10 < usual * 7).collect();
        let blinks = dim.windows(2).filter(|pair| !pair[0] && pair[1]).count();
        // Every 7 to 16 s: between two and five in forty seconds.
        assert!((2..=5).contains(&blinks), "{blinks}");
        // Each under a third of a second, eased in and out.
        let longest = dim.split(|dim| !dim).map(<[bool]>::len).max().unwrap_or(0);
        assert!(longest <= 16, "{longest}");
        // Without the eye, no blink.
        let mut plain = Face::new();
        plain.light(Lit::of(true, "white", 3).unwrap());
        let plain: Vec<u32> = play(&mut plain, Mood::Resting, 0, 1_000, 2_050)[50..]
            .iter()
            .map(brightness)
            .collect();
        assert!(plain.iter().all(|light| *light * 10 >= usual * 7));
    }

    /// A person a little to the lamp's left of the middle of the picture,
    /// where the head has turned toward them and stopped short.
    const NEAR_THE_MIDDLE: [f32; 4] = [0.40, 0.40, 0.50, 0.60];

    fn strong() -> Face {
        let mut face = Face::new();
        face.eye(true);
        face.eye_strong(true);
        face
    }

    #[test]
    fn the_strong_eye_leans_toward_a_person_the_plain_eye_looks_straight_at() {
        let run = |strong: bool, seen: Option<[f32; 2]>| {
            let mut face = Face::new();
            face.eye(true);
            face.eye_strong(strong);
            face.toward(seen);
            *play(&mut face, Mood::Listening, 120, 1_000, 100)
                .last()
                .unwrap()
        };
        // Near the middle the plain eye is the whole eye, as with nobody.
        assert_eq!(run(false, at(NEAR_THE_MIDDLE)), run(false, None));
        // The strong eye leans: its far side dims, its near side keeps its
        // light.
        let (leaning, whole) = (run(true, at(NEAR_THE_MIDDLE)), run(true, None));
        assert!(around(&leaning, 24) * 10 < around(&whole, 24) * 9);
        assert!(around(&leaning, 8) * 10 >= around(&whole, 8) * 9);
        // Further off, its far side is dimmer than the plain eye's.
        assert!(around(&run(true, at(ON_THE_LEFT)), 24) < around(&run(false, at(ON_THE_LEFT)), 24));
    }

    #[test]
    fn the_strong_eye_notices_a_person_who_arrives_once_and_settles() {
        // The strong eye beside the plain one, frame for frame, so that the
        // resting breath is the same in both.
        let mut face = strong();
        let mut plain = Face::new();
        plain.eye(true);
        for each in [&mut face, &mut plain] {
            each.light(Lit::of(true, "white", 3).unwrap());
        }
        let mut both = |seen: Option<[f32; 2]>, from: u64, frames: u64| {
            face.toward(seen);
            plain.toward(seen);
            let strong = play(&mut face, Mood::Resting, 0, from, frames);
            let plain = play(&mut plain, Mood::Resting, 0, from, frames);
            strong
                .iter()
                .zip(&plain)
                .map(|(strong, plain)| brightness(strong) as f32 / brightness(plain).max(1) as f32)
                .collect::<Vec<f32>>()
        };
        // Nobody for five seconds: the two are the same.
        let settled = both(None, 1_000, 250);
        assert!(
            settled[200..]
                .iter()
                .all(|ratio| (ratio - 1.0).abs() < 0.02)
        );
        // Someone arrives: brighter at once, the same again within two seconds.
        let arrived = both(at(IN_THE_MIDDLE), 5_001_000, 100);
        let peak = arrived[..15].iter().cloned().fold(0.0, f32::max);
        assert!(peak > 1.2, "{peak}");
        assert!((arrived[99] - 1.0).abs() < 0.03, "{}", arrived[99]);
        // Gone for a moment and back: no second notice.
        both(None, 7_001_000, 50);
        let again = both(at(IN_THE_MIDDLE), 8_001_000, 30);
        assert!(again.iter().all(|ratio| *ratio < 1.05), "{again:?}");
        // Gone for longer than three seconds, then back: noticed again.
        both(None, 8_601_000, 200);
        let back = both(at(IN_THE_MIDDLE), 12_601_000, 15);
        assert!(back.iter().cloned().fold(0.0, f32::max) > 1.2);
    }

    #[test]
    fn the_strong_eye_without_the_eye_changes_nothing() {
        let (mut before, mut now) = (Face::new(), Face::new());
        now.eye_strong(true);
        now.toward(at(NEAR_THE_MIDDLE));
        for (mood, from) in [
            (Mood::Resting, 1_000),
            (Mood::Listening, 21_000_000),
            (Mood::Speaking, 25_000_000),
        ] {
            assert_eq!(
                play(&mut before, mood, 140, from, 100),
                play(&mut now, mood, 140, from, 100),
                "{mood:?}"
            );
        }
    }

    #[test]
    fn a_face_is_placed_as_seen_from_in_front() {
        let [right, up] = toward(
            ON_THE_LEFT[0],
            ON_THE_LEFT[1],
            ON_THE_LEFT[2],
            ON_THE_LEFT[3],
        );
        assert!(right > 0.3 && up.abs() < 0.01);
        let [right, up] = toward(0.4, 0.0, 0.6, 0.2);
        assert!(right.abs() < 0.01 && up > 0.3);
    }

    /// #1670: with no breath from Mood the resting breath is the old code's,
    /// `wave(seconds, 5.2, 0.0, 0.0)` on the boot clock's seconds within the
    /// hour, at every moment of an hour and across its turn; and a face never
    /// told is the face told `None`, frame for frame, at rest and with the
    /// eye.
    #[test]
    fn with_no_breath_from_mood_the_face_is_as_before() {
        for step in 0..200_000u64 {
            let now_us = 3_500_000_000 + step * 1_013;
            let old = {
                let seconds = (now_us % 3_600_000_000) as f32 / 1_000_000.0;
                0.5 - 0.5 * (TAU * seconds / 5.2).cos()
            };
            assert_eq!(resting_breath(None, now_us), old, "{now_us}");
        }
        let mut before = Face::new();
        let mut now = Face::new();
        for face in [&mut before, &mut now] {
            face.light(Lit::of(true, "white", 3).unwrap());
            face.eye(true);
        }
        for frame in 0..1_500u64 {
            let at = 1_000 + frame * 20_000;
            now.breath(None);
            assert_eq!(
                before.render(Mood::Resting, 0, at, 48),
                now.render(Mood::Resting, 0, at, 48),
                "{frame}"
            );
        }
    }

    /// Breaths in `frames` resting frames of a face given the lamp's one
    /// breath at `pace` from phase 0 (none: no breath given), each a rise
    /// through the middle of its light, counted once.
    fn breaths(pace: Option<f32>, frames: u64) -> usize {
        let mut face = Face::new();
        face.light(Lit::of(true, "white", 3).unwrap());
        let lights: Vec<u32> = (0..frames)
            .map(|frame| {
                let at = 1_000 + frame * 20_000;
                face.breath(pace.map(|pace| {
                    let turns = at as f64 / lamp_msgs::MOOD_BREATH_US as f64 * f64::from(pace);
                    (turns.fract(), pace)
                }));
                brightness(&face.render(Mood::Resting, 0, at, 48))
            })
            .skip(50)
            .collect();
        let (low, high) = (*lights.iter().min().unwrap(), *lights.iter().max().unwrap());
        let (under, over) = (low + (high - low) / 3, high - (high - low) / 3);
        let mut below = false;
        let mut rises = 0;
        for light in lights {
            if light <= under {
                below = true;
            } else if light >= over && below {
                below = false;
                rises += 1;
            }
        }
        rises
    }

    /// #1670: at rest the ring breathes the lamp's one breath: about one in
    /// 5.2 s at the plain pace, the same as with none given, about one in
    /// 4.2 s at the quickest and one in 7.4 s at the slowest.
    #[test]
    fn at_rest_the_ring_breathes_the_lamps_one_breath() {
        // A minute at fifty frames a second, after the first second.
        let frames = 3_050;
        let unset = breaths(None, frames);
        assert!((11..=12).contains(&unset), "{unset}");
        assert_eq!(breaths(Some(1.0), frames), unset);
        let quick = breaths(Some(lamp_msgs::MOOD_PACE_QUICKEST), frames);
        assert!((14..=15).contains(&quick), "{quick}");
        let slow = breaths(Some(lamp_msgs::MOOD_PACE_SLOWEST), frames);
        assert!((8..=9).contains(&slow), "{slow}");
    }

    /// #1670: the resting breath is where the given phase says: out at 0,
    /// in at a half, whatever the clock.
    #[test]
    fn the_resting_breath_is_at_the_phase_it_is_given() {
        let held = |phase: f64| {
            let mut face = Face::new();
            face.light(Lit::of(true, "white", 3).unwrap());
            face.breath(Some((phase, 1.0)));
            // Long enough to ease in; the clock goes on, the phase stays.
            let frames = play(&mut face, Mood::Resting, 0, 1_000, 200);
            brightness(frames.last().unwrap())
        };
        let (out, half_in, full_in) = (held(0.0), held(0.25), held(0.5));
        assert!(
            out < half_in && half_in < full_in,
            "{out} {half_in} {full_in}"
        );
        assert_eq!(held(1.0 - 1e-9), out);
    }

    /// Blinks in a minute and a half at rest with the eye, at `pace`.
    fn blinks(pace: f32) -> usize {
        let mut face = Face::new();
        face.light(Lit::of(true, "white", 3).unwrap());
        face.eye(true);
        face.breath(Some((0.0, pace)));
        let lights: Vec<u32> = play(&mut face, Mood::Resting, 0, 1_000, 4_550)[50..]
            .iter()
            .map(brightness)
            .collect();
        let mut sorted = lights.clone();
        sorted.sort_unstable();
        let usual = sorted[sorted.len() / 2];
        let dim: Vec<bool> = lights.iter().map(|light| *light * 10 < usual * 7).collect();
        dim.windows(2).filter(|pair| !pair[0] && pair[1]).count()
    }

    /// #1670: the eye's blinks at rest come sooner at a quicker pace and
    /// later at a slower one, and stay brief: a blink is not slowed.
    #[test]
    fn at_rest_the_eye_blinks_at_the_lamps_pace() {
        let (slow, plain, quick) = (
            blinks(lamp_msgs::MOOD_PACE_SLOWEST),
            blinks(1.0),
            blinks(lamp_msgs::MOOD_PACE_QUICKEST),
        );
        assert!(slow < plain && plain < quick, "{slow} {plain} {quick}");
    }

    /// A face speaking at `level` for `frames` frames from `from_us`, with
    /// the switch `on` and a skill's light `held`.
    fn speaking_with(
        on: bool,
        held: Option<Lit>,
        level: u8,
        from_us: u64,
        frames: u64,
    ) -> Vec<[Rgb; PIXEL_COUNT]> {
        let mut face = Face::new();
        face.tints_speaking(on);
        face.skill_light(held);
        play(&mut face, Mood::Speaking, level, from_us, frames)
    }

    /// Each channel summed over the ring.
    fn channels(frame: &[Rgb; PIXEL_COUNT]) -> [u32; 3] {
        frame.iter().fold([0; 3], |[r, g, b], pixel| {
            [
                r + u32::from(pixel.red),
                g + u32::from(pixel.green),
                b + u32::from(pixel.blue),
            ]
        })
    }

    /// El on #1722 (14:35): Welcome back's warm light was covered by the
    /// Speaking face for its whole life. Off unless set: a skill's light
    /// held changes no frame of any turn's face.
    #[test]
    fn without_the_switch_a_held_skill_light_leaves_every_face_as_it_was() {
        let blue = Lit::of(true, "blue", 4).unwrap();
        for mood in [Mood::Listening, Mood::Thinking, Mood::Speaking] {
            let mut plain = Face::new();
            let mut held = Face::new();
            held.skill_light(Some(blue));
            assert_eq!(
                play(&mut plain, mood, 120, 1, 100),
                play(&mut held, mood, 120, 1, 100),
                "{mood:?}"
            );
        }
    }

    /// With `LAMP_SKILL_LIGHT_SPEAKING=on`, the Speaking face takes the held
    /// light's colour and, quiet, [`SKILL_SPEAKING_LEVEL`]'s glow whatever
    /// the light's level (4 and 5 alike), and still follows the voice; the
    /// other faces of a turn are their own. That the voice keeps two thirds
    /// or more of plain Speaking's span at every level is the next test's.
    #[test]
    fn with_the_switch_a_held_skill_light_tints_speaking_in_its_colour_at_level_three() {
        let blue = |level| Some(Lit::of(true, "blue", level).unwrap());
        let last = |frames: Vec<[Rgb; PIXEL_COUNT]>| *frames.last().unwrap();
        let plain = last(speaking_with(true, None, 0, 1, 50));
        let tinted = last(speaking_with(true, blue(4), 0, 1, 50));
        let [pr, _, pb] = channels(&plain);
        let [tr, _, tb] = channels(&tinted);
        // Blue leads where Speaking's own warm had red leading.
        assert!(
            pr > pb && tb > tr,
            "{:?} {:?}",
            channels(&plain),
            channels(&tinted)
        );
        // Quiet, the middle level's glow over Speaking's least, at level 4
        // and at level 5 alike.
        assert!(
            total(&tinted) > total(&plain),
            "{} {}",
            total(&tinted),
            total(&plain)
        );
        let five = last(speaking_with(true, blue(5), 0, 1, 50));
        assert_eq!(five, tinted);
        // Still the lamp speaking: brighter with its voice.
        let loud = last(speaking_with(true, blue(4), 255, 1, 50));
        assert!(total(&loud) > total(&tinted) * 11 / 10);
        // Never over the ceiling.
        assert!(
            speaking_with(true, blue(5), 255, 1, 100)
                .iter()
                .flatten()
                .all(|pixel| pixel.red <= 48 && pixel.green <= 48 && pixel.blue <= 48)
        );
        // Listening and Thinking are their own, held light or not.
        for mood in [Mood::Listening, Mood::Thinking] {
            let mut plain = Face::new();
            plain.tints_speaking(true);
            let mut held = plain.clone();
            held.skill_light(blue(4));
            assert_eq!(
                play(&mut plain, mood, 120, 1, 100),
                play(&mut held, mood, 120, 1, 100),
                "{mood:?}"
            );
        }
        // A held light that is out (#1677's ember) tints nothing.
        let out = Some(Lit::of(false, "blue", 4).unwrap());
        assert_eq!(
            speaking_with(true, out, 120, 1, 50),
            speaking_with(true, None, 120, 1, 50)
        );
    }

    /// Seventh on #2253 (16:18): at levels 4 and 5 the least glow left the
    /// voice only the gap to the ceiling, 0.90 to 1.0 at level 5, the idle
    /// turn's own swing, so the face read as a lit disc, not a mouth. At
    /// every level the voice still moves the tinted face over at least two
    /// thirds of plain Speaking's span, as a share of its loudest.
    #[test]
    fn a_held_skill_light_at_any_level_leaves_the_voice_its_span() {
        let last = |frames: Vec<[Rgb; PIXEL_COUNT]>| *frames.last().unwrap();
        let span = |held: Option<Lit>| {
            let quiet = total(&last(speaking_with(true, held, 0, 1, 50)));
            let loud = total(&last(speaking_with(true, held, 255, 1, 50)));
            f64::from(loud - quiet) / f64::from(loud)
        };
        let plain = span(None);
        for level in 1..=LEVELS {
            let tinted = span(Some(Lit::of(true, "blue", level).unwrap()));
            assert!(
                tinted >= plain * 2.0 / 3.0,
                "level {level}: {tinted:.2} against plain {plain:.2}"
            );
        }
    }

    /// Welcome back's warm glow comes at the person's level, 1 on most
    /// lamps: a held light under [`SKILL_SPEAKING_LEVEL`] tints at it too.
    #[test]
    fn a_held_skill_light_under_the_middle_level_tints_speaking_at_the_middle() {
        let warm = |level| Some(Lit::of(true, "warm", level).unwrap());
        let at_three = speaking_with(true, warm(3), 60, 1, 60);
        assert_eq!(speaking_with(true, warm(1), 60, 1, 60), at_three);
        assert_eq!(speaking_with(true, warm(2), 60, 1, 60), at_three);
        assert_ne!(speaking_with(true, None, 60, 1, 60), at_three);
    }

    /// Given back, the face is plain Speaking again from the next frame,
    /// eased as any change: a tenth of a second on most of the way, by
    /// half a second as if no light had been held.
    #[test]
    fn a_skill_light_given_back_leaves_plain_speaking_at_once() {
        let mut plain = Face::new();
        plain.tints_speaking(true);
        let mut held = plain.clone();
        held.skill_light(Some(Lit::of(true, "blue", 4).unwrap()));
        let apart = |a: &[Rgb; PIXEL_COUNT], b: &[Rgb; PIXEL_COUNT]| -> u32 {
            a.iter()
                .zip(b)
                .map(|(a, b)| {
                    u32::from(a.red.abs_diff(b.red))
                        + u32::from(a.green.abs_diff(b.green))
                        + u32::from(a.blue.abs_diff(b.blue))
                })
                .sum()
        };
        let before = play(&mut held, Mood::Speaking, 100, 1, 50);
        let wide = apart(
            before.last().unwrap(),
            play(&mut plain, Mood::Speaking, 100, 1, 50).last().unwrap(),
        );
        assert!(wide > 0);
        held.skill_light(None);
        let after = play(&mut held, Mood::Speaking, 100, 1_000_001, 25);
        let plain_after = play(&mut plain, Mood::Speaking, 100, 1_000_001, 25);
        let left = |frame: usize| apart(&after[frame], &plain_after[frame]) as f32 / wide as f32;
        assert!(left(0) < 0.9, "it begins at once: {}", left(0));
        assert!(left(5) < 0.3, "{}", left(5));
        assert!(left(24) < 0.02, "{}", left(24));
    }

    /// El on #1722: a skill's light on the Speaking face never shows over
    /// the privacy switch. The tinted face is a turn's frame like any other,
    /// written only under the turn's permit: once the switch closes, the
    /// ring refuses it and writes nothing.
    #[test]
    fn a_tinted_speaking_frame_is_refused_once_the_privacy_switch_closes() {
        use lamp_ipc::monotonic_us;
        use lamp_ring::{ChannelCeiling, FrameSink, GuardedRing};
        use lamp_turn_owner::{
            AdmissionState, CaptureState, Controller, MonoTime, OutputKind, Permission,
        };
        let now = || MonoTime::from_micros(monotonic_us());
        let boot = crate::process::new_boot().unwrap();
        let mut controller = Controller::new(boot, now());
        controller
            .set_microphone_permission(now(), Permission::Allowed)
            .unwrap();
        let until = MonoTime::from_micros(now().as_micros() + 500_000);
        controller
            .set_capture(now(), CaptureState::RetainingUntil(until))
            .unwrap();
        controller
            .set_admission(now(), AdmissionState::OpenUntil(until))
            .unwrap();
        let owner = controller
            .admit(now(), lamp_turn_owner::AdmittedInput::NewTurn)
            .unwrap();
        controller.input_ended(now(), owner).unwrap();
        let plan = controller
            .plan_output(now(), owner, OutputKind::Light, None)
            .unwrap();
        let permit = controller.issue_output(now(), plan, 100_000).unwrap();
        struct Counted(std::sync::Arc<std::sync::atomic::AtomicUsize>);
        impl FrameSink for Counted {
            fn write_frame(&mut self, _: &lamp_ring::EncodedFrame) -> std::io::Result<()> {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }
        }
        let writes = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut ring = GuardedRing::new(Counted(writes.clone()), boot).unwrap();
        ring.install_authority(controller.snapshot(now()).unwrap())
            .unwrap();
        let mut face = Face::new();
        face.tints_speaking(true);
        face.skill_light(Lit::of(true, "warm", 4));
        let pixels = face.render(Mood::Speaking, 120, monotonic_us(), 40);
        let ceiling = ChannelCeiling::new(40).unwrap();
        // Permitted: the tinted face is written.
        assert!(ring.show(&pixels, ceiling, permit).is_ok());
        // The switch closes: the same frame is refused.
        controller
            .set_microphone_permission(now(), Permission::Denied)
            .unwrap();
        ring.install_authority(controller.snapshot(now()).unwrap())
            .unwrap();
        let written = writes.load(std::sync::atomic::Ordering::SeqCst);
        assert!(ring.show(&pixels, ceiling, permit).is_err());
        // Refused: no tinted frame is written (only the guard's own black,
        // if it blanks the ring as the switch closes).
        let tinted = pixels.map(|pixel| pixel != lamp_ring::Rgb::BLACK);
        assert!(tinted.iter().any(|lit| *lit));
        assert!(
            writes.load(std::sync::atomic::Ordering::SeqCst) <= written + 1,
            "nothing written under privacy but black"
        );
    }

    /// #1362's one dip: a level lower, as a share of the light's own glow.
    #[test]
    fn one_step_down_is_the_next_level_or_half_the_lowest() {
        let share = |level| Lit::of(true, "white", level).unwrap().step_down_share();
        assert_eq!(share(1), 0.5);
        for level in 2..=LEVELS {
            let index = usize::from(level) - 1;
            assert_eq!(share(level), LEVEL_GLOW[index - 1].0 / LEVEL_GLOW[index].0);
            assert!((0.2..1.0).contains(&share(level)), "{level}");
        }
    }
}
