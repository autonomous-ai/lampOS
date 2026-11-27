//! A Store install that finished, told to the voice on the lamp's bus
//! (`store/installed`, `lamp_msgs::Installed`): the product owner's ask that
//! an item's onboarding starts at its install, and the first builder's
//! ruling on #7814 (comment 7209938102). One event an install, for a Store
//! app ([`crate::apps`]) and a Store skill ([`crate::skills::install`]),
//! never for an update. It names the item, says the owner installed it
//! (never a person's name: `lamp-cloud` never reads one from the payload),
//! and carries the item's first-run line when it declares one. The voice
//! decides whether and when Lamp says it (`LAMP_INSTALL_FIRST_RUN`).
//!
//! Nothing here waits: the bus is a local socket write, and a lamp with no
//! bus, or a voice not listening, tells nobody and changes nothing else.
use lamp_msgs::installed::{APP, FirstRun, Installed, OWNER, SKILL};
use serde_json::Value;
use std::sync::Mutex;

/// The bus the events go on, set once at the start ([`switch`]).
static BUS: Mutex<Option<lamp_bus::Bus>> = Mutex::new(None);

/// The bus the events go on; none tells nobody.
pub fn switch(bus: Option<lamp_bus::Bus>) {
    if let Ok(mut kept) = BUS.lock() {
        *kept = bus;
    }
}

/// An app's install from its manifest (`lamp-skill.json`), finished at
/// `finished_us`: its name, title and first-run line as the package says
/// them. None when they are not an app's.
pub fn of_app(manifest: &Value, finished_us: u64) -> Option<Installed> {
    let name = manifest["name"].as_str()?;
    let title = manifest["title"].as_str().unwrap_or(name);
    let first_run = manifest["first_run"]["say"].as_str().map(str::to_owned);
    of(APP, name, title, first_run, finished_us)
}

/// A skill's install, finished at `finished_us`.
pub fn of_skill(
    name: &str,
    title: &str,
    first_run: Option<String>,
    finished_us: u64,
) -> Option<Installed> {
    of(SKILL, name, title, first_run, finished_us)
}

fn of(
    item: &str,
    name: &str,
    title: &str,
    first_run: Option<String>,
    finished_us: u64,
) -> Option<Installed> {
    let event = Installed {
        item: item.to_owned(),
        name: name.to_owned(),
        title: title.trim().to_owned(),
        by: OWNER.to_owned(),
        // A line that isn't right is left out, and the lamp says only the
        // name ready: the package's own reader refused it already.
        first_run: first_run
            .map(|say| FirstRun { say })
            .filter(|first_run| first_run.check().is_ok()),
        finished_us,
    };
    event.check().is_ok().then_some(event)
}

/// `event` on `store/installed`; the journal says what and whether it has a
/// first-run line, never the line itself.
pub fn tell(event: &Installed) {
    let Ok(kept) = BUS.lock() else {
        return;
    };
    let Some(bus) = kept.as_ref() else {
        return;
    };
    let told = bus
        .publisher_declared(
            Installed::TOPIC,
            &Installed::declaration(),
            crate::lamp::SOURCE,
        )
        .and_then(|mut out| {
            out.publish_json(lamp_ipc::monotonic_us(), event)
                .map(|_| ())
        });
    match told {
        Ok(()) => eprintln!(
            "lamp-cloud: the owner installed {} {}: told on {} (first-run line: {}) (#7814)",
            event.item,
            event.name,
            Installed::TOPIC,
            if event.declares_first_run() {
                "its own"
            } else {
                "none, its name ready"
            }
        ),
        Err(error) => eprintln!(
            "lamp-cloud: the owner installed {} {}, not told on {}: {error} (#7814)",
            event.item,
            event.name,
            Installed::TOPIC
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_apps_install_names_it_says_the_owner_and_carries_its_line() {
        let event = of_app(
            &json!({"name": "gym-coach", "title": "Gym Coach",
                "first_run": {"say": "Gym Coach is ready. Want to set your first plan?"}}),
            7,
        )
        .unwrap();
        assert_eq!(
            (
                event.item.as_str(),
                event.name.as_str(),
                event.title.as_str(),
                event.by.as_str()
            ),
            ("app", "gym-coach", "Gym Coach", "owner")
        );
        assert_eq!(
            event.line(),
            "Gym Coach is ready. Want to set your first plan?"
        );
        let plain = of_app(&json!({"name": "gym-coach", "title": "Gym Coach"}), 7).unwrap();
        assert!(!plain.declares_first_run());
        assert_eq!(plain.line(), "Gym Coach is ready.");
        // A line the reader would refuse is left out, never said.
        let bad = of_app(
            &json!({"name": "gym-coach", "title": "Gym Coach", "first_run": {"say": "A? B?"}}),
            7,
        )
        .unwrap();
        assert_eq!(bad.line(), "Gym Coach is ready.");
        assert!(of_app(&json!({"title": "No name"}), 7).is_none());
    }

    #[test]
    fn a_skills_install_is_the_same_event() {
        let event = of_skill("gym-tips", "Gym tips", Some("Want a tip now?".into()), 9).unwrap();
        assert_eq!(event.item, "skill");
        assert_eq!(event.line(), "Want a tip now?");
        assert!(of_skill("Not A Name", "x", None, 9).is_none());
    }

    #[test]
    fn an_install_goes_on_the_bus_for_the_voice() {
        let root = tempfile::tempdir().unwrap();
        let bus = lamp_bus::Bus::new(root.path()).unwrap();
        let mut voice = bus
            .subscribe_typed(Installed::TOPIC, "voice", Installed::TYPE)
            .unwrap();
        switch(Some(bus));
        let event = of_skill("gym-tips", "Gym tips", None, 9).unwrap();
        let mut heard = None;
        for _ in 0..50 {
            tell(&event);
            if let Some(message) = voice.recv(std::time::Duration::from_millis(100)).unwrap() {
                heard = Some(message);
                break;
            }
        }
        switch(None);
        let message = heard.expect("the voice hears the install");
        assert_eq!(message.header.source, crate::lamp::SOURCE);
        assert_eq!(message.parse::<Installed>().unwrap(), event);
    }
}
