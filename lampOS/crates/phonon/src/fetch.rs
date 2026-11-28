//! The model and voices, fetched once and kept (#8754): every file in
//! [`lamp_msgs::phonon::MODEL_FILES`] and [`lamp_msgs::phonon::VOICES`],
//! from their pinned Hugging Face commits over https. A file is written as
//! `<name>.part` and renamed into place only when its size and SHA-256
//! match its pin; one that does not is removed and the fetch fails, so a
//! half or wrong file is never loaded. A kept file is checked again at
//! every start.
use lamp_msgs::phonon::{
    MODEL_BASE, MODEL_FILES, MODEL_FOLDER, Pinned, VOICE_SIZE, VOICES, VOICES_BASE,
};
use lamp_update::fetch::Channel;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

/// One file to keep: where it goes under the model's folder, where it is
/// fetched from, its size and SHA-256.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wanted {
    pub path: String,
    pub url: String,
    pub size: u64,
    pub sha256: &'static str,
}

/// Every file the model needs: the model's own, then each voice as
/// `voices/<Name>.safetensors`, the name the voice asks by.
pub fn wanted() -> Vec<Wanted> {
    let model = MODEL_FILES.iter().map(|file: &Pinned| Wanted {
        path: file.name.to_owned(),
        url: format!("{MODEL_BASE}/{}", file.source),
        size: file.size,
        sha256: file.sha256,
    });
    let voices = VOICES.iter().map(|voice| Wanted {
        path: format!("voices/{}.safetensors", voice.name),
        url: format!("{VOICES_BASE}/voices/{}.safetensors", voice.id),
        size: VOICE_SIZE,
        sha256: voice.sha256,
    });
    model.chain(voices).collect()
}

/// The model's folder under `state`.
pub fn folder(state: &Path) -> PathBuf {
    state.join(MODEL_FOLDER)
}

/// The SHA-256 of the file at `path`, as lowercase hex, and its size.
fn sha256_of(path: &Path) -> io::Result<(u64, String)> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 16];
    let mut size = 0;
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        hasher.update(&buffer[..n]);
    }
    Ok((size, hex(&hasher.finalize())))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Whether the file at `path` is `wanted`: its size, then its SHA-256.
pub fn kept(path: &Path, wanted: &Wanted) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.len() == wanted.size)
        && sha256_of(path).is_ok_and(|(size, sha)| size == wanted.size && sha == wanted.sha256)
}

/// What [`ensure`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ensured {
    /// Files fetched this time.
    pub fetched: usize,
    /// Files already kept and checked.
    pub kept: usize,
}

/// Every [`wanted`] file kept under `state`'s model folder: those already
/// there and right are kept, the rest fetched through `channel` and
/// checked. Fails at the first file that can't be fetched or doesn't
/// match its pin, which is not kept.
pub fn ensure(state: &Path, channel: &mut dyn Channel) -> Result<Ensured, String> {
    let root = folder(state);
    fs::create_dir_all(root.join("voices")).map_err(|e| format!("the model's folder: {e}"))?;
    let mut ensured = Ensured::default();
    for wanted in wanted() {
        let path = root.join(&wanted.path);
        if kept(&path, &wanted) {
            ensured.kept += 1;
            continue;
        }
        let part = root.join(format!("{}.part", wanted.path));
        let fetched = (|| {
            let mut out = fs::File::create(&part).map_err(|e| e.to_string())?;
            let mut hashing = Hashing {
                out: &mut out,
                hasher: Sha256::new(),
                size: 0,
            };
            channel
                .fetch(&wanted.url, wanted.size, &mut hashing)
                .map_err(|e| e.to_string())?;
            let (size, sha) = (hashing.size, hex(&hashing.hasher.finalize()));
            out.sync_all().map_err(|e| e.to_string())?;
            if size != wanted.size || sha != wanted.sha256 {
                return Err("its size or SHA-256 is not the pinned one: not kept".to_owned());
            }
            fs::rename(&part, &path).map_err(|e| e.to_string())
        })();
        if let Err(why) = fetched {
            let _ = fs::remove_file(&part);
            return Err(format!("{}: {why}", wanted.path));
        }
        ensured.fetched += 1;
    }
    Ok(ensured)
}

/// A writer that hashes what goes through it.
struct Hashing<'a> {
    out: &'a mut fs::File,
    hasher: Sha256,
    size: u64,
}

impl Write for Hashing<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let n = self.out.write(bytes)?;
        self.hasher.update(&bytes[..n]);
        self.size += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_update::fetch::Fetch;
    use std::collections::HashMap;

    /// A channel that serves `files` by URL and counts what it was asked.
    struct Served {
        files: HashMap<String, Vec<u8>>,
        asked: Vec<String>,
    }

    impl Channel for Served {
        fn fetch(&mut self, url: &str, most: u64, out: &mut dyn Write) -> Result<u64, Fetch> {
            self.asked.push(url.to_owned());
            let bytes = self.files.get(url).ok_or(Fetch::Missing)?;
            if bytes.len() as u64 > most {
                return Err(Fetch::Failed("too large".into()));
            }
            out.write_all(bytes)
                .map_err(|e| Fetch::Failed(e.to_string()))?;
            Ok(bytes.len() as u64)
        }
    }

    #[test]
    fn every_file_is_wanted_once_from_its_pinned_commit_over_https() {
        let wanted = wanted();
        assert_eq!(wanted.len(), MODEL_FILES.len() + VOICES.len());
        let mut paths = std::collections::BTreeSet::new();
        for file in &wanted {
            assert!(paths.insert(file.path.clone()), "{} twice", file.path);
            assert!(
                file.url
                    .starts_with("https://huggingface.co/gradium/phonon-")
            );
            assert!(!file.path.contains(".."));
        }
        assert!(paths.contains("model.q8.gguf") && paths.contains("voices/Freya.safetensors"));
    }

    /// A file whose bytes don't match its pin is never kept, and the fetch
    /// fails naming it; the files before it stay kept.
    #[test]
    fn a_file_that_does_not_match_its_pin_is_not_kept() {
        let state = tempfile::tempdir().unwrap();
        let first = &wanted()[0];
        let mut channel = Served {
            files: HashMap::from([(first.url.clone(), vec![b'x'; first.size as usize])]),
            asked: Vec::new(),
        };
        let why = ensure(state.path(), &mut channel).unwrap_err();
        assert!(why.starts_with("config.json: its size or SHA-256"), "{why}");
        let root = folder(state.path());
        assert!(!root.join("config.json").exists());
        assert!(!root.join("config.json.part").exists());
        assert_eq!(channel.asked.len(), 1);
    }

    /// A file the channel doesn't have fails the fetch, leaving nothing.
    #[test]
    fn a_missing_file_fails_the_fetch_and_leaves_nothing() {
        let state = tempfile::tempdir().unwrap();
        let mut channel = Served {
            files: HashMap::new(),
            asked: Vec::new(),
        };
        let why = ensure(state.path(), &mut channel).unwrap_err();
        assert!(why.contains("no such file"), "{why}");
        assert!(!folder(state.path()).join("config.json").exists());
    }

    /// A file kept and right is checked, not fetched again; a kept file
    /// changed since is fetched again.
    #[test]
    fn a_kept_file_is_checked_and_a_changed_one_fetched_again() {
        let state = tempfile::tempdir().unwrap();
        let file = Wanted {
            path: "x".into(),
            url: "https://x".into(),
            size: 3,
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        };
        let path = state.path().join("x");
        fs::write(&path, b"abc").unwrap();
        assert!(kept(&path, &file));
        fs::write(&path, b"abd").unwrap();
        assert!(!kept(&path, &file));
        fs::write(&path, b"abcd").unwrap();
        assert!(!kept(&path, &file));
    }
}
