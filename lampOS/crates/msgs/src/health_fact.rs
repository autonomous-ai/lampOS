//! What the voice keeps in the owner's own memory of an Apple Health read
//! (#7900, the first builder's yes of 19 Nov, 14:37): one dated kept thing
//! under their person, worded `"{PREFIX}YYYY-MM-DD: ..."`, kept with
//! [`REMEMBER_OP`] so the memory marks it as Apple Health's (never found by
//! its words: a person's own line in them stays), one a day replaced by the
//! next read, gone after [`KEPT_DAYS`], and all removed ([`FORGET_OP`]) when
//! Apple Health is unlinked in the app (the first builder's ruling, 19 Nov
//! 15:46). One copy of the wording and ops, read by the voice, which keeps
//! them, and by `lamp-memory` and `lamp-cloud`, which remove them.

/// How every kept Apple Health fact starts; the read's local date follows.
pub const PREFIX: &str = "From Apple Health, read on ";

/// Memory's request that keeps one: `{"op": REMEMBER_OP, "person",
/// "at_unix_ms", "text"}`. It replaces the person's fact of the same local
/// day, and is linked to no exchange: forgetting it takes nothing said.
pub const REMEMBER_OP: &str = "remember_apple_health";

/// How many local days a kept fact lasts, the day it was read the first:
/// read on the 1st, it is gone on the 15th.
pub const KEPT_DAYS: u32 = 14;

/// Memory's request that removes every person's kept Apple Health facts,
/// found by their mark: `{"op": FORGET_OP}`, nothing else. The
/// app's narrow forget socket takes it too (`lamp-memory --forget-socket`).
pub const FORGET_OP: &str = "forget_apple_health";

/// Whether `text` is worded as a kept Apple Health fact ([`PREFIX`]): the
/// voice's check before it keeps one. Memory never finds them by it: a
/// person's own line may have the same words.
pub fn is_one(text: &str) -> bool {
    text.starts_with(PREFIX)
}

/// The kept fact for `line` (what was read, short), read on `date`
/// (`YYYY-MM-DD`, the summary's local day).
pub fn text(date: &str, line: &str) -> String {
    format!("{PREFIX}{date}: {line}.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kept_fact_is_worded_with_its_read_day() {
        let kept = text(
            "2026-11-19",
            "slept 6 h 52 m last night; 4,210 steps so far today",
        );
        assert_eq!(
            kept,
            "From Apple Health, read on 2026-11-19: slept 6 h 52 m last night; 4,210 steps so far today."
        );
        assert!(is_one(&kept));
        assert!(!is_one("takes her coffee black"));
        assert!(!is_one("I read on Apple Health that..."));
    }
}
