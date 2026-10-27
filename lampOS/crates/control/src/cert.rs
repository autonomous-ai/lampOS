//! The lamp's own TLS certificate for the door (docs/control.md): made once
//! on the lamp, kept in its state folder, and pinned by each computer at
//! pairing by its SHA-256, which the app's pairing code carries. No
//! certificate authority is involved: the pin is the trust.

use std::io;
use std::path::Path;

use sha2::{Digest, Sha256};

/// The door's certificate and key, both DER, and the certificate's pin.
#[derive(Clone)]
pub struct Identity {
    pub cert: Vec<u8>,
    pub key: Vec<u8>,
    /// SHA-256 of the certificate's DER, lowercase hex.
    pub pin: String,
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the key.
        formatter
            .debug_struct("Identity")
            .field("pin", &self.pin)
            .finish_non_exhaustive()
    }
}

impl Identity {
    /// The pin's 32 bytes, as every pairing proof is made over them.
    pub fn pin_bytes(&self) -> [u8; 32] {
        Sha256::digest(&self.cert).into()
    }
}

/// The pin of a certificate's DER.
pub fn pin_of(cert: &[u8]) -> String {
    Sha256::digest(cert)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The certificate in `dir` (`cert.der`, `key.der`), made for `host`.local
/// if there is none yet: P-256, valid to 2100, the key 0600.
pub fn load_or_make(dir: &Path, host: &str) -> io::Result<Identity> {
    let cert_path = dir.join("cert.der");
    let key_path = dir.join("key.der");
    if let (Ok(cert), Ok(key)) = (std::fs::read(&cert_path), std::fs::read(&key_path)) {
        let pin = pin_of(&cert);
        return Ok(Identity { cert, key, pin });
    }
    let key = rcgen::KeyPair::generate().map_err(io::Error::other)?;
    let mut params = rcgen::CertificateParams::new(vec![format!("{host}.local"), host.to_owned()])
        .map_err(io::Error::other)?;
    params.not_before = rcgen::date_time_ymd(2026, 1, 1);
    params.not_after = rcgen::date_time_ymd(2100, 1, 1);
    let mut name = rcgen::DistinguishedName::new();
    name.push(rcgen::DnType::CommonName, format!("{host}.local"));
    params.distinguished_name = name;
    let cert = params.self_signed(&key).map_err(io::Error::other)?;
    let cert = cert.der().to_vec();
    let key = key.serialize_der();
    write_private(&key_path, &key)?;
    write_private(&cert_path, &cert)?;
    let pin = pin_of(&cert);
    Ok(Identity { cert, key, pin })
}

/// Whole, synced, renamed over, 0600.
fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    let temporary = path.with_extension("new");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::rename(&temporary, path)?;
    if let Some(parent) = path.parent() {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;

    #[test]
    fn made_once_then_kept_with_a_private_key_and_a_stable_pin() {
        let dir = std::env::temp_dir().join(format!("lamp-control-cert-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let made = load_or_make(&dir, "lamp-test").unwrap();
        assert_eq!(made.pin.len(), 64);
        assert_eq!(made.pin, pin_of(&made.cert));
        assert_eq!(
            std::fs::metadata(dir.join("key.der"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let again = load_or_make(&dir, "lamp-test").unwrap();
        assert_eq!(again.pin, made.pin);
        assert!(!format!("{made:?}").contains(&format!("{:?}", made.key)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
