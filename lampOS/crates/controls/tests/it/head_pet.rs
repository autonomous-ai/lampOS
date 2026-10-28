//! A pet on the head (#3997, Part of #4133): the pads taking turns alone,
//! said on `controls/pet` with `LAMP_HEAD_PET` on, and nothing else changed.
//! The timeline is lamp-06c1's (28 Oct, 18:4x, read-only): `head-3` and
//! `head-4`, its two head pads, as `controls/touch_state` said them, each
//! level held until the next row.
use lamp_controls::{input::Level, touch::*};
use lamp_msgs::{ClockStamp, HeadPet, TouchGesture};

/// (seconds, mask): bit 2 is `head-3`, bit 3 is `head-4`, as on lamp-06c1.
const TIMELINE: &[(f64, u16)] = &[
    (6.64, 0b1000),
    (6.79, 0),
    (8.45, 0b1000),
    (8.58, 0),
    (10.42, 0b1000),
    (10.56, 0),
    // The pet, left and right for about 3 s.
    (18.26, 0b0100),
    (18.36, 0b1100),
    (18.38, 0b1000),
    (18.54, 0b1100),
    (18.66, 0b1000),
    (19.05, 0b1100),
    (19.17, 0b1000),
    (19.33, 0),
    (19.35, 0b0100),
    (19.44, 0b1100),
    (19.45, 0b1000),
    (19.59, 0),
    (19.62, 0b0100),
    (19.74, 0),
    (19.75, 0b1000),
    (19.89, 0b1100),
    (19.90, 0b0100),
    (20.03, 0),
    (20.04, 0b1000),
    (20.19, 0b0100),
    (20.32, 0b1000),
    (20.48, 0b0100),
    (20.59, 0b1100),
    (20.60, 0b1000),
    (20.74, 0),
    (20.75, 0b0100),
    (20.86, 0b1100),
    (20.88, 0b1000),
    (21.03, 0b0100),
    (21.16, 0),
    (21.17, 0b1000),
    (21.32, 0b0100),
    (21.44, 0),
    // A pat with both pads.
    (30.53, 0b1100),
    (30.65, 0b1000),
    (30.68, 0),
    // A still hand, about 3 s.
    (39.81, 0b1000),
    (39.88, 0b1100),
    (42.73, 0b1000),
    (42.80, 0),
    // A still hand, about 5 s.
    (48.41, 0b1000),
    (48.76, 0b1100),
    (53.50, 0b1000),
    (53.55, 0),
];

/// Where the rig's clock puts the timeline's `from`: after the head settled.
const START_US: u64 = 510_000;

#[derive(Default)]
struct Seen {
    /// Each gesture, by how long after the contact began.
    gestures: Vec<(TouchGesture, u64)>,
    pets: Vec<HeadPet>,
    /// The head's `held_us` at each acquisition, by when.
    held: Vec<(u64, Option<u64>)>,
}

struct Rig {
    engine: TouchEngine,
    sequence: u64,
}

impl Rig {
    fn new(pet: Option<HoldSleeps>) -> Self {
        let config = TouchConfig {
            enabled: true,
            head_pads: (1..=4).map(|pad| format!("head{pad}")).collect(),
            ..TouchConfig::default()
        };
        let mut rig = Self {
            engine: TouchEngine::new(config, "boot", "controls").unwrap(),
            sequence: 0,
        };
        rig.engine.set_head_pet(pet);
        for at in (0..=500_000).step_by(50_000) {
            let seen = rig.gpio(at, 0);
            assert!(seen.0.events.is_empty());
        }
        rig
    }
    fn gpio(&mut self, at: u64, mask: u16) -> (TouchOutput, Option<u64>) {
        self.sequence += 1;
        let sample = GpioSample {
            stamp: ClockStamp {
                clock_id: "boot".into(),
                at_us: at,
                sequence: self.sequence,
            },
            session_id: "gpio".into(),
            primary: None,
            reset: None,
            head_pads: (0..4)
                .map(|pad| Level {
                    name: format!("head{}", pad + 1),
                    active: mask & (1 << pad) != 0,
                })
                .collect(),
        };
        self.engine.ingest_gpio(at, sample).unwrap();
        let output = self.engine.tick(at);
        output.state.check().unwrap();
        for event in &output.events {
            event.check().unwrap();
        }
        for pet in &output.pets {
            pet.check().unwrap();
            assert!(pet.fresh_at("boot", at));
        }
        let held = output.state.sources[0].held_us;
        (output, held)
    }
    /// Levels every 10 ms, each held until the next, from `rows` with
    /// `from` at [`START_US`], up to `to`.
    fn play(&mut self, rows: &[(u64, u16)], to: u64) -> Seen {
        let mut seen = Seen::default();
        let mut began = None;
        for at in (START_US..to).step_by(10_000) {
            let mask = rows
                .iter()
                .take_while(|(row, _)| *row <= at)
                .last()
                .map_or(0, |(_, mask)| *mask);
            let (output, held) = self.gpio(at, mask);
            for event in output.events {
                if event.gesture == TouchGesture::Touch {
                    began = Some(event.began_us);
                }
                seen.gestures
                    .push((event.gesture, event.observed_us - event.began_us));
            }
            seen.pets.extend(output.pets);
            if began.is_some() {
                seen.held.push((at, held));
            }
        }
        seen
    }
    /// The timeline from `from` up to `through` seconds, then 300 ms let go.
    fn timeline(&mut self, from: f64, through: f64) -> Seen {
        let us = |seconds: f64| START_US + ((seconds - from) * 1e6).round() as u64;
        let rows: Vec<(u64, u16)> = TIMELINE
            .iter()
            .filter(|(at, _)| (from..=through).contains(at))
            .map(|(at, mask)| (us(*at), *mask))
            .collect();
        self.play(&rows, us(through) + 300_000)
    }
}

fn gestures(seen: &Seen) -> Vec<TouchGesture> {
    seen.gestures.iter().map(|(gesture, _)| *gesture).collect()
}

/// The measured pet: one pet told apart about 1.2 s in, after the Stroke at
/// 1 s, which stays; no Hold, and no `held_us` from the pet on, so neither
/// the ring's countdown nor the sleep breath plays; then said at most ten
/// times a second and ended when let go.
#[test]
fn a_pet_from_the_measured_timeline_is_one_pet_and_no_sleep() {
    let mut rig = Rig::new(Some(HoldSleeps::PetOnly));
    let seen = rig.timeline(18.26, 21.44);
    assert_eq!(
        gestures(&seen),
        [TouchGesture::Touch, TouchGesture::Stroke],
        "{:?}",
        seen.gestures
    );
    assert!((1_000_000..1_020_000).contains(&seen.gestures[1].1));
    let told: Vec<_> = seen
        .pets
        .iter()
        .filter(|pet| pet.petted_us == pet.observed_us)
        .collect();
    assert_eq!(told.len(), 1);
    let first = told[0];
    assert_eq!(first, &seen.pets[0]);
    assert_eq!(first.swaps, 3);
    assert!(!first.resting && !first.ended);
    let after = first.petted_us - first.began_us;
    assert!((1_150_000..=1_300_000).contains(&after), "{after}");
    // One contact, said again while it lasts, at most every 100 ms.
    assert!(
        seen.pets
            .iter()
            .all(|pet| pet.contact_id == first.contact_id)
    );
    for pair in seen.pets.windows(2) {
        assert!(pair[1].stamp.sequence > pair[0].stamp.sequence);
        assert!(pair[1].ended || pair[1].observed_us - pair[0].observed_us >= PET_EVERY_US);
    }
    assert!(seen.pets.len() >= 15, "{}", seen.pets.len());
    let last = seen.pets.last().unwrap();
    assert!(last.ended && last.resting);
    assert_eq!(seen.pets.iter().filter(|pet| pet.ended).count(), 1);
    assert!(last.swaps > first.swaps);
    assert!(
        (1.0..=6.0).contains(&last.swaps_per_second),
        "{}",
        last.swaps_per_second
    );
    // The hand counted until the pet, and not from it on.
    for (at, held) in &seen.held {
        if *at < first.petted_us {
            assert!(held.is_some() || at - first.began_us < 30_000, "{at}");
        } else {
            assert_eq!(*held, None, "{at}");
        }
    }
}

/// The same pet with the switch off: today's Stroke at 1 s and Hold (sleep)
/// at 3 s, `held_us` counting all through, and no pet.
#[test]
fn the_same_pet_with_the_switch_off_is_todays_stroke_and_hold() {
    let mut rig = Rig::new(None);
    let seen = rig.timeline(18.26, 21.44);
    assert_eq!(
        gestures(&seen),
        [
            TouchGesture::Touch,
            TouchGesture::Stroke,
            TouchGesture::Hold
        ]
    );
    assert!((3_000_000..3_020_000).contains(&seen.gestures[2].1));
    assert!(seen.pets.is_empty());
    let counted = seen.held.iter().filter(|(_, held)| held.is_some()).count();
    assert!(counted >= 310, "{counted}");
}

/// Everything on the timeline, with the switch off, is said byte for byte
/// as by an engine that never heard of a pet; with it on, only the pet
/// differs.
#[test]
fn the_switch_off_is_identical_to_today() {
    let rows = |from: f64| -> Vec<(u64, u16)> {
        TIMELINE
            .iter()
            .map(|(at, mask)| (START_US + ((at - from) * 1e6).round() as u64, *mask))
            .collect()
    };
    let to = START_US + 50_000_000;
    let today = {
        let config = TouchConfig {
            enabled: true,
            head_pads: (1..=4).map(|pad| format!("head{pad}")).collect(),
            ..TouchConfig::default()
        };
        let mut rig = Rig {
            engine: TouchEngine::new(config, "boot", "controls").unwrap(),
            sequence: 0,
        };
        for at in (0..=500_000).step_by(50_000) {
            rig.gpio(at, 0);
        }
        rig
    };
    let mut frames = Vec::new();
    for mut rig in [today, Rig::new(None)] {
        let rows = rows(5.0);
        let mut said = Vec::new();
        for at in (START_US..to).step_by(10_000) {
            let mask = rows
                .iter()
                .take_while(|(row, _)| *row <= at)
                .last()
                .map_or(0, |(_, mask)| *mask);
            let (output, _) = rig.gpio(at, mask);
            said.push(serde_json::to_string(&output).unwrap());
        }
        frames.push(said);
    }
    assert_eq!(frames[0], frames[1]);
    assert!(frames[1].iter().all(|frame| !frame.contains("\"pets\"")));
}

/// lamp-06c1's still hands, both pads together: about 3 s is a Stroke and
/// let go before sleep, about 5 s a Stroke and then sleep at 3 s, as today,
/// with the switch on; never a pet, and `held_us` counting all through.
#[test]
fn a_still_hand_still_strokes_and_sleeps() {
    for (from, through, kept) in [
        (
            39.81,
            42.80,
            vec![TouchGesture::Touch, TouchGesture::Stroke],
        ),
        (
            48.41,
            53.55,
            vec![
                TouchGesture::Touch,
                TouchGesture::Stroke,
                TouchGesture::Hold,
            ],
        ),
    ] {
        let on = Rig::new(Some(HoldSleeps::PetOnly)).timeline(from, through);
        let off = Rig::new(None).timeline(from, through);
        assert_eq!(gestures(&on), kept, "{from}");
        assert_eq!(on.gestures, off.gestures, "{from}");
        assert!(on.pets.is_empty(), "{from}");
        assert_eq!(on.held, off.held, "{from}");
    }
}

/// lamp-06c1's taps, on one pad for 130-150 ms and on both for 120 ms: each a
/// Touch and a Tap, and no pet.
#[test]
fn a_tap_and_a_pat_are_taps() {
    for (from, through) in [(6.64, 6.79), (8.45, 8.58), (10.42, 10.56), (30.53, 30.68)] {
        let seen = Rig::new(Some(HoldSleeps::PetOnly)).timeline(from, through);
        assert_eq!(
            gestures(&seen),
            [TouchGesture::Touch, TouchGesture::Tap],
            "{from}"
        );
        assert!(seen.pets.is_empty(), "{from}");
    }
}

/// Sides taking turns every `side_us`, from [`START_US`] to `for_us` after.
fn turns(side_us: u64, for_us: u64) -> Vec<(u64, u16)> {
    (0..for_us / side_us)
        .map(|turn| {
            let mask = if turn % 2 == 0 { 0b0100 } else { 0b1000 };
            (START_US + turn * side_us, mask)
        })
        .chain([(START_US + for_us, 0)])
        .collect()
}

/// A quick pet let go under a second: a pet, and never a Tap (which would ask
/// the lamp to listen).
#[test]
fn a_quick_pet_let_go_under_a_second_is_no_tap() {
    let mut rig = Rig::new(Some(HoldSleeps::PetOnly));
    let seen = rig.play(&turns(100_000, 900_000), START_US + 1_300_000);
    assert_eq!(gestures(&seen), [TouchGesture::Touch]);
    assert_eq!(seen.pets[0].swaps, 3);
    assert!(seen.pets.last().unwrap().ended);
    // Off, the same hand is today's Tap.
    let seen = Rig::new(None).play(&turns(100_000, 900_000), START_US + 1_300_000);
    assert_eq!(gestures(&seen), [TouchGesture::Touch, TouchGesture::Tap]);
}

/// A still hand on both pads that shifts, one pad alone for 20 ms now on
/// one side and now the other: no swap counts, so no pet, and the Hold at
/// 3 s stays.
#[test]
fn single_pad_slivers_under_40ms_are_no_swap() {
    let mut rows = Vec::new();
    for turn in 0..20u64 {
        let at = START_US + turn * 200_000;
        let alone = if turn % 2 == 0 { 0b0100 } else { 0b1000 };
        rows.push((at, alone));
        rows.push((at + 20_000, 0b1100));
    }
    rows.push((START_US + 4_000_000, 0));
    let seen = Rig::new(Some(HoldSleeps::PetOnly)).play(&rows, START_US + 4_300_000);
    assert_eq!(
        gestures(&seen),
        [
            TouchGesture::Touch,
            TouchGesture::Stroke,
            TouchGesture::Hold
        ]
    );
    assert!(seen.pets.is_empty());
}

/// A head the body feels pushed asks nothing, a pet neither.
#[test]
fn a_pushed_head_is_never_a_pet() {
    let mut rig = Rig::new(Some(HoldSleeps::PetOnly));
    rig.engine.set_head_pushed(true);
    let seen = rig.play(&turns(150_000, 3_000_000), START_US + 3_300_000);
    assert!(seen.gestures.is_empty());
    assert!(seen.pets.is_empty());
}

/// #4133's choice, one constant away: with `Never`, no head hand asks sleep,
/// a still one keeps its `held_us` (the lean stays), and a pet is still a
/// pet. The constant itself is `PetOnly` until its owners say yes.
#[test]
fn hold_sleeps_never_drops_every_heads_sleep() {
    assert_eq!(HEAD_PET_HOLD, HoldSleeps::PetOnly);
    let still = Rig::new(Some(HoldSleeps::Never)).timeline(48.41, 53.55);
    assert_eq!(
        gestures(&still),
        [TouchGesture::Touch, TouchGesture::Stroke]
    );
    assert!(still.pets.is_empty());
    let off = Rig::new(None).timeline(48.41, 53.55);
    assert_eq!(still.held, off.held);
    let pet = Rig::new(Some(HoldSleeps::Never)).timeline(18.26, 21.44);
    assert_eq!(gestures(&pet), [TouchGesture::Touch, TouchGesture::Stroke]);
    assert_eq!(
        pet.pets
            .iter()
            .filter(|pet| pet.petted_us == pet.observed_us)
            .count(),
        1
    );
}

/// The pet's intensity: livelier with faster turns, and resting once the
/// hand stays still after them.
#[test]
fn a_pets_intensity_follows_the_hand() {
    let mut rows = turns(100_000, 1_000_000);
    rows.pop();
    rows.push((START_US + 1_000_000, 0b1100));
    rows.push((START_US + 2_000_000, 0));
    let seen = Rig::new(Some(HoldSleeps::PetOnly)).play(&rows, START_US + 2_300_000);
    let moving = seen.pets.iter().find(|pet| !pet.resting).unwrap();
    let resting = seen
        .pets
        .iter()
        .find(|pet| pet.resting && !pet.ended)
        .unwrap();
    assert!(resting.observed_us >= START_US + 1_000_000 + PET_RESTING_US - 100_000);
    assert!(resting.swaps_per_second < moving.swaps_per_second.max(9.0));
    let slow =
        Rig::new(Some(HoldSleeps::PetOnly)).play(&turns(250_000, 2_000_000), START_US + 2_300_000);
    let fast =
        Rig::new(Some(HoldSleeps::PetOnly)).play(&turns(100_000, 2_000_000), START_US + 2_300_000);
    let rate = |seen: &Seen| seen.pets.last().unwrap().swaps_per_second;
    assert!(
        rate(&fast) > rate(&slow) * 1.5,
        "{} {}",
        rate(&fast),
        rate(&slow)
    );
}

/// `LAMP_HEAD_PET` is on only when it says exactly `on`.
#[test]
fn the_switch_is_on_only_when_it_says_on() {
    assert_eq!(head_pet(Some("on")), Some(HEAD_PET_HOLD));
    for value in [
        None,
        Some(""),
        Some("off"),
        Some("ON"),
        Some("1"),
        Some("detect"),
    ] {
        assert_eq!(head_pet(value), None, "{value:?}");
    }
    assert_eq!(HEAD_PET_ENV, "LAMP_HEAD_PET");
}
