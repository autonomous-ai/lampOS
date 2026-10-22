//! A libtest-compatible entrypoint that runs before creating any threads.
//! The cut checker forks a copy of a prepared fixture, so it cannot execute
//! inside libtest's worker threads. Its one named case is visible to nextest.
use std::process::ExitCode;

// Existing round fixtures include helpers used by other ordinary tests.
// This binary needs their shared setup, not every imported helper.
#[cfg(target_os = "linux")]
#[allow(dead_code, unused_imports)]
#[path = "round.rs"]
mod fixture;

const NAME: &str = "persisted_self_switches_survive_sigkill";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut list = false;
    let mut ignored = false;
    let mut exact = false;
    let mut filters = Vec::new();
    let mut skip = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let argument = &args[index];
        match argument.as_str() {
            "--list" => list = true,
            "--ignored" => ignored = true,
            "--exact" => exact = true,
            "--nocapture" | "--include-ignored" | "--show-output" | "--quiet" | "-q" => {}
            "--format" | "--color" | "--test-threads" => {
                index += 1;
                if args.get(index).is_none() {
                    eprintln!("{argument} needs a value");
                    return ExitCode::from(2);
                }
            }
            "--skip" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    eprintln!("--skip needs a value");
                    return ExitCode::from(2);
                };
                skip.push(value.as_str());
            }
            "--help" | "-h" => {
                println!("{NAME}: Linux-only single-threaded syscall cut check");
                return ExitCode::SUCCESS;
            }
            option
                if option.starts_with("--test-threads=")
                    || option.starts_with("--color=")
                    || option.starts_with("--format=") => {}
            option if option.starts_with('-') => {
                eprintln!("unsupported test argument: {option}");
                return ExitCode::from(2);
            }
            filter => filters.push(filter),
        }
        index += 1;
    }
    // nextest asks for the ignored list separately. On Mac this case must
    // appear there, rather than running a skip and being counted as a pass.
    let selected = (!ignored || cfg!(not(target_os = "linux")))
        && (filters.is_empty()
            || filters.iter().any(|filter| {
                if exact {
                    NAME == *filter
                } else {
                    NAME.contains(*filter)
                }
            }))
        && !skip.iter().any(|filter| NAME.contains(*filter));
    if list {
        if selected {
            println!("{NAME}: test");
        }
        return ExitCode::SUCCESS;
    }
    if !selected {
        println!(
            "running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out"
        );
        return ExitCode::SUCCESS;
    }
    #[cfg(not(target_os = "linux"))]
    {
        println!(
            "running 1 test\ntest {NAME} ... ignored, requires Linux syscall tracing\n\ntest result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out"
        );
        ExitCode::SUCCESS
    }
    #[cfg(target_os = "linux")]
    {
        println!("running 1 test");
        match std::panic::catch_unwind(fixture::process_cuts::run) {
            Ok(()) => {
                println!(
                    "test {NAME} ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out"
                );
                ExitCode::SUCCESS
            }
            Err(_) => {
                println!(
                    "test {NAME} ... FAILED\n\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out"
                );
                ExitCode::from(101)
            }
        }
    }
}
