//! Inert, bounded face-repair transaction core. Nothing here listens on a
//! socket, reads an environment variable, captures a frame or writes a store.
//! Trusted ingress must separately prove account authority and the intended
//! candidate's spoken consent. Neither an account start nor a name proves it.
//!
//! State/capture calls have a host target below 1 ms: bounded in-memory work,
//! no filesystem/network calls or waits. Only `start` requests OS entropy.
//! `CommitWork` transfers the consumed store precondition and prints to one
//! exclusive writer off the capture path; this module never runs that writer.
use crate::who::{MOST_PRINTS, PRINT, RepairError, RepairTarget};
use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};

pub const ENVIRONMENT: &str = "LAMP_FACE_REPAIR";
pub const TICKET_FOR: Duration = Duration::from_secs(120);
pub const CAPTURE_FOR: Duration = Duration::from_secs(4);
/// Consent must reach this core within the source evidence's freshness.
pub const FRESH_PROOF: Duration = Duration::from_secs(3);
/// An accepted consent still expires before a delayed owner commit.
pub const CONSENT_FOR: Duration = Duration::from_secs(30);
const CANDIDATE_FOR: Duration = Duration::from_secs(3);
const MOST_RECEIPTS: usize = 4;

/// Opaque account-generation authority supplied by the authenticated ingress.
/// It is not an app field, a model argument, or proof of a person's identity.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AccountGeneration([u8; 16]);
impl AccountGeneration {
    pub fn from_trusted_ingress(bytes: [u8; 16]) -> Option<Self> {
        (bytes != [0; 16]).then_some(Self(bytes))
    }
}

/// Opaque bearer value: format at the account boundary only, never in logs.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Ticket([u8; 16]);
impl Ticket {
    pub fn from_bytes(bytes: [u8; 16]) -> Option<Self> {
        (bytes != [0; 16]).then_some(Self(bytes))
    }
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

/// A passing identity under the exact Eyes publisher boot, not a person ID.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Candidate {
    boot: [u8; 8],
    face: u32,
}
impl Candidate {
    pub fn from_source(boot: &str, face: &str) -> Option<Self> {
        let boot = publisher_boot(boot)?;
        let digits = face.strip_prefix('f')?;
        if digits.is_empty()
            || digits.len() > 10
            || !digits.bytes().all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        let face = digits.parse().ok().filter(|face| *face > 0)?;
        Some(Self { boot, face })
    }
    pub fn qualified(&self) -> String {
        let boot = self
            .boot
            .iter()
            .fold(String::with_capacity(16), |text, byte| {
                text + &format!("{byte:02x}")
            });
        format!("{boot}/f{}", self.face)
    }
}
/// One actual camera acquisition. `acquired_at` is its original monotonic
/// source timestamp, not delivery/processing time and not advanced on a replay.
/// The capture adapter must keep it tied to the candidate from that same frame.
#[derive(Clone, Copy)]
pub struct FrameObservation {
    candidate: Option<Candidate>,
    faces: usize,
    acquired_at: Instant,
}
impl FrameObservation {
    pub fn from_source(candidate: Option<Candidate>, faces: usize, acquired_at: Instant) -> Self {
        Self {
            candidate,
            faces,
            acquired_at,
        }
    }
}

fn publisher_boot(text: &str) -> Option<[u8; 8]> {
    if text.len() != 16
        || !text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    let mut boot = [0; 8];
    for (index, byte) in boot.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(boot)
}

/// The actual admitted microphone request. No written or replayed request
/// may be passed as this value by ingress, even when it contains the word yes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SpeechRequest {
    worker: [u8; 16],
    session: u64,
    request: u64,
}
impl SpeechRequest {
    pub fn from_trusted_ingress(worker: [u8; 16], session: u64, request: u64) -> Option<Self> {
        (worker != [0; 16] && session > 0 && request > 0).then_some(Self {
            worker,
            session,
            request,
        })
    }
    pub fn parts(self) -> ([u8; 16], u64, u64) {
        (self.worker, self.session, self.request)
    }
}

/// A consumed, non-serializable proof token. The constructor is deliberately
/// a trusted-ingress seam, not a verifier: its caller must already validate
/// an explicit spoken yes to this ticket's question already asked at the
/// lamp, current request/session authority and exactly this boot-qualified
/// face speaking through that request's actual audio interval.
/// Account initiation, a model call, a visible face or a label is insufficient.
pub struct ConsentProof {
    ticket: Ticket,
    request: SpeechRequest,
    candidate: Candidate,
    heard_at: Instant,
    valid_until: Instant,
}
impl ConsentProof {
    pub fn from_trusted_ingress(
        ticket: Ticket,
        request: SpeechRequest,
        candidate: Candidate,
        heard_at: Instant,
        valid_until: Instant,
    ) -> Option<Self> {
        let lifetime = valid_until.checked_duration_since(heard_at)?;
        (!lifetime.is_zero() && lifetime <= CONSENT_FOR).then_some(Self {
            ticket,
            request,
            candidate,
            heard_at,
            valid_until,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Waiting,
    Capturing,
    Staged,
    Committing,
    Committed,
    Cancelled,
    Expired,
    Refused,
    NotWritten,
    Indeterminate,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Off,
    Busy,
    Entropy,
    Unknown,
    WrongPhase,
    NoCandidate,
    InvalidProof,
    InvalidPrints,
    StaleFrame,
    Unattested,
    Cancelled,
    Committing,
}

struct Active {
    account: AccountGeneration,
    ticket: Ticket,
    target: Option<RepairTarget>,
    started: Instant,
    deadline: Instant,
    phase: Status,
    candidate: Option<Candidate>,
    seen_at: Option<Instant>,
    last_frame: Option<Instant>,
    consent: Option<ConsentProof>,
    capture_started: Option<Instant>,
    last_print: Option<Instant>,
    prints: Vec<Vec<f32>>,
    completion: Option<Arc<()>>,
}
struct Receipt {
    account: AccountGeneration,
    ticket: Ticket,
    status: Status,
}

/// One pending transaction and at most four terminal receipts, all RAM-only.
/// Restart forgets RAM tickets and receipts. After a commit handoff the
/// canonical file may contain the complete old OR new record; reconcile it,
/// never infer rollback or unchanged prints from an unknown ticket.
pub struct Controller {
    enabled: bool,
    boot: [u8; 8],
    active: Option<Active>,
    receipts: VecDeque<Receipt>,
}
impl Controller {
    /// Runtime must pass the exact switch and publisher boot; this inert core
    /// reads neither. No enabling side effect or RPC is registered here.
    pub fn new(enabled: bool, eyes_boot: &str) -> Option<Self> {
        Some(Self {
            enabled,
            boot: publisher_boot(eyes_boot)?,
            active: None,
            receipts: VecDeque::new(),
        })
    }

    pub fn start(
        &mut self,
        account: AccountGeneration,
        target: RepairTarget,
        now: Instant,
    ) -> Result<Ticket, Error> {
        self.start_with_entropy(account, target, now, |bytes| {
            getrandom::fill(bytes).map_err(|_| ())
        })
    }
    fn start_with_entropy(
        &mut self,
        account: AccountGeneration,
        target: RepairTarget,
        now: Instant,
        entropy: impl FnOnce(&mut [u8; 16]) -> Result<(), ()>,
    ) -> Result<Ticket, Error> {
        if !self.enabled {
            return Err(Error::Off);
        }
        self.tick(now);
        if self.active.is_some() {
            return Err(Error::Busy);
        }
        let mut bytes = [0; 16];
        entropy(&mut bytes).map_err(|()| Error::Entropy)?;
        let ticket = Ticket::from_bytes(bytes).ok_or(Error::Entropy)?;
        if self.receipts.iter().any(|receipt| receipt.ticket == ticket) {
            return Err(Error::Entropy);
        }
        let deadline = now.checked_add(TICKET_FOR).ok_or(Error::Entropy)?;
        self.active = Some(Active {
            account,
            ticket,
            target: Some(target),
            started: now,
            deadline,
            phase: Status::Waiting,
            candidate: None,
            seen_at: None,
            last_frame: None,
            consent: None,
            capture_started: None,
            last_print: None,
            prints: Vec::with_capacity(MOST_PRINTS),
            completion: None,
        });
        Ok(ticket)
    }

    /// Trusted current-account/camera/frame observations. No retarget is ever
    /// allowed after the first single-face pin. Before a pin, waiting for one
    /// face is harmless; it grants nothing and still has the original deadline.
    pub fn observe(
        &mut self,
        account: AccountGeneration,
        frame: FrameObservation,
        allowed: bool,
        now: Instant,
    ) -> Result<Status, Error> {
        if !self.enabled {
            return Err(Error::Off);
        }
        self.tick(now);
        let active = self.active.as_mut().ok_or(Error::Unknown)?;
        if active.phase == Status::Committing {
            return Ok(Status::Committing);
        }
        // Also reject revoked authority on frame delivery. Runtime privacy
        // or account transitions must call `invalidate` without waiting for it.
        if active.account != account || !allowed {
            self.finish(Status::Cancelled);
            return Err(Error::Cancelled);
        }
        if frame.acquired_at < active.started
            || frame.acquired_at > now
            || now.duration_since(frame.acquired_at) >= CANDIDATE_FOR
            || active
                .last_frame
                .is_some_and(|previous| frame.acquired_at <= previous)
        {
            return Err(Error::StaleFrame);
        }
        active.last_frame = Some(frame.acquired_at);
        let cancelled = frame.faces > 1
            || frame
                .candidate
                .is_some_and(|candidate| candidate.boot != self.boot)
            || (frame.faces != 1 && frame.candidate.is_some())
            || active
                .candidate
                .is_some_and(|before| frame.faces != 1 || frame.candidate != Some(before));
        if cancelled {
            self.finish(Status::Cancelled);
            return Err(Error::Cancelled);
        }
        if let Some(candidate) = frame.candidate {
            active.candidate.get_or_insert(candidate);
            active.seen_at = Some(frame.acquired_at);
        }
        Ok(active.phase)
    }

    pub fn accept_consent(
        &mut self,
        account: AccountGeneration,
        ticket: Ticket,
        proof: ConsentProof,
        now: Instant,
    ) -> Result<Status, Error> {
        if !self.enabled {
            return Err(Error::Off);
        }
        if proof.ticket != ticket
            || now < proof.heard_at
            || now >= proof.valid_until
            || now.duration_since(proof.heard_at) >= FRESH_PROOF
        {
            return Err(Error::InvalidProof);
        }
        self.tick(now);
        let active = self.matching(account, ticket)?;
        if active.phase != Status::Waiting {
            return Err(Error::WrongPhase);
        }
        let candidate = active.candidate.ok_or(Error::NoCandidate)?;
        if candidate != proof.candidate || proof.heard_at < active.started {
            return Err(Error::InvalidProof);
        }
        active.consent = Some(proof);
        active.capture_started = Some(now);
        active.phase = Status::Capturing;
        Ok(active.phase)
    }

    /// Supply an embedding of this candidate from the exact latest observed
    /// frame, captured after consent. Trusted ingress, not this state machine,
    /// establishes that the embedding came from that frame. No inference runs.
    pub fn push_print(
        &mut self,
        account: AccountGeneration,
        ticket: Ticket,
        candidate: Candidate,
        frame_at: Instant,
        print: Vec<f32>,
        now: Instant,
    ) -> Result<Status, Error> {
        if !self.enabled {
            return Err(Error::Off);
        }
        self.tick(now);
        let active = self.matching(account, ticket)?;
        if active.phase != Status::Capturing {
            return Err(Error::WrongPhase);
        }
        // Check size before iterating: even a malformed caller vector must
        // not turn one capture tick into unbounded validation work.
        if print.len() != PRINT {
            self.finish(Status::Cancelled);
            return Err(Error::InvalidPrints);
        }
        let norm = print.iter().map(|value| value * value).sum::<f32>();
        if !print.iter().all(|value| value.is_finite())
            || !norm.is_finite()
            || (norm - 1.0).abs() > 0.001
        {
            self.finish(Status::Cancelled);
            return Err(Error::InvalidPrints);
        }
        if active.candidate != Some(candidate) {
            self.finish(Status::Cancelled);
            return Err(Error::Cancelled);
        }
        if active.seen_at != Some(frame_at)
            || now < frame_at
            || active.capture_started.is_none_or(|start| frame_at <= start)
            || active
                .last_print
                .is_some_and(|previous| frame_at <= previous)
        {
            return Err(Error::StaleFrame);
        }
        active.last_print = Some(frame_at);
        // Retain exactly the validated values, not arbitrary spare capacity
        // in a caller-owned allocation. Five prints stay bounded in RAM.
        active.prints.push(print.as_slice().to_vec());
        if active.prints.len() == MOST_PRINTS {
            active.phase = Status::Staged;
        }
        Ok(active.phase)
    }

    /// The authenticated owner explicitly attests this exact staged candidate.
    /// Claim is a one-way handoff: after it, cancellation/expiry cannot assert
    /// rollback of a write already in flight. Only worker completion settles it.
    pub fn begin_commit(
        &mut self,
        account: AccountGeneration,
        ticket: Ticket,
        attested: bool,
        now: Instant,
    ) -> Result<CommitWork, Error> {
        if !self.enabled {
            return Err(Error::Off);
        }
        self.tick(now);
        let active = self.matching(account, ticket)?;
        if active.phase != Status::Staged {
            return Err(Error::WrongPhase);
        }
        if !attested {
            return Err(Error::Unattested);
        }
        let target = active.target.take().ok_or(Error::WrongPhase)?;
        let proof = active.consent.take().ok_or(Error::InvalidProof)?;
        let seal = Arc::new(());
        active.completion = Some(seal.clone());
        active.phase = Status::Committing;
        Ok(CommitWork {
            target,
            prints: std::mem::take(&mut active.prints),
            request: proof.request,
            completion: Completion {
                account,
                ticket,
                seal,
            },
        })
    }

    /// Called only by the trusted exclusive writer's completion path. Results
    /// carry no raw names, paths or embeddings. Indeterminate is never rollback.
    pub fn complete(
        &mut self,
        completion: Completion,
        result: Result<(), RepairError>,
    ) -> Result<Status, Error> {
        let active = self.matching(completion.account, completion.ticket)?;
        if active.phase != Status::Committing
            || !active
                .completion
                .as_ref()
                .is_some_and(|seal| Arc::ptr_eq(seal, &completion.seal))
        {
            return Err(Error::WrongPhase);
        }
        let status = match result {
            Ok(()) => Status::Committed,
            Err(RepairError::NotWritten) => Status::NotWritten,
            Err(RepairError::Indeterminate) => Status::Indeterminate,
            Err(
                RepairError::TargetChanged
                | RepairError::KnownElsewhere
                | RepairError::InvalidPrints,
            ) => Status::Refused,
        };
        self.finish(status);
        Ok(status)
    }

    /// Trusted runtime privacy/account invalidation, independent of a frame,
    /// ticket or the account that started the transaction. No app/model route
    /// may expose this entry point. Drops pending target, consent and prints;
    /// a consumed write handoff remains Committing until its real completion.
    /// Disabled or absent active work returns Off or Unknown respectively;
    /// existing expiry/terminal receipts are retained, never rewritten.
    pub fn invalidate(&mut self, now: Instant) -> Result<Status, Error> {
        if !self.enabled {
            return Err(Error::Off);
        }
        self.tick(now);
        let active = self.active.as_ref().ok_or(Error::Unknown)?;
        if active.phase == Status::Committing {
            return Ok(Status::Committing);
        }
        self.finish(Status::Cancelled);
        Ok(Status::Cancelled)
    }

    pub fn cancel(
        &mut self,
        account: AccountGeneration,
        ticket: Ticket,
        now: Instant,
    ) -> Result<Status, Error> {
        if !self.enabled {
            return Err(Error::Off);
        }
        self.tick(now);
        let active = self.matching(account, ticket)?;
        if active.phase == Status::Committing {
            return Err(Error::Committing);
        }
        self.finish(Status::Cancelled);
        Ok(Status::Cancelled)
    }
    pub fn status(
        &mut self,
        account: AccountGeneration,
        ticket: Ticket,
        now: Instant,
    ) -> Option<Status> {
        self.tick(now);
        self.active
            .as_ref()
            .filter(|active| active.account == account && active.ticket == ticket)
            .map(|active| active.phase)
            .or_else(|| {
                self.receipts
                    .iter()
                    .find(|receipt| receipt.account == account && receipt.ticket == ticket)
                    .map(|receipt| receipt.status)
            })
    }
    /// Invoke on capture ticks too, so disappearance or timeout needs no app
    /// poll. This only drops bounded RAM; it performs no store I/O or waits.
    pub fn tick(&mut self, now: Instant) {
        let expired = self.active.as_ref().is_some_and(|active| {
            active.phase != Status::Committing
                && (now < active.started
                    || now >= active.deadline
                    || active.seen_at.is_some_and(|seen| {
                        now < seen || now.duration_since(seen) >= CANDIDATE_FOR
                    })
                    || active
                        .consent
                        .as_ref()
                        .is_some_and(|proof| now >= proof.valid_until)
                    || (active.phase == Status::Capturing
                        && active
                            .capture_started
                            .is_some_and(|start| now.duration_since(start) >= CAPTURE_FOR)))
        });
        if expired {
            self.finish(Status::Expired);
        }
    }
    fn matching(
        &mut self,
        account: AccountGeneration,
        ticket: Ticket,
    ) -> Result<&mut Active, Error> {
        self.active
            .as_mut()
            .filter(|active| active.account == account && active.ticket == ticket)
            .ok_or(Error::Unknown)
    }
    fn finish(&mut self, status: Status) {
        if let Some(active) = self.active.take() {
            if self.receipts.len() == MOST_RECEIPTS {
                self.receipts.pop_front();
            }
            self.receipts.push_back(Receipt {
                account: active.account,
                ticket: active.ticket,
                status,
            });
        }
    }
}

/// The exclusive off-capture writer owns this non-cloneable work item. It
/// must return the opaque completion exactly once even on store refusal.
pub struct CommitWork {
    target: RepairTarget,
    prints: Vec<Vec<f32>>,
    request: SpeechRequest,
    completion: Completion,
}
impl CommitWork {
    pub fn request(&self) -> SpeechRequest {
        self.request
    }
    pub fn into_parts(self) -> (RepairTarget, Vec<Vec<f32>>, Completion) {
        (self.target, self.prints, self.completion)
    }
}
/// Not serialized, cloned or constructed by account/model input.
pub struct Completion {
    account: AccountGeneration,
    ticket: Ticket,
    seal: Arc<()>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::who::{PRINT, People};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const BOOT: &str = "0123456789abcdef";

    struct Fixture {
        people: People,
        root: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "eyes-repair-controller-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&root).unwrap();
            let mut people = People::open(root.join("people.json")).unwrap();
            people
                .remember("Fixture Target", vec![print(0)], 1)
                .unwrap();
            Self { people, root }
        }
        fn target(&self) -> RepairTarget {
            self.people.repair_target("Fixture Target").unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    fn print(axis: usize) -> Vec<f32> {
        let mut print = vec![0.0; PRINT];
        print[axis] = 1.0;
        print
    }
    fn account(byte: u8) -> AccountGeneration {
        AccountGeneration::from_trusted_ingress([byte; 16]).unwrap()
    }
    fn face(id: &str) -> Candidate {
        Candidate::from_source(BOOT, id).unwrap()
    }
    fn proof(ticket: Ticket, candidate: Candidate, at: Instant) -> ConsentProof {
        ConsentProof::from_trusted_ingress(
            ticket,
            SpeechRequest::from_trusted_ingress([3; 16], 1, 1).unwrap(),
            candidate,
            at,
            at + CONSENT_FOR,
        )
        .unwrap()
    }
    fn started(fixture: &Fixture, at: Instant) -> (Controller, Ticket) {
        let mut controller = Controller::new(true, BOOT).unwrap();
        let ticket = controller.start(account(1), fixture.target(), at).unwrap();
        (controller, ticket)
    }
    fn staged(fixture: &Fixture, at: Instant) -> (Controller, Ticket) {
        let (mut controller, ticket) = started(fixture, at);
        controller
            .observe(
                account(1),
                FrameObservation::from_source(Some(face("f1")), 1, at),
                true,
                at,
            )
            .unwrap();
        controller
            .accept_consent(account(1), ticket, proof(ticket, face("f1"), at), at)
            .unwrap();
        for index in 1..=MOST_PRINTS {
            let now = at + Duration::from_millis(index as u64 * 100);
            controller
                .observe(
                    account(1),
                    FrameObservation::from_source(Some(face("f1")), 1, now),
                    true,
                    now,
                )
                .unwrap();
            controller
                .push_print(account(1), ticket, face("f1"), now, print(2), now)
                .unwrap();
        }
        assert_eq!(
            controller.status(account(1), ticket, at + Duration::from_secs(1)),
            Some(Status::Staged)
        );
        (controller, ticket)
    }

    #[test]
    fn off_busy_and_entropy_failure_create_no_authority_or_store_change() {
        let fixture = Fixture::new();
        let at = Instant::now();
        let original = std::fs::read(fixture.root.join("people.json")).unwrap();
        let mut off = Controller::new(false, BOOT).unwrap();
        assert!(matches!(
            off.start(account(1), fixture.target(), at),
            Err(Error::Off)
        ));
        let (mut controller, ticket) = started(&fixture, at);
        assert!(matches!(
            controller.start(account(1), fixture.target(), at),
            Err(Error::Busy)
        ));
        assert_eq!(
            controller.cancel(account(1), ticket, at),
            Ok(Status::Cancelled)
        );
        assert!(matches!(
            controller.start_with_entropy(account(1), fixture.target(), at, |_| Err(())),
            Err(Error::Entropy)
        ));
        assert_eq!(
            std::fs::read(fixture.root.join("people.json")).unwrap(),
            original
        );
    }

    #[test]
    fn account_start_and_a_name_never_replace_the_old_record() {
        let fixture = Fixture::new();
        let at = Instant::now();
        let (mut controller, ticket) = started(&fixture, at);
        controller
            .observe(
                account(1),
                FrameObservation::from_source(Some(face("f1")), 1, at),
                true,
                at,
            )
            .unwrap();
        assert!(matches!(
            controller.begin_commit(account(1), ticket, true, at),
            Err(Error::WrongPhase)
        ));
        assert!(matches!(
            controller.push_print(account(1), ticket, face("f1"), at, print(2), at),
            Err(Error::WrongPhase)
        ));
        assert!(matches!(
            controller.begin_commit(account(2), ticket, true, at),
            Err(Error::Unknown)
        ));
        assert_eq!(fixture.people.who(&print(0)).unwrap().0, "Fixture Target");
        assert_eq!(fixture.people.who(&print(2)), None);
    }

    #[test]
    fn proof_is_ticket_request_candidate_and_time_bound() {
        let fixture = Fixture::new();
        let at = Instant::now();
        let (mut controller, ticket) = started(&fixture, at);
        controller
            .observe(
                account(1),
                FrameObservation::from_source(Some(face("f1")), 1, at),
                true,
                at,
            )
            .unwrap();
        for candidate in [
            face("f2"),
            Candidate::from_source("fedcba9876543210", "f1").unwrap(),
        ] {
            assert_eq!(
                controller.accept_consent(account(1), ticket, proof(ticket, candidate, at), at),
                Err(Error::InvalidProof)
            );
        }
        let wrong = Ticket::from_bytes([9; 16]).unwrap();
        assert_eq!(
            controller.accept_consent(account(1), ticket, proof(wrong, face("f1"), at), at),
            Err(Error::InvalidProof)
        );
        assert_eq!(
            controller.accept_consent(
                account(1),
                ticket,
                proof(ticket, face("f1"), at),
                at + FRESH_PROOF
            ),
            Err(Error::InvalidProof)
        );
        assert!(SpeechRequest::from_trusted_ingress([0; 16], 1, 1).is_none());
        assert!(SpeechRequest::from_trusted_ingress([3; 16], 0, 1).is_none());
        assert!(
            ConsentProof::from_trusted_ingress(
                ticket,
                SpeechRequest::from_trusted_ingress([3; 16], 1, 1).unwrap(),
                face("f1"),
                at,
                at
            )
            .is_none()
        );
        assert_eq!(
            controller.accept_consent(account(1), ticket, proof(ticket, face("f1"), at), at),
            Ok(Status::Capturing)
        );
    }

    #[test]
    fn a_pinned_candidate_cannot_disappear_retarget_or_cross_privacy_or_account() {
        let fixture = Fixture::new();
        let at = Instant::now();
        for (owner, candidate, count, allowed) in [
            (account(1), None, 0, true),
            (account(1), Some(face("f2")), 1, true),
            (
                account(1),
                Some(Candidate::from_source("fedcba9876543210", "f1").unwrap()),
                1,
                true,
            ),
            (account(1), Some(face("f1")), 2, true),
            (account(1), Some(face("f1")), 1, false),
            (account(2), Some(face("f1")), 1, true),
        ] {
            let (mut controller, ticket) = staged(&fixture, at);
            assert_eq!(
                controller.observe(
                    owner,
                    FrameObservation::from_source(candidate, count, at + Duration::from_secs(1)),
                    allowed,
                    at + Duration::from_secs(1)
                ),
                Err(Error::Cancelled)
            );
            assert_eq!(
                controller.status(account(1), ticket, at + Duration::from_secs(1)),
                Some(Status::Cancelled)
            );
            assert!(
                controller
                    .begin_commit(account(1), ticket, true, at + Duration::from_secs(1))
                    .is_err()
            );
        }
    }

    #[test]
    fn captures_are_new_bounded_unit_prints_for_the_pinned_frame() {
        let fixture = Fixture::new();
        let at = Instant::now();
        for mut invalid in [
            vec![],
            vec![0.0; PRINT],
            vec![1.0; PRINT],
            vec![0.0; PRINT + 1],
            print(2),
        ] {
            if invalid.len() == PRINT && invalid[2] == 1.0 {
                invalid[2] = f32::NAN;
            }
            let (mut controller, ticket) = started(&fixture, at);
            controller
                .observe(
                    account(1),
                    FrameObservation::from_source(Some(face("f1")), 1, at),
                    true,
                    at,
                )
                .unwrap();
            controller
                .accept_consent(account(1), ticket, proof(ticket, face("f1"), at), at)
                .unwrap();
            assert_eq!(
                controller.push_print(account(1), ticket, face("f1"), at, invalid, at),
                Err(Error::InvalidPrints)
            );
        }
        let (mut controller, ticket) = started(&fixture, at);
        controller
            .observe(
                account(1),
                FrameObservation::from_source(Some(face("f1")), 1, at),
                true,
                at,
            )
            .unwrap();
        controller
            .accept_consent(account(1), ticket, proof(ticket, face("f1"), at), at)
            .unwrap();
        let now = at + Duration::from_millis(100);
        controller
            .observe(
                account(1),
                FrameObservation::from_source(Some(face("f1")), 1, now),
                true,
                now,
            )
            .unwrap();
        assert_eq!(
            controller.push_print(account(1), ticket, face("f1"), at, print(2), now),
            Err(Error::StaleFrame)
        );
    }

    #[test]
    fn capture_and_handoff_retain_only_the_bounded_print_values() {
        let fixture = Fixture::new();
        let at = Instant::now();
        let (mut controller, ticket) = started(&fixture, at);
        controller
            .observe(
                account(1),
                FrameObservation::from_source(Some(face("f1")), 1, at),
                true,
                at,
            )
            .unwrap();
        controller
            .accept_consent(account(1), ticket, proof(ticket, face("f1"), at), at)
            .unwrap();
        for index in 1..=MOST_PRINTS {
            let now = at + Duration::from_millis(index as u64 * 100);
            controller
                .observe(
                    account(1),
                    FrameObservation::from_source(Some(face("f1")), 1, now),
                    true,
                    now,
                )
                .unwrap();
            let mut oversized = Vec::with_capacity(PRINT * 64);
            oversized.extend_from_slice(&print(2));
            assert!(oversized.capacity() > PRINT);
            controller
                .push_print(account(1), ticket, face("f1"), now, oversized, now)
                .unwrap();
            for retained in &controller.active.as_ref().unwrap().prints {
                assert_eq!(retained, &print(2));
                assert_eq!(retained.capacity(), PRINT);
            }
        }
        let work = controller
            .begin_commit(account(1), ticket, true, at + Duration::from_secs(1))
            .unwrap();
        let (_, prints, _) = work.into_parts();
        assert_eq!(prints.len(), MOST_PRINTS);
        for retained in prints {
            assert_eq!(retained, print(2));
            assert_eq!(retained.capacity(), PRINT);
        }
    }

    #[test]
    fn ticket_and_capture_deadlines_do_not_extend_on_status_reads() {
        let fixture = Fixture::new();
        let at = Instant::now();
        let (mut controller, ticket) = started(&fixture, at);
        assert_eq!(
            controller.status(
                account(1),
                ticket,
                at + TICKET_FOR - Duration::from_nanos(1)
            ),
            Some(Status::Waiting)
        );
        assert_eq!(
            controller.status(account(1), ticket, at + TICKET_FOR),
            Some(Status::Expired)
        );
        let (mut controller, ticket) = started(&fixture, at);
        controller
            .observe(
                account(1),
                FrameObservation::from_source(Some(face("f1")), 1, at),
                true,
                at,
            )
            .unwrap();
        controller
            .accept_consent(account(1), ticket, proof(ticket, face("f1"), at), at)
            .unwrap();
        for second in 1..=3 {
            controller
                .observe(
                    account(1),
                    FrameObservation::from_source(
                        Some(face("f1")),
                        1,
                        at + Duration::from_secs(second),
                    ),
                    true,
                    at + Duration::from_secs(second),
                )
                .unwrap();
        }
        assert_eq!(
            controller.status(account(1), ticket, at + CAPTURE_FOR),
            Some(Status::Expired)
        );
    }

    #[test]
    fn final_attestation_consumes_one_work_item_and_completion_is_truthful() {
        let mut fixture = Fixture::new();
        let at = Instant::now();
        let (mut controller, ticket) = staged(&fixture, at);
        let now = at + Duration::from_secs(1);
        assert!(matches!(
            controller.begin_commit(account(1), ticket, false, now),
            Err(Error::Unattested)
        ));
        let work = controller
            .begin_commit(account(1), ticket, true, now)
            .unwrap();
        assert_eq!(work.request().parts(), ([3; 16], 1, 1));
        assert!(matches!(
            controller.begin_commit(account(1), ticket, true, now),
            Err(Error::WrongPhase)
        ));
        assert_eq!(
            controller.cancel(account(1), ticket, now),
            Err(Error::Committing)
        );
        assert_eq!(
            controller.observe(
                account(2),
                FrameObservation::from_source(None, 0, now),
                false,
                now
            ),
            Ok(Status::Committing)
        );
        assert_eq!(
            controller.status(account(1), ticket, at + TICKET_FOR),
            Some(Status::Committing)
        );
        let (target, prints, completion) = work.into_parts();
        let result = fixture.people.replace_prints(target, prints);
        assert_eq!(
            controller.complete(completion, result),
            Ok(Status::Committed)
        );
        assert_eq!(fixture.people.who(&print(2)).unwrap().0, "Fixture Target");
        assert!(
            controller
                .begin_commit(account(1), ticket, true, now)
                .is_err()
        );
        assert_eq!(
            controller.status(account(1), ticket, now),
            Some(Status::Committed)
        );
    }

    #[test]
    fn store_cas_and_prewrite_or_postrename_failures_never_retry_approval() {
        let mut fixture = Fixture::new();
        let at = Instant::now();
        let (mut controller, ticket) = staged(&fixture, at);
        fixture
            .people
            .remember("Fixture Other", vec![print(1)], 2)
            .unwrap();
        let (target, prints, completion) = controller
            .begin_commit(account(1), ticket, true, at + Duration::from_secs(1))
            .unwrap()
            .into_parts();
        let result = fixture.people.replace_prints(target, prints);
        assert_eq!(result, Err(RepairError::TargetChanged));
        assert_eq!(controller.complete(completion, result), Ok(Status::Refused));
        for (result, status) in [
            (Err(RepairError::NotWritten), Status::NotWritten),
            (Err(RepairError::Indeterminate), Status::Indeterminate),
        ] {
            let (mut controller, ticket) = staged(&fixture, at);
            let (_, _, completion) = controller
                .begin_commit(account(1), ticket, true, at + Duration::from_secs(1))
                .unwrap()
                .into_parts();
            assert_eq!(controller.complete(completion, result), Ok(status));
            assert!(
                controller
                    .begin_commit(account(1), ticket, true, at)
                    .is_err()
            );
        }
    }

    #[test]
    fn source_identity_shapes_and_unknown_account_tickets_fail_closed() {
        for (boot, id) in [
            ("old", "f1"),
            ("0123456789ABCDEF", "f1"),
            (BOOT, "f0"),
            (BOOT, "f-1"),
            (BOOT, "f4294967296"),
        ] {
            assert!(Candidate::from_source(boot, id).is_none());
        }
        assert!(AccountGeneration::from_trusted_ingress([0; 16]).is_none());
        assert!(Ticket::from_bytes([0; 16]).is_none());
        assert_eq!(face("f1").qualified(), "0123456789abcdef/f1");
        let fixture = Fixture::new();
        let at = Instant::now();
        let (mut controller, ticket) = started(&fixture, at);
        assert_eq!(controller.status(account(2), ticket, at), None);
        assert_eq!(
            controller.cancel(account(2), ticket, at),
            Err(Error::Unknown)
        );
        assert_eq!(
            controller.status(account(1), ticket, at),
            Some(Status::Waiting)
        );
        controller.cancel(account(1), ticket, at).unwrap();
        assert!(matches!(
            controller.start_with_entropy(account(1), fixture.target(), at, |bytes| {
                *bytes = *ticket.as_bytes();
                Ok(())
            }),
            Err(Error::Entropy)
        ));
    }

    #[test]
    fn a_fresh_frame_is_consumed_once_and_a_different_frame_subject_cancels() {
        let fixture = Fixture::new();
        let at = Instant::now();
        let (mut controller, ticket) = started(&fixture, at);
        controller
            .observe(
                account(1),
                FrameObservation::from_source(Some(face("f1")), 1, at),
                true,
                at,
            )
            .unwrap();
        controller
            .accept_consent(account(1), ticket, proof(ticket, face("f1"), at), at)
            .unwrap();
        let frame_at = at + Duration::from_millis(100);
        controller
            .observe(
                account(1),
                FrameObservation::from_source(Some(face("f1")), 1, frame_at),
                true,
                frame_at,
            )
            .unwrap();
        controller
            .push_print(account(1), ticket, face("f1"), frame_at, print(2), frame_at)
            .unwrap();
        assert_eq!(
            controller.push_print(account(1), ticket, face("f1"), frame_at, print(2), frame_at),
            Err(Error::StaleFrame)
        );
        assert_eq!(
            controller.push_print(account(1), ticket, face("f2"), frame_at, print(2), frame_at),
            Err(Error::Cancelled)
        );
        assert_eq!(
            controller.status(account(1), ticket, frame_at),
            Some(Status::Cancelled)
        );
    }

    #[test]
    fn staged_consent_expires_even_with_continuous_candidate_and_owner_polls() {
        let fixture = Fixture::new();
        let at = Instant::now();
        let (mut controller, ticket) = staged(&fixture, at);
        for second in 1..30 {
            let now = at + Duration::from_secs(second);
            assert_eq!(
                controller.observe(
                    account(1),
                    FrameObservation::from_source(Some(face("f1")), 1, now),
                    true,
                    now
                ),
                Ok(Status::Staged)
            );
            assert_eq!(
                controller.status(account(1), ticket, now),
                Some(Status::Staged)
            );
        }
        assert_eq!(
            controller.status(account(1), ticket, at + CONSENT_FOR),
            Some(Status::Expired)
        );
        assert!(
            controller
                .begin_commit(account(1), ticket, true, at + CONSENT_FOR)
                .is_err()
        );
    }

    #[test]
    fn delivery_time_does_not_refresh_a_delayed_or_future_source_frame() {
        let fixture = Fixture::new();
        let at = Instant::now();
        let (mut controller, ticket) = started(&fixture, at);
        assert_eq!(
            controller.observe(
                account(1),
                FrameObservation::from_source(Some(face("f1")), 1, at + Duration::from_secs(1)),
                true,
                at
            ),
            Err(Error::StaleFrame)
        );
        assert_eq!(
            controller.observe(
                account(1),
                FrameObservation::from_source(Some(face("f1")), 1, at),
                true,
                at + CANDIDATE_FOR
            ),
            Err(Error::StaleFrame)
        );
        assert_eq!(
            controller.status(account(1), ticket, at + CANDIDATE_FOR),
            Some(Status::Waiting)
        );
        let acquired = at + Duration::from_secs(1);
        let delivered = at + Duration::from_millis(3_500);
        assert_eq!(
            controller.observe(
                account(1),
                FrameObservation::from_source(Some(face("f1")), 1, acquired),
                true,
                delivered
            ),
            Ok(Status::Waiting)
        );
        assert_eq!(
            controller.status(account(1), ticket, acquired + CANDIDATE_FOR),
            Some(Status::Expired)
        );
    }

    #[test]
    fn replaying_one_frame_cannot_supply_new_freshness_or_five_prints() {
        let fixture = Fixture::new();
        let at = Instant::now();
        let (mut controller, ticket) = started(&fixture, at);
        controller
            .observe(
                account(1),
                FrameObservation::from_source(Some(face("f1")), 1, at),
                true,
                at,
            )
            .unwrap();
        controller
            .accept_consent(account(1), ticket, proof(ticket, face("f1"), at), at)
            .unwrap();
        let acquired = at + Duration::from_millis(100);
        let delivered = at + Duration::from_millis(200);
        controller
            .observe(
                account(1),
                FrameObservation::from_source(Some(face("f1")), 1, acquired),
                true,
                delivered,
            )
            .unwrap();
        controller
            .push_print(
                account(1),
                ticket,
                face("f1"),
                acquired,
                print(2),
                delivered,
            )
            .unwrap();
        for delay in 1..3 {
            let now = acquired + Duration::from_secs(delay);
            assert_eq!(
                controller.observe(
                    account(1),
                    FrameObservation::from_source(Some(face("f1")), 1, acquired),
                    true,
                    now
                ),
                Err(Error::StaleFrame)
            );
            assert_eq!(
                controller.push_print(account(1), ticket, face("f1"), now, print(2), now),
                Err(Error::StaleFrame)
            );
            assert_eq!(
                controller.push_print(account(1), ticket, face("f1"), acquired, print(2), now),
                Err(Error::StaleFrame)
            );
            assert_eq!(
                controller.status(account(1), ticket, now),
                Some(Status::Capturing)
            );
        }
        assert_eq!(
            controller.status(account(1), ticket, acquired + CANDIDATE_FOR),
            Some(Status::Expired)
        );
    }

    #[test]
    fn terminal_receipts_are_bounded_and_a_restart_remembers_no_ticket() {
        let fixture = Fixture::new();
        let at = Instant::now();
        let mut controller = Controller::new(true, BOOT).unwrap();
        let mut first = None;
        for index in 0..=MOST_RECEIPTS {
            let ticket = controller
                .start_with_entropy(account(1), fixture.target(), at, |bytes| {
                    bytes.fill(index as u8 + 1);
                    Ok(())
                })
                .unwrap();
            first.get_or_insert(ticket);
            controller.cancel(account(1), ticket, at).unwrap();
        }
        assert_eq!(controller.receipts.len(), MOST_RECEIPTS);
        assert_eq!(controller.status(account(1), first.unwrap(), at), None);
        let mut restarted = Controller::new(true, BOOT).unwrap();
        assert_eq!(restarted.status(account(1), first.unwrap(), at), None);
    }

    #[test]
    fn runtime_privacy_or_account_invalidation_needs_no_new_frame() {
        let fixture = Fixture::new();
        let original = std::fs::read(fixture.root.join("people.json")).unwrap();
        let at = Instant::now();
        for (phase, pinned) in [
            (Status::Waiting, false),
            (Status::Waiting, true),
            (Status::Capturing, true),
            (Status::Staged, true),
        ] {
            let (mut controller, ticket) = if phase == Status::Staged {
                staged(&fixture, at)
            } else {
                started(&fixture, at)
            };
            if pinned && phase != Status::Staged {
                controller
                    .observe(
                        account(1),
                        FrameObservation::from_source(Some(face("f1")), 1, at),
                        true,
                        at,
                    )
                    .unwrap();
            }
            if phase == Status::Capturing {
                controller
                    .accept_consent(account(1), ticket, proof(ticket, face("f1"), at), at)
                    .unwrap();
                let frame_at = at + Duration::from_millis(100);
                controller
                    .observe(
                        account(1),
                        FrameObservation::from_source(Some(face("f1")), 1, frame_at),
                        true,
                        frame_at,
                    )
                    .unwrap();
                controller
                    .push_print(account(1), ticket, face("f1"), frame_at, print(2), frame_at)
                    .unwrap();
                assert_eq!(controller.active.as_ref().unwrap().prints.len(), 1);
            }
            let now = at + Duration::from_secs(1);
            assert_eq!(controller.status(account(1), ticket, now), Some(phase));
            assert!(controller.active.as_ref().unwrap().target.is_some());
            // A trusted privacy/account transition calls this entry point
            // directly; neither a new frame nor the old account is required.
            assert_eq!(controller.invalidate(now), Ok(Status::Cancelled));
            assert!(
                controller.active.is_none(),
                "target, consent and prints dropped"
            );
            assert_eq!(
                controller.status(account(1), ticket, now),
                Some(Status::Cancelled)
            );
            assert_eq!(controller.status(account(2), ticket, now), None);
            for owner in [account(1), account(2)] {
                assert!(matches!(
                    controller.begin_commit(owner, ticket, true, now),
                    Err(Error::Unknown)
                ));
                assert_eq!(
                    controller.accept_consent(owner, ticket, proof(ticket, face("f1"), at), now),
                    Err(Error::Unknown)
                );
                assert_eq!(
                    controller.push_print(owner, ticket, face("f1"), now, print(2), now),
                    Err(Error::Unknown)
                );
            }
            assert_eq!(
                std::fs::read(fixture.root.join("people.json")).unwrap(),
                original
            );
        }
    }

    #[test]
    fn runtime_invalidation_preserves_off_empty_and_expired_status() {
        let fixture = Fixture::new();
        let at = Instant::now();
        let mut off = Controller::new(false, BOOT).unwrap();
        assert_eq!(off.invalidate(at), Err(Error::Off));
        assert!(off.active.is_none() && off.receipts.is_empty());
        let mut empty = Controller::new(true, BOOT).unwrap();
        assert_eq!(empty.invalidate(at), Err(Error::Unknown));
        assert!(empty.receipts.is_empty());

        let (mut controller, ticket) = started(&fixture, at);
        assert_eq!(controller.invalidate(at), Ok(Status::Cancelled));
        assert_eq!(controller.invalidate(at), Err(Error::Unknown));
        assert_eq!(controller.receipts.len(), 1);
        assert_eq!(
            controller.status(account(1), ticket, at),
            Some(Status::Cancelled)
        );

        let (mut controller, ticket) = started(&fixture, at);
        assert_eq!(controller.invalidate(at + TICKET_FOR), Err(Error::Unknown));
        assert_eq!(
            controller.status(account(1), ticket, at + TICKET_FOR),
            Some(Status::Expired)
        );
    }

    #[test]
    fn runtime_invalidation_cannot_roll_back_a_commit_handoff() {
        let mut fixture = Fixture::new();
        let at = Instant::now();
        let (mut controller, ticket) = staged(&fixture, at);
        let work = controller
            .begin_commit(account(1), ticket, true, at + Duration::from_secs(1))
            .unwrap();
        // Even after every pre-commit deadline, runtime invalidation cannot
        // recover consumed work or claim that an in-flight write was cancelled.
        let now = at + TICKET_FOR;
        assert_eq!(controller.invalidate(now), Ok(Status::Committing));
        assert_eq!(controller.invalidate(now), Ok(Status::Committing));
        assert_eq!(
            controller.status(account(1), ticket, now),
            Some(Status::Committing)
        );
        assert_eq!(
            controller.cancel(account(1), ticket, now),
            Err(Error::Committing)
        );
        assert!(matches!(
            controller.begin_commit(account(1), ticket, true, now),
            Err(Error::WrongPhase)
        ));
        assert!(controller.active.as_ref().unwrap().target.is_none());
        assert!(controller.active.as_ref().unwrap().prints.is_empty());
        let (target, prints, completion) = work.into_parts();
        assert_eq!(prints.len(), MOST_PRINTS);
        let result = fixture.people.replace_prints(target, prints);
        assert_eq!(
            controller.complete(completion, result),
            Ok(Status::Committed)
        );
        assert_eq!(fixture.people.who(&print(2)).unwrap().0, "Fixture Target");
        assert_eq!(controller.invalidate(now), Err(Error::Unknown));
        assert_eq!(
            controller.status(account(1), ticket, now),
            Some(Status::Committed)
        );
    }
}
