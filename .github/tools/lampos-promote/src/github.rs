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

const MAX_JSON: u64 = 64 * 1024 * 1024;
const COMMAND_LIMIT: Duration = Duration::from_secs(600);
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
fn command(arguments: &[String], output: Output) -> Result<Answer> {
    let mut child = Command::new("gh")
        .args(arguments)
        .env("GH_HOST", "github.com")
        .env_remove("LAMPOS_UPDATE_KEY")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|_| "cannot start GitHub CLI")?;
    let stdout = child.stdout.take().ok_or("GitHub CLI has no output pipe")?;
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
        let mut args = words(&["api", "--method", "GET"]);
        if pages {
            args.extend(words(&["--paginate", "--slurp"]));
        }
        args.push(format!("repos/{REPO}/{endpoint}"));
        self.json(&args)
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
    fn releases(&mut self) -> Result<Vec<Release>> {
        let pages: Vec<Vec<Release>> = self.api("releases?per_page=100", true)?;
        Ok(pages.into_iter().flatten().collect())
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
