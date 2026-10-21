//! How long the memory takes to answer, measured through its socket on
//! whatever machine runs this. Not run with the other tests, and nothing in
//! it passes or fails on a time: run it where the answer matters and read
//! what it prints.
//!
//! `cargo test -p lamp-memory --test it timing:: -- --ignored --nocapture --skip days_timing --skip health_timing --skip voice_timing`
use lamp_memory::{Options, Server, Store};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{fs::DirBuilderExt, net::UnixStream},
    path::Path,
    thread,
    time::{Duration, Instant},
};

/// 2026-10-13 12:00 UTC.
const NOON: u64 = 1_791_892_800_000;
const DAY_MS: u64 = 86_400_000;

/// Connect, ask, read the answer, close: what a client pays for a request.
fn ask(socket: &Path, request: &str) -> (Duration, usize) {
    let began = Instant::now();
    let mut stream = BufReader::new(UnixStream::connect(socket).unwrap());
    stream.get_mut().write_all(request.as_bytes()).unwrap();
    stream.get_mut().write_all(b"\n").unwrap();
    let mut answer = String::new();
    stream.read_line(&mut answer).unwrap();
    assert!(answer.starts_with("{\"ok\":true"), "{answer}");
    (began.elapsed(), answer.len())
}

fn report(what: &str, mut times: Vec<Duration>, bytes: usize) {
    times.sort();
    let ms = |time: Duration| time.as_secs_f64() * 1000.0;
    println!(
        "{what:<34} n={:<3} median {:>7.2} ms   slowest {:>7.2} ms   answer {bytes} bytes",
        times.len(),
        ms(times[times.len() / 2]),
        ms(times[times.len() - 1]),
    );
}

#[test]
#[ignore = "a measurement, not a check"]
fn how_long_each_request_takes_with_a_month_of_talk_behind_it() {
    let scratch = std::env::temp_dir().join(format!("lm-{}-t", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    let person = scratch.join("store/everyone");
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&person)
        .unwrap();
    // Thirty days of two hundred exchanges, about 330 bytes of text each,
    // written as the store writes them, and two hundred kept things.
    let (heard, said) = (
        "what did we say about the coffee order ".repeat(3),
        "we said ".repeat(26),
    );
    let mut id = 0u64;
    let mut bytes = 0;
    for day in 0..30u64 {
        let at = NOON - (29 - day) * DAY_MS;
        let mut journal = String::new();
        for n in 0..200u64 {
            id += 1;
            let line = serde_json::json!({"id":id,"at_unix_ms":at + n * 1000,"heard":format!("{heard}{n}"),"said":format!("{said}{n}")});
            journal.push_str(&format!("{line}\n"));
        }
        bytes += journal.len();
        // Thirty days ending on 2026-10-13: 14 to 30 September, then 1 to
        // 13 October.
        let name = format!(
            "journal-2026-{:02}-{:02}.jsonl",
            if day < 17 { 9 } else { 10 },
            if day < 17 { day + 14 } else { day - 16 }
        );
        fs::write(person.join(name), journal).unwrap();
    }
    let mut kept = String::new();
    for n in 0..200u64 {
        id += 1;
        let line = serde_json::json!({"id":id,"at_unix_ms":NOON,"text":format!("takes the coffee black on day {n}")});
        kept.push_str(&format!("{line}\n"));
    }
    fs::write(person.join("kept.jsonl"), kept).unwrap();
    println!("journal: 30 days, 6000 exchanges, {bytes} bytes; kept: 200");

    let socket = scratch.join("m.sock");
    let server = Server::bind(&socket).unwrap();
    let began = Instant::now();
    let options = Options {
        dir: scratch.join("store"),
        keep_days: 30,
        local: lamp_memory::store::LocalTime::Offset(0),
    };
    let mut store = Store::open(options, NOON).unwrap();
    report("opening the store", vec![began.elapsed()], 0);
    let stopper = server.stopper();
    let serving = thread::spawn(move || server.run(&mut store, &mut || NOON, &mut |_| {}));

    let measure = |what: &str, count: usize, request: &dyn Fn(usize) -> String| {
        let asked: Vec<(Duration, usize)> = (0..count).map(|n| ask(&socket, &request(n))).collect();
        let bytes = asked.last().unwrap().1;
        report(
            what,
            asked.into_iter().map(|(time, _)| time).collect(),
            bytes,
        );
    };
    measure("status (reads everything)", 5, &|_| {
        r#"{"op":"status"}"#.into()
    });
    measure("append", 50, &|n| {
        format!(
            r#"{{"op":"append","at_unix_ms":{},"heard":"{heard}{n}","said":"{said}{n}"}}"#,
            NOON + n as u64
        )
    });
    measure("remember (list full: rewritten)", 20, &|n| {
        format!(
            r#"{{"op":"remember","at_unix_ms":{NOON},"text":"the order is two bags, number {n}"}}"#
        )
    });
    measure("kept", 20, &|_| r#"{"op":"kept","max_bytes":4096}"#.into());
    measure("recall, newest", 20, &|_| r#"{"op":"recall"}"#.into());
    measure("recall, a day", 20, &|_| {
        r#"{"op":"recall","day":"2026-10-01","limit":50,"max_bytes":16384}"#.into()
    });
    measure("recall, words (reads everything)", 20, &|_| {
        r#"{"op":"recall","query":"coffee order bags","limit":50,"max_bytes":16384}"#.into()
    });
    measure("forget_last (reads everything)", 20, &|_| {
        r#"{"op":"forget_last"}"#.into()
    });

    stopper.stop();
    serving.join().unwrap().unwrap();
    fs::remove_dir_all(&scratch).unwrap();
}
