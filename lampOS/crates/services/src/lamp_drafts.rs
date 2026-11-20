//! The Gmail drafts the lamp itself saved (`LAMP_GMAIL_DRAFT_EDITS`): the
//! first builder's rule on #8005, after lamp-9adf on 20 Nov, where the
//! lamp's own save had failed and "that draft" resolved to one the owner
//! wrote in Gmail, whose delete a spoken yes then confirmed. A change or a
//! delete takes only a draft named here; every other draft in Drafts can
//! be read, never written.
//!
//! - **Kept beside the links.** In `<store>/`[`FILE`] (root's, 0600, the
//!   store folder 0700), so it survives a restart, and a wipe (a sign-out,
//!   an unpairing, a reset) takes it with everything else.
//! - **By account, UID and UIDVALIDITY.** Each draft is the account it was
//!   saved in and its place in Drafts (Gmail's APPENDUID: its UID and
//!   Drafts' `UIDVALIDITY`), never a word of the mail. A save adds it; a
//!   change replaces it by the new draft's (none when Gmail names none, so
//!   that one is no longer changed); a delete, a draft found gone or found
//!   not the lamp's (Drafts renumbered, or a Message-ID the lamp didn't
//!   write, `mail::edit_draft_over`), and unlinking Gmail, take it away.
//! - **Erring toward refusing.** A file that can't be read is an empty
//!   record: nothing is the lamp's, so nothing is changed. At most
//!   [`KEPT_MOST`] are kept, the oldest dropped first.
use crate::mail::DraftUid;
use crate::skill_writes::{keep, read};
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

/// The record's file in the store folder.
pub const FILE: &str = "gmail-lamp-drafts.json";

/// The most drafts kept as the lamp's own.
pub const KEPT_MOST: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Saved {
    account: String,
    #[serde(flatten)]
    at: DraftUid,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Record {
    drafts: Vec<Saved>,
}

/// Whose a draft is, by [`LampDrafts::whose`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Whose {
    /// The lamp saved it, at this place in Drafts.
    Lamps(DraftUid),
    /// Not the lamp's, though it has saved others in this account.
    Other,
    /// The lamp has saved no draft in this account that is still its own.
    NoneSaved,
}

/// The record in one store folder. The caller holds one lock across each
/// change, so two never lose one another's.
#[derive(Debug)]
pub struct LampDrafts {
    path: PathBuf,
}

impl LampDrafts {
    pub fn new(store: &Path) -> Self {
        LampDrafts {
            path: store.join(FILE),
        }
    }

    fn record(&self) -> Record {
        read(&self.path)
    }

    /// Whether the draft of `uid` in `account`'s Drafts is the lamp's own,
    /// and its place when it is.
    pub fn whose(&self, account: &str, uid: u32) -> Whose {
        let record = self.record();
        let mine = record.drafts.iter().filter(|one| one.account == account);
        let mut any = false;
        for one in mine {
            if one.at.uid == uid {
                return Whose::Lamps(one.at);
            }
            any = true;
        }
        if any { Whose::Other } else { Whose::NoneSaved }
    }

    /// The draft at `at` the lamp just saved in `account`'s Drafts.
    pub fn saved(&self, account: &str, at: DraftUid) -> io::Result<()> {
        self.change(account, None, Some(at))
    }

    /// The lamp's draft of UID `old` replaced by `new` (none when Gmail
    /// named none), or deleted, gone or found not the lamp's with none.
    pub fn replaced(&self, account: &str, old: u32, new: Option<DraftUid>) -> io::Result<()> {
        self.change(account, Some(old), new)
    }

    /// Every draft kept for `account` taken away: its Gmail link removed.
    pub fn forget(&self, account: &str) -> io::Result<()> {
        let mut record = self.record();
        record.drafts.retain(|one| one.account != account);
        keep(&self.path, &record)
    }

    fn change(&self, account: &str, old: Option<u32>, new: Option<DraftUid>) -> io::Result<()> {
        let mut record = self.record();
        record.drafts.retain(|one| {
            !(one.account == account
                && (Some(one.at.uid) == old || Some(one.at.uid) == new.map(|new| new.uid)))
        });
        if let Some(at) = new {
            record.drafts.push(Saved {
                account: account.to_owned(),
                at,
            });
        }
        let over = record.drafts.len().saturating_sub(KEPT_MOST);
        record.drafts.drain(..over);
        keep(&self.path, &record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn at(uid: u32) -> DraftUid {
        DraftUid { validity: 7, uid }
    }

    #[test]
    fn a_saved_draft_is_the_lamps_until_changed_or_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let drafts = LampDrafts::new(dir.path());
        assert_eq!(drafts.whose("me@home.test", 12), Whose::NoneSaved);
        drafts.saved("me@home.test", at(12)).unwrap();
        assert_eq!(drafts.whose("me@home.test", 12), Whose::Lamps(at(12)));
        assert_eq!(drafts.whose("me@home.test", 40), Whose::Other);
        // Another account's draft of the same UID is never the lamp's.
        assert_eq!(drafts.whose("you@work.test", 12), Whose::NoneSaved);
        // Read again from the file, as after a restart.
        let again = LampDrafts::new(dir.path());
        drafts.replaced("me@home.test", 12, Some(at(112))).unwrap();
        assert_eq!(again.whose("me@home.test", 12), Whose::Other);
        assert_eq!(again.whose("me@home.test", 112), Whose::Lamps(at(112)));
        again.replaced("me@home.test", 112, None).unwrap();
        assert_eq!(drafts.whose("me@home.test", 112), Whose::NoneSaved);
        let mode = std::fs::metadata(dir.path().join(FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    /// Kept on disk, by account: one account's record is never another's,
    /// and a forget of one leaves the other's.
    #[test]
    fn the_record_is_kept_on_disk_by_account() {
        let dir = tempfile::tempdir().unwrap();
        let drafts = LampDrafts::new(dir.path());
        drafts.saved("me@home.test", at(12)).unwrap();
        drafts.saved("you@work.test", at(30)).unwrap();
        let text = std::fs::read_to_string(dir.path().join(FILE)).unwrap();
        assert_eq!(
            text,
            r#"{"drafts":[{"account":"me@home.test","validity":7,"uid":12},{"account":"you@work.test","validity":7,"uid":30}]}"#
        );
        assert_eq!(drafts.whose("you@work.test", 12), Whose::Other);
        assert_eq!(drafts.whose("me@home.test", 30), Whose::Other);
        drafts.forget("me@home.test").unwrap();
        assert_eq!(drafts.whose("me@home.test", 12), Whose::NoneSaved);
        assert_eq!(drafts.whose("you@work.test", 30), Whose::Lamps(at(30)));
    }

    #[test]
    fn an_unreadable_record_owns_nothing_and_the_oldest_go_first() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), b"not json").unwrap();
        let drafts = LampDrafts::new(dir.path());
        assert_eq!(drafts.whose("me@home.test", 12), Whose::NoneSaved);
        // An entry kept without its validity (an older record) reads as none.
        std::fs::write(
            dir.path().join(FILE),
            br#"{"drafts":[{"account":"me@home.test","uid":12}]}"#,
        )
        .unwrap();
        assert_eq!(drafts.whose("me@home.test", 12), Whose::NoneSaved);
        for uid in 0..=KEPT_MOST as u32 {
            drafts.saved("me@home.test", at(uid)).unwrap();
        }
        assert_eq!(drafts.whose("me@home.test", 0), Whose::Other);
        assert_eq!(drafts.whose("me@home.test", 1), Whose::Lamps(at(1)));
        assert_eq!(
            drafts.whose("me@home.test", KEPT_MOST as u32),
            Whose::Lamps(at(KEPT_MOST as u32))
        );
    }
}
