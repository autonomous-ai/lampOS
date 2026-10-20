//! `probe_handover PROVIDER_CONFIG SECOND_CONFIG [RUNS] [LABEL]`: which
//! function the second route's model calls for the played hand-over asks,
//! with two of the staff (`lamp_voice::second_provider::probe::handover`,
//! #1722). Run only when named, where the voice runs, with the voice unit's
//! environment. It prints counts only.
use std::{io, path::PathBuf};

fn main() -> io::Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [provider, second, rest @ ..] = arguments.as_slice() else {
        return Err(io::Error::other(
            "usage: probe_handover PROVIDER_CONFIG SECOND_CONFIG [RUNS] [LABEL]",
        ));
    };
    let runs = match rest.first() {
        Some(runs) => runs.parse().map_err(io::Error::other)?,
        None => 10,
    };
    let label = rest.get(1).map_or("this build", String::as_str);
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(lamp_voice::second_provider::probe::handover::run(
            &PathBuf::from(provider),
            &PathBuf::from(second),
            runs,
            label,
        ))
}
