//! The lamp's local time (#820): a fixed offset east of UTC, as the parts had
//! it (`--utc-offset-minutes`), or a time zone whose offset is taken at each
//! moment (`--zone`), so the hour of the day and the day's boundary stay right
//! across a daylight-saving change in a home outside Vietnam.
//!
//! `--zone system` follows `/etc/localtime`. The lamp's setup points it at the
//! phone's zone by renaming a new link over it (`crates/setup`), often after
//! the parts have started with the image's zone. So the file is looked at again
//! at every reading (three `stat` calls and a `readlink`) and read again only when it changed: a
//! new zone counts from the next reading, without a restart. `TZ` is not read;
//! the lamp's units do not set it, and the file is what setup writes.

use jiff::{
    Timestamp,
    tz::{TimeZone, TimeZoneDatabase},
};
use std::{
    fs::{self, File},
    io::{self, Read},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError},
};

/// The lamp's quiet hours, 22:00 to 07:00 local (`docs/control.md`,
/// "Limits, rates and quiet hours"): the one definition every part that
/// holds a computer to them reads (Fourteen and Nine on #3504), until the
/// app has an owner's setting.
pub const QUIET_FROM_MINUTE: u16 = 22 * 60;
pub const QUIET_TO_MINUTE: u16 = 7 * 60;

/// Whether the local `minute_of_day` (0 to 1439) is in the quiet hours.
pub fn quiet_hours(minute_of_day: u16) -> bool {
    QuietWindow::DEFAULT.contains(minute_of_day)
}

/// A lamp's own quiet hours, local minutes from `from` up to `to`, across
/// midnight when `to` comes first: lamp-control's `--quiet-hours`, read
/// once at its start. [`QuietWindow::DEFAULT`] is the one above.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuietWindow {
    pub from: u16,
    pub to: u16,
}

impl QuietWindow {
    pub const DEFAULT: Self = Self {
        from: QUIET_FROM_MINUTE,
        to: QUIET_TO_MINUTE,
    };

    /// Whether the local `minute_of_day` is inside the window.
    pub fn contains(&self, minute_of_day: u16) -> bool {
        if self.from < self.to {
            (self.from..self.to).contains(&minute_of_day)
        } else {
            !(self.to..self.from).contains(&minute_of_day)
        }
    }

    /// `HH:MM-HH:MM` as a window, or `off` as none; anything else, or a
    /// window whose ends are the same minute, is refused with why.
    pub fn parse(text: &str) -> Result<Option<Self>, String> {
        let text = text.trim();
        if text == "off" {
            return Ok(None);
        }
        let minute = |end: &str| -> Option<u16> {
            let (hour, minute) = end.split_once(':')?;
            let all_digits =
                |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
            if !all_digits(hour) || hour.len() > 2 || !all_digits(minute) || minute.len() != 2 {
                return None;
            }
            let (hour, minute): (u16, u16) = (hour.parse().ok()?, minute.parse().ok()?);
            (hour < 24 && minute < 60).then_some(hour * 60 + minute)
        };
        let window = text
            .split_once('-')
            .and_then(|(from, to)| {
                Some(Self {
                    from: minute(from)?,
                    to: minute(to)?,
                })
            })
            .ok_or_else(|| format!("{text:?}: HH:MM-HH:MM, such as 22:00-07:00, or off"))?;
        if window.from == window.to {
            return Err(format!("{text:?}: its two ends are the same minute"));
        }
        Ok(Some(window))
    }
}

impl std::fmt::Display for QuietWindow {
    /// "22:00 to 07:00".
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let hhmm = |minute: u16| format!("{:02}:{:02}", minute / 60, minute % 60);
        write!(f, "{} to {}", hhmm(self.from), hhmm(self.to))
    }
}

/// The system's zone file, or a link to one.
pub const SYSTEM_LOCALTIME: &str = "/etc/localtime";
/// The system's zone by name, read when the zone file cannot be.
pub const SYSTEM_TIMEZONE: &str = "/etc/timezone";

/// A zone file is a few kilobytes; nothing longer is read.
const MOST_BYTES: u64 = 1 << 20;
const DAY_MS: i128 = 86_400_000;

/// Where the lamp is in time.
#[derive(Debug)]
pub enum LocalTime {
    /// Minutes east of UTC, the same all year (`--utc-offset-minutes`).
    Offset(i32),
    /// A named zone (`--zone Asia/Ho_Chi_Minh`).
    Zone(TimeZone),
    /// The system's zone (`--zone system`), followed as it changes.
    System(SystemZone),
}

impl LocalTime {
    /// `system`, or an IANA zone name. The zone data is bundled, so a lamp
    /// missing its own still knows the zones. No silent UTC: a zone that
    /// cannot be found is an error.
    pub fn zone(name: &str) -> io::Result<Self> {
        if name == "system" {
            return SystemZone::open(SYSTEM_LOCALTIME, SYSTEM_TIMEZONE).map(Self::System);
        }
        TimeZone::get(name)
            .map(Self::Zone)
            .map_err(|error| io::Error::other(format!("time zone {name} is unknown: {error}")))
    }

    /// Minutes east of UTC at the moment `unix_ms`.
    pub fn offset_minutes_at(&self, unix_ms: u64) -> i32 {
        let zoned = |zone: &TimeZone| {
            i64::try_from(unix_ms)
                .ok()
                .and_then(|ms| Timestamp::from_millisecond(ms).ok())
                .map_or(0, |at| zone.to_offset(at).seconds() / 60)
        };
        match self {
            Self::Offset(minutes) => *minutes,
            Self::Zone(zone) => zoned(zone),
            Self::System(system) => zoned(&system.current()),
        }
    }

    /// The zone's IANA name, for a part that keeps a zone by name (Schedule,
    /// #820): a named zone's own, or the system's as it is now. Nothing for a
    /// fixed offset, or a system zone that names none.
    pub fn name(&self) -> Option<String> {
        match self {
            Self::Offset(_) => None,
            Self::Zone(zone) => zone.iana_name().map(str::to_owned),
            Self::System(system) => system.name(),
        }
    }

    /// The local day of `unix_ms`, in days since 1970-01-01.
    pub fn day(&self, unix_ms: u64) -> i64 {
        self.local_ms(unix_ms).div_euclid(DAY_MS) as i64
    }

    /// The local minute of the day at `unix_ms`, 0 to 1439.
    pub fn minute_of_day(&self, unix_ms: u64) -> u16 {
        (self.local_ms(unix_ms).rem_euclid(DAY_MS) / 60_000) as u16
    }

    /// The local hour of the day at `unix_ms`, 0 to 23.
    pub fn hour_of_day(&self, unix_ms: u64) -> u8 {
        (self.minute_of_day(unix_ms) / 60) as u8
    }

    fn local_ms(&self, unix_ms: u64) -> i128 {
        i128::from(unix_ms) + i128::from(self.offset_minutes_at(unix_ms)) * 60_000
    }
}

/// Seconds east of UTC at `unix_s` (seconds since 1970 UTC) in the zone of
/// a compiled zone file (RFC 8536) already read: its listed changes, and
/// past them its closing rule, so a slim file's summer time is worked out as
/// a full one's. For a part that holds a zone's file itself, as the voice
/// does for another place's time and to tell two zones apart (#820).
/// Nothing for bytes that are no zone file.
pub fn offset_seconds_in(tzif: &[u8], unix_s: i64) -> Option<i32> {
    let zone = TimeZone::tzif("zone", tzif).ok()?;
    let at = Timestamp::from_second(unix_s).ok()?;
    Some(zone.to_offset(at).seconds())
}

/// The system's zone: the zone file at `localtime` (a link to one, as setup
/// makes it, or a copy), and, when that cannot be read, the zone `timezone`
/// names.
#[derive(Debug)]
pub struct SystemZone {
    localtime: PathBuf,
    timezone: PathBuf,
    /// Boxed: what is looked at is far larger than the other kinds of local
    /// time, and only this kind needs it.
    state: Box<Mutex<State>>,
}

#[derive(Debug)]
struct State {
    /// The files as they were when `zone` was read.
    seen: Seen,
    zone: TimeZone,
}

/// One file's identity: renaming a new one over it, or writing it in place,
/// changes it.
type Identity = (u64, u64, u64, i64, i64);

fn identity(metadata: &fs::Metadata) -> Identity {
    (
        metadata.dev(),
        metadata.ino(),
        metadata.len(),
        metadata.mtime(),
        metadata.mtime_nsec(),
    )
}

/// Everything the zone is read from, each looked at on its own (#844): the
/// link, even when what it points at is gone; where it points; the file it
/// leads to; and the name file read when that cannot be. `None` for one that
/// is not there.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Seen {
    link: Option<Identity>,
    target: Option<PathBuf>,
    file: Option<Identity>,
    named: Option<Identity>,
}

fn look(localtime: &Path, timezone: &Path) -> Seen {
    Seen {
        link: fs::symlink_metadata(localtime).ok().map(|m| identity(&m)),
        target: fs::read_link(localtime).ok(),
        file: fs::metadata(localtime).ok().map(|m| identity(&m)),
        named: fs::metadata(timezone).ok().map(|m| identity(&m)),
    }
}

/// `America/New_York` from a link to `/usr/share/zoneinfo/America/New_York`.
fn name_of_link(localtime: &Path) -> Option<String> {
    let target = fs::read_link(localtime).ok()?;
    let target = target.to_str()?;
    let (_, name) = target.rsplit_once("zoneinfo/")?;
    (!name.is_empty()).then(|| name.to_owned())
}

fn read(localtime: &Path, timezone: &Path) -> io::Result<TimeZone> {
    let from_file = (|| {
        let mut data = Vec::new();
        File::open(localtime)?
            .take(MOST_BYTES)
            .read_to_end(&mut data)?;
        let name = name_of_link(localtime);
        TimeZone::tzif(name.as_deref().unwrap_or("localtime"), &data).map_err(io::Error::other)
    })();
    from_file.or_else(|file_error| {
        let named = fs::read_to_string(timezone).map_err(|error| {
            io::Error::other(format!(
                "the system's time zone is unknown: {}: {file_error}; {}: {error}",
                localtime.display(),
                timezone.display(),
            ))
        })?;
        TimeZone::get(named.trim()).map_err(|error| {
            io::Error::other(format!(
                "the system's time zone is unknown: {}: {file_error}; {}: {error}",
                localtime.display(),
                timezone.display(),
            ))
        })
    })
}

impl SystemZone {
    /// Reads the zone now; a zone that cannot be read is an error.
    pub fn open(localtime: impl Into<PathBuf>, timezone: impl Into<PathBuf>) -> io::Result<Self> {
        let (localtime, timezone) = (localtime.into(), timezone.into());
        let seen = look(&localtime, &timezone);
        let zone = read(&localtime, &timezone)?;
        Ok(Self {
            localtime,
            timezone,
            state: Box::new(Mutex::new(State { seen, zone })),
        })
    }

    /// The zone's IANA name as the files are now: where the link points, or,
    /// when the zone file is no link into a zone folder, the name file's
    /// text. Only a name the bundled zones know: a system's own zone folder
    /// has names such as `localtime` that are no zone (ARM64 on #1825).
    pub fn name(&self) -> Option<String> {
        let known = |name: &str| TimeZoneDatabase::bundled().get(name).is_ok();
        name_of_link(&self.localtime)
            .filter(|name| known(name))
            .or_else(|| {
                let named = fs::read_to_string(&self.timezone).ok()?;
                let named = named.trim();
                known(named).then(|| named.to_owned())
            })
    }

    /// The zone as the files are now. Read again only when they changed; a
    /// change that cannot be read keeps the last zone and is tried again at
    /// the next reading.
    pub fn current(&self) -> TimeZone {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let seen = look(&self.localtime, &self.timezone);
        if seen != state.seen
            && let Ok(zone) = read(&self.localtime, &self.timezone)
        {
            state.zone = zone;
            state.seen = seen;
        }
        state.zone.clone()
    }
}
