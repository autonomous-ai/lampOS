//! The usage-limit line with when the limit comes back (#7346), in the
//! owner's language and the lamp's local time.

use crate::language::Language;

/// [`super::DOWN_LINE_LIMIT`] with when it comes back: `reset` and `now`
/// in seconds since 1970 UTC, `offset` the lamp's seconds east of UTC.
/// Within a day, the hour ("It comes back at 7 AM."); further, the day.
/// Nothing for a reset already past.
pub(crate) fn line_at(
    _language: Option<Language>,
    _reset: i64,
    _now: i64,
    _offset: i32,
) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-11-17T00:00:00Z, 7 AM in Ho Chi Minh City, a Tuesday there.
    const RESET: i64 = 1_794_873_600;
    const HCM: i32 = 7 * 3_600;

    #[test]
    fn within_a_day_the_hour_is_said_in_english_and_vietnamese() {
        let now = RESET - 3 * 3_600;
        assert_eq!(
            line_at(None, RESET, now, HCM).as_deref(),
            Some("I've hit my usage limit. It comes back at 7 AM.")
        );
        assert_eq!(
            line_at(Some(Language::Vietnamese), RESET, now, HCM).as_deref(),
            Some("Mình hết hạn mức rồi, 7 giờ sáng sẽ có lại.")
        );
        let half = RESET + 12 * 3_600 + 30 * 60;
        assert_eq!(
            line_at(None, half, now, HCM).as_deref(),
            Some("I've hit my usage limit. It comes back at 7:30 PM.")
        );
        assert_eq!(
            line_at(Some(Language::Vietnamese), half, now, HCM).as_deref(),
            Some("Mình hết hạn mức rồi, 7 giờ 30 tối sẽ có lại.")
        );
    }

    #[test]
    fn past_a_day_the_day_is_said_and_a_past_reset_says_nothing() {
        let now = RESET - 2 * 86_400;
        assert_eq!(
            line_at(None, RESET, now, HCM).as_deref(),
            Some("I've hit my usage limit. It comes back on Tuesday.")
        );
        assert_eq!(
            line_at(Some(Language::Vietnamese), RESET, now, HCM).as_deref(),
            Some("Mình hết hạn mức rồi, thứ Ba sẽ có lại.")
        );
        assert_eq!(line_at(None, RESET, RESET, HCM), None);
        assert_eq!(line_at(None, RESET, RESET + 60, HCM), None);
    }

    #[test]
    fn each_language_says_the_hour() {
        let now = RESET - 3_600;
        for language in [Language::Japanese, Language::Chinese, Language::Korean] {
            let line = line_at(Some(language), RESET, now, HCM).unwrap_or_default();
            assert!(line.contains('7'), "{language:?}: {line}");
        }
    }
}
