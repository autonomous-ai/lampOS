//! Interrupt the real deletion traversal, then run the reset's resume path.

use super::*;
use std::{
    os::unix::fs::symlink,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    root: PathBuf,
    skills: PathBuf,
    target: PathBuf,
    private: PathBuf,
    plan: Plan,
}

impl Fixture {
    fn new(name: &str, nested: bool, marked: bool) -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "lamp-reset-skill-cut-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let skills = root.join("skills");
        let target = skills.join(name);
        let private = if nested {
            target.join("desk-tutor")
        } else {
            target.clone()
        };
        fs::create_dir_all(private.join("notes/deeper")).unwrap();
        if marked {
            fs::write(private.join(INSTALLED_FROM), b"{}").unwrap();
        }
        fs::write(private.join("SKILL.md"), b"the previous owner's words").unwrap();
        fs::write(private.join("notes/deeper/private.txt"), b"private notes").unwrap();
        fs::create_dir_all(skills.join("onboarding")).unwrap();
        fs::write(skills.join("onboarding/SKILL.md"), b"shipped core").unwrap();
        fs::create_dir_all(root.join("outside")).unwrap();
        fs::write(root.join("outside/keep"), b"outside").unwrap();
        symlink(root.join("outside"), private.join("linked")).unwrap();
        symlink(root.join("outside"), skills.join(".install-linked-42")).unwrap();
        fs::create_dir_all(skills.join(".install-not-a-pid")).unwrap();
        fs::write(skills.join(".install-not-a-pid/keep"), b"unrelated").unwrap();
        let plan = Plan {
            marker: root.join("done"),
            journal: root.join("reset.json"),
            wipe: vec![root.join("person-data")],
            stop: vec!["/bin/false".into()],
            reboot: vec!["/bin/false".into()],
            detach: vec![],
        };
        fs::write(&plan.journal, b"{}").unwrap();
        Self {
            root,
            skills,
            target,
            private,
            plan,
        }
    }

    fn assert_kept(&self) {
        assert_eq!(
            fs::read(self.skills.join("onboarding/SKILL.md")).unwrap(),
            b"shipped core"
        );
        assert_eq!(
            fs::read(self.root.join("outside/keep")).unwrap(),
            b"outside"
        );
        assert!(
            fs::symlink_metadata(self.skills.join(".install-linked-42"))
                .unwrap()
                .is_symlink()
        );
        assert_eq!(
            fs::read(self.skills.join(".install-not-a-pid/keep")).unwrap(),
            b"unrelated"
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

struct Removal<'a> {
    fixture: &'a Fixture,
    fail_at: Option<usize>,
    calls: usize,
}

impl Effects for Removal<'_> {
    fn run(&mut self, _: &[String]) -> io::Result<()> {
        panic!("resume runs no command")
    }
    fn read(&mut self, _: &[String]) -> io::Result<String> {
        panic!("resume reads no command")
    }
    fn sync(&mut self) {}
    fn force_reboot(&mut self) {
        panic!("resume does not reboot")
    }
    fn clear_volatile(&mut self) -> io::Result<()> {
        panic!("resume does not start parts")
    }
    fn remove_installed_skills(&mut self) -> io::Result<usize> {
        remove_installed_skills_with(&self.fixture.skills, &mut || {
            self.calls += 1;
            if self.fail_at == Some(self.calls) {
                return Err(io::Error::other("injected interruption"));
            }
            Ok(())
        })
        .map(|removed| removed.len())
    }
}

fn interrupt_each_removal(name: &str, nested: bool, marked: bool) {
    let full = Fixture::new(name, nested, marked);
    let mut effects = Removal {
        fixture: &full,
        fail_at: None,
        calls: 0,
    };
    resume(&full.plan, &mut effects).unwrap();
    let boundaries = effects.calls;
    assert!(
        boundaries >= 7,
        "nested removals and the final folder are covered"
    );
    assert!(!full.target.exists());
    full.assert_kept();

    for cut in 1..=boundaries {
        let fixture = Fixture::new(name, nested, marked);
        let mut effects = Removal {
            fixture: &fixture,
            fail_at: Some(cut),
            calls: 0,
        };
        assert!(resume(&fixture.plan, &mut effects).is_err(), "cut {cut}");
        assert!(
            fixture.plan.journal.exists(),
            "failed removal keeps the reset pending"
        );
        effects.fail_at = None;
        resume(&fixture.plan, &mut effects).unwrap();
        assert!(!fixture.plan.journal.exists());
        assert!(!fixture.private.join("SKILL.md").exists());
        assert!(!fixture.private.join("notes").exists());
        assert!(
            !fixture.target.exists(),
            "the person's folder name goes too"
        );
        assert!(!fixture.skills.join(SKILL_PENDING).exists());
        fixture.assert_kept();
    }
}

#[test]
fn a_cut_at_each_skill_removal_keeps_the_reset_pending_until_private_files_are_gone() {
    interrupt_each_removal("desk-tutor", false, true);
}

#[test]
fn a_killed_install_or_remove_is_erased_even_before_its_package_mark_exists() {
    for marked in [false, true] {
        interrupt_each_removal(".install-desk-tutor-4242", true, marked);
        interrupt_each_removal(".removed-desk-tutor-4242", false, marked);
    }
}

#[test]
fn a_pending_reset_without_a_package_mark_is_finished_even_if_interrupted_again() {
    interrupt_each_removal(SKILL_PENDING, false, false);
}

#[test]
fn pending_cleanup_precedes_the_next_skill_and_refuses_a_link_in_its_place() {
    let fixture = Fixture::new(SKILL_PENDING, false, false);
    let other = fixture.skills.join("another-private-name");
    fs::create_dir(&other).unwrap();
    fs::write(other.join(INSTALLED_FROM), b"{}").unwrap();
    fs::write(other.join("SKILL.md"), b"another person's words").unwrap();
    assert_eq!(
        remove_installed_skills(&fixture.skills).unwrap(),
        [fixture.target.clone(), other.clone()]
    );
    assert!(!other.exists() && !fixture.target.exists());
    fixture.assert_kept();

    symlink(fixture.root.join("outside"), &fixture.target).unwrap();
    let mut effects = Removal {
        fixture: &fixture,
        fail_at: None,
        calls: 0,
    };
    assert!(resume(&fixture.plan, &mut effects).is_err());
    assert!(fixture.plan.journal.exists());
    fixture.assert_kept();
}
