//! The Wi-Fi changed after setup (#1959), against a fake network manager:
//! the networks it can see, what each gives, and a clock of its own.
use lamp_setup::wifi::Host;
use lamp_setup::wifi_change::{
    Asked, BACK_WAIT, Changed, JOIN_WAIT, LOST_MOST, Mouth, Rejoin, SAY_BACK, SAY_JOINED, SAY_LOST,
    answer, change, line_for, parse, say_after,
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
        };
        fake.start_radio();
        fake.stations = 0;
        fake.radio.clear();
        fake
    }

    fn start_radio(&mut self) {
        self.stations += 1;
        self.started_ms = self.now_ms;
        self.wrong_key = false;
        self.joined = None;
        let conf = String::from_utf8(self.conf.clone().unwrap_or_default()).unwrap();
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
        self.joined
            .as_ref()
            .filter(|(_, network)| self.now_ms >= self.started_ms + network.after_ms)
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
}

const GOOD: Network = Network {
    password: FAKE_PASSWORD,
    address: true,
    broker: true,
    after_ms: 8_000,
};
const OLD: Network = Network {
    password: OLD_PASSWORD,
    address: true,
    broker: true,
    after_ms: 5_000,
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
    // Joined at 8 s, within the bound.
    assert!(fake.now_ms <= 9_000, "{}", fake.now_ms);
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
        why.ends_with("the old Wi-Fi did not come back within 60 s, and the lamp keeps trying it"),
        "{why}"
    );
    assert_eq!(fake.conf.as_deref(), Some(&old_conf()[..]));
    assert_eq!(fake.hotspots, 0);
    let bound = (JOIN_WAIT + BACK_WAIT).as_millis() as u64 + 2_000;
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

impl Mouth for Heard {
    fn say(&mut self, text: &str) {
        self.0.push((self.1, text.to_owned()));
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
        "I can't reach any Wi-Fi. Hold my button for five seconds to set me up again."
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
    assert!(mouth.heard.iter().all(|(_, line)| line == SAY_LOST));
    for pair in at.windows(2) {
        assert!(pair[1] - pair[0] >= 60_000, "{at:?}");
    }
    // And it stops at the bound when nothing ever comes back.
    let mut fake = Fake::new(&[]);
    let started = fake.now_ms;
    let mut heard = Heard::default();
    let said = say_after(&mut fake, &mut heard, &changed, BROKER);
    assert_eq!(said as u64, LOST_MOST.as_secs() / 60);
    assert!(fake.now_ms - started <= LOST_MOST.as_millis() as u64 + 5_000);
}

#[test]
fn the_helpers_units_are_its_own_one_change_at_a_time() {
    let unit = include_str!("../../../deploy/product/lamp-setup-wifi@.service");
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
    let socket = include_str!("../../../deploy/product/lamp-setup-wifi.socket");
    assert!(socket.contains("ListenStream=/run/lampos-setup-wifi/wifi.sock\n"));
    assert!(socket.contains("SocketGroup=lampos\nSocketMode=0660\n"));
    assert!(socket.contains("Accept=yes\nMaxConnections=1\n"));
}
