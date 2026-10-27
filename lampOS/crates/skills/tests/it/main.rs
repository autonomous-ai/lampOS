//! `lamp-skills`'s integration tests, one module a file, built as one binary so
//! the crate is linked once for them, not once a file (#1806).
mod addon;
mod call_wait;
mod daily_engine;
mod format;
mod journal_output;
mod made;
mod night_sleep;
mod package;
mod routine_service;
mod skills;
mod store;
mod welcome_stuck;

/// The daily journal at `path`, opened as the next process would. A test
/// that drops a journal and opens its folder's again, as a restart does, can
/// find the folder's lock still held for a moment: a child another test in
/// this binary is starting keeps a copy of every open file until its exec,
/// the locked folder's too (#1569 row 59, as rows 27 and 57). So "already
/// owned" is tried again for up to 5 s. The product opens its journal once a
/// process, and a second owner's refusal is `src/daily.rs`'s own tests'.
pub fn journal(path: impl AsRef<std::path::Path>) -> lamp_skills::daily::Journal {
    let path = path.as_ref();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match lamp_skills::daily::Journal::open(path) {
            Ok(journal) => return journal,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    && std::time::Instant::now() < until =>
            {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(error) => panic!("{}: {error}", path.display()),
        }
    }
}
