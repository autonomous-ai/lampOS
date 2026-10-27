use super::*;
use std::{
    os::unix::fs::{PermissionsExt, symlink},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Asked {
    Text(PathBuf),
    Entries(PathBuf),
}

#[derive(Default)]
struct Answers {
    texts: BTreeMap<PathBuf, Option<String>>,
    directories: BTreeMap<PathBuf, Option<Vec<PathBuf>>>,
}

#[derive(Default)]
struct Recording {
    answers: Answers,
    asked: Vec<Asked>,
}

impl HeadInputs for Recording {
    fn text(&mut self, path: &Path) -> Result<Option<String>, String> {
        self.asked.push(Asked::Text(path.into()));
        let answer = OrdinaryInputs.text(path)?;
        if let Some(previous) = self.answers.texts.insert(path.into(), answer.clone()) {
            assert_eq!(previous, answer, "a fixture changed during recording");
        }
        Ok(answer)
    }

    fn entries(&mut self, directory: &Path) -> Result<Option<ConfEntries>, String> {
        self.asked.push(Asked::Entries(directory.into()));
        // Only this fixture recorder eagerly captures membership. The ordinary
        // implementation remains lazy, tested separately below.
        let answer = OrdinaryInputs
            .entries(directory)?
            .map(|entries| entries.collect::<Result<Vec<_>, _>>())
            .transpose()?;
        if let Some(previous) = self
            .answers
            .directories
            .insert(directory.into(), answer.clone())
        {
            assert_eq!(
                previous, answer,
                "fixture membership changed during recording"
            );
        }
        Ok(answer.map(|paths| Box::new(paths.into_iter().map(Ok)) as ConfEntries))
    }
}

struct Replay {
    answers: Answers,
    asked: Vec<Asked>,
}

impl HeadInputs for Replay {
    fn text(&mut self, path: &Path) -> Result<Option<String>, String> {
        self.asked.push(Asked::Text(path.into()));
        self.answers
            .texts
            .get(path)
            .cloned()
            .ok_or_else(|| "uncaptured text input".into())
    }

    fn entries(&mut self, directory: &Path) -> Result<Option<ConfEntries>, String> {
        self.asked.push(Asked::Entries(directory.into()));
        self.answers
            .directories
            .get(directory)
            .cloned()
            .ok_or_else(|| "uncaptured directory input".to_owned())
            .map(|answer| answer.map(|paths| Box::new(paths.into_iter().map(Ok)) as ConfEntries))
    }
}

fn put(root: &Path, relative: &str, text: &str) {
    let path = root.join(relative.trim_start_matches('/'));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn fixture(head: bool) -> (tempfile::TempDir, BTreeMap<String, String>, Setting) {
    let root = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let unit = |part: &str, args: &str, environment: &str| {
        format!("[Service]\nExecStart=/opt/lampOS/{part}/current/lamp-{part} {args}\n{environment}")
    };
    let carried = BTreeMap::from([
        (
            VOICE.into(),
            unit(
                "voice",
                "--controls-mute /run/lampos-bus/mute",
                &format!(
                    "Environment=LAMP_HEAD_TOUCH={}\n",
                    if head { "on" } else { "off" }
                ),
            ),
        ),
        (
            EXPRESSION.into(),
            unit("expression", if head { "--head-touch" } else { "" }, ""),
        ),
        (
            CONTROLS.into(),
            unit("controls", &format!("--pins {PINS}"), ""),
        ),
        (
            BODY.into(),
            unit(
                "body",
                "%H $BODY_POSES",
                "Environment=LAMP_HEAD_TOUCH=on\nEnvironmentFile=/etc/lampOS/body/lamp.env\nEnvironmentFile=-/var/lib/lampos-body/measured.env\n",
            ),
        ),
    ]);
    for (name, text) in &carried {
        put(root.path(), &format!("etc/systemd/system/{name}"), text);
    }
    put(root.path(), BODY_ENV, "BODY_POSES=\"1,2,3\"\n");
    fs::create_dir(root.path().join("etc/systemd/system/service.d")).unwrap();
    let setting = Setting::new(
        PINS,
        r#"{"head_pads":[{"name":"head-1","line":41,"active_low":true,"pull":"up"}],"legacy_head":false}"#,
    );
    put(root.path(), PINS, setting.text.as_deref().unwrap());
    (root, carried, setting)
}

#[test]
fn projection_and_verification_replay_closed_inputs_without_filesystem_fallback() {
    for head in [false, true] {
        for omitted in [false, true] {
            for use_carried in [false, true] {
                let (root, all_carried, setting) = fixture(head);
                let root_path = root.path().to_owned();
                let units = root_path.join("etc/systemd/system");
                let carried = if use_carried {
                    all_carried
                } else {
                    BTreeMap::new()
                };
                let settings = [setting];
                let initial = if omitted {
                    Vec::new()
                } else {
                    vec![Step {
                        setting: settings[0].clone(),
                        act: Act::Write,
                    }]
                };
                let mut expected = initial.clone();
                project(&root_path, &units, &carried, &settings, &mut expected).unwrap();
                verify(&root_path, &units, &carried, &expected).unwrap();
                let mut recording = Recording::default();
                let mut recorded = initial.clone();
                project_with(
                    &mut recording,
                    &root_path,
                    &units,
                    &carried,
                    &settings,
                    &mut recorded,
                )
                .unwrap();
                verify_with(&mut recording, &root_path, &units, &carried, &recorded).unwrap();
                assert_eq!(recorded, expected);
                assert_eq!(
                    recording.answers.directories.len(),
                    if head { 30 } else { 25 }
                );
                assert_eq!(
                    recording
                        .answers
                        .texts
                        .contains_key(&root_path.join(BODY_ENV.trim_start_matches('/'))),
                    head
                );
                assert_eq!(
                    recording.answers.texts.contains_key(&units.join(BODY)),
                    head && !use_carried
                );
                assert_eq!(
                    recording
                        .answers
                        .directories
                        .get(&root_path.join("etc/systemd/system/service.d")),
                    Some(&Some(Vec::new()))
                );
                assert_eq!(
                    recording
                        .answers
                        .directories
                        .get(&root_path.join("run/systemd/system/service.d")),
                    Some(&None)
                );
                let text = expected
                    .first()
                    .and_then(|step| step.setting.text.as_deref())
                    .unwrap_or(settings[0].text.as_deref().unwrap());
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(text).unwrap()["legacy_head"],
                    !head
                );
                let requested = recording.asked;
                root.close().unwrap();
                let mut replay = Replay {
                    answers: recording.answers,
                    asked: Vec::new(),
                };
                let mut again = initial;
                project_with(
                    &mut replay,
                    &root_path,
                    &units,
                    &carried,
                    &settings,
                    &mut again,
                )
                .unwrap();
                verify_with(&mut replay, &root_path, &units, &carried, &again).unwrap();
                assert_eq!(again, expected);
                assert_eq!(replay.asked, requested);
            }
        }
    }
}

#[test]
fn an_uncaptured_key_never_becomes_a_captured_absence() {
    let mut replay = Replay {
        answers: Answers::default(),
        asked: Vec::new(),
    };
    let path = Path::new("/fixture/absent");
    assert!(replay.text(path).is_err());
    assert!(replay.entries(path).is_err());
    replay.answers.texts.insert(path.into(), None);
    replay.answers.directories.insert(path.into(), None);
    assert_eq!(replay.text(path).unwrap(), None);
    assert!(replay.entries(path).unwrap().is_none());
}

#[test]
fn rejecting_an_early_override_does_not_consume_a_later_directory_error() {
    struct Early {
        yielded: Arc<AtomicUsize>,
    }
    impl HeadInputs for Early {
        fn text(&mut self, _path: &Path) -> Result<Option<String>, String> {
            Ok(Some("[Service]\nEnvironment=LAMP_HEAD_TOUCH=off\n".into()))
        }
        fn entries(&mut self, _directory: &Path) -> Result<Option<ConfEntries>, String> {
            let yielded = self.yielded.clone();
            let entries: Vec<Result<PathBuf, String>> = vec![
                Ok(PathBuf::from("/fixture/early.conf")),
                Err("later directory error".into()),
            ];
            Ok(Some(Box::new(entries.into_iter().inspect(move |_| {
                yielded.fetch_add(1, Ordering::SeqCst);
            }))))
        }
    }
    let yielded = Arc::new(AtomicUsize::new(0));
    let mut inputs = Early {
        yielded: yielded.clone(),
    };
    let error = no_mode_overrides(&mut inputs, Path::new("/fixture"), VOICE).unwrap_err();
    assert!(error.contains("local override"));
    assert!(!error.contains("later directory error"));
    assert_eq!(yielded.load(Ordering::SeqCst), 1);
}

#[test]
fn ordinary_inputs_keep_regular_links_null_masks_and_directory_aliases() {
    let root = tempfile::tempdir().unwrap();
    put(root.path(), "real/value", "unchanged");
    symlink("real/value", root.path().join("link")).unwrap();
    symlink("/dev/null", root.path().join("mask")).unwrap();
    symlink("real", root.path().join("alias")).unwrap();
    let mut inputs = OrdinaryInputs;
    assert_eq!(
        inputs.text(&root.path().join("link")).unwrap().as_deref(),
        Some("unchanged")
    );
    assert_eq!(
        inputs.text(&root.path().join("mask")).unwrap().as_deref(),
        Some("")
    );
    let paths = inputs
        .entries(&root.path().join("alias"))
        .unwrap()
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(paths, vec![root.path().join("alias/value")]);
    assert_eq!(
        fs::read_to_string(root.path().join("real/value")).unwrap(),
        "unchanged"
    );
}
