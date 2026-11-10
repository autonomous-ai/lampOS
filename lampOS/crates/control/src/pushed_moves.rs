//! A computer's recorded moves, from the packages it pushed (docs/control.md,
//! "Play a recorded move"): when `skill.push` is installed, the package's
//! `moves/NAME.json` files are checked as a `move` is and kept for this
//! computer, so `play` with a name can send one as it sends a `move`.
//!
//! The package is the computer's own and the move files are what it could
//! send as `move` frames anyway: nothing here is a new power, only a name
//! for frames the lamp already holds to the body's checks. A move that
//! fails the check is left out and told, the skill still installs. The
//! files are kept one file for each of the computer's skills, written aside
//! and renamed into place, so a push again of the same skill replaces
//! them whole or not at all, and are deleted with the skill or the pairing.

use std::io::{BufRead, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

use lamp_body::moves::Move;
use serde::Deserialize;

/// The folder in the state folder the computers' moves are kept in.
pub const MOVES_FOLDER: &str = "moves";
/// The most moves one skill keeps, as the body keeps (`--moves`).
pub const MOST_MOVES: usize = 64;
/// The most a move file may hold, as the body reads one (`--moves`).
pub const MOST_FILE_BYTES: u64 = 256 * 1024;

/// What a package's `moves/` came to: the skill it is, the moves kept and
/// the ones left out with why.
#[derive(Debug, Default, PartialEq)]
pub struct Found {
    pub skill: String,
    pub moves: Vec<Move>,
    pub left_out: Vec<(String, String)>,
}

impl Found {
    /// What the install's answer says of them.
    pub fn told(&self) -> serde_json::Value {
        serde_json::json!({
            "kept": self.moves.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
            "left_out": self
                .left_out
                .iter()
                .map(|(name, why)| serde_json::json!({"name": name, "why": why}))
                .collect::<Vec<_>>(),
        })
    }
}

/// A move's file name, as a move is asked by name (`play`'s schema): a
/// letter, then letters, digits, `_` and `-`, at most 32 as the body keeps.
fn move_name_ok(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= 32
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// A computer's id or a skill's name as a path part: never a path.
fn part_ok(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= 64
        && part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// The moves in a package's bytes (`SKILL/moves/NAME.json`), each held to
/// `check` as a computer's `move` is (the door's pre-check). Every file's bytes
/// are held before the count is made; the push's own size cap (1 MiB) bounds it.
pub fn found_in(
    package: &[u8],
    check: &dyn Fn(&Move) -> Result<(), String>,
) -> Result<Found, String> {
    let mut archive = tar::Archive::new(package);
    let entries = archive
        .entries()
        .map_err(|error| format!("not a tar: {error}"))?;
    let mut skill: Option<String> = None;
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut oversized: Vec<String> = Vec::new();
    for entry in entries {
        let mut entry = entry.map_err(|error| format!("not a tar: {error}"))?;
        let path = String::from_utf8(entry.path_bytes().into_owned())
            .map_err(|_| "a path that is not UTF-8".to_owned())?;
        let path = path.trim_end_matches('/').to_owned();
        let Some((top, rest)) = path.split_once('/') else {
            continue;
        };
        match &skill {
            Some(seen) if seen != top => {
                return Err(format!(
                    "a second folder beside {seen}: a package holds one skill"
                ));
            }
            Some(_) => {}
            None => skill = Some(top.to_owned()),
        }
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let Some(stem) = rest
            .strip_prefix("moves/")
            .and_then(|file| file.strip_suffix(".json"))
            .filter(|stem| !stem.contains('/'))
        else {
            continue;
        };
        if entry.size() > MOST_FILE_BYTES {
            oversized.push(stem.to_owned());
            continue;
        }
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|error| format!("{path}: {error}"))?;
        files.push((stem.to_owned(), bytes));
    }
    let skill = skill.ok_or_else(|| "no files in the package".to_owned())?;
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut found = Found {
        skill,
        ..Found::default()
    };
    for stem in oversized {
        found
            .left_out
            .push((stem, format!("more than {MOST_FILE_BYTES} bytes")));
    }
    for (stem, bytes) in files {
        let why = if found.moves.len() >= MOST_MOVES {
            Err(format!(
                "more than {MOST_MOVES} moves: the first {MOST_MOVES} by name are kept"
            ))
        } else if !move_name_ok(&stem) {
            Err("a name of a letter then letters, digits, _ and -, at most 32".to_owned())
        } else {
            serde_json::from_slice::<Move>(&bytes)
                .map_err(|error| format!("not a move: {error}"))
                .and_then(|mv| {
                    if mv.name != stem {
                        return Err(format!(
                            "the file is moves/{stem}.json but the move's name is {:?}",
                            mv.name
                        ));
                    }
                    check(&mv).map(|()| mv)
                })
        };
        match why {
            Ok(mv) => found.moves.push(mv),
            Err(why) => found.left_out.push((stem, why)),
        }
    }
    found.left_out.sort();
    Ok(found)
}

/// One skill's file, a line each: the names (`{"skill": …, "names": […]}`),
/// then each move in that order, so that a name looked for among the
/// computer's skills reads the first line only, one move is read without
/// the others, and a replacement is one renamed file.
#[derive(Deserialize)]
struct Head {
    names: Vec<String>,
}

/// The computers' moves kept under one folder, `CLIENT/SKILL.json`.
pub struct Moves {
    folder: PathBuf,
}

impl Moves {
    pub fn new(folder: PathBuf) -> Self {
        Self { folder }
    }

    fn dir(&self, client: &str) -> Option<PathBuf> {
        part_ok(client).then(|| self.folder.join(client))
    }

    /// `found`'s moves of `client`, in place of the skill's kept before:
    /// written aside and renamed, so whole or not at all; none left, the
    /// skill's file is deleted.
    pub fn keep(&self, client: &str, found: &Found) -> std::io::Result<()> {
        let (Some(dir), true) = (self.dir(client), part_ok(&found.skill)) else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "not a computer's or a skill's name",
            ));
        };
        let path = dir.join(format!("{}.json", found.skill));
        if found.moves.is_empty() {
            return match std::fs::remove_file(&path) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
                _ => Ok(()),
            };
        }
        std::fs::create_dir_all(&dir)?;
        let names: Vec<&str> = found.moves.iter().map(|mv| mv.name.as_str()).collect();
        let mut bytes =
            serde_json::to_vec(&serde_json::json!({"skill": found.skill, "names": names}))
                .map_err(std::io::Error::other)?;
        bytes.push(b'\n');
        for mv in &found.moves {
            serde_json::to_writer(&mut bytes, mv).map_err(std::io::Error::other)?;
            bytes.push(b'\n');
        }
        let aside = path.with_extension("json.new");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&aside)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        std::fs::rename(&aside, path)
    }

    /// `skill`'s moves of `client` deleted (a removed skill).
    pub fn remove_skill(&self, client: &str, skill: &str) {
        if let (Some(dir), true) = (self.dir(client), part_ok(skill)) {
            let _ = std::fs::remove_file(dir.join(format!("{skill}.json")));
        }
    }

    /// Every move of `client` deleted (a revoke).
    pub fn forget(&self, client: &str) {
        if let Some(dir) = self.dir(client) {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    fn path(&self, client: &str, skill: &str) -> Option<PathBuf> {
        Some(self.dir(client)?.join(format!("{skill}.json")))
    }

    /// The names of one skill's moves: its file's first line.
    fn names(&self, client: &str, skill: &str) -> Option<Vec<String>> {
        let mut line = String::new();
        std::io::BufReader::new(std::fs::File::open(self.path(client, skill)?).ok()?)
            .read_line(&mut line)
            .ok()?;
        serde_json::from_str::<Head>(&line)
            .ok()
            .map(|head| head.names)
    }

    /// One skill's move `name`: its own line of the file.
    fn read(&self, client: &str, skill: &str, name: &str) -> Option<Move> {
        let mut lines =
            std::io::BufReader::new(std::fs::File::open(self.path(client, skill)?).ok()?)
                .lines()
                .map_while(Result::ok);
        let head: Head = serde_json::from_str(&lines.next()?).ok()?;
        let at = head.names.iter().position(|kept| kept == name)?;
        serde_json::from_str(&lines.nth(at)?).ok()
    }

    /// The move `name` of `client`, from `skill` if told which, else from the
    /// one skill that has it: refused with why when none or when two have.
    pub fn find(&self, client: &str, name: &str, skill: Option<&str>) -> Result<Move, String> {
        if let Some(skill) = skill {
            let names = part_ok(skill)
                .then(|| self.names(client, skill))
                .flatten()
                .ok_or_else(|| format!("no skill {skill} of yours has recorded moves"))?;
            return names
                .iter()
                .any(|kept| kept == name)
                .then(|| self.read(client, skill, name))
                .flatten()
                .ok_or_else(|| format!("no move {name} in your skill {skill}"));
        }
        let mut skills: Vec<String> = std::fs::read_dir(self.dir(client).ok_or("not a computer")?)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let file = entry.file_name().into_string().ok()?;
                file.strip_suffix(".json").map(str::to_owned)
            })
            .collect();
        skills.sort();
        // The names first, one file after another; only the one that has the
        // name is read whole.
        let have: Vec<String> = skills
            .into_iter()
            .filter(|skill| {
                self.names(client, skill)
                    .is_some_and(|names| names.iter().any(|kept| kept == name))
            })
            .collect();
        match have.as_slice() {
            [] => Err(format!(
                "no move {name} in your pushed skills: skill.push one with moves/{name}.json"
            )),
            [skill] => self
                .read(client, skill, name)
                .ok_or_else(|| format!("no move {name} in your pushed skills")),
            _ => Err(format!(
                "the move {name} is in more than one of your skills ({}): say which with skill",
                have.join(", ")
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the door holds a computer's move to today.
    const CHECK: fn(&Move) -> Result<(), String> = lamp_body::moves::check;

    fn tar_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (path, bytes) in files {
            let mut header = tar::Header::new_ustar();
            header.set_entry_type(tar::EntryType::Regular);
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, path, *bytes).unwrap();
        }
        builder.into_inner().unwrap()
    }

    fn wave(name: &str, yaw: i16) -> String {
        // 8 s: a lean of `yaw` counts and back, no step over half of it.
        let frames: Vec<[i16; 5]> = [0, yaw / 2, yaw, yaw / 2, 0, yaw / 2, yaw, yaw / 2, 0]
            .iter()
            .map(|yaw| [0, 0, *yaw, 0, 0])
            .collect();
        serde_json::json!({"name": name, "step_ms": 1000, "frames": frames}).to_string()
    }

    fn folder(tag: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("lamp-control-moves-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        folder
    }

    #[test]
    fn a_packages_moves_are_found_and_the_bad_ones_left_out_with_why() {
        let package = tar_of(&[
            ("desk-wave/skill.toml", b"kind = \"installable\""),
            ("desk-wave/moves/wave.json", wave("wave", 100).as_bytes()),
            // Past any lamp's reach: 600 counts of yaw.
            ("desk-wave/moves/far.json", wave("far", 600).as_bytes()),
            // The name in the file is not the file's.
            (
                "desk-wave/moves/other.json",
                wave("not-other", 100).as_bytes(),
            ),
            // Not a move.
            ("desk-wave/moves/junk.json", b"{\"nope\": 1}"),
            // Not a move's place.
            ("desk-wave/notes/wave2.json", wave("wave2", 100).as_bytes()),
            ("desk-wave/moves/readme.txt", b"hello"),
        ]);
        let found = found_in(&package, &CHECK).unwrap();
        assert_eq!(found.skill, "desk-wave");
        assert_eq!(
            found
                .moves
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>(),
            ["wave"]
        );
        let mut left: Vec<(&str, &str)> = found
            .left_out
            .iter()
            .map(|(n, w)| (n.as_str(), w.as_str()))
            .collect();
        left.sort();
        assert_eq!(
            left.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
            ["far", "junk", "other"]
        );
        assert!(left[0].1.contains("farther than"), "{}", left[0].1);
        assert!(left[2].1.contains("not-other"), "{}", left[2].1);
        assert_eq!(found.told()["kept"], serde_json::json!(["wave"]));
    }

    #[test]
    fn a_package_without_moves_has_none_and_one_with_too_many_keeps_the_first_64() {
        let none = found_in(&tar_of(&[("quiet/skill.toml", b"x")]), &CHECK).unwrap();
        assert_eq!(
            (none.skill.as_str(), none.moves.len(), none.left_out.len()),
            ("quiet", 0, 0)
        );
        let mut files: Vec<(String, String)> = (0..66)
            .map(|n| {
                (
                    format!("many/moves/m{n:02}.json"),
                    wave(&format!("m{n:02}"), 100),
                )
            })
            .collect();
        files.sort();
        let refs: Vec<(&str, &[u8])> = files
            .iter()
            .map(|(p, b)| (p.as_str(), b.as_bytes()))
            .collect();
        let found = found_in(&tar_of(&refs), &CHECK).unwrap();
        assert_eq!(found.moves.len(), MOST_MOVES);
        assert_eq!(found.left_out.len(), 2);
        assert!(
            found.left_out[0].1.contains("64"),
            "{}",
            found.left_out[0].1
        );
        assert!(found_in(b"not a tar at all", &CHECK).is_err());
    }

    #[test]
    fn a_kept_move_is_found_by_name_and_a_push_again_replaces_the_skills_moves_whole() {
        let store = Moves::new(folder("keep"));
        let v1 = found_in(
            &tar_of(&[
                ("a/moves/wave.json", wave("wave", 100).as_bytes()),
                ("a/moves/wiggle.json", wave("wiggle", 100).as_bytes()),
            ]),
            &CHECK,
        )
        .unwrap();
        store.keep("c_1", &v1).unwrap();
        assert_eq!(store.find("c_1", "wave", None).unwrap().frames[2][2], 100);
        assert!(store.find("c_1", "wiggle", Some("a")).is_ok());
        let v2 = found_in(
            &tar_of(&[("a/moves/wave.json", wave("wave", 150).as_bytes())]),
            &CHECK,
        )
        .unwrap();
        store.keep("c_1", &v2).unwrap();
        assert_eq!(store.find("c_1", "wave", None).unwrap().frames[2][2], 150);
        assert!(
            store
                .find("c_1", "wiggle", None)
                .unwrap_err()
                .contains("no move"),
            "gone with the old version"
        );
        // A half-written file beside it is never read.
        std::fs::write(store.folder.join("c_1").join("a.json.new"), b"{broken").unwrap();
        assert_eq!(store.find("c_1", "wave", None).unwrap().frames[2][2], 150);
        // Another computer's moves are never found.
        assert!(
            store
                .find("c_2", "wave", None)
                .unwrap_err()
                .contains("no move")
        );
    }

    #[test]
    fn a_name_in_two_skills_is_refused_naming_them_unless_the_skill_is_told() {
        let store = Moves::new(folder("two"));
        for (skill, yaw) in [("alpha", 100), ("beta", 140)] {
            let path = format!("{skill}/moves/wave.json");
            store
                .keep(
                    "c_1",
                    &found_in(
                        &tar_of(&[(path.as_str(), wave("wave", yaw).as_bytes())]),
                        &CHECK,
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        let why = store.find("c_1", "wave", None).unwrap_err();
        assert!(
            why.contains("alpha") && why.contains("beta") && why.contains("skill"),
            "{why}"
        );
        assert_eq!(
            store.find("c_1", "wave", Some("beta")).unwrap().frames[2][2],
            140
        );
        assert!(
            store
                .find("c_1", "wave", Some("gamma"))
                .unwrap_err()
                .contains("gamma")
        );
    }

    #[test]
    fn a_removed_skill_or_a_revoked_computer_has_no_moves_left() {
        let store = Moves::new(folder("gone"));
        for skill in ["alpha", "beta"] {
            let path = format!("{skill}/moves/{skill}-wave.json");
            store
                .keep(
                    "c_1",
                    &found_in(
                        &tar_of(&[(
                            path.as_str(),
                            wave(&format!("{skill}-wave"), 100).as_bytes(),
                        )]),
                        &CHECK,
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        store.remove_skill("c_1", "alpha");
        assert!(store.find("c_1", "alpha-wave", None).is_err());
        assert!(store.find("c_1", "beta-wave", None).is_ok());
        store.forget("c_1");
        assert!(store.find("c_1", "beta-wave", None).is_err());
        assert!(!store.folder.join("c_1").exists());
        // Names that are not names never leave the folder.
        for bad in ["../x", "a/b", "", ".hidden"] {
            assert!(store.find("c_1", "wave", Some(bad)).is_err(), "{bad:?}");
            store.remove_skill(bad, "x");
            store.forget(bad);
        }
    }

    /// The worst a computer can hold: 16 skills of 64 moves of 10 s at 40 ms
    /// (251 frames), a name looked for among them; the typical case is the
    /// test after this.
    #[test]
    fn finding_a_move_among_the_most_a_computer_can_keep_is_still_quick() {
        let store = Moves::new(folder("worst"));
        let frames: Vec<[i16; 5]> = (0..251)
            .map(|at| {
                // A slow lean, no step over a count: well under the speed caps.
                let up = i16::try_from(at.min(250 - at)).unwrap();
                [0, 0, up.min(100), 0, 0]
            })
            .collect();
        for skill in 0..16 {
            let moves: Vec<Move> = (0..64)
                .map(|n| Move {
                    name: format!("m{skill:02}-{n:02}"),
                    step_ms: 40,
                    frames: frames.clone(),
                })
                .collect();
            let found = Found {
                skill: format!("s{skill:02}"),
                moves,
                left_out: Vec::new(),
            };
            store.keep("c_1", &found).unwrap();
        }
        let started = std::time::Instant::now();
        for _ in 0..10 {
            store.find("c_1", "m15-63", None).unwrap();
        }
        let each = started.elapsed() / 10;
        eprintln!("find among 16 skills of 64 long moves: {each:?} each");
        // Measured 3-5 ms in a debug build (less on the lamp); a loose bound so
        // a busy machine does not fail it.
        assert!(each < std::time::Duration::from_millis(20), "{each:?} each");
    }

    #[test]
    fn finding_a_move_among_the_16_skills_a_lamp_keeps_takes_a_few_milliseconds() {
        let store = Moves::new(folder("fast"));
        for skill in 0..16 {
            let files: Vec<(String, String)> = (0..4)
                .map(|n| {
                    (
                        format!("s{skill:02}/moves/m{skill:02}-{n}.json"),
                        wave(&format!("m{skill:02}-{n}"), 100),
                    )
                })
                .collect();
            let refs: Vec<(&str, &[u8])> = files
                .iter()
                .map(|(p, b)| (p.as_str(), b.as_bytes()))
                .collect();
            store
                .keep("c_1", &found_in(&tar_of(&refs), &CHECK).unwrap())
                .unwrap();
        }
        let started = std::time::Instant::now();
        for _ in 0..20 {
            store.find("c_1", "m15-3", None).unwrap();
        }
        let each = started.elapsed() / 20;
        eprintln!("find among 16 skills of 4 moves: {each:?} each");
        assert!(each < std::time::Duration::from_millis(5), "{each:?} each");
    }
}
