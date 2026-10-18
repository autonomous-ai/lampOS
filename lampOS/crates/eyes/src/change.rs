//! With nobody in view, the detector is not run on a frame like the last
//! one it ran on (`--look-on-change on`, off unless given; #705, the lamp's
//! heat).
//!
//! With nobody in view the eyes look every 600 ms, and each look costs
//! lamp-4ace 104 to 130 ms of a core (the 10 s lines, 02:00 to 05:00 on
//! 18 Oct): about 20% of one core all night for an empty room. A person
//! who comes into view changes the picture, so a frame like the last one
//! the detector ran on, with nobody in that one, holds nobody either. Such
//! a frame is said to hold no face without the detector, and the detector
//! still runs at least every [`LOOK_AT_LEAST_US`], whatever the picture,
//! and at every glance until the picture has stayed still for
//! [`SETTLE_GLANCES`] glances in a row: a person who walks in and sits
//! down is detected at each glance while they move and at the two after,
//! so a frame the detector misses (dim, half turned) costs a glance, not
//! 3 s (the reviewer on #1441). It was 3 s of glances after each change;
//! on lamp-4ace's empty office that kept 64 % of the glances detected as
//! "settling" (#1460, 18 Oct, 11:22 to 11:42).
//!
//! A frame is compared as a grid of [`ACROSS`] by [`DOWN`] cells, each the
//! mean grey of its pixels: a sensor's noise averages out over a cell's
//! hundred pixels, a face does not. It changed when [`CELLS`] cells or more
//! are [`GREYS`] or more brighter or darker than in the last frame the
//! detector ran on, not the last one seen, so a slow change adds up.
use crate::picture::{HEIGHT, Picture, WIDTH};

pub const ACROSS: usize = 32;
pub const DOWN: usize = 24;
/// A cell this much brighter or darker, of 255, has changed.
pub const GREYS: f32 = 12.0;
/// A cell's own noise, learned from the glances of an empty room, counts
/// this many times over: a cell changed past [`GREYS`] and past this many
/// times its noise has changed. The noise rises five times slower than it
/// falls, so it settles low, at about 0.4 of a steady noise's spread. Four
/// times that (as it was) is 1.8 spreads, which about 48 of 768 cells pass
/// on noise alone in a simulation of steady noise, consistent with
/// lamp-4ace's empty office (33 a glance, #1479, 18 Oct, 14:26) though not
/// yet shown to be its cause; eight times, with the rates below, is about 3.7
/// spreads: under one cell a glance.
pub const NOISE_TIMES: f32 = 8.0;
/// How much of a cell's noise each compared glance teaches, rising and
/// falling: it falls about ten glances back (six seconds), and rises five
/// times slower, so a few seconds of a person moving in one place unseen
/// do not teach their cells to be noise (the reviewer on #1460: at one
/// rate, 19 to 22 of such a person's 40 glances were skipped).
///
/// The rates and [`NOISE_TIMES`] are chosen together (a simulation of the
/// three: steady noise, a person moving 50 greys in one place, a flicker of
/// 80 that stops): under one cell a glance of steady noise passes; the
/// person stays a change for fourteen glances (8 s); a cell is deaf for
/// about 26 glances (16 s) after a learned flicker stops, while the 3 s look
/// still runs. At 0.02 and 0.1 with four times, 48 cells passed on noise.
const NOISE_RISES: f32 = 0.01;
const NOISE_FALLS: f32 = 0.05;
/// This many changed cells are a changed frame: a face at 0.15 of the
/// picture's height (`sight::LEAST_SIZE`, the smallest the body turns to)
/// changes six or more cells from 15 greys of contrast. A smaller face,
/// or a fainter one, may change fewer and wait for the 3 s look.
pub const CELLS: usize = 3;
/// The detector runs at least this often, changed or not.
pub const LOOK_AT_LEAST_US: u64 = 3_000_000;
/// And at every glance until the picture has been still this many glances
/// in a row after a change.
pub const SETTLE_GLANCES: u32 = 2;

/// A frame as the cells' mean greys.
#[derive(Clone, Debug, PartialEq)]
pub struct Grid(Vec<f32>);

impl Grid {
    /// The part of `picture` the frame fills, cut into the cells.
    pub fn of(picture: &Picture) -> Self {
        let (wide, tall) = picture.filled;
        let plane = WIDTH * HEIGHT;
        let mut sums = vec![0f32; ACROSS * DOWN];
        let mut counts = vec![0u32; ACROSS * DOWN];
        for y in 0..tall {
            let row = (y * DOWN / tall.max(1)).min(DOWN - 1) * ACROSS;
            for x in 0..wide {
                let at = y * WIDTH + x;
                let grey = (picture.planes[at]
                    + picture.planes[plane + at]
                    + picture.planes[2 * plane + at])
                    / 3.0;
                let cell = row + (x * ACROSS / wide.max(1)).min(ACROSS - 1);
                sums[cell] += grey;
                counts[cell] += 1;
            }
        }
        Self(
            sums.iter()
                .zip(&counts)
                .map(|(sum, count)| sum / (*count).max(1) as f32)
                .collect(),
        )
    }

    /// How many cells are [`GREYS`] or more brighter or darker.
    fn cells_changed_from(&self, before: &Self) -> usize {
        self.0
            .iter()
            .zip(&before.0)
            .filter(|(now, then)| (*now - *then).abs() >= GREYS)
            .count()
    }

    /// Each cell's change from `before`, less its row's median change: what
    /// the whole frame did (the camera's exposure) and what a whole row did
    /// (a light's flicker, which a rolling shutter draws as bands across)
    /// are taken out, and what moved in one place stays (#1441: 49 cells a
    /// frame changed past the bar in lamp-4ace's empty office).
    fn residuals_from(&self, before: &Self) -> Vec<f32> {
        let mut residuals = Vec::with_capacity(ACROSS * DOWN);
        let mut row = [0f32; ACROSS];
        for (now, then) in self
            .0
            .chunks_exact(ACROSS)
            .zip(before.0.chunks_exact(ACROSS))
        {
            for ((delta, now), then) in row.iter_mut().zip(now).zip(then) {
                *delta = now - then;
            }
            let mut sorted = row;
            sorted.sort_unstable_by(f32::total_cmp);
            let median = (sorted[ACROSS / 2 - 1] + sorted[ACROSS / 2]) / 2.0;
            residuals.extend(row.iter().map(|delta| delta - median));
        }
        residuals
    }

    /// Changed past the bar with the row's shift taken out, and no noise
    /// learned yet.
    #[cfg(test)]
    fn changed_from(&self, before: &Self) -> bool {
        self.residuals_from(before)
            .iter()
            .filter(|residual| residual.abs() >= GREYS)
            .count()
            >= CELLS
    }
}

/// Whether the detector runs on this glance: always without
/// `--look-on-change` (`change` is `None`), else as [`Change::detect`] says.
pub fn detect(
    change: Option<&mut Change>,
    picture: &Picture,
    busy: Option<Busy>,
    now_us: u64,
) -> bool {
    change.is_none_or(|change| change.detect(Grid::of(picture), busy, now_us))
}

/// Whether the lamp's own light is changing, as the eyes see it at each
/// glance: `light/state` differs from one of the two glances before (a
/// change and the ring's fade into it, about 1.2 s). The ring lights what
/// the camera sees, and on lamp-4ace the twenty minutes with the light
/// scene changed far more cells than a quiet twenty (#1479, 18 Oct; the two
/// were different builds, so that is a pointer, not a measure). A change
/// the camera sees then is the lamp's own, not someone coming in; the 3 s
/// look still runs. The lamp speaking is not counted: people come over or
/// turn to a lamp exactly when it talks (the reviewer on #1479), and the
/// ring's speaking face changes no colour.
#[derive(Debug, Default)]
pub struct OwnLight {
    seen: std::collections::VecDeque<Option<lamp_msgs::Light>>,
}

impl OwnLight {
    /// This glance's `light/state` (none if unknown): whether a change seen
    /// now is the lamp's own.
    pub fn lit(&mut self, light: Option<lamp_msgs::Light>) -> bool {
        self.seen.push_back(light);
        if self.seen.len() > 3 {
            self.seen.pop_front();
        }
        let newest = self.seen.back().cloned().flatten();
        self.seen
            .iter()
            .rev()
            .skip(1)
            .any(|before| before.is_some() && newest.is_some() && *before != newest)
    }
}

/// Nobody has been found for this long: the room counts as empty.
pub const EMPTY_AFTER_US: u64 = 4_000_000;

/// Why the room does not count as empty, so every glance is detected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Busy {
    /// A full look is asked for.
    Asked,
    /// Someone is being remembered.
    Remembering,
    /// A face was found under [`EMPTY_AFTER_US`] ago.
    Face,
}

/// Why the room is not empty, if it is not: a full look asked for, someone
/// being remembered, or a face found under [`EMPTY_AFTER_US`] ago (or ever
/// none of them, and it is empty).
pub fn busy(asked: bool, remembering: bool, face_ago_us: Option<u64>) -> Option<Busy> {
    if asked {
        Some(Busy::Asked)
    } else if remembering {
        Some(Busy::Remembering)
    } else if face_ago_us.is_some_and(|ago| ago < EMPTY_AFTER_US) {
        Some(Busy::Face)
    } else {
        None
    }
}

/// Whether the room counts as empty: [`busy`] says no reason.
pub fn empty_room(asked: bool, remembering: bool, face_ago_us: Option<u64>) -> bool {
    busy(asked, remembering, face_ago_us).is_none()
}

/// Why each glance was detected or not, since the last
/// [`Change::take_why`], for the 10 s line: what a night on a lamp says
/// about where the detector still runs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Why {
    pub asked: u32,
    pub remembering: u32,
    pub face: u32,
    /// The first glance, with nothing to compare.
    pub first: u32,
    /// The picture changed past the bar from the last frame detected.
    pub changed: u32,
    /// Changed, but while the lamp's own light was changing
    /// ([`OwnLight`]): not counted as a change.
    pub lit: u32,
    /// Unchanged, but fewer than [`SETTLE_GLANCES`] glances since a change.
    pub settling: u32,
    /// Unchanged and still, but [`LOOK_AT_LEAST_US`] since the last.
    pub every: u32,
    /// Not detected.
    pub skipped: u32,
    /// The most cells changed in a glance compared, and the sum, over
    /// `compared` glances: against the bar of [`CELLS`]. These are the
    /// cells that decide: past [`GREYS`] and their own noise, after the
    /// row's shift is taken out.
    pub most_cells: usize,
    pub cells: usize,
    pub compared: u32,
    /// The same glances counted the plain way (each cell's change from the
    /// frame, past [`GREYS`]) and with only the row's shift taken out, to
    /// say which of the two the room's noise was.
    pub plain_cells: usize,
    pub row_cells: usize,
    /// The sum of each compared glance's median shift over the frame, its
    /// size in greys: the camera's exposure.
    pub frame_shift: f32,
    /// The sum over compared glances of the median and the 90th percentile
    /// of their cells' changes, the row's shift taken out, in greys: what
    /// an empty room's own picture does from glance to glance (#1479 on
    /// lamp-4ace: 33 cells a glance past the bar after eleven minutes).
    pub residual_median: f32,
    pub residual_p90: f32,
    /// The cells' learned noise when the line was taken, median and 90th
    /// percentile over the cells: what the noise settled to.
    pub noise_median: f32,
    pub noise_p90: f32,
}

impl Why {
    pub fn line(&self) -> String {
        format!(
            "; not detected while the lamp's light changed: {}; detected: {} changed, {} settling, {} every 3 s, {} first, {} near a face, {} asked, {} remembering; cells changed over {} empty-room glances: most {}, mean {:.1} (bar {CELLS}); plain mean {:.1}, rows taken out {:.1}; the frame's shift {:.1} greys",
            self.lit,
            self.changed,
            self.settling,
            self.every,
            self.first,
            self.face,
            self.asked,
            self.remembering,
            self.compared,
            self.most_cells,
            self.cells as f32 / self.compared.max(1) as f32,
            self.plain_cells as f32 / self.compared.max(1) as f32,
            self.row_cells as f32 / self.compared.max(1) as f32,
            self.frame_shift / self.compared.max(1) as f32
        ) + &format!(
            "; a glance's cells changed by, median {:.1} and p90 {:.1} greys; the learned noise over cells, median {:.1} and p90 {:.1} (a cell counts past {GREYS} and {NOISE_TIMES}x it)",
            self.residual_median / self.compared.max(1) as f32,
            self.residual_p90 / self.compared.max(1) as f32,
            self.noise_median,
            self.noise_p90
        )
    }
}

/// When the detector last ran, and on what, and when the picture last
/// changed.
#[derive(Default)]
pub struct Change {
    last: Option<(Grid, u64)>,
    /// Unchanged glances in a row since the last change, if there was one.
    since_change: Option<u32>,
    /// Each cell's noise, learned from empty-room glances; empty until the
    /// first.
    noise: Vec<f32>,
    /// The lamp's own light is changing, or it speaks, for this glance
    /// ([`Change::set_lit`]).
    lit: bool,
    why: Why,
}

impl Change {
    /// Whether the lamp's own light is changing for the next glance
    /// ([`OwnLight::lit`]): a change then is the lamp's, not the room's.
    pub fn set_lit(&mut self, lit: bool) {
        self.lit = lit;
    }

    /// Whether the detector runs on this frame. `busy`: why the room is not
    /// empty, if it is not; then it always runs.
    pub fn detect(&mut self, grid: Grid, busy: Option<Busy>, now_us: u64) -> bool {
        let compared = self.last.as_ref().map(|(last, _)| {
            let residuals = grid.residuals_from(last);
            if self.noise.len() != residuals.len() {
                self.noise = vec![0.0; residuals.len()];
            }
            let cells = residuals
                .iter()
                .zip(&self.noise)
                .filter(|(residual, noise)| residual.abs() >= GREYS.max(NOISE_TIMES * **noise))
                .count();
            let row_cells = residuals
                .iter()
                .filter(|residual| residual.abs() >= GREYS)
                .count();
            let mut deltas: Vec<f32> = grid
                .0
                .iter()
                .zip(&last.0)
                .map(|(now, then)| now - then)
                .collect();
            deltas.sort_unstable_by(f32::total_cmp);
            let shift = deltas[deltas.len() / 2].abs();
            (
                cells,
                row_cells,
                grid.cells_changed_from(last),
                shift,
                residuals,
            )
        });
        let cells = compared.as_ref().map(|compared| compared.0);
        let lit = self.lit && cells.is_some_and(|cells| cells >= CELLS);
        let changed = !self.lit && cells.is_some_and(|cells| cells >= CELLS);
        self.since_change = if changed {
            Some(0)
        } else {
            self.since_change.map(|still| still.saturating_add(1))
        };
        // The changed glance and the [`SETTLE_GLANCES`] still ones after it.
        let moving = self
            .since_change
            .is_some_and(|still| still <= SETTLE_GLANCES);
        let due = self
            .last
            .as_ref()
            .is_none_or(|(_, at)| now_us.saturating_sub(*at) >= LOOK_AT_LEAST_US);
        let why = &mut self.why;
        if busy.is_none()
            && let Some((cells, row_cells, plain_cells, shift, residuals)) = compared
        {
            why.most_cells = why.most_cells.max(cells);
            why.cells += cells;
            why.compared += 1;
            why.row_cells += row_cells;
            why.plain_cells += plain_cells;
            why.frame_shift += shift;
            let mut sizes: Vec<f32> = residuals.iter().map(|residual| residual.abs()).collect();
            sizes.sort_unstable_by(f32::total_cmp);
            why.residual_median += sizes[sizes.len() / 2];
            why.residual_p90 += sizes[sizes.len() * 9 / 10];
            // The room's own noise, learned only while it is empty, not
            // from the lamp's own light, and after the glance is judged.
            for (noise, residual) in self.noise.iter_mut().zip(residuals).filter(|_| !self.lit) {
                let towards = residual.abs() - *noise;
                *noise += towards
                    * if towards > 0.0 {
                        NOISE_RISES
                    } else {
                        NOISE_FALLS
                    };
            }
        }
        let counted = match busy {
            Some(Busy::Asked) => &mut why.asked,
            Some(Busy::Remembering) => &mut why.remembering,
            Some(Busy::Face) => &mut why.face,
            None if cells.is_none() => &mut why.first,
            None if changed => &mut why.changed,
            None if lit && !moving && !due => &mut why.lit,
            None if moving => &mut why.settling,
            None if due => &mut why.every,
            None => &mut why.skipped,
        };
        *counted += 1;
        let detect = busy.is_some() || moving || due;
        if detect {
            self.last = Some((grid, now_us));
        }
        detect
    }

    /// Looks said to hold no face without the detector, since the last
    /// [`Change::take_why`].
    pub fn skipped(&self) -> u32 {
        self.why.skipped + self.why.lit
    }

    pub fn take_why(&mut self) -> Why {
        if !self.noise.is_empty() {
            let mut noise = self.noise.clone();
            noise.sort_unstable_by(f32::total_cmp);
            self.why.noise_median = noise[noise.len() / 2];
            self.why.noise_p90 = noise[noise.len() * 9 / 10];
        }
        std::mem::take(&mut self.why)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A room at 320 by 240: a grey wall with a darker desk, the sensor's
    /// noise of up to 4 greys on every pixel, and a box drawn in.
    fn room(seed: u32, drift: f32, face: Option<(usize, usize, usize)>) -> Picture {
        scene(seed, drift, false, 0.0, face)
    }

    /// `room`, with the wall's top right `lit` 40 greys brighter, as the
    /// ring's light falls on it.
    fn scene_lit(seed: u32, lit: bool) -> Picture {
        let plain = scene(seed, 0.0, false, 0.0, None);
        let mut rgb = Vec::with_capacity(320 * 240 * 3);
        for y in 0..240usize {
            for x in 0..320usize {
                let at = y * WIDTH + x;
                let plane = WIDTH * HEIGHT;
                let lift = if lit && x >= 200 && y < 120 {
                    40.0
                } else {
                    0.0
                };
                for p in [2, 1, 0] {
                    rgb.push((plain.planes[p * plane + at] + lift).min(255.0) as u8);
                }
            }
        }
        Picture::from_rgb(&rgb, 320, 240).unwrap()
    }

    /// `room`, with a light's flicker drawn as bands across, of up to 30
    /// greys a row and new each frame, and a screen in the top left corner
    /// (40 by 30 pixels) `screen` greys brighter or darker than the wall.
    fn scene(
        seed: u32,
        drift: f32,
        bands: bool,
        screen: f32,
        face: Option<(usize, usize, usize)>,
    ) -> Picture {
        let mut noise = seed;
        let mut rgb = Vec::with_capacity(320 * 240 * 3);
        let mut band = 0.0;
        for y in 0..240usize {
            if bands && y % 6 == 0 {
                noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                band = ((noise >> 24) % 61) as f32 - 30.0;
            }
            for x in 0..320usize {
                noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let wobble = ((noise >> 24) % 9) as f32 - 4.0;
                let mut grey = if y > 170 { 70.0 } else { 150.0 } + drift + band + wobble;
                if x < 40 && y < 30 {
                    grey += screen;
                }
                if let Some((left, top, side)) = face
                    && (left..left + side * 3 / 4).contains(&x)
                    && (top..top + side).contains(&y)
                {
                    grey = 205.0 + wobble;
                }
                let grey = grey.clamp(0.0, 255.0) as u8;
                rgb.extend([grey, grey, grey]);
            }
        }
        Picture::from_rgb(&rgb, 320, 240).unwrap()
    }

    const GLANCE_US: u64 = 600_000;

    /// (a) The sensor's noise is no change; a face at 0.2 of the height
    /// coming into a corner is, and so is one at 0.14 on a plain wall. The
    /// whole frame's light changing, however far, is not: the camera's
    /// exposure or the room's light, which the 3 s look still covers.
    #[test]
    fn noise_is_no_change_and_a_face_is_and_the_frames_light_is_not() {
        let empty = Grid::of(&room(1, 0.0, None));
        for seed in 2..20 {
            assert!(
                !Grid::of(&room(seed, 0.0, None)).changed_from(&empty),
                "{seed}"
            );
        }
        for (corner, side) in [((0, 0), 48), ((270, 190), 48), ((150, 60), 34)] {
            let face = Grid::of(&room(3, 0.0, Some((corner.0, corner.1, side))));
            assert!(face.changed_from(&empty), "{corner:?} {side}");
        }
        // The light rising five greys a glance, 25 in all against the last
        // frame detected: no change.
        let mut change = Change::default();
        assert!(change.detect(empty.clone(), None, 0));
        let mut detected = Vec::new();
        for glance in 1..=5u64 {
            let grid = Grid::of(&room(glance as u32 + 5, 5.0 * glance as f32, None));
            if change.detect(grid, None, glance * GLANCE_US / 10) {
                detected.push(glance);
            }
        }
        assert!(detected.is_empty(), "{detected:?}");
        let why = change.take_why();
        assert!(why.plain_cells > 0 && why.frame_shift > 0.0, "{why:?}");
    }

    /// (a) #1441 on lamp-4ace: the empty office's picture changed 49 cells
    /// a frame. A light's flicker, bands of up to 30 greys new each frame,
    /// and the frame's exposure moving with it, are no change; nor, once
    /// learned, is a screen that flickers in one corner. A face coming in
    /// through all of that still is, at once.
    #[test]
    fn flicker_bands_exposure_and_a_screen_are_no_change_but_a_face_is() {
        let mut change = Change::default();
        let mut detected = Vec::new();
        for glance in 0..40u64 {
            let screen = if glance % 2 == 0 { 40.0 } else { -40.0 };
            let face = (glance == 30).then_some((200, 80, 48));
            let exposure = (glance % 3) as f32 * 6.0;
            let grid = Grid::of(&scene(glance as u32, exposure, true, screen, face));
            if change.detect(grid, None, glance * GLANCE_US) {
                detected.push(glance);
            }
        }
        // The first glance, the screen while its noise is learned (it
        // changes every glance until then), then the 3 s look; the face at
        // 30 at once, and the glances after it settling.
        let why = change.take_why();
        assert!(detected.contains(&30), "{detected:?}");
        assert!(why.skipped >= 10, "{why:?} {detected:?}");
        assert!(why.plain_cells > why.row_cells, "{why:?}");
        // Without the screen, flicker and exposure alone never count.
        let mut change = Change::default();
        let mut detected = Vec::new();
        for glance in 0..20u64 {
            let grid = Grid::of(&scene(
                glance as u32,
                (glance % 3) as f32 * 6.0,
                true,
                0.0,
                None,
            ));
            if change.detect(grid, None, glance * GLANCE_US) {
                detected.push(glance);
            }
        }
        assert_eq!(detected, [0, 5, 10, 15]);
    }

    /// (b) An empty, still room is detected once every 3 s; a changed frame
    /// at the next glance; and a room that is not empty at every glance.
    #[test]
    fn an_empty_still_room_is_detected_every_three_seconds_and_a_change_at_once() {
        let mut change = Change::default();
        let mut detected = Vec::new();
        for glance in 0..20u64 {
            let face = (glance == 12).then_some((100, 50, 48));
            let grid = Grid::of(&room(glance as u32, 0.0, face));
            if change.detect(grid, None, glance * GLANCE_US) {
                detected.push(glance);
            }
        }
        // 0 s, 3 s, 6 s; the face at 7.2 s; back to empty at 7.8 s, and
        // the two still glances after it.
        assert_eq!(detected, [0, 5, 10, 12, 13, 14, 15]);
        assert_eq!(change.take_why().skipped, 13);
        assert_eq!(change.take_why().skipped, 0);
        for glance in 20..30u64 {
            let grid = Grid::of(&room(glance as u32, 0.0, None));
            assert!(change.detect(grid, Some(Busy::Face), glance * GLANCE_US));
        }
        assert_eq!(change.take_why().skipped, 0);
    }

    /// #1479 on lamp-4ace: an empty room whose cells each move by a few
    /// greys from glance to glance, past the bar of 12 now and then (a
    /// spread of 8 greys a cell, as no sensor's grain gives but the room's
    /// light and the encoder can), is still once its noise is learned: of
    /// the glances after the first minute, nearly none count as changed.
    #[test]
    fn a_room_whose_cells_each_move_a_little_is_still_once_learned() {
        let mut change = Change::default();
        let mut rng = 7u32;
        let mut changed_late = 0;
        for glance in 0..200u64 {
            let mut grid = Grid::of(&room(glance as u32, 0.0, None));
            for cell in grid.0.iter_mut() {
                // About normal, spread 8: the sum of four uniform draws.
                let mut sum = 0.0;
                for _ in 0..4 {
                    rng = rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    sum += (rng >> 8) as f32 / (1u32 << 24) as f32 - 0.5;
                }
                *cell += sum * 8.0 * 3.46;
            }
            change.detect(grid, None, glance * GLANCE_US);
            if glance == 100 {
                change.take_why();
            }
        }
        changed_late += change.take_why().changed;
        assert!(
            changed_late <= 5,
            "{changed_late} of the last 100 glances changed"
        );
    }

    /// The reviewer on #1479: a cell's noise falls five times faster than
    /// it rises, so a flicker that stops (a screen switched off) does not
    /// leave its cells deaf for long: a face at its place thirty glances
    /// later is seen.
    #[test]
    fn a_flicker_that_stops_does_not_leave_its_cells_deaf_for_long() {
        let mut change = Change::default();
        for glance in 0..30u64 {
            let screen = if glance % 2 == 0 { 40.0 } else { -40.0 };
            change.detect(
                Grid::of(&scene(glance as u32, 0.0, false, screen, None)),
                None,
                glance * GLANCE_US,
            );
        }
        for glance in 30..60u64 {
            change.detect(
                Grid::of(&scene(glance as u32, 0.0, false, 0.0, None)),
                None,
                glance * GLANCE_US,
            );
        }
        let before = change.take_why();
        let face = Grid::of(&scene(60, 0.0, false, 0.0, Some((0, 0, 28))));
        change.detect(face, None, 60 * GLANCE_US);
        assert_eq!(change.take_why().changed, 1, "{before:?}");
    }

    /// #1479 on lamp-4ace: the lamp's own light lights what the camera
    /// sees. A change while it changes (or while the lamp speaks) is not a
    /// change; the first glance after is, once, against the frame from
    /// before, and then the room is still again. A person coming in while
    /// the light changes is seen at that first glance after.
    #[test]
    fn the_lamps_own_light_changing_is_not_a_change() {
        let mut change = Change::default();
        let mut detected = Vec::new();
        for glance in 0..16u64 {
            // The ring turns warm at glance 3: the wall nearest it, the
            // top right, is 40 greys brighter from then on.
            let lit_wall = glance >= 3;
            change.set_lit(matches!(glance, 3 | 4));
            let grid = Grid::of(&scene_lit(glance as u32, lit_wall));
            if change.detect(grid, None, glance * GLANCE_US) {
                detected.push(glance);
            }
        }
        let why = change.take_why();
        // 0 first; 3 and 4 lit and not detected; 5 the change, once, and
        // two settling; then the 3 s look.
        assert_eq!(detected, [0, 5, 6, 7, 12], "{why:?}");
        assert_eq!((why.lit, why.changed), (2, 1), "{why:?}");
        // `light/state` changing makes this glance and the next lit.
        let warm = |level| {
            Some(lamp_msgs::Light {
                on: true,
                colour: "warm".into(),
                level,
            })
        };
        let mut own = OwnLight::default();
        let lit: Vec<bool> = [warm(3), warm(3), warm(4), warm(4), warm(4), None, warm(4)]
            .into_iter()
            .map(|light| own.lit(light))
            .collect();
        assert_eq!(lit, [false, false, true, true, false, false, false]);
    }

    /// The reviewer on #1479: someone who walks up while the lamp speaks,
    /// its light steady, is detected at once, as in a quiet room: the
    /// voice speaking makes no glance the lamp's own.
    #[test]
    fn a_walk_in_while_the_lamp_speaks_is_detected_at_once() {
        let warm = Some(lamp_msgs::Light {
            on: true,
            colour: "warm".into(),
            level: 3,
        });
        let mut own = OwnLight::default();
        let mut change = Change::default();
        let mut detected = Vec::new();
        for glance in 0..20u64 {
            let face = match glance {
                10..=13 => Some((40 + 50 * (glance as usize - 10), 60, 60)),
                14.. => Some((190, 60, 60)),
                _ => None,
            };
            change.set_lit(own.lit(warm.clone()));
            if change.detect(
                Grid::of(&room(glance as u32, 0.0, face)),
                None,
                glance * GLANCE_US,
            ) {
                detected.push(glance);
            }
        }
        assert!(
            detected.starts_with(&[0, 5, 10, 11, 12, 13]),
            "{detected:?}"
        );
    }

    /// The reviewer on #1460: someone moving in one place unseen (pacing,
    /// fidgeting, their back turned) is not learned as noise in a few
    /// seconds: a cell's noise rises five times slower than it falls, so
    /// their first eight seconds of glances are all detected.
    #[test]
    fn a_person_moving_in_one_place_is_not_learned_as_noise_in_seconds() {
        let mut change = Change::default();
        let mut detected = Vec::new();
        for glance in 0..30u64 {
            let fidget = if glance % 2 == 0 { 25.0 } else { -25.0 };
            let grid = Grid::of(&scene(glance as u32, 0.0, false, fidget, None));
            if change.detect(grid, None, glance * GLANCE_US) {
                detected.push(glance);
            }
        }
        assert!(
            detected.starts_with(&(0..14).collect::<Vec<_>>()),
            "{detected:?}"
        );
        // Learned in time, as a screen is: the rest are not all detected.
        assert!(detected.len() < 30, "{detected:?}");
    }

    /// (b) Someone walks in over four glances and sits still: every
    /// glance while they move and the two still ones after are detected,
    /// so a frame the detector misses costs a glance, not 3 s.
    #[test]
    fn a_walk_in_and_sit_down_is_detected_at_every_glance_until_still_for_two() {
        let mut change = Change::default();
        let mut detected = Vec::new();
        for glance in 0..30u64 {
            let face = match glance {
                10..=13 => Some((40 + 50 * (glance as usize - 10), 60, 60)),
                14.. => Some((190, 60, 60)),
                _ => None,
            };
            let grid = Grid::of(&room(glance as u32, 0.0, face));
            if change.detect(grid, None, glance * GLANCE_US) {
                detected.push(glance);
            }
        }
        // Sat down where the fourth step put them (glance 13, 7.8 s): the
        // last change, so 14 and 15 too, then every 3 s.
        assert_eq!(detected, [0, 5, 10, 11, 12, 13, 14, 15, 20, 25]);
    }

    /// (c) Without the switch every glance is detected; with it, an empty,
    /// still room every 3 s.
    #[test]
    fn without_the_switch_every_glance_is_detected() {
        let picture = room(1, 0.0, None);
        let mut change = Change::default();
        let mut on = Vec::new();
        for glance in 0..20u64 {
            assert!(detect(None, &picture, None, glance * GLANCE_US));
            if detect(Some(&mut change), &picture, None, glance * GLANCE_US) {
                on.push(glance);
            }
        }
        assert_eq!(on, [0, 5, 10, 15]);
    }

    /// Each glance is counted by why it was detected or not, with the
    /// cells changed, for the 10 s line.
    #[test]
    fn each_glance_is_counted_by_why() {
        let mut change = Change::default();
        for glance in 0..20u64 {
            let face = (glance == 12).then_some((100, 50, 48));
            change.detect(
                Grid::of(&room(glance as u32, 0.0, face)),
                None,
                glance * GLANCE_US,
            );
        }
        for glance in 20..23u64 {
            let grid = Grid::of(&room(glance as u32, 0.0, None));
            change.detect(grid, Some(Busy::Face), glance * GLANCE_US);
        }
        let grid = Grid::of(&room(23, 0.0, None));
        change.detect(grid, Some(Busy::Asked), 23 * GLANCE_US);
        let why = change.take_why();
        // As `an_empty_still_room_is_…`: 0 first; 5 and 10 every 3 s; the
        // face in and out at 12 and 13; settling at 14 and 15; thirteen
        // skipped.
        assert_eq!(
            (why.first, why.every, why.changed, why.settling, why.skipped),
            (1, 2, 2, 2, 13)
        );
        assert_eq!((why.face, why.asked, why.remembering), (3, 1, 0));
        assert_eq!(why.compared, 19);
        assert!(why.most_cells >= CELLS && why.cells >= 2 * CELLS, "{why:?}");
        assert!(why.line().contains("2 changed, 2 settling, 2 every 3 s"));
        // What the room's picture does, and the noise learned: a glance's
        // cells move by under a grey of sensor noise, a face's by far more.
        assert!(
            why.residual_median > 0.0 && why.residual_p90 >= why.residual_median,
            "{why:?}"
        );
        assert!(
            why.noise_p90 >= why.noise_median && why.noise_median > 0.0,
            "{why:?}"
        );
        assert!(
            why.line().contains("the learned noise over cells, median"),
            "{}",
            why.line()
        );
        // A second take starts the counts again; the noise is still the
        // cells' own, as learned.
        let again = change.take_why();
        assert_eq!(
            Why {
                noise_median: again.noise_median,
                noise_p90: again.noise_p90,
                ..Why::default()
            },
            again
        );
        assert_eq!(busy(false, false, Some(1)), Some(Busy::Face));
        assert_eq!(busy(true, true, Some(1)), Some(Busy::Asked));
        assert_eq!(busy(false, true, None), Some(Busy::Remembering));
        assert_eq!(busy(false, false, None), None);
    }

    /// (b) A face found under 4 s ago, a full look asked for or someone
    /// being remembered is never an empty room.
    #[test]
    fn a_face_an_ask_or_a_remember_is_never_an_empty_room() {
        assert!(empty_room(false, false, None));
        assert!(empty_room(false, false, Some(EMPTY_AFTER_US)));
        assert!(!empty_room(false, false, Some(EMPTY_AFTER_US - 1)));
        assert!(!empty_room(true, false, None));
        assert!(!empty_room(false, true, None));
    }

    /// A frame the working size is not (a full look's working frame is
    /// shrunk into the corner) is cut by what it fills.
    #[test]
    fn the_grid_is_of_what_the_frame_fills() {
        let rgb = vec![90u8; 320 * 180 * 3];
        let grid = Grid::of(&Picture::from_rgb(&rgb, 320, 180).unwrap());
        assert!(grid.0.iter().all(|grey| (*grey - 90.0).abs() < 1e-3));
    }
}
