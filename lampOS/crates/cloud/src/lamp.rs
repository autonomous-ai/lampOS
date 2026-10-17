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
use std::process::{Command, Stdio};
use std::time::Duration;

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

/// The Wi-Fi's name, as `iw` says it; empty on ethernet or without `iw`.
fn ssid(iface: &str) -> String {
    let Ok(output) = Command::new("iw")
        .args(["dev", iface, "link"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return String::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.trim().strip_prefix("SSID: ").map(str::to_owned))
        .unwrap_or_default()
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
        json!({"build": self.build, "lamp-cloud": env!("CARGO_PKG_VERSION")})
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
            "ssid": if iface.is_empty() { String::new() } else { ssid(&iface) },
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
}
