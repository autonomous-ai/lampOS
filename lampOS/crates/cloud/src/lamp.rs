//! The lamp the answers read from, through the parts that own each thing:
//! the volume through `lamp-actions`' socket (its only writer), the
//! microphones and health from the bus, the build from `lamp-update`'s
//! state, the network and the host from `/proc` and `/sys`. Nothing here
//! needs root.
use crate::answer::{Lamp, Mic, Volume};
use lamp_bus::Bus;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, UdpSocket};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// How long `lamp-actions` has to answer: its own callers get a second.
const ACTIONS_WAIT: Duration = Duration::from_secs(2);

/// Where each part is read.
#[derive(Clone, Debug)]
pub struct Places {
    /// `lamp-actions`' socket (`/run/lampos-actions/call.sock`).
    pub actions: PathBuf,
    /// `/proc`, `/sys` and `/etc`, as the host has them; a test's own.
    pub proc: PathBuf,
    pub sys: PathBuf,
    pub etc: PathBuf,
}

impl Default for Places {
    fn default() -> Self {
        Self {
            actions: "/run/lampos-actions/call.sock".into(),
            proc: "/proc".into(),
            sys: "/sys".into(),
            etc: "/etc".into(),
        }
    }
}

pub struct Real {
    pub places: Places,
    pub bus: Option<Bus>,
    /// The backend's device id.
    pub device_id: String,
    /// The installed build, as `version` says it.
    pub build: String,
    /// The Wi-Fi's name, asked of `iw` within a bound and kept a while.
    pub wifi: Wifi,
}

/// How long `iw` has to say the Wi-Fi's name: the runtime that asks also
/// drives the broker's keepalive, so a hung `iw` must not hold it (Fourth on
/// #1404).
pub const IW_WITHIN: Duration = Duration::from_millis(500);
/// How long a killed `iw` has to be gone before it is left for a later ask.
pub const KILL_GONE_WITHIN: Duration = Duration::from_millis(20);
/// How long a name `iw` said is kept before it is asked again.
pub const SSID_KEPT: Duration = Duration::from_secs(60);

/// The Wi-Fi's name, from `program` (`iw`), bounded by `within` and kept for
/// [`SSID_KEPT`] per interface.
#[derive(Debug)]
pub struct Wifi {
    pub program: PathBuf,
    pub within: Duration,
    kept: Option<(Instant, String, String)>,
    /// An `iw` killed but not yet gone. One asleep in the Wi-Fi driver
    /// (waiting on `rtnl_lock` or the wiphy mutex) cannot die until the
    /// driver lets it go, so it is never waited for: it is looked at again
    /// at each ask, and while it is there no other is started (the reviewer
    /// on #1423).
    stuck: Option<Child>,
}

impl Default for Wifi {
    fn default() -> Self {
        Self {
            program: "iw".into(),
            within: IW_WITHIN,
            kept: None,
            stuck: None,
        }
    }
}

impl Wifi {
    /// The name for `iface`: kept if asked within [`SSID_KEPT`], else asked
    /// again. An `iw` that does not answer in time is killed and the name is
    /// empty, and kept as such, so it is not asked again until then. Never
    /// waits past `within`: a killed `iw` still there is reaped at a later
    /// ask, and until then the name is empty and no `iw` is started.
    pub fn ssid(&mut self, iface: &str, now: Instant) -> String {
        if let Some(child) = self.stuck.as_mut()
            && !matches!(child.try_wait(), Ok(None))
        {
            self.stuck = None;
        }
        if let Some((at, kept_iface, name)) = &self.kept
            && kept_iface == iface
            && now.saturating_duration_since(*at) < SSID_KEPT
        {
            return name.clone();
        }
        if self.stuck.is_some() {
            return String::new();
        }
        let name = match ssid(&self.program, iface, self.within) {
            Ok(name) => name,
            Err(killed) => {
                self.stuck = Some(killed);
                String::new()
            }
        };
        self.kept = Some((now, iface.to_owned(), name.clone()));
        name
    }
}

/// The build from `lamp-update`'s `state.json`: `<sequence>+<commit>`, the
/// commit's first twelve; empty when there is none.
pub fn build(state: &str) -> String {
    let Ok(state) = serde_json::from_str::<Value>(state) else {
        return String::new();
    };
    let installed = &state["installed"];
    match (installed["sequence"].as_u64(), installed["commit"].as_str()) {
        (Some(sequence), Some(commit)) => {
            format!("{sequence}+{}", commit.get(..12).unwrap_or(commit))
        }
        _ => String::new(),
    }
}

/// The default route's interface and gateway, from `/proc/net/route`.
pub fn default_route(route: &str) -> Option<(String, Ipv4Addr)> {
    route.lines().skip(1).find_map(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let (iface, destination, gateway) = (fields.first()?, fields.get(1)?, fields.get(2)?);
        if *destination != "00000000" {
            return None;
        }
        let gateway = u32::from_str_radix(gateway, 16).ok()?;
        Some(((*iface).to_owned(), Ipv4Addr::from(gateway.swap_bytes())))
    })
}

/// The address the lamp reaches `gateway` from: a UDP socket connected to
/// it says which, and sends nothing.
fn source_for(gateway: Ipv4Addr) -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect((gateway, 9)).ok()?;
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) if !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

/// Whether `child` is gone within `within`, looking every millisecond and
/// never waiting past it: reaped if so, else handed back.
fn gone_within(mut child: Child, within: Duration) -> Result<(), Child> {
    let given_up = Instant::now() + within;
    loop {
        match child.try_wait() {
            Ok(None) if Instant::now() < given_up => std::thread::sleep(Duration::from_millis(1)),
            Ok(None) => return Err(child),
            _ => return Ok(()),
        }
    }
}

/// The Wi-Fi's name, as `program` (`iw`) says it within `within`; empty on
/// ethernet or without `iw`. One that does not answer in time is killed and
/// handed back unless it is already gone: it is not waited for.
fn ssid(program: &Path, iface: &str, within: Duration) -> Result<String, Child> {
    let Ok(mut child) = Command::new(program)
        .args(["dev", iface, "link"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return Ok(String::new());
    };
    let deadline = Instant::now() + within;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
            Ok(None) => {
                // A killed process is gone within a moment, unless the
                // driver holds it: that one is handed back, not waited for.
                let _ = child.kill();
                return gone_within(child, KILL_GONE_WITHIN).map(|()| String::new());
            }
            // Not known whether it is still there: killed, and handed back
            // to be looked at again (El on #1423).
            Err(_) => {
                let _ = child.kill();
                return Err(child);
            }
        }
    }
    let mut out = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = std::io::Read::read_to_string(&mut stdout, &mut out);
    }
    Ok(out
        .lines()
        .find_map(|line| line.trim().strip_prefix("SSID: ").map(str::to_owned))
        .unwrap_or_default())
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path)
        .map(|text| text.trim().to_owned())
        .unwrap_or_default()
}

impl Real {
    pub fn hostname(&self) -> String {
        read(&self.places.proc.join("sys/kernel/hostname"))
    }

    pub fn timezone(&self) -> String {
        read(&self.places.etc.join("timezone"))
    }

    /// The default route's own address, for `local_ip`.
    pub fn local_ip(&self) -> String {
        default_route(&read(&self.places.proc.join("net/route")))
            .and_then(|(_, gateway)| source_for(gateway))
            .map(|ip| ip.to_string())
            .unwrap_or_default()
    }

    /// One call on `lamp-actions`' socket: its answer, `ok` or not.
    fn actions(&self, name: &str, args: Value) -> Result<Value, String> {
        let gone = || "the lamp's actions are not running".to_owned();
        let mut stream = UnixStream::connect(&self.places.actions).map_err(|_| gone())?;
        stream
            .set_read_timeout(Some(ACTIONS_WAIT))
            .map_err(|_| gone())?;
        stream
            .set_write_timeout(Some(ACTIONS_WAIT))
            .map_err(|_| gone())?;
        let mut line = json!({"op": "call", "name": name, "args": args}).to_string();
        line.push('\n');
        stream.write_all(line.as_bytes()).map_err(|_| gone())?;
        let mut answer = String::new();
        BufReader::new(stream)
            .read_line(&mut answer)
            .map_err(|_| "the lamp's actions did not answer".to_owned())?;
        let answer: Value = serde_json::from_str(&answer)
            .map_err(|_| "the lamp's actions said nonsense".to_owned())?;
        if answer["ok"] == true {
            Ok(answer)
        } else {
            Err(answer["error"].as_str().unwrap_or("not done").to_owned())
        }
    }

    fn level(answer: &Value) -> Result<Volume, String> {
        serde_json::from_value::<Level>(answer["level"].clone())
            .map(|level| Volume {
                volume: level.volume,
                raw: level.raw,
                max_volume: level.max_volume,
            })
            .map_err(|_| "the lamp's actions said nonsense".to_owned())
    }

    fn latest(&self, topic: &str) -> Option<lamp_bus::Message> {
        self.bus.as_ref()?.latest(topic).ok().flatten()
    }
}

#[derive(serde::Deserialize)]
struct Level {
    volume: u8,
    raw: u8,
    max_volume: u8,
}

impl Lamp for Real {
    fn versions(&mut self) -> Value {
        // `firmware` is what the app's About reads (Fourth on #1404): the
        // build, as `build` also says.
        json!({"firmware": self.build, "build": self.build, "lamp-cloud": env!("CARGO_PKG_VERSION")})
    }

    fn network(&mut self) -> Value {
        let route = default_route(&read(&self.places.proc.join("net/route")));
        let (iface, gateway) = route
            .clone()
            .map_or((String::new(), String::new()), |(i, g)| (i, g.to_string()));
        let mac = if iface.is_empty() {
            String::new()
        } else {
            read(
                &self
                    .places
                    .sys
                    .join("class/net")
                    .join(&iface)
                    .join("address"),
            )
        };
        json!({
            "private_ip": route.and_then(|(_, gateway)| source_for(gateway)).map(|ip| ip.to_string()).unwrap_or_default(),
            "interface": iface,
            "mac": mac,
            "ssid": if iface.is_empty() { String::new() } else { self.wifi.ssid(&iface, Instant::now()) },
            "gateway": gateway,
        })
    }

    fn host(&mut self) -> Value {
        let uptime = read(&self.places.proc.join("uptime"))
            .split_whitespace()
            .next()
            .and_then(|seconds| seconds.parse::<f64>().ok())
            .map_or(0, |seconds| seconds as u64);
        let hostname = self.hostname();
        json!({
            "hostname": hostname,
            "device_id": self.device_id,
            "device_name": hostname,
            "uptime_seconds": uptime,
            "timezone": self.timezone(),
        })
    }

    fn volume(&mut self) -> Result<Volume, String> {
        Self::level(&self.actions("volume_level", json!({}))?)
    }

    fn set_volume(&mut self, volume: u8) -> Result<Volume, String> {
        Self::level(&self.actions("volume_to", json!({"volume": volume}))?)
    }

    fn mic(&mut self) -> Result<Mic, String> {
        let privacy = self
            .latest(lamp_msgs::MicrophonePrivacy::TOPIC)
            .filter(|message| !message.expired)
            .and_then(|message| message.parse::<lamp_msgs::MicrophonePrivacy>().ok());
        let muted = self
            .latest(lamp_msgs::VoiceMute::TOPIC)
            .and_then(|message| message.parse::<lamp_msgs::VoiceMute>().ok())
            .is_some_and(|mute| mute.muted);
        Ok(Mic {
            muted,
            hw_switch_muted: privacy.as_ref().and_then(|privacy| privacy.physical_muted),
            // The voice renews its lease every 10 ms while it runs.
            available: privacy.is_some(),
        })
    }

    fn health(&mut self) -> Result<Value, String> {
        let message = self
            .latest(lamp_msgs::Health::TOPIC)
            .ok_or("lamp-health is not reporting")?;
        let health = message
            .parse::<lamp_msgs::Health>()
            .map_err(|_| "lamp-health said something unreadable".to_owned())?;
        Ok(json!({
            "level": health.level,
            "message": health.message,
            "stale": message.expired,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    #[test]
    fn the_build_and_the_default_route_are_read_as_written() {
        assert_eq!(
            build(
                r#"{"installed":{"sequence":812,"commit":"460a1a1aa2b3c4d5e6f7"},"previous":null}"#
            ),
            "812+460a1a1aa2b3"
        );
        assert_eq!(build("{}"), "");
        assert_eq!(build("nonsense"), "");
        let route = "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\n\
            wlan0\t0001A8C0\t00000000\t0001\t0\t0\t600\t00FFFFFF\n\
            wlan0\t00000000\t0101A8C0\t0003\t0\t0\t600\t00000000\n";
        assert_eq!(
            default_route(route),
            Some(("wlan0".into(), Ipv4Addr::new(192, 168, 1, 1)))
        );
        assert_eq!(default_route("Iface\tDestination\n"), None);
    }

    fn places(name: &str) -> (Places, PathBuf) {
        let root = PathBuf::from(format!("/tmp/lcl-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["proc/sys/kernel", "proc/net", "sys/class/net/wlan0", "etc"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        std::fs::write(root.join("proc/sys/kernel/hostname"), "lamp-7f72\n").unwrap();
        std::fs::write(root.join("proc/uptime"), "86400.52 1000.0\n").unwrap();
        std::fs::write(root.join("etc/timezone"), "Asia/Ho_Chi_Minh\n").unwrap();
        (
            Places {
                actions: root.join("call.sock"),
                proc: root.join("proc"),
                sys: root.join("sys"),
                etc: root.join("etc"),
            },
            root,
        )
    }

    /// The volume goes through `lamp-actions`' socket, in its own line
    /// protocol, and its answer is the app's numbers; with no executive
    /// running, a plain reason.
    #[test]
    fn the_volume_is_asked_of_lamp_actions_and_the_host_read_from_proc() {
        let (places, root) = places("vol");
        let listener = UnixListener::bind(&places.actions).unwrap();
        let serving = std::thread::spawn(move || {
            let mut asked = Vec::new();
            for _ in 0..2 {
                let (stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                asked.push(serde_json::from_str::<Value>(&line).unwrap());
                let mut stream = stream;
                stream
                    .write_all(
                        b"{\"ok\":true,\"level\":{\"volume\":35,\"raw\":35,\"max_volume\":100}}\n",
                    )
                    .unwrap();
            }
            asked
        });
        let mut lamp = Real {
            places: places.clone(),
            bus: None,
            device_id: "d-42".into(),
            build: "812+460a1a1aa2b3".into(),
            wifi: Wifi::default(),
        };
        let level = Volume {
            volume: 35,
            raw: 35,
            max_volume: 100,
        };
        assert_eq!(lamp.volume(), Ok(level));
        assert_eq!(lamp.set_volume(35), Ok(level));
        let asked = serving.join().unwrap();
        assert_eq!(
            asked[0],
            json!({"op": "call", "name": "volume_level", "args": {}})
        );
        assert_eq!(
            asked[1],
            json!({"op": "call", "name": "volume_to", "args": {"volume": 35}})
        );
        std::fs::remove_file(&places.actions).unwrap();
        assert_eq!(
            lamp.volume(),
            Err("the lamp's actions are not running".into())
        );
        let host = lamp.host();
        assert_eq!(
            host,
            json!({"hostname": "lamp-7f72", "device_id": "d-42", "device_name": "lamp-7f72",
                "uptime_seconds": 86400, "timezone": "Asia/Ho_Chi_Minh"})
        );
        assert_eq!(lamp.versions()["build"], "812+460a1a1aa2b3");
        // The app's About reads `firmware` (Fourth on #1404).
        assert_eq!(lamp.versions()["firmware"], "812+460a1a1aa2b3");
        // No bus: the microphones are unknown, health not reporting.
        assert_eq!(
            lamp.mic(),
            Ok(Mic {
                muted: false,
                hw_switch_muted: None,
                available: false
            })
        );
        assert!(lamp.health().is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    /// A stand-in `iw`: a script in a folder of its own that says `said`
    /// after `sleep` seconds.
    fn fake_iw(name: &str, said: &str, sleep: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("lamp-cloud-iw-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("iw");
        std::fs::write(&path, format!("#!/bin/sh\nsleep {sleep}\necho '{said}'\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// Fourth on #1404: `iw` is asked within a bound, so a hung one cannot
    /// hold the runtime that drives the keepalive; it is killed, the name is
    /// empty, and nothing is asked again until the kept minute ends.
    #[test]
    fn the_wifis_name_is_asked_within_a_bound_and_kept_a_minute() {
        // An answering `iw` is given far more than the bound, so a loaded
        // machine starting the stand-in's shell slowly never fails it (El on
        // #1423: one unexplained failure in 27 runs on a busy Mac).
        let roomy = Duration::from_secs(10);
        let answering = fake_iw("answering", "\tSSID: Example Home", "0");
        assert_eq!(
            ssid(&answering, "wlan0", roomy).ok().unwrap(),
            "Example Home"
        );
        let hung = fake_iw("hung", "\tSSID: Never", "5");
        let began = Instant::now();
        let said = ssid(&hung, "wlan0", IW_WITHIN);
        assert!(said.as_ref().is_err() || said.as_ref().is_ok_and(String::is_empty));
        if let Err(mut killed) = said {
            let _ = killed.wait();
        }
        assert!(
            began.elapsed() < Duration::from_secs(2),
            "{:?}",
            began.elapsed()
        );
        assert_eq!(
            ssid(Path::new("/nonexistent/iw"), "wlan0", IW_WITHIN)
                .ok()
                .unwrap(),
            ""
        );
        // Kept a minute per interface: a hung `iw` is not asked again then.
        let mut wifi = Wifi {
            program: answering.clone(),
            within: roomy,
            ..Wifi::default()
        };
        let now = Instant::now();
        assert_eq!(wifi.ssid("wlan0", now), "Example Home");
        wifi.program = hung.clone();
        wifi.within = IW_WITHIN;
        // Kept: the hung `iw`, asked, would have said nothing.
        assert_eq!(
            wifi.ssid("wlan0", now + SSID_KEPT - Duration::from_secs(1)),
            "Example Home"
        );
        assert_eq!(wifi.ssid("wlan0", now + SSID_KEPT), "");
        // The killed stand-in is gone by the next ask, however loaded the
        // machine, and is reaped there.
        std::thread::sleep(Duration::from_millis(200));
        wifi.program = answering.clone();
        wifi.within = roomy;
        assert_eq!(wifi.ssid("wlan1", now + SSID_KEPT), "Example Home");
        for path in [answering, hung] {
            std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
        }
    }

    /// The reviewer on #1423: after the kill, `ssid()` looks for the child
    /// for `KILL_GONE_WITHIN` and no longer. One still there (a `sleep 30`
    /// never killed stands for an `iw` asleep in the driver) is handed back
    /// in about that time, not waited for; one gone is reaped.
    #[test]
    fn a_child_still_there_after_the_kill_is_handed_back_not_waited_for() {
        let living = Command::new("sleep").arg("30").spawn().unwrap();
        let began = Instant::now();
        let mut living = gone_within(living, KILL_GONE_WITHIN).unwrap_err();
        let took = began.elapsed();
        assert!(
            took >= KILL_GONE_WITHIN && took < Duration::from_millis(500),
            "{took:?}"
        );
        assert!(matches!(living.try_wait(), Ok(None)));
        living.kill().unwrap();
        assert_eq!(gone_within(living, Duration::from_secs(5)).ok(), Some(()));
    }

    /// The reviewer on #1423: an `iw` asleep in the driver outlives its
    /// kill. It is never waited for: while it is there the name is empty at
    /// once and no other `iw` is started, and once it is gone it is reaped
    /// and `iw` is asked again. (No process here can ignore a kill, so one
    /// left running stands for it.)
    #[test]
    fn a_killed_iw_that_will_not_die_is_never_waited_for() {
        let answering = fake_iw("after-stuck", "\tSSID: Example Home", "0");
        let mut wifi = Wifi {
            program: answering.clone(),
            within: Duration::from_secs(10),
            ..Wifi::default()
        };
        wifi.stuck = Some(Command::new("sleep").arg("30").spawn().unwrap());
        let now = Instant::now();
        let began = Instant::now();
        // Not kept, not asked: empty at once, the stuck one still there.
        assert_eq!(wifi.ssid("wlan0", now), "");
        assert!(
            began.elapsed() < Duration::from_millis(100),
            "{:?}",
            began.elapsed()
        );
        assert!(wifi.stuck.is_some());
        // Gone: reaped, and `iw` is asked again.
        wifi.stuck.as_mut().unwrap().kill().unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            wifi.ssid("wlan0", now + Duration::from_secs(1)),
            "Example Home"
        );
        assert!(wifi.stuck.is_none());
        std::fs::remove_dir_all(answering.parent().unwrap()).unwrap();
    }
}
