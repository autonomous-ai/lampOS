use lamp_apps::effects::judge_unit_empty;
use std::io;

fn events(text: &'static str) -> impl Fn(&str) -> io::Result<String> {
    move |_| Ok(text.to_owned())
}

#[test]
fn a_failed_show_is_an_error_never_empty() {
    let ok = events("populated 0\nfrozen 0\n");
    // Even when it printed a cgroup that reads empty, or nothing at all.
    for stdout in [
        "/lampos.slice/lampos-apps.slice/lampapp@x.service\n",
        "",
        "garbage",
    ] {
        assert!(judge_unit_empty(false, stdout, &ok).is_err(), "{stdout:?}");
    }
}

#[test]
fn no_cgroup_is_empty_and_a_gone_events_file_is_empty() {
    assert_eq!(
        judge_unit_empty(true, "\n", &events("populated 1\n")),
        Ok(true)
    );
    let gone = |_: &str| Err(io::Error::from(io::ErrorKind::NotFound));
    assert_eq!(judge_unit_empty(true, "/a/b\n", &gone), Ok(true));
}

#[test]
fn populated_decides_and_covers_children() {
    let group = "/lampos.slice/lampos-apps.slice/lampapp@x.service\n";
    assert_eq!(
        judge_unit_empty(true, group, &events("populated 0\nfrozen 0\n")),
        Ok(true)
    );
    assert_eq!(
        judge_unit_empty(true, group, &events("populated 1\nfrozen 0\n")),
        Ok(false)
    );
}

#[test]
fn a_file_that_cannot_be_read_or_understood_is_an_error() {
    let group = "/a/b\n";
    for text in [
        "",
        "frozen 0\n",
        "populated 2\n",
        "populated\n",
        "populated x\n",
    ] {
        let reader = move |_: &str| Ok(text.to_owned());
        assert!(judge_unit_empty(true, group, &reader).is_err(), "{text:?}");
    }
    let denied = |_: &str| Err(io::Error::from(io::ErrorKind::PermissionDenied));
    assert!(judge_unit_empty(true, group, &denied).is_err());
}

#[test]
fn a_path_that_is_not_a_cgroup_path_is_an_error() {
    let ok = events("populated 0\n");
    for stdout in ["relative/path", "/a/../b", "garbage", "/a\n/b"] {
        assert!(judge_unit_empty(true, stdout, &ok).is_err(), "{stdout:?}");
    }
}

#[test]
fn the_admin_socket_is_waited_for_and_given_up_on() {
    use lamp_apps::client::wait_for_path;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("admin.sock");
    // Never there: every look is made, a pause between each, then no.
    let mut pauses = 0;
    assert!(!wait_for_path(&path, 4, || pauses += 1));
    assert_eq!(pauses, 3);
    // There from the third look: the wait ends at once.
    let mut pauses = 0;
    let there = path.clone();
    assert!(wait_for_path(&path, 10, || {
        pauses += 1;
        if pauses == 2 {
            std::fs::write(&there, b"").unwrap();
        }
    }));
    assert_eq!(pauses, 2);
}
