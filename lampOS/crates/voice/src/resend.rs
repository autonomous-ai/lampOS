//! A line lost with its connection, sent again once (#705, the first builder
//! at 21:00 and 22:25 on 17 Oct, `LAMP_RESEND_LOST_LINE=on`).
//!
//! On lamp-cb94 every `barrier_timeout`, `write_timeout` and most
//! `transport` drops cost the line a person was waiting on: the session was
//! ready again in about 1.2 to 2 s, and the line was never answered. With
//! the switch, such a line is sent once more, on the next session, if
//! nothing of it can have been answered or acted on and nothing about the
//! person's choice has changed since. These are the rules; keeping the audio
//! and sending it is the provider worker's.
//!
//! What is never sent again: a line that had any output or any tool call
//! (a timer may already be set), one from before a mute or a privacy change
//! (another privacy generation), one already sent again, one longer than
//! [`KEEP_MOST_SAMPLES`], one whose next session is not ready within
//! [`RESEND_WITHIN_US`] of its end, one that failed for a reason a second
//! try does not mend (quota, refusal, authentication, a silent model), and
//! one the person has since followed with a new line.

/// Where the coordinator reads the switch. Off unless `on`.
pub const ENVIRONMENT: &str = "LAMP_RESEND_LOST_LINE";
/// The provider worker's flag for it.
pub const FLAG: &str = "--resend-lost-line";
/// The most of a line kept for sending again: 15 s at 16 kHz (480 KB).
pub const KEEP_MOST_SAMPLES: usize = 15 * 16_000;
/// From the line's end to the next ready session, at most.
pub const RESEND_WITHIN_US: u64 = 5_000_000;

/// The provider worker's argument for the switch, from the environment.
pub fn arguments_from_environment() -> Vec<String> {
    arguments(std::env::var(ENVIRONMENT).ok().as_deref())
}

fn arguments(value: Option<&str>) -> Vec<String> {
    if value == Some("on") {
        vec![FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

/// Take a leading [`FLAG`] off a worker's flags.
pub fn take(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// The provider worker journal's line for an accepted replay (`line_resent`).
pub fn resent_record(request: u64, session: u64, at_us: u64) -> serde_json::Value {
    serde_json::json!({"kind":"line_resent","turn":request,"provider_session":session,"at_us":at_us,
        "basis":"lost with its connection before any output or tool call; sent once on the next session"})
}

/// The provider worker journal's reason a held line is not sent again (`resend_refused`).
pub fn refused_record(request: u64, why: Refused, at_us: u64) -> serde_json::Value {
    serde_json::json!({"kind":"resend_refused","turn":request,"why":why.name(),"at_us":at_us})
}

/// What is known of a failed line when the next session is ready.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lost {
    /// Where it was when its connection was lost.
    pub stage: crate::provider_worker::FailedStage,
    /// Why.
    pub fault: crate::provider_worker::Fault,
    /// Any audio, text or transcript of an answer reached this side.
    pub had_output: bool,
    /// The model called a tool for it.
    pub had_tool_call: bool,
    /// Its privacy generation, and the one now.
    pub generation: u64,
    pub generation_now: u64,
    /// Its kept samples, and whether some were not kept (it ran longer).
    pub samples: usize,
    pub overflowed: bool,
    /// When its input ended (`None`: still open when lost) and when the
    /// next session became ready, on one monotonic clock.
    pub ended_us: Option<u64>,
    pub ready_us: u64,
    /// It is already a line sent again.
    pub resent: bool,
    /// The person has begun a newer line since.
    pub newer_line: bool,
}

/// Why a lost line is not sent again, for the provider worker journal (`resend_refused`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    Output,
    ToolCall,
    OtherGeneration,
    Resent,
    NewerLine,
    TooLong,
    NothingKept,
    TooLate,
    Stage,
    Fault,
    /// The next session refused to take it (`try_resend_for_session`).
    NotTaken,
}

impl Refused {
    pub fn name(self) -> &'static str {
        match self {
            Self::Output => "it had output",
            Self::ToolCall => "a tool was called for it",
            Self::OtherGeneration => "a mute or privacy change came since",
            Self::Resent => "it was already sent again",
            Self::NewerLine => "a newer line has begun",
            Self::TooLong => "longer than is kept",
            Self::NothingKept => "nothing of it was kept",
            Self::TooLate => "the next session was not ready within 5 s of its end",
            Self::Stage => "it was not delivered, or had begun to be answered",
            Self::Fault => "a second try does not mend this failure",
            Self::NotTaken => "the next session did not take it",
        }
    }
}

/// Whether a second try can mend a failure: the connection was lost or
/// fell behind. Not the service refusing, a quota or a key, not the model
/// staying silent (`response_timeout`: it would only stay silent again),
/// and not a fault here.
pub fn mendable(fault: crate::provider_worker::Fault) -> bool {
    use crate::provider_worker::Fault;
    matches!(
        fault,
        Fault::BarrierTimeout
            | Fault::WriteTimeout { .. }
            | Fault::Transport
            | Fault::PeerClosed { .. }
            | Fault::ServerGoAway
            | Fault::StalledResponse
            | Fault::ReadTimeout
    )
}

/// Whether `lost` is sent again once, and if not, why.
pub fn judge(lost: &Lost) -> Result<(), Refused> {
    use crate::provider_worker::FailedStage;
    if lost.resent {
        return Err(Refused::Resent);
    }
    if lost.had_output {
        return Err(Refused::Output);
    }
    if lost.had_tool_call {
        return Err(Refused::ToolCall);
    }
    if lost.generation != lost.generation_now {
        return Err(Refused::OtherGeneration);
    }
    if lost.newer_line {
        return Err(Refused::NewerLine);
    }
    if !matches!(
        lost.stage,
        FailedStage::InputOpen | FailedStage::AwaitingResponse
    ) {
        return Err(Refused::Stage);
    }
    if !mendable(lost.fault) {
        return Err(Refused::Fault);
    }
    if lost.overflowed || lost.samples > KEEP_MOST_SAMPLES {
        return Err(Refused::TooLong);
    }
    if lost.samples == 0 {
        return Err(Refused::NothingKept);
    }
    // A line still open when lost is judged from when it was lost, which is
    // no earlier than its end would have been.
    let ended = lost.ended_us.unwrap_or(lost.ready_us);
    if lost.ready_us.saturating_sub(ended) > RESEND_WITHIN_US {
        return Err(Refused::TooLate);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_worker::{FailedStage, Fault};

    /// lamp-cb94's shape (#705, 13:39 UTC): a line waiting behind a cut-off
    /// response, lost to `barrier_timeout` 3.5 s after it ended, the next
    /// session ready 1.2 s later.
    fn cb94() -> Lost {
        Lost {
            stage: FailedStage::AwaitingResponse,
            fault: Fault::BarrierTimeout,
            had_output: false,
            had_tool_call: false,
            generation: 4,
            generation_now: 4,
            samples: 3 * 16_000,
            overflowed: false,
            ended_us: Some(10_000_000),
            ready_us: 14_700_000,
            resent: false,
            newer_line: false,
        }
    }

    #[test]
    fn the_switch_is_on_only_and_its_flag_comes_off() {
        assert_eq!(arguments(Some("on")), [FLAG]);
        for value in [None, Some(""), Some("1"), Some("ON"), Some("off")] {
            assert!(arguments(value).is_empty(), "{value:?}");
        }
        let flags: Vec<String> = [FLAG, "--next"].map(str::to_owned).to_vec();
        assert_eq!(take(&flags), (true, &flags[1..]));
        assert_eq!(take(&flags[1..]), (false, &flags[1..]));
    }

    /// cb94's lost line is sent again; each guard alone refuses it.
    #[test]
    fn a_lost_line_is_sent_again_once_and_only_when_nothing_could_have_acted_on_it() {
        assert_eq!(judge(&cb94()), Ok(()));
        let refused = |change: fn(&mut Lost)| {
            let mut lost = cb94();
            change(&mut lost);
            judge(&lost).unwrap_err()
        };
        assert_eq!(refused(|l| l.resent = true), Refused::Resent);
        assert_eq!(refused(|l| l.had_output = true), Refused::Output);
        assert_eq!(refused(|l| l.had_tool_call = true), Refused::ToolCall);
        assert_eq!(refused(|l| l.generation_now = 5), Refused::OtherGeneration);
        assert_eq!(refused(|l| l.newer_line = true), Refused::NewerLine);
        assert_eq!(
            refused(|l| l.stage = FailedStage::NotDelivered),
            Refused::Stage
        );
        assert_eq!(
            refused(|l| l.stage = FailedStage::Responding),
            Refused::Stage
        );
        assert_eq!(refused(|l| l.fault = Fault::QuotaExceeded), Refused::Fault);
        assert_eq!(
            refused(|l| l.fault = Fault::ResponseTimeout),
            Refused::Fault
        );
        assert_eq!(refused(|l| l.fault = Fault::Authentication), Refused::Fault);
        assert_eq!(refused(|l| l.overflowed = true), Refused::TooLong);
        assert_eq!(
            refused(|l| l.samples = KEEP_MOST_SAMPLES + 1),
            Refused::TooLong
        );
        assert_eq!(refused(|l| l.samples = 0), Refused::NothingKept);
        assert_eq!(refused(|l| l.ready_us = 15_000_001), Refused::TooLate);
        // At the bound exactly, it is sent.
        let mut edge = cb94();
        edge.ready_us = 15_000_000;
        assert_eq!(judge(&edge), Ok(()));
        // Lost while still open: judged from when it was lost.
        let mut open = cb94();
        open.stage = FailedStage::InputOpen;
        open.ended_us = None;
        assert_eq!(judge(&open), Ok(()));
    }

    /// The provider worker journal's two lines carry the turn and the plain why.
    #[test]
    fn the_trace_says_what_was_resent_and_why_one_was_not() {
        let sent = resent_record(7, 3, 42);
        assert_eq!(
            (sent["kind"].as_str(), sent["turn"].as_u64()),
            (Some("line_resent"), Some(7))
        );
        let refused = refused_record(7, Refused::ToolCall, 42);
        assert_eq!(refused["kind"], "resend_refused");
        assert_eq!(refused["why"], "a tool was called for it");
        assert_eq!(
            refused_record(7, Refused::NotTaken, 42)["why"],
            "the next session did not take it"
        );
    }

    /// The failures a second try mends, and those it does not.
    #[test]
    fn only_a_lost_or_lagging_connection_is_mended_by_a_second_try() {
        for fault in [
            Fault::BarrierTimeout,
            Fault::WriteTimeout {
                write: None,
                kib: None,
                deadline_ms: None,
            },
            Fault::Transport,
            Fault::PeerClosed { code: Some(1008) },
            Fault::ServerGoAway,
            Fault::StalledResponse,
            Fault::ReadTimeout,
        ] {
            assert!(mendable(fault), "{fault:?}");
        }
        for fault in [
            Fault::ResponseTimeout,
            Fault::CompletionTimeout,
            Fault::Authentication,
            Fault::RateLimited,
            Fault::QuotaExceeded,
            Fault::ServerUnavailable,
            Fault::ServerRejected,
            Fault::Protocol,
            Fault::Local,
            Fault::ConnectTimeout,
            Fault::SetupTimeout,
        ] {
            assert!(!mendable(fault), "{fault:?}");
        }
    }
}
