//! What a kept proposal becomes (#2530 part 1): a request to the part that
//! owns the output, never a write of the judge's own. A light is asked of the
//! ring on `light/request` at [`Priority::Idle`], below every skill, the
//! conversation, safety and a person: at `Skill` an add-on's newer claim would
//! push a skill's own glow aside, since the ring shows the newest of equals.
//! The judge keeps one asker per add-on, `addon:<name>`; its claim lapses by
//! itself at the proposal's `holds_ms` (at most 30 s), and a person's own
//! light is over it at once.
use lamp_msgs::{AddonProposal, Claim, ClockStamp, Light, LightRequest, MoodNudge, Priority};

/// The claim's source for an add-on's light: one a partner's skill cannot
/// take, as a skill's is `skill:<name>`.
pub fn source(addon: &str) -> String {
    let mut source = format!("addon:{addon}");
    source.truncate(64);
    source
}

/// How long an add-on's light may lie over a person's own light that rests:
/// a passing glow, never their colour (the first builder on #2530, 20:1x;
/// the evening glow's rule, #1672).
pub const BRIEF_GLOW_US: u64 = 5_000_000;

/// The ring's request for a kept `light` proposal, on `clock_id` at
/// `now_us`, holding as long as the proposal does (at most
/// [`Claim::MOST_US`]), or over the person's own light (`over_persons`, one
/// that rests) only [`BRIEF_GLOW_US`]: the ring then shows their light again
/// by itself. `None` for any other act, or a light that does not pass the
/// ring's own check.
pub fn light_request(
    proposal: &AddonProposal,
    clock_id: &str,
    now_us: u64,
    over_persons: bool,
) -> Option<LightRequest> {
    let light = proposal.light.as_ref()?;
    let most_us = if over_persons {
        BRIEF_GLOW_US
    } else {
        Claim::MOST_US
    };
    let holds_us = (u64::from(proposal.holds_ms) * 1_000).min(most_us);
    let request = LightRequest {
        claim: Claim {
            source: source(&proposal.addon),
            priority: Priority::Idle,
            clock_id: clock_id.into(),
            until_us: now_us + holds_us,
        },
        light: Light::new(true, &light.colour, light.level).ok()?,
        pattern: light.pattern,
        fade_ms: None,
    };
    request.check().ok().map(|()| request)
}

/// Mood's nudge for a kept `mood` proposal, the `sequence`th the judge
/// says on `clock_id` at `now_us`, holding its `for_ms`. `None` for any
/// other act, or a nudge that does not pass Mood's own check (warmth down).
pub fn mood_nudge(
    proposal: &AddonProposal,
    clock_id: &str,
    now_us: u64,
    sequence: u64,
) -> Option<MoodNudge> {
    let mood = proposal.mood.as_ref()?;
    let nudge = MoodNudge {
        stamp: ClockStamp {
            clock_id: clock_id.into(),
            at_us: now_us,
            sequence,
        },
        addon: proposal.addon.clone(),
        energy: mood.energy.unwrap_or(0.0),
        warmth: mood.warmth.unwrap_or(0.0),
        until_us: now_us + u64::from(mood.for_ms) * 1_000,
    };
    nudge.check().ok().map(|()| nudge)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::{AddonLight, AddonMood, LightPattern, PatternKind};

    fn glow() -> AddonProposal {
        AddonProposal {
            proposal: 7,
            addon: "evening-glow".into(),
            version: "1.0.0".into(),
            act: "light".into(),
            why: "evening".into(),
            sure: 0.6,
            holds_ms: 30_000,
            light: Some(AddonLight {
                colour: "warm".into(),
                level: 2,
                pattern: Some(LightPattern {
                    kind: PatternKind::Breathe,
                    period_ms: 8_000,
                    low: 1,
                }),
            }),
            gesture: None,
            mood: None,
            note: None,
        }
    }

    #[test]
    fn a_kept_light_is_asked_of_the_ring_at_idle_for_as_long_as_it_holds() {
        let request = light_request(&glow(), "boot-a", 1_000, false).unwrap();
        assert_eq!(request.claim.source, "addon:evening-glow");
        assert_eq!(request.claim.priority, Priority::Idle);
        assert_eq!(request.claim.clock_id, "boot-a");
        assert_eq!(request.claim.until_us, 1_000 + 30_000_000);
        assert!(request.claim.holds_at("boot-a", 1_000));
        assert!(
            !request.claim.holds_at("boot-b", 1_000),
            "another boot's clock"
        );
        assert_eq!(request.light, Light::new(true, "warm", 2).unwrap());
        assert_eq!(request.pattern.unwrap().period_ms, 8_000);
        // Below every skill, so a skill's glow is never pushed aside.
        assert!(request.claim.priority < Priority::Skill);
    }

    #[test]
    fn a_shorter_proposal_holds_shorter_and_only_a_light_is_asked() {
        let short = AddonProposal {
            holds_ms: 5_000,
            ..glow()
        };
        assert_eq!(
            light_request(&short, "b", 0, false).unwrap().claim.until_us,
            5_000_000
        );
        let lift = AddonProposal {
            act: "mood".into(),
            light: None,
            mood: Some(AddonMood {
                energy: Some(0.1),
                warmth: None,
                for_ms: 600_000,
            }),
            ..glow()
        };
        assert!(light_request(&lift, "b", 0, false).is_none());
        let nudge = mood_nudge(&lift, "b", 5, 1).unwrap();
        assert_eq!((nudge.energy, nudge.warmth), (0.1, 0.0));
        assert_eq!(nudge.until_us, 5 + 600_000_000);
        assert!(mood_nudge(&glow(), "b", 5, 1).is_none());
    }

    /// Over a person's own light, which rests, a few seconds only: then the
    /// ring shows theirs again by itself.
    #[test]
    fn over_a_persons_light_a_glow_is_brief() {
        let request = light_request(&glow(), "b", 1_000, true).unwrap();
        assert_eq!(request.claim.until_us, 1_000 + BRIEF_GLOW_US);
        assert_eq!(request.claim.priority, Priority::Idle);
    }

    #[test]
    fn the_source_stays_within_a_claims_sixty_four() {
        let long = "a".repeat(64);
        assert_eq!(source(&long).len(), 64);
        assert!(source(&long).starts_with("addon:"));
    }
}
