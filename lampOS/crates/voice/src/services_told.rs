//! What the owner linked in the app's Connectors, told to the voice's second
//! route beside the owner's own spoken line (`LAMP_SERVICES_TOLD`, #5523).
//!
//! A marketing tester linked a service in the app on 6 Nov and asked the
//! lamp about it: the lamp said that was the app's job and it didn't know.
//! The three functions were declared (`crate::services`), but nothing said
//! what was linked, and the character says not to pretend to finish tasks
//! it cannot reach, so the model answered from that and called nothing.
//! With the switch, a line admitted as the owner's is told, in one short
//! note, the services linked now by name and what the lamp can do with
//! them; every other line, a guest's, the room's, a held one or a text, is
//! told nothing, as `services_connected` tells them nothing (`OWNER_ONLY`).
//!
//! What is linked comes from `lamp-services` itself, `services_connected`
//! on its tools socket, asked off the line's path as a line begins
//! ([`crate::services::Linked`]): the note reads what was last said, never
//! waits. With nothing linked, or `lamp-services` not telling, nothing is
//! told: the model asks `services_connected`, as before.
//!
//! Off unless the voice's environment says exactly `on`, and only with the
//! services' folder (`LAMP_SERVICES=on`, `--services-dir`).

/// Where the second worker reads whether what is linked is told. `on` only.
pub const ENVIRONMENT: &str = "LAMP_SERVICES_TOLD";

/// Whether the switch `value` is on.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// [`on`] as the environment says.
pub fn on_from_environment() -> bool {
    on(std::env::var(ENVIRONMENT).ok().as_deref())
}

/// What the model is told of `names`, the services linked now.
pub fn note(names: &[String]) -> String {
    format!(
        "(For you to know, not to answer: the lamp's owner linked these services to you in the app's Connectors: {}. They are yours to use for the owner: read from one with service_read, and send, post or create with service_write, when they ask. Asked about one, use it, or say plainly what you can do with it; never say it is the app's job or that you don't know it.)",
        names.join(", ")
    )
}

/// The note for a line whose owner is `owner` (the owner's memory id when
/// the line is theirs, `crate::services::owner_of`), with `names` linked:
/// none for another's line, and none with nothing linked.
pub fn note_for(owner: Option<&str>, names: &[String]) -> Option<String> {
    owner?;
    (!names.is_empty()).then(|| note(names))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(said: &[&str]) -> Vec<String> {
        said.iter().map(|name| (*name).to_owned()).collect()
    }

    /// #5523: the owner's line is told what is linked, by name, and that
    /// the lamp uses it itself; nobody else's line is, and with nothing
    /// linked nothing is told.
    #[test]
    fn the_owners_line_is_told_what_is_linked_and_no_one_elses() {
        let linked = names(&["Notion", "Gmail"]);
        let told = note_for(Some("owner"), &linked).expect("the owner's line is told");
        assert!(told.contains("Notion, Gmail"), "{told}");
        assert!(told.contains("service_read"), "{told}");
        assert!(told.contains("never say it is the app's job"), "{told}");
        assert_eq!(note_for(None, &linked), None);
        assert_eq!(note_for(Some("owner"), &[]), None);
    }

    #[test]
    fn the_switch_is_on_only_for_on() {
        assert!(on(Some("on")));
        for off in [None, Some(""), Some("off"), Some("ON"), Some("1")] {
            assert!(!on(off), "{off:?}");
        }
    }
}
