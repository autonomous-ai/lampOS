//! How loud the audio sent to speech to text for one request was, for the
//! line's end in the journal (#8776): 16 kHz after the canceller, the same
//! samples the service gets, counted where each block goes out.

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(samples: &[i16]) -> Vec<u8> {
        samples.iter().flat_map(|sample| sample.to_le_bytes()).collect()
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
        let rms = (all.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / all.len() as f64).sqrt();
        assert!((level.rms_dbfs() - db(rms)).abs() < 0.1, "{} vs {}", level.rms_dbfs(), db(rms));
        assert_eq!(level.clipped(), 3);
        assert_eq!(level.over_floor_percent(), Some(50));
        assert_eq!(
            level.told(),
            format!(", peak 0.0 dBFS, rms {:.1} dBFS, clipped 3, over the floor 50%", db(rms))
        );
        // A line with nothing sent says so, and no floor says none.
        let none = SentLevel::new(None);
        assert_eq!(
            none.told(),
            ", peak -120.0 dBFS, rms -120.0 dBFS, clipped 0, over the floor none"
        );
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
