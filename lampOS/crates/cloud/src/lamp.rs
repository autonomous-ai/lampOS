//! The lamp the answers read from, through the parts that own each thing:
//! the volume through `lamp-actions`' socket (its only writer), the
//! microphones and health from the bus, the person's light asked of the
//! ring on the bus as anyone asks it, the build from `lamp-update`'s
//! state, the network and the host from `/proc` and `/sys`. Nothing here
//! needs root.
use crate::answer::{Lamp, Mic, PREVIEW_HOLDS, Volume};
use lamp_bus::{Bus, Publisher, Subscriber};
use lamp_msgs::{ChatIn, ChatOut, Claim, Light, LightRequest, MessageOut, Priority};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, UdpSocket};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// How long `lamp-actions` has to answer: its own callers get a second.
const ACTIONS_WAIT: Duration = Duration::from_secs(2);
/// The name `lamp-cloud` asks for the light under (`light/command`'s
/// publishers, a preview's `claim.source`).
pub const SOURCE: &str = "cloud";
/// A preview's priority: over a skill's glow, under the person's own light,
/// so "make your light blue" said at the lamp ends a preview at once and it
/// stays given way while the drag goes on (the reviewer on #1464; the ring
/// gives way every request below `person` to the person's light).
pub const PREVIEW_PRIORITY: Priority = Priority::Conversation;
/// How long the ring has to show the light asked for: it looks every
/// 50 ms (`ring_light`), and the runtime that waits also drives the
/// broker's keepalive.
pub const SHOWN_WITHIN: Duration = Duration::from_secs(1);
const NO_RING: &str = "the lamp's light is not running";

/// Kept while `lamp-setup recover` runs with its switch on (#3020): the
/// same path as `lamp_setup::recover::ON_FILE`, which a test holds equal.
pub const RECOVERY_ON: &str = "/run/lampos-setup-recover/on";

/// Where each part is read.
#[derive(Clone, Debug)]
pub struct Places {
    /// `lamp-actions`' socket (`/run/lampos-actions/call.sock`).
    pub actions: PathBuf,
    /// `/proc`, `/sys` and `/etc`, as the host has them; a test's own.
    pub proc: PathBuf,
    pub sys: PathBuf,
    pub etc: PathBuf,
    /// `/run`, as the host has it; a test's own.
    pub run: PathBuf,
}

impl Default for Places {
    fn default() -> Self {
        Self {
            actions: "/run/lampos-actions/call.sock".into(),
            proc: "/proc".into(),
            sys: "/sys".into(),
            etc: "/etc".into(),
            run: "/run".into(),
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
    /// The light asked of the ring.
    pub ring: Ring,
    /// Texts from the app and the answers to them (#1673).
    pub texts: Texts,
    /// Health and crash reports, with `LAMP_REPORTS=on` (#1683).
    pub reports: Option<std::sync::Arc<crate::reports::Shared>>,
    /// Where the app's skills page reads the skills, with
    /// `LAMP_SKILLS_PAGE=on` (#1722).
    pub skills: Option<crate::skills::Places>,
    /// Names and the time zone changed after setup, with
    /// `LAMP_SETTINGS_CHANGE=on` (#1959).
    pub changes: Option<std::sync::Arc<crate::settings::Changes>>,
    /// Where a face from the app's photo goes, with `LAMP_PHOTO_FACES=on`
    /// (#1968, `crate::photos`).
    pub photos: Option<crate::photos::Place>,
    /// The eyes asked for the faces they know and to forget one, with
    /// `LAMP_FACES_PAGE=on` (#2192, `crate::faces`).
    pub faces: Option<crate::faces::Eyes>,
    /// The lamp's timers and reminders for the app, with
    /// `LAMP_APP_SCHEDULE=on` (#2195, `crate::schedule`).
    pub schedule: Option<lamp_actions::schedule::Adapter>,
    /// The lamp's software update for the app, with `LAMP_APP_UPDATE=on`
    /// (#2194, `crate::update`).
    pub update_page: bool,
    /// The app's checks for an update asked of lamp-update, with the same
    /// switch (#4153, `crate::update::Checks`).
    pub update_checks: crate::update::Checks,
    /// Setup's change helper's socket ([`crate::settings::SOCKET`]), asked
    /// to restart the lamp, with `LAMP_APP_RESTART=on` (#2193,
    /// `system.reboot`).
    pub restart: Option<PathBuf>,
    /// Setup's change helper asked to turn the lamp off, with
    /// `LAMP_SHUTDOWN=on` (#3697, `system.shutdown`).
    pub shutdown: Option<Shutdown>,
    /// lamp-skills' call socket, for guard mode from the owner's app, with
    /// `LAMP_GUARD_FROM_APP=on` (#2562, `crate::guard`).
    pub guard: Option<PathBuf>,
    /// lamp-voice's app-make socket, for a skill typed in the owner's app,
    /// with `LAMP_APP_MAKE_SKILL=on` and the skills page (#2813,
    /// `crate::make_skill`).
    pub make_skill: Option<PathBuf>,
}

/// `on` takes the app's `system.reboot` (#2193), with `LAMP_CLOUD=on`.
pub const RESTART_ENVIRONMENT: &str = "LAMP_APP_RESTART";

/// The app's `system.shutdown` (#3697): setup's change helper's socket, and
/// its yes once given, with when, so an app that asks again within
/// [`YES_KEPT`] is answered the same and the helper is asked once.
#[derive(Debug, Default)]
pub struct Shutdown {
    pub socket: PathBuf,
    pub accepted: Option<(Value, Instant)>,
}

/// How long the helper's yes to a shutdown is kept: a lamp turns off well
/// within it. One still on after it was refused by the runner (an update
/// took the lock first), so the helper is asked again, and the app hears
/// why instead of a `success` that does nothing (the Reviewer-lead's P3 on
/// #3735).
pub const YES_KEPT: Duration = Duration::from_secs(60);

/// `on` takes the app's `system.shutdown` (#3697): `lamp_setup::shutdown::SWITCH`,
/// named here as lamp-cloud doesn't build `lamp-setup`.
pub const SHUTDOWN_ENVIRONMENT: &str = "LAMP_SHUTDOWN";

impl Shutdown {
    /// Asked on setup's change helper's socket, with `LAMP_SHUTDOWN=on`.
    pub fn from_environment() -> Option<Self> {
        let value = std::env::var(SHUTDOWN_ENVIRONMENT).ok();
        crate::reports::switched_on(value.as_deref()).then(|| Self {
            socket: crate::settings::SOCKET.into(),
            accepted: None,
        })
    }
}

/// One restart or shutdown request to setup's change helper, in its line
/// protocol (`lamp_keeper::restart`'s requests).
fn ask_power(socket: &std::path::Path, request: &str) -> Result<Value, String> {
    crate::settings::ask(socket, request, RESTART_WAIT).map(Option::unwrap_or_default)
}

/// The helper reads one line and checks one lock before it answers: bounded
/// short, as the broker's loop waits on it (`crate::photos::DEADLINE`).
pub const RESTART_WAIT: Duration = Duration::from_secs(2);

/// Texts from the app (`crate::chat`, #1673): whether they are taken, the
/// bus's `chat/in` and `chat/out`, made at first use, and the runs waited
/// on.
#[derive(Default)]
pub struct Texts {
    pub on: bool,
    input: Option<Publisher>,
    output: Option<Subscriber>,
    stream: crate::chat::Stream,
    /// The skills' lines for the phone (`messages/out`, `crate::unasked`),
    /// listened for from the start, and those waiting to go.
    unasked: Option<Subscriber>,
    outbox: crate::unasked::Outbox,
    /// The eyes' known file (`LAMP_CLOUD_KNOWN_FILE`): beside it, their
    /// `tellable`, read as a named text goes (#2562). None: no text goes
    /// named.
    known_file: Option<PathBuf>,
}

/// Where the eyes' known file is read from, for guard mode's named texts
/// (#2562): the same file as `lamp-skills`' `LAMP_SKILLS_KNOWN_FILE`.
pub const KNOWN_FILE_ENVIRONMENT: &str = "LAMP_CLOUD_KNOWN_FILE";

impl Texts {
    /// Taken, or not, as `LAMP_MESSAGES` says.
    pub fn on(on: bool) -> Self {
        Self {
            on,
            ..Self::default()
        }
    }

    /// The eyes' known file, beside which their `tellable` is read as a
    /// named text goes.
    pub fn knowing(mut self, known_file: Option<PathBuf>) -> Self {
        self.known_file = known_file;
        self
    }
}

/// What `lamp-cloud` has asked of the ring: its publishers, made at first
/// use, and the preview it shows until given back.
#[derive(Default)]
pub struct Ring {
    command: Option<Publisher>,
    request: Option<Publisher>,
    previewing: Option<Light>,
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

    /// The lamp's name in the app: the one the person gave it after setup
    /// (`device.rename`, #1959), else the host's name, as before.
    pub fn device_name(&self) -> String {
        crate::settings::display_name(&read(&self.places.etc.join("lampOS/setup/display-name")))
            .unwrap_or_else(|| self.hostname())
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

    /// The host's boot, the clock a preview's claim is on (the ring's own).
    fn boot(&self) -> Result<String, String> {
        Some(read(&self.places.proc.join("sys/kernel/random/boot_id")))
            .filter(|boot| !boot.is_empty())
            .ok_or("the host's boot cannot be read".to_owned())
    }

    /// Asks the ring for `light` over the person's until `until_us` on the
    /// host's monotonic clock; a time already passed gives it back.
    fn request(&mut self, light: &Light, until_us: u64) -> Result<(), String> {
        let clock_id = self.boot()?;
        let bus = self.bus.as_ref().ok_or(NO_RING)?;
        let out = match &mut self.ring.request {
            Some(out) => out,
            empty => empty.insert(
                bus.publisher_declared(LightRequest::TOPIC, &LightRequest::declaration(), SOURCE)
                    .map_err(|_| "the light cannot be asked for here".to_owned())?,
            ),
        };
        let request = LightRequest {
            claim: Claim {
                source: SOURCE.into(),
                priority: PREVIEW_PRIORITY,
                clock_id,
                until_us,
            },
            light: light.clone(),
            pattern: None,
            fade_ms: None,
        };
        out.publish_json(lamp_ipc::monotonic_us(), &request)
            .map(|_| ())
            .map_err(|_| "the light could not be asked for".to_owned())
    }
}

/// `system.info`'s `reset` from the controls' pins at `pins` (#2937):
/// `hold` with a `reset` line (a product lamp), `none` without (a lamp moved
/// from V1, whose pins never carry one); none for a file that can't be read
/// as an object, so the app reads it as before.
fn reset_of(pins: &std::path::Path) -> Option<&'static str> {
    let pins: serde_json::Map<String, Value> =
        serde_json::from_str(&std::fs::read_to_string(pins).ok()?).ok()?;
    Some(if pins.contains_key("reset") {
        "hold"
    } else {
        "none"
    })
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
            "device_name": self.device_name(),
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

    fn light(&mut self) -> Result<Light, String> {
        self.latest(Light::TOPICS[1])
            .filter(|message| !message.expired)
            .ok_or(NO_RING)?
            .parse::<Light>()
            .map_err(|_| "the lamp's light said something unreadable".to_owned())
    }

    /// A preview still shown is given back first, so the light saved is the
    /// one seen (V1's save cancels its preview).
    fn set_light(&mut self, light: &Light) -> Result<Light, String> {
        if let Some(shown) = self.ring.previewing.take() {
            let _ = self.request(&shown, lamp_ipc::monotonic_us().saturating_sub(1));
        }
        let bus = self.bus.as_ref().ok_or(NO_RING)?;
        let out = match &mut self.ring.command {
            Some(out) => out,
            empty => empty.insert(
                bus.publisher_declared(Light::TOPICS[0], &Light::declaration(), SOURCE)
                    .map_err(|_| "the light cannot be asked for here".to_owned())?,
            ),
        };
        out.publish_json(lamp_ipc::monotonic_us(), light)
            .map_err(|_| "the light could not be asked for".to_owned())?;
        let asked = Instant::now();
        loop {
            if self.light().as_ref() == Ok(light) {
                return Ok(light.clone());
            }
            if asked.elapsed() >= SHOWN_WITHIN {
                return Err("the light was asked for, but the ring did not show it".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Only where the ring takes lights that lapse (`LAMP_LIGHT_REQUESTS=on`,
    /// the product's): elsewhere a preview would show nothing, and is
    /// refused.
    fn preview_light(&mut self, light: &Light) -> Result<(), String> {
        let bus = self.bus.as_ref().ok_or(NO_RING)?;
        let taken = bus
            .subscribers(LightRequest::TOPIC)
            .is_ok_and(|listeners| listeners.iter().any(|listener| listener.listening));
        if !taken {
            return Err("this lamp's light takes no previews".into());
        }
        let until_us = lamp_ipc::monotonic_us() + PREVIEW_HOLDS.as_micros() as u64;
        self.request(light, until_us)?;
        self.ring.previewing = Some(light.clone());
        Ok(())
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

    fn recovery(&self) -> bool {
        self.places
            .run
            .join(RECOVERY_ON.trim_start_matches("/run/"))
            .exists()
    }

    fn reset(&self) -> Option<&'static str> {
        reset_of(&self.places.etc.join("lampOS/controls/controls-pins.json"))
    }

    fn texting(&self) -> bool {
        self.texts.on && self.bus.is_some()
    }

    fn offline_will(&self) -> bool {
        std::env::var(crate::online::OFFLINE_WILL_ENVIRONMENT).is_ok_and(|value| value == "on")
    }

    fn reports(&self) -> Option<std::sync::Arc<crate::reports::Shared>> {
        self.reports.clone()
    }

    fn skills(&self) -> Option<crate::skills::Places> {
        self.skills.clone()
    }

    fn changes(&self) -> Option<std::sync::Arc<crate::settings::Changes>> {
        self.changes.clone()
    }

    fn photos(&self) -> Option<crate::photos::Place> {
        self.photos.clone()
    }

    fn faces(&self) -> Option<crate::faces::Eyes> {
        self.faces.clone()
    }

    fn schedule(&self) -> Option<&lamp_actions::schedule::Adapter> {
        self.schedule.as_ref()
    }

    fn update_page(&self) -> bool {
        self.update_page
    }

    fn restarts(&self) -> bool {
        self.restart.is_some()
    }

    fn shuts_down(&self) -> bool {
        self.shutdown.is_some()
    }

    fn guard(&self) -> Option<PathBuf> {
        self.guard.clone()
    }

    fn make_skill(&self) -> Option<PathBuf> {
        self.make_skill.clone()
    }

    fn update_status(&self) -> Option<lamp_bus::Message> {
        self.latest(lamp_msgs::UpdateStatus::TOPIC)
    }

    fn check_update(&mut self, now: Instant) -> Result<(), String> {
        self.update_checks.ask(self.bus.as_ref(), now)
    }

    fn restart(&mut self) -> Result<Value, String> {
        let socket = self
            .restart
            .as_deref()
            .ok_or_else(|| crate::wire::not_here("system.reboot"))?;
        ask_power(socket, lamp_keeper::restart::REQUEST)
    }

    fn shut_down(&mut self) -> Result<Value, String> {
        let shutdown = self
            .shutdown
            .as_mut()
            .ok_or_else(|| crate::wire::not_here("system.shutdown"))?;
        if let Some((accepted, at)) = &shutdown.accepted
            && at.elapsed() < YES_KEPT
        {
            return Ok(accepted.clone());
        }
        let accepted = ask_power(&shutdown.socket, lamp_keeper::restart::SHUTDOWN_REQUEST)?;
        shutdown.accepted = Some((accepted.clone(), Instant::now()));
        Ok(accepted)
    }

    fn new_run(&mut self, now_ms: u64) -> String {
        self.texts.stream.new_run(now_ms)
    }

    fn send_text(&mut self, text: &ChatIn, now_ms: u64) -> Result<(), String> {
        let bus = self.bus.as_ref().ok_or(crate::chat::NOBODY)?;
        // Listened for before the text goes, so no piece of its answer is
        // missed.
        if self.texts.output.is_none() {
            self.texts.output = Some(
                bus.subscribe_typed(ChatOut::TOPIC, SOURCE, ChatOut::TYPE)
                    .map_err(|_| crate::chat::NOBODY.to_owned())?,
            );
        }
        let input = match &mut self.texts.input {
            Some(input) => input,
            empty => empty.insert(
                bus.publisher_declared(ChatIn::TOPIC, &ChatIn::declaration(), SOURCE)
                    .map_err(|_| crate::chat::NOBODY.to_owned())?,
            ),
        };
        let published = input
            .publish_json(lamp_ipc::monotonic_us(), text)
            .map_err(|_| crate::chat::NOBODY.to_owned())?;
        // Nobody listening: the app is told at once, not after a minute.
        if published.delivered == 0 {
            return Err(crate::chat::NOBODY.to_owned());
        }
        self.texts.stream.acked(text, now_ms);
        Ok(())
    }

    fn unasked(&mut self, now_ms: u64, give: bool) -> crate::unasked::Heard {
        if !self.texts.on {
            return crate::unasked::Heard::default();
        }
        if self.texts.unasked.is_none()
            && let Some(bus) = self.bus.as_ref()
        {
            self.texts.unasked = bus
                .subscribe_typed(MessageOut::TOPIC, SOURCE, MessageOut::TYPE)
                .ok();
        }
        let mut heard = crate::unasked::Heard::default();
        let texts = &mut self.texts;
        if let Some(input) = texts.unasked.as_mut() {
            while let Ok(Some(message)) = input.recv(Duration::ZERO) {
                let Ok(out) = message.parse::<MessageOut>() else {
                    continue;
                };
                let header = &message.header;
                let stream = &mut texts.stream;
                let taken = texts.outbox.take(
                    (&header.source, &header.boot, header.seq),
                    &out,
                    now_ms,
                    |text| stream.unasked(text, now_ms),
                );
                heard.lines.extend(crate::unasked::dropped_line(
                    &crate::unasked::skill_of(&out.from),
                    &taken,
                ));
            }
        }
        if give {
            // Read now, as it goes on to the broker: a yes taken back while
            // the text waited on the bus names nobody (#2562).
            let known = texts.known_file.as_deref();
            heard.due = texts
                .outbox
                .due_checked(|named| lamp_msgs::known::still_tellable(known, named));
        } else {
            // The broker is away: what waits is held, with no name (#2562).
            texts.outbox.hold();
        }
        heard
            .lines
            .extend(texts.outbox.held_lines().into_iter().map(str::to_owned));
        heard
    }

    fn owe_unasked(&mut self, pieces: Vec<crate::unasked::Piece>) {
        self.texts.outbox.owe(pieces);
    }

    fn unasked_sent(&mut self) -> u64 {
        self.texts.outbox.sent()
    }

    fn chat_events(&mut self, now_ms: u64) -> Vec<Value> {
        // Task news comes with no text asked first: listened for from the
        // start.
        if self.texts.output.is_none()
            && let Some(bus) = self.bus.as_ref()
        {
            self.texts.output = bus
                .subscribe_typed(ChatOut::TOPIC, SOURCE, ChatOut::TYPE)
                .ok();
        }
        let mut sent = Vec::new();
        if let Some(output) = self.texts.output.as_mut() {
            while let Ok(Some(message)) = output.recv(Duration::ZERO) {
                if let Ok(out) = message.parse::<ChatOut>()
                    && out.check().is_ok()
                {
                    sent.extend(self.texts.stream.piece(&out, now_ms));
                }
            }
        }
        sent.extend(self.texts.stream.due(now_ms));
        sent
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
                run: root.join("run"),
            },
            root,
        )
    }

    /// #3020: `recovery` while `lamp-setup recover` runs with its switch on,
    /// which keeps its file in its runtime folder; not otherwise.
    #[test]
    fn recovery_is_read_from_setups_file() {
        let (places, root) = places("recovery");
        let lamp = Real {
            places: places.clone(),
            bus: None,
            device_id: "d-42".into(),
            build: "812+460a1a1aa2b3".into(),
            wifi: Wifi::default(),
            ring: Ring::default(),
            texts: Texts::default(),
            reports: None,
            skills: None,
            changes: None,
            photos: None,
            faces: None,
            schedule: None,
            update_page: false,
            update_checks: Default::default(),
            restart: None,
            guard: None,
            make_skill: None,
            shutdown: None,
        };
        assert!(!lamp.recovery());
        let on = places.run.join("lampos-setup-recover/on");
        std::fs::create_dir_all(on.parent().unwrap()).unwrap();
        std::fs::write(&on, "").unwrap();
        assert!(lamp.recovery());
        let _ = std::fs::remove_dir_all(root);
        assert_eq!(RECOVERY_ON, lamp_setup::recover::ON_FILE);
        assert_eq!(SHUTDOWN_ENVIRONMENT, lamp_setup::shutdown::SWITCH);
    }

    /// #2937: the reset the app is told, from the controls' pins: `hold`
    /// with a reset line, `none` without one (a lamp moved from V1), and
    /// nothing when the pins can't be read.
    #[test]
    fn the_reset_is_read_from_the_controls_pins() {
        let (places, root) = places("reset");
        let pins = places.etc.join("lampOS/controls/controls-pins.json");
        assert_eq!(reset_of(&pins), None);
        std::fs::create_dir_all(pins.parent().unwrap()).unwrap();
        std::fs::write(
            &pins,
            "{\"chip\":\"/dev/gpiochip0\",\"reset\":{\"name\":\"reset\",\"line\":100}}",
        )
        .unwrap();
        assert_eq!(reset_of(&pins), Some("hold"));
        std::fs::write(&pins, "{\"chip\":\"/dev/gpiochip0\",\"legacy_head\":true}").unwrap();
        assert_eq!(reset_of(&pins), Some("none"));
        std::fs::write(&pins, "[1]").unwrap();
        assert_eq!(reset_of(&pins), None);
        let _ = std::fs::remove_dir_all(root);
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
            ring: Ring::default(),
            texts: Texts::default(),
            reports: None,
            skills: None,
            changes: None,
            photos: None,
            faces: None,
            schedule: None,
            update_page: false,
            update_checks: Default::default(),
            restart: None,
            guard: None,
            make_skill: None,
            shutdown: None,
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
        // The lamp's name in the app, once the person gave it one (#1959):
        // only `device_name`; the host's own name stays.
        std::fs::create_dir_all(places.etc.join("lampOS/setup")).unwrap();
        std::fs::write(places.etc.join("lampOS/setup/display-name"), "Bàn học\n").unwrap();
        let host = lamp.host();
        assert_eq!(
            (&host["device_name"], &host["hostname"]),
            (&json!("Bàn học"), &json!("lamp-7f72"))
        );
        std::fs::write(places.etc.join("lampOS/setup/display-name"), "\n").unwrap();
        assert_eq!(lamp.host()["device_name"], "lamp-7f72");
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

    /// A stand-in ring on a bus of its own: it shows each light asked for on
    /// `light/command` (saying it on `light/state`, as the ring does, every
    /// 100 ms) and keeps each `light/request`, until `stop` is set.
    fn ring(
        bus: &Bus,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> std::thread::JoinHandle<Vec<LightRequest>> {
        let mut asked = bus
            .subscribe_typed(Light::TOPICS[0], "ring", Light::TYPE)
            .unwrap();
        let mut requests = bus
            .subscribe_typed(LightRequest::TOPIC, "ring", LightRequest::TYPE)
            .unwrap();
        let mut state = bus
            .publisher_declared(Light::TOPICS[1], &Light::declaration(), "ring")
            .unwrap();
        std::thread::spawn(move || {
            let mut shown = crate::light::first();
            let mut kept = Vec::new();
            let mut said = Instant::now() - Duration::from_secs(1);
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                while let Ok(Some(message)) = requests.recv(Duration::ZERO) {
                    kept.push(message.parse::<LightRequest>().unwrap());
                }
                if let Ok(Some(message)) = asked.recv(Duration::from_millis(5)) {
                    shown = message.parse::<Light>().unwrap();
                    said = Instant::now() - Duration::from_secs(1);
                }
                if said.elapsed() >= Duration::from_millis(100) {
                    state
                        .publish_json(lamp_ipc::monotonic_us(), &shown)
                        .unwrap();
                    said = Instant::now();
                }
            }
            // One asked just before the light it went with, read together.
            while let Ok(Some(message)) = requests.recv(Duration::ZERO) {
                kept.push(message.parse::<LightRequest>().unwrap());
            }
            kept
        })
    }

    /// The first builder's 10:2x on #705: the app's Light row on the
    /// person's light. Saved, it is asked on `light/command` and read back
    /// from `light/state`; previewed, it is asked on `light/request` under
    /// the person's priority for ten seconds on the host's boot clock, and
    /// given back before a save. A lamp whose ring takes no previews says
    /// so, and a ring that does not show a light in time is said.
    #[test]
    fn the_persons_light_is_asked_of_the_ring_and_a_preview_lapses_or_is_given_back() {
        let (places, root) = places("light");
        std::fs::create_dir_all(places.proc.join("sys/kernel/random")).unwrap();
        std::fs::write(places.proc.join("sys/kernel/random/boot_id"), "boot-test\n").unwrap();
        let bus_root = root.join("bus");
        std::fs::create_dir_all(&bus_root).unwrap();
        let mut lamp = Real {
            places,
            bus: Some(Bus::new(&bus_root).unwrap()),
            device_id: "d-42".into(),
            build: String::new(),
            wifi: Wifi::default(),
            ring: Ring::default(),
            texts: Texts::default(),
            reports: None,
            skills: None,
            changes: None,
            photos: None,
            faces: None,
            schedule: None,
            update_page: false,
            update_checks: Default::default(),
            restart: None,
            guard: None,
            make_skill: None,
            shutdown: None,
        };
        let blue = Light::new(true, "blue", 3).unwrap();
        // No ring yet.
        assert_eq!(lamp.light(), Err(NO_RING.to_owned()));
        assert_eq!(
            lamp.preview_light(&blue),
            Err("this lamp's light takes no previews".to_owned())
        );
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let ringing = ring(&Bus::new(&bus_root).unwrap(), stop.clone());
        let began = Instant::now();
        while lamp.light().is_err() && began.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(lamp.light(), Ok(crate::light::first()));
        let pink = Light::new(true, "pink", 1).unwrap();
        let before = lamp_ipc::monotonic_us();
        assert_eq!(lamp.preview_light(&pink), Ok(()));
        let after = lamp_ipc::monotonic_us();
        assert_eq!(lamp.set_light(&blue), Ok(blue.clone()));
        assert_eq!(lamp.light(), Ok(blue.clone()));
        let saved = Instant::now();
        let dropped = Light::new(true, "red", 2).unwrap();
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let requests = ringing.join().unwrap();
        // A ring that has stopped shows nothing asked of it: said in time.
        assert!(lamp.set_light(&dropped).is_err());
        assert!(saved.elapsed() < SHOWN_WITHIN + Duration::from_millis(500));
        assert_eq!(requests.len(), 2, "{requests:?}");
        let (shown, given_back) = (&requests[0], &requests[1]);
        assert_eq!(shown.light, pink);
        assert_eq!(given_back.light, pink);
        for request in &requests {
            assert_eq!(request.claim.source, "cloud");
            assert_eq!(request.claim.priority, PREVIEW_PRIORITY);
            assert_eq!(request.claim.clock_id, "boot-test");
        }
        let holds = PREVIEW_HOLDS.as_micros() as u64;
        assert!((before + holds..=after + holds).contains(&shown.claim.until_us));
        assert!(given_back.claim.until_us < lamp_ipc::monotonic_us());
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
        // The bound itself is shown above: here only that the name is
        // empty, at a test's 50 ms, not 500 (#1806).
        wifi.within = Duration::from_millis(50);
        // Kept: the hung `iw`, asked, would have said nothing.
        assert_eq!(
            wifi.ssid("wlan0", now + SSID_KEPT - Duration::from_secs(1)),
            "Example Home"
        );
        assert_eq!(wifi.ssid("wlan0", now + SSID_KEPT), "");
        // The killed stand-in is reaped at an ask once it is gone, and no
        // other is started while it is there: asked until it answers, not
        // after a fixed 200 ms (#1806).
        wifi.program = answering.clone();
        wifi.within = roomy;
        // Well short of the stand-in's own `sleep 5`, so one never killed
        // fails here.
        let until = Instant::now() + Duration::from_secs(2);
        while wifi.ssid("wlan1", now + SSID_KEPT).is_empty() {
            assert!(Instant::now() < until, "the killed stand-in never went");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(wifi.stuck.is_none());
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
