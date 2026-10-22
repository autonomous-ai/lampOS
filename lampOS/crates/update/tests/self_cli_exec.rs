//! The real CLI exec check starts on one thread, outside libtest's workers.
//! Like self-process-cuts, its one named checker is discoverable by nextest.
use std::process::ExitCode;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "aarch64", target_arch = "x86_64")
))]
#[path = "self_cli_exec/linux.rs"]
mod linux;

const NAME: &str = "actual_cli_exec_confirms_before_the_next_round";
const SUPPORTED: bool = cfg!(all(
    target_os = "linux",
    any(target_arch = "aarch64", target_arch = "x86_64")
));

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mut list, mut ignored, mut exact) = (false, false, false);
    let (mut filters, mut skip) = (Vec::new(), Vec::new());
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
                println!("{NAME}: single-threaded Linux ARM64/x86-64 CLI exec check");
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
    let selected = (!ignored || !SUPPORTED)
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
    #[cfg(not(all(
        target_os = "linux",
        any(target_arch = "aarch64", target_arch = "x86_64")
    )))]
    {
        println!(
            "running 1 test\ntest {NAME} ... ignored, requires Linux ARM64/x86-64 ptrace\n\ntest result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out"
        );
        ExitCode::SUCCESS
    }
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "aarch64", target_arch = "x86_64")
    ))]
    {
        println!("running 1 test");
        match std::panic::catch_unwind(linux::run) {
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
