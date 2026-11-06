//! Nonpersonal invitation identity for the core onboarding skill (#841).
use crate::Invalid;
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnboardingStep {
    Privacy,
    /// One prompt to call the lamp by name from the person's usual seat
    /// (#1803). Only the prompt's identity travels here, never a measurement.
    HowYouSound,
    /// One invitation to move the lamp's head gently by hand (#1800). Only
    /// its identity travels here, never what the body felt.
    GivingWay,
    /// A step a newer voice names (one release forward, the reviewer on
    /// #1964): the core skill still hands its token to the say service,
    /// which alone decides whether it is current. Never sent.
    #[serde(other)]
    Unknown,
}

/// The runner owns these identities. A new invitation token is required after
/// a restart or redo; the say service must match its exact current pending token.
/// No sensor evidence, person data or guessed switch location travels here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnboardingInvitation {
    pub invitation: u64,
    pub generation: u64,
    pub attempt: u64,
    pub step: OnboardingStep,
    pub ready: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnboardingPending {
    pub invitations: Vec<OnboardingInvitation>,
    /// The setup walk is under way (#4367): from its start to its last
    /// step's outcome, the gaps between steps and a redo included, whether
    /// or not an invitation is pending. Skills that keep out of setup gate
    /// on it, since `invitations` is empty through the steps the model
    /// leads by talk. Absent from an older voice: false.
    #[serde(default)]
    pub walk: bool,
}

impl OnboardingPending {
    pub const TOPIC: &'static str = "onboarding/pending";
    pub const TYPE: &'static str = "lamp_msgs/OnboardingPending";
    pub const VERSION: u32 = 1;

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Duration::from_millis(500))
            .lasting(Duration::from_millis(1500))
    }

    pub fn check(&self) -> Result<(), Invalid> {
        if self.invitations.len() > 1 {
            return Err(Invalid(
                "onboarding has at most one pending invitation".into(),
            ));
        }
        if self
            .invitations
            .iter()
            .any(|item| item.invitation == 0 || item.generation == 0 || item.attempt == 0)
        {
            return Err(Invalid(
                "onboarding identity components must be nonzero".into(),
            ));
        }
        Ok(())
    }
}

/// The setup walk's steps, by the names its file keeps each outcome under
/// (`steps` in `/var/lib/lampOS/voice/onboarding/progress.json`): the
/// voice's `Step::IDS`, and how lamp-update tells a walk under way (#3689).
/// The same nine since the 7071 image; not walk order.
pub const ONBOARDING_STEPS: [&str; 9] = [
    "where_you_sit",
    "who_you_are",
    "how_you_sound",
    "touch",
    "privacy",
    "giving_way",
    "room_sounds",
    "face_expressions",
    "preferences",
];

pub fn onboarding_topics() -> Vec<(&'static str, Declaration)> {
    vec![(OnboardingPending::TOPIC, OnboardingPending::declaration())]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_nonzero_privacy_invitation_is_valid() {
        let mut pending = OnboardingPending::default();
        assert!(pending.check().is_ok());
        let invitation = OnboardingInvitation {
            invitation: 81,
            generation: 2,
            attempt: 3,
            step: OnboardingStep::Privacy,
            ready: true,
        };
        pending.invitations.push(invitation.clone());
        assert!(pending.check().is_ok());
        let text = serde_json::to_string(&pending).unwrap();
        assert_eq!(
            serde_json::from_str::<OnboardingPending>(&text).unwrap(),
            pending
        );
        for field in ["invitation", "generation", "attempt"] {
            let mut value = serde_json::to_value(&pending).unwrap();
            value["invitations"][0][field] = 0.into();
            assert!(
                serde_json::from_value::<OnboardingPending>(value)
                    .unwrap()
                    .check()
                    .is_err()
            );
        }
        pending.invitations.push(invitation);
        assert!(pending.check().is_err());
        let newer: OnboardingPending =
            serde_json::from_str(&text.replace("privacy", "a_newer_step")).unwrap();
        assert_eq!(newer.invitations[0].step, OnboardingStep::Unknown);
        let giving: OnboardingPending =
            serde_json::from_str(&text.replace("privacy", "giving_way")).unwrap();
        assert_eq!(giving.invitations[0].step, OnboardingStep::GivingWay);
        assert!(newer.check().is_ok());
        let near: OnboardingPending =
            serde_json::from_str(&text.replace("privacy", "how_you_sound")).unwrap();
        assert_eq!(near.invitations[0].step, OnboardingStep::HowYouSound);
        assert!(near.check().is_ok());
        let giving: OnboardingPending =
            serde_json::from_str(&text.replace("privacy", "giving_way")).unwrap();
        assert_eq!(giving.invitations[0].step, OnboardingStep::GivingWay);
    }
}
