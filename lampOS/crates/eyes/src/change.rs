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
//! [`STILL_FOR_US`]: a person who walks in and sits down is detected at
//! each glance while they move and for 3 s after, so a frame the detector
//! misses (dim, half turned) costs a glance, not 3 s (the reviewer on
//! #1441).
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
/// This many changed cells are a changed frame: a face at 0.15 of the
/// picture's height (`sight::LEAST_SIZE`, the smallest the body turns to)
/// changes six or more cells from 15 greys of contrast. A smaller face,
/// or a fainter one, may change fewer and wait for the 3 s look.
pub const CELLS: usize = 3;
/// The detector runs at least this often, changed or not.
pub const LOOK_AT_LEAST_US: u64 = 3_000_000;
/// And at every glance until the picture has been still this long.
pub const STILL_FOR_US: u64 = 3_000_000;

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

    #[cfg(test)]
    fn changed_from(&self, before: &Self) -> bool {
        self.cells_changed_from(before) >= CELLS
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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Why {
    pub asked: u32,
    pub remembering: u32,
    pub face: u32,
    /// The first glance, with nothing to compare.
    pub first: u32,
    /// The picture changed past the bar from the last frame detected.
    pub changed: u32,
    /// Unchanged, but changed under [`STILL_FOR_US`] ago.
    pub settling: u32,
    /// Unchanged and still, but [`LOOK_AT_LEAST_US`] since the last.
    pub every: u32,
    /// Not detected.
    pub skipped: u32,
    /// The most cells changed in a glance compared, and the sum, over
    /// `compared` glances: against the bar of [`CELLS`].
    pub most_cells: usize,
    pub cells: usize,
    pub compared: u32,
}

impl Why {
    pub fn line(&self) -> String {
        format!(
            "; detected: {} changed, {} settling, {} every 3 s, {} first, {} near a face, {} asked, {} remembering; cells changed: most {}, mean {:.1} (bar {CELLS})",
            self.changed,
            self.settling,
            self.every,
            self.first,
            self.face,
            self.asked,
            self.remembering,
            self.most_cells,
            self.cells as f32 / self.compared.max(1) as f32
        )
    }
}

/// When the detector last ran, and on what, and when the picture last
/// changed.
#[derive(Default)]
pub struct Change {
    last: Option<(Grid, u64)>,
    changed_at: Option<u64>,
    why: Why,
}

impl Change {
    /// Whether the detector runs on this frame. `busy`: why the room is not
    /// empty, if it is not; then it always runs.
    pub fn detect(&mut self, grid: Grid, busy: Option<Busy>, now_us: u64) -> bool {
        let cells = self
            .last
            .as_ref()
            .map(|(last, _)| grid.cells_changed_from(last));
        let changed = cells.is_some_and(|cells| cells >= CELLS);
        if changed {
            self.changed_at = Some(now_us);
        }
        let moving = self
            .changed_at
            .is_some_and(|at| now_us.saturating_sub(at) < STILL_FOR_US);
        let due = self
            .last
            .as_ref()
            .is_none_or(|(_, at)| now_us.saturating_sub(*at) >= LOOK_AT_LEAST_US);
        let why = &mut self.why;
        if busy.is_none()
            && let Some(cells) = cells
        {
            why.most_cells = why.most_cells.max(cells);
            why.cells += cells;
            why.compared += 1;
        }
        let counted = match busy {
            Some(Busy::Asked) => &mut why.asked,
            Some(Busy::Remembering) => &mut why.remembering,
            Some(Busy::Face) => &mut why.face,
            None if cells.is_none() => &mut why.first,
            None if changed => &mut why.changed,
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
        self.why.skipped
    }

    pub fn take_why(&mut self) -> Why {
        std::mem::take(&mut self.why)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A room at 320 by 240: a grey wall with a darker desk, the sensor's
    /// noise of up to 4 greys on every pixel, and a box drawn in.
    fn room(seed: u32, drift: f32, face: Option<(usize, usize, usize)>) -> Picture {
        let mut noise = seed;
        let mut rgb = Vec::with_capacity(320 * 240 * 3);
        for y in 0..240usize {
            for x in 0..320usize {
                noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let wobble = ((noise >> 24) % 9) as f32 - 4.0;
                let mut grey = if y > 170 { 70.0 } else { 150.0 } + drift + wobble;
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
    /// coming into a corner is, and so is one at 0.14 on a plain wall; so
    /// is a slow change, added up over glances against the last frame the
    /// detector ran on.
    #[test]
    fn noise_is_no_change_and_a_face_or_a_slow_drift_is() {
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
        // The light rising five greys a glance: never a change from the
        // glance before, a change from where the detector last ran.
        let mut change = Change::default();
        assert!(change.detect(empty.clone(), None, 0));
        let mut detected = Vec::new();
        for glance in 1..=5u64 {
            let grid = Grid::of(&room(glance as u32 + 5, 5.0 * glance as f32, None));
            let before = Grid::of(&room(glance as u32 + 4, 5.0 * (glance - 1) as f32, None));
            assert!(!grid.changed_from(&before), "{glance}");
            if change.detect(grid, None, glance * GLANCE_US / 10) {
                detected.push(glance);
            }
        }
        // Caught at the third glance, then detected while the picture
        // has not been still for 3 s.
        assert_eq!(detected, [3, 4, 5]);
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
        // every glance until the picture has been still for 3 s.
        assert_eq!(detected, [0, 5, 10, 12, 13, 14, 15, 16, 17]);
        assert_eq!(change.take_why().skipped, 11);
        assert_eq!(change.take_why().skipped, 0);
        for glance in 20..30u64 {
            let grid = Grid::of(&room(glance as u32, 0.0, None));
            assert!(change.detect(grid, Some(Busy::Face), glance * GLANCE_US));
        }
        assert_eq!(change.take_why().skipped, 0);
    }

    /// (b) Someone walks in over four glances and sits still: every
    /// glance while they move and for 3 s after is detected, so a frame
    /// the detector misses costs a glance, not 3 s.
    #[test]
    fn a_walk_in_and_sit_down_is_detected_at_every_glance_until_still_for_three_seconds() {
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
        // last change, so detected to 10.2 s, then every 3 s.
        assert_eq!(detected, [0, 5, 10, 11, 12, 13, 14, 15, 16, 17, 22, 27]);
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
        // face in and out at 12 and 13; settling to 17; eleven skipped.
        assert_eq!(
            (why.first, why.every, why.changed, why.settling, why.skipped),
            (1, 2, 2, 4, 11)
        );
        assert_eq!((why.face, why.asked, why.remembering), (3, 1, 0));
        assert_eq!(why.compared, 19);
        assert!(why.most_cells >= CELLS && why.cells >= 2 * CELLS, "{why:?}");
        assert!(why.line().contains("2 changed, 4 settling, 2 every 3 s"));
        assert_eq!(change.take_why(), Why::default());
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
