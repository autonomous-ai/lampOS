//! Moves a skill brings, as files (#940, `docs/skills.md`): the same as the
//! body's own gestures ([`crate::clips`]), read at start from a folder
//! (`lamp-body --moves DIR`) instead of built in.
//!
//! One move a file, `NAME.json`:
//!
//! ```json
//! {"name": "wiggle", "step_ms": 750, "frames": [[0, 0, 0, 0, 0], [4, 0, 2, 6, 6], [0, 0, 0, 0, 0]]}
//! ```
//!
//! Each frame is offsets in encoder counts from the pose a standing lamp is
//! in, for head pitch, head roll, base yaw, shoulder and elbow, `step_ms`
//! apart. A move is held to what the built-in clips promise: it begins and
//! ends at nothing, lasts 1.5 to 10 s, takes no joint farther than
//! [`FARTHEST`] and asks none to go faster than [`FASTEST`]. One that does
//! not is refused with the reason, and the others are still taken. It is
//! asked for by name as a gesture is (`{"gesture":"wiggle"}` on the mood
//! socket), and played as one, only while the lamp stands.
//!
//! A skill sends its own move whole on the mood socket instead
//! (`{"move":{"name":...,"step_ms":...,"frames":[...]}}`, [`Sent`]): held
//! here to the same [`check`] (`lamp-skills` checks it the same way when it
//! loads the skill), then played as a gesture is, through the same
//! smoothing, speed limits and anti-tip rule ([`crate::cog`]) as the body's
//! own, and only as a gesture may be: never paused, held or slumped.
use crate::clips::{self, Clip};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// The farthest a move may take each joint from the pose, in counts: head
/// pitch, head roll, base yaw, shoulder, elbow. As the built-in clips.
pub const FARTHEST: [i16; 5] = [200, 200, 150, 350, 350];
/// The fastest a move may ask each joint to go, counts a second, with a
/// count of rounding either way at each end of a step. As the built-in
/// clips.
pub const FASTEST: [f32; 5] = [900.0, 1_300.0, 1_000.0, 800.0, 800.0];
/// At most this many moves are read from a folder; a folder with more is
/// refused whole.
pub const MOST_MOVES: usize = 64;
/// At most this many entries of the folder are looked at, moves or not; a
/// folder with more is refused whole.
pub const MOST_ENTRIES: usize = 1_024;
/// A move's file is at most this long.
const MOST_BYTES: u64 = 256 * 1024;
/// The mood socket reads a datagram of at most this many bytes: more than
/// any move [`check`] takes, as `serde_json` writes it (251 frames of at
/// most 27 bytes).
pub const MOST_DATAGRAM: usize = 16 * 1024;
const SHORTEST_US: u64 = 1_500_000;
const LONGEST_US: u64 = 10_000_000;

/// What a folder of moves gave: the moves taken, in the order of their
/// file names, and each file refused with why.
#[derive(Debug, Default)]
pub struct Loaded {
    pub moves: Vec<&'static Clip>,
    pub refused: Vec<(PathBuf, String)>,
}

impl Loaded {
    /// The move of this name, if one was taken.
    pub fn named(&self, name: &str) -> Option<&'static Clip> {
        self.moves.iter().copied().find(|clip| clip.name == name)
    }
}

/// A move as its file holds it and a skill sends it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Move {
    pub name: String,
    pub step_ms: u16,
    pub frames: Vec<[i16; 5]>,
}

/// Who asks for a gesture on the mood socket (#940, `docs/skills.md`): the
/// one order every part that owns an actuator gives way in, a person over
/// safety, over the conversation, over a skill, over idle life.
pub use lamp_msgs::Priority;

/// Why a gesture asked while one of this priority holds the body is
/// dropped, naming who holds it.
pub fn busy(holder: Priority) -> &'static str {
    match holder {
        Priority::Idle => "dropped: busy, idle",
        Priority::Skill => "dropped: busy, skill",
        Priority::Conversation => "dropped: busy, conversation",
        Priority::Safety => "dropped: busy, safety",
        Priority::Person => "dropped: busy, person",
    }
}

/// Why a `replace` gesture (a paired computer's, through lamp-control) is
/// refused: what plays is the conversation's or the person's, and the
/// person always wins.
pub const PERSON_USING: &str = "dropped: the person is using the lamp";

/// Whether an asker's name is lamp-control's, for a paired computer:
/// `control:<client>` as it sends it, or `lamp-control/control:<client>`
/// as `--caller-caps` names it after the unit that sent it. Only such an
/// ask's `replace` is honoured.
pub fn from_control(source: &str) -> bool {
    source.starts_with("control:") || source.starts_with("lamp-control/control:")
}

/// Why a gesture playing was ended for a `replace` one: it eases out from
/// where it is, and the new one plays after.
pub const REPLACED: &str = "ended: replaced by a newer motion";

/// Why a gesture playing was ended for one asked at this priority.
pub fn ended(by: Priority) -> &'static str {
    match by {
        Priority::Idle => "ended: by idle",
        Priority::Skill => "ended: by skill",
        Priority::Conversation => "ended: by conversation",
        Priority::Safety => "ended: by safety",
        Priority::Person => "ended: by person",
    }
}

/// Why a skill's or idle gesture playing was ended for a hand that came
/// on the lamp as it played: an unprompted gesture gives way to a hand.
pub const ENDED_BY_HAND: &str = "ended: by a hand";

/// An asker's name as its datagram gave it (`skill:dance`), kept in place
/// so the motor loop never allocates for it, to say who lost the body and
/// to whom on `body/gesture` (#940). At most [`Source::MOST`] bytes and
/// printable: a longer or empty one is not kept (the fifth on #940), since
/// it comes off a socket anyone on the lamp may write to.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Source {
    bytes: [u8; Source::MOST],
    len: u8,
}

impl Source {
    pub const MOST: usize = 64;

    pub fn new(text: &str) -> Option<Self> {
        if text.is_empty() || text.len() > Self::MOST || text.chars().any(char::is_control) {
            return None;
        }
        let mut bytes = [0; Self::MOST];
        bytes[..text.len()].copy_from_slice(text.as_bytes());
        Some(Self {
            bytes,
            len: text.len() as u8,
        })
    }

    pub fn as_str(&self) -> &str {
        // Whole UTF-8 by `new`, which copied a `str` whole.
        std::str::from_utf8(&self.bytes[..usize::from(self.len)]).unwrap_or_default()
    }
}

impl std::fmt::Debug for Source {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.as_str(), formatter)
    }
}

/// Who asked for a gesture or a move on the mood socket, as its datagram
/// says, under `lamp-body --arbitration on`: neither, for every caller
/// before #940.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Asker {
    /// Free text naming the asker, for the journal (`skill:dance`, `voice`).
    pub source: Option<String>,
    pub priority: Option<Priority>,
    /// `"replace": true`: ends the motion playing, if it is `skill` or
    /// lower ([`crate::body::Asked::replace`]). Absent, false.
    pub replace: bool,
}

impl Asker {
    pub fn is_none(&self) -> bool {
        self.source.is_none() && self.priority.is_none()
    }

    pub fn priority(&self) -> Priority {
        self.priority.unwrap_or(Priority::Conversation)
    }
}

/// A `{"move":{...}}` datagram's move and who asks for it; `None` for a
/// datagram that is not a move, or says anything else.
fn asked(datagram: &[u8]) -> Option<(serde_json::Value, Asker)> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Message {
        #[serde(rename = "move")]
        sent: serde_json::Value,
        source: Option<String>,
        priority: Option<Priority>,
        #[serde(default)]
        replace: bool,
    }
    let message: Message = serde_json::from_slice(datagram).ok()?;
    let asker = Asker {
        source: message.source,
        priority: message.priority,
        replace: message.replace,
    };
    Some((message.sent, asker))
}

/// At most this many times a run a sent move takes the place of one of the
/// same name; past it, a changed move is refused until the body restarts.
pub const MOST_REPLACED: usize = 64;

/// Moves a skill sent whole on the mood socket, one a name: the same move
/// sent again plays what was kept, and a changed one of the same name (a
/// skill edited and reloaded) takes its place, at most [`MOST_REPLACED`]
/// times a run (a replaced move stays in memory, as it may still be
/// playing). A name a `--moves` file holds is refused ([`Sent::besides`]).
#[derive(Debug, Default)]
pub struct Sent {
    kept: Vec<&'static Clip>,
    replaced: usize,
    files: Vec<&'static str>,
}

impl Sent {
    /// None sent yet, beside the moves read from `--moves`, whose names a
    /// sent move may not take.
    pub fn besides(files: &Loaded) -> Self {
        Self {
            files: files.moves.iter().map(|clip| clip.name).collect(),
            ..Self::default()
        }
    }

    /// The clip of a `{"move":{...}}` datagram, or why the body cannot play
    /// it; `None` for a datagram that is not a move.
    pub fn take(&mut self, datagram: &[u8]) -> Option<Result<&'static Clip, String>> {
        self.take_asked(datagram).map(|(taken, _)| taken)
    }

    /// The same, for a datagram that may also say who asks (`source`,
    /// `priority`), under `lamp-body --arbitration on` (#940).
    pub fn take_asked(
        &mut self,
        datagram: &[u8],
    ) -> Option<(Result<&'static Clip, String>, Asker)> {
        let (sent, asker) = asked(datagram)?;
        Some((self.keep(sent), asker))
    }

    fn keep(&mut self, sent: serde_json::Value) -> Result<&'static Clip, String> {
        let sent: Move =
            serde_json::from_value(sent).map_err(|error| format!("not a move: {error}"))?;
        check(&sent).map_err(|why| format!("move {:?}: {why}", sent.name))?;
        if self.files.contains(&sent.name.as_str()) {
            return Err(format!(
                "move {:?}: a --moves file's move has the name",
                sent.name
            ));
        }
        let same_name = self.kept.iter().position(|clip| clip.name == sent.name);
        if let Some(kept) = same_name.map(|at| self.kept[at])
            && kept.step_ms == sent.step_ms
            && kept.frames == sent.frames
        {
            return Ok(kept);
        }
        if same_name.is_none() && self.kept.len() >= MOST_MOVES {
            return Err(format!(
                "move {:?}: {MOST_MOVES} moves of other names were sent already, none of a new name until the body restarts",
                sent.name
            ));
        }
        if same_name.is_some() && self.replaced >= MOST_REPLACED {
            return Err(format!(
                "move {:?}: sent moves were changed {MOST_REPLACED} times already, none changed again until the body restarts",
                sent.name
            ));
        }
        // ponytail: a replaced move stays in memory (it may still be
        // playing; at most 7 KB each, 64 a run); own the frames (`Arc`)
        // if skills must be edited more than that in one run.
        let clip = keep(sent);
        match same_name {
            Some(at) => {
                self.kept[at] = clip;
                self.replaced += 1;
            }
            None => self.kept.push(clip),
        }
        Ok(clip)
    }
}

/// Read every `*.json` in `folder`. A folder that cannot be read is an
/// error; a file that is not a move this body can play is refused and the
/// rest are taken. A folder of more than [`MOST_MOVES`] moves or
/// [`MOST_ENTRIES`] entries is refused whole, in one line and without
/// reading on, so the start's work stays bounded and which moves play never
/// depends on how many were left there. The moves live as long as the body:
/// they are read once, at start.
pub fn load(folder: &Path) -> io::Result<Loaded> {
    let mut files = Vec::new();
    for (looked, entry) in fs::read_dir(folder)?.enumerate() {
        let too_many = if looked >= MOST_ENTRIES {
            Some(format!("more than {MOST_ENTRIES} entries"))
        } else if files.len() > MOST_MOVES {
            Some(format!("more than {MOST_MOVES} moves"))
        } else {
            None
        };
        if let Some(why) = too_many {
            return Ok(Loaded {
                moves: Vec::new(),
                refused: vec![(
                    folder.to_owned(),
                    format!("{why} in the folder: none taken"),
                )],
            });
        }
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        // A Mac's `._` files and anything hidden are not moves.
        if name.starts_with('.') || path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        files.push(path);
    }
    if files.len() > MOST_MOVES {
        return Ok(Loaded {
            moves: Vec::new(),
            refused: vec![(
                folder.to_owned(),
                format!("more than {MOST_MOVES} moves in the folder: none taken"),
            )],
        });
    }
    files.sort();
    let mut loaded = Loaded::default();
    for path in files {
        let taken = read(&path).and_then(|file| {
            check(&file)?;
            // The first file in name order keeps the name.
            if loaded.named(&file.name).is_some() {
                return Err(format!("{:?} is already another file's move", file.name));
            }
            Ok(keep(file))
        });
        match taken {
            Ok(clip) => loaded.moves.push(clip),
            Err(why) => loaded.refused.push((path, why)),
        }
    }
    Ok(loaded)
}

/// One move file: refused unless a regular file of at most 256 KiB, before
/// it is read, and then unless JSON of exactly a move's three fields. Not
/// [`check`]ed.
pub fn read(path: &Path) -> Result<Move, String> {
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err("not a file".into());
    }
    if metadata.len() > MOST_BYTES {
        return Err(format!("longer than {MOST_BYTES} bytes"));
    }
    let text = fs::read(path).map_err(|error| error.to_string())?;
    serde_json::from_slice(&text).map_err(|error| format!("not a move: {error}"))
}

/// Whether this body can play the move, or why not: the bounds the
/// built-in clips keep. Whether it may play now, from the pose it is in
/// (the anti-tip rule), is the body's to judge as it begins.
pub fn check(file: &Move) -> Result<(), String> {
    let name = &file.name;
    if name.is_empty()
        || name.len() > 32
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_".contains(&byte))
    {
        return Err(format!(
            "{name:?} is not a name: 1 to 32 of a-z, 0-9, - and _"
        ));
    }
    if clips::named(name).is_some() || name == clips::CALLED.name || crate::idle::is_move_name(name)
    {
        return Err(format!("{name:?} is one of the body's own gestures"));
    }
    if !(40..=1_000).contains(&file.step_ms) {
        return Err(format!("step_ms {} is not from 40 to 1000", file.step_ms));
    }
    let frames = &file.frames;
    let duration_us = (frames.len().saturating_sub(1) as u64) * u64::from(file.step_ms) * 1_000;
    if !(SHORTEST_US..=LONGEST_US).contains(&duration_us) {
        return Err(format!(
            "it lasts {} ms, not 1500 to 10000",
            duration_us / 1_000
        ));
    }
    if frames.first() != Some(&[0; 5]) || frames.last() != Some(&[0; 5]) {
        return Err("it does not begin and end at nothing".into());
    }
    let step = f32::from(file.step_ms) / 1_000.0;
    for (at, pair) in frames.windows(2).enumerate() {
        for joint in 0..5 {
            if pair[1][joint].unsigned_abs() > FARTHEST[joint].unsigned_abs() {
                return Err(format!(
                    "frame {} takes joint {joint} {} counts, farther than {}",
                    at + 1,
                    pair[1][joint],
                    FARTHEST[joint]
                ));
            }
            let speed = (f32::from(pair[1][joint]) - f32::from(pair[0][joint])).abs() / step;
            if speed > FASTEST[joint] + 2.0 / step {
                return Err(format!(
                    "frame {} asks joint {joint} for {speed:.0} counts a second, faster than {}",
                    at + 1,
                    FASTEST[joint]
                ));
            }
        }
    }
    Ok(())
}

/// A checked move as a clip that lasts as long as the body.
fn keep(file: Move) -> &'static Clip {
    Box::leak(Box::new(Clip {
        name: Box::leak(file.name.into_boxed_str()),
        step_ms: file.step_ms,
        frames: Box::leak(file.frames.into_boxed_slice()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fifth on #940: an asker's name comes off a socket, so one longer
    /// than 64 bytes, empty or with a control character is not kept.
    #[test]
    fn a_source_is_kept_whole_up_to_64_printable_bytes_or_not_at_all() {
        let longest = format!("skill:{}", "a".repeat(58));
        assert_eq!(Source::new(&longest).unwrap().as_str(), longest);
        assert_eq!(Source::new("skill:café").unwrap().as_str(), "skill:café");
        for refused in [format!("{longest}b"), String::new(), "skill:\u{7}".into()] {
            assert_eq!(Source::new(&refused), None, "{refused:?}");
        }
        // Over 64 bytes in fewer characters: refused, never cut mid-letter.
        assert_eq!(Source::new(&format!("skill:{}", "é".repeat(30))), None);
    }

    struct Folder(PathBuf);
    impl Folder {
        fn new(name: &str) -> Self {
            let folder =
                std::env::temp_dir().join(format!("lamp-moves-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&folder);
            fs::create_dir_all(&folder).unwrap();
            Self(folder)
        }
        fn write(&self, file: &str, text: &str) {
            fs::write(self.0.join(file), text).unwrap();
        }
    }
    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A 2 s move: up to 40 counts of head pitch and back, 50 ms a step.
    fn good(name: &str) -> String {
        let mut frames = vec!["[0,0,0,0,0]".to_owned()];
        for step in 1..40 {
            let pitch = if step <= 20 {
                step * 2
            } else {
                (40 - step) * 2
            };
            frames.push(format!("[{pitch},0,0,0,0]"));
        }
        frames.push("[0,0,0,0,0]".into());
        format!(
            "{{\"name\":\"{name}\",\"step_ms\":50,\"frames\":[{}]}}",
            frames.join(",")
        )
    }

    #[test]
    fn a_folder_of_moves_is_read_in_file_order_and_each_plays_as_a_gesture() {
        let folder = Folder::new("good");
        folder.write("b-wiggle.json", &good("wiggle"));
        folder.write("a-peek.json", &good("peek-left"));
        folder.write("._a-peek.json", "\0\0mac");
        folder.write("notes.txt", "not a move");
        let loaded = load(&folder.0).unwrap();
        assert!(loaded.refused.is_empty(), "{:?}", loaded.refused);
        let names: Vec<_> = loaded.moves.iter().map(|clip| clip.name).collect();
        assert_eq!(names, ["peek-left", "wiggle"]);
        let wiggle = loaded.named("wiggle").unwrap();
        assert_eq!(wiggle.duration_us(), 2_000_000);
        assert_eq!(wiggle.at(1_000_000), Some([40.0, 0.0, 0.0, 0.0, 0.0]));
        assert_eq!(wiggle.at(2_000_000), None);
        assert_eq!(loaded.named("nod"), None);
    }

    #[test]
    fn a_move_the_body_cannot_play_is_refused_with_why_and_the_rest_are_taken() {
        let folder = Folder::new("refused");
        folder.write("ok.json", &good("ok"));
        let cases = [
            ("bad-json.json", "{".to_owned(), "not a move"),
            (
                "extra.json",
                good("extra").replace("\"step_ms\"", "\"speed\":1,\"step_ms\""),
                "not a move",
            ),
            ("nod.json", good("nod"), "body's own gestures"),
            ("called.json", good("called"), "body's own gestures"),
            ("look-up.json", good("look_up"), "body's own gestures"),
            ("z-again.json", good("ok"), "already another file's move"),
            ("upper.json", good("Wiggle"), "is not a name"),
            (
                "slow-step.json",
                good("slow").replace("\"step_ms\":50", "\"step_ms\":20"),
                "not from 40 to 1000",
            ),
            (
                "short.json",
                "{\"name\":\"short\",\"step_ms\":50,\"frames\":[[0,0,0,0,0],[0,0,0,0,0]]}".into(),
                "not 1500 to 10000",
            ),
            (
                "long.json",
                good("long").replace("\"step_ms\":50", "\"step_ms\":300"),
                "not 1500 to 10000",
            ),
            (
                "ends.json",
                good("ends").replace("[2,0,0,0,0],[0,0,0,0,0]]", "[2,0,0,0,0],[2,0,0,0,0]]"),
                "begin and end at nothing",
            ),
            (
                "far.json",
                good("far").replace("[40,0,0,0,0]", "[40,0,151,0,0]"),
                "farther than 150",
            ),
            (
                "fast.json",
                good("fast").replace("[40,0,0,0,0]", "[40,0,0,0,90]"),
                "faster than 800",
            ),
        ];
        for (file, text, _) in &cases {
            folder.write(file, text);
        }
        let loaded = load(&folder.0).unwrap();
        let names: Vec<_> = loaded.moves.iter().map(|clip| clip.name).collect();
        assert_eq!(names, ["ok"]);
        assert_eq!(loaded.refused.len(), cases.len());
        for (file, _, why) in cases {
            let (_, said) = loaded
                .refused
                .iter()
                .find(|(path, _)| path.ends_with(file))
                .unwrap_or_else(|| panic!("{file} not refused"));
            assert!(said.contains(why), "{file}: {said}");
        }
    }

    #[test]
    fn a_folder_that_cannot_be_read_is_an_error_and_an_oversized_one_is_refused_whole() {
        assert!(load(Path::new("/nonexistent/lamp-moves")).is_err());
        let folder = Folder::new("many");
        for n in 0..MOST_MOVES {
            folder.write(&format!("{n:03}.json"), &good(&format!("m{n}")));
        }
        assert_eq!(load(&folder.0).unwrap().moves.len(), MOST_MOVES);
        // One more: none taken, in one line naming the folder.
        folder.write("064.json", &good("m64"));
        let loaded = load(&folder.0).unwrap();
        assert!(loaded.moves.is_empty());
        assert_eq!(loaded.refused.len(), 1);
        assert_eq!(loaded.refused[0].0, folder.0);
        assert!(
            loaded.refused[0].1.contains("more than 64 moves"),
            "{:?}",
            loaded.refused
        );
        // A folder of many other files is not read through either.
        let crowded = Folder::new("crowded");
        for n in 0..=MOST_ENTRIES {
            crowded.write(&format!("{n:04}.txt"), "");
        }
        let loaded = load(&crowded.0).unwrap();
        assert!(loaded.moves.is_empty());
        assert_eq!(loaded.refused.len(), 1);
        assert!(
            loaded.refused[0].1.contains("more than 1024 entries"),
            "{:?}",
            loaded.refused
        );
    }

    #[test]
    fn the_documented_example_is_a_move_this_body_takes() {
        let folder = Folder::new("example");
        let module = include_str!("moves.rs");
        let example = module
            .lines()
            .find_map(|line| line.strip_prefix("//! {\"name\""))
            .map(|rest| format!("{{\"name\"{rest}"))
            .unwrap();
        folder.write("wiggle.json", &example);
        let loaded = load(&folder.0).unwrap();
        assert!(loaded.refused.is_empty(), "{:?}", loaded.refused);
        assert_eq!(loaded.named("wiggle").unwrap().duration_us(), 1_500_000);
    }

    #[test]
    fn a_move_sent_whole_is_checked_as_a_file_is_and_kept_once() {
        let mut sent = Sent::default();
        let datagram = format!("{{\"move\":{}}}", good("wiggle"));
        assert!(datagram.len() < MOST_DATAGRAM);
        let first = sent.take(datagram.as_bytes()).unwrap().unwrap();
        assert_eq!(first.name, "wiggle");
        assert_eq!(first.at(1_000_000), Some([40.0, 0.0, 0.0, 0.0, 0.0]));
        // The same move again plays what was kept.
        let again = sent.take(datagram.as_bytes()).unwrap().unwrap();
        assert!(std::ptr::eq(first, again));
        assert_eq!(sent.kept.len(), 1);
        // Edited, the same name takes the new frames' place.
        let edited = datagram.replace("[40,0,0,0,0]", "[38,0,0,0,0]");
        let edited = sent.take(edited.as_bytes()).unwrap().unwrap();
        assert_eq!(edited.at(1_000_000), Some([38.0, 0.0, 0.0, 0.0, 0.0]));
        assert_eq!(sent.kept.len(), 1);
        // Past 64 names a new name is refused; an old one still plays.
        for n in 1..MOST_MOVES {
            let other = format!("{{\"move\":{}}}", good(&format!("m{n}")));
            sent.take(other.as_bytes()).unwrap().unwrap();
        }
        let new = format!("{{\"move\":{}}}", good("one-more"));
        let why = sent.take(new.as_bytes()).unwrap().unwrap_err();
        assert!(why.contains("64 moves of other names"), "{why}");
        assert!(sent.take(datagram.as_bytes()).unwrap().is_ok());
        assert_eq!(sent.kept.len(), MOST_MOVES);
        // Changed again and again, a move is changed 64 times a run and then
        // refused with why; what was kept last still plays.
        let changed =
            |n: i16| datagram.replace("[40,0,0,0,0]", &format!("[{},0,0,0,0]", 30 + n % 2));
        let mut n = 0;
        let why = loop {
            match sent.take(changed(n).as_bytes()).unwrap() {
                Ok(_) => n += 1,
                Err(why) => break why,
            }
        };
        assert!(why.contains("changed 64 times"), "{why}");
        assert_eq!(sent.replaced, MOST_REPLACED);
        assert!(sent.take(changed(n - 1).as_bytes()).unwrap().is_ok());
        // A name a --moves file holds is refused with why.
        const WIGGLE: Clip = Clip {
            name: "wiggle",
            step_ms: 100,
            frames: &[[0; 5], [20, 0, 0, 0, 0], [0; 5]],
        };
        let files = Loaded {
            moves: vec![&WIGGLE],
            refused: Vec::new(),
        };
        let why = Sent::besides(&files)
            .take(datagram.as_bytes())
            .unwrap()
            .unwrap_err();
        assert!(why.contains("a --moves file's move has the name"), "{why}");
        let mut sent = Sent::default();
        // One the body cannot play is refused with why.
        let far = datagram.replace("[40,0,0,0,0]", "[40,0,151,0,0]");
        let why = sent.take(far.as_bytes()).unwrap().unwrap_err();
        assert!(
            why.contains("\"wiggle\"") && why.contains("farther than 150"),
            "{why}"
        );
        let nod = format!("{{\"move\":{}}}", good("nod"));
        assert!(
            sent.take(nod.as_bytes())
                .unwrap()
                .unwrap_err()
                .contains("own gestures")
        );
        assert!(sent.take(br#"{"move":{"name":"x"}}"#).unwrap().is_err());
        // A gesture or a mood is not a move.
        assert!(sent.take(br#"{"gesture":"nod"}"#).is_none());
        assert!(sent.take(br#"{"mood":"listening","level":3}"#).is_none());
        // The widest move check could take (not one it takes) fits in a datagram.
        let largest = Move {
            name: "a".repeat(32),
            step_ms: 40,
            frames: (0..251)
                .map(|at| {
                    if at == 0 || at == 250 {
                        [0; 5]
                    } else {
                        [-200, -200, -150, -350, -350]
                    }
                })
                .collect(),
        };
        let text = serde_json::json!({ "move": largest }).to_string();
        assert!(text.len() < MOST_DATAGRAM, "{}", text.len());
    }

    #[test]
    fn the_promise_is_the_built_in_clips_own() {
        // Every built-in clip would be taken as a move file.
        for clip in clips::ALL.into_iter().chain([&clips::CALLED]) {
            if clip.duration_us() < SHORTEST_US {
                continue;
            }
            let file = Move {
                name: format!("{}-again", clip.name),
                step_ms: clip.step_ms,
                frames: clip.frames.to_vec(),
            };
            assert!(check(&file).is_ok(), "{}", clip.name);
        }
    }
}
