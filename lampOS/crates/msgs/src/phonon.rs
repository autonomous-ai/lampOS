//! Phonon, the voice that runs on the lamp (#8754, `docs/phonon.md`):
//! Gradium's Phonon 31m, English only, Apache 2.0, spoken by `lamp-phonon`
//! with no key and no cloud. One copy of what its three users share: root's
//! keys helper (`lamp-setup keys`, which keeps the person's choice), the
//! service (which fetches, checks and loads the model) and the voice (which
//! asks it for each sentence over its socket).
//!
//! Only `phonon-31m-en` may ship: Phonon 40m and the multilingual model are
//! CC BY-NC 4.0, and nothing here names them.

/// The service's socket, the voice's way in (its unit's `RuntimeDirectory=`).
pub const SOCKET: &str = "/run/lampos-phonon/phonon.sock";

/// The person's choice, one voice name a line, in the keys helper's folder
/// (`lamp_setup::keys::FOLDER`); the voice and the service read it as the
/// credential `own_phonon.voice`. Absent: Phonon is not chosen, the service
/// idles and the voice speaks as before.
pub const CHOSEN_FILE: &str = "phonon.voice";
/// [`CHOSEN_FILE`] as a unit's credential (`LoadCredential=own:<folder>`).
pub const CHOSEN_CREDENTIAL: &str = "own_phonon.voice";

/// The `keys.get` and `keys.set` service id.
pub const SERVICE: &str = "phonon";
/// The name the app shows for it.
pub const NAME: &str = "On this lamp";

/// The sample rate the service sends, mono: the voice's own
/// (`TTS_SAMPLE_RATE`), so nothing is resampled.
pub const SAMPLE_RATE: u32 = 24_000;

/// The Hugging Face repository the model is fetched from, pinned to one
/// commit (Apache 2.0, read 26 Nov 2026).
pub const MODEL_BASE: &str =
    "https://huggingface.co/gradium/phonon-31m-en/resolve/92741c5b07d085ff840e26fea8ce43110b87852f";
/// The voice catalog (Apache 2.0), pinned to one commit.
pub const VOICES_BASE: &str =
    "https://huggingface.co/gradium/phonon-voices/resolve/da6c9ec0d47be8df5c87c730a3aa97187f3229c3";
/// The folder under the service's state the model and voices are kept in.
pub const MODEL_FOLDER: &str = "phonon-31m-en";

/// One file fetched: its name in [`MODEL_FOLDER`], its path at its source,
/// its size and SHA-256. Kept only when both match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pinned {
    pub name: &'static str,
    pub source: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

/// The model's files, from [`MODEL_BASE`]: the q8 weights the CPU runs.
pub const MODEL_FILES: [Pinned; 3] = [
    Pinned {
        name: "config.json",
        source: "config.json",
        size: 4_634,
        sha256: "b5d0f88353638370d94035b7d73d7d6577c0554d78f8d7ecfd40490aae39ae85",
    },
    Pinned {
        name: "tokenizer.json",
        source: "tokenizer.json",
        size: 277_967,
        sha256: "691a00068fa7b989f2f3e5ae5daa2d1a5bb319f2d5360deafcfb19a8bb6e7936",
    },
    Pinned {
        name: "model.q8.gguf",
        source: "model.q8.gguf",
        size: 64_512_352,
        sha256: "a7d9aad2cd6dc66473f24a08bc6d2939030ef4d751ef82349361fbef3272dbae",
    },
];

/// One English voice offered: the name the app shows and the voice asks
/// by, its catalog id at [`VOICES_BASE`]`/voices/<id>.safetensors`, and the
/// file's SHA-256 (every one is 256 136 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Voice {
    pub name: &'static str,
    pub id: &'static str,
    pub sha256: &'static str,
}

/// The size of every voice file.
pub const VOICE_SIZE: u64 = 256_136;

/// The catalog's American and British English voices whose names are one
/// plain word (24 of 25: Elodie-Rose is left out for its hyphen, the
/// character voices for not being a lamp's), read 26 Nov 2026. The first is
/// the default.
pub const VOICES: [Voice; 24] = [
    Voice {
        name: "Freya",
        id: "GgfEkEJtxZR7gnpy",
        sha256: "eb3cd47418349e618408c16bea7153988b3ad04b41a6033039dd474a9a4dcd23",
    },
    Voice {
        name: "Zoey",
        id: "NbpkqMVS3CJeq2j8",
        sha256: "97c7f8c253413574406ab80dd23f49c3ebb2517163b00a9bf86b9c4cf43e7e23",
    },
    Voice {
        name: "Juniper",
        id: "NIWIOv5vTi5eRMj8",
        sha256: "444378eb06b7c492b86e8fc7d13e4db1a3c0cf3e68891a960bf6f0a41a13ef35",
    },
    Voice {
        name: "Everly",
        id: "vjFyp2BcCyTlUmYb",
        sha256: "958ea94da14f46dd48c7f64adac11ed2ad2aa45f7ccd165dfebab5272664402e",
    },
    Voice {
        name: "Harper",
        id: "4SZHfMpw-p46Ywgs",
        sha256: "6b3da6e2d796a20c51af892c482bd6509fb88267a221aff83c870843dee2a123",
    },
    Voice {
        name: "Hadley",
        id: "bXwrkHhURL1VFJdG",
        sha256: "a879938aa9a2df0a8220bdadef88137929f71e887bd91e7db205bd5fa11661d2",
    },
    Voice {
        name: "Delia",
        id: "nGzMHMPAWGaJFHOo",
        sha256: "a0c3dcb91b2247eca56536be9ae8e8becc1c807b1136c5fe101552d6a155b24a",
    },
    Voice {
        name: "Easton",
        id: "LMDIYE0Oa377Qh6Z",
        sha256: "45c582673cb20edb04ddcca2b5a8865ddfb0a82599efadb269021e87d20fbdd4",
    },
    Voice {
        name: "Micah",
        id: "40QrMVgAxFiUnP9f",
        sha256: "0de1098d1051686a138d068e83ff09deb5053328bc74ec9541a1fdb90bfc24da",
    },
    Voice {
        name: "Trevor",
        id: "0zLApsZfVLLNaEGl",
        sha256: "dc1341c8937af5534e6bbcc5c0fc5e7c10312229b59061228074f93683fe2261",
    },
    Voice {
        name: "Russell",
        id: "_6Aslh2DxfmnRLmP",
        sha256: "beae9e4030efce0a935fcb0f807dc0784d598bcaa87ad4aacb047436b1eaa411",
    },
    Voice {
        name: "Preston",
        id: "qIxlZ7Ei8KvWlyQH",
        sha256: "6e04b84d3ae8751b22bca0a4f63936f9cb648f4f2182b2d0788071ea1c0b68a1",
    },
    Voice {
        name: "Miles",
        id: "d9XCyhjo4JhkHF1d",
        sha256: "eab20e4f159b48cfbc01b1885804e24f681f80e7a64f909182763a4e3361bee8",
    },
    Voice {
        name: "Maeve",
        id: "6PWnV0Nq4wu7RVBT",
        sha256: "56494ee67d533ad227ff0dfd0b1c13ff5aa91f1ad6e596e66c2986ba3005e844",
    },
    Voice {
        name: "Pippa",
        id: "uem82D50GRv2Dwma",
        sha256: "d41a8f01d6e33450f1ce062d332914da10beee99e585dab531750706f187c48a",
    },
    Voice {
        name: "Imogen",
        id: "gDT1nz7Ie36ZhL-C",
        sha256: "1980ab3c35fa98a25b5f3b5e643fd852e14ca8a12a4a26b06aaf1abf2a8c68b1",
    },
    Voice {
        name: "Lottie",
        id: "m9MxpY6IUGXtWp76",
        sha256: "a906ebf17e43fded12b3d6bd077abb2df33bc86eff51c1beeb82e5d416527db3",
    },
    Voice {
        name: "Bryony",
        id: "gfP44RlHwr2kEwk7",
        sha256: "bcc207a73b7885e45b49ab633d3377a10158292e3a7cef9cd6764ad107f1cb9b",
    },
    Voice {
        name: "Alfie",
        id: "X6NceqF285Gr04gb",
        sha256: "4de75497fff3030168e8aa6dca23ffeb332b1bc8c25d0bf3507ceadac953ae9c",
    },
    Voice {
        name: "Hugh",
        id: "YyKcxPG7iIgS3jsK",
        sha256: "3169d74b47a54d1eb15e4c243f56393abe9fe90c7e1c308ecf96ee67618b152a",
    },
    Voice {
        name: "Edwin",
        id: "zUCxzrOrugTSis03",
        sha256: "ff1d0735b8b45710462b1d1f846da040f970c6eb62c7c2f36dbfb8e5bde74f1e",
    },
    Voice {
        name: "Spencer",
        id: "JOW0vchyswkQUMDS",
        sha256: "cc9ac1107b0ffce282374a19beb7bfa95847724e6b7afbac991b4ae4277d5e6d",
    },
    Voice {
        name: "Toby",
        id: "dME3IWyZBvmh1n1q",
        sha256: "09ab315c7f4b72b4d71e66cb756184cedfb1f14d222d096e115bf95bfe7b29dc",
    },
    Voice {
        name: "Teddy",
        id: "Zn7g6NxLqXlJb7Qi",
        sha256: "baea0ea9506f3237a5ebfc61afb7c72dbd189c5603b199ef42fbb00dbda379e0",
    },
];

/// The voice the name gives, in any case; `None` for one not offered.
pub fn voice(name: &str) -> Option<Voice> {
    let name = name.trim();
    VOICES
        .into_iter()
        .find(|voice| voice.name.eq_ignore_ascii_case(name))
}

/// The voices' names, in [`VOICES`]' order: what the app's pick list shows.
pub fn voice_names() -> Vec<&'static str> {
    VOICES.iter().map(|voice| voice.name).collect()
}

/// Whether the lamp's language file's text is English, the only language
/// Phonon 31m speaks: empty or `en`, as the voice reads it.
pub fn speaks(language: &str) -> bool {
    matches!(language.trim(), "" | "en")
}

/// The longest request line the service reads, in bytes.
pub const LINE_MOST: usize = 4_096;

/// One request, one line: `{"say": text, "voice": name}`.
pub fn request(text: &str, voice: &str) -> String {
    let mut line = serde_json::json!({"say": text, "voice": voice}).to_string();
    line.push('\n');
    line
}

/// A request line read: the text and the voice's name, or why not.
pub fn parse_request(line: &str) -> Result<(String, Voice), &'static str> {
    if line.len() > LINE_MOST {
        return Err("the request is too long");
    }
    let value: serde_json::Value =
        serde_json::from_str(line.trim_end()).map_err(|_| "the request is not JSON")?;
    let text = value
        .get("say")
        .and_then(serde_json::Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .ok_or("the request has no text")?;
    let voice = value
        .get("voice")
        .and_then(serde_json::Value::as_str)
        .and_then(voice)
        .ok_or("the voice is not one offered")?;
    Ok((text.to_owned(), voice))
}

/// The answer's framing: each frame a little-endian `u32` count of samples,
/// then that many little-endian `i16` samples; a count of 0 ends the speech,
/// [`FAILED`] says it failed. A closed connection before either is a failure
/// too; the voice closing it stops the speech.
pub const FAILED: u32 = u32::MAX;
/// The most samples one frame carries: one second.
pub const FRAME_MOST: u32 = SAMPLE_RATE;

/// One frame of `samples`, as it goes on the socket.
pub fn frame(samples: &[i16]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(4 + samples.len() * 2);
    bytes.extend_from_slice(&(samples.len() as u32).to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

/// The end of the speech, as it goes on the socket.
pub const END: [u8; 4] = 0u32.to_le_bytes();
/// The failure, as it goes on the socket.
pub const FAIL: [u8; 4] = FAILED.to_le_bytes();

/// A frame's head read: its sample count, the end, or the failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Head {
    Samples(usize),
    End,
    Failed,
}

/// A frame's four-byte head read; a count over [`FRAME_MOST`] is a failure.
pub fn head(bytes: [u8; 4]) -> Head {
    match u32::from_le_bytes(bytes) {
        0 => Head::End,
        n if n <= FRAME_MOST => Head::Samples(n as usize),
        _ => Head::Failed,
    }
}

/// Samples in `[-1, 1]` as the speaker's `i16`, clamped.
pub fn to_i16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|sample| (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every voice offered is a plain word the app's 1-64 letters and digits
    /// rule takes, offered once, with a 16-character catalog id and a
    /// SHA-256; the default is first.
    #[test]
    fn the_voices_are_plain_names_once_each_with_pinned_files() {
        let mut seen = std::collections::BTreeSet::new();
        for voice in VOICES {
            assert!(
                (1..=64).contains(&voice.name.len())
                    && voice.name.bytes().all(|b| b.is_ascii_alphanumeric()),
                "{}",
                voice.name
            );
            assert!(
                seen.insert(voice.name.to_ascii_lowercase()),
                "{} twice",
                voice.name
            );
            assert_eq!(voice.id.len(), 16, "{}", voice.name);
            assert!(
                voice
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
                "{}",
                voice.id
            );
            assert!(sha256_shaped(voice.sha256), "{}", voice.name);
        }
        assert_eq!(VOICES[0].name, "Freya");
        assert_eq!(voice_names().len(), VOICES.len());
        for file in MODEL_FILES {
            assert!(sha256_shaped(file.sha256) && file.size > 0, "{}", file.name);
        }
    }

    /// Only the Apache 2.0 model is ever fetched: the 31m English one, at a
    /// pinned commit, over https.
    #[test]
    fn only_the_apache_model_is_fetched_at_a_pinned_commit() {
        assert!(MODEL_BASE.starts_with("https://huggingface.co/gradium/phonon-31m-en/resolve/"));
        assert!(!MODEL_BASE.contains("40m") && !MODEL_BASE.contains("multilingual"));
        for base in [MODEL_BASE, VOICES_BASE] {
            let commit = base.rsplit('/').next().unwrap();
            assert!(commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn a_voice_is_found_in_any_case_and_only_when_offered() {
        assert_eq!(voice("freya").map(|v| v.name), Some("Freya"));
        assert_eq!(voice(" TOBY ").map(|v| v.name), Some("Toby"));
        for name in ["", "Elodie-Rose", "Sterling", "../Freya", "Kore"] {
            assert_eq!(voice(name), None, "{name}");
        }
    }

    #[test]
    fn phonon_speaks_english_only() {
        for text in ["", "en", "en\n"] {
            assert!(speaks(text), "{text:?}");
        }
        for text in ["vi", "fr", "ja", "zh", "ko", "english"] {
            assert!(!speaks(text), "{text:?}");
        }
    }

    #[test]
    fn a_request_goes_and_comes_back_whole_and_a_bad_one_is_refused() {
        let line = request("Hello \"there\".\nTwo lines.", "harper");
        assert!(line.ends_with('\n') && line.matches('\n').count() == 1);
        let (text, voice) = parse_request(&line).unwrap();
        assert_eq!(text, "Hello \"there\".\nTwo lines.");
        assert_eq!(voice.name, "Harper");
        for line in [
            "not json".to_owned(),
            r#"{"say":"hi"}"#.to_owned(),
            r#"{"say":"  ","voice":"Freya"}"#.to_owned(),
            r#"{"say":"hi","voice":"Kore"}"#.to_owned(),
            request(&"x".repeat(LINE_MOST), "Freya"),
        ] {
            assert!(parse_request(&line).is_err(), "{line}");
        }
    }

    #[test]
    fn frames_carry_samples_and_end_or_fail() {
        let bytes = frame(&[1, -2, i16::MAX]);
        assert_eq!(bytes.len(), 4 + 6);
        assert_eq!(head(bytes[..4].try_into().unwrap()), Head::Samples(3));
        assert_eq!(i16::from_le_bytes([bytes[6], bytes[7]]), -2);
        assert_eq!(head(END), Head::End);
        assert_eq!(head(FAIL), Head::Failed);
        assert_eq!(head((FRAME_MOST + 1).to_le_bytes()), Head::Failed);
        assert_eq!(to_i16(&[0.0, 2.0, -2.0]), [0, i16::MAX, -i16::MAX]);
    }

    fn sha256_shaped(text: &str) -> bool {
        text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    }
}
