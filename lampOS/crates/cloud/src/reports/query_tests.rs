use super::*;
use crate::reports as public_reports;
use std::fs;
use std::path::Path;

fn shell(script: &str) -> Command {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", script]);
    command
}

fn query(script: &str) -> io::Result<Vec<u8>> {
    output_within(&mut shell(script), Instant::now() + Duration::from_secs(3))
}

#[test]
fn query_reads_output_with_closed_stdin_and_discarded_stderr() {
    assert_eq!(
        query("if read line; then exit 1; fi; printf active; printf hidden >&2").unwrap(),
        b"active"
    );
}

#[test]
fn query_refuses_nonzero_exit_and_spawn_failure() {
    let error = query("printf partial; exit 7").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Other);
    assert!(error.to_string().contains("exit status: 7"), "{error}");
    let root = tempfile::tempdir().unwrap();
    assert_eq!(
        output_within(
            &mut Command::new(root.path().join("missing")),
            Instant::now() + Duration::from_secs(3),
        )
        .unwrap_err()
        .kind(),
        io::ErrorKind::NotFound
    );
}

#[test]
fn public_output_accepts_256_kib_and_refuses_one_byte_more() {
    // Pin the public contract independently of the implementation constant.
    let full = "head -c 262144 /dev/zero | tr '\\000' x";
    let accepted = public_reports::bounded_output_within(
        Path::new("/bin/sh"),
        &["-c", full],
        Duration::from_secs(20),
    )
    .unwrap();
    assert_eq!(accepted.len(), 262_144);
    assert!(accepted.bytes().all(|byte| byte == b'x'));
    assert!(
        public_reports::bounded_output_within(
            Path::new("/bin/sh"),
            &["-c", &format!("{full}; printf x")],
            Duration::from_secs(20),
        )
        .is_none(),
        "public reader accepted output beyond 256 KiB"
    );
}

#[test]
fn public_output_refuses_invalid_utf8_instead_of_a_partial_string() {
    assert_eq!(
        public_reports::bounded_output_within(
            Path::new("/bin/sh"),
            &["-c", "printf 'valid'; printf '\\377'"],
            Duration::from_secs(3),
        ),
        None
    );
}

#[test]
fn query_deadline_applies_after_stdout_closes() {
    let root = tempfile::tempdir().unwrap();
    let ready = root.path().join("stdout-closed");
    let error = output_within(
        shell("exec 1>&-; printf ready > \"$1\"; exec /bin/sleep 30")
            .arg("closed-output")
            .arg(&ready),
        Instant::now() + Duration::from_secs(2),
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(
        ready.is_file(),
        "deadline control must reach stdout closure"
    );
}

#[test]
fn query_timeout_cleans_the_exited_leaders_pipe_holding_descendant() {
    let root = tempfile::tempdir().unwrap();
    let ready = root.path().join("ready");
    let escaped = root.path().join("escaped");
    let result = public_reports::bounded_output_within(
        Path::new("/bin/sh"),
        &[
            "-c",
            "(printf ready > \"$1\"; /bin/sleep 3; printf escaped > \"$2\") & exit 0",
            "pipe-holder",
            ready.to_str().unwrap(),
            escaped.to_str().unwrap(),
        ],
        Duration::from_secs(2),
    );
    assert!(
        result.is_none(),
        "public reader accepted a pipe held past its deadline"
    );
    std::thread::sleep(Duration::from_millis(1500));
    assert!(
        ready.is_file(),
        "descendant must reach the pipe-holding control"
    );
    assert!(!escaped.exists(), "descendant survived owned-group cleanup");
}

#[test]
fn a_failed_query_cleans_its_same_group_child_even_after_output_closes() {
    let root = tempfile::tempdir().unwrap();
    let ready = root.path().join("closed-output-ready");
    let escaped = root.path().join("escaped");
    let release = root.path().join("release");
    let error = output_within(
        shell(
            "(exec 1>&- 2>&-; printf ready > \"$1\"; \
             attempts=0; \
             while [ ! -f \"$3\" ] && [ \"$attempts\" -lt 250 ]; do \
               /bin/sleep 0.02; attempts=$((attempts+1)); \
             done; \
             [ ! -f \"$3\" ] || printf escaped > \"$2\") & \
             attempts=0; \
             while [ ! -f \"$1\" ] && [ \"$attempts\" -lt 100 ]; do \
               /bin/sleep 0.02; attempts=$((attempts+1)); \
             done; \
             [ -f \"$1\" ] || exit 99; exit 7",
        )
        .arg("closed-output-descendant")
        .arg(&ready)
        .arg(&escaped)
        .arg(&release),
        Instant::now() + Duration::from_secs(4),
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Other);
    assert!(
        error.to_string().contains("exit status: 7"),
        "expected the witnessed leader failure, not setup failure: {error}"
    );
    assert!(ready.is_file(), "child must close output before exit 7");
    // Release only after the query's failure, never before cleanup is required.
    fs::write(&release, b"go").unwrap();
    // This is a bounded observation, not a scheduler-independent death proof.
    // The old-bug child exits naturally; never signal a possibly reaped group.
    std::thread::sleep(Duration::from_millis(1500));
    assert!(
        !escaped.exists(),
        "failed query left its closed-output descendant running"
    );
}

#[test]
fn invalid_utf8_cleans_closed_output_child() {
    let root = tempfile::tempdir().unwrap();
    let ready = root.path().join("closed-output-ready");
    let escaped = root.path().join("escaped");
    let release = root.path().join("release");
    let error = output_within(
        shell(
            "(exec 1>&- 2>&-; printf ready > \"$1\"; \
             attempts=0; \
             while [ ! -f \"$3\" ] && [ \"$attempts\" -lt 250 ]; do \
               /bin/sleep 0.02; attempts=$((attempts+1)); \
             done; \
             [ ! -f \"$3\" ] || printf escaped > \"$2\") & \
             attempts=0; \
             while [ ! -f \"$1\" ] && [ \"$attempts\" -lt 100 ]; do \
               /bin/sleep 0.02; attempts=$((attempts+1)); \
             done; \
             [ -f \"$1\" ] || exit 99; printf '\\377'; exit 0",
        )
        .arg("invalid-output-descendant")
        .arg(&ready)
        .arg(&escaped)
        .arg(&release),
        Instant::now() + Duration::from_secs(4),
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(error.to_string(), "cloud query output is not UTF-8");
    assert!(
        ready.is_file(),
        "child must close output before invalid reply"
    );
    // Invalid text must fail before releasing leader ownership, even when
    // its exit status is zero, so the still-owned group is cleaned up.
    fs::write(&release, b"go").unwrap();
    // As above, this is a bounded observation; never signal a reaped group.
    std::thread::sleep(Duration::from_millis(1500));
    assert!(
        !escaped.exists(),
        "invalid query output left its closed-output descendant running"
    );
}

// Keeps writing 256-byte lines until killed. Its rate stays modest so a run
// with the cap removed fills memory slowly until the deadline instead.
const ENDLESS: &str = "while :; do printf '%0256d' 0; done";
const ENDLESS_DEADLINE: Duration = Duration::from_secs(6);

fn pid_in(path: &Path) -> String {
    let pid = fs::read_to_string(path).unwrap();
    assert!(
        !pid.is_empty() && pid.bytes().all(|byte| byte.is_ascii_digit()),
        "bad pid file {pid:?}"
    );
    pid
}

/// True while `kill -0` still finds `pid`. A killed orphan stays a zombie
/// until init reaps it, so wait a bounded time for it to be gone.
fn still_running(pid: &str) -> bool {
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        let alive = Command::new("/bin/sh")
            .args(["-c", "kill -0 \"$1\" 2>/dev/null", "probe", pid])
            .status()
            .unwrap()
            .success();
        if !alive || Instant::now() >= until {
            return alive;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn assert_refused_at_the_cap(error: io::Error, started: Instant) {
    let elapsed = started.elapsed();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData, "{error}");
    assert_eq!(error.to_string(), "cloud query output exceeds 256 KiB");
    // The cap is reached in well under a second; half the deadline is slack.
    assert!(
        elapsed < ENDLESS_DEADLINE / 2,
        "endless writer refused only after {elapsed:?}"
    );
}

#[test]
fn an_endless_writer_is_refused_at_the_cap_and_its_group_killed() {
    let root = tempfile::tempdir().unwrap();
    let leader = root.path().join("leader");
    let started = Instant::now();
    let error = output_within(
        shell(&format!("printf '%s' $$ > \"$1\"; {ENDLESS}"))
            .arg("endless-writer")
            .arg(&leader),
        started + ENDLESS_DEADLINE,
    )
    .unwrap_err();
    assert_refused_at_the_cap(error, started);
    let pid = pid_in(&leader);
    assert!(
        !still_running(&pid),
        "endless writer {pid} survived the cap"
    );
}

#[test]
fn an_endless_writer_whose_child_holds_the_pipe_is_refused_and_both_killed() {
    let root = tempfile::tempdir().unwrap();
    let leader = root.path().join("leader");
    let child = root.path().join("child");
    let started = Instant::now();
    let error = output_within(
        shell(&format!(
            "printf '%s' $$ > \"$1\"; \
             /bin/sh -c 'printf %s $$ > \"$1\"; exec /bin/sleep 30' pipe-holder \"$2\" & \
             attempts=0; \
             while [ ! -s \"$2\" ] && [ \"$attempts\" -lt 100 ]; do \
               /bin/sleep 0.02; attempts=$((attempts+1)); \
             done; \
             [ -s \"$2\" ] || exit 99; {ENDLESS}"
        ))
        .arg("endless-writer-with-child")
        .arg(&leader)
        .arg(&child),
        started + ENDLESS_DEADLINE,
    )
    .unwrap_err();
    assert_refused_at_the_cap(error, started);
    let leader = pid_in(&leader);
    let child = pid_in(&child);
    assert_ne!(leader, child);
    assert!(!still_running(&leader), "endless writer {leader} survived");
    assert!(
        !still_running(&child),
        "pipe-holding child {child} survived the cap"
    );
}
