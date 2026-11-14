//! A sitting kept across a short restart of lamp-eyes (`docs/eyes.md`,
//! "A sitting kept across a restart").
//!
//! `room/activity`'s `sitting_s` is counted in memory from the sitting's
//! first face ([`crate::activity::Judge`]), on CLOCK_MONOTONIC, which runs on
//! through a restart of the program and begins again only at a boot. Each
//! update restarts lamp-eyes, so without this every nudge that waits on
//! `sitting_s` (a sip of water after 50 minutes, rest your eyes after 45, a
//! comfort break after 90) began its wait again at each update.
//!
//! The sitting is written to one small file, [`PATH`]: on tmpfs, made at
//! boot by the product's tmpfiles (`deploy/product/lampos.conf`), so neither a
//! stop nor a restart of lamp-eyes removes it and a reboot does. It holds the
//! boot, when it was written, when the sitting began, when and where its
//! sitter's face was last seen, its baseline, and what was said: a box, a
//! pitch and times, never a print, a yawn or a picture. It is taken back at
//! this run's first look only on the same boot, written within
//! [`RoomActivity::SITTING_GAP_S`] of now, its sitter last seen within that
//! gap of the look, and only if a near face of that look is at the sitter's
//! place ([`crate::activity::Judge::take_back`]). The restart's seconds then
//! count as a gap in the sitting, as a look away of as long does, and never
//! more than that gap.
use crate::activity::Look;
use lamp_msgs::{Activity, RoomActivity};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

/// Where the product keeps the sitting: `lampos`'s folder, 0700, made at
/// boot by the product's tmpfiles, apart from the eyes' runtime folder, which
/// a restart empties (and whose frames must not outlive it).
pub const PATH: &str = "/run/lampos-eyes-kept/sitting.json";
/// While a sitting goes on, it is written again at most this often, so the
/// file says the sitter was seen within this long; at once when it begins,
/// ends or changes otherwise (its baseline, or what is said).
pub const WRITE_EVERY_US: u64 = 30_000_000;
const VERSION: u32 = 1;
/// The largest file read.
const MOST_BYTES: u64 = 4 * 1024;
/// A sitting's gap: a kept one last seen longer ago is never taken back.
pub const GAP_US: u64 = RoomActivity::SITTING_GAP_S as u64 * 1_000_000;

/// What a restart keeps of a sitting, on the bus's monotonic clock.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sitting {
    /// When the sitting began: `sitting_s` counts from it.
    pub from_us: u64,
    /// When the sitter's face was last seen.
    pub last_face_us: u64,
    /// Their last look, where they are followed from.
    pub sitter: Look,
    /// The sitting's baseline, once in: leaning in and head down need it.
    pub baseline: Option<Look>,
    /// What is said, since when.
    pub state: Option<(Activity, u64)>,
}

impl Sitting {
    /// The same but for the sitter's last look and when it was seen, which
    /// move at each look and are written only every [`WRITE_EVERY_US`].
    fn same_but_seen(&self, other: &Self) -> bool {
        (self.from_us, self.baseline, self.state) == (other.from_us, other.baseline, other.state)
    }
}

#[derive(Serialize, Deserialize)]
struct Kept {
    version: u32,
    boot: String,
    written_us: u64,
    sitting: Sitting,
}

/// Where the sitting is kept, on which boot, and what was last written.
pub struct Keeper {
    file: PathBuf,
    boot: String,
    written: Option<(u64, Option<Sitting>)>,
    faulted: bool,
}

impl Keeper {
    pub fn new(file: PathBuf, boot: String) -> Self {
        Self {
            file,
            boot,
            written: None,
            faulted: false,
        }
    }

    pub fn file(&self) -> &Path {
        &self.file
    }

    /// The sitting kept on this boot, if the file is there, is this boot's,
    /// and was written at most [`RoomActivity::SITTING_GAP_S`] before
    /// `now_us` and not after it. A file of another boot, an older one, or
    /// one that can't be read, is none.
    pub fn read(&self, now_us: u64) -> io::Result<Option<Sitting>> {
        let mut text = String::new();
        match fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&self.file)
        {
            Ok(file) => file.take(MOST_BYTES).read_to_string(&mut text)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let kept: Kept = serde_json::from_str(&text)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if kept.version != VERSION
            || kept.boot != self.boot
            || kept.written_us > now_us
            || now_us - kept.written_us > GAP_US
        {
            return Ok(None);
        }
        Ok(Some(kept.sitting))
    }

    /// The sitting at `now_us` ([`crate::activity::Judge::sitting`]) written
    /// when it differs from the last written but for where and when its
    /// sitter was last seen, or, while one goes on, [`WRITE_EVERY_US`] after
    /// the last write; none removes the file. The first failure is said.
    pub fn keep(&mut self, sitting: Option<Sitting>, now_us: u64) {
        let due = match &self.written {
            None => true,
            Some((at, before)) => match (before, &sitting) {
                (None, None) => false,
                (Some(before), Some(now)) => {
                    !before.same_but_seen(now) || now_us.saturating_sub(*at) >= WRITE_EVERY_US
                }
                _ => true,
            },
        };
        if !due {
            return;
        }
        let done = match sitting {
            Some(sitting) => write(
                &self.file,
                &Kept {
                    version: VERSION,
                    boot: self.boot.clone(),
                    written_us: now_us,
                    sitting,
                },
            ),
            None => match fs::remove_file(&self.file) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
                _ => Ok(()),
            },
        };
        match done {
            Ok(()) => self.faulted = false,
            Err(error) if !self.faulted => {
                self.faulted = true;
                eprintln!(
                    "lamp-eyes: the sitting not kept in {}: {error}",
                    self.file.display()
                );
            }
            Err(_) => {}
        }
        // Tried again at the next change or the next cadence, not at once.
        self.written = Some((now_us, sitting));
    }
}

/// The product's keeper at the start, if its folder is there and the boot
/// can be told, and the sitting it kept, each said once in the journal.
pub fn at_start(now_us: u64) -> (Option<Keeper>, Option<Sitting>) {
    let file = PathBuf::from(PATH);
    let folder_there = file.parent().is_some_and(Path::is_dir);
    let Some(boot) = lamp_bus::kernel_boot_id().filter(|_| folder_there) else {
        eprintln!("lamp-eyes: a sitting is not kept across a restart: no {PATH} folder or boot id");
        return (None, None);
    };
    let keeper = Keeper::new(file, boot.to_owned());
    let kept = match keeper.read(now_us) {
        Ok(kept) => kept,
        Err(error) => {
            eprintln!("lamp-eyes: the kept sitting not read: {error}");
            None
        }
    };
    if let Some(sitting) = &kept {
        eprintln!(
            "lamp-eyes: a sitting of {} s kept from before the restart, its sitter seen {} s ago: taken back if the first look finds them",
            now_us.saturating_sub(sitting.from_us) / 1_000_000,
            now_us.saturating_sub(sitting.last_face_us) / 1_000_000
        );
    }
    (Some(keeper), kept)
}

/// The file written whole: beside it, made new (never through a link, a
/// file a cut-off run left there removed first), then moved over it. Not
/// synced: it is on tmpfs, which a power cut empties anyway.
fn write(file: &Path, kept: &Kept) -> io::Result<()> {
    let text = serde_json::to_vec(kept).map_err(io::Error::other)?;
    let new = file.with_extension("new");
    match fs::symlink_metadata(&new) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(io::Error::other(format!(
                "{} is a link: refused",
                new.display()
            )));
        }
        Ok(_) => fs::remove_file(&new)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut out = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW)
        .mode(0o600)
        .open(&new)?;
    out.write_all(&text)?;
    drop(out);
    fs::rename(&new, file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::Judge;
    use std::os::unix::fs::PermissionsExt;

    const S: u64 = 1_000_000;
    const SEATED: Look = Look {
        height: 0.22,
        centre_x: 0.6,
        centre_y: 0.45,
        across: 0.45,
        pitch: 0.5,
    };
    /// Someone else, across the picture from where the sitter sat.
    const ELSEWHERE: Look = Look {
        centre_x: 1.1,
        across: 0.85,
        ..SEATED
    };

    fn folder(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("lamp-eyes-sitting-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    /// Looks every 500 ms from `from` to `to` s, each `face`, the run's
    /// keeper writing at each, as the eyes' tick does.
    fn looks(judge: &mut Judge, keeper: &mut Keeper, from: u64, to: u64, face: Option<Look>) {
        let mut at = from * S;
        while at < to * S {
            judge.looked(at, face.as_slice());
            keeper.keep(judge.sitting(at), at);
            at += S / 2;
        }
    }

    /// A sitting of 30 minutes, its last look at 1800 s, kept as a run
    /// writes it on `boot-a`.
    fn sat_half_an_hour(file: &Path) -> Judge {
        let mut judge = Judge::default();
        let mut keeper = Keeper::new(file.to_path_buf(), "boot-a".into());
        looks(&mut judge, &mut keeper, 0, 1800, Some(SEATED));
        assert_eq!(judge.now(1800 * S).unwrap().sitting_s, 1800);
        judge
    }

    /// The next run's first look at `at_s`, as `bus_out` gives it: what was
    /// taken back, and what is said then.
    fn restarted(file: &Path, boot: &str, at_s: u64, face: Option<Look>) -> (bool, RoomActivity) {
        let keeper = Keeper::new(file.to_path_buf(), boot.into());
        let mut judge = Judge::default();
        let faces: Vec<Look> = face.into_iter().collect();
        let taken = keeper
            .read(at_s * S)
            .unwrap()
            .is_some_and(|kept| judge.take_back(at_s * S, kept, &faces));
        judge.looked(at_s * S, &faces);
        (taken, judge.now(at_s * S).unwrap())
    }

    /// A 10 s restart on the same boot, the sitter still there: the sitting
    /// goes on from its start, its baseline in, the restart's seconds a gap
    /// within it and never more.
    #[test]
    fn a_sitting_is_kept_across_a_ten_second_restart_on_the_same_boot() {
        let dir = folder("kept");
        let file = dir.join("sitting.json");
        sat_half_an_hour(&file);
        let mode = fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let (taken, now) = restarted(&file, "boot-a", 1810, Some(SEATED));
        assert!(taken);
        assert_eq!((now.state, now.sitting_s), (Activity::AtDesk, 1810));
        assert!(now.baseline, "the baseline is kept");
        assert!(!dir.join("sitting.new").exists());
    }

    /// Past the sitting's gap, on another boot, or with the first look not
    /// finding the sitter at their place: a new sitting from that look.
    #[test]
    fn a_sitting_is_not_kept_past_its_gap_across_a_boot_or_without_the_sitter() {
        let dir = folder("not");
        let file = dir.join("sitting.json");
        sat_half_an_hour(&file);
        // The file was last written within 30 s of the last look.
        let gap = u64::from(RoomActivity::SITTING_GAP_S);
        for (boot, at_s, face, why) in [
            ("boot-a", 1800 + gap + 1, Some(SEATED), "past the gap"),
            ("boot-b", 1810, Some(SEATED), "another boot"),
            ("boot-a", 1810, None, "nobody at the first look"),
            ("boot-a", 1810, Some(ELSEWHERE), "someone elsewhere"),
            ("boot-a", 1700, Some(SEATED), "written after now"),
        ] {
            let (taken, now) = restarted(&file, boot, at_s, face);
            assert!(!taken, "{why}");
            assert_eq!(now.sitting_s, 0, "{why}");
            assert!(!now.baseline, "{why}");
        }
        // Written within the gap, but its sitter last seen past it (a file
        // kept by a run that never looked again): not taken back either.
        let mut judge = Judge::default();
        let kept = Sitting {
            from_us: 0,
            last_face_us: 10 * S,
            sitter: SEATED,
            baseline: None,
            state: None,
        };
        assert!(!judge.take_back((10 + gap + 1) * S, kept, &[SEATED]));
    }

    /// Told by face (`Judge::with_prints`), a kept sitting is never taken
    /// back, since no print of its sitter was kept: someone else sitting
    /// down in their place within the gap begins a new sitting at that look,
    /// and their print is the new sitting's first.
    #[test]
    fn told_by_face_someone_else_at_the_place_begins_a_new_sitting() {
        let dir = folder("face");
        let file = dir.join("sitting.json");
        sat_half_an_hour(&file);
        let kept = Keeper::new(file.clone(), "boot-a".into())
            .read(1810 * S)
            .unwrap()
            .unwrap();
        let mut judge = Judge::with_prints();
        assert!(!judge.take_back(1810 * S, kept, &[SEATED]));
        judge.looked(1810 * S, &[SEATED]);
        assert_eq!(judge.wants_print(1810 * S), Some(0), "the sitter's own");
        let mut someone_else = vec![0.0f32; 128];
        someone_else[0] = 1.0;
        assert_eq!(judge.printed(1810 * S, Some(&someone_else)), None);
        let now = judge.now(1810 * S).unwrap();
        assert_eq!((now.sitting_s, now.baseline), (0, false));
        // By place, the same look takes the sitting back.
        assert!(Judge::default().take_back(1810 * S, kept, &[SEATED]));
    }

    /// Written at once at a change and only every 30 s while the sitting
    /// only goes on; removed when it ends; read back on its boot only.
    #[test]
    fn written_at_a_change_and_on_a_cadence_and_removed_when_the_sitting_ends() {
        let dir = folder("cadence");
        let file = dir.join("sitting.json");
        let mut keeper = Keeper::new(file.clone(), "boot-a".into());
        let sitting = |last_face_us| Sitting {
            from_us: S,
            last_face_us,
            sitter: SEATED,
            baseline: None,
            state: Some((Activity::AtDesk, S)),
        };
        keeper.keep(Some(sitting(S)), S);
        let first = fs::read(&file).unwrap();
        keeper.keep(Some(sitting(20 * S)), 20 * S);
        assert_eq!(fs::read(&file).unwrap(), first, "only seen again: not due");
        keeper.keep(Some(sitting(31 * S)), 31 * S);
        assert_eq!(keeper.read(32 * S).unwrap(), Some(sitting(31 * S)));
        let leaning = Sitting {
            state: Some((Activity::LeaningIn, 32 * S)),
            ..sitting(32 * S)
        };
        keeper.keep(Some(leaning), 32 * S);
        assert_eq!(keeper.read(33 * S).unwrap(), Some(leaning), "a change");
        keeper.keep(None, 40 * S);
        assert!(!file.exists(), "ended: removed");
        fs::write(&file, "not json").unwrap();
        assert!(keeper.read(41 * S).is_err());
    }
}
