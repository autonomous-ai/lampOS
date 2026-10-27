//! Runtime-only face consent. A model call or transcript cannot mint it.
use super::{Generation, Proof};
use crate::memory_scope::RequestKey;
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, Weak, atomic::Ordering};

/// Same bounded reply window as onboarding's existing name read-back.
pub(crate) const WITHIN_US: u64 = crate::onboarding::ASK_BACK_AFTER_US;
pub(crate) const QUESTION: &str = "May I remember your face, so I know you next time? If you'd like that, look at me and say, 'Yes, my name is ...' with your name.";
pub(crate) const QUESTION_NOTE: &str = r#"(Say exactly this face-consent question, nothing before or after it: "May I remember your face, so I know you next time? If you'd like that, look at me and say, 'Yes, my name is ...' with your name.")"#;

/// Internal provider/coordinator message, never a tool argument. No heard words.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub(crate) worker: [u8; 16],
    pub(crate) session: u64,
    pub(crate) request: u64,
    pub(crate) face: String,
    pub(crate) epoch: u64,
    pub(crate) issued_us: u64,
}
impl Candidate {
    pub(crate) fn fresh(&self, now_us: u64) -> bool {
        self.issued_us > 0
            && now_us >= self.issued_us
            && now_us - self.issued_us < super::FRESH_US
            && self.face.len() <= super::FACE_BYTES
    }
}

/// Full source domain for assistant output. Numeric render turns alone do
/// not identify a request across workers or provider sessions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputKey {
    pub(crate) worker: [u8; 16],
    pub(crate) session: u64,
    pub(crate) request: u64,
}
impl OutputKey {
    pub(crate) fn from_key(key: RequestKey) -> Self {
        Self {
            worker: key.worker.into(),
            session: key.session,
            request: key.request,
        }
    }
}

struct Pending {
    candidate: Candidate,
    generation: Generation,
    /// Only a matching, fully played typed question supplies this time.
    delivered: Option<u64>,
    question: Option<([u8; 16], u64)>,
}
worker_global! {
    fn pending() -> Mutex<Option<Pending>> = Mutex::new(None);
}

/// One weak link closes the IPC race between raw B proof admission and an
/// already-observed intervening output's cancellation. It keeps no lease alive.
struct Consumed {
    candidate: Candidate,
    question: ([u8; 16], u64),
    key: RequestKey,
    token: Weak<Mutex<Option<RequestKey>>>,
    active: Weak<std::sync::atomic::AtomicBool>,
}
worker_global! {
    fn consumed() -> Mutex<Option<Consumed>> = Mutex::new(None);
}
fn revoke(old: Consumed) {
    // Preserve the existing final pre-wire and in-flight proof fence even
    // after the one-shot token was taken by a tool invocation.
    if let Some(active) = old.active.upgrade() {
        active.store(false, Ordering::Release);
    }
    if let Some(token) = old.token.upgrade()
        && let Ok(mut grant) = token.lock()
        && *grant == Some(old.key)
    {
        grant.take();
    }
}
fn cancel_consumed(
    candidate: &Candidate,
    question: Option<([u8; 16], u64)>,
    by_output: Option<OutputKey>,
) {
    if let Ok(mut slot) = consumed().lock()
        && slot.as_ref().is_some_and(|old| {
            old.candidate == *candidate
                && question.is_none_or(|question| old.question == question)
                && by_output != Some(OutputKey::from_key(old.key))
        })
        && let Some(old) = slot.take()
    {
        revoke(old);
    }
}

/// Consume only this exact current request's one-shot proof, including
/// worker/session identity; an internal proof clone cannot lend it elsewhere.
pub(crate) fn take(proof: &Proof, key: RequestKey) -> bool {
    proof.consent.lock().ok().and_then(|mut grant| grant.take()) == Some(key)
}

pub(crate) fn clear() {
    if let Ok(mut pending) = pending().lock() {
        pending.take();
    }
    if let Ok(mut consumed) = consumed().lock()
        && let Some(old) = consumed.take()
    {
        revoke(old);
    }
}

/// Called after the raw current proof exists, before it is published or carries
/// are joined. Consume at most one invitation; an unrelated/other person's
/// reply cannot leave permission waiting for a later yes.
pub(crate) fn heard(proof: &mut Proof, key: RequestKey, now_us: u64) -> Option<Candidate> {
    let Ok(mut slot) = pending().lock() else {
        return None;
    };
    if let Some(old) = slot.take() {
        let c = &old.candidate;
        let same = c.worker == <[u8; 16]>::from(key.worker)
            && c.session == key.session
            && c.request != key.request
            && c.face == proof.face
            && c.epoch == proof.generation.value
            && old.generation.same_epoch(proof.generation)
            && c.epoch == old.generation.epoch.load(Ordering::Acquire);
        if let Some(delivered) = old.delivered
            && same
            && proof.began_us > delivered
            && now_us >= delivered
            && now_us - delivered < WITHIN_US
            && crate::known_faces::face_agreement(&proof.heard)
            && let Some(question) = old.question
        {
            if let Ok(mut consumed) = consumed().lock()
                && let Ok(mut grant) = proof.consent.lock()
            {
                *grant = Some(key);
                *consumed = Some(Consumed {
                    candidate: old.candidate,
                    question,
                    key,
                    token: std::sync::Arc::downgrade(&proof.consent),
                    active: std::sync::Arc::downgrade(&proof.active),
                });
            }
            return None;
        }
    }
    if !proof.is_current(now_us) || !crate::known_faces::may_ask_face_consent(&proof.heard) {
        return None;
    }
    let candidate = Candidate {
        worker: key.worker.into(),
        session: key.session,
        request: key.request,
        face: proof.face.clone(),
        epoch: proof.generation.value,
        issued_us: proof.ended_us,
    };
    *slot = Some(Pending {
        candidate: candidate.clone(),
        generation: proof.generation,
        delivered: None,
        question: None,
    });
    Some(candidate)
}

/// Only the coordinator's exact typed invitation completion, not an ordinary
/// provider transcript, may arm the separate one-shot question lifetime.
pub(crate) fn bind(candidate: &Candidate, boot: [u8; 16], request: u64) -> bool {
    let Ok(mut slot) = pending().lock() else {
        return false;
    };
    let Some(old) = slot.as_mut().filter(|old| {
        old.candidate == *candidate
            && candidate.fresh(lamp_ipc::monotonic_us())
            && old.generation.value == old.generation.epoch.load(Ordering::Acquire)
    }) else {
        return false;
    };
    if old.question.is_some() {
        return false;
    }
    old.question = Some((boot, request));
    true
}

#[cfg(test)]
pub(crate) fn delivered(
    candidate: &Candidate,
    boot: [u8; 16],
    request: u64,
    whole: bool,
    delivered_us: u64,
) {
    delivered_from(candidate, boot, request, whole, delivered_us, None);
}
pub(crate) fn delivered_from(
    candidate: &Candidate,
    boot: [u8; 16],
    request: u64,
    whole: bool,
    delivered_us: u64,
    by_output: Option<OutputKey>,
) {
    let Ok(mut slot) = pending().lock() else {
        return;
    };
    if !whole {
        cancel_consumed(candidate, Some((boot, request)), by_output);
    }
    if !slot
        .as_ref()
        .is_some_and(|old| old.candidate == *candidate && old.question == Some((boot, request)))
    {
        return;
    }
    if whole && slot.as_ref().is_some_and(|old| old.delivered.is_some()) {
        return; // Duplicate delivery cannot renew the one-shot reply window.
    }
    let Some(old) = slot.take() else { return };
    if whole
        && candidate.fresh(delivered_us)
        && old.generation.value == old.generation.epoch.load(Ordering::Acquire)
    {
        *slot = Some(Pending {
            delivered: Some(delivered_us),
            ..old
        });
    }
}

/// Expiring the question's reply window cannot shorten the independent
/// lifetime of a B proof already admitted within that window.
pub(crate) fn expired(candidate: &Candidate, boot: [u8; 16], request: u64) {
    if let Ok(mut slot) = pending().lock()
        && slot
            .as_ref()
            .is_some_and(|old| old.candidate == *candidate && old.question == Some((boot, request)))
    {
        slot.take();
    }
}

/// A dropped/cut invitation cannot borrow a later successful delivery.
pub(crate) fn canceled(candidate: &Candidate) {
    cancel_consumed(candidate, None, None);
    if let Ok(mut slot) = pending().lock()
        && slot.as_ref().is_some_and(|old| old.candidate == *candidate)
    {
        slot.take();
    }
}

/// Coordinator-only purpose-specific output, kept separate from the caller's
/// completed request lease. It cannot be armed by generic transcript events.
#[derive(Default)]
pub(crate) struct Questions {
    current: Option<Question>,
}
struct Question {
    candidate: Candidate,
    privacy: u64,
    source_done: bool,
    delivered: Option<u64>,
    bound: Option<([u8; 16], u64)>,
}
impl Questions {
    pub(crate) fn offer(&mut self, candidate: Candidate, privacy: u64, now_us: u64) {
        self.current = candidate.fresh(now_us).then_some(Question {
            candidate,
            privacy,
            source_done: false,
            delivered: None,
            bound: None,
        });
    }
    pub(crate) fn source_finished(&mut self, request: u64, completed: bool) {
        if let Some(q) = self
            .current
            .as_mut()
            .filter(|q| q.candidate.request == request)
        {
            if completed {
                q.source_done = true;
            } else {
                self.current = None;
            }
        }
    }
    pub(crate) fn ready(&self, privacy: u64, now_us: u64) -> Option<Candidate> {
        self.current
            .as_ref()
            .filter(|q| {
                q.source_done
                    && q.bound.is_none()
                    && q.privacy == privacy
                    && q.candidate.fresh(now_us)
            })
            .map(|q| q.candidate.clone())
    }
    pub(crate) fn bind(&mut self, candidate: &Candidate, boot: [u8; 16], turn: u64) -> bool {
        let Some(q) = self
            .current
            .as_mut()
            .filter(|q| q.candidate == *candidate && q.source_done && q.bound.is_none())
        else {
            return false;
        };
        q.bound = Some((boot, turn));
        true
    }
    pub(crate) fn finished(
        &mut self,
        boot: [u8; 16],
        turn: u64,
        whole: bool,
        now_us: u64,
    ) -> Option<crate::provider_worker::ProviderInput> {
        if self
            .current
            .as_ref()
            .is_none_or(|q| q.bound != Some((boot, turn)))
        {
            return None;
        }
        let q = self.current.as_mut()?;
        if whole && q.delivered.is_some() {
            return None;
        }
        let receipt = crate::provider_worker::ProviderInput::FaceConsentDelivery {
            request: turn,
            privacy_generation: q.privacy,
            candidate: q.candidate.clone(),
            voice_boot: boot,
            delivered_us: now_us,
            whole,
            by_output: None,
            pending_only: false,
        };
        if whole {
            q.delivered = Some(now_us);
        } else {
            self.current = None;
        }
        Some(receipt)
    }
    /// Explicit user cancellation also applies after the question's owner retired.
    pub(crate) fn cancel(&mut self, now_us: u64) -> Option<crate::provider_worker::ProviderInput> {
        if let Some((boot, turn)) = self.current.as_ref().and_then(|q| q.bound) {
            self.finished(boot, turn, false, now_us)
        } else {
            self.current = None;
            None
        }
    }
    /// Another assistant output cannot leave a prior face question waiting.
    /// B's already-minted request grant is independent and is not revoked here.
    pub(crate) fn other_output(
        &mut self,
        boot: [u8; 16],
        output: OutputKey,
        now_us: u64,
    ) -> Option<crate::provider_worker::ProviderInput> {
        if self
            .current
            .as_ref()
            .is_some_and(|q| q.delivered.is_some() && q.bound != Some((boot, output.request)))
        {
            let mut receipt = self.cancel(now_us)?;
            if let crate::provider_worker::ProviderInput::FaceConsentDelivery {
                by_output, ..
            } = &mut receipt
            {
                *by_output = Some(output);
            }
            Some(receipt)
        } else {
            None
        }
    }
    pub(crate) fn unsettled(
        &mut self,
        current: Option<([u8; 16], u64)>,
        privacy: u64,
        now_us: u64,
    ) -> Option<crate::provider_worker::ProviderInput> {
        let q = self.current.as_ref()?;
        if let Some((boot, turn)) = q.bound {
            let expired = q
                .delivered
                .is_some_and(|at| now_us < at || now_us - at >= WITHIN_US);
            if privacy != q.privacy || (q.delivered.is_none() && current != Some((boot, turn))) {
                return self.finished(boot, turn, false, now_us);
            }
            if expired {
                let mut receipt = self.finished(boot, turn, false, now_us)?;
                if let crate::provider_worker::ProviderInput::FaceConsentDelivery {
                    pending_only,
                    ..
                } = &mut receipt
                {
                    *pending_only = true;
                }
                return Some(receipt);
            }
        } else if !q.candidate.fresh(now_us)
            || privacy != q.privacy
            || (!q.source_done && current.is_none())
            || current.is_some_and(|(_, turn)| turn != q.candidate.request)
        {
            self.current = None;
        }
        None
    }
}
