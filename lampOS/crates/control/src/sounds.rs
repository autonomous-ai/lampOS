//! A paired computer's own clips (docs/control.md, "Sounds"): uploaded in
//! parts with `sound.upload`, checked whole, and kept in the state folder's
//! `sounds/<client>/<name>.wav`, which the voice plays from
//! (`lamp_voice::say::read_clip`, its `LAMP_SAY_CLIPS`). Then `sound` with
//! the clip's name plays it, `sound.list` lists the computer's own, and
//! `sound.remove` deletes one.
//!
//! The bounds are the lamp's, not a policy (the local leader's 3 Nov):
//!
//! - **A clip:** a WAV of 16-bit mono PCM at 24 kHz, the speaker path's, at
//!   most 30 s, and no louder than the voice's own cap for a file sound
//!   (no sample past -12 dBFS, no 10 ms past -30 dBFS RMS), the speaker's
//!   and the ear's bound. The voice reads it again with the same checks
//!   when it plays it.
//! - **A computer's store:** at most [`CLIPS_MOST`] clips and
//!   [`STORE_MOST_BYTES`] in all, so no computer fills the lamp's disk.
//! - **A revoke deletes them** ([`Sounds::forget`]): nothing of a computer
//!   the owner let go stays on the lamp.

use std::collections::HashMap;
use std::io::{Cursor, Write as _};
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::protocol::{Error, ErrorCode};

/// The folder in the state folder the clips are kept in, one folder for
/// each computer.
pub const SOUNDS_FOLDER: &str = "sounds";
/// A clip's rate: the voice's speaker path's (`lamp_voice::say::SOUND_RATE`).
pub const CLIP_RATE: u32 = 24_000;
/// A clip's longest, in frames: 30 s (`lamp_voice::say::CLIP_MOST_FRAMES`).
pub const CLIP_MOST_FRAMES: u32 = CLIP_RATE * 30;
/// A clip's largest file (`lamp_voice::say::CLIP_MOST_BYTES`).
pub const CLIP_MOST_BYTES: usize = 44 + 2 * CLIP_MOST_FRAMES as usize + 4_096;
/// No sample past this (`lamp_voice::say::SOUND_PEAK_DBFS`).
pub const PEAK_DBFS: f64 = -12.0;
/// No 10 ms louder than this, as RMS (`lamp_voice::say::SOUND_LOUDEST_DBFS`).
pub const LOUDEST_DBFS: f64 = -30.0;
/// The window the loudness is measured over: 10 ms.
const WINDOW: usize = 240;
/// The most clips one computer keeps.
pub const CLIPS_MOST: usize = 64;
/// The most bytes one computer's clips take in all: 20 MB.
pub const STORE_MOST_BYTES: u64 = 20_000_000;
/// The longest one upload may take from its first part to its last, in µs.
pub const UPLOAD_WITHIN_US: u64 = 120_000_000;

/// What a part did.
#[derive(Debug, PartialEq)]
pub enum Step {
    /// Taken: this many parts are in.
    Got(u32),
    /// The last part: the clip is whole, checked and kept.
    Kept(Value),
}

/// One part's fields, as `sound.upload` sends them.
pub struct Part<'a> {
    pub name: &'a str,
    pub sha256: &'a str,
    pub size: usize,
    pub part: u32,
    pub parts: u32,
    pub data: &'a str,
}

/// An upload being gathered.
struct Flight {
    name: String,
    sha256: String,
    size: usize,
    parts: u32,
    next: u32,
    bytes: Vec<u8>,
    started_us: u64,
}

/// The computers' clips: where they're kept, and each computer's one
/// upload in flight.
pub struct Sounds {
    folder: PathBuf,
    flights: HashMap<String, Flight>,
}

impl Sounds {
    /// The clips kept under `folder` (the state folder's [`SOUNDS_FOLDER`]).
    pub fn new(folder: PathBuf) -> Self {
        Self {
            folder,
            flights: HashMap::new(),
        }
    }

    /// Where `client`'s clip `name` is kept, whether or not it is.
    pub fn path(folder: &Path, client: &str, name: &str) -> Option<PathBuf> {
        safe_client(client).then(|| folder.join(client).join(format!("{name}.wav")))
    }

    /// One part from `client`. Part 0 starts an upload, dropping one it had
    /// in flight (a broken upload is sent again from 0, never resumed). Any
    /// other part must be the next of this computer's upload, the same
    /// clip, within [`UPLOAD_WITHIN_US`] of its first part, or the upload is
    /// dropped. The last part checks the size, the SHA-256, the clip itself
    /// and the computer's store, and keeps it, over one of the same name.
    pub fn part(&mut self, client: &str, part: &Part<'_>, now_us: u64) -> Result<Step, Error> {
        let bad = |why: &str| Error::new(ErrorCode::BadRequest, why);
        if !safe_client(client) {
            return Err(Error::new(
                ErrorCode::Refused,
                "this computer's id can't name a folder",
            ));
        }
        if part.part == 0 {
            self.flights.insert(
                client.to_owned(),
                Flight {
                    name: part.name.to_owned(),
                    sha256: part.sha256.to_ascii_lowercase(),
                    size: part.size,
                    parts: part.parts,
                    next: 0,
                    bytes: Vec::with_capacity(part.size),
                    started_us: now_us,
                },
            );
        }
        let Some(flight) = self.flights.get_mut(client) else {
            return Err(bad("part: an upload starts at part 0"));
        };
        let same = flight.name == part.name
            && flight.sha256 == part.sha256.to_ascii_lowercase()
            && flight.size == part.size
            && flight.parts == part.parts;
        let failed = if !same {
            Some(
                "part: not the upload in flight (its name, sha256, size and parts are kept from part 0); send it again from 0",
            )
        } else if part.part != flight.next {
            Some("part: out of order; send the upload again from 0")
        } else if now_us.saturating_sub(flight.started_us) > UPLOAD_WITHIN_US {
            Some("part: the upload took over 120 s; send it again from 0")
        } else {
            None
        };
        if let Some(why) = failed {
            self.flights.remove(client);
            return Err(bad(why));
        }
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(part.data) else {
            self.flights.remove(client);
            return Err(bad("data: not base64; send the upload again from 0"));
        };
        if flight.bytes.len() + bytes.len() > flight.size {
            self.flights.remove(client);
            return Err(bad("data: more bytes than size"));
        }
        flight.bytes.extend_from_slice(&bytes);
        flight.next += 1;
        if flight.next < flight.parts {
            return Ok(Step::Got(flight.next));
        }
        let flight = self.flights.remove(client).expect("in flight");
        if flight.bytes.len() != flight.size {
            return Err(bad("data: fewer bytes than size"));
        }
        if hex(&Sha256::digest(&flight.bytes)) != flight.sha256 {
            return Err(bad("sha256: not the clip's"));
        }
        let frames = check_clip(&flight.bytes).map_err(|why| bad(&format!("clip: {why}")))?;
        self.fits(client, &flight.name, flight.bytes.len() as u64)?;
        let path = Self::path(&self.folder, client, &flight.name).expect("a safe client");
        keep(&path, &flight.bytes).map_err(|error| {
            eprintln!("lamp-control: sound.upload: keeping the clip: {error}");
            Error::new(ErrorCode::Refused, "the lamp could not keep the clip")
        })?;
        Ok(Step::Kept(about(
            &flight.name,
            flight.bytes.len() as u64,
            frames,
        )))
    }

    /// Whether `client`'s store takes a clip `name` of `bytes`, one of the
    /// same name replaced.
    fn fits(&self, client: &str, name: &str, bytes: u64) -> Result<(), Error> {
        let others: Vec<(String, u64)> = self
            .kept(client)
            .into_iter()
            .filter(|(kept, _)| kept != name)
            .collect();
        if others.len() >= CLIPS_MOST {
            return Err(Error::new(
                ErrorCode::Refused,
                format!("this computer keeps {CLIPS_MOST} clips already: remove one first"),
            ));
        }
        let used: u64 = others.iter().map(|(_, size)| size).sum();
        if used + bytes > STORE_MOST_BYTES {
            return Err(Error::new(
                ErrorCode::Refused,
                format!(
                    "this computer's clips would take over {STORE_MOST_BYTES} bytes ({used} kept): remove one first"
                ),
            ));
        }
        Ok(())
    }

    /// `client`'s clips by name, with each one's size, in name order.
    fn kept(&self, client: &str) -> Vec<(String, u64)> {
        let mut kept: Vec<(String, u64)> = std::fs::read_dir(self.folder.join(client))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                let name = name.strip_suffix(".wav")?.to_owned();
                let metadata = entry.metadata().ok()?;
                metadata.is_file().then_some((name, metadata.len()))
            })
            .collect();
        kept.sort();
        kept
    }

    /// `sound.list`: `client`'s clips, each its name, bytes and length.
    pub fn list(&self, client: &str) -> Value {
        if !safe_client(client) {
            return json!([]);
        }
        let sounds: Vec<Value> = self
            .kept(client)
            .into_iter()
            .map(|(name, bytes)| {
                let frames = std::fs::read(self.folder.join(client).join(format!("{name}.wav")))
                    .ok()
                    .and_then(|wav| hound::WavReader::new(Cursor::new(wav)).ok())
                    .map_or(0, |reader| reader.duration());
                about(&name, bytes, frames)
            })
            .collect();
        json!(sounds)
    }

    /// `sound.remove`: `client`'s clip `name` deleted.
    pub fn remove(&self, client: &str, name: &str) -> Result<(), Error> {
        let path = Self::path(&self.folder, client, name).ok_or_else(|| {
            Error::new(ErrorCode::Refused, "this computer's id can't name a folder")
        })?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(Error::new(
                ErrorCode::Refused,
                format!("no clip named {name} on this lamp for this computer"),
            )),
            Err(error) => {
                eprintln!("lamp-control: sound.remove: {error}");
                Err(Error::new(
                    ErrorCode::Refused,
                    "the lamp could not remove the clip",
                ))
            }
        }
    }

    /// `client` revoked: its upload in flight dropped and every clip of
    /// it deleted. Said in the journal if the folder can't be removed.
    pub fn forget(&mut self, client: &str) {
        self.flights.remove(client);
        if !safe_client(client) {
            return;
        }
        match std::fs::remove_dir_all(self.folder.join(client)) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                eprintln!("lamp-control: removing a revoked computer's clips: {error}");
            }
            _ => {}
        }
    }
}

/// A clip as `sound.upload` and `sound.list` say it.
fn about(name: &str, bytes: u64, frames: u32) -> Value {
    json!({"name": name, "bytes": bytes, "ms": u64::from(frames) * 1_000 / u64::from(CLIP_RATE)})
}

/// A computer's id as a folder's name: lamp-control's own ids are `c_` and
/// hex, never a path.
pub(crate) fn safe_client(client: &str) -> bool {
    !client.is_empty()
        && client.len() <= 64
        && client
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// The voice's checks of a clip, said at upload so a computer learns of a
/// clip the voice would refuse then, not when it plays: 16-bit mono PCM at
/// [`CLIP_RATE`], 1 to [`CLIP_MOST_FRAMES`] frames, no sample past
/// [`PEAK_DBFS`] and no 10 ms louder than [`LOUDEST_DBFS`] RMS. Its frames.
pub fn check_clip(bytes: &[u8]) -> Result<u32, String> {
    let reader = hound::WavReader::new(Cursor::new(bytes)).map_err(|_| "not a WAV file")?;
    let spec = reader.spec();
    if spec.channels != 1
        || spec.bits_per_sample != 16
        || spec.sample_format != hound::SampleFormat::Int
        || spec.sample_rate != CLIP_RATE
    {
        return Err(format!("not 16-bit mono PCM at {CLIP_RATE} Hz"));
    }
    let frames = reader.duration();
    if !(1..=CLIP_MOST_FRAMES).contains(&frames) {
        return Err(format!("not 1 to {CLIP_MOST_FRAMES} frames (30 s at most)"));
    }
    let samples = reader
        .into_samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "a WAV file cut short")?;
    let full = 32_767.0;
    let peak = samples
        .iter()
        .map(|&sample| f64::from(sample).abs())
        .fold(0.0, f64::max);
    if peak > full * 10f64.powf(PEAK_DBFS / 20.0) {
        return Err(format!("a peak past {PEAK_DBFS} dBFS"));
    }
    let loudest = samples
        .chunks(WINDOW)
        .map(|window| {
            let power = window.iter().map(|&s| f64::from(s).powi(2)).sum::<f64>();
            (power / window.len() as f64).sqrt()
        })
        .fold(0.0, f64::max);
    if loudest > full * 10f64.powf(LOUDEST_DBFS / 20.0) {
        return Err(format!("louder than {LOUDEST_DBFS} dBFS RMS over 10 ms"));
    }
    Ok(frames)
}

/// The clip written aside and renamed into place, 0600 in a 0700 folder,
/// so the voice never reads half of one.
fn keep(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(folder) = path.parent() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(folder)?;
    }
    let aside = path.with_extension("new");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&aside)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::rename(&aside, path)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000;

    fn wav(samples: &[i16]) -> Vec<u8> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: CLIP_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut bytes = Cursor::new(Vec::new());
        let mut writer = hound::WavWriter::new(&mut bytes, spec).unwrap();
        for &sample in samples {
            writer.write_sample(sample).unwrap();
        }
        writer.finalize().unwrap();
        bytes.into_inner()
    }

    fn folder(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lamp-control-sounds-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// `bytes` sent as `name` in parts of at most `chunk` bytes.
    fn upload(
        sounds: &mut Sounds,
        client: &str,
        name: &str,
        bytes: &[u8],
        chunk: usize,
        now_us: u64,
    ) -> Result<Step, Error> {
        let sha256 = hex(&Sha256::digest(bytes));
        let chunks: Vec<&[u8]> = bytes.chunks(chunk).collect();
        let mut last = Err(Error::new(ErrorCode::BadRequest, "no parts"));
        for (index, piece) in chunks.iter().enumerate() {
            let data = base64::engine::general_purpose::STANDARD.encode(piece);
            last = Ok(sounds.part(
                client,
                &Part {
                    name,
                    sha256: &sha256,
                    size: bytes.len(),
                    part: index as u32,
                    parts: chunks.len() as u32,
                    data: &data,
                },
                now_us,
            )?);
        }
        last
    }

    /// The voice's own constants, so a clip lamp-control keeps is one the
    /// voice plays, and one it refuses is refused at upload.
    #[test]
    fn the_checks_are_the_voices_own() {
        assert_eq!(CLIP_RATE, lamp_voice::say::SOUND_RATE);
        assert_eq!(CLIP_MOST_FRAMES, lamp_voice::say::CLIP_MOST_FRAMES);
        assert_eq!(CLIP_MOST_BYTES as u64, lamp_voice::say::CLIP_MOST_BYTES);
        assert_eq!(PEAK_DBFS, lamp_voice::say::SOUND_PEAK_DBFS);
        assert_eq!(LOUDEST_DBFS, lamp_voice::say::SOUND_LOUDEST_DBFS);
        assert_eq!(
            lamp_voice::say::DEFAULT_CLIPS,
            format!("/var/lib/lampos-control/{SOUNDS_FOLDER}")
        );
        for samples in [
            vec![1_000_i16; 4_800],
            vec![1_100_i16; 4_800],
            vec![0_i16; CLIP_MOST_FRAMES as usize + 1],
            {
                let mut clicks = vec![0_i16; 4_800];
                clicks[10] = i16::MAX;
                clicks
            },
        ] {
            let bytes = wav(&samples);
            assert_eq!(
                check_clip(&bytes).map(drop),
                lamp_voice::say::decode_clip(&bytes).map(drop),
            );
        }
    }

    #[test]
    fn a_clip_is_uploaded_in_parts_kept_listed_and_removed() {
        let dir = folder("parts");
        let mut sounds = Sounds::new(dir.clone());
        // 20 s at 1,000 (-30.3 dBFS RMS): about 960 KB, in 36 KiB parts.
        let clip = wav(&vec![1_000_i16; CLIP_RATE as usize * 20]);
        let kept = upload(&mut sounds, "c_1", "chime", &clip, 36 * 1024, S).unwrap();
        assert_eq!(
            kept,
            Step::Kept(json!({"name": "chime", "bytes": clip.len(), "ms": 20_000}))
        );
        let path = dir.join("c_1").join("chime.wav");
        assert_eq!(std::fs::read(&path).unwrap(), clip);
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            sounds.list("c_1"),
            json!([{"name": "chime", "bytes": clip.len(), "ms": 20_000}])
        );
        // Another computer's are its own.
        assert_eq!(sounds.list("c_2"), json!([]));
        assert!(sounds.remove("c_2", "chime").is_err());
        sounds.remove("c_1", "chime").unwrap();
        assert_eq!(sounds.list("c_1"), json!([]));
        assert!(
            sounds
                .remove("c_1", "chime")
                .unwrap_err()
                .why
                .starts_with("no clip named chime")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_broken_or_wrong_upload_keeps_nothing() {
        let dir = folder("broken");
        let mut sounds = Sounds::new(dir.clone());
        let clip = wav(&vec![1_000_i16; 4_800]);
        let sha256 = hex(&Sha256::digest(&clip));
        let data = base64::engine::general_purpose::STANDARD.encode(&clip[..100]);
        let part = |part| Part {
            name: "a",
            sha256: &sha256,
            size: clip.len(),
            part,
            parts: 3,
            data: &data,
        };
        // Not from 0, then out of order.
        assert!(sounds.part("c_1", &part(1), S).is_err());
        assert_eq!(sounds.part("c_1", &part(0), S), Ok(Step::Got(1)));
        assert!(
            sounds
                .part("c_1", &part(2), S)
                .unwrap_err()
                .why
                .contains("out of order")
        );
        // Too slow.
        assert_eq!(sounds.part("c_1", &part(0), S), Ok(Step::Got(1)));
        assert!(
            sounds
                .part("c_1", &part(1), S + UPLOAD_WITHIN_US + 1)
                .unwrap_err()
                .why
                .contains("120 s")
        );
        // Not its SHA-256; not a clip the voice plays.
        let wrong = hex(&Sha256::digest(b"other"));
        assert_eq!(
            upload_with(&mut sounds, &clip, &wrong).unwrap_err().why,
            "sha256: not the clip's"
        );
        let loud = wav(&vec![1_100_i16; 4_800]);
        assert_eq!(
            upload(&mut sounds, "c_1", "a", &loud, 4_096, S)
                .unwrap_err()
                .why,
            "clip: louder than -30 dBFS RMS over 10 ms"
        );
        assert_eq!(
            upload(&mut sounds, "c_1", "a", b"not a wav", 4_096, S)
                .unwrap_err()
                .why,
            "clip: not a WAV file"
        );
        assert_eq!(sounds.list("c_1"), json!([]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn upload_with(sounds: &mut Sounds, clip: &[u8], sha256: &str) -> Result<Step, Error> {
        let data = base64::engine::general_purpose::STANDARD.encode(clip);
        sounds.part(
            "c_1",
            &Part {
                name: "a",
                sha256,
                size: clip.len(),
                part: 0,
                parts: 1,
                data: &data,
            },
            S,
        )
    }

    /// A computer keeps at most 64 clips and 20 MB; one of the same name is
    /// replaced, not counted twice.
    #[test]
    fn a_computers_store_is_capped_by_count_and_bytes() {
        let dir = folder("cap");
        let mut sounds = Sounds::new(dir.clone());
        let small = wav(&[1_000_i16; 240]);
        for index in 0..CLIPS_MOST {
            upload(&mut sounds, "c_1", &format!("s{index}"), &small, 4_096, S).unwrap();
        }
        assert!(
            upload(&mut sounds, "c_1", "one-more", &small, 4_096, S)
                .unwrap_err()
                .why
                .contains("64 clips")
        );
        // The same name again replaces it.
        upload(&mut sounds, "c_1", "s0", &small, 4_096, S).unwrap();
        // Another computer has its own store.
        upload(&mut sounds, "c_2", "one-more", &small, 4_096, S).unwrap();
        let _ = std::fs::remove_dir_all(&dir);

        let mut sounds = Sounds::new(dir.clone());
        // 29 s clips, about 1.39 MB each: 14 fit in 20 MB, the 15th doesn't.
        let long = wav(&vec![1_000_i16; CLIP_RATE as usize * 29]);
        for index in 0..14 {
            upload(
                &mut sounds,
                "c_1",
                &format!("l{index}"),
                &long,
                48 * 1024,
                S,
            )
            .unwrap();
        }
        assert!(
            upload(&mut sounds, "c_1", "l14", &long, 48 * 1024, S)
                .unwrap_err()
                .why
                .contains("20000000 bytes")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_revoke_deletes_the_computers_clips_and_its_upload() {
        let dir = folder("revoke");
        let mut sounds = Sounds::new(dir.clone());
        let clip = wav(&[1_000_i16; 240]);
        upload(&mut sounds, "c_1", "a", &clip, 4_096, S).unwrap();
        upload(&mut sounds, "c_2", "a", &clip, 4_096, S).unwrap();
        sounds.forget("c_1");
        assert!(!dir.join("c_1").exists());
        assert_eq!(sounds.list("c_1"), json!([]));
        assert_eq!(sounds.list("c_2").as_array().unwrap().len(), 1);
        // A computer never paired, or forgotten twice: nothing to say.
        sounds.forget("c_1");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
