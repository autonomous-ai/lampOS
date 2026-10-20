//! The memory's id for a person the eyes know, and the eyes' `known` file
//! that carries it (#2176, `docs/memory.md` "Each person's own"). One copy,
//! read by the eyes that write the file, the voice that keys memory on it,
//! the skills that read the name, and the cloud that forgets a person.
//!
//! With [`ENVIRONMENT`] on, each remembered person keeps one id from the
//! first time they are remembered, through every later remembering and a
//! rename, and the eyes write it beside the name: `NAME\nID\n`. Off, the
//! file is the name alone, `NAME\n`, as before, and memory's id is
//! [`person_id`] of the name.

/// Where the stable id is switched on, for the eyes and the voice alike.
/// `on` only.
pub const ENVIRONMENT: &str = "LAMP_MEMORY_STABLE_ID";

/// Whether the stable id is on.
pub fn stable_ids() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// The memory's person id for a name: `p-` and the first 16 hex digits of
/// SHA-256 of the trimmed, lower-cased name. It fits the memory's
/// `[a-z0-9_-]{1,32}`, and no name is ever in a path or a log line. With
/// the stable id, the id a person is first given, and the id of anyone
/// remembered before it was on, so nobody's memory moves.
pub fn person_id(name: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(name.trim().to_lowercase().as_bytes());
    digest[..8]
        .iter()
        .fold(String::from("p-"), |id, byte| id + &format!("{byte:02x}"))
}

/// Whether `id` is one the eyes give: [`person_id`]'s shape, or it with a
/// `-` and up to three digits, given when that id is already another's.
pub fn is_person_id(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("p-") else {
        return false;
    };
    let (hex, suffix) = rest.split_at_checked(16).unwrap_or((rest, ""));
    hex.len() == 16
        && hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && (suffix.is_empty()
            || suffix.strip_prefix('-').is_some_and(|digits| {
                (1..=3).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit())
            }))
}

/// `base`, or, when another already has it, `base` with the lowest `-N`
/// (from 2) nobody has: two people never share one id.
pub fn unique_id(base: &str, taken: &[&str]) -> String {
    if !taken.contains(&base) {
        return base.to_owned();
    }
    (2..1000)
        .map(|n| format!("{base}-{n}"))
        .find(|id| !taken.contains(&id.as_str()))
        .unwrap_or_else(|| base.to_owned())
}

/// The `known` file's text for this person: the name, and the id on the
/// line after it when there is one.
pub fn known_text(name: &str, id: Option<&str>) -> String {
    match id {
        Some(id) => format!("{name}\n{id}"),
        None => name.to_owned(),
    }
}

/// The name and, when there is one, the id in a `known` file's text: the
/// first line is the name, trimmed; a second line, if any, must be an id
/// ([`is_person_id`]), else nothing is read. No name, or a third line, is
/// nothing. Each reader keeps its own bounds on the name.
pub fn read_known(text: &str) -> Option<(&str, Option<&str>)> {
    let mut lines = text.trim_end().lines();
    let name = lines.next()?.trim();
    let id = match lines.next() {
        None => None,
        Some(line) => {
            let id = line.trim();
            if !is_person_id(id) {
                return None;
            }
            Some(id)
        }
    };
    (!name.is_empty() && lines.next().is_none()).then_some((name, id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_person_id_is_their_name_hashed_as_the_voice_and_cloud_always_made_it() {
        // SHA-256 of "minh" begins be9688998f085098.
        assert_eq!(person_id("Minh"), "p-be9688998f085098");
        assert_eq!(person_id("Lan"), "p-094a367b026246fb");
        assert_eq!(person_id(" MINH "), person_id("minh"));
        assert!(is_person_id(&person_id("Nguyễn Văn A")));
    }

    #[test]
    fn only_the_ids_the_eyes_give_are_ids() {
        assert!(is_person_id("p-be9688998f085098"));
        assert!(is_person_id("p-be9688998f085098-2"));
        assert!(is_person_id("p-be9688998f085098-999"));
        for not in [
            "",
            "p-",
            "be9688998f085098",
            "p-be9688998f08509",
            "p-BE9688998F085098",
            "p-be9688998f085098-",
            "p-be9688998f085098-1000",
            "p-be9688998f085098x",
            "p-../../etc/passwd00",
            "r-be9688998f085098",
        ] {
            assert!(!is_person_id(not), "{not}");
        }
    }

    #[test]
    fn two_people_never_share_one_id() {
        let minh = person_id("Minh");
        assert_eq!(unique_id(&minh, &[]), minh);
        assert_eq!(unique_id(&minh, &["p-0000000000000000"]), minh);
        assert_eq!(unique_id(&minh, &[&minh]), format!("{minh}-2"));
        let two = format!("{minh}-2");
        assert_eq!(unique_id(&minh, &[&minh, &two]), format!("{minh}-3"));
        assert!(is_person_id(&unique_id(&minh, &[&minh])));
    }

    #[test]
    fn the_known_file_is_the_name_alone_or_the_name_then_the_id() {
        let id = person_id("Minh");
        assert_eq!(known_text("Sam", None), "Sam");
        assert_eq!(known_text("Sam", Some(&id)), format!("Sam\n{id}"));
        assert_eq!(read_known("Sam\n"), Some(("Sam", None)));
        assert_eq!(
            read_known(&format!("Sam\n{id}\n")),
            Some(("Sam", Some(id.as_str())))
        );
        assert_eq!(read_known("Nguyễn Văn A\n"), Some(("Nguyễn Văn A", None)));
        assert_eq!(read_known("\n"), None);
        assert_eq!(
            read_known("Sam\nBeta\n"),
            None,
            "a second name is not an id"
        );
        assert_eq!(
            read_known(&format!("Sam\n{id}\n{id}\n")),
            None,
            "three lines"
        );
        assert_eq!(read_known(&format!("\n{id}\n")), None, "no name");
    }
}
