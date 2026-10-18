//! Optional bounded acquisition recording, never on the pin-reader thread.
use crate::replay::{MAX_BYTES, MAX_DURATION_US, MAX_LINE_BYTES, MAX_LINES, Record};
use lamp_msgs::ControlFault;
use serde::Serialize;
use std::{
    fs::OpenOptions,
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread,
    time::Duration,
};

/// How long [`Recorder::finish`] waits for the writer to drain and sync.
const FINISH_WINDOW: Duration = Duration::from_millis(100);

const QUEUE_CAPACITY: usize = 32;
const TERMINAL_BYTES: u64 = 128;

#[derive(Default)]
struct WriteProgress {
    bytes: u64,
    first_us: Option<u64>,
    previous_us: Option<u64>,
    touch: bool,
}

#[derive(Default)]
struct Counters {
    written: AtomicU64,
    dropped: AtomicU64,
    failed: AtomicBool,
    tail_gap: AtomicBool,
    privacy: AtomicBool,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct RecordStats {
    pub written: u64,
    pub dropped: u64,
    pub failed: bool,
    pub finished: bool,
}

pub struct Recorder {
    sender: SyncSender<(u64, Vec<u8>)>,
    done: Receiver<()>,
    counters: Arc<Counters>,
    gap: bool,
    touch: bool,
}

/// Keep the writer independent of disk scheduling so queue-loss and replay
/// bounds can be verified with an injected writer. Production starts with
/// empty progress; boundary tests seed already-written byte/count accounting.
fn write_records(
    writer: &mut impl Write,
    records: impl IntoIterator<Item = (u64, Vec<u8>)>,
    counters: &Counters,
    progress: &mut WriteProgress,
) {
    for (now_us, line) in records {
        let first = progress.first_us.unwrap_or(now_us);
        let next_bytes = progress.bytes.saturating_add(line.len() as u64);
        if progress.previous_us.is_some_and(|last| now_us < last)
            || now_us < first
            || now_us - first > MAX_DURATION_US
            || counters.written.load(Ordering::Relaxed) >= (MAX_LINES - 1) as u64
            || line.len() > MAX_LINE_BYTES + 1
            || next_bytes > MAX_BYTES - TERMINAL_BYTES
            || writer.write_all(&line).is_err()
        {
            counters.failed.store(true, Ordering::Relaxed);
            break;
        }
        progress.bytes = next_bytes;
        progress.first_us = Some(first);
        progress.previous_us = Some(now_us);
        counters.written.fetch_add(1, Ordering::Relaxed);
    }
    // Reserve one line and 128 bytes for a conservative terminal fault. It
    // closes pending timers at the last recorded time, including when the
    // dropped sample was the final one in the clip. I/O failure may leave a
    // partial line; failed remains true even if the final write succeeds.
    if counters.tail_gap.load(Ordering::Acquire) || counters.failed.load(Ordering::Relaxed) {
        let result = overflow(
            progress.previous_us.unwrap_or(0),
            progress.touch,
            counters.privacy.load(Ordering::Relaxed),
        )
        .map_err(io::Error::from)
        .and_then(|mut line| {
            line.push(b'\n');
            writer.write_all(&line)?;
            progress.bytes += line.len() as u64;
            Ok(())
        });
        if result.is_err() {
            counters.failed.store(true, Ordering::Relaxed);
        } else {
            counters.written.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl Recorder {
    pub fn create(path: &Path) -> io::Result<Self> {
        Self::create_format(path, false)
    }

    pub fn create_touch(path: &Path) -> io::Result<Self> {
        Self::create_format(path, true)
    }

    fn create_format(path: &Path, touch: bool) -> io::Result<Self> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(path)?;
        let (sender, receiver) = mpsc::sync_channel::<(u64, Vec<u8>)>(QUEUE_CAPACITY);
        let (finish, done) = mpsc::sync_channel(1);
        let counters = Arc::new(Counters::default());
        let worker = counters.clone();
        thread::Builder::new()
            .name("controls-record".into())
            .spawn(move || {
                write_records(
                    &mut file,
                    receiver,
                    &worker,
                    &mut WriteProgress {
                        touch,
                        ..WriteProgress::default()
                    },
                );
                if file.sync_all().is_err() {
                    worker.failed.store(true, Ordering::Relaxed);
                }
                let _ = finish.send(());
            })?;
        Ok(Self {
            sender,
            done,
            counters,
            gap: false,
            touch,
        })
    }

    /// A lost recording sample is represented by an overflow before any later
    /// sample. The real engine is unaffected; replay must not span that gap.
    pub fn submit(&mut self, record: &Record) {
        if record.privacy.is_some() || record.privacy_fault.is_some() {
            self.counters.privacy.store(true, Ordering::Relaxed);
        }
        if self.touch {
            self.counters.failed.store(true, Ordering::Relaxed);
            return;
        }
        if self.gap {
            let gap = Record {
                now_us: record.now_us,
                observation: None,
                fault: Some(ControlFault::Overflow),
                privacy: None,
                privacy_fault: self
                    .counters
                    .privacy
                    .load(Ordering::Relaxed)
                    .then_some(ControlFault::Overflow),
            };
            if !self.send(&gap) {
                return;
            }
            self.gap = false;
            self.counters.tail_gap.store(false, Ordering::Release);
        }
        if !self.send(record) {
            self.gap = true;
            self.counters.tail_gap.store(true, Ordering::Release);
        }
    }

    /// The touch format carries independent GPIO and strip acquisitions. A
    /// recording drop invalidates both sources in replay and never pretends
    /// that the last known levels remained sampled through the gap.
    pub fn submit_touch(&mut self, record: &crate::touch_replay::Record) {
        if !self.touch {
            self.counters.failed.store(true, Ordering::Relaxed);
            return;
        }
        if self.gap {
            if !self.send_bytes(record.now_us, overflow(record.now_us, true, true)) {
                return;
            }
            self.gap = false;
            self.counters.tail_gap.store(false, Ordering::Release);
        }
        if !self.send_bytes(record.now_us, serde_json::to_vec(record)) {
            self.gap = true;
            self.counters.tail_gap.store(true, Ordering::Release);
        }
    }

    fn send(&self, record: &Record) -> bool {
        let refused;
        let record = if record.privacy.as_ref().is_some_and(|sample| {
            sample.check().is_err()
                || record.privacy_fault.is_some()
                || record.observation.is_some()
                || record.fault.is_some()
        }) {
            refused = Record {
                now_us: record.now_us,
                observation: None,
                fault: None,
                privacy: None,
                privacy_fault: Some(ControlFault::Invalid),
            };
            &refused
        } else if record.observation.is_some() && record.fault.is_some()
            || record
                .observation
                .as_ref()
                .is_some_and(|sample| sample.check().is_err())
        {
            refused = Record {
                now_us: record.now_us,
                observation: None,
                fault: Some(ControlFault::Invalid),

                privacy: None,
                privacy_fault: None,
            };
            &refused
        } else {
            record
        };
        self.send_bytes(record.now_us, serde_json::to_vec(record))
    }

    fn send_bytes(&self, now_us: u64, encoded: Result<Vec<u8>, serde_json::Error>) -> bool {
        let mut line = match encoded {
            Ok(line) => line,
            Err(_) => {
                self.counters.failed.store(true, Ordering::Relaxed);
                return false;
            }
        };
        if line.len() > MAX_LINE_BYTES {
            self.counters.failed.store(true, Ordering::Relaxed);
            return false;
        }
        line.push(b'\n');
        match self.sender.try_send((now_us, line)) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                self.counters.dropped.fetch_add(1, Ordering::Relaxed);
                false
            }
            Err(TrySendError::Disconnected(_)) => {
                self.counters.failed.store(true, Ordering::Relaxed);
                false
            }
        }
    }

    pub fn stats(&self) -> RecordStats {
        RecordStats {
            written: self.counters.written.load(Ordering::Relaxed),
            dropped: self.counters.dropped.load(Ordering::Relaxed),
            failed: self.counters.failed.load(Ordering::Relaxed),
            finished: false,
        }
    }

    /// Give the optional writer a bounded finish window, then leave any stalled
    /// disk work behind. A timeout is incomplete evidence, not a successful log.
    pub fn finish(self) -> RecordStats {
        self.finish_within(FINISH_WINDOW)
    }

    /// [`Self::finish`] with its window given: a test on a busy machine, whose
    /// writer's last `sync_all` may take longer than the lamp's 100 ms.
    pub fn finish_within(self, window: Duration) -> RecordStats {
        let Self {
            sender,
            done,
            counters,
            ..
        } = self;
        drop(sender);
        let finished = done.recv_timeout(window).is_ok();
        RecordStats {
            written: counters.written.load(Ordering::Relaxed),
            dropped: counters.dropped.load(Ordering::Relaxed),
            failed: counters.failed.load(Ordering::Relaxed),
            finished,
        }
    }
}

fn overflow(now_us: u64, touch: bool, privacy: bool) -> Result<Vec<u8>, serde_json::Error> {
    if touch {
        serde_json::to_vec(&crate::touch_replay::Record {
            now_us,
            input: crate::touch_replay::Input::AllFault {
                fault: ControlFault::Overflow,
            },
        })
    } else {
        serde_json::to_vec(&Record {
            now_us,
            observation: None,
            fault: Some(ControlFault::Overflow),
            privacy: None,
            privacy_fault: privacy.then_some(ControlFault::Overflow),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        engine::Config,
        input::{Level, Observation},
        replay,
    };
    use lamp_msgs::ClockStamp;

    #[test]
    fn touch_recording_queue_loss_marks_both_sources_instead_of_splicing_a_hold() {
        use crate::touch_replay::{self, Input};
        let (mut recorder, receiver) = recorder(2);
        recorder.touch = true;
        let counters = recorder.counters.clone();
        let row = |now_us| touch_replay::Record {
            now_us,
            input: Input::AllFault {
                fault: ControlFault::Missing,
            },
        };
        recorder.submit_touch(&row(100));
        recorder.submit_touch(&row(200));
        recorder.submit_touch(&row(300));
        assert_eq!(recorder.stats().dropped, 1);
        let prefix = [receiver.recv().unwrap(), receiver.recv().unwrap()];
        recorder.submit_touch(&row(400));
        assert!(!recorder.gap);
        drop(recorder);
        let mut bytes = Vec::new();
        write_records(
            &mut bytes,
            prefix.into_iter().chain(receiver),
            &counters,
            &mut WriteProgress {
                touch: true,
                ..WriteProgress::default()
            },
        );
        let saved = touch_replay::read(bytes.as_slice()).unwrap();
        assert_eq!(saved.len(), 4);
        assert_eq!(saved[2].now_us, 400);
        assert!(matches!(
            saved[2].input,
            Input::AllFault {
                fault: ControlFault::Overflow
            }
        ));
        assert!(matches!(
            saved[3].input,
            Input::AllFault {
                fault: ControlFault::Missing
            }
        ));
        assert!(!counters.failed.load(Ordering::Relaxed));
    }

    #[test]
    fn a_final_touch_recording_drop_is_an_overflow_at_the_last_saved_time() {
        use crate::touch_replay::{self, Input};
        let (mut recorder, receiver) = recorder(1);
        recorder.touch = true;
        let counters = recorder.counters.clone();
        for now_us in [100, 200] {
            recorder.submit_touch(&touch_replay::Record {
                now_us,
                input: Input::GpioFault {
                    fault: ControlFault::Missing,
                },
            });
        }
        assert_eq!(recorder.stats().dropped, 1);
        drop(recorder);
        let mut bytes = Vec::new();
        write_records(
            &mut bytes,
            receiver,
            &counters,
            &mut WriteProgress {
                touch: true,
                ..WriteProgress::default()
            },
        );
        let saved = touch_replay::read(bytes.as_slice()).unwrap();
        assert_eq!(saved.len(), 2);
        assert_eq!(saved[1].now_us, 100);
        assert!(matches!(
            saved[1].input,
            Input::AllFault {
                fault: ControlFault::Overflow
            }
        ));
        assert!((overflow(u64::MAX, true, true).unwrap().len() as u64) < TERMINAL_BYTES);
        assert!((overflow(u64::MAX, false, true).unwrap().len() as u64) < TERMINAL_BYTES);
    }

    #[test]
    fn recording_formats_cannot_be_mixed_silently() {
        let (mut legacy, _) = recorder(1);
        legacy.submit_touch(&crate::touch_replay::Record {
            now_us: 100,
            input: crate::touch_replay::Input::AllFault {
                fault: ControlFault::Missing,
            },
        });
        assert!(legacy.stats().failed);
        let (mut touch, _) = recorder(1);
        touch.touch = true;
        touch.submit(&tick(100));
        assert!(touch.stats().failed);
    }

    fn tick(now_us: u64) -> Record {
        Record {
            now_us,
            observation: None,
            fault: None,

            privacy: None,
            privacy_fault: None,
        }
    }

    fn encoded(record: &Record) -> (u64, Vec<u8>) {
        let mut line = serde_json::to_vec(record).unwrap();
        line.push(b'\n');
        (record.now_us, line)
    }

    // No worker consumes this queue until the test explicitly drains it. A
    // full queue and its final dropped item are therefore deterministic.
    fn recorder(capacity: usize) -> (Recorder, Receiver<(u64, Vec<u8>)>) {
        let (sender, receiver) = mpsc::sync_channel(capacity);
        let (_, done) = mpsc::sync_channel(1);
        (
            Recorder {
                sender,
                done,
                counters: Arc::new(Counters::default()),
                gap: false,
                touch: false,
            },
            receiver,
        )
    }

    #[test]
    fn final_dropped_privacy_acquisition_records_unknown_instead_of_retaining_high() {
        let (mut recorder, receiver) = recorder(1);
        let sample = |at, sequence, muted| Record {
            now_us: at,
            observation: None,
            fault: None,
            privacy_fault: None,
            privacy: Some(lamp_msgs::PrivacySwitch {
                stamp: lamp_msgs::ClockStamp {
                    clock_id: "boot".into(),
                    at_us: at,
                    sequence,
                },
                reader_session_id: "privacy-reader".into(),
                muted: Some(muted),
                fault: None,
            }),
        };
        recorder.submit(&sample(100, 1, false));
        recorder.submit(&sample(200, 2, true));
        assert_eq!(recorder.stats().dropped, 1);
        let lines: Vec<_> = receiver.try_iter().collect();
        let mut bytes = vec![];
        write_records(
            &mut bytes,
            lines,
            &recorder.counters,
            &mut WriteProgress::default(),
        );
        let records = crate::replay::read(bytes.as_slice()).unwrap();
        assert_eq!(
            records.last().unwrap().privacy_fault,
            Some(ControlFault::Overflow)
        );
        let mut replayed = vec![];
        crate::replay::run_with_privacy(
            crate::engine::Config::default(),
            "boot",
            &records,
            None,
            &mut replayed,
            true,
        )
        .unwrap();
        for row in std::str::from_utf8(&replayed).unwrap().lines() {
            let frame: crate::replay::Frame = serde_json::from_str(row).unwrap();
            let privacy = frame.output.state.privacy.unwrap();
            assert_eq!(privacy.muted, None);
            assert_eq!(privacy.fault, Some(ControlFault::Overflow));
        }
    }

    fn head_sample(now_us: u64, sequence: u64, active: bool) -> Record {
        Record {
            now_us,
            observation: Some(Observation {
                stamp: ClockStamp {
                    clock_id: "boot".into(),
                    at_us: now_us,
                    sequence,
                },
                session_id: "reader".into(),
                buttons: vec![],
                head_pads: vec![Level {
                    name: "head".into(),
                    active,
                }],
                base_mask: None,
            }),
            fault: None,

            privacy: None,
            privacy_fault: None,
        }
    }

    #[test]
    fn a_final_full_queue_drop_cancels_a_pet_pending_in_the_recorded_tail() {
        let config = Config {
            enabled: true,
            head_pads: vec!["head".into()],
            ..Config::default()
        };
        let times = (0..=600_000)
            .step_by(50_000)
            .chain(std::iter::once(610_000))
            .chain((660_000..=1_960_000).step_by(50_000))
            .chain(std::iter::once(2_000_000));
        let input: Vec<_> = times
            .enumerate()
            .map(|(index, at)| head_sample(at, index as u64 + 1, at == 600_000))
            .collect();
        // This complete, fresh prefix produces a pet at 2.010 s during the
        // ordinary 200 ms replay tail. The dropped live fault must cancel it.
        assert_eq!(
            replay::run(config.clone(), "boot", &input, None, io::sink())
                .unwrap()
                .gestures,
            1
        );

        let (mut recorder, receiver) = recorder(QUEUE_CAPACITY);
        let counters = recorder.counters.clone();
        let mut drained = Vec::new();
        let prelude = input.len() - QUEUE_CAPACITY;
        for (index, record) in input.iter().enumerate() {
            recorder.submit(record);
            if index < prelude {
                drained.push(receiver.recv().unwrap());
            }
        }
        recorder.submit(&Record {
            now_us: 2_005_000,
            observation: None,
            fault: Some(ControlFault::Missing),

            privacy: None,
            privacy_fault: None,
        });
        assert_eq!(recorder.stats().dropped, 1);
        assert!(recorder.gap);
        drop(recorder);
        let mut bytes = Vec::new();
        write_records(
            &mut bytes,
            drained.into_iter().chain(receiver),
            &counters,
            &mut WriteProgress::default(),
        );
        let saved = replay::read(bytes.as_slice()).unwrap();
        let last = saved.last().unwrap();
        assert_eq!(last.now_us, 2_000_000);
        assert_eq!(last.fault, Some(ControlFault::Overflow));
        assert_eq!(saved.len(), input.len() + 1);
        assert_eq!(
            replay::run(config, "boot", &saved, None, io::sink())
                .unwrap()
                .gestures,
            0
        );
        assert!(!counters.failed.load(Ordering::Relaxed));
    }

    #[test]
    fn a_recovered_queue_gap_precedes_the_next_sample_without_a_spurious_tail_fault() {
        let (mut recorder, receiver) = recorder(2);
        let counters = recorder.counters.clone();
        recorder.submit(&tick(0));
        recorder.submit(&tick(1));
        recorder.submit(&tick(2));
        assert_eq!(recorder.stats().dropped, 1);
        let drained = [receiver.recv().unwrap(), receiver.recv().unwrap()];
        recorder.submit(&tick(3));
        assert!(!recorder.gap);
        drop(recorder);
        let mut bytes = Vec::new();
        write_records(
            &mut bytes,
            drained.into_iter().chain(receiver),
            &counters,
            &mut WriteProgress::default(),
        );
        let saved = replay::read(bytes.as_slice()).unwrap();
        assert_eq!(saved.len(), 4);
        assert_eq!(saved[2].fault, Some(ControlFault::Overflow));
        assert_eq!(saved[2].now_us, 3);
        assert_eq!(saved[3].now_us, 3);
        assert_eq!(saved[3].fault, None);
    }

    #[test]
    fn the_hundred_thousandth_record_is_reserved_for_the_terminal_fault() {
        let counters = Counters::default();
        // Seed the already-written prefix at the actual production count,
        // without generating 99,998 redundant records or performing any I/O.
        counters
            .written
            .store((MAX_LINES - 2) as u64, Ordering::Relaxed);
        let mut bytes = Vec::new();
        write_records(
            &mut bytes,
            [encoded(&tick(100)), encoded(&tick(101))],
            &counters,
            &mut WriteProgress::default(),
        );
        let suffix = replay::read(bytes.as_slice()).unwrap();
        assert_eq!(counters.written.load(Ordering::Relaxed), 100_000);
        assert!(counters.failed.load(Ordering::Relaxed));
        assert_eq!(suffix.len(), 2);
        assert_eq!(suffix[0].now_us, 100);
        assert_eq!(suffix[0].fault, None);
        assert_eq!(suffix[1].now_us, 100);
        assert_eq!(suffix[1].fault, Some(ControlFault::Overflow));
    }

    #[test]
    fn the_thirty_two_mib_limit_keeps_a_hundred_twenty_eight_byte_fault_reserve() {
        let first = encoded(&tick(100));
        let counters = Counters::default();
        // As above, seed only the accounting for a previously written prefix;
        // the boundary is the real 32 MiB cap, not a smaller test-only limit.
        let mut progress = WriteProgress {
            bytes: MAX_BYTES - TERMINAL_BYTES - first.1.len() as u64,
            first_us: Some(99),
            previous_us: Some(99),
            touch: false,
        };
        let prefix_bytes = progress.bytes;
        let mut bytes = Vec::new();
        write_records(
            &mut bytes,
            [first, encoded(&tick(101))],
            &counters,
            &mut progress,
        );
        let suffix = replay::read(bytes.as_slice()).unwrap();
        assert_eq!(suffix.len(), 2);
        assert_eq!(suffix[1].fault, Some(ControlFault::Overflow));
        assert_eq!(suffix[1].now_us, 100);
        assert_eq!(progress.bytes, prefix_bytes + bytes.len() as u64);
        assert!(progress.bytes <= 32 * 1024 * 1024);
        assert!(counters.failed.load(Ordering::Relaxed));
        let longest_fault = encoded(&Record {
            now_us: u64::MAX,
            observation: None,
            fault: Some(ControlFault::Overflow),

            privacy: None,
            privacy_fault: None,
        });
        assert!(longest_fault.1.len() as u64 <= TERMINAL_BYTES);
    }

    #[test]
    fn sixteen_kib_json_lines_are_accepted_but_one_more_byte_ends_the_clip() {
        let padded = |at, length| {
            let mut line = serde_json::to_vec(&tick(at)).unwrap();
            line.resize(length, b' ');
            line.push(b'\n');
            (at, line)
        };
        let counters = Counters::default();
        let mut bytes = Vec::new();
        write_records(
            &mut bytes,
            [padded(100, MAX_LINE_BYTES), padded(101, MAX_LINE_BYTES + 1)],
            &counters,
            &mut WriteProgress::default(),
        );
        let saved = replay::read(bytes.as_slice()).unwrap();
        assert_eq!(saved.len(), 2);
        assert_eq!(saved[0].now_us, 100);
        assert_eq!(saved[1].fault, Some(ControlFault::Overflow));
        assert!(counters.failed.load(Ordering::Relaxed));
    }

    #[test]
    fn the_ten_minute_boundary_is_inclusive_and_overrun_does_not_extend_the_log() {
        for (delta, expected_fault) in [(MAX_DURATION_US, false), (MAX_DURATION_US + 1, true)] {
            let counters = Counters::default();
            let mut bytes = Vec::new();
            write_records(
                &mut bytes,
                [encoded(&tick(100)), encoded(&tick(100 + delta))],
                &counters,
                &mut WriteProgress::default(),
            );
            let saved = replay::read(bytes.as_slice()).unwrap();
            assert_eq!(saved.len(), 2);
            assert_eq!(counters.failed.load(Ordering::Relaxed), expected_fault);
            if expected_fault {
                assert_eq!(saved[1].now_us, 100);
                assert_eq!(saved[1].fault, Some(ControlFault::Overflow));
            } else {
                assert_eq!(saved[1].now_us, 600_000_100);
                assert_eq!(saved[1].fault, None);
            }
        }
    }

    #[test]
    fn backwards_receipt_time_ends_at_the_previous_time_with_a_fault() {
        let counters = Counters::default();
        let mut bytes = Vec::new();
        write_records(
            &mut bytes,
            [encoded(&tick(100)), encoded(&tick(99)), encoded(&tick(101))],
            &counters,
            &mut WriteProgress::default(),
        );
        let saved = replay::read(bytes.as_slice()).unwrap();
        assert_eq!(saved.len(), 2);
        assert_eq!(saved[1].now_us, 100);
        assert_eq!(saved[1].fault, Some(ControlFault::Overflow));
        assert!(counters.failed.load(Ordering::Relaxed));
    }

    #[test]
    fn structurally_invalid_samples_become_small_invalid_records_before_queueing() {
        let mut large_name = head_sample(7, 1, false);
        large_name.observation.as_mut().unwrap().session_id = "x".repeat(MAX_LINE_BYTES * 2);
        let mut too_many = head_sample(8, 2, false);
        too_many.observation.as_mut().unwrap().head_pads = vec![
            Level {
                name: "head".into(),
                active: false,
            };
            4096
        ];
        let mut duplicate = head_sample(9, 3, false);
        let sample = duplicate.observation.as_mut().unwrap();
        sample.head_pads.push(sample.head_pads[0].clone());
        let mut invalid_mask = head_sample(10, 4, false);
        invalid_mask.observation.as_mut().unwrap().base_mask = Some(0x1000);
        let mut contradictory = head_sample(11, 5, false);
        contradictory.fault = Some(ControlFault::Missing);
        let (mut recorder, receiver) = recorder(QUEUE_CAPACITY);
        for record in [large_name, too_many, duplicate, invalid_mask, contradictory] {
            recorder.submit(&record);
            let (at, line) = receiver.recv().unwrap();
            assert_eq!(at, record.now_us);
            assert!(line.len() <= TERMINAL_BYTES as usize);
            let saved = replay::read(line.as_slice()).unwrap();
            assert_eq!(saved.len(), 1);
            assert_eq!(saved[0].now_us, record.now_us);
            assert!(saved[0].observation.is_none());
            assert_eq!(saved[0].fault, Some(ControlFault::Invalid));
        }
        assert_eq!(recorder.stats().dropped, 0);
        assert!(!recorder.stats().failed);
    }

    #[test]
    fn a_partial_writer_failure_is_reported_even_if_no_terminal_fault_can_be_saved() {
        struct FailingWriter(Vec<u8>);
        impl Write for FailingWriter {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                let count = (8 - self.0.len()).min(bytes.len());
                if count == 0 {
                    return Err(io::Error::other("injected writer failure"));
                }
                self.0.extend_from_slice(&bytes[..count]);
                Ok(count)
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let counters = Counters::default();
        let mut writer = FailingWriter(Vec::new());
        write_records(
            &mut writer,
            [encoded(&tick(100))],
            &counters,
            &mut WriteProgress::default(),
        );
        assert!(counters.failed.load(Ordering::Relaxed));
        assert_eq!(counters.written.load(Ordering::Relaxed), 0);
        assert!(replay::read(writer.0.as_slice()).is_err());
    }
}
