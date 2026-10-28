use super::*;
use crate::onboarding::legacy_words::*;
use std::{
    fs,
    os::unix::fs::symlink,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

const SHIPPED: &str = include_str!("../../../../../skills/onboarding/skill.toml");

struct Folder(std::path::PathBuf);
impl Folder {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "onboarding-skill-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(path.join("onboarding")).unwrap();
        Self(path)
    }
    fn file(&self) -> std::path::PathBuf {
        self.0.join("onboarding/skill.toml")
    }
    fn write(&self, text: &str) {
        fs::write(self.file(), text).unwrap();
    }
}
impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn shipped_definition_matches_the_original_order_words_and_reoffers_byte_for_byte() {
    let definition = parse(SHIPPED).unwrap();
    let expected = [
        (Step::WhereYouSit, SIT_NOTE, SIT_OFFER),
        (Step::WhoYouAre, WHO_NOTE, WHO_NOTE),
        (Step::HowYouSound, NEAR_FIRST, NEAR_FIRST),
        (Step::Touch, TOUCH_NOTE, TOUCH_OFFER),
        (Step::Privacy, PRIVACY_INVITATION, PRIVACY_INVITATION),
        (Step::GivingWay, GIVE_ASK, GIVE_ASK),
        (Step::RoomSounds, ROOM_SOUNDS_NOTE, ROOM_SOUNDS_NOTE),
        (
            Step::FaceExpressions,
            FACE_EXPRESSIONS_NOTE,
            FACE_EXPRESSIONS_NOTE,
        ),
        (Step::Preferences, PREFERENCES_NOTE, PREFERENCES_NOTE),
    ];
    assert_eq!(definition.order, expected.map(|(step, _, _)| step));
    assert_eq!(definition.reoffer_after_s, 86_400);
    for (step, first, reoffer) in expected {
        assert_eq!(definition.note(step, false).as_bytes(), first.as_bytes());
        assert_eq!(definition.note(step, true).as_bytes(), reoffer.as_bytes());
    }
    assert_eq!(definition.nudge(Step::WhoYouAre), Some((WHO_NUDGE, 1)));
    assert_eq!(
        definition.nudge(Step::Preferences),
        Some((PREFERENCES_NUDGE, 2))
    );
    for step in definition
        .order
        .iter()
        .copied()
        .filter(|step| !matches!(step, Step::WhoYouAre | Step::Preferences))
    {
        assert_eq!(definition.nudge(step), None);
    }
    assert_eq!(
        definition.words.sit_no_face.as_bytes(),
        SIT_NO_FACE.as_bytes()
    );
    assert_eq!(
        definition.words.sit_closer.as_bytes(),
        SIT_CLOSER.as_bytes()
    );
    assert_eq!(definition.words.sit_turn.as_bytes(), SIT_TURN.as_bytes());
    assert_eq!(definition.words.sit_done.as_bytes(), SIT_DONE.as_bytes());
    assert_eq!(
        definition.words.sit_failed.as_bytes(),
        SIT_FAILED.as_bytes()
    );
    assert_eq!(definition.words.who_again.as_bytes(), WHO_AGAIN.as_bytes());
    assert_eq!(
        definition.words.touch_remind.as_bytes(),
        TOUCH_REMIND.as_bytes()
    );
    assert_eq!(
        definition.words.privacy_done.as_bytes(),
        PRIVACY_DONE.as_bytes()
    );
    assert_eq!(
        definition.words.privacy_done_camera.as_bytes(),
        PRIVACY_DONE_CAMERA.as_bytes()
    );
    assert_eq!(
        definition.words.privacy_failed.as_bytes(),
        PRIVACY_FAILED.as_bytes()
    );
    assert_eq!(
        definition.words.near_again.as_bytes(),
        NEAR_AGAIN.as_bytes()
    );
    assert_eq!(definition.words.near_missed.as_deref(), Some(NEAR_MISSED));
    assert_eq!(definition.words.near_done.as_bytes(), NEAR_DONE.as_bytes());
    assert_eq!(
        definition.words.near_failed.as_bytes(),
        NEAR_FAILED.as_bytes()
    );
    assert_eq!(
        definition.words.sentence_first.as_bytes(),
        SENTENCE_FIRST.as_bytes()
    );
    assert_eq!(
        definition.words.sentence_louder.as_bytes(),
        SENTENCE_LOUDER.as_bytes()
    );
    assert_eq!(
        definition.words.sentence_closer.as_bytes(),
        SENTENCE_CLOSER.as_bytes()
    );
    assert_eq!(
        definition.words.sentence_again.as_bytes(),
        SENTENCE_AGAIN.as_bytes()
    );
    assert_eq!(
        definition.words.sentence_done.as_bytes(),
        SENTENCE_DONE.as_bytes()
    );
    assert_eq!(
        definition.words.sentence_failed.as_bytes(),
        SENTENCE_FAILED.as_bytes()
    );
    assert_eq!(definition.words.touch_done.as_deref(), Some(TOUCH_DONE));
    assert_eq!(definition.words.touch_failed.as_deref(), Some(TOUCH_FAILED));
    assert_eq!(definition.words.give_again.as_deref(), Some(GIVE_AGAIN));
    assert_eq!(definition.words.give_felt.as_deref(), Some(GIVE_FELT));
    assert_eq!(definition.words.give_prompt.as_deref(), Some(GIVE_PROMPT));
    assert_eq!(definition.words.give_more.as_deref(), Some(GIVE_MORE));
    for (step, line) in [
        (Step::WhereYouSit, WHERE_YOU_SIT_SKIPPED),
        (Step::HowYouSound, HOW_YOU_SOUND_SKIPPED),
        (Step::Touch, TOUCH_SKIPPED),
        (Step::GivingWay, GIVING_WAY_SKIPPED),
    ] {
        assert_eq!(definition.skipped(step), Some(line));
    }
    assert_eq!(definition.skipped(Step::Privacy), None);
    assert_eq!(definition.words.give_done.as_bytes(), GIVE_DONE.as_bytes());
    assert_eq!(
        definition.words.give_failed.as_bytes(),
        GIVE_FAILED.as_bytes()
    );
    assert_eq!(
        definition.words.who_confirm.as_bytes(),
        WHO_CONFIRM.as_bytes()
    );
}

#[test]
fn invalid_sequence_bounds_words_and_slots_are_refused() {
    for (from, to) in [
        ("order = [\"where_you_sit\",", "order = [\"who_you_are\","),
        (
            "\"where_you_sit\", \"who_you_are\"",
            "\"unknown\", \"who_you_are\"",
        ),
        ("reoffer_after_s = 86400", "reoffer_after_s = 0"),
        ("reoffer_after_s = 86400", "reoffer_after_s = 31536001"),
        ("after_answers = 1", "after_answers = 0"),
        ("after_answers = 1", "after_answers = 9"),
        ("{location}", "{unknown}"),
        ("{name}", "{name}{name}"),
        ("{name}", "{name"),
        ("[onboarding.words]", "[onboarding.typo]"),
        ("[onboarding.steps.touch]", "[onboarding.steps.typo]"),
        // Only a measured step may have a skip line (#3769).
        (
            "[onboarding.steps.privacy]\n",
            "[onboarding.steps.privacy]\nskipped = \"(Say only this.)\"\n",
        ),
    ] {
        let changed = SHIPPED.replacen(from, to, 1);
        assert_ne!(changed, SHIPPED, "missing fixture replacement: {from}");
        assert!(parse(&changed).is_err(), "accepted {from} -> {to}");
    }
    for mutate in [
        |definition: &mut Definition| {
            definition.order.pop();
        },
        |definition: &mut Definition| {
            definition.steps.remove(&Step::Touch);
        },
        |definition: &mut Definition| {
            definition.words.who_again.clear();
        },
        |definition: &mut Definition| {
            definition.words.who_again = "x".repeat(crate::say::MOST_WORDS_BYTES + 1);
        },
        |definition: &mut Definition| {
            definition.words.who_again.push('\n');
        },
        |definition: &mut Definition| {
            definition.steps.get_mut(&Step::Touch).unwrap().nudge = Some(Nudge {
                words: "skip now".into(),
                after_answers: 1,
            });
        },
        |definition: &mut Definition| {
            definition.steps.get_mut(&Step::Privacy).unwrap().first =
                format!("{}{{location}}", "x".repeat(400));
        },
    ] {
        let mut definition = parse(SHIPPED).unwrap();
        mutate(&mut definition);
        assert!(definition.checked().is_err());
    }
    assert!(parse(&SHIPPED.repeat(10)).is_err());
    assert!(parse("kind = \"core\"").is_err());
}

/// Every shipped "Say only this" line has its exact words for the second
/// route's fallback (`onboarding::fixed_words`, lamp-52e6 28 Oct 16:42);
/// every line asking the model to ask, wait or call has none.
#[test]
fn every_shipped_say_only_line_has_fixed_words_and_no_other_line_does() {
    fn strings(value: &toml::Value, into: &mut Vec<String>) {
        match value {
            toml::Value::String(text) if text.starts_with('(') => into.push(text.clone()),
            toml::Value::Table(table) => table.values().for_each(|value| strings(value, into)),
            toml::Value::Array(items) => items.iter().for_each(|value| strings(value, into)),
            _ => {}
        }
    }
    let mut notes = Vec::new();
    strings(&toml::from_str::<toml::Value>(SHIPPED).unwrap(), &mut notes);
    let (mut fixed, mut led) = (0, 0);
    for note in &notes {
        let words = crate::onboarding::fixed_words(note);
        if note.starts_with("(Say only this, nothing before or after it: \"") {
            let words = words.unwrap_or_else(|| panic!("{note}"));
            assert!(!words.contains('"') && note.contains(words), "{note}");
            fixed += 1;
        } else {
            assert_eq!(words, None, "{note}");
            led += 1;
        }
    }
    assert!(fixed > 30 && led > 5, "{fixed} {led}");
    assert_eq!(
        crate::onboarding::fixed_words(
            "(Say only this, nothing before or after it: \"Hi.\" Then call setup_step.)"
        ),
        None
    );
}

/// The onboarding file as lamp-voice shipped it before #3769, which an
/// older updater's `install` still writes from its own built-in copy.
pub(crate) const BEFORE_3769: &str = include_str!("before_3769.toml");

/// Reviewer-two's P2 on #3781: an older updater's `install` writes its own
/// built-in onboarding file, without #3769's lines. The new voice still
/// loads it, and the shipped file with only those lines removed, so the
/// walk is never turned off by them.
#[test]
fn a_file_without_the_3769_lines_still_loads() {
    let removed: String = SHIPPED
        .lines()
        .filter(|line| {
            ![
                "touch_done = ",
                "touch_failed = ",
                "give_again = ",
                "give_felt = ",
                "give_prompt = ",
                "give_more = ",
                "near_missed = ",
                "skipped = ",
            ]
            .iter()
            .any(|field| line.starts_with(field))
        })
        .map(|line| format!("{line}\n"))
        .collect();
    for (what, file) in [
        ("lines removed", removed.as_str()),
        ("before #3769", BEFORE_3769),
    ] {
        assert_ne!(file, SHIPPED, "{what}");
        let definition = parse(file).unwrap_or_else(|why| panic!("{what}: {why}"));
        assert_eq!(definition.words.touch_done, None, "{what}");
        assert_eq!(definition.words.touch_failed, None, "{what}");
        assert_eq!(definition.words.give_again, None, "{what}");
        assert_eq!(definition.words.give_felt, None, "{what}");
        assert_eq!(definition.words.give_prompt, None, "{what}");
        assert_eq!(definition.words.give_more, None, "{what}");
        assert_eq!(definition.words.near_missed, None, "{what}");
    }
}

#[test]
fn both_consent_clauses_hold_for_first_and_reoffer_in_the_installed_file() {
    let folder = Folder::new();
    let edit = |text: &str, step: &str, mode: &str, from: &str, to: &str| {
        let header = format!("[onboarding.steps.{step}]\n");
        let (before, section) = text.split_once(&header).unwrap();
        let key = format!("{mode} = ");
        let (before_note, note_and_rest) = section.split_once(&key).unwrap();
        let (note, rest) = note_and_rest.split_once('\n').unwrap();
        assert!(note.contains(from));
        format!(
            "{before}{header}{before_note}{key}{}\n{rest}",
            note.replace(from, to)
        )
    };
    for (step, clause) in [
        (
            "who_you_are",
            "only after a clear yes call remember_my_face",
        ),
        ("room_sounds", "done only after a clear yes"),
        ("face_expressions", "done only after a clear yes"),
    ] {
        for mode in ["first", "reoffer"] {
            for replacement in ["", "a yes is not needed"] {
                let text = edit(SHIPPED, step, mode, clause, replacement);
                assert!(
                    parse(&text).is_err(),
                    "accepted {step}/{mode}/{replacement}"
                );
                folder.write(&text);
                assert!(read_installed(&folder.0).is_err());
            }
            let text = edit(
                SHIPPED,
                step,
                mode,
                "Nobody has spoken.",
                "Speak briefly. Nobody has spoken.",
            );
            assert!(parse(&text).is_ok());
            folder.write(&text);
            let loaded = read_installed(&folder.0).unwrap();
            assert!(
                loaded
                    .note(Step::named(step).unwrap(), mode == "reoffer")
                    .contains("Speak briefly. ")
            );
        }
    }
}

#[test]
fn installed_file_is_bounded_regular_and_has_no_fallback() {
    let folder = Folder::new();
    assert!(read_installed(&folder.0).is_err());
    folder.write("not valid TOML");
    assert!(read_installed(&folder.0).is_err());
    folder.write(&SHIPPED.replacen("kind = \"core\"", "kind = \"installable\"", 1));
    assert!(read_installed(&folder.0).is_err());
    folder.write(&format!("enabled = false\n{SHIPPED}"));
    assert!(read_installed(&folder.0).is_err());
    folder.write(SHIPPED);
    assert!(read_installed(&folder.0).is_ok());
    fs::OpenOptions::new()
        .write(true)
        .open(folder.file())
        .unwrap()
        .set_len(MOST_MANIFEST_BYTES + 1)
        .unwrap();
    assert!(read_installed(&folder.0).is_err());
    fs::remove_file(folder.file()).unwrap();
    fs::create_dir(folder.file()).unwrap();
    assert!(read_installed(&folder.0).is_err());
    fs::remove_dir(folder.file()).unwrap();
    let real = folder.0.join("real.toml");
    fs::write(&real, SHIPPED).unwrap();
    symlink(&real, folder.file()).unwrap();
    assert!(read_installed(&folder.0).is_err());
    fs::remove_file(folder.file()).unwrap();
    // rustix does not expose mkfifoat on Apple hosts; this POSIX fixture
    // exercises the same nonblocking regular-file check on both targets.
    assert!(
        std::process::Command::new("mkfifo")
            .arg(folder.file())
            .status()
            .unwrap()
            .success()
    );
    let began = Instant::now();
    assert!(read_installed(&folder.0).is_err());
    assert!(
        began.elapsed() < std::time::Duration::from_millis(100),
        "a FIFO must not wait for a writer"
    );
}

#[test]
fn template_values_are_filled_once_and_never_become_slots() {
    let definition = parse(SHIPPED).unwrap();
    assert_eq!(
        definition.privacy_invitation(false, "by {name}"),
        PRIVACY_INVITATION.replace("{location}", "by {name}")
    );
    assert_eq!(
        definition.who_confirm("{location}").unwrap(),
        WHO_CONFIRM.replace("{name}", "{location}")
    );
}

#[test]
fn an_expanded_name_confirmation_fits_one_note_without_cutting_a_name() {
    let definition = parse(SHIPPED).unwrap();
    let fixed = definition.words.who_confirm.len() - "{name}".len();
    let name = "x".repeat(lamp_gemini::wire::MAX_NOTE_BYTES - fixed);
    assert_eq!(
        definition.who_confirm(&name).unwrap().len(),
        lamp_gemini::wire::MAX_NOTE_BYTES
    );
    assert!(definition.who_confirm(&(name + "é")).is_none());
}

#[test]
fn installed_startup_and_cached_selection_have_no_turn_path_io() {
    let folder = Folder::new();
    folder.write(SHIPPED);
    let began = Instant::now();
    let definition = read_installed(&folder.0).unwrap();
    let startup = began.elapsed();
    fs::remove_file(folder.file()).unwrap();
    let began = Instant::now();
    for _ in 0..10_000 {
        std::hint::black_box(definition.note(Step::WhereYouSit, false));
        std::hint::black_box(definition.note(Step::Touch, true));
    }
    let cached = began.elapsed();
    assert_eq!(definition.note(Step::Touch, true), TOUCH_OFFER);
    eprintln!(
        "onboarding skill timing: startup_read_validate_us={} cached_selection_pair_mean_ns={} iterations=10000",
        startup.as_micros(),
        cached.as_nanos() / 10_000
    );
}

/// Every line the shipped skill gives a step: its notes and the `[words]`
/// keys named for it (#3769).
fn lines_of(step: Step) -> Vec<String> {
    let file: toml::Table = toml::from_str(SHIPPED).unwrap();
    let onboarding = file["onboarding"].as_table().unwrap();
    let mut lines = Vec::new();
    for (key, value) in onboarding["steps"][step.name()].as_table().unwrap() {
        match value {
            toml::Value::String(line) => lines.push(line.clone()),
            toml::Value::Table(nudge) if key == "nudge" => {
                lines.push(nudge["words"].as_str().unwrap().to_owned())
            }
            _ => {}
        }
    }
    let prefixes: &[&str] = match step {
        Step::WhereYouSit => &["sit_"],
        Step::WhoYouAre => &["who_"],
        Step::HowYouSound => &["near_", "sentence_"],
        Step::Touch => &["touch_"],
        Step::Privacy => &["privacy_"],
        Step::GivingWay => &["give_"],
        _ => &[],
    };
    for (key, value) in onboarding["words"].as_table().unwrap() {
        if prefixes.iter().any(|prefix| key.starts_with(prefix)) {
            lines.push(value.as_str().unwrap().to_owned());
        }
    }
    lines
}

#[test]
fn each_step_says_what_to_do_what_if_it_didnt_work_and_that_it_worked() {
    // (step, what to do, didn't work, worked), each in the step's own lines.
    // Giving way is parked (#4063) and left as it is.
    let three = [
        (
            Step::WhereYouSit,
            "Sit where you usually sit and look at me.",
            "I can't see your face yet.",
            "Got it, I can see you well there.",
        ),
        (
            Step::WhoYouAre,
            "What should I call you?",
            "Sorry, what should I call you?",
            "Nice to meet you.",
        ),
        (
            Step::HowYouSound,
            "say 'Hey Lamp' from your seat",
            "I didn't catch that. Say 'Hey Lamp' again, from your seat.",
            "Got it, that's three.",
        ),
        (
            Step::Touch,
            "Tap the top of my head once, then let go.",
            "I didn't feel that. Touch the very top of my head with your fingertip, once.",
            "Got it, I felt that.",
        ),
        (
            Step::Privacy,
            "Close it, then open it again.",
            "The switch check didn't finish.",
            "Got it, the switch closes my microphones.",
        ),
        (
            Step::RoomSounds,
            "May I notice music and laughter in the room?",
            "Sorry, was that a yes or a no?",
            "Got it, I'll notice music and laughter.",
        ),
        (
            Step::FaceExpressions,
            "May I notice when people look happy or upset",
            "Sorry, was that a yes or a no?",
            "Got it, I'll notice how people look.",
        ),
        (
            Step::Preferences,
            "Are there hours when I should keep quiet?",
            "Sorry, could you say that again?",
            "Got it, that's everything.",
        ),
    ];
    for (step, todo, again, worked) in three {
        let lines = lines_of(step);
        for line in [todo, again, worked] {
            assert!(
                lines.iter().any(|said| said.contains(line)),
                "{} lacks {line:?}",
                step.name()
            );
        }
    }
    // Touch says "head" only.
    assert!(
        lines_of(Step::Touch)
            .iter()
            .all(|line| !line.contains("pad"))
    );
}

#[test]
fn no_step_says_another_steps_words() {
    let marks = [
        (Step::WhereYouSit, "usually sit"),
        (Step::WhoYouAre, "should I call you"),
        (Step::HowYouSound, "Hey Lamp"),
        (Step::Touch, "top of my head"),
        (Step::Privacy, "privacy switch"),
        (Step::GivingWay, "tilt my head"),
        (Step::RoomSounds, "music and laughter"),
        (Step::FaceExpressions, "happy or upset"),
        (Step::Preferences, "keep quiet"),
    ];
    for (step, _) in marks {
        for line in lines_of(step) {
            for (other, mark) in marks {
                assert!(
                    other == step || !line.contains(mark),
                    "{} says {}'s {mark:?}: {line}",
                    step.name(),
                    other.name()
                );
            }
        }
    }
}
