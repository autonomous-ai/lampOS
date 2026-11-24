//! A bounded, non-replayed history lane ahead of normal provider input.
use super::*;
use crate::fixed_line_history::Receipt;

#[derive(Default)]
pub(super) struct Pending(VecDeque<Receipt>);
impl Pending {
    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn push(&mut self, receipt: Receipt) {
        if self.0.len() == 4 {
            eprintln!("lamp-voice: fixed-line history queue full");
        } else {
            self.0.push_back(receipt);
        }
    }
    pub fn clear(&mut self) {
        self.0.clear();
    }
    /// True means later ordinary input may proceed. No model turn is admitted.
    pub fn flush(
        &mut self,
        worker: &mut Worker,
        cloud: &CloudAvailability,
        snapshot: Snapshot,
    ) -> io::Result<bool> {
        while let Some(receipt) = self.0.front() {
            let origin = &receipt.line.origin;
            if !receipt.current(monotonic_us())
                || receipt.owner.boot() != snapshot.boot()
                || crate::process::boot_text(worker.boot) != origin.worker_boot
                || worker.history_epoch() != Some(origin.route_epoch)
                || cloud.ready_session() != Some(origin.provider_session)
                || snapshot.microphone_permission() != Permission::Allowed
                || snapshot.microphone_generation() != origin.microphone_generation
                || snapshot.camera_generation() != origin.camera_generation
            {
                self.0.pop_front();
                eprintln!("lamp-voice: fixed-line history binding expired");
                continue;
            }
            match worker.channels.data.send(ProviderInput::PlayedFixedLine {
                receipt: Box::new(receipt.clone()),
            }) {
                Ok(()) => {
                    self.0.pop_front();
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(false),
                Err(error) => return Err(error),
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
