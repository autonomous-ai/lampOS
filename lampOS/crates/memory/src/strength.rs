//! How sure a line is, read from its own words (the memory owner's
//! Tiers-ideas plan, 3 Nov, P2): `said`, a `plan` ("I will", "going to",
//! "định") or a `maybe` ("maybe", "not decided", "not booked", "có lẽ",
//! "chưa quyết"). Adapted from memory-tiers (`Strength`, `combine` and
//! `constrained_by_words` in `memory-tiers/src/types.rs`, branch
//! `tiers/recall-adapter`; see `docs/memory-tiers.md`), with Vietnamese
//! words and the "not … decided/booked" negation added.
//!
//! Computed at read only, never stored: nothing new is kept, and a line is
//! only ever labelled weaker than said, never dropped or made stronger. A
//! word list, not a classifier: English and Vietnamese only, and "I will
//! never …" reads as a plan (weaker than said, the safe side).

/// How sure a line is. `Said` is a line as it was said; a reply carries no
/// label for it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Strength {
    Said,
    Plan,
    Maybe,
}

/// Words that make a line a maybe by themselves.
const MAYBE: &[&str] = &[
    "maybe",
    "perhaps",
    "might",
    "possibly",
    "undecided",
    "unsure",
];
/// Words that, after a negation within [`NEGATION_REACH`] words, make a
/// line a maybe: "I have not decided or booked a class".
const UNSETTLED: &[&str] = &["decided", "booked", "sure", "certain", "confirmed"];
/// A negation: "not", the `t` of "haven't" (split at the apostrophe) and
/// "never".
const NEGATION: &[&str] = &["not", "t", "never"];
const NEGATION_REACH: usize = 3;
/// Vietnamese two-word maybes: "có lẽ" (perhaps), "chưa quyết" (not decided
/// yet, as in "chưa quyết định"), "chưa chắc" and "không chắc" (not sure),
/// "chưa đặt" (not booked).
const MAYBE_PAIRS: &[[&str; 2]] = &[
    ["có", "lẽ"],
    ["chưa", "quyết"],
    ["chưa", "chắc"],
    ["không", "chắc"],
    ["chưa", "đặt"],
];
/// Words that make a line a plan: "will" and "'ll" (split at the
/// apostrophe), "plan", "intend", "gonna".
const PLAN: &[&str] = &[
    "will",
    "ll",
    "plan",
    "plans",
    "planning",
    "intend",
    "intends",
    "intending",
    "gonna",
];
/// Two-word plans: "going to", and Vietnamese "dự định" and "kế hoạch".
const PLAN_PAIRS: &[[&str; 2]] = &[["going", "to"], ["dự", "định"], ["kế", "hoạch"]];
/// Words before "định" that make it not a plan: "quyết định" (decide),
/// "nhất định" (surely), "cố định" (fixed), "ổn định" (stable), "xác định"
/// (determine), "quy định" (rule), "giả định" (assume), "khẳng định"
/// (affirm), "chỉ định" (assign).
const NOT_PLAN_BEFORE_DINH: &[&str] = &[
    "quyết", "nhất", "cố", "ổn", "xác", "quy", "giả", "khẳng", "chỉ",
];

impl Strength {
    /// The weaker of the two: a maybe over a plan over said (Tiers'
    /// `combine`).
    pub fn combine(self, other: Self) -> Self {
        if self == Self::Maybe || other == Self::Maybe {
            Self::Maybe
        } else if self == Self::Plan || other == Self::Plan {
            Self::Plan
        } else {
            Self::Said
        }
    }

    /// This strength, made weaker when `text`'s words say so; never
    /// stronger (Tiers' `constrained_by_words`): a question, a negation or
    /// any other word can only keep it or lower it.
    pub fn constrained_by_words(self, text: &str) -> Self {
        let words: Vec<String> = text
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .map(str::to_lowercase)
            .collect();
        let pair = |pairs: &[[&str; 2]]| {
            words
                .windows(2)
                .any(|two| pairs.iter().any(|[a, b]| two[0] == *a && two[1] == *b))
        };
        let maybe = words.iter().any(|word| MAYBE.contains(&word.as_str()))
            || pair(MAYBE_PAIRS)
            || words.iter().enumerate().any(|(at, word)| {
                UNSETTLED.contains(&word.as_str())
                    && words[at.saturating_sub(NEGATION_REACH)..at]
                        .iter()
                        .any(|before| NEGATION.contains(&before.as_str()))
            });
        if maybe {
            return self.combine(Self::Maybe);
        }
        let plan = words.iter().any(|word| PLAN.contains(&word.as_str()))
            || pair(PLAN_PAIRS)
            || words.iter().enumerate().any(|(at, word)| {
                word == "định"
                    && !at.checked_sub(1).is_some_and(|before| {
                        NOT_PLAN_BEFORE_DINH.contains(&words[before].as_str())
                    })
            });
        if plan { self.combine(Self::Plan) } else { self }
    }

    /// A line's strength from its own words.
    pub fn of(text: &str) -> Self {
        Self::Said.constrained_by_words(text)
    }

    /// The reply's `strength` value: `"maybe"` or `"plan"`, none for said.
    pub fn label(self) -> Option<&'static str> {
        match self {
            Self::Said => None,
            Self::Plan => Some("plan"),
            Self::Maybe => Some("maybe"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Strength::{self, Maybe, Plan, Said};

    #[test]
    fn words_label_english_and_vietnamese_lines() {
        let table: &[(&str, Strength)] = &[
            ("I take my coffee black", Said),
            ("My tea is jasmine", Said),
            (
                "Maybe I will learn pottery; I have not decided or booked a class.",
                Maybe,
            ),
            ("I might go to Da Lat", Maybe),
            ("Perhaps a red bike", Maybe),
            ("I haven't decided yet", Maybe),
            ("I haven’t booked the flight", Maybe),
            ("Not sure about the dentist", Maybe),
            ("The class is not yet confirmed", Maybe),
            ("I will learn pottery", Plan),
            ("I'll call my sister on Sunday", Plan),
            ("I am going to visit Hue", Plan),
            ("I plan to repaint the kitchen", Plan),
            ("Có lẽ tôi sẽ học gốm", Maybe),
            ("Tôi chưa quyết định", Maybe),
            ("Tôi chưa đặt lớp", Maybe),
            ("Không chắc lắm", Maybe),
            ("Tôi định học gốm", Plan),
            ("Tôi có dự định đi Huế", Plan),
            ("Tôi đã quyết định rồi", Said),
            ("Nhà tôi ổn định", Said),
            ("Trà của tôi là trà nhài", Said),
            ("", Said),
        ];
        for (text, strength) in table {
            assert_eq!(Strength::of(text), *strength, "{text:?}");
        }
    }

    /// A question or a negation never raises a line: "booked" after "Did I"
    /// or a later plain line keeps the maybe, and nothing lifts a plan.
    #[test]
    fn a_question_or_negation_is_never_upgraded() {
        for text in [
            "Did I book it? I booked it.",
            "I booked it",
            "No.",
            "Will I?",
            "Đã đặt rồi",
        ] {
            assert_eq!(Maybe.constrained_by_words(text), Maybe, "{text:?}");
            assert_ne!(Plan.constrained_by_words(text), Said, "{text:?}");
        }
        assert_eq!(Said.combine(Maybe), Maybe);
        assert_eq!(Plan.combine(Said), Plan);
        assert_eq!(Maybe.combine(Plan), Maybe);
    }

    #[test]
    fn said_carries_no_label() {
        assert_eq!(Said.label(), None);
        assert_eq!(Plan.label(), Some("plan"));
        assert_eq!(Maybe.label(), Some("maybe"));
    }
}
