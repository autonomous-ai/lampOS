use crate::invalid;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::Path,
};

pub const SCOPE: &str = "sensor5-monday-local-av";

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Consent {
    pub schema_version: u32,
    pub participant_alias: String,
    pub operator_alias: String,
    pub accepted: bool,
    pub scope: String,
    pub accepted_at_utc: String,
    pub consent_text_sha256: String,
}

pub struct Accepted {
    pub record: Consent,
    pub record_bytes: Vec<u8>,
    pub text_bytes: Vec<u8>,
    pub record_sha256: String,
}

pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn hex_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn participant(value: &str) -> bool {
    matches!(value, "P01" | "P02" | "P03" | "P04")
}

fn operator(value: &str) -> bool {
    value.len() == 3
        && value.starts_with('O')
        && value[1..].bytes().all(|b| b.is_ascii_digit())
        && value != "O00"
}

fn utc(value: &str) -> bool {
    let b = value.as_bytes();
    (b.len() == 20 || (22..=24).contains(&b.len()))
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && b.last() == Some(&b'Z')
        && (b.len() == 20 || b[19] == b'.' && b[20..b.len() - 1].iter().all(u8::is_ascii_digit))
        && b[..19]
            .iter()
            .enumerate()
            .all(|(i, c)| matches!(i, 4 | 7 | 10 | 13 | 16) || c.is_ascii_digit())
        && ("01"..="12").contains(&&value[5..7])
        && ("01"..="31").contains(&&value[8..10])
        && &value[11..13] <= "23"
        && &value[14..16] <= "59"
        && &value[17..19] <= "59"
}

pub fn private_bytes(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o077 != 0
    {
        return Err(invalid(
            "input must be a private regular file owned by this user",
        ));
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(invalid("input exceeds bounded size"));
    }
    Ok(bytes)
}

pub fn load(
    record: &Path,
    text: &Path,
    expected_hash: &str,
    alias: &str,
    operator_alias: &str,
) -> io::Result<Accepted> {
    let record_bytes = private_bytes(record, 8192)?;
    let text_bytes = private_bytes(text, 16_384)?;
    validate(
        record_bytes,
        text_bytes,
        expected_hash,
        alias,
        operator_alias,
    )
}

pub fn validate(
    record_bytes: Vec<u8>,
    text_bytes: Vec<u8>,
    expected_hash: &str,
    alias: &str,
    operator_alias: &str,
) -> io::Result<Accepted> {
    let record_sha256 = sha256(&record_bytes);
    if !hex_hash(expected_hash) || record_sha256 != expected_hash.to_ascii_lowercase() {
        return Err(invalid("consent record hash mismatch"));
    }
    let record: Consent = serde_json::from_slice(&record_bytes)?;
    if record.schema_version != 1
        || !record.accepted
        || record.scope != SCOPE
        || !participant(alias)
        || record.participant_alias != alias
        || !operator(operator_alias)
        || record.operator_alias != operator_alias
        || alias == operator_alias
        || !utc(&record.accepted_at_utc)
        || !hex_hash(&record.consent_text_sha256)
        || sha256(&text_bytes) != record.consent_text_sha256.to_ascii_lowercase()
    {
        return Err(invalid(
            "accepted consent record, aliases, scope, UTC or source text do not match",
        ));
    }
    let text =
        std::str::from_utf8(&text_bytes).map_err(|_| invalid("consent text must be UTF-8"))?;
    let aliases: Vec<_> = text
        .lines()
        .filter_map(|line| line.strip_prefix("Participant alias: "))
        .collect();
    let decisions: Vec<_> = text
        .lines()
        .filter_map(|line| line.strip_prefix("Consent accepted: "))
        .collect();
    let confirmations: Vec<_> = text
        .lines()
        .filter_map(|line| line.strip_prefix("Participant confirmation/signature (local only):"))
        .collect();
    if aliases != [alias]
        || decisions != ["yes"]
        || confirmations.len() != 1
        || confirmations[0].trim().is_empty()
    {
        return Err(invalid(
            "original consent text must contain one matching alias, explicit yes and nonempty confirmation/signature",
        ));
    }
    Ok(Accepted {
        record,
        record_bytes,
        text_bytes,
        record_sha256,
    })
}

pub fn new_directory(path: &Path) -> io::Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|p| matches!(p, std::path::Component::ParentDir))
    {
        return Err(invalid("a fresh absolute directory is required"));
    }
    fs::DirBuilder::new().mode(0o700).create(path)
}

pub fn create_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

pub fn prepare(path: &Path) -> io::Result<()> {
    if path
        .to_str()
        .is_none_or(|s| s.bytes().any(|b| b.is_ascii_whitespace()))
    {
        return Err(invalid("frame-link path cannot contain whitespace"));
    }
    new_directory(path)?;
    let mut random = [0u8; 32];
    fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
    let controller: [u8; 16] = random[..16].try_into().expect("16 bytes");
    let reader: [u8; 16] = random[16..].try_into().expect("16 bytes");
    if controller == [0; 16] || reader == [0; 16] || controller == reader {
        return Err(invalid("OS entropy yielded invalid link boots"));
    }
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let text = format!(
        "LAMP_EYES_FRAME_LINK=--frame-link {} --frame-link-controller {} --frame-link-reader {}\n",
        path.join("camera.frames").display(),
        hex(&controller),
        hex(&reader)
    );
    create_private(&path.join("frame-link.env"), text.as_bytes())
}
