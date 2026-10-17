//! Sliding window maximum with exponential decay.
//!
//! Ported from `modules/audio_processing/echo_detector/moving_max.h/cc`.

/// Decay factor: max decays to 1% after 460 updates past window end.
const DECAY_FACTOR: f32 = 0.99;

/// Tracks the maximum value within a sliding window.
#[derive(Debug)]
pub(crate) struct MovingMax {
    max_value: f32,
    counter: usize,
    window_size: usize,
}

impl MovingMax {
    pub(crate) fn new(window_size: usize) -> Self {
        debug_assert!(window_size > 0);
        Self {
            max_value: 0.0,
            counter: 0,
            window_size,
        }
    }

    pub(crate) fn update(&mut self, value: f32) {
        if self.counter >= self.window_size - 1 {
            self.max_value *= DECAY_FACTOR;
        } else {
            self.counter += 1;
        }
        if value > self.max_value {
            self.max_value = value;
            self.counter = 0;
        }
    }

    pub(crate) fn max(&self) -> f32 {
        self.max_value
    }

    pub(crate) fn clear(&mut self) {
        self.max_value = 0.0;
        self.counter = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple() {
        let mut mm = MovingMax::new(5);
        mm.update(1.0);
        mm.update(1.1);
        mm.update(1.9);
        mm.update(1.87);
        mm.update(1.89);
        assert_eq!(mm.max(), 1.9);
    }

    #[test]
    fn sliding_window() {
        let mut mm = MovingMax::new(5);
        mm.update(1.0);
        mm.update(1.9);
        mm.update(1.7);
        mm.update(1.87);
        mm.update(1.89);
        mm.update(1.3);
        mm.update(1.2);
        assert!(mm.max() < 1.9);
    }

    #[test]
    fn clear_test() {
        let mut mm = MovingMax::new(5);
        mm.update(1.0);
        mm.update(1.1);
        mm.update(1.9);
        mm.update(1.87);
        mm.update(1.89);
        assert_eq!(mm.max(), 1.9);
        mm.clear();
        assert_eq!(mm.max(), 0.0);
    }

    #[test]
    fn decay() {
        let mut mm = MovingMax::new(1);
        mm.update(1.0);
        let mut previous_value = 1.0;
        for _ in 0..500 {
            mm.update(0.0);
            assert!(mm.max() < previous_value);
            assert!(mm.max() > 0.0);
            previous_value = mm.max();
        }
        assert!(mm.max() < 0.01);
    }
}
