//! The owner picked by face in the app (#5495, `docs/cloud.md`, "The owner
//! by face"; second-voice.md, "The owner"). With `LAMP_OWNER_FACE=on`, once
//! the app picked a face, the owner is that face, found by the id the eyes
//! keep for it (`lamp_msgs::owner_face`), under whatever name the eyes keep
//! it now. The typed name and the face setup's "who you are" kept no longer
//! count, so a name alone, typed or said, is never the owner.
//!
//! The pick is read from root's file beside the typed name's
//! (`/etc/lampOS/person/owner-face`; the voice runs as root), again at each
//! look, so a pick or a clear from the app holds from the next reply. The
//! eyes' faces and their ids are asked off any reply's path, with the
//! setup owner's refresh (`known_faces::keep_owner_fresh`, every 10 s), and
//! dropped at once when a face is forgotten. Fail-closed: a pick whose face
//! the eyes don't keep now (removed, merged away, an older eyes build, or
//! eyes not reached) is no owner, never the typed name.
//!
//! With no pick ever made, or the switch off, nothing here decides: the
//! typed name and the setup owner are the owner, as before.
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use lamp_msgs::owner_face::Pick;

/// The resolved owner's name, for `lamp-skills`' reads (#5495): an absolute
/// path the voice writes the owner's name to at each refresh, one line, or
/// empty for none. Unset, nothing is written.
pub const NAME_FILE_ENVIRONMENT: &str = "LAMP_OWNER_NAME_FILE";

/// The eyes' faces as last asked: (face id, name), `None` before the first
/// answer, after the eyes couldn't be reached, and after a forget; and how
/// many forgets there were, so an answer asked before one is dropped.
type Faces = (Option<Vec<(String, String)>>, u64);
static FACES: Mutex<Faces> = Mutex::new((None, 0));

fn faces() -> std::sync::MutexGuard<'static, Faces> {
    FACES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A test's pick and the eyes' faces (`None`: not known now).
#[cfg(test)]
pub(crate) type ForTest = (Pick, Option<Vec<(String, String)>>);

#[cfg(test)]
thread_local! {
    /// This thread's switch, pick and faces, for a test.
    static FOR_TEST: std::cell::RefCell<Option<ForTest>> =
        const { std::cell::RefCell::new(None) };
}

/// Tests: the switch on, with this pick and these faces, on this thread;
/// `None` reads the environment again.
#[cfg(test)]
pub(crate) fn set_for_test(given: Option<ForTest>) {
    FOR_TEST.with(|cell| *cell.borrow_mut() = given);
}

/// Whether `LAMP_OWNER_FACE` is exactly `on` here.
pub fn on() -> bool {
    #[cfg(test)]
    if FOR_TEST.with(|cell| cell.borrow().is_some()) {
        return true;
    }
    lamp_msgs::owner_face::on()
}

/// The pick's file: beside the typed name's (`LAMP_PERSON_NAME`), else the
/// product's.
fn pick_file() -> PathBuf {
    std::env::var_os(crate::person::ENVIRONMENT)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .map_or_else(
            || PathBuf::from(lamp_msgs::owner_face::FILE),
            |name| lamp_msgs::owner_face::beside_name(&name),
        )
}

/// The owner by face now: `None` while the typed name decides (the switch
/// off, or no face ever picked); `Some(None)` for no owner; `Some(name)`
/// for the picked face, under the name the eyes keep it now.
pub fn owner() -> Option<Option<String>> {
    #[cfg(test)]
    if let Some((pick, faces)) = FOR_TEST.with(|cell| cell.borrow().clone()) {
        return resolve(&pick, faces.as_deref());
    }
    if !on() {
        return None;
    }
    let pick = lamp_msgs::owner_face::read(&pick_file());
    if !pick.made() {
        return None;
    }
    let known = faces().0.clone();
    resolve(&pick, known.as_deref())
}

/// [`owner`] from the `pick` and the eyes' `faces` (`None`: not known now).
pub fn resolve(pick: &Pick, faces: Option<&[(String, String)]>) -> Option<Option<String>> {
    match pick {
        Pick::Never => None,
        Pick::Nobody => Some(None),
        Pick::Face(id) => Some(
            faces
                .and_then(|faces| faces.iter().find(|(face, _)| face == id))
                .map(|(_, name)| name.clone()),
        ),
    }
}

/// The eyes' `{"people":[{"name","face"}…]}` as (face id, name), each
/// with a face id; `None` for an answer that isn't one.
pub fn faces_of(answer: &serde_json::Value) -> Option<Vec<(String, String)>> {
    let people = answer["people"].as_array()?;
    Some(
        people
            .iter()
            .filter_map(|person| {
                let face = person["face"]
                    .as_str()
                    .filter(|face| lamp_msgs::owner_face::is_face_id(face))?;
                Some((face.to_owned(), person["name"].as_str()?.to_owned()))
            })
            .collect(),
    )
}

/// One refresh of the eyes' faces, asked on `socket` within
/// `known_faces::LIST_WITHIN`: eyes not reached, or with no ids to say,
/// leave none known (no owner by face); eyes reached but slow keep what
/// was known, as the setup owner's refresh does. Dropped if a face was
/// forgotten while the eyes were asked.
pub fn refresh(socket: Option<&Path>) {
    let forgets = faces().1;
    let ask = serde_json::json!({"list": true, "waiting": true, "ids": true});
    let found = match socket
        .and_then(|socket| crate::tools::reached(socket, &ask, crate::known_faces::LIST_WITHIN))
    {
        Some(Some(answer)) => faces_of(&answer),
        Some(None) => return,
        None => None,
    };
    {
        let mut now = faces();
        if now.1 != forgets {
            return;
        }
        now.0 = found;
    }
    publish();
}

/// With the switch on, the eyes' faces asked every
/// `known_faces::OWNER_EVERY` on a thread of its own, and once first at the
/// worker's start when a face was picked, off any reply's path, so its
/// first reply (and the Live route's name, read once a session) knows the
/// owner. Started once per process, however many parts ask for it.
pub fn keep_fresh(socket: Option<PathBuf>) {
    if !lamp_msgs::owner_face::on() {
        return;
    }
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        if lamp_msgs::owner_face::read(&pick_file()).made() {
            refresh(socket.as_deref());
        }
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(crate::known_faces::OWNER_EVERY);
                refresh(socket.as_deref());
            }
        });
    });
}

/// A face forgotten, whoever's: the faces are known again only at the next
/// refresh, so a picked face just forgotten is never the owner meanwhile.
pub fn forgotten() {
    {
        let mut now = faces();
        now.0 = None;
        now.1 = now.1.wrapping_add(1);
    }
    publish();
}

/// A face saved or renamed (`remember_my_face`): the faces asked again off
/// the call's path, so the owner's face is told by its new name soon.
pub fn saved(socket: Option<&Path>) {
    if !lamp_msgs::owner_face::on() {
        return;
    }
    let socket = socket.map(Path::to_path_buf);
    std::thread::spawn(move || refresh(socket.as_deref()));
}

/// The owner's name as the lamp knows it now, by face or typed
/// (`person::now`), written for `lamp-skills` to the file
/// [`NAME_FILE_ENVIRONMENT`] names: one line, or empty for none; root's,
/// 0644, staged and renamed into place. Only with the switch on.
fn publish() {
    if !on() {
        return;
    }
    let Some(path) = std::env::var_os(NAME_FILE_ENVIRONMENT)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    else {
        return;
    };
    let name = crate::person::now().unwrap_or_default();
    if let Err(error) = write_name(&path, &name) {
        eprintln!(
            "lamp-voice: the owner's name for the skills could not be written: {}",
            error.kind()
        );
    }
}

/// `name` and a newline at `path`, 0644, staged beside it and renamed.
fn write_name(path: &Path, name: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut staged = path.as_os_str().to_owned();
    staged.push(".new");
    let staged = PathBuf::from(staged);
    let _ = std::fs::remove_file(&staged);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(&staged)?;
    file.write_all(format!("{name}\n").as_bytes())?;
    file.sync_all()?;
    std::fs::rename(&staged, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const B: &str = "f-3c9a51d07be24e18";

    #[test]
    fn a_pick_is_the_face_by_its_id_and_nobody_once_it_is_gone() {
        let faces = vec![(B.to_owned(), "B".to_owned())];
        assert_eq!(resolve(&Pick::Never, Some(&faces)), None);
        assert_eq!(
            resolve(&Pick::Face(B.into()), Some(&faces)),
            Some(Some("B".into()))
        );
        assert_eq!(resolve(&Pick::Face(B.into()), Some(&[])), Some(None));
        // Eyes not known now: no owner, never the typed name.
        assert_eq!(resolve(&Pick::Face(B.into()), None), Some(None));
        assert_eq!(resolve(&Pick::Nobody, Some(&faces)), Some(None));
    }

    #[test]
    fn the_eyes_faces_are_read_with_their_ids_only() {
        let answer = serde_json::json!({"people": [
            {"name": "B", "face": B, "added_unix_s": 1},
            {"name": "A", "face": null},
            {"name": "C", "face": "Lan"},
        ], "waiting": ["D"]});
        assert_eq!(
            faces_of(&answer),
            Some(vec![(B.to_owned(), "B".to_owned())])
        );
        assert_eq!(faces_of(&serde_json::json!({"error": "unknown"})), None);
    }

    #[test]
    fn the_name_for_the_skills_is_one_line_written_whole() {
        let dir = std::env::temp_dir().join(format!("voice-owner-face-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("name");
        write_name(&path, "B").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "B\n");
        assert_eq!(crate::person::read(&path), Ok(Some("B".into())));
        write_name(&path, "").unwrap();
        assert_eq!(crate::person::read(&path), Ok(None));
        let _ = std::fs::remove_dir_all(dir);
    }
}
