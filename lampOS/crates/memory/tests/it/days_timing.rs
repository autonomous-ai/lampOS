//! Comparable file-store and opt-in-store timings on the machine that runs
//! this. First request means a fresh process/store, not a cold filesystem cache.
//! Run with `cargo test -p lamp-memory --release --test it days_timing:: -- --ignored --nocapture`.
use lamp_memory::{Options, Server, Store, days::Days, server::Memory};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{fs::DirBuilderExt, net::UnixStream},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

const NOW: u64 = 1_791_892_800_000;
const DAY: u64 = 86_400_000;

fn fixture(root: &Path) -> Options {
    let person = root.join("store/everyone");
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&person)
        .unwrap();
    let heard = "what did we say about the coffee order ".repeat(3);
    let said = "we said ".repeat(26);
    let mut id = 0;
    for day in 0..30 {
        let at = NOW - (29 - day) * DAY;
        let mut lines = String::new();
        for n in 0..200 {
            id += 1;
            lines.push_str(&json!({"id":id,"at_unix_ms":at+n*1000,"heard":format!("{heard}{n}"),"said":format!("{said}{n}")}).to_string());
            lines.push('\n');
        }
        let name = format!(
            "journal-2026-{:02}-{:02}.jsonl",
            if day < 17 { 9 } else { 10 },
            if day < 17 { day + 14 } else { day - 16 }
        );
        fs::write(person.join(name), lines).unwrap();
    }
    let mut kept = String::new();
    for n in 0..200 {
        id += 1;
        kept.push_str(
            &json!({"id":id,"at_unix_ms":NOW,"text":format!("takes the coffee black on day {n}")})
                .to_string(),
        );
        kept.push('\n');
    }
    fs::write(person.join("kept.jsonl"), kept).unwrap();
    Options {
        dir: root.join("store"),
        keep_days: 30,
        local: lamp_memory::store::LocalTime::Offset(0),
    }
}

fn request(socket: &Path, body: &Value) -> (f64, Value, usize) {
    let began = Instant::now();
    let mut stream = BufReader::new(UnixStream::connect(socket).unwrap());
    stream
        .get_mut()
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    writeln!(stream.get_mut(), "{body}").unwrap();
    let mut line = String::new();
    stream.read_line(&mut line).unwrap();
    let elapsed = began.elapsed().as_secs_f64() * 1000.0;
    let response: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(response["ok"], true, "{response}");
    (elapsed, response, line.len())
}

fn measure(socket: &Path, backend: &str, op: &str, body: Value) {
    let (first, _, bytes) = request(socket, &body);
    let mut times: Vec<f64> = (0..50).map(|_| request(socket, &body).0).collect();
    times.sort_by(f64::total_cmp);
    println!(
        "{}",
        json!({"backend":backend,"op":op,"first_ms":first,"repeated":50,"median_ms":times[25],"p95_ms":times[47],"max_ms":times[49],"response_bytes":bytes})
    );
}

#[test]
#[ignore = "local measurement; does not qualify a lamp"]
fn file_and_ranked_reads_have_the_same_socket_boundary_and_corpus() {
    println!(
        "{}",
        json!({"corpus":"30 days, 6000 exchanges, 200 kept; same fixture as legacy timing","boundary":"connect, serialize/send request, read full newline-terminated response; excludes JSON decoding","first":"fresh store and first request; filesystem cache is not evicted","build":if cfg!(debug_assertions){"debug"}else{"release"},"os":std::env::consts::OS,"arch":std::env::consts::ARCH})
    );
    for ranked in [false, true] {
        let backend = if ranked { "days" } else { "files" };
        let root: PathBuf =
            std::env::temp_dir().join(format!("lm-dt-{}-{backend}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let options = fixture(&root);
        let socket = root.join("m.sock");
        let server = Server::bind(&socket).unwrap();
        let began = Instant::now();
        let mut store: Box<dyn Memory + Send> = if ranked {
            Box::new(Days::open(options, NOW).unwrap())
        } else {
            Box::new(Store::open(options, NOW).unwrap())
        };
        println!(
            "{}",
            json!({"backend":backend,"op":if ranked{"first_open_with_import"}else{"first_open"},"ms":began.elapsed().as_secs_f64()*1000.0,"runs":1})
        );
        let stop = server.stopper();
        let worker = thread::spawn(move || server.run(&mut *store, &mut || NOW, &mut |_| {}));
        measure(
            &socket,
            backend,
            "recall_words",
            json!({"op":"recall","query":"coffee order bags","limit":50,"max_bytes":16384}),
        );
        measure(&socket, backend, "recall_newest", json!({"op":"recall"}));
        measure(
            &socket,
            backend,
            "recall_day",
            json!({"op":"recall","day":"2026-10-01","limit":50,"max_bytes":16384}),
        );
        measure(
            &socket,
            backend,
            "kept",
            json!({"op":"kept","max_bytes":1200}),
        );
        if ranked {
            measure(
                &socket,
                backend,
                "search",
                json!({"op":"search","query":"coffee order bags","limit":50,"max_bytes":16384}),
            );
            measure(
                &socket,
                backend,
                "session_snapshot",
                json!({"op":"session_open","session":"bench","max_bytes":1200}),
            );
            let (_, prepared, _) = request(
                &socket,
                &json!({"op":"conversation_end","conversation":"bench","source_ids":[5801]}),
            );
            // A model worker has not answered this prepared job. Reads keep
            // using the service while that external dependency is pending.
            assert!(prepared["input"]["job"].is_string());
            measure(
                &socket,
                backend,
                "recall_while_model_pending",
                json!({"op":"recall","query":"coffee order bags","limit":50,"max_bytes":16384}),
            );
        }
        stop.stop();
        worker.join().unwrap().unwrap();
        if ranked {
            let began = Instant::now();
            let reopened = Days::open(
                Options {
                    dir: root.join("store"),
                    keep_days: 30,
                    local: lamp_memory::store::LocalTime::Offset(0),
                },
                NOW,
            )
            .unwrap();
            println!(
                "{}",
                json!({"backend":backend,"op":"reopen_after_import","ms":began.elapsed().as_secs_f64()*1000.0,"runs":1,"database_bytes":fs::metadata(root.join("store/memory-days.sqlite3")).unwrap().len()})
            );
            drop(reopened);
        }
        fs::remove_dir_all(&root).unwrap();
    }
}
