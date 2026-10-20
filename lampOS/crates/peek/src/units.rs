//! The lamp's units for `/status`: each one's state, the build it runs
//! against the build its unit starts, and its last error line; and the
//! voice's end-of-speech worker, from the newest session's records.
use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::Command,
};

/// What `systemctl show` is asked about each unit.
const PROPERTIES: &str = "--property=Id,ActiveState,SubState,ExecStart,MainPID";
/// The longest error line shown.
const MOST_ERROR_CHARS: usize = 200;

/// One unit as the page shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UnitRow {
    pub id: String,
    pub active: String,
    pub sub: String,
    /// The build the unit starts: the hash its program's folder is named
    /// by, once `current` and other links are followed.
    pub release: Option<String>,
    /// The build its main process runs, from `/proc/<pid>/exe`.
    pub running: Option<String>,
    pub last_error: Option<String>,
}

/// The build a program's path names: its folder, when that is a hash, as
/// the lamp installs them (`/opt/lampOS/body/<sha256>/lamp-body`).
pub fn build_of(program: &Path) -> Option<String> {
    let folder = program.parent()?.file_name()?.to_str()?;
    (folder.len() >= 12 && folder.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| folder.to_owned())
}

/// The rows of `systemctl show`'s records (`Key=value`, a blank line
/// between two units). `resolve` follows a program's links, `exe_of` reads
/// a process's program, and `last_error` a unit's last error line.
pub fn rows(
    show: &str,
    resolve: &dyn Fn(&Path) -> Option<PathBuf>,
    exe_of: &dyn Fn(u32) -> Option<PathBuf>,
    last_error: &dyn Fn(&str) -> Option<String>,
) -> Vec<UnitRow> {
    let mut rows = Vec::new();
    for record in show.split("\n\n") {
        let mut row = UnitRow::default();
        let mut pid = 0;
        for line in record.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key {
                "Id" => row.id = value.to_owned(),
                "ActiveState" => row.active = value.to_owned(),
                "SubState" => row.sub = value.to_owned(),
                "MainPID" => pid = value.parse().unwrap_or(0),
                "ExecStart" => {
                    row.release = program_of(value)
                        .and_then(|program| resolve(Path::new(program)))
                        .and_then(|program| build_of(&program));
                }
                _ => {}
            }
        }
        if row.id.is_empty() {
            continue;
        }
        if pid > 0 {
            row.running = exe_of(pid).and_then(|program| build_of(&program));
        }
        row.last_error = last_error(&row.id);
        rows.push(row);
    }
    rows
}

/// `{ path=/opt/lampOS/body/current/lamp-body ; argv[]=... }` to its path.
fn program_of(exec_start: &str) -> Option<&str> {
    exec_start.split("path=").nth(1)?.split([' ', ';']).next()
}

/// The units on this lamp, from systemd: the keeper's own patterns.
pub fn read() -> Vec<UnitRow> {
    let Ok(output) = Command::new("systemctl")
        .arg("show")
        .arg(PROPERTIES)
        .args(lamp_keeper::units::PATTERNS)
        .output()
    else {
        return Vec::new();
    };
    rows(
        &String::from_utf8_lossy(&output.stdout),
        &|program| fs::canonicalize(program).ok(),
        &|pid| {
            // A program replaced since it started reads "<path> (deleted)":
            // its folder still names the build it runs.
            let link = fs::read_link(format!("/proc/{pid}/exe")).ok()?;
            let text = link.to_string_lossy();
            Some(PathBuf::from(text.trim_end_matches(" (deleted)")))
        },
        &journal_error,
    )
}

/// The unit's last line at priority error or worse, cut short.
fn journal_error(unit: &str) -> Option<String> {
    let output = Command::new("journalctl")
        .args([
            "--no-pager",
            "--quiet",
            "--output=cat",
            "--priority=err",
            "--lines=1",
            "--unit",
            unit,
        ])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().last()?.trim();
    (!line.is_empty()).then(|| line.chars().take(MOST_ERROR_CHARS).collect())
}

/// The end-of-speech worker's newest state in the newest session under
/// `sessions` (`<session>/metadata/events-*.jsonl`), and when it was set on
/// the session's clock: only `endpoint_worker` records are read.
pub fn endpoint_worker(sessions: &Path) -> Option<(String, u64)> {
    let newest = fs::read_dir(sessions)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .max_by_key(|entry| entry.metadata().and_then(|meta| meta.modified()).ok())?
        .path();
    let mut found: Option<(String, u64)> = None;
    for entry in fs::read_dir(newest.join("metadata"))
        .ok()?
        .filter_map(Result::ok)
    {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let events = name.starts_with("events-")
            && name.ends_with(".jsonl")
            && entry.file_type().is_ok_and(|kind| kind.is_file());
        if !events {
            continue;
        }
        let Ok(file) = fs::File::open(entry.path()) else {
            continue;
        };
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            if !line.contains("\"endpoint_worker\"") {
                continue;
            }
            let Ok(record) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if record["kind"] != "endpoint_worker" {
                continue;
            }
            let (Some(at), Some(state)) = (
                record["at_us"].as_u64(),
                record["availability"]["state"].as_str(),
            ) else {
                continue;
            };
            if found.as_ref().is_none_or(|(_, newest)| at >= *newest) {
                found = Some((state.to_owned(), at));
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHOW: &str = "Id=lamp-body.service\nActiveState=active\nSubState=running\n\
        ExecStart={ path=/opt/lampOS/body/current/lamp-body ; argv[]=/opt/lampOS/body/current/lamp-body ; }\n\
        MainPID=41\n\n\
        Id=lamp-eyes.service\nActiveState=active\nSubState=running\n\
        ExecStart={ path=/opt/lampOS/eyes/current/lamp-eyes ; argv[]=x ; }\nMainPID=42\n\n\
        Id=lampos.target\nActiveState=active\nSubState=active\nMainPID=0\n\n\
        Id=lamp-name.service\nActiveState=failed\nSubState=failed\n\
        ExecStart={ path=/opt/lampOS/name/current/lamp-name ; }\nMainPID=0\n";
    const OLD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const NEW: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn a_unit_running_another_build_than_its_release_shows_both() {
        let rows = rows(
            SHOW,
            &|program| {
                let part = program.parent()?.parent()?.file_name()?.to_str()?;
                Some(PathBuf::from(format!(
                    "/opt/lampOS/{part}/{NEW}/lamp-{part}"
                )))
            },
            // The body still runs the build before: the eyes the new one.
            &|pid| match pid {
                41 => Some(PathBuf::from(format!("/opt/lampOS/body/{OLD}/lamp-body"))),
                42 => Some(PathBuf::from(format!("/opt/lampOS/eyes/{NEW}/lamp-eyes"))),
                _ => None,
            },
            &|unit| match unit {
                "lamp-name.service" => Some("refused --stop-least".to_owned()),
                "lamp-eyes.service" => Some("<script>alert(1)</script>".to_owned()),
                _ => None,
            },
        );
        let ids: Vec<_> = rows.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "lamp-body.service",
                "lamp-eyes.service",
                "lampos.target",
                "lamp-name.service"
            ]
        );
        assert_eq!(rows[0].running.as_deref(), Some(OLD));
        assert_eq!(rows[0].release.as_deref(), Some(NEW));
        assert_eq!(rows[1].running, rows[1].release);
        assert_eq!(
            (rows[2].release.as_ref(), rows[2].running.as_ref()),
            (None, None)
        );
        assert_eq!(rows[3].running, None, "no main process");
        assert_eq!(rows[3].last_error.as_deref(), Some("refused --stop-least"));
        let page = crate::view::status_page(&rows, None, std::time::Duration::ZERO);
        assert_eq!(page.matches("class=\"differs\"").count(), 1, "{page}");
        // An error line is text from outside the page: shown, never run
        // (the reviewer on #2150, P3 2).
        assert!(
            page.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
            "{page}"
        );
        assert!(!page.contains("<script>"), "{page}");
    }

    #[test]
    fn a_folder_that_is_no_hash_names_no_build() {
        assert_eq!(
            build_of(Path::new("/opt/lampOS/body/current/lamp-body")),
            None
        );
        assert_eq!(build_of(Path::new("/usr/bin/lamp-body")), None);
        assert_eq!(
            build_of(Path::new(&format!("/opt/lampOS/releases/{NEW}/lamp-body"))).as_deref(),
            Some(NEW)
        );
    }

    #[test]
    fn the_worker_state_is_the_newest_of_the_newest_session_and_nothing_else_is_read() {
        let root = std::env::temp_dir().join(format!("peek-sessions-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let old = root.join("s1/metadata");
        let new = root.join("s2/metadata");
        fs::create_dir_all(&old).unwrap();
        fs::write(
            old.join("events-0.jsonl"),
            "{\"kind\":\"endpoint_worker\",\"at_us\":9,\"availability\":{\"state\":\"failed\"}}\n",
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::create_dir_all(&new).unwrap();
        fs::write(
            new.join("events-0.jsonl"),
            "{\"kind\":\"endpoint_worker\",\"at_us\":30,\"availability\":{\"state\":\"ready\"}}\n\
             {\"kind\":\"heard\",\"at_us\":40,\"text\":\"endpoint_worker\"}\n\
             not json\n",
        )
        .unwrap();
        fs::write(
            new.join("events-1.jsonl"),
            "{\"kind\":\"endpoint_worker\",\"at_us\":10,\"availability\":{\"state\":\"loading\"}}\n",
        )
        .unwrap();
        // The newest session is the one made last, s2: its newest state is
        // `ready`, in the earlier of its two files.
        assert_eq!(endpoint_worker(&root), Some(("ready".to_owned(), 30)));
        assert_eq!(endpoint_worker(&root.join("none")), None);
        fs::remove_dir_all(root).unwrap();
    }
}
