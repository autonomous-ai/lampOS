//! Completed fixed lines, kept only by their original worker and conversation scope.
//! No entry here grants input, tool, memory-write or playback authority.
use crate::memory_scope::{HistoryScopeProof, ScopeLease};
use crate::say::FixedLine;
use lamp_msgs::FixedLineOrigin;
use lamp_turn_owner::TurnOwner;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

pub(crate) const DELIVERY_US: u64 = 100_000;
const HISTORY_US: u64 = 600_000_000;
const ORIGINS: usize = 16;
const LINES: usize = 12;

/// The parent's attestation follows final playback retirement, never merely begun.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub line: FixedLine,
    pub owner: TurnOwner,
    pub played_at_us: u64,
}
impl Receipt {
    pub fn current(&self, now: u64) -> bool {
        self.line.check()
            && self.owner.turn() > self.line.origin.request
            && self.played_at_us >= self.line.origin.issued_us
            && now >= self.played_at_us
            && now - self.played_at_us <= DELIVERY_US
            && now.saturating_sub(self.line.origin.issued_us) <= HISTORY_US
    }
}

#[derive(Default)]
struct RouteState {
    epoch: u64,
    active: bool,
    privacy: Option<(bool, u64, bool, u64)>,
    ready_session: Option<u64>,
}
/// Shared only within one Engine. Invalidating it also invalidates copied messages.
#[derive(Default)]
pub(crate) struct Route {
    state: Mutex<RouteState>,
    version: Arc<AtomicU64>,
    authority_until: Arc<AtomicU64>,
}
impl Route {
    pub fn authority(&self, until_us: u64) {
        self.authority_until.store(until_us, Ordering::Release);
    }
    fn changed(&self) {
        // Exhaustion leaves every future token invalid instead of wrapping.
        let _ = self
            .version
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |v| {
                Some(v.saturating_add(1))
            });
    }
    pub fn activate(&self, epoch: u64, active: bool) -> std::io::Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| std::io::Error::other("history route poisoned"))?;
        if epoch == 0 || epoch < state.epoch || (active && !state.active && epoch == state.epoch) {
            return Err(std::io::Error::other("stale history route activation"));
        }
        if state.epoch != epoch || state.active != active {
            self.changed();
            state.epoch = epoch;
            state.active = active;
        }
        Ok(())
    }
    pub fn privacy(&self, privacy: (bool, u64, bool, u64)) {
        if let Ok(mut state) = self.state.lock() {
            if state.privacy != Some(privacy) {
                self.changed();
                state.privacy = Some(privacy);
            }
        } else {
            self.version.store(u64::MAX, Ordering::Release);
        }
    }
    /// Provider reconnects can change session without replacing this worker.
    /// Revoke before its link update is published; repeated readiness is inert.
    pub fn link(&self, link: &crate::provider_link::ProviderLink) {
        let ready_session = matches!(
            link.state,
            crate::provider_link::ProviderLinkState::Ready { .. }
        )
        .then_some(link.session)
        .filter(|session| *session != 0);
        if let Ok(mut state) = self.state.lock() {
            if state.ready_session != ready_session {
                self.changed();
                state.ready_session = ready_session;
            }
        } else {
            self.version.store(u64::MAX, Ordering::Release);
        }
    }
    pub fn origin(&self, context: &lamp_msgs::RequestedAudioContext) -> Option<FixedLineOrigin> {
        let state = self.state.try_lock().ok()?;
        let (_, camera_generation, microphone, microphone_generation) = state.privacy?;
        if !state.active
            || state.ready_session != Some(context.provider_session)
            || !microphone
            || microphone_generation != context.privacy_generation
            || lamp_ipc::monotonic_us() >= self.authority_until.load(Ordering::Acquire)
        {
            return None;
        }
        Some(FixedLineOrigin {
            worker_boot: context.worker_boot.clone(),
            provider_session: context.provider_session,
            request: context.request,
            call_id: context.call_id.clone(),
            route_epoch: state.epoch,
            microphone_generation,
            camera_generation,
            issued_us: context.issued_us,
        })
    }
    fn stamp(&self, origin: &FixedLineOrigin) -> Option<(Arc<AtomicU64>, u64)> {
        let state = self.state.try_lock().ok()?;
        let (_, camera, microphone, generation) = state.privacy?;
        let version = self.version.load(Ordering::Acquire);
        (state.active
            && state.ready_session == Some(origin.provider_session)
            && state.epoch == origin.route_epoch
            && microphone
            && generation == origin.microphone_generation
            && camera == origin.camera_generation
            && version != u64::MAX
            && lamp_ipc::monotonic_us() < self.authority_until.load(Ordering::Acquire))
        .then(|| (self.version.clone(), version))
    }
}

/// Local provenance on a history message; serde never sends it to the model.
/// Forget revokes this exact token, including copies already in a delayed retry.
pub(crate) struct Proof {
    scope: HistoryScopeProof,
    live: AtomicBool,
    route: Arc<AtomicU64>,
    version: u64,
    issued_us: u64,
    expires_us: u64,
    authority_until: Arc<AtomicU64>,
    forget_revision: u64,
}
impl std::fmt::Debug for Proof {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FixedLineHistoryProof")
    }
}
impl PartialEq for Proof {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}
impl Proof {
    pub fn valid(&self) -> bool {
        self.scope.valid() && self.binding_valid_at(lamp_ipc::monotonic_us())
    }
    #[cfg(test)]
    fn valid_at(&self, now_us: u64) -> bool {
        self.scope.valid() && self.binding_valid_at(now_us)
    }
    fn binding_valid_at(&self, now_us: u64) -> bool {
        self.binding_valid_at_with(now_us, || {})
    }
    fn binding_valid_at_with(&self, now_us: u64, between: impl FnOnce()) -> bool {
        let version = self.route.load(Ordering::Acquire);
        // A deterministic test interleaves a renewal here; production's no-op
        // is optimized away. Recheck after the deadline read so fresh authority
        // cannot be combined with a version sampled before privacy/route change.
        between();
        self.live.load(Ordering::Acquire)
            && version == self.version
            && now_us < self.authority_until.load(Ordering::Acquire)
            && crate::remember::fixed_history_revision() == Some(self.forget_revision)
            && now_us >= self.issued_us
            && now_us <= self.expires_us
            && self.route.load(Ordering::Acquire) == version
    }
    fn revoke(&self) {
        self.live.store(false, Ordering::Release);
    }
    fn belongs_to(&self, person: Option<&str>) -> bool {
        self.scope.matches_scope(person) && self.binding_valid_at(lamp_ipc::monotonic_us())
    }
}
struct Origin {
    context: FixedLineOrigin,
    proof: Arc<Proof>,
    at: Instant,
}
struct Played {
    owner: TurnOwner,
    at: Instant,
    text: String,
    played_at_us: u64,
    proof: Arc<Proof>,
}
/// Owned by Thread but never included in its save, journal, summary or recall.
pub(crate) struct History {
    origins: VecDeque<Origin>,
    played: VecDeque<Played>,
    highest_playback: u64,
}
impl History {
    pub const fn new() -> Self {
        Self {
            origins: VecDeque::new(),
            played: VecDeque::new(),
            highest_playback: 0,
        }
    }
    pub fn clear(&mut self) {
        for origin in &self.origins {
            origin.proof.revoke();
        }
        self.origins.clear();
        self.played.clear();
    }
    /// An explicit conversation close revokes only origins already issued at
    /// its cutoff. Keep revoked identities until bounded eviction, so the same
    /// call cannot register again after a close; copied messages share revocation.
    pub fn close_at(&mut self, cutoff: Instant) {
        for origin in &self.origins {
            if origin.at <= cutoff {
                origin.proof.revoke();
            }
        }
        self.played.retain(|played| played.proof.valid());
    }
    pub fn register(&mut self, origin: FixedLineOrigin, lease: &ScopeLease, route: &Route) -> bool {
        let key = lease.key();
        if origin.check().is_err()
            || key.request != origin.request
            || key.session != origin.provider_session
            || crate::process::boot_text(key.worker) != origin.worker_boot
        {
            return false;
        }
        let issued_us = origin.issued_us;
        // Sample Instant first, then the shared clock: conversion cannot date
        // an old origin after a close merely because the samples were delayed.
        let observed = Instant::now();
        let Some(age_us) = lamp_ipc::monotonic_us().checked_sub(issued_us) else {
            return false;
        };
        if age_us > HISTORY_US {
            return false;
        }
        let Some(at) = observed.checked_sub(Duration::from_micros(age_us)) else {
            return false;
        };
        let Some(scope) = lease.history_proof() else {
            return false;
        };
        let authority_until = route.authority_until.clone();
        let Some(forget_revision) = crate::remember::fixed_history_revision() else {
            return false;
        };
        let Some((route, version)) = route.stamp(&origin) else {
            return false;
        };
        if let Some(existing) = self.origins.iter().find(|o| o.context.same_call(&origin)) {
            return existing.context == origin && existing.proof.valid();
        }
        if self.origins.len() == ORIGINS
            && let Some(old) = self.origins.pop_front()
        {
            old.proof.revoke();
        }
        self.played.retain(|p| p.proof.valid());
        let expires_us = issued_us.saturating_add(HISTORY_US);
        self.origins.push_back(Origin {
            context: origin,
            at,
            proof: Arc::new(Proof {
                scope,
                live: AtomicBool::new(true),
                route,
                version,
                issued_us,
                expires_us,
                authority_until,
                forget_revision,
            }),
        });
        true
    }
    /// Synchronous ordered-lane insertion. It never consumes pending heard words.
    pub fn accept(&mut self, receipt: &Receipt, now_us: u64, now: Instant) -> bool {
        if !receipt.current(now_us) {
            return false;
        }
        let Some(origin) = self
            .origins
            .iter()
            .find(|o| o.context == receipt.line.origin && o.proof.valid())
        else {
            return false;
        };
        if let Some(prior) = self.played.iter().find(|p| p.owner == receipt.owner) {
            return prior.text == receipt.line.text
                && prior.played_at_us == receipt.played_at_us
                && Arc::ptr_eq(&prior.proof, &origin.proof);
        }
        if receipt.owner.turn() <= self.highest_playback {
            return false;
        }
        let Some(at) = now.checked_sub(Duration::from_micros(now_us - receipt.played_at_us)) else {
            return false;
        };
        if self.played.len() == LINES {
            self.played.pop_front();
        }
        self.highest_playback = receipt.owner.turn();
        self.played.push_back(Played {
            owner: receipt.owner,
            at,
            played_at_us: receipt.played_at_us,
            text: receipt.line.text.clone(),
            proof: origin.proof.clone(),
        });
        true
    }
    pub fn recent(
        &self,
        person: Option<&str>,
        now: Instant,
    ) -> Vec<(Instant, crate::thread::Recent)> {
        self.played
            .iter()
            .filter(|p| {
                p.proof.belongs_to(person)
                    && now.saturating_duration_since(p.at) <= Duration::from_micros(HISTORY_US)
            })
            .map(|p| {
                (
                    p.at,
                    crate::thread::Recent {
                        lamp: p.text.clone(),
                        fixed_line: Some(p.proof.clone()),
                        ..Default::default()
                    },
                )
            })
            .collect()
    }
}
impl Drop for History {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests;
