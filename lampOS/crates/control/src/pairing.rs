//! Pairing and `hello` (docs/control.md, "Pairing"): who may call, proved
//! with keys, never by a caller's word.
//!
//! - Every connection opens with the lamp's `welcome`, carrying a fresh
//!   32-byte nonce ([`nonce`]).
//! - **Pair, once per computer:** the owner's app gets a one-time code of 8
//!   digits from the lamp ([`Codes::issue`], 5 minutes), **with the pin of
//!   the lamp's own certificate** (its SHA-256). The computer checks the
//!   TLS certificate against that pin **before it sends any frame** (only
//!   the TLS handshake and the WebSocket's upgrade request go first), then
//!   proves it holds the code without sending it: `proof` is HMAC-SHA256,
//!   keyed with the code's 8 ASCII digits, over [`PAIR_CONTEXT`], the pin's
//!   32 bytes, its Ed25519 public key's 32 bytes and the nonce
//!   ([`pair_proof`]). A code serves one pairing.
//! - **The attacker it is built against** (the Reviewer-lead's and
//!   Fourteen's P2 on #3736): a host on the home network that answers the
//!   computer's first connection, as `<lamp>.local`, before the real lamp.
//!   An 8-digit code is short enough to search offline from one `pair`
//!   frame, so the frame must never reach such a host: the pin from the
//!   app, checked before `pair`, keeps it from ever being sent there, and
//!   the pin inside the proof makes a proof sent through a host in the
//!   middle fail at the real lamp. On the lamp itself, tries are held to
//!   one each [`TRY_EVERY_US`] across every connection, and a wrong proof
//!   closes its connection: 300 tries in a code's 5 minutes, against 10^8
//!   codes. Junk proofs don't void the owner's code.
//! - **Hello, every connection:** the client signs [`HELLO_CONTEXT`] and the
//!   nonce with its key ([`hello_holds`]).
//!
//! The context prefixes keep a signature or proof made for one step from
//! being replayed as another (blue and Nine on #3504, 27 Oct).

use std::collections::BTreeSet;
use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ring::{hmac, rand::SecureRandom, signature};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::protocol::Grant;

/// What `pair`'s proof is made over first.
pub const PAIR_CONTEXT: &[u8] = b"lampos/v1/pair\0";
/// What `hello`'s signature is made over first.
pub const HELLO_CONTEXT: &[u8] = b"lampos/v1/hello\0";
/// How long a code serves, in µs.
pub const CODE_FOR_US: u64 = 5 * 60 * 1_000_000;
/// The least time between two pairing tries, across every connection.
pub const TRY_EVERY_US: u64 = 1_000_000;
/// How many computers one lamp keeps paired.
pub const CLIENTS_MAX: usize = 16;

/// A fresh nonce for a connection's `welcome`.
pub fn nonce(random: &dyn SecureRandom) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    random.fill(&mut bytes).expect("the system's random source");
    bytes
}

/// `pair`'s proof for `code`, the lamp certificate's `pin`, `key` and
/// `nonce`.
pub fn pair_proof(code: &str, pin: &[u8; 32], key: &[u8; 32], nonce: &[u8; 32]) -> [u8; 32] {
    let tag = hmac::sign(
        &hmac::Key::new(hmac::HMAC_SHA256, code.as_bytes()),
        &[PAIR_CONTEXT, pin, key, nonce].concat(),
    );
    tag.as_ref().try_into().expect("HMAC-SHA256 is 32 bytes")
}

/// Whether `sig` is `key`'s Ed25519 signature over [`HELLO_CONTEXT`] and
/// `nonce`.
pub fn hello_holds(key: &[u8; 32], nonce: &[u8; 32], sig: &[u8]) -> bool {
    signature::UnparsedPublicKey::new(&signature::ED25519, key)
        .verify(&[HELLO_CONTEXT, nonce].concat(), sig)
        .is_ok()
}

/// A paired client's id: `c_` and the first 16 hex digits of its key's
/// SHA-256, so the same key always has the same id.
pub fn client_id(key: &[u8; 32]) -> String {
    let digest = Sha256::digest(key);
    let hex: String = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("c_{hex}")
}

/// Base64 to exactly `N` bytes.
pub fn bytes_of<const N: usize>(text: &str) -> Option<[u8; N]> {
    STANDARD.decode(text).ok()?.try_into().ok()
}

/// The one-time code the owner's app asked for, if one is out.
#[derive(Debug, Default)]
pub struct Codes {
    out: Option<Code>,
    tried_us: Option<u64>,
}

#[derive(Debug)]
struct Code {
    digits: String,
    until_us: u64,
}

/// Why a pairing was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PairRefused {
    /// No code out, or it ran out, was used or was voided.
    NoCode,
    /// The proof doesn't hold for the code out.
    WrongProof,
    /// Another try came less than [`TRY_EVERY_US`] ago: try again in this
    /// many µs.
    TooSoon(u64),
    /// The lamp keeps [`CLIENTS_MAX`] already.
    Full,
}

impl Codes {
    pub fn new() -> Self {
        Self::default()
    }

    /// A new code of 8 digits for `CODE_FOR_US`; any earlier one is void.
    pub fn issue(&mut self, random: &dyn SecureRandom, now_us: u64) -> String {
        // Uniform over 10^8: draw again past the largest multiple of it.
        let digits = loop {
            let mut bytes = [0u8; 4];
            random.fill(&mut bytes).expect("the system's random source");
            let n = u32::from_le_bytes(bytes);
            if n < 4_200_000_000 {
                break format!("{:08}", n % 100_000_000);
            }
        };
        self.out = Some(Code {
            digits: digits.clone(),
            until_us: now_us + CODE_FOR_US,
        });
        digits
    }

    /// Whether `proof` holds for the code out, the lamp's `pin`, `key` and
    /// `nonce`: the code is spent if it does. One try each
    /// [`TRY_EVERY_US`], whatever its outcome.
    pub fn redeem(
        &mut self,
        pin: &[u8; 32],
        key: &[u8; 32],
        nonce: &[u8; 32],
        proof: &[u8],
        now_us: u64,
    ) -> Result<(), PairRefused> {
        if let Some(tried) = self.tried_us
            && now_us < tried + TRY_EVERY_US
        {
            return Err(PairRefused::TooSoon(tried + TRY_EVERY_US - now_us));
        }
        self.tried_us = Some(now_us);
        let Some(code) = self.out.as_ref().filter(|code| code.until_us > now_us) else {
            self.out = None;
            return Err(PairRefused::NoCode);
        };
        let tag_key = hmac::Key::new(hmac::HMAC_SHA256, code.digits.as_bytes());
        if hmac::verify(&tag_key, &[PAIR_CONTEXT, pin, key, nonce].concat(), proof).is_ok() {
            self.out = None;
            return Ok(());
        }
        Err(PairRefused::WrongProof)
    }
}

/// A paired computer, as kept on the lamp.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Client {
    pub id: String,
    pub name: String,
    /// Its Ed25519 public key, base64.
    pub key: String,
    pub grants: BTreeSet<Grant>,
    /// Seconds since the epoch, by the lamp's wall clock.
    pub paired_at: u64,
}

/// Every paired computer, kept as one JSON file the lamp's root owns
/// (`/var/lib/lampos-control/clients.json`, 0600).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clients {
    pub clients: Vec<Client>,
}

impl Clients {
    /// The clients kept at `path`; none if it isn't there yet.
    pub fn load(path: &Path) -> std::io::Result<Self> {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }

    /// Writes them whole beside `path`, synced, and renames it over, so a
    /// power cut leaves the old file or the new one; 0600.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt as _;
        let mut temporary = path.as_os_str().to_owned();
        temporary.push(".new");
        let temporary = std::path::PathBuf::from(temporary);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)?;
        if let Some(parent) = path.parent() {
            std::fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    }

    /// The client with `id`.
    pub fn get(&self, id: &str) -> Option<&Client> {
        self.clients.iter().find(|client| client.id == id)
    }

    /// Whether `key` could be added: paired already, or room for one more.
    pub fn has_room_for(&self, key: &[u8; 32]) -> bool {
        self.get(&client_id(key)).is_some() || self.clients.len() < CLIENTS_MAX
    }

    /// Adds (or, for a key paired before, replaces) a client: its id, and
    /// the entry it replaced, to put back if keeping it fails.
    pub fn add(
        &mut self,
        name: &str,
        key: &[u8; 32],
        grants: BTreeSet<Grant>,
        paired_at: u64,
    ) -> Result<(String, Option<Client>), PairRefused> {
        if !self.has_room_for(key) {
            return Err(PairRefused::Full);
        }
        let id = client_id(key);
        let before = self
            .clients
            .iter()
            .position(|client| client.id == id)
            .map(|at| self.clients.remove(at));
        self.clients.push(Client {
            id: id.clone(),
            name: name.to_owned(),
            key: STANDARD.encode(key),
            grants,
            paired_at,
        });
        Ok((id, before))
    }

    /// Puts back what an [`Clients::add`] replaced (or removes what it added),
    /// when keeping the change failed.
    pub fn undo_add(&mut self, id: &str, before: Option<Client>) {
        self.clients.retain(|client| client.id != id);
        if let Some(before) = before {
            self.clients.push(before);
        }
    }

    /// Removes a client: whether it was there.
    pub fn revoke(&mut self, id: &str) -> bool {
        let before = self.clients.len();
        self.clients.retain(|client| client.id != id);
        self.clients.len() != before
    }

    /// The client `id` names, if its `sig` over `nonce` holds.
    pub fn hello(&self, id: &str, nonce: &[u8; 32], sig: &str) -> Option<&Client> {
        let client = self.get(id)?;
        let key: [u8; 32] = bytes_of(&client.key)?;
        let sig: [u8; 64] = bytes_of(sig)?;
        hello_holds(&key, nonce, &sig).then_some(client)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::KeyPair as _;

    const S: u64 = 1_000_000;

    fn pair_of_keys(seed: u8) -> signature::Ed25519KeyPair {
        signature::Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).unwrap()
    }

    fn public(keys: &signature::Ed25519KeyPair) -> [u8; 32] {
        keys.public_key().as_ref().try_into().unwrap()
    }

    const PIN: [u8; 32] = [7; 32];

    #[test]
    fn a_code_pairs_once_within_its_minutes_and_only_with_its_proof_over_this_lamps_pin() {
        let random = ring::rand::SystemRandom::new();
        let mut codes = Codes::new();
        let key = public(&pair_of_keys(1));
        let nonce = nonce(&random);
        assert_eq!(
            codes.redeem(&PIN, &key, &nonce, &[0; 32], 0),
            Err(PairRefused::NoCode)
        );
        let code = codes.issue(&random, 0);
        assert_eq!(code.len(), 8);
        assert!(code.bytes().all(|b| b.is_ascii_digit()));
        // Over another nonce, for another code, or made for another lamp's
        // certificate (a host in the middle): wrong.
        let other = [9u8; 32];
        assert_eq!(
            codes.redeem(
                &PIN,
                &key,
                &nonce,
                &pair_proof(&code, &PIN, &key, &other),
                S
            ),
            Err(PairRefused::WrongProof)
        );
        assert_eq!(
            codes.redeem(
                &PIN,
                &key,
                &nonce,
                &pair_proof("00000000", &PIN, &key, &nonce),
                2 * S
            ),
            Err(PairRefused::WrongProof)
        );
        assert_eq!(
            codes.redeem(
                &PIN,
                &key,
                &nonce,
                &pair_proof(&code, &other, &key, &nonce),
                3 * S
            ),
            Err(PairRefused::WrongProof)
        );
        assert_eq!(
            codes.redeem(
                &PIN,
                &key,
                &nonce,
                &pair_proof(&code, &PIN, &key, &nonce),
                4 * S
            ),
            Ok(())
        );
        // Spent.
        assert_eq!(
            codes.redeem(
                &PIN,
                &key,
                &nonce,
                &pair_proof(&code, &PIN, &key, &nonce),
                5 * S
            ),
            Err(PairRefused::NoCode)
        );
        // Run out.
        let code = codes.issue(&random, 0);
        assert_eq!(
            codes.redeem(
                &PIN,
                &key,
                &nonce,
                &pair_proof(&code, &PIN, &key, &nonce),
                CODE_FOR_US
            ),
            Err(PairRefused::NoCode)
        );
    }

    #[test]
    fn tries_are_one_a_second_across_connections_and_junk_never_voids_the_code() {
        let random = ring::rand::SystemRandom::new();
        let mut codes = Codes::new();
        let key = public(&pair_of_keys(1));
        let nonce = nonce(&random);
        let code = codes.issue(&random, 0);
        assert_eq!(
            codes.redeem(&PIN, &key, &nonce, &[0; 32], S),
            Err(PairRefused::WrongProof)
        );
        // A second try within the second, from anyone, isn't even checked.
        assert_eq!(
            codes.redeem(
                &PIN,
                &key,
                &nonce,
                &pair_proof(&code, &PIN, &key, &nonce),
                S + 400_000
            ),
            Err(PairRefused::TooSoon(600_000))
        );
        // Many junk proofs, a second apart, leave the owner's code standing.
        for n in 2..50 {
            assert_eq!(
                codes.redeem(&PIN, &key, &nonce, &[0; 32], n * S),
                Err(PairRefused::WrongProof)
            );
        }
        assert_eq!(
            codes.redeem(
                &PIN,
                &key,
                &nonce,
                &pair_proof(&code, &PIN, &key, &nonce),
                50 * S
            ),
            Ok(())
        );
        // A new code voids the old one.
        let first = codes.issue(&random, 0);
        let second = codes.issue(&random, 0);
        if first != second {
            assert_eq!(
                codes.redeem(
                    &PIN,
                    &key,
                    &nonce,
                    &pair_proof(&first, &PIN, &key, &nonce),
                    60 * S
                ),
                Err(PairRefused::WrongProof)
            );
        }
        assert_eq!(
            codes.redeem(
                &PIN,
                &key,
                &nonce,
                &pair_proof(&second, &PIN, &key, &nonce),
                61 * S
            ),
            Ok(())
        );
    }

    #[test]
    fn hello_holds_only_for_the_paired_key_over_this_connections_nonce() {
        let random = ring::rand::SystemRandom::new();
        let keys = pair_of_keys(1);
        let key = public(&keys);
        let mut clients = Clients::default();
        let id = clients
            .add(
                "Dev's laptop",
                &key,
                [Grant::Body, Grant::Light].into(),
                1_793_000_000,
            )
            .unwrap()
            .0;
        assert_eq!(id, client_id(&key));
        assert!(id.starts_with("c_") && id.len() == 18);
        let nonce = nonce(&random);
        let sig = STANDARD.encode(keys.sign(&[HELLO_CONTEXT, &nonce].concat()));
        assert_eq!(
            clients.hello(&id, &nonce, &sig).map(|c| c.name.as_str()),
            Some("Dev's laptop")
        );
        // Another connection's nonce, no context, another key, or no such id.
        assert!(clients.hello(&id, &[7u8; 32], &sig).is_none());
        let bare = STANDARD.encode(keys.sign(&nonce));
        assert!(clients.hello(&id, &nonce, &bare).is_none());
        let stranger = STANDARD.encode(pair_of_keys(2).sign(&[HELLO_CONTEXT, &nonce].concat()));
        assert!(clients.hello(&id, &nonce, &stranger).is_none());
        assert!(clients.hello("c_0000000000000000", &nonce, &sig).is_none());
        // A pair proof can't stand in for a hello signature.
        assert!(!hello_holds(
            &key,
            &nonce,
            &pair_proof("12345678", &PIN, &key, &nonce)
        ));
        assert!(clients.revoke(&id));
        assert!(clients.hello(&id, &nonce, &sig).is_none());
        assert!(!clients.revoke(&id));
    }

    #[test]
    fn the_clients_file_is_kept_whole_and_private() {
        use std::os::unix::fs::PermissionsExt as _;
        let directory =
            std::env::temp_dir().join(format!("lamp-control-clients-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("clients.json");
        assert_eq!(Clients::load(&path).unwrap(), Clients::default());
        let mut clients = Clients::default();
        clients
            .add("a", &public(&pair_of_keys(1)), [Grant::Speak].into(), 1)
            .unwrap();
        // The same key pairing again replaces itself.
        clients
            .add(
                "a again",
                &public(&pair_of_keys(1)),
                [Grant::Body].into(),
                2,
            )
            .unwrap();
        assert_eq!(clients.clients.len(), 1);
        clients.save(&path).unwrap();
        assert_eq!(Clients::load(&path).unwrap(), clients);
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::write(&path, b"{\"clients\":[],\"extra\":1}").unwrap();
        assert!(Clients::load(&path).is_err());
        for seed in 2..=CLIENTS_MAX as u8 {
            clients
                .add("x", &public(&pair_of_keys(seed)), BTreeSet::new(), 3)
                .unwrap();
        }
        assert_eq!(
            clients.add(
                "one too many",
                &public(&pair_of_keys(200)),
                BTreeSet::new(),
                4
            ),
            Err(PairRefused::Full)
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_failed_keep_puts_back_the_entry_a_re_pairing_replaced() {
        let mut clients = Clients::default();
        let key = public(&pair_of_keys(1));
        let (id, before) = clients
            .add("first", &key, [Grant::Speak].into(), 1)
            .unwrap();
        assert!(before.is_none());
        let (same, before) = clients
            .add("second", &key, [Grant::Body].into(), 2)
            .unwrap();
        assert_eq!(same, id);
        clients.undo_add(&id, before);
        assert_eq!(clients.get(&id).map(|c| c.name.as_str()), Some("first"));
        let (fresh, before) = clients
            .add("other", &public(&pair_of_keys(2)), BTreeSet::new(), 3)
            .unwrap();
        clients.undo_add(&fresh, before);
        assert!(clients.get(&fresh).is_none());
    }

    #[test]
    fn base64_reads_only_exact_lengths() {
        assert!(bytes_of::<32>(&STANDARD.encode([1u8; 32])).is_some());
        assert!(bytes_of::<32>(&STANDARD.encode([1u8; 31])).is_none());
        assert!(bytes_of::<32>("not base64!").is_none());
    }
}
