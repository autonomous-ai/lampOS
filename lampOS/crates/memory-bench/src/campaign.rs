//! Durable, exclusive campaign accounting shared by every run and service.
//! Unsettled reservations survive a crash; a new run never gets a fresh balance.
use crate::{Result, hash, invalid, model::Receipt, protocol::valid_id};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};

const MAX_BYTES: u64 = 256 * 1024 * 1024;
const MAX_LINE: usize = 4096;
const MAX_CALLS: usize = 250_000;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub approved_cap_microusd: u64,
    /// Hash of the approved public model, price and scope configuration. No keys.
    pub policy_sha256: String,
}
fn digest_ok(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
impl Plan {
    fn validate(&self) -> Result<()> {
        if self.approved_cap_microusd == 0 || !digest_ok(&self.policy_sha256) {
            return Err(invalid("campaign requires an explicit cap and policy hash"));
        }
        Ok(())
    }
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Change {
    Reserve {
        run: String,
        call: u64,
        request_hash: String,
        amount: u64,
    },
    Settle {
        run: String,
        call: u64,
        request_hash: String,
        reserved: u64,
        charged: u64,
        accounting_failed: bool,
    },
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    sequence: u64,
    previous_hash: String,
    change: Change,
    hash: String,
}
struct Reservation {
    request_hash: String,
    amount: u64,
    settled: bool,
}
/// Keep this handle alive for the whole run. The OS releases its exclusive
/// lock on process exit, while the journal retains every uncertain charge.
pub struct Campaign {
    file: File,
    plan: Plan,
    previous: String,
    sequence: u64,
    bytes: u64,
    charged: u64,
    stopped: bool,
    calls: BTreeMap<(String, u64), Reservation>,
}
fn private_file(path: &Path, create: bool) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .append(true)
        .create_new(create)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o077 != 0 {
        return Err(invalid("campaign file must be private and regular"));
    }
    Ok(file)
}
impl Campaign {
    /// `planned` is the requested run/plan allowance, never inferred approval.
    /// A command must receive owner-approved settings before calling this API.
    pub fn create(path: &Path, plan: Plan, planned: u64) -> Result<Self> {
        plan.validate()?;
        if planned == 0 || planned > plan.approved_cap_microusd {
            return Err(invalid("planned spending exceeds approved campaign cap"));
        }
        fs::DirBuilder::new().mode(0o700).create(path)?;
        let mut config = private_file(&path.join("plan.json"), true)?;
        let bytes = serde_json::to_vec(&plan)?;
        config.write_all(&bytes)?;
        config.sync_all()?;
        let file = private_file(&path.join("costs.jsonl"), true)?;
        file.try_lock()
            .map_err(|_| invalid("campaign is already in use or cannot be locked"))?;
        file.sync_all()?;
        File::open(path)?.sync_all()?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        File::open(parent)?.sync_all()?;
        Ok(Self {
            file,
            plan,
            previous: hash(&bytes),
            sequence: 0,
            bytes: 0,
            charged: 0,
            stopped: false,
            calls: BTreeMap::new(),
        })
    }
    /// Reopening replays the complete journal. Partial/corrupt writes are a
    /// refusal, never a reason to reset the balance or repair the file silently.
    pub fn open(path: &Path, expected: Plan, planned: u64) -> Result<Self> {
        expected.validate()?;
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_dir() || metadata.permissions().mode() & 0o077 != 0 {
            return Err(invalid(
                "campaign directory must be private and not a symlink",
            ));
        }
        let mut file = private_file(&path.join("costs.jsonl"), false)?;
        file.try_lock()
            .map_err(|_| invalid("campaign is already in use or cannot be locked"))?;
        let mut bytes = Vec::new();
        private_file(&path.join("plan.json"), false)?
            .take(MAX_LINE as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_LINE || serde_json::from_slice::<Plan>(&bytes)? != expected {
            return Err(invalid(
                "campaign cap or public policy differs from approval",
            ));
        }
        let length = file.metadata()?.len();
        if length > MAX_BYTES {
            return Err(invalid("campaign journal exceeds bound"));
        }
        file.seek(SeekFrom::Start(0))?;
        let mut campaign = Self {
            file,
            plan: expected,
            previous: hash(&bytes),
            sequence: 0,
            bytes: length,
            charged: 0,
            stopped: false,
            calls: BTreeMap::new(),
        };
        let mut reader = BufReader::new(campaign.file.try_clone()?);
        loop {
            let mut line = Vec::new();
            let count =
                Read::take(&mut reader, MAX_LINE as u64 + 1).read_until(b'\n', &mut line)?;
            if count == 0 {
                break;
            }
            if count > MAX_LINE || line.last() != Some(&b'\n') {
                return Err(invalid("campaign journal has a partial or oversized row"));
            }
            let row: Entry = serde_json::from_slice(&line)?;
            if row.sequence != campaign.sequence
                || row.previous_hash != campaign.previous
                || row.hash != campaign.digest(&row.change)?
            {
                return Err(invalid("campaign journal hash chain differs"));
            }
            campaign.apply(&row.change)?;
            campaign.previous = row.hash;
            campaign.sequence += 1;
        }
        campaign.check_plan(planned)?;
        Ok(campaign)
    }
    pub fn charged_microusd(&self) -> u64 {
        self.charged
    }
    pub fn remaining_microusd(&self) -> u64 {
        self.plan.approved_cap_microusd.saturating_sub(self.charged)
    }
    pub fn check_plan(&self, planned: u64) -> Result<()> {
        if self.stopped || planned == 0 || planned > self.remaining_microusd() {
            return Err(invalid(
                "planned spending exceeds remaining campaign balance",
            ));
        }
        Ok(())
    }
    fn digest(&self, change: &Change) -> Result<String> {
        Ok(hash(&serde_json::to_vec(&(
            self.sequence,
            &self.previous,
            change,
        ))?))
    }
    fn apply(&mut self, change: &Change) -> Result<()> {
        match change {
            Change::Reserve {
                run,
                call,
                request_hash,
                amount,
            } => {
                if !valid_id(run)
                    || *call == 0
                    || !digest_ok(request_hash)
                    || *amount == 0
                    || self.calls.len() >= MAX_CALLS
                    || self.calls.contains_key(&(run.clone(), *call))
                {
                    return Err(invalid("invalid or duplicate campaign reservation"));
                }
                self.check_plan(*amount)?;
                self.charged += amount;
                self.calls.insert(
                    (run.clone(), *call),
                    Reservation {
                        request_hash: request_hash.clone(),
                        amount: *amount,
                        settled: false,
                    },
                );
            }
            Change::Settle {
                run,
                call,
                request_hash,
                reserved,
                charged,
                accounting_failed,
            } => {
                let entry = self
                    .calls
                    .get_mut(&(run.clone(), *call))
                    .ok_or_else(|| invalid("campaign settlement has no reservation"))?;
                if entry.settled || entry.request_hash != *request_hash || entry.amount != *reserved
                {
                    return Err(invalid(
                        "campaign settlement differs or was already applied",
                    ));
                }
                entry.settled = true;
                self.charged = (self.charged - entry.amount).saturating_add(*charged);
                self.stopped |= *accounting_failed
                    || charged > reserved
                    || self.charged > self.plan.approved_cap_microusd;
            }
        }
        Ok(())
    }
    /// Attach before the run's own audit write. If either audit fails, Client
    /// refuses dispatch. An uncertain call remains charged its full reservation.
    pub fn audit(&mut self, run: &str, stage: &str, receipt: &Receipt) -> Result<()> {
        if self.stopped {
            return Err(invalid("campaign stopped after accounting failure"));
        }
        let change = match stage {
            "model_reserved" if receipt.charged_microusd == receipt.reserved_microusd => {
                Change::Reserve {
                    run: run.into(),
                    call: receipt.call_number,
                    request_hash: receipt.request_hash.clone(),
                    amount: receipt.reserved_microusd,
                }
            }
            "model_completed" => {
                if receipt.usage.is_none()
                    && receipt.charged_microusd < receipt.reserved_microusd
                    && !(receipt.outcome == "expired_before_dispatch"
                        && receipt.charged_microusd == 0)
                {
                    return Err(invalid("unknown model usage cannot refund a reservation"));
                }
                Change::Settle {
                    run: run.into(),
                    call: receipt.call_number,
                    request_hash: receipt.request_hash.clone(),
                    reserved: receipt.reserved_microusd,
                    charged: receipt.charged_microusd,
                    accounting_failed: matches!(
                        receipt.outcome.as_str(),
                        "reservation_exceeded" | "accounting_overflow" | "evidence_failed"
                    ),
                }
            }
            _ => return Err(invalid("unknown campaign accounting stage")),
        };
        let digest = self.digest(&change)?;
        let row = Entry {
            sequence: self.sequence,
            previous_hash: self.previous.clone(),
            change,
            hash: digest.clone(),
        };
        let mut bytes = serde_json::to_vec(&row)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_LINE || self.bytes.saturating_add(bytes.len() as u64) > MAX_BYTES {
            self.stopped = true;
            return Err(invalid("campaign journal capacity exhausted"));
        }
        self.apply(&row.change)?;
        // A failed append poisons this handle. No dispatch can follow it; on
        // restart the journal either retains the reservation or refuses replay.
        if let Err(error) = self
            .file
            .write_all(&bytes)
            .and_then(|()| self.file.sync_data())
        {
            self.stopped = true;
            return Err(error.into());
        }
        self.bytes += bytes.len() as u64;
        self.sequence += 1;
        self.previous = digest;
        if self.stopped {
            return Err(invalid("provider charge exceeded the campaign reservation"));
        }
        Ok(())
    }
}
