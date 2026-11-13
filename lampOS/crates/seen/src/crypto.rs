//! V1's encryption for the perception service, ported byte for byte from
//! `hal/drivers/sensing/crypto.py` on `main` (#6543), so the server needs
//! no change:
//!
//! - a session key of 32 random bytes, AES-256-GCM;
//! - the session key sent RSA-encrypted under the server's public key, with
//!   OAEP: SHA-256 as the hash and in MGF1, no label;
//! - each message under a fresh 12-byte random nonce and no associated
//!   data; `cipher_data` is the ciphertext with GCM's 16-byte tag after it,
//!   as Python's `AESGCM.encrypt` returns it;
//! - every binary field standard Base64 with padding;
//! - the same JSON envelopes: an HTTP request
//!   `{"nonce","cipher_data","encrypted_key"}`, an HTTP response
//!   `{"nonce","cipher_data"}`, a WebSocket key exchange
//!   `{"type":"key_exchange","encrypted_key"}` answered by
//!   `{"status":"key_exchange_ok"}`, then each WebSocket message
//!   `{"nonce","cipher_data","type":"encrypted"}`.
//!
//! There is no plaintext path: nothing here returns a message unencrypted,
//! and no session exists without the server's key. A caller with no key, or
//! no randomness, sends nothing.
use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, KeyInit},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use rand_core::{CryptoRng, RngCore};
use rsa::{Oaep, RsaPublicKey, pkcs1::DecodeRsaPublicKey, pkcs8::DecodePublicKey};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

/// The AES-GCM nonce's length (V1's `GCM_NONCE_SIZE`).
pub const NONCE_BYTES: usize = 12;
/// The session key's length: AES-256.
pub const KEY_BYTES: usize = 32;

/// Why nothing can be sent or read.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// The server's public key is not an RSA public key in PEM.
    PublicKey,
    /// No randomness for a key or a nonce.
    Random,
    /// The session key could not be encrypted under the server's key.
    Encrypt,
    /// An envelope that is not V1's shape, not Base64, or a nonce not 12
    /// bytes long.
    Envelope,
    /// The message does not decrypt under this session (wrong key or
    /// tampered).
    Decrypt,
    /// The plaintext of a WebSocket message is not UTF-8.
    Text,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Error::PublicKey => "the server's public key is not an RSA key in PEM",
            Error::Random => "no randomness for a key or a nonce",
            Error::Encrypt => "the session key could not be encrypted",
            Error::Envelope => "not an encrypted envelope",
            Error::Decrypt => "the message does not decrypt",
            Error::Text => "the message is not text",
        })
    }
}

impl std::error::Error for Error {}

/// The server's RSA public key from PEM: SubjectPublicKeyInfo
/// (`BEGIN PUBLIC KEY`) or PKCS#1 (`BEGIN RSA PUBLIC KEY`), as
/// `load_pem_public_key` takes both; anything else is refused.
pub fn public_key_from_pem(pem: &str) -> Result<RsaPublicKey, Error> {
    RsaPublicKey::from_public_key_pem(pem.trim())
        .or_else(|_| RsaPublicKey::from_pkcs1_pem(pem.trim()))
        .map_err(|_| Error::PublicKey)
}

/// One message's nonce and ciphertext, Base64 (V1's `CipherPayload`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cipher {
    pub nonce: String,
    pub cipher_data: String,
}

/// An HTTP request's body (V1's `CipherHTTPRequest`), in pydantic's field
/// order.
#[derive(Serialize)]
struct HttpRequest<'a> {
    nonce: &'a str,
    cipher_data: &'a str,
    encrypted_key: &'a str,
}

/// A WebSocket message after the exchange (V1's `WSCipherMessage`). V1
/// takes `type` as optional, `"encrypted"` when given.
#[derive(Serialize, Deserialize)]
struct WsMessage {
    nonce: String,
    cipher_data: String,
    #[serde(rename = "type", default = "encrypted")]
    kind: String,
}

fn encrypted() -> String {
    "encrypted".to_owned()
}

/// The WebSocket key exchange (V1's `WSKeyExchangeRequest`).
#[derive(Serialize)]
struct KeyExchange<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    encrypted_key: &'a str,
}

/// Whether the server's answer to the key exchange accepts it: a JSON
/// object whose `status` is `key_exchange_ok`. Anything else and the caller
/// drops the connection; it never goes on in plaintext.
pub fn key_exchange_ok(reply: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(reply)
        .ok()
        .and_then(|value| {
            value
                .get("status")
                .and_then(|s| s.as_str())
                .map(str::to_owned)
        })
        .as_deref()
        == Some("key_exchange_ok")
}

/// One AES-256-GCM session (V1's `CryptoSession`): its key, random, and that
/// key encrypted under the server's public key. The key itself is not kept
/// beside the cipher, and the cipher's key schedule is wiped when dropped.
pub struct Session {
    cipher: Aes256Gcm,
    encrypted_key_b64: String,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Session(..)")
    }
}

impl Session {
    /// A new session under the server's key, its key and the OAEP padding's
    /// seed from `rng` (the system's randomness in the product).
    pub fn new(server: &RsaPublicKey, rng: &mut (impl RngCore + CryptoRng)) -> Result<Self, Error> {
        let mut key = Zeroizing::new([0u8; KEY_BYTES]);
        rng.try_fill_bytes(key.as_mut_slice())
            .map_err(|_| Error::Random)?;
        Self::with_key(&key, server, rng)
    }

    /// A session with the system's randomness.
    pub fn with_system_random(server: &RsaPublicKey) -> Result<Self, Error> {
        Self::new(server, &mut rand_core::OsRng)
    }

    /// A session with the given key: the server's side in tests, which
    /// opens the key exchange and then speaks under the same key.
    pub(crate) fn with_key(
        key: &[u8; KEY_BYTES],
        server: &RsaPublicKey,
        rng: &mut (impl RngCore + CryptoRng),
    ) -> Result<Self, Error> {
        let encrypted = server
            .encrypt(rng, Oaep::new::<sha2::Sha256>(), key)
            .map_err(|_| Error::Encrypt)?;
        Ok(Self {
            cipher: Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key)),
            encrypted_key_b64: STANDARD.encode(encrypted),
        })
    }

    /// The session key, encrypted under the server's key, Base64.
    pub fn encrypted_key_b64(&self) -> &str {
        &self.encrypted_key_b64
    }

    /// `plaintext` under a fresh random nonce.
    pub fn encrypt(
        &self,
        plaintext: &[u8],
        rng: &mut (impl RngCore + CryptoRng),
    ) -> Result<Cipher, Error> {
        let mut nonce = [0u8; NONCE_BYTES];
        rng.try_fill_bytes(&mut nonce).map_err(|_| Error::Random)?;
        self.encrypt_with_nonce(&nonce, plaintext)
    }

    /// `plaintext` under `nonce`. Only [`Session::encrypt`] picks a nonce in
    /// use; a nonce is never used twice with one key.
    fn encrypt_with_nonce(
        &self,
        nonce: &[u8; NONCE_BYTES],
        plaintext: &[u8],
    ) -> Result<Cipher, Error> {
        let sealed = self
            .cipher
            .encrypt(Nonce::from_slice(nonce), plaintext)
            .map_err(|_| Error::Encrypt)?;
        Ok(Cipher {
            nonce: STANDARD.encode(nonce),
            cipher_data: STANDARD.encode(sealed),
        })
    }

    /// The plaintext of a message under this session.
    pub fn decrypt(&self, cipher: &Cipher) -> Result<Vec<u8>, Error> {
        let nonce = STANDARD
            .decode(&cipher.nonce)
            .map_err(|_| Error::Envelope)?;
        if nonce.len() != NONCE_BYTES {
            return Err(Error::Envelope);
        }
        let sealed = STANDARD
            .decode(&cipher.cipher_data)
            .map_err(|_| Error::Envelope)?;
        self.cipher
            .decrypt(Nonce::from_slice(&nonce), sealed.as_slice())
            .map_err(|_| Error::Decrypt)
    }

    /// An HTTP request's body: `plain` encrypted, with the session key.
    pub fn wrap_http_request(
        &self,
        plain: &[u8],
        rng: &mut (impl RngCore + CryptoRng),
    ) -> Result<Vec<u8>, Error> {
        let cipher = self.encrypt(plain, rng)?;
        serde_json::to_vec(&HttpRequest {
            nonce: &cipher.nonce,
            cipher_data: &cipher.cipher_data,
            encrypted_key: &self.encrypted_key_b64,
        })
        .map_err(|_| Error::Envelope)
    }

    /// An HTTP response's plaintext.
    pub fn unwrap_http_response(&self, body: &[u8]) -> Result<Vec<u8>, Error> {
        let cipher: Cipher = serde_json::from_slice(body).map_err(|_| Error::Envelope)?;
        self.decrypt(&cipher)
    }

    /// The key exchange that opens a WebSocket session, sent before
    /// anything else.
    pub fn key_exchange(&self) -> String {
        serde_json::to_string(&KeyExchange {
            kind: "key_exchange",
            encrypted_key: &self.encrypted_key_b64,
        })
        .unwrap_or_default()
    }

    /// A WebSocket text message, encrypted.
    pub fn wrap_ws_message(
        &self,
        plain: &str,
        rng: &mut (impl RngCore + CryptoRng),
    ) -> Result<String, Error> {
        let cipher = self.encrypt(plain.as_bytes(), rng)?;
        Self::ws_envelope(cipher)
    }

    fn ws_envelope(cipher: Cipher) -> Result<String, Error> {
        serde_json::to_string(&WsMessage {
            nonce: cipher.nonce,
            cipher_data: cipher.cipher_data,
            kind: encrypted(),
        })
        .map_err(|_| Error::Envelope)
    }

    /// A WebSocket text message's plaintext.
    pub fn unwrap_ws_message(&self, raw: &str) -> Result<String, Error> {
        let message: WsMessage = serde_json::from_str(raw).map_err(|_| Error::Envelope)?;
        if message.kind != "encrypted" {
            return Err(Error::Envelope);
        }
        let plain = self.decrypt(&Cipher {
            nonce: message.nonce,
            cipher_data: message.cipher_data,
        })?;
        String::from_utf8(plain).map_err(|_| Error::Text)
    }
}

#[cfg(test)]
mod tests;
