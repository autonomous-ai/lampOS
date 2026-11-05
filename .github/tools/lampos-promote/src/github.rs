//! GitHub CLI adapter. No shell or signing secret reaches child commands.
use crate::{
    Asset, HashSize, Job, REPO, Release, Remote, Repository, Result, WorkflowRun, require,
};
use ring::digest::{Context, SHA256};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::{
    fs,
    io::Read,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

/// The largest JSON answer read. The paginated release list grew past
/// 64 MiB on 5 Nov (every train publishes a pre-release with notes), which
/// blocked the promote of 10226; releases are now looked up by tag.
const MAX_JSON: u64 = 512 * 1024 * 1024;
const COMMAND_LIMIT: Duration = Duration::from_secs(600);
/// The most of the CLI's error text read, only to tell GitHub's 5xx: it is
/// never printed or kept.
const MOST_STDERR: u64 = 64 * 1024;
/// GitHub's own 500, 502, 503 or 504 (#4103: three 504s on 28 Oct) is
/// asked again after these pauses, for a read only. A change is never
/// repeated blind; a promotion that failed is run again whole.
const RETRY_PAUSES: [Duration; 3] = [
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(30),
];
const SERVER_FAILED: &str = "GitHub answered 5xx; remote state may need inspection";
/// A read of something GitHub does not have; never asked again.
const NOT_FOUND: &str = "GitHub answered 404";
/// Whether the CLI's error text says GitHub answered one of `statuses`.
fn answered(stderr: &[u8], statuses: &[&[u8; 8]]) -> bool {
    stderr.windows(8).enumerate().any(|(at, window)| {
        let digit_after = stderr.get(at + 8).is_some_and(u8::is_ascii_digit);
        let word_before = at > 0 && stderr[at - 1].is_ascii_alphanumeric();
        statuses.iter().any(|status| window == &status[..]) && !digit_after && !word_before
    })
}
/// Whether the CLI's error text says GitHub answered 500, 502, 503 or 504.
fn server_error(stderr: &[u8]) -> bool {
    answered(
        stderr,
        &[b"HTTP 500", b"HTTP 502", b"HTTP 503", b"HTTP 504"],
    )
}
/// Whether the CLI's error text says GitHub answered 404.
fn not_found(stderr: &[u8]) -> bool {
    answered(stderr, &[b"HTTP 404"])
}
/// A 404 as None; any other failure stays one.
fn found<T>(answer: Result<T>) -> Result<Option<T>> {
    match answer {
        Ok(value) => Ok(Some(value)),
        Err(why) if why == NOT_FOUND => Ok(None),
        Err(why) => Err(why),
    }
}
/// The one release named by `tag`; the whole list is never read.
fn release_endpoint(tag: &str) -> String {
    format!("releases/tags/{tag}")
}
/// `attempt` again after each of `pauses` while it fails with GitHub's 5xx.
fn retried<T>(
    pauses: &[Duration],
    mut sleep: impl FnMut(Duration),
    mut attempt: impl FnMut() -> Result<T>,
) -> Result<T> {
    for pause in pauses {
        match attempt() {
            Err(why) if why == SERVER_FAILED => {
                eprintln!("{why}; read again in {} s", pause.as_secs());
                sleep(*pause);
            }
            done => return done,
        }
    }
    attempt()
}
#[derive(Clone, Copy)]
enum Output {
    Bytes(u64),
    Hash(u64),
}
enum Answer {
    Bytes(Vec<u8>),
    Hash(HashSize),
}
fn consume(mut source: impl Read, output: Output) -> Result<Answer> {
    let most = match output {
        Output::Bytes(most) | Output::Hash(most) => most,
    };
    let mut bytes = Vec::new();
    let mut digest = Context::new(&SHA256);
    let mut size = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = source
            .read(&mut buffer)
            .map_err(|_| "cannot read command output")?;
        if read == 0 {
            break;
        }
        size = size
            .checked_add(read as u64)
            .ok_or("output size overflow")?;
        require(size <= most, "command output exceeds size bound")?;
        match output {
            Output::Bytes(_) => bytes.extend_from_slice(&buffer[..read]),
            Output::Hash(_) => digest.update(&buffer[..read]),
        }
    }
    Ok(match output {
        Output::Bytes(_) => Answer::Bytes(bytes),
        Output::Hash(_) => Answer::Hash(HashSize {
            size,
            sha256: lamp_update::manifest::hex(digest.finish().as_ref()),
        }),
    })
}
/// One `gh` call, read again on GitHub's 5xx when it is a GET.
fn command(arguments: &[String], output: Output) -> Result<Answer> {
    let read_only = arguments
        .windows(2)
        .any(|pair| pair[0] == "--method" && pair[1] == "GET");
    if read_only {
        retried(&RETRY_PAUSES, thread::sleep, || {
            command_once(arguments, output)
        })
    } else {
        command_once(arguments, output)
    }
}
fn command_once(arguments: &[String], output: Output) -> Result<Answer> {
    let mut child = Command::new("gh")
        .args(arguments)
        .env("GH_HOST", "github.com")
        .env_remove("LAMPOS_UPDATE_KEY")
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|_| "cannot start GitHub CLI")?;
    let stdout = child.stdout.take().ok_or("GitHub CLI has no output pipe")?;
    let mut stderr = child.stderr.take().ok_or("GitHub CLI has no error pipe")?;
    // Read whole so the CLI never blocks or gets EPIPE on it, kept only up
    // to its bound (Fourteen's P3 on #4121).
    let errors = thread::spawn(move || {
        let mut kept = Vec::new();
        let _ = Read::by_ref(&mut stderr)
            .take(MOST_STDERR)
            .read_to_end(&mut kept);
        let _ = std::io::copy(&mut stderr, &mut std::io::sink());
        kept
    });
    let (sender, receiver) = mpsc::sync_channel(1);
    let reader = thread::spawn(move || {
        let _ = sender.send(consume(stdout, output));
    });
    let until = Instant::now() + COMMAND_LIMIT;
    let mut answer = None;
    let result = loop {
        if answer.is_none() {
            match receiver.try_recv() {
                Ok(value) => answer = Some(value),
                Err(mpsc::TryRecvError::Empty) => (),
                Err(mpsc::TryRecvError::Disconnected) => {
                    break Err("GitHub output reader stopped".to_owned());
                }
            }
        }
        if answer.as_ref().is_some_and(std::result::Result::is_err) {
            break answer.take().expect("checked answer");
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    // Its error pipe closes as it exits; a moment to read it.
                    let read_by = Instant::now() + Duration::from_secs(2);
                    while !errors.is_finished() && Instant::now() < read_by {
                        thread::sleep(Duration::from_millis(10));
                    }
                    if !errors.is_finished() {
                        // Left to end with the pipe; said, not taken as a 5xx
                        // (Reviewer-two's P3 on #4121).
                        break Err("GitHub CLI failed and its error pipe stayed open".to_owned());
                    }
                    let said = errors.join().unwrap_or_default();
                    if server_error(&said) {
                        break Err(SERVER_FAILED.to_owned());
                    }
                    if not_found(&said) {
                        break Err(NOT_FOUND.to_owned());
                    }
                    break Err("GitHub CLI failed; remote state may need inspection".to_owned());
                }
                break answer.take().unwrap_or_else(|| {
                    receiver
                        .recv_timeout(Duration::from_secs(2))
                        .unwrap_or_else(|_| Err("GitHub output pipe did not close".to_owned()))
                });
            }
            Ok(None) => (),
            Err(_) => break Err("cannot wait for GitHub CLI".to_owned()),
        }
        if Instant::now() >= until {
            break Err("GitHub CLI exceeded 600 seconds".to_owned());
        }
        thread::sleep(Duration::from_millis(20));
    };
    // Kill and reap on errors, including an oversized response. A completed child
    // tolerates these calls. No child keeps writing after this operation returns.
    let _ = child.kill();
    let _ = child.wait();
    // The CLI owns stdout; killing it closes the pipe. Never wait for a rogue
    // descendant that inherited the pipe beyond the already bounded receive.
    if reader.is_finished() {
        let _ = reader.join();
    }
    result
}
fn words(items: &[&str]) -> Vec<String> {
    items.iter().map(|word| (*word).to_owned()).collect()
}
#[derive(Default)]
pub struct Github;
impl Github {
    fn api<T: DeserializeOwned>(&mut self, endpoint: &str, pages: bool) -> Result<T> {
        self.json(&Self::api_arguments(endpoint, pages))
    }
    fn api_arguments(endpoint: &str, pages: bool) -> Vec<String> {
        let mut args = words(&["api", "--method", "GET"]);
        if pages {
            args.extend(words(&["--paginate", "--slurp"]));
        }
        args.push(format!("repos/{REPO}/{endpoint}"));
        args
    }
    fn json<T: DeserializeOwned>(&mut self, args: &[String]) -> Result<T> {
        let Answer::Bytes(bytes) = command(args, Output::Bytes(MAX_JSON))? else {
            unreachable!()
        };
        serde_json::from_slice(&bytes).map_err(|_| "unreadable GitHub response".to_owned())
    }
    fn asset_arguments(asset: &Asset) -> Vec<String> {
        words(&[
            "api",
            "--method",
            "GET",
            &format!("repos/{REPO}/releases/assets/{}", asset.id),
            "-H",
            "Accept: application/octet-stream",
        ])
    }
    fn upload_file(
        tag: &str,
        name: &str,
        bytes: &[u8],
        create: Option<(&str, &str)>,
    ) -> Result<()> {
        let folder = tempfile::tempdir().map_err(|_| "cannot create upload staging directory")?;
        let asset = folder.path().join(name);
        fs::write(&asset, bytes).map_err(|_| "cannot stage envelope")?;
        let mut args = words(&["release"]);
        if let Some((sha, notes)) = create {
            let note_file = folder.path().join("notes.txt");
            fs::write(&note_file, notes).map_err(|_| "cannot stage release notes")?;
            args.extend(words(&[
                "create",
                tag,
                "--repo",
                REPO,
                "--target",
                sha,
                "--title",
                "lampOS stable",
                "--notes-file",
            ]));
            args.push(note_file.to_string_lossy().into_owned());
            args.extend(words(&["--prerelease", "--latest=false"]));
        } else {
            args.extend(words(&["upload", tag, "--repo", REPO]));
        }
        args.push(asset.to_string_lossy().into_owned());
        command(&args, Output::Bytes(MAX_JSON))?;
        Ok(())
    }
}
impl Remote for Github {
    fn repository(&mut self) -> Result<Repository> {
        self.json(&words(&[
            "api",
            "--method",
            "GET",
            &format!("repos/{REPO}"),
        ]))
    }
    fn release(&mut self, tag: &str) -> Result<Option<Release>> {
        require(super::plain(tag), "unsafe release tag")?;
        found(self.api(&release_endpoint(tag), false))
    }
    fn tag(&mut self, name: &str) -> Result<Option<String>> {
        let refs: Vec<Value> = self.api(&format!("git/matching-refs/tags/{name}"), false)?;
        require(
            refs.iter()
                .all(|entry| entry.get("ref").and_then(Value::as_str).is_some()),
            "uncertain tag inventory",
        )?;
        let expected = format!("refs/tags/{name}");
        let matches: Vec<_> = refs
            .into_iter()
            .filter(|v| v["ref"].as_str() == Some(&expected))
            .collect();
        require(matches.len() <= 1, "duplicate tag reference")?;
        let Some(reference) = matches.into_iter().next() else {
            return Ok(None);
        };
        let mut object = reference["object"].clone();
        for _ in 0..5 {
            let sha = object["sha"].as_str().ok_or("tag has no SHA")?;
            require(super::commit(sha), "tag has invalid SHA")?;
            match object["type"].as_str() {
                Some("commit") => return Ok(Some(sha.to_owned())),
                Some("tag") => {
                    let tag: Value = self.api(&format!("git/tags/{sha}"), false)?;
                    object = tag["object"].clone();
                }
                _ => return Err("tag does not name a commit".to_owned()),
            }
        }
        Err("too many annotated tag indirections".to_owned())
    }
    fn assets(&mut self, release: u64) -> Result<Vec<Asset>> {
        let pages: Vec<Vec<Asset>> =
            self.api(&format!("releases/{release}/assets?per_page=100"), true)?;
        Ok(pages.into_iter().flatten().collect())
    }
    fn read(&mut self, asset: &Asset, most: u64) -> Result<Vec<u8>> {
        let Answer::Bytes(bytes) = command(&Self::asset_arguments(asset), Output::Bytes(most))?
        else {
            unreachable!()
        };
        Ok(bytes)
    }
    fn hash(&mut self, asset: &Asset, most: u64) -> Result<HashSize> {
        let Answer::Hash(result) = command(&Self::asset_arguments(asset), Output::Hash(most))?
        else {
            unreachable!()
        };
        Ok(result)
    }
    fn run(&mut self, run: u64, attempt: u64) -> Result<WorkflowRun> {
        self.api(&format!("actions/runs/{run}/attempts/{attempt}"), false)
    }
    fn jobs(&mut self, run: u64, attempt: u64) -> Result<Vec<Job>> {
        #[derive(serde::Deserialize)]
        struct Page {
            jobs: Vec<Job>,
        }
        let pages: Vec<Page> = self.api(
            &format!("actions/runs/{run}/attempts/{attempt}/jobs?per_page=100"),
            true,
        )?;
        Ok(pages.into_iter().flat_map(|page| page.jobs).collect())
    }
    fn create(
        &mut self,
        tag: &str,
        sha: &str,
        notes: &str,
        name: &str,
        bytes: &[u8],
    ) -> Result<()> {
        Self::upload_file(tag, name, bytes, Some((sha, notes)))
    }
    fn upload(&mut self, tag: &str, name: &str, bytes: &[u8]) -> Result<()> {
        Self::upload_file(tag, name, bytes, None)
    }
    fn delete(&mut self, id: u64) -> Result<()> {
        command(
            &words(&[
                "api",
                "--method",
                "DELETE",
                &format!("repos/{REPO}/releases/assets/{id}"),
            ]),
            Output::Bytes(MAX_JSON),
        )?;
        Ok(())
    }
    fn rename(&mut self, id: u64, name: &str) -> Result<Asset> {
        self.json(&words(&[
            "api",
            "--method",
            "PATCH",
            &format!("repos/{REPO}/releases/assets/{id}"),
            "-f",
            &format!("name={name}"),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unrelated_release_notes_may_be_null_but_selected_notes_still_refuse() {
        let release: Release = serde_json::from_value(serde_json::json!({
            "id": 1, "tag_name": "unrelated", "target_commitish": "a".repeat(40),
            "draft": false, "prerelease": false, "body": null
        }))
        .unwrap();
        assert!(release.body.is_empty());
        assert!(crate::notes(&release, 10).is_err());
    }
    #[test]
    fn github_5xx_is_told_and_4xx_is_not() {
        for said in [
            &b"HTTP 504: We couldn't respond to your request in time"[..],
            b"gh: Server Error (HTTP 500)",
            b"x\nHTTP 502: Bad Gateway",
            b"HTTP 503",
        ] {
            assert!(server_error(said), "{}", String::from_utf8_lossy(said));
        }
        for said in [
            &b"HTTP 403: Resource not accessible by integration"[..],
            b"HTTP 404: Not Found",
            b"HTTP 5040",
            b"XHTTP 504",
            b"stream error: CANCEL",
            b"",
        ] {
            assert!(!server_error(said), "{}", String::from_utf8_lossy(said));
        }
    }
    #[test]
    fn a_404_is_told_and_read_as_absent() {
        assert!(not_found(b"gh: Not Found (HTTP 404)"));
        for said in [&b"HTTP 403"[..], b"HTTP 4040", b"XHTTP 404", b"HTTP 504"] {
            assert!(!not_found(said), "{}", String::from_utf8_lossy(said));
        }
        assert_eq!(found::<u8>(Err(NOT_FOUND.to_owned())), Ok(None));
        assert_eq!(found(Ok(7)), Ok(Some(7)));
        assert_eq!(
            found::<u8>(Err(SERVER_FAILED.to_owned())),
            Err(SERVER_FAILED.to_owned())
        );
        // A 404 is never asked again.
        let mut asked = 0;
        let answer: Result<u8> = retried(
            &[Duration::from_secs(1)],
            |_| {},
            || {
                asked += 1;
                Err(NOT_FOUND.to_owned())
            },
        );
        assert_eq!((answer, asked), (Err(NOT_FOUND.to_owned()), 1));
    }
    #[test]
    fn a_release_is_one_get_by_tag_and_never_the_list() {
        let args = Github::api_arguments(&release_endpoint(crate::STABLE), false);
        assert_eq!(
            args,
            words(&[
                "api",
                "--method",
                "GET",
                &format!("repos/{REPO}/releases/tags/lampos-stable"),
            ])
        );
        // The GET keeps the 5xx retry; no page of the release list is asked.
        assert!(args.windows(2).any(|p| p[0] == "--method" && p[1] == "GET"));
        assert!(!args.iter().any(|a| a == "--paginate"));
        // Nor does any endpoint here ask for it.
        assert!(!include_str!("github.rs").contains(concat!("releases", "?per_page")));
    }
    #[test]
    fn a_5xx_read_is_asked_again_after_each_pause_and_nothing_else_is() {
        let pauses = [Duration::from_secs(1), Duration::from_secs(2)];
        // Two 5xx, then an answer.
        let (mut slept, mut left) = (Vec::new(), 2);
        let answer = retried(
            &pauses,
            |pause| slept.push(pause),
            || {
                if left > 0 {
                    left -= 1;
                    Err(SERVER_FAILED.to_owned())
                } else {
                    Ok(7)
                }
            },
        );
        assert_eq!(answer, Ok(7));
        assert_eq!(slept, pauses);
        // Always 5xx: asked once more than the pauses, then the 5xx.
        let (mut slept, mut asked) = (Vec::new(), 0);
        let answer: Result<u8> = retried(
            &pauses,
            |pause| slept.push(pause),
            || {
                asked += 1;
                Err(SERVER_FAILED.to_owned())
            },
        );
        assert_eq!(answer, Err(SERVER_FAILED.to_owned()));
        assert_eq!((asked, slept.len()), (3, 2));
        // Any other failure is never asked again.
        let (mut slept, mut asked) = (Vec::new(), 0);
        let answer: Result<u8> = retried(
            &pauses,
            |pause| slept.push(pause),
            || {
                asked += 1;
                Err("GitHub CLI failed; remote state may need inspection".to_owned())
            },
        );
        assert!(answer.is_err());
        assert_eq!((asked, slept.len()), (1, 0));
    }
    #[test]
    fn stream_hash_is_bounded_and_matches_bytes() {
        let bytes = vec![b'a'; 100_000];
        let Answer::Hash(info) =
            consume(bytes.as_slice(), Output::Hash(bytes.len() as u64)).unwrap()
        else {
            panic!()
        };
        assert_eq!(info.size, bytes.len() as u64);
        assert_eq!(info.sha256, lamp_update::manifest::sha256_hex(&bytes));
        assert!(consume(bytes.as_slice(), Output::Hash(99_999)).is_err());
        assert!(consume(bytes.as_slice(), Output::Bytes(99_999)).is_err());
    }
}
