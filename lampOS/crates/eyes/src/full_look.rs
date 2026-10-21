//! A full look (#836): one larger picture, taken when the voice service asks
//! because the lamp said it would look, after the head has stopped moving.
//!
//! The owner held something up at lamp-4ace and asked "what do you see?".
//! The picture that went was the eyes' working frame, 320 by 240: the room,
//! the person side-on, and nothing to tell what was held up. With
//! `--full-look WIDTHxHEIGHT` the eyes' stream runs at that size all the
//! time (the detector shrinks every frame to its own 320 by 256 as it always
//! could), the working frame is kept in the sight file at 320 across as
//! before ([`working`]), and `--full-look-file` holds only a still made for
//! one request and removed [`KEPT_US`] after.
//!
//! The stream is not switched to a larger mode for the still and back: on
//! UVC that is a stop and a start, and lamp-52e6's camera often fails to
//! start again for minutes (#763). One mode, opened once, costs a larger
//! decode a look and nothing at the moment of asking.
//!
//! The head is the body's: it already turns to the face the eyes see and to
//! a call. The still waits only while `body/joint_states` says it moves: a
//! head joint turning faster than [`STILL_RADIANS_A_SECOND`], or asked to a
//! place more than [`ASKED_RADIANS`] from where it stands. A head that has
//! not moved since the ask and was not asked to is taken at once, with no
//! fixed wait (#842: the build before waited at least 0.3 s on every look
//! with a body). Once the head has moved since the ask, a reading slower
//! than [`STILL_RADIANS_A_SECOND`] may be a pause inside the move (lamp-52e6,
//! 18.88 s into `look-at-name-lamp-52e6-20261014`: base_yaw at -263 counts
//! a second, then -25 and 0, then +225), so the still waits [`STILL_FOR_US`]
//! with every head joint within [`DRIFT_RADIANS`] of where it stopped, as it
//! also does when the body does not say how fast the joints turn. A reading
//! older than [`FRESH_MS`] is taken as moving. A head that moves again
//! before the frame comes is waited for again. No longer than
//! [`SETTLE_AT_MOST_US`] in all.
//! With no body on the bus the still is taken at once.
//!
//! With `--full-look-movement on` (off unless given) the head settles on
//! movement only: no joint's distance to its goal counts, and what is waited
//! for is the camera's aim turning ([`aim_speeds`]), the shoulder and the
//! elbow with the head's joints. Judged joint by joint (#1251), an arm
//! standing up held the still to the 2 s cap: 329 of 4,113 asks replayed on
//! lamp-52e6, against 14 off. The shoulder and the elbow rise together and
//! the aim turns by their difference, so it is 36 (`tests/full_look_replay.rs`).
use lamp_msgs::JointState;
use std::f64::consts::TAU;
use zune_jpeg::{
    JpegDecoder,
    zune_core::{colorspace::ColorSpace, options::DecoderOptions},
};

/// The joints that move the camera.
pub const HEAD: [&str; 3] = ["base_yaw", "head_pitch", "head_roll"];
/// Six of a servo's 4096 counts a turn: how far a still head may drift.
pub const DRIFT_RADIANS: f64 = 6.0 * TAU / 4_096.0;
/// Two counts in the 80 ms over which the body works out how fast a joint
/// turns: more than a count of jitter, less than a head following a slow
/// move at 50 counts a second.
pub const STILL_RADIANS_A_SECOND: f64 = 25.0 * TAU / 4_096.0;
/// 64 counts: a head joint asked to a place this far from where it stands
/// is about to turn. A resting head stands short of its goal under its own
/// weight: head_roll 7 counts on lamp-4ace (2,051 against 2,044,
/// 2026-10-14), and head_pitch up to 36 counts on lamp-52e6 (30 for 1.4 s,
/// `look-at-name-lamp-52e6-20261014`); a margin under that would wait the
/// whole [`SETTLE_AT_MOST_US`] at rest again (#842). At a move's start the
/// goal leads by about 30 counts or less, so this seldom fires: the speed
/// and the wait after a movement are what catch a turn.
pub const ASKED_RADIANS: f64 = 64.0 * TAU / 4_096.0;
/// Two of the body's 80 ms publish periods: a reading older than this says
/// nothing of the head now and is taken as moving.
pub const FRESH_MS: u64 = 160;
/// How long a head whose speed the body does not say must stay put.
pub const STILL_FOR_US: u64 = 300_000;
pub const SETTLE_AT_MOST_US: u64 = 2_000_000;
/// No still this long after the ask: there is none for it.
pub const GIVE_UP_US: u64 = 2_500_000;
/// A still is on the lamp no longer than this: the voice service reads it
/// for the one request it was taken for, once the lamp has finished saying
/// it will look, and drops a look that has not begun 15 s after it said so
/// (`lamp_voice::sight::LOOK_BEGINS_WITHIN`). At 3 s, as first built, a
/// look that went later than that went without its still (#842).
pub const KEPT_US: u64 = 15_000_000;
/// The still's longer side at most. What a model is shown is scaled to
/// about this anyway, and a picture this size fits the provider's bound.
pub const LONGEST_SIDE: usize = 1_024;
/// Qualities tried in turn until the picture fits.
const QUALITIES: [u8; 4] = [85, 75, 65, 50];
const MAX_SIDE: usize = 4_096;

/// One full look asked for: its id from 1, and the still's longer side when
/// the ask names one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Ask {
    pub id: u64,
    pub side: Option<usize>,
}

/// The longest side an ask may name: the largest the decoder takes.
pub const ASKED_SIDE: std::ops::RangeInclusive<usize> = 320..=MAX_SIDE;

/// The most bytes of a still asked for at a side of its own, for writing on
/// paper (Desk tutor, #941). The provider worker sends a still as base64
/// inside one websocket message, and `lamp_gemini` bounds every message at
/// `MAX_WIRE_BYTES`, 256 KiB (its write buffer at 4 KiB more): 184 KiB is
/// 245 KiB as base64, which leaves room for the look's note (1 KiB at most)
/// and the JSON around it. That message bound is the real one; Google's own
/// request bound is far larger.
pub const LARGE_MOST_BYTES: usize = 184 * 1024;

impl Ask {
    /// The still's bytes and longer side at most: 64 KiB and 1024 across
    /// unless the ask names a side, as before; with one, that side and
    /// [`LARGE_MOST_BYTES`]. What the id adds is left out of both.
    pub fn bounds(self) -> (usize, usize) {
        match self.side {
            None => (crate::kept::MOST_BYTES - TAG_BYTES, LONGEST_SIDE),
            Some(side) => (LARGE_MOST_BYTES - TAG_BYTES, side),
        }
    }
}

/// Exactly `{"look_full":ID}` or `{"look_full":ID,"side":PIXELS}`, the id
/// from 1 and the side within [`ASKED_SIDE`]: anything else asks nothing.
pub fn asked(datagram: &[u8]) -> Option<Ask> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire {
        look_full: u64,
        side: Option<usize>,
    }
    let wire = serde_json::from_slice::<Wire>(datagram).ok()?;
    (wire.look_full > 0 && wire.side.is_none_or(|side| ASKED_SIDE.contains(&side))).then_some(Ask {
        id: wire.look_full,
        side: wire.side,
    })
}

/// Bytes a still's id takes: what [`tagged`] adds, at most.
pub const TAG_BYTES: usize = 64;

/// The still with the id of the ask it was taken for, as a JPEG comment
/// right after the start of the image: `lamp-look ID`. The voice service
/// takes a still only with its own ask's id (#842), so a still of an
/// earlier ask written late is never taken for a later one. A number, no
/// more.
pub fn tagged(jpeg: &[u8], id: u64) -> Vec<u8> {
    let text = format!("lamp-look {id}");
    let length = (text.len() + 2) as u16;
    let mut out = Vec::with_capacity(jpeg.len() + text.len() + 4);
    out.extend_from_slice(&jpeg[..2.min(jpeg.len())]);
    out.extend_from_slice(&[0xff, 0xfe]);
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(text.as_bytes());
    out.extend_from_slice(jpeg.get(2..).unwrap_or_default());
    out
}

/// What the body says of the head.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Head {
    /// No body says anything: nothing to wait for.
    Absent,
    Moving,
    /// Not turning as its speed says, and not asked to: taken at once.
    Stopped([f64; 3]),
    /// Speed not said: where each head joint is, to be watched for
    /// [`STILL_FOR_US`].
    Still([f64; 3]),
}

/// [`head_by`] of a reading published at `at_unix_ms`, moving when it is
/// older than [`FRESH_MS`] at `now_unix_ms`.
pub fn head_of(
    joints: &JointState,
    at_unix_ms: u64,
    now_unix_ms: u64,
    movement_only: bool,
) -> Head {
    if now_unix_ms.saturating_sub(at_unix_ms) > FRESH_MS {
        Head::Moving
    } else {
        head_by(joints, movement_only)
    }
}

/// The wall clock in milliseconds since 1970, as the bus stamps a message.
pub fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// The head from what the body says: moving when a head joint turns faster
/// than [`STILL_RADIANS_A_SECOND`] or is asked more than [`ASKED_RADIANS`]
/// from where it stands; stopped when every speed is said and none is.
/// What the body does not say is not waited for, except that a head
/// without speeds is watched for drift ([`Wanted::body`]).
pub fn head(joints: &JointState) -> Head {
    head_by(joints, false)
}

/// [`head`], or with `movement_only` on movement alone: the camera's aim
/// ([`aim_speeds`]) turning faster than [`STILL_RADIANS_A_SECOND`] on any
/// axis, with no goal read.
// ponytail: the arm is judged by its speed only, not by drift, so the one
// reading before the body has a speed misses an arm already moving; watch
// its drift too if a still is ever taken in that first 80 ms.
pub fn head_by(joints: &JointState, movement_only: bool) -> Head {
    let given = |list: &[f64]| list.len() == joints.name.len();
    let (speed, placed) = (given(&joints.velocity), given(&joints.position));
    if movement_only
        && speed
        && aim_speeds(joints)
            .iter()
            .any(|turning| turning.abs() > STILL_RADIANS_A_SECOND)
    {
        return Head::Moving;
    }
    let mut at = [0.0; 3];
    for (index, name) in joints.name.iter().enumerate() {
        let Some(which) = HEAD.iter().position(|head| head == name) else {
            continue;
        };
        if !movement_only && speed && joints.velocity[index].abs() > STILL_RADIANS_A_SECOND {
            return Head::Moving;
        }
        if placed {
            at[which] = joints.position[index];
            if !movement_only
                && given(&joints.commanded)
                && (joints.commanded[index] - at[which]).abs() > ASKED_RADIANS
            {
                return Head::Moving;
            }
        }
    }
    if speed {
        Head::Stopped(at)
    } else {
        Head::Still(at)
    }
}

/// How fast the camera's aim turns, in radians a second: about the base's
/// yaw, the head's roll, and in the arm's plane, where the shoulder, the
/// elbow and head pitch turn about parallel axes, head pitch plus shoulder
/// less elbow. The elbow turns the other way to the shoulder
/// (`robots/lamp/urdf/lamp.urdf`: axes +y and -y), so an arm standing up,
/// both rising at 110 to 175 counts a second on lamp-52e6, carries the head
/// up with its aim turning by their difference: 551 and 474 counts of rise
/// leave head pitch's aim 77 counts off (`look-at-name-lamp-52e6-20261014`,
/// 3.1 to 8.1 s). A joint missing from the reading counts as not turning.
// ponytail: the camera's rise as the arm carries it is not counted. At 0.24 m
// of arm and 160 counts a second it is under a pixel a frame at 1 m and 1024
// across; count it if stills of near things blur while the arm stands up.
pub fn aim_speeds(joints: &JointState) -> [f64; 3] {
    let of = |name: &str| {
        joints
            .name
            .iter()
            .position(|joint| joint == name)
            .and_then(|index| joints.velocity.get(index))
            .copied()
            .unwrap_or(0.0)
    };
    [
        of("base_yaw"),
        of("head_roll"),
        of("head_pitch") + of("shoulder") - of("elbow"),
    ]
}

/// One full look asked for: when the head has settled, and the first frame
/// after that is the still.
#[derive(Debug)]
pub struct Wanted {
    asked_us: u64,
    /// Since when the head has been still, and where it was then.
    still_since: Option<(u64, [f64; 3])>,
    /// Whether the head has moved since the ask: then a stopped reading may
    /// be a pause inside the move, and is watched as still.
    turned: bool,
    /// When it settled, and how many frames there had been by then.
    settled: Option<(u64, u64)>,
}

impl Wanted {
    pub fn new(now_us: u64) -> Self {
        Self {
            asked_us: now_us,
            still_since: None,
            turned: false,
            settled: None,
        }
    }

    /// What the body says of the head now. `frames` is how many frames the
    /// camera has given so far. A head that moves again before the still is
    /// taken is waited for again, until [`SETTLE_AT_MOST_US`].
    pub fn body(&mut self, head: Head, now_us: u64, frames: u64) {
        let capped = now_us.saturating_sub(self.asked_us) >= SETTLE_AT_MOST_US;
        let head = match head {
            Head::Stopped(at) if self.turned => Head::Still(at),
            head => head,
        };
        let mut moved = false;
        let settled = match head {
            Head::Absent | Head::Stopped(_) => true,
            Head::Moving => {
                moved = true;
                false
            }
            Head::Still(at) => {
                // A head that drifts is a head still being turned slowly.
                moved = self.still_since.is_some_and(|(_, was)| {
                    was.iter()
                        .zip(at)
                        .any(|(was, now)| (was - now).abs() > DRIFT_RADIANS)
                });
                if moved {
                    self.still_since = None;
                }
                let (since, _) = *self.still_since.get_or_insert((now_us, at));
                now_us.saturating_sub(since) >= STILL_FOR_US
            }
        };
        if moved {
            self.turned = true;
            if matches!(head, Head::Moving) {
                self.still_since = None;
            }
            if !capped {
                self.settled = None;
            }
        }
        if self.settled.is_none() && (settled || capped) {
            self.settled = Some((now_us, frames));
        }
    }

    /// Whether the newest frame, the `frames`th, is the still: the first
    /// one to come after the head settled.
    pub fn take(&self, frames: u64) -> bool {
        self.settled.is_some_and(|(_, by)| frames > by)
    }

    /// How long the head was waited for, once it settled.
    pub fn waited_us(&self) -> Option<u64> {
        self.settled.map(|(at, _)| at.saturating_sub(self.asked_us))
    }

    pub fn given_up(&self, now_us: u64) -> bool {
        now_us.saturating_sub(self.asked_us) >= GIVE_UP_US
    }
}

/// The camera's frame as a picture the provider takes: itself when it is a
/// whole JPEG small enough already, or else decoded, shrunk to
/// [`LONGEST_SIDE`] by averaging and encoded at the best quality that fits
/// in `most_bytes`. With its size, across and down.
pub fn fit(frame: &[u8], most_bytes: usize) -> Result<(Vec<u8>, (usize, usize)), String> {
    fit_within(frame, most_bytes, LONGEST_SIDE)
}

/// The still for one ask, within its [`Ask::bounds`].
pub fn fit_for(frame: &[u8], ask: Ask) -> Result<(Vec<u8>, (usize, usize)), String> {
    let (most_bytes, longest) = ask.bounds();
    fit_within(frame, most_bytes, longest)
}

/// The working frame kept in `--sight-file` while the stream runs at the
/// full size: the frame at 320 across, as the eyes kept it before (#873
/// reads it at any time). A frame already that small goes as it is.
// ponytail: decodes the frame a second time after the detector's decode;
// share one decode if the eyes' ms a look at the full size matters.
pub fn working(frame: &[u8]) -> Result<Vec<u8>, String> {
    fit_within(frame, crate::kept::MOST_BYTES, WORKING_SIDE).map(|(jpeg, _)| jpeg)
}

/// The working frame's longer side.
pub const WORKING_SIDE: usize = 320;

fn fit_within(
    frame: &[u8],
    most_bytes: usize,
    longest: usize,
) -> Result<(Vec<u8>, (usize, usize)), String> {
    let options = DecoderOptions::default()
        .set_max_width(MAX_SIDE)
        .set_max_height(MAX_SIDE)
        .jpeg_set_out_colorspace(ColorSpace::RGB);
    let mut decoder = JpegDecoder::new_with_options(frame, options);
    decoder
        .decode_headers()
        .map_err(|error| format!("jpeg: {error:?}"))?;
    let (width, height) = decoder.dimensions().ok_or("a jpeg without a size")?;
    if frame.len() <= most_bytes && width.max(height) <= longest && crate::kept::whole_jpeg(frame) {
        return Ok((frame.to_vec(), (width, height)));
    }
    let rgb = decoder
        .decode()
        .map_err(|error| format!("jpeg: {error:?}"))?;
    let (rgb, size) = shrink(&rgb, (width, height), longest);
    encode_within(rgb, size, most_bytes)
}

/// The working frame from the look's own picture, already decoded and
/// shrunk to 320 across (`--sight-from-look`): only an encode, where
/// [`working`] decodes the full frame a second time and shrinks it (5.9
/// against 0.72 ms on an x86 laptop, about 41 ms a look on lamp-4ace, Ten on
/// #2602, 22 Oct 05:4x).
pub fn working_from_picture(picture: &crate::picture::Picture) -> Result<Vec<u8>, String> {
    use crate::picture::{HEIGHT, WIDTH};
    let (across, down) = picture.filled;
    if across == 0 || down == 0 || picture.planes.len() != 3 * WIDTH * HEIGHT {
        return Err("no picture".into());
    }
    // The planes are blue, green and red; the encoder takes red first.
    let plane = WIDTH * HEIGHT;
    let mut rgb = Vec::with_capacity(across * down * 3);
    for y in 0..down {
        for x in 0..across {
            let at = y * WIDTH + x;
            rgb.extend([2, 1, 0].map(|channel| {
                picture.planes[channel * plane + at]
                    .clamp(0.0, 255.0)
                    .round() as u8
            }));
        }
    }
    encode_within(rgb, (across, down), crate::kept::MOST_BYTES).map(|(jpeg, _)| jpeg)
}

/// Red, green and blue bytes encoded at the best quality that fits in
/// `most_bytes`, shrunk further if none does.
fn encode_within(
    mut rgb: Vec<u8>,
    mut size: (usize, usize),
    most_bytes: usize,
) -> Result<(Vec<u8>, (usize, usize)), String> {
    let encode = |rgb: &[u8], (across, down): (usize, usize), quality: u8| {
        let mut out = Vec::new();
        jpeg_encoder::Encoder::new(&mut out, quality)
            .encode(
                rgb,
                across as u16,
                down as u16,
                jpeg_encoder::ColorType::Rgb,
            )
            .map(|()| out)
            .map_err(|error| format!("encoding: {error}"))
    };
    let lowest = QUALITIES[QUALITIES.len() - 1];
    loop {
        // The lowest quality first: a size it does not fit at is shrunk at
        // once, not encoded four times. At 3264x2448 that was 13 encodes.
        let least = encode(&rgb, size, lowest)?;
        if least.len() <= most_bytes {
            for quality in &QUALITIES[..QUALITIES.len() - 1] {
                let out = encode(&rgb, size, *quality)?;
                if out.len() <= most_bytes {
                    return Ok((out, size));
                }
            }
            return Ok((least, size));
        }
        if size.0.max(size.1) <= 160 {
            return Err("no size fits".into());
        }
        (rgb, size) = shrink(&rgb, size, size.0.max(size.1) * 3 / 4);
    }
}

/// Red, green and blue bytes shrunk so that the longer side is at most
/// `longest`, each pixel the average of those it covers. Never enlarged.
fn shrink(
    rgb: &[u8],
    (width, height): (usize, usize),
    longest: usize,
) -> (Vec<u8>, (usize, usize)) {
    let scale = (width.max(height) as f64 / longest.max(1) as f64).max(1.0);
    let to = (
        ((width as f64 / scale).round() as usize).clamp(1, width),
        ((height as f64 / scale).round() as usize).clamp(1, height),
    );
    let span = |at: usize, of: usize, into: usize| {
        let first = at * of / into;
        (first, ((at + 1) * of / into).max(first + 1).min(of))
    };
    let mut out = Vec::with_capacity(to.0 * to.1 * 3);
    for y in 0..to.1 {
        let (top, bottom) = span(y, height, to.1);
        for x in 0..to.0 {
            let (left, right) = span(x, width, to.0);
            let mut sum = [0u32; 3];
            for row in top..bottom {
                for pixel in
                    rgb[(row * width + left) * 3..(row * width + right) * 3].chunks_exact(3)
                {
                    for channel in 0..3 {
                        sum[channel] += u32::from(pixel[channel]);
                    }
                }
            }
            let count = ((bottom - top) * (right - left)) as u32;
            out.extend(sum.map(|total| ((total + count / 2) / count) as u8));
        }
    }
    (out, to)
}

/// Opens tried at the full size before the working size is tried too: six,
/// thirty seconds at one try every five. A camera that refuses for a moment
/// still opens at the full size; one that refuses for minutes (lamp-52e6,
/// lamp-4ace: 6 to 18 min, #763) gets every other try at the working size,
/// so the eyes see as soon as the camera lets them, and [`Upgrade`] brings
/// the full size back later.
pub const FULL_FIRST: u32 = 6;

/// Whether the next open is at the full size, after `failures` opens in a
/// row that did not open: the first [`FULL_FIRST`], then every other one.
pub fn full_next(failures: u32) -> bool {
    failures < FULL_FIRST || (failures - FULL_FIRST) % 2 == 1
}

/// After the stream opened at the working size for want of the full one:
/// when to close it and try the full size once more. Only after streaming
/// that long with nobody in view and no look asked for, so that nobody sees
/// the eyes shut; then after half an hour, then every hour.
#[derive(Debug, Default)]
pub struct Upgrade {
    /// Since when the stream has run at the working size, if it does.
    fallback_since_us: Option<u64>,
    /// Tries made since the stream last opened at the full size.
    tries: u32,
}

pub const UPGRADE_AFTER_US: [u64; 3] = [600_000_000, 1_800_000_000, 3_600_000_000];

impl Upgrade {
    /// The stream opened now, at the full size or not.
    pub fn opened(&mut self, full: bool, now_us: u64) {
        if full {
            *self = Self::default();
        } else {
            self.fallback_since_us = Some(now_us);
        }
    }

    /// Whether to close the working-size stream now and try the full size.
    pub fn due(&self, now_us: u64, somebody_in_view: bool, look_pending: bool) -> bool {
        let wait = UPGRADE_AFTER_US[(self.tries as usize).min(UPGRADE_AFTER_US.len() - 1)];
        !somebody_in_view
            && !look_pending
            && self
                .fallback_since_us
                .is_some_and(|since| now_us.saturating_sub(since) >= wait)
    }

    /// A try is made now: the stream closes.
    pub fn tried(&mut self) -> u32 {
        self.fallback_since_us = None;
        self.tries += 1;
        self.tries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000;

    const COUNT: f64 = TAU / 4_096.0;
    const STILL: Head = Head::Still([0.1, 0.5, 0.4]);

    /// The body's message: the head's yaw `off` from its goal `goal`,
    /// turning at `velocity`; the shoulder far from its goal throughout.
    fn joints(goal: f64, off: f64, velocity: f64) -> JointState {
        let name = ["base_yaw", "shoulder", "elbow", "head_roll", "head_pitch"];
        JointState {
            name: name.map(str::to_owned).to_vec(),
            position: vec![goal - off, 0.2, 0.3, 0.4, 0.5],
            velocity: vec![velocity, 0.0, 0.0, 0.0, 0.0],
            effort: vec![],
            commanded: vec![goal, 0.2 + 1.0, 0.3, 0.4, 0.5],
        }
    }

    const MINUTE: u64 = 60_000_000;

    #[test]
    fn the_full_size_is_tried_first_six_times_then_every_other_time() {
        let sizes: Vec<bool> = (0..10).map(full_next).collect();
        assert_eq!(
            sizes,
            [true, true, true, true, true, true, false, true, false, true]
        );
    }

    #[test]
    fn a_fallback_stream_tries_the_full_size_after_ten_quiet_minutes_and_backs_off() {
        let mut upgrade = Upgrade::default();
        // Opened at the full size: nothing to do.
        upgrade.opened(true, 0);
        assert!(!upgrade.due(100 * MINUTE, false, false));
        // Fallen back at minute 0.
        upgrade.opened(false, 0);
        assert!(!upgrade.due(10 * MINUTE - 1, false, false));
        // Not while somebody is in view or a look is asked for.
        assert!(!upgrade.due(10 * MINUTE, true, false));
        assert!(!upgrade.due(10 * MINUTE, false, true));
        assert!(upgrade.due(10 * MINUTE, false, false));
        // The try fails: reopened at the working size at minute 11, the next
        // try half an hour later, then every hour.
        assert_eq!(upgrade.tried(), 1);
        assert!(!upgrade.due(11 * MINUTE, false, false));
        upgrade.opened(false, 11 * MINUTE);
        assert!(!upgrade.due(40 * MINUTE, false, false));
        assert!(upgrade.due(41 * MINUTE, false, false));
        assert_eq!(upgrade.tried(), 2);
        upgrade.opened(false, 42 * MINUTE);
        assert!(!upgrade.due(101 * MINUTE, false, false));
        assert!(upgrade.due(102 * MINUTE, false, false));
        assert_eq!(upgrade.tried(), 3);
        upgrade.opened(false, 103 * MINUTE);
        assert!(upgrade.due(163 * MINUTE, false, false));
        // A try that opens at the full size ends it.
        upgrade.tried();
        upgrade.opened(true, 164 * MINUTE);
        assert!(!upgrade.due(1_000 * MINUTE, false, false));
        upgrade.opened(false, 1_000 * MINUTE);
        assert!(upgrade.due(1_010 * MINUTE, false, false));
    }

    #[test]
    fn only_the_exact_ask_is_one() {
        let ask = |id, side| Some(Ask { id, side });
        assert_eq!(asked(br#"{"look_full":42}"#), ask(42, None));
        assert_eq!(
            asked(br#"{"look_full":42,"side":3264}"#),
            ask(42, Some(3_264))
        );
        assert_eq!(asked(br#"{"side":320,"look_full":9}"#), ask(9, Some(320)));
        for not in [
            &br#"{"look_full":42,"side":319}"#[..],
            br#"{"look_full":42,"side":4097}"#,
            br#"{"look_full":42,"side":"3264x2448"}"#,
            br#"{"look_full":42,"side":-1}"#,
            br#"{"look_full":0,"side":1024}"#,
            br#"{"look_full":true}"#,
            br#"{"look_full":0}"#,
            br#"{"look_full":-1}"#,
            br#"{"camera":true}"#,
            br#"{"look_full":4,"keep":true}"#,
            b"",
        ] {
            assert_eq!(asked(not), None);
        }
    }

    #[test]
    fn a_still_carries_its_ask_s_id_and_stays_a_whole_jpeg() {
        let (still, _) = fit(&camera_frame(320, 240, 80), 64 * 1024 - TAG_BYTES).unwrap();
        let marked = tagged(&still, 42);
        assert!(marked.len() <= still.len() + TAG_BYTES);
        assert!(crate::kept::whole_jpeg(&marked));
        // The exact bytes the voice service looks for after the start.
        let mark = b"\xff\xd8\xff\xfe\x00\x0elamp-look 42";
        assert_eq!(&marked[..mark.len()], &mark[..]);
        assert_eq!(&marked[mark.len()..], &still[2..]);
        assert!(crate::picture::Picture::from_jpeg(&marked).is_ok());
        assert!(tagged(&still, u64::MAX).len() <= still.len() + TAG_BYTES);
    }

    #[test]
    fn the_head_is_stopped_when_it_is_not_turning_and_not_asked_to() {
        const STOPPED: Head = Head::Stopped([0.1, 0.5, 0.4]);
        assert_eq!(head(&joints(0.1, 0.0, 0.0)), STOPPED);
        // Standing short of its goal, as lamp-4ace's head rests: stopped.
        assert_eq!(
            head(&joints(0.1, 12.0 * COUNT, 0.0)),
            Head::Stopped([0.1 - 12.0 * COUNT, 0.5, 0.4])
        );
        assert_eq!(
            head(&joints(0.1, 5.0 * COUNT, COUNT)),
            Head::Stopped([0.1 - 5.0 * COUNT, 0.5, 0.4])
        );
        // A count behind its goal but turning at 50 counts a second.
        assert_eq!(head(&joints(0.1, COUNT, 50.0 * COUNT)), Head::Moving);
        // Not turning yet, but asked to a place 80 counts away; at rest 36
        // counts short, as lamp-52e6's head_pitch, it is stopped.
        assert_eq!(head(&joints(0.1, 80.0 * COUNT, 0.0)), Head::Moving);
        assert!(matches!(
            head(&joints(0.1, 36.0 * COUNT, 0.0)),
            Head::Stopped(_)
        ));
        // Without its speed, a head is only still, to be watched.
        let mut unsaid = joints(0.1, 0.0, 0.0);
        unsaid.velocity.clear();
        assert_eq!(head(&unsaid), STILL);
        // The shoulder turning does not turn the camera's aim as the head
        // joints do, and is not waited for, nor is its goal far away.
        let mut shoulder = joints(0.1, 0.0, 0.0);
        shoulder.velocity[1] = 100.0 * COUNT;
        assert_eq!(head(&shoulder), STOPPED);
        let mut unknown = joints(0.1, 1.0, 0.0);
        unknown.position.clear();
        unknown.velocity.clear();
        assert_eq!(head(&unknown), Head::Still([0.0; 3]));
    }

    #[test]
    fn a_head_resting_short_of_goals_the_body_nudges_is_taken_at_once() {
        // lamp-4ace at rest, 2026-10-14 (`rest-glance-lamp-4ace-20261014`):
        // goals [pitch, roll, yaw] 1951-1952, 2043-2044, 2016-2023 against
        // positions 1952, 2051, 2023, one count of jitter; a message every
        // 80 ms as the body offers them. The rule on the goal never held
        // there and every still waited the whole 2 s; the rule on 0.3 s of
        // stillness waited 0.3 s for this head, which does not move (#842).
        let radians = |counts: f64| (counts - 2_048.0) * COUNT;
        let goals = [
            [1_952.0, 2_044.0, 2_021.0],
            [1_951.0, 2_043.0, 2_023.0],
            [1_951.0, 2_044.0, 2_016.0],
            [1_952.0, 2_044.0, 2_016.0],
        ];
        let mut wanted = Wanted::new(0);
        for step in 0..25u64 {
            let [pitch, roll, yaw] = goals[step as usize % goals.len()];
            let jitter = (step % 2) as f64;
            let joints = JointState {
                name: ["base_yaw", "shoulder", "elbow", "head_roll", "head_pitch"]
                    .map(str::to_owned)
                    .to_vec(),
                position: [2_023.0, 2_055.0, 2_047.0, 2_051.0, 1_952.0 + jitter]
                    .map(radians)
                    .to_vec(),
                velocity: vec![0.0, 0.0, 0.0, 0.0, (2.0 * jitter - 1.0) * COUNT / 0.08],
                effort: vec![],
                commanded: [yaw, 2_055.0, 2_047.0, roll, pitch].map(radians).to_vec(),
            };
            wanted.body(head(&joints), step * 80 * MS, step);
        }
        assert_eq!(wanted.waited_us(), Some(0));
        assert!(wanted.take(1));
    }

    #[test]
    fn a_head_turning_or_asked_to_turn_is_waited_for_until_it_stops() {
        // Asked 200 counts away at 0 and not yet turning, turning at 400
        // counts a second from 40 ms, arrived at 540 and resting 7 counts
        // short, as the body says every 20 ms.
        let mut wanted = Wanted::new(0);
        for at in (0..=840u64).step_by(20) {
            let (off, velocity) = match at {
                0..40 => (200.0, 0.0),
                40..540 => (200.0 - (at - 40) as f64 * 0.4, 400.0),
                _ => (7.0, 0.0),
            };
            let joints = joints(0.1, off.max(7.0) * COUNT, velocity * COUNT);
            wanted.body(head(&joints), at * MS, at / 100);
            if at < 840 {
                assert!(!wanted.take(at / 100 + 1), "taken at {at} ms");
            }
        }
        // It moved since the ask: taken 0.3 s after it stopped.
        assert_eq!(wanted.waited_us(), Some(840 * MS));
        assert!(wanted.take(9));
    }

    /// lamp-52e6 standing: the shoulder 30 counts short of its goal at
    /// 2,657 and pushing, not moving; head_pitch 80 counts short at rest.
    fn standing(shoulder_speed: f64, yaw_speed: f64) -> JointState {
        let radians = |counts: f64| (counts - 2_048.0) * COUNT;
        JointState {
            name: ["base_yaw", "shoulder", "elbow", "head_roll", "head_pitch"]
                .map(str::to_owned)
                .to_vec(),
            position: [2_015.0, 2_627.0, 2_069.0, 2_051.0, 1_888.0]
                .map(radians)
                .to_vec(),
            velocity: vec![yaw_speed * COUNT, shoulder_speed * COUNT, 0.0, 0.0, 0.0],
            effort: vec![],
            commanded: [2_015.0, 2_657.0, 2_069.0, 2_051.0, 1_968.0]
                .map(radians)
                .to_vec(),
        }
    }

    #[test]
    fn on_movement_only_a_joint_resting_short_is_taken_at_once_and_a_move_waits() {
        // Resting short, not moving: at once. With the goal rule, head_pitch
        // 80 counts short is taken for a turn and waits the whole 2 s.
        let mut wanted = Wanted::new(0);
        wanted.body(head_by(&standing(0.0, 0.0), true), 0, 0);
        assert_eq!(wanted.waited_us(), Some(0));
        assert!(wanted.take(1));
        // The shoulder standing up at 200 counts a second for 400 ms carries
        // the camera: waited for, then 0.3 s still, as a head that turns.
        // Without movement only the arm is not read and it is taken at once.
        let mut wanted = Wanted::new(0);
        for at in (0..=700u64).step_by(20) {
            let speed = if at < 400 { 200.0 } else { 0.0 };
            wanted.body(head_by(&standing(speed, 0.0), true), at * MS, at / 33);
            if at < 700 {
                assert!(!wanted.take(at / 33 + 1), "taken at {at} ms");
            }
        }
        assert_eq!(wanted.waited_us(), Some(700 * MS));
        // A turning head the same way, 0.3 s after it stops.
        let mut wanted = Wanted::new(0);
        for at in (0..=700u64).step_by(20) {
            let speed = if at < 400 { 263.0 } else { 0.0 };
            wanted.body(head_by(&standing(0.0, speed), true), at * MS, at / 33);
        }
        assert_eq!(wanted.waited_us(), Some(700 * MS));
    }

    #[test]
    fn on_movement_only_an_arm_standing_up_with_the_aim_held_is_not_a_move() {
        // lamp-52e6 standing up (3.1 to 8.1 s into
        // `look-at-name-lamp-52e6-20261014`): shoulder and elbow both rising
        // at about 150 counts a second turn the camera's aim by their
        // difference.
        let arm = |shoulder: f64, elbow: f64, pitch: f64| {
            let mut joints = standing(0.0, 0.0);
            joints.velocity = vec![0.0, shoulder * COUNT, elbow * COUNT, 0.0, pitch * COUNT];
            head_by(&joints, true)
        };
        assert!(matches!(arm(150.0, 138.0, 0.0), Head::Stopped(_)));
        // The shoulder or the elbow alone turns it, as head pitch does, and
        // head pitch turning against the arm holds it.
        assert_eq!(arm(150.0, 0.0, 0.0), Head::Moving);
        assert_eq!(arm(0.0, 113.0, 0.0), Head::Moving);
        assert_eq!(arm(0.0, 0.0, 75.0), Head::Moving);
        assert!(matches!(arm(150.0, 0.0, -150.0), Head::Stopped(_)));
        // Off, the arm is not read.
        let mut joints = standing(0.0, 0.0);
        joints.velocity[1] = 150.0 * COUNT;
        joints.commanded = joints.position.clone();
        assert!(matches!(head_by(&joints, false), Head::Stopped(_)));
    }

    #[test]
    fn without_movement_only_the_head_settles_as_before() {
        // The goal rule stands and the arm is not read.
        assert_eq!(head_by(&standing(0.0, 0.0), false), Head::Moving);
        assert!(matches!(
            head_by(&standing(200.0, 0.0), false),
            Head::Moving
        ));
        let mut resting = standing(200.0, 0.0);
        resting.commanded = resting.position.clone();
        assert!(matches!(head(&resting), Head::Stopped(_)));
        assert_eq!(head_by(&resting, false), head(&resting));
        assert_eq!(head_by(&resting, true), Head::Moving);
    }

    #[test]
    fn a_pause_inside_a_move_is_not_taken_for_a_stop() {
        // lamp-52e6, 18.88 s into `look-at-name-lamp-52e6-20261014`: base_yaw
        // at -263 counts a second, then -25 and 0 with its goal 8 and 28
        // counts away, then +225; a reading every 80 ms.
        let mut wanted = Wanted::new(0);
        for (step, (off, velocity)) in [(40.0, -263.0), (8.0, -25.0), (28.0, 0.0), (20.0, 225.0)]
            .into_iter()
            .enumerate()
        {
            let at = step as u64 * 80;
            wanted.body(
                head(&joints(0.1, off * COUNT, velocity * COUNT)),
                at * MS,
                at / 33,
            );
            assert!(!wanted.take(at / 33 + 1), "taken at {at} ms");
        }
    }

    #[test]
    fn an_old_reading_is_taken_as_moving() {
        let still = joints(0.1, 0.0, 0.0);
        assert_eq!(
            head_of(&still, 1_000, 1_000 + FRESH_MS, false),
            Head::Stopped([0.1, 0.5, 0.4])
        );
        assert_eq!(
            head_of(&still, 1_000, 1_001 + FRESH_MS, false),
            Head::Moving
        );
        assert_eq!(head_of(&still, 1_000, 1_001 + FRESH_MS, true), Head::Moving);
    }

    #[test]
    fn a_truly_still_head_gets_the_first_frame_after_it_settles() {
        let mut wanted = Wanted::new(0);
        // Turning toward the person for half a second: no still.
        for at in (0..500).step_by(20) {
            wanted.body(Head::Moving, at * MS, at / 100);
            assert!(!wanted.take(at / 100 + 1));
        }
        // Still from 500 ms: settled at 800, after 8 frames.
        for at in (500..800).step_by(20) {
            wanted.body(STILL, at * MS, 8);
            assert!(!wanted.take(8));
        }
        wanted.body(STILL, 800 * MS, 8);
        assert_eq!(wanted.waited_us(), Some(800 * MS));
        // The frame it had is from before; the next one is the still.
        assert!(!wanted.take(8));
        assert!(wanted.take(9));
    }

    #[test]
    fn a_head_following_a_slow_move_with_a_count_of_lag_is_not_still() {
        // Reviewer's case on #842: 50 counts a second, one count behind,
        // as the body's own publisher reports it every 20 ms.
        let mut wanted = Wanted::new(0);
        for step in 0..100u64 {
            let goal = step as f64 * COUNT;
            let joints = joints(goal, COUNT, 50.0 * COUNT);
            wanted.body(head(&joints), step * 20 * MS, step);
            assert!(!wanted.take(step + 1), "taken at {} ms", step * 20);
        }
        // Even were its speed not told: it drifts, a count a step.
        let mut wanted = Wanted::new(0);
        for step in 0..100u64 {
            let goal = step as f64 * COUNT;
            let mut joints = joints(goal, COUNT, 0.0);
            joints.velocity.clear();
            wanted.body(head(&joints), step * 20 * MS, step);
            assert!(!wanted.take(step + 1), "taken at {} ms", step * 20);
        }
        // The 2 s bound still holds.
        wanted.body(Head::Moving, SETTLE_AT_MOST_US, 100);
        assert!(wanted.take(101));
    }

    #[test]
    fn a_head_that_moves_again_before_the_frame_is_waited_for_again() {
        // Reviewer's case on #842: settled at 300 ms, moving at 350, a
        // frame at 400.
        let mut wanted = Wanted::new(0);
        wanted.body(STILL, 0, 0);
        wanted.body(STILL, 300 * MS, 3);
        assert_eq!(wanted.waited_us(), Some(300 * MS));
        wanted.body(Head::Moving, 350 * MS, 3);
        assert!(!wanted.take(4));
        assert_eq!(wanted.waited_us(), None);
        // So does one that drifts while it seemed still.
        wanted.body(STILL, 400 * MS, 4);
        wanted.body(Head::Still([0.2, 0.5, 0.4]), 600 * MS, 5);
        wanted.body(Head::Still([0.2, 0.5, 0.4]), 800 * MS, 6);
        assert!(!wanted.take(7));
        wanted.body(Head::Still([0.2, 0.5, 0.4]), 900 * MS, 6);
        assert!(wanted.take(7));
        // After the bound a movement no longer holds the still back.
        let mut late = Wanted::new(0);
        late.body(Head::Moving, SETTLE_AT_MOST_US, 1);
        late.body(Head::Moving, SETTLE_AT_MOST_US + 20 * MS, 1);
        assert!(late.take(2));
    }

    #[test]
    fn a_head_that_never_settles_or_no_body_still_gets_a_still() {
        let mut moving = Wanted::new(0);
        for at in (0..SETTLE_AT_MOST_US).step_by(20_000) {
            moving.body(Head::Moving, at, 1);
        }
        assert!(!moving.take(2));
        moving.body(Head::Moving, SETTLE_AT_MOST_US, 1);
        assert!(moving.take(2));
        // Moving, then still but not long enough: the bound holds.
        let mut late = Wanted::new(0);
        late.body(STILL, SETTLE_AT_MOST_US - 100 * MS, 1);
        late.body(STILL, SETTLE_AT_MOST_US, 1);
        assert_eq!(late.waited_us(), Some(SETTLE_AT_MOST_US));
        // No body: at once.
        let mut alone = Wanted::new(5 * MS);
        alone.body(Head::Absent, 5 * MS, 3);
        assert_eq!(alone.waited_us(), Some(0));
        assert!(alone.take(4));
        assert!(!alone.given_up(5 * MS + GIVE_UP_US - 1));
        assert!(alone.given_up(5 * MS + GIVE_UP_US));
    }

    fn camera_frame(width: usize, height: usize, quality: u8) -> Vec<u8> {
        // A room as a camera gives it: shapes with hard edges, light falling
        // off across them, and a little sensor noise everywhere.
        let mut rgb = Vec::with_capacity(width * height * 3);
        for y in 0..height {
            for x in 0..width {
                let (u, v) = (x * 64 / width, y * 48 / height);
                let block = ((u * 37 + v * 91) % 7) as u32 * 30;
                let light = (x + y) as u32 * 120 / (width + height) as u32;
                let noise = ((x * 7_919 + y * 104_729) % 13) as u32;
                let value = |shift: u32| ((block + light + noise + shift) % 256) as u8;
                rgb.extend([value(0), value(40), value(90)]);
            }
        }
        let mut out = Vec::new();
        jpeg_encoder::Encoder::new(&mut out, quality)
            .encode(
                &rgb,
                width as u16,
                height as u16,
                jpeg_encoder::ColorType::Rgb,
            )
            .unwrap();
        out
    }

    #[test]
    fn a_large_frame_is_made_to_fit_and_a_small_whole_one_goes_as_it_is() {
        let most = 64 * 1024;
        let large = camera_frame(1_280, 960, 90);
        assert!(large.len() > most);
        let started = std::time::Instant::now();
        let (still, size) = fit(&large, most).unwrap();
        eprintln!(
            "1280x960 of {} bytes to {size:?} of {} bytes in {} ms",
            large.len(),
            still.len(),
            started.elapsed().as_millis()
        );
        assert!(still.len() <= most);
        // As large as fits, at most 1024 across, and the same shape.
        assert!(size.0 <= LONGEST_SIDE && size.0 >= 640 && size.0 * 3 == size.1 * 4);
        assert!(crate::kept::whole_jpeg(&still));
        let small = camera_frame(320, 240, 80);
        assert!(small.len() <= most);
        assert_eq!(fit(&small, most).unwrap(), (small.clone(), (320, 240)));
        assert!(fit(b"not a jpeg", most).is_err());
    }

    /// `--sight-from-look`: the working frame made from the look's own
    /// decoded picture is the one `working` makes, 320 by 240 and a whole
    /// JPEG under the sight file's bound, with the same picture in it (its
    /// mean colour within a few greys), for an encode alone.
    #[test]
    fn the_working_frame_from_the_looks_picture_is_the_same_frame() {
        let decode = |jpeg: &[u8]| {
            let mut decoder = JpegDecoder::new_with_options(
                jpeg,
                DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGB),
            );
            let rgb = decoder.decode().unwrap();
            (rgb, decoder.dimensions().unwrap())
        };
        let mean = |rgb: &[u8], channel: usize| {
            rgb.iter()
                .skip(channel)
                .step_by(3)
                .map(|v| f64::from(*v))
                .sum::<f64>()
                / (rgb.len() / 3) as f64
        };
        let frame = camera_frame(1_280, 960, 90);
        let shrunk = working(&frame).unwrap();
        let picture = crate::picture::Picture::from_jpeg(&frame).unwrap();
        let from_look = working_from_picture(&picture).unwrap();
        assert!(crate::kept::whole_jpeg(&from_look));
        assert!(from_look.len() <= crate::kept::MOST_BYTES);
        let ((a, a_size), (b, b_size)) = (decode(&shrunk), decode(&from_look));
        assert_eq!(a_size, (320, 240));
        assert_eq!(b_size, (320, 240));
        for channel in 0..3 {
            assert!(
                (mean(&a, channel) - mean(&b, channel)).abs() < 4.0,
                "channel {channel}: {} against {}",
                mean(&a, channel),
                mean(&b, channel)
            );
        }
        // Red stays red: the planes are blue first, the JPEG red first.
        let red = vec![[250u8, 10, 10]; 320 * 240].concat();
        let picture = crate::picture::Picture::from_rgb(&red, 320, 240).unwrap();
        let (rgb, _) = decode(&working_from_picture(&picture).unwrap());
        assert!(mean(&rgb, 0) > 200.0 && mean(&rgb, 2) < 60.0);
        // No picture, no frame.
        let empty = crate::picture::Picture {
            planes: Vec::new(),
            filled: (0, 0),
        };
        assert!(working_from_picture(&empty).is_err());
    }

    #[test]
    fn an_ask_without_a_side_is_bounded_as_before_and_one_with_a_side_by_its_own() {
        let before = Ask { id: 1, side: None };
        assert_eq!(before.bounds(), (64 * 1024 - TAG_BYTES, LONGEST_SIDE));
        let paper = Ask {
            id: 1,
            side: Some(3_264),
        };
        assert_eq!(paper.bounds(), (LARGE_MOST_BYTES - TAG_BYTES, 3_264));
        // Without a side the still is the one made before (#836).
        let frame = camera_frame(1_280, 960, 90);
        assert_eq!(
            fit_for(&frame, before).unwrap(),
            fit(&frame, 64 * 1024 - TAG_BYTES).unwrap()
        );
        // A side smaller than the frame's is kept to; a larger one is not
        // enlarged to.
        let (_, size) = fit_for(
            &frame,
            Ask {
                id: 1,
                side: Some(640),
            },
        )
        .unwrap();
        assert_eq!(size.0, 640);
        let (still, size) = fit_for(
            &frame,
            Ask {
                id: 1,
                side: Some(4_096),
            },
        )
        .unwrap();
        assert!(size.0 <= 1_280 && still.len() <= LARGE_MOST_BYTES - TAG_BYTES);
        // Tagged, it still fits the larger file.
        assert!(tagged(&still, u64::MAX).len() <= LARGE_MOST_BYTES);
    }

    /// The camera's largest mode (3264x2448, ZVCAI 01da:5875) asked for at
    /// its own side, as Desk tutor asks: made to fit the larger bound, and
    /// what it costs here. Decode is what every look pays at this size.
    #[test]
    fn the_camera_s_largest_frame_fits_the_larger_bound() {
        let large = camera_frame(3_264, 2_448, 90);
        let started = std::time::Instant::now();
        let picture = crate::picture::Picture::from_jpeg(&large).unwrap();
        let decoded = started.elapsed();
        assert_eq!(picture.filled, (320, 240));
        let started = std::time::Instant::now();
        let working = working(&large).unwrap();
        let worked = started.elapsed();
        assert!(working.len() <= crate::kept::MOST_BYTES);
        let started = std::time::Instant::now();
        let ask = Ask {
            id: 1,
            side: Some(3_264),
        };
        let (still, size) = fit_for(&large, ask).unwrap();
        eprintln!(
            "3264x2448 of {} bytes: the detector's decode {} ms, the working frame {} ms, a still of {size:?} and {} bytes in {} ms",
            large.len(),
            decoded.as_millis(),
            worked.as_millis(),
            still.len(),
            started.elapsed().as_millis()
        );
        assert!(still.len() <= LARGE_MOST_BYTES - TAG_BYTES);
        // 4:3 within the rounding of each shrink.
        assert!(size.0.abs_diff(size.1 * 4 / 3) <= 2 && size.0 > LONGEST_SIDE);
        assert!(crate::kept::whole_jpeg(&still));
    }

    #[test]
    fn the_working_frame_of_a_full_size_stream_is_320_by_240() {
        let (working, size) = fit_within(
            &camera_frame(1_280, 960, 90),
            crate::kept::MOST_BYTES,
            WORKING_SIDE,
        )
        .unwrap();
        assert_eq!(size, (320, 240));
        assert!(crate::kept::whole_jpeg(&working));
        assert!(working.len() <= crate::kept::MOST_BYTES);
        assert_eq!(
            super::working(&camera_frame(1_280, 960, 90)).unwrap(),
            working
        );
        // A 320x240 frame, as a stream fallen back to it gives, goes as it is.
        let small = camera_frame(320, 240, 80);
        assert_eq!(super::working(&small).unwrap(), small);
    }

    #[test]
    fn shrinking_averages_what_each_pixel_covers() {
        // Two by two of black and white is one grey pixel.
        let rgb = [0, 0, 0, 255, 255, 255, 255, 255, 255, 0, 0, 0];
        assert_eq!(shrink(&rgb, (2, 2), 1), (vec![128, 128, 128], (1, 1)));
        // Never enlarged.
        assert_eq!(shrink(&rgb, (2, 2), 10), (rgb.to_vec(), (2, 2)));
    }
}
