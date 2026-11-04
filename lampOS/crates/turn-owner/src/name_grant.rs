//! A name-only listening grant (migrate-P2, the Queue comment
//! <https://github.com/autonomous-ai/lampOS/issues/3997#issuecomment-6526292114>).
//!
//! A lamp put to sleep by a hand held on its head may still wake when it
//! hears its name, with `LAMP_SLEEP_HEARS_NAME` and the privacy slide open.
//! The reviewer's condition was "don't capture under Denied": the privacy
//! wait's [`crate::Controller`] keeps its microphone and camera permissions
//! `Denied` throughout, so `voice/privacy` still says not permitted, Ears
//! stays closed and a capture worker under a [`crate::Snapshot`] stays
//! silent. This grant is a second, separate authority beside it, for one
//! narrow worker only: the sleep capture, which reads one microphone and
//! hands its blocks to the name listener, with no speaker, no echo
//! reference, no speech detector and no speech to text.
//!
//! It is a lease, as a snapshot is: issued with a strictly increasing
//! revision, never longer than [`MAX_AUTHORITY_LEASE_US`], renewed by the
//! wait like its other authority, and revoked at once when the slide is no
//! longer observed open and fresh. A worker holds it through a
//! [`NameGrantGuard`], which refuses another controller's grant, an older
//! revision and one issued in its future, and lets the microphone be read
//! only while the newest grant is granted and unexpired.
use crate::{BootId, Error, MAX_AUTHORITY_LEASE_US, MonoTime};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU64;

/// One name-only grant, or its revocation (`granted: false`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NameGrant {
    boot: BootId,
    revision: NonZeroU64,
    issued_at: MonoTime,
    expires_at: MonoTime,
    granted: bool,
}

impl NameGrant {
    pub fn boot(self) -> BootId {
        self.boot
    }
    pub fn revision(self) -> u64 {
        self.revision.get()
    }
    pub fn issued_at(self) -> MonoTime {
        self.issued_at
    }
    pub fn expires_at(self) -> MonoTime {
        self.expires_at
    }
    /// Whether this one grants at all: a revocation never does.
    pub fn granted(self) -> bool {
        self.granted
    }
}

/// The privacy wait's issuer of name-only grants. It has no microphone
/// permission of its own and changes none of the controller's.
#[derive(Debug)]
pub struct NameGrantIssuer {
    boot: BootId,
    revision: u64,
}

impl NameGrantIssuer {
    pub fn new(boot: BootId) -> Self {
        Self { boot, revision: 0 }
    }

    /// A grant from `now` for [`MAX_AUTHORITY_LEASE_US`]: the slide was
    /// just observed open and fresh. Call again before it expires.
    pub fn grant(&mut self, now: MonoTime) -> Result<NameGrant, Error> {
        self.issue(now, true)
    }

    /// A revocation, which ends any earlier grant at once: the slide closed,
    /// went stale, or the sleep it was for is over.
    pub fn revoke(&mut self, now: MonoTime) -> Result<NameGrant, Error> {
        self.issue(now, false)
    }

    fn issue(&mut self, now: MonoTime, granted: bool) -> Result<NameGrant, Error> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(Error::CounterExhausted)?;
        let revision = NonZeroU64::new(self.revision).ok_or(Error::CounterExhausted)?;
        let expires_at = if granted {
            now.checked_add(MAX_AUTHORITY_LEASE_US)
                .ok_or(Error::InvalidLease)?
        } else {
            now
        };
        Ok(NameGrant {
            boot: self.boot,
            revision,
            issued_at: now,
            expires_at,
            granted,
        })
    }
}

/// The sleep capture worker's hold on its grants: bound to the controller
/// that started it, as a [`crate::BoundaryGuard`] is.
#[derive(Debug)]
pub struct NameGrantGuard {
    expected_boot: BootId,
    current: Option<NameGrant>,
}

impl NameGrantGuard {
    pub fn new(expected_boot: BootId) -> Self {
        Self {
            expected_boot,
            current: None,
        }
    }

    /// Take `grant`, received at `now`. Another controller's, an older or
    /// repeated revision, one issued after `now` and one whose lease is above
    /// its bound are refused; a refusal leaves no grant in force.
    pub fn install(&mut self, now: MonoTime, grant: NameGrant) -> Result<(), Error> {
        if grant.boot != self.expected_boot {
            self.current = None;
            return Err(Error::WrongBoot);
        }
        if self
            .current
            .is_some_and(|current| grant.revision <= current.revision)
        {
            self.current = None;
            return Err(Error::StaleSnapshot);
        }
        if grant.issued_at > now {
            self.current = None;
            return Err(Error::FutureState);
        }
        let lease = grant.expires_at.elapsed_since(grant.issued_at);
        if lease.is_none_or(|lease| lease > MAX_AUTHORITY_LEASE_US) {
            self.current = None;
            return Err(Error::InvalidLease);
        }
        self.current = Some(grant);
        Ok(())
    }

    /// Whether the microphone may be read at `now`: the newest grant is a
    /// grant, and has not expired.
    pub fn allows(&self, now: MonoTime) -> bool {
        self.current
            .is_some_and(|grant| grant.granted && now < grant.expires_at)
    }

    /// Whether a grant was ever taken: a worker that has had none yet is
    /// still starting.
    pub fn ever_granted(&self) -> bool {
        self.current.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boot(byte: u8) -> BootId {
        BootId::new([byte; 16]).unwrap()
    }
    fn at(us: u64) -> MonoTime {
        MonoTime::from_micros(us)
    }

    #[test]
    fn a_grant_allows_only_until_its_lease_ends() {
        let mut issuer = NameGrantIssuer::new(boot(1));
        let mut guard = NameGrantGuard::new(boot(1));
        assert!(!guard.allows(at(1_000)));
        guard
            .install(at(1_000), issuer.grant(at(1_000)).unwrap())
            .unwrap();
        assert!(guard.allows(at(1_000 + MAX_AUTHORITY_LEASE_US - 1)));
        assert!(!guard.allows(at(1_000 + MAX_AUTHORITY_LEASE_US)));
    }

    #[test]
    fn a_revocation_ends_the_grant_at_once() {
        let mut issuer = NameGrantIssuer::new(boot(1));
        let mut guard = NameGrantGuard::new(boot(1));
        guard
            .install(at(10), issuer.grant(at(10)).unwrap())
            .unwrap();
        guard
            .install(at(20), issuer.revoke(at(20)).unwrap())
            .unwrap();
        assert!(!guard.allows(at(20)));
        assert!(!guard.allows(at(21)));
    }

    #[test]
    fn another_controllers_old_or_future_grant_is_refused_and_leaves_none() {
        let mut issuer = NameGrantIssuer::new(boot(1));
        let mut other = NameGrantIssuer::new(boot(2));
        let mut guard = NameGrantGuard::new(boot(1));
        let first = issuer.grant(at(10)).unwrap();
        let second = issuer.grant(at(20)).unwrap();
        guard.install(at(20), second).unwrap();
        assert_eq!(guard.install(at(21), first), Err(Error::StaleSnapshot));
        assert!(!guard.allows(at(21)));
        guard
            .install(at(30), issuer.grant(at(30)).unwrap())
            .unwrap();
        assert_eq!(
            guard.install(at(31), other.grant(at(31)).unwrap()),
            Err(Error::WrongBoot)
        );
        assert!(!guard.allows(at(31)));
        let future = issuer.grant(at(500)).unwrap();
        assert_eq!(guard.install(at(40), future), Err(Error::FutureState));
        assert!(!guard.allows(at(40)));
    }

    #[test]
    fn a_grant_keeps_its_shape_on_the_wire_and_refuses_extra_fields() {
        let mut issuer = NameGrantIssuer::new(boot(1));
        let grant = issuer.grant(at(10)).unwrap();
        let text = serde_json::to_string(&grant).unwrap();
        assert_eq!(serde_json::from_str::<NameGrant>(&text).unwrap(), grant);
        let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
        value["microphone"] = serde_json::json!("allowed");
        assert!(serde_json::from_value::<NameGrant>(value).is_err());
    }
}
