//! Fixed-size ring buffer for floating point values.
//!
//! Ported from `modules/audio_processing/echo_detector/circular_buffer.h/cc`.

/// Ring buffer containing floating point values.
#[derive(Debug)]
pub(crate) struct CircularBuffer {
    buffer: Vec<f32>,
    next_insertion_index: usize,
    nr_elements_in_buffer: usize,
}

impl CircularBuffer {
    pub(crate) fn new(size: usize) -> Self {
        Self {
            buffer: vec![0.0; size],
            next_insertion_index: 0,
            nr_elements_in_buffer: 0,
        }
    }

    pub(crate) fn push(&mut self, value: f32) {
        self.buffer[self.next_insertion_index] = value;
        self.next_insertion_index += 1;
        self.next_insertion_index %= self.buffer.len();
        self.nr_elements_in_buffer = (self.nr_elements_in_buffer + 1).min(self.buffer.len());
    }

    pub(crate) fn pop(&mut self) -> Option<f32> {
        if self.nr_elements_in_buffer == 0 {
            return None;
        }
        let index = (self.buffer.len() + self.next_insertion_index - self.nr_elements_in_buffer)
            % self.buffer.len();
        self.nr_elements_in_buffer -= 1;
        Some(self.buffer[index])
    }

    pub(crate) fn size(&self) -> usize {
        self.nr_elements_in_buffer
    }

    pub(crate) fn clear(&mut self) {
        self.buffer.fill(0.0);
        self.next_insertion_index = 0;
        self.nr_elements_in_buffer = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn less_than_max() {
        let mut buf = CircularBuffer::new(3);
        buf.push(1.0);
        buf.push(2.0);
        assert_eq!(Some(1.0), buf.pop());
        assert_eq!(Some(2.0), buf.pop());
    }

    #[test]
    fn fill() {
        let mut buf = CircularBuffer::new(3);
        buf.push(1.0);
        buf.push(2.0);
        buf.push(3.0);
        assert_eq!(Some(1.0), buf.pop());
        assert_eq!(Some(2.0), buf.pop());
        assert_eq!(Some(3.0), buf.pop());
    }

    #[test]
    fn overflow() {
        let mut buf = CircularBuffer::new(3);
        buf.push(1.0);
        buf.push(2.0);
        buf.push(3.0);
        buf.push(4.0);
        // The first insert should have been forgotten.
        assert_eq!(Some(2.0), buf.pop());
        assert_eq!(Some(3.0), buf.pop());
        assert_eq!(Some(4.0), buf.pop());
    }

    #[test]
    fn read_from_empty() {
        let mut buf = CircularBuffer::new(3);
        assert_eq!(None, buf.pop());
    }
}
