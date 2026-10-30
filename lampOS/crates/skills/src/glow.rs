//! A skill's light asked of the ring on `light/request` (#940,
//! `docs/skills.md`), with `LAMP_LIGHT_REQUESTS=on`. The skill never touches
//! the person's light: it asks from `skill:<name>` at skill priority, asks
//! again while its run keeps the light, and gives it back by asking with a
//! time already passed. The ring shows it over the person's light until
//! then, or until the person asks for theirs, and then shows theirs again
//! by itself. When the ring holds the light but shows another over it, it
//! says so on `light/outranked`: the run says what was over it in its
//! journal and at its give-back, and keeps asking, so its light comes back
//! by itself once the other lapses (#940).
use crate::Call;
use lamp_bus::{Bus, Publisher, Subscriber};
use lamp_msgs::{
    Claim, Light, LightOutranked, LightPattern, LightReached, LightRequest, PatternKind, Priority,
};
use std::{
    collections::HashMap,
    io,
    sync::{Arc, Mutex, MutexGuard, PoisonError, mpsc},
    thread::JoinHandle,
    time::{Duration, Instant},
};

/// The switch, off unless `on`, as the ring's.
pub const REQUESTS_ENVIRONMENT: &str = "LAMP_LIGHT_REQUESTS";
/// A skill's light is asked again this often while its run keeps it...
pub const GLOW_EVERY: Duration = Duration::from_secs(10);
/// ...and each ask holds this long, so a late one still comes before the
/// last has lapsed: a gap would let the glow come back over a light the
/// person asked for meanwhile (#993).
pub const GLOW_HOLDS_US: u64 = 15_000_000;

/// The light a colour with no level asks for when the light's own is not
/// known: the middle, so a page lit by a skill can be read (the seventh on
/// #1132). A skill that needs a level names it.
pub const LEVEL_UNKNOWN: u8 = 3;

/// The evening's warmth, coolest first (#1672): the ring's white and its
/// amber `warm`, as the actions part's warmer steps them. Warm is as warm as
/// evening light goes; orange and the other colours are a colour the person
/// chose, which a glow does not change, only dims.
pub const EVENING_WARMTH: [&str; 2] = ["white", "warm"];

/// The ring's levels, 1 to [`LEVELS`].
pub const LEVELS: u8 = 5;

/// What [`Call::Relative`] is, against the person's light as the ring says
/// it shows it (`light/state`), or why nothing is to be shown: the light
/// off or not known. It is always at least one level from the person's
/// light, and only down (the first builder on #1672: "one visible level,
/// never the colour", and at 21:18 "down, never up"): a warmth that is
/// already as warm as it goes is no change a person sees, so the level is
/// what shows the glow. At level 1 there is none under it, and nothing is
/// asked: someone at their dimmest light is already resting. A call with
/// `warm_at_dimmest` (Glad's and Lively's, #3691) asks the warmth alone
/// there, steady at level 1, when the colour can still warm; when it is
/// already warm, nothing.
///
/// A [`Call::Light`] with `no_brighter` is made never brighter than the
/// person's light (#3691: Not feeling well's pale breath over a person at
/// level 1): its level at most theirs, a breath's low under that, and
/// steady at level 1, where no breath goes lower. Over a light that is off
/// nothing is asked; with the light not known, it is asked as written.
/// Any other call is as it was.
pub fn relative(now: Option<&Light>, call: &Call) -> Result<Call, &'static str> {
    if let Call::Light {
        level: Some(asked),
        pattern,
        no_brighter: true,
        ..
    } = call
    {
        return no_brighter(now, call, *asked, *pattern);
    }
    let Call::Relative {
        warmer,
        dimmer,
        lasting,
        breathe_ms,
        warm_at_dimmest,
    } = call
    else {
        return Ok(call.clone());
    };
    let Some(now) = now else {
        return Err("the light shown is not known");
    };
    if !now.on {
        return Err("the light is off");
    }
    let colour = match EVENING_WARMTH.iter().position(|shade| *shade == now.colour) {
        Some(at) => EVENING_WARMTH[(at + usize::from(*warmer)).min(EVENING_WARMTH.len() - 1)],
        None => now.colour.as_str(),
    };
    // At least one level down, whatever `dimmer` says; never up.
    let low = now.level.saturating_sub((*dimmer).max(1)).max(1);
    if low == now.level {
        // No level under it: the warmth alone, steady, if it is asked for
        // and the colour has a step left (#3691).
        if *warm_at_dimmest && colour != now.colour {
            return Ok(Call::Light {
                colour: colour.into(),
                level: Some(now.level),
                lasting: lasting.clone(),
                pattern: None,
                keep: false,
                until_ends: false,
                no_brighter: false,
            });
        }
        return Err("the person's light is at its dimmest: no evening glow");
    }
    // With a breath, it stays at the person's level and breathes down the
    // steps and back; steady, it is the steps down.
    let (level, pattern) = match breathe_ms {
        Some(period_ms) => (
            now.level,
            Some(LightPattern {
                kind: PatternKind::Breathe,
                period_ms: *period_ms,
                low,
            }),
        ),
        None => (low, None),
    };
    Ok(Call::Light {
        colour: colour.into(),
        level: Some(level),
        lasting: lasting.clone(),
        pattern,
        keep: false,
        until_ends: false,
        no_brighter: false,
    })
}

/// A `no_brighter` light over `now`: its level at most the person's, and a
/// breath's low under it; a breath with no level under it is steady.
fn no_brighter(
    now: Option<&Light>,
    call: &Call,
    asked: u8,
    pattern: Option<LightPattern>,
) -> Result<Call, &'static str> {
    let Call::Light {
        colour,
        lasting,
        keep,
        until_ends,
        ..
    } = call
    else {
        return Ok(call.clone());
    };
    let level = match now {
        // Not known: as written, a light checked on a lamp at that level.
        None => asked,
        Some(now) if !now.on => {
            return Err("the light is off: no light brighter than the person's");
        }
        Some(now) => asked.min(now.level.max(1)),
    };
    let pattern = pattern.and_then(|pattern| match pattern.kind {
        PatternKind::Breathe if level > 1 => Some(LightPattern {
            low: pattern.low.min(level - 1),
            ..pattern
        }),
        PatternKind::Breathe => None,
        _ => pattern.check(level).is_ok().then_some(pattern),
    });
    Ok(Call::Light {
        colour: colour.clone(),
        level: Some(level),
        lasting: lasting.clone(),
        pattern,
        keep: *keep,
        until_ends: *until_ends,
        no_brighter: false,
    })
}

/// How a skill asks the ring for its light: the bus, to see that the ring
/// takes `light/request`, the publisher on it, the boot its claims are on,
/// and how often a light kept is asked again.
#[derive(Clone)]
pub struct Glows {
    pub bus: Bus,
    pub out: Arc<Mutex<Publisher>>,
    pub clock_id: String,
    pub every: Duration,
    /// When each skill's last pulse was drawn, as the ring said it on
    /// `light/reached`, on the program's clock: the ring plays one a minute
    /// from an asker, counted from what it drew, and the run says so.
    pub pulses: Arc<Mutex<HashMap<String, Instant>>>,
}

/// A lock that a panic elsewhere left poisoned is still used: what it holds
/// is a light or a publisher, whole either way (the seventh on #1132).
fn locked<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One run's light, asked of the ring while the run lasts.
pub struct Glow {
    glows: Glows,
    source: String,
    /// What the run's light steps have made of the light so far: the light
    /// shown before the run, then each step's.
    light: Option<Light>,
    /// Once asked: what is asked again, and by whom.
    renewing: Option<Renewing>,
}

/// What is asked of the ring: the light, how it moves, and how slowly it
/// is faded into (#1677).
type Asked = (Light, Option<LightPattern>, Option<u32>);

/// The renewals of a skill's light: the light asked again, with its
/// pattern and fade, their stop, and their thread, which gives back what
/// the light was outranked by.
struct Renewing {
    held: Arc<Mutex<Asked>>,
    stop: mpsc::Sender<()>,
    thread: JoinHandle<Vec<String>>,
}

/// `light/outranked`, subscribed to under a name of its own for each run,
/// before its light is first asked, so the ring's answer is not missed.
fn outranked(bus: &Bus) -> Option<Subscriber> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let number = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    bus.subscribe_typed(
        LightOutranked::TOPIC,
        &format!("skills_glow_{number}"),
        LightOutranked::TYPE,
    )
    .map_err(|error| eprintln!("lamp-skills: light/outranked not subscribed to: {error}"))
    .ok()
}

/// `light/reached`, subscribed to under a name of its own for each run
/// before its light is first asked: a pulse the ring drew is counted from
/// there (the reviewer on #1494).
fn reached(bus: &Bus) -> Option<Subscriber> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let number = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    bus.subscribe_typed(
        LightReached::TOPIC,
        &format!("skills_seen_{number}"),
        LightReached::TYPE,
    )
    .map_err(|error| eprintln!("lamp-skills: light/reached not subscribed to: {error}"))
    .ok()
}

/// A pulse of `source`'s the ring said drawn since the last look, while
/// the light it holds has one, counted in `pulses` from then; its renewals,
/// said again, keep the first.
fn seen(
    reached: Option<&mut Subscriber>,
    source: &str,
    held: &Mutex<Asked>,
    pulses: &Mutex<HashMap<String, Instant>>,
) {
    let Some(reached) = reached else {
        return;
    };
    while let Ok(Some(message)) = reached.recv(Duration::ZERO) {
        let Ok(drawn) = message.parse::<LightReached>() else {
            continue;
        };
        let pulsing = locked(held)
            .1
            .is_some_and(|pattern| pattern.kind == PatternKind::Pulse);
        if drawn.source == source && pulsing {
            let now = crate::clock::now();
            let gap = Duration::from_micros(LightPattern::PULSE_GAP_US);
            let mut pulses = locked(pulses);
            pulses.retain(|_, at| now.saturating_duration_since(*at) < gap);
            pulses.entry(source.to_owned()).or_insert(now);
        }
    }
}

/// What `source`'s light was said outranked by since the last look, each
/// added to `by` once, in the order heard, and said in the journal.
fn heard(outranked: Option<&mut Subscriber>, source: &str, by: &mut Vec<String>) {
    let Some(outranked) = outranked else {
        return;
    };
    while let Ok(Some(message)) = outranked.recv(Duration::ZERO) {
        let Ok(told) = message.parse::<LightOutranked>() else {
            continue;
        };
        if told.source == source && !by.contains(&told.by) {
            eprintln!("lamp-skills: {source}'s light is outranked by {}", told.by);
            by.push(told.by);
        }
    }
}

impl Glow {
    /// A run of `skill`, with the light `shown` before it, if known.
    pub fn new(glows: &Glows, skill: &str, shown: Option<Light>) -> Self {
        // A claim names its source in at most 64 characters, and a skill's
        // name may have 64 of its own.
        let source: String = format!("skill:{skill}").chars().take(64).collect();
        Self {
            glows: glows.clone(),
            source,
            light: shown,
            renewing: None,
        }
    }

    /// Asks the ring for what `call`, a light, a dim or the light going
    /// out, makes of the light, and says how it went.
    pub fn ask(&mut self, call: &Call) -> String {
        let (what, verb) = match call {
            Call::Light { colour, .. } => (format!("light {colour}"), "dim"),
            Call::Out { .. } => ("light out".into(), "put out"),
            _ => ("dim".into(), "dim"),
        };
        let Some(light) = asked(call, self.light.as_ref()) else {
            return format!("{what}: left alone, since no light is on to {verb}");
        };
        // The skill's steps build on each other as written, whether or not
        // the last one could be asked. A dim is steady.
        self.light = Some(light.clone());
        let pattern = match call {
            Call::Light { pattern, .. } => *pattern,
            _ => None,
        };
        // A light going out is faded into, when its step says over how long.
        let fade = match call {
            Call::Out { over_ms, .. } => *over_ms,
            _ => None,
        };
        // Looked at each time, so a ring that starts during the run is asked
        // from its next step on (the seventh on #1132).
        let taken = self
            .glows
            .bus
            .subscribers(LightRequest::TOPIC)
            .is_ok_and(|subscribers| !subscribers.is_empty());
        if !taken {
            return format!(
                "{what}: left alone, since nothing takes light/request (the ring needs {REQUESTS_ENVIRONMENT}=on)"
            );
        }
        // Listened to before the first ask, and by the renewals after it.
        let (mut told, mut drawn) = match self.renewing {
            Some(_) => (None, None),
            None => (outranked(&self.glows.bus), reached(&self.glows.bus)),
        };
        let until_us = lamp_ipc::monotonic_us() + GLOW_HOLDS_US;
        let asking = (light.clone(), pattern, fade);
        let delivered = match ask_ring(&self.glows, &self.source, &asking, until_us) {
            Ok(delivered) => delivered,
            Err(error) => return format!("{what}: light/request not written ({error})"),
        };
        let untaken = if delivered == 0 {
            ", but no subscriber took it"
        } else {
            ""
        };
        let level = light.level;
        let moving = pattern.map_or(String::new(), |pattern| self.moving(pattern));
        let fading = fade.map_or(String::new(), |ms| format!(", faded into over {ms} ms"));
        match &self.renewing {
            Some(renewing) => *locked(&renewing.held) = asking,
            None => {
                let held = Arc::new(Mutex::new(asking));
                let (stop, stopped) = mpsc::channel::<()>();
                let (glows, source, again) =
                    (self.glows.clone(), self.source.clone(), Arc::clone(&held));
                let thread = std::thread::spawn(move || {
                    let mut by = Vec::new();
                    // Asked again on the program's clock; each claim
                    // holds on the host's, which the ring reads.
                    while let Err(mpsc::RecvTimeoutError::Timeout) =
                        crate::clock::recv_timeout(&stopped, glows.every)
                    {
                        heard(told.as_mut(), &source, &mut by);
                        seen(drawn.as_mut(), &source, &again, &glows.pulses);
                        let asked = locked(&again).clone();
                        let until_us = lamp_ipc::monotonic_us() + GLOW_HOLDS_US;
                        if let Err(error) = ask_ring(&glows, &source, &asked, until_us) {
                            eprintln!("lamp-skills: {source}'s light not asked again: {error}");
                        }
                    }
                    heard(told.as_mut(), &source, &mut by);
                    seen(drawn.as_mut(), &source, &again, &glows.pulses);
                    by
                });
                self.renewing = Some(Renewing { held, stop, thread });
            }
        }
        if matches!(call, Call::Out { .. }) {
            return format!("{what}: asked of the ring{fading}{untaken}");
        }
        format!("{what}: asked of the ring at level {level}{moving}{untaken}")
    }

    /// How the run's line says a pattern (#940 item 3), and for a pulse
    /// whether the ring plays it: one a minute from the same skill, counted
    /// from the last the ring drew.
    fn moving(&self, pattern: LightPattern) -> String {
        let ms = pattern.period_ms;
        match pattern.kind {
            PatternKind::Breathe => {
                return format!(", breathing every {ms} ms down to {}", pattern.low);
            }
            PatternKind::Candle => return format!(", flickering as a candle every {ms} ms"),
            PatternKind::Pulse => {}
        }
        let said = format!(", with one pulse of {ms} ms");
        let now = crate::clock::now();
        let gap = Duration::from_micros(LightPattern::PULSE_GAP_US);
        match locked(&self.glows.pulses)
            .get(&self.source)
            .filter(|at| now.saturating_duration_since(**at) < gap)
        {
            Some(at) => format!(
                "{said}; the pulse is not played: one a minute, and the last was shown {} s ago",
                now.saturating_duration_since(*at).as_secs()
            ),
            None => said,
        }
    }

    /// Stops asking and gives the light back, if it was asked for.
    pub fn give_back(self) -> Option<String> {
        let Self {
            glows,
            source,
            renewing,
            ..
        } = self;
        let Renewing { held, stop, thread } = renewing?;
        drop(stop);
        let by = thread.join().unwrap_or_default();
        let asked = locked(&held).clone();
        // A time already passed: the ring lets it go at once, and shows the
        // person's light again.
        let until_us = lamp_ipc::monotonic_us().saturating_sub(1);
        let back = match ask_ring(&glows, &source, &asked, until_us) {
            Ok(_) => "light given back: asked again with a time already passed".into(),
            Err(error) => format!(
                "light not given back at once: light/request not written ({error}); the ring lets it go within {} s",
                GLOW_HOLDS_US / 1_000_000
            ),
        };
        Some(if by.is_empty() {
            back
        } else {
            format!("{back}; outranked by {}", by.join(", "))
        })
    }
}

/// The light a step asks for, from what the light is so far (`from`): a
/// colour at its level, or else at the level the light has
/// ([`LEVEL_UNKNOWN`] when that is not known); a dim steps down from a
/// light that is on, never below 1; out is that light off (the ring's
/// ember, never dark: it still shows the lamp can hear). None for a dim or
/// out with no light on.
pub fn asked(call: &Call, from: Option<&Light>) -> Option<Light> {
    match call {
        Call::Light { colour, level, .. } => Some(Light {
            on: true,
            colour: colour.clone(),
            level: level.unwrap_or_else(|| from.map_or(LEVEL_UNKNOWN, |from| from.level)),
        }),
        Call::Dim { steps, .. } => from.filter(|from| from.on).map(|from| Light {
            on: true,
            colour: from.colour.clone(),
            level: from.level.saturating_sub(*steps).max(1),
        }),
        Call::Out { .. } => from.filter(|from| from.on).map(|from| Light {
            on: false,
            ..from.clone()
        }),
        _ => None,
    }
}

/// One `light/request` from `source` at skill priority for a light, its
/// pattern and its fade, holding until `until_us` on the host's monotonic
/// clock, and how many subscribers took it: none is said in the journal,
/// as a ring that never got it shows nothing.
fn ask_ring(
    glows: &Glows,
    source: &str,
    (light, pattern, fade_ms): &Asked,
    until_us: u64,
) -> io::Result<usize> {
    let request = LightRequest {
        claim: Claim {
            source: source.to_owned(),
            priority: Priority::Skill,
            clock_id: glows.clock_id.clone(),
            until_us,
        },
        light: light.clone(),
        pattern: *pattern,
        fade_ms: *fade_ms,
    };
    let published = locked(&glows.out).publish_json(lamp_ipc::monotonic_us(), &request)?;
    if published.delivered == 0 {
        eprintln!(
            "lamp-skills: {source}'s light/request taken by no subscriber ({} dropped)",
            published.dropped
        );
    }
    Ok(published.delivered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_bus::{Bus, Subscriber};

    fn light(on: bool, colour: &str, level: u8) -> Light {
        Light {
            on,
            colour: colour.into(),
            level,
        }
    }

    fn colour(colour: &str, level: Option<u8>) -> Call {
        Call::Light {
            colour: colour.into(),
            level,
            lasting: None,
            pattern: None,
            keep: false,
            until_ends: false,
            no_brighter: false,
        }
    }

    fn dim(steps: u8) -> Call {
        Call::Dim {
            steps,
            until_ends: false,
        }
    }

    #[test]
    fn a_step_asks_for_its_colour_at_its_level_or_the_lights_and_a_dim_steps_down() {
        let blue = light(true, "blue", 3);
        assert_eq!(
            asked(&colour("warm", Some(5)), Some(&blue)),
            Some(light(true, "warm", 5))
        );
        assert_eq!(
            asked(&colour("warm", None), Some(&blue)),
            Some(light(true, "warm", 3))
        );
        // Not known: the middle, so a page can be read.
        assert_eq!(
            asked(&colour("warm", None), None),
            Some(light(true, "warm", LEVEL_UNKNOWN))
        );
        assert_eq!(asked(&dim(1), Some(&blue)), Some(light(true, "blue", 2)));
        assert_eq!(asked(&dim(4), Some(&blue)), Some(light(true, "blue", 1)));
        // Nothing on to dim, or not known: left alone.
        assert_eq!(asked(&dim(1), Some(&light(false, "blue", 3))), None);
        assert_eq!(asked(&dim(1), None), None);
    }

    /// A bus, the skills' asking on it, and nothing yet taking requests.
    fn a_bus(every: Duration) -> (tempfile::TempDir, Glows) {
        let dir = tempfile::Builder::new()
            .prefix("gl")
            .tempdir_in("/tmp")
            .unwrap();
        let bus = Bus::new(dir.path()).unwrap();
        let out = bus
            .publisher_declared(LightRequest::TOPIC, &LightRequest::declaration(), "skills")
            .unwrap();
        let glows = Glows {
            bus,
            out: Arc::new(Mutex::new(out)),
            clock_id: "boot-test".into(),
            every,
            pulses: Arc::default(),
        };
        (dir, glows)
    }

    /// The same with a ring taking requests.
    fn on_a_bus(every: Duration) -> (tempfile::TempDir, Glows, Subscriber) {
        let (dir, glows) = a_bus(every);
        let ring = glows.bus.subscribe(LightRequest::TOPIC, "ring").unwrap();
        (dir, glows, ring)
    }

    fn taken(ring: &mut Subscriber) -> Vec<LightRequest> {
        let mut requests = Vec::new();
        while let Ok(Some(message)) = ring.recv(Duration::ZERO) {
            requests.push(message.parse().unwrap());
        }
        requests
    }

    #[test]
    fn a_light_is_asked_from_the_skill_asked_again_while_kept_and_given_back() {
        let (_dir, glows, mut ring) = on_a_bus(Duration::from_millis(50));
        let mut glow = Glow::new(&glows, "desk-tutor", Some(light(true, "blue", 3)));
        let said = glow.ask(&colour("white", Some(5)));
        assert_eq!(said, "light white: asked of the ring at level 5");
        let first = taken(&mut ring);
        assert_eq!(first.len(), 1);
        let now = lamp_ipc::monotonic_us();
        assert_eq!(first[0].light, light(true, "white", 5));
        assert_eq!(first[0].claim.source, "skill:desk-tutor");
        assert_eq!(first[0].claim.priority, Priority::Skill);
        assert_eq!(first[0].claim.clock_id, "boot-test");
        assert!(first[0].claim.holds_at("boot-test", now));
        // A dim in the same run steps down from the light asked.
        assert_eq!(glow.ask(&dim(2)), "dim: asked of the ring at level 3");
        std::thread::sleep(Duration::from_millis(180));
        let again = taken(&mut ring);
        assert!(again.len() >= 3, "asked again every 50 ms: {again:?}");
        assert!(
            again[1..]
                .iter()
                .all(|request| request.light == light(true, "white", 3)
                    && request
                        .claim
                        .holds_at("boot-test", lamp_ipc::monotonic_us())),
            "{again:?}"
        );
        assert_eq!(
            glow.give_back().as_deref(),
            Some("light given back: asked again with a time already passed")
        );
        let back = taken(&mut ring);
        assert_eq!(back.len(), 1, "no renewal after the give-back: {back:?}");
        assert!(
            !back[0]
                .claim
                .holds_at("boot-test", lamp_ipc::monotonic_us())
        );
        std::thread::sleep(Duration::from_millis(120));
        assert!(taken(&mut ring).is_empty(), "the renewals have stopped");
    }

    /// #940: the ring says a run's light was outranked; the run says by
    /// what at its give-back, each once, and not another's.
    #[test]
    fn a_light_outranked_is_said_at_the_give_back_by_what() {
        let (_dir, glows, mut ring) = on_a_bus(Duration::from_millis(50));
        let mut says = glows
            .bus
            .publisher_declared(
                LightOutranked::TOPIC,
                &LightOutranked::declaration(),
                "ring",
            )
            .unwrap();
        let mut glow = Glow::new(&glows, "desk-tutor", Some(light(true, "blue", 3)));
        glow.ask(&colour("white", Some(5)));
        assert_eq!(taken(&mut ring).len(), 1);
        let told = |source: &str, by: &str| LightOutranked {
            source: source.into(),
            by: by.into(),
        };
        for (at, (source, by)) in [
            ("skill:desk-tutor", "skill:stretch-break"),
            ("skill:stretch-break", "skill:desk-tutor"),
            ("skill:desk-tutor", "skill:stretch-break"),
            ("skill:desk-tutor", "person"),
        ]
        .into_iter()
        .enumerate()
        {
            says.publish_json(at as u64 + 1, &told(source, by)).unwrap();
        }
        std::thread::sleep(Duration::from_millis(120));
        assert_eq!(
            glow.give_back().as_deref(),
            Some(
                "light given back: asked again with a time already passed; \
                 outranked by skill:stretch-break, person"
            )
        );
    }

    /// #940 item 3: a light's pattern is asked of the ring with it and with
    /// each renewal, a dim is steady, and a skill's second pulse within a
    /// minute of one the ring drew is said not played; a pulse the ring
    /// never drew is not counted (the reviewer on #1494).
    #[test]
    fn a_pattern_is_asked_with_the_light_and_a_second_pulse_is_said_not_played() {
        let (_dir, glows, mut ring) = on_a_bus(Duration::from_millis(50));
        let mut says = glows
            .bus
            .publisher_declared(LightReached::TOPIC, &LightReached::declaration(), "ring")
            .unwrap();
        let patterned = |kind, period_ms| Call::Light {
            colour: "warm".into(),
            level: Some(3),
            lasting: None,
            pattern: Some(LightPattern {
                kind,
                period_ms,
                low: 1,
            }),
            keep: false,
            until_ends: false,
            no_brighter: false,
        };
        let mut glow = Glow::new(&glows, "calm", None);
        assert_eq!(
            glow.ask(&patterned(PatternKind::Breathe, 6_000)),
            "light warm: asked of the ring at level 3, breathing every 6000 ms down to 1"
        );
        std::thread::sleep(Duration::from_millis(120));
        let asked = taken(&mut ring);
        assert!(asked.len() >= 2, "{asked:?}");
        assert!(
            asked
                .iter()
                .all(|request| request.pattern.map(|p| p.kind) == Some(PatternKind::Breathe)),
            "renewals carry it: {asked:?}"
        );
        assert_eq!(glow.ask(&dim(1)), "dim: asked of the ring at level 2");
        std::thread::sleep(Duration::from_millis(80));
        assert!(
            taken(&mut ring)
                .iter()
                .all(|request| request.pattern.is_none())
        );
        glow.give_back();
        let pulse = patterned(PatternKind::Pulse, 4_000);
        // Drawn: the ring says so, and the next within the minute is not
        // played.
        let mut news = Glow::new(&glows, "news", None);
        assert_eq!(
            news.ask(&pulse),
            "light warm: asked of the ring at level 3, with one pulse of 4000 ms"
        );
        let drawn = LightReached {
            source: "skill:news".into(),
            light: Light {
                on: true,
                colour: "warm".into(),
                level: 3,
            },
        };
        says.publish_json(1, &drawn).unwrap();
        std::thread::sleep(Duration::from_millis(30));
        news.give_back();
        let mut again = Glow::new(&glows, "news", None);
        assert!(
            again
                .ask(&pulse)
                .ends_with("the pulse is not played: one a minute, and the last was shown 0 s ago")
        );
        again.give_back();
        // Never drawn (a turn over it, or outranked): not counted.
        let mut quiet = Glow::new(&glows, "quiet", None);
        quiet.ask(&pulse);
        quiet.give_back();
        let mut next = Glow::new(&glows, "quiet", None);
        assert_eq!(
            next.ask(&pulse),
            "light warm: asked of the ring at level 3, with one pulse of 4000 ms"
        );
        next.give_back();
    }

    #[test]
    fn nothing_is_asked_without_a_taker_or_a_light_to_dim_and_nothing_given_back() {
        let (_dir, glows) = a_bus(Duration::from_millis(50));
        let mut glow = Glow::new(&glows, "hello", Some(light(true, "blue", 3)));
        assert!(
            glow.ask(&colour("warm", None))
                .contains("left alone, since nothing takes light/request"),
        );
        assert_eq!(glow.give_back(), None);
        let mut ring = glows.bus.subscribe(LightRequest::TOPIC, "ring").unwrap();
        let mut glow = Glow::new(&glows, "hello", Some(light(false, "blue", 3)));
        assert_eq!(
            glow.ask(&dim(1)),
            "dim: left alone, since no light is on to dim"
        );
        assert_eq!(glow.give_back(), None);
        assert!(taken(&mut ring).is_empty());
    }

    /// The seventh on #1132: a ring that starts during a run is asked from
    /// the run's next step on.
    #[test]
    fn a_ring_that_starts_during_a_run_is_asked_from_the_next_step() {
        let (_dir, glows) = a_bus(Duration::from_secs(10));
        let mut glow = Glow::new(&glows, "desk-tutor", Some(light(true, "blue", 3)));
        assert!(glow.ask(&colour("white", Some(5))).contains("left alone"));
        let mut ring = glows.bus.subscribe(LightRequest::TOPIC, "ring").unwrap();
        assert_eq!(
            glow.ask(&dim(1)),
            "dim: asked of the ring at level 4",
            "the first step's light is the one the second steps down from"
        );
        assert_eq!(taken(&mut ring).len(), 1);
        assert!(glow.give_back().is_some());
    }

    #[test]
    fn a_long_name_is_cut_to_a_source_the_ring_takes() {
        let (_dir, glows, mut ring) = on_a_bus(Duration::from_secs(10));
        let name = "a".repeat(64);
        let mut glow = Glow::new(&glows, &name, None);
        glow.ask(&colour("warm", Some(2)));
        let request = taken(&mut ring).remove(0);
        assert_eq!(request.claim.source.len(), 64);
        assert!(request.check().is_ok());
        glow.give_back();
    }

    fn evening(breathe_ms: Option<u32>) -> Call {
        Call::Relative {
            warmer: 1,
            dimmer: 1,
            lasting: Some("20s".into()),
            breathe_ms,
            warm_at_dimmest: false,
        }
    }

    /// Glad's and Lively's light (#3691): the evening's, and the warmth
    /// alone at the dimmest level.
    fn feeling(breathe_ms: Option<u32>) -> Call {
        Call::Relative {
            warmer: 1,
            dimmer: 1,
            lasting: Some("12s".into()),
            breathe_ms,
            warm_at_dimmest: true,
        }
    }

    /// The level and the breath an evening ask over `now` comes to.
    fn evening_over(now: &Light, breathe: Option<u32>) -> (String, u8, Option<(u8, u8)>) {
        let Ok(Call::Light {
            colour,
            level: Some(level),
            pattern,
            ..
        }) = relative(Some(now), &evening(breathe))
        else {
            panic!("a light over {now:?}");
        };
        if let Some(pattern) = pattern {
            assert_eq!(pattern.kind, PatternKind::Breathe);
            assert!(pattern.check(level).is_ok(), "{pattern:?} at {level}");
        }
        (colour, level, pattern.map(|pattern| (level, pattern.low)))
    }

    /// Row 7 on lamp-06c1 (#1672), 17:41: the person's light was warm 3.
    /// Warmer goes no further than warm, so the glow is the level: warm 3
    /// breathing down to 2 and back, steady warm 2 without the breath. Its
    /// colour is the person's (the first builder: "one visible level, never
    /// the colour").
    #[test]
    fn over_a_warm_three_the_evening_breathes_a_level_down_in_the_persons_colour() {
        let warm_three = light(true, "warm", 3);
        assert_eq!(
            evening_over(&warm_three, Some(10_000)),
            ("warm".into(), 3, Some((3, 2)))
        );
        assert_eq!(evening_over(&warm_three, None), ("warm".into(), 2, None));
    }

    /// At the dimmest level there is no step under it, and the glow never
    /// goes up (the first builder, 21:18 on #2655): nothing is asked, and
    /// the journal says why.
    #[test]
    fn at_the_dimmest_level_the_evening_asks_nothing() {
        for colour in ["warm", "orange", "white"] {
            for breathe in [Some(10_000), None] {
                assert_eq!(
                    relative(Some(&light(true, colour, 1)), &evening(breathe)),
                    Err("the person's light is at its dimmest: no evening glow"),
                    "{colour}"
                );
            }
        }
    }

    /// Whatever the person's light above the dimmest, the glow is at least
    /// a level under it, never over: the breath from their level down, or
    /// steady under it. A dimmer of 0 still moves a level.
    #[test]
    fn every_evening_ask_is_at_least_one_level_under_the_persons_light() {
        for (dimmer, warm_at_dimmest) in [0, 1, 4].into_iter().flat_map(|d| [(d, false), (d, true)])
        {
            for level in 2..=LEVELS {
                for breathe_ms in [Some(10_000), None] {
                    let now = light(true, "warm", level);
                    let call = Call::Relative {
                        warmer: 1,
                        dimmer,
                        lasting: Some("20s".into()),
                        breathe_ms,
                        warm_at_dimmest,
                    };
                    let Ok(Call::Light {
                        colour,
                        level: Some(asked),
                        pattern,
                        ..
                    }) = relative(Some(&now), &call)
                    else {
                        panic!("a light at {level}");
                    };
                    assert_eq!(colour, "warm");
                    assert!((1..=level).contains(&asked));
                    let reaches = match pattern {
                        Some(pattern) => {
                            assert!(pattern.check(asked).is_ok());
                            assert_eq!(level, asked);
                            asked - pattern.low
                        }
                        None => level - asked,
                    };
                    assert!(
                        reaches >= 1,
                        "dimmer {dimmer} at {level}: {asked} {pattern:?}"
                    );
                }
            }
        }
    }

    /// A light that is off is the person's darkness, and is left so; one
    /// not known has nothing to step from.
    #[test]
    fn a_light_off_or_not_known_asks_nothing() {
        for breathe in [Some(10_000), None] {
            assert_eq!(
                relative(Some(&light(false, "warm", 3)), &evening(breathe)),
                Err("the light is off")
            );
            assert_eq!(
                relative(None, &evening(breathe)),
                Err("the light shown is not known")
            );
        }
    }

    #[test]
    fn a_cool_brighter_light_is_asked_one_step_warmer_and_one_dimmer() {
        // Breathing: from the person's level down one step and back.
        let Ok(Call::Light {
            colour,
            level,
            pattern,
            lasting,
            keep,
            until_ends,
            no_brighter: false,
        }) = relative(Some(&light(true, "white", 4)), &evening(Some(10_000)))
        else {
            panic!("a light");
        };
        assert_eq!((colour.as_str(), level), ("warm", Some(4)));
        let pattern = pattern.unwrap();
        assert_eq!(
            (pattern.kind, pattern.period_ms, pattern.low),
            (PatternKind::Breathe, 10_000, 3)
        );
        assert!(pattern.check(4).is_ok());
        assert_eq!(lasting.as_deref(), Some("20s"));
        assert!(!keep && !until_ends);
        // Steady: one step dimmer.
        let Ok(Call::Light {
            colour,
            level,
            pattern,
            ..
        }) = relative(Some(&light(true, "white", 4)), &evening(None))
        else {
            panic!("a light");
        };
        assert_eq!((colour.as_str(), level, pattern), ("warm", Some(3), None));
        // A colour the person chose keeps its colour and is only dimmed.
        let Ok(Call::Light { colour, level, .. }) =
            relative(Some(&light(true, "blue", 3)), &evening(None))
        else {
            panic!("a light");
        };
        assert_eq!((colour.as_str(), level), ("blue", Some(2)));
    }

    /// Glad and Lively at level 1 (#3691, D): with the colour still white,
    /// the warmth alone, steady at 1, never a level up; already warm, or a
    /// colour the person chose, nothing, as the evening.
    #[test]
    fn at_the_dimmest_level_a_feeling_asks_the_warmth_alone_while_the_colour_is_white() {
        for breathe in [Some(6_000), None] {
            assert_eq!(
                relative(Some(&light(true, "white", 1)), &feeling(breathe)),
                Ok(Call::Light {
                    colour: "warm".into(),
                    level: Some(1),
                    lasting: Some("12s".into()),
                    pattern: None,
                    keep: false,
                    until_ends: false,
                    no_brighter: false,
                }),
                "{breathe:?}"
            );
            for colour in ["warm", "orange"] {
                assert_eq!(
                    relative(Some(&light(true, colour, 1)), &feeling(breathe)),
                    Err("the person's light is at its dimmest: no evening glow"),
                    "{colour}"
                );
            }
        }
    }

    /// Above level 1 a feeling's light is the evening's, warmth and all.
    #[test]
    fn above_the_dimmest_level_a_feeling_is_the_evening_glow() {
        for colour in ["white", "warm", "blue"] {
            for level in 2..=LEVELS {
                for breathe in [Some(6_000), None] {
                    let now = light(true, colour, level);
                    let mut asked = relative(Some(&now), &feeling(breathe)).unwrap();
                    let mut evening = relative(Some(&now), &evening(breathe)).unwrap();
                    for call in [&mut asked, &mut evening] {
                        if let Call::Light { lasting, .. } = call {
                            *lasting = None;
                        }
                    }
                    assert_eq!(asked, evening, "{colour} {level} {breathe:?}");
                }
            }
        }
    }

    fn unwell() -> Call {
        Call::Light {
            colour: "white".into(),
            level: Some(2),
            lasting: None,
            pattern: Some(LightPattern {
                kind: PatternKind::Breathe,
                period_ms: 10_000,
                low: 1,
            }),
            keep: false,
            until_ends: true,
            no_brighter: true,
        }
    }

    /// Not feeling well's pale breath (#3691, E) is never brighter than the
    /// person's light: over level 1 it is a steady white 1; at 2 and up it
    /// is as written, white 2 breathing down to 1.
    #[test]
    fn a_no_brighter_light_is_at_most_the_persons_level() {
        for colour in ["warm", "white"] {
            let Ok(Call::Light {
                colour: asked,
                level,
                pattern,
                until_ends,
                no_brighter,
                ..
            }) = relative(Some(&light(true, colour, 1)), &unwell())
            else {
                panic!("a light over {colour} 1");
            };
            assert_eq!((asked.as_str(), level, pattern), ("white", Some(1), None));
            assert!(until_ends && !no_brighter);
            for level in 2..=LEVELS {
                let Ok(Call::Light {
                    level: asked,
                    pattern,
                    ..
                }) = relative(Some(&light(true, colour, level)), &unwell())
                else {
                    panic!("a light over {colour} {level}");
                };
                assert_eq!(asked, Some(2));
                let pattern = pattern.unwrap();
                assert_eq!((pattern.kind, pattern.low), (PatternKind::Breathe, 1));
                assert!(pattern.check(2).is_ok());
            }
        }
        // A brighter ask over a person at 3: their 3, breathing down to 2.
        let mut bright = unwell();
        if let Call::Light { level, pattern, .. } = &mut bright {
            *level = Some(5);
            if let Some(pattern) = pattern {
                pattern.low = 4;
            }
        }
        let Ok(Call::Light { level, pattern, .. }) =
            relative(Some(&light(true, "warm", 3)), &bright)
        else {
            panic!("a light");
        };
        assert_eq!(
            (level, pattern.map(|pattern| pattern.low)),
            (Some(3), Some(2))
        );
    }

    /// Over a light that is off, nothing; not known, as written.
    #[test]
    fn a_no_brighter_light_over_a_light_off_asks_nothing_and_unknown_asks_as_written() {
        assert!(relative(Some(&light(false, "warm", 3)), &unwell()).is_err());
        let Ok(Call::Light {
            level,
            pattern,
            no_brighter,
            ..
        }) = relative(None, &unwell())
        else {
            panic!("a light");
        };
        assert_eq!(
            (level, pattern.map(|pattern| pattern.low)),
            (Some(2), Some(1))
        );
        assert!(!no_brighter);
    }
}
