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
use lamp_ring::{PIXEL_COUNT, Rgb};
use std::f32::consts::TAU;

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
/// After a tap's listening face at rest (#711), the face settles back to its
/// own light this slowly: most of the way in about half a second, so that it
/// eases and does not snap (First, #711).
const SETTLE_EASE_US: f32 = 170_000.0;
const SETTLE_US: u64 = 600_000;
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
        }
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
        let spacing = |count: u64| {
            let spread = (count.wrapping_add(1).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 40)
                % BLINK_EVERY_US.1;
            BLINK_EVERY_US.0 + spread
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
        let brighten = 1.0 + NOTICE_GAIN * notice;
        // A hand kept on the head (#711): the countdown to sleep dims it.
        let countdown = self.held_us.map_or(0.0, |held| {
            ((held as f32 - COUNTDOWN_FROM_US) / (COUNTDOWN_TO_US - COUNTDOWN_FROM_US))
                .clamp(0.0, 1.0)
        });
        let open = open * blink * (1.0 - (1.0 - COUNTDOWN_LOW) * countdown);
        let seconds = (now_us % 3_600_000_000) as f32 / 1_000_000.0;
        let ease = follow(step_us, if settling { SETTLE_EASE_US } else { EASE_US });
        let accent_t = (self.accent != 0
            && now_us >= self.accent_at_us
            && now_us - self.accent_at_us < ACCENT_US)
            .then(|| (now_us - self.accent_at_us) as f32 / ACCENT_US as f32);
        let mut frame = [Rgb::BLACK; PIXEL_COUNT];
        for (pixel, (shown, out)) in self.shown.iter_mut().zip(&mut frame).enumerate() {
            let angle = TAU * pixel as f32 / PIXEL_COUNT as f32;
            let wanted = look(mood, self.level, seconds, angle, self.lit, ceiling, look_to);
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

/// The wanted colour of one pixel, each channel as a share of the ceiling.
/// `look_to`: the eye's look, as the angle around the ring toward the
/// person and how much the far side dims, from 0 to 1.
fn look(
    mood: Mood,
    level: f32,
    seconds: f32,
    angle: f32,
    lit: Lit,
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
            let breath = wave(seconds, 5.2, 0.0, 0.0);
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
            (
                [1.0, 0.72, 0.30],
                (0.26 + 0.74 * level.powf(0.8)) * (0.90 + 0.10 * turn),
            )
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
}
