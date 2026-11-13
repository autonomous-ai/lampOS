//! A skill the person made, removed by voice (#6960).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::make_skill_local::tests::keeping;
    use crate::skill_write_yes as yes;
    use serde_json::{Value, json};
    use std::path::{Path, PathBuf};
    use std::time::Instant;

    /// The person's made skills kept in `keeping`'s index, each by its name
    /// and whether it is on (an on one has its folder in the skills folder,
    /// as the manage helper put it there).
    fn made_here(keeping: &crate::made_skill::Keeping, made: &[(&str, bool)]) {
        std::fs::create_dir_all(&keeping.directory).unwrap();
        let entries: Vec<Value> = made
            .iter()
            .enumerate()
            .map(|(at, (name, on))| {
                let package = format!("{name}-1.0.0.tar");
                std::fs::write(keeping.directory.join(&package), "tar").unwrap();
                if *on {
                    installed(keeping, name, "installable");
                }
                json!({
                    "name": name,
                    "version": "1.0.0",
                    "sha256": format!("{at:x}").repeat(64),
                    "description": "",
                    "package": package,
                    "on": on,
                    "from": format!("local:{name}"),
                })
            })
            .collect();
        std::fs::write(
            keeping.directory.join(crate::made_skill::INDEX),
            serde_json::to_vec(&entries).unwrap(),
        )
        .unwrap();
    }

    /// A skill folder on the lamp named `name`, of `kind`.
    fn installed(keeping: &crate::made_skill::Keeping, name: &str, kind: &str) {
        let folder = keeping.installed.join(name);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("skill.toml"),
            format!("kind = \"{kind}\"\nversion = \"1.0.0\"\n"),
        )
        .unwrap();
    }

    fn names(directory: &Path) -> Vec<String> {
        crate::made_skill::entries(directory)
            .iter()
            .filter_map(|entry| entry["name"].as_str().map(str::to_owned))
            .collect()
    }

    /// Twelve's lamp: moon-fact made by voice and on, evening-dim beside
    /// it, faq (core) and look-for-things (from the Store) on the lamp.
    fn twelves_lamp(test: &str) -> (PathBuf, crate::made_skill::Keeping) {
        yes::clear_for_test();
        let (root, keeping) = keeping(test);
        made_here(&keeping, &[("moon-fact", true), ("evening-dim", true)]);
        installed(&keeping, "faq", "core");
        installed(&keeping, "look-for-things", "installable");
        (root, keeping)
    }

    fn ask(name: &str, request: u64) -> Ask<'_> {
        Ask {
            name,
            request,
            heard: true,
            sends_here: true,
        }
    }

    fn owner() -> bool {
        true
    }

    /// The id the remove's ask waits on.
    fn id_of(told: &Value) -> String {
        told[yes::APPROVE_ID]
            .as_str()
            .unwrap_or_else(|| panic!("no wait: {told}"))
            .to_owned()
    }

    /// Twelve's three lines on lamp-c0a4 (#6960), as the model passes the
    /// name it heard to `make_skill`'s `remove`: "moon fact", the
    /// speech-to-text's "moon-fax", and "the moon facts skill, the one you
    /// made for me". Each finds the person's own moon-fact and asks once,
    /// by its spoken name, "Remove moon fact?"; nothing is removed yet.
    #[test]
    fn twelves_three_lines_each_ask_remove_moon_fact() {
        for heard in ["moon fact", "moon-fax", "the moon facts skill"] {
            let (root, keeping) = twelves_lamp("remove-three-lines");
            let now = Instant::now();
            let told = asked(ask(heard, 1), &keeping, owner, now);
            assert_eq!(told["remove"], "moon-fact", "{heard}: {told}");
            let next = told["next"].as_str().unwrap();
            assert!(next.contains("\"Remove moon fact?\""), "{heard}: {next}");
            assert!(yes::waits(&id_of(&told)), "{heard}");
            assert!(yes::awaited(now));
            assert_eq!(names(&keeping.directory), ["moon-fact", "evening-dim"]);
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    /// `make_skill` is offered for a remove (line 1 went to `lamp_help`,
    /// lines 2 and 3 to a read-back: nothing in `make_skill` named one), as
    /// a parameter of the function the voice has, never a new function
    /// (the voice is at its 48), and within the link's bounds with every
    /// switch on.
    #[test]
    fn make_skill_is_offered_for_a_remove_within_the_bounds() {
        crate::made_skill::set_writes_here(Some(true));
        crate::made_skill::set_scripts_here(Some(true));
        crate::skill_task_yes::set_on_here(Some(true));
        for tool in [crate::make_skill_local::tool()] {
            assert!(tool.description.contains("remove"), "{}", tool.description);
            assert!(tool.description.contains("delete"), "{}", tool.description);
            assert!(tool.description.len() <= 1_024, "{}", tool.description.len());
            assert!(!tool.description.contains("yes"), "{}", tool.description);
            let parameters = tool.parameters.unwrap();
            assert!(parameters["properties"][REMOVE].is_object(), "{parameters}");
            assert!(parameters.to_string().len() <= 4_096);
        }
        // Every switch off: still offered, as `make_skill` is.
        crate::skill_task_yes::set_on_here(Some(false));
        crate::made_skill::set_scripts_here(Some(false));
        crate::made_skill::set_writes_here(Some(false));
        let off = crate::make_skill_local::tool();
        assert!(off.description.contains("remove"), "{}", off.description);
        assert!(off.parameters.unwrap()["properties"][REMOVE].is_object());
        crate::skill_task_yes::set_on_here(None);
        crate::made_skill::set_scripts_here(None);
        crate::made_skill::set_writes_here(None);
    }

    /// The question said whole, the owner's yes in a later line removes
    /// moon-fact through the manage helper's own remove and drops it from
    /// the made skills; evening-dim stays.
    #[test]
    fn the_yes_removes_it_through_the_helper() {
        let (root, keeping) = twelves_lamp("remove-yes");
        let now = Instant::now();
        let id = id_of(&asked(ask("moon-fax", 1), &keeping, owner, now));
        yes::lamp_said(1, "Remove moon fact?");
        assert!(yes::approve_forced_now(now), "the next line must answer it");
        assert_eq!(yes::answered(&id, 2, Some("yes")), None);
        assert_eq!(
            crate::make_skill_local::not_owners_yes_refused(&id, || true),
            None
        );
        let mut helper = Vec::new();
        let told = yes::approve(&id, 2, true, true, now, |asks| {
            removed(&keeping, asks, |name| {
                helper.push(name.to_owned());
                Ok(())
            })
        });
        assert_eq!(told["approved"], true, "{told}");
        assert_eq!(told["removed"], "moon-fact", "{told}");
        assert!(told["next"].as_str().unwrap().contains("moon fact"), "{told}");
        assert_eq!(helper, ["moon-fact"]);
        assert_eq!(names(&keeping.directory), ["evening-dim"]);
        assert!(!yes::awaited(now));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A no keeps it, and the wait ends; an unclear answer asks once more by
    /// its name, and a second unclear keeps it too. Nothing is asked of the
    /// helper either way.
    #[test]
    fn a_no_or_twice_unclear_keeps_it() {
        let (root, keeping) = twelves_lamp("remove-no");
        let now = Instant::now();
        let id = id_of(&asked(ask("moon fact", 1), &keeping, owner, now));
        yes::lamp_said(1, "Remove moon fact?");
        let told = yes::answered(&id, 2, Some("no")).unwrap();
        assert_eq!(told["approved"], false);
        let next = told["next"].as_str().unwrap();
        assert!(next.contains("moon fact stays"), "{next}");
        assert!(!yes::awaited(now));
        assert_eq!(names(&keeping.directory), ["moon-fact", "evening-dim"]);

        let id = id_of(&asked(ask("moon fact", 3), &keeping, owner, now));
        yes::lamp_said(3, "Remove moon fact?");
        let once = yes::answered(&id, 4, Some("unclear")).unwrap();
        assert!(
            once["next"].as_str().unwrap().contains("remove moon fact"),
            "{once}"
        );
        assert!(yes::awaited(now));
        let again = yes::answered(&id, 5, None).unwrap();
        assert!(again["next"].as_str().unwrap().contains("moon fact stays"));
        assert!(!yes::awaited(now));
        assert_eq!(names(&keeping.directory), ["moon-fact", "evening-dim"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A visitor's yes removes nothing: the wait is dropped, as every made
    /// skill's yes is the owner's only.
    #[test]
    fn a_visitors_yes_removes_nothing() {
        let (root, keeping) = twelves_lamp("remove-visitor");
        let now = Instant::now();
        let id = id_of(&asked(ask("moon fact", 1), &keeping, owner, now));
        yes::lamp_said(1, "Remove moon fact?");
        let refused = crate::make_skill_local::not_owners_yes_refused(&id, || false).unwrap();
        assert!(
            refused["error"].as_str().unwrap().contains("Not removed"),
            "{refused}"
        );
        assert!(!yes::awaited(now));
        assert_eq!(names(&keeping.directory), ["moon-fact", "evening-dim"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A yes before the question was heard whole, or in its own line,
    /// removes nothing.
    #[test]
    fn a_yes_before_the_question_was_said_removes_nothing() {
        let (root, keeping) = twelves_lamp("remove-unsaid");
        let now = Instant::now();
        let id = id_of(&asked(ask("moon fact", 1), &keeping, owner, now));
        let never = |_: &yes::Asks| -> Result<Value, String> { panic!("removed") };
        let told = yes::approve(&id, 2, true, true, now, never);
        assert!(told["error"].is_string(), "{told}");
        assert!(told["next"].as_str().unwrap().contains("Remove moon fact?"));
        assert!(yes::waits(&id));
        yes::lamp_said(1, "Remove moon fact?");
        let told = yes::approve(&id, 1, true, true, now, never);
        assert!(told["error"].is_string(), "{told}");
        assert_eq!(names(&keeping.directory), ["moon-fact", "evening-dim"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A core skill's name, or a Store skill's, is refused in words: it
    /// isn't one the person made, so it is never removed by voice, and
    /// nothing waits.
    #[test]
    fn a_core_or_store_skill_is_refused_in_words() {
        let (root, keeping) = twelves_lamp("remove-core");
        let now = Instant::now();
        for (heard, spoken) in [("faq", "faq"), ("look for things", "look for things")] {
            let told = asked(ask(heard, 1), &keeping, owner, now);
            assert_eq!(told["error"], NOT_MADE, "{heard}: {told}");
            let next = told["next"].as_str().unwrap();
            assert!(next.contains(&format!("{spoken} isn't a skill they made")), "{next}");
            assert!(told.get(yes::APPROVE_ID).is_none(), "{told}");
            assert!(!yes::awaited(now));
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// No skill they made near the name: said so, naming theirs (two here)
    /// to the owner only; a near name is named to the owner only too, and
    /// the exact one asks without asking whose line it was.
    #[test]
    fn none_near_is_said_and_names_are_told_the_owner_only() {
        let (root, keeping) = twelves_lamp("remove-none");
        let now = Instant::now();
        let told = asked(ask("weather report", 1), &keeping, owner, now);
        assert_eq!(told["error"], NO_MADE, "{told}");
        let next = told["next"].as_str().unwrap();
        assert!(next.contains("moon fact") && next.contains("evening dim"), "{next}");
        assert!(!yes::awaited(now));
        for heard in ["weather report", "moon-fax"] {
            let told = asked(ask(heard, 1), &keeping, || false, now);
            assert_eq!(told["error"], NO_MADE, "{heard}: {told}");
            let next = told["next"].as_str().unwrap();
            assert!(!next.contains("moon fact"), "{heard}: {next}");
            assert!(!yes::awaited(now));
        }
        let told = asked(ask("moon fact", 1), &keeping, || panic!("asked whose"), now);
        assert_eq!(told["remove"], "moon-fact", "{told}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Two of theirs as near: asked once which, short, and nothing waits.
    #[test]
    fn two_as_near_ask_which() {
        yes::clear_for_test();
        let (root, keeping) = keeping("remove-alike");
        made_here(&keeping, &[("moon-fact", true), ("noon-fact", false)]);
        let now = Instant::now();
        let told = asked(ask("loon fact", 1), &keeping, owner, now);
        let next = told["next"].as_str().unwrap();
        assert!(next.contains("moon fact or noon fact"), "{next}");
        assert!(!yes::awaited(now));
        // One made and off is removed from the made skills alone: the
        // helper has nothing to take off.
        let id = id_of(&asked(ask("noon fact", 2), &keeping, owner, now));
        yes::lamp_said(2, "Remove noon fact?");
        let told = yes::approve(&id, 3, true, true, now, |asks| {
            removed(&keeping, asks, |_| panic!("asked the helper"))
        });
        assert_eq!(told["removed"], "noon-fact", "{told}");
        assert_eq!(names(&keeping.directory), ["moon-fact"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Unheard, or on the Live route (no echo judge to take the answer),
    /// nothing is asked and nothing waits.
    #[test]
    fn nothing_is_asked_unheard_or_on_the_live_route() {
        let (root, keeping) = twelves_lamp("remove-live");
        let now = Instant::now();
        let mut unheard = ask("moon fact", 1);
        unheard.heard = false;
        assert!(asked(unheard, &keeping, owner, now)["error"].is_string());
        let mut live = ask("moon fact", 1);
        live.sends_here = false;
        let told = asked(live, &keeping, owner, now);
        assert!(told["error"].is_string(), "{told}");
        assert!(told["next"].as_str().unwrap().contains("other voice"), "{told}");
        assert!(!yes::awaited(now));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The nearest name is a spelling distance over the name's own words,
    /// as a skill's name is written, never a list of words.
    #[test]
    fn nearest_is_spelling_over_the_names_words() {
        assert_eq!(apart("moon fact", "moon-fact"), 0);
        assert_eq!(apart("My Moon Fact skill", "moon-fact"), 0);
        assert_eq!(apart("the moon fact", "moon-fact"), 0);
        assert_eq!(apart("moon-fax", "moon-fact"), 2);
        assert_eq!(apart("the moon facts skill", "moon-fact"), 1);
        assert_eq!(apart("lamp test email two", "lamp-test-email-2"), 0);
        assert!(near(2, "moon-fact"));
        assert!(!near(3, "moon-fact"));
        assert!(!near(1, "dim"));
    }
}
