//! Worker-owned state mirrored over the existing ordered, credited output lane.
pub mod conversation;
pub mod names;

use crate::{provider_flow::OutputSender, provider_worker::ProviderOutput, transport::Channel};
use std::{io, time::Instant};

/// One publisher per worker boot. No request owns these snapshots: cancelling a
/// request does not discard them. Only a successfully sent Ended stops them.
#[derive(Default)]
pub(crate) struct Source {
    conversation: conversation::Source,
    names: names::NamesPublisher,
    ended: bool,
}

impl Source {
    /// At most one state packet per existing worker tick, using the same credit
    /// counter as audio and lifecycle output. Contended state locks never wait;
    /// retry on the next tick. No state lock is held across transport I/O.
    pub(crate) fn publish(
        &mut self,
        flow: &mut OutputSender,
        channel: &mut Channel,
        now: Instant,
        now_us: u64,
    ) -> io::Result<()> {
        if self.ended || !flow.can_send() {
            return Ok(());
        }
        let deadline = crate::thread::THREAD
            .try_lock()
            .ok()
            .map(|thread| thread.conversation_deadline());
        if let Some(deadline) = deadline
            && let Some(pending) = self.conversation.peek(deadline, now, now_us)
        {
            let event = ProviderOutput::ConversationState {
                snapshot: pending.snapshot,
            };
            if flow.try_send(|| channel.send(&event))? {
                self.conversation.sent(pending);
            }
            return Ok(());
        }
        if let Some(chunk) = self.names.next()?.cloned() {
            let event = ProviderOutput::MachineNames { chunk };
            if flow.try_send(|| channel.send(&event))? {
                self.names.sent();
            }
        }
        Ok(())
    }

    /// Call after the actual send succeeds, not merely when an output is taken
    /// from Engine or discarded by its request fence.
    pub(crate) fn output_sent(&mut self, event: &ProviderOutput) {
        if matches!(event, ProviderOutput::Ended { .. }) {
            self.ended = true;
        }
    }
}
