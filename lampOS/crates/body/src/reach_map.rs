//! The reach map (#7513): which shoulder, elbow and pitch poses are safe, as a per-lamp file the body only READS.
//!
//! `tools/reach` makes the map (the CAD's parts not touching each other or the table, and V1's anti-tip rule; masses are
//! V1's estimates until a lamp's own are measured). A per-lamp file is the same map with the lamp's `unit` and `frame`:
//! the lamp's counts minus `frame.offset` are the map's counts. This module is the pure part: [`ReachMap::parse`],
//! [`ReachMap::is_safe`], [`ReachMap::limits`] and [`ReachMap::restrict`] answer exactly what `tools/reach/pose-table.json`
//! says (the page guard, the Python port and this module must all give those answers); [`read_trusted`] is the one place
//! that touches the file (read only, `O_NOFOLLOW`, a regular file owned by the trusted uid and writable by no one else).
//! REFUSE-ONLY: the map can hold a goal at the edge of the safe run or not send it; it never widens what the body allows.
//! Nothing here is wired into the body yet (the switch is `LAMP_BODY_REACH_MAP`, off).

use std::path::Path;

/// The joints the map holds, in the order of its axes.
pub const MAPPED: [&str; 3] = ["shoulder", "elbow", "pitch"];

/// A pose in the map's counts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub shoulder: f64,
    pub elbow: f64,
    pub pitch: f64,
}

/// What `restrict` says of one goal (degrees, the page's unit: shoulder and elbow from folded, pitch up positive).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Restricted {
    /// Sent as asked.
    Sent(f64),
    /// Past the safe run: held at its inside edge (rounded inward to 0.1 deg).
    HeldAtEdge(f64),
    /// From a pose the map calls unsafe, a goal no nearer to safe: not sent, the joint stays where it is.
    RefusedFurther(f64),
}

/// The map's lamp: lamp-4ace's folded arm and head home (counts), from which a pose's degrees are laid out (the page guard's).
const FOLDED_SHOULDER: f64 = 2048.0;
const FOLDED_ELBOW: f64 = 2047.0;
const HEAD_HOME: f64 = 1772.0;
const DEG_A_COUNT: f64 = 360.0 / 4096.0;
/// A map file is a few hundred kilobytes at most; one over this is not one.
const MAX_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug)]
pub struct ReachMap {
    shoulder: Vec<f64>,
    elbow: Vec<f64>,
    pitch: Vec<f64>,
    /// `runs[i][j]`: the safe head pitch runs (counts) at shoulder `i`, elbow `j`.
    runs: Vec<Vec<Vec<(f64, f64)>>>,
    unit: Option<String>,
    offset: Pose,
}

fn axis_of(value: &serde_json::Value, name: &str) -> Result<Vec<f64>, String> {
    let axis: Vec<f64> = value["axes"][name]
        .as_array()
        .ok_or_else(|| format!("not a reach map: no {name} axis"))?
        .iter()
        .map(|v| {
            v.as_f64()
                .ok_or_else(|| format!("not a reach map: a {name} axis value is not a number"))
        })
        .collect::<Result<_, _>>()?;
    if axis.len() < 2 {
        return Err(format!("not a reach map: the {name} axis is too short"));
    }
    Ok(axis)
}

/// The index of the axis value nearest `value` (the first of equals, as the page guard).
fn nearest(axis: &[f64], value: f64) -> usize {
    let mut at = 0;
    for (i, v) in axis.iter().enumerate().skip(1) {
        if (v - value).abs() < (axis[at] - value).abs() {
            at = i;
        }
    }
    at
}

fn get(pose: Pose, joint: &str) -> f64 {
    match joint {
        "shoulder" => pose.shoulder,
        "elbow" => pose.elbow,
        _ => pose.pitch,
    }
}

fn with(pose: Pose, joint: &str, value: f64) -> Pose {
    match joint {
        "shoulder" => Pose {
            shoulder: value,
            ..pose
        },
        "elbow" => Pose {
            elbow: value,
            ..pose
        },
        _ => Pose {
            pitch: value,
            ..pose
        },
    }
}

/// The map's counts for the joints' degrees (shoulder and elbow from folded, pitch up positive).
fn counts_of(deg: [f64; 3]) -> Pose {
    Pose {
        shoulder: FOLDED_SHOULDER + deg[0] / DEG_A_COUNT,
        elbow: FOLDED_ELBOW + deg[1] / DEG_A_COUNT,
        pitch: HEAD_HOME - deg[2] / DEG_A_COUNT,
    }
}

fn deg_of(joint: &str, counts: f64) -> f64 {
    match joint {
        "pitch" => (HEAD_HOME - counts) * DEG_A_COUNT,
        "shoulder" => (counts - FOLDED_SHOULDER) * DEG_A_COUNT,
        _ => (counts - FOLDED_ELBOW) * DEG_A_COUNT,
    }
}

fn index_of(joint: &str) -> usize {
    MAPPED.iter().position(|name| *name == joint).unwrap_or(2)
}

/// JavaScript's `Math.round` (halves go up), which the page guard's tenths use.
fn round_tenths(value: f64) -> f64 {
    (value * 10.0 + 0.5).floor() / 10.0
}

impl ReachMap {
    /// Parse a map file's text (the page's JSON). The unit is `None` for the page's own lamp-4ace map.
    pub fn parse(text: &str) -> Result<Self, String> {
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|error| format!("not a reach map: {error}"))?;
        let (shoulder, elbow, pitch) = (
            axis_of(&value, "shoulder")?,
            axis_of(&value, "elbow")?,
            axis_of(&value, "pitch")?,
        );
        let rows = value["safe_pitch_runs"]
            .as_array()
            .ok_or("not a reach map: no safe_pitch_runs")?;
        if rows.len() != shoulder.len() {
            return Err("the runs do not fit the axes".to_owned());
        }
        let mut runs = Vec::with_capacity(rows.len());
        for row in rows {
            let row = row
                .as_array()
                .filter(|row| row.len() == elbow.len())
                .ok_or("the runs do not fit the axes")?;
            let mut out = Vec::with_capacity(row.len());
            for cell in row {
                let mut cell_runs = Vec::new();
                for run in cell.as_array().ok_or("the runs do not fit the axes")? {
                    let pair = run
                        .as_array()
                        .filter(|pair| pair.len() == 2)
                        .ok_or("a run is not [low, high]")?;
                    match (pair[0].as_f64(), pair[1].as_f64()) {
                        (Some(low), Some(high)) => cell_runs.push((low, high)),
                        _ => return Err("a run is not [low, high]".to_owned()),
                    }
                }
                out.push(cell_runs);
            }
            runs.push(out);
        }
        let unit = value["unit"].as_str().map(str::to_owned);
        let off = &value["frame"]["offset"];
        let offset = Pose {
            shoulder: off["shoulder"].as_f64().unwrap_or(0.0),
            elbow: off["elbow"].as_f64().unwrap_or(0.0),
            pitch: off["pitch"].as_f64().unwrap_or(0.0),
        };
        Ok(Self {
            shoulder,
            elbow,
            pitch,
            runs,
            unit,
            offset,
        })
    }

    /// The lamp this file is for, if it names one.
    pub fn unit(&self) -> Option<&str> {
        self.unit.as_deref()
    }

    /// Whether this file may be used on `unit`: only the lamp it names (a file with no unit is the page's lamp-4ace map).
    pub fn for_unit(&self, unit: &str) -> bool {
        self.unit.as_deref() == Some(unit)
    }

    /// A lamp's pose (its own counts) in the map's counts: the frame's offset taken off.
    pub fn to_map_counts(&self, lamp: Pose) -> Pose {
        Pose {
            shoulder: lamp.shoulder - self.offset.shoulder,
            elbow: lamp.elbow - self.offset.elbow,
            pitch: lamp.pitch - self.offset.pitch,
        }
    }

    fn cell(&self, pose: Pose) -> &[(f64, f64)] {
        &self.runs[nearest(&self.shoulder, pose.shoulder)][nearest(&self.elbow, pose.elbow)]
    }

    fn safe_at(&self, pose: Pose) -> bool {
        pose.pitch >= self.pitch[0]
            && pose.pitch <= self.pitch[self.pitch.len() - 1]
            && self
                .cell(pose)
                .iter()
                .any(|(low, high)| pose.pitch >= *low && pose.pitch <= *high)
    }

    /// Whether the pose (map counts) is a safe grid pose, and still is `shrink` counts away on every mapped axis.
    pub fn is_safe(&self, pose: Pose, shrink: f64) -> bool {
        if !self.safe_at(pose) {
            return false;
        }
        if shrink > 0.0 {
            for joint in MAPPED {
                for sign in [-1.0, 1.0] {
                    if !self.safe_at(with(pose, joint, get(pose, joint) + sign * shrink)) {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// The counts `joint` may go to with the others held at `pose`, `shrink` counts in from each end of its run and never past the pose;
    /// `None` when the pose is not safe.
    pub fn limits(&self, pose: Pose, joint: &str, shrink: f64) -> Option<(f64, f64)> {
        if !self.is_safe(pose, 0.0) {
            return None;
        }
        let at = get(pose, joint);
        let (low, high) = if joint == "pitch" {
            // The run of safe pitches at this shoulder and elbow that holds the pitch: the same ends wherever in it the head is.
            *self.cell(pose).iter().find(|(a, b)| at >= *a && at <= *b)?
        } else {
            let axis = if joint == "shoulder" {
                &self.shoulder
            } else {
                &self.elbow
            };
            let safe = |value: f64| self.safe_at(with(pose, joint, value));
            let i = nearest(axis, at);
            if !safe(axis[i]) {
                return Some((at, at));
            }
            let (mut lo, mut hi) = (i, i);
            while lo > 0 && safe(axis[lo - 1]) {
                lo -= 1;
            }
            while hi < axis.len() - 1 && safe(axis[hi + 1]) {
                hi += 1;
            }
            (axis[lo].min(at), axis[hi].max(at))
        };
        Some((at.min(low + shrink), at.max(high - shrink)))
    }

    /// How many counts (the most on any axis) the pose is from a safe grid pose: 0 on a safe one.
    fn distance(&self, pose: Pose) -> f64 {
        if self.is_safe(pose, 0.0) {
            return 0.0;
        }
        let mut best = f64::INFINITY;
        for (i, sh) in self.shoulder.iter().enumerate() {
            for (j, el) in self.elbow.iter().enumerate() {
                let base = (sh - pose.shoulder).abs().max((el - pose.elbow).abs());
                if base >= best {
                    continue;
                }
                for (low, high) in &self.runs[i][j] {
                    let off = if pose.pitch < *low {
                        low - pose.pitch
                    } else if pose.pitch > *high {
                        pose.pitch - high
                    } else {
                        0.0
                    };
                    best = best.min(base.max(off));
                }
            }
        }
        best
    }

    /// How many counts `joint` alone must move, the others held, to reach a safe pose: 0 on a safe one, infinity when no value of it is safe here.
    fn line_distance(&self, pose: Pose, joint: &str) -> f64 {
        if self.is_safe(pose, 0.0) {
            return 0.0;
        }
        if joint == "pitch" {
            return self
                .cell(pose)
                .iter()
                .map(|(low, high)| {
                    if pose.pitch < *low {
                        low - pose.pitch
                    } else {
                        pose.pitch - high
                    }
                })
                .fold(f64::INFINITY, f64::min);
        }
        let axis = if joint == "shoulder" {
            &self.shoulder
        } else {
            &self.elbow
        };
        axis.iter()
            .filter(|v| self.is_safe(with(pose, joint, **v), 0.0))
            .map(|v| (v - get(pose, joint)).abs())
            .fold(f64::INFINITY, f64::min)
    }

    /// The page guard's `restrict`: a goal in degrees for `joint`, the others at their `goals`, the lamp now at `here` (degrees,
    /// `[shoulder, elbow, pitch]`).
    pub fn restrict(
        &self,
        here: [f64; 3],
        goals: [f64; 3],
        joint: &str,
        deg: f64,
        shrink: f64,
    ) -> Restricted {
        if !MAPPED.contains(&joint) || !deg.is_finite() {
            return Restricted::Sent(deg);
        }
        let k = index_of(joint);
        let now = counts_of(here);
        let mut held_deg = goals;
        held_deg[k] = here[k];
        let held = counts_of(held_deg);
        let mut goal_deg = goals;
        goal_deg[k] = deg;
        let goal = counts_of(goal_deg);
        if self.is_safe(held, 0.0) {
            let Some((low, high)) = self.limits(held, joint, shrink) else {
                return Restricted::Sent(deg);
            };
            let (a, b) = {
                let (x, y) = (deg_of(joint, low), deg_of(joint, high));
                (x.min(y), x.max(y))
            };
            if deg >= a && deg <= b {
                return Restricted::Sent(deg);
            }
            // Rounded toward the inside, so the held value is itself inside the range.
            let clamped = if deg > b {
                (b * 10.0).floor() / 10.0
            } else {
                (a * 10.0).ceil() / 10.0
            };
            return Restricted::HeldAtEdge(clamped);
        }
        // The pose is not safe now: only a goal no further from safe than the lamp is. One joint's own way to a safe pose is
        // compared when it has one, else all the joints' together.
        let from_goal = self.line_distance(goal, joint);
        let from_here = self.line_distance(with(held, joint, get(now, joint)), joint);
        let closer = if from_goal.is_finite() && from_here.is_finite() {
            from_goal <= from_here
        } else {
            self.distance(goal) <= self.distance(now)
        };
        if closer {
            Restricted::Sent(deg)
        } else {
            Restricted::RefusedFurther(round_tenths(here[k]))
        }
    }
}

/// The text of the file at `path`, opened read only without following a link, and only when it is a regular file owned by
/// `trusted_uid` (root in the product) that no one else can write.
pub fn read_trusted(path: &Path, trusted_uid: u32) -> Result<String, String> {
    use rustix::fs::{FileType, Mode, OFlags};
    use std::io::Read;
    let fd = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| {
        if error == rustix::io::Errno::NOENT {
            format!("no reach map {}", path.display())
        } else if error == rustix::io::Errno::LOOP {
            format!("the reach map {} is a link: not followed", path.display())
        } else {
            format!("the reach map is unreadable ({error})")
        }
    })?;
    let stat =
        rustix::fs::fstat(&fd).map_err(|error| format!("the reach map is unreadable ({error})"))?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile {
        return Err("the reach map is not a regular file".to_owned());
    }
    if stat.st_uid != trusted_uid {
        return Err(format!(
            "the reach map is owned by uid {}, not by uid {trusted_uid}",
            stat.st_uid
        ));
    }
    if stat.st_mode & 0o022 != 0 {
        return Err("the reach map can be written by someone other than its owner".to_owned());
    }
    if u64::try_from(stat.st_size).map_or(true, |size| size > MAX_BYTES) {
        return Err("the reach map is too large to be one".to_owned());
    }
    let mut text = String::new();
    std::fs::File::from(fd)
        .take(MAX_BYTES)
        .read_to_string(&mut text)
        .map_err(|error| format!("the reach map is unreadable ({error})"))?;
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// The map the page uses and the table every reader must answer (`tools/reach`), read from the repository.
    const MAP: &str = include_str!("../../../sdk/js/examples/twin/reach-map-4ace.json");
    const TABLE: &str = include_str!("../../../tools/reach/pose-table.json");

    fn table() -> Value {
        serde_json::from_str(TABLE).expect("the pose table is JSON")
    }
    fn pose(v: &Value) -> Pose {
        Pose {
            shoulder: v["shoulder"].as_f64().unwrap(),
            elbow: v["elbow"].as_f64().unwrap(),
            pitch: v["pitch"].as_f64().unwrap(),
        }
    }
    fn round3(v: f64) -> f64 {
        (v * 1000.0).round() / 1000.0
    }

    #[test]
    fn the_map_the_page_uses_parses_and_names_no_lamp() {
        let map = ReachMap::parse(MAP).expect("the page's map parses");
        assert_eq!(map.unit(), None);
    }

    #[test]
    fn a_text_that_is_not_a_map_is_refused() {
        assert!(ReachMap::parse("{}").is_err());
        assert!(ReachMap::parse("not json").is_err());
        let mut v: Value = serde_json::from_str(MAP).unwrap();
        v["safe_pitch_runs"].as_array_mut().unwrap().pop();
        assert!(
            ReachMap::parse(&v.to_string()).is_err(),
            "the runs must fit the axes"
        );
    }

    #[test]
    fn every_case_of_the_shared_table_is_answered_as_the_page_guard_does() {
        let map = ReachMap::parse(MAP).unwrap();
        let t = table();
        let cases = t["cases"].as_array().unwrap();
        assert!(cases.len() >= 12);
        for c in cases {
            let name = c["name"].as_str().unwrap();
            let p = pose(&c["pose"]);
            assert_eq!(
                map.is_safe(p, 0.0),
                c["safe"].as_bool().unwrap(),
                "{name}: safe"
            );
            assert_eq!(
                map.is_safe(p, 48.0),
                c["safe_with_margin_48"].as_bool().unwrap(),
                "{name}: safe with the margin"
            );
            for joint in MAPPED {
                for (shrink, key) in [(0.0, "ranges_shrink_0"), (48.0, "ranges_shrink_48")] {
                    let want = &c[key][joint];
                    let got = map
                        .limits(p, joint, shrink)
                        .map(|(a, b)| [round3(a), round3(b)]);
                    let want = want
                        .as_array()
                        .map(|a| [a[0].as_f64().unwrap(), a[1].as_f64().unwrap()]);
                    assert_eq!(got, want, "{name}: {joint} range at {shrink}");
                }
            }
        }
    }

    #[test]
    fn every_restrict_case_of_the_table_is_answered_as_the_page_guard_does() {
        let map = ReachMap::parse(MAP).unwrap();
        let t = table();
        let cases = t["restrict_cases"].as_array().unwrap();
        assert!(cases.len() >= 8);
        for c in cases {
            let name = c["name"].as_str().unwrap();
            let d = &c["pose_deg"];
            let here = [
                d["shoulder"].as_f64().unwrap(),
                d["elbow"].as_f64().unwrap(),
                d["pitch"].as_f64().unwrap(),
            ];
            let got = map.restrict(
                here,
                here,
                c["joint"].as_str().unwrap(),
                c["goal_deg"].as_f64().unwrap(),
                48.0,
            );
            let want_deg = c["deg"].as_f64().unwrap();
            let (deg, kind) = match got {
                Restricted::Sent(d) => (d, None),
                Restricted::HeldAtEdge(d) => (d, Some("held_at_edge")),
                Restricted::RefusedFurther(d) => (d, Some("refused_further")),
            };
            assert!(
                (deg - want_deg).abs() < 1e-6,
                "{name}: sent {deg}, table {want_deg}"
            );
            assert_eq!(kind, c["kind"].as_str(), "{name}: kind");
        }
    }

    #[test]
    fn a_per_lamp_map_is_for_its_lamp_only_and_lays_that_lamps_counts_on_the_maps() {
        let mut v: Value = serde_json::from_str(MAP).unwrap();
        v["unit"] = "lamp-75c9".into();
        v["frame"] = serde_json::json!({"unit":"lamp-75c9","map":{"shoulder":2048.0,"elbow":2047.0,"pitch":1772.0},
            "lamp":{"shoulder":2076.0,"elbow":2045.0,"pitch":1772.0},"offset":{"shoulder":28.0,"elbow":-2.0,"pitch":0.0}});
        let map = ReachMap::parse(&v.to_string()).unwrap();
        assert_eq!(map.unit(), Some("lamp-75c9"));
        assert!(map.for_unit("lamp-75c9"));
        assert!(!map.for_unit("lamp-4ace"));
        for f in table()["frame_cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["unit"] == "lamp-75c9")
        {
            assert_eq!(map.to_map_counts(pose(&f["lamp"])), pose(&f["map"]));
        }
    }

    #[test]
    fn a_trusted_file_is_read_and_a_link_or_a_writable_one_is_not() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = std::env::temp_dir().join(format!("reach-map-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("map.json");
        std::fs::write(&file, MAP).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        let me = rustix::process::getuid().as_raw();
        assert_eq!(read_trusted(&file, me).unwrap(), MAP);
        assert!(read_trusted(&file, me + 1).is_err(), "another owner");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o664)).unwrap();
        assert!(read_trusted(&file, me).is_err(), "group-writable");
        let link = dir.join("link.json");
        symlink(&file, &link).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_trusted(&link, me).is_err(), "a link is not followed");
        assert!(read_trusted(&dir.join("none.json"), me).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
