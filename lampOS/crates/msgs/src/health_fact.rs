//! What the voice keeps in the owner's own memory of an Apple Health read
//! (#7900, the first builder's yes of 19 Nov, 14:37): one dated kept thing
//! under their person, worded `"{PREFIX}YYYY-MM-DD: ..."`, so the memory
//! can find every one of them by its start alone and remove them all
//! ([`FORGET_OP`]) when Apple Health is unlinked in the app. One copy of the
//! wording, read by the voice, which keeps them, and by `lamp-memory` and
//! `lamp-cloud`, which remove them.

/// How every kept Apple Health fact starts; the read's local date follows.
pub const PREFIX: &str = "From Apple Health, read on ";

/// Memory's request that removes every person's kept Apple Health facts,
/// and what they were made from: `{"op": FORGET_OP}`, nothing else. The
/// app's narrow forget socket takes it too (`lamp-memory --forget-socket`).
pub const FORGET_OP: &str = "forget_apple_health";

/// Whether `text` is a kept Apple Health fact ([`PREFIX`]).
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
    fn a_kept_fact_is_found_by_its_start_alone() {
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
