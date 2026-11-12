//! Kit 5754 helper: skips lamp-skills' own clock through its test socket
//! (`--clock-control PATH`, docs/skills.md "A test's clock"). Each line
//! `{"skip_us": N}` moves the program's waits N us ahead and is answered
//! `{"now_us": ...}` or `{"error": ...}`.
//!
//!   kit5754-skip SOCKET TOTAL_S STEP_S PAUSE_MS [STOPFILE]
//!
//! Sends TOTAL_S / STEP_S skips of STEP_S seconds, PAUSE_MS of real time
//! between them, so every message published meanwhile is taken in on the
//! clock as it was before the next skip. Prints one line per skip and a
//! last line `skipped S s in N skips` (exit 0) or `skip refused: ...`
//! (exit 1). Stops early, exit 3, when STOPFILE exists.
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 5 {
        eprintln!("usage: kit5754-skip SOCKET TOTAL_S STEP_S PAUSE_MS [STOPFILE]");
        std::process::exit(2);
    }
    let parse = |i: usize| -> u64 {
        args[i].parse().unwrap_or_else(|_| {
            eprintln!("not a number: {}", args[i]);
            std::process::exit(2)
        })
    };
    let (total, step, pause) = (parse(2), parse(3), parse(4));
    if step == 0 || step > 600 || total > 24 * 3600 {
        eprintln!("STEP_S is 1..600 and TOTAL_S at most a day");
        std::process::exit(2);
    }
    let stop = args.get(5).map(Path::new);
    let stream = UnixStream::connect(&args[1]).unwrap_or_else(|error| {
        println!("skip refused: cannot connect to {}: {error}", args[1]);
        std::process::exit(1)
    });
    stream.set_read_timeout(Some(Duration::from_secs(30))).ok();
    let mut reader = BufReader::new(stream.try_clone().expect("clone"));
    let mut writer = stream;
    let (mut done, mut n) = (0u64, 0u64);
    while done < total {
        if stop.is_some_and(Path::exists) {
            println!("stopped by the stop file after {done} s in {n} skips");
            std::process::exit(3);
        }
        let by = step.min(total - done);
        if writeln!(writer, "{{\"skip_us\": {}}}", by * 1_000_000).is_err() {
            println!("skip refused: the socket closed after {done} s");
            std::process::exit(1);
        }
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(read) if read > 0 && line.contains("\"now_us\"") => {
                done += by;
                n += 1;
                println!("skip {n}: +{by} s, total {done} s, answer {}", line.trim());
            }
            Ok(_) | Err(_) => {
                println!("skip refused after {done} s: {}", line.trim());
                std::process::exit(1);
            }
        }
        std::thread::sleep(Duration::from_millis(pause));
    }
    println!("skipped {done} s in {n} skips");
}
