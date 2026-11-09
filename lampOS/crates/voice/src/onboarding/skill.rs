//! The installed onboarding skill's data, checked by Voice and Skills alike.
//! Loaded once before the service opens audio; no embedded production fallback.
use super::Step;
use crate::skill_words::{checked_words, fill};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::OpenOptions,
    io::Read,
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

/// A skill manifest is small and regular. Never wait for a FIFO or follow its link.
pub const MOST_MANIFEST_BYTES: u64 = 65_536;

// Consent remains a mechanism even when the rest of a note is editable data.
const FACE_CONSENT: &str = "only after a clear yes call remember_my_face";
/// Room sounds and face expressions alike.
const ROOM_SOUNDS_CONSENT: &str = "done only after a clear yes";

/// "Preferences"' second question, the walk's own once the quiet hours are
/// heard: the model, its answer to them given, often asked nothing more
/// (lamp-29e7's 8545 re-test). Fixed words, not the file's: an older
/// lamp-skills refuses a new key.
pub(crate) const PREFERENCES_HELLO: &str =
    "(Say only this, nothing before or after it: \"May I say hello when you sit down?\")";

/// "Face expressions"' lines, the walk's own once it hears a plain yes or no
/// (#3769): the words the model is told to say after its `setup_step`.
pub(crate) const FACE_EXPRESSIONS_YES: &str =
    "(Say only this, nothing before or after it: \"Got it, I'll notice how people look.\")";
pub(crate) const FACE_EXPRESSIONS_NO: &str =
    "(Say only this, nothing before or after it: \"Okay, I won't.\")";

/// "Preferences"' done line, the walk's own once the step is done: the
/// walk's last words, so they also say where the app's buttons and gestures
/// guide is (#1711 item 1).
pub(crate) const PREFERENCES_DONE: &str = "(Say only this, nothing before or after it: \"All done! I can set timers and reminders, answer questions, and change my light. Just say 'Hey Lamp'. You can add skills any time in the Autonomous app.\")";

/// Said once after "how you sound"'s call is accepted (#4080, the lead,
/// 9 Nov): one try of the voice route, never asked again; the step is done
/// on the call alone, and the walk waits for it at most
/// `TRY_IT_US` ([`super::Onboarding::tick`]).
pub(crate) const NEAR_TRY: &str = "(Say only this, nothing before or after it: \"Now try it: say 'Hey Lamp, turn your light blue.'\")";

/// "Who you are" ended with the name only on a lamp that keeps faces
/// (#5139): what is remembered waits for a face, so the person is told, and
/// the face is asked for once more later ([`FACE_AGAIN`]). In code, not the
/// file: an older build refuses a new key.
pub(crate) const NAME_ONLY: &str = "(Say only this, nothing before or after it: \"Got your name. I'll remember things for you once I've seen your face.\")";

/// The face asked for once more, after the walk, when "who you are" ended
/// with the name only (#5139). The name is known by then; a yes and the
/// save end it, a no or silence leaves it, and it is never asked again.
pub(crate) const FACE_AGAIN: &str = "(Nobody has spoken. Ask exactly this: \"One more thing: may I remember your face now, so I can remember things for you?\" Only after a clear yes call remember_my_face with the name they told you; if you don't know it, ask it first. Never use remember for this. If they would rather not, say only: \"That's fine.\")";

/// [`PREFERENCES_DONE`] that also names a few things to try first, said
/// once per setup with `LAMP_ONBOARDING_FIRST_TRIES` on (#5316, the first
/// customers' stage 2 on #5296): the introduction ends on what to do next,
/// a few everyday things, never a list of features.
pub(crate) const PREFERENCES_DONE_TRIES: &str = "(Say only this, nothing before or after it: \"All done! I can set timers and reminders, answer questions, and change my light. Just say 'Hey Lamp'. You can add skills any time in the Autonomous app. If you'd like somewhere to start, ask me for a timer, the weather, or anything you're curious about. You can also add a skill from the Skill Store in the app, or just ask me to make one.\")";

/// [`PREFERENCES_DONE`] ending with the one sentence about Harness
/// (`harness_line::SENTENCE`), inside the same quoted line: one closing
/// turn (#5488, with `LAMP_ONBOARDING_HARNESS_LINE` on and Harness not
/// signed in).
pub(crate) const PREFERENCES_DONE_HARNESS: &str = "(Say only this, nothing before or after it: \"All done! I can set timers and reminders, answer questions, and change my light. Just say 'Hey Lamp'. You can add skills any time in the Autonomous app. If you work on a computer, I can take on tasks for you: open Harness in the app to connect.\")";

/// [`PREFERENCES_DONE_TRIES`] ending with the same sentence, with
/// `LAMP_ONBOARDING_FIRST_TRIES` on too: still one closing turn, so a new
/// owner never hears two speeches at the end (the PM on #5488).
pub(crate) const PREFERENCES_DONE_TRIES_HARNESS: &str = "(Say only this, nothing before or after it: \"All done! I can set timers and reminders, answer questions, and change my light. Just say 'Hey Lamp'. You can add skills any time in the Autonomous app. If you'd like somewhere to start, ask me for a timer, the weather, or anything you're curious about. You can also add a skill from the Skill Store in the app, or just ask me to make one. If you work on a computer, I can take on tasks for you: open Harness in the app to connect.\")";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub(crate) order: Vec<Step>,
    pub(crate) reoffer_after_s: u64,
    /// A step left `later` (nobody tried it) is offered again this long
    /// after the walk ends, the same sitting, then once more after
    /// `reoffer_after_s` (#3996). Optional, as #3781's lines are.
    #[serde(default = "later_after_s")]
    pub(crate) later_after_s: u64,
    pub(crate) steps: BTreeMap<Step, StepWords>,
    pub(crate) words: Words,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StepWords {
    pub(crate) first: String,
    pub(crate) reoffer: String,
    /// Said by the runner when the model reports the step skipped: only a
    /// step the model can only skip, and never privacy, may have one
    /// (#3769). Optional, as #3781's lines are: an older updater's file has
    /// none, and then the skip is said by nobody.
    #[serde(default)]
    pub(crate) skipped: Option<String>,
    /// Said first when a `later` step is offered again, naming it, so a
    /// person half an hour on knows why the lamp asks (#3996, the first
    /// builder's P2 on #4012). Optional: without one, only `reoffer`.
    /// "Where you sit" is never `later`: its line is said in place of its
    /// `reoffer` on a later visit ([`Definition::visit_reoffer`]).
    #[serde(default)]
    pub(crate) later_reoffer: Option<String>,
    pub(crate) nudge: Option<Nudge>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Nudge {
    pub(crate) words: String,
    pub(crate) after_answers: u8,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Words {
    pub(crate) sit_no_face: String,
    pub(crate) sit_closer: String,
    pub(crate) sit_turn: String,
    pub(crate) sit_done: String,
    pub(crate) sit_failed: String,
    pub(crate) who_again: String,
    pub(crate) touch_remind: String,
    /// #3769's lines are optional: an older updater's `install` writes its
    /// own built-in onboarding file, without them, and the walk must still
    /// run. Without one, the walk says what it said before (Reviewer-two's
    /// P2 on #3781).
    #[serde(default)]
    pub(crate) touch_done: Option<String>,
    #[serde(default)]
    pub(crate) touch_failed: Option<String>,
    pub(crate) privacy_done: String,
    pub(crate) privacy_done_camera: String,
    pub(crate) privacy_failed: String,
    pub(crate) near_again: String,
    /// #3702's line after a call not caught; optional for the same reason
    /// as #3769's. Without it, `near_again` is said.
    #[serde(default)]
    pub(crate) near_missed: Option<String>,
    pub(crate) near_done: String,
    pub(crate) near_failed: String,
    pub(crate) sentence_first: String,
    pub(crate) sentence_louder: String,
    pub(crate) sentence_closer: String,
    pub(crate) sentence_again: String,
    pub(crate) sentence_done: String,
    pub(crate) sentence_failed: String,
    #[serde(default)]
    pub(crate) give_again: Option<String>,
    /// #3702's lines, optional as #3769's are: said at once when the body
    /// first feels a push in a try, and once when it feels none a few
    /// seconds after the invitation.
    #[serde(default)]
    pub(crate) give_felt: Option<String>,
    #[serde(default)]
    pub(crate) give_prompt: Option<String>,
    /// Said each time a push is let go before the head moved enough.
    #[serde(default)]
    pub(crate) give_more: Option<String>,
    /// Said when a step ends with no attempt (`later`, #3996).
    #[serde(default)]
    pub(crate) later: Option<String>,
    pub(crate) give_done: String,
    pub(crate) give_failed: String,
    pub(crate) who_confirm: String,
}

/// [`Definition::later_after_s`] without one in the file: 30 minutes.
fn later_after_s() -> u64 {
    1_800
}

/// What a step that ends with no attempt says without a line in the file.
pub(crate) const LATER: &str =
    "(Say only this, nothing before or after it: \"No problem, we'll try that one later.\")";

/// What a step says when its last `later` re-offer ends with no attempt:
/// it is never offered again, so no promise of later. In code, not the
/// file: a new key would be refused by an older build reading it.
pub(crate) const LAST_LATER: &str = "(Say only this, nothing before or after it: \"That's fine, you can redo the setup later. Just ask me to redo the setup.\")";

impl Definition {
    /// The line said when a step ends `later` (#3996).
    pub(crate) fn later_line(&self) -> String {
        self.words.later.clone().unwrap_or_else(|| LATER.to_owned())
    }

    /// Words for all nine mechanisms; the walk is the skill's `order`, where
    /// you sit and who you are among some of them, each once (#5499). Data
    /// cannot add an actuator, relax a physical check, or remove the privacy
    /// flow's priority.
    pub fn checked(self) -> Result<Self, String> {
        let distinct: BTreeSet<_> = self.order.iter().copied().collect();
        if distinct.len() != self.order.len()
            || !distinct.contains(&Step::WhereYouSit)
            || !distinct.contains(&Step::WhoYouAre)
            || self.steps.len() != Step::IDS.len()
        {
            return Err(
                "[onboarding] steps must name all nine steps, and order where_you_sit, who_you_are and others once each"
                    .into(),
            );
        }
        if !(1..=31_536_000).contains(&self.reoffer_after_s) {
            return Err("[onboarding] reoffer_after_s is 1 to 31536000".into());
        }
        if !(1..=31_536_000).contains(&self.later_after_s) {
            return Err("[onboarding] later_after_s is 1 to 31536000".into());
        }
        for (step, words) in &self.steps {
            let slots: &[&str] = if *step == Step::Privacy {
                &["location"]
            } else {
                &[]
            };
            checked_words(
                "onboarding step",
                &[
                    ("first", &words.first, slots),
                    ("reoffer", &words.reoffer, slots),
                ],
            )?;
            let consent = match step {
                Step::WhoYouAre => Some(FACE_CONSENT),
                Step::RoomSounds | Step::FaceExpressions => Some(ROOM_SOUNDS_CONSENT),
                _ => None,
            };
            if consent.is_some_and(|clause| {
                [&words.first, &words.reoffer]
                    .iter()
                    .any(|note| !note.contains(clause))
            }) {
                return Err(
                    "[onboarding] first and reoffer notes must retain their clear-yes consent clause"
                        .into(),
                );
            }
            // A verified hardware location is at most 160 Unicode characters.
            if *step == Step::Privacy
                && [&words.first, &words.reoffer].iter().any(|line| {
                    line.len() - "{location}".len() + 160 * 4 > lamp_gemini::wire::MAX_NOTE_BYTES
                })
            {
                return Err("[onboarding] privacy words plus the location exceed one note".into());
            }
            if words.skipped.is_some() && !step.says_its_skip() {
                return Err(
                    "[onboarding] only where_you_sit, how_you_sound, touch and giving_way may have a skipped line"
                        .into(),
                );
            }
            if let Some(line) = &words.later_reoffer {
                checked_words("onboarding step", &[("later_reoffer", line, &[])])?;
            }
            if let Some(skipped) = &words.skipped {
                checked_words("onboarding step", &[("skipped", skipped, &[])])?;
            }
            if let Some(nudge) = &words.nudge {
                if !matches!(step, Step::WhoYouAre | Step::Preferences)
                    || !(1..=8).contains(&nudge.after_answers)
                {
                    return Err(
                        "[onboarding] a nudge is who_you_are or preferences after 1 to 8 answers"
                            .into(),
                    );
                }
                checked_words("onboarding nudge", &[("words", &nudge.words, &[])])?;
            }
        }
        checked_words(
            "onboarding words",
            &[
                ("sit_no_face", &self.words.sit_no_face, &[]),
                ("sit_closer", &self.words.sit_closer, &[]),
                ("sit_turn", &self.words.sit_turn, &[]),
                ("sit_done", &self.words.sit_done, &[]),
                ("sit_failed", &self.words.sit_failed, &[]),
                ("who_again", &self.words.who_again, &[]),
                ("touch_remind", &self.words.touch_remind, &[]),
                ("privacy_done", &self.words.privacy_done, &[]),
                ("privacy_done_camera", &self.words.privacy_done_camera, &[]),
                ("privacy_failed", &self.words.privacy_failed, &[]),
                ("near_again", &self.words.near_again, &[]),
                ("near_done", &self.words.near_done, &[]),
                ("near_failed", &self.words.near_failed, &[]),
                ("sentence_first", &self.words.sentence_first, &[]),
                ("sentence_louder", &self.words.sentence_louder, &[]),
                ("sentence_closer", &self.words.sentence_closer, &[]),
                ("sentence_again", &self.words.sentence_again, &[]),
                ("sentence_done", &self.words.sentence_done, &[]),
                ("sentence_failed", &self.words.sentence_failed, &[]),
                ("give_done", &self.words.give_done, &[]),
                ("give_failed", &self.words.give_failed, &[]),
                ("who_confirm", &self.words.who_confirm, &["name"]),
            ],
        )?;
        for (name, line) in [
            ("touch_done", &self.words.touch_done),
            ("touch_failed", &self.words.touch_failed),
            ("give_again", &self.words.give_again),
            ("give_felt", &self.words.give_felt),
            ("give_prompt", &self.words.give_prompt),
            ("give_more", &self.words.give_more),
            ("near_missed", &self.words.near_missed),
            ("later", &self.words.later),
        ] {
            if let Some(line) = line {
                checked_words("onboarding words", &[(name, line, &[])])?;
            }
        }
        Ok(self)
    }

    pub(crate) fn note(&self, step: Step, again: bool) -> &str {
        let words = &self.steps[&step];
        if again { &words.reoffer } else { &words.first }
    }

    pub(crate) fn later_reoffer(&self, step: Step) -> Option<&str> {
        self.steps[&step].later_reoffer.as_deref()
    }

    /// The note a kept step is offered again with on a later visit: its
    /// `reoffer`. "Where you sit"'s `reoffer` is also its short line within
    /// a walk (#4095), so a later visit says its `later_reoffer` instead, a
    /// lead-in as touch's "One more thing: …" (jarvis's P3 on #4095). A
    /// new key would be refused by an older build reading this file.
    pub(crate) fn visit_reoffer(&self, step: Step) -> &str {
        match self.later_reoffer(step) {
            Some(line) if step == Step::WhereYouSit => line,
            _ => self.note(step, true),
        }
    }

    pub(crate) fn skipped(&self, step: Step) -> Option<&str> {
        self.steps[&step].skipped.as_deref()
    }

    pub(crate) fn nudge(&self, step: Step) -> Option<(&str, u8)> {
        self.steps[&step]
            .nudge
            .as_ref()
            .map(|nudge| (nudge.words.as_str(), nudge.after_answers))
    }

    pub(crate) fn privacy_invitation(&self, again: bool, location: &str) -> String {
        fill(self.note(Step::Privacy, again), &[("location", location)])
    }

    pub(crate) fn who_confirm(&self, name: &str) -> Option<String> {
        // Name recognition has no byte limit. Refuse an overlong confirmation
        // rather than truncate the name or send a note the provider cannot take.
        let size = self.words.who_confirm.len() - "{name}".len() + name.len();
        (size <= lamp_gemini::wire::MAX_NOTE_BYTES)
            .then(|| fill(&self.words.who_confirm, &[("name", name)]))
    }
}

/// Read only the data section of a core onboarding manifest. Other skill fields
/// have the full install-time validator in Skills; both use `Definition::checked`.
pub fn parse(text: &str) -> Result<Definition, String> {
    if text.len() as u64 > MOST_MANIFEST_BYTES {
        return Err("onboarding skill manifest exceeds 65536 bytes".into());
    }
    let mut file: toml::Table =
        toml::from_str(text).map_err(|_| "invalid onboarding skill TOML")?;
    let granted = file
        .get("permissions")
        .and_then(toml::Value::as_table)
        .and_then(|permissions| permissions.get("core"))
        .and_then(toml::Value::as_array)
        .is_some_and(|grants| {
            grants
                .iter()
                .any(|grant| grant.as_str() == Some("onboarding"))
        });
    if file.get("kind").and_then(toml::Value::as_str) != Some("core")
        || !granted
        || file
            .get("enabled")
            .is_some_and(|value| value.as_bool() != Some(true))
    {
        return Err("onboarding skill must be enabled, core and granted onboarding".into());
    }
    let found = file
        .remove("onboarding")
        .ok_or("onboarding skill has no [onboarding] data")?;
    let definition: Definition = found
        .try_into()
        .map_err(|_| "invalid onboarding skill data")?;
    definition.checked()
}

pub fn read_installed(dir: &Path) -> Result<Definition, String> {
    let path = dir.join("onboarding/skill.toml");
    let file = OpenOptions::new()
        .read(true)
        .custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32)
        .open(path)
        .map_err(|_| "onboarding skill cannot be opened")?;
    let metadata = file
        .metadata()
        .map_err(|_| "onboarding skill cannot be inspected")?;
    if !metadata.is_file() || metadata.len() > MOST_MANIFEST_BYTES {
        return Err("onboarding skill must be a regular file of at most 65536 bytes".into());
    }
    let mut text = String::new();
    file.take(MOST_MANIFEST_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|_| "onboarding skill cannot be read")?;
    parse(&text)
}

/// Tests load the shipped data explicitly; production never embeds it.
/// The runner's tests walk all nine steps in their order before #5499 (the
/// steps' declared order), so each stays tested while out of the walk.
#[cfg(test)]
pub(crate) fn fixture() -> std::sync::Arc<Definition> {
    let mut definition = parse(include_str!("../../../../skills/onboarding/skill.toml")).unwrap();
    definition.order = Step::IDS
        .iter()
        .filter_map(|name| Step::named(name))
        .collect();
    std::sync::Arc::new(definition.checked().unwrap())
}

#[cfg(test)]
pub(crate) mod tests;
