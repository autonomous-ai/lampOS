//! `lamp-seems-measure READS LABELS [--delete]` (#1671, step 0): how often
//! the face read was wrong, counted from `lamp-eyes --seems-log`'s reads and
//! a labelled session's notes, one line a label, `{"t": unix_s, "says":
//! "positive"|"negative"|"neutral"}`. Prints, for each reader and each
//! confidence, the reads judged, the wrong ones, the rate and whether it is
//! under the bar (wrong at most 1 in 10). With `--delete`, both files are
//! removed afterwards, as the measure asks (the first builder, 14:04 and
//! 16:16 on #1671): stop lamp-eyes, or take `--seems-log` off it, first, or
//! it goes on writing to the removed file. Under 20 reads judged say "too
//! few", not whether the bar is met. It reads and prints numbers only.
use lamp_eyes::seems_log::{measure, table};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (reads, labels, delete) = match args.as_slice() {
        [reads, labels] => (reads, labels, false),
        [reads, labels, flag] if flag == "--delete" => (reads, labels, true),
        _ => {
            eprintln!("usage: lamp-seems-measure READS LABELS [--delete]");
            return ExitCode::from(2);
        }
    };
    let text = |path: &str| {
        std::fs::read_to_string(path).map_err(|error| format!("{path} can't be read: {error}"))
    };
    let rows = match text(reads)
        .and_then(|reads| text(labels).map(|labels| (reads, labels)))
        .and_then(|(reads, labels)| measure(&reads, &labels))
    {
        Ok(rows) => rows,
        Err(error) => {
            eprintln!("lamp-seems-measure: {error}");
            return ExitCode::from(1);
        }
    };
    print!("{}", table(&rows));
    if delete {
        for path in [reads, labels] {
            if let Err(error) = std::fs::remove_file(path) {
                eprintln!("lamp-seems-measure: {path} not removed: {error}");
                return ExitCode::from(1);
            }
        }
        println!("both files removed");
    }
    ExitCode::SUCCESS
}
