//! How loud the audio sent to speech to text for one request was, for the
//! line's end in the journal (#8776): 16 kHz after the canceller, the same
//! samples the service gets, counted where each block goes out.

/// The samples in one of the coordinator's capture blocks, 20 ms at
/// 16 kHz: the block the faint floor judges, by its level
/// ([`crate::ring_face::level_of`]).
const BLOCK_SAMPLES: usize = 320;

/// The worker's flag for the faint floor `--faint-floor-db` sets (its
/// lowest), on the face's scale: what [`SentLevel`] counts the share over.
pub const FLAG: &str = "--sent-floor-level";

/// The floor this worker was given, if any ([`take`]).
static FLOOR: std::sync::OnceLock<u8> = std::sync::OnceLock::new();

/// The worker's arguments for the serve's faint floor; none without one.
pub fn arguments(floor: Option<u8>) -> Vec<String> {
    floor.map_or_else(Vec::new, |level| vec![FLAG.to_owned(), level.to_string()])
}

/// Take a leading [`FLAG`] and its level off a worker's flags; the level
/// is the worker's floor from then on.
pub fn take(flags: &[String]) -> &[String] {
    match flags {
        [flag, value, rest @ ..] if flag == FLAG => {
            if let Ok(level) = value.parse::<u8>() {
                let _ = FLOOR.set(level);
            }
            rest
        }
        _ => flags,
    }
}

/// The floor [`take`] took, if any.
pub fn floor() -> Option<u8> {
    FLOOR.get().copied()
}

/// The level of every sample sent on one request's stream, kept as it goes
/// out: the loudest, the sum of squares, the clipped ones and those in
/// blocks over the floor. O(1) a sample; nothing of the sound is kept but
/// the block being judged.
#[derive(Debug, Default)]
pub struct SentLevel {
    floor: Option<u8>,
    samples: u64,
    squares: u64,
    peak: u32,
    clipped: u64,
    /// Samples in whole blocks at or over the floor.
    over: u64,
    /// The block being filled, judged when whole (or at the end).
    block: Vec<i16>,
}

impl SentLevel {
    pub fn new(floor: Option<u8>) -> Self {
        Self {
            floor,
            block: Vec::with_capacity(BLOCK_SAMPLES),
            ..Self::default()
        }
    }

    /// Little-endian 16-bit samples sent.
    pub fn sent(&mut self, bytes: &[u8]) {
        for pair in bytes.chunks_exact(2) {
            let sample = i16::from_le_bytes([pair[0], pair[1]]);
            let size = u32::from(sample.unsigned_abs());
            self.samples += 1;
            self.squares += u64::from(size * size);
            self.peak = self.peak.max(size);
            if size >= i16::MAX as u32 {
                self.clipped += 1;
            }
            if let Some(floor) = self.floor {
                self.block.push(sample);
                if self.block.len() == BLOCK_SAMPLES {
                    if crate::ring_face::level_of(&self.block) >= floor {
                        self.over += BLOCK_SAMPLES as u64;
                    }
                    self.block.clear();
                }
            }
        }
    }

    /// The loudest sample, in dBFS; -120 for none.
    pub fn peak_dbfs(&self) -> f64 {
        dbfs(f64::from(self.peak))
    }

    /// The rms of all the samples sent, in dBFS; -120 for none.
    pub fn rms_dbfs(&self) -> f64 {
        if self.samples == 0 {
            return -120.0;
        }
        dbfs((self.squares as f64 / self.samples as f64).sqrt())
    }

    /// Samples at 32767, -32767 or -32768.
    pub fn clipped(&self) -> u64 {
        self.clipped
    }

    /// The share of the samples sent that were in blocks at or over the
    /// floor, in whole percent, the block being filled judged as it is;
    /// none without a floor or a sample.
    pub fn over_floor_percent(&self) -> Option<u64> {
        let floor = self.floor?;
        if self.samples == 0 {
            return None;
        }
        let part = if !self.block.is_empty() && crate::ring_face::level_of(&self.block) >= floor {
            self.block.len() as u64
        } else {
            0
        };
        Some(((self.over + part) as f64 * 100.0 / self.samples as f64).round() as u64)
    }

    /// For the line's end: `, peak X dBFS, rms Y dBFS, clipped K, over the
    /// floor P%` (`none` without a floor).
    pub fn told(&self) -> String {
        format!(
            ", peak {:.1} dBFS, rms {:.1} dBFS, clipped {}, over the floor {}",
            self.peak_dbfs(),
            self.rms_dbfs(),
            self.clipped(),
            self.over_floor_percent()
                .map_or("none".to_owned(), |share| format!("{share}%"))
        )
    }
}

/// A level against a full-scale 16-bit sample, in dBFS, no lower than -120.
fn dbfs(level: f64) -> f64 {
    if level <= 0.0 {
        return -120.0;
    }
    (20.0 * (level / 32_768.0).log10()).max(-120.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(samples: &[i16]) -> Vec<u8> {
        samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect()
    }

    fn db(value: f64) -> f64 {
        20.0 * (value / 32_768.0).log10()
    }

    /// A near full-scale sine with three samples on the rails, a voice over
    /// the floor, then silence: peak, rms, clipped and the share over the
    /// floor are those of the samples, sent in blocks that cross the 20 ms
    /// blocks the floor is judged on.
    #[test]
    fn the_level_sent_is_its_peak_rms_clipped_samples_and_share_over_the_floor() {
        // 1 kHz at 16 kHz: 10 of the floor's blocks each.
        let sine = |amplitude: f64, count: usize| -> Vec<i16> {
            (0..count)
                .map(|n| {
                    (amplitude * (2.0 * std::f64::consts::PI * n as f64 / 16.0).sin()).round()
                        as i16
                })
                .collect()
        };
        let mut loud = sine(29_000.0, 3_200);
        loud[100] = i16::MAX;
        loud[200] = i16::MIN;
        loud[300] = -i16::MAX;
        // About -21 dBFS, over a -28 dBFS floor.
        let voice = sine(4_200.0, 3_200);
        let silence = vec![0i16; 6_400];
        let all: Vec<i16> = [loud.clone(), voice, silence].concat();
        // -28 dBFS on the face's scale, as `--faint-floor-db -28` gives it.
        let mut level = SentLevel::new(Some(150));
        for chunk in all.chunks(300) {
            level.sent(&bytes(chunk));
        }
        assert_eq!(level.peak_dbfs(), 0.0);
        let rms =
            (all.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / all.len() as f64).sqrt();
        assert!(
            (level.rms_dbfs() - db(rms)).abs() < 0.1,
            "{} vs {}",
            level.rms_dbfs(),
            db(rms)
        );
        assert_eq!(level.clipped(), 3);
        assert_eq!(level.over_floor_percent(), Some(50));
        assert_eq!(
            level.told(),
            format!(
                ", peak 0.0 dBFS, rms {:.1} dBFS, clipped 3, over the floor 50%",
                db(rms)
            )
        );
        // A line with nothing sent says so, and no floor says none.
        let none = SentLevel::new(None);
        assert_eq!(
            none.told(),
            ", peak -120.0 dBFS, rms -120.0 dBFS, clipped 0, over the floor none"
        );
    }

    /// The serve's floor goes to the worker as its flag, and none without.
    #[test]
    fn the_floor_goes_to_the_worker_only_when_set() {
        assert!(arguments(None).is_empty());
        assert_eq!(arguments(Some(150)), [FLAG, "150"]);
        let others = ["--other".to_owned()];
        assert_eq!(take(&others), others);
    }

    /// Half a block left at the end is judged as the floor judges a block,
    /// and the next samples sent make it whole.
    #[test]
    fn a_part_block_at_the_end_is_judged_too() {
        let mut level = SentLevel::new(Some(150));
        level.sent(&bytes(&[8_000; 160]));
        assert_eq!(level.over_floor_percent(), Some(100));
        level.sent(&bytes(&[0; 160]));
        assert_eq!(level.over_floor_percent(), Some(100));
        level.sent(&bytes(&[0; 320]));
        assert_eq!(level.over_floor_percent(), Some(50));
    }
}
