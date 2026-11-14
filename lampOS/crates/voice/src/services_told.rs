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
//! The Live route is told the same note beside the owner's open line, once
//! a session for each set of names ([`LiveTold`]): its session keeps what
//! it was told, so a line that says the same again is not told again. As a
//! session keeps it, a guest later in it would hear the names, so the Live
//! route is told only in the private room (`LAMP_PRIVATE_ROOM`, every line
//! the owner's); and a session whose services are all since unlinked is
//! told so once.
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

/// What the model is told of `names`, the services linked now, and where
/// one not among them is linked: the words `lamp-services` answers a call
/// to an unlinked service with (`NOT_LINKED`, #6847), for a service of the
/// lamp's catalogue ([`linkable`]) only; for any other, the lamp can't link
/// it yet (`CANT_LINK`), as `lamp-services` answers (Thirteen's P2 on
/// #7013: "How do I link my Outlook?" must not be sent to a screen with no
/// Outlook). Without them the model, asked of a service missing from the
/// list, never calls `service_read`, so never hears where to link it
/// (#5867).
pub fn note(names: &[String]) -> String {
    format!(
        "(For you to know, not to answer: the lamp's owner linked these services to you in the app's Linked accounts: {}. They are yours to use for the owner: read from one with service_read, and send, post or create with service_write, when they ask. Asked about one, use it, or say plainly what you can do with it; never say it is the app's job or that you don't know it. A service the lamp can link ({}) but not in this list {} Any other service the lamp can't link yet: say so. {})",
        names.join(", "),
        linkable(),
        lamp_services::serve::NOT_LINKED,
        lamp_services::serve::CANT_LINK
    )
}

/// The services the lamp can link, by the names a person hears, each once,
/// in the catalogue's order: one service under two links ("Gmail with
/// Google sign-in", Figma's two codes) is said by its first name.
pub fn linkable() -> String {
    let mut said: Vec<&str> = Vec::new();
    for service in lamp_services::catalog::SERVICES {
        if !said.iter().any(|name| service.name.starts_with(name)) {
            said.push(service.name);
        }
    }
    said.join(", ")
}

/// The note for a line whose owner is `owner` (the owner's memory id when
/// the line is theirs, `crate::services::owner_of`), with `names` linked:
/// none for another's line, and none with nothing linked.
pub fn note_for(owner: Option<&str>, names: &[String]) -> Option<String> {
    owner?;
    (!names.is_empty()).then(|| note(names))
}

/// What the Live route is told when everything it was told of in this
/// session has since been unlinked (Jarvis's P3 on #5598), and where the
/// ones the lamp can link are, in `CANT_LINK`'s words (#5867), which never
/// say every service can be linked (Thirteen's P2 on #7013).
pub const UNLINKED_NOTE: &str = "(For you to know, not to answer: nothing is linked to you in the app's Linked accounts now. The services you were told of earlier in this conversation are no longer linked. The services it can link are in the app: open this lamp, then Settings, then Linked accounts.)";

/// What the Live route told of what is linked: once a session for each
/// set of names, so a session is told again only when a link is made or
/// removed, or in a fresh session.
#[derive(Debug, Default)]
pub struct LiveTold {
    told: Option<(u64, Vec<String>)>,
}

impl LiveTold {
    /// The note due beside a line of `session` whose owner is `owner`, with
    /// `names` linked ([`note_for`]): none when this session was told these
    /// names already, and none outside the private room (`private_room`,
    /// `crate::person::private_room`): a Live session keeps what it was
    /// told, so a guest later in it would hear the names (Jarvis's P2 on
    /// #5598). Everything told in this session since unlinked:
    /// [`UNLINKED_NOTE`], once.
    pub fn due(
        &self,
        session: u64,
        owner: Option<&str>,
        names: &[String],
        private_room: bool,
    ) -> Option<String> {
        if !private_room {
            return None;
        }
        let said = self
            .told
            .as_ref()
            .filter(|(told, _)| *told == session)
            .map(|(_, said)| said.as_slice());
        if said == Some(names) {
            return None;
        }
        if names.is_empty() {
            owner?;
            return said
                .is_some_and(|said| !said.is_empty())
                .then(|| UNLINKED_NOTE.to_owned());
        }
        note_for(owner, names)
    }

    /// `names` told in `session`.
    pub fn told(&mut self, session: u64, names: Vec<String>) {
        self.told = Some((session, names));
    }
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

    /// #5523 on the Live route: the owner's line is told once a session; a
    /// link made since, or a fresh session, is told again; another's line
    /// never is.
    #[test]
    fn services_told_the_live_route_once_a_session_for_each_set_of_names() {
        let mut live = LiveTold::default();
        let notion = names(&["Notion"]);
        assert_eq!(live.due(1, None, &notion, true), None);
        let told = live
            .due(1, Some("owner"), &notion, true)
            .expect("the owner's line is told");
        assert_eq!(told, note(&notion));
        live.told(1, notion.clone());
        assert_eq!(live.due(1, Some("owner"), &notion, true), None);
        let both = names(&["Notion", "Gmail"]);
        assert_eq!(live.due(1, Some("owner"), &both, true), Some(note(&both)));
        assert_eq!(
            live.due(2, Some("owner"), &notion, true),
            Some(note(&notion))
        );
        assert_eq!(live.due(2, Some("owner"), &[], true), None);
    }

    /// Jarvis's P2 on #5598: a Live session keeps what it was told, so a
    /// guest later in it could hear the names. Only the private room, where
    /// every line is the owner's (the PM's ruling (a) on #5523), tells the
    /// Live route; with it off, the Live route is told nothing.
    #[test]
    fn the_live_route_is_told_only_in_the_private_room() {
        let live = LiveTold::default();
        let notion = names(&["Notion"]);
        assert_eq!(live.due(1, Some("owner"), &notion, false), None);
        assert_eq!(
            live.due(1, Some("owner"), &notion, true),
            Some(note(&notion))
        );
    }

    /// Jarvis's P3 on #5598: once everything told in a session is unlinked,
    /// the session is told so once, not left with the old names; a session
    /// told nothing is told nothing.
    #[test]
    fn a_live_session_is_told_once_when_everything_is_unlinked() {
        let mut live = LiveTold::default();
        assert_eq!(live.due(1, Some("owner"), &[], true), None);
        live.told(1, names(&["Notion"]));
        assert_eq!(
            live.due(1, Some("owner"), &[], true),
            Some(UNLINKED_NOTE.to_owned())
        );
        assert_eq!(live.due(1, None, &[], true), None);
        live.told(1, Vec::new());
        assert_eq!(live.due(1, Some("owner"), &[], true), None);
        assert_eq!(live.due(2, Some("owner"), &[], true), None);
    }

    /// #5867: asked of a service not in the list, the model never calls
    /// `service_read`, so the told note and [`UNLINKED_NOTE`] say where one
    /// is linked themselves, in the words `lamp-services` uses
    /// (`NOT_LINKED`), and never the old Connectors section.
    #[test]
    fn the_told_notes_say_where_a_service_is_linked() {
        let not_linked = lamp_services::serve::NOT_LINKED;
        let (_, path) = not_linked
            .split_once("in the app: ")
            .expect("NOT_LINKED says where in the app");
        assert_eq!(path, "open this lamp, then Settings, then Linked accounts.");
        let cant_link = lamp_services::serve::CANT_LINK;
        assert!(
            cant_link.ends_with(&format!("in the app: {path}")),
            "{cant_link}"
        );
        let told = note(&names(&["Gmail"]));
        assert!(
            told.ends_with(&format!(
                "A service the lamp can link ({}) but not in this list {not_linked} Any other service the lamp can't link yet: say so. {cant_link})",
                linkable()
            )),
            "{told}"
        );
        assert!(
            UNLINKED_NOTE.ends_with(&format!("{cant_link})")),
            "{UNLINKED_NOTE}"
        );
        for told in [told.as_str(), UNLINKED_NOTE] {
            assert!(told.starts_with("(For you to know, not to answer: "));
            assert!(!told.contains("Connectors"), "{told}");
        }
    }

    /// Thirteen's P2 on #7013: a service outside the lamp's catalogue, such
    /// as Outlook, is never said to be linkable; each one in it is named
    /// once, by the name a person hears.
    #[test]
    fn only_the_services_the_lamp_can_link_are_said_to_be_linkable() {
        let said = linkable();
        // As services.md quotes it.
        assert_eq!(
            said,
            "Gmail, Google Calendar, Google Drive, Figma, GitHub, Linear, Asana, Ahrefs, Facebook Page, Slack, monday.com, HubSpot, Amplitude, Notion"
        );
        let listed: Vec<&str> = said.split(", ").collect();
        assert_eq!(listed.first(), Some(&"Gmail"), "{said}");
        for name in ["Google Calendar", "Slack", "Notion", "Figma"] {
            assert_eq!(
                listed.iter().filter(|one| **one == name).count(),
                1,
                "{name}: {said}"
            );
        }
        assert!(!said.contains("Google sign-in"), "{said}");
        assert!(!said.contains("Outlook"), "{said}");
        assert!(note(&names(&["Gmail"])).contains("can't link yet"));
    }

    #[test]
    fn the_switch_is_on_only_for_on() {
        assert!(on(Some("on")));
        for off in [None, Some(""), Some("off"), Some("ON"), Some("1")] {
            assert!(!on(off), "{off:?}");
        }
    }
}
