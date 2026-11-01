//! Whether a request was said to the lamp from close to it, without its
//! name (#753, `docs/attention.md`, "Said to the lamp from close to it").
//!
//! Two signs the provider worker already reads, taken over the request's
//! own sound rather than at the moment its answer begins:
//!
//! - **Near.** The room microphone's highest rise over the room's quiet
//!   while the voice microphone heard the request (`ears/level`), at
//!   [`crate::attention::NEAR_RISE_DB`] or more. On lamp-4ace on
//!   2026-10-14 lines said to the lamp from the desk rose 19.5 to 26.5 dB,
//!   people farther off 9.8 to 15.1 dB (`docs/ears.md`).
//! - **A close face turned to the lamp.** Of the eyes' looks taken while
//!   the person spoke or in the [`LEAD_US`] before (`humans/faces/tracked`),
//!   the share that saw a face judged turned to the lamp whose box is at
//!   least [`CLOSE_FACE_HEIGHT`] of the picture's height, at
//!   [`FACING_SHARE`] or more of at least [`LEAST_LOOKS`] looks. V1's own
//!   gaze rule, measured on its lamps: a share and not one look, because
//!   one look's judgement of a head's turn is noisy; from just before the
//!   speech, because people turn before they speak (`hal/config.py`,
//!   `GAZE_*`). Facing alone is a weak sign: in the one robot study with
//!   numbers, looking at the robot went with speaking to it 65% of the time
//!   (`docs/said-to-whom.md`). So is a loud voice alone: someone at the desk
//!   may be talking to someone else. Together they say more than either.
//!
//! The voice microphone's own level is not used: its processing evens out
//! its level, and the two microphones' streams have clocks of their own.
//!
//! What is judged is published once for each request on
//! `attention/addressed` ([`AddressedOut`]), and in a busy room it lets an
//! answer out ([`crate::attention::Attention::set_addressed`]). Off unless
//! the voice's environment says `LAMP_ADDRESSED=on`, and only with
//! `LAMP_ATTENTION` set, which follows the requests.
use lamp_bus::{Bus, Message, Publisher};
use lamp_msgs::{Addressed, Faces};
use std::{collections::VecDeque, io, path::Path};

/// Where the coordinator reads the switch. `on` only.
pub const ENVIRONMENT: &str = "LAMP_ADDRESSED";
/// The provider worker's flag for it, after the attention flags.
pub const FLAG: &str = "--addressed";
/// A face this tall, as a share of the picture's height, is someone at the
/// lamp (`lamp-eyes`, `sight::LEAST_SIZE`; the Policy's `NEAR_HEIGHT`). V1
/// measured it on 2026-09-30: office colleagues 8.3% to 13.6%, the person
/// at the desk 19.2% to 46% (`GAZE_BEARING_MIN_FACE_HEIGHT_FRAC`).
pub const CLOSE_FACE_HEIGHT: f32 = 0.15;
/// Looks this long before the request's sound began count: people turn
/// before they speak (V1's `GAZE_WINDOW_S`).
pub const LEAD_US: u64 = 1_500_000;
/// Facing: at least this share of the looks saw a close face turned to the
/// lamp. V1's `GAZE_BEARING_MIN_FACING_RATIO`: a still person centred in
/// view measured 50% against its first line of 60%, and under 0.4 one
/// glance among two or three looks would be enough.
pub const FACING_SHARE: f32 = 0.4;
/// Fewer looks than this say nothing of facing (V1's `GAZE_MIN_SAMPLES`).
pub const LEAST_LOOKS: usize = 2;
/// The looks kept, at most this long.
const KEPT_US: u64 = 30_000_000;
/// The looks kept, at most this many: four a second for 30 s, and more.
const MOST_LOOKS: usize = 200;

/// The provider worker's argument for the switch in the environment.
pub fn arguments_from_environment() -> Vec<String> {
    arguments(std::env::var(ENVIRONMENT).ok().as_deref())
}

fn arguments(value: Option<&str>) -> Vec<String> {
    match value {
        Some("on") => vec![FLAG.to_owned()],
        _ => Vec::new(),
    }
}

/// Take a leading `--addressed` off a worker's flags, after the attention
/// flags.
pub fn take(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// The tallest face turned to the lamp in one look, as a share of the
/// picture's height; 0 when none in it is.
pub fn tallest_turned(faces: &Faces) -> f32 {
    faces
        .faces
        .iter()
        .filter(|face| face.facing == Some(true))
        .map(|face| (face.ymax - face.ymin).clamp(0.0, 1.0))
        .fold(0.0, f32::max)
}

/// The eyes' looks of the last 30 s: when each was taken, by the lamp's
/// monotonic clock, and its tallest face turned to the lamp.
#[derive(Default)]
pub struct Looks {
    looks: VecDeque<(u64, f32)>,
}

impl Looks {
    /// The eyes' newest message, as the bus holds it: kept if it is still
    /// fresh and newer than the last one kept.
    pub fn read(&mut self, message: &Message, now_us: u64) {
        if message.expired {
            return;
        }
        let Ok(faces) = message.parse::<Faces>() else {
            return;
        };
        self.look(message.header.at_us, &faces, now_us);
    }

    /// One look, taken at `at_us`.
    pub fn look(&mut self, at_us: u64, faces: &Faces, now_us: u64) {
        if self.looks.back().is_none_or(|(last, _)| at_us > *last) {
            self.looks.push_back((at_us, tallest_turned(faces)));
        }
        while self.looks.len() > MOST_LOOKS
            || self
                .looks
                .front()
                .is_some_and(|(at, _)| now_us.saturating_sub(*at) > KEPT_US)
        {
            self.looks.pop_front();
        }
    }

    /// The looks taken from [`LEAD_US`] before `began_us` until `until_us`:
    /// how many, the tallest face turned to the lamp in them, and the share
    /// that saw a close one.
    pub fn between(&self, began_us: u64, until_us: u64) -> Seen {
        let from = began_us.saturating_sub(LEAD_US);
        let heights: Vec<f32> = self
            .looks
            .iter()
            .filter(|(at, _)| (from..=until_us).contains(at))
            .map(|(_, height)| *height)
            .collect();
        let close = heights
            .iter()
            .filter(|height| **height >= CLOSE_FACE_HEIGHT)
            .count();
        Seen {
            looks: heights.len(),
            tallest: heights.iter().copied().reduce(f32::max),
            share: (heights.len() >= LEAST_LOOKS).then(|| close as f32 / heights.len() as f32),
        }
    }
}

/// What the eyes saw over one request.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seen {
    pub looks: usize,
    pub tallest: Option<f32>,
    pub share: Option<f32>,
}

/// The request in hand, as the worker follows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Request {
    pub session: u64,
    pub request: u64,
    pub began_us: u64,
    pub ended_us: Option<u64>,
    /// The surest the lamp's name was heard for it, 0 when not.
    pub name_score: f32,
}

/// Whether `said` is the judgement of `request` already.
pub fn judged(said: Option<&Addressed>, request: &Request) -> bool {
    said.is_some_and(|said| said.session == request.session && said.request == request.request)
}

/// The signs for `request`, over its sound until it ended or, if it has
/// not, until `now_us`. `rises` are the room microphone's rises over its
/// quiet, each with when it was read.
pub fn judge(
    request: Request,
    rises: &VecDeque<(u64, f32)>,
    looks: &Looks,
    now_us: u64,
) -> Addressed {
    let until = request.ended_us.unwrap_or(now_us).max(request.began_us);
    let rise_db = rises
        .iter()
        .filter(|(at, _)| (request.began_us..=until).contains(at))
        .map(|(_, rise)| *rise)
        .filter(|rise| rise.is_finite())
        .reduce(f32::max)
        .map(|rise| rise.clamp(-120.0, 120.0));
    let bar = crate::attention::near_rise_db();
    let near = rise_db.map(|rise| rise >= bar);
    let seen = looks.between(request.began_us, until);
    let facing_close = seen.share.map(|share| share >= FACING_SHARE);
    Addressed {
        session: request.session,
        request: request.request,
        began_us: request.began_us,
        ended_us: request.ended_us,
        rise_db,
        near,
        looks: u16::try_from(seen.looks).unwrap_or(u16::MAX),
        facing_height: seen.tallest,
        facing_share: seen.share,
        facing_close,
        name_score: request.name_score.clamp(0.0, 1.0),
        addressed: near == Some(true) && facing_close == Some(true),
        like_asker: None,
    }
}

/// How alike the line `request` sounds to the person the lamp is in
/// conversation with, for the judgement's `like_asker` (#4306): the cosine
/// of its voice print against the print of the line that began the
/// conversation ([`crate::same_voice::Voices::like_asker`]). Nothing without
/// the same-voice check (`LAMP_SAME_VOICE_MODEL`), while no conversation is
/// going on (`going_on`: the thread's `going_on`, an exchange with the
/// lamp's words in it within [`crate::thread::GOING_ON_FOR`]), or while
/// either print is not known: a
/// line under 1.5 s has none, and one whose print is still being made when
/// it is judged has none yet. Only told: no rule reads it.
pub fn like_asker<P: crate::same_voice::Printer>(
    voices: Option<&mut crate::same_voice::Voices<P>>,
    request: u64,
    going_on: bool,
) -> Option<f32> {
    if !going_on {
        return None;
    }
    voices?
        .like_asker(request)
        .filter(|like| like.is_finite())
        .map(|like| like.clamp(-1.0, 1.0))
}

/// `attention/addressed` on the bus: once for each request.
pub struct AddressedOut {
    publisher: Publisher,
    told: Option<(u64, u64)>,
}

impl AddressedOut {
    pub fn on(root: &Path) -> io::Result<Self> {
        let bus = Bus::new(root)?;
        Ok(Self {
            publisher: bus.publisher_declared(
                Addressed::TOPIC,
                &Addressed::declaration(),
                crate::bus_out::SOURCE,
            )?,
            told: None,
        })
    }

    /// Whether `said`'s request was already told.
    pub fn told(&self, session: u64, request: u64) -> bool {
        self.told == Some((session, request))
    }

    /// Publish `said` at `now_us`, unless its request was told already.
    /// True when it was published now.
    pub fn tell(&mut self, said: &Addressed, now_us: u64) -> io::Result<bool> {
        if self.told(said.session, said.request) {
            return Ok(false);
        }
        self.told = Some((said.session, said.request));
        self.publisher.publish_json(now_us, said)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::Face;

    fn face(height: f32, facing: Option<bool>) -> Face {
        Face {
            speaking: None,
            id: "f".into(),
            // From the top edge, so that the height is exact.
            xmin: 0.4,
            ymin: 0.0,
            xmax: 0.4 + height,
            ymax: height,
            confidence: 0.9,
            yaw: None,
            facing,
        }
    }

    fn faces(list: Vec<Face>) -> Faces {
        Faces { faces: list }
    }

    fn request(began_us: u64, ended_us: Option<u64>) -> Request {
        Request {
            session: 2,
            request: 7,
            began_us,
            ended_us,
            name_score: 0.0,
        }
    }

    #[test]
    fn the_switch_is_on_only_when_the_environment_says_on() {
        assert_eq!(arguments(Some("on")), vec![FLAG.to_owned()]);
        for value in [None, Some(""), Some("off"), Some("ON"), Some("busy")] {
            assert!(arguments(value).is_empty(), "{value:?}");
        }
        let flags = [FLAG.to_owned(), "--bus-root".to_owned()];
        assert_eq!(take(&flags), (true, &flags[1..]));
        assert_eq!(take(&flags[1..]), (false, &flags[1..]));
    }

    /// The provider worker's flags in the order the coordinator puts them
    /// (`coordinator::provider_arguments`), taken in the order the worker's
    /// `main` takes them: a flag out of place would reach the camera's
    /// parser and stop the worker.
    #[test]
    fn the_switch_is_taken_in_its_place_among_the_workers_flags() {
        use crate::attention;
        let taken = |flags: &[String]| {
            let (on, flags) = attention::take(flags);
            let (busy, flags) = attention::take_busy(flags);
            let (near, flags) = attention::take_near(flags);
            let (passing, flags) = attention::take_passing(flags);
            let (spoken_to, flags) = attention::take_spoken_to(flags);
            let (_, flags) = attention::take_by_day(flags);
            let (addressed, flags) = take(flags);
            let (_, flags) = attention::take_name(flags);
            let (_, _, flags) = crate::called::take(flags);
            let (root, flags) = crate::bus_out::take_root(flags).unwrap();
            let (_, flags) = crate::first_output::take(flags).unwrap();
            (
                (
                    on,
                    busy,
                    near,
                    passing,
                    spoken_to,
                    addressed,
                    root.is_some(),
                ),
                flags.to_vec(),
            )
        };
        let flags = |list: &[&str]| {
            list.iter()
                .map(|flag| (*flag).to_owned())
                .collect::<Vec<_>>()
        };
        let root = [crate::bus_out::ROOT_FLAG, "/run/lampos-bus"];
        let all = flags(
            &[
                attention::FLAG,
                attention::BUSY_FLAG,
                attention::NEAR_FLAG,
                FLAG,
            ]
            .into_iter()
            .chain(root)
            .collect::<Vec<_>>(),
        );
        assert_eq!(
            taken(&all),
            ((true, true, true, false, false, true, true), vec![])
        );
        // With "the lamp" in passing ignored (#829), between the two.
        let passing = flags(
            &[
                attention::FLAG,
                attention::BUSY_FLAG,
                attention::NEAR_FLAG,
                attention::PASSING_FLAG,
                FLAG,
            ]
            .into_iter()
            .chain(root)
            .collect::<Vec<_>>(),
        );
        assert_eq!(
            taken(&passing),
            ((true, true, true, true, false, true, true), vec![])
        );
        // With the lamp speaking only when spoken to (#829, step 2), right
        // after it, with it or without it.
        for (passing, spoken_to) in [(true, true), (false, true)] {
            let list = [attention::FLAG, attention::BUSY_FLAG, attention::NEAR_FLAG]
                .into_iter()
                .chain(passing.then_some(attention::PASSING_FLAG))
                .chain([attention::SPOKEN_TO_FLAG, FLAG])
                .chain(root)
                .collect::<Vec<_>>();
            assert_eq!(
                taken(&flags(&list)),
                ((true, true, true, passing, spoken_to, true, true), vec![])
            );
        }
        // And after the day's own bars (`LAMP_SPOKEN_TO=day-and-night`).
        let by_day = flags(
            &[
                attention::FLAG,
                attention::SPOKEN_TO_FLAG,
                attention::BY_DAY_FLAG,
                FLAG,
            ]
            .into_iter()
            .chain(root)
            .collect::<Vec<_>>(),
        );
        assert_eq!(
            taken(&by_day),
            ((true, false, false, false, true, true, true), vec![])
        );
        // Alone, it is taken too, and the worker leaves it off: it follows
        // the requests only with the attention rule.
        let alone = flags(&[FLAG].into_iter().chain(root).collect::<Vec<_>>());
        assert_eq!(
            taken(&alone),
            ((false, false, false, false, false, true, true), vec![])
        );
        let busy = flags(&[attention::FLAG, attention::BUSY_FLAG, FLAG]);
        assert_eq!(
            taken(&busy),
            ((true, true, false, false, false, true, false), vec![])
        );
    }

    #[test]
    fn only_a_face_turned_to_the_lamp_counts_and_the_tallest_wins() {
        assert_eq!(tallest_turned(&faces(vec![])), 0.0);
        assert_eq!(
            tallest_turned(&faces(vec![face(0.4, Some(false)), face(0.3, None)])),
            0.0
        );
        assert_eq!(
            tallest_turned(&faces(vec![face(0.1, Some(true)), face(0.25, Some(true))])),
            0.25
        );
    }

    /// The looks of one request: `heights` taken 100 ms apart from `from`.
    fn looks_of(from: u64, heights: &[(f32, Option<bool>)]) -> Looks {
        let mut looks = Looks::default();
        for (n, (height, facing)) in heights.iter().enumerate() {
            let at = from + n as u64 * 100_000;
            looks.look(at, &faces(vec![face(*height, *facing)]), at);
        }
        looks
    }

    #[test]
    fn close_and_facing_while_the_person_spoke_is_addressed() {
        let rises = VecDeque::from([(1_000_000, 4.0), (1_200_000, 22.0), (1_400_000, 9.0)]);
        let looks = looks_of(1_100_000, &[(0.2, Some(true)), (0.25, Some(true))]);
        let said = judge(
            request(1_000_000, Some(1_500_000)),
            &rises,
            &looks,
            1_600_000,
        );
        assert_eq!(said.rise_db, Some(22.0));
        assert_eq!(said.near, Some(true));
        assert_eq!(said.looks, 2);
        assert_eq!(said.facing_height, Some(0.25));
        assert_eq!(said.facing_share, Some(1.0));
        assert_eq!(said.facing_close, Some(true));
        assert!(said.addressed);
        assert!(said.check().is_ok());
    }

    #[test]
    fn far_or_small_or_turned_away_is_not_addressed() {
        let near = VecDeque::from([(1_200_000, 22.0)]);
        let far = VecDeque::from([(1_200_000, 12.0)]);
        let close_facing = [(0.2, Some(true)), (0.2, Some(true))];
        let small_facing = [(0.1, Some(true)), (0.1, Some(true))];
        let close_away = [(0.3, Some(false)), (0.3, None)];
        for (rises, seen, near_said, close_said) in [
            (&far, &close_facing, Some(false), Some(true)),
            (&near, &small_facing, Some(true), Some(false)),
            (&near, &close_away, Some(true), Some(false)),
        ] {
            let looks = looks_of(1_100_000, seen);
            let said = judge(
                request(1_000_000, Some(1_500_000)),
                rises,
                &looks,
                1_600_000,
            );
            assert_eq!(said.near, near_said);
            assert_eq!(said.facing_close, close_said);
            assert!(!said.addressed);
            assert!(said.check().is_ok());
        }
    }

    #[test]
    fn one_glance_among_the_looks_is_not_facing_and_one_look_says_nothing() {
        let rises = VecDeque::from([(1_200_000, 22.0)]);
        let glance = [
            (0.2, Some(false)),
            (0.2, Some(true)),
            (0.2, Some(false)),
            (0.2, Some(false)),
        ];
        let said = judge(
            request(1_000_000, Some(1_500_000)),
            &rises,
            &looks_of(1_000_000, &glance),
            1_600_000,
        );
        assert_eq!((said.looks, said.facing_share), (4, Some(0.25)));
        assert_eq!(said.facing_close, Some(false));
        // Two of five is the line, and counts.
        let two_of_five = [
            (0.2, Some(true)),
            (0.2, Some(false)),
            (0.2, Some(true)),
            (0.2, None),
            (0.0, None),
        ];
        let said = judge(
            request(1_000_000, Some(1_500_000)),
            &rises,
            &looks_of(1_000_000, &two_of_five),
            1_600_000,
        );
        assert_eq!(said.facing_share, Some(0.4));
        assert!(said.addressed);
        // One look is too few to say either way.
        let said = judge(
            request(1_000_000, Some(1_500_000)),
            &rises,
            &looks_of(1_100_000, &[(0.3, Some(true))]),
            1_600_000,
        );
        assert_eq!((said.looks, said.facing_height), (1, Some(0.3)));
        assert_eq!((said.facing_share, said.facing_close), (None, None));
        assert!(!said.addressed);
        assert!(said.check().is_ok());
    }

    #[test]
    fn nothing_heard_or_seen_in_the_window_is_unknown_not_no() {
        // A rise before the request and looks long before it say nothing
        // of it.
        let rises = VecDeque::from([(500_000, 30.0), (2_000_000, 30.0)]);
        let mut looks = looks_of(100_000, &[(0.3, Some(true)), (0.3, Some(true))]);
        let said = judge(
            request(3_000_000, Some(4_000_000)),
            &rises,
            &looks,
            4_100_000,
        );
        assert_eq!((said.rise_db, said.near), (None, None));
        assert_eq!(
            (said.looks, said.facing_height, said.facing_close),
            (0, None, None)
        );
        assert!(!said.addressed);
        assert!(said.check().is_ok());
        // The eyes looked and saw nobody turned to it: that is a no.
        looks.look(3_500_000, &faces(vec![]), 3_500_000);
        looks.look(3_600_000, &faces(vec![]), 3_600_000);
        let said = judge(
            request(3_000_000, Some(4_000_000)),
            &rises,
            &looks,
            4_100_000,
        );
        assert_eq!(
            (said.facing_height, said.facing_share),
            (Some(0.0), Some(0.0))
        );
        assert_eq!(said.facing_close, Some(false));
    }

    #[test]
    fn looks_up_to_a_second_and_a_half_before_the_person_began_count() {
        let rises = VecDeque::from([(3_100_000, 25.0)]);
        let turned = [(0.2, Some(true)), (0.2, Some(true))];
        // Both from 1.5 s before or later: counted. One 1.6 s before: not,
        // and the one left is too few.
        assert!(
            judge(
                request(3_000_000, None),
                &rises,
                &looks_of(1_500_000, &turned),
                3_200_000
            )
            .addressed
        );
        let early = judge(
            request(3_000_000, None),
            &rises,
            &looks_of(1_400_000, &turned),
            3_200_000,
        );
        assert_eq!(early.looks, 1);
        assert!(!early.addressed);
    }

    #[test]
    fn a_request_still_being_said_is_judged_up_to_now() {
        let rises = VecDeque::from([(1_100_000, 10.0), (1_900_000, 24.0)]);
        let looks = looks_of(1_700_000, &[(0.2, Some(true)), (0.2, Some(true))]);
        let early = judge(request(1_000_000, None), &rises, &looks, 1_500_000);
        assert_eq!(early.near, Some(false));
        assert_eq!(early.facing_close, None);
        let later = judge(request(1_000_000, None), &rises, &looks, 2_000_000);
        assert!(later.addressed);
        assert_eq!(later.ended_us, None);
    }

    /// Prints by a line's first sample, given at once: 1 the asker's voice,
    /// 3 the asker's a little off (0.90 against it), 4 a colleague's (0.20).
    #[derive(Default)]
    struct ByFirstSample(VecDeque<(u64, Result<Vec<f32>, String>)>);

    impl crate::same_voice::Printer for ByFirstSample {
        fn ask(&mut self, request: u64, samples: &[i16]) -> bool {
            let print = match samples[0] {
                1 => vec![1.0, 0.0, 0.0],
                3 => vec![0.9, 0.0, -0.436],
                _ => vec![0.2, 0.0, 0.98],
            };
            self.0.push_back((request, Ok(print)));
            true
        }
        fn answer(&mut self) -> Option<(u64, Result<Vec<f32>, String>)> {
            self.0.pop_front()
        }
    }

    /// #4306: a follow-up in the asker's voice reads high, a colleague's
    /// line at the desk low; nothing while no conversation is going on or
    /// without the same-voice check. Beside the judgement it is a message
    /// the Policy takes.
    #[test]
    fn like_asker_is_told_only_in_a_conversation_and_high_in_the_askers_voice() {
        use crate::same_voice::{LEAST_SAMPLES, Voices};
        let mut voices = Voices::new(ByFirstSample::default());
        let say = |voices: &mut Voices<ByFirstSample>, request: u64, voice: i16| {
            voices.began(request, true);
            voices.heard(request, &vec![voice; LEAST_SAMPLES * 2]);
            voices.ended(request);
        };
        say(&mut voices, 1, 1);
        voices.decided(1, true, false);
        say(&mut voices, 2, 3);
        say(&mut voices, 3, 4);
        let near = |like: Option<f32>, to: f32| like.is_some_and(|like| (like - to).abs() < 0.01);
        assert!(near(like_asker(Some(&mut voices), 2, true), 0.9));
        assert!(near(like_asker(Some(&mut voices), 3, true), 0.2));
        assert_eq!(like_asker(Some(&mut voices), 2, false), None);
        assert_eq!(like_asker::<ByFirstSample>(None, 2, true), None);
        let said = Addressed {
            like_asker: like_asker(Some(&mut voices), 3, true),
            ..judge(
                request(1_000_000, Some(1_500_000)),
                &VecDeque::new(),
                &Looks::default(),
                1_600_000,
            )
        };
        assert!(said.like_asker.is_some());
        assert!(said.check().is_ok());
    }

    #[test]
    fn a_look_on_the_bus_goes_in_and_the_judgement_goes_out_once_for_each_request() {
        use std::time::Duration;
        const WAIT: Duration = Duration::from_millis(500);
        // A root short enough for a socket's path on a Mac.
        let root = std::path::PathBuf::from(format!("/tmp/lva-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut eyes = bus
            .publisher_declared(Faces::TOPIC, &Faces::declaration(), "eyes")
            .unwrap();
        eyes.expires_after(Some(Duration::from_secs(2)));
        let mut out = AddressedOut::on(&root).unwrap();
        let mut policy = bus
            .subscribe_typed(Addressed::TOPIC, "policy", Addressed::TYPE)
            .unwrap();
        assert_eq!(
            bus.declaration(Addressed::TOPIC).unwrap(),
            Some(Addressed::declaration())
        );
        // A subscriber is found within the bus's 200 ms.
        std::thread::sleep(Duration::from_millis(250));
        let mut looks = Looks::default();
        for at in [1_100_000, 1_350_000] {
            eyes.publish_json(at, &faces(vec![face(0.2, Some(true))]))
                .unwrap();
            looks.read(&bus.latest(Faces::TOPIC).unwrap().unwrap(), at + 50_000);
            // When the eyes took it, not when it was read.
            assert_eq!(looks.looks.back(), Some(&(at, 0.2)));
            // Read again, the same look is not kept twice.
            looks.read(&bus.latest(Faces::TOPIC).unwrap().unwrap(), at + 150_000);
        }
        assert_eq!(looks.looks.len(), 2);
        let rises = VecDeque::from([(1_200_000, 21.0)]);
        let said = judge(
            request(1_000_000, Some(1_500_000)),
            &rises,
            &looks,
            1_600_000,
        );
        assert!(said.addressed);
        assert!(!out.told(said.session, said.request));
        assert!(out.tell(&said, 1_600_000).unwrap());
        assert!(out.told(said.session, said.request));
        assert!(!out.tell(&said, 1_700_000).unwrap());
        let message = policy.recv(WAIT).unwrap().expect("the judgement");
        assert_eq!(message.header.source, crate::bus_out::SOURCE);
        assert_eq!(message.header.at_us, 1_600_000);
        let heard: Addressed = message.parse().unwrap();
        assert_eq!(heard, said);
        assert!(heard.check().is_ok());
        assert!(policy.recv(Duration::from_millis(100)).unwrap().is_none());
        // The next request is told in its turn.
        let next = Addressed {
            request: said.request + 1,
            ..said
        };
        assert!(out.tell(&next, 2_000_000).unwrap());
        let heard: Addressed = policy.recv(WAIT).unwrap().unwrap().parse().unwrap();
        assert_eq!(heard.request, said.request + 1);
        // A look that is no longer fresh is not kept.
        eyes.expires_after(Some(Duration::from_millis(1)));
        eyes.publish_json(2_100_000, &faces(vec![face(0.3, Some(true))]))
            .unwrap();
        std::thread::sleep(Duration::from_millis(1 + lamp_bus::EXPIRY_GRACE_MS + 30));
        let stale = bus.latest(Faces::TOPIC).unwrap().unwrap();
        assert!(stale.expired);
        looks.read(&stale, 2_200_000);
        assert_eq!(looks.looks.back(), Some(&(1_350_000, 0.2)));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_looks_kept_are_bounded_and_in_order() {
        let mut looks = Looks::default();
        let seen = faces(vec![face(0.2, Some(true))]);
        for at in 0..1_000u64 {
            looks.look(at * 10_000, &seen, at * 10_000);
        }
        assert!(looks.looks.len() <= MOST_LOOKS);
        // The same look read twice, or an older one, is not kept again.
        let newest = looks.looks.back().unwrap().0;
        looks.look(newest, &seen, newest);
        looks.look(newest - 1, &seen, newest);
        assert_eq!(looks.looks.back().unwrap().0, newest);
        assert!(
            looks
                .looks
                .iter()
                .zip(looks.looks.iter().skip(1))
                .all(|(a, b)| a.0 < b.0)
        );
        // Thirty seconds on, the old looks are gone.
        looks.look(newest + KEPT_US + 1, &seen, newest + KEPT_US + 1);
        assert_eq!(looks.looks.len(), 1);
    }
}
