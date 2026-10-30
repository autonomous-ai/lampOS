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
    /// How much of a chat request DeepSeek's prompt cache holds
    /// (`lamp-voice probe-cache`, #1712): counts only, run by hand.
    ProbeCache,
    /// Which function DeepSeek calls for each light-and-volume follow-up
    /// line (`lamp-voice probe-tools`, #1712): counts only, run by hand.
    ProbeTools,
    /// What the second route's chat says to a cut-in told its note
    /// (`lamp-voice probe-cut-in`, #3997): counts only, run by hand.
    ProbeCutIn,
}

impl Command {
    pub const ALL: [Command; 16] = [
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
        Command::ProbeCache,
        Command::ProbeTools,
        Command::ProbeCutIn,
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
            Command::ProbeCache => "probe-cache",
            Command::ProbeTools => "probe-tools",
            Command::ProbeCutIn => "probe-cut-in",
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

    /// What the worker runs at under the voice's real-time RR 20 (#1608):
    /// it takes this on its first thread, before any other starts.
    pub const fn scheduling(self) -> Scheduling {
        match self {
            // The sound in and out: the audio path the RR 20 is for.
            WorkerRole::Capture | WorkerRole::Speaker => Scheduling::KeepsVoices,
            // Light work whose timing is seen, so above the lamp's ordinary
            // parts but below the voice's own threads.
            WorkerRole::Ring => Scheduling::Realtime(RING_REALTIME_PRIORITY),
            // The network, pictures and leases: nothing for which a busy
            // real-time thread should take the CPU from `lamp-controls`
            // (its privacy leases) or the eyes.
            WorkerRole::Provider
            | WorkerRole::Second
            | WorkerRole::Camera
            | WorkerRole::Privacy
            | WorkerRole::Probe => Scheduling::Ordinary,
        }
    }
}

/// How a worker runs beside the voice (`WorkerRole::scheduling`, #1608).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheduling {
    /// The voice's own real-time policy and priority, RR 20.
    KeepsVoices,
    /// Round-robin at this lower real-time priority, if it started higher.
    Realtime(u8),
    /// The ordinary policy at nice 0, or nicer if it started nicer.
    Ordinary,
}

/// The ring's real-time priority: below the voice's RR 20.
pub const RING_REALTIME_PRIORITY: u8 = 10;
/// The voice's own, as its units set it (`CPUSchedulingPriority=20`).
pub const VOICE_REALTIME_PRIORITY: u8 = 20;
const _: () = assert!(RING_REALTIME_PRIORITY < VOICE_REALTIME_PRIORITY);

/// Programs the voice starts that are not itself, by file name: they run in
/// the voice's service too.
pub const HELPERS: [&str; 1] = ["lamp-turn-end"];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// #1608: only the sound in and out keep the voice's RR 20; the ring is
    /// below it, and every other role at the ordinary policy.
    #[test]
    fn only_the_audio_keeps_the_voices_real_time_priority() {
        for role in WorkerRole::ALL {
            let expected = match role.as_str() {
                "capture" | "speaker" => Scheduling::KeepsVoices,
                "ring" => Scheduling::Realtime(10),
                _ => Scheduling::Ordinary,
            };
            assert_eq!(role.scheduling(), expected, "{role:?}");
        }
    }

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
