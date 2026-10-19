//! The voice's processes, named in one place (#1071).
//!
//! `lamp-voice` starts itself with one of [`Command`]'s words, and a worker
//! with one of [`WorkerRole`]'s. The records guard reads the same lists to
//! know a live voice process when it sees one. Before this, the guard kept
//! its own copy: a role the voice added (`second`, and before it
//! `schedule-consumer`) stopped every cleanup until someone read the journal.
//!
//! A new role goes here first. The voice refuses to start a role that is not
//! here, so a role that is missing fails the voice's own tests, never first
//! the guard on a lamp.

/// The first word after the voice's executable, for every process that runs
/// as the voice: its service, its directed sessions and checks, what the
/// service starts beside itself, and the read-only reports run by hand on a
/// lamp. `lamp-voice` refuses any other first word, so a command added to
/// the voice and not here fails before it can run beside the service.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    Serve,
    Directed,
    DirectedFixture,
    CameraInspect,
    ProviderCheck,
    ScheduleConsumer,
    Worker,
    ShortTurns,
    SttCheck,
    Marked,
    TurnTimes,
    /// The stall lines of tonight (`lamp-voice stalls`, #1178): read only.
    Stalls,
    /// The second route's candidate services timed from the lamp
    /// (`lamp-voice probe-cascade`): times only, run by hand.
    ProbeCascade,
}

impl Command {
    pub const ALL: [Command; 13] = [
        Command::Serve,
        Command::Directed,
        Command::DirectedFixture,
        Command::CameraInspect,
        Command::ProviderCheck,
        Command::ScheduleConsumer,
        Command::Worker,
        Command::ShortTurns,
        Command::SttCheck,
        Command::Marked,
        Command::TurnTimes,
        Command::Stalls,
        Command::ProbeCascade,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Command::Serve => "serve",
            Command::Directed => "directed",
            Command::DirectedFixture => "directed-fixture",
            Command::CameraInspect => "camera-inspect",
            Command::ProviderCheck => "provider-check",
            Command::ScheduleConsumer => "schedule-consumer",
            Command::Worker => "worker",
            Command::ShortTurns => "short-turns",
            Command::SttCheck => "stt-check",
            Command::Marked => "marked",
            Command::TurnTimes => "turn-times",
            Command::Stalls => "stalls",
            Command::ProbeCascade => "probe-cascade",
        }
    }

    pub fn parse(word: &str) -> Option<Command> {
        Command::ALL
            .into_iter()
            .find(|command| command.as_str() == word)
    }
}

/// The role after `worker`: which worker the voice started.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WorkerRole {
    Capture,
    Speaker,
    Provider,
    Second,
    Camera,
    Ring,
    Privacy,
    Probe,
}

impl WorkerRole {
    pub const ALL: [WorkerRole; 8] = [
        WorkerRole::Capture,
        WorkerRole::Speaker,
        WorkerRole::Provider,
        WorkerRole::Second,
        WorkerRole::Camera,
        WorkerRole::Ring,
        WorkerRole::Privacy,
        WorkerRole::Probe,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            WorkerRole::Capture => "capture",
            WorkerRole::Speaker => "speaker",
            WorkerRole::Provider => "provider",
            WorkerRole::Second => "second",
            WorkerRole::Camera => "camera",
            WorkerRole::Ring => "ring",
            WorkerRole::Privacy => "privacy",
            WorkerRole::Probe => "probe",
        }
    }

    pub fn parse(word: &str) -> Option<WorkerRole> {
        WorkerRole::ALL
            .into_iter()
            .find(|role| role.as_str() == word)
    }
}

/// Programs the voice starts that are not itself, by file name: they run in
/// the voice's service too.
pub const HELPERS: [&str; 1] = ["lamp-turn-end"];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_name_is_parsed_back_and_none_is_given_twice() {
        let mut seen = HashSet::new();
        for command in Command::ALL {
            assert_eq!(Command::parse(command.as_str()), Some(command));
            assert!(seen.insert(command.as_str()), "{}", command.as_str());
        }
        let mut seen = HashSet::new();
        for role in WorkerRole::ALL {
            assert_eq!(WorkerRole::parse(role.as_str()), Some(role));
            assert!(seen.insert(role.as_str()), "{}", role.as_str());
        }
        assert_eq!(WorkerRole::parse("unknown"), None);
        assert_eq!(Command::parse("Serve"), None);
    }
}
