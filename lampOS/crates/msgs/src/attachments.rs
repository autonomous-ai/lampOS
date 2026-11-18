//! A photo or a text the owner sent from the app (`attachments/arrived`,
//! #7659, `LAMP_ATTACHMENTS`). `lamp-cloud` fetches it from the backend and
//! keeps it in [`AttachRef::DIR`]; this message says where, never what: a
//! path, an id, a kind and a size, as [`crate::ImageRef`] says where a
//! picture is. No bytes and no name go on the bus.
use crate::Invalid;
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Where an attachment from the app is and what it is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachRef {
    /// The file that holds it: an absolute path whose last part is `id`.
    pub path: String,
    /// The first 16 lowercase hex of its SHA-256, as the backend's kind
    /// `attach.<id>` names it.
    pub id: String,
    /// [`AttachRef::PHOTO`] or [`AttachRef::TEXT`].
    pub kind: String,
    /// `image/jpeg` for a photo; `text/plain` or `text/markdown` for a text.
    pub mime: String,
    /// Its size, at most [`AttachRef::MOST_PHOTO_BYTES`] or
    /// [`AttachRef::MOST_TEXT_BYTES`].
    pub bytes: u64,
    /// When the app sent it, as the backend said: Unix ms (0 if unsaid).
    pub sent_at: u64,
}

impl AttachRef {
    pub const TYPE: &str = "lamp_msgs/AttachRef";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "attachments/arrived";
    /// The folder `lamp-cloud` keeps them in: tmpfs, so gone at a reboot.
    pub const DIR: &str = "/run/lampos-attachments";
    pub const PHOTO: &str = "photo";
    pub const TEXT: &str = "text";
    /// A photo: a JPEG the app already made small (a written look's own
    /// bound on Gemini Live, `MAX_LOOK_IMAGE_BYTES`).
    pub const MOST_PHOTO_BYTES: u64 = 184 * 1_024;
    /// A text, or a PDF's text as the backend took it out.
    pub const MOST_TEXT_BYTES: u64 = 64 * 1_024;
    /// How long it is news: the owner's next line within this carries it.
    pub const LASTS: Duration = Duration::from_secs(120);

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event).lasting(Self::LASTS)
    }

    /// The most bytes of `kind`, or none for a kind not taken.
    pub fn most_bytes(kind: &str) -> Option<u64> {
        match kind {
            Self::PHOTO => Some(Self::MOST_PHOTO_BYTES),
            Self::TEXT => Some(Self::MOST_TEXT_BYTES),
            _ => None,
        }
    }

    /// Whether `id` is 16 lowercase hex.
    pub fn id_ok(id: &str) -> bool {
        id.len() == 16 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    }

    /// Whether it is in the product's folder, [`Self::DIR`].
    pub fn in_inbox(&self) -> bool {
        self.path == format!("{}/{}", Self::DIR, self.id)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        if !Self::id_ok(&self.id) {
            return Err(Invalid("an attachment's id is 16 lowercase hex".into()));
        }
        if !self.path.starts_with('/')
            || self.path.contains("/../")
            || !self.path.ends_with(&format!("/{}", self.id))
        {
            return Err(Invalid(
                "an attachment's path is absolute and ends in its id".into(),
            ));
        }
        let mime_ok = match self.kind.as_str() {
            Self::PHOTO => self.mime == "image/jpeg",
            Self::TEXT => matches!(self.mime.as_str(), "text/plain" | "text/markdown"),
            _ => false,
        };
        if !mime_ok {
            return Err(Invalid(
                "an attachment is a photo (image/jpeg) or a text (text/plain, text/markdown)"
                    .into(),
            ));
        }
        match Self::most_bytes(&self.kind) {
            Some(most) if (1..=most).contains(&self.bytes) => Ok(()),
            _ => Err(Invalid(
                "an attachment is 1 byte to 184 KiB (a photo) or 64 KiB (a text)".into(),
            )),
        }
    }
}

/// The attachments' topic (#7659), apart from the first topics.
pub fn attachments_topics() -> Vec<(&'static str, Declaration)> {
    vec![(AttachRef::TOPIC, AttachRef::declaration())]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn photo() -> AttachRef {
        AttachRef {
            path: "/run/lampos-attachments/0123456789abcdef".into(),
            id: "0123456789abcdef".into(),
            kind: "photo".into(),
            mime: "image/jpeg".into(),
            bytes: 2_048,
            sent_at: 1_763_000_000_000,
        }
    }

    #[test]
    fn an_attachment_is_a_path_and_what_it_is_never_its_bytes() {
        let photo = photo();
        assert_eq!(photo.check(), Ok(()));
        assert!(photo.in_inbox());
        let text = serde_json::to_string(&photo).unwrap();
        assert_eq!(
            text,
            r#"{"path":"/run/lampos-attachments/0123456789abcdef","id":"0123456789abcdef","kind":"photo","mime":"image/jpeg","bytes":2048,"sent_at":1763000000000}"#
        );
        assert_eq!(serde_json::from_str::<AttachRef>(&text).unwrap(), photo);
        let declaration = AttachRef::declaration();
        assert_eq!(declaration.kind, Kind::Event);
        assert_eq!(declaration.lifespan_ms, Some(120_000));
        assert_eq!(
            crate::declaration_of(AttachRef::TOPIC),
            Some(AttachRef::declaration())
        );
    }

    #[test]
    fn an_attachment_out_of_its_bounds_is_refused() {
        let bad = [
            AttachRef {
                id: "0123456789ABCDEF".into(),
                path: "/run/lampos-attachments/0123456789ABCDEF".into(),
                ..photo()
            },
            AttachRef {
                path: "run/lampos-attachments/0123456789abcdef".into(),
                ..photo()
            },
            AttachRef {
                path: "/run/lampos-attachments/fedcba9876543210".into(),
                ..photo()
            },
            AttachRef {
                mime: "image/png".into(),
                ..photo()
            },
            AttachRef {
                kind: "pdf".into(),
                ..photo()
            },
            AttachRef {
                bytes: 184 * 1_024 + 1,
                ..photo()
            },
            AttachRef {
                bytes: 0,
                ..photo()
            },
            AttachRef {
                kind: "text".into(),
                mime: "text/plain".into(),
                bytes: 64 * 1_024 + 1,
                ..photo()
            },
        ];
        for attach in bad {
            assert!(attach.check().is_err(), "{attach:?}");
        }
        let elsewhere = AttachRef {
            path: "/tmp/x/0123456789abcdef".into(),
            ..photo()
        };
        assert_eq!(elsewhere.check(), Ok(()));
        assert!(!elsewhere.in_inbox());
    }
}
