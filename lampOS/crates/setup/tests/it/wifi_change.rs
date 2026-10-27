//! The Wi-Fi changed after setup (#1959), against a fake network manager:
//! the networks it can see, what each gives, and a clock of its own.
use lamp_setup::wifi::Host;
use lamp_setup::wifi_change::{
    Asked, BACK_TRIES, BACK_WAIT, CONFIRM_WAIT, Changed, JOIN_WAIT, LOST_MOST, Mouth,
    RECORDING_LOST, RESTORE_MOST, Rejoin, Restored, SAY_BACK, SAY_JOINED, SAY_LOST, SayService,
    answer, change, line_for, parse, restore, say_after,
};
use serde_json::json;
use std::{collections::HashMap, io, time::Duration};

/// An obvious fake, never a real password.
const FAKE_PASSWORD: &str = "fake-pass-NEW-1234";
const OLD_PASSWORD: &str = "fake-pass-OLD-5678";
const BROKER: &str = "broker.test:1883";

/// What a network gives once its file is in place.
#[derive(Clone, Copy)]
struct Network {
    password: &'static str,
    /// An address from its router.
    address: bool,
    /// The broker reachable through it.
    broker: bool,
    /// It joins only from this many milliseconds after the radio's start.
    after_ms: u64,
    /// Its router is off until this time on the fake's clock (a restart);
    /// 0 when it is always on.
    back_at_ms: u64,
}

/// The fake: wpa_supplicant's file, the old one kept, the networks in range
/// and a clock that only the waits move.
struct Fake {
    conf: Option<Vec<u8>>,
    previous: Option<Vec<u8>>,
    networks: HashMap<String, Network>,
    /// What the radio joined at its last start, and when.
    joined: Option<(String, Network)>,
    wrong_key: bool,
    started_ms: u64,
    now_ms: u64,
    stations: u32,
    hotspots: u32,
    brokers: Vec<String>,
    /// The file as it was each time the radio started.
    radio: Vec<Vec<u8>>,
    /// `wpa_cli reconfigure`s asked.
    reloads: u32,
    /// Whether the app confirms a proven network (through it).
    app_confirms: bool,
    /// The confirmations asked for.
    asked_confirm: u32,
    /// The old network run from wpa_supplicant's memory, its file untouched.
    holds: u32,
    /// Holds that fail part-way, as the real one can (#2686's P2): its
    /// networks removed first, then a step refused or timed out, so
    /// wpa_supplicant is left holding no network.
    holds_failing: u32,
    /// When each hold was asked.
    hold_at_ms: Vec<u64>,
}

fn hex(text: &str) -> String {
    text.bytes().map(|b| format!("{b:02x}")).collect()
}

impl Fake {
    fn new(networks: &[(&str, Network)]) -> Self {
        let mut fake = Self {
            conf: Some(
                lamp_setup::wifi::wpa_conf("VN", "Old Home", OLD_PASSWORD)
                    .unwrap()
                    .into_bytes(),
            ),
            previous: None,
            networks: networks
                .iter()
                .map(|(name, network)| ((*name).to_owned(), *network))
                .collect(),
            joined: None,
            wrong_key: false,
            started_ms: 0,
            now_ms: 0,
            stations: 0,
            hotspots: 0,
            brokers: Vec::new(),
            radio: Vec::new(),
            reloads: 0,
            app_confirms: true,
            asked_confirm: 0,
            holds: 0,
            holds_failing: 0,
            hold_at_ms: Vec::new(),
        };
        fake.start_radio();
        fake.stations = 0;
        fake.radio.clear();
        fake
    }

    fn start_radio(&mut self) {
        self.stations += 1;
        self.load();
    }

    /// wpa_supplicant reading its file: the network it runs is the file's.
    fn load(&mut self) {
        let conf = self.conf.clone().unwrap_or_default();
        self.run(&conf);
    }

    /// wpa_supplicant running `conf`'s network, from its file or its memory.
    fn run(&mut self, conf: &[u8]) {
        self.started_ms = self.now_ms;
        self.wrong_key = false;
        self.joined = None;
        let conf = String::from_utf8(conf.to_vec()).unwrap();
        self.radio.push(conf.clone().into_bytes());
        for (name, network) in &self.networks {
            if conf.contains(&format!("ssid={}\n", hex(name))) {
                if conf.contains(&format!("psk=\"{}\"", network.password)) {
                    self.joined = Some((name.clone(), *network));
                } else {
                    self.wrong_key = true;
                }
            }
        }
    }

    fn up(&self) -> Option<&(String, Network)> {
        self.joined.as_ref().filter(|(_, network)| {
            self.now_ms >= self.started_ms + network.after_ms && self.now_ms >= network.back_at_ms
        })
    }
}

impl Host for Fake {
    fn read_conf(&mut self) -> io::Result<Option<Vec<u8>>> {
        Ok(self.conf.clone())
    }
    fn write_conf(&mut self, bytes: Option<&[u8]>) -> io::Result<()> {
        self.conf = bytes.map(<[u8]>::to_vec);
        Ok(())
    }
    fn station(&mut self) -> io::Result<()> {
        self.start_radio();
        Ok(())
    }
    fn hotspot(&mut self) -> io::Result<()> {
        self.hotspots += 1;
        Ok(())
    }
    fn internet(&mut self) -> bool {
        self.up().is_some_and(|(_, network)| network.broker)
    }
    fn current_ssid(&mut self) -> Option<String> {
        self.up().map(|(name, _)| name.clone())
    }
    fn has_station_ip(&mut self) -> bool {
        self.up().is_some_and(|(_, network)| network.address)
    }
    fn wpa_state(&mut self) -> String {
        if self.up().is_some() {
            "COMPLETED"
        } else {
            "SCANNING"
        }
        .into()
    }
    fn wpa_events(&mut self) -> Vec<String> {
        if self.wrong_key {
            vec![
                "wlan0: Trying to associate with aa (SSID='New Home')".into(),
                "wlan0: CTRL-EVENT-SSID-TEMP-DISABLED id=0 ssid=\"New Home\" auth_failures=2 duration=30 reason=WRONG_KEY".into(),
            ]
        } else if self.up().is_some() {
            vec!["wlan0: CTRL-EVENT-CONNECTED - Connection to aa completed".into()]
        } else {
            vec!["wlan0: Trying to associate with aa (SSID='New Home')".into()]
        }
    }
    fn now_ms(&mut self) -> u64 {
        self.now_ms
    }
    fn sleep(&mut self, duration: Duration) {
        self.now_ms += duration.as_millis() as u64;
    }
}

impl Rejoin for Fake {
    fn broker(&mut self, address: &str) -> bool {
        self.brokers.push(address.to_owned());
        self.up()
            .is_some_and(|(_, network)| network.address && network.broker)
    }
    fn keep_previous(&mut self, bytes: Option<&[u8]>) -> io::Result<()> {
        self.previous = bytes.map(<[u8]>::to_vec);
        Ok(())
    }
    fn read_previous(&mut self) -> io::Result<Option<Vec<u8>>> {
        Ok(self.previous.clone())
    }
    fn reload(&mut self) -> io::Result<()> {
        self.reloads += 1;
        self.load();
        Ok(())
    }
    fn app_confirms(&mut self, _ssid: &str, most: Duration) -> bool {
        self.asked_confirm += 1;
        // The app's confirmation comes through the new network, or never.
        let through = self
            .up()
            .is_some_and(|(_, network)| network.address && network.broker);
        if self.app_confirms && through {
            self.now_ms += 2_000;
            true
        } else {
            self.now_ms += most.as_millis() as u64;
            false
        }
    }
    fn hold(&mut self, conf: &[u8]) -> io::Result<()> {
        self.holds += 1;
        self.hold_at_ms.push(self.now_ms);
        if self.holds_failing > 0 {
            self.holds_failing -= 1;
            // `REMOVE_NETWORK all` was taken; `ADD_NETWORK` was not.
            self.started_ms = self.now_ms;
            self.wrong_key = false;
            self.joined = None;
            return Err(io::Error::other("wpa_supplicant refused a network"));
        }
        self.run(conf);
        Ok(())
    }
}

const GOOD: Network = Network {
    password: FAKE_PASSWORD,
    address: true,
    broker: true,
    after_ms: 8_000,
    back_at_ms: 0,
};
const OLD: Network = Network {
    password: OLD_PASSWORD,
    address: true,
    broker: true,
    after_ms: 5_000,
    back_at_ms: 0,
};

fn asked(ssid: &str) -> Asked {
    Asked {
        ssid: ssid.into(),
        password: FAKE_PASSWORD.into(),
        broker: BROKER.into(),
    }
}

fn old_conf() -> Vec<u8> {
    lamp_setup::wifi::wpa_conf("VN", "Old Home", OLD_PASSWORD)
        .unwrap()
        .into_bytes()
}

fn request(ssid: &str, password: &str) -> String {
    json!({"wifi": {"ssid": ssid, "password": password, "broker": BROKER}}).to_string()
}

#[test]
fn a_network_that_joins_and_reaches_the_broker_is_kept_and_the_old_file_goes() {
    let mut fake = Fake::new(&[("Old Home", OLD), ("New Home", GOOD)]);
    assert_eq!(change(&mut fake, "VN", &asked("New Home")), Changed::Joined);
    let conf = String::from_utf8(fake.conf.clone().unwrap()).unwrap();
    assert!(conf.contains(&format!("ssid={}\n", hex("New Home"))));
    assert_eq!(fake.stations, 1);
    assert_eq!(fake.hotspots, 0);
    assert_eq!(
        fake.previous, None,
        "the old file is removed once confirmed"
    );
    assert_eq!(fake.brokers.last().map(String::as_str), Some(BROKER));
    // Joined at 8 s, within the bound, and the app's confirmation 2 s later.
    assert!(fake.now_ms <= 11_000, "{}", fake.now_ms);
}

#[test]
fn the_old_file_is_kept_until_the_new_network_is_confirmed() {
    // A network that comes up only late: until it does, the old file is kept.
    let mut fake = Fake::new(&[
        ("Old Home", OLD),
        (
            "New Home",
            Network {
                after_ms: 59_000,
                ..GOOD
            },
        ),
    ]);
    struct Watch<'a>(&'a mut Fake, bool);
    impl Host for Watch<'_> {
        fn read_conf(&mut self) -> io::Result<Option<Vec<u8>>> {
            self.0.read_conf()
        }
        fn write_conf(&mut self, bytes: Option<&[u8]>) -> io::Result<()> {
            self.0.write_conf(bytes)
        }
        fn station(&mut self) -> io::Result<()> {
            self.0.station()
        }
        fn hotspot(&mut self) -> io::Result<()> {
            self.0.hotspot()
        }
        fn internet(&mut self) -> bool {
            self.0.internet()
        }
        fn current_ssid(&mut self) -> Option<String> {
            self.0.current_ssid()
        }
        fn has_station_ip(&mut self) -> bool {
            self.0.has_station_ip()
        }
        fn wpa_state(&mut self) -> String {
            self.0.wpa_state()
        }
        fn wpa_events(&mut self) -> Vec<String> {
            self.0.wpa_events()
        }
        fn now_ms(&mut self) -> u64 {
            self.0.now_ms()
        }
        fn sleep(&mut self, duration: Duration) {
            // While waiting for the new network, the old file is there.
            self.1 &= self.0.previous.as_deref() == Some(&old_conf()[..]);
            self.0.sleep(duration);
        }
    }
    impl Rejoin for Watch<'_> {
        fn broker(&mut self, address: &str) -> bool {
            self.0.broker(address)
        }
        fn keep_previous(&mut self, bytes: Option<&[u8]>) -> io::Result<()> {
            self.0.keep_previous(bytes)
        }
        fn read_previous(&mut self) -> io::Result<Option<Vec<u8>>> {
            self.0.read_previous()
        }
        fn reload(&mut self) -> io::Result<()> {
            self.0.reload()
        }
        fn app_confirms(&mut self, ssid: &str, most: Duration) -> bool {
            self.0.app_confirms(ssid, most)
        }
        fn hold(&mut self, conf: &[u8]) -> io::Result<()> {
            self.0.hold(conf)
        }
    }
    let mut watch = Watch(&mut fake, true);
    assert_eq!(
        change(&mut watch, "VN", &asked("New Home")),
        Changed::Joined
    );
    assert!(watch.1, "the old file was not kept while waiting");
    assert_eq!(fake.previous, None);
}

#[test]
fn a_wrong_password_goes_back_to_the_old_network_and_fails() {
    let mut fake = Fake::new(&[
        ("Old Home", OLD),
        (
            "New Home",
            Network {
                password: "another-fake-pass",
                ..GOOD
            },
        ),
    ]);
    let changed = change(&mut fake, "VN", &asked("New Home"));
    let Changed::Back { why } = changed else {
        panic!("{changed:?}");
    };
    assert_eq!(
        why,
        "wrong WiFi password: the router rejected the key; the lamp is back on its old Wi-Fi"
    );
    assert_eq!(fake.conf.as_deref(), Some(&old_conf()[..]));
    assert_eq!(fake.radio.last().unwrap(), &old_conf());
    assert_eq!(fake.stations, 2);
    assert_eq!(fake.hotspots, 0, "never the hotspot after setup");
    assert_eq!(fake.previous, None);
    // Stopped early on the key, well before the bound.
    assert!(
        fake.now_ms < JOIN_WAIT.as_millis() as u64,
        "{}",
        fake.now_ms
    );
}

#[test]
fn a_network_out_of_range_is_waited_for_the_bound_then_the_old_one_comes_back() {
    let mut fake = Fake::new(&[("Old Home", OLD)]);
    let changed = change(&mut fake, "VN", &asked("Far Away"));
    assert!(matches!(changed, Changed::Back { .. }), "{changed:?}");
    assert!(fake.now_ms >= JOIN_WAIT.as_millis() as u64);
    assert_eq!(fake.conf.as_deref(), Some(&old_conf()[..]));
}

#[test]
fn a_network_joined_without_the_broker_is_not_kept() {
    let mut fake = Fake::new(&[
        ("Old Home", OLD),
        (
            "Cafe Portal",
            Network {
                broker: false,
                ..GOOD
            },
        ),
    ]);
    let changed = change(&mut fake, "VN", &asked("Cafe Portal"));
    assert_eq!(
        changed,
        Changed::Back {
            why: "connected to the new WiFi, but the lamp's server could not be reached; the lamp is back on its old Wi-Fi".into()
        }
    );
    assert_eq!(fake.conf.as_deref(), Some(&old_conf()[..]));
    // And one with no address from its router.
    let mut fake = Fake::new(&[
        ("Old Home", OLD),
        (
            "No Address",
            Network {
                address: false,
                ..GOOD
            },
        ),
    ]);
    let changed = change(&mut fake, "VN", &asked("No Address"));
    assert!(matches!(changed, Changed::Back { .. }), "{changed:?}");
    assert_eq!(fake.conf.as_deref(), Some(&old_conf()[..]));
}

#[test]
fn an_old_network_that_does_not_come_back_is_kept_and_tried_never_the_hotspot() {
    // The old router is off now: the new one fails, and so does the old.
    let mut fake = Fake::new(&[(
        "New Home",
        Network {
            broker: false,
            ..GOOD
        },
    )]);
    let started = fake.now_ms;
    let changed = change(&mut fake, "VN", &asked("New Home"));
    let Changed::NotBack { why } = changed else {
        panic!("{changed:?}");
    };
    assert!(
        why.ends_with(
            "the old Wi-Fi did not come back after 3 tries, and the lamp keeps trying it"
        ),
        "{why}"
    );
    assert_eq!(fake.conf.as_deref(), Some(&old_conf()[..]));
    assert_eq!(fake.hotspots, 0);
    let bound = (JOIN_WAIT + BACK_WAIT * BACK_TRIES).as_millis() as u64 + 5_000;
    assert!(fake.now_ms - started <= bound, "{}", fake.now_ms);
}

#[test]
fn a_lamp_with_no_wifi_file_is_refused_and_nothing_changes() {
    let mut fake = Fake::new(&[("New Home", GOOD)]);
    fake.conf = None;
    assert_eq!(
        change(&mut fake, "VN", &asked("New Home")),
        Changed::Refused("the lamp has no Wi-Fi to come back to".into())
    );
    assert_eq!(fake.stations, 0);
    assert_eq!(fake.conf, None);
}

#[test]
fn a_request_is_checked_with_setups_own_words() {
    assert_eq!(
        parse(&request("New Home", FAKE_PASSWORD), "VN"),
        Ok(asked("New Home"))
    );
    for (line, why) in [
        (request("", FAKE_PASSWORD), "choose a Wi-Fi network"),
        (
            request(&"x".repeat(33), FAKE_PASSWORD),
            "the Wi-Fi name is longer than 32 bytes",
        ),
        (
            request("New Home", "short"),
            "a Wi-Fi password has 8 to 63 characters",
        ),
        (
            request("New Home", "fake-pass\nline"),
            "the Wi-Fi password has a character the lamp cannot use",
        ),
        (
            json!({"wifi": {"ssid": "New Home", "password": FAKE_PASSWORD, "broker": "nowhere"}})
                .to_string(),
            "the lamp is not on its broker",
        ),
        (
            json!({"timezone": "Europe/Paris"}).to_string(),
            "the request names no change this lamp makes",
        ),
        ("not json".to_owned(), "the request is not JSON"),
    ] {
        assert_eq!(parse(&line, "VN"), Err(why.to_owned()), "{line}");
    }
    assert!(!format!("{:?}", asked("New Home")).contains(FAKE_PASSWORD));
}

/// The networks in range, the request line, and whether the switch is on.
type Case = (Vec<(&'static str, Network)>, String, bool);

/// Every reply and every journal line, for every outcome: none holds the
/// password, and the journal holds no network name either.
#[test]
fn the_password_is_in_no_reply_error_or_journal_line() {
    let new_ok = ("New Home", GOOD);
    let wrong = (
        "New Home",
        Network {
            password: "another-fake-pass",
            ..GOOD
        },
    );
    let no_broker = (
        "New Home",
        Network {
            broker: false,
            ..GOOD
        },
    );
    let cases: Vec<Case> = vec![
        (
            vec![("Old Home", OLD), new_ok],
            request("New Home", FAKE_PASSWORD),
            true,
        ),
        (
            vec![("Old Home", OLD), wrong],
            request("New Home", FAKE_PASSWORD),
            true,
        ),
        (
            vec![("Old Home", OLD), no_broker],
            request("New Home", FAKE_PASSWORD),
            true,
        ),
        (vec![no_broker], request("New Home", FAKE_PASSWORD), true),
        (
            vec![("Old Home", OLD)],
            request("New Home", FAKE_PASSWORD),
            false,
        ),
        (
            vec![("Old Home", OLD)],
            request("New Home", &format!("{FAKE_PASSWORD}\u{7}")),
            true,
        ),
        (
            vec![("Old Home", OLD)],
            request("New Home", "fake-pw"),
            true,
        ),
    ];
    for (networks, line, enabled) in cases {
        let mut fake = Fake::new(&networks);
        let answered = answer(&line, "VN", enabled, &mut fake);
        let (text, said) = (answered.reply.to_string(), answered.said);
        for secret in [FAKE_PASSWORD, "fake-pw", OLD_PASSWORD] {
            assert!(!text.contains(secret), "{text}");
            assert!(!said.contains(secret), "{said}");
        }
        assert!(
            !said.contains("New Home") && !said.contains("Old Home"),
            "{said}"
        );
    }
}

#[test]
fn the_replies_say_done_with_the_name_or_why_not() {
    let mut fake = Fake::new(&[("Old Home", OLD), ("New Home", GOOD)]);
    let answered = answer(&request("New Home", FAKE_PASSWORD), "VN", true, &mut fake);
    let (reply, said) = (answered.reply, answered.said);
    assert_eq!(answered.changed, Some((Changed::Joined, BROKER.to_owned())));
    assert_eq!(reply, json!({"done": {"ssid": "New Home"}}));
    assert_eq!(
        said,
        "the Wi-Fi was changed, to a name of 8 bytes, and the broker reached"
    );
    let mut fake = Fake::new(&[("Old Home", OLD)]);
    let reply = answer(&request("New Home", FAKE_PASSWORD), "VN", true, &mut fake).reply;
    assert!(
        reply["refused"]
            .as_str()
            .unwrap()
            .ends_with("the lamp is back on its old Wi-Fi")
    );
}

#[test]
fn with_the_switch_off_every_request_is_refused_and_the_radio_untouched() {
    let mut fake = Fake::new(&[("Old Home", OLD), ("New Home", GOOD)]);
    let answered = answer(&request("New Home", FAKE_PASSWORD), "VN", false, &mut fake);
    let (reply, said) = (answered.reply, answered.said);
    assert_eq!(answered.changed, None);
    assert_eq!(
        reply,
        json!({"refused": "Wi-Fi changes after setup are off"})
    );
    assert_eq!(said, "refused: Wi-Fi changes after setup are off");
    assert_eq!(fake.stations, 0);
    assert_eq!(fake.conf.as_deref(), Some(&old_conf()[..]));
}

/// The voice's say service, as heard.
#[derive(Default)]
struct Heard(Vec<(u64, String)>, u64);

/// A recording is noted as `played NAME`: never words for the model.
impl Mouth for Heard {
    fn say(&mut self, text: &str) {
        self.0.push((self.1, text.to_owned()));
    }
    fn play(&mut self, recording: &str) -> bool {
        self.0.push((self.1, format!("played {recording}")));
        true
    }
}

/// A mouth that notes the fake's clock as each line is asked.
struct Timed<'a> {
    heard: Vec<(u64, String)>,
    clock: &'a std::cell::Cell<u64>,
}

impl Mouth for Timed<'_> {
    fn say(&mut self, text: &str) {
        self.heard.push((self.clock.get(), text.to_owned()));
    }
    fn play(&mut self, recording: &str) -> bool {
        self.heard
            .push((self.clock.get(), format!("played {recording}")));
        true
    }
}

#[test]
fn the_lamp_says_what_happened_in_a_few_words_and_never_the_name() {
    assert_eq!(line_for(&Changed::Joined), Some(SAY_JOINED));
    assert_eq!(
        line_for(&Changed::Back { why: String::new() }),
        Some(SAY_BACK)
    );
    assert_eq!(
        line_for(&Changed::NotBack { why: String::new() }),
        Some(SAY_LOST)
    );
    assert_eq!(line_for(&Changed::Refused(String::new())), None);
    assert_eq!(SAY_JOINED, "I'm on the new Wi-Fi now.");
    assert_eq!(
        SAY_BACK,
        "I couldn't join that one, so I've gone back to the old Wi-Fi."
    );
    assert_eq!(
        SAY_LOST,
        "I can't reach any Wi-Fi. If you changed your Wi-Fi's name or password, push a paperclip into the hole marked reset on the back of my base and hold it for five seconds. That erases me and starts setup again."
    );
    // After a real change and a real fall-back: one line each, no name.
    for (networks, want) in [
        (vec![("Old Home", OLD), ("New Home", GOOD)], SAY_JOINED),
        (
            vec![
                ("Old Home", OLD),
                (
                    "New Home",
                    Network {
                        password: "another-fake-pass",
                        ..GOOD
                    },
                ),
            ],
            SAY_BACK,
        ),
    ] {
        let mut fake = Fake::new(&networks);
        let answered = answer(&request("New Home", FAKE_PASSWORD), "VN", true, &mut fake);
        let (changed, broker) = answered.changed.unwrap();
        let mut heard = Heard::default();
        assert_eq!(say_after(&mut fake, &mut heard, &changed, &broker), 1);
        assert_eq!(heard.0.len(), 1);
        assert_eq!(heard.0[0].1, want);
        for (_, line) in &heard.0 {
            assert!(!line.contains("Home") && !line.contains(FAKE_PASSWORD));
        }
    }
}

#[test]
fn with_no_wifi_back_the_lost_line_is_said_at_most_once_a_minute_until_one_is() {
    // Neither network: the old router comes back 150 s after the fall-back.
    let mut fake = Fake::new(&[(
        "Old Home",
        Network {
            after_ms: 150_000,
            ..OLD
        },
    )]);
    let started = fake.now_ms;
    let clock = std::cell::Cell::new(0);
    struct Clocked<'a>(&'a mut Fake, &'a std::cell::Cell<u64>);
    impl Host for Clocked<'_> {
        fn read_conf(&mut self) -> io::Result<Option<Vec<u8>>> {
            self.0.read_conf()
        }
        fn write_conf(&mut self, bytes: Option<&[u8]>) -> io::Result<()> {
            self.0.write_conf(bytes)
        }
        fn station(&mut self) -> io::Result<()> {
            self.0.station()
        }
        fn hotspot(&mut self) -> io::Result<()> {
            self.0.hotspot()
        }
        fn internet(&mut self) -> bool {
            self.0.internet()
        }
        fn current_ssid(&mut self) -> Option<String> {
            self.0.current_ssid()
        }
        fn has_station_ip(&mut self) -> bool {
            self.0.has_station_ip()
        }
        fn wpa_state(&mut self) -> String {
            self.0.wpa_state()
        }
        fn wpa_events(&mut self) -> Vec<String> {
            self.0.wpa_events()
        }
        fn now_ms(&mut self) -> u64 {
            self.0.now_ms()
        }
        fn sleep(&mut self, duration: Duration) {
            self.0.sleep(duration);
            self.1.set(self.0.now_ms);
        }
    }
    impl Rejoin for Clocked<'_> {
        fn broker(&mut self, address: &str) -> bool {
            self.0.broker(address)
        }
        fn keep_previous(&mut self, bytes: Option<&[u8]>) -> io::Result<()> {
            self.0.keep_previous(bytes)
        }
        fn read_previous(&mut self) -> io::Result<Option<Vec<u8>>> {
            self.0.read_previous()
        }
        fn reload(&mut self) -> io::Result<()> {
            self.0.reload()
        }
        fn app_confirms(&mut self, ssid: &str, most: Duration) -> bool {
            self.0.app_confirms(ssid, most)
        }
        fn hold(&mut self, conf: &[u8]) -> io::Result<()> {
            self.0.hold(conf)
        }
    }
    let mut host = Clocked(&mut fake, &clock);
    // The old network comes back 150 s after the radio's last start.
    host.0.started_ms = started;
    let mut mouth = Timed {
        heard: Vec::new(),
        clock: &clock,
    };
    let changed = Changed::NotBack { why: String::new() };
    let said = say_after(&mut host, &mut mouth, &changed, BROKER);
    let at: Vec<u64> = mouth.heard.iter().map(|(at, _)| *at - started).collect();
    assert_eq!(said, 3, "{at:?}");
    // Every time the recording, never the model's words (#1959).
    assert!(
        mouth
            .heard
            .iter()
            .all(|(_, line)| line == "played wifi_lost")
    );
    for pair in at.windows(2) {
        assert!(pair[1] - pair[0] >= 60_000, "{at:?}");
    }
    // And it stops at the bound when nothing ever comes back.
    let mut fake = Fake::new(&[]);
    let started = fake.now_ms;
    let mut heard = Heard::default();
    let said = say_after(&mut fake, &mut heard, &changed, BROKER);
    assert_eq!(said as u64, LOST_MOST.as_secs() / 60);
    assert_eq!(heard.0.len() as u64, LOST_MOST.as_secs() / 60);
    assert!(heard.0.iter().all(|(_, line)| line == "played wifi_lost"));
    assert!(fake.now_ms - started <= LOST_MOST.as_millis() as u64 + 5_000);
}

/// #3743: a router's restart (2-3 minutes) never hears the no-Wi-Fi line.
/// The old router restarts as the change begins and is back 2.5 minutes
/// later; the new network's key is wrong, so the lamp is back to waiting for
/// the old one within seconds, the worst case. It hears only that the change
/// was not taken. And when the old router never comes back, the line is
/// first said 3 minutes at least after the lamp left it.
#[test]
fn a_router_restart_of_two_and_a_half_minutes_never_hears_the_lost_line() {
    let wrong_key = Network {
        password: "another-fake-pass",
        ..GOOD
    };
    let restarting = Network {
        back_at_ms: 150_000,
        ..OLD
    };
    let mut fake = Fake::new(&[("Old Home", restarting), ("New Home", wrong_key)]);
    let changed = change(&mut fake, "VN", &asked("New Home"));
    assert!(matches!(changed, Changed::Back { .. }), "{changed:?}");
    let mut heard = Heard::default();
    assert_eq!(say_after(&mut fake, &mut heard, &changed, BROKER), 1);
    assert_eq!(heard.0.len(), 1);
    assert_eq!(heard.0[0].1, SAY_BACK);

    // Never back: the first lost line no sooner than 3 minutes on.
    let gone = Network {
        back_at_ms: u64::MAX,
        ..OLD
    };
    let mut fake = Fake::new(&[("Old Home", gone), ("New Home", wrong_key)]);
    let left = fake.now_ms;
    let changed = change(&mut fake, "VN", &asked("New Home"));
    assert!(matches!(changed, Changed::NotBack { .. }), "{changed:?}");
    let first = fake.now_ms - left;
    assert!(first >= 180_000, "first said {first} ms after the loss");
    let mut heard = Heard::default();
    let said = say_after(&mut fake, &mut heard, &changed, BROKER);
    assert!(said >= 1);
    assert_eq!(said as u64, LOST_MOST.as_secs() / 60);
}

#[test]
fn the_helpers_units_are_its_own_one_change_at_a_time() {
    let unit = include_str!("../../../../deploy/product/lamp-setup-wifi@.service");
    assert!(unit.contains(
        "ExecStart=/opt/lampOS/setup/current/lamp-setup wifi --config /etc/lampOS/setup/serve.json --enable\n"
    ));
    assert!(unit.contains("StandardInput=socket\nStandardOutput=socket\n"));
    assert!(!unit.contains("PrivateNetwork=yes"));
    for line in [
        "NoNewPrivileges=yes\n",
        "ProtectSystem=strict\n",
        "ReadWritePaths=/etc /run /tmp\n",
        "ProtectHome=yes\n",
        "PrivateTmp=no\n",
        "RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6 AF_NETLINK\n",
    ] {
        assert!(unit.contains(line), "{line}");
    }
    let socket = include_str!("../../../../deploy/product/lamp-setup-wifi.socket");
    assert!(socket.contains("ListenStream=/run/lampos-setup-wifi/wifi.sock\n"));
    assert!(socket.contains("SocketGroup=lampos\nSocketMode=0660\n"));
    assert!(socket.contains("Accept=yes\nMaxConnections=1\n"));
}

/// A say socket that reads one ask and answers `answers`, line by line;
/// the ask comes back from the thread.
fn say_socket(tag: &str, answers: &'static str) -> (SayService, std::thread::JoinHandle<String>) {
    use std::io::{BufRead, BufReader, Write};
    let dir = std::path::PathBuf::from(format!("/tmp/lsw-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("say.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let served = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut asked = String::new();
        BufReader::new(&stream).read_line(&mut asked).unwrap();
        stream.write_all(answers.as_bytes()).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        asked
    });
    (SayService { socket }, served)
}

/// #1959: with no Wi-Fi the lost line is the voice's recording, asked by
/// name, never words for the model; one not there is said in the journal,
/// with the voice's reason, and nothing is heard.
#[test]
fn the_lost_line_is_the_recording_and_one_missing_is_journalled_not_said() {
    assert_eq!(RECORDING_LOST, "wifi_lost");
    let (mut say, served) = say_socket(
        "missing",
        "{\"outcome\":\"refused\",\"why\":\"the recording is not there: not_provisioned\"}\n",
    );
    assert_eq!(
        say.play_recording(RECORDING_LOST),
        Some(
            "the recording wifi_lost was not played: refused, the recording is not there: \
             not_provisioned"
                .into()
        )
    );
    let asked: serde_json::Value = serde_json::from_str(&served.join().unwrap()).unwrap();
    assert_eq!(
        asked,
        json!({"skill":"wifi","recording":"wifi_lost","priority":7,"expires_in_ms":60_000})
    );
    assert!(asked.get("text").is_none() && asked.get("note").is_none());
    // Begun: nothing to journal.
    let (say_begun, served) = say_socket(
        "begun",
        "{\"event\":\"begun\",\"at_us\":5}\n{\"outcome\":\"spoken\"}\n",
    );
    assert_eq!(say_begun.play_recording(RECORDING_LOST), None);
    served.join().unwrap();
    // No say service at all: journalled, nothing said.
    say.socket = "/tmp/lsw-nobody-here/say.sock".into();
    assert_eq!(
        say.play_recording(RECORDING_LOST),
        Some("the voice's say service is not there; the recording wifi_lost was not played".into())
    );
    // The other two lines stay words for the model, said while online.
    for changed in [Changed::Joined, Changed::Back { why: String::new() }] {
        let mut fake = Fake::new(&[("Old Home", OLD)]);
        let mut heard = Heard::default();
        assert_eq!(say_after(&mut fake, &mut heard, &changed, BROKER), 1);
        assert_eq!(heard.0[0].1, line_for(&changed).unwrap());
        assert!(!heard.0[0].1.starts_with("played"));
    }
}

/// #1959: the product's voice loads the recorded lines from one fixed
/// folder, in one language, so the no-Wi-Fi line can be played there.
#[test]
fn the_product_voice_loads_its_recorded_lines_from_the_fixed_folder() {
    let unit = include_str!("../../../../deploy/product/lamp-voice.service");
    let start = unit
        .lines()
        .find(|line| line.starts_with("ExecStart="))
        .unwrap();
    assert!(
        start.ends_with(
            " --failure-notice-dir /opt/lampOS/voice/notices --failure-notice-language en-US"
        ),
        "{start}"
    );
    assert!(unit.contains("\nEnvironment=LAMP_SAY=on\n"));
}

/// #1959's P1 (lamp-0c89, build 4814): a wrong password left the lamp off
/// every network. The fake as above, with what a real lamp can do after the
/// helper has decided: the radio's script failing on the way back, the
/// network dropping right after it was confirmed, the helper cut mid-change.
struct Twist<'a> {
    fake: &'a mut Fake,
    /// The radio's start, counted from 1, that fails, as `device-sta-mode`
    /// under `set -e` does: the radio is not restarted, so wpa_supplicant
    /// keeps the network it last loaded.
    fail_station: Option<u32>,
    station_calls: u32,
    /// The new network goes away right after the broker was first reached.
    drop_after_confirm: bool,
    dropped: bool,
    /// The helper is cut here (killed, a power cut): its next wait panics.
    cut_at_ms: Option<u64>,
    /// The radio's start, counted from 1, that hangs until its bound
    /// (`wifi::RADIO_TIMEOUT`) and is killed there, the radio untouched.
    hang_station: Option<u32>,
    /// `wpa_cli reconfigure`s that fail before one is taken.
    reloads_failing: u32,
    /// Every radio start fails from here on (the first builder's P3 1).
    stations_failing: bool,
    /// The write of the file, counted from 1, that fails (P3 2).
    fail_write: Option<u32>,
    write_calls: u32,
    /// Removals of `.lamp-previous` that fail before one is taken (P3 3).
    removes_failing: u32,
    /// `.lamp-previous` cannot be read (P3 5).
    previous_unreadable: bool,
    /// Every write of the file from this one, counted from 1, fails.
    writes_failing_from: Option<u32>,
    /// An address is always there, whatever network is joined: one left
    /// from the network before, or another's.
    address_always: bool,
    /// Every radio start hangs until its bound.
    stations_hanging: bool,
}

impl<'a> Twist<'a> {
    fn new(fake: &'a mut Fake) -> Self {
        Self {
            fake,
            fail_station: None,
            station_calls: 0,
            drop_after_confirm: false,
            dropped: false,
            cut_at_ms: None,
            hang_station: None,
            reloads_failing: 0,
            stations_failing: false,
            fail_write: None,
            write_calls: 0,
            removes_failing: 0,
            previous_unreadable: false,
            writes_failing_from: None,
            address_always: false,
            stations_hanging: false,
        }
    }
}

impl Host for Twist<'_> {
    fn read_conf(&mut self) -> io::Result<Option<Vec<u8>>> {
        self.fake.read_conf()
    }
    fn write_conf(&mut self, bytes: Option<&[u8]>) -> io::Result<()> {
        self.write_calls += 1;
        if self.fail_write == Some(self.write_calls)
            || self
                .writes_failing_from
                .is_some_and(|from| self.write_calls >= from)
        {
            return Err(io::Error::other("no space left on device"));
        }
        self.fake.write_conf(bytes)
    }
    fn station(&mut self) -> io::Result<()> {
        self.station_calls += 1;
        if self.stations_failing || self.fail_station == Some(self.station_calls) {
            return Err(io::Error::other(
                "device-sta-mode ended with exit status: 1",
            ));
        }
        if self.stations_hanging || self.hang_station == Some(self.station_calls) {
            self.fake.now_ms += lamp_setup::wifi::RADIO_TIMEOUT.as_millis() as u64;
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "device-sta-mode hung",
            ));
        }
        self.fake.station()
    }
    fn hotspot(&mut self) -> io::Result<()> {
        self.fake.hotspot()
    }
    fn internet(&mut self) -> bool {
        !self.dropped && self.fake.internet()
    }
    fn current_ssid(&mut self) -> Option<String> {
        if self.dropped {
            return None;
        }
        self.fake.current_ssid()
    }
    fn has_station_ip(&mut self) -> bool {
        self.address_always || (!self.dropped && self.fake.has_station_ip())
    }
    fn wpa_state(&mut self) -> String {
        self.fake.wpa_state()
    }
    fn wpa_events(&mut self) -> Vec<String> {
        self.fake.wpa_events()
    }
    fn now_ms(&mut self) -> u64 {
        self.fake.now_ms()
    }
    fn sleep(&mut self, duration: Duration) {
        if self.cut_at_ms.is_some_and(|at| self.fake.now_ms >= at) {
            panic!("the helper was cut mid-change");
        }
        self.fake.sleep(duration);
    }
}

impl Rejoin for Twist<'_> {
    fn broker(&mut self, address: &str) -> bool {
        if self.dropped {
            return false;
        }
        let reached = self.fake.broker(address);
        if reached && self.drop_after_confirm {
            self.dropped = true;
        }
        reached
    }
    fn keep_previous(&mut self, bytes: Option<&[u8]>) -> io::Result<()> {
        if bytes.is_none() && self.removes_failing > 0 {
            self.removes_failing -= 1;
            return Err(io::Error::other("read-only file system"));
        }
        self.fake.keep_previous(bytes)
    }
    fn read_previous(&mut self) -> io::Result<Option<Vec<u8>>> {
        if self.previous_unreadable {
            return Err(io::Error::other("input/output error"));
        }
        self.fake.read_previous()
    }
    fn reload(&mut self) -> io::Result<()> {
        if self.reloads_failing > 0 {
            self.reloads_failing -= 1;
            return Err(io::Error::other("wpa_cli reconfigure was not taken"));
        }
        self.fake.reload()
    }
    fn app_confirms(&mut self, ssid: &str, most: Duration) -> bool {
        if self.dropped {
            self.fake.now_ms += most.as_millis() as u64;
            return false;
        }
        self.fake.app_confirms(ssid, most)
    }
    fn hold(&mut self, conf: &[u8]) -> io::Result<()> {
        self.fake.hold(conf)
    }
}

fn wrong_password_fake() -> Fake {
    Fake::new(&[
        ("Old Home", OLD),
        (
            "New Home",
            Network {
                password: "another-fake-pass",
                ..GOOD
            },
        ),
    ])
}

#[test]
fn p1_a_radio_restart_that_fails_on_the_way_back_is_tried_again_until_the_old_network_is_back() {
    // The join's restart works; the way back's (the 2nd) fails once, as
    // `device-sta-mode` exits under `set -e` (e.g. `ip link set wlan0 up`).
    let mut fake = wrong_password_fake();
    let mut twist = Twist::new(&mut fake);
    twist.fail_station = Some(2);
    let changed = change(&mut twist, "VN", &asked("New Home"));
    assert!(
        matches!(changed, Changed::Back { .. }),
        "a failed restart on the way back is not tried again: {changed:?}"
    );
    assert_eq!(
        fake.radio.last().unwrap(),
        &old_conf(),
        "the radio runs the old network"
    );
}

#[test]
fn p1_a_network_lost_right_after_it_was_confirmed_comes_back_to_the_old_one() {
    // Confirmed at the decision point (name, address, broker once), then gone:
    // the app has not confirmed it, so the old network must still be there.
    let mut fake = Fake::new(&[("Old Home", OLD), ("New Home", GOOD)]);
    let mut twist = Twist::new(&mut fake);
    twist.drop_after_confirm = true;
    let changed = change(&mut twist, "VN", &asked("New Home"));
    assert_ne!(
        changed,
        Changed::Joined,
        "a network confirmed once and then lost is kept, and the old file deleted"
    );
    assert_eq!(fake.conf.as_deref(), Some(&old_conf()[..]));
}

#[test]
fn p1_a_helper_cut_mid_change_leaves_the_old_network_to_boot_on() {
    // Cut 1 s into the join (killed, or the power cut the held lamp needed):
    // nothing after that runs, so what is on disk is what the lamp boots on.
    let mut fake = wrong_password_fake();
    let mut twist = Twist::new(&mut fake);
    twist.cut_at_ms = Some(1_000);
    let cut = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        change(&mut twist, "VN", &asked("New Home"))
    }));
    assert!(cut.is_err(), "the helper was not cut");
    // What boots next: lamp-setup-wifi-restore.service, before wpa_supplicant.
    let stations = fake.stations;
    assert_eq!(restore(&mut fake, false), Restored::Put);
    assert_eq!(
        fake.conf.as_deref(),
        Some(&old_conf()[..]),
        "the unverified new network is the file the lamp boots on"
    );
    assert_eq!(fake.previous, None);
    assert_eq!(fake.stations, stations, "at boot only the file");
    assert_eq!(fake.reloads, 0);
}

#[test]
fn p1_a_radio_script_that_hangs_is_timed_out_and_the_radio_tried_again() {
    // The way back's first restart hangs (a `systemctl restart` stuck) and
    // wpa_supplicant's control socket refuses the first reconfigure: killed
    // at its bound, then tried again, and the old network comes back.
    let mut fake = wrong_password_fake();
    let mut twist = Twist::new(&mut fake);
    twist.hang_station = Some(2);
    twist.reloads_failing = 1;
    let changed = change(&mut twist, "VN", &asked("New Home"));
    assert!(matches!(changed, Changed::Back { .. }), "{changed:?}");
    assert!(
        twist.station_calls >= 3,
        "not tried again: {}",
        twist.station_calls
    );
    assert_eq!(fake.radio.last().unwrap(), &old_conf());
    assert_eq!(fake.previous, None);
}

/// And the real bound: a command that hangs is killed there.
#[test]
fn p1_a_command_that_hangs_is_killed_at_its_bound() {
    let started = std::time::Instant::now();
    let mut command = std::process::Command::new("sleep");
    command.arg("30");
    let ran = lamp_setup::wifi::within(&mut command, Duration::from_millis(300));
    assert_eq!(ran.unwrap_err().kind(), io::ErrorKind::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(5));
    let mut command = std::process::Command::new("echo");
    command.arg("OK");
    let (status, out) = lamp_setup::wifi::within(&mut command, Duration::from_secs(5)).unwrap();
    assert!(status.success());
    assert_eq!(out, b"OK\n");
}

#[test]
fn p1_a_proven_network_the_app_does_not_confirm_within_120_s_goes_back() {
    // Joined, an address, the broker reached: but the app closed, the phone
    // gone. Back as if the join had failed.
    let mut fake = Fake::new(&[("Old Home", OLD), ("New Home", GOOD)]);
    fake.app_confirms = false;
    let started = fake.now_ms;
    let changed = change(&mut fake, "VN", &asked("New Home"));
    let Changed::Back { why } = changed else {
        panic!("{changed:?}");
    };
    assert_eq!(
        why,
        "the app did not confirm the new Wi-Fi within 120 s; the lamp is back on its old Wi-Fi"
    );
    assert_eq!(fake.asked_confirm, 1);
    assert!(fake.now_ms - started >= CONFIRM_WAIT.as_millis() as u64);
    assert_eq!(fake.conf.as_deref(), Some(&old_conf()[..]));
    assert_eq!(fake.radio.last().unwrap(), &old_conf());
    assert_eq!(fake.previous, None);
    // And with it: kept, the old file gone only then.
    let mut fake = Fake::new(&[("Old Home", OLD), ("New Home", GOOD)]);
    assert_eq!(change(&mut fake, "VN", &asked("New Home")), Changed::Joined);
    assert_eq!(fake.asked_confirm, 1);
    assert_eq!(fake.previous, None);
}

#[test]
fn p1_the_helpers_stop_puts_a_cut_changes_old_network_back_live() {
    // Killed while waiting for the new network (RuntimeMaxSec, a crash):
    // ExecStopPost's `wifi-restore` writes the old file, wpa_supplicant
    // re-reads it, the radio restarts.
    let mut fake = wrong_password_fake();
    let mut twist = Twist::new(&mut fake);
    twist.cut_at_ms = Some(1_000);
    let cut = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        change(&mut twist, "VN", &asked("New Home"))
    }));
    assert!(cut.is_err());
    assert_eq!(fake.previous.as_deref(), Some(&old_conf()[..]));
    let stations = fake.stations;
    assert_eq!(restore(&mut fake, true), Restored::Put);
    assert_eq!(fake.conf.as_deref(), Some(&old_conf()[..]));
    assert_eq!(fake.reloads, 1);
    assert_eq!(fake.stations, stations + 1);
    assert_eq!(fake.radio.last().unwrap(), &old_conf());
    assert_eq!(fake.previous, None);
    // A change that ended left nothing: the stop after it changes nothing.
    let mut fake = wrong_password_fake();
    let _ = change(&mut fake, "VN", &asked("New Home"));
    let (stations, reloads) = (fake.stations, fake.reloads);
    assert_eq!(restore(&mut fake, true), Restored::Nothing);
    assert_eq!((fake.stations, fake.reloads), (stations, reloads));
}

#[test]
fn p1_the_units_restore_at_the_helpers_stop_and_at_boot() {
    let unit = include_str!("../../../../deploy/product/lamp-setup-wifi@.service");
    assert!(unit.contains("\nExecStopPost=/opt/lampOS/setup/current/lamp-setup wifi-restore\n"));
    assert!(unit.contains("\nRuntimeMaxSec=25min\n"));
    // ExecStopPost's restore ends within RESTORE_MOST (the first builder's
    // follow-up 4): systemd's default 90 s would cut it, 8 min held a
    // shutdown for minutes.
    assert!(unit.contains("\nTimeoutStopSec=2min\n"));
    assert!(RESTORE_MOST + Duration::from_secs(25) <= Duration::from_secs(120));
    let boot = include_str!("../../../../deploy/product/lamp-setup-wifi-restore.service");
    for line in [
        "\nDefaultDependencies=no\n",
        // dhcpcd's wpa_supplicant hook (device-sta-mode drops `nohook
        // wpa_supplicant`) would start one on the new file (P3 4).
        "\nBefore=wpa_supplicant@wlan0.service dhcpcd.service network-pre.target\n",
        "\nConditionPathExists=/etc/wpa_supplicant/wpa_supplicant-wlan0.conf.lamp-previous\n",
        "\nType=oneshot\n",
        "\nExecStart=/opt/lampOS/setup/current/lamp-setup wifi-restore --boot\n",
        "\nWantedBy=multi-user.target\n",
    ] {
        assert!(boot.contains(line), "{line}");
    }
}

#[test]
fn p1_a_lost_line_the_voice_lacks_is_said_in_plain_words() {
    // The wifi_lost clip is not rendered yet: the voice refuses the
    // recording, so the same words go as text.
    struct NoClip(Vec<String>);
    impl Mouth for NoClip {
        fn say(&mut self, text: &str) {
            self.0.push(text.to_owned());
        }
        fn play(&mut self, recording: &str) -> bool {
            self.0.push(format!("asked {recording}"));
            false
        }
    }
    let mut fake = Fake::new(&[]);
    let mut mouth = NoClip(Vec::new());
    let changed = Changed::NotBack { why: String::new() };
    assert!(say_after(&mut fake, &mut mouth, &changed, BROKER) >= 1);
    assert_eq!(mouth.0[0], format!("asked {RECORDING_LOST}"));
    assert_eq!(mouth.0[1], SAY_LOST);
}

/// The reviewer-lead's P2 and P3 on #2617: the helper's confirmation window
/// is the one that counts, and it starts only once `lamp-cloud` says its
/// link is up again over the new network. `lamp-cloud`'s side of the
/// helper's socket, the confirmation sent `confirm_at` after it says so; whether the helper kept the network, what it said, and how long
/// after the window line it decided.
fn confirmed_with(
    window: Duration,
    confirm_at: Duration,
) -> (bool, Vec<serde_json::Value>, Duration) {
    use std::io::{BufRead, BufReader, Write};
    let (helper_end, mut cloud) = std::os::unix::net::UnixStream::pair().unwrap();
    let reader = BufReader::new(helper_end.try_clone().unwrap());
    let helper = std::thread::spawn(move || {
        let mut out = helper_end;
        let kept = lamp_setup::wifi_change::confirmed_on(
            reader,
            &mut out,
            "New Home",
            Duration::from_secs(5),
            window,
        );
        (kept, std::time::Instant::now())
    });
    let mut lines = BufReader::new(cloud.try_clone().unwrap());
    let mut read = || {
        let mut line = String::new();
        lines.read_line(&mut line).unwrap();
        serde_json::from_str::<serde_json::Value>(&line).unwrap()
    };
    let joined = read();
    // The link comes back a while after the join: the window is not spent.
    std::thread::sleep(Duration::from_millis(400));
    cloud.write_all(b"{\"online\":true}\n").unwrap();
    let window_at = std::time::Instant::now();
    std::thread::sleep(confirm_at);
    let _ = cloud.write_all(b"{\"confirm\":true}\n");
    let (kept, decided) = helper.join().unwrap();
    (kept, vec![joined], decided - window_at)
}

#[test]
fn p2_the_window_starts_when_lamp_cloud_is_online_again_and_a_confirm_1_s_before_its_end_keeps_the_network()
 {
    let (kept, said, _) = confirmed_with(Duration::from_secs(2), Duration::from_secs(1));
    assert!(kept);
    // The bounds lamp-cloud keeps, the helper's: the whole window, from the
    // moment the link is up, not less the 400 ms it took to come back.
    assert_eq!(
        said[0],
        json!({"joined": {"ssid": "New Home", "online_within_ms": 5_000, "confirm_within_ms": 2_000}})
    );
}

#[test]
fn p3_a_confirm_1_s_after_the_helpers_window_is_no_confirmation() {
    let (kept, _, decided) = confirmed_with(Duration::from_secs(1), Duration::from_secs(2));
    assert!(!kept);
    // Decided at the window's end and its slack, not at the late
    // confirmation.
    assert!(decided < Duration::from_millis(1_800), "{decided:?}");
}

#[test]
fn p2_no_link_back_within_its_bound_is_no_confirmation() {
    use std::io::{BufRead, BufReader};
    let (helper_end, cloud) = std::os::unix::net::UnixStream::pair().unwrap();
    let reader = BufReader::new(helper_end.try_clone().unwrap());
    let started = std::time::Instant::now();
    let helper = std::thread::spawn(move || {
        let mut out = helper_end;
        lamp_setup::wifi_change::confirmed_on(
            reader,
            &mut out,
            "New Home",
            Duration::from_millis(300),
            Duration::from_secs(5),
        )
    });
    let mut joined = String::new();
    BufReader::new(cloud.try_clone().unwrap())
        .read_line(&mut joined)
        .unwrap();
    assert!(!helper.join().unwrap());
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn p3_1_the_stop_time_restore_with_the_radio_failing_keeps_the_old_file_and_tries_again() {
    let mut fake = wrong_password_fake();
    let mut twist = Twist::new(&mut fake);
    twist.cut_at_ms = Some(1_000);
    let cut = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        change(&mut twist, "VN", &asked("New Home"))
    }));
    assert!(cut.is_err());
    twist.cut_at_ms = None;
    twist.stations_failing = true;
    twist.reloads_failing = u32::MAX;
    let calls = twist.station_calls;
    assert_eq!(restore(&mut twist, true), Restored::NotBack);
    // As many tries as fit in the stop's bound (follow-up 4).
    assert!((1..=BACK_TRIES).contains(&(twist.station_calls - calls)));
    // The file is the old one, and `.lamp-previous` stays for the boot's
    // restore: nothing is deleted before the old network is back.
    assert_eq!(fake.conf.as_deref(), Some(&old_conf()[..]));
    assert_eq!(fake.previous.as_deref(), Some(&old_conf()[..]));
    // With the radio working again, it is back and the old file goes.
    let mut twist = Twist::new(&mut fake);
    twist.reloads_failing = 1;
    assert_eq!(restore(&mut twist, true), Restored::Put);
    assert_eq!(fake.previous, None);
}

#[test]
fn p3_2_the_old_files_write_failing_once_still_ends_on_the_old_network() {
    let mut fake = wrong_password_fake();
    let mut twist = Twist::new(&mut fake);
    // The 1st write is the new network's, the 2nd the old one's.
    twist.fail_write = Some(2);
    let started = twist.fake.now_ms;
    let changed = change(&mut twist, "VN", &asked("New Home"));
    assert!(matches!(changed, Changed::Back { .. }), "{changed:?}");
    assert!(twist.write_calls >= 3);
    assert_eq!(fake.conf.as_deref(), Some(&old_conf()[..]));
    assert_eq!(fake.radio.last().unwrap(), &old_conf());
    assert_eq!(fake.previous, None);
    // At once: not after the 10 minutes of the lost line.
    assert!(fake.now_ms - started < 5 * 60_000);
}

#[test]
fn p3_3_a_confirmed_change_whose_old_file_will_not_go_cannot_be_undone_by_a_restore() {
    let new_conf = lamp_setup::wifi::wpa_conf("VN", "New Home", FAKE_PASSWORD)
        .unwrap()
        .into_bytes();
    // Removed on the second try.
    let mut fake = Fake::new(&[("Old Home", OLD), ("New Home", GOOD)]);
    let mut twist = Twist::new(&mut fake);
    twist.removes_failing = 1;
    assert_eq!(
        change(&mut twist, "VN", &asked("New Home")),
        Changed::Joined
    );
    assert_eq!(fake.previous, None);
    // Never removed: `.lamp-previous` is the new network, so a restore at
    // the stop or the boot keeps the confirmed change.
    let mut fake = Fake::new(&[("Old Home", OLD), ("New Home", GOOD)]);
    let mut twist = Twist::new(&mut fake);
    twist.removes_failing = u32::MAX;
    assert_eq!(
        change(&mut twist, "VN", &asked("New Home")),
        Changed::Joined
    );
    assert_eq!(fake.previous.as_deref(), Some(&new_conf[..]));
    let _ = restore(&mut fake, false);
    assert_eq!(fake.conf.as_deref(), Some(&new_conf[..]));
}

#[test]
fn p3_5_an_unreadable_old_file_is_an_error_not_nothing() {
    let mut fake = wrong_password_fake();
    let mut twist = Twist::new(&mut fake);
    twist.previous_unreadable = true;
    assert_eq!(restore(&mut twist, true), Restored::Failed);
    assert_eq!(restore(&mut twist, false), Restored::Failed);
}

#[test]
fn p3_a_cut_during_the_confirm_wait_leaves_the_old_network_to_restore() {
    // Killed while the app's confirmation is waited for (RuntimeMaxSec, a
    // crash): the new network is live, `.lamp-previous` the old one.
    let mut fake = Fake::new(&[("Old Home", OLD), ("New Home", GOOD)]);
    let mut twist = Twist::new(&mut fake);
    let cut = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        struct Panicking<'a, 'b>(&'a mut Twist<'b>);
        impl Host for Panicking<'_, '_> {
            fn read_conf(&mut self) -> io::Result<Option<Vec<u8>>> {
                self.0.read_conf()
            }
            fn write_conf(&mut self, bytes: Option<&[u8]>) -> io::Result<()> {
                self.0.write_conf(bytes)
            }
            fn station(&mut self) -> io::Result<()> {
                self.0.station()
            }
            fn hotspot(&mut self) -> io::Result<()> {
                self.0.hotspot()
            }
            fn internet(&mut self) -> bool {
                self.0.internet()
            }
            fn current_ssid(&mut self) -> Option<String> {
                self.0.current_ssid()
            }
            fn has_station_ip(&mut self) -> bool {
                self.0.has_station_ip()
            }
            fn wpa_state(&mut self) -> String {
                self.0.wpa_state()
            }
            fn wpa_events(&mut self) -> Vec<String> {
                self.0.wpa_events()
            }
            fn now_ms(&mut self) -> u64 {
                self.0.now_ms()
            }
            fn sleep(&mut self, duration: Duration) {
                self.0.sleep(duration);
            }
        }
        impl Rejoin for Panicking<'_, '_> {
            fn broker(&mut self, address: &str) -> bool {
                self.0.broker(address)
            }
            fn keep_previous(&mut self, bytes: Option<&[u8]>) -> io::Result<()> {
                self.0.keep_previous(bytes)
            }
            fn read_previous(&mut self) -> io::Result<Option<Vec<u8>>> {
                self.0.read_previous()
            }
            fn reload(&mut self) -> io::Result<()> {
                self.0.reload()
            }
            fn app_confirms(&mut self, _ssid: &str, _most: Duration) -> bool {
                panic!("the helper was cut while the app's confirmation was waited for");
            }
            fn hold(&mut self, conf: &[u8]) -> io::Result<()> {
                self.0.hold(conf)
            }
        }
        change(&mut Panicking(&mut twist), "VN", &asked("New Home"))
    }));
    assert!(cut.is_err());
    assert_eq!(fake.previous.as_deref(), Some(&old_conf()[..]));
    assert_eq!(restore(&mut fake, true), Restored::Put);
    assert_eq!(fake.conf.as_deref(), Some(&old_conf()[..]));
    assert_eq!(fake.radio.last().unwrap(), &old_conf());
    assert_eq!(fake.previous, None);
}

/// The first builder's follow-up 1 on #2617: the old network is back only
/// when it is the one joined, not on any address (one left from the new
/// network, or another network's).
#[test]
fn followup_1_back_is_the_old_network_joined_not_any_address() {
    // The old network is gone; an address is there all the same.
    let mut fake = Fake::new(&[(
        "New Home",
        Network {
            password: "another-fake-pass",
            ..GOOD
        },
    )]);
    fake.previous = Some(old_conf());
    let mut twist = Twist::new(&mut fake);
    twist.address_always = true;
    assert_eq!(restore(&mut twist, true), Restored::NotBack);
    assert_eq!(fake.previous.as_deref(), Some(&old_conf()[..]));
    // With the old network there, it is back.
    let mut fake = wrong_password_fake();
    fake.previous = Some(old_conf());
    let mut twist = Twist::new(&mut fake);
    twist.address_always = true;
    assert_eq!(restore(&mut twist, true), Restored::Put);
    // The old network's name, from setup's file and from V1's.
    assert_eq!(
        lamp_setup::wifi_change::conf_ssid(&old_conf()).as_deref(),
        Some("Old Home")
    );
    let v1 = b"ctrl_interface=DIR=/run/wpa_supplicant\nnetwork={\n\tbssid=aa:bb:cc:dd:ee:ff\n\tssid=\"Nh\xc3\xa0 Lan\"\n\tscan_ssid=1\n\tpsk=\"fake-pass-OLD-5678\"\n}\n";
    assert_eq!(
        lamp_setup::wifi_change::conf_ssid(v1).as_deref(),
        Some("Nh\u{e0} Lan")
    );
    assert_eq!(lamp_setup::wifi_change::conf_ssid(b"country=VN\n"), None);
}

/// Follow-up 4: the stop-time restore ends within its bound, well inside the
/// unit's stop timeout, even with every radio start hanging; the kept file
/// stays for the boot's restore, the backstop.
#[test]
fn followup_4_the_stop_time_restore_is_bounded_within_the_stop_timeout() {
    let mut fake = wrong_password_fake();
    fake.previous = Some(old_conf());
    fake.networks.remove("Old Home");
    let mut twist = Twist::new(&mut fake);
    twist.stations_hanging = true;
    let started = twist.fake.now_ms;
    assert_eq!(restore(&mut twist, true), Restored::NotBack);
    let took = twist.fake.now_ms - started;
    assert!(took <= RESTORE_MOST.as_millis() as u64, "{took} ms");
    assert_eq!(fake.previous.as_deref(), Some(&old_conf()[..]));
    let unit = include_str!("../../../../deploy/product/lamp-setup-wifi@.service");
    assert!(unit.contains("\nTimeoutStopSec=2min\n"));
    // Its bound, a poll's wpa_cli queries past it and put_back's 2 s.
    let worst = RESTORE_MOST
        + 2 * lamp_setup::wifi::QUERY_TIMEOUT
        + Duration::from_secs(1 + u64::from(BACK_TRIES - 1));
    assert!(worst < Duration::from_secs(120), "{worst:?}");
}

/// Follow-up 6 (P3 2's remainder): the old file's three writes all fail; the
/// old network is run from wpa_supplicant's memory at once, not left to the
/// stop's restore, and the old file stays kept for that restore.
#[test]
fn followup_6_an_old_file_that_cannot_be_written_is_run_from_memory_at_once() {
    let mut fake = wrong_password_fake();
    let mut twist = Twist::new(&mut fake);
    // The 1st write is the new network's; every one after fails.
    twist.writes_failing_from = Some(2);
    let started = twist.fake.now_ms;
    let changed = change(&mut twist, "VN", &asked("New Home"));
    let Changed::Back { why } = &changed else {
        panic!("{changed:?}");
    };
    assert!(why.contains("from memory"), "{why}");
    assert_eq!(fake.holds, 1);
    assert_eq!(
        fake.radio.last().unwrap(),
        &old_conf(),
        "the radio runs the old network"
    );
    assert_eq!(fake.current_ssid().as_deref(), Some("Old Home"));
    assert_eq!(fake.previous.as_deref(), Some(&old_conf()[..]));
    assert!(fake.now_ms - started < JOIN_WAIT.as_millis() as u64 + 2 * 60_000);
    assert_eq!(line_for(&changed), Some(SAY_BACK));
}

/// #2686's P2: the old file cannot be written and every hold fails
/// part-way, its networks already removed. The lamp is not left on no
/// network: wpa_supplicant reads the live file again, the new network that
/// worked, and the stop's restore leaves it there.
#[test]
fn p2_a_hold_that_fails_part_way_leaves_the_lamp_on_the_live_network() {
    let mut fake = Fake::new(&[("Old Home", OLD), ("New Home", GOOD)]);
    fake.app_confirms = false;
    fake.holds_failing = BACK_TRIES;
    let mut twist = Twist::new(&mut fake);
    twist.writes_failing_from = Some(2);
    let changed = change(&mut twist, "VN", &asked("New Home"));
    assert!(matches!(changed, Changed::NotBack { .. }), "{changed:?}");
    // The tries were spaced, not back to back.
    let at = &twist.fake.hold_at_ms;
    assert_eq!(at.len(), BACK_TRIES as usize);
    assert!(at.windows(2).all(|w| w[1] - w[0] >= 1_000), "{at:?}");
    twist.sleep(Duration::from_secs(10));
    assert_eq!(twist.current_ssid().as_deref(), Some("New Home"));
    assert_eq!(restore(&mut twist, true), Restored::Failed);
    twist.sleep(Duration::from_secs(10));
    assert_eq!(twist.current_ssid().as_deref(), Some("New Home"));
    assert_eq!(fake.previous.as_deref(), Some(&old_conf()[..]));
    // One hold failing part-way, the next taken: back on the old network.
    let mut fake = Fake::new(&[("Old Home", OLD), ("New Home", GOOD)]);
    fake.app_confirms = false;
    fake.holds_failing = 1;
    let mut twist = Twist::new(&mut fake);
    twist.writes_failing_from = Some(2);
    let changed = change(&mut twist, "VN", &asked("New Home"));
    assert!(matches!(changed, Changed::Back { .. }), "{changed:?}");
    assert_eq!(fake.holds, 2);
    assert_eq!(fake.current_ssid().as_deref(), Some("Old Home"));
}
