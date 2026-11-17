//! An ask for the lamp to do something itself at a time, again and again,
//! is a skill to make, never a reminder or a read now
//! (`LAMP_SKILL_RECURRING_TOLD`). On lamp-9adf on 17 Nov the owner's spoken
//! "Lamp, make a skill: every morning at nine, read me my new emails",
//! misheard without the word "skill", was answered with `service_read` (the
//! mail read now) and `repeat_reminder` (a daily 09:00 reminder whose text
//! was the words): a reminder only says its words, so nothing would ever
//! read the mail. Typed in the app, the same ask made the skill.
//!
//! With the switch, and only while `make_skill` is declared
//! (`LAMP_MAKE_SKILL`), the model is told so in words, in the three
//! functions' own words: `make_skill`'s request ([`REQUEST_TOLD`]),
//! `set_reminder` and `repeat_reminder` ([`REMINDER_TOLD`]), and
//! `service_read` ([`READ_TOLD`]). The model judges what was meant (the
//! owner's rule, 13 Nov): nothing here matches the person's words. Off,
//! every description is as it was.
use lamp_gemini::Tool;

/// Only the exact value `on`; off unless set.
pub const ENVIRONMENT: &str = "LAMP_SKILL_RECURRING_TOLD";

#[cfg(test)]
thread_local! {
    static HERE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Whether the switch is on in this process.
pub fn on() -> bool {
    #[cfg(test)]
    if let Some(on) = HERE.with(std::cell::Cell::get) {
        return on;
    }
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// [`on`] for this test's thread alone.
#[cfg(test)]
pub(crate) fn set_on_here(on: Option<bool>) {
    HERE.with(|here| here.set(on));
}

/// What `make_skill`'s `request` parameter adds: the ask whole, by its
/// meaning, however the words came out. On the parameter, not the
/// function's description, which with every switch on has no room left
/// within the link's 1,024 bytes.
pub const REQUEST_TOLD: &str = " When the person wants the lamp itself to do something at a \
     time or on a repeat (\"every morning at nine, read me my new emails\", \"every day at 3 PM \
     read my calendar\"), that is a skill: pass the ask here whole, and never set a reminder or \
     read it now instead. Speech to text may mishear \"skill\": judge the ask by what it means.";

/// What `set_reminder`'s and `repeat_reminder`'s descriptions add.
pub const REMINDER_TOLD: &str = " A reminder only tells the person its words at a time. When \
     they want the lamp itself to do something at a time or every day (read their mail or \
     calendar, change the light), call make_skill instead, never a reminder.";

/// What `service_read`'s description adds, first, beside its rule
/// (`crate::services::tools_told`), so it is never the part cut.
pub const READ_TOLD: &str = "This reads now. When the person wants it read later or on a \
     repeat (\"every morning at nine, read me my new emails\"), call make_skill instead.";

/// The reminders told, by name.
const REMINDERS: [&str; 2] = ["set_reminder", "repeat_reminder"];

/// The link's most for a description.
const MOST_DESCRIPTION_BYTES: usize = 1_024;

/// Each of `tools`' reminders told [`REMINDER_TOLD`] after its own words,
/// when the two still fit the link's most; one that would not is left as
/// it was.
pub fn tell_reminders(tools: &mut [Tool]) {
    for tool in tools
        .iter_mut()
        .filter(|tool| REMINDERS.contains(&tool.name.as_str()))
    {
        if !tool.description.contains(REMINDER_TOLD.trim())
            && tool.description.len() + REMINDER_TOLD.len() <= MOST_DESCRIPTION_BYTES
        {
            tool.description.push_str(REMINDER_TOLD);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(name: &str, description: &str) -> Tool {
        Tool {
            name: name.into(),
            description: description.into(),
            parameters: None,
            silent: false,
        }
    }

    #[test]
    fn the_reminders_alone_are_told_once_within_the_bound() {
        let mut tools = vec![
            tool("set_reminder", "Set one."),
            tool("repeat_reminder", "Repeat one."),
            tool("set_timer", "Time one."),
            tool("repeat_reminder", &"x".repeat(MOST_DESCRIPTION_BYTES - 10)),
        ];
        tell_reminders(&mut tools);
        tell_reminders(&mut tools);
        assert_eq!(tools[0].description, format!("Set one.{REMINDER_TOLD}"));
        assert_eq!(tools[1].description, format!("Repeat one.{REMINDER_TOLD}"));
        assert_eq!(tools[2].description, "Time one.");
        assert_eq!(tools[3].description.len(), MOST_DESCRIPTION_BYTES - 10);
    }

    /// The executive's own reminders, every way they are declared, still
    /// fit the link once told.
    #[test]
    fn the_executives_reminders_fit_once_told() {
        for every in [false, true] {
            let mut tools: Vec<Tool> = lamp_actions::schedule::catalog_with(every)
                .into_iter()
                .map(|function| tool(function.name, function.description))
                .collect();
            tell_reminders(&mut tools);
            for name in REMINDERS {
                let told = tools.iter().find(|tool| tool.name == name).unwrap();
                assert!(told.description.ends_with(REMINDER_TOLD), "{name}");
                assert!(told.description.len() <= MOST_DESCRIPTION_BYTES);
            }
        }
    }

    #[test]
    fn the_switch_is_on_with_on_alone() {
        set_on_here(None);
        // Unset in tests: off.
        assert!(!on());
        set_on_here(Some(true));
        assert!(on());
        set_on_here(None);
    }
}
