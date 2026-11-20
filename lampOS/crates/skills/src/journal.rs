//! What `lamp-skills` writes to its journal about the skills that read
//! `humans/doing_more` (#6543, the privacy read on #8375): their names say
//! what was seen (a drink, eating, a yawn or a stretch, celebrating), and
//! the journal stays on the lamp across reboots, past a forget and the
//! opt-in going off. So unless [`LABELS`] is `on` in `lamp-skills`'
//! environment, a journal line that names one says [`NEUTRAL`] in its
//! place, and the readings that name a kind ([`READINGS`]) say [`READING`],
//! as `lamp-seen`'s own journal keeps to counts (docs/seen.md, "The
//! journal"). The skills hidden are [`crate::SEEN_PRODUCT`] always, every
//! skill an engine loads that names the topic, and every name kept in
//! [`crate::SEEN_MORE_FILE`]. Only the journal changes: what goes on the
//! bus is as before.
use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};

/// `lamp-seen`'s diagnostics switch (`lamp_seen::JOURNAL_LABELS`), read
/// here too, once: `on` exactly names them. In no unit.
pub const LABELS: &str = "LAMP_SEEN_JOURNAL_LABELS";
/// Said in a hidden skill's place.
pub const NEUTRAL: &str = "a seen reaction";
/// Said in a reading's place.
pub const READING: &str = "a reading";
/// The words that name a kind seen, on a line that names a hidden skill or
/// `humans/doing_more`: its fields (`lamp_msgs::HumanDoingMore`), longest
/// first, and Seen tired's two trigger names.
pub const READINGS: [&str; 14] = [
    "desk_work_s_ago",
    "celebrate_seen",
    "stretch_s_ago",
    "stretch_seen",
    "tired_s_ago",
    "tired_seen",
    "drank_seen",
    "yawn_s_ago",
    "yawn_seen",
    "desk_work",
    "ate_seen",
    "desk_s",
    "stretch",
    "yawn",
];

/// Whether [`LABELS`] is on: read once, at the first line.
pub fn labels() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var(LABELS).is_ok_and(|value| value == "on"))
}

fn hidden() -> &'static Mutex<BTreeSet<String>> {
    static HIDDEN: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
    HIDDEN.get_or_init(|| {
        Mutex::new(
            crate::SEEN_PRODUCT
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
        )
    })
}

/// `names` kept out of the journal from now on, while this process runs.
pub fn hide<'a>(names: impl IntoIterator<Item = &'a str>) {
    let mut hidden = hidden()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for name in names {
        if !hidden.contains(name) {
            hidden.insert(name.to_owned());
        }
    }
}

/// The skills kept out of the journal now.
pub fn hidden_now() -> BTreeSet<String> {
    hidden()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// `line` as the journal says it: as it is with `labels`, else with each of
/// `hidden` as [`NEUTRAL`] and, on a line that named one or names
/// `humans/doing_more`, each of [`READINGS`] as a whole word as [`READING`].
pub fn redact(line: &str, hidden: &BTreeSet<String>, labels: bool) -> String {
    if labels {
        return line.to_owned();
    }
    let named = hidden.iter().any(|name| line.contains(name.as_str()));
    if !named && !line.contains(crate::SEEN_MORE_TOPIC) {
        return line.to_owned();
    }
    let mut names: Vec<&String> = hidden.iter().collect();
    names.sort_by_key(|name| std::cmp::Reverse(name.len()));
    let mut out = line.to_owned();
    for name in names {
        out = out.replace(name.as_str(), NEUTRAL);
    }
    for word in READINGS {
        out = whole_words(&out, word, READING);
    }
    out
}

/// `text` with each `word` that stands alone (no letter, digit or `_` on
/// either side) as `with`.
fn whole_words(text: &str, word: &str, with: &str) -> String {
    let part = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut before: Option<char> = None;
    while let Some(at) = rest.find(word) {
        let (head, tail) = rest.split_at(at);
        let last = head.chars().next_back().or(before);
        let after = tail[word.len()..].chars().next();
        out.push_str(head);
        if part(last) || part(after) {
            out.push_str(word);
        } else {
            out.push_str(with);
        }
        before = word.chars().next_back();
        rest = &tail[word.len()..];
    }
    out.push_str(rest);
    out
}

/// One journal line, to standard error: [`redact`]ed by [`hidden_now`]
/// unless [`labels`].
pub fn line(text: String) {
    let said = if labels() {
        text
    } else {
        let hidden = hidden()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        redact(&text, &hidden, false)
    };
    std::eprintln!("{said}");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn four() -> BTreeSet<String> {
        crate::SEEN_PRODUCT
            .iter()
            .map(|name| (*name).to_owned())
            .collect()
    }

    #[test]
    fn a_seen_reaction_is_never_named_with_the_switch_off() {
        let lines = [
            "lamp-skills: seen-drink Triggered: humans/doing_more has held for 0 s",
            "lamp-skills: seen-eat Done: look at the face: told the body; say: spoken",
            "lamp-skills: skill:seen-tired: humans/doing_more not given to it",
            "lamp-skills: seen-tired Triggered: yawn (humans/doing_more yawn_seen above 0)",
            "lamp-skills: seen-celebrate Asked: celebrate_seen rose",
        ];
        for line in lines {
            let said = redact(line, &four(), false);
            for word in crate::SEEN_PRODUCT
                .iter()
                .chain(["yawn", "yawn_seen", "celebrate_seen"].iter())
            {
                assert!(!said.contains(word), "{word} in {said}");
            }
            assert!(said.contains(NEUTRAL), "{said}");
            // On, as it was.
            assert_eq!(redact(line, &four(), true), line);
        }
        assert_eq!(
            redact(lines[0], &four(), false),
            "lamp-skills: a seen reaction Triggered: humans/doing_more has held for 0 s"
        );
        // Another skill's line is as it was, readings and all.
        let other = "lamp-skills: comfort-break Triggered: drank_seen above 1; yawn";
        assert_eq!(redact(other, &four(), false), other);
        // A word that only holds a reading's is left.
        assert_eq!(
            whole_words("yawning yawn_seen_x yawn.", "yawn", READING),
            "yawning yawn_seen_x a reading."
        );
    }

    #[test]
    fn the_four_and_any_name_hidden_stay_out() {
        hide(["tired-glow"]);
        let hidden = hidden_now();
        for name in crate::SEEN_PRODUCT.iter().chain(["tired-glow"].iter()) {
            assert!(hidden.contains(*name), "{name}");
        }
        let said = redact("lamp-skills: tired-glow Done: light: shown", &hidden, false);
        assert_eq!(said, "lamp-skills: a seen reaction Done: light: shown");
    }
}
