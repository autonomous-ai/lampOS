//! Storage-only readers for harmless additive metadata. The public types also
//! cross Voice IPC, where unknown fields must still be refused. Remote derives
//! construct those same types without relaxing their wire deserializers.
//! Unknown enum variants, missing required options and duplicate known fields
//! remain errors. State::check still validates every admission/proof binding.
use super::{
    AckAttempt, Correlation, DeliveryKey, DeliveryView, PreparedToken, QualifiedCompletion,
    QualifiedRetirement, Resolution, RetirementBasis,
};
use crate::protocol::Request;
use serde::{Deserialize, Deserializer};

#[derive(Deserialize)]
#[serde(remote = "DeliveryKey")]
struct Key {
    store_id: String,
    id: u64,
    delivery: u64,
}

#[derive(Deserialize)]
#[serde(remote = "Correlation")]
struct SavedCorrelation {
    consumer_session: String,
    sequence: u64,
}

#[derive(Deserialize)]
#[serde(remote = "PreparedToken")]
struct Prepared {
    #[serde(deserialize_with = "Key::deserialize")]
    key: DeliveryKey,
    attempt: u64,
    #[serde(deserialize_with = "SavedCorrelation::deserialize")]
    correlation: Correlation,
}

#[derive(Deserialize)]
#[serde(remote = "QualifiedCompletion")]
struct Completion {
    #[serde(deserialize_with = "SavedCorrelation::deserialize")]
    correlation: Correlation,
    owner_boot: [u8; 16],
    turn: u64,
    microphone_generation: u64,
    provider_session: u64,
    playback_sequence: u64,
    final_epoch: u64,
    final_frame: u64,
    retired_generations: u64,
}

#[derive(Deserialize)]
#[serde(remote = "RetirementBasis", tag = "kind", rename_all = "snake_case")]
enum Basis {
    NeverAdmitted {},
    OwnerFenced {
        owner_boot: [u8; 16],
        turn: u64,
        microphone_generation: u64,
    },
    PreviousProcessExited {},
}

#[derive(Deserialize)]
#[serde(remote = "QualifiedRetirement")]
pub(super) struct Retirement {
    #[serde(deserialize_with = "SavedCorrelation::deserialize")]
    correlation: Correlation,
    retired_by_session: String,
    #[serde(deserialize_with = "Basis::deserialize")]
    basis: RetirementBasis,
}

#[derive(Deserialize)]
#[serde(remote = "Resolution", tag = "kind", rename_all = "snake_case")]
enum SavedResolution {
    AckConfirmed { revision: u64 },
    Superseded { revision: u64, delivery: u64 },
    Gone { revision: u64 },
}

#[derive(Deserialize)]
#[serde(remote = "AckAttempt")]
struct Ack {
    #[serde(deserialize_with = "Key::deserialize")]
    key: DeliveryKey,
    attempt: u64,
    // An exact retransmission must never silently discard request semantics.
    // Additions to this strict request or its guard require a schema bump.
    request: Request,
}

#[derive(Deserialize)]
#[serde(remote = "DeliveryView")]
pub(super) struct View {
    #[serde(deserialize_with = "Prepared::deserialize")]
    prepared: PreparedToken,
    #[serde(deserialize_with = "completion")]
    completion: Option<QualifiedCompletion>,
    #[serde(deserialize_with = "ack")]
    ack: Option<AckAttempt>,
    #[serde(deserialize_with = "resolution")]
    resolution: Option<Resolution>,
}

fn completion<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<QualifiedCompletion>, D::Error> {
    #[derive(Deserialize)]
    struct Saved(#[serde(deserialize_with = "Completion::deserialize")] QualifiedCompletion);
    Ok(Option::<Saved>::deserialize(deserializer)?.map(|saved| saved.0))
}

fn ack<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<AckAttempt>, D::Error> {
    #[derive(Deserialize)]
    struct Saved(#[serde(deserialize_with = "Ack::deserialize")] AckAttempt);
    Ok(Option::<Saved>::deserialize(deserializer)?.map(|saved| saved.0))
}

fn resolution<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Resolution>, D::Error> {
    #[derive(Deserialize)]
    struct Saved(#[serde(deserialize_with = "SavedResolution::deserialize")] Resolution);
    Ok(Option::<Saved>::deserialize(deserializer)?.map(|saved| saved.0))
}
