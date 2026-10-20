//! The ring. At the factory and when asked, each of the ten colours is
//! asked for on `light/test`, which the ring's owner (`lamp-voice`) shows
//! for two seconds at most and never lets touch the person's light, and is
//! seen on `light/drawn`, which the owner says only from its own write
//! receipts. The self-test never writes `light/command`: a request the
//! person makes ends a test at once, and the check then says so.
//!
//! At first boot it only listens to `light/state`, which says what the
//! owner took, not what it drew, so that is at best `not_checked`.
use crate::{Check, Clock, Host};
use lamp_bus::Bus;
use lamp_msgs::{Light, PartResult, Verdict};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Where a test colour is asked for, and where what was drawn is said.
pub const TEST_TOPIC: &str = "light/test";
pub const DRAWN_TOPIC: &str = "light/drawn";
/// The name the self-test asks under.
pub const SOURCE: &str = "selftest";
/// A colour not drawn within this is not coming: the owner looks every
/// 50 ms and draws every 25 ms.
pub const DRAWN_WITHIN: Duration = Duration::from_millis(1_000);
/// How long each colour is held once drawn, so a person at the factory
/// sees it; under the owner's two seconds.
pub const HOLD: Duration = Duration::from_millis(300);
/// The level the colours are asked at: bright enough to tell apart.
pub const LEVEL: u8 = 3;

/// The ring says what it shows once a second; a state older than this is
/// from a ring that stopped.
pub const STATE_FRESH: Duration = Duration::from_secs(3);

/// The light the ring says it shows, and how long ago it said it.
fn shown(bus: &Bus) -> Result<Option<(Light, Duration)>, String> {
    match crate::listen::newest::<Light>(bus, Light::TOPICS[1])? {
        Some(seen) => Ok(Some((seen.value, seen.age))),
        None => Ok(None),
    }
}

/// What the ring's owner says, judged.
pub fn ring(heard: &Result<Option<(Light, Duration)>, String>) -> PartResult {
    let part = "ring";
    match heard {
        Err(why) => PartResult::new(
            part,
            Verdict::Fail,
            &format!("{} cannot be read: {why}", Light::TOPICS[1]),
        ),
        Ok(None) => PartResult::new(
            part,
            Verdict::NotChecked,
            &format!(
                "nothing was ever said on {}: is lamp-voice running?",
                Light::TOPICS[1]
            ),
        ),
        Ok(Some((_, age))) if *age > STATE_FRESH => PartResult::new(
            part,
            Verdict::Fail,
            &format!(
                "the newest {} is {:.1} s old: the ring stopped saying what it shows",
                Light::TOPICS[1],
                age.as_secs_f64()
            ),
        ),
        Ok(Some((light, _))) => PartResult::new(
            part,
            Verdict::NotChecked,
            "listened only: the ring's owner runs, and what it draws cannot be read back",
        )
        .with("colour", &light.colour),
    }
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Whether the ring drew `light` at or after `since_ms`.
fn drawn_since(bus: &Bus, light: &Light, since_ms: u64) -> bool {
    matches!(
        bus.latest(DRAWN_TOPIC),
        Ok(Some(message)) if message.header.at_unix_ms >= since_ms
            && message.parse::<Light>().is_ok_and(|drawn| drawn == *light)
    )
}

/// Whether anyone asked for a light on `light/command` at or after
/// `since_ms`: the person's request ends a test.
fn asked_by_person(bus: &Bus, since_ms: u64) -> bool {
    matches!(
        bus.latest(Light::TOPICS[0]),
        Ok(Some(message)) if message.header.at_unix_ms >= since_ms
    )
}

/// What came of asking for the colours.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shown {
    pub drawn: Vec<&'static str>,
    pub missed: Vec<&'static str>,
    /// The person asked for a light during the check.
    pub interrupted: bool,
    /// Ran out of time before every colour was asked.
    pub out_of_time: bool,
}

/// Judges the colours asked for.
pub fn colours(shown: &Shown) -> PartResult {
    let part = "ring";
    let result = if shown.interrupted {
        PartResult::new(
            part,
            Verdict::NotChecked,
            "a light was asked for during the check, which ends it; it was left as asked",
        )
    } else if shown.drawn.is_empty() && !shown.missed.is_empty() {
        PartResult::new(
            part,
            Verdict::NotChecked,
            "the ring drew none of the colours: it does not draw under the privacy switch or while a turn owns the lamp",
        )
    } else if !shown.missed.is_empty() {
        PartResult::new(
            part,
            Verdict::Fail,
            &format!("not drawn within 1 s: {}", shown.missed.join(", ")),
        )
    } else if shown.out_of_time {
        PartResult::new(
            part,
            Verdict::NotChecked,
            &format!(
                "ran out of time after {} of {} colours",
                shown.drawn.len(),
                Light::COLOURS.len()
            ),
        )
    } else {
        PartResult::new(part, Verdict::Pass, "")
    };
    result.with("drawn", shown.drawn.len())
}

/// Asks for each colour on `light/test` and watches `light/drawn`.
pub fn ask_colours(bus: &Bus, deadline: Instant) -> Result<Shown, String> {
    ask_colours_on(bus, deadline, &mut Host)
}

fn ask_colours_on(bus: &Bus, deadline: Instant, clock: &mut impl Clock) -> Result<Shown, String> {
    let mut asks = bus
        .publisher_declared(TEST_TOPIC, &Light::declaration(), SOURCE)
        .map_err(|e| e.to_string())?;
    let began_ms = unix_ms();
    let mut shown = Shown::default();
    for (n, colour) in Light::COLOURS.into_iter().enumerate() {
        if clock.now() + DRAWN_WITHIN > deadline {
            shown.out_of_time = true;
            break;
        }
        let light = Light {
            on: true,
            colour: colour.into(),
            level: LEVEL,
        };
        // The person's request is looked for right before each colour, so
        // that no test is asked after it.
        if asked_by_person(bus, began_ms) {
            shown.interrupted = true;
            return Ok(shown);
        }
        let asked_ms = unix_ms();
        asks.publish_json(n as u64 + 1, &light)
            .map_err(|e| e.to_string())?;
        let until = clock.now() + DRAWN_WITHIN;
        let mut drawn = false;
        while clock.now() < until {
            if asked_by_person(bus, began_ms) {
                shown.interrupted = true;
                return Ok(shown);
            }
            if drawn_since(bus, &light, asked_ms) {
                drawn = true;
                break;
            }
            clock.pause(Duration::from_millis(20));
        }
        if drawn {
            shown.drawn.push(colour);
            // Held for the person at the factory to see, watching all the
            // while for their own request.
            let until = clock.now() + HOLD;
            while clock.now() < until {
                if asked_by_person(bus, began_ms) {
                    shown.interrupted = true;
                    return Ok(shown);
                }
                clock.pause(Duration::from_millis(20));
            }
        } else {
            shown.missed.push(colour);
        }
    }
    shown.interrupted = asked_by_person(bus, began_ms);
    Ok(shown)
}

pub struct Ring<'a> {
    pub bus: &'a Bus,
    /// Whether to ask for the colours (the factory, on demand) or only
    /// listen (first boot).
    pub active: bool,
}

impl Check for Ring<'_> {
    fn part(&self) -> &str {
        "ring"
    }

    fn run(&mut self, deadline: Instant) -> PartResult {
        self.run_on(deadline, &mut Host)
    }
}

impl Ring<'_> {
    fn run_on(&mut self, deadline: Instant, clock: &mut impl Clock) -> PartResult {
        if !self.active {
            return ring(&shown(self.bus));
        }
        match shown(self.bus) {
            Ok(Some((_, age))) if age <= STATE_FRESH => {}
            heard => return ring(&heard),
        }
        match ask_colours_on(self.bus, deadline, clock) {
            Ok(shown) => colours(&shown),
            Err(why) => PartResult::new(
                "ring",
                Verdict::NotChecked,
                &format!("{TEST_TOPIC} cannot be asked: {why}"),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn light(colour: &str) -> Light {
        Light {
            on: true,
            colour: colour.into(),
            level: 2,
        }
    }

    #[test]
    fn a_ring_that_says_what_it_shows_is_listened_to_never_passed() {
        let heard = ring(&Ok(Some((light("blue"), Duration::from_millis(400)))));
        assert_eq!(heard.verdict, Verdict::NotChecked);
        assert!(
            heard.reason.starts_with("listened only"),
            "{}",
            heard.reason
        );
        assert_eq!(ring(&Ok(None)).verdict, Verdict::NotChecked);
        let stopped = ring(&Ok(Some((light("blue"), Duration::from_secs(9)))));
        assert_eq!(stopped.verdict, Verdict::Fail);
        assert_eq!(ring(&Err("gone".into())).verdict, Verdict::Fail);
    }

    #[test]
    fn at_first_boot_the_check_asks_nothing_of_the_ring() {
        let root = std::path::PathBuf::from(format!("/tmp/ls-ring-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut owner = bus
            .publisher_declared(Light::TOPICS[1], &Light::declaration(), "ring")
            .unwrap();
        owner.publish_json(1, &light("warm")).unwrap();
        let result = Ring {
            bus: &bus,
            active: false,
        }
        .run(Instant::now() + Duration::from_secs(5));
        assert_eq!(result.verdict, Verdict::NotChecked, "{result:?}");
        assert!(
            bus.latest(Light::TOPICS[0]).unwrap().is_none(),
            "nothing was asked on light/command"
        );
        drop(owner);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// A ring owner as lamp-voice's: shows a test colour unless it is
    /// `refuses` or it refuses all (the privacy switch), says what it drew,
    /// and says the person's light on `light/state`. `handled` is the last
    /// colour asked that it has drawn or refused.
    fn owner(
        root: &std::path::Path,
        refuses: Option<&'static str>,
        draws: bool,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
        handled: std::sync::Arc<std::sync::atomic::AtomicU64>,
    ) -> std::thread::JoinHandle<()> {
        let bus = Bus::new(root).unwrap();
        let mut tests = bus
            .subscribe_typed(TEST_TOPIC, "ring", Light::TYPE)
            .unwrap();
        let mut state = bus
            .publisher_declared(Light::TOPICS[1], &Light::declaration(), "ring")
            .unwrap();
        let mut drawn = bus
            .publisher_declared(DRAWN_TOPIC, &Light::declaration(), "ring")
            .unwrap();
        state.publish_json(0, &light("warm")).unwrap();
        std::thread::spawn(move || {
            let mut clock = 0;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                clock += 1;
                state.publish_json(clock, &light("warm")).unwrap();
                if let Ok(Some(message)) = tests.recv(Duration::from_millis(20)) {
                    if let Ok(test) = message.parse::<Light>()
                        && draws
                        && Some(test.colour.as_str()) != refuses
                    {
                        drawn.publish_json(clock, &test).unwrap();
                    }
                    handled.store(message.header.seq, std::sync::atomic::Ordering::Release);
                }
            }
        })
    }

    /// The check's clock in a test: it stands still until the check
    /// pauses, and then moves on only once the stand-in ring owner has
    /// drawn or refused every colour asked so far. So a colour is never
    /// missed for want of the owner's thread, and the 1 s for a colour that
    /// is refused, and each 300 ms hold, take no real time.
    struct Stepped<'a> {
        at: Instant,
        bus: &'a Bus,
        handled: std::sync::Arc<std::sync::atomic::AtomicU64>,
    }

    impl Clock for Stepped<'_> {
        fn now(&self) -> Instant {
            self.at
        }
        fn pause(&mut self, pause: Duration) {
            let asked = self
                .bus
                .latest(TEST_TOPIC)
                .unwrap()
                .map_or(0, |message| message.header.seq);
            let until = Instant::now() + Duration::from_secs(5);
            while self.handled.load(std::sync::atomic::Ordering::Acquire) < asked {
                assert!(
                    Instant::now() < until,
                    "the ring owner never took colour {asked}"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            self.at += pause;
        }
    }

    /// The check against a stand-in ring owner, on the stepped clock or,
    /// with a person who asks for a light after `person_at`, on the host's.
    fn run_with(
        name: &str,
        refuses: Option<&'static str>,
        draws: bool,
        person_at: Option<Duration>,
    ) -> (PartResult, Bus, std::path::PathBuf) {
        let root = std::path::PathBuf::from(format!("/tmp/ls-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let handled = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let ring_owner = owner(&root, refuses, draws, stop.clone(), handled.clone());
        let person = person_at.map(|after| {
            let bus = Bus::new(&root).unwrap();
            std::thread::spawn(move || {
                let mut asks = bus
                    .publisher_declared(Light::TOPICS[0], &Light::declaration(), "actions")
                    .unwrap();
                std::thread::sleep(after);
                asks.publish_json(1, &light("blue")).unwrap();
                asks
            })
        });
        let mut ring = Ring {
            bus: &bus,
            active: true,
        };
        let result = if person.is_some() {
            // The person's request comes after a real time: the host's
            // clock.
            ring.run(Instant::now() + Duration::from_secs(60))
        } else {
            let mut clock = Stepped {
                at: Instant::now(),
                bus: &bus,
                handled,
            };
            let deadline = clock.at + Duration::from_secs(60);
            ring.run_on(deadline, &mut clock)
        };
        let _asks = person.map(|p| p.join().unwrap());
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        ring_owner.join().unwrap();
        (result, bus, root)
    }

    #[test]
    fn every_colour_drawn_passes_and_the_persons_light_is_never_written() {
        let (result, bus, root) = run_with("ring-all", None, true, None);
        assert_eq!(result.verdict, Verdict::Pass, "{result:?}");
        assert!(
            result
                .values
                .iter()
                .any(|v| v.key == "drawn" && v.value == "10")
        );
        assert!(
            bus.latest(Light::TOPICS[0]).unwrap().is_none(),
            "no light/command"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_colour_never_drawn_fails_naming_it() {
        let (result, _, root) = run_with("ring-red", Some("red"), true, None);
        assert_eq!(result.verdict, Verdict::Fail, "{result:?}");
        assert_eq!(result.reason, "not drawn within 1 s: red");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_ring_that_draws_nothing_is_not_checked_not_failed() {
        let (result, _, root) = run_with("ring-none", None, false, None);
        assert_eq!(result.verdict, Verdict::NotChecked, "{result:?}");
        assert!(result.reason.contains("privacy"), "{}", result.reason);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_light_the_person_asks_for_ends_the_check_and_stands() {
        let (result, bus, root) =
            run_with("ring-person", None, true, Some(Duration::from_millis(500)));
        assert_eq!(result.verdict, Verdict::NotChecked, "{result:?}");
        assert!(result.reason.contains("left as asked"), "{}", result.reason);
        let asked: Light = bus
            .latest(Light::TOPICS[0])
            .unwrap()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(asked, light("blue"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_light_asked_during_a_hold_stops_the_check_before_another_colour_is_asked() {
        let root = std::path::PathBuf::from(format!("/tmp/ls-ring-hold-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut seen_tests = bus
            .subscribe_typed(TEST_TOPIC, "witness", Light::TYPE)
            .unwrap();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let handled = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let ring_owner = owner(&root, None, true, stop.clone(), handled);
        // The person asks once the first colour is drawn, inside its hold.
        let person_bus = Bus::new(&root).unwrap();
        let person = std::thread::spawn(move || {
            let mut asks = person_bus
                .publisher_declared(Light::TOPICS[0], &Light::declaration(), "actions")
                .unwrap();
            while !matches!(person_bus.latest(DRAWN_TOPIC), Ok(Some(_))) {
                std::thread::sleep(Duration::from_millis(5));
            }
            std::thread::sleep(Duration::from_millis(100));
            asks.publish_json(1, &light("blue")).unwrap();
            asks
        });
        let result = Ring {
            bus: &bus,
            active: true,
        }
        .run(Instant::now() + Duration::from_secs(60));
        let _asks = person.join().unwrap();
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        ring_owner.join().unwrap();
        assert_eq!(result.verdict, Verdict::NotChecked, "{result:?}");
        let asked_ms = bus
            .latest(Light::TOPICS[0])
            .unwrap()
            .unwrap()
            .header
            .at_unix_ms;
        let mut tests = 0;
        while let Some(message) = seen_tests.recv(Duration::from_millis(50)).unwrap() {
            tests += 1;
            assert!(
                message.header.at_unix_ms <= asked_ms,
                "a test colour was asked after the person's light"
            );
        }
        assert_eq!(tests, 1, "only the first colour was asked");
        std::fs::remove_dir_all(root).unwrap();
    }
}
