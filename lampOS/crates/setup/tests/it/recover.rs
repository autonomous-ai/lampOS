//! #3020: the policy that opens setup's hotspot when a set-up lamp's saved
//! network is gone (`lamp_setup::recover`), on a clock and looks of its own.
use lamp_setup::recover::{
    Act, JOIN_WAIT_MS, LOSS_GRACE_MS, PHONE_HOLDS_MS, RECORDING_OPENED, RETRY_EVERY_MS, Recovery,
    SAY_OPENED, Seen, State, on,
};

const GONE: Seen = Seen {
    online: false,
    associated: false,
    busy: false,
    phones: Some(0),
};
const BACK: Seen = Seen {
    online: true,
    ..GONE
};

/// Looks every 5 s from `from_ms` to `to_ms`, the acts asked for and when.
fn looks(policy: &mut Recovery, from_ms: u64, to_ms: u64, seen: Seen) -> Vec<(u64, Act)> {
    (from_ms..=to_ms)
        .step_by(5_000)
        .filter_map(|at| policy.step(at, seen).map(|act| (at, act)))
        .collect()
}

#[test]
fn the_network_gone_90_s_opens_the_hotspot_once() {
    let mut policy = Recovery::default();
    assert_eq!(policy.step(0, BACK), None);
    let acts = looks(&mut policy, 5_000, 200_000, GONE);
    // Lost at 5 s, open at 95 s, and not again before the 2 min retry.
    assert_eq!(acts[0], (5_000 + LOSS_GRACE_MS, Act::OpenHotspot));
    assert!(policy.recovering());
    assert_eq!(acts.len(), 1, "{acts:?}");
}

/// #3743 (agreed with Sixth on #3020): the hotspot's line is said once a
/// loss, as it first opens; not at its reopening after a failed try of the
/// saved network, and again only for a new loss.
#[test]
fn the_hotspots_line_is_said_once_a_loss() {
    let mut policy = Recovery::default();
    let mut said = Vec::new();
    let mut look = |policy: &mut Recovery, at: u64, seen: Seen| {
        let act = policy.step(at, seen);
        if policy.says_opened(act) {
            said.push(at);
        }
        act
    };
    for at in (0..=LOSS_GRACE_MS).step_by(5_000) {
        look(&mut policy, at, GONE);
    }
    // A try that doesn't join, and the hotspot opened again: nothing said.
    let retry = LOSS_GRACE_MS + RETRY_EVERY_MS;
    for at in (LOSS_GRACE_MS + 5_000..=retry + JOIN_WAIT_MS).step_by(5_000) {
        look(&mut policy, at, GONE);
    }
    assert!(policy.recovering());
    // Back, then gone again: a new loss, said again at its own 90 s.
    let back = retry + JOIN_WAIT_MS + 5_000;
    assert_eq!(look(&mut policy, back, BACK), None);
    for at in (back + 5_000..=back + 5_000 + LOSS_GRACE_MS).step_by(5_000) {
        look(&mut policy, at, GONE);
    }
    assert_eq!(said, [LOSS_GRACE_MS, back + 5_000 + LOSS_GRACE_MS]);
    // The recording's name and words, and the words fit any loader's 10 s.
    assert_eq!(RECORDING_OPENED, "wifi_recovering");
    assert_eq!(
        SAY_OPENED,
        "I can't reach my Wi-Fi, so I've opened setup. Set my Wi-Fi again in the app."
    );
    assert!(!SAY_OPENED.contains("reset") && !SAY_OPENED.contains("erase"));
}

/// Second's P3 on #4157: a retry that joins the radio but gets no address,
/// or that a setup or Wi-Fi change takes, goes back to `Lost`, and its 90 s
/// open the hotspot again; the line was said for this loss already, so it
/// isn't said again until the lamp has been online.
#[test]
fn a_retry_back_to_lost_does_not_say_the_line_again() {
    let slow = Seen {
        associated: true,
        ..GONE
    };
    let busy = Seen { busy: true, ..GONE };
    for seen in [slow, busy] {
        let mut policy = Recovery::default();
        let mut said = Vec::new();
        let mut look = |policy: &mut Recovery, at: u64, seen: Seen| {
            let act = policy.step(at, seen);
            if policy.says_opened(act) {
                said.push(at);
            }
            act
        };
        for at in (0..=LOSS_GRACE_MS).step_by(5_000) {
            look(&mut policy, at, GONE);
        }
        let retry = LOSS_GRACE_MS + RETRY_EVERY_MS;
        assert_eq!(look(&mut policy, retry, GONE), Some(Act::Rejoin));
        // Only slow, or busy: lost afresh, not recovering.
        assert_eq!(look(&mut policy, retry + 5_000, seen), None);
        assert!(!policy.recovering());
        let mut reopened = None;
        for at in (retry + 10_000..=retry + 10_000 + LOSS_GRACE_MS).step_by(5_000) {
            if look(&mut policy, at, GONE) == Some(Act::OpenHotspot) {
                reopened = Some(at);
            }
        }
        assert!(reopened.is_some(), "{seen:?}: the hotspot opens again");
        assert_eq!(said, [LOSS_GRACE_MS], "{seen:?}: said once this loss");
    }
}

#[test]
fn the_network_back_at_80_s_opens_nothing() {
    let mut policy = Recovery::default();
    assert!(looks(&mut policy, 0, 80_000, GONE).is_empty());
    assert_eq!(policy.step(85_000, BACK), None);
    assert_eq!(policy.state(), State::Online);
    // Gone again: its 90 s start over from there.
    assert!(looks(&mut policy, 90_000, 175_000, GONE).is_empty());
    assert_eq!(policy.step(180_000, GONE), Some(Act::OpenHotspot));
}

#[test]
fn the_network_back_while_the_hotspot_is_open_is_rejoined_within_2_min() {
    let mut policy = Recovery::default();
    let opened = looks(&mut policy, 0, LOSS_GRACE_MS, GONE);
    assert_eq!(opened, [(LOSS_GRACE_MS, Act::OpenHotspot)]);
    // The router comes back at once; the hotspot has no phone on it: the
    // saved network is tried at the retry, and joined.
    let retry = LOSS_GRACE_MS + RETRY_EVERY_MS;
    let acts = looks(&mut policy, LOSS_GRACE_MS + 5_000, retry, GONE);
    assert_eq!(acts, [(retry, Act::Rejoin)]);
    assert_eq!(policy.step(retry + 10_000, BACK), None);
    assert_eq!(policy.state(), State::Online);
    assert!(!policy.recovering());
}

#[test]
fn a_try_that_does_not_join_in_45_s_opens_the_hotspot_again() {
    let mut policy = Recovery::default();
    looks(&mut policy, 0, LOSS_GRACE_MS, GONE);
    let retry = LOSS_GRACE_MS + RETRY_EVERY_MS;
    assert_eq!(policy.step(retry, GONE), Some(Act::Rejoin));
    assert_eq!(policy.step(retry + JOIN_WAIT_MS - 5_000, GONE), None);
    assert_eq!(
        policy.step(retry + JOIN_WAIT_MS, GONE),
        Some(Act::OpenHotspot)
    );
    assert!(policy.recovering());
}

/// Reviewer-two on #3024: a try of the saved network that has joined its
/// radio and waits for an address (slow DHCP), or one a Wi-Fi change has
/// taken, is never cut back to the hotspot, at 45 s or after. Once that
/// clears with no address, the whole 90 s run again before the hotspot;
/// and an address in the meantime is online.
#[test]
fn a_try_that_is_only_slow_or_busy_is_not_cut_and_then_counts_as_lost() {
    let slow = Seen {
        associated: true,
        ..GONE
    };
    let busy = Seen { busy: true, ..GONE };
    for seen in [slow, busy] {
        let mut policy = Recovery::default();
        looks(&mut policy, 0, LOSS_GRACE_MS, GONE);
        let retry = LOSS_GRACE_MS + RETRY_EVERY_MS;
        assert_eq!(policy.step(retry, GONE), Some(Act::Rejoin));
        // Just before the deadline it still waits, then the guard holds.
        assert_eq!(policy.step(retry + JOIN_WAIT_MS - 5_000, GONE), None);
        assert!(
            looks(&mut policy, retry + JOIN_WAIT_MS, retry + 300_000, seen).is_empty(),
            "{seen:?}"
        );
        assert!(!policy.recovering(), "{seen:?}");
        // Cleared, no address: 90 s from the last look slow or busy.
        let cleared = retry + 300_000;
        assert!(
            looks(
                &mut policy,
                cleared + 5_000,
                cleared + LOSS_GRACE_MS - 5_000,
                GONE
            )
            .is_empty()
        );
        assert_eq!(
            policy.step(cleared + LOSS_GRACE_MS, GONE),
            Some(Act::OpenHotspot)
        );

        // The same, an address while it waits: online.
        let mut policy = Recovery::default();
        looks(&mut policy, 0, LOSS_GRACE_MS, GONE);
        assert_eq!(policy.step(retry, GONE), Some(Act::Rejoin));
        looks(&mut policy, retry + 5_000, retry + 100_000, seen);
        assert_eq!(policy.step(retry + 105_000, BACK), None);
        assert_eq!(policy.state(), State::Online);
    }
}

#[test]
fn a_lamp_only_slow_to_get_its_address_never_opens_it() {
    // Joined to a known network's radio (the saved one, or another it
    // knows), its address not there yet, for 10 minutes.
    let slow = Seen {
        associated: true,
        ..GONE
    };
    let mut policy = Recovery::default();
    assert!(looks(&mut policy, 0, 600_000, slow).is_empty());
    assert!(!policy.recovering());
}

#[test]
fn another_known_network_taken_is_online_and_opens_nothing() {
    // wpa_supplicant takes another network the saved file names: an
    // address, so online.
    let mut policy = Recovery::default();
    assert!(looks(&mut policy, 0, 600_000, BACK).is_empty());
    assert_eq!(policy.state(), State::Online);
}

#[test]
fn setup_or_a_wifi_change_running_holds_it() {
    let busy = Seen { busy: true, ..GONE };
    let mut policy = Recovery::default();
    assert!(looks(&mut policy, 0, 600_000, busy).is_empty());
    // Done, still gone: a whole 90 s from its last look busy (600 s).
    assert!(looks(&mut policy, 605_000, 685_000, GONE).is_empty());
    assert_eq!(policy.step(690_000, GONE), Some(Act::OpenHotspot));
}

#[test]
fn a_setup_run_is_never_cut_by_a_retry() {
    let mut policy = Recovery::default();
    looks(&mut policy, 0, LOSS_GRACE_MS, GONE);
    let setting_up = Seen {
        busy: true,
        phones: Some(1),
        ..GONE
    };
    assert!(looks(&mut policy, LOSS_GRACE_MS + 5_000, 3_000_000, setting_up).is_empty());
    assert!(policy.recovering());
    // The run over, nobody on: the retry, long due.
    assert_eq!(policy.step(3_005_000, GONE), Some(Act::Rejoin));
}

/// El's P2 on #3024: a phone that joined the open hotspot by itself (it
/// remembers it from setup) holds the retry 10 min past due, not all night.
#[test]
fn a_phone_holds_a_retry_10_min_at_most() {
    for phones in [Some(1), None] {
        let mut policy = Recovery::default();
        looks(&mut policy, 0, LOSS_GRACE_MS, GONE);
        let due = LOSS_GRACE_MS + RETRY_EVERY_MS;
        let phone = Seen { phones, ..GONE };
        let acts = looks(&mut policy, LOSS_GRACE_MS + 5_000, due + 900_000, phone);
        assert_eq!(acts[0], (due + PHONE_HOLDS_MS, Act::Rejoin), "{phones:?}");
    }
}

#[test]
fn a_changed_password_still_opens_it() {
    // The saved network in reach but never joining (a wrong key): not
    // associated, so gone, and the hotspot stays up between retries.
    let mut policy = Recovery::default();
    let acts = looks(&mut policy, 0, LOSS_GRACE_MS + 60_000, GONE);
    assert_eq!(acts, [(LOSS_GRACE_MS, Act::OpenHotspot)]);
}

#[test]
fn a_new_network_set_from_the_hotspot_ends_recovery() {
    // The app sets the new network through setup's page, which joins it:
    // online, and recovery is over.
    let mut policy = Recovery::default();
    looks(&mut policy, 0, LOSS_GRACE_MS, GONE);
    assert_eq!(policy.step(LOSS_GRACE_MS + 30_000, BACK), None);
    assert_eq!(policy.state(), State::Online);
}

#[test]
fn the_switch_is_on_for_on_only() {
    assert!(on(Some("on")));
    for value in [None, Some(""), Some("On"), Some("1"), Some("off")] {
        assert!(!on(value), "{value:?}");
    }
}

/// The product unit's own command line runs (the lead's agent on #1711, 27
/// Oct): build 7071's `lamp-setup` matched `recover` only after it required
/// `--plan`, which the unit never gives, so on lamp-52e6 the unit restarted 76
/// times in 85 min. Its `ExecStart`, the configuration swapped for a test
/// file and the switch off, ends at once and well, saying recovery is off.
/// Without `--enable` it says so; with a `--plan` it is refused.
#[test]
fn the_product_units_command_line_runs_and_ends_when_recovery_is_off() {
    const UNIT: &str = include_str!("../../../../deploy/product/lamp-setup-recover.service");
    let exec = UNIT
        .lines()
        .find_map(|line| line.strip_prefix("ExecStart="))
        .expect("the unit's ExecStart");
    let mut words = exec.split_whitespace();
    assert!(words.next().unwrap().ends_with("/lamp-setup"), "{exec}");
    let unit_args: Vec<&str> = words.collect();
    assert_eq!(
        unit_args,
        [
            "recover",
            "--config",
            "/etc/lampOS/setup/serve.json",
            "--enable"
        ]
    );
    let dir = std::env::temp_dir().join(format!("lamp-setup-recover-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let config = dir.join("serve.json");
    std::fs::write(&config, include_str!("../../fixtures/serve-own-page.json")).unwrap();
    let run = |args: &[&str]| {
        let args: Vec<std::ffi::OsString> = args
            .iter()
            .map(|arg| {
                if *arg == "/etc/lampOS/setup/serve.json" {
                    config.clone().into_os_string()
                } else {
                    (*arg).into()
                }
            })
            .collect();
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_lamp-setup"))
            .args(&args)
            .env_remove(lamp_setup::recover::SWITCH)
            .output()
            .unwrap();
        (
            output.status.success(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    };
    let (ok, _, said) = run(&unit_args);
    assert!(ok, "{said}");
    assert!(said.contains("recover: off"), "{said}");
    assert!(!said.contains("--plan is required"), "{said}");
    let (ok, out, said) = run(&["recover", "--config", "/etc/lampOS/setup/serve.json"]);
    assert!(ok, "{said}");
    assert!(out.contains("\"enabled\":false"), "{out}");
    let (ok, _, said) = run(&[
        "recover",
        "--config",
        "/etc/lampOS/setup/serve.json",
        "--plan",
        "/x",
        "--enable",
    ]);
    assert!(!ok);
    assert!(
        said.contains("recover takes --config and --enable"),
        "{said}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// #4863: a made-up lamp's files under a folder of its own, its radio as
/// `device-ap-mode` leaves it or as `device-sta-mode` puts it back.
struct Radio {
    root: std::path::PathBuf,
    stations: u32,
    /// How many `station()` calls fail from now, as the image's script does
    /// at an `iw` with no fallback: hostapd already disabled, wpa_supplicant
    /// still masked and wlan0 still denied to dhcpcd.
    fails: u32,
    now_ms: u64,
}

const WPA_LINK: &str = "etc/systemd/system/multi-user.target.wants/wpa_supplicant@wlan0.service";
const HOSTAPD_LINK: &str = "etc/systemd/system/multi-user.target.wants/hostapd.service";
const WPA_MASK: &str = "etc/systemd/system/wpa_supplicant@wlan0.service";

impl Radio {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("lamp-setup-radio-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for folder in [
            "etc/systemd/system/multi-user.target.wants",
            "var/lib/lampos-setup",
            "sys/class/net/wlan0",
        ] {
            std::fs::create_dir_all(root.join(folder)).unwrap();
        }
        std::fs::write(root.join("etc/dhcpcd.conf"), "hostname\nclientid\n").unwrap();
        // A station: wpa_supplicant enabled, as device-sta-mode leaves it.
        std::os::unix::fs::symlink(
            "/lib/systemd/system/wpa_supplicant@.service",
            root.join(WPA_LINK),
        )
        .unwrap();
        Self {
            root,
            stations: 0,
            fails: 0,
            now_ms: 0,
        }
    }
    fn path(&self, name: &str) -> std::path::PathBuf {
        self.root.join(name)
    }
    fn marker(&self) -> std::path::PathBuf {
        self.path("var/lib/lampos-setup/done")
    }
    fn previous(&self) -> std::path::PathBuf {
        self.path("etc/wpa_supplicant/wpa_supplicant-wlan0.conf.lamp-previous")
    }
    fn set_up(self) -> Self {
        std::fs::write(self.marker(), "").unwrap();
        self
    }
    /// What `device-ap-mode` leaves that outlives a boot.
    fn hotspot_left(self) -> Self {
        std::fs::remove_file(self.path(WPA_LINK)).unwrap();
        std::os::unix::fs::symlink("/dev/null", self.path(WPA_MASK)).unwrap();
        std::os::unix::fs::symlink(
            "/lib/systemd/system/hostapd.service",
            self.path(HOSTAPD_LINK),
        )
        .unwrap();
        std::fs::write(
            self.path("etc/dhcpcd.conf"),
            "denyinterfaces wlan0\nhostname\nclientid\n",
        )
        .unwrap();
        self
    }
    fn start(&mut self, setup_active: bool) -> Option<std::io::Result<()>> {
        let (root, marker, previous) = (self.root.clone(), self.marker(), self.previous());
        lamp_setup::recover::at_start(self, &root, &marker, &previous, || setup_active)
    }
}

impl Drop for Radio {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl lamp_setup::wifi::Host for Radio {
    fn read_conf(&mut self) -> std::io::Result<Option<Vec<u8>>> {
        Ok(None)
    }
    fn write_conf(&mut self, _: Option<&[u8]>) -> std::io::Result<()> {
        Ok(())
    }
    /// As `device-sta-mode`: hostapd disabled first; then, past its `iw`,
    /// the denied interface and the mask go and wpa_supplicant is enabled.
    fn station(&mut self) -> std::io::Result<()> {
        self.stations += 1;
        let _ = std::fs::remove_file(self.path(HOSTAPD_LINK));
        if self.fails > 0 {
            self.fails -= 1;
            return Err(std::io::Error::other("device-sta-mode ended with 1"));
        }
        std::fs::write(self.path("etc/dhcpcd.conf"), "hostname\nclientid\n")?;
        let _ = std::fs::remove_file(self.path(WPA_MASK));
        if std::fs::symlink_metadata(self.path(WPA_LINK)).is_err() {
            std::os::unix::fs::symlink(
                "/lib/systemd/system/wpa_supplicant@.service",
                self.path(WPA_LINK),
            )?;
        }
        Ok(())
    }
    fn hotspot(&mut self) -> std::io::Result<()> {
        unreachable!("never opened here")
    }
    fn internet(&mut self) -> bool {
        false
    }
    fn current_ssid(&mut self) -> Option<String> {
        None
    }
    fn has_station_ip(&mut self) -> bool {
        false
    }
    fn wpa_state(&mut self) -> String {
        String::new()
    }
    fn wpa_events(&mut self) -> Vec<String> {
        vec![]
    }
    fn now_ms(&mut self) -> u64 {
        self.now_ms
    }
    fn sleep(&mut self, duration: std::time::Duration) {
        self.now_ms += duration.as_millis() as u64;
    }
}

/// #4863: recover's stop closes its hotspot and the station comes back once.
#[test]
fn closing_the_hotspot_on_a_stop_returns_the_station_once() {
    let mut radio = Radio::new("close").set_up().hotspot_left();
    let marker = radio.marker();
    assert!(lamp_setup::recover::closed(&mut radio, &marker).is_some_and(|done| done.is_ok()));
    assert_eq!(radio.stations, 1);
    assert!(!lamp_setup::recover::left_as_hotspot(&radio.root));
}

/// #4863: the marker gone is a reset, whose own setup takes the hotspot.
#[test]
fn closing_the_hotspot_after_a_reset_leaves_it_to_setup() {
    let mut radio = Radio::new("close-reset").hotspot_left();
    let marker = radio.marker();
    assert!(lamp_setup::recover::closed(&mut radio, &marker).is_none());
    assert_eq!(radio.stations, 0);
}

/// #4863: a set-up lamp that lost power with the hotspot open is a station
/// again at the start, the switch off (it is never read here).
#[test]
fn a_set_up_lamp_left_a_hotspot_is_a_station_again_at_the_start_switch_or_not() {
    for left in ["mask", "dhcpcd", "not enabled"] {
        let mut radio = Radio::new(&format!("boot-{left}")).set_up();
        // Any one change alone leaves the lamp with no client network.
        match left {
            "mask" => std::os::unix::fs::symlink("/dev/null", radio.path(WPA_MASK)).unwrap(),
            "dhcpcd" => std::fs::write(
                radio.path("etc/dhcpcd.conf"),
                "denyinterfaces wlan0\nhostname\n",
            )
            .unwrap(),
            // Fifth's P3 on #4931: cut between device-sta-mode's unmask and
            // its enable, nothing else left of the hotspot.
            _ => std::fs::remove_file(radio.path(WPA_LINK)).unwrap(),
        }
        assert!(lamp_setup::recover::left_as_hotspot(&radio.root), "{left}");
        assert!(
            radio.start(false).is_some_and(|done| done.is_ok()),
            "{left}"
        );
        assert_eq!(radio.stations, 1, "{left}");
        assert!(!lamp_setup::recover::left_as_hotspot(&radio.root), "{left}");
    }
}

/// #4863: before the marker the hotspot is setup's own, meant: untouched.
#[test]
fn a_lamp_not_yet_set_up_keeps_its_first_setup_hotspot() {
    let mut radio = Radio::new("boot-fresh").hotspot_left();
    assert!(radio.start(false).is_none());
    assert_eq!(radio.stations, 0);
    assert!(lamp_setup::recover::left_as_hotspot(&radio.root));
    // A fresh image's wpa_supplicant not yet enabled, nothing else: left too.
    let mut fresh = Radio::new("boot-fresh-unenabled");
    std::fs::remove_file(fresh.path(WPA_LINK)).unwrap();
    assert!(fresh.start(false).is_none());
    assert_eq!(fresh.stations, 0);
}

/// #4863: setup running, or a Wi-Fi change's old file kept: theirs is the
/// radio, untouched. A lamp already a station is untouched too.
#[test]
fn setup_running_a_wifi_change_or_a_station_already_leave_the_radio_alone() {
    let mut radio = Radio::new("boot-setup").set_up().hotspot_left();
    assert!(radio.start(true).is_none());
    std::fs::create_dir_all(radio.path("etc/wpa_supplicant")).unwrap();
    std::fs::write(radio.previous(), "network={}\n").unwrap();
    assert!(radio.start(false).is_none());
    assert_eq!(radio.stations, 0);
    let mut station = Radio::new("boot-station").set_up();
    assert!(!lamp_setup::recover::left_as_hotspot(&station.root));
    assert!(station.start(false).is_none());
    assert_eq!(station.stations, 0);
}

/// #4863, Fifth's P2 on #4931: a return that fails half way (hotspot
/// disabled, wpa_supplicant still masked) is tried again at the same start,
/// 20 s apart, until it takes.
#[test]
fn a_return_failed_half_way_is_tried_again_at_the_same_start() {
    use lamp_setup::recover::{STATION_RETRY_MS, STATION_TRIES};
    let mut radio = Radio::new("boot-half").set_up().hotspot_left();
    radio.fails = STATION_TRIES - 1;
    assert!(radio.start(false).is_some_and(|done| done.is_ok()));
    assert_eq!(radio.stations, STATION_TRIES);
    assert_eq!(
        radio.now_ms,
        u64::from(STATION_TRIES - 1) * STATION_RETRY_MS
    );
    assert!(!radio.path(HOSTAPD_LINK).exists());
    assert!(!lamp_setup::recover::left_as_hotspot(&radio.root));
}

/// #4863: every try failing leaves the hotspot's state, so the next start
/// (the next boot) tries again; a return that took clears it, so the one
/// after does nothing.
#[test]
fn a_failed_return_is_tried_again_at_the_next_start_and_a_good_one_is_not() {
    use lamp_setup::recover::STATION_TRIES;
    let mut radio = Radio::new("boot-retry").set_up().hotspot_left();
    radio.fails = u32::MAX;
    assert!(radio.start(false).is_some_and(|done| done.is_err()));
    assert_eq!(radio.stations, STATION_TRIES);
    assert!(lamp_setup::recover::left_as_hotspot(&radio.root));
    assert!(radio.start(false).is_some_and(|done| done.is_err()));
    assert_eq!(radio.stations, 2 * STATION_TRIES);
    radio.fails = 0;
    assert!(radio.start(false).is_some_and(|done| done.is_ok()));
    assert!(radio.start(false).is_none());
    assert_eq!(radio.stations, 2 * STATION_TRIES + 1);
}

/// #4863: the interface not there yet is waited for, 30 s at most, then
/// the station is tried all the same.
#[test]
fn the_start_waits_for_the_interface_a_bounded_time() {
    let mut radio = Radio::new("boot-late").set_up().hotspot_left();
    std::fs::remove_dir(radio.path("sys/class/net/wlan0")).unwrap();
    assert!(radio.start(false).is_some());
    assert_eq!(radio.now_ms, lamp_setup::recover::INTERFACE_WAIT_MS);
    assert_eq!(radio.stations, 1);
}

/// #5550: a refused `wifi_recovering` (the lamp speaking, a higher say, the
/// voice not up yet) is asked again 5 s, 10 s and 20 s apart, never past
/// its ask's 60 s from the first, and only while the hotspot is open.
#[test]
fn a_refused_hotspot_line_is_asked_again_within_its_minute() {
    use lamp_setup::recover::SAY_AGAIN_WITHIN_MS;
    let mut policy = Recovery::default();
    let open = looks(&mut policy, 0, LOSS_GRACE_MS, GONE);
    assert_eq!(open, [(LOSS_GRACE_MS, Act::OpenHotspot)]);
    assert!(policy.says_opened(Some(Act::OpenHotspot)));
    // Refused every time: asked at +5 s, +15 s and +35 s, then no more.
    policy.not_played(LOSS_GRACE_MS);
    let phone = Seen {
        phones: Some(1),
        ..GONE
    };
    let mut again = Vec::new();
    for at in (LOSS_GRACE_MS + 5_000..LOSS_GRACE_MS + RETRY_EVERY_MS).step_by(5_000) {
        assert_eq!(policy.step(at, phone), None);
        if policy.says_again(at) {
            again.push(at - LOSS_GRACE_MS);
            policy.not_played(at);
        }
    }
    assert_eq!(again, [5_000, 15_000, 35_000]);
    assert!(again.iter().all(|at| *at <= SAY_AGAIN_WITHIN_MS));

    // Played at its first retry: not asked again.
    let mut policy = Recovery::default();
    looks(&mut policy, 0, LOSS_GRACE_MS, GONE);
    assert!(policy.says_opened(Some(Act::OpenHotspot)));
    policy.not_played(LOSS_GRACE_MS);
    assert!(!policy.says_again(LOSS_GRACE_MS + 4_000));
    assert!(policy.says_again(LOSS_GRACE_MS + 5_000));
    assert!(!policy.says_again(LOSS_GRACE_MS + 30_000));

    // Back online before it is due: not said, and not owed to a new loss.
    let mut policy = Recovery::default();
    looks(&mut policy, 0, LOSS_GRACE_MS, GONE);
    assert!(policy.says_opened(Some(Act::OpenHotspot)));
    policy.not_played(LOSS_GRACE_MS);
    policy.step(LOSS_GRACE_MS + 2_000, BACK);
    assert!(!policy.says_again(LOSS_GRACE_MS + 5_000));
    policy.step(LOSS_GRACE_MS + 6_000, GONE);
    assert!(!policy.says_again(LOSS_GRACE_MS + 10_000));

    // A try of the saved network (the hotspot closed): not said then.
    let mut policy = Recovery::default();
    looks(&mut policy, 0, LOSS_GRACE_MS, GONE);
    assert!(policy.says_opened(Some(Act::OpenHotspot)));
    policy.not_played(LOSS_GRACE_MS + RETRY_EVERY_MS - 5_000);
    assert_eq!(
        policy.step(LOSS_GRACE_MS + RETRY_EVERY_MS, GONE),
        Some(Act::Rejoin)
    );
    assert!(!policy.says_again(LOSS_GRACE_MS + RETRY_EVERY_MS));
}

/// #5550, the reviewer-lead's P2: a lamp held on its network's radio with
/// no address never opens the hotspot, so `recover` never asks its line;
/// the Wi-Fi change's helper then says its own lost line.
#[test]
fn a_lamp_associated_with_no_address_never_says_the_hotspot_line() {
    let mut policy = Recovery::default();
    let slow = Seen {
        associated: true,
        ..GONE
    };
    for at in (0..=10 * 60_000).step_by(5_000) {
        let act = policy.step(at, slow);
        assert_eq!(act, None);
        assert!(!policy.says_line(act, at), "{at}");
    }
}

/// #5550, P3: `recover`'s one ask at each look (`says_line`): the first
/// opening for a loss, then a line not played asked again, and nothing
/// once one was played or at a reopening for the same loss.
#[test]
fn recover_asks_its_line_at_the_opening_and_again_only_when_not_played() {
    let mut policy = Recovery::default();
    let mut asked = Vec::new();
    for at in (0..=LOSS_GRACE_MS + 30_000).step_by(5_000) {
        let act = policy.step(at, GONE);
        if policy.says_line(act, at) {
            asked.push(at);
            // Refused the first time only.
            if asked.len() == 1 {
                policy.not_played(at);
            }
        }
    }
    assert_eq!(asked, [LOSS_GRACE_MS, LOSS_GRACE_MS + 5_000]);
    // A reopening after a failed try of the saved network: not asked.
    let rejoin = LOSS_GRACE_MS + RETRY_EVERY_MS;
    assert_eq!(policy.step(rejoin, GONE), Some(Act::Rejoin));
    let reopen = rejoin + JOIN_WAIT_MS;
    let act = policy.step(reopen, GONE);
    assert_eq!(act, Some(Act::OpenHotspot));
    assert!(!policy.says_line(act, reopen));
}

/// #5550, P3: a line not played is not asked again while a setup run on
/// the hotspot's page (or a Wi-Fi change) is in progress: the person is
/// already setting the network. It stays due for the next look not busy.
#[test]
fn a_line_asked_again_waits_while_the_person_is_setting_the_network() {
    let mut policy = Recovery::default();
    looks(&mut policy, 0, LOSS_GRACE_MS, GONE);
    assert!(policy.says_line(Some(Act::OpenHotspot), LOSS_GRACE_MS));
    policy.not_played(LOSS_GRACE_MS);
    let setting = Seen {
        busy: true,
        phones: Some(1),
        ..GONE
    };
    for at in [LOSS_GRACE_MS + 5_000, LOSS_GRACE_MS + 10_000] {
        let act = policy.step(at, setting);
        assert_eq!(act, None);
        assert!(!policy.says_line(act, at), "{at}");
    }
    let at = LOSS_GRACE_MS + 15_000;
    let act = policy.step(at, GONE);
    assert!(policy.says_line(act, at));
}

/// #5550: `say_line` keeps its file only for a line the voice took, and a
/// refused one is asked again.
#[test]
fn the_hotspot_line_is_kept_as_told_only_once_played() {
    let dir = std::env::temp_dir().join(format!("lamp-setup-told-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let told = dir.join("told");
    let mut policy = Recovery::default();
    looks(&mut policy, 0, LOSS_GRACE_MS, GONE);
    assert!(policy.says_line(Some(Act::OpenHotspot), LOSS_GRACE_MS));
    let mut names = Vec::new();
    lamp_setup::recover::say_line(&mut policy, LOSS_GRACE_MS, &told, |name| {
        names.push(name.to_owned());
        Some("refused: speaking".into())
    });
    assert!(!told.exists());
    assert!(policy.says_again(LOSS_GRACE_MS + 5_000));
    lamp_setup::recover::say_line(&mut policy, LOSS_GRACE_MS + 5_000, &told, |name| {
        names.push(name.to_owned());
        None
    });
    assert!(told.exists());
    assert_eq!(names, [RECORDING_OPENED, RECORDING_OPENED]);
    assert!(!policy.says_again(LOSS_GRACE_MS + 60_000));
    let _ = std::fs::remove_dir_all(&dir);
}
