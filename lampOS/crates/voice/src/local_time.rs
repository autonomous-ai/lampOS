//! What time it is where the lamp stands, in words, for a new session's
//! instruction.
//!
//! The model has no clock. On lamp-4ace on 2026-10-13 it told two people who
//! asked how it was, at eleven in the morning, that it was "enjoying the quiet
//! evening on the desk". The zone comes from the system's own compiled zone
//! file (`/etc/localtime`, RFC 8536), read by `lamp-local-time` as every
//! other part reads it (#820): past the changes a zone file lists, its
//! closing rule is worked out, so a slim file's summer time is right too.

/// Seconds east of UTC in the zone of this compiled zone file at `now`
/// (seconds since 1970 UTC): the zone's listed changes, and past them its
/// closing rule (`lamp-local-time`, #820). Nothing for what is no zone file.
pub fn offset_at(file: &[u8], now: i64) -> Option<i32> {
    lamp_local_time::offset_seconds_in(file, now).filter(|offset| offset.abs() < 86_400)
}

/// Year, month and day of a count of days since 1970-01-01.
pub(crate) fn civil(days: i64) -> (i64, usize, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = if month_part < 10 {
        month_part + 3
    } else {
        month_part - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month as usize, day)
}

/// The count of days since 1970-01-01 of a `YYYY-MM-DD`, as the memory
/// names a day; nothing for anything else or a day no calendar has.
pub(crate) fn day_of(name: &str) -> Option<i64> {
    let bytes = name.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(at, byte)| at == 4 || at == 7 || byte.is_ascii_digit())
    {
        return None;
    }
    let (year, month, day): (i64, i64, i64) = (
        name[..4].parse().ok()?,
        name[5..7].parse().ok()?,
        name[8..].parse().ok()?,
    );
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let shifted_year = if month <= 2 { year - 1 } else { year };
    let era = shifted_year.div_euclid(400);
    let year_of_era = shifted_year.rem_euclid(400);
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    // A 31 February is no day: it must read back as itself.
    (civil(days) == (year, month as usize, day)).then_some(days)
}

/// A day as a person says it: its weekday, the day and the month
/// ("Saturday 24 October").
pub(crate) fn day_said(days: i64) -> String {
    let (_, month, day) = civil(days);
    format!("{} {day} {}", weekday(days), MONTHS[month - 1])
}

/// Whether a word is a weekday as [`day_said`] names it.
pub(crate) fn is_weekday(word: &str) -> bool {
    DAYS.contains(&word)
}

/// The weekday of a count of days since 1970-01-01.
pub(crate) fn weekday(days: i64) -> &'static str {
    DAYS[days.rem_euclid(7) as usize]
}

const DAYS: [&str; 7] = [
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
];
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// The moment `now` in the zone of this compiled zone file, in words: the
/// weekday, the date, the time on a twelve-hour clock and the part of the
/// day.
fn words(
    file: &[u8],
    now: i64,
) -> Option<(&'static str, i64, &'static str, i64, i64, i64, &'static str)> {
    let local = now.checked_add(i64::from(offset_at(file, now)?))?;
    let (days, seconds) = (local.div_euclid(86_400), local.rem_euclid(86_400));
    let (year, month, day) = civil(days);
    let (hour, minute) = (seconds / 3_600, seconds % 3_600 / 60);
    let part = match hour {
        5..=11 => "in the morning",
        12..=16 => "in the afternoon",
        17..=20 => "in the evening",
        _ => "at night",
    };
    let weekday = DAYS[days.rem_euclid(7) as usize];
    let month = MONTHS[month - 1];
    let hour = if hour % 12 == 0 { 12 } else { hour % 12 };
    Some((weekday, day, month, year, hour, minute, part))
}

/// What a session opened at `now` is told about the time, in the zone of
/// this compiled zone file.
pub fn told(file: &[u8], now: i64) -> Option<String> {
    let (weekday, day, month, year, hour, minute, part) = words(file, now)?;
    Some(format!(
        "\n\nThe time\nIt is {weekday} {day} {month} {year}, around {hour}:{minute:02} {part}. You have no clock, so this may be up to an hour old: if someone asks the time, give it roughly."
    ))
}

/// The same for a session that may ask the time ([`crate::tools::THE_TIME`]):
/// what it is told on opening is for the part of the day, not for telling.
pub fn told_with_clock(file: &[u8], now: i64) -> Option<String> {
    let (weekday, day, month, year, hour, minute, part) = words(file, now)?;
    Some(format!(
        "\n\nThe time\nYou woke on {weekday} {day} {month} {year}, around {hour}:{minute:02} {part}, and it may be much later now. If someone asks the time or the date, or how long until something, call the_time first and tell them what it gives."
    ))
}

/// The same for a request built as it is asked (the second route, #1712):
/// the time then, exactly and in the words a person says it in
/// ([`said`]), to answer from. On lamp-4ace on 2026-10-19 the second
/// route, told [`told_with_clock`]'s "call the_time first", spent a round
/// on "what time is it?" whose first event came 2100 ms later.
pub fn told_asked(file: &[u8], now: i64) -> Option<String> {
    Some(format!(
        "\n\nThe time\n{} Your clock read this as you were asked: when someone asks the time or the date, or how long until something, answer from it.",
        said(file, now)?
    ))
}

/// The time at `now`, exactly, for the model that asked
/// ([`crate::tools::THE_TIME`]), with the words a person would say it in:
/// on lamp-4ace on 2026-10-14 the model, given "6:23 in the evening", said
/// "twenty-three past in the evening" and lost the hour.
pub fn said(file: &[u8], now: i64) -> Option<String> {
    said_after("It is", file, now)
}

/// [`said`], led by `lead`, the words that say whose time it is: built
/// from its parts, so a change to `said`'s wording cannot run the two
/// together (Ten on #1602).
fn said_after(lead: &str, file: &[u8], now: i64) -> Option<String> {
    let (weekday, day, month, year, hour, minute, part) = words(file, now)?;
    Some(format!(
        "{lead} {hour}:{minute:02} {part}, {weekday} {day} {month} {year}. Said aloud: {} {part}.",
        aloud(hour, minute)
    ))
}

/// Where the system keeps its compiled zone files, one per place.
pub const ZONES: &str = "/usr/share/zoneinfo";
/// The areas a place's name alone is looked for in ("London" is
/// `Europe/London`).
const AREAS: [&str; 8] = [
    "Europe",
    "America",
    "Asia",
    "Africa",
    "Australia",
    "Pacific",
    "Atlantic",
    "Indian",
];
/// What the model is told when it asks the time of a place this lamp has no
/// zone file for: never the lamp's own time in its place.
/// It says how to ask again: a city that is no zone's name ("Beijing",
/// "Hanoi") is asked again by its zone (Jarvis on #1575).
pub const PLACE_UNKNOWN: &str = "I don't know the time in that place: it is not a time zone I know. Call again with the place's IANA time zone, such as Asia/Shanghai for Beijing or Asia/Ho_Chi_Minh for Hanoi.";

/// Whether `zone` may name a file under [`ZONES`]: an IANA zone name such
/// as `Europe/London` or `America/Argentina/Buenos_Aires`, with no way out
/// of the folder.
fn zone_name_ok(zone: &str) -> bool {
    !zone.is_empty()
        && zone.len() <= 64
        && !zone.starts_with('/')
        && !zone.ends_with('/')
        && !zone.contains("..")
        && !zone.contains("//")
        && zone
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'+' | b'-' | b'/'))
}

/// The compiled zone file for the place the model named, from `zones` (the
/// system's [`ZONES`]), and its zone name: the name as given, spaces as
/// underscores, or, for a name alone ("London", "New York"), the first of
/// `AREAS` that has it. Nothing for a name that could leave the folder,
/// or a file that is not a zone file.
pub fn zone_file(zones: &std::path::Path, place: &str) -> Option<(String, Vec<u8>)> {
    let place = place.trim().replace(' ', "_");
    let read = |zone: &str| {
        let file = std::fs::read(zones.join(zone)).ok()?;
        file.starts_with(b"TZif").then(|| (zone.to_owned(), file))
    };
    if !zone_name_ok(&place) {
        return None;
    }
    // A name with no area is looked for in the areas only, and `UTC`: the
    // folder's top-level `EST`, `MST` and `HST` have no summer time, so
    // "EST" for the East Coast would be an hour off New York's half the
    // year (the reviewer on #1552).
    if !place.contains('/') {
        return if place == "UTC" {
            read(&place)
        } else {
            AREAS
                .iter()
                .find_map(|area| read(&format!("{area}/{place}")))
        };
    }
    read(&place)
}

/// The lamp's own place as the unit names it (`LAMP_PLACE`, "Hanoi,
/// Vietnam"), over its zone's: a zone names one city for a whole country or
/// region, and lamp-4ace, in Hanoi, keeps `Asia/Ho_Chi_Minh`, so "the
/// weather" was asked about the wrong city (#1712). Passed to the second
/// route's worker alone (`process.rs`).
pub const PLACE_ENVIRONMENT: &str = "LAMP_PLACE";
/// The most of it taken.
const PLACE_BYTES: usize = 60;

/// [`PLACE_ENVIRONMENT`] as `value` gives it: trimmed, at most
/// [`PLACE_BYTES`] of letters (any script), digits, spaces, `,`, `-`, `.`
/// and `'`; anything else is no place.
pub fn place_from(value: Option<&str>) -> Option<String> {
    let place = value?.trim();
    (!place.is_empty()
        && place.len() <= PLACE_BYTES
        && place
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, ' ' | ',' | '-' | '.' | '\'')))
    .then(|| place.to_owned())
}

/// The lamp's own place from its environment ([`place_from`]).
pub fn place_here() -> Option<String> {
    place_from(std::env::var(PLACE_ENVIRONMENT).ok().as_deref())
}

/// Told beside the time: where the lamp is when its place is set
/// ([`place_here`]), and that what is asked about without a place is there;
/// and either way that the area near the person is not known, so it is
/// asked, never picked. On lamp-3e83 on 29 Oct a district misheard by
/// speech to text was taken for another, said as fact and searched (#4337).
pub fn place_told(place: Option<&str>) -> Option<String> {
    Some(match place {
        Some(place) => format!(
            " You are in {place}. When someone asks about the weather, the news or anything nearby without naming a place, they mean {place}. {AREA_ASKED}"
        ),
        None => format!(" You don't know where you are beyond your time zone. {AREA_ASKED}"),
    })
}

/// [`place_told`]'s rule for the area near the person (#4337).
pub const AREA_ASKED: &str = "You don't know which part of town they are in: when they ask for something near them, or name a place you aren't sure you heard right, ask which area they mean before you search, and never pick one yourself. When you looked somewhere they didn't name, say where. The weather and the time are for where you are: never ask which area for those.";

/// Told beside the time instead of [`place_told`] while the owner's place
/// from the app is kept (#4333, `crate::owner_place`): that place is told
/// beside a request that needs it, so here the model is told neither the
/// unit's place nor that it knows none (#4337), only to ask when it is not
/// sure it heard a place, and to say where it looked when nobody named it,
/// never [`AREA_ASKED`]'s "ask before you search".
pub fn place_told_owner_kept() -> String {
    " Where they are is told beside a request that needs it. If you aren't sure you heard a place right, ask which area they mean; when you looked somewhere they didn't name, say where.".to_owned()
}

/// The place the lamp's own zone names ("Ho Chi Minh"), or `None` when it
/// names none ([`own_place`]).
pub fn zone_place_here() -> Option<String> {
    zone_here().as_deref().and_then(own_place)
}

/// The month and year where the lamp stands ("October 2026"), for a search
/// of what is new (#1712); `None` when the clock is not set or not known.
pub fn month_and_year_here() -> Option<String> {
    if unset_here() {
        return None;
    }
    let (file, now) = here()?;
    let (_, _, month, year, ..) = words(&file, now)?;
    Some(format!("{month} {year}"))
}

/// A zone's place as people say it: its last part, underscores as spaces
/// (`America/New_York` is "New York").
pub fn place_name(zone: &str) -> String {
    // `Etc/GMT-7` is seven hours ahead: POSIX turns the sign, so it is said
    // as people write it, "UTC+7" (Jarvis on #1575). Found after any folder
    // before it (`posix/Etc/GMT-7`), and a zero either way is "UTC" (the
    // reviewer on #1589).
    if let Some((_, hours)) = zone.split_once("Etc/GMT") {
        let (sign, number) = match hours.as_bytes().first() {
            Some(b'-') => ("+", &hours[1..]),
            Some(b'+') => ("-", &hours[1..]),
            _ => ("", hours),
        };
        return match number.parse::<u32>() {
            _ if number.is_empty() => "UTC".to_owned(),
            Ok(0) => "UTC".to_owned(),
            Ok(_) if !sign.is_empty() => format!("UTC{sign}{number}"),
            _ => zone.to_owned(),
        };
    }
    zone.rsplit('/').next().unwrap_or(zone).replace('_', " ")
}

/// [`place_name`] for a zone under one of the areas of places ([`AREAS`],
/// `Antarctica`, `Arctic`), after any `posix/` or `right/` folder, and
/// `None` for every other: UTC and its offsets (`Etc/GMT-7`), the old
/// POSIX names (`CET`, `EST5EDT`), `Factory`, and the backward names whose
/// last part is no place (`US/Pacific`, `Brazil/East`). Said of the lamp's
/// own time, "Here in UTC+7" or "Here in Pacific" sounds like a machine's
/// clock, not a place (Ten on #1602, the reviewer on #1609), so the lamp's
/// own is then only "here".
fn own_place(zone: &str) -> Option<String> {
    let zone = zone
        .strip_prefix("posix/")
        .or_else(|| zone.strip_prefix("right/"))
        .unwrap_or(zone);
    let (area, _) = zone.split_once('/')?;
    (AREAS.contains(&area) || matches!(area, "Antarctica" | "Arctic")).then(|| place_name(zone))
}

/// How the lamp's own time is led in, from its zone: "Here in Ho Chi
/// Minh", or "Here" when the zone names no place ([`own_place`]). The
/// unit's own place goes over the zone's ([`place_here`]).
fn here_in(zone: &str) -> String {
    here_in_at(place_here().as_deref(), zone)
}

fn here_in_at(set: Option<&str>, zone: &str) -> String {
    set.map(str::to_owned)
        .or_else(|| own_place(zone))
        .map_or_else(|| "Here".to_owned(), |place| format!("Here in {place}"))
}

/// The time at `now` in the place of `zone` (its file `there`), for the
/// model that asked for it ([`crate::tools::THE_TIME`], `LAMP_TIME_PLACES`,
/// #1547): whose time it is, said aloud, and the lamp's own beside it, so
/// it is never given as the other place's. On lamp-4ace on 18 October
/// "what time is it in London" was answered with the lamp's own, in Ho
/// Chi Minh City. `here` is the lamp's own zone's name and file, if known.
pub fn said_in(zone: &str, there: &[u8], here: Option<(&str, &[u8])>, now: i64) -> Option<String> {
    let (weekday, day, month, year, hour, minute, part) = words(there, now)?;
    let place = place_name(zone);
    let mut said = format!(
        "In {place} it is {hour}:{minute:02} {part}, {weekday} {day} {month} {year}. Said aloud: {} {part}.",
        aloud(hour, minute)
    );
    match here {
        // The same zone file is the same time, whatever its name
        // (`Asia/Saigon` is `Asia/Ho_Chi_Minh`).
        Some((_, file)) if file == there => said.push_str(" That is the time here too."),
        Some((home, file)) => {
            if let Some((_, _, _, _, hour, minute, part)) = words(file, now) {
                said.push_str(&format!(
                    " {} it is {hour}:{minute:02} {part}.",
                    here_in(home)
                ));
            }
        }
        None => {}
    }
    Some(said)
}

/// The lamp's own zone's name: where `/etc/localtime` links, the zone file
/// its time comes from, or `/etc/timezone`'s text, as Debian keeps it, only
/// when that is no link (`zone_named`).
pub fn zone_here() -> Option<String> {
    zone_named(
        std::fs::read_link("/etc/localtime").ok().as_deref(),
        std::fs::read_to_string("/etc/timezone").ok().as_deref(),
    )
}

/// The zone's name from where `/etc/localtime` links (`link`), whose file
/// is the time the lamp keeps, and only when it is not a link from
/// `/etc/timezone`'s text: `timedatectl set-timezone` changes the link and
/// not the text, so a text left from the image would name another zone
/// (the reviewer on #1552).
fn zone_named(link: Option<&std::path::Path>, text: Option<&str>) -> Option<String> {
    // A link that names no zone (one outside a `zoneinfo` folder) leaves
    // the text to name it (Jarvis on #1575).
    let linked = link
        .and_then(|link| link.to_str())
        .and_then(|link| link.split_once("zoneinfo/"))
        .map(|(_, zone)| zone)
        .filter(|zone| zone_name_ok(zone));
    match linked {
        Some(zone) => Some(zone.to_owned()),
        None => text
            .map(|text| text.trim().to_owned())
            .filter(|zone| zone_name_ok(zone)),
    }
}

/// What the model is told when it asks the time of `place`, now
/// (`LAMP_TIME_PLACES`): `Err` with what to say when the clock is known not
/// to be set or the place is not known.
pub fn now_in(place: &str) -> Result<Option<String>, &'static str> {
    if unset_here() {
        return Err(SAID_UNSET);
    }
    let Some((zone, there)) = zone_file(std::path::Path::new(ZONES), place) else {
        return Err(PLACE_UNKNOWN);
    };
    let Some((file, now)) = here() else {
        return Ok(None);
    };
    let home = zone_here();
    let here = home.as_deref().map(|home| (home, file.as_slice()));
    Ok(said_in(&zone, &there, here, now))
}

/// What the opening note adds with `LAMP_TIME_PLACES`: whose time it gives,
/// and to ask the function for anywhere else.
pub fn told_place(home: Option<&str>) -> String {
    told_place_at(place_here().as_deref(), home)
}

/// [`told_place`] with the unit's place `set` over the zone's.
fn told_place_at(set: Option<&str>, home: Option<&str>) -> String {
    let here = set
        .map(str::to_owned)
        .or_else(|| home.and_then(own_place))
        .map_or_else(
            || "the time where you are".to_owned(),
            |place| format!("the time where you are, in {place}"),
        );
    format!(
        " This is {here}. For the time anywhere else, call the_time with that place, never this time."
    )
}

const NUMBERS: [&str; 31] = [
    "",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "quarter",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
    "twenty",
    "twenty-one",
    "twenty-two",
    "twenty-three",
    "twenty-four",
    "twenty-five",
    "twenty-six",
    "twenty-seven",
    "twenty-eight",
    "twenty-nine",
    "half",
];

/// A time on a twelve-hour clock as people say it: "six o'clock", a whole
/// five as past or to ("ten past six", "quarter to seven", "twenty-five to
/// one"), and any other minute as the clock reads ("six twenty-three",
/// "twelve forty-two", "six oh seven"). On lamp-4ace on 2026-10-16 at 00:42
/// the lamp said "eighteen to one at night", which nobody says.
fn aloud(hour: i64, minute: i64) -> String {
    let next = hour % 12 + 1;
    let hour_word = NUMBERS[hour as usize];
    match minute {
        0 => format!("{hour_word} o'clock"),
        5 | 10 | 15 | 20 | 25 | 30 => format!("{} past {hour_word}", NUMBERS[minute as usize]),
        35 | 40 | 45 | 50 | 55 => format!(
            "{} to {}",
            NUMBERS[(60 - minute) as usize],
            NUMBERS[next as usize]
        ),
        1..=9 => format!("{hour_word} oh {}", NUMBERS[minute as usize]),
        _ => format!("{hour_word} {}", minute_words(minute)),
    }
}

/// A minute from 10 to 59 as it is read off a clock: "eleven", "forty-two".
fn minute_words(minute: i64) -> String {
    const TENS: [&str; 6] = ["", "", "twenty", "thirty", "forty", "fifty"];
    match minute {
        10..=19 => [
            "ten",
            "eleven",
            "twelve",
            "thirteen",
            "fourteen",
            "fifteen",
            "sixteen",
            "seventeen",
            "eighteen",
            "nineteen",
        ][(minute - 10) as usize]
            .to_owned(),
        _ if minute % 10 == 0 => TENS[(minute / 10) as usize].to_owned(),
        _ => format!(
            "{}-{}",
            TENS[(minute / 10) as usize],
            NUMBERS[(minute % 10) as usize]
        ),
    }
}

/// The lamp's own zone as every part reads it (`lamp-local-time`'s
/// `system`, #820): `/etc/localtime`, taken up again when setup changes it,
/// or the zone `/etc/timezone` names. Opened at its first use, and at each
/// later one until it opens; nothing while neither can be read.
pub fn system() -> Option<&'static lamp_local_time::LocalTime> {
    static SYSTEM: std::sync::OnceLock<lamp_local_time::LocalTime> = std::sync::OnceLock::new();
    if let Some(zone) = SYSTEM.get() {
        return Some(zone);
    }
    let zone = lamp_local_time::LocalTime::zone("system").ok()?;
    Some(SYSTEM.get_or_init(|| zone))
}

/// How many seconds east of UTC this machine's local time is now.
pub fn offset_here() -> Option<i32> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis();
    Some(system()?.offset_minutes_at(u64::try_from(now).ok()?) * 60)
}

/// `on` for the voice to ask the kernel whether the clock is set before it
/// tells or is told a time (#830, `docs/offline.md`). The lamp has no
/// battery clock: until a time client sets it, it is its last saved time.
pub const KERNEL_CLOCK_ENVIRONMENT: &str = "LAMP_KERNEL_CLOCK";

/// What a session is told when the clock has not been set.
pub const TOLD_UNSET: &str = "\n\nThe time\nYour clock has not been set yet, so you do not know the time or the date. If someone asks, say plainly that you do not know the time right now; never guess one.";

/// What the model is told when it asks the time and the clock is not set.
pub const SAID_UNSET: &str =
    "My clock has not been set yet, so I do not know the time or the date right now.";

/// Whether the clock is known not to be set: only with the switch on, and
/// only when the kernel does not say it is set.
pub fn unset(switch: Option<&str>, kernel: Option<bool>) -> bool {
    switch == Some("on") && kernel != Some(true)
}

fn unset_here() -> bool {
    unset(
        std::env::var(KERNEL_CLOCK_ENVIRONMENT).ok().as_deref(),
        lamp_ipc::clock_synced(),
    )
}

/// The same for this machine, now; with `places` (`LAMP_TIME_PLACES`), a
/// session that may ask the time is told whose time it is
/// ([`told_place`]).
pub fn told_here(clock: bool, places: bool) -> Option<String> {
    if unset_here() {
        return Some(TOLD_UNSET.to_owned());
    }
    let (file, now) = here()?;
    if clock {
        let told = told_with_clock(&file, now)?;
        Some(if places {
            told + &told_place(zone_here().as_deref())
        } else {
            told
        })
    } else {
        told(&file, now)
    }
}

/// [`told_asked`] for this machine, now, with `clock` (`LAMP_CLOCK`);
/// with `places` (`LAMP_TIME_PLACES`), whose time it is and to ask
/// [`crate::tools::THE_TIME`] for anywhere else ([`told_place`]). Without
/// `clock`, what a session without it is told ([`told_here`]), as before
/// (the first builder on #1921).
pub fn told_asked_here(clock: bool, places: bool) -> Option<String> {
    if !clock {
        return told_here(false, false);
    }
    if unset_here() {
        return Some(TOLD_UNSET.to_owned());
    }
    let (file, now) = here()?;
    let told = told_asked(&file, now)?;
    Some(if places {
        told + &told_place(zone_here().as_deref())
    } else {
        told
    })
}

/// The time now where the lamp stands, exactly, in words; `Err` with what
/// to say when the clock is known not to be set.
pub fn now_here() -> Result<Option<String>, &'static str> {
    if unset_here() {
        return Err(SAID_UNSET);
    }
    let Some((file, now)) = here() else {
        return Ok(None);
    };
    Ok(said(&file, now))
}

/// [`now_here`] naming whose time it is, for `LAMP_TIME_PLACES`: with the
/// switch, every answer of the function says its place, its own one too
/// (the reviewer on #1589), so the model never gives it as another's.
pub fn now_here_named() -> Result<Option<String>, &'static str> {
    if unset_here() {
        return Err(SAID_UNSET);
    }
    let Some((file, now)) = here() else {
        return Ok(None);
    };
    Ok(said_here(&file, zone_here().as_deref(), now))
}

/// [`said`], naming the lamp's own place when its zone is known: "Here in
/// Ho Chi Minh it is 7:32 in the evening, …", or "Here it is …" for a zone
/// that names no place ([`own_place`]).
pub fn said_here(file: &[u8], zone: Option<&str>, now: i64) -> Option<String> {
    match zone {
        Some(zone) => said_after(&format!("{} it is", here_in(zone)), file, now),
        None => said(file, now),
    }
}

/// Whether `now` is night in the zone of this compiled zone file: from
/// 21:00 to 4:59, as [`words`] calls it "at night". Nothing when the zone
/// cannot be worked out.
pub fn night(file: &[u8], now: i64) -> Option<bool> {
    let local = now.checked_add(i64::from(offset_at(file, now)?))?;
    Some(!(5..=20).contains(&(local.rem_euclid(86_400) / 3_600)))
}

/// Whether it is night, as the lamp should take it: never while its clock
/// is known not to be set ([`unset`]). The lamp has no battery clock: until
/// a time client sets it, it shows the last time it saved, so a lamp
/// switched off at 22:00 and on at 09:00 where NTP is blocked would read
/// night in the morning (the reviewer on #1083). Nor when the zone cannot
/// be worked out: day is the stricter rule.
pub fn night_taken(clock_unset: bool, file: &[u8], now: i64) -> bool {
    !clock_unset && night(file, now) == Some(true)
}

/// The same for this machine, now. A zone or clock that cannot be read is
/// day.
pub fn night_here() -> bool {
    let unset = unset_here();
    here().is_some_and(|(file, now)| night_taken(unset, &file, now))
}

/// What every hello is told after the part of the day ([`hello_told`]).
pub const HELLO_ONLY: &str = "Say only a hello and their name if you know it: no weather, no day or date, no time, no question and no offer.";

/// What a hello's note (Welcome back's, `who = true`) is told of the time:
/// the part of the day at `now` in the zone of this compiled zone file, as
/// [`words`] calls it, and to say a hello and the name only. At 15:42 on
/// lamp-4ace a hello said "Evening" (the model picked the part itself) and
/// added the weather, the weekday and "how's it going"; the first builder,
/// 30 Oct, on the launch testers' findings: the part of the day from the
/// lamp's local time, never the model's own. At night, or with no zone or
/// time to work it out from, it names no part of the day at all.
pub fn hello_told(file: Option<&[u8]>, now: i64) -> String {
    let part = file
        .and_then(|file| words(file, now))
        .map(|(.., part)| part);
    let when = match part {
        Some("in the morning") => Some("the morning"),
        Some("in the afternoon") => Some("the afternoon"),
        Some("in the evening") => Some("the evening"),
        _ => None,
    };
    match (part, when) {
        (_, Some(when)) => format!(
            "\n\n(It is {when} here now: if you name the part of the day, it is {when}, never another. {HELLO_ONLY})"
        ),
        (Some(_), None) => format!(
            "\n\n(It is night here now: do not name the part of the day, just say hello. {HELLO_ONLY})"
        ),
        (None, _) => format!(
            "\n\n(You do not know the part of the day here: do not name it, just say hello. {HELLO_ONLY})"
        ),
    }
}

/// [`hello_told`] for this machine, now: no part of the day while its clock
/// is known not to be set ([`unset`]) or its zone cannot be read.
pub fn hello_told_here() -> String {
    match here().filter(|_| !unset_here()) {
        Some((file, now)) => hello_told(Some(&file), now),
        None => hello_told(None, 0),
    }
}

fn here() -> Option<(Vec<u8>, i64)> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    let file = std::fs::read("/etc/localtime").ok()?;
    Some((file, i64::try_from(now).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A day the memory names reads back as the day it is, and as a person
    /// says it; what is no day is nothing (#1693, `LAMP_MEMORY_EVENTS`).
    #[test]
    fn a_named_day_is_read_back_and_said_as_a_person_would() {
        assert_eq!(day_of("1970-01-01"), Some(0));
        let saturday = day_of("2026-10-24").unwrap();
        assert_eq!(civil(saturday), (2026, 10, 24));
        assert_eq!(day_said(saturday), "Saturday 24 October");
        assert_eq!(weekday(day_of("2026-10-21").unwrap()), "Wednesday");
        assert_eq!(
            day_said(day_of("2028-02-29").unwrap()),
            "Tuesday 29 February"
        );
        for bad in [
            "2026-02-30",
            "2027-02-29",
            "2026-13-01",
            "2026-00-10",
            "2026-1-24",
            "24/10/2026",
            "2026-10-24T00",
            "+026-10-24",
            "",
        ] {
            assert_eq!(day_of(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_clock_is_unset_only_with_the_switch_on_and_no_set_from_the_kernel() {
        assert!(!unset(None, Some(false)));
        assert!(!unset(Some("off"), None));
        assert!(!unset(Some("on"), Some(true)));
        assert!(unset(Some("on"), Some(false)));
        assert!(unset(Some("on"), None));
        assert!(TOLD_UNSET.contains("not been set") && SAID_UNSET.contains("not been set"));
    }

    /// A zone file: `changes` are (moment, kind), `offsets` the seconds east
    /// of each kind, `rule` the line after the second table.
    fn zone(version: u8, changes: &[(i64, u8)], offsets: &[i32], rule: &str) -> Vec<u8> {
        let table = |wide: bool| {
            let mut bytes = b"TZif".to_vec();
            bytes.push(version);
            bytes.extend([0; 15]);
            for count in [0, 0, 0, changes.len(), offsets.len(), 4] {
                bytes.extend((count as u32).to_be_bytes());
            }
            for (moment, _) in changes {
                if wide {
                    bytes.extend(moment.to_be_bytes());
                } else {
                    bytes.extend((*moment as i32).to_be_bytes());
                }
            }
            bytes.extend(changes.iter().map(|(_, kind)| *kind));
            for offset in offsets {
                bytes.extend(offset.to_be_bytes());
                bytes.extend([0, 0]);
            }
            bytes.extend(b"LMT\0");
            bytes
        };
        let mut file = table(false);
        if version >= b'2' {
            file.extend(table(true));
            file.extend(format!("\n{rule}\n").bytes());
        }
        file
    }

    // 2026-10-13 04:14:00 UTC, a Tuesday.
    const NOW: i64 = 1_791_864_840;

    /// A zone file as a slim one is written: no change listed, its closing
    /// rule only, so every moment is worked out from the rule.
    fn ruled(standard: i32, rule: &str) -> Vec<u8> {
        zone(b'2', &[], &[standard], rule)
    }

    /// #1547: the closing rule worked out across a change, both ways, on
    /// the moments the platform's own zone files give (Python's zoneinfo,
    /// 2026): London and New York in and out of summer time, Sydney's
    /// summer across the new year, Tokyo with none.
    #[test]
    fn a_zone_s_closing_rule_is_worked_out_across_its_changes() {
        let london = ruled(0, "GMT0BST,M3.5.0/1,M10.5.0");
        let new_york = ruled(-18_000, "EST5EDT,M3.2.0,M11.1.0");
        let sydney = ruled(36_000, "AEST-10AEDT,M10.1.0,M4.1.0/3");
        let tokyo = ruled(32_400, "JST-9");
        for (file, at, offset) in [
            (&london, 1_774_744_200, 0),         // 29 Mar 00:30 UTC
            (&london, 1_774_747_800, 3_600),     // 29 Mar 01:30 UTC
            (&london, 1_792_888_200, 3_600),     // 25 Oct 00:30 UTC
            (&london, 1_792_891_800, 0),         // 25 Oct 01:30 UTC
            (&new_york, 1_793_511_000, -14_400), // 1 Nov 05:30 UTC
            (&new_york, 1_793_514_600, -18_000), // 1 Nov 06:30 UTC
            (&sydney, 1_768_435_200, 39_600),    // 15 Jan
            (&sydney, 1_782_864_000, 36_000),    // 1 Jul
            (&tokyo, 1_792_316_400, 32_400),
        ] {
            assert_eq!(offset_at(file, at), Some(offset), "{at}");
        }
        // A rule of the other forms POSIX writes is worked out too; one
        // that is no rule gives nothing.
        assert_eq!(
            offset_at(&ruled(0, "XXX0YYY,J60,J300"), 1_792_316_400),
            Some(3_600)
        );
        assert_eq!(
            offset_at(&ruled(0, "GMT0BST,M3.6.0,M10.5.0"), 1_792_316_400),
            None
        );
    }

    /// #1547: lamp-4ace on 18 October at 16:40, "what time is it in
    /// London", answered with its own time. Now London's own, said aloud,
    /// with the lamp's beside it; and the lamp's own place said once.
    #[test]
    fn another_place_s_time_names_the_place_and_the_lamp_s_own_beside_it() {
        let saigon = ruled(25_200, "<+07>-7");
        let london = ruled(0, "GMT0BST,M3.5.0/1,M10.5.0");
        let at = 1_792_316_400; // 2026-10-18 09:40 UTC
        assert_eq!(
            said_in(
                "Europe/London",
                &london,
                Some(("Asia/Ho_Chi_Minh", &saigon)),
                at
            )
            .as_deref(),
            Some(
                "In London it is 10:40 in the morning, Sunday 18 October 2026. Said aloud: twenty to eleven in the morning. Here in Ho Chi Minh it is 4:40 in the afternoon."
            )
        );
        assert_eq!(
            said_in(
                "Asia/Ho_Chi_Minh",
                &saigon,
                Some(("Asia/Ho_Chi_Minh", &saigon)),
                at
            )
            .as_deref(),
            Some(
                "In Ho Chi Minh it is 4:40 in the afternoon, Sunday 18 October 2026. Said aloud: twenty to five in the afternoon. That is the time here too."
            )
        );
        // An alias of the lamp's own zone is its own time too: the same
        // file (the reviewer on #1552).
        assert!(
            said_in(
                "Asia/Saigon",
                &saigon,
                Some(("Asia/Ho_Chi_Minh", &saigon)),
                at
            )
            .unwrap()
            .ends_with("That is the time here too.")
        );
        // The lamp's own time names its place too (the reviewer on #1589).
        assert_eq!(
            said_here(&saigon, Some("Asia/Ho_Chi_Minh"), at).as_deref(),
            Some(
                "Here in Ho Chi Minh it is 4:40 in the afternoon, Sunday 18 October 2026. Said aloud: twenty to five in the afternoon."
            )
        );
        assert_eq!(said_here(&saigon, None, at), said(&saigon, at));
        // A lamp whose zone names no place says only "here" (Ten on #1602).
        for zone in [
            "Etc/GMT-7",
            "posix/Etc/GMT-7",
            "Etc/UTC",
            "UTC",
            "Zulu",
            // Named no place either (the reviewer on #1609).
            "Factory",
            "CET",
            "EST5EDT",
            "US/Pacific",
            "Mexico/General",
            "Brazil/East",
            "right/US/Eastern",
        ] {
            assert_eq!(
                said_here(&saigon, Some(zone), at).as_deref(),
                Some(
                    "Here it is 4:40 in the afternoon, Sunday 18 October 2026. Said aloud: twenty to five in the afternoon."
                ),
                "{zone}"
            );
            assert!(
                said_in("Europe/London", &london, Some((zone, &saigon)), at)
                    .unwrap()
                    .ends_with(
                        "twenty to eleven in the morning. Here it is 4:40 in the afternoon."
                    ),
                "{zone}"
            );
            assert_eq!(
                told_place(Some(zone)),
                " This is the time where you are. For the time anywhere else, call the_time with that place, never this time.",
                "{zone}"
            );
        }
        // Asked for by the person, an offset is said as they wrote it.
        assert!(
            said_in("Etc/GMT-7", &saigon, None, at)
                .unwrap()
                .starts_with("In UTC+7 it is 4:40 in the afternoon")
        );
        for (zone, place) in [
            ("Asia/Ho_Chi_Minh", "Ho Chi Minh"),
            ("posix/Asia/Ho_Chi_Minh", "Ho Chi Minh"),
            ("right/Europe/London", "London"),
            ("America/Argentina/Buenos_Aires", "Buenos Aires"),
            ("Antarctica/Troll", "Troll"),
        ] {
            assert_eq!(own_place(zone).as_deref(), Some(place), "{zone}");
        }
        let new_york = ruled(-18_000, "EST5EDT,M3.2.0,M11.1.0");
        assert!(
            said_in("America/New_York", &new_york, None, at)
                .unwrap()
                .starts_with("In New York it is 5:40 in the morning, Sunday 18 October 2026.")
        );
        assert_eq!(
            told_place(Some("Asia/Ho_Chi_Minh")),
            " This is the time where you are, in Ho Chi Minh. For the time anywhere else, call the_time with that place, never this time."
        );
    }

    /// A place is looked for under the zone folder only, by its zone name
    /// or its name alone in the usual areas, and must be a zone file.
    /// #1712: the unit's place goes over the zone's in what the model is
    /// told and in "here"'s time: lamp-4ace in Hanoi keeps
    /// `Asia/Ho_Chi_Minh`. A strange value is no place.
    #[test]
    fn the_units_place_goes_over_the_zones() {
        assert_eq!(
            place_from(Some(" Hanoi, Vietnam ")).as_deref(),
            Some("Hanoi, Vietnam")
        );
        assert_eq!(place_from(Some("Đà Nẵng")).as_deref(), Some("Đà Nẵng"));
        for odd in [
            None,
            Some(""),
            Some("  "),
            Some("Hanoi; rm -rf"),
            Some("x\ny"),
        ] {
            assert_eq!(place_from(odd), None, "{odd:?}");
        }
        assert_eq!(place_from(Some(&"a".repeat(61))), None);
        let zone = Some("Asia/Ho_Chi_Minh");
        assert!(told_place_at(None, zone).contains("in Ho Chi Minh."));
        assert!(told_place_at(Some("Hanoi"), zone).contains("in Hanoi."));
        assert_eq!(
            here_in_at(Some("Hanoi"), "Asia/Ho_Chi_Minh"),
            "Here in Hanoi"
        );
        assert_eq!(here_in_at(None, "Asia/Ho_Chi_Minh"), "Here in Ho Chi Minh");
        assert_eq!(here_in_at(None, "Etc/GMT-7"), "Here");
        // #4337: with no place, it says so, and either way the area near
        // the person is asked, never picked.
        assert_eq!(
            place_told(None).unwrap(),
            format!(" You don't know where you are beyond your time zone. {AREA_ASKED}")
        );
        assert_eq!(
            place_told(Some("Hanoi")).unwrap(),
            format!(
                " You are in Hanoi. When someone asks about the weather, the news or anything nearby without naming a place, they mean Hanoi. {AREA_ASKED}"
            )
        );
        assert!(AREA_ASKED.contains("ask which area") && AREA_ASKED.contains("never pick"));
        // Aimed at a mishearing, and never at the weather or the time (the
        // Reviewer-lead's and Reviewer-three's P3s on #4350).
        assert!(AREA_ASKED.contains("aren't sure you heard right"));
        assert!(AREA_ASKED.contains("never ask which area for those"));
    }

    #[test]
    fn a_place_is_found_by_its_zone_or_its_name_and_never_outside_the_zones() {
        let dir = std::env::temp_dir().join(format!("lamp-zones-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for zone in ["Europe/London", "America/New_York", "UTC"] {
            let path = dir.join(zone);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, ruled(0, "GMT0BST,M3.5.0/1,M10.5.0")).unwrap();
        }
        std::fs::write(dir.join("Europe/notes"), "not a zone").unwrap();
        let found = |place: &str| zone_file(&dir, place).map(|(zone, _)| zone);
        assert_eq!(found("Europe/London").as_deref(), Some("Europe/London"));
        assert_eq!(found("London").as_deref(), Some("Europe/London"));
        assert_eq!(found("New York").as_deref(), Some("America/New_York"));
        assert_eq!(found(" UTC ").as_deref(), Some("UTC"));
        // A top-level fixed zone is not a place (the reviewer on #1552).
        std::fs::write(dir.join("EST"), ruled(-18_000, "EST5")).unwrap();
        assert_eq!(found("EST"), None);
        for refused in [
            "Middle_Earth/Rivendell",
            "Europe/notes",
            "../etc/passwd",
            "Europe/../UTC",
            "/etc/localtime",
            "Europe/",
            "",
            "Europe/London;rm",
        ] {
            assert_eq!(found(refused), None, "{refused:?}");
        }
        assert_eq!(place_name("America/Argentina/Buenos_Aires"), "Buenos Aires");
        // The lamp's own zone is where `/etc/localtime` links, a stale
        // `/etc/timezone` beside it notwithstanding.
        let link = std::path::Path::new("/usr/share/zoneinfo/Asia/Ho_Chi_Minh");
        assert_eq!(
            zone_named(Some(link), Some("Etc/UTC\n")).as_deref(),
            Some("Asia/Ho_Chi_Minh")
        );
        assert_eq!(
            zone_named(None, Some("Etc/UTC\n")).as_deref(),
            Some("Etc/UTC")
        );
        assert_eq!(zone_named(None, Some("../x")), None);
        assert_eq!(zone_named(None, None), None);
        // A link outside a zone folder leaves the text to name the zone.
        let elsewhere = std::path::Path::new("/opt/zones/hcm");
        assert_eq!(
            zone_named(Some(elsewhere), Some("Asia/Ho_Chi_Minh\n")).as_deref(),
            Some("Asia/Ho_Chi_Minh")
        );
        assert_eq!(zone_named(Some(elsewhere), None), None);
        // The `Etc/GMT` zones are said as people write them: POSIX's sign
        // is turned.
        assert_eq!(place_name("Etc/GMT-7"), "UTC+7");
        assert_eq!(place_name("posix/Etc/GMT-7"), "UTC+7");
        assert_eq!(place_name("right/Etc/GMT+5"), "UTC-5");
        assert_eq!(place_name("Etc/GMT-0"), "UTC");
        assert_eq!(place_name("Etc/GMT+0"), "UTC");
        assert_eq!(place_name("Etc/GMT-14"), "UTC+14");
        assert_eq!(place_name("Etc/GMT+5"), "UTC-5");
        assert_eq!(place_name("Etc/GMT"), "UTC");
        assert_eq!(place_name("Etc/GMT0"), "UTC");
        assert_eq!(place_name("Etc/UTC"), "UTC");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn it_reads_the_offset_now_from_a_zone_file_of_either_kind() {
        // Ho Chi Minh City as the lamp has it: its last change in 1975.
        let saigon = zone(
            b'2',
            &[(-2_004_073_600, 1), (171_820_800, 2)],
            &[25_590, 28_800, 25_200],
            // The rule names the zone as its last change does: a file
            // whose two disagree is no zone file.
            "LMT-7",
        );
        assert_eq!(offset_at(&saigon, NOW), Some(25_200));
        assert_eq!(offset_at(&saigon, 0), Some(28_800));
        assert_eq!(offset_at(&saigon, -3_000_000_000), Some(25_590));
        // An old file with one table and no rule, and one with no change at all.
        assert_eq!(
            offset_at(&zone(0, &[(100, 1)], &[0, 3_600], ""), NOW),
            Some(3_600)
        );
        assert_eq!(offset_at(&zone(b'2', &[], &[0], "UTC0"), NOW), Some(0));
        // Summer time: told from the listed changes, and past them from the
        // rule (#820). A file that lists New York's change of 8 March 2026
        // and no later one: 1 November's change is worked out from its
        // rule, where before the lamp's own time was nothing.
        let listed = zone(
            b'3',
            &[(1_772_953_200, 1)],
            &[-18_000, -14_400],
            "LMT5LMT,M3.2.0,M11.1.0",
        );
        assert_eq!(offset_at(&listed, 1_772_953_199), Some(-18_000));
        assert_eq!(offset_at(&listed, NOW), Some(-14_400));
        assert_eq!(offset_at(&listed, 1_793_512_799), Some(-14_400));
        assert_eq!(offset_at(&listed, 1_793_512_800), Some(-18_000));
        // Not a zone file, a cut one, an absurd one.
        assert_eq!(offset_at(b"", NOW), None);
        assert_eq!(offset_at(&saigon[..60], NOW), None);
        assert_eq!(offset_at(&zone(0, &[], &[400_000], ""), NOW), None);
        let mut wrong = saigon.clone();
        wrong[0] = b'X';
        assert_eq!(offset_at(&wrong, NOW), None);
    }

    /// Night is 21:00 to 4:59 where the lamp stands, as its words say "at
    /// night" (#829): lamp-4ace's 03:28 call was at night, the first
    /// builder's 15:23 table by day.
    #[test]
    fn night_is_from_nine_in_the_evening_to_five_in_the_morning_where_the_lamp_stands() {
        let saigon = zone(b'2', &[], &[25_200], "<+07>-7");
        // NOW is 11:14 there: its midnight, and a time of day after it.
        let at = |hour: i64, minute: i64| NOW - (11 * 3_600 + 14 * 60) + hour * 3_600 + minute * 60;
        for (hour, minute, is_night) in [
            (3, 28, true),
            (4, 59, true),
            (5, 0, false),
            (15, 23, false),
            (20, 59, false),
            (21, 0, true),
            (23, 59, true),
        ] {
            assert_eq!(
                night(&saigon, at(hour, minute)),
                Some(is_night),
                "{hour}:{minute:02}"
            );
        }
        // The same moment is 4:14 at night in UTC.
        assert_eq!(night(&zone(b'2', &[], &[0], "UTC0"), NOW), Some(true));
        assert_eq!(night(b"", NOW), None);
        // Taken as night only with the clock set, and a zone read.
        assert!(night_taken(false, &saigon, at(3, 28)));
        assert!(!night_taken(true, &saigon, at(3, 28)));
        assert!(!night_taken(false, b"", at(3, 28)));
        assert!(!night_taken(false, &saigon, at(15, 23)));
    }

    #[test]
    fn it_tells_the_day_and_the_part_of_it() {
        let zone_at = |offset: i32| zone(b'2', &[], &[offset], "");
        let said = |offset: i32, now: i64| told(&zone_at(offset), now).unwrap();
        assert!(
            said(25_200, NOW)
                .contains("It is Tuesday 13 October 2026, around 11:14 in the morning."),
            "{}",
            said(25_200, NOW)
        );
        assert!(said(0, NOW).contains("Tuesday 13 October 2026, around 4:14 at night."));
        // The day turns with the zone, and so do the month and the year.
        assert!(said(-36_000, NOW).contains("Monday 12 October 2026, around 6:14 in the evening."));
        assert!(said(0, 0).contains("Thursday 1 January 1970, around 12:00 at night."));
        assert!(said(-3_600, 0).contains("Wednesday 31 December 1969, around 11:00 at night."));
        assert!(
            said(0, 951_782_400 + 12 * 3_600)
                .contains("Tuesday 29 February 2000, around 12:00 in the afternoon.")
        );
        // No offset to be had, nothing told.
        assert_eq!(told(b"TZif", NOW), None);
    }

    #[test]
    fn the_model_that_asks_is_given_the_minute_and_no_hedge() {
        let saigon = zone(b'2', &[], &[25_200], "<+07>-7");
        assert_eq!(
            said(&saigon, NOW).unwrap(),
            "It is 11:14 in the morning, Tuesday 13 October 2026. Said aloud: eleven fourteen in the morning."
        );
        // Thirty-three minutes on, as the session opened at 10:48 on
        // 2026-10-14 was asked at 11:17: the minute moves with the clock.
        assert_eq!(
            said(&saigon, NOW + 33 * 60).unwrap(),
            "It is 11:47 in the morning, Tuesday 13 October 2026. Said aloud: eleven forty-seven in the morning."
        );
        assert_eq!(said(b"TZif", NOW), None);
        // Told on opening when it may ask: the part of the day, and to ask.
        let opening = told_with_clock(&saigon, NOW).unwrap();
        assert!(
            opening.contains("You woke on Tuesday 13 October 2026, around 11:14 in the morning")
        );
        assert!(opening.contains("call the_time first") && !opening.contains("roughly"));
    }

    /// #1712: a request built as it is asked (the second route) is told
    /// the time then, exactly and said aloud, to answer from, never to
    /// call the_time for it nor that it may be old.
    #[test]
    fn a_request_built_as_it_is_asked_is_told_the_time_to_answer_from() {
        let saigon = zone(b'2', &[], &[25_200], "<+07>-7");
        assert_eq!(
            told_asked(&saigon, NOW).unwrap(),
            "\n\nThe time\nIt is 11:14 in the morning, Tuesday 13 October 2026. Said aloud: eleven fourteen in the morning. Your clock read this as you were asked: when someone asks the time or the date, or how long until something, answer from it."
        );
        assert!(
            told_asked(&saigon, NOW + 33 * 60)
                .unwrap()
                .contains("11:47")
        );
        assert_eq!(told_asked(b"TZif", NOW), None);
    }

    #[test]
    fn a_time_is_said_as_a_person_says_it_with_its_hour() {
        assert_eq!(aloud(6, 23), "six twenty-three");
        assert_eq!(aloud(6, 0), "six o'clock");
        assert_eq!(aloud(6, 15), "quarter past six");
        assert_eq!(aloud(6, 30), "half past six");
        assert_eq!(aloud(6, 45), "quarter to seven");
        assert_eq!(aloud(6, 10), "ten past six");
        assert_eq!(aloud(6, 50), "ten to seven");
        assert_eq!(aloud(12, 35), "twenty-five to one");
        assert_eq!(aloud(12, 42), "twelve forty-two");
        assert_eq!(aloud(12, 59), "twelve fifty-nine");
        assert_eq!(aloud(11, 31), "eleven thirty-one");
        assert_eq!(aloud(12, 1), "twelve oh one");
        assert_eq!(aloud(6, 7), "six oh seven");
        assert_eq!(aloud(9, 11), "nine eleven");
        assert_eq!(aloud(9, 40), "twenty to ten");
        for hour in 1..=12 {
            for minute in 0..60 {
                let said = aloud(hour, minute);
                assert!(
                    !said.starts_with(' ') && !said.contains("  ") && !said.ends_with(' '),
                    "{said}"
                );
            }
        }
    }

    /// The first builder, 30 Oct, on the launch testers' findings: a hello
    /// at 15:42 in Ho Chi Minh said "Evening". Its note is told the part of
    /// the day from the lamp's own time, none at night or with no zone, and
    /// to say a hello and the name only.
    #[test]
    fn a_hello_is_told_the_part_of_the_day_from_the_lamps_own_time() {
        let saigon = zone(b'2', &[], &[25_200], "<+07>-7");
        // 2026-10-30 00:00 local in Ho Chi Minh, 29 Oct 17:00 UTC.
        let midnight = 1_793_293_200;
        let at = |hour: i64, minute: i64| midnight + hour * 3_600 + minute * 60;
        let told = |hour, minute| hello_told(Some(&saigon), at(hour, minute));
        let afternoon = told(15, 42);
        assert_eq!(
            afternoon,
            format!(
                "\n\n(It is the afternoon here now: if you name the part of the day, it is the afternoon, never another. {HELLO_ONLY})"
            )
        );
        assert!(!afternoon.contains("evening") && !afternoon.contains("morning"));
        assert!(told(9, 0).contains("It is the morning here now"));
        assert!(told(18, 0).contains("It is the evening here now"));
        let night = told(23, 0);
        assert!(night.contains("do not name the part of the day"));
        assert!(!night.contains("the evening") && !night.contains("the morning"));
        for unknown in [
            hello_told(None, at(15, 42)),
            hello_told(Some(b"no zone"), at(15, 42)),
        ] {
            assert!(unknown.contains("You do not know the part of the day here"));
            assert!(unknown.ends_with(&format!("{HELLO_ONLY})")));
        }
        for hello in [told(15, 42), told(9, 0), told(18, 0), told(23, 0)] {
            assert!(hello.starts_with("\n\n(") && hello.ends_with(&format!("{HELLO_ONLY})")));
        }
    }
}
