//! After a stable promotion: the notes' "Built but not on yet" part, and the
//! notes kept on the build's own release (`lampos_promote::notes`). Never a
//! gate on the promotion, which has already stood.
use std::{path::Path, process::ExitCode};
fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    let sequence = match arguments.as_slice() {
        [flag, value] if flag == "--sequence" => value.parse::<u64>().ok().filter(|s| *s > 0),
        _ => None,
    };
    let Some(sequence) = sequence else {
        eprintln!("usage: lampos-notes --sequence POSITIVE_INTEGER");
        return ExitCode::from(2);
    };
    let scratch = std::env::var_os("RUNNER_TEMP").map_or_else(std::env::temp_dir, Into::into);
    match lampos_promote::notes::fill(Path::new("lampOS"), sequence, &scratch) {
        Ok(done) => {
            println!("{done}");
            ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("the stable notes' built-but-not-on part: {why}");
            ExitCode::FAILURE
        }
    }
}
