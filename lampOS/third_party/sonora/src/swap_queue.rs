//! Single-threaded swap queue.
//!
//! A fixed-size queue where elements are moved via `std::mem::swap` rather
//! than copy, avoiding allocations in the hot path.
//!
//! Ported from `rtc_base/swap_queue.h`.

use std::mem;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A fixed-size, single-threaded swap queue.
///
/// Elements are moved via swap rather than copy. All methods take `&mut self`,
/// so concurrent access is prevented at compile time.
#[derive(Debug)]
pub(crate) struct SwapQueue<T> {
    queue: Vec<T>,
    next_write_index: usize,
    next_read_index: usize,
    num_elements: AtomicUsize,
}

impl<T: Default> SwapQueue<T> {
    /// Creates a queue of the given size filled with default-constructed Ts.
    #[cfg(test)]
    pub(crate) fn new(size: usize) -> Self {
        let mut queue = Vec::with_capacity(size);
        for _ in 0..size {
            queue.push(T::default());
        }
        Self {
            queue,
            next_write_index: 0,
            next_read_index: 0,
            num_elements: AtomicUsize::new(0),
        }
    }
}

impl<T> SwapQueue<T> {
    /// Creates a queue of the given size filled with copies of `prototype`.
    pub(crate) fn with_prototype(size: usize, prototype: T) -> Self
    where
        T: Clone,
    {
        Self {
            queue: vec![prototype; size],
            next_write_index: 0,
            next_read_index: 0,
            num_elements: AtomicUsize::new(0),
        }
    }

    /// Resets the queue to have zero content while maintaining the queue size.
    /// Only safe to call from the consumer side.
    pub(crate) fn clear(&mut self) {
        let old = self.num_elements.swap(0, Ordering::Relaxed);
        self.next_read_index += old;
        if self.next_read_index >= self.queue.len() {
            self.next_read_index -= self.queue.len();
        }
        debug_assert!(self.next_read_index < self.queue.len());
    }

    /// Inserts a "full" T at the back of the queue by swapping `*input` with
    /// an "empty" T from the queue.
    ///
    /// Returns `true` if the item was inserted, `false` if the queue was full.
    pub(crate) fn insert(&mut self, input: &mut T) -> bool {
        if self.num_elements.load(Ordering::Acquire) == self.queue.len() {
            return false;
        }

        mem::swap(input, &mut self.queue[self.next_write_index]);

        self.num_elements.fetch_add(1, Ordering::Release);

        self.next_write_index += 1;
        if self.next_write_index == self.queue.len() {
            self.next_write_index = 0;
        }

        true
    }

    /// Removes the frontmost "full" T from the queue by swapping it with
    /// the "empty" T in `*output`.
    ///
    /// Returns `true` if an item was removed, `false` if the queue was empty.
    pub(crate) fn remove(&mut self, output: &mut T) -> bool {
        if self.num_elements.load(Ordering::Acquire) == 0 {
            return false;
        }

        mem::swap(output, &mut self.queue[self.next_read_index]);

        self.num_elements.fetch_sub(1, Ordering::Release);

        self.next_read_index += 1;
        if self.next_read_index == self.queue.len() {
            self.next_read_index = 0;
        }

        true
    }

    /// Returns the current number of elements in the queue.
    ///
    /// Since elements may be concurrently added, the caller must treat this
    /// as a lower bound, not an exact count. May only be called by the
    /// consumer.
    #[cfg(test)]
    pub(crate) fn size_at_least(&self) -> usize {
        self.num_elements.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_insert_remove() {
        let mut queue = SwapQueue::<i32>::new(3);
        let mut val = 42;
        assert!(queue.insert(&mut val));
        assert_eq!(val, 0); // swapped with default

        let mut out = 0;
        assert!(queue.remove(&mut out));
        assert_eq!(out, 42);
    }

    #[test]
    fn full_queue_rejects_insert() {
        let mut queue = SwapQueue::<i32>::new(2);
        let mut a = 1;
        let mut b = 2;
        let mut c = 3;
        assert!(queue.insert(&mut a));
        assert!(queue.insert(&mut b));
        assert!(!queue.insert(&mut c)); // full
        assert_eq!(c, 3); // unchanged
    }

    #[test]
    fn empty_queue_rejects_remove() {
        let mut queue = SwapQueue::<i32>::new(2);
        let mut out = 0;
        assert!(!queue.remove(&mut out));
        assert_eq!(out, 0); // unchanged
    }

    #[test]
    fn fifo_ordering() {
        let mut queue = SwapQueue::<i32>::new(4);
        let mut vals = [10, 20, 30];
        for v in &mut vals {
            queue.insert(v);
        }

        let mut out = 0;
        queue.remove(&mut out);
        assert_eq!(out, 10);
        queue.remove(&mut out);
        assert_eq!(out, 20);
        queue.remove(&mut out);
        assert_eq!(out, 30);
    }

    #[test]
    fn wraparound() {
        let mut queue = SwapQueue::<i32>::new(2);
        let mut val = 1;
        queue.insert(&mut val);
        val = 2;
        queue.insert(&mut val);

        let mut out = 0;
        queue.remove(&mut out);
        assert_eq!(out, 1);

        // Insert again — wraps around.
        val = 3;
        queue.insert(&mut val);

        queue.remove(&mut out);
        assert_eq!(out, 2);
        queue.remove(&mut out);
        assert_eq!(out, 3);
    }

    #[test]
    fn clear_empties_queue() {
        let mut queue = SwapQueue::<i32>::new(3);
        let mut val = 1;
        queue.insert(&mut val);
        val = 2;
        queue.insert(&mut val);

        assert_eq!(queue.size_at_least(), 2);
        queue.clear();
        assert_eq!(queue.size_at_least(), 0);

        let mut out = 0;
        assert!(!queue.remove(&mut out));
    }

    #[test]
    fn size_at_least() {
        let mut queue = SwapQueue::<i32>::new(3);
        assert_eq!(queue.size_at_least(), 0);

        let mut val = 1;
        queue.insert(&mut val);
        assert_eq!(queue.size_at_least(), 1);

        val = 2;
        queue.insert(&mut val);
        assert_eq!(queue.size_at_least(), 2);

        let mut out = 0;
        queue.remove(&mut out);
        assert_eq!(queue.size_at_least(), 1);
    }

    #[test]
    fn with_prototype() {
        let mut queue = SwapQueue::with_prototype(2, vec![0i32; 10]);
        let mut input = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        assert!(queue.insert(&mut input));
        // Swapped with prototype — should get a vec of length 10 with zeros.
        assert_eq!(input.len(), 10);
        assert!(input.iter().all(|&x| x == 0));

        let mut output = vec![0i32; 10];
        assert!(queue.remove(&mut output));
        assert_eq!(output, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    }

    #[test]
    fn runtime_setting_style_usage() {
        // Simulates how AudioProcessingImpl uses SwapQueue with RuntimeSettings.
        #[derive(Debug, Default, Clone)]
        struct Setting {
            kind: u8,
            value: f32,
        }

        let mut queue = SwapQueue::new(10);
        let mut s = Setting {
            kind: 1,
            value: 3.125,
        };
        assert!(queue.insert(&mut s));
        assert_eq!(s.kind, 0); // swapped with default

        let mut out = Setting::default();
        assert!(queue.remove(&mut out));
        assert_eq!(out.kind, 1);
        assert!((out.value - 3.125).abs() < 0.001);
    }
}
