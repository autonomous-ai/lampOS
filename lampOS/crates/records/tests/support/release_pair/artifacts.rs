use super::{Result, error};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

#[derive(Clone, Serialize)]
pub struct Artifact {
    pub release: u64,
    pub part: &'static str,
    pub source: &'static str,
    pub sha256: &'static str,
    pub size: u64,
    pub path: PathBuf,
}

pub struct Pair {
    pub old_records: Artifact,
    pub current_records: Artifact,
    pub old_voice: Artifact,
    pub current_voice: Artifact,
}

pub fn digest(path: &Path) -> Result<String> {
    let mut file: fs::File = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK
            | rustix::fs::OFlags::CLOEXEC
            | rustix::fs::OFlags::NOATIME,
        rustix::fs::Mode::empty(),
    )
    .map_err(error)?
    .into();
    let metadata = file.metadata().map_err(error)?;
    if !metadata.is_file() || metadata.nlink() != 1 || metadata.len() > 32 * 1024 * 1024 {
        return Err("artifact/fixture must be a bounded single-link regular file".into());
    }
    let mut hash = Sha256::new();
    let mut bytes = [0; 32 * 1024];
    let mut total = 0usize;
    loop {
        let count = file.read(&mut bytes).map_err(error)?;
        if count == 0 {
            break;
        }
        total += count;
        if total > 32 * 1024 * 1024 {
            return Err("artifact/fixture grew beyond original byte bound".into());
        }
        hash.update(&bytes[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

impl Artifact {
    fn load(
        variable: &str,
        release: u64,
        part: &'static str,
        source: &'static str,
        sha256: &'static str,
        size: u64,
    ) -> Result<Self> {
        let path =
            PathBuf::from(std::env::var_os(variable).ok_or_else(|| format!("missing {variable}"))?);
        if !path.is_absolute() {
            return Err(format!("{variable} must be absolute"));
        }
        let metadata = fs::symlink_metadata(&path).map_err(error)?;
        if !metadata.is_file() || metadata.len() != size || metadata.nlink() != 1 {
            return Err(format!("wrong artifact shape/size for {variable}"));
        }
        if digest(&path)? != sha256 {
            return Err(format!("wrong pinned digest for {variable}"));
        }
        let mut file: fs::File = rustix::fs::open(
            &path,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::NONBLOCK
                | rustix::fs::OFlags::CLOEXEC
                | rustix::fs::OFlags::NOATIME,
            rustix::fs::Mode::empty(),
        )
        .map_err(error)?
        .into();
        let mut header = [0; 64];
        file.read_exact(&mut header).map_err(error)?;
        if &header[..4] != b"\x7fELF"
            || header[4] != 2
            || header[5] != 1
            || u16::from_le_bytes([header[18], header[19]]) != 183
        {
            return Err(format!(
                "{variable} is not an ELF64 little-endian AArch64 executable"
            ));
        }
        Ok(Self {
            release,
            part,
            source,
            sha256,
            size,
            path,
        })
    }
    pub fn verify(&self) -> Result<()> {
        if fs::metadata(&self.path).map_err(error)?.len() != self.size
            || digest(&self.path)? != self.sha256
        {
            return Err("artifact changed during fixture".into());
        }
        Ok(())
    }
}
impl Pair {
    pub fn load() -> Result<Self> {
        const OLD: &str = "ee53765c8d014eb00e41c1a5a7054d2546c18bdb";
        const CURRENT: &str = "be16c3327524ccef8a1908a3b8ebf40c16efba8a";
        Ok(Self {
            old_records: Artifact::load(
                "LAMP_PAIR_6408_RECORDS",
                6408,
                "lamp-records",
                OLD,
                "a1839349ca730d0051033bf37a43238dfc0deb2a32184e600893424724722031",
                1393272,
            )?,
            current_records: Artifact::load(
                "LAMP_PAIR_CURRENT_RECORDS",
                6449,
                "lamp-records",
                CURRENT,
                "b18095bc74064e9949d9ed16c8f3c9a4c8c46e35892d3922800cf4aea8bb668f",
                1406976,
            )?,
            old_voice: Artifact::load(
                "LAMP_PAIR_6408_VOICE",
                6408,
                "lamp-voice",
                OLD,
                "af7cd0d1e384ebddcca2b58eea94176c80aba573a5f47f4cd95cc8ce1a7ad084",
                28375944,
            )?,
            current_voice: Artifact::load(
                "LAMP_PAIR_CURRENT_VOICE",
                6449,
                "lamp-voice",
                CURRENT,
                "21783ba5fd6654d96164f7426dc27f2c72f06a074ce91fe06b256feec697c6da",
                28372464,
            )?,
        })
    }
    pub fn verify(&self) -> Result<()> {
        for part in [
            &self.old_records,
            &self.current_records,
            &self.old_voice,
            &self.current_voice,
        ] {
            part.verify()?;
        }
        Ok(())
    }
}
