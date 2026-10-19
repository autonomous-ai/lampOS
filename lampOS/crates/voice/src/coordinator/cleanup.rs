//! Bounded shutdown evidence; collecting a failure never decides whether cleanup runs.
use std::{fmt::Write, io};

const ERROR_BYTES: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stage {
    RingCleanup,
    #[cfg(test)]
    Fixture,
    DiagnosticReceipt,
    RingReceipt,
    WorkerFault,
    ReferenceFault,
    ControlReceive,
    CameraObserve,
    CameraReceive,
    WorkerExit,
    ForcedStop,
    Motor,
    RingBlank,
    CameraFinish,
    Endpoint,
    Certification,
    Unsolicited,
}

struct Failure {
    stage: Stage,
    owner: &'static str,
    error: io::Error,
    occurrences: u64,
}

#[derive(Default)]
pub(super) struct Failures {
    // At most 17 stages x 8 owners, independent of shutdown-loop iterations.
    entries: Vec<Failure>,
}

impl Failures {
    pub(super) fn push(&mut self, stage: Stage, owner: &str, error: io::Error) {
        let owner = match owner {
            "speaker" => "speaker",
            "capture" => "capture",
            "provider" => "provider",
            "privacy" => "privacy",
            "ring" => "ring",
            "camera" => "camera",
            "second" => "second",
            _ => "session",
        };
        if let Some(first) = self
            .entries
            .iter_mut()
            .find(|f| f.stage == stage && f.owner == owner)
        {
            first.occurrences = first.occurrences.saturating_add(1);
            return;
        }
        // Keep short single errors intact, including their native kind/code.
        // Long messages are cut on a UTF-8 boundary, including the suffix.
        let mut text = error.to_string();
        let error = if text.len() > ERROR_BYTES {
            let mut end = ERROR_BYTES - "... [truncated]".len();
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
            text.push_str("... [truncated]");
            io::Error::new(error.kind(), text)
        } else {
            error
        };
        self.entries.push(Failure {
            stage,
            owner,
            error,
            occurrences: 1,
        });
    }

    pub(super) fn finish(mut self) -> io::Result<()> {
        if self.entries.is_empty() {
            return Ok(());
        }
        if self.entries.len() == 1 && self.entries[0].occurrences == 1 {
            return Err(self.entries.pop().unwrap().error);
        }
        let kind = self.entries[0].error.kind();
        let mut joined = String::new();
        for failure in self.entries {
            if !joined.is_empty() {
                joined.push_str("; ");
            }
            write!(
                joined,
                "{} {:?}: {}",
                failure.owner, failure.stage, failure.error
            )
            .expect("writing to String");
            if failure.occurrences > 1 {
                write!(joined, " ({} occurrences)", failure.occurrences)
                    .expect("writing to String");
            }
        }
        Err(io::Error::new(kind, joined))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_native_error_keeps_its_code_and_kind() {
        let mut failures = Failures::default();
        failures.push(
            Stage::WorkerExit,
            "capture",
            io::Error::from_raw_os_error(5),
        );
        assert_eq!(failures.finish().unwrap_err().raw_os_error(), Some(5));
    }

    #[test]
    fn worker_and_endpoint_failures_survive_repeated_observations() {
        let mut failures = Failures::default();
        for _ in 0..10_000 {
            failures.push(
                Stage::ForcedStop,
                "speaker",
                io::Error::new(io::ErrorKind::TimedOut, "speaker forced"),
            );
        }
        failures.push(
            Stage::ForcedStop,
            "capture",
            io::Error::other("capture forced"),
        );
        failures.push(
            Stage::Endpoint,
            "session",
            io::Error::other("endpoint still pending"),
        );
        failures.push(
            Stage::Certification,
            "speaker",
            io::Error::other("missing diagnostic finish"),
        );
        assert_eq!(failures.entries.len(), 4);
        let error = failures.finish().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        let text = error.to_string();
        for cause in [
            "speaker forced",
            "capture forced",
            "endpoint still pending",
            "missing diagnostic finish",
            "10000 occurrences",
        ] {
            assert!(text.contains(cause), "{text}");
        }
        let positions = [
            "speaker forced",
            "capture forced",
            "endpoint still pending",
            "missing diagnostic finish",
        ]
        .map(|cause| text.find(cause).unwrap());
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]), "{text}");
    }

    #[test]
    fn every_stage_and_owner_stays_within_the_output_bound() {
        let mut failures = Failures::default();
        for stage in [
            Stage::RingCleanup,
            Stage::Fixture,
            Stage::DiagnosticReceipt,
            Stage::RingReceipt,
            Stage::WorkerFault,
            Stage::ReferenceFault,
            Stage::ControlReceive,
            Stage::CameraObserve,
            Stage::CameraReceive,
            Stage::WorkerExit,
            Stage::ForcedStop,
            Stage::Motor,
            Stage::RingBlank,
            Stage::CameraFinish,
            Stage::Endpoint,
            Stage::Certification,
            Stage::Unsolicited,
        ] {
            for owner in [
                "speaker", "capture", "provider", "privacy", "ring", "camera", "second", "session",
            ] {
                failures.push(stage, owner, io::Error::other("x".repeat(2048)));
            }
        }
        assert_eq!(failures.entries.len(), 136);
        for entry in &mut failures.entries {
            entry.occurrences = u64::MAX;
        }
        assert!(failures.finish().unwrap_err().to_string().len() < 160 * 1024);
    }

    #[test]
    fn repeated_keys_and_utf8_messages_have_bounded_storage() {
        let mut failures = Failures::default();
        for index in 0..10_000 {
            failures.push(
                Stage::ControlReceive,
                "capture",
                io::Error::other(format!("{index}:{}", "\u{20ac}".repeat(2000))),
            );
        }
        assert_eq!(failures.entries.len(), 1);
        let first = &mut failures.entries[0];
        assert!(first.error.to_string().len() <= ERROR_BYTES);
        first.occurrences = u64::MAX;
        failures.push(Stage::ControlReceive, "capture", io::Error::other("again"));
        assert_eq!(failures.entries[0].occurrences, u64::MAX);
        assert!(failures.finish().unwrap_err().to_string().len() < ERROR_BYTES + 100);
    }
}
