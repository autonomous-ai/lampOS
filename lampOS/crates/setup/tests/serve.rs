use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use lamp_setup::{
    http::{Shared, router},
    scan,
    serve::{
        Accepted, Backend, Config, Files, Phase, Save, SetupRequest, Status, backend_link, check,
        run, start, start_unless_stopped, zone_env, zone_known,
    },
    wifi::Host,
};
use serde_json::{Value, json};
use std::{
    fs, io,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tower::ServiceExt;

static NEXT: AtomicU64 = AtomicU64::new(1);

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "lamp-setup-serve-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(path.join("zoneinfo/Asia")).unwrap();
        fs::create_dir_all(path.join("zoneinfo/America")).unwrap();
        fs::write(path.join("zoneinfo/Asia/Ho_Chi_Minh"), b"TZif").unwrap();
        fs::write(path.join("zoneinfo/America/New_York"), b"TZif").unwrap();
        fs::write(path.join("zoneinfo/leapseconds"), b"# leap seconds\n").unwrap();
        fs::create_dir_all(path.join("etc")).unwrap();
        fs::write(path.join("etc/real-localtime"), b"TZif").unwrap();
        std::os::unix::fs::symlink(
            path.join("etc/real-localtime"),
            path.join("zoneinfo/localtime"),
        )
        .unwrap();
        Self(path)
    }
    fn config(&self, page: bool) -> Config {
        Config {
            listen: "127.0.0.1:5000".into(),
            marker: self.0.join("etc/setup-done"),
            credential: self.0.join("etc/credential"),
            policy_key: self.0.join("etc/jev.key"),
            name: self.0.join("etc/name"),
            country: "VN".into(),
            interface: "wlan0".into(),
            hotspot_address: "192.168.100.1".into(),
            page,
            zoneinfo: self.0.join("zoneinfo"),
            localtime: self.0.join("etc/localtime"),
            timezone_file: self.0.join("etc/timezone"),
            zone_env: self.0.join("etc/zone.env"),
            backend: self.0.join("etc/backend.json"),
            web_start: vec![],
            web_stop: vec![],
            free_port: vec![],
            light: false,
        }
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn request(value: Value) -> SetupRequest {
    serde_json::from_value(value).unwrap()
}

fn good() -> Value {
    json!({"ssid": "Home", "password": "password1", "llm_api_key": "AIza-key_1",
           "device_id": "d1", "name": " Lan ", "timezone": "Asia/Ho_Chi_Minh",
           // V1's page sends these too; they are accepted and ignored.
           "channel": "telegram", "mqtt_port": 1883})
}

#[test]
fn a_request_is_checked_before_anything_is_touched() {
    let root = Root::new();
    let config = root.config(false);
    assert_eq!(
        check(&request(good()), &config).unwrap(),
        Accepted {
            ssid: "Home".into(),
            password: "password1".into(),
            credential: Some("AIza-key_1".into()),
            name: Some("Lan".into()),
            timezone: Some("Asia/Ho_Chi_Minh".into()),
            // A port alone is no link.
            backend: None,
        }
    );
    // The key is the user's (the owner, #806, 16:13). Without one, a lamp
    // that has none, or only half of it, is refused: it would have Wi-Fi and
    // no voice (the reviewer on #1120).
    let bare = request(json!({"ssid": "Cafe", "password": ""}));
    assert_eq!(
        check(&bare, &config).unwrap_err(),
        "the lamp needs its voice key"
    );
    fs::write(&config.credential, b"AIza-old").unwrap();
    assert!(
        check(&bare, &config).is_err(),
        "the Policy's half is missing"
    );
    // With both, the key already there stays, as on a lamp that ran V1.
    fs::write(&config.policy_key, b"AIza-old").unwrap();
    let without = check(&bare, &config).unwrap();
    assert_eq!(
        (without.credential, without.name, without.timezone),
        (None, None, None)
    );
    for (field, value) in [
        ("ssid", json!("")),
        ("ssid", json!("Home\nnetwork={")),
        ("password", json!("short")),
        ("llm_api_key", json!("two words")),
        ("llm_api_key", json!("k".repeat(513))),
        ("name", json!("x".repeat(65))),
        ("name", json!("La\nn")),
    ] {
        let mut body = good();
        body[field] = value.clone();
        assert!(check(&request(body), &config).is_err(), "{field} {value}");
    }
    // A zone the lamp does not know is skipped, not a reason to refuse setup.
    for zone in [
        "../etc/passwd",
        "Asia/Nowhere",
        "Asia/./Ho_Chi_Minh",
        "localtime",
        "leapseconds",
    ] {
        let mut body = good();
        body["timezone"] = json!(zone);
        assert_eq!(
            check(&request(body), &config).unwrap().timezone,
            None,
            "{zone}"
        );
    }
    assert!(zone_known("America/New_York", &config.zoneinfo));
    assert!(
        !zone_known("America", &config.zoneinfo),
        "a directory is no zone"
    );
    // Reviewer, #817: Debian's zoneinfo/localtime links to /etc/localtime.
    assert!(!zone_known("localtime", &config.zoneinfo));
    assert!(
        !zone_known("leapseconds", &config.zoneinfo),
        "not a TZif file"
    );
}

/// A lamp whose join ends after `seconds` of fake time as `connects` says.
/// Its hotspot names it `lamp-4ace`, as the image's `device-ap-mode` names a
/// fresh image that still has the image's own name.
struct Lamp {
    connects: bool,
    conf: Option<Vec<u8>>,
    calls: Arc<Mutex<Vec<String>>>,
    name: Arc<Mutex<String>>,
    now_ms: u64,
}
impl Lamp {
    fn new(connects: bool) -> Self {
        Self {
            connects,
            conf: None,
            calls: Arc::default(),
            name: Arc::new(Mutex::new("orangepizero2w".into())),
            now_ms: 0,
        }
    }
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}
impl Host for Lamp {
    fn read_conf(&mut self) -> io::Result<Option<Vec<u8>>> {
        Ok(self.conf.clone())
    }
    fn write_conf(&mut self, bytes: Option<&[u8]>) -> io::Result<()> {
        self.conf = bytes.map(<[u8]>::to_vec);
        Ok(())
    }
    fn station(&mut self) -> io::Result<()> {
        self.calls.lock().unwrap().push("station".into());
        Ok(())
    }
    fn hotspot(&mut self) -> io::Result<()> {
        self.calls.lock().unwrap().push("hotspot".into());
        *self.name.lock().unwrap() = "lamp-4ace".into();
        Ok(())
    }
    fn internet(&mut self) -> bool {
        self.connects && self.now_ms >= 3_000
    }
    fn current_ssid(&mut self) -> Option<String> {
        self.internet().then(|| "Home".into())
    }
    fn has_station_ip(&mut self) -> bool {
        false
    }
    fn wpa_state(&mut self) -> String {
        "SCANNING".into()
    }
    fn wpa_events(&mut self) -> Vec<String> {
        if self.connects {
            vec![]
        } else {
            vec![
                "wlan0: WPA: 4-Way Handshake failed - pre-shared key may be incorrect".into(),
                "wlan0: WPA: 4-Way Handshake failed - pre-shared key may be incorrect".into(),
            ]
        }
    }
    fn now_ms(&mut self) -> u64 {
        self.now_ms
    }
    fn sleep(&mut self, duration: Duration) {
        self.now_ms += duration.as_millis() as u64;
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[derive(Default)]
struct Saved {
    calls: Vec<String>,
    fail_on: Option<&'static str>,
}
impl Save for Saved {
    fn credential(&mut self, key: &str) -> io::Result<()> {
        self.record("credential", key)
    }
    fn name(&mut self, name: &str) -> io::Result<()> {
        self.record("name", name)
    }
    fn timezone(&mut self, zone: &str) -> io::Result<()> {
        self.record("timezone", zone)
    }
    fn backend(&mut self, link: &Backend) -> io::Result<()> {
        self.record(
            "backend",
            &format!(
                "{}:{} {} {}",
                link.mqtt_endpoint, link.mqtt_port, link.fa_channel, link.fd_channel
            ),
        )
    }
    fn marker(&mut self) -> io::Result<()> {
        self.record("marker", "")
    }
}
impl Saved {
    fn record(&mut self, what: &'static str, value: &str) -> io::Result<()> {
        if self.fail_on == Some(what) {
            return Err(io::Error::other("disk full"));
        }
        self.calls.push(format!("{what}={value}"));
        Ok(())
    }
}

fn accepted() -> Accepted {
    check(&request(good()), &Root::new().config(false)).unwrap()
}

#[test]
fn a_joined_lamp_saves_the_settings_then_the_marker_and_says_it_is_set_up() {
    let status = Arc::new(Mutex::new(Status::new("lamp-4ace".into())));
    let mut saved = Saved::default();
    run(
        &status,
        &accepted(),
        "VN",
        &mut Lamp::new(true),
        &mut saved,
        || Some("192.168.1.23".into()),
    );
    let status = status.lock().unwrap().clone();
    assert_eq!(status.phase, Phase::Connected);
    assert!(status.set_up_completed);
    assert_eq!(status.run, 1);
    // V1's page keeps its "Preparing your robot's voice…" screen.
    assert_eq!(status.runtime_phase, "preparing");
    assert_eq!(status.lan_ip, "192.168.1.23", "published while joining");
    assert_eq!(
        saved.calls,
        [
            "name=Lan",
            "timezone=Asia/Ho_Chi_Minh",
            "credential=AIza-key_1",
            "marker="
        ]
    );
}

#[test]
fn a_wrong_password_saves_nothing_and_names_the_reason() {
    let status = Arc::new(Mutex::new(Status::new("lamp-4ace".into())));
    let mut saved = Saved::default();
    let mut lamp = Lamp::new(false);
    run(&status, &accepted(), "VN", &mut lamp, &mut saved, || None);
    let status = status.lock().unwrap().clone();
    assert_eq!(status.phase, Phase::Failed);
    assert!(!status.set_up_completed);
    assert!(
        status.error.starts_with("wrong WiFi password"),
        "{}",
        status.error
    );
    assert!(saved.calls.is_empty());
    assert_eq!(lamp.calls(), ["station", "hotspot"]);
}

#[test]
fn settings_that_cannot_be_saved_leave_no_marker_and_bring_the_hotspot_back() {
    let status = Arc::new(Mutex::new(Status::new("lamp-4ace".into())));
    let mut saved = Saved {
        fail_on: Some("timezone"),
        ..Saved::default()
    };
    let mut lamp = Lamp::new(true);
    lamp.conf = Some(b"country=VN\n".to_vec());
    run(&status, &accepted(), "VN", &mut lamp, &mut saved, || None);
    // The joined network's file is not kept: the previous one is back.
    assert_eq!(lamp.conf.as_deref(), Some(&b"country=VN\n"[..]));
    let status = status.lock().unwrap().clone();
    assert_eq!(status.phase, Phase::Failed);
    assert!(!status.set_up_completed);
    assert!(!saved.calls.iter().any(|call| call.starts_with("marker")));
    // The key is saved last, so the one there stays as it was.
    assert!(
        !saved
            .calls
            .iter()
            .any(|call| call.starts_with("credential"))
    );
    assert_eq!(lamp.calls(), ["station", "hotspot"]);
}

/// Nine on #1120: a debug line never shows the Wi-Fi password or the key.
#[test]
fn neither_the_password_nor_the_key_is_ever_debug_printed() {
    let shown = format!("{:?} {:?}", request(good()), accepted());
    assert!(!shown.contains("password1"), "{shown}");
    assert!(!shown.contains("AIza-key_1"), "{shown}");
    assert!(
        shown.contains("Home") && shown.contains("<hidden>"),
        "{shown}"
    );
}

/// `start` on `lamp`, its commands and its name read logged with the
/// hotspot; a command whose program is `fail` fails.
fn start_on(lamp: &mut Lamp, config: &Config, fail: &str) -> (io::Result<Status>, Vec<String>) {
    let (calls, name) = (Arc::clone(&lamp.calls), Arc::clone(&lamp.name));
    let named = Arc::clone(&calls);
    let mut logged = Vec::new();
    let started = start(
        config,
        lamp,
        |argv| {
            calls.lock().unwrap().push(argv.join(" "));
            if argv[0] == fail {
                return Err(io::Error::other("Unit nginx.service not loaded."));
            }
            Ok(())
        },
        move || {
            named.lock().unwrap().push("name".into());
            name.lock().unwrap().clone()
        },
        |line| logged.push(line.to_owned()),
    );
    (started, logged)
}

#[test]
fn the_name_is_read_after_the_hotspot_which_names_a_fresh_lamp() {
    // Reviewer, #817: on a fresh image `device-ap-mode` renames the host to
    // `lamp-xxxx`; a name read before it sends the page to the wrong `.local`.
    let root = Root::new();
    let mut lamp = Lamp::new(true);
    let (started, _) = start_on(&mut lamp, &root.config(false), "");
    assert_eq!(started.unwrap().mac, "lamp-4ace");
    assert_eq!(lamp.calls(), ["hotspot", "name"]);
}

fn fixture(text: &str) -> Config {
    let config: Config = serde_json::from_str(text).unwrap();
    config.check().unwrap();
    config
}

/// The product's pair (#1016, install.md): the own page's settings and the
/// product reset plan agree on the marker, and what setup saves of the
/// person, the name and the user's key in both its places, is what the reset
/// wipes (the owner, #806, 16:13). The time zone setup writes
/// (`/etc/localtime`, `/etc/timezone`) is kept: it is where the lamp is, not
/// who had it (the fifth on #1016).
#[test]
fn the_own_page_and_the_product_plan_agree_on_what_setup_saves() {
    let own = fixture(include_str!("../fixtures/serve-own-page.json"));
    let plan: lamp_setup::reset::Plan =
        serde_json::from_str(include_str!("../fixtures/reset-plan-product.json")).unwrap();
    plan.check().unwrap();
    assert_eq!(own.marker, plan.marker);
    for saved in [&own.name, &own.credential, &own.policy_key] {
        assert!(
            plan.wipe.iter().any(|wiped| saved.starts_with(wiped)),
            "{} is saved and not wiped",
            saved.display()
        );
    }
    assert_eq!(
        own.credential,
        std::path::Path::new("/etc/lampOS/voice/credential")
    );
    assert_eq!(
        own.policy_key,
        std::path::Path::new("/etc/lampOS/policy/jev.key")
    );
}

/// Thirteen on #1016: a key a setup left half-written is removed at start,
/// in both its places, with the name's, the marker's and the backend link's
/// temporary files (the link holds the broker's password, #1396).
#[test]
fn a_leftover_temporary_key_is_removed() {
    let root = Root::new();
    let config = root.config(true);
    let tmp = |path: &std::path::Path| path.with_extension("lamp-setup-tmp");
    assert_eq!(
        lamp_setup::serve::leftover_temporaries(&config),
        [
            tmp(&config.name),
            tmp(&config.marker),
            tmp(&config.credential),
            tmp(&config.policy_key),
            tmp(&config.backend),
        ]
    );
}

/// Settings that name no key files (#1016's) take the product's places.
#[test]
fn settings_without_key_files_take_the_products_places() {
    let plain = fixture(
        r#"{"listen":"0.0.0.0:80","marker":"/var/lib/lampos-setup/done",
            "name":"/etc/lampOS/person/name","country":"VN","page":true}"#,
    );
    assert_eq!(
        plain.credential,
        std::path::Path::new("/etc/lampOS/voice/credential")
    );
    assert_eq!(
        plain.policy_key,
        std::path::Path::new("/etc/lampOS/policy/jev.key")
    );
}

#[test]
fn the_own_page_stops_nginx_after_the_hotspot_restarts_it_and_before_answering() {
    // Reviewer, #817: `device-ap-mode` restarts nginx, which holds port 80.
    let own = fixture(include_str!("../fixtures/serve-own-page.json"));
    assert!(own.page);
    assert_eq!(own.listen, "0.0.0.0:80");
    // The product's setup shows the setup light (#806, First, 17 Oct 14:44).
    assert!(own.light);
    let mut lamp = Lamp::new(true);
    let (started, logged) = start_on(&mut lamp, &own, "");
    started.unwrap();
    assert_eq!(
        lamp.calls(),
        ["hotspot", "/usr/bin/systemctl stop nginx", "name"]
    );
    assert!(logged.is_empty(), "{logged:?}");

    // A lamp without nginx has nothing to stop: logged, and setup goes on.
    let mut lamp = Lamp::new(true);
    let (started, logged) = start_on(&mut lamp, &own, "/usr/bin/systemctl");
    assert_eq!(started.unwrap().mac, "lamp-4ace");
    assert_eq!(logged.len(), 1);
    assert!(logged[0].contains("stop nginx"), "{logged:?}");

    // Behind V1's nginx nothing changes: nginx is started, never stopped.
    let behind = fixture(include_str!("../fixtures/serve-behind-v1-nginx.json"));
    assert!(behind.free_port.is_empty() && !behind.page && !behind.light);
    let mut lamp = Lamp::new(true);
    let (started, _) = start_on(&mut lamp, &behind, "");
    started.unwrap();
    assert_eq!(
        lamp.calls(),
        ["hotspot", "/usr/bin/systemctl start nginx", "name"]
    );
    // There, nginx that cannot start is a failure to serve.
    let mut lamp = Lamp::new(true);
    assert!(
        start_on(&mut lamp, &behind, "/usr/bin/systemctl")
            .0
            .is_err()
    );

    let relative = Config {
        free_port: vec!["systemctl".into(), "stop".into(), "nginx".into()],
        ..own
    };
    assert!(relative.check().is_err());
}

#[test]
fn the_files_are_private_and_the_zone_is_linked_as_v1_does() {
    let root = Root::new();
    let config = root.config(false);
    let mut files = Files(&config);
    files.credential("AIza-key_1").unwrap();
    files.name("Lan").unwrap();
    files.timezone("Asia/Ho_Chi_Minh").unwrap();
    files.marker().unwrap();
    for path in [
        &config.credential,
        &config.policy_key,
        &config.name,
        &config.marker,
    ] {
        let mode = fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "{}", path.display());
    }
    // The user's key, to the voice and to the Policy's judge.
    assert_eq!(fs::read(&config.credential).unwrap(), b"AIza-key_1");
    assert_eq!(fs::read(&config.policy_key).unwrap(), b"AIza-key_1");
    assert_eq!(fs::read(&config.name).unwrap(), b"Lan\n");
    assert_eq!(
        fs::read_link(&config.localtime).unwrap(),
        config.zoneinfo.join("Asia/Ho_Chi_Minh")
    );
    assert_eq!(
        fs::read(&config.timezone_file).unwrap(),
        b"Asia/Ho_Chi_Minh\n"
    );
    // Schedule's and skills' zone (#820), readable by the parts, as install's.
    assert_eq!(
        fs::read_to_string(&config.zone_env).unwrap(),
        zone_env("Asia/Ho_Chi_Minh", 420)
    );
    let mode = fs::metadata(&config.zone_env).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o644);
    // A second zone replaces the first: a lamp set up outside +07.
    files.timezone("America/New_York").unwrap();
    assert_eq!(
        fs::read_link(&config.localtime).unwrap(),
        config.zoneinfo.join("America/New_York")
    );
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let west = lamp_local_time::LocalTime::zone("America/New_York")
        .unwrap()
        .offset_minutes_at(now_ms);
    assert!(west == -300 || west == -240, "{west}");
    let written = fs::read_to_string(&config.zone_env).unwrap();
    assert_eq!(written, zone_env("America/New_York", west));
    assert!(written.contains("\nLAMP_ZONE=America/New_York\n"));
    // A zone the parts' zone data lacks leaves theirs as it was: schedule
    // would not start on it.
    files.timezone("Mars/Olympus_Mons").unwrap();
    assert_eq!(fs::read_to_string(&config.zone_env).unwrap(), written);
    assert_eq!(
        fs::read(&config.timezone_file).unwrap(),
        b"Mars/Olympus_Mons\n"
    );
}

/// Nine on #1120: when the Policy's key cannot be written, the voice's is
/// not changed either, so both keep one key, and no temporary file is left.
#[test]
fn a_key_the_policy_cannot_take_leaves_the_voices_as_it_was() {
    let root = Root::new();
    let mut config = root.config(false);
    fs::write(&config.credential, b"AIza-old").unwrap();
    // A file where the Policy's folder should be.
    fs::write(root.0.join("etc/policy"), b"").unwrap();
    config.policy_key = root.0.join("etc/policy/jev.key");
    assert!(Files(&config).credential("AIza-new").is_err());
    assert_eq!(fs::read(&config.credential).unwrap(), b"AIza-old");
    assert!(!config.credential.with_extension("lamp-setup-tmp").exists());
}

static SCANS: AtomicU64 = AtomicU64::new(0);

/// The router, its status, the runs it was asked to start, and its files.
type App = (
    axum::Router,
    Arc<Mutex<Status>>,
    Arc<Mutex<Vec<Accepted>>>,
    Root,
);

fn app(page: bool) -> App {
    let root = Root::new();
    let status = Arc::new(Mutex::new(Status::new("lamp-4ace".into())));
    let started = Arc::new(Mutex::new(Vec::new()));
    let shared = Arc::new(Shared {
        config: root.config(page),
        status: Arc::clone(&status),
        start: Box::new({
            let started = Arc::clone(&started);
            move |accepted| started.lock().unwrap().push(accepted)
        }),
        scanned: Mutex::new(None),
        last_good: Mutex::new(None),
        scan: Box::new(|| {
            SCANS.fetch_add(1, Ordering::Relaxed);
            Ok(scan::parse(
                "BSS 34:3a:20:95:78:93(on wlan0)\n\tsignal: -41.00 dBm\n\tSSID: Home\n\tDS Parameter set: channel 6\n",
            ))
        }),
    });
    (router(shared), status, started, root)
}

/// A router whose scans fail the first `refusals` times, as a radio that is
/// the hotspot may refuse them, and the list seen before the hotspot.
fn refusing(refusals: u64, before: Option<Vec<scan::Network>>) -> (Arc<Shared>, Root) {
    let root = Root::new();
    let count = Arc::new(AtomicU64::new(0));
    let shared = Arc::new(Shared {
        config: root.config(true),
        status: Arc::new(Mutex::new(Status::new("lamp-06c1".into()))),
        start: Box::new(|_| {}),
        scanned: Mutex::new(None),
        last_good: Mutex::new(before),
        scan: Box::new(move || {
            if count.fetch_add(1, Ordering::Relaxed) < refusals {
                Err(std::io::Error::other("iw scan failed"))
            } else {
                Ok(scan::parse(
                    "BSS 34:3a:20:95:78:93(on wlan0)\n\tsignal: -50.00 dBm\n\tSSID: Office\n",
                ))
            }
        }),
    });
    (shared, root)
}

fn named(list: &[scan::Network]) -> Vec<String> {
    list.iter().map(|network| network.ssid.clone()).collect()
}

/// lamp-06c1, 2026-10-16 12:40: the phone said "Could not look for
/// networks" until it asked a few times. A list seen before the hotspot,
/// or by any scan since, answers when the radio refuses one.
#[tokio::test(flavor = "current_thread")]
async fn a_refused_scan_is_answered_with_the_last_list_seen() {
    use lamp_setup::http::list_networks;
    let home = scan::parse("BSS 11:22:33:44:55:66(on wlan0)\n\tSSID: Home\n");
    let (shared, _root) = refusing(1, Some(home));
    assert_eq!(named(&list_networks(&shared).unwrap()), ["Home"]);
    // Through the route, in V1's shape.
    let (shared_route, _root_route) = refusing(
        u64::MAX,
        Some(scan::parse(
            "BSS 11:22:33:44:55:66(on wlan0)\n\tSSID: Home\n",
        )),
    );
    let (code, body) = call(&router(shared_route), "GET", "/api/network", Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(body["data"][0]["ssid"], "Home");
    // A scan that works becomes the list kept, and answers a later refusal.
    assert_eq!(named(&list_networks(&shared).unwrap()), ["Office"]);
    *shared.scanned.lock().unwrap() = None;
    assert_eq!(named(&list_networks(&shared).unwrap()), ["Office"]);
    // With nothing seen yet, a refusal is the page's error.
    let (empty, _root_empty) = refusing(u64::MAX, None);
    assert!(list_networks(&empty).is_err());
    let (code, _) = call(&router(empty), "GET", "/api/network", Value::Null).await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
}

/// The fifth on #1045: some radios as the hotspot answer a scan with no
/// networks rather than a refusal. An empty answer never replaces a list
/// seen before, and does not count as a warm list.
#[tokio::test(flavor = "current_thread")]
async fn an_empty_scan_never_replaces_a_list_seen_before() {
    use lamp_setup::http::{list_networks, warm};
    let empty = |before: Option<Vec<scan::Network>>| {
        let root = Root::new();
        let shared = Arc::new(Shared {
            config: root.config(true),
            status: Arc::new(Mutex::new(Status::new("lamp-06c1".into()))),
            start: Box::new(|_| {}),
            scanned: Mutex::new(None),
            last_good: Mutex::new(before),
            scan: Box::new(|| Ok(Vec::new())),
        });
        (shared, root)
    };
    let home = scan::parse("BSS 11:22:33:44:55:66(on wlan0)\n\tSSID: Home\n");
    let (shared, _root) = empty(Some(home));
    assert_eq!(named(&list_networks(&shared).unwrap()), ["Home"]);
    assert_eq!(
        named(shared.last_good.lock().unwrap().as_ref().unwrap()),
        ["Home"]
    );
    let (code, body) = call(
        &router(Arc::clone(&shared)),
        "GET",
        "/api/network",
        Value::Null,
    )
    .await;
    assert_eq!(
        (code, body["data"][0]["ssid"].clone()),
        (StatusCode::OK, json!("Home"))
    );
    // A page that already has a list to show ends the warm-up: more scans
    // as the hotspot would only answer empty again.
    assert!(warm(&shared, 3, std::time::Duration::from_millis(1)));
    // The answer is kept for 5 s like any other (the reviewer on #1064):
    // a second poll within it runs no scan.
    let scans = Arc::new(AtomicU64::new(0));
    let counted = Arc::clone(&scans);
    let (once, _root_once) = {
        let root = Root::new();
        let shared = Arc::new(Shared {
            config: root.config(true),
            status: Arc::new(Mutex::new(Status::new("lamp-06c1".into()))),
            start: Box::new(|_| {}),
            scanned: Mutex::new(None),
            last_good: Mutex::new(Some(scan::parse(
                "BSS 11:22:33:44:55:66(on wlan0)\n\tSSID: Home\n",
            ))),
            scan: Box::new(move || {
                counted.fetch_add(1, Ordering::Relaxed);
                Ok(Vec::new())
            }),
        });
        (shared, root)
    };
    assert_eq!(named(&list_networks(&once).unwrap()), ["Home"]);
    assert_eq!(named(&list_networks(&once).unwrap()), ["Home"]);
    assert_eq!(scans.load(Ordering::Relaxed), 1);
    // With nothing seen, the empty list is the answer, as it was.
    let (none, _root_none) = empty(None);
    assert!(list_networks(&none).unwrap().is_empty());
}

#[test]
fn a_few_scans_before_anyone_asks_warm_the_list() {
    use lamp_setup::http::warm;
    let (shared, _root) = refusing(2, None);
    assert!(warm(&shared, 3, std::time::Duration::from_millis(1)));
    assert_eq!(
        named(shared.last_good.lock().unwrap().as_ref().unwrap()),
        ["Office"]
    );
    let (never, _root_never) = refusing(u64::MAX, None);
    assert!(!warm(&never, 3, std::time::Duration::from_millis(1)));
    assert!(never.last_good.lock().unwrap().is_none());
}

async fn call(app: &axum::Router, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .body(if method == "GET" {
            Body::empty()
        } else {
            Body::from(body.to_string())
        })
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let code = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    (code, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

#[tokio::test(flavor = "current_thread")]
async fn v1s_routes_answer_in_v1s_shapes() {
    let (app, status, started, _root) = app(false);
    let (code, body) = call(&app, "GET", "/api/device/setup/status", Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        body,
        json!({"status": 1, "message": null, "data": {"phase": "idle", "lan_ip": "", "error": "",
               "mac": "lamp-4ace", "run": 0, "set_up_completed": false, "runtime_phase": "",
               // Not V1's: the app tells its two Lamps apart by it (#1396).
               "generation": "lamp-v2"}})
    );
    let before = SCANS.load(Ordering::Relaxed);
    let (code, body) = call(&app, "GET", "/api/network", Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    // A second ask within 5 s gets the same list without a second scan.
    let (_, again) = call(&app, "GET", "/api/network", Value::Null).await;
    assert_eq!(again, body);
    assert_eq!(SCANS.load(Ordering::Relaxed) - before, 1);
    assert_eq!(
        body["data"],
        json!([{"bssid": "34:3a:20:95:78:93", "ssid": "Home", "mode": "STA", "channel": 6,
                "rate": "", "signal": -41, "linkRate": 0, "security": ""}])
    );
    let (code, body) = call(
        &app,
        "POST",
        "/api/device/setup",
        json!({"ssid": "Home", "password": "short", "llm_api_key": "k"}),
    )
    .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    assert_eq!(body["status"], 0);
    assert!(body["message"].as_str().unwrap().contains("8 to 63"));
    assert!(started.lock().unwrap().is_empty());

    // A last run's address goes, so V1's page cannot follow it.
    status.lock().unwrap().lan_ip = "192.168.1.99".into();
    let (code, body) = call(&app, "POST", "/api/device/setup", good()).await;
    assert_eq!(status.lock().unwrap().lan_ip, "");
    assert_eq!(
        (code, body),
        (
            StatusCode::OK,
            json!({"status": 1, "data": true, "message": null})
        )
    );
    assert_eq!(started.lock().unwrap().len(), 1);
    assert_eq!(status.lock().unwrap().phase, Phase::Connecting);
    let (code, _) = call(&app, "POST", "/api/device/setup", good()).await;
    assert_eq!(code, StatusCode::CONFLICT, "one run at a time");

    status.lock().unwrap().phase = Phase::Connected;
    status.lock().unwrap().set_up_completed = true;
    let (code, _) = call(&app, "POST", "/api/device/setup", good()).await;
    assert_eq!(
        code,
        StatusCode::CONFLICT,
        "a set-up lamp is not set up again here"
    );
    assert_eq!(started.lock().unwrap().len(), 1);
    // No page without `page`.
    let (code, _) = call(&app, "GET", "/setup", Value::Null).await;
    assert_eq!(code, StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "current_thread")]
async fn the_own_page_and_the_phones_checks_are_served_only_with_page() {
    let (app, _, _, _root) = app(true);
    for path in ["/", "/setup"] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let html = to_bytes(response.into_body(), 1 << 20).await.unwrap();
        assert!(String::from_utf8_lossy(&html).contains("/api/device/setup/status"));
    }
    for path in [
        "/generate_204",
        "/hotspot-detect.html",
        "/ncsi.txt",
        "/connecttest.txt",
    ] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT, "{path}");
    }
}

#[test]
fn the_scan_reads_v1s_fields_and_iws_escapes() {
    let output = "BSS 34:3a:20:95:78:93(on wlan0) -- associated\n\
                  \tfreq: 2437\n\tsignal: -48.00 dBm\n\tSSID: Caf\\xc3\\xa9\\ Home\n\
                  \tDS Parameter set: channel 6\n\tHESSID: 00:00:00:00:00:00\n\
                  BSS 11:22:33:44:55:66(on wlan0)\n\tsignal: -80.00 dBm\n\tSSID: \n\
                  BSS aa:bb:cc:dd:ee:ff(on wlan0)\n\tsignal: -67.50 dBm\n\tSSID: Neighbour\n\tSSID: second\n";
    let list = scan::parse(output);
    assert_eq!(list.len(), 2, "a hidden network has no name to choose");
    assert_eq!(list[0].ssid, "Café Home");
    assert_eq!((list[0].signal, list[0].channel), (-48, 6));
    assert_eq!(list[1].ssid, "Neighbour", "the first SSID line wins");
    assert_eq!(list[1].signal, -67);
}

/// A stop around the hotspot's start (the reviewers on #1336 and #1341):
/// before it, nothing is started; a start that fails with a stop pending is
/// that stop, its error kept in the line; with no stop a failure stays one.
#[test]
fn a_stop_around_the_hotspot_is_a_stop_and_a_failure_without_one_stays_a_failure() {
    let ended = || Err(io::Error::other("device-ap-mode ended with signal: 15"));
    let started = || Ok(Status::new("lamp-4ace".into()));

    // Asked to stop during the look: the hotspot is never started.
    let mut logged = Vec::new();
    let mut began = false;
    let outcome = start_unless_stopped(
        || true,
        || {
            began = true;
            started()
        },
        |line| logged.push(line.to_owned()),
    );
    assert!(outcome.unwrap().is_none() && !began);
    assert_eq!(logged, ["asked to stop before the hotspot"]);

    // systemd's stop ended the hotspot script too: a stop, with its error.
    let mut asked = 0;
    let mut logged = Vec::new();
    let outcome = start_unless_stopped(
        || {
            asked += 1;
            asked > 1
        },
        ended,
        |line| logged.push(line.to_owned()),
    );
    assert!(outcome.unwrap().is_none());
    assert_eq!(
        logged,
        ["asked to stop while the hotspot came up (device-ap-mode ended with signal: 15)"]
    );

    // No stop: the failure ends setup with its error, as before.
    let mut logged = Vec::new();
    let failed = start_unless_stopped(|| false, ended, |line| logged.push(line.to_owned()));
    assert_eq!(
        failed.unwrap_err().to_string(),
        "device-ap-mode ended with signal: 15"
    );
    assert!(logged.is_empty());

    // A start that worked is served, even with a stop pending: serving's
    // own loop ends it at once, after which the light is put out.
    let mut asked = 0;
    let outcome = start_unless_stopped(
        || {
            asked += 1;
            asked > 1
        },
        started,
        |_| {},
    );
    assert_eq!(outcome.unwrap().unwrap().mac, "lamp-4ace");
    assert_eq!(asked, 1, "a good start is not asked about a stop again");
}

/// The Autonomous app's setup body for a lamp, field for field
/// (`ecm-sds-mobile` v3, `AutonomousOsSetupConfig.buildSetupPayload`; #1396),
/// with made-up values.
fn app_body() -> Value {
    json!({"ssid": "Home", "password": "password1",
           "telegram_bot_token": "none", "telegram_user_id": "none",
           "device_id": "dev-42", "llm_base_url": "https://gateway.example/v1",
           "llm_api_key": "AIza-key_1", "llm_model": "auto",
           "mqtt_endpoint": "broker.example", "mqtt_port": 1883,
           "mqtt_username": "lamp-dev-42", "mqtt_password": "s3cret pass",
           "fa_channel": "fa/dev-42", "fd_channel": "fd/dev-42",
           "name": "Lan", "timezone": "Asia/Ho_Chi_Minh"})
}

fn app_link() -> Backend {
    Backend {
        device_id: "dev-42".into(),
        llm_base_url: "https://gateway.example/v1".into(),
        mqtt_endpoint: "broker.example".into(),
        mqtt_port: 1883,
        mqtt_username: "lamp-dev-42".into(),
        mqtt_password: "s3cret pass".into(),
        fa_channel: "fa/dev-42".into(),
        fd_channel: "fd/dev-42".into(),
    }
}

#[test]
fn the_apps_setup_keeps_the_backends_link_and_never_shows_its_password() {
    let root = Root::new();
    let request = request(app_body());
    let accepted = check(&request, &root.config(false)).unwrap();
    assert_eq!(accepted.backend, Some(app_link()));
    for shown in [format!("{request:?}"), format!("{accepted:?}")] {
        assert!(!shown.contains("s3cret"), "{shown}");
        assert!(!shown.contains("AIza-key_1"), "{shown}");
        assert!(shown.contains("broker.example"), "{shown}");
    }
    // A port as digits, as a link would carry it, is the same port.
    let mut digits = app_body();
    digits["mqtt_port"] = json!("1883");
    assert_eq!(
        check(&self::request(digits), &root.config(false))
            .unwrap()
            .backend,
        Some(app_link())
    );
}

#[test]
fn a_broken_or_partial_link_is_none_and_never_fails_setup() {
    let root = Root::new();
    let config = root.config(false);
    for (field, value) in [
        ("mqtt_port", json!("abc")),
        ("mqtt_port", json!(0)),
        ("mqtt_port", json!(70000)),
        ("fd_channel", json!("")),
        ("mqtt_endpoint", json!("")),
        ("mqtt_endpoint", json!("broker example")),
        ("mqtt_username", json!("x".repeat(513))),
        ("mqtt_password", json!("line\nbreak")),
        // `lamp-cloud` is the lamp in the app by its id (Third on #1401).
        ("device_id", json!("")),
    ] {
        let mut body = app_body();
        body[field] = value;
        let accepted = check(&request(body), &config).unwrap();
        assert_eq!(accepted.backend, None, "{field}");
        assert_eq!(accepted.ssid, "Home", "{field}: setup goes on");
    }
}

#[test]
fn the_link_is_saved_after_the_zone_and_before_the_key_and_the_marker() {
    let status = Arc::new(Mutex::new(Status::new("lamp-4ace".into())));
    let mut saved = Saved::default();
    let accepted = check(&request(app_body()), &Root::new().config(false)).unwrap();
    run(
        &status,
        &accepted,
        "VN",
        &mut Lamp::new(true),
        &mut saved,
        || Some("192.168.1.23".into()),
    );
    assert_eq!(
        saved.calls,
        [
            "name=Lan",
            "timezone=Asia/Ho_Chi_Minh",
            "backend=broker.example:1883 fa/dev-42 fd/dev-42",
            "credential=AIza-key_1",
            "marker="
        ]
    );
    assert!(status.lock().unwrap().set_up_completed);
}

#[test]
fn the_link_is_written_root_only_and_whole() {
    use std::os::unix::fs::PermissionsExt;
    let root = Root::new();
    let config = root.config(false);
    Files(&config).backend(&app_link()).unwrap();
    let mode = fs::metadata(&config.backend).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let written: Value = serde_json::from_slice(&fs::read(&config.backend).unwrap()).unwrap();
    assert_eq!(written["mqtt_endpoint"], "broker.example");
    assert_eq!(written["mqtt_port"], 1883);
    assert_eq!(written["mqtt_password"], "s3cret pass");
    assert_eq!(written["fd_channel"], "fd/dev-42");
    assert!(
        lamp_setup::serve::leftover_temporaries(&config)
            .contains(&config.backend.with_extension("lamp-setup-tmp")),
        "a crashed write's temporary file holds the password: removed at start"
    );
}

#[test]
fn the_products_reset_wipes_the_link_where_the_product_writes_it() {
    let own: Config =
        serde_json::from_str(include_str!("../fixtures/serve-own-page.json")).unwrap();
    assert_eq!(
        own.backend,
        std::path::Path::new("/etc/lampOS/setup/backend.json")
    );
    let plan: Value =
        serde_json::from_str(include_str!("../fixtures/reset-plan-product.json")).unwrap();
    assert!(
        plan["wipe"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "/etc/lampOS/setup/backend.json"),
        "the 5 s reset removes the backend's credentials with the person's data"
    );
}

#[test]
fn a_named_but_broken_link_names_its_fields_and_never_their_values() {
    let mut body = app_body();
    body["mqtt_port"] = json!("abc");
    body["fd_channel"] = json!("");
    body["mqtt_password"] = json!("bad\u{7}pass");
    let fields = backend_link(&request(body)).unwrap_err();
    assert_eq!(fields, ["fd_channel", "mqtt_password", "mqtt_port"]);
    // No endpoint is no link at all, as from V1's page: nothing to name.
    assert!(backend_link(&request(good())).unwrap_err().is_empty());
}

#[test]
fn a_link_that_cannot_be_written_never_fails_setup() {
    let status = Arc::new(Mutex::new(Status::new("lamp-4ace".into())));
    let mut saved = Saved {
        fail_on: Some("backend"),
        ..Saved::default()
    };
    let accepted = check(&request(app_body()), &Root::new().config(false)).unwrap();
    run(
        &status,
        &accepted,
        "VN",
        &mut Lamp::new(true),
        &mut saved,
        || Some("192.168.1.23".into()),
    );
    let status = status.lock().unwrap().clone();
    assert_eq!(status.phase, Phase::Connected);
    assert!(status.set_up_completed);
    assert_eq!(
        saved.calls,
        [
            "name=Lan",
            "timezone=Asia/Ho_Chi_Minh",
            "credential=AIza-key_1",
            "marker="
        ]
    );
}

#[test]
fn a_null_or_a_number_in_a_link_field_never_fails_setup() {
    let root = Root::new();
    let config = root.config(false);
    for field in [
        "llm_base_url",
        "device_id",
        "mqtt_endpoint",
        "mqtt_username",
        "mqtt_password",
        "fa_channel",
        "fd_channel",
    ] {
        let mut body = app_body();
        body[field] = Value::Null;
        let accepted = check(&request(body), &config).unwrap();
        assert_eq!(accepted.ssid, "Home", "{field}: setup goes on");
        let mut body = app_body();
        body[field] = json!(42);
        assert!(
            check(&request(body), &config).is_ok(),
            "{field} as a number"
        );
    }
    // A numeric id is the id, as digits.
    let mut body = app_body();
    body["device_id"] = json!(42);
    assert_eq!(
        check(&request(body), &config)
            .unwrap()
            .backend
            .unwrap()
            .device_id,
        "42"
    );
    // A null endpoint is no link at all; a null channel is a broken one.
    let mut body = app_body();
    body["mqtt_endpoint"] = Value::Null;
    assert!(backend_link(&request(body)).unwrap_err().is_empty());
    let mut body = app_body();
    body["fd_channel"] = Value::Null;
    assert_eq!(backend_link(&request(body)).unwrap_err(), ["fd_channel"]);
}
