use lamp_apps::ask::{OFF, Parts, Request, answer, build_sequence, parse};
use lamp_apps::client::AdminYes;
use lamp_apps::layout::Layout;
use lamp_apps::lock::Running;
use lamp_apps::package::{AppPackage, AppSpec, sha256_hex};
use lamp_apps::peer::{CLOUD, VOICE, is_an_asker};
use lamp_apps::peer::{peer_of, set_read_timeout, start_of_stat, there_first};
use lamp_apps::real::{NoReader, SkillsFolder, uid_in};
use lamp_apps::testing::{FakeCompanions, FakeEffects, FakeYes, StubReader};
use lamp_apps::yes::{Op, Yes, YesKey};
use serde_json::json;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::time::Duration;

fn cg(path: &str) -> String {
    format!("0::{path}\n")
}

#[test]
fn only_cloud_and_the_voice_may_ask() {
    assert!(is_an_asker(&cg(CLOUD)));
    assert!(is_an_asker(&cg(VOICE)));
    assert!(is_an_asker(&format!("1:name=systemd:{CLOUD}\n")));
    for other in [
        "/lampos.slice/lampos-parts.slice/lamp-control.service",
        "/lampos.slice/lampos-apps.slice/lampapp@alpha.service",
        "/system.slice/lamp-cloud.service",
        "/lampos.slice/lampos-parts.slice/lamp-cloud.service.evil",
        "/lampos.slice/lampos-parts.slice/lamp-cloud.servicex/a",
        "",
    ] {
        assert!(!is_an_asker(&cg(other)), "{other}");
    }
    // A controller line other than the unified one or name=systemd.
    assert!(!is_an_asker(&format!("4:cpu:{CLOUD}\n")));
    // The unit's own cgroup only, not one below it.
    assert!(!is_an_asker(&cg(&format!("{CLOUD}/sub"))));
}

#[test]
fn a_request_is_parsed_strictly() {
    assert_eq!(parse("{\"list\":{}}"), Ok(Request::List {}));
    assert_eq!(parse("{\"start\":\"a\"}\n"), Ok(Request::Start("a".into())));
    assert_eq!(
        parse("{\"install\":{\"path\":\"/x/p.tar\"}}"),
        Ok(Request::Install {
            path: "/x/p.tar".into()
        })
    );
    for bad in [
        "",
        "{}",
        "{\"list\":{},\"stop\":\"a\"}",
        "{\"install\":{\"path\":\"/a\",\"file\":\"/etc\"}}",
        "{\"start\":1}",
        "{\"run\":\"a\"}",
        "nope",
    ] {
        assert!(parse(bad).is_err(), "{bad}");
    }
}

struct Rig {
    _root: tempfile::TempDir,
    layout: Layout,
    fx: FakeEffects,
    skills: FakeCompanions,
}

fn rig() -> Rig {
    let root = tempfile::tempdir().unwrap();
    Rig {
        layout: Layout::under(root.path()),
        _root: root,
        fx: FakeEffects::default(),
        skills: FakeCompanions::default(),
    }
}

#[test]
fn off_refuses_everything_but_the_list() {
    let mut r = rig();
    let mut yes = FakeYes::granting(vec![]);
    let mut parts = Parts {
        layout: &r.layout,
        fx: &mut r.fx,
        skills: &mut r.skills,
        yes: &mut yes,
        reader: &NoReader,
        staged_owner: 0,
        build: Some(1),
    };
    for line in [
        "{\"start\":\"a\"}",
        "{\"stop\":\"a\"}",
        "{\"remove\":\"a\"}",
        "{\"install\":{\"path\":\"/a\"}}",
    ] {
        assert_eq!(answer(false, line, &mut parts), json!({"refused": OFF}));
    }
    assert_eq!(
        answer(false, "{\"list\":{}}", &mut parts),
        json!({"ok": {"apps": []}})
    );
    assert!(r.fx.calls.is_empty() && yes.asked == 0);
}

#[test]
fn on_a_start_of_nothing_says_why() {
    let mut r = rig();
    let mut yes = FakeYes::granting(vec![]);
    let mut parts = Parts {
        layout: &r.layout,
        fx: &mut r.fx,
        skills: &mut r.skills,
        yes: &mut yes,
        reader: &NoReader,
        staged_owner: 0,
        build: None,
    };
    assert_eq!(
        answer(true, "{\"start\":\"ghost\"}", &mut parts),
        json!({"refused": "not installed"})
    );
    assert!(answer(true, "garbage", &mut parts)["refused"].is_string());
}

#[test]
fn an_install_goes_through_the_staging_rule_the_yes_and_the_reader() {
    use std::os::unix::fs::PermissionsExt;
    let mut r = rig();
    std::fs::create_dir_all(&r.layout.staging).unwrap();
    let bytes = b"package bytes".to_vec();
    let file = r.layout.staging.join("p.tar");
    std::fs::write(&file, &bytes).unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    let owner = std::os::unix::fs::MetadataExt::uid(&std::fs::metadata(&file).unwrap());
    let package = AppPackage {
        app: AppSpec {
            name: "alpha".into(),
            title: "T".into(),
            version: "1.0.0".into(),
            min_build: 1,
            net: false,
            logs: false,
            grants: vec!["speak".into()],
            entry: "app.main".into(),
        },
        files: BTreeMap::from([("src/app/main.py".to_owned(), b"x".to_vec())]),
        skills: vec![],
    };
    let key = YesKey {
        op: Op::Install,
        name: "alpha".into(),
        version: "1.0.0".into(),
        sha256: sha256_hex(&bytes),
        grants: vec!["speak".into()],
        net: false,
        logs: false,
    };
    let mut yes = FakeYes::granting(vec![key]);
    let reader = StubReader::new(package);
    let mut parts = Parts {
        layout: &r.layout,
        fx: &mut r.fx,
        skills: &mut r.skills,
        yes: &mut yes,
        reader: &reader,
        staged_owner: owner,
        build: Some(200),
    };
    let line = format!("{{\"install\":{{\"path\":\"{}\"}}}}", file.display());
    // A path is no file name; a file of another owner is refused.
    assert!(
        answer(
            true,
            "{\"install\":{\"path\":\"/etc/../p.tar\"}}",
            &mut parts
        )["refused"]
            .is_string()
    );
    parts.staged_owner = owner.wrapping_add(1);
    assert!(answer(true, &line, &mut parts)["refused"].is_string());
    assert!(!r.layout.code.join("alpha").exists());
    parts.staged_owner = owner;
    assert_eq!(answer(true, &line, &mut parts), json!({"ok": "installed"}));
    assert_eq!(
        answer(true, "{\"list\":{}}", &mut parts)["ok"]["apps"][0]["name"],
        "alpha"
    );
    // The yes was used up: the same ask again says already installed.
    assert!(answer(true, &line, &mut parts)["refused"].is_string());
}

#[test]
fn the_build_sequence_is_read_from_the_update_state_only() {
    assert_eq!(
        build_sequence("{\"installed\":{\"sequence\":14072,\"commit\":\"abc\"}}"),
        Some(14072)
    );
    for bad in [
        "",
        "abc",
        "812+460a1a1aa2b3",
        "{\"build\":\"143+abc\"}",
        "{\"installed\":{\"sequence\":\"7\"}}",
        "{\"installed\":{}}",
    ] {
        assert_eq!(build_sequence(bad), None, "{bad}");
    }
}

#[test]
fn the_real_yes_asks_lamp_control_one_line_and_reads_one_line() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("admin.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let server = std::thread::spawn(move || {
        let mut seen = Vec::new();
        for reply in ["{\"ok\":true}\n", "{\"refused\":\"no yes\"}\n", "ok\n"] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            seen.push(line);
            stream.write_all(reply.as_bytes()).unwrap();
        }
        seen
    });
    let key = YesKey {
        op: Op::Remove,
        name: "alpha".into(),
        version: "1.0.0".into(),
        sha256: "0".repeat(64),
        grants: vec![],
        net: false,
        logs: false,
    };
    let mut yes = AdminYes {
        socket,
        units: vec![own_cgroup()],
    };
    assert_eq!(yes.take(&key), Ok(()));
    assert_eq!(yes.take(&key), Err("no yes".into()));
    assert!(yes.take(&key).is_err());
    let seen = server.join().unwrap();
    assert!(seen[0].starts_with("{\"app_yes_take\":{"), "{}", seen[0]);
    assert!(seen[0].ends_with("}\n"));
}

#[test]
fn a_gone_socket_is_a_no() {
    let dir = tempfile::tempdir().unwrap();
    let mut yes = AdminYes {
        socket: dir.path().join("none.sock"),
        units: vec![own_cgroup()],
    };
    let key = YesKey {
        op: Op::Install,
        name: "a".into(),
        version: "1.0.0".into(),
        sha256: "0".repeat(64),
        grants: vec![],
        net: false,
        logs: false,
    };
    assert!(yes.take(&key).is_err());
}

#[test]
fn a_sweep_runs_only_with_no_instance_alive() {
    let dir = tempfile::tempdir().unwrap();
    {
        let _alive = Running::shared(dir.path(), Duration::from_secs(1)).unwrap();
        let _alive2 = Running::shared(dir.path(), Duration::from_secs(1)).unwrap();
        assert!(Running::alone(dir.path()).unwrap().is_none());
    }
    let alone = Running::alone(dir.path()).unwrap();
    assert!(alone.is_some());
    // While the sweep holds it, an instance waits, then is told busy.
    assert!(Running::shared(dir.path(), Duration::from_millis(100)).is_err());
}

#[test]
fn the_real_companions_place_nothing_and_say_so() {
    use lamp_apps::package::SkillFolder;
    use lamp_apps::remove::Companions;
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("weather")).unwrap();
    let mut skills = SkillsFolder {
        skills: dir.path().into(),
    };
    assert!(skills.place("a", &[]).is_ok());
    let folder = SkillFolder {
        name: "tutor".into(),
        files: BTreeMap::new(),
    };
    assert!(skills.place("a", &[folder]).is_err());
    assert!(skills.remove_for("a").is_ok());
    assert!(skills.taken_by_skill("weather"));
    assert!(!skills.taken_by_skill("alpha"));
}

#[test]
fn the_staging_owner_is_read_from_passwd() {
    let passwd = "root:x:0:0:r:/root:/bin/sh\nlampos:x:995:994::/var/lib/lampos:/sbin/nologin\n";
    assert_eq!(uid_in(passwd, "lampos"), Some(995));
    assert_eq!(uid_in(passwd, "root"), Some(0));
    assert_eq!(uid_in(passwd, "lampo"), None);
    assert_eq!(uid_in("lampos:x:abc:1\n", "lampos"), None);
}

/// This process's own cgroup path, as the peer test's "lamp-control".
fn own_cgroup() -> String {
    let text = std::fs::read_to_string("/proc/self/cgroup").unwrap();
    text.lines()
        .filter_map(|l| l.splitn(3, ':').nth(2))
        .next()
        .unwrap_or("/")
        .to_owned()
}

#[test]
fn an_answer_from_another_peer_than_lamp_control_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("admin.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let server = std::thread::spawn(move || {
        // It would say yes, if it were asked and believed.
        if let Ok((mut stream, _)) = listener.accept() {
            let _ = stream.write_all(b"{\"ok\":true}\n");
        }
    });
    let key = YesKey {
        op: Op::Install,
        name: "a".into(),
        version: "1.0.0".into(),
        sha256: "0".repeat(64),
        grants: vec![],
        net: false,
        logs: false,
    };
    // The product's: lamp-control's cgroup. This test process is not in it.
    let mut yes = AdminYes::product();
    yes.socket = socket;
    let error = yes.take(&key).unwrap_err();
    assert!(error.contains("not answered by lamp-control"), "{error}");
    let _ = server.join();
}

#[test]
fn the_peer_is_read_from_the_connection_with_its_start_time_held() {
    let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
    let peer = peer_of(&a).unwrap();
    assert_eq!(
        peer.uid,
        std::os::unix::fs::MetadataExt::uid(&std::fs::metadata("/proc/self").unwrap())
    );
    assert!(peer.cgroup.contains(&own_cgroup()));
    // field 22 of a stat line, whatever the command's name holds
    let stat = "123 (we) ird )) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 987654 20";
    assert_eq!(start_of_stat(stat), Some(987654));
    assert_eq!(start_of_stat("garbage"), None);
    assert!(there_first(10, 10) && there_first(9, 10) && !there_first(11, 10));
}

#[test]
fn a_silent_caller_is_cut_by_the_read_timeout() {
    let (mut a, b) = std::os::unix::net::UnixStream::pair().unwrap();
    set_read_timeout(&b, std::time::Duration::from_millis(150)).unwrap();
    let started = std::time::Instant::now();
    let mut line = String::new();
    let error = BufReader::new(&b).read_line(&mut line).unwrap_err();
    assert!(matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    ));
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    let _ = a.write_all(b"");
}

#[test]
fn a_line_over_the_cap_is_refused() {
    let mut r = rig();
    let mut yes = FakeYes::granting(vec![]);
    let mut parts = Parts {
        layout: &r.layout,
        fx: &mut r.fx,
        skills: &mut r.skills,
        yes: &mut yes,
        reader: &NoReader,
        staged_owner: 0,
        build: Some(1),
    };
    let long = format!("{{\"start\":\"{}\"}}", "a".repeat(5000));
    assert!(answer(true, &long, &mut parts)["refused"].is_string());
}

#[test]
fn the_binary_has_no_way_round_the_caller_check_and_checks_before_it_reads() {
    let main =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/main.rs")).unwrap();
    assert!(!main.contains("ANY_ASKER"));
    for flag in ["--key-file", "--skills", "--build-file"] {
        assert!(!main.contains(flag), "{flag}");
    }
    let check = main.find("peer_of(std::io::stdin())").unwrap();
    let timeout = main.find("set_read_timeout(").unwrap();
    let lock = main.find("Running::shared").unwrap();
    let read = main.find("read_line").unwrap();
    assert!(check < timeout && timeout < lock && lock < read);
}
