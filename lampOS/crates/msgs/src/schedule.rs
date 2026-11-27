//! Metadata for the Schedule owner's durable due queue. A bus value is neither
//! a durable delivery record nor permission to speak; the owner serves the
//! exact notice over its private socket and records completion there.
use crate::{ClockStamp, Invalid};
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Pending notices and schedules waiting for a usable clock share this bound.
pub const SCHEDULE_MAX_ITEMS: u32 = 128;
/// Last millisecond of 9999-12-31 UTC.
pub const SCHEDULE_MAX_UNIX_MS: i64 = 253_402_300_799_999;

/// One pending occurrence, without the person's reminder text. The store keeps
/// this identity until an explicit durable acknowledgement, cancellation, or
/// snooze. Renewing a bus heartbeat does not create another delivery.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleNotice {
    pub id: u64,
    pub delivery: u64,
    /// Scheduled instant, in milliseconds since 1970 UTC, through year 9999.
    pub due_unix_ms: i64,
    /// Current known lateness in milliseconds; absent when the clock cannot
    /// establish it. Absence must not be interpreted as zero lateness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub late_by_ms: Option<u64>,
    pub late: bool,
}

impl ScheduleNotice {
    pub fn check(&self) -> Result<(), Invalid> {
        if self.id == 0 || self.delivery == 0 {
            return Err(Invalid("schedule id and delivery must be positive".into()));
        }
        if !(0..=SCHEDULE_MAX_UNIX_MS).contains(&self.due_unix_ms) {
            return Err(Invalid(
                "schedule due_unix_ms must be within years 1970 through 9999".into(),
            ));
        }
        Ok(())
    }
}

/// Why Schedule cannot currently establish its next due-work horizon.
/// These are availability categories, never a reminder's identity or text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleHorizonUnknown {
    Clock,
    Unread,
    RetainedStore,
    Unavailable,
}

/// The clocks needed to age a next-work horizon conservatively. `Wall` is
/// required when ANY nonpending active deadline depends on wall time, even
/// when an elapsed timer supplies the minimum duration at this sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "basis", rename_all = "snake_case")]
pub enum ScheduleHorizonClock {
    Elapsed,
    /// Trusted Unix milliseconds from the same source sample as `sampled_boot_ms`.
    Wall {
        sampled_unix_ms: i64,
    },
}

impl<'de> Deserialize<'de> for ScheduleHorizonClock {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "basis", rename_all = "snake_case")]
        enum Wire {
            Elapsed,
            Wall { sampled_unix_ms: i64 },
        }
        Ok(match string_tagged(deserializer, "basis")? {
            Wire::Elapsed => Self::Elapsed,
            Wire::Wall { sampled_unix_ms } => Self::Wall { sampled_unix_ms },
        })
    }
}

/// Schedule's next due-work horizon at the exact source `ClockSample.boot_ms`
/// (Linux BOOTTIME), distinct from the later MONOTONIC publication stamp. A
/// receiver must validate the host boot and age `sampled_boot_ms` independently;
/// a fresh publication does not make an old sample fresh. This is metadata,
/// never permission to speak or run a check.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ScheduleHorizon {
    Empty {
        sampled_boot_ms: u64,
    },
    Next {
        sampled_boot_ms: u64,
        in_ms: u64,
        clock: ScheduleHorizonClock,
    },
    Unknown {
        reason: ScheduleHorizonUnknown,
    },
}

impl<'de> Deserialize<'de> for ScheduleHorizon {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "state", rename_all = "snake_case")]
        enum Wire {
            Empty {
                sampled_boot_ms: u64,
            },
            Next {
                sampled_boot_ms: u64,
                in_ms: u64,
                clock: ScheduleHorizonClock,
            },
            Unknown {
                reason: ScheduleHorizonUnknown,
            },
        }
        Ok(match string_tagged(deserializer, "state")? {
            Wire::Empty { sampled_boot_ms } => Self::Empty { sampled_boot_ms },
            Wire::Next {
                sampled_boot_ms,
                in_ms,
                clock,
            } => Self::Next {
                sampled_boot_ms,
                in_ms,
                clock,
            },
            Wire::Unknown { reason } => Self::Unknown { reason },
        })
    }
}

// Serde's internally tagged enum reader also accepts numeric variant indexes.
// These new wire tags are strings only. Intercept the tag without rebuilding
// the object, preserving the normal reader's duplicate and additive fields.
fn string_tagged<'de, D, T>(deserializer: D, tag: &'static str) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Tagged<T> {
        tag: &'static str,
        marker: std::marker::PhantomData<T>,
    }
    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Tagged<T> {
        type Value = T;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("an object with a string discriminator")
        }

        fn visit_map<A: serde::de::MapAccess<'de>>(self, map: A) -> Result<T, A::Error> {
            T::deserialize(serde::de::value::MapAccessDeserializer::new(StringTag {
                map,
                tag: self.tag,
                next_is_tag: false,
            }))
        }
    }
    deserializer.deserialize_map(Tagged {
        tag,
        marker: std::marker::PhantomData,
    })
}

struct StringTag<A> {
    map: A,
    tag: &'static str,
    next_is_tag: bool,
}

impl<'de, A: serde::de::MapAccess<'de>> serde::de::MapAccess<'de> for StringTag<A> {
    type Error = A::Error;

    fn next_key_seed<K: serde::de::DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Self::Error> {
        let Some(key) = self.map.next_key::<String>()? else {
            return Ok(None);
        };
        self.next_is_tag = key == self.tag;
        seed.deserialize(serde::de::value::StringDeserializer::new(key))
            .map(Some)
    }

    fn next_value_seed<V: serde::de::DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, Self::Error> {
        if std::mem::take(&mut self.next_is_tag) {
            let tag = self.map.next_value::<String>()?;
            seed.deserialize(serde::de::value::StringDeserializer::new(tag))
        } else {
            self.map.next_value_seed(seed)
        }
    }

    fn size_hint(&self) -> Option<usize> {
        self.map.size_hint()
    }
}

impl ScheduleHorizon {
    /// A remaining duration for unchanged sampled deadlines, not admission
    /// permission. This stateless helper cannot remember intermediate clock
    /// corrections or durable pending transitions. Discard the old snapshot
    /// after an observed correction or owner change and require fresh state;
    /// freshness alone does not exclude an intervening state change.
    /// The caller still validates the parent message, current host boot,
    /// source/publisher identity and ordering, readiness and sample freshness.
    /// A fresh publication cannot make the source sample fresh.
    ///
    /// Elapsed horizons need only the same-boot elapsed clock. Wall horizons
    /// require a currently trusted Unix sample within years 1970 through 9999
    /// and age by the larger of boot time and positive wall-clock advancement.
    /// A negative wall delta is zero, so wall time cannot extend the duration
    /// beyond the elapsed-only bound. `Empty` and `Unknown` have no remaining
    /// duration and return `None`.
    pub fn remaining_ms(
        self,
        current_boot_ms: u64,
        current_trusted_unix_ms: Option<i64>,
    ) -> Option<u64> {
        let Self::Next {
            sampled_boot_ms,
            in_ms,
            clock,
        } = self
        else {
            return None;
        };
        if in_ms > i64::MAX as u64 {
            return None;
        }
        let boot_age = current_boot_ms.checked_sub(sampled_boot_ms)?;
        let age = match clock {
            ScheduleHorizonClock::Elapsed => boot_age,
            ScheduleHorizonClock::Wall { sampled_unix_ms } => {
                let current_unix_ms = current_trusted_unix_ms?;
                if !(0..=SCHEDULE_MAX_UNIX_MS).contains(&sampled_unix_ms)
                    || !(0..=SCHEDULE_MAX_UNIX_MS).contains(&current_unix_ms)
                {
                    return None;
                }
                let wall_age =
                    u64::try_from(current_unix_ms.checked_sub(sampled_unix_ms)?.max(0)).ok()?;
                boot_age.max(wall_age)
            }
        };
        Some(in_ms.saturating_sub(age))
    }
}

/// The oldest actionable notice from the Schedule owner's persistent queue.
/// Consumers validate freshness, host boot, publisher identity and ordering,
/// then retrieve this exact store/id/delivery from the private socket before
/// considering speech. Only confirmed completion may be acknowledged.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleDue {
    pub stamp: ClockStamp,
    /// Persistent store identity: exactly 32 lowercase hexadecimal characters.
    pub store_id: String,
    pub revision: u64,
    /// False in shadow mode: retained metadata cannot authorize an action.
    pub enabled: bool,
    /// Whether the Schedule owner can currently serve due work, not whether
    /// Voice can speak or retain a person's input.
    pub ready: bool,
    pub pending: u32,
    pub waiting_for_clock: u32,
    /// Present exactly when enabled, ready and pending is positive. Disabled
    /// or unavailable owners may still report counts but offer no notice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oldest: Option<ScheduleNotice>,
    /// Unsupported when absent, never an empty schedule. A known horizon
    /// requires an enabled, ready owner with no clock-blocked schedules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub horizon: Option<ScheduleHorizon>,
}

impl ScheduleDue {
    pub const TYPE: &str = "lamp_msgs/ScheduleDue";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "schedule/due";

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Duration::from_secs(1))
            .lasting(Duration::from_secs(3))
    }

    /// Structural validation only. The receiver must independently validate
    /// current clock/boot, message age and monotonic publisher ordering.
    pub fn check(&self) -> Result<(), Invalid> {
        self.stamp.check()?;
        if self.store_id.len() != 32
            || !self
                .store_id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(Invalid(
                "schedule store_id must be 32 lowercase hexadecimal characters".into(),
            ));
        }
        if self.pending > SCHEDULE_MAX_ITEMS
            || self.waiting_for_clock > SCHEDULE_MAX_ITEMS
            || self.pending + self.waiting_for_clock > SCHEDULE_MAX_ITEMS
        {
            return Err(Invalid(
                "schedule pending and waiting_for_clock must total at most 128".into(),
            ));
        }
        if self.oldest.is_some() != (self.pending > 0 && self.enabled && self.ready) {
            return Err(Invalid(
                "schedule oldest requires enabled, ready and positive pending, and is required then"
                    .into(),
            ));
        }
        if let Some(notice) = &self.oldest {
            notice.check()?;
        }
        if let Some(horizon) = self.horizon {
            if !matches!(horizon, ScheduleHorizon::Unknown { .. })
                && (!self.enabled || !self.ready || self.waiting_for_clock != 0)
            {
                return Err(Invalid(
                    "known schedule horizon requires enabled, ready and no waiting_for_clock"
                        .into(),
                ));
            }
            match horizon {
                ScheduleHorizon::Empty { .. } if self.pending != 0 => {
                    return Err(Invalid(
                        "empty schedule horizon requires no pending work".into(),
                    ));
                }
                ScheduleHorizon::Next { in_ms, .. }
                    if in_ms > i64::MAX as u64 || (self.pending > 0 && in_ms != 0) =>
                {
                    return Err(Invalid(
                        "next schedule horizon requires bounded in_ms and zero for pending work"
                            .into(),
                    ));
                }
                ScheduleHorizon::Next {
                    clock: ScheduleHorizonClock::Wall { sampled_unix_ms },
                    ..
                } if !(0..=SCHEDULE_MAX_UNIX_MS).contains(&sampled_unix_ms) => {
                    return Err(Invalid(
                        "schedule horizon sampled_unix_ms must be within years 1970 through 9999"
                            .into(),
                    ));
                }
                _ => {}
            }
        }
        Ok(())
    }
}

/// Optional Schedule topics, separate from the original seventeen topics.
pub fn schedule_topics() -> Vec<(&'static str, Declaration)> {
    vec![(ScheduleDue::TOPIC, ScheduleDue::declaration())]
}
