use base64::{Engine, engine::general_purpose::STANDARD};
use lamp_update::manifest;
use lampos_promote::*;
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn key() -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed_unchecked(&[17; 32]).unwrap()
}
fn public() -> [u8; 32] {
    key().public_key().as_ref().try_into().unwrap()
}
fn context(sequence: u64) -> Context {
    Context {
        sequence,
        run: 900,
        attempt: 3,
        key: public(),
    }
}
fn payload(sequence: u64) -> Value {
    json!({"schema":1,"channel":"lamp-voice","sequence":sequence,"commit":"a".repeat(40),
        "files_url":format!("https://github.com/{REPO}/releases/download/lampos-lamp-voice-{sequence}/"),
        "files":[{"name":"lamp-update","size":6,"sha256":manifest::sha256_hex(b"binary")}],
        "units":[{"lamp":"product","unit":"lamp-update.service","size":5,"sha256":manifest::sha256_hex(b"unit\n")}],
        "min_updater":manifest::PRODUCT_UNITS_UPDATER,"settings":[{"path":"/etc/lampOS/records.json","text":"{}\n","sha256":manifest::sha256_hex(b"{}\n"),"mode":"0644","removed":false}]})
}
fn envelope(value: &Value) -> Vec<u8> {
    manifest::seal(&serde_json::to_vec(value).unwrap(), &key()).unwrap()
}
#[derive(Clone)]
struct Fake {
    releases: Vec<Release>,
    tags: BTreeMap<String, String>,
    assets: BTreeMap<u64, Vec<(Asset, Vec<u8>)>>,
    run: WorkflowRun,
    jobs: Vec<Job>,
    next: u64,
    writes: usize,
    renames: usize,
    fail: Option<(usize, bool)>,
    fail_restore: bool,
    fail_final_read: bool,
    final_read_failed: bool,
    corrupt_stage: bool,
    uncertain_releases: bool,
    upload_fail: Option<bool>,
    changed_head_during_upload: bool,
    foreign_canonical_on_failure: bool,
    deleted: Vec<String>,
    delete_fails: bool,
}
impl Fake {
    fn new(previous: Option<u64>) -> Self {
        let sequence = 10;
        let sha = "a".repeat(40);
        let tag = format!("lampos-lamp-voice-{sequence}");
        let source = Release {
            id: 1,
            tag_name: tag.clone(),
            target_commitish: sha.clone(),
            draft: false,
            prerelease: true,
            body: format!(
                "Built and tested from {sha}; ARM64 workflow run 12, attempt 2, sequence {sequence}.\n"
            ),
        };
        let mut fake = Self {
            releases: vec![source],
            tags: BTreeMap::from([(tag, sha.clone())]),
            assets: BTreeMap::new(),
            run: WorkflowRun {
                id: 12,
                run_attempt: 2,
                run_number: sequence,
                head_sha: sha.clone(),
                head_branch: CHANNEL.into(),
                path: WORKFLOW.into(),
                event: "push".into(),
                status: "completed".into(),
                conclusion: Some("success".into()),
                repository: Repository {
                    full_name: REPO.into(),
                    private: false,
                },
            },
            jobs: [
                "lampOS — build and test on Linux ARM64 (1/1)",
                "lampOS — sign and publish the tested lamp-voice artifact",
            ]
            .map(|name| Job {
                name: name.into(),
                status: "completed".into(),
                conclusion: Some("success".into()),
                run_id: 12,
                head_sha: sha.clone(),
            })
            .to_vec(),
            next: 100,
            writes: 0,
            renames: 0,
            fail: None,
            fail_restore: false,
            fail_final_read: false,
            final_read_failed: false,
            corrupt_stage: false,
            uncertain_releases: false,
            upload_fail: None,
            changed_head_during_upload: false,
            foreign_canonical_on_failure: false,
            deleted: Vec::new(),
            delete_fails: false,
        };
        fake.source_payload(payload(sequence));
        if let Some(previous) = previous {
            fake.releases.push(Release {
                id: 2,
                tag_name: STABLE.into(),
                target_commitish: sha.clone(),
                draft: false,
                prerelease: true,
                body: "stable".into(),
            });
            fake.tags.insert(STABLE.into(), sha);
            fake.add(2, manifest::ENVELOPE, envelope(&payload(previous)));
        }
        fake
    }
    fn add(&mut self, release: u64, name: &str, bytes: Vec<u8>) -> Asset {
        self.next += 1;
        let asset = Asset {
            id: self.next,
            name: name.into(),
            size: bytes.len() as u64,
            state: "uploaded".into(),
        };
        self.assets
            .entry(release)
            .or_default()
            .push((asset.clone(), bytes));
        asset
    }
    fn source_payload(&mut self, value: Value) {
        self.assets.insert(1, Vec::new());
        self.add(1, "lamp-update", b"binary".to_vec());
        self.add(1, "product.lamp-update.service", b"unit\n".to_vec());
        self.add(1, "manifest.json", serde_json::to_vec(&value).unwrap());
        self.add(1, manifest::ENVELOPE, envelope(&value));
    }
    fn current(&self) -> Option<(Asset, Vec<u8>)> {
        self.releases
            .iter()
            .find(|r| r.tag_name == STABLE)
            .and_then(|r| self.assets.get(&r.id))
            .and_then(|items| items.iter().find(|(a, _)| a.name == manifest::ENVELOPE))
            .cloned()
    }
    fn source_bytes(&self) -> Vec<u8> {
        self.assets[&1]
            .iter()
            .find(|(a, _)| a.name == manifest::ENVELOPE)
            .unwrap()
            .1
            .clone()
    }
    fn bytes(&self, id: u64) -> Vec<u8> {
        self.assets
            .values()
            .flatten()
            .find(|(a, _)| a.id == id)
            .unwrap()
            .1
            .clone()
    }
}
impl Remote for Fake {
    fn repository(&mut self) -> Result<Repository> {
        Ok(Repository {
            full_name: REPO.into(),
            private: false,
        })
    }
    fn releases(&mut self) -> Result<Vec<Release>> {
        if self.uncertain_releases {
            Err("unavailable".into())
        } else {
            Ok(self.releases.clone())
        }
    }
    fn tag(&mut self, name: &str) -> Result<Option<String>> {
        Ok(self.tags.get(name).cloned())
    }
    fn assets(&mut self, release: u64) -> Result<Vec<Asset>> {
        Ok(self
            .assets
            .get(&release)
            .into_iter()
            .flatten()
            .map(|(a, _)| a.clone())
            .collect())
    }
    fn read(&mut self, asset: &Asset, _most: u64) -> Result<Vec<u8>> {
        if self.fail_final_read
            && !self.final_read_failed
            && self.renames >= 2
            && asset.name == manifest::ENVELOPE
        {
            self.final_read_failed = true;
            return Err("final verification read lost".into());
        }
        Ok(self.bytes(asset.id))
    }
    fn hash(&mut self, asset: &Asset, _most: u64) -> Result<HashSize> {
        let bytes = self.bytes(asset.id);
        Ok(HashSize {
            size: bytes.len() as u64,
            sha256: manifest::sha256_hex(&bytes),
        })
    }
    fn run(&mut self, run: u64, attempt: u64) -> Result<WorkflowRun> {
        assert_eq!((run, attempt), (12, 2));
        Ok(self.run.clone())
    }
    fn jobs(&mut self, run: u64, attempt: u64) -> Result<Vec<Job>> {
        assert_eq!((run, attempt), (12, 2));
        Ok(self.jobs.clone())
    }
    fn create(
        &mut self,
        tag: &str,
        sha: &str,
        notes: &str,
        name: &str,
        bytes: &[u8],
    ) -> Result<()> {
        self.writes += 1;
        assert!(!self.releases.iter().any(|r| r.tag_name == tag));
        self.releases.push(Release {
            id: 2,
            tag_name: tag.into(),
            target_commitish: sha.into(),
            draft: false,
            prerelease: true,
            body: notes.into(),
        });
        self.tags.insert(tag.into(), sha.into());
        self.add(2, name, bytes.to_vec());
        Ok(())
    }
    fn upload(&mut self, tag: &str, name: &str, bytes: &[u8]) -> Result<()> {
        self.writes += 1;
        if self.upload_fail == Some(false) {
            return Err("upload failed before write".into());
        }
        let release = self.releases.iter().find(|r| r.tag_name == tag).unwrap().id;
        let mut bytes = bytes.to_vec();
        if self.corrupt_stage {
            bytes[0] ^= 1;
        }
        self.add(release, name, bytes);
        if self.upload_fail == Some(true) {
            return Err("upload response lost".into());
        }
        if self.changed_head_during_upload {
            self.assets
                .get_mut(&release)
                .unwrap()
                .iter_mut()
                .find(|(a, _)| a.name == manifest::ENVELOPE)
                .unwrap()
                .0
                .name = "externally-replaced-envelope.json".into();
            self.add(release, manifest::ENVELOPE, envelope(&payload(11)));
        }
        Ok(())
    }
    fn delete(&mut self, id: u64) -> Result<()> {
        if self.delete_fails {
            return Err("delete refused".into());
        }
        let list = self
            .assets
            .values_mut()
            .find(|items| items.iter().any(|(a, _)| a.id == id))
            .ok_or("asset absent")?;
        let at = list.iter().position(|(a, _)| a.id == id).unwrap();
        self.deleted.push(list.remove(at).0.name);
        Ok(())
    }
    fn rename(&mut self, id: u64, name: &str) -> Result<Asset> {
        self.writes += 1;
        self.renames += 1;
        let fail = self.fail == Some((self.renames, false));
        if fail {
            self.fail = None;
            return Err("before rename".into());
        }
        if self.fail_restore && self.renames > 2 {
            return Err("restore unavailable".into());
        }
        let list = self
            .assets
            .values_mut()
            .find(|items| items.iter().any(|(a, _)| a.id == id))
            .unwrap();
        if list.iter().any(|(a, _)| a.name == name && a.id != id) {
            return Err("asset name already exists".into());
        }
        let asset = &mut list.iter_mut().find(|(a, _)| a.id == id).unwrap().0;
        asset.name = name.into();
        let result = asset.clone();
        if self.fail == Some((self.renames, true)) {
            self.fail = None;
            if self.foreign_canonical_on_failure {
                self.assets
                    .get_mut(&2)
                    .unwrap()
                    .iter_mut()
                    .find(|(a, _)| a.id == id)
                    .unwrap()
                    .0
                    .name = "externally-moved-candidate.json".into();
                self.add(2, manifest::ENVELOPE, b"foreign canonical bytes".to_vec());
            }
            return Err("lost rename response".into());
        }
        Ok(result)
    }
}
#[test]
fn promotion_preserves_signed_bytes_and_old_backup() {
    let mut fake = Fake::new(Some(9));
    let old = fake.current().unwrap();
    let source = fake.source_bytes();
    let receipt = promote(&mut fake, &context(10)).unwrap();
    assert_eq!(receipt.outcome, "promoted");
    assert_eq!(receipt.source_run, 12);
    assert_eq!(receipt.source_attempt, 2);
    assert_eq!(fake.current().unwrap().1, source);
    assert_eq!(fake.bytes(old.0.id), old.1);
    let old_after = fake.assets[&2]
        .iter()
        .find(|(a, _)| a.id == old.0.id)
        .unwrap();
    assert_eq!(
        old_after.0.name,
        format!("lampos-manifest-previous-10-{}.json", old.0.id)
    );
    assert_eq!(receipt.previous_asset, Some(old.0.id));
    assert_eq!(fake.renames, 2);
}
#[test]
fn bootstrap_copies_only_envelope() {
    let mut fake = Fake::new(None);
    let source = fake.source_bytes();
    let result = promote(&mut fake, &context(10)).unwrap();
    assert_eq!(result.previous_asset, None);
    assert_eq!(fake.assets[&2].len(), 1);
    assert_eq!(fake.current().unwrap().1, source);
}
#[test]
fn equal_identical_is_noop_and_older_is_refused() {
    let mut fake = Fake::new(Some(10));
    assert_eq!(
        promote(&mut fake, &context(10)).unwrap().outcome,
        "unchanged"
    );
    assert_eq!(fake.writes, 0);
    let mut fake = Fake::new(Some(11));
    assert!(
        promote(&mut fake, &context(10))
            .unwrap_err()
            .contains("older")
    );
    assert_eq!(fake.writes, 0);
}
#[test]
fn equal_sequence_different_valid_envelope_refuses() {
    let mut fake = Fake::new(Some(10));
    let mut altered = payload(10);
    altered["commit"] = json!("b".repeat(40));
    fake.assets.insert(2, Vec::new());
    fake.add(2, manifest::ENVELOPE, envelope(&altered));
    assert!(
        promote(&mut fake, &context(10))
            .unwrap_err()
            .contains("different envelope")
    );
    assert_eq!(fake.writes, 0);
}
#[test]
fn failures_before_and_after_each_rename_restore_exact_old_bytes() {
    for rename in 1..=2 {
        for after in [false, true] {
            let mut fake = Fake::new(Some(9));
            let old = fake.current().unwrap();
            fake.fail = Some((rename, after));
            let error = promote(&mut fake, &context(10)).unwrap_err();
            assert!(
                error.contains("previous canonical restored"),
                "{rename}/{after}: {error}"
            );
            assert_eq!(fake.current(), Some(old));
        }
    }
}
#[test]
fn bootstrap_lost_rename_response_removes_canonical_and_keeps_stage() {
    let mut fake = Fake::new(None);
    fake.fail = Some((1, true));
    assert!(promote(&mut fake, &context(10)).is_err());
    assert!(fake.current().is_none());
    assert_eq!(fake.assets[&2].len(), 1);
    assert!(fake.assets[&2][0].0.name.contains("staged-10-900-3"));
}
#[test]
fn final_verification_failure_restores_old_bytes() {
    let mut fake = Fake::new(Some(9));
    let old = fake.current();
    fake.fail_final_read = true;
    assert!(promote(&mut fake, &context(10)).is_err());
    assert_eq!(fake.current(), old);
}
#[test]
fn restoration_failure_reports_retained_asset_ids() {
    let mut fake = Fake::new(Some(9));
    fake.fail = Some((2, true));
    fake.fail_restore = true;
    let error = promote(&mut fake, &context(10)).unwrap_err();
    assert!(error.contains("restoration failed"));
    assert!(error.contains("retained assets"));
    assert_eq!(fake.assets[&2].len(), 2);
}
#[test]
fn missing_canonical_or_orphan_tag_never_bootstraps_over_existing_state() {
    let mut fake = Fake::new(Some(9));
    fake.assets.insert(2, Vec::new());
    assert!(
        promote(&mut fake, &context(10))
            .unwrap_err()
            .contains("no canonical")
    );
    assert_eq!(fake.writes, 0);
    let mut fake = Fake::new(None);
    fake.tags.insert(STABLE.into(), "a".repeat(40));
    assert!(
        promote(&mut fake, &context(10))
            .unwrap_err()
            .contains("tag exists")
    );
    assert_eq!(fake.writes, 0);
}
#[test]
fn changed_downloaded_payload_refuses_before_writes() {
    for name in [
        "lamp-update",
        "product.lamp-update.service",
        "manifest.json",
        manifest::ENVELOPE,
    ] {
        let mut fake = Fake::new(Some(9));
        fake.assets
            .get_mut(&1)
            .unwrap()
            .iter_mut()
            .find(|(a, _)| a.name == name)
            .unwrap()
            .1[0] ^= 1;
        assert!(promote(&mut fake, &context(10)).is_err(), "{name}");
        assert_eq!(fake.writes, 0);
    }
}
#[test]
fn staged_corruption_leaves_old_canonical_intact() {
    let mut fake = Fake::new(Some(9));
    let old = fake.current();
    fake.corrupt_stage = true;
    assert!(
        promote(&mut fake, &context(10))
            .unwrap_err()
            .contains("staged envelope bytes")
    );
    assert_eq!(fake.current(), old);
    assert_eq!(fake.renames, 0);
}
#[test]
fn wrong_trusted_key_refuses_before_writes() {
    let mut fake = Fake::new(Some(9));
    let mut context = context(10);
    context.key = [9; 32];
    assert!(promote(&mut fake, &context).is_err());
    assert_eq!(fake.writes, 0);
}
#[test]
fn source_provenance_must_match_original_successful_attempt() {
    for change in 0..13 {
        let mut fake = Fake::new(Some(9));
        match change {
            0 => fake.run.run_attempt = 3,
            1 => fake.run.conclusion = Some("failure".into()),
            2 => fake.run.event = "pull_request".into(),
            3 => fake.run.head_sha = "b".repeat(40),
            4 => fake.run.head_branch = "other".into(),
            5 => fake.run.run_number = 11,
            6 => fake.run.path = ".github/workflows/other.yml".into(),
            7 => fake.run.repository.private = true,
            8 => fake.jobs[1].conclusion = Some("skipped".into()),
            9 => fake.jobs[0].conclusion = Some("failure".into()),
            10 => fake.releases[0].draft = true,
            11 => {
                fake.tags
                    .insert("lampos-lamp-voice-10".into(), "b".repeat(40));
            }
            12 => fake.releases[0].body = "unproven release".into(),
            _ => unreachable!(),
        }
        assert!(promote(&mut fake, &context(10)).is_err(), "case {change}");
        assert_eq!(fake.writes, 0);
    }
}
#[test]
fn signed_identity_must_match_source_and_fixed_url() {
    for change in 0..3 {
        let mut fake = Fake::new(Some(9));
        let mut value = payload(10);
        match change {
            0 => value["channel"] = json!("other"),
            1 => value["commit"] = json!("b".repeat(40)),
            2 => value["files_url"] = json!("https://example.com/files/"),
            _ => unreachable!(),
        }
        fake.source_payload(value);
        assert!(promote(&mut fake, &context(10)).is_err());
        assert_eq!(fake.writes, 0);
    }
}
#[test]
fn external_models_are_explicitly_unsupported() {
    let mut fake = Fake::new(Some(9));
    let mut value = payload(10);
    value["models"] = json!([{"path":"/opt/lampOS/eyes/models/a.bin","url":"https://example.com/a.bin","bytes":1,"sha256":"a".repeat(64)}]);
    fake.source_payload(value);
    assert!(
        promote(&mut fake, &context(10))
            .unwrap_err()
            .contains("external model")
    );
    assert_eq!(fake.writes, 0);
}
#[test]
fn uncertain_or_duplicate_inventories_refuse() {
    let mut fake = Fake::new(Some(9));
    fake.uncertain_releases = true;
    assert!(promote(&mut fake, &context(10)).is_err());
    assert_eq!(fake.writes, 0);
    let mut fake = Fake::new(Some(9));
    fake.releases.push(fake.releases[0].clone());
    assert!(promote(&mut fake, &context(10)).is_err());
    assert_eq!(fake.writes, 0);
    let mut fake = Fake::new(Some(9));
    fake.add(1, "lamp-update", b"binary".to_vec());
    assert!(promote(&mut fake, &context(10)).is_err());
    assert_eq!(fake.writes, 0);
}
#[test]
fn staging_and_backup_names_are_never_overwritten() {
    for stage in [true, false] {
        let mut fake = Fake::new(Some(9));
        let old = fake.current();
        let name = if stage {
            "lampos-manifest-staged-10-900-3.json".into()
        } else {
            format!(
                "lampos-manifest-previous-10-{}.json",
                old.as_ref().unwrap().0.id
            )
        };
        fake.add(2, &name, b"retained".to_vec());
        assert!(promote(&mut fake, &context(10)).is_err());
        assert_eq!(fake.current(), old);
        assert!(
            fake.assets[&2]
                .iter()
                .any(|(a, b)| a.name == name && b == b"retained")
        );
    }
}
#[test]
fn context_requires_exact_switch_and_trusted_public_key() {
    let mut env: BTreeMap<String, String> = [
        ("GITHUB_ACTIONS", "true"),
        ("GITHUB_REPOSITORY", REPO),
        ("GITHUB_EVENT_NAME", "workflow_dispatch"),
        ("GITHUB_REF", "refs/heads/lamp-voice"),
        ("LAMPOS_STABLE_PROMOTION", "on"),
        ("GITHUB_RUN_ID", "900"),
        ("GITHUB_RUN_ATTEMPT", "3"),
        ("PROMOTE_FULL", "false"),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v.into()))
    .collect();
    env.insert("LAMPOS_UPDATE_PUBLIC_KEY".into(), STANDARD.encode(public()));
    assert!(Context::environment("10", &env).is_ok());
    for field in [
        "GITHUB_ACTIONS",
        "GITHUB_REPOSITORY",
        "GITHUB_EVENT_NAME",
        "GITHUB_REF",
        "LAMPOS_STABLE_PROMOTION",
        "LAMPOS_UPDATE_PUBLIC_KEY",
        "GITHUB_RUN_ID",
        "GITHUB_RUN_ATTEMPT",
        "PROMOTE_FULL",
    ] {
        let mut wrong = env.clone();
        wrong.insert(field.into(), "wrong".into());
        assert!(Context::environment("10", &wrong).is_err(), "{field}");
    }
    env.insert("LAMPOS_STABLE_PROMOTION".into(), "ON".into());
    assert!(Context::environment("10", &env).is_err());
    for invalid in [
        "",
        "0",
        "01",
        "-1",
        "+1",
        "1; echo bad",
        "18446744073709551616",
    ] {
        assert!(positive(invalid).is_err(), "{invalid}");
    }
    assert_eq!(positive("18446744073709551615").unwrap(), u64::MAX);
}

#[test]
fn upload_failure_before_or_after_remote_write_preserves_old_canonical() {
    for after in [false, true] {
        let mut fake = Fake::new(Some(9));
        let old = fake.current();
        fake.upload_fail = Some(after);
        assert!(promote(&mut fake, &context(10)).is_err());
        assert_eq!(fake.current(), old);
        assert_eq!(fake.renames, 0);
        assert_eq!(fake.assets[&2].len(), if after { 2 } else { 1 });
    }
}
#[test]
fn stable_changed_while_staging_is_not_overwritten() {
    let mut fake = Fake::new(Some(9));
    let old = fake.current().unwrap();
    fake.changed_head_during_upload = true;
    assert!(
        promote(&mut fake, &context(10))
            .unwrap_err()
            .contains("changed before promotion")
    );
    assert_eq!(fake.current().unwrap().1, envelope(&payload(11)));
    assert_eq!(fake.bytes(old.0.id), old.1);
    assert_eq!(fake.renames, 0);
}
#[test]
fn foreign_canonical_during_recovery_is_untouched() {
    let mut fake = Fake::new(Some(9));
    let old = fake.current().unwrap();
    fake.fail = Some((2, true));
    fake.foreign_canonical_on_failure = true;
    let error = promote(&mut fake, &context(10)).unwrap_err();
    assert!(error.contains("canonical changed outside this promotion"));
    assert!(error.contains(&format!("retained assets {}/", old.0.id)));
    assert_eq!(fake.current().unwrap().1, b"foreign canonical bytes");
    assert_eq!(fake.bytes(old.0.id), old.1);
    assert_eq!(fake.renames, 2);
}
#[test]
fn signature_valid_future_updater_requirement_refuses_before_writes() {
    let mut fake = Fake::new(Some(9));
    let mut value = payload(10);
    value["min_updater"] = json!(manifest::UPDATER + 1);
    fake.source_payload(value);
    assert!(manifest::signed(&fake.source_bytes(), &public()).is_ok());
    let error = promote(&mut fake, &context(10)).unwrap_err();
    assert!(error.contains("updater"), "{error}");
    assert_eq!(fake.writes, 0);
}

/// Older promotions' backups on stable, and stages left by runs: names as
/// `promote` writes them, sequences below the promotion's 10 (#4161).
fn with_old_backups(fake: &mut Fake, count: u64, staged: &[u64]) {
    for n in 0..count {
        let id = 5_000 + n;
        fake.add(
            2,
            &format!("lampos-manifest-previous-{}-{id}.json", 1 + n % 9),
            b"old".to_vec(),
        );
    }
    for seq in staged {
        fake.add(
            2,
            &format!("lampos-manifest-staged-{seq}-7-1.json"),
            b"left".to_vec(),
        );
    }
}
fn stable_names(fake: &Fake) -> Vec<String> {
    fake.assets[&2]
        .iter()
        .map(|(a, _)| a.name.clone())
        .collect()
}
#[test]
fn a_promotion_keeps_the_newest_backups_and_deletes_the_rest() {
    let mut fake = Fake::new(Some(9));
    with_old_backups(&mut fake, 30, &[3, 12]);
    let source = fake.source_bytes();
    let receipt = promote(&mut fake, &context(10)).unwrap();
    assert_eq!(receipt.outcome, "promoted");
    assert_eq!(fake.current().unwrap().1, source);
    let names = stable_names(&fake);
    let backups: Vec<&String> = names
        .iter()
        .filter(|n| n.starts_with("lampos-manifest-previous-"))
        .collect();
    assert_eq!(backups.len(), lampos_promote::KEEP_BACKUPS);
    assert!(
        backups
            .iter()
            .any(|n| n.starts_with("lampos-manifest-previous-10-")),
        "this promotion's own backup is kept: {backups:?}"
    );
    assert!(names.contains(&"lampos-manifest-staged-12-7-1.json".to_owned()));
    assert!(!names.contains(&"lampos-manifest-staged-3-7-1.json".to_owned()));
    assert_eq!(fake.deleted.len(), 30 - 19 + 1);
}
#[test]
fn a_cleanup_that_fails_leaves_the_promotion_standing() {
    let mut fake = Fake::new(Some(9));
    with_old_backups(&mut fake, 30, &[]);
    fake.delete_fails = true;
    let source = fake.source_bytes();
    let receipt = promote(&mut fake, &context(10)).unwrap();
    assert_eq!(receipt.outcome, "promoted");
    assert_eq!(fake.current().unwrap().1, source);
}
#[test]
fn a_failed_promotion_deletes_nothing() {
    let mut fake = Fake::new(Some(9));
    with_old_backups(&mut fake, 30, &[3]);
    fake.fail = Some((2, false));
    assert!(promote(&mut fake, &context(10)).is_err());
    assert!(fake.deleted.is_empty());
}
