//! An app pushed from the owner's own paired computer (docs/apps.md, "An app
//! from your own computer"): asked by lamp-control alone, with no owner's yes,
//! marked with the computer, replaced by the same computer's push, never by
//! the Store, and removed by that computer.
use lamp_apps::ask::{Parts, answer, answer_door};
use lamp_apps::install::{OTHER_COMPUTERS, PUSHED_HERE, STORE_HAS_IT, installed_record};
use lamp_apps::layout::Layout;
use lamp_apps::manage::NOT_PUSHED_BY;
use lamp_apps::package::{AppPackage, AppSpec, sha256_hex};
use lamp_apps::peer::{CLOUD, CONTROL, VOICE, door_ok, is_an_asker, is_the_door};
use lamp_apps::testing::{FakeCompanions, FakeEffects, FakeYes, StubReader};
use lamp_apps::yes::{Op, YesKey};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt;

fn package(version: &str) -> AppPackage {
    AppPackage {
        app: AppSpec {
            name: "gym-coach".into(),
            title: "Gym Coach".into(),
            version: version.into(),
            min_build: 1,
            net: false,
            logs: false,
            grants: vec!["speak".into(), "hear".into()],
            entry: "app.main".into(),
        },
        files: BTreeMap::from([
            ("src/app/main.py".to_owned(), b"print('hi')\n".to_vec()),
            ("lamp-skill.json".to_owned(), b"{}".to_vec()),
        ]),
        skills: vec![],
    }
}

struct Rig {
    _root: tempfile::TempDir,
    layout: Layout,
    fx: FakeEffects,
    skills: FakeCompanions,
    owner: u32,
}

fn rig() -> Rig {
    let root = tempfile::tempdir().unwrap();
    let layout = Layout::under(root.path());
    std::fs::create_dir_all(&layout.staging).unwrap();
    let owner = std::fs::metadata(&layout.staging).unwrap().uid();
    Rig {
        fx: FakeEffects::cleaning(&layout),
        layout,
        _root: root,
        skills: FakeCompanions::default(),
        owner,
    }
}

/// A staged package of `tag`'s bytes: the path lamp-control names.
fn staged(r: &Rig, tag: &str) -> String {
    let path = r.layout.staging.join(format!("pushed-{tag}.tar"));
    std::fs::write(&path, tag.as_bytes()).unwrap();
    path.display().to_string()
}

fn door(r: &mut Rig, reader: &StubReader, line: &Value) -> Value {
    let mut yes = FakeYes::granting(vec![]);
    let mut parts = Parts {
        layout: &r.layout,
        fx: &mut r.fx,
        skills: &mut r.skills,
        yes: &mut yes,
        reader,
        staged_owner: r.owner,
        build: Some(200),
    };
    let said = answer_door(true, &line.to_string(), &mut parts);
    assert_eq!(yes.asked, 0, "a push never takes an owner's yes");
    said
}

fn push(r: &mut Rig, version: &str, client: &str, tag: &str) -> Value {
    let path = staged(r, tag);
    door(
        r,
        &StubReader::new(package(version)),
        &json!({"install_pushed": {"path": path, "client": client, "client_name": "desk mac"}}),
    )
}

#[test]
fn only_lamp_control_asks_the_doors_verbs() {
    let cg = |path: &str| format!("0::{path}\n");
    assert!(is_the_door(&cg(CONTROL)));
    assert!(door_ok(0, 1000, &cg(CONTROL)) && door_ok(1000, 1000, &cg(CONTROL)));
    assert!(
        !door_ok(61_234, 1000, &cg(CONTROL)),
        "a dynamic user is no door"
    );
    for other in [
        CLOUD,
        VOICE,
        "/lampos.slice/lampos-apps.slice/lamp-app@a.service",
    ] {
        assert!(!is_the_door(&cg(other)), "{other}");
    }
    // lamp-control stays no asker of the Store's verbs.
    assert!(!is_an_asker(&cg(CONTROL)));
}

#[test]
fn a_paired_computers_push_installs_and_runs_with_no_yes_marked_with_the_computer() {
    let mut r = rig();
    let said = push(&mut r, "0.1.0", "c_1", "one");
    assert_eq!(
        said,
        json!({"ok": {"name": "gym-coach", "version": "0.1.0", "updated": false, "started": true}})
    );
    assert!(
        r.fx.calls
            .iter()
            .any(|call| call.contains("restart") && call.contains("lamp-app@gym-coach.service")),
        "{:?}",
        r.fx.calls
    );
    let record = installed_record(&r.layout, "gym-coach").unwrap();
    let pushed = record.pushed.unwrap();
    assert_eq!(pushed.client, "c_1");
    assert_eq!(pushed.client_name.as_deref(), Some("desk mac"));
    assert_eq!(record.sha256, sha256_hex(b"one"));
    // Listed for that computer, and for nobody else.
    let reader = StubReader::new(package("0.1.0"));
    let listed = door(&mut r, &reader, &json!({"list_pushed_by": "c_1"}));
    assert_eq!(listed["ok"]["apps"][0]["name"], "gym-coach");
    assert_eq!(listed["ok"]["apps"][0]["pushed"]["client"], "c_1");
    let other = door(&mut r, &reader, &json!({"list_pushed_by": "c_2"}));
    assert_eq!(other, json!({"ok": {"apps": []}}));
}

#[test]
fn a_re_push_replaces_any_version_and_another_computer_or_the_store_is_refused() {
    let mut r = rig();
    push(&mut r, "0.2.0", "c_1", "one");
    // The same version again, and an older one: each replaces.
    let again = push(&mut r, "0.2.0", "c_1", "two");
    assert_eq!(again["ok"]["updated"], true, "{again}");
    assert_eq!(
        installed_record(&r.layout, "gym-coach").unwrap().sha256,
        sha256_hex(b"two")
    );
    let older = push(&mut r, "0.1.0", "c_1", "three");
    assert_eq!(older["ok"]["version"], "0.1.0", "{older}");
    // Another computer's push of the name.
    assert_eq!(
        push(&mut r, "0.3.0", "c_2", "four"),
        json!({"refused": OTHER_COMPUTERS})
    );
    // A Store install over it, even a newer and rightly yes'd one, is refused.
    let path = staged(&r, "store");
    let newer = package("9.0.0");
    let mut yes = FakeYes::granting(vec![YesKey {
        op: Op::Update,
        name: "gym-coach".into(),
        version: "9.0.0".into(),
        sha256: sha256_hex(b"store"),
        grants: vec![],
        net: false,
        logs: false,
    }]);
    let reader = StubReader::new(newer);
    let mut parts = Parts {
        layout: &r.layout,
        fx: &mut r.fx,
        skills: &mut r.skills,
        yes: &mut yes,
        reader: &reader,
        staged_owner: r.owner,
        build: Some(200),
    };
    let store = answer(
        true,
        &json!({"install": {"path": path}}).to_string(),
        &mut parts,
    );
    assert_eq!(store, json!({"refused": PUSHED_HERE}));
    assert_eq!(yes.asked, 0, "refused before the yes is spent");
    assert_eq!(
        installed_record(&r.layout, "gym-coach").unwrap().version,
        "0.1.0"
    );
}

#[test]
fn a_push_never_takes_a_store_apps_name() {
    let mut r = rig();
    let path = staged(&r, "store");
    let mut yes = FakeYes::granting(vec![YesKey {
        op: Op::Install,
        name: "gym-coach".into(),
        version: "1.0.0".into(),
        sha256: sha256_hex(b"store"),
        grants: vec!["hear".into(), "speak".into()],
        net: false,
        logs: false,
    }]);
    let reader = StubReader::new(package("1.0.0"));
    let mut parts = Parts {
        layout: &r.layout,
        fx: &mut r.fx,
        skills: &mut r.skills,
        yes: &mut yes,
        reader: &reader,
        staged_owner: r.owner,
        build: Some(200),
    };
    let store = answer(
        true,
        &json!({"install": {"path": path}}).to_string(),
        &mut parts,
    );
    assert_eq!(store, json!({"ok": "installed"}));
    assert_eq!(
        push(&mut r, "2.0.0", "c_1", "pushed"),
        json!({"refused": STORE_HAS_IT})
    );
    // The door can't remove a Store app either.
    let reader = StubReader::new(package("1.0.0"));
    assert_eq!(
        door(
            &mut r,
            &reader,
            &json!({"remove_pushed": {"client": "c_1", "name": "gym-coach"}})
        ),
        json!({"refused": NOT_PUSHED_BY})
    );
}

#[test]
fn a_remove_by_its_computer_removes_and_another_computers_is_refused() {
    let mut r = rig();
    push(&mut r, "0.1.0", "c_1", "one");
    let reader = StubReader::new(package("0.1.0"));
    assert_eq!(
        door(
            &mut r,
            &reader,
            &json!({"remove_pushed": {"client": "c_2", "name": "gym-coach"}})
        ),
        json!({"refused": NOT_PUSHED_BY})
    );
    assert!(installed_record(&r.layout, "gym-coach").is_some());
    assert_eq!(
        door(
            &mut r,
            &reader,
            &json!({"remove_pushed": {"client": "c_1", "name": "gym-coach"}})
        ),
        json!({"ok": "removed"})
    );
    assert!(installed_record(&r.layout, "gym-coach").is_none());
    assert!(!r.layout.code.join("gym-coach").exists());
    assert!(r.fx.calls.iter().any(|call| call == "app_forget gym-coach"));
}

#[test]
fn the_door_and_the_owners_verbs_are_never_crossed_and_off_refuses_all() {
    let mut r = rig();
    let reader = StubReader::new(package("0.1.0"));
    // The Store's verbs from lamp-control, and the door's from lamp-cloud.
    for line in [json!({"install": {"path": "/x"}}), json!({"list": {}})] {
        assert!(
            door(&mut r, &reader, &line)["refused"].is_string(),
            "{line}"
        );
    }
    let mut yes = FakeYes::granting(vec![]);
    let mut parts = Parts {
        layout: &r.layout,
        fx: &mut r.fx,
        skills: &mut r.skills,
        yes: &mut yes,
        reader: &reader,
        staged_owner: r.owner,
        build: Some(200),
    };
    for line in [
        json!({"install_pushed": {"path": "/x", "client": "c_1"}}),
        json!({"list_pushed_by": "c_1"}),
        json!({"remove_pushed": {"client": "c_1", "name": "a"}}),
    ] {
        assert!(
            answer(true, &line.to_string(), &mut parts)["refused"].is_string(),
            "{line}"
        );
    }
    assert_eq!(
        answer_door(
            false,
            &json!({"list_pushed_by": "c_1"}).to_string(),
            &mut parts
        ),
        json!({"refused": lamp_apps::ask::OFF})
    );
}
