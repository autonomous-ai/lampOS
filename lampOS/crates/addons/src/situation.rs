//! What the judge knows of the lamp, folded from the bus a message at a time
//! ([`Watcher`]), so the rules ([`crate::rules`]) see one [`Situation`]. The
//! part that runs the judge reads the topics and hands each message here with
//! the time it was read; nothing here reads the bus or a clock itself.
use crate::rules::{ASLEEP_FRESH_US, Reading, Situation};
use lamp_msgs::{
    ExpressionRestPhase, ExpressionRestState, Light, MicrophonePrivacy, MoodState,
    OnboardingPending, Pause, Room, RoomState, Voice, VoiceMute, VoiceState,
};

/// Who writes `light/command` when a person sets the light: the actions part
/// (by voice, or a skill's light a person asked to keep) and `lamp-cloud`
/// (the app). Its header's `source` says which.
pub const PERSONS_LIGHT_SOURCES: [&str; 2] = ["actions", "cloud"];

/// A person's light rests this long after they set it, if the lamp has not
/// slept since: then an add-on may lay a brief glow over it (the first
/// builder on #2530, 20:1x).
pub const PERSONS_LIGHT_RESTS_US: u64 = 12 * 3_600_000_000;

/// The topics the judge reads, each with what it tells.
pub const TOPICS: [&str; 10] = [
    MicrophonePrivacy::TOPIC,
    Pause::TOPIC,
    VoiceMute::TOPIC,
    ExpressionRestState::TOPIC,
    OnboardingPending::TOPIC,
    VoiceState::TOPIC,
    RoomState::TOPIC,
    "light/command",
    "light/state",
    MoodState::TOPIC,
];

/// The readings as they came.
#[derive(Clone, Debug)]
pub struct Watcher {
    /// When the judge began reading, so a `voice/mute` never said can be
    /// told from one not read yet.
    began_us: u64,
    /// Only the wait for a mute never said, not a said mute's freshness.
    mute_grace_us: u64,
    privacy_closed: Option<Reading<bool>>,
    paused: Option<bool>,
    muted_asleep: Option<Reading<bool>>,
    resting: Option<Reading<bool>>,
    /// Setup's last word had an invitation pending.
    setting_up: bool,
    /// The voice's publisher boot, as `voice/state` last came: a new one is
    /// a restarted voice, which may say no mute any more.
    voice_boot: Option<String>,
    voice_resting: Option<Reading<bool>>,
    /// Since when the voice has rested, while it still does.
    resting_since_us: Option<u64>,
    room_quiet: Option<Reading<bool>>,
    /// The newest light a person set this boot, if any, and when, on the
    /// boot's clock (its message's own time, so one read at the judge's
    /// start keeps its age).
    persons: Option<(Light, u64)>,
    /// Mood has said `asleep` since the person set it.
    slept_since_persons: bool,
    /// The boot's clock the judge's times are on: Mood's stamps on another
    /// are not compared with them.
    clock_id: Option<String>,
    /// Until when a glow the judge asked of the ring holds: while it shows,
    /// the ring's light is the add-on's, over the person's still there.
    glow_until_us: u64,
    /// The light the ring says is the person's (`light/state`).
    shown: Option<Light>,
}

impl Default for Watcher {
    fn default() -> Self {
        Self {
            began_us: 0,
            mute_grace_us: ASLEEP_FRESH_US,
            privacy_closed: None,
            paused: None,
            muted_asleep: None,
            resting: None,
            setting_up: false,
            voice_boot: None,
            voice_resting: None,
            resting_since_us: None,
            room_quiet: None,
            persons: None,
            slept_since_persons: false,
            clock_id: None,
            glow_until_us: 0,
            shown: None,
        }
    }
}

/// How fresh the rest skill's word must be to count: its topic lasts 500 ms.
const REST_FRESH_US: u64 = 1_000_000;

impl Watcher {
    /// Mood's stamps count only on `clock_id`, the judge's own.
    pub fn on_clock(mut self, clock_id: &str) -> Self {
        self.clock_id = Some(clock_id.to_owned());
        self
    }

    /// A watcher that began reading at `began_us`, waiting `mute_grace_us`
    /// before a mute never said is known absent. The product passes
    /// [`ASLEEP_FRESH_US`]; tests may pass a shorter wait.
    pub fn new(began_us: u64, mute_grace_us: u64) -> Self {
        Self {
            began_us,
            mute_grace_us,
            ..Self::default()
        }
    }

    /// `voice/privacy`: closed unless the voice permits capture now. The
    /// voice's own lease is the authority on the switch, on a lamp with a
    /// slide or without one.
    pub fn privacy(&mut self, privacy: &MicrophonePrivacy, at_us: u64) {
        self.privacy_closed = Some(Reading::new(!privacy.permitted, at_us));
    }

    /// `lamp/pause`.
    pub fn pause(&mut self, pause: &Pause) {
        self.paused = Some(pause.paused);
    }

    /// `voice/mute`: muted because it was put to sleep.
    pub fn mute(&mut self, mute: &VoiceMute, at_us: u64) {
        self.muted_asleep = Some(Reading::new(mute.asleep(), at_us));
    }

    /// `expression/rest_state`: resting is asleep too.
    pub fn rest(&mut self, rest: &ExpressionRestState, at_us: u64) {
        self.resting = Some(Reading::new(
            rest.phase == ExpressionRestPhase::Resting,
            at_us,
        ));
    }

    /// `onboarding/pending`: setting up while it holds an invitation. Only
    /// a lamp with setup on says it; its last word stands, so a setup that
    /// stopped mid-step keeps add-ons out, the cautious way.
    pub fn onboarding(&mut self, pending: &OnboardingPending) {
        self.setting_up = !pending.invitations.is_empty();
    }

    /// `voice/state`, from the voice's publisher `boot`. A voice restarted
    /// (a new boot) may run without the head's controls and say no mute, so
    /// the old mute is forgotten and its silence is waited for afresh, as at
    /// the judge's start; otherwise that mute would go stale and hold the
    /// lamp asleep until the judge restarted.
    pub fn voice(&mut self, voice: &VoiceState, at_us: u64, boot: &str) {
        if self
            .voice_boot
            .as_deref()
            .is_some_and(|known| known != boot)
        {
            self.muted_asleep = None;
            self.began_us = at_us;
        }
        if self.voice_boot.as_deref() != Some(boot) {
            self.voice_boot = Some(boot.to_owned());
        }
        let resting = voice.state == Voice::Resting;
        self.voice_resting = Some(Reading::new(resting, at_us));
        // Since when it rests: its first resting word after anything else,
        // or after the judge began reading (taken the cautious way, as if
        // it had just spoken).
        self.resting_since_us = match (resting, self.resting_since_us) {
            (true, Some(since)) => Some(since),
            (true, None) => Some(at_us),
            (false, _) => None,
        };
    }

    /// `room/state`: only `quiet` is quiet; `unknown` is not.
    pub fn room(&mut self, room: &RoomState, at_us: u64) {
        self.room_quiet = Some(Reading::new(room.state == Room::Quiet, at_us));
    }

    /// `light/command`, with its header's `source` and time (`at_us`): a
    /// person's light when the actions part or the app wrote it.
    pub fn light_command(&mut self, light: &Light, source: &str, at_us: u64) {
        if PERSONS_LIGHT_SOURCES.contains(&source) {
            self.persons = Some((light.clone(), at_us));
            self.slept_since_persons = false;
        }
    }

    /// `mood/state`: Mood asleep after the person set their light.
    pub fn mood(&mut self, mood: &MoodState) {
        if mood.asleep
            && self
                .clock_id
                .as_deref()
                .is_none_or(|clock_id| clock_id == mood.stamp.clock_id)
            && self
                .persons
                .as_ref()
                .is_some_and(|(_, at_us)| mood.stamp.at_us > *at_us)
        {
            self.slept_since_persons = true;
        }
    }

    /// The judge asked the ring for a glow holding until `until_us`.
    pub fn glowing_until(&mut self, until_us: u64) {
        self.glow_until_us = self.glow_until_us.max(until_us);
    }

    /// `light/state`.
    pub fn light_state(&mut self, light: &Light) {
        self.shown = Some(light.clone());
    }

    /// The situation at `now_us`. Asleep is the voice's sleep mute, or the
    /// rest skill resting while its word is fresh: the expression part is
    /// off on most lamps, so its silence does not count as awake or asleep.
    /// The voice says its mute every second only with the head's controls
    /// on; on a lamp without them, nothing said once the judge has read for
    /// its configured grace ([`ASLEEP_FRESH_US`] on the product) means it has
    /// no sleep to say, and the rest skill alone tells it. A mute said once
    /// and then gone stale stays cautious.
    /// The person's light is shown while the ring's light is the one a
    /// person set this boot, or one of the judge's own glows is over it;
    /// with none set, the ring shows its own. It rests once the lamp has
    /// slept since it was set, or [`PERSONS_LIGHT_RESTS_US`] have passed.
    pub fn situation(&self, now_us: u64) -> Situation {
        let resting = self.resting.is_some_and(|reading| {
            reading.value && now_us.saturating_sub(reading.at_us) <= REST_FRESH_US
        });
        Situation {
            privacy_closed: self.privacy_closed,
            paused: self.paused,
            asleep: match self.muted_asleep {
                Some(reading) => Some(Reading::new(reading.value || resting, reading.at_us)),
                None if now_us.saturating_sub(self.began_us) >= self.mute_grace_us => {
                    Some(Reading::new(resting, now_us))
                }
                None => None,
            },
            onboarding: self.setting_up,
            voice_resting: self.voice_resting,
            resting_since_us: self.resting_since_us,
            room_quiet: self.room_quiet,
            persons_light: match (&self.persons, &self.shown) {
                (Some(_), _) if now_us < self.glow_until_us => true,
                (Some((persons, _)), Some(shown)) => same_light(persons, shown),
                // A person set one, and the ring has not said what it shows:
                // theirs, the cautious way.
                (Some(_), None) => true,
                (None, _) => false,
            },
            // A light they set off never rests: off stays dark (the
            // reviewer-lead's P2 on #2640).
            persons_light_rests: self.persons.as_ref().is_some_and(|(persons, at_us)| {
                persons.on
                    && (self.slept_since_persons
                        || now_us.saturating_sub(*at_us) >= PERSONS_LIGHT_RESTS_US)
            }),
        }
    }
}

/// Two lights alike as the person sees them: off is off whatever its colour.
fn same_light(a: &Light, b: &Light) -> bool {
    a.on == b.on && (!a.on || (a.colour == b.colour && a.level == b.level))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp() -> lamp_msgs::ClockStamp {
        lamp_msgs::ClockStamp {
            clock_id: "boot".into(),
            at_us: 1,
            sequence: 1,
        }
    }

    fn light(on: bool, colour: &str, level: u8) -> Light {
        Light::new(on, colour, level).unwrap()
    }

    #[test]
    fn a_light_a_person_set_is_theirs_while_it_shows_and_the_lamps_otherwise() {
        let mut watcher = Watcher::default();
        watcher.light_state(&light(true, "warm", 1));
        assert!(
            !watcher.situation(0).persons_light,
            "the ring's own, as it starts"
        );
        // A skill's or the ring's own word is not a person's.
        watcher.light_command(&light(true, "blue", 3), "skills", 0);
        assert!(!watcher.situation(0).persons_light);
        watcher.light_command(&light(true, "blue", 3), "actions", 0);
        watcher.light_state(&light(true, "blue", 3));
        assert!(watcher.situation(0).persons_light);
        // The app's too.
        watcher.light_command(&light(true, "green", 2), "cloud", 0);
        watcher.light_state(&light(true, "green", 2));
        assert!(watcher.situation(0).persons_light);
        // Off is off, whatever colour was asked.
        watcher.light_command(&light(false, "green", 2), "cloud", 0);
        watcher.light_state(&light(false, "warm", 1));
        assert!(watcher.situation(0).persons_light);
        // Shown differently from what they set: not theirs now.
        watcher.light_state(&light(true, "warm", 1));
        assert!(!watcher.situation(0).persons_light);
    }

    fn mood(asleep: bool, at_us: u64) -> MoodState {
        MoodState {
            stamp: lamp_msgs::ClockStamp {
                clock_id: "boot".into(),
                at_us,
                sequence: 1,
            },
            session_id: "s".into(),
            awake: if asleep { 0.0 } else { 0.8 },
            energy: 0.3,
            warmth: 0.6,
            asleep,
            attention: None,
            familiarity: lamp_msgs::Familiarity::Unknown,
            familiar: None,
            felt: None,
            breath: None,
        }
    }

    /// The first builder on #2530 (20:1x): a person's light rests once the
    /// lamp has slept since they set it, or 12 h have passed; a newer
    /// choice of theirs is fresh again.
    #[test]
    fn a_persons_light_rests_after_a_sleep_or_twelve_hours() {
        let mut watcher = Watcher::default();
        let orange = light(true, "orange", 1);
        watcher.light_command(&orange, "actions", 1_000);
        watcher.light_state(&orange);
        let s = watcher.situation(2_000);
        assert!(s.persons_light && !s.persons_light_rests);
        // A sleep said before they chose it does not count.
        watcher.mood(&mood(true, 500));
        assert!(!watcher.situation(2_000).persons_light_rests);
        watcher.mood(&mood(false, 1_500));
        assert!(!watcher.situation(2_000).persons_light_rests);
        watcher.mood(&mood(true, 1_600));
        assert!(watcher.situation(2_000).persons_light_rests);
        // Chosen again: fresh until 12 h on.
        watcher.light_command(&orange, "cloud", 10_000);
        assert!(
            !watcher
                .situation(10_000 + PERSONS_LIGHT_RESTS_US - 1)
                .persons_light_rests
        );
        assert!(
            watcher
                .situation(10_000 + PERSONS_LIGHT_RESTS_US)
                .persons_light_rests
        );
    }

    /// The reviewer-lead's P2 on #2640: a light they set off never rests,
    /// slept since or not, however long ago: off stays dark.
    #[test]
    fn a_light_set_off_never_rests() {
        let mut watcher = Watcher::default();
        let off = light(false, "warm", 1);
        watcher.light_command(&off, "actions", 1_000);
        watcher.light_state(&off);
        watcher.mood(&mood(true, 2_000));
        let s = watcher.situation(1_000 + 2 * PERSONS_LIGHT_RESTS_US);
        assert!(s.persons_light && !s.persons_light_rests);
    }

    /// The P3: Mood's sleep counts only on the judge's own clock.
    #[test]
    fn a_sleep_on_another_boots_clock_does_not_rest_it() {
        let mut watcher = Watcher::default().on_clock("this-boot");
        watcher.light_command(&light(true, "orange", 1), "actions", 1_000);
        watcher.mood(&mood(true, 2_000)); // stamped on "boot"
        assert!(!watcher.situation(3_000).persons_light_rests);
        let mut ours = mood(true, 2_500);
        ours.stamp.clock_id = "this-boot".into();
        watcher.mood(&ours);
        assert!(watcher.situation(3_000).persons_light_rests);
    }

    /// While the judge's own glow shows over it, the person's light is still
    /// there: the ring's other colour is the add-on's, not the lamp's own.
    #[test]
    fn the_judges_own_glow_over_a_persons_light_keeps_it_theirs() {
        let mut watcher = Watcher::default();
        watcher.light_command(&light(true, "orange", 1), "actions", 0);
        watcher.light_state(&light(true, "warm", 2));
        assert!(!watcher.situation(1_000).persons_light);
        watcher.glowing_until(5_000);
        assert!(watcher.situation(1_000).persons_light);
        assert!(!watcher.situation(5_000).persons_light);
    }

    #[test]
    fn a_light_set_with_nothing_shown_yet_counts_as_theirs() {
        let mut watcher = Watcher::default();
        watcher.light_command(&light(true, "blue", 3), "actions", 0);
        assert!(watcher.situation(0).persons_light);
    }

    #[test]
    fn the_rest_skill_resting_is_asleep_only_while_its_word_is_fresh() {
        let mut watcher = Watcher::default();
        let mute = |muted: bool| VoiceMute {
            muted,
            reason: None,
        };
        watcher.mute(&mute(false), 1_000_000);
        assert!(!watcher.situation(1_000_000).asleep.unwrap().value);
        let resting = ExpressionRestState {
            stamp: stamp(),
            expression_session_id: "s".into(),
            enabled: true,
            request: None,
            phase: ExpressionRestPhase::Resting,
            reason: None,
        };
        watcher.rest(&resting, 1_000_000);
        assert!(watcher.situation(1_500_000).asleep.unwrap().value);
        assert!(!watcher.situation(2_500_000).asleep.unwrap().value);
    }

    #[test]
    fn without_a_sleep_mute_said_the_rest_skill_alone_tells_asleep() {
        let mut watcher = Watcher::new(1_000_000, ASLEEP_FRESH_US);
        // Not read long enough to know the voice says no mute.
        assert_eq!(watcher.situation(2_000_000).asleep, None);
        let awake = watcher.situation(4_000_000).asleep.unwrap();
        assert!(!awake.value && awake.at_us == 4_000_000);
        let resting = ExpressionRestState {
            stamp: stamp(),
            expression_session_id: "s".into(),
            enabled: true,
            request: None,
            phase: ExpressionRestPhase::Resting,
            reason: None,
        };
        watcher.rest(&resting, 4_000_000);
        assert!(watcher.situation(4_500_000).asleep.unwrap().value);
        // A mute said once and gone stale keeps its own age.
        watcher.mute(
            &VoiceMute {
                muted: false,
                reason: None,
            },
            5_000_000,
        );
        assert_eq!(
            watcher.situation(9_000_000).asleep.unwrap().at_us,
            5_000_000
        );
    }

    /// The reviewer-lead's P3 on #2553: a voice restarted without the head's
    /// controls says no mute, so the old one must not hold the lamp asleep.
    #[test]
    fn a_restarted_voice_forgets_the_old_mute_and_is_waited_for_afresh() {
        let mut watcher = Watcher::new(0, ASLEEP_FRESH_US);
        let resting = VoiceState {
            state: Voice::Resting,
        };
        watcher.voice(&resting, 1_000_000, "voice-a");
        watcher.mute(
            &VoiceMute {
                muted: false,
                reason: None,
            },
            1_000_000,
        );
        // The same voice saying its state again keeps its mute.
        watcher.voice(&resting, 2_000_000, "voice-a");
        assert_eq!(
            watcher.situation(9_000_000).asleep.unwrap().at_us,
            1_000_000
        );
        // Restarted: unknown for a moment, then the rest skill alone.
        watcher.voice(&resting, 10_000_000, "voice-b");
        assert_eq!(watcher.situation(11_000_000).asleep, None);
        assert!(!watcher.situation(13_000_000).asleep.unwrap().value);
    }

    #[test]
    fn a_test_mute_grace_ends_at_its_boundary_and_restarts_with_the_voice() {
        let resting = VoiceState {
            state: Voice::Resting,
        };
        for grace in [0, 200_000] {
            let mut watcher = Watcher::new(1_000_000, grace);
            watcher.voice(&resting, 1_000_000, "voice-a");
            if grace > 0 {
                assert_eq!(watcher.situation(1_000_000 + grace - 1).asleep, None);
            }
            assert_eq!(
                watcher.situation(1_000_000 + grace).asleep,
                Some(Reading::new(false, 1_000_000 + grace))
            );
            watcher.mute(
                &VoiceMute {
                    muted: true,
                    reason: Some(lamp_msgs::MuteReason::Sleep),
                },
                2_000_000,
            );
            watcher.voice(&resting, 3_000_000, "voice-b");
            if grace > 0 {
                assert_eq!(watcher.situation(3_000_000 + grace - 1).asleep, None);
            }
            // A repeated state from this boot does not begin the wait again.
            watcher.voice(&resting, 3_000_000 + grace / 2, "voice-b");
            assert_eq!(
                watcher.situation(3_000_000 + grace).asleep,
                Some(Reading::new(false, 3_000_000 + grace))
            );
        }
    }

    #[test]
    fn a_test_mute_grace_keeps_a_said_mutes_three_second_freshness() {
        use crate::rules::{Drop, Judge, Verdict};

        let proposal = lamp_msgs::AddonProposal {
            proposal: 1,
            addon: "test".into(),
            version: "1.0.0".into(),
            act: "light".into(),
            why: "evening".into(),
            sure: 0.6,
            holds_ms: 30_000,
            light: Some(lamp_msgs::AddonLight {
                colour: "warm".into(),
                level: 2,
                pattern: None,
            }),
            gesture: None,
            mood: None,
            note: None,
        };
        for grace in [0, 200_000] {
            let mut watcher = Watcher::new(0, grace);
            watcher.mute(
                &VoiceMute {
                    muted: false,
                    reason: None,
                },
                1_000_000,
            );
            for (now_us, expected) in [
                (4_000_000, Verdict::Kept),
                (4_000_001, Verdict::Dropped(Drop::Asleep)),
            ] {
                let mut situation = watcher.situation(now_us);
                assert_eq!(situation.asleep, Some(Reading::new(false, 1_000_000)));
                situation.privacy_closed = Some(Reading::new(false, now_us));
                situation.voice_resting = Some(Reading::new(true, now_us));
                situation.room_quiet = Some(Reading::new(true, now_us));
                assert_eq!(
                    Judge::default().judge(&proposal, 0, &situation, now_us),
                    expected
                );
            }
        }
    }

    #[test]
    fn setup_with_an_invitation_pending_is_setting_up() {
        let mut watcher = Watcher::default();
        assert!(!watcher.situation(0).onboarding);
        let invitation = lamp_msgs::OnboardingInvitation {
            invitation: 1,
            generation: 1,
            attempt: 1,
            step: lamp_msgs::OnboardingStep::Privacy,
            ready: true,
        };
        watcher.onboarding(&OnboardingPending {
            invitations: vec![invitation],
            walk: true,
        });
        assert!(watcher.situation(0).onboarding);
        watcher.onboarding(&OnboardingPending {
            invitations: Vec::new(),
            walk: false,
        });
        assert!(!watcher.situation(0).onboarding);
    }

    /// Since when the voice rests: from its first resting word after
    /// anything else, kept while it rests, and gone once it doesn't.
    #[test]
    fn the_voice_rests_since_its_first_resting_word() {
        let mut watcher = Watcher::default();
        let state = |state| VoiceState { state };
        watcher.voice(&state(Voice::Resting), 1, "v");
        watcher.voice(&state(Voice::Resting), 5, "v");
        assert_eq!(watcher.situation(5).resting_since_us, Some(1));
        watcher.voice(&state(Voice::Speaking), 6, "v");
        assert_eq!(watcher.situation(6).resting_since_us, None);
        watcher.voice(&state(Voice::Resting), 9, "v");
        assert_eq!(watcher.situation(9).resting_since_us, Some(9));
    }

    #[test]
    fn privacy_is_closed_unless_capture_is_permitted() {
        let mut watcher = Watcher::default();
        assert!(watcher.situation(0).privacy_closed.is_none());
        let mut privacy = MicrophonePrivacy {
            stamp: stamp(),
            session_id: "0123456789abcdef".into(),
            controller_boot: "0123456789abcdef0123456789abcdef".into(),
            microphone_generation: 1,
            observed_us: 1,
            expires_at_us: 2,
            permitted: true,
            physical_muted: Some(false),
            no_slide: false,
        };
        watcher.privacy(&privacy, 5);
        assert_eq!(
            watcher.situation(5).privacy_closed,
            Some(Reading::new(false, 5))
        );
        privacy.permitted = false;
        watcher.privacy(&privacy, 6);
        assert_eq!(
            watcher.situation(6).privacy_closed,
            Some(Reading::new(true, 6))
        );
    }

    #[test]
    fn only_a_quiet_room_and_a_resting_voice_count() {
        let mut watcher = Watcher::default();
        watcher.room(
            &RoomState {
                state: Room::Unknown,
            },
            1,
        );
        watcher.voice(
            &VoiceState {
                state: Voice::Listening,
            },
            1,
            "voice-a",
        );
        let s = watcher.situation(1);
        assert!(!s.room_quiet.unwrap().value);
        assert!(!s.voice_resting.unwrap().value);
        watcher.room(&RoomState { state: Room::Quiet }, 2);
        watcher.voice(
            &VoiceState {
                state: Voice::Resting,
            },
            2,
            "voice-a",
        );
        let s = watcher.situation(2);
        assert!(s.room_quiet.unwrap().value && s.voice_resting.unwrap().value);
        watcher.pause(&Pause { paused: true });
        assert_eq!(watcher.situation(2).paused, Some(true));
    }
}
