//! A worker thread fed through one slot that holds only the newest item:
//! the look offers and never waits, an item the worker has not taken yet is
//! replaced by a newer one (dropped, never queued), and the worker's results
//! come back on a short channel the look drains. The seems read (Face Mesh
//! V2 then Blendshape V2, 85 ms on a big core and 290 ms on a small one under
//! tract) runs here, off the look (the reviewer-lead's P2 on #2872).
use std::{
    io,
    sync::{
        Arc, Condvar, Mutex,
        mpsc::{self, Receiver},
    },
    thread,
};

struct Slot<T> {
    item: Option<T>,
    closed: bool,
}

type Shared<T> = Arc<(Mutex<Slot<T>>, Condvar)>;

/// The look's end of the slot. Dropping it stops the worker once its
/// current item is done.
pub struct Latest<T> {
    shared: Shared<T>,
}

impl<T> Latest<T> {
    /// Put `item` in the slot. True if an item the worker had not taken yet
    /// was dropped for it.
    pub fn offer(&self, item: T) -> bool {
        let (slot, ready) = &*self.shared;
        let dropped = slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .item
            .replace(item)
            .is_some();
        ready.notify_one();
        dropped
    }
}

impl<T> Drop for Latest<T> {
    fn drop(&mut self) {
        let (slot, ready) = &*self.shared;
        slot.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .closed = true;
        ready.notify_one();
    }
}

/// How many results may wait for the look; more are dropped, as a stalled
/// look would find them stale.
const RESULTS: usize = 4;

/// A thread named `name` that runs `work` on the newest item offered, one
/// at a time, and sends each result back.
pub fn worker<T, R>(
    name: &str,
    mut work: impl FnMut(T) -> R + Send + 'static,
) -> io::Result<(Latest<T>, Receiver<R>)>
where
    T: Send + 'static,
    R: Send + 'static,
{
    let shared: Shared<T> = Arc::new((
        Mutex::new(Slot {
            item: None,
            closed: false,
        }),
        Condvar::new(),
    ));
    let (send, results) = mpsc::sync_channel(RESULTS);
    let theirs = shared.clone();
    thread::Builder::new().name(name.into()).spawn(move || {
        let (slot, ready) = &*theirs;
        loop {
            let item = {
                let mut slot = slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                loop {
                    if let Some(item) = slot.item.take() {
                        break item;
                    }
                    if slot.closed {
                        return;
                    }
                    slot = ready
                        .wait(slot)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                }
            };
            let _ = send.try_send(work(item));
        }
    })?;
    Ok((Latest { shared }, results))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// A slow reader never slows the one offering, and only the newest item
    /// offered while it is busy is read: the ones between are dropped.
    #[test]
    fn a_busy_worker_never_holds_up_the_offer_and_reads_only_the_newest() {
        let (began, started) = mpsc::channel();
        let (latest, results) = worker("test-latest", move |item: u32| {
            let _ = began.send(item);
            thread::sleep(Duration::from_millis(300));
            item
        })
        .unwrap();
        assert!(!latest.offer(1));
        assert_eq!(started.recv_timeout(Duration::from_secs(5)), Ok(1));
        // While it reads 1: looks keep their pace, each offer at once.
        let mut slowest = Duration::ZERO;
        for item in 2..=20 {
            let at = Instant::now();
            let dropped = latest.offer(item);
            slowest = slowest.max(at.elapsed());
            assert_eq!(dropped, item > 2, "item {item}");
            thread::sleep(Duration::from_millis(5));
        }
        assert!(slowest < Duration::from_millis(50), "{slowest:?}");
        let read: Vec<u32> = (0..2)
            .map(|_| results.recv_timeout(Duration::from_secs(5)).unwrap())
            .collect();
        assert_eq!(read, [1, 20]);
        assert!(results.recv_timeout(Duration::from_millis(400)).is_err());
        drop(latest);
        assert!(
            started
                .recv_timeout(Duration::from_secs(5))
                .is_ok_and(|item| item == 20)
        );
    }
}
