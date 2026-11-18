//! The usage-limit line with when the limit comes back (#7346), in the
//! owner's language and the lamp's local time.

use crate::language::Language;

/// [`super::DOWN_LINE_LIMIT`] with when it comes back: `reset` and `now`
/// in seconds since 1970 UTC, `offset` the lamp's seconds east of UTC.
/// Within a day, the hour ("It comes back at 7 AM."); further, the day.
/// Nothing for a reset already past.
pub(crate) fn line_at(
    language: Option<Language>,
    reset: i64,
    now: i64,
    offset: i32,
) -> Option<String> {
    if reset <= now {
        return None;
    }
    let local = reset + i64::from(offset);
    let seconds = local.rem_euclid(86_400);
    let (hour, minute) = (seconds / 3_600, seconds % 3_600 / 60);
    // Sunday first: 1970-01-01 was a Thursday.
    let weekday = (local.div_euclid(86_400) + 4).rem_euclid(7) as usize;
    let a_day_or_more = reset - now > 86_400;
    let twelve = match hour % 12 {
        0 => 12,
        hour => hour,
    };
    Some(match language {
        None => {
            let when = if a_day_or_more {
                let days = [
                    "Sunday",
                    "Monday",
                    "Tuesday",
                    "Wednesday",
                    "Thursday",
                    "Friday",
                    "Saturday",
                ];
                format!("on {}", days[weekday])
            } else {
                let half = if hour < 12 { "AM" } else { "PM" };
                match minute {
                    0 => format!("at {twelve} {half}"),
                    _ => format!("at {twelve}:{minute:02} {half}"),
                }
            };
            format!("I've hit my usage limit. It comes back {when}.")
        }
        Some(Language::Vietnamese) => {
            let when = if a_day_or_more {
                [
                    "chủ nhật",
                    "thứ Hai",
                    "thứ Ba",
                    "thứ Tư",
                    "thứ Năm",
                    "thứ Sáu",
                    "thứ Bảy",
                ][weekday]
                    .to_owned()
            } else {
                let part = match hour {
                    0..=3 => "đêm",
                    4..=10 => "sáng",
                    11..=12 => "trưa",
                    13..=17 => "chiều",
                    _ => "tối",
                };
                match minute {
                    0 => format!("{twelve} giờ {part}"),
                    _ => format!("{twelve} giờ {minute} {part}"),
                }
            };
            format!("Mình hết hạn mức rồi, {when} sẽ có lại.")
        }
        Some(Language::Japanese) => {
            let when = if a_day_or_more {
                [
                    "日曜日",
                    "月曜日",
                    "火曜日",
                    "水曜日",
                    "木曜日",
                    "金曜日",
                    "土曜日",
                ][weekday]
                    .to_owned()
            } else {
                let half = if hour < 12 { "午前" } else { "午後" };
                let hour = hour % 12;
                match minute {
                    0 => format!("{half}{hour}時"),
                    _ => format!("{half}{hour}時{minute}分"),
                }
            };
            format!("利用上限に達しちゃった。{when}に戻るよ。")
        }
        Some(Language::Chinese) => {
            let when = if a_day_or_more {
                [
                    "星期日",
                    "星期一",
                    "星期二",
                    "星期三",
                    "星期四",
                    "星期五",
                    "星期六",
                ][weekday]
                    .to_owned()
            } else {
                let half = if hour < 12 { "上午" } else { "下午" };
                match minute {
                    0 => format!("{half}{twelve}点"),
                    _ => format!("{half}{twelve}点{minute}分"),
                }
            };
            format!("我的额度用完了，{when}恢复。")
        }
        Some(Language::Korean) => {
            let when = if a_day_or_more {
                [
                    "일요일",
                    "월요일",
                    "화요일",
                    "수요일",
                    "목요일",
                    "금요일",
                    "토요일",
                ][weekday]
                    .to_owned()
            } else {
                let half = if hour < 12 { "오전" } else { "오후" };
                match minute {
                    0 => format!("{half} {twelve}시"),
                    _ => format!("{half} {twelve}시 {minute}분"),
                }
            };
            format!("사용 한도를 다 썼어요. {when}에 돌아와요.")
        }
    })
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
