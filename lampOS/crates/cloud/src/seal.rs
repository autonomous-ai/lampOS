//! A secret sealed to the app's one-time key, so only that app can open it
//! (#5299's option (a), docs/cloud.md, "The developer door"): HPKE
//! (RFC 9180) in its base mode, single shot, with the suite
//! DHKEM(X25519, HKDF-SHA256), HKDF-SHA256 and ChaCha20-Poly1305 (ids
//! 0x0020, 0x0001 and 0x0003). The app opens it with a stock HPKE library
//! (CryptoKit's `HPKE` on iOS 17 and later, Tink's `HpkeDecrypt` on
//! Android), so the framing is the standard's, not ours.
//!
//! Built from `ring` alone, the crate the lamp's TLS already uses: an
//! ephemeral X25519 key for the KEM, HMAC-SHA256 for the labeled extract
//! (HKDF-Extract is HMAC with the salt as its key), `hkdf` for the labeled
//! expand, and ChaCha20-Poly1305 for the one message. Only sequence 0 is
//! ever sealed: one message per key, so the nonce is the base nonce.
use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, Nonce, UnboundKey};
use ring::agreement::{self, EphemeralPrivateKey, UnparsedPublicKey, X25519};
use ring::hkdf::{self, HKDF_SHA256, KeyType};
use ring::hmac;
use ring::rand::SystemRandom;

/// RFC 9180's version label, before every labeled extract and expand.
const VERSION: &[u8] = b"HPKE-v1";
/// The KEM's own suite id: `"KEM"` and DHKEM(X25519, HKDF-SHA256)'s 0x0020.
const KEM_SUITE: &[u8] = b"KEM\x00\x20";
/// The whole suite's id: `"HPKE"`, then the KEM's, the KDF's and the
/// AEAD's ids, two bytes each.
const HPKE_SUITE: &[u8] = b"HPKE\x00\x20\x00\x01\x00\x03";
/// The base mode: no pre-shared key, no sender key.
const MODE_BASE: u8 = 0x00;
/// An X25519 public key, the app's or the lamp's one-time one (`enc`).
pub const KEY_BYTES: usize = 32;
/// ChaCha20-Poly1305's key, and HKDF-SHA256's output, in bytes.
const SECRET_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;

/// What the app is sent: the lamp's one-time public key and the sealed
/// message with its tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sealed {
    pub enc: [u8; KEY_BYTES],
    pub ct: Vec<u8>,
}

/// Why nothing was sealed. Never carries the message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The app's key is no X25519 public key, or the DH gave all zeros (a
    /// small-order point, RFC 9180's 7.1.4 says to abort).
    BadKey,
    /// The system's random source or the cipher failed.
    Failed,
}

/// `pt` sealed to the app's X25519 public key `pk_r`, under `info` and
/// `aad`: HPKE's `SealBase` with a new ephemeral key of the lamp's, from the
/// system's random source.
pub fn seal(pk_r: &[u8; KEY_BYTES], info: &[u8], aad: &[u8], pt: &[u8]) -> Result<Sealed, Refused> {
    let random = SystemRandom::new();
    let ephemeral = EphemeralPrivateKey::generate(&X25519, &random).map_err(|_| Refused::Failed)?;
    let enc: [u8; KEY_BYTES] = ephemeral
        .compute_public_key()
        .map_err(|_| Refused::Failed)?
        .as_ref()
        .try_into()
        .map_err(|_| Refused::Failed)?;
    let shared_secret = agreement::agree_ephemeral(
        ephemeral,
        &UnparsedPublicKey::new(&X25519, pk_r),
        |dh| -> Result<[u8; SECRET_BYTES], Refused> {
            let dh: &[u8; KEY_BYTES] = dh.try_into().map_err(|_| Refused::BadKey)?;
            if dh.iter().all(|byte| *byte == 0) {
                return Err(Refused::BadKey);
            }
            Ok(extract_and_expand(dh, &enc, pk_r))
        },
    )
    .map_err(|_| Refused::BadKey)??;
    let schedule = key_schedule(&shared_secret, info);
    let ct = seal_with(&schedule, aad, pt)?;
    Ok(Sealed { enc, ct })
}

/// The key schedule's three outputs, for one context.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Schedule {
    pub key: [u8; SECRET_BYTES],
    pub base_nonce: [u8; NONCE_BYTES],
    pub exporter_secret: [u8; SECRET_BYTES],
}

/// DHKEM's `ExtractAndExpand(dh, kem_context)`, `kem_context` being `enc`
/// then `pk_r`: the shared secret.
pub(crate) fn extract_and_expand(
    dh: &[u8; KEY_BYTES],
    enc: &[u8; KEY_BYTES],
    pk_r: &[u8; KEY_BYTES],
) -> [u8; SECRET_BYTES] {
    let eae_prk = labeled_extract(KEM_SUITE, b"", b"eae_prk", dh);
    let mut shared_secret = [0u8; SECRET_BYTES];
    labeled_expand(
        KEM_SUITE,
        &eae_prk,
        b"shared_secret",
        &[enc, pk_r],
        &mut shared_secret,
    );
    shared_secret
}

/// RFC 9180's `KeySchedule` in the base mode: no PSK and no PSK id.
pub(crate) fn key_schedule(shared_secret: &[u8; SECRET_BYTES], info: &[u8]) -> Schedule {
    let psk_id_hash = labeled_extract(HPKE_SUITE, b"", b"psk_id_hash", b"");
    let info_hash = labeled_extract(HPKE_SUITE, b"", b"info_hash", info);
    let context: Vec<u8> = [&[MODE_BASE][..], &psk_id_hash, &info_hash].concat();
    let secret = labeled_extract(HPKE_SUITE, shared_secret, b"secret", b"");
    let mut schedule = Schedule {
        key: [0; SECRET_BYTES],
        base_nonce: [0; NONCE_BYTES],
        exporter_secret: [0; SECRET_BYTES],
    };
    labeled_expand(HPKE_SUITE, &secret, b"key", &[&context], &mut schedule.key);
    labeled_expand(
        HPKE_SUITE,
        &secret,
        b"base_nonce",
        &[&context],
        &mut schedule.base_nonce,
    );
    labeled_expand(
        HPKE_SUITE,
        &secret,
        b"exp",
        &[&context],
        &mut schedule.exporter_secret,
    );
    schedule
}

/// The context's `Seal` at sequence 0: `pt` under `aad`, its tag after it.
pub(crate) fn seal_with(schedule: &Schedule, aad: &[u8], pt: &[u8]) -> Result<Vec<u8>, Refused> {
    let key = LessSafeKey::new(
        UnboundKey::new(&CHACHA20_POLY1305, &schedule.key).map_err(|_| Refused::Failed)?,
    );
    let mut sealed = pt.to_vec();
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(schedule.base_nonce),
        Aad::from(aad),
        &mut sealed,
    )
    .map_err(|_| Refused::Failed)?;
    Ok(sealed)
}

/// `LabeledExtract(salt, label, ikm)`: HKDF-Extract, which is HMAC-SHA256
/// keyed with the salt, over the version, the suite id, the label and `ikm`.
fn labeled_extract(suite: &[u8], salt: &[u8], label: &[u8], ikm: &[u8]) -> [u8; SECRET_BYTES] {
    let key = hmac::Key::new(hmac::HMAC_SHA256, salt);
    let mut context = hmac::Context::with_key(&key);
    for part in [VERSION, suite, label, ikm] {
        context.update(part);
    }
    context
        .sign()
        .as_ref()
        .try_into()
        .expect("SHA-256 is 32 bytes")
}

/// `LabeledExpand(prk, label, info, L)`, `L` being `out`'s length: HKDF-Expand
/// with `L` as two bytes, the version, the suite id, the label and `info`.
fn labeled_expand(
    suite: &[u8],
    prk: &[u8; SECRET_BYTES],
    label: &[u8],
    info: &[&[u8]],
    out: &mut [u8],
) {
    let length = u16::try_from(out.len())
        .expect("a short output")
        .to_be_bytes();
    let mut parts: Vec<&[u8]> = vec![&length, VERSION, suite, label];
    parts.extend_from_slice(info);
    // The PRK here is a labeled extract's output: a full-length HMAC.
    hkdf::Prk::new_less_safe(HKDF_SHA256, prk)
        .expand(&parts, Length(out.len()))
        .and_then(|okm| okm.fill(out))
        .expect("at most 255 blocks of HKDF-SHA256");
}

/// An HKDF output length, for `ring`'s expand.
struct Length(usize);

impl KeyType for Length {
    fn len(&self) -> usize {
        self.0
    }
}

/// The app's side, for tests only: HPKE's `OpenBase` with the app's private
/// key `sk_r`, the X25519 done with `curve25519-dalek` (a test-only
/// dependency), since `ring` can't load a fixed private key.
#[cfg(test)]
pub(crate) fn open(
    sk_r: &[u8; KEY_BYTES],
    sealed: &Sealed,
    info: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, ()> {
    use curve25519_dalek::montgomery::MontgomeryPoint;
    let pk_r = MontgomeryPoint::mul_base_clamped(*sk_r).to_bytes();
    let dh = MontgomeryPoint(sealed.enc).mul_clamped(*sk_r).to_bytes();
    if dh.iter().all(|byte| *byte == 0) {
        return Err(());
    }
    let schedule = key_schedule(&extract_and_expand(&dh, &sealed.enc, &pk_r), info);
    let key = LessSafeKey::new(UnboundKey::new(&CHACHA20_POLY1305, &schedule.key).map_err(|_| ())?);
    let mut opened = sealed.ct.clone();
    let pt = key
        .open_in_place(
            Nonce::assume_unique_for_key(schedule.base_nonce),
            Aad::from(aad),
            &mut opened,
        )
        .map_err(|_| ())?;
    Ok(pt.to_vec())
}

/// For tests: an X25519 public key from its private key.
#[cfg(test)]
pub(crate) fn public_of(sk: &[u8; KEY_BYTES]) -> [u8; KEY_BYTES] {
    curve25519_dalek::montgomery::MontgomeryPoint::mul_base_clamped(*sk).to_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        let text: String = text.split_whitespace().collect();
        (0..text.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap())
            .collect()
    }

    fn hex32(text: &str) -> [u8; 32] {
        hex(text).try_into().unwrap()
    }

    // RFC 9180, Appendix A.2.1 ("Base Setup Information" for
    // DHKEM(X25519, HKDF-SHA256), HKDF-SHA256, ChaCha20Poly1305) and
    // A.2.1.1 ("Encryptions", sequence number 0), copied from
    // https://www.rfc-editor.org/rfc/rfc9180.txt.
    const INFO: &str = "4f6465206f6e2061204772656369616e2055726e";
    const SK_EM: &str = "f4ec9b33b792c372c1d2c2063507b684ef925b8c75a42dbcbf57d63ccd381600";
    const PK_RM: &str = "4310ee97d88cc1f088a5576c77ab0cf5c3ac797f3d95139c6c84b5429c59662a";
    const SK_RM: &str = "8057991eef8f1f1af18f4a9491d16a1ce333f695d4db8e38da75975c4478e0fb";
    const ENC: &str = "1afa08d3dec047a643885163f1180476fa7ddb54c6a8029ea33f95796bf2ac4a";
    const SHARED_SECRET: &str = "0bbe78490412b4bbea4812666f7916932b828bba79942424abb65244930d69a7";
    const KEY: &str = "ad2744de8e17f4ebba575b3f5f5a8fa1f69c2a07f6e7500bc60ca6e3e3ec1c91";
    const BASE_NONCE: &str = "5c4d98150661b848853b547f";
    const EXPORTER_SECRET: &str =
        "a3b010d4994890e2c6968a36f64470d3c824c8f5029942feb11e7a74b2921922";
    const PT: &str = "4265617574792069732074727574682c20747275746820626561757479";
    const AAD_0: &str = "436f756e742d30";
    const CT_0: &str = "1c5250d8034ec2b784ba2cfd69dbdb8af406cfe3ff938e131f0def8c8b60b4db
                        21993c62ce81883d2dd1b51a28";

    /// The key schedule from the vector's shared secret gives its key, base
    /// nonce and exporter secret, and sequence 0's seal gives its `ct`.
    /// The lamp's own DH can't be run on the vector's `skEm`: `ring` makes
    /// X25519 keys only from its own random source and loads no fixed one,
    /// so that step is `ring`'s, tested there; the KEM's extract and expand
    /// is checked below from the DH done by `curve25519-dalek`.
    #[test]
    fn the_rfc_9180_a_2_1_vector_holds() {
        let schedule = key_schedule(&hex32(SHARED_SECRET), &hex(INFO));
        assert_eq!(schedule.key, hex32(KEY));
        assert_eq!(schedule.base_nonce.to_vec(), hex(BASE_NONCE));
        assert_eq!(schedule.exporter_secret, hex32(EXPORTER_SECRET));
        assert_eq!(
            seal_with(&schedule, &hex(AAD_0), &hex(PT)).unwrap(),
            hex(CT_0)
        );
    }

    /// The KEM's extract and expand from the vector's keys: the DH of
    /// `skEm` and `pkRm` (by `curve25519-dalek`) gives its shared secret,
    /// and `skRm` opens sequence 0's `ct` at `enc`.
    #[test]
    fn the_vectors_kem_and_open_hold() {
        use curve25519_dalek::montgomery::MontgomeryPoint;
        let enc = hex32(ENC);
        assert_eq!(public_of(&hex32(SK_EM)), enc);
        assert_eq!(public_of(&hex32(SK_RM)), hex32(PK_RM));
        let dh = MontgomeryPoint(hex32(PK_RM))
            .mul_clamped(hex32(SK_EM))
            .to_bytes();
        assert_eq!(
            extract_and_expand(&dh, &enc, &hex32(PK_RM)),
            hex32(SHARED_SECRET)
        );
        let sealed = Sealed { enc, ct: hex(CT_0) };
        assert_eq!(
            open(&hex32(SK_RM), &sealed, &hex(INFO), &hex(AAD_0)),
            Ok(hex(PT))
        );
    }

    /// Sealed by the lamp, opened by the app's private key; anything changed
    /// on the way fails to open.
    #[test]
    fn what_is_sealed_opens_only_as_it_was_sealed() {
        let sk = [7u8; 32];
        let pk = public_of(&sk);
        let info = b"lampos/v1/control.code";
        let aad = b"d-42\nr-1";
        let sealed = seal(&pk, info, aad, b"a secret").unwrap();
        assert_eq!(sealed.ct.len(), b"a secret".len() + ring::aead::MAX_TAG_LEN);
        assert_eq!(open(&sk, &sealed, info, aad), Ok(b"a secret".to_vec()));
        // A new ephemeral key each time.
        assert_ne!(seal(&pk, info, aad, b"a secret").unwrap().enc, sealed.enc);

        let mut ct = sealed.clone();
        ct.ct[0] ^= 1;
        assert_eq!(open(&sk, &ct, info, aad), Err(()), "ct changed");
        let mut tag = sealed.clone();
        *tag.ct.last_mut().unwrap() ^= 1;
        assert_eq!(open(&sk, &tag, info, aad), Err(()), "tag changed");
        let mut enc = sealed.clone();
        enc.enc[0] ^= 1;
        assert_eq!(open(&sk, &enc, info, aad), Err(()), "enc changed");
        assert_eq!(
            open(&sk, &sealed, info, b"d-42\nr-2"),
            Err(()),
            "another request"
        );
        assert_eq!(
            open(&sk, &sealed, info, b"d-43\nr-1"),
            Err(()),
            "another lamp"
        );
        assert_eq!(
            open(&sk, &sealed, b"lampos/v1/other", aad),
            Err(()),
            "another info"
        );
        assert_eq!(open(&[8u8; 32], &sealed, info, aad), Err(()), "another key");
    }

    /// A small-order point (here all zeros) gives an all-zero DH: refused,
    /// as RFC 9180's 7.1.4 asks.
    #[test]
    fn a_small_order_key_is_refused() {
        assert_eq!(seal(&[0u8; 32], b"i", b"a", b"p"), Err(Refused::BadKey));
    }
}
