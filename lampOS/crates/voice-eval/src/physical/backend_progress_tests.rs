//! Controlled-reader bounds, with no consumer progress until the reader closes.
use super::*;
use std::{
    io::{self, Cursor, Read},
    os::unix::net::UnixStream,
};

const GUARD: Duration = Duration::from_secs(5);
type Line = std::result::Result<SessionLine, String>;

struct CloseNotice<R> {
    reader: Option<R>,
    closed: SyncSender<()>,
}

impl<R: Read> Read for CloseNotice<R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.reader.as_mut().unwrap().read(bytes)
    }
}

impl<R> Drop for CloseNotice<R> {
    fn drop(&mut self) {
        // The acknowledgement follows actual input closure, including an OS
        // socket, rather than merely announcing that Drop is about to run.
        drop(self.reader.take());
        let _ = self.closed.try_send(());
    }
}

fn controlled<R: Read + Send + 'static>(
    reader: R,
    capacity: usize,
) -> (Receiver<Line>, Arc<AtomicU64>, Receiver<()>) {
    let (closed, notice) = mpsc::sync_channel(1);
    let enqueued = Arc::new(AtomicU64::new(0));
    let lines = relay_reader_with_progress_capacity(
        CloseNotice {
            reader: Some(reader),
            closed,
        },
        Some(enqueued.clone()),
        capacity,
    );
    (lines, enqueued, notice)
}

fn append(bytes: &mut Vec<u8>, line: &SessionLine) {
    serde_json::to_writer(&mut *bytes, line).unwrap();
    bytes.push(b'\n');
}

fn trace(index: usize) -> SessionLine {
    SessionLine::Trace {
        record: json!({"kind":"fixture_record", "sequence":index}),
    }
}

fn trace_bytes(count: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    for index in 0..count {
        append(&mut bytes, &trace(index));
    }
    bytes
}

fn closed_before_consumption(closed: Receiver<()>) {
    closed
        .recv_timeout(GUARD)
        .expect("the controlled reader blocked before the backend drained its queue");
}

fn drain(lines: Receiver<Line>, count: usize) -> Vec<Line> {
    let records = (0..count)
        .map(|_| lines.recv_timeout(GUARD).expect("missing queued record"))
        .collect();
    assert!(matches!(
        lines.recv_timeout(GUARD),
        Err(RecvTimeoutError::Disconnected)
    ));
    records
}

#[test]
fn more_than_the_default_queue_is_enqueued_before_any_backend_consumption() {
    let count = 5_000;
    let (lines, enqueued, closed) = controlled(
        Cursor::new(trace_bytes(count)),
        FIXTURE_RELAY_QUEUE_CAPACITY,
    );
    closed_before_consumption(closed);
    assert_eq!(enqueued.load(Ordering::Acquire), count as u64);
    for (index, record) in drain(lines, count).into_iter().enumerate() {
        assert_eq!(record, Ok(trace(index)));
    }
}

#[test]
fn the_maximum_final_trace_and_session_end_are_enqueued_before_consumption() {
    let count = crate::physical::protocol::MAX_TRACE_LINES;
    let mut bytes = trace_bytes(count);
    let end = SessionLine::SessionEnd {
        lamp_us: 9_000_000,
        exit_code: Some(0),
        killed: false,
        events_sha256: None,
        events_lines: count,
        error: None,
        stray_processes_stopped: false,
        serve: None,
    };
    append(&mut bytes, &end);
    let (lines, enqueued, closed) = controlled(Cursor::new(bytes), FIXTURE_RELAY_QUEUE_CAPACITY);
    closed_before_consumption(closed);
    assert_eq!(enqueued.load(Ordering::Acquire), (count + 1) as u64);
    let mut records = drain(lines, count + 1).into_iter();
    for index in 0..count {
        assert_eq!(records.next(), Some(Ok(trace(index))));
    }
    assert_eq!(records.next(), Some(Ok(end)));
    assert!(records.next().is_none());
}

#[test]
fn the_exact_fixture_limit_preserves_every_record_without_an_overflow_error() {
    let (lines, enqueued, closed) = controlled(Cursor::new(trace_bytes(3)), 3);
    closed_before_consumption(closed);
    assert_eq!(enqueued.load(Ordering::Acquire), 3);
    assert_eq!(
        drain(lines, 3),
        (0..3).map(|i| Ok(trace(i))).collect::<Vec<_>>()
    );
}

#[test]
fn overflow_keeps_a_terminal_error_slot_and_closes_the_real_transport() {
    let (reader, mut writer) = UnixStream::pair().unwrap();
    writer.set_write_timeout(Some(GUARD)).unwrap();
    let (lines, enqueued, closed) = controlled(reader, 2);
    writer.write_all(&trace_bytes(3)).unwrap();
    closed_before_consumption(closed);
    // No queue item has been consumed, yet a waiting relay writer can observe
    // the closed read end instead of remaining blocked behind the full queue.
    // A child another test forks at that moment holds a copy of the read end
    // until its exec (CLOEXEC), so a write may still be taken for a moment
    // (#1462): it writes until one is refused, for at most `GUARD`.
    let until = std::time::Instant::now() + GUARD;
    let error = loop {
        match writer.write_all(b"still writing\n") {
            Ok(()) if std::time::Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(1));
            }
            Ok(()) => panic!("still written {GUARD:?} after the read end closed"),
            Err(error) => break error,
        }
    };
    assert!(matches!(
        error.kind(),
        io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset | io::ErrorKind::NotConnected
    ));
    assert_eq!(enqueued.load(Ordering::Acquire), 2);
    let records = drain(lines, 3);
    assert_eq!(records[0], Ok(trace(0)));
    assert_eq!(records[1], Ok(trace(1)));
    assert_eq!(
        records[2],
        Err("fixture relay exceeded 2 framed records in one session; reading stopped".into())
    );
}

#[test]
fn complete_malformed_frames_count_but_partial_eof_does_not() {
    let mut bytes = trace_bytes(1);
    bytes.extend_from_slice(b"\xff\n");
    bytes.extend(std::iter::repeat_n(b'x', MAX_LINE_BYTES + 1));
    bytes.extend_from_slice(b"\nnot json\npartial");
    let (lines, enqueued, closed) = controlled(Cursor::new(bytes), 8);
    closed_before_consumption(closed);
    assert_eq!(enqueued.load(Ordering::Acquire), 4);
    let records = drain(lines, 5);
    assert_eq!(records[0], Ok(trace(0)));
    assert!(records[1].as_ref().unwrap_err().contains("not valid UTF-8"));
    assert!(
        records[2]
            .as_ref()
            .unwrap_err()
            .contains("exceeds the 65536-byte limit")
    );
    assert!(records[3].is_err());
    assert!(
        records[4]
            .as_ref()
            .unwrap_err()
            .contains("closed inside a line")
    );
}

struct ReadFailure(Cursor<Vec<u8>>);

impl Read for ReadFailure {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if self.0.position() == self.0.get_ref().len() as u64 {
            return Err(io::Error::other("injected read failure"));
        }
        self.0.read(bytes)
    }
}

#[test]
fn a_read_error_uses_the_reserved_slot_after_the_last_allowed_record() {
    let (lines, enqueued, closed) = controlled(ReadFailure(Cursor::new(trace_bytes(2))), 2);
    closed_before_consumption(closed);
    assert_eq!(enqueued.load(Ordering::Acquire), 2);
    let records = drain(lines, 3);
    assert_eq!(records[0], Ok(trace(0)));
    assert_eq!(records[1], Ok(trace(1)));
    assert_eq!(
        records[2],
        Err("relay transport read failed: injected read failure".into())
    );
}

struct GatedRead {
    reader: Cursor<Vec<u8>>,
    proceed: Option<Receiver<()>>,
}

impl Read for GatedRead {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if let Some(proceed) = self.proceed.take() {
            proceed
                .recv_timeout(GUARD)
                .map_err(|_| io::Error::other("read gate was never released"))?;
        }
        self.reader.read(bytes)
    }
}

#[test]
fn a_frame_that_cannot_be_enqueued_never_advances_progress() {
    let (proceed, gate) = mpsc::sync_channel(1);
    let (lines, enqueued, closed) = controlled(
        GatedRead {
            reader: Cursor::new(trace_bytes(1)),
            proceed: Some(gate),
        },
        2,
    );
    drop(lines);
    proceed.send(()).unwrap();
    closed_before_consumption(closed);
    assert_eq!(enqueued.load(Ordering::Acquire), 0);
}
