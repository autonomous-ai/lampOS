//! The lamp's own light on the bus (`docs/bus.md`): the ring's owner takes
//! what is asked for on `light/command` and says what it shows on
//! `light/state`, both `lamp_msgs::Light`.
//!
//! Whoever wants the light changed (the actions program, when someone
//! says "make your light blue") publishes the light wanted and reads the
//! state back; it never writes the ring. The newest wanted light is kept
//! by the bus, so a ring that starts later, or again, takes up the light
//! that was last asked for.
//!
//! The self-test asks for a colour on `light/test`, not `light/command`:
//! it is shown for [`TEST_HOLDS_US`] at most, then the person's light
//! again, and any `light/command` ends it at once. So a test never writes
//! over the person's light, and `light/state` goes on saying the person's.
//! What the ring actually drew is said on `light/drawn`, from the ring's
//! own write receipts: a draw it refused (the privacy switch, a turn)
//! says nothing (#812).
//!
//! With `LAMP_LIGHT_REQUESTS=on` (#940) the ring also takes lights asked
//! for that lapse, on `light/request` ([`crate::ring_requests`]): the
//! highest priority that holds is shown over the person's light, and the
//! person's light again once none does. `light/state` goes on saying the
//! person's light, and `light/drawn` what was drawn. Once a request's light
//! has been shown on resting frames for [`REACHED_AFTER_US`] running, the
//! ring says so on `light/reached`, naming its asker, and again each time
//! the asker renews it while it is shown, so a skill can wait for the light
//! it asked for. A turn's face has colours of its own and never shows the
//! lamp's light, so nothing is reached inside a turn (the reviewer on #1166).
//! A request the ring holds but does not show, because another request or
//! the person's light is over it, is said on `light/outranked`, naming
//! what is over it, so its asker is told it lost (#940).
//!
//! With `LAMP_SKILL_LIGHT_SPEAKING=on` as well (El on #1722), a request's
//! light shown also tints the Speaking face ([`RingLight::held`],
//! [`crate::ring_face::Face::skill_light`]), so a skill's light held through
//! its words is seen with them; the other faces of a turn are their own.
//!
//! With `LAMP_LIGHT_KEEP=on` (the first builder on #1464) the person's light
//! is also kept on disk ([`KEEP_PATH`]), written when it changes, at most
//! once a second, so a lamp set warm at level 3 wakes up at level 3: the bus
//! keeps it only in `/run`, which a restart empties. At start the bus's
//! value comes first, so a light asked for since is never undone, then the
//! kept one, then the first. Only the person's light is kept: a test, a
//! request or a turn's face never is.
//!
//! Nothing here can stop the ring. A bus that cannot be used leaves the
//! lamp with the light it has, and a request that is not a light there is
//! (a colour that does not exist, a level outside 1 to 5) is passed over.
use crate::{ring_face::Lit, ring_requests::Requests};
use lamp_bus::{Bus, Publisher, Subscriber};
use lamp_msgs::{Light, LightOutranked, LightPattern, LightReached, LightRequest, PatternKind};
use std::{io, path::Path, time::Duration};

/// The name the ring's owner publishes under.
pub const SOURCE: &str = "ring";
/// What is asked for is looked at this often: on about every pass of the
/// ring's loop (`ring_worker`'s 2 ms; a pass that drew a cue frame reads
/// its clock late, so the next can come under 2 ms and wait a pass, which
/// only happens inside a turn, whose face does not show the light; El on
/// #1900). `lamp-actions` answers a light call only
/// once this says it shows the light, and the model speaks after that
/// answer, so the person hears this wait on every light call. It was 50 ms
/// (#1806).
const LOOK_EVERY_US: u64 = 2_000;
/// What is shown is said this often when nothing changes, as the topic
/// promises, and a state means nothing this long after it was said: a
/// ring that has stopped is not a light that can be asked for.
const SAY_EVERY_US: u64 = 1_000_000;
const STATE_LASTS: Duration = Duration::from_secs(3);
/// The topic a self-test asks for a colour on.
pub const TEST_TOPIC: &str = "light/test";
/// The topic what was drawn is said on.
pub const DRAWN_TOPIC: &str = "light/drawn";
/// A test colour is shown this long at most.
pub const TEST_HOLDS_US: u64 = 2_000_000;
/// At most this many requests are taken in one look.
const MOST_AT_ONCE: usize = 8;
/// A request's light counts as reached once resting frames have shown it
/// for this long running: five of the face's 70 ms fade times
/// (`ring_face::EASE_US`), so 99% of the way.
pub const REACHED_AFTER_US: u64 = 350_000;
/// Frames further apart than this begin the count again: the face eases at
/// most 100 ms in one step (`ring_face::LONGEST_STEP_US`), so after a stall
/// the light is not as far along as the time says.
const FRAMES_APART_US: u64 = 100_000;
/// The switch for lights asked for that lapse (#940), off unless `on`.
pub const REQUESTS_ENVIRONMENT: &str = "LAMP_LIGHT_REQUESTS";
const REQUESTS_FLAG: &str = "--light-requests";
/// The switch for a skill's light held tinting the Speaking face (El on
/// #1722), off unless `on`; it needs [`REQUESTS_ENVIRONMENT`].
pub const SPEAKING_ENVIRONMENT: &str = "LAMP_SKILL_LIGHT_SPEAKING";
const SPEAKING_FLAG: &str = "--skill-light-speaking";
/// The switch for keeping the person's light across a restart, off unless
/// `on`, and where it is kept (the reset wipes it).
pub const KEEP_ENVIRONMENT: &str = "LAMP_LIGHT_KEEP";
pub const KEEP_PATH: &str = "/var/lib/lampOS/light/person.json";
/// The switch for the person's light while the voice waits out a mute or a
/// closed privacy switch (#1816), off unless `on`: `light/state` said and
/// `light/command` taken with no session, the ring left as the wait has it.
pub const WHILE_MUTED_ENVIRONMENT: &str = "LAMP_LIGHT_WHILE_MUTED";
const KEEP_FLAG: &str = "--light-keep";
/// A dev lamp's switch, off unless `on`: every pulse asked for is played,
/// not one a minute per asker ([`LightPattern::PULSE_GAP_US`]), so a demo
/// can pulse as often as it asks. Never on a product: there twice in a row
/// is a nag (the first builder on #940).
pub const EVERY_PULSE_ENVIRONMENT: &str = "LAMP_RING_EVERY_PULSE";
const EVERY_PULSE_FLAG: &str = "--every-pulse";
/// The kept light is written at most this often.
pub const KEEP_EVERY_US: u64 = 1_000_000;

/// The ring worker's flag for lights asked for that lapse, with
/// `LAMP_LIGHT_REQUESTS=on`.
pub fn arguments_from_environment() -> Vec<String> {
    requests_arguments(std::env::var(REQUESTS_ENVIRONMENT).ok().as_deref())
}

fn requests_arguments(value: Option<&str>) -> Vec<String> {
    match value {
        Some("on") => vec![REQUESTS_FLAG.to_owned()],
        _ => Vec::new(),
    }
}

/// Take a leading `--light-requests` off the ring worker's flags.
pub fn take(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == REQUESTS_FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// The ring worker's flag for a skill's light on the Speaking face, with
/// `LAMP_SKILL_LIGHT_SPEAKING=on`.
pub fn speaking_arguments_from_environment() -> Vec<String> {
    speaking_arguments(std::env::var(SPEAKING_ENVIRONMENT).ok().as_deref())
}

fn speaking_arguments(value: Option<&str>) -> Vec<String> {
    match value {
        Some("on") => vec![SPEAKING_FLAG.to_owned()],
        _ => Vec::new(),
    }
}

/// The ring worker's flag for every pulse played, with
/// `LAMP_RING_EVERY_PULSE=on` (a dev lamp's switch).
pub fn every_pulse_arguments_from_environment() -> Vec<String> {
    every_pulse_arguments(std::env::var(EVERY_PULSE_ENVIRONMENT).ok().as_deref())
}

fn every_pulse_arguments(value: Option<&str>) -> Vec<String> {
    match value {
        Some("on") => vec![EVERY_PULSE_FLAG.to_owned()],
        _ => Vec::new(),
    }
}

/// Take a leading `--every-pulse` off the ring worker's flags, after
/// [`take_keep`].
pub fn take_every_pulse(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == EVERY_PULSE_FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// Take a leading `--skill-light-speaking` off the ring worker's flags,
/// after [`take`].
pub fn take_speaking(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == SPEAKING_FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// The ring worker's flag for keeping the person's light, with
/// `LAMP_LIGHT_KEEP=on`.
pub fn keep_arguments_from_environment() -> Vec<String> {
    keep_arguments(std::env::var(KEEP_ENVIRONMENT).ok().as_deref())
}

fn keep_arguments(value: Option<&str>) -> Vec<String> {
    match value {
        Some("on") => vec![KEEP_FLAG.to_owned(), KEEP_PATH.to_owned()],
        _ => Vec::new(),
    }
}

/// Take a leading `--light-keep PATH` off the ring worker's flags.
pub fn take_keep(flags: &[String]) -> (Option<&Path>, &[String]) {
    match flags {
        [flag, path, rest @ ..] if flag == KEEP_FLAG => (Some(Path::new(path)), rest),
        _ => (None, flags),
    }
}

/// The person's light for the voice's muted wait (#1816), with
/// [`WHILE_MUTED_ENVIRONMENT`] on, a ring (`ring`) and the bus on: the ring
/// worker, which owns it in a session, does not run while the microphones
/// are off, so the app's light row found the light not running. The same
/// light as the ring worker's, kept on disk where [`KEEP_ENVIRONMENT`] says,
/// with no requests; a self-test colour is read but, like everything here,
/// never drawn. It draws nothing: whatever the wait shows on
/// the ring (the privacy face, the asleep face, dark) stays. A bus that
/// cannot be used is said once, and the wait goes on without it.
pub fn while_muted_from_environment(ring: bool) -> Option<RingLight> {
    let switch = std::env::var(WHILE_MUTED_ENVIRONMENT).ok();
    let flags = crate::bus_out::worker_arguments_from_environment();
    let root = crate::bus_out::take_root(&flags).ok()?.0;
    let keep = std::env::var(KEEP_ENVIRONMENT).ok();
    while_muted(switch.as_deref(), ring, root.as_deref(), keep.as_deref())
}

fn while_muted(
    switch: Option<&str>,
    ring: bool,
    root: Option<&Path>,
    keep: Option<&str>,
) -> Option<RingLight> {
    if switch != Some("on") || !ring {
        return None;
    }
    let keep = (keep == Some("on")).then(|| Path::new(KEEP_PATH));
    match RingLight::on_keeping(root?, keep) {
        Ok((light, _)) => Some(light),
        Err(error) => {
            eprintln!("lamp-voice: the person's light is not said while muted: {error}");
            None
        }
    }
}

/// The light kept at `path`, if it is one there is.
fn kept_on_disk(path: &Path) -> Option<Light> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<Light>(&text)
        .ok()
        .filter(|light| lit(light).is_some())
}

/// Write `light` to `path` whole: a file beside it, then a rename.
fn keep_on_disk(path: &Path, light: &Light) -> io::Result<()> {
    let mut beside = path.as_os_str().to_owned();
    beside.push(".new");
    let beside = std::path::PathBuf::from(beside);
    std::fs::write(
        &beside,
        serde_json::to_vec(light).map_err(io::Error::other)?,
    )?;
    std::fs::rename(&beside, path)
}

/// Where the person's light is kept, what was last written and when, and
/// whether the last write failed (said once until one succeeds).
struct Keep {
    path: std::path::PathBuf,
    written: Option<Light>,
    written_at_us: Option<u64>,
    failing: bool,
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The light before anyone has asked for one, as the message.
pub fn first() -> Light {
    Light {
        on: true,
        colour: "warm".into(),
        level: 1,
    }
}

/// A message as the face's own light, if it is one there is.
pub fn lit(light: &Light) -> Option<Lit> {
    light.check().ok()?;
    Lit::of(light.on, &light.colour, light.level)
}

pub struct RingLight {
    asked: Subscriber,
    shown: Publisher,
    tests: Subscriber,
    drawn: Publisher,
    now: Light,
    /// A self-test's colour and until when it is shown.
    testing: Option<(Light, u64)>,
    /// When the newest light the person asked for was asked (wall clock,
    /// as the bus stamps it). A test asked no later is never shown.
    asked_at_ms: u64,
    looked_at_us: u64,
    said_at_us: Option<u64>,
    /// The light last said drawn, and when.
    drew: Option<(Light, u64)>,
    /// Lights asked for that lapse, when switched on, and the one shown.
    requests: Option<(Subscriber, Requests)>,
    requested: Option<Light>,
    /// How slowly the shown request asked to be faded into, in ms (#1677).
    fading: Option<u32>,
    /// The shown request's pattern, its asker and light, and since when it
    /// moves (#940 item 3): from the first resting frame that draws it, and
    /// a renewal keeps that, so a pulse plays once and is seen.
    moving: Option<Moving>,
    /// When each asker's last pulse began: a second inside
    /// [`LightPattern::PULSE_GAP_US`] is drawn as already played.
    pulses: Vec<(String, u64)>,
    /// Every pulse played, not one a minute per asker
    /// (`LAMP_RING_EVERY_PULSE=on`, a dev lamp's).
    every_pulse: bool,
    /// Where a request's light is said reached, when switched on.
    reached: Option<Publisher>,
    /// Where the person's light is kept across a restart, when switched on.
    keep: Option<Keep>,
    /// Where a request held but not shown is said outranked, when switched
    /// on.
    outranked: Option<Publisher>,
    /// The request the face shows, until its light is said reached.
    reaching: Option<Reaching>,
    /// Whether the request shown tints the Speaking face too
    /// (`LAMP_SKILL_LIGHT_SPEAKING=on`, [`RingLight::held`]).
    speaking: bool,
}

/// A shown request's pattern, and its asker and light, so that only a new
/// one starts it again, and since when on the ring's clock: none until a
/// resting frame draws it.
struct Moving {
    source: String,
    light: Light,
    pattern: LightPattern,
    since_us: Option<u64>,
}

/// At most this many askers' pulses are remembered: more than a minute's
/// worth of every asker the ring keeps.
const MOST_PULSES: usize = 2 * crate::ring_requests::MOST_SOURCES;

/// A request's light the face shows: its asker, the light, since when
/// resting frames have shown it running, the last such frame, whether it
/// has been said reached, and whether its asker renewed it since.
struct Reaching {
    source: String,
    light: Light,
    drawn_since_us: Option<u64>,
    last_frame_us: Option<u64>,
    said: bool,
    renewed: bool,
}

impl RingLight {
    /// The ring's light on the bus under `root`, and the light to begin
    /// with: the one last asked for, if the bus keeps one that there is,
    /// or else the first.
    pub fn on(root: &Path) -> io::Result<(Self, Lit)> {
        Self::on_keeping(root, None)
    }

    /// As [`RingLight::on`], and with `keep` the person's light is kept
    /// there across a restart: begun with when the bus keeps none.
    pub fn on_keeping(root: &Path, keep: Option<&Path>) -> io::Result<(Self, Lit)> {
        let bus = Bus::new(root)?;
        let [wanted, state] = Light::TOPICS;
        let asked = bus.subscribe_typed(wanted, SOURCE, Light::TYPE)?;
        let mut shown = bus.publisher_declared(state, &Light::declaration(), SOURCE)?;
        shown.expires_after(Some(STATE_LASTS));
        let tests = bus.subscribe_typed(TEST_TOPIC, SOURCE, Light::TYPE)?;
        let mut drawn = bus.publisher_declared(DRAWN_TOPIC, &Light::declaration(), SOURCE)?;
        drawn.expires_after(Some(STATE_LASTS));
        let kept = bus
            .latest(wanted)
            .ok()
            .flatten()
            .and_then(|message| message.parse::<Light>().ok())
            .filter(|light| lit(light).is_some());
        let on_disk = keep.and_then(kept_on_disk);
        // What is on disk is already kept; a light from the bus is written.
        let written = kept.is_none().then(|| on_disk.clone()).flatten();
        let now = kept.or(on_disk).unwrap_or_else(first);
        let begins = lit(&now).unwrap_or(Lit::first());
        Ok((
            Self {
                asked,
                shown,
                tests,
                drawn,
                now,
                testing: None,
                asked_at_ms: 0,
                looked_at_us: 0,
                said_at_us: None,
                drew: None,
                requests: None,
                requested: None,
                fading: None,
                moving: None,
                pulses: Vec::new(),
                every_pulse: false,
                reached: None,
                outranked: None,
                reaching: None,
                speaking: false,
                keep: keep.map(|path| Keep {
                    path: path.to_owned(),
                    written,
                    written_at_us: None,
                    failing: false,
                }),
            },
            begins,
        ))
    }

    /// Write the person's light where it is kept, when it is not what was
    /// last written there and a second has gone since the last write: the
    /// last change of a burst is written a second after the one before.
    fn keep(&mut self, now_us: u64) {
        let Some(keep) = self.keep.as_mut() else {
            return;
        };
        if keep.written.as_ref() == Some(&self.now)
            || keep
                .written_at_us
                .is_some_and(|at| now_us.saturating_sub(at) < KEEP_EVERY_US)
        {
            return;
        }
        keep.written_at_us = Some(now_us);
        match keep_on_disk(&keep.path, &self.now) {
            Ok(()) => {
                keep.written = Some(self.now.clone());
                keep.failing = false;
            }
            Err(error) => {
                if !keep.failing {
                    eprintln!(
                        "lamp-voice ring: the person's light is not kept across a restart: {error}"
                    );
                }
                keep.failing = true;
            }
        }
    }

    /// Also take lights asked for that lapse, on `light/request` on the bus
    /// under `root`, whose claims are on the monotonic clock `clock_id`.
    pub fn take_requests(&mut self, root: &Path, clock_id: &str) -> io::Result<()> {
        let bus = Bus::new(root)?;
        let asked = bus.subscribe_typed(LightRequest::TOPIC, SOURCE, LightRequest::TYPE)?;
        let reached =
            bus.publisher_declared(LightReached::TOPIC, &LightReached::declaration(), SOURCE)?;
        let outranked = bus.publisher_declared(
            LightOutranked::TOPIC,
            &LightOutranked::declaration(),
            SOURCE,
        )?;
        self.requests = Some((asked, Requests::new(clock_id)));
        self.reached = Some(reached);
        self.outranked = Some(outranked);
        Ok(())
    }

    /// Call from the ring's loop with its clock. A light to show from now
    /// on, if another than the one shown was asked for since the last
    /// call. What is shown is said when it changes and once a second.
    pub fn tick(&mut self, now_us: u64) -> Option<Lit> {
        self.keep(now_us);
        let mut changed = None;
        let mut requested_changed = false;
        if now_us.saturating_sub(self.looked_at_us) >= LOOK_EVERY_US {
            self.looked_at_us = now_us;
            // The newest wall-clock stamp of a person's light read in this
            // look, if one was: a light read is the person asking, whatever
            // its stamp says after the clock was set back.
            let mut person_in_look: Option<u64> = None;
            for _ in 0..MOST_AT_ONCE {
                let Ok(Some(message)) = self.asked.recv(Duration::ZERO) else {
                    break;
                };
                let Ok(asked) = message.parse::<Light>() else {
                    continue;
                };
                // The newest that is a light there is, is the one taken.
                // The person's request ends any test, even one for the
                // light already shown.
                if let Some(face) = lit(&asked) {
                    if self.testing.take().is_some() || asked != self.now {
                        changed = Some(face);
                    }
                    self.now = asked;
                    self.asked_at_ms = self.asked_at_ms.max(message.header.at_unix_ms);
                    let at = message.header.at_unix_ms;
                    person_in_look = Some(person_in_look.map_or(at, |newest| newest.max(at)));
                }
            }
            if let Some((asked, requests)) = &mut self.requests {
                let renewing = self
                    .reaching
                    .as_ref()
                    .map(|reaching| (reaching.source.clone(), reaching.light.clone()));
                if person_in_look.is_some() {
                    requests.person_asked(now_us);
                }
                for _ in 0..MOST_AT_ONCE {
                    let Ok(Some(message)) = asked.recv(Duration::ZERO) else {
                        break;
                    };
                    let Ok(request) = message.parse::<LightRequest>() else {
                        continue;
                    };
                    if message.expired {
                        continue;
                    }
                    // Sent at or before a person's light read in this same
                    // look: a tie goes to the person. Wall clocks order only
                    // what one look reads, so a clock set back cannot hold a
                    // later request off; across looks, reading order is.
                    let before_person =
                        person_in_look.is_some_and(|at| message.header.at_unix_ms <= at);
                    // The same asker asking the same light again: said again
                    // once shown, so an asker that asks twice is not left
                    // waiting (the sixth on #1166).
                    let again = renewing.as_ref().is_some_and(|(source, light)| {
                        *source == request.claim.source && *light == request.light
                    });
                    if requests.take(request, now_us, before_person)
                        && again
                        && let Some(reaching) = self.reaching.as_mut()
                    {
                        reaching.renewed = true;
                    }
                }
            }
            for _ in 0..MOST_AT_ONCE {
                let Ok(Some(message)) = self.tests.recv(Duration::ZERO) else {
                    break;
                };
                // A test asked for more than a second ago, kept by the bus
                // for a ring that started since, is not shown.
                if message.expired || unix_ms().saturating_sub(message.header.at_unix_ms) > 1_000 {
                    continue;
                }
                // The person's request is the newer wish: a test asked at
                // or before it, in this look or a later one, is passed over.
                // The bus stamps milliseconds, so a tie goes to the person.
                if message.header.at_unix_ms <= self.asked_at_ms {
                    continue;
                }
                let Ok(test) = message.parse::<Light>() else {
                    continue;
                };
                if let Some(face) = lit(&test) {
                    self.testing = Some((test, now_us.saturating_add(TEST_HOLDS_US)));
                    changed = Some(face);
                }
            }
        }
        if let Some((_, until)) = self.testing
            && now_us >= until
        {
            self.testing = None;
            changed = lit(&self.now);
        }
        if let Some((_, requests)) = &mut self.requests {
            // Each asker that lost is told, whatever the face shows now: a
            // turn or a test hides the winner too, but it still holds.
            if let Some(outranked) = self.outranked.as_mut() {
                requests.outranked(now_us, |source, by| {
                    let told = LightOutranked {
                        source: source.to_owned(),
                        by: by.to_owned(),
                    };
                    let _ = outranked.publish_json(now_us, &told);
                });
            }
            let shown = requests.shown(now_us);
            self.fading = shown.and_then(|request| request.fade_ms);
            // Its pattern, started afresh only for a new asker, light or
            // pattern.
            let wanted = shown.and_then(|request| {
                request
                    .pattern
                    .map(|pattern| (request.claim.source.as_str(), &request.light, pattern))
            });
            let same = match (&self.moving, wanted) {
                (Some(moving), Some((source, light, pattern))) => {
                    moving.source == source && moving.light == *light && moving.pattern == pattern
                }
                (None, None) => true,
                _ => false,
            };
            if !same {
                self.moving = wanted.map(|(source, light, pattern)| Moving {
                    source: source.to_owned(),
                    light: light.clone(),
                    pattern,
                    since_us: None,
                });
            }
            let light = shown.map(|request| &request.light);
            if light != self.requested.as_ref() {
                self.requested = light.cloned();
                requested_changed = true;
            }
            // The request on the face, unless a test is over it: a new asker
            // or a new light is reached afresh.
            let on_face = shown.filter(|_| self.testing.is_none());
            let same = match (on_face, &self.reaching) {
                (Some(request), Some(reaching)) => {
                    request.claim.source == reaching.source && request.light == reaching.light
                }
                (None, None) => true,
                _ => false,
            };
            if !same {
                self.reaching = on_face.map(|request| Reaching {
                    source: request.claim.source.clone(),
                    light: request.light.clone(),
                    drawn_since_us: None,
                    last_frame_us: None,
                    said: false,
                    renewed: false,
                });
            }
        }
        // A request taken, lapsed or overruled changes the face as well, and
        // what changed is always given as the face shows it.
        if changed.is_some() || requested_changed {
            changed = lit(self.face());
        }
        let due = self
            .said_at_us
            .is_none_or(|at| now_us.saturating_sub(at) >= SAY_EVERY_US);
        if changed.is_some() || due {
            self.said_at_us = Some(now_us);
            let _ = self.shown.publish_json(now_us, &self.now);
        }
        changed
    }

    /// The light the ring is showing, as it says it.
    pub fn now(&self) -> &Light {
        &self.now
    }

    /// How the face's light moves, and since when (#940 item 3): the shown
    /// request's pattern, none under a self-test or for the person's light.
    pub fn pattern(&self) -> Option<(LightPattern, u64)> {
        let moving = self.moving.as_ref().filter(|_| self.testing.is_none())?;
        Some((moving.pattern, moving.since_us?))
    }

    /// How slowly the face comes into its light, in ms (#1677): the shown
    /// request's fade, as a skill's light going out when nobody is there
    /// asks it; none under a self-test, for the person's light or for a
    /// request that asks none.
    pub fn fade(&self) -> Option<u32> {
        self.fading.filter(|_| self.testing.is_none())
    }

    /// A resting frame drew the face at `drawn_at_us`, showing the lamp's
    /// own light or not: the shown request's pattern starts on the first
    /// that does, never under a self-test. A pulse from an asker whose last
    /// began within [`LightPattern::PULSE_GAP_US`] starts as already played
    /// (the first builder on #940: "something came" twice is a nag), and
    /// the minute counts from what was drawn, so a pulse a turn hid, or an
    /// outranked one, is not counted (the reviewer on #1494).
    fn start_moving(&mut self, drawn_at_us: u64, own_light: bool) {
        let Some(moving) = self
            .moving
            .as_mut()
            .filter(|moving| own_light && moving.since_us.is_none() && self.testing.is_none())
        else {
            return;
        };
        let mut since_us = drawn_at_us;
        if moving.pattern.kind == PatternKind::Pulse && !self.every_pulse {
            let gap = LightPattern::PULSE_GAP_US;
            self.pulses
                .retain(|(_, at)| drawn_at_us.saturating_sub(*at) < gap);
            if self.pulses.iter().any(|(asker, _)| *asker == moving.source) {
                since_us = drawn_at_us.saturating_sub(u64::from(moving.pattern.period_ms) * 1_000);
                eprintln!(
                    "lamp-voice: ring: a second pulse from {} within a minute is not played",
                    moving.source
                );
            } else if self.pulses.len() < MOST_PULSES {
                self.pulses.push((moving.source.clone(), drawn_at_us));
            }
        }
        moving.since_us = Some(since_us);
    }

    /// Play every pulse, not one a minute per asker (a dev lamp's
    /// `LAMP_RING_EVERY_PULSE=on`).
    pub fn every_pulse(&mut self, on: bool) {
        self.every_pulse = on;
    }

    /// Let the request shown tint the Speaking face as well, with
    /// `LAMP_SKILL_LIGHT_SPEAKING=on` and requests taken (El on #1722).
    pub fn tint_speaking(&mut self, on: bool) {
        self.speaking = on;
    }

    /// Whether the request shown tints the Speaking face
    /// ([`crate::ring_face::Face::tints_speaking`]).
    pub fn tints_speaking(&self) -> bool {
        self.speaking
    }

    /// The light a request holds on the face now, as the face's light: none
    /// under a self-test, for the person's own light, or without requests.
    /// For the Speaking face's tint ([`crate::ring_face::Face::skill_light`]).
    pub fn held(&self) -> Option<Lit> {
        self.requested
            .as_ref()
            .filter(|_| self.testing.is_none())
            .and_then(lit)
    }

    /// The light given to the face now: a test's, a request's that holds,
    /// or the person's.
    pub fn face(&self) -> &Light {
        self.testing
            .as_ref()
            .map(|(test, _)| test)
            .or(self.requested.as_ref())
            .unwrap_or(&self.now)
    }

    /// A resting frame written at `drawn_at_us`, showing the lamp's own
    /// light or not (a tap's listening face, the face closing after a turn):
    /// a request's light shown for [`REACHED_AFTER_US`] running is said
    /// reached, and again after its asker renewed it.
    fn reach(&mut self, drawn_at_us: u64, own_light: bool) {
        let Some(reaching) = self.reaching.as_mut() else {
            return;
        };
        let stalled = reaching
            .last_frame_us
            .is_some_and(|last| drawn_at_us.saturating_sub(last) > FRAMES_APART_US);
        reaching.last_frame_us = Some(drawn_at_us);
        if !own_light || stalled {
            reaching.drawn_since_us = None;
        }
        if !own_light {
            return;
        }
        let since = *reaching.drawn_since_us.get_or_insert(drawn_at_us);
        if drawn_at_us.saturating_sub(since) < REACHED_AFTER_US
            || (reaching.said && !reaching.renewed)
        {
            return;
        }
        reaching.said = true;
        reaching.renewed = false;
        if let Some(reached) = self.reached.as_mut() {
            let _ = reached.publish_json(
                drawn_at_us,
                &LightReached {
                    source: reaching.source.clone(),
                    light: reaching.light.clone(),
                },
            );
        }
    }

    /// Call when the ring wrote a resting frame, at the write's own time,
    /// with whether the face showed its own light in it
    /// ([`crate::ring_face::Face::rests_on_its_light`]). What it drew is
    /// said when it changes and once a second.
    pub fn drew(&mut self, drawn_at_us: u64, own_light: bool) {
        self.reach(drawn_at_us, own_light);
        self.start_moving(drawn_at_us, own_light);
        let light = self.face().clone();
        if self.drew.as_ref().is_some_and(|(last, at)| {
            *last == light && drawn_at_us.saturating_sub(*at) < SAY_EVERY_US
        }) {
            return;
        }
        let _ = self.drawn.publish_json(drawn_at_us, &light);
        self.drew = Some((light, drawn_at_us));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_bus::Kind;

    /// How far these tests move the ring's clock at a time: past a look,
    /// as when a look was every 50 ms (#1806).
    const STEP_US: u64 = 50_000;

    struct Root(std::path::PathBuf);
    impl Root {
        fn new(name: &str) -> Self {
            let root = std::path::PathBuf::from(format!("/tmp/lv-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }
    impl Drop for Root {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn light(on: bool, colour: &str, level: u8) -> Light {
        Light {
            on,
            colour: colour.into(),
            level,
        }
    }

    fn state(bus: &Bus) -> Option<Light> {
        bus.latest(Light::TOPICS[1])
            .unwrap()
            .map(|message| message.parse().unwrap())
    }

    /// Tick until the ring has taken a new light, for a second at most.
    fn taken(ring: &mut RingLight, from_us: &mut u64) -> Option<Lit> {
        let started = std::time::Instant::now();
        while started.elapsed() < Duration::from_secs(5) {
            *from_us += STEP_US;
            if let Some(lit) = ring.tick(*from_us) {
                return Some(lit);
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        None
    }

    /// #1816: the muted wait's light is opened only with the switch on, a
    /// ring and a bus.
    #[test]
    fn the_light_while_muted_is_off_unless_on_with_a_ring_and_a_bus() {
        let root = Root::new("light-while-muted");
        for (switch, ring, bus) in [
            (None, true, true),
            (Some("yes"), true, true),
            (Some("on"), false, true),
            (Some("on"), true, false),
        ] {
            let root = bus.then_some(root.0.as_path());
            assert!(
                while_muted(switch, ring, root, None).is_none(),
                "{switch:?} {ring} {bus}"
            );
        }
        let light = while_muted(Some("on"), true, Some(&root.0), None).unwrap();
        assert_eq!(light.now(), &first());
    }

    #[test]
    fn the_ring_begins_with_the_first_light_and_says_what_it_shows() {
        let root = Root::new("light-first");
        let (mut ring, begins) = RingLight::on(&root.0).unwrap();
        assert_eq!(begins, Lit::first());
        assert_eq!(ring.now(), &first());
        let bus = Bus::new(&root.0).unwrap();
        // Nothing is said until its loop runs.
        assert_eq!(state(&bus), None);
        assert_eq!(ring.tick(10_000), None);
        assert_eq!(state(&bus), Some(light(true, "warm", 1)));
        // Declared as the type has it, and good for three seconds.
        let said = bus.latest(Light::TOPICS[1]).unwrap().unwrap();
        assert_eq!(said.header.source, "ring");
        assert_eq!(said.header.expires_ms, Some(3_000));
        assert_eq!(
            bus.declaration(Light::TOPICS[1]).unwrap(),
            Some(Light::declaration())
        );
    }

    /// A light asked for is taken at the ring's next pass of its loop, not
    /// at a later look: `lamp-actions` answers the voice only once the
    /// ring says it shows it (#1806; looks were 50 ms apart).
    #[test]
    fn a_light_asked_for_is_taken_at_the_rings_next_pass() {
        let root = Root::new("light-pass");
        let (mut ring, _) = RingLight::on(&root.0).unwrap();
        let bus = Bus::new(&root.0).unwrap();
        let mut asks = bus
            .publisher_declared(Light::TOPICS[0], &Light::declaration(), "actions")
            .unwrap();
        let mut at = 0u64;
        ring.tick(at);
        asks.publish_json(1, &light(true, "blue", 3)).unwrap();
        assert_eq!(taken(&mut ring, &mut at), Lit::of(true, "blue", 3));
        // That pass looked. The next light asked is taken one pass on.
        asks.publish_json(2, &light(true, "red", 3)).unwrap();
        assert_eq!(ring.tick(at + 2_000), Lit::of(true, "red", 3));
        assert_eq!(state(&bus), Some(light(true, "red", 3)));
    }

    #[test]
    fn a_light_asked_for_is_taken_shown_and_said_at_once() {
        let root = Root::new("light-asked");
        let (mut ring, _) = RingLight::on(&root.0).unwrap();
        let bus = Bus::new(&root.0).unwrap();
        let mut asks = bus
            .publisher_declared(Light::TOPICS[0], &Light::declaration(), "actions")
            .unwrap();
        let mut at = 0u64;
        ring.tick(at);
        asks.publish_json(1, &light(true, "blue", 3)).unwrap();
        assert_eq!(taken(&mut ring, &mut at), Lit::of(true, "blue", 3));
        assert_eq!(state(&bus), Some(light(true, "blue", 3)));
        // The same again changes nothing.
        asks.publish_json(2, &light(true, "blue", 3)).unwrap();
        for _ in 0..5 {
            at += STEP_US;
            assert_eq!(ring.tick(at), None);
            std::thread::sleep(Duration::from_millis(2));
        }
        // Off keeps the colour it would be.
        asks.publish_json(3, &light(false, "blue", 3)).unwrap();
        let off = taken(&mut ring, &mut at).unwrap();
        assert!(!off.on);
        assert_eq!(state(&bus), Some(light(false, "blue", 3)));
        // Several at once: the newest is the one shown.
        for (n, colour) in ["red", "green", "pink"].into_iter().enumerate() {
            asks.publish_json(4 + n as u64, &light(true, colour, 5))
                .unwrap();
        }
        let mut last = None;
        while let Some(lit) = taken(&mut ring, &mut at) {
            last = Some(lit);
            if ring.now().colour == "pink" {
                break;
            }
        }
        assert_eq!(last, Lit::of(true, "pink", 5));
        assert_eq!(state(&bus), Some(light(true, "pink", 5)));
    }

    #[test]
    fn what_is_not_a_light_there_is_is_passed_over() {
        let root = Root::new("light-odd");
        let (mut ring, _) = RingLight::on(&root.0).unwrap();
        let bus = Bus::new(&root.0).unwrap();
        let mut asks = bus
            .publisher_declared(Light::TOPICS[0], &Light::declaration(), "actions")
            .unwrap();
        let mut at = 0u64;
        ring.tick(at);
        asks.publish_json(1, &light(true, "mauve", 2)).unwrap();
        asks.publish_json(2, &light(true, "blue", 9)).unwrap();
        asks.publish_json(3, &serde_json::json!({"rgb": [0, 0, 255]}))
            .unwrap();
        for _ in 0..10 {
            at += STEP_US;
            assert_eq!(ring.tick(at), None);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(ring.now(), &first());
        // And a light there is, after them, is taken.
        asks.publish_json(4, &light(true, "cyan", 2)).unwrap();
        assert_eq!(taken(&mut ring, &mut at), Lit::of(true, "cyan", 2));
    }

    #[test]
    fn a_ring_that_starts_later_takes_up_the_light_last_asked_for() {
        let root = Root::new("light-kept");
        let bus = Bus::new(&root.0).unwrap();
        let mut asks = bus
            .publisher_declared(Light::TOPICS[0], &Light::declaration(), "actions")
            .unwrap();
        asks.publish_json(1, &light(true, "purple", 4)).unwrap();
        let (mut ring, begins) = RingLight::on(&root.0).unwrap();
        assert_eq!(Some(begins), Lit::of(true, "purple", 4));
        ring.tick(1_000);
        assert_eq!(state(&bus), Some(light(true, "purple", 4)));
        drop(ring);
        // A kept request that is not a light there is: the first light.
        let other = Root::new("light-kept-odd");
        let bus = Bus::new(&other.0).unwrap();
        let mut asks = bus
            .publisher(Light::TOPICS[0], Kind::Latest, "old")
            .unwrap();
        asks.publish_json(1, &serde_json::json!({"rgb": [1, 2, 3]}))
            .unwrap();
        // A topic declared by an older program as no type at all cannot be
        // subscribed to as a light: the ring goes on without the bus.
        match RingLight::on(&other.0) {
            Ok((_, begins)) => assert_eq!(begins, Lit::first()),
            Err(error) => assert!(!error.to_string().is_empty()),
        }
    }

    #[test]
    fn what_it_shows_is_said_again_every_second_and_not_more_often() {
        let root = Root::new("light-said");
        let (mut ring, _) = RingLight::on(&root.0).unwrap();
        let bus = Bus::new(&root.0).unwrap();
        let mut hears = bus
            .subscribe_typed(Light::TOPICS[1], "test", Light::TYPE)
            .unwrap();
        // Three seconds of the ring's loop, a tick every 2 ms.
        for tick in 0..1_500u64 {
            ring.tick(tick * 2_000);
        }
        let mut said = 0;
        while hears.recv(Duration::from_millis(50)).unwrap().is_some() {
            said += 1;
        }
        assert_eq!(said, 3);
    }

    #[test]
    fn a_test_colour_is_shown_for_a_while_and_never_touches_the_persons_light() {
        let root = Root::new("light-test");
        let (mut ring, _) = RingLight::on(&root.0).unwrap();
        let bus = Bus::new(&root.0).unwrap();
        let mut asks = bus
            .publisher_declared(Light::TOPICS[0], &Light::declaration(), "actions")
            .unwrap();
        let mut tests = bus
            .publisher_declared(TEST_TOPIC, &Light::declaration(), "selftest")
            .unwrap();
        let mut at = 0u64;
        ring.tick(at);
        asks.publish_json(1, &light(true, "blue", 2)).unwrap();
        assert_eq!(taken(&mut ring, &mut at), Lit::of(true, "blue", 2));
        // Asked in a later millisecond: a test in the same one as the
        // person's request ties, and the person wins a tie.
        std::thread::sleep(Duration::from_millis(2));
        tests.publish_json(1, &light(true, "red", 3)).unwrap();
        assert_eq!(taken(&mut ring, &mut at), Lit::of(true, "red", 3));
        assert_eq!(ring.face(), &light(true, "red", 3));
        // The person's light is still what is said, and still what is asked.
        assert_eq!(state(&bus), Some(light(true, "blue", 2)));
        let asked: Light = bus
            .latest(Light::TOPICS[0])
            .unwrap()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(asked, light(true, "blue", 2));
        // After its hold, the person's light again.
        at += TEST_HOLDS_US;
        assert_eq!(ring.tick(at), Lit::of(true, "blue", 2));
        assert_eq!(ring.face(), &light(true, "blue", 2));
    }

    #[test]
    fn the_persons_request_ends_a_test_at_once_even_for_the_light_already_asked() {
        let root = Root::new("light-test-ends");
        let (mut ring, _) = RingLight::on(&root.0).unwrap();
        let bus = Bus::new(&root.0).unwrap();
        let mut asks = bus
            .publisher_declared(Light::TOPICS[0], &Light::declaration(), "actions")
            .unwrap();
        let mut tests = bus
            .publisher_declared(TEST_TOPIC, &Light::declaration(), "selftest")
            .unwrap();
        let mut at = 0u64;
        ring.tick(at);
        tests.publish_json(1, &light(true, "green", 3)).unwrap();
        assert_eq!(taken(&mut ring, &mut at), Lit::of(true, "green", 3));
        // The light the ring already had: it still ends the test.
        asks.publish_json(1, &first()).unwrap();
        assert_eq!(taken(&mut ring, &mut at), lit(&first()));
        assert_eq!(ring.face(), &first());
        asks.publish_json(2, &light(false, "pink", 5)).unwrap();
        assert_eq!(taken(&mut ring, &mut at), Lit::of(false, "pink", 5));
        assert_eq!(ring.face(), &light(false, "pink", 5));
    }

    #[test]
    fn what_is_drawn_is_said_only_when_drawn_on_change_and_every_second() {
        let root = Root::new("light-drawn");
        let (mut ring, _) = RingLight::on(&root.0).unwrap();
        let bus = Bus::new(&root.0).unwrap();
        let mut hears = bus
            .subscribe_typed(DRAWN_TOPIC, "test", Light::TYPE)
            .unwrap();
        // Ticks with no write: nothing drawn is said.
        for tick in 0..100u64 {
            ring.tick(tick * 2_000);
        }
        assert!(hears.recv(Duration::from_millis(50)).unwrap().is_none());
        // Writes every 25 ms for two seconds: said twice, the same light.
        for n in 0..80u64 {
            ring.drew(1_000_000 + n * 25_000, true);
        }
        let mut said = Vec::new();
        while let Some(message) = hears.recv(Duration::from_millis(50)).unwrap() {
            said.push((message.header.at_us, message.parse::<Light>().unwrap()));
        }
        assert_eq!(said.len(), 2, "{said:?}");
        assert_eq!(said[0], (1_000_000, first()));
        // A changed light is said at its first write.
        let mut tests = bus
            .publisher_declared(TEST_TOPIC, &Light::declaration(), "selftest")
            .unwrap();
        tests.publish_json(1, &light(true, "cyan", 4)).unwrap();
        let mut at = 3_000_000;
        taken(&mut ring, &mut at).unwrap();
        ring.drew(3_100_000, true);
        let message = hears.recv(Duration::from_millis(200)).unwrap().unwrap();
        assert_eq!(message.parse::<Light>().unwrap(), light(true, "cyan", 4));
    }

    #[test]
    fn a_test_older_than_the_persons_request_in_the_same_look_is_passed_over() {
        let root = Root::new("light-test-order");
        let (mut ring, _) = RingLight::on(&root.0).unwrap();
        let bus = Bus::new(&root.0).unwrap();
        let mut asks = bus
            .publisher_declared(Light::TOPICS[0], &Light::declaration(), "actions")
            .unwrap();
        let mut tests = bus
            .publisher_declared(TEST_TOPIC, &Light::declaration(), "selftest")
            .unwrap();
        let mut at = 0u64;
        ring.tick(at);
        // Both before one look: the test first, the person 27 ms later.
        tests.publish_json(1, &light(true, "red", 3)).unwrap();
        std::thread::sleep(Duration::from_millis(27));
        asks.publish_json(1, &light(false, "blue", 5)).unwrap();
        std::thread::sleep(Duration::from_millis(5));
        at += STEP_US;
        assert_eq!(ring.tick(at), Lit::of(false, "blue", 5));
        assert_eq!(ring.face(), &light(false, "blue", 5));
        // And it stays the person's: the test is not taken later either.
        for _ in 0..5 {
            at += STEP_US;
            ring.tick(at);
        }
        assert_eq!(ring.face(), &light(false, "blue", 5));
        // The same, with the person asking for the light already set.
        tests.publish_json(2, &light(true, "green", 3)).unwrap();
        std::thread::sleep(Duration::from_millis(5));
        asks.publish_json(2, &light(false, "blue", 5)).unwrap();
        std::thread::sleep(Duration::from_millis(5));
        at += STEP_US;
        ring.tick(at);
        assert_eq!(ring.face(), &light(false, "blue", 5));
        // A test asked after the person's request is still shown.
        std::thread::sleep(Duration::from_millis(5));
        tests.publish_json(3, &light(true, "cyan", 2)).unwrap();
        assert_eq!(taken(&mut ring, &mut at), Lit::of(true, "cyan", 2));
    }

    fn request(
        source: &str,
        priority: lamp_msgs::Priority,
        until_us: u64,
        colour: &str,
    ) -> LightRequest {
        LightRequest {
            claim: lamp_msgs::Claim {
                source: source.into(),
                priority,
                clock_id: "boot-test".into(),
                until_us,
            },
            light: light(true, colour, 4),
            pattern: None,
            fade_ms: None,
        }
    }

    #[test]
    fn the_switch_is_one_flag_taken_off_the_ring_workers_flags() {
        assert_eq!(requests_arguments(Some("on")), ["--light-requests"]);
        assert!(requests_arguments(Some("1")).is_empty());
        assert!(requests_arguments(None).is_empty());
        let flags = ["--light-requests".to_owned(), "--x".to_owned()];
        assert_eq!(take(&flags), (true, &flags[1..]));
        assert_eq!(take(&flags[1..]), (false, &flags[1..]));
        // A skill's light on the Speaking face (El on #1722): its own flag.
        assert_eq!(speaking_arguments(Some("on")), ["--skill-light-speaking"]);
        assert!(speaking_arguments(Some("true")).is_empty());
        assert!(speaking_arguments(None).is_empty());
        let flags = ["--skill-light-speaking".to_owned(), "--x".to_owned()];
        assert_eq!(take_speaking(&flags), (true, &flags[1..]));
        assert_eq!(take_speaking(&flags[1..]), (false, &flags[1..]));
    }

    /// The reviewer on #993: off unless switched on. A ring without
    /// `take_requests` takes no `light/request` at all: the same glow that a
    /// switched-on ring shows leaves this one on the person's light, and
    /// says nothing on `light/outranked` either, though the person's light
    /// is what it shows (the reviewer on #1466).
    #[test]
    fn without_the_switch_a_light_request_changes_nothing() {
        let root = Root::new("light-request-off");
        let (mut ring, _) = RingLight::on(&root.0).unwrap();
        let bus = Bus::new(&root.0).unwrap();
        let mut outranked = bus.subscribe(LightOutranked::TOPIC, "skills").unwrap();
        let mut asks = bus
            .publisher_declared(Light::TOPICS[0], &Light::declaration(), "actions")
            .unwrap();
        let mut glows = bus
            .publisher_declared(LightRequest::TOPIC, &LightRequest::declaration(), "skills")
            .unwrap();
        let mut at = 0u64;
        ring.tick(at);
        asks.publish_json(1, &light(true, "blue", 2)).unwrap();
        assert_eq!(taken(&mut ring, &mut at), Lit::of(true, "blue", 2));
        let glow = request(
            "skill:welcome",
            lamp_msgs::Priority::Skill,
            at + 10_000_000,
            "green",
        );
        glows.publish_json(1, &glow).unwrap();
        for _ in 0..10 {
            at += STEP_US;
            assert_eq!(ring.tick(at), None);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(ring.face(), &light(true, "blue", 2));
        assert!(
            outranked.recv(Duration::ZERO).unwrap().is_none(),
            "nothing said on light/outranked without the switch"
        );
    }

    /// #940: a skill's glow is shown over the person's light while its
    /// claim holds, and the person's light comes back by itself when it
    /// lapses; `light/state` says the person's light all along.
    #[test]
    fn a_light_asked_for_that_lapses_is_shown_and_gives_the_persons_light_back() {
        let root = Root::new("light-request");
        let (mut ring, _) = RingLight::on(&root.0).unwrap();
        ring.take_requests(&root.0, "boot-test").unwrap();
        let bus = Bus::new(&root.0).unwrap();
        let mut asks = bus
            .publisher_declared(Light::TOPICS[0], &Light::declaration(), "actions")
            .unwrap();
        let mut glows = bus
            .publisher_declared(LightRequest::TOPIC, &LightRequest::declaration(), "skills")
            .unwrap();
        let mut at = 0u64;
        ring.tick(at);
        asks.publish_json(1, &light(true, "blue", 2)).unwrap();
        assert_eq!(taken(&mut ring, &mut at), Lit::of(true, "blue", 2));
        // The person's own light is held by no skill (El on #1722).
        assert_eq!(ring.held(), None);
        std::thread::sleep(Duration::from_millis(2));
        let until = at + 10_000_000;
        glows
            .publish_json(
                1,
                &request("skill:welcome", lamp_msgs::Priority::Skill, until, "green"),
            )
            .unwrap();
        assert_eq!(taken(&mut ring, &mut at), Lit::of(true, "green", 4));
        assert_eq!(ring.face(), &light(true, "green", 4));
        assert_eq!(ring.held(), Lit::of(true, "green", 4));
        assert_eq!(state(&bus), Some(light(true, "blue", 2)));
        // From another boot, it is not shown.
        let mut other = request("skill:other", lamp_msgs::Priority::Safety, until, "red");
        other.claim.clock_id = "boot-two".into();
        glows.publish_json(2, &other).unwrap();
        for _ in 0..5 {
            at += STEP_US;
            assert_eq!(ring.tick(at), None);
            std::thread::sleep(Duration::from_millis(2));
        }
        // Its time passes: the person's light again, with no one asking.
        assert_eq!(ring.tick(until), Lit::of(true, "blue", 2));
        assert_eq!(ring.face(), &light(true, "blue", 2));
        assert_eq!(ring.held(), None, "given back, nothing tints Speaking");
    }

    /// Resting frames at the ring's 25 ms pace from `at` for `for_us`,
    /// showing the lamp's own light or not, and each light said reached,
    /// with when.
    fn rested_for(
        ring: &mut RingLight,
        reached: &mut lamp_bus::Subscriber,
        at: &mut u64,
        for_us: u64,
        own_light: bool,
    ) -> Vec<(u64, LightReached)> {
        let until = *at + for_us;
        let mut said = Vec::new();
        while *at < until {
            ring.drew(*at, own_light);
            while let Some(message) = reached.recv(Duration::ZERO).unwrap() {
                said.push((*at, message.parse().unwrap()));
            }
            *at += 25_000;
        }
        said
    }

    /// Ticks enough for whatever was published to be read.
    fn looked(ring: &mut RingLight, at: &mut u64) {
        for _ in 0..5 {
            *at += STEP_US;
            ring.tick(*at);
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// #1146: a request's light shown on resting frames for 350 ms running
    /// is said reached, naming its asker, so a skill can wait for its
    /// light. Not before a frame shows it, not on a frame whose face is
    /// another (a tap's), counted again after a stall, said again when its
    /// asker renews it, afresh for a new light or after a self-test over it,
    /// and never for a light the person's own ended first.
    #[test]
    fn a_requests_light_shown_long_enough_is_said_reached() {
        let root = Root::new("light-reached");
        let (mut ring, _) = RingLight::on(&root.0).unwrap();
        ring.take_requests(&root.0, "boot-test").unwrap();
        let bus = Bus::new(&root.0).unwrap();
        let mut reached = bus.subscribe(LightReached::TOPIC, "test").unwrap();
        let mut asks = bus
            .publisher_declared(Light::TOPICS[0], &Light::declaration(), "actions")
            .unwrap();
        let mut tests = bus
            .publisher_declared(TEST_TOPIC, &Light::declaration(), "test")
            .unwrap();
        let mut glows = bus
            .publisher_declared(LightRequest::TOPIC, &LightRequest::declaration(), "skills")
            .unwrap();
        let mut at = 0u64;
        ring.tick(at);
        let white = |level: u8| {
            let mut asked = request(
                "skill:desk-tutor",
                lamp_msgs::Priority::Skill,
                25_000_000,
                "white",
            );
            asked.light.level = level;
            asked
        };
        let white_5 = LightReached {
            source: "skill:desk-tutor".into(),
            light: light(true, "white", 5),
        };
        glows.publish_json(1, &white(5)).unwrap();
        assert_eq!(taken(&mut ring, &mut at), Lit::of(true, "white", 5));
        // Taken, but with no frame drawn nothing is said, however long; nor
        // while a tap's listening face is over it.
        at += 2_000_000;
        ring.tick(at);
        assert!(reached.recv(Duration::ZERO).unwrap().is_none());
        assert!(rested_for(&mut ring, &mut reached, &mut at, 1_000_000, false).is_empty());
        // Shown for 200 ms, a 150 ms stall, then the count begins again.
        assert!(rested_for(&mut ring, &mut reached, &mut at, 200_000, true).is_empty());
        at += 150_000;
        let again = at;
        let said = rested_for(&mut ring, &mut reached, &mut at, 1_000_000, true);
        assert_eq!(said, [(again + REACHED_AFTER_US, white_5.clone())]);
        // Renewed, the same light: said again at the next frame, once.
        glows.publish_json(2, &white(5)).unwrap();
        looked(&mut ring, &mut at);
        let said = rested_for(&mut ring, &mut reached, &mut at, 1_000_000, true);
        assert_eq!(said.len(), 1, "{said:?}");
        assert_eq!(said[0].1, white_5);
        // A new light from the same asker, a dim: reached afresh.
        glows.publish_json(3, &white(2)).unwrap();
        assert_eq!(taken(&mut ring, &mut at), Lit::of(true, "white", 2));
        let said = rested_for(&mut ring, &mut reached, &mut at, 1_000_000, true);
        assert_eq!(said.len(), 1);
        assert_eq!(said[0].1.light, light(true, "white", 2));
        // A self-test's colour over it: nothing while it shows, and the
        // request's light, back after it, is reached afresh.
        tests.publish_json(1, &light(true, "cyan", 2)).unwrap();
        assert_eq!(taken(&mut ring, &mut at), Lit::of(true, "cyan", 2));
        assert!(rested_for(&mut ring, &mut reached, &mut at, 1_000_000, true).is_empty());
        at += TEST_HOLDS_US;
        assert_eq!(ring.tick(at), Lit::of(true, "white", 2));
        let said = rested_for(&mut ring, &mut reached, &mut at, 1_000_000, true);
        assert_eq!(said.len(), 1);
        // A new light, then the person's own before it is reached: never.
        glows.publish_json(4, &white(4)).unwrap();
        assert_eq!(taken(&mut ring, &mut at), Lit::of(true, "white", 4));
        assert!(rested_for(&mut ring, &mut reached, &mut at, 100_000, true).is_empty());
        std::thread::sleep(Duration::from_millis(2));
        asks.publish_json(1, &light(true, "blue", 3)).unwrap();
        assert_eq!(taken(&mut ring, &mut at), Lit::of(true, "blue", 3));
        assert!(rested_for(&mut ring, &mut reached, &mut at, 1_000_000, true).is_empty());
    }

    /// A request that lapses before it is shown long enough is never said
    /// reached.
    #[test]
    fn a_request_that_lapses_first_is_never_said_reached() {
        let root = Root::new("light-reached-lapse");
        let (mut ring, _) = RingLight::on(&root.0).unwrap();
        ring.take_requests(&root.0, "boot-test").unwrap();
        let bus = Bus::new(&root.0).unwrap();
        let mut reached = bus.subscribe(LightReached::TOPIC, "test").unwrap();
        let mut glows = bus
            .publisher_declared(LightRequest::TOPIC, &LightRequest::declaration(), "skills")
            .unwrap();
        let mut at = 0u64;
        ring.tick(at);
        let until = at + 600_000;
        glows
            .publish_json(
                1,
                &request("skill:brief", lamp_msgs::Priority::Skill, until, "green"),
            )
            .unwrap();
        assert_eq!(taken(&mut ring, &mut at), Lit::of(true, "green", 4));
        // Shown for 200 ms, then it lapses: the person's light again.
        assert!(rested_for(&mut ring, &mut reached, &mut at, 200_000, true).is_empty());
        at = at.max(until);
        assert!(ring.tick(at).is_some());
        assert!(rested_for(&mut ring, &mut reached, &mut at, 1_000_000, true).is_empty());
    }

    /// The person comes first: their own light ends a skill's glow, which
    /// stays ended while the skill renews it.
    #[test]
    fn the_persons_light_ends_a_skills_glow_and_its_renewals() {
        let root = Root::new("light-request-person");
        let (mut ring, _) = RingLight::on(&root.0).unwrap();
        ring.take_requests(&root.0, "boot-test").unwrap();
        let bus = Bus::new(&root.0).unwrap();
        let mut asks = bus
            .publisher_declared(Light::TOPICS[0], &Light::declaration(), "actions")
            .unwrap();
        let mut glows = bus
            .publisher_declared(LightRequest::TOPIC, &LightRequest::declaration(), "skills")
            .unwrap();
        let mut at = 0u64;
        ring.tick(at);
        let glow = |at: u64| {
            request(
                "skill:focus",
                lamp_msgs::Priority::Skill,
                at + 5_000_000,
                "purple",
            )
        };
        glows.publish_json(1, &glow(at)).unwrap();
        assert_eq!(taken(&mut ring, &mut at), Lit::of(true, "purple", 4));
        std::thread::sleep(Duration::from_millis(2));
        asks.publish_json(1, &light(true, "white", 5)).unwrap();
        assert_eq!(taken(&mut ring, &mut at), Lit::of(true, "white", 5));
        std::thread::sleep(Duration::from_millis(2));
        glows.publish_json(2, &glow(at)).unwrap();
        for _ in 0..5 {
            at += STEP_US;
            assert_eq!(ring.tick(at), None);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(ring.face(), &light(true, "white", 5));
    }

    /// #1677: the shown request's fade goes to the face while it is shown,
    /// never under a self-test, and none once it is given back; a fade out
    /// of bounds is not a request the ring takes.
    #[test]
    fn a_requests_fade_goes_to_the_face_while_it_is_shown() {
        let root = Root::new("light-fade");
        let (mut ring, _) = RingLight::on(&root.0).unwrap();
        ring.take_requests(&root.0, "boot-test").unwrap();
        let bus = Bus::new(&root.0).unwrap();
        let mut glows = bus
            .publisher_declared(LightRequest::TOPIC, &LightRequest::declaration(), "skills")
            .unwrap();
        let mut tests = bus
            .publisher_declared(TEST_TOPIC, &Light::declaration(), "selftest")
            .unwrap();
        let skill = lamp_msgs::Priority::Skill;
        let out = |until_us, fade_ms| {
            let mut asked = request("skill:light-out", skill, until_us, "warm");
            asked.light.on = false;
            asked.fade_ms = fade_ms;
            asked
        };
        let mut at = 0u64;
        ring.tick(at);
        assert_eq!(ring.fade(), None, "the person's light");
        glows
            .publish_json(1, &out(20_000_000, Some(30_000)))
            .unwrap();
        looked(&mut ring, &mut at);
        assert_eq!(ring.face(), &light(false, "warm", 4));
        assert_eq!(ring.fade(), Some(30_000));
        // A self-test over it is drawn as ever, and the fade comes back after.
        tests.publish_json(2, &light(true, "red", 2)).unwrap();
        looked(&mut ring, &mut at);
        assert_eq!(ring.fade(), None);
        at += TEST_HOLDS_US;
        looked(&mut ring, &mut at);
        assert_eq!(ring.fade(), Some(30_000));
        // Given back: the person's light, at the face's own ease.
        glows.publish_json(3, &out(0, Some(30_000))).unwrap();
        looked(&mut ring, &mut at);
        assert_eq!(ring.face(), &first());
        assert_eq!(ring.fade(), None);
        // Past a minute it is not a fade: the request is passed over.
        glows
            .publish_json(4, &out(at + 20_000_000, Some(60_001)))
            .unwrap();
        looked(&mut ring, &mut at);
        assert_eq!((ring.face(), ring.fade()), (&first(), None));
    }

    /// #940 item 3: the shown request's pattern goes to the face from the
    /// first resting frame that draws it; a renewal keeps that, a self-test
    /// hides it, and a second pulse from the same asker within a minute of
    /// one drawn is drawn as already played (the first builder on #940). A
    /// pulse no frame drew (a turn's face over it) is not counted (the
    /// reviewer on #1494).
    #[test]
    fn a_requests_pattern_moves_from_its_first_draw_and_pulses_once_a_minute() {
        let root = Root::new("light-pattern");
        let (mut ring, _) = RingLight::on(&root.0).unwrap();
        ring.take_requests(&root.0, "boot-test").unwrap();
        let bus = Bus::new(&root.0).unwrap();
        let mut glows = bus
            .publisher_declared(LightRequest::TOPIC, &LightRequest::declaration(), "skills")
            .unwrap();
        let mut tests = bus
            .publisher_declared(TEST_TOPIC, &Light::declaration(), "selftest")
            .unwrap();
        let skill = lamp_msgs::Priority::Skill;
        let with = |source: &str, until_us, kind, period_ms| {
            let mut asked = request(source, skill, until_us, "green");
            asked.pattern = Some(LightPattern {
                kind,
                period_ms,
                low: 1,
            });
            asked
        };
        let mut at = 0u64;
        ring.tick(at);
        assert_eq!(ring.pattern(), None, "the person's light is steady");
        let breath = with("skill:calm", 20_000_000, PatternKind::Breathe, 6_000);
        glows.publish_json(1, &breath).unwrap();
        looked(&mut ring, &mut at);
        assert_eq!(ring.pattern(), None, "not drawn yet");
        ring.drew(at, false);
        assert_eq!(ring.pattern(), None, "a turn's face drew over it");
        ring.drew(at, true);
        let (pattern, since) = ring.pattern().unwrap();
        assert_eq!((pattern.kind, since), (PatternKind::Breathe, at));
        // A renewal keeps when it began.
        glows.publish_json(2, &breath).unwrap();
        looked(&mut ring, &mut at);
        ring.drew(at, true);
        assert_eq!(ring.pattern().unwrap().1, since);
        // A self-test over it is steady, and the breath goes on after it.
        tests.publish_json(3, &light(true, "red", 2)).unwrap();
        looked(&mut ring, &mut at);
        ring.drew(at, true);
        assert_eq!(ring.pattern(), None);
        at += TEST_HOLDS_US;
        looked(&mut ring, &mut at);
        assert_eq!(ring.pattern().unwrap().1, since);
        // Given back: steady again.
        glows
            .publish_json(4, &with("skill:calm", 0, PatternKind::Breathe, 6_000))
            .unwrap();
        looked(&mut ring, &mut at);
        assert_eq!(ring.pattern(), None);
        // A pulse plays from its first draw; the same skill's next within a
        // minute is drawn as played, another skill's is not.
        let pulse = |source: &str, at: u64| with(source, at + 5_000_000, PatternKind::Pulse, 4_000);
        // Asked, looked at, and drawn by a resting frame of the lamp's own
        // light or another face: how long ago its pulse began, if it did.
        let mut shown = |ring: &mut RingLight, at: &mut u64, number, source: &str, own| {
            // Ticked first, as the ring always is: what lapsed is gone.
            looked(ring, at);
            glows.publish_json(number, &pulse(source, *at)).unwrap();
            looked(ring, at);
            ring.drew(*at, own);
            ring.pattern().map(|(_, since)| *at - since)
        };
        assert_eq!(shown(&mut ring, &mut at, 5, "skill:news", true), Some(0));
        at += 10_000_000;
        assert_eq!(
            shown(&mut ring, &mut at, 6, "skill:news", true),
            Some(4_000_000),
            "within the minute: played already"
        );
        at += 10_000_000;
        assert_eq!(shown(&mut ring, &mut at, 7, "skill:mail", true), Some(0));
        at += LightPattern::PULSE_GAP_US;
        assert_eq!(
            shown(&mut ring, &mut at, 8, "skill:news", true),
            Some(0),
            "a minute on, it plays"
        );
        // A pulse only a turn's face drew is not counted: the next plays.
        at += 10_000_000;
        assert_eq!(shown(&mut ring, &mut at, 9, "skill:quiet", false), None);
        at += 10_000_000;
        assert_eq!(shown(&mut ring, &mut at, 10, "skill:quiet", true), Some(0));
    }

    /// A dev lamp's `LAMP_RING_EVERY_PULSE=on`: the same asker's second
    /// pulse within the minute plays from its start, not as played; off
    /// (the default and the product's), it is drawn as played.
    #[test]
    fn every_pulse_plays_each_pulse_on_a_dev_lamp_and_only_there() {
        assert_eq!(
            every_pulse_arguments(Some("on")),
            vec![EVERY_PULSE_FLAG.to_owned()]
        );
        for value in [None, Some("off"), Some("ON"), Some("")] {
            assert!(every_pulse_arguments(value).is_empty(), "{value:?}");
        }
        let flags = vec![EVERY_PULSE_FLAG.to_owned(), "--rest".to_owned()];
        assert_eq!(take_every_pulse(&flags), (true, &flags[1..]));
        assert_eq!(take_every_pulse(&flags[1..]), (false, &flags[1..]));
        for (every, second) in [(false, Some(4_000_000)), (true, Some(0))] {
            let root = Root::new(if every {
                "every-pulse-on"
            } else {
                "every-pulse-off"
            });
            let (mut ring, _) = RingLight::on(&root.0).unwrap();
            ring.every_pulse(every);
            ring.take_requests(&root.0, "boot-test").unwrap();
            let bus = Bus::new(&root.0).unwrap();
            let mut glows = bus
                .publisher_declared(LightRequest::TOPIC, &LightRequest::declaration(), "skills")
                .unwrap();
            let mut at = 0u64;
            ring.tick(at);
            let mut shown = |ring: &mut RingLight, at: &mut u64, number| {
                looked(ring, at);
                let mut asked = request(
                    "skill:news",
                    lamp_msgs::Priority::Skill,
                    *at + 5_000_000,
                    "red",
                );
                asked.pattern = Some(LightPattern {
                    kind: PatternKind::Pulse,
                    period_ms: 4_000,
                    low: 1,
                });
                glows.publish_json(number, &asked).unwrap();
                looked(ring, at);
                ring.drew(*at, true);
                ring.pattern().map(|(_, since)| *at - since)
            };
            assert_eq!(shown(&mut ring, &mut at, 1), Some(0));
            at += 10_000_000;
            assert_eq!(shown(&mut ring, &mut at, 2), second, "every_pulse {every}");
        }
    }

    /// #940: two skills ask for the light at once. The newer is shown and
    /// the other is told on `light/outranked` what is over it, once, and
    /// again after its renewal; it is shown by itself once the winner's
    /// lease ends, and under the person's light it is told it is theirs.
    #[test]
    fn a_skill_whose_light_is_not_shown_is_told_what_is_over_it() {
        let root = Root::new("light-outranked");
        let (mut ring, _) = RingLight::on(&root.0).unwrap();
        ring.take_requests(&root.0, "boot-test").unwrap();
        let bus = Bus::new(&root.0).unwrap();
        let mut outranked = bus.subscribe(LightOutranked::TOPIC, "skills").unwrap();
        let mut asks = bus
            .publisher_declared(Light::TOPICS[0], &Light::declaration(), "actions")
            .unwrap();
        let mut glows = bus
            .publisher_declared(LightRequest::TOPIC, &LightRequest::declaration(), "skills")
            .unwrap();
        let told = |outranked: &mut lamp_bus::Subscriber| {
            let mut said = Vec::new();
            while let Some(message) = outranked.recv(Duration::ZERO).unwrap() {
                let told: LightOutranked = message.parse().unwrap();
                said.push(format!("{} by {}", told.source, told.by));
            }
            said
        };
        let mut at = 0u64;
        ring.tick(at);
        let skill = lamp_msgs::Priority::Skill;
        let tutor = request("skill:tutor", skill, 10_000_000, "white");
        glows.publish_json(1, &tutor).unwrap();
        looked(&mut ring, &mut at);
        assert!(told(&mut outranked).is_empty(), "alone, it is shown");
        glows
            .publish_json(2, &request("skill:stretch", skill, 1_500_000, "green"))
            .unwrap();
        looked(&mut ring, &mut at);
        assert_eq!(ring.face(), &light(true, "green", 4));
        assert_eq!(told(&mut outranked), ["skill:tutor by skill:stretch"]);
        looked(&mut ring, &mut at);
        assert!(told(&mut outranked).is_empty(), "said once");
        // Its renewal is told again, and the newer still shown.
        glows.publish_json(3, &tutor).unwrap();
        looked(&mut ring, &mut at);
        assert_eq!(ring.face(), &light(true, "green", 4));
        assert_eq!(told(&mut outranked), ["skill:tutor by skill:stretch"]);
        // The winner's lease ends: the loser is shown by itself, told
        // nothing.
        at = 1_500_000;
        looked(&mut ring, &mut at);
        assert_eq!(ring.face(), &light(true, "white", 4));
        assert!(told(&mut outranked).is_empty());
        asks.publish_json(4, &light(true, "blue", 2)).unwrap();
        looked(&mut ring, &mut at);
        assert_eq!(ring.face(), &light(true, "blue", 2));
        assert_eq!(told(&mut outranked), ["skill:tutor by person"]);
    }

    /// The ring's own socket on `topic`, for a datagram made by hand.
    fn ring_socket(root: &Path, topic: &str) -> std::path::PathBuf {
        std::fs::read_dir(root.join(topic).join("subs"))
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("ring."))
            })
            .unwrap()
    }

    /// A person's light from `actions`, stamped `at_unix_ms` by its wall
    /// clock whatever the time is.
    fn person_at(root: &Path, seq: u64, at_unix_ms: u64, colour: &str) {
        let mut bytes = serde_json::to_vec(&serde_json::json!({
            "topic": Light::TOPICS[0], "kind": "latest", "enc": "json", "type": Light::TYPE,
            "source": "actions", "boot": "0123456789abcdef", "seq": seq,
            "at_us": seq, "at_unix_ms": at_unix_ms
        }))
        .unwrap();
        bytes.push(b'\n');
        bytes.extend(serde_json::to_vec(&light(true, colour, 2)).unwrap());
        std::os::unix::net::UnixDatagram::unbound()
            .unwrap()
            .send_to(&bytes, ring_socket(root, Light::TOPICS[0]))
            .unwrap();
    }

    /// The reviewer on #993 (11:35): the wall clock set back. A person's
    /// light stamped earlier than one before it is still the person asking,
    /// and a request sent after it in a later look is not held off by the
    /// older, later-stamped light.
    #[test]
    fn a_clock_set_back_neither_hides_the_persons_light_nor_holds_a_glow_off() {
        let root = Root::new("light-request-clock");
        let (mut ring, _) = RingLight::on(&root.0).unwrap();
        ring.take_requests(&root.0, "boot-test").unwrap();
        let bus = Bus::new(&root.0).unwrap();
        let mut glows = bus
            .publisher_declared(LightRequest::TOPIC, &LightRequest::declaration(), "skills")
            .unwrap();
        let mut at = 0u64;
        ring.tick(at);
        let now_ms = unix_ms();
        // Before the clock went back: a light stamped an hour ahead.
        person_at(&root.0, 1, now_ms + 3_600_000, "blue");
        assert_eq!(taken(&mut ring, &mut at), Lit::of(true, "blue", 2));
        // A glow asked after it, in a later look, with today's earlier stamp.
        let glow = |at: u64| {
            request(
                "skill:focus",
                lamp_msgs::Priority::Skill,
                at + 5_000_000,
                "purple",
            )
        };
        glows.publish_json(1, &glow(at)).unwrap();
        assert_eq!(taken(&mut ring, &mut at), Lit::of(true, "purple", 4));
        // The clock is back: the person's next light is stamped earlier
        // than the first, and still ends the glow.
        person_at(&root.0, 2, now_ms, "white");
        assert_eq!(taken(&mut ring, &mut at), Lit::of(true, "white", 2));
        glows.publish_json(2, &glow(at)).unwrap();
        for _ in 0..5 {
            at += STEP_US;
            assert_eq!(ring.tick(at), None);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(ring.face(), &light(true, "white", 2));
    }

    fn kept(path: &Path) -> Option<Light> {
        serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
    }

    /// The first builder on #1464: with `LAMP_LIGHT_KEEP=on` a lamp set blue
    /// at level 3 wakes up blue at level 3. The bus's value, a light asked
    /// for since, comes before the kept one; a kept file that is not a light
    /// is passed over; a burst of changes writes at most once a second and
    /// never loses the last; only the person's light is kept.
    #[test]
    fn the_persons_light_is_kept_across_a_restart_when_switched_on() {
        let disk = Root::new("light-keep-disk");
        let path = disk.0.join("person.json");
        let blue = light(true, "blue", 3);
        let mut at = 0u64;
        {
            let root = Root::new("light-keep-1");
            let (mut ring, begins) = RingLight::on_keeping(&root.0, Some(&path)).unwrap();
            assert_eq!(begins, Lit::first());
            let bus = Bus::new(&root.0).unwrap();
            let mut asks = bus
                .publisher_declared(Light::TOPICS[0], &Light::declaration(), "actions")
                .unwrap();
            let mut tests = bus
                .publisher_declared(TEST_TOPIC, &Light::declaration(), "selftest")
                .unwrap();
            asks.publish_json(1, &light(true, "red", 5)).unwrap();
            assert_eq!(taken(&mut ring, &mut at), Lit::of(true, "red", 5));
            // The first tick wrote the first light; red waits its second.
            assert_eq!(kept(&path), Some(first()));
            std::thread::sleep(Duration::from_millis(2));
            asks.publish_json(2, &blue).unwrap();
            assert_eq!(taken(&mut ring, &mut at), Lit::of(true, "blue", 3));
            ring.tick(at + 1);
            assert_eq!(kept(&path), Some(first()));
            // A second on, the last of the burst is written, not red.
            at += KEEP_EVERY_US;
            ring.tick(at);
            assert_eq!(kept(&path), Some(blue.clone()));
            // A self-test's colour is shown, never kept.
            std::thread::sleep(Duration::from_millis(2));
            tests.publish_json(3, &light(true, "green", 5)).unwrap();
            assert_eq!(taken(&mut ring, &mut at), Lit::of(true, "green", 5));
            at += KEEP_EVERY_US;
            ring.tick(at);
            assert_eq!(kept(&path), Some(blue.clone()));
        }
        // A restart: the bus is empty, the kept light is begun with, and
        // said on `light/state` as the person's.
        let root = Root::new("light-keep-2");
        let (mut ring, begins) = RingLight::on_keeping(&root.0, Some(&path)).unwrap();
        assert_eq!(begins, Lit::of(true, "blue", 3).unwrap());
        assert_eq!(ring.now(), &blue);
        ring.tick(at + STEP_US);
        assert_eq!(state(&Bus::new(&root.0).unwrap()), Some(blue.clone()));
        // Switched off, nothing on disk is read.
        let off = Root::new("light-keep-off");
        assert_eq!(RingLight::on(&off.0).unwrap().1, Lit::first());
        // A light the bus keeps comes first, and is then written.
        let asked = Root::new("light-keep-bus");
        Bus::new(&asked.0)
            .unwrap()
            .publisher_declared(Light::TOPICS[0], &Light::declaration(), "actions")
            .unwrap()
            .publish_json(1, &light(true, "purple", 2))
            .unwrap();
        let (mut ring, begins) = RingLight::on_keeping(&asked.0, Some(&path)).unwrap();
        assert_eq!(begins, Lit::of(true, "purple", 2).unwrap());
        ring.tick(1);
        assert_eq!(kept(&path), Some(light(true, "purple", 2)));
        // Not a light there is: the first.
        for text in ["nonsense", r#"{"on":true,"colour":"violet","level":3}"#] {
            std::fs::write(&path, text).unwrap();
            let root = Root::new("light-keep-bad");
            assert_eq!(
                RingLight::on_keeping(&root.0, Some(&path)).unwrap().1,
                Lit::first()
            );
        }
    }

    #[test]
    fn the_keep_switch_is_off_unless_on_and_its_flag_is_taken() {
        assert!(keep_arguments(None).is_empty());
        assert!(keep_arguments(Some("off")).is_empty());
        let on = keep_arguments(Some("on"));
        assert_eq!(on, ["--light-keep", KEEP_PATH]);
        let rest = ["--other".to_owned()];
        let flags: Vec<String> = on.iter().cloned().chain(rest.clone()).collect();
        assert_eq!(take_keep(&flags), (Some(Path::new(KEEP_PATH)), &rest[..]));
        assert_eq!(take_keep(&rest), (None, &rest[..]));
    }

    /// The summed channels of a frame.
    fn brightness(frame: &[lamp_ring::Rgb; lamp_ring::PIXEL_COUNT]) -> u32 {
        frame
            .iter()
            .map(|pixel| u32::from(pixel.red) + u32::from(pixel.green) + u32::from(pixel.blue))
            .sum()
    }

    /// The mean of a face resting steady in warm at `level`, at the
    /// product's ceiling (24), after it has eased in.
    fn steady_warm(level: u8) -> u32 {
        let mut face = crate::ring_face::Face::new();
        face.light(Lit::of(true, "warm", level).unwrap());
        let frames: Vec<u32> = (0..800u64)
            .map(|frame| {
                brightness(&face.render(crate::ring_face::Mood::Resting, 0, frame * 25_000, 24))
            })
            .collect();
        frames[200..].iter().sum::<u32>() / 600
    }

    /// Row 7 on lamp-06c1 (#1672): the person's light was warm 3, and the
    /// evening glow it resolved to is warm 3 breathing down to 2 every 10 s
    /// for 20 s. Through the ring's own loop at its 25 ms resting pace and
    /// the product's ceiling, the face swings from about level 2's glow to
    /// about level 3's: the breath is drawn, a level deep.
    #[test]
    fn the_evening_glow_over_a_warm_three_breathes_down_to_level_two_and_back() {
        let root = Root::new("light-evening");
        let (mut ring, _) = RingLight::on(&root.0).unwrap();
        ring.take_requests(&root.0, "boot-test").unwrap();
        let bus = Bus::new(&root.0).unwrap();
        let mut asks = bus
            .publisher_declared(Light::TOPICS[0], &Light::declaration(), "actions")
            .unwrap();
        let mut glows = bus
            .publisher_declared(LightRequest::TOPIC, &LightRequest::declaration(), "skills")
            .unwrap();
        let mut face = crate::ring_face::Face::new();
        let mut at = 0u64;
        ring.tick(at);
        asks.publish_json(1, &light(true, "warm", 3)).unwrap();
        face.light(taken(&mut ring, &mut at).unwrap());
        let mut evening = request(
            "skill:bedtime-morning",
            lamp_msgs::Priority::Skill,
            at + 30_000_000,
            "warm",
        );
        evening.light.level = 3;
        evening.pattern = Some(LightPattern {
            kind: PatternKind::Breathe,
            period_ms: 10_000,
            low: 2,
        });
        glows.publish_json(2, &evening).unwrap();
        // Until the ring has taken the request off the bus, on the clock it
        // was asked at, so the frames below are the breath's and none wait.
        // A pattern begins on its first resting frame, so each wait draws
        // one.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while ring.pattern().is_none() && std::time::Instant::now() < deadline {
            at += STEP_US;
            if let Some(lit) = ring.tick(at) {
                face.light(lit);
            }
            ring.drew(at, true);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            ring.pattern().is_some(),
            "the ring took the evening's request"
        );
        let mut drawn = Vec::new();
        let until = at + 20_000_000;
        while at < until {
            if let Some(lit) = ring.tick(at) {
                face.light(lit);
            }
            face.pattern(ring.pattern());
            let frame = face.render(crate::ring_face::Mood::Resting, 0, at, 24);
            ring.drew(at, face.rests_on_its_light(at));
            drawn.push(brightness(&frame));
            at += 25_000;
        }
        assert!(ring.pattern().is_some(), "the breath is the face's");
        let (two, three) = (steady_warm(2), steady_warm(3));
        // After the first second, while the ring has taken it.
        let breathing = &drawn[40..];
        let (least, most) = (
            *breathing.iter().min().unwrap(),
            *breathing.iter().max().unwrap(),
        );
        assert!(two < three, "{two} under {three}");
        assert!(least * 10 <= two * 12, "{least} against level 2's {two}");
        assert!(most * 10 >= three * 9, "{most} against level 3's {three}");
    }
}
