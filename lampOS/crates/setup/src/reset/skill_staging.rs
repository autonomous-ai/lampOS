//! The folders a killed skills install or remove leaves behind, including
//! an install cut before its package mark is written. Keep this classifier
//! equal to `lamp_skills::package::left_by_a_run` without making skills a
//! runtime dependency of setup.

/// Only `.install-NAME-PID` and `.removed-NAME-PID`, with the same name and
/// PID bounds as the skills package writer.
pub(super) fn left_by_a_run(name: &str) -> bool {
    [".install-", ".removed-"].iter().any(|what| {
        name.strip_prefix(what)
            .and_then(|rest| rest.rsplit_once('-'))
            .is_some_and(|(skill, pid)| {
                skill_name_ok(skill)
                    && (1..=10).contains(&pid.len())
                    && pid.bytes().all(|byte| byte.is_ascii_digit())
            })
    })
}

/// `lamp_skills::package::skill_name_ok`: 1 to 64 lowercase ASCII letters,
/// digits and hyphens. Its public counterpart is pinned by the test below.
fn skill_name_ok(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The skills package classifier's own examples, plus its length bounds.
    #[test]
    fn only_a_runs_own_folder_names_are_taken_for_what_it_left() {
        for name in [
            ".install-desk-tutor-4242",
            ".removed-desk-tutor-1",
            ".install-a-0",
        ] {
            assert!(left_by_a_run(name), "{name}");
        }
        for name in [
            "desk-tutor",
            "install-desk-tutor-4242",
            ".install-desk-tutor",
            ".install-desk-tutor-",
            ".install--4242",
            ".install-Desk-4242",
            ".install-desk_tutor-4242",
            ".install-desk-tutor-42a",
            ".install-desk-tutor-12345678901",
            ".installer-desk-tutor-4242",
            ".removed-../x-1",
            ".lamp-skills.lock",
        ] {
            assert!(!left_by_a_run(name), "{name}");
        }
        for prefix in [".install-", ".removed-"] {
            assert!(left_by_a_run(&format!(
                "{prefix}{}-1234567890",
                "a".repeat(64)
            )));
            assert!(!left_by_a_run(&format!("{prefix}{}-1", "a".repeat(65))));
        }
    }

    #[test]
    fn skill_names_match_the_package_writers_rules() {
        for name in [
            "",
            "a",
            "desk-tutor",
            "0",
            "-",
            "a-",
            "-a",
            "a_b",
            "A",
            "a.b",
            "a/b",
            "é",
        ] {
            assert_eq!(
                skill_name_ok(name),
                lamp_skills::package::skill_name_ok(name),
                "{name}"
            );
        }
        for length in [63, 64, 65] {
            let name = "a".repeat(length);
            assert_eq!(
                skill_name_ok(&name),
                lamp_skills::package::skill_name_ok(&name)
            );
        }
    }
}
