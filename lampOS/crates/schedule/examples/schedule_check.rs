//! Portable storage/IPC qualification, using only this binary and its private
//! temporary files. Example:
//! schedule_check --scratch-parent /var/lib/lamp --samples 100
//!
//! Calendar/boot timestamps and downtime are authored. Instant measures real
//! computation, persistence and local IPC on the running host. Nothing here
//! measures sound, Voice delivery, physical downtime or power-loss survival.
use lamp_bus::Bus;
use lamp_msgs::{ClockStamp, ScheduleDue};
use lamp_schedule::{
    engine::{Engine, MAX_ITEMS},
    protocol::{Guard, MAX_PAGE_SIZE, MAX_REPLY_BYTES, Request},
    runtime::{self, Clock, Config},
    store::Store,
    time::{ClockSample, When},
};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const WALL_MS: i64 = 1_791_964_800_000;
const DOWNTIME_MS: u64 = 3_600_000;
const TIMER_SECONDS: u64 = 600;
const BOOT: &str = "schedule-check-authored-boot";
const WAIT_LIMIT: Duration = Duration::from_secs(5);
const CALL_LIMIT: Duration = Duration::from_secs(2);

fn context(stage: impl AsRef<str>, error: io::Error) -> io::Error {
    // Startup retry decisions still depend on the original error kind.
    io::Error::new(error.kind(), format!("{}: {error}", stage.as_ref()))
}

fn require(condition: bool, message: impl Into<String>) -> io::Result<()> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message.into()))
    }
}

fn sample(at_ms: u64) -> ClockSample {
    ClockSample {
        unix_ms: WALL_MS + at_ms as i64,
        boot_ms: at_ms,
        boot_id: BOOT.into(),
        wall_trusted: true,
    }
}

fn number(value: &Value, field: &str) -> io::Result<u64> {
    value[field]
        .as_u64()
        .ok_or_else(|| io::Error::other(format!("missing numeric reply field {field}")))
}

fn accepted(value: Value) -> io::Result<Value> {
    require(
        value["ok"] == true,
        format!("qualification request was refused: {value}"),
    )?;
    Ok(value)
}

fn guard(status: &Value, id: String) -> io::Result<Guard> {
    Ok(Guard {
        store_id: status["store_id"]
            .as_str()
            .ok_or_else(|| io::Error::other("reply lacks store_id"))?
            .into(),
        revision: number(status, "revision")?,
        request_id: id,
    })
}

fn statistics(boundary: &str, values: &[Duration]) -> Value {
    let all: Vec<f64> = values
        .iter()
        .map(|value| value.as_secs_f64() * 1000.0)
        .collect();
    let warm = all[1..].to_vec();
    let mut sorted = warm.clone();
    sorted.sort_by(f64::total_cmp);
    let median = if sorted.is_empty() {
        None
    } else {
        let middle = sorted.len() / 2;
        Some(if sorted.len().is_multiple_of(2) {
            (sorted[middle - 1] + sorted[middle]) / 2.0
        } else {
            sorted[middle]
        })
    };
    json!({
        "boundary":boundary,"unit":"milliseconds","count":all.len(),"first_ms":all[0],
        "warm_count":warm.len(),"warm_median_ms":median,"warm_max_ms":sorted.last(),
        "max_ms":all.iter().copied().reduce(f64::max),"warm_ms":warm,"all_ms":all
    })
}

fn disk_checks(path: &Path, warm: usize) -> io::Result<Value> {
    let now = sample(0);
    let started = Instant::now();
    let mut engine = Engine::open(
        Store::open(path).map_err(|error| context("fresh Store::open", error))?,
        "Etc/UTC",
        &now,
        "11111111111111111111111111111111",
    )
    .map_err(|error| context("initial Engine::open", io::Error::other(error)))?;
    let initialization = started.elapsed();
    let mut sets = Vec::new();
    for index in 0..=warm {
        let request = Request::Set {
            guard: Guard {
                store_id: engine.store_id().into(),
                revision: engine.revision(),
                request_id: format!("disk-set-{index}"),
            },
            text: format!("Qualification timer {index}"),
            when: When::After {
                seconds: TIMER_SECONDS,
            },
        };
        let began = Instant::now();
        let reply = engine.answer(request, &now).map_err(|error| {
            context(
                format!("Engine::answer Set index {index}"),
                io::Error::other(error),
            )
        })?;
        sets.push(began.elapsed());
        let reply = accepted(reply)
            .map_err(|error| context(format!("Set receipt index {index}"), error))?;
        require(
            number(&reply, "id")? == index as u64 + 1,
            "disk set returned an unexpected identity",
        )?;
        require(
            reply["due_unix_ms"] == WALL_MS + TIMER_SECONDS as i64 * 1000,
            "elapsed timer has the wrong deadline",
        )?;
    }
    let count = warm + 1;
    require(
        engine.counts(&now) == (count, 0, 0),
        "disk set count differs from successful receipts",
    )?;
    let mut lists = Vec::new();
    for index in 0..=warm {
        let request = Request::List {
            after_id: 0,
            limit: MAX_PAGE_SIZE,
            pending_only: false,
        };
        let began = Instant::now();
        let reply = engine.answer(request, &now).map_err(|error| {
            context(
                format!("Engine::answer List index {index}"),
                io::Error::other(error),
            )
        })?;
        lists.push(began.elapsed());
        let reply = accepted(reply)
            .map_err(|error| context(format!("List receipt index {index}"), error))?;
        require(
            reply["items"]
                .as_array()
                .is_some_and(|items| items.len() == count.min(MAX_PAGE_SIZE as usize)),
            "list page lost scheduled items",
        )?;
    }
    let original_id = engine.store_id().to_owned();
    drop(engine);
    let resumed = sample(DOWNTIME_MS);
    let began = Instant::now();
    let mut engine = Engine::open(
        Store::open(path).map_err(|error| context("same-boot Store::open", error))?,
        "Etc/UTC",
        &resumed,
        "22222222222222222222222222222222",
    )
    .map_err(|error| context("same-boot Engine::open", io::Error::other(error)))?;
    let reopened = began.elapsed();
    require(
        engine.store_id() == original_id && engine.counts(&resumed) == (count, 0, 0),
        "same-boot restart lost state or changed store identity",
    )?;
    let began = Instant::now();
    require(
        engine
            .tick(&resumed)
            .map_err(|error| context("overdue Engine::tick", io::Error::other(error)))?,
        "downtime did not produce a durable pending transition",
    )?;
    let due_commit = began.elapsed();
    require(
        engine.counts(&resumed) == (count, count, 0),
        "recovered due count is incorrect",
    )?;
    let mut acks = Vec::new();
    for index in 0..count {
        let due = engine.due(
            &resumed,
            ClockStamp {
                clock_id: BOOT.into(),
                at_us: DOWNTIME_MS * 1000,
                sequence: index as u64 + 1,
            },
            true,
        );
        due.check().map_err(|error| {
            context(
                format!("due check before Ack index {index}"),
                io::Error::other(error),
            )
        })?;
        require(
            due.pending as usize == count - index,
            "ack did not remove exactly one pending occurrence",
        )?;
        let notice = due
            .oldest
            .ok_or_else(|| io::Error::other("pending work lacks an exact delivery token"))?;
        require(
            notice.id == index as u64 + 1 && notice.delivery == 1 && notice.late,
            "recovered occurrence has wrong identity or late flag",
        )?;
        require(
            notice.late_by_ms == Some(DOWNTIME_MS - TIMER_SECONDS * 1000),
            "authored overdue duration is wrong",
        )?;
        let request = Request::Ack {
            guard: Guard {
                store_id: engine.store_id().into(),
                revision: engine.revision(),
                request_id: format!("disk-ack-{}", notice.id),
            },
            id: notice.id,
            delivery: notice.delivery,
        };
        let began = Instant::now();
        let receipt = engine.answer(request.clone(), &resumed).map_err(|error| {
            context(
                format!("Engine::answer Ack index {index}"),
                io::Error::other(error),
            )
        })?;
        acks.push(began.elapsed());
        let receipt = accepted(receipt)
            .map_err(|error| context(format!("Ack receipt index {index}"), error))?;
        require(
            receipt["acknowledged"] == true,
            "ack did not return durable completion",
        )?;
        if index == 0 {
            let revision = engine.revision();
            let replayed = engine.answer(request, &resumed).map_err(|error| {
                context("Engine::answer exact Ack replay", io::Error::other(error))
            })?;
            require(
                replayed == receipt && engine.revision() == revision,
                "exact ack replay changed state or its receipt",
            )?;
        }
    }
    require(
        engine.counts(&resumed) == (0, 0, 0),
        "acknowledgements did not empty the one-shot queue",
    )?;
    drop(engine);
    let reopened_empty = Engine::open(
        Store::open(path).map_err(|error| context("post-Ack Store::open", error))?,
        "Etc/UTC",
        &resumed,
        "33333333333333333333333333333333",
    )
    .map_err(|error| context("post-Ack Engine::open", io::Error::other(error)))?;
    require(
        reopened_empty.counts(&resumed) == (0, 0, 0),
        "acknowledgements did not survive reopen",
    )?;
    Ok(json!({
        "checks":{"set_receipts":count,"restart_preserved_identity_and_items":true,"recovered_pending":count,
            "every_recovered_notice_late":true,"exact_delivery_acks":count,"immediate_ack_replay_equal":true,"pending_after_ack_and_reopen":0},
        "timings":{
            "fresh_open":statistics("Store::open plus Engine::open: create/initialize database, durable initial snapshot, zone/state validation; excludes scratch-directory creation",&[initialization]),
            "set":statistics("Engine::answer(Set), including validation, snapshot serialization, SQLite commit and directory sync; excludes request construction and wire JSON",&sets),
            "list_first_page":statistics("Engine::answer(List first page of at most 8 items), including current tick and view construction; excludes request construction and wire JSON",&lists),
            "same_boot_reopen":statistics("Store::open plus Engine::open on the existing database, including recovery/integrity/state validation; excludes closing the previous owner",&[reopened]),
            "overdue_tick":statistics("one Engine::tick at authored +3600 seconds, including persisting all newly pending items",&[due_commit]),
            "ack":statistics("Engine::answer(Ack) through durable receipt, one exact occurrence each; excludes token/guard construction and replay verification",&acks)
        }
    }))
}

#[derive(Clone)]
struct Injected(Arc<Mutex<ClockSample>>);
impl Clock for Injected {
    fn sample(&self) -> io::Result<ClockSample> {
        self.0
            .lock()
            .map(|value| value.clone())
            .map_err(|_| io::Error::other("authored clock lock poisoned"))
    }
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|left| !left.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "qualification wait expired"))
}

fn wait_ready(stream: &UnixStream, events: PollFlags, deadline: Instant) -> io::Result<()> {
    loop {
        let timeout = Timespec::try_from(remaining(deadline)?)
            .map_err(|error| context("IPC poll timeout conversion", io::Error::other(error)))?;
        let mut descriptors = [PollFd::new(stream, events)];
        match poll(&mut descriptors, Some(&timeout)) {
            Ok(0) => continue,
            Ok(_) => {
                remaining(deadline)?;
                let returned = descriptors[0].revents();
                require(
                    !returned.contains(PollFlags::NVAL),
                    "IPC poll invalid descriptor",
                )?;
                // A closed peer can leave readable bytes. Let read/write report
                // EOF or the actual socket error after buffered data is drained.
                if returned.intersects(events | PollFlags::HUP | PollFlags::ERR) {
                    return Ok(());
                }
            }
            Err(error) if error == rustix::io::Errno::INTR => {}
            Err(error) => return Err(context("IPC poll", error.into())),
        }
    }
}

fn write_bytes(stream: &mut UnixStream, bytes: &[u8], deadline: Instant) -> io::Result<()> {
    let mut written = 0;
    while written < bytes.len() {
        remaining(deadline).map_err(|error| context("IPC write deadline", error))?;
        match stream.write(&bytes[written..]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "IPC write: qualification socket closed during write",
                ));
            }
            Ok(count) => written += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                wait_ready(stream, PollFlags::OUT, deadline)
                    .map_err(|error| context("IPC write readiness", error))?;
            }
            Err(error) => return Err(context("IPC write", error)),
        }
    }
    remaining(deadline).map_err(|error| context("IPC complete-write deadline", error))?;
    Ok(())
}

fn read_reply(stream: &mut UnixStream, deadline: Instant) -> io::Result<Value> {
    let mut reply = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        remaining(deadline).map_err(|error| context("IPC read deadline", error))?;
        let count = match stream.read(&mut chunk) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "IPC read: qualification socket closed without a complete reply",
                ));
            }
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                wait_ready(stream, PollFlags::IN, deadline)
                    .map_err(|error| context("IPC read readiness", error))?;
                continue;
            }
            Err(error) => return Err(context("IPC read", error)),
        };
        require(
            reply.len() + count <= MAX_REPLY_BYTES + 1,
            "qualification socket reply exceeds protocol limit",
        )?;
        reply.extend_from_slice(&chunk[..count]);
        if let Some(end) = reply.iter().position(|byte| *byte == b'\n') {
            require(
                end + 1 == reply.len(),
                "qualification socket returned data after its reply",
            )?;
            let reply = accepted(
                serde_json::from_slice(&reply[..end])
                    .map_err(|error| context("IPC parse reply", io::Error::other(error)))?,
            )
            .map_err(|error| context("IPC receipt", error))?;
            remaining(deadline).map_err(|error| context("IPC complete-reply deadline", error))?;
            return Ok(reply);
        }
    }
}

fn ask_until(path: &Path, request: &Request, deadline: Instant) -> io::Result<Value> {
    let deadline = deadline.min(Instant::now() + CALL_LIMIT);
    // The budget starts before connect. This isolated fixture uses one caller
    // and a private listener; a blocking OS connect stall is not qualified.
    let mut stream = UnixStream::connect(path).map_err(|error| context("IPC connect", error))?;
    stream
        .set_nonblocking(true)
        .map_err(|error| context("IPC set_nonblocking", error))?;
    let mut bytes = serde_json::to_vec(request)
        .map_err(|error| context("IPC serialize request", io::Error::other(error)))?;
    bytes.push(b'\n');
    write_bytes(&mut stream, &bytes, deadline)?;
    read_reply(&mut stream, deadline)
}

fn ask(path: &Path, request: &Request) -> io::Result<Value> {
    ask_until(path, request, Instant::now() + CALL_LIMIT)
}

struct Running {
    config: Config,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<io::Result<()>>>,
    cleanup_safe: Arc<AtomicBool>,
}

impl Running {
    fn start(config: Config, clock: Injected, cleanup_safe: Arc<AtomicBool>) -> io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker_config = config.clone();
        let handle = thread::Builder::new()
            .name("schedule-qualification".into())
            .spawn(move || runtime::serve(worker_config, clock, worker_stop))
            .map_err(|error| context("runtime thread spawn", error))?;
        cleanup_safe.store(false, Ordering::Release);
        let mut running = Self {
            config,
            stop,
            handle: Some(handle),
            cleanup_safe,
        };
        let deadline = Instant::now() + WAIT_LIMIT;
        loop {
            if running
                .handle
                .as_ref()
                .is_some_and(|handle| handle.is_finished())
            {
                running
                    .shutdown()
                    .map_err(|error| context("runtime exited during startup", error))?;
                return Err(io::Error::other(
                    "Schedule exited before qualification became ready",
                ));
            }
            match ask_until(&running.config.socket, &Request::Status {}, deadline) {
                Ok(status) => {
                    require(
                        number(&status, "items")
                            .map_err(|error| context("runtime startup status", error))?
                            == 0,
                        "runtime scratch store was not empty",
                    )?;
                    return Ok(running);
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                    ) =>
                {
                    thread::sleep(
                        remaining(deadline)
                            .map_err(|error| context("runtime startup retry deadline", error))?
                            .min(Duration::from_millis(5)),
                    );
                }
                Err(error) => return Err(context("runtime startup status", error)),
            }
        }
    }

    fn shutdown(&mut self) -> io::Result<()> {
        self.stop.store(true, Ordering::Release);
        let deadline = Instant::now() + WAIT_LIMIT;
        while self
            .handle
            .as_ref()
            .is_some_and(|handle| !handle.is_finished())
        {
            let Ok(left) = remaining(deadline) else {
                self.handle.take();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "runtime did not stop within 5 seconds; scratch must be retained",
                ));
            };
            thread::sleep(left.min(Duration::from_millis(5)));
        }
        if let Some(handle) = self.handle.take() {
            handle
                .join()
                .map_err(|_| {
                    io::Error::other("runtime shutdown join: qualification runtime panicked")
                })?
                .map_err(|error| context("runtime shutdown serve result", error))?;
            self.cleanup_safe.store(true, Ordering::Release);
        }
        Ok(())
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if self.handle.is_some()
            && let Err(error) = self.shutdown()
        {
            eprintln!("schedule_check shutdown failed: {error}");
        }
    }
}

fn check_pending_pages(socket: &Path, expected: usize) -> io::Result<usize> {
    let deadline = Instant::now() + WAIT_LIMIT;
    let mut after_id = 0;
    let mut seen = 0;
    for page in 0..=MAX_ITEMS / usize::from(MAX_PAGE_SIZE) {
        let reply = ask_until(
            socket,
            &Request::List {
                after_id,
                limit: MAX_PAGE_SIZE,
                pending_only: true,
            },
            deadline,
        )
        .map_err(|error| {
            context(
                format!("pending List page {page} after ID {after_id}"),
                error,
            )
        })?;
        let items = reply["items"]
            .as_array()
            .ok_or_else(|| io::Error::other("pending page lacks items"))?;
        for item in items {
            let id = number(item, "id")?;
            require(
                id > after_id
                    && item["state"] == "pending"
                    && item["late"] == true
                    && item["delivery"] == 1,
                "pending page has duplicate, nonpending or nonlate occurrence",
            )?;
            after_id = id;
            seen += 1;
        }
        if reply["next_after_id"].is_null() {
            require(seen == expected, "pending pages lost occurrences")?;
            return Ok(seen);
        }
        require(
            number(&reply, "next_after_id")? == after_id && !items.is_empty(),
            "pending cursor did not advance",
        )?;
    }
    Err(io::Error::other(
        "pending pagination exceeded schedule capacity",
    ))
}

fn ipc_measurements(running: &Running, clock: &Injected, warm: usize) -> io::Result<Value> {
    let socket = &running.config.socket;
    let count = warm + 1;
    let mut sets = Vec::new();
    let mut statuses = Vec::new();
    let mut store_id = String::new();
    for index in 0..=warm {
        // Guard acquisition is deliberately outside the measured set call.
        let status = ask(socket, &Request::Status {})
            .map_err(|error| context(format!("guard Status before Set index {index}"), error))?;
        let request = Request::Set {
            guard: guard(&status, format!("ipc-set-{index}")).map_err(|error| {
                context(format!("guard construction for Set index {index}"), error)
            })?,
            text: format!("IPC qualification timer {index}"),
            when: When::After {
                seconds: TIMER_SECONDS,
            },
        };
        let began = Instant::now();
        let reply =
            ask(socket, &request).map_err(|error| context(format!("Set index {index}"), error))?;
        sets.push(began.elapsed());
        require(
            number(&reply, "id")? == index as u64 + 1,
            "IPC set returned an unexpected identity",
        )?;
        store_id = reply["store_id"]
            .as_str()
            .ok_or_else(|| io::Error::other("IPC receipt lacks store identity"))?
            .into();
    }
    for index in 0..=warm {
        let began = Instant::now();
        let reply = ask(socket, &Request::Status {})
            .map_err(|error| context(format!("Status index {index}"), error))?;
        statuses.push(began.elapsed());
        require(
            number(&reply, "items")? as usize == count && number(&reply, "pending")? == 0,
            "IPC status count differs from durable receipts",
        )?;
    }
    let bus = Bus::new(&running.config.bus_root).map_err(|error| context("due Bus::new", error))?;
    let mut subscriber = bus
        .subscribe_typed(ScheduleDue::TOPIC, "q", ScheduleDue::TYPE)
        .map_err(|error| context("due bus subscribe_typed", error))?;
    let began = Instant::now();
    *clock
        .0
        .lock()
        .map_err(|_| io::Error::other("authored clock lock poisoned"))? = sample(DOWNTIME_MS);
    let deadline = began + WAIT_LIMIT;
    let due = loop {
        if let Some(message) = subscriber
            .recv(
                remaining(deadline)
                    .map_err(|error| context("due bus receive deadline", error))?
                    .min(Duration::from_millis(50)),
            )
            .map_err(|error| context("due bus recv", error))?
        {
            require(
                !message.expired,
                "bus handed over an expired qualification notice",
            )?;
            let due: ScheduleDue = message
                .parse()
                .map_err(|error| context("due bus message parse", error))?;
            due.check()
                .map_err(|error| context("due bus message check", io::Error::other(error)))?;
            require(
                due.store_id == store_id && due.stamp.clock_id == BOOT && due.enabled && due.ready,
                "bus notice has wrong identity or an unhealthy owner",
            )?;
            if due.pending as usize == count {
                break due;
            }
            require(due.pending == 0, "due transition exposed a partial batch")?;
        }
    };
    let due_receive = began.elapsed();
    require(
        due.waiting_for_clock == 0
            && due
                .oldest
                .as_ref()
                .is_some_and(|notice| notice.id == 1 && notice.delivery == 1 && notice.late),
        "due notice lacks a late exact occurrence",
    )?;
    let verified = check_pending_pages(socket, count)
        .map_err(|error| context("pending-page verification", error))?;
    let silent =
        UnixStream::connect(socket).map_err(|error| context("silent client IPC connect", error))?;
    let began = Instant::now();
    let beside_silent = ask(socket, &Request::Status {})
        .map_err(|error| context("Status beside silent client", error))?;
    let silent_status = began.elapsed();
    require(
        number(&beside_silent, "pending")? as usize == count,
        "silent client blocked or changed pending work",
    )?;
    drop(silent);
    Ok(json!({
        "checks":{"set_receipts":count,"status_replies":count,"received_pending":due.pending,
            "every_pending_item_verified_late":verified,"silent_client_status_succeeded":true},
        "timings":{
            "set_roundtrip":statistics("Unix connect, request serialization/write, owner queue/validation/durable commit, reply read/parse/ok validation; excludes guard status request",&sets),
            "status_roundtrip":statistics("Unix connect, status request write, owner reply read/parse/ok validation",&statuses),
            "due_to_bus_receive":statistics("authored clock jump through worker scheduling (nominal 50 ms scan), pending commit, publish and subscriber receive (datagram or retained Latest); excludes subscription setup",&[due_receive]),
            "status_beside_silent_client":statistics("one status Unix roundtrip after opening a second connection that sends nothing; excludes silent connect",&[silent_status])
        }
    }))
}

fn ipc_checks(root: &Path, warm: usize, cleanup_safe: Arc<AtomicBool>) -> io::Result<Value> {
    let clock = Injected(Arc::new(Mutex::new(sample(0))));
    let config = Config {
        enabled: true,
        dir: root.join("i"),
        socket: root.join("r/s"),
        bus_root: root.join("b"),
        zone: "Etc/UTC".into(),
        utc_offset_minutes: None,
        run_for: None,
        report_every: lamp_schedule::runtime::REPORT_EVERY,
        request_deadline: lamp_schedule::runtime::REQUEST_DEADLINE,
    };
    let mut running = Running::start(config, clock.clone(), cleanup_safe)
        .map_err(|error| context("runtime startup", error))?;
    let result = ipc_measurements(&running, &clock, warm);
    let stopped = running
        .shutdown()
        .map_err(|error| context("runtime shutdown", error));
    match (result, stopped) {
        (Ok(report), Ok(())) => Ok(report),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(stop)) => Err(io::Error::other(format!(
            "{error}; shutdown also failed: {stop}"
        ))),
    }
}

fn arguments() -> io::Result<(PathBuf, usize)> {
    let mut args = std::env::args_os().skip(1);
    let mut parent = None;
    let mut samples = None;
    while let Some(option) = args.next() {
        match option.to_str() {
            Some("--scratch-parent") if parent.is_none() => {
                parent = Some(PathBuf::from(args.next().ok_or_else(|| {
                    io::Error::other("--scratch-parent needs an existing absolute path")
                })?))
            }
            Some("--samples") if samples.is_none() => {
                let value = args
                    .next()
                    .ok_or_else(|| io::Error::other("--samples needs 1..100"))?;
                samples = Some(
                    value
                        .to_str()
                        .ok_or_else(|| io::Error::other("sample count must be UTF-8"))?
                        .parse::<usize>()
                        .map_err(io::Error::other)?,
                );
            }
            _ => {
                return Err(io::Error::other(
                    "usage: schedule_check --scratch-parent ABS [--samples 1..100]; unknown or duplicate option",
                ));
            }
        }
    }
    let parent = parent.ok_or_else(|| {
        io::Error::other("--scratch-parent ABS is required; choose the persistent disk to qualify")
    })?;
    require(
        parent.is_absolute() && parent.is_dir(),
        "scratch parent must be an existing absolute directory",
    )?;
    let parent = parent
        .canonicalize()
        .map_err(|error| context("scratch parent canonicalize", error))?;
    require(
        parent.to_str().is_some(),
        "scratch parent must have a UTF-8 path for the JSON report",
    )?;
    let samples = samples.unwrap_or(100);
    require(
        (1..=100).contains(&samples) && samples < MAX_ITEMS,
        "--samples must be 1..100 warm runs",
    )?;
    Ok((parent, samples))
}

fn run() -> io::Result<Value> {
    let (parent, warm) = arguments().map_err(|error| context("arguments", error))?;
    let root = tempfile::Builder::new()
        .prefix("sc-")
        .rand_bytes(4)
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(&parent)
        .map_err(|error| context("private scratch directory creation", error))?;
    let path = root.path().to_path_buf();
    let cleanup_safe = Arc::new(AtomicBool::new(true));
    let result = (|| {
        let disk = disk_checks(&path.join("d"), warm)
            .map_err(|error| context("direct-engine stage", error))?;
        let ipc = ipc_checks(&path, warm, cleanup_safe.clone())
            .map_err(|error| context("runtime stage", error))?;
        Ok(json!({
            "ok":true,"kind":"schedule_storage_and_local_ipc_qualification",
            "os":std::env::consts::OS,"arch":std::env::consts::ARCH,"debug_assertions":cfg!(debug_assertions),
            "scratch_parent":parent,"private_scratch_directory":path,
            "paths":{"engine_database_directory":path.join("d"),"runtime_database_directory":path.join("i"),"socket":path.join("r/s"),"bus_root":path.join("b")},
            "cleanup":"own temporary directory removed after all owners stopped",
            "time_basis":{"durations":"real host Instant elapsed; filesystem and local IPC included as labelled",
                "calendar":"authored trusted UTC and authored same-boot elapsed clock; system clocks unchanged",
                "initial_unix_ms":WALL_MS,"initial_boot_ms":0,"injected_jump_ms":DOWNTIME_MS,"timer_seconds":TIMER_SECONDS,
                "runtime_scan_interval_ms":runtime::TICK.as_millis(),"acoustics_or_physical_downtime_measured":false},
            "warm_samples_per_series":warm,"timer_capacity":MAX_ITEMS,"per_call_functional_cap_ms":CALL_LIMIT.as_millis(),
            "reference_response_target_ms":250,"target_latency_asserted":false,
            "reference_target_boundary":"healthy storage: complete valid request through durable response; target only, not a lamp guarantee",
            "notes":["Choose a short scratch parent path for Unix socket limits.","A tmpfs or RAM-backed scratch parent does not qualify persistent storage.","First means first call of that operation after the setup described in its boundary, not whole-process cold start.","The IPC deadline starts before connect; reads/writes use nonblocking polling with that cumulative deadline. A blocking OS connect stall is not qualified.","Filesystem syscalls can stall independently of bounded IPC/wait loops; this does not prove physical power-loss survival."],
            "disk":disk,"runtime":ipc
        }))
    })();
    if !cleanup_safe.load(Ordering::Acquire) {
        let retained = root.keep();
        let cause = result.err().map_or_else(
            || "runtime shutdown was incomplete".into(),
            |error: io::Error| error.to_string(),
        );
        return Err(io::Error::other(format!(
            "{cause}; retained own scratch directory {}",
            retained.display()
        )));
    }
    let cleanup = root
        .close()
        .map_err(|error| context("scratch cleanup", error));
    match (result, cleanup) {
        (Ok(report), Ok(())) => Ok(report),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => Err(io::Error::other(format!(
            "{error}; scratch cleanup failed: {cleanup}"
        ))),
    }
}

fn main() {
    match run() {
        Ok(report) => println!("{report}"),
        Err(error) => {
            println!(
                "{}",
                json!({"ok":false,"error":error.to_string(),"os":std::env::consts::OS,"arch":std::env::consts::ARCH})
            );
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn closed_reply(bytes: &[u8]) -> io::Result<Value> {
        let (mut receiver, mut peer) = UnixStream::pair()?;
        peer.write_all(bytes)?;
        drop(peer);
        // Setting O_NONBLOCK succeeds after close; setting SO_RCVTIMEO may
        // fail with EINVAL on Darwin even when unread reply bytes remain.
        receiver.set_nonblocking(true)?;
        read_reply(&mut receiver, Instant::now() + CALL_LIMIT)
    }

    fn framed(value: &Value) -> Vec<u8> {
        let mut bytes = serde_json::to_vec(value).unwrap();
        bytes.push(b'\n');
        bytes
    }

    #[test]
    fn a_buffered_reply_larger_than_one_read_survives_peer_close() {
        let expected = json!({"ok":true,"padding":"x".repeat(4096)});
        let bytes = framed(&expected);
        assert!(bytes.len() > 1024);
        assert_eq!(closed_reply(&bytes).unwrap(), expected);
    }

    #[test]
    fn a_reply_at_the_byte_limit_is_accepted() {
        let overhead = serde_json::to_vec(&json!({"ok":true,"padding":""}))
            .unwrap()
            .len();
        let expected = json!({"ok":true,"padding":"x".repeat(MAX_REPLY_BYTES - overhead)});
        let bytes = framed(&expected);
        assert_eq!(bytes.len(), MAX_REPLY_BYTES + 1);
        let (mut receiver, mut peer) = UnixStream::pair().unwrap();
        receiver.set_nonblocking(true).unwrap();
        peer.set_write_timeout(Some(CALL_LIMIT)).unwrap();
        let writer = thread::spawn(move || peer.write_all(&bytes));
        let result = read_reply(&mut receiver, Instant::now() + CALL_LIMIT);
        writer.join().unwrap().unwrap();
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn a_fragmented_reply_waits_for_the_same_complete_frame() {
        let (mut receiver, mut peer) = UnixStream::pair().unwrap();
        receiver.set_nonblocking(true).unwrap();
        let writer = thread::spawn(move || -> io::Result<()> {
            for part in [b"{\"ok\":".as_slice(), b"true,\"value\":", b"23}\n"] {
                peer.write_all(part)?;
                thread::sleep(Duration::from_millis(10));
            }
            Ok(())
        });
        let result = read_reply(&mut receiver, Instant::now() + CALL_LIMIT);
        writer.join().unwrap().unwrap();
        assert_eq!(result.unwrap(), json!({"ok":true,"value":23}));
    }

    #[test]
    fn incoming_fragments_do_not_extend_the_absolute_read_deadline() {
        let (mut receiver, mut peer) = UnixStream::pair().unwrap();
        receiver.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let writer_stop = stop.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        let writer = thread::spawn(move || -> io::Result<usize> {
            peer.write_all(b"{\"ok\":true,\"padding\":\"")?;
            ready_tx.send(()).unwrap();
            let mut written = 0;
            while !writer_stop.load(Ordering::Acquire) {
                peer.write_all(b"x")?;
                written += 1;
                thread::sleep(Duration::from_millis(10));
            }
            Ok(written)
        });
        ready_rx.recv_timeout(CALL_LIMIT).unwrap();
        let began = Instant::now();
        let result = read_reply(&mut receiver, began + Duration::from_millis(150));
        let elapsed = began.elapsed();
        stop.store(true, Ordering::Release);
        let fragments = writer.join().unwrap().unwrap();
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert!(fragments > 0);
        assert!(
            elapsed < CALL_LIMIT,
            "cumulative deadline was extended: {elapsed:?}"
        );
    }

    #[test]
    fn malformed_json_is_rejected_after_complete_framing() {
        let error = closed_reply(b"{malformed}\n").unwrap_err();
        assert!(error.to_string().contains("IPC parse reply"));
    }

    #[test]
    fn an_oversized_reply_is_rejected_without_waiting_for_eof() {
        let (mut receiver, mut peer) = UnixStream::pair().unwrap();
        receiver.set_nonblocking(true).unwrap();
        peer.set_write_timeout(Some(CALL_LIMIT)).unwrap();
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
        let writer = thread::spawn(move || -> io::Result<()> {
            peer.write_all(&vec![b' '; MAX_REPLY_BYTES + 2])?;
            // Keep the peer open until the reader has rejected the byte count.
            release_rx.recv_timeout(CALL_LIMIT).unwrap();
            Ok(())
        });
        let result = read_reply(&mut receiver, Instant::now() + CALL_LIMIT);
        release_tx.send(()).unwrap();
        writer.join().unwrap().unwrap();
        let error = result.unwrap_err();
        assert!(error.to_string().contains("reply exceeds protocol limit"));
    }

    #[test]
    fn eof_without_newline_does_not_complete_a_valid_json_value() {
        let error = closed_reply(b"{\"ok\":true}").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn bytes_after_the_reply_frame_are_rejected() {
        let error = closed_reply(b"{\"ok\":true}\nextra").unwrap_err();
        assert!(error.to_string().contains("data after its reply"));
    }

    #[test]
    fn nonblocking_partial_writes_deliver_every_byte() {
        let (mut sender, mut peer) = UnixStream::pair().unwrap();
        sender.set_nonblocking(true).unwrap();
        peer.set_read_timeout(Some(CALL_LIMIT)).unwrap();
        let expected = vec![0x5a; 2 * 1024 * 1024];
        // The peer does not read until the first partial write is confirmed.
        let first = sender.write(&expected).unwrap();
        assert!(first > 0 && first < expected.len());
        let length = expected.len();
        let reader = thread::spawn(move || -> io::Result<Vec<u8>> {
            let mut received = vec![0; length];
            peer.read_exact(&mut received)?;
            Ok(received)
        });
        let result = write_bytes(&mut sender, &expected[first..], Instant::now() + CALL_LIMIT);
        drop(sender);
        let received = reader.join().unwrap().unwrap();
        result.unwrap();
        assert_eq!(received, expected);
    }

    #[test]
    fn an_unread_peer_cannot_extend_the_absolute_write_deadline() {
        let (mut sender, peer) = UnixStream::pair().unwrap();
        sender.set_nonblocking(true).unwrap();
        let bytes = vec![0x5a; 2 * 1024 * 1024];
        let began = Instant::now();
        let result = write_bytes(&mut sender, &bytes, began + Duration::from_millis(100));
        let elapsed = began.elapsed();
        drop(peer);
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert!(
            elapsed < CALL_LIMIT,
            "write deadline was extended: {elapsed:?}"
        );
    }
}
