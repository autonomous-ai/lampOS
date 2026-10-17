//! A worker's first control message may have waited for the receiver to
//! start reading (#762). Any later message may wait only as long as the
//! receiver was itself held up, at most 1.5 s (#784): a receiver that has
//! not looked for longer than its bound is taken to have been held up, as by
//! the kernel re-tuning lamp-4ace's SD/MMC hosts, and what queued meanwhile
//! is in order and still carries its own deadlines.
use lamp_ipc::monotonic_us;
use lamp_voice::{
    process::{SessionDirectory, new_boot},
    transport::WorkerChannels,
    wire::WorkerEvent,
};

#[test]
fn a_first_control_message_may_wait_for_the_receiver_and_nothing_after_it() {
    let directory = SessionDirectory::create().unwrap();
    let (parent_boot, worker_boot) = (new_boot().unwrap(), new_boot().unwrap());
    let mut parent =
        WorkerChannels::bind(&directory.path, "capture", parent_boot, worker_boot, false).unwrap();
    let mut worker =
        WorkerChannels::bind(&directory.path, "capture", parent_boot, worker_boot, true).unwrap();
    parent.connect(&directory.path, "capture", false).unwrap();
    worker.connect(&directory.path, "capture", true).unwrap();
    // Both sides on one clock the test moves on by what it once slept: the
    // same ages and the same gaps between looks, without the 3.7 s (#1128).
    let mut t = monotonic_us();
    // The first control message waits 300 ms, as at start: taken.
    worker.control.send_with(WorkerEvent::Ready, || t).unwrap();
    t += 300_000;
    assert!(matches!(
        parent.control.receive_with::<WorkerEvent>(|| t).unwrap(),
        Some(WorkerEvent::Ready)
    ));
    // A later one that waits past the longest hold-up is refused, and says
    // which socket.
    worker.control.send_with(WorkerEvent::Ready, || t).unwrap();
    t += 1_700_000;
    let error = parent
        .control
        .receive_with::<WorkerEvent>(|| t)
        .unwrap_err();
    assert!(error.to_string().starts_with("capture.c.p: "), "{error}");
    // A first data message gets no first-message grace either.
    worker.data.send_with(WorkerEvent::Ready, || t).unwrap();
    t += 1_700_000;
    assert!(parent.data.receive_with::<WorkerEvent>(|| t).is_err());

    // The plain `send` and `receive` read the host's clock: a message sent
    // now is taken now (the fifth on #1128, so their wiring stays tested).
    let (parent_boot, worker_boot) = (new_boot().unwrap(), new_boot().unwrap());
    let mut parent =
        WorkerChannels::bind(&directory.path, "speaker", parent_boot, worker_boot, false).unwrap();
    let mut worker =
        WorkerChannels::bind(&directory.path, "speaker", parent_boot, worker_boot, true).unwrap();
    parent.connect(&directory.path, "speaker", false).unwrap();
    worker.connect(&directory.path, "speaker", true).unwrap();
    worker.control.send(WorkerEvent::Ready).unwrap();
    assert!(matches!(
        parent.control.receive::<WorkerEvent>().unwrap(),
        Some(WorkerEvent::Ready)
    ));
}
