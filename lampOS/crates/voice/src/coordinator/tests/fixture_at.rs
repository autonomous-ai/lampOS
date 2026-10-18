//! `directed-fixture --fixture-at-ms` (diagnostic only): the fixture's own
//! written turn is admitted at the due time on the session's clock, with no
//! heard turn. The clock here is stepped in 10 ms capture blocks; nothing
//! sleeps.
use super::*;

const BLOCK_US: u64 = 10_000;

struct Session {
    owner: Controller,
    reply: Option<Reply>,
    queue: VecDeque<ProviderInput>,
    trace: SessionState,
}

impl Session {
    fn new() -> Self {
        let mut owner = Controller::new(new_boot().unwrap(), now());
        owner
            .set_microphone_permission(now(), Permission::Allowed)
            .unwrap();
        let until = MonoTime::from_micros(monotonic_us() + 1_000_000);
        owner
            .set_capture(now(), CaptureState::RetainingUntil(until))
            .unwrap();
        owner
            .set_admission(now(), AdmissionState::OpenUntil(until))
            .unwrap();
        Self {
            owner,
            reply: None,
            queue: VecDeque::new(),
            trace: SessionState::default(),
        }
    }

    /// Steps the session from its opening at `opened_us` for `blocks`
    /// blocks with the lamp at rest and nobody heard; the session time at
    /// which the fixture's turn began, if it did.
    fn run(&mut self, at_ms: Option<u64>, opened_us: u64, blocks: u64) -> Option<u64> {
        let mut due = at_ms.map(|ms| opened_us + ms * 1_000);
        for block in 0..blocks {
            let clock = opened_us + block * BLOCK_US;
            let started = start_timed_fixture(
                &mut due,
                clock,
                self.reply.is_none().then_some(1),
                &mut self.owner,
                &mut self.reply,
                &mut self.queue,
                PlayoutStart::default(),
                &mut self.trace,
                |_| Ok(()),
            )
            .unwrap();
            if started.is_some() {
                return Some(clock - opened_us);
            }
        }
        None
    }
}

#[test]
fn a_timed_fixture_starts_at_its_time_with_no_turn_heard() {
    let mut session = Session::new();
    let at = session.run(Some(3_000), 50_000_000, 1_000).unwrap();
    assert!(
        (3_000_000..3_000_000 + BLOCK_US).contains(&at),
        "started at {at} us"
    );
    assert!(matches!(
        session.queue.front(),
        Some(ProviderInput::Written { session: 1, .. })
    ));
    let turn = session.reply.as_ref().unwrap().owner.turn();
    assert!(
        session
            .trace
            .iter()
            .any(|event| event["kind"] == "input_admitted"
                && event["admission_basis"] == "fixture_at_ms"
                && event["turn"] == turn)
    );
    assert!(
        session
            .trace
            .iter()
            .any(|event| event["kind"] == "fixture_timed_start")
    );
}

#[test]
fn without_a_time_the_fixture_never_starts_unheard() {
    let mut session = Session::new();
    assert_eq!(session.run(None, 50_000_000, 6_000), None);
    assert!(session.queue.is_empty() && session.reply.is_none());
}

#[test]
fn a_timed_fixture_waits_while_the_lamp_is_busy_and_starts_once() {
    let mut session = Session::new();
    let opened = 50_000_000;
    let mut due = Some(opened + 3_000_000);
    let mut start = |session: &mut Session, clock, at_rest: bool| {
        start_timed_fixture(
            &mut due,
            clock,
            at_rest.then_some(1),
            &mut session.owner,
            &mut session.reply,
            &mut session.queue,
            PlayoutStart::default(),
            &mut session.trace,
            |_| Ok(()),
        )
        .unwrap()
        .is_some()
    };
    assert!(!start(&mut session, opened + 3_500_000, false));
    assert!(start(&mut session, opened + 3_600_000, true));
    assert!(!start(&mut session, opened + 9_000_000, true));
    assert_eq!(session.queue.len(), 1);
}
