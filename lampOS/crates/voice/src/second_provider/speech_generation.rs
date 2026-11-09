//! Provider-local speech cancellation. The wire protocol stays request-based;
//! events already returned to the worker are outside this boundary.
use super::ProviderOutput;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Debug, Default)]
pub(super) struct Generation(Arc<AtomicBool>);
impl Generation {
    pub(super) fn current(&self) -> bool {
        !self.0.load(Ordering::Acquire)
    }
    pub(super) fn invalidate(&self) {
        self.0.store(true, Ordering::Release);
    }
}
#[derive(Debug)]
pub(super) struct Output {
    event: ProviderOutput,
    generation: Option<Generation>,
}
impl From<ProviderOutput> for Output {
    fn from(event: ProviderOutput) -> Self {
        Self {
            event,
            generation: None,
        }
    }
}
impl Output {
    /// Raw fixture inspection must not hide output behind the cancellation filter.
    #[cfg(test)]
    pub(super) fn unfiltered(self) -> ProviderOutput {
        self.event
    }

    pub(super) fn speech(event: ProviderOutput, generation: Generation) -> Self {
        Self {
            event,
            generation: Some(generation),
        }
    }
    pub(super) fn current(self) -> Option<ProviderOutput> {
        self.generation
            .as_ref()
            .is_none_or(Generation::current)
            .then_some(self.event)
    }
}
/// Closing the writer does not cancel its last generation: its speech drains.
/// A reset followed immediately by close still observes the unseen reset first.
pub(super) async fn changed(receiver: &mut tokio::sync::watch::Receiver<Generation>) {
    if receiver.changed().await.is_err() {
        std::future::pending::<()>().await;
    }
}
