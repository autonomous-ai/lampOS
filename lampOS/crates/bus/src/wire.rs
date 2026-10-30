//! One message as it travels: a line of JSON saying what it is, a line end,
//! then the payload's bytes as they are.
//!
//! The header is text so that anything can read it: a shell, a script, a
//! person with `lamp-bus tap`. The payload is left alone so that sound costs
//! nothing to carry. The whole of it is one datagram, and the same bytes are
//! what a topic's `latest` file holds.
use crate::name;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    fs,
    io::{self, Read},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

/// The most one message may be, header and payload together. A datagram is
/// taken whole or not at all, and this is under what a Linux datagram
/// socket takes with the sizes it is given by default.
pub const MOST_BYTES: usize = 60_000;

/// The payload is one JSON value.
pub const JSON: &str = "json";
/// There is no payload: the header is the message.
pub const NONE: &str = "none";

/// How far past its time a message is still handed over: two processes read
/// the same clock a moment apart, and a value that lasts 20 ms is not late
/// because the reader was scheduled 21 ms after the writer.
pub const EXPIRY_GRACE_MS: u64 = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A value that replaces the one before it: where the servos are, who is
    /// in the room. The newest is also kept in the topic's `latest` file.
    Latest,
    /// Something that happened and is over: a piece of sound, a word heard.
    /// Nothing is kept: whoever was not listening does not get it.
    Event,
}

/// What every message says of itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub topic: String,
    pub kind: Kind,
    /// How to read the payload: [`JSON`], [`NONE`] or a media label.
    pub enc: String,
    /// What the payload is, as the publisher declared it for the topic
    /// (`lamp_msgs/JointState`). A publisher that declared nothing, and a
    /// recording made before there were declarations, say nothing here.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
    /// The publisher's name.
    pub source: String,
    /// Sixteen hex characters picked when the publisher was made. `seq`
    /// counts within one of these.
    pub boot: String,
    /// From 1, one more with every message this publisher sent on the topic.
    pub seq: u64,
    /// When, by the publisher's monotonic clock, in microseconds.
    pub at_us: u64,
    /// When, by the publisher's wall clock, in milliseconds since 1970.
    pub at_unix_ms: u64,
    /// How long after `at_unix_ms` the value still means something. A
    /// subscriber is not handed a message older than that.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_ms: Option<u64>,
    /// The kernel's boot id (`/proc/sys/kernel/random/boot_id`), said only
    /// by a publisher whose `at_us` is CLOCK_MONOTONIC
    /// ([`Publisher::stamp_monotonic_clock`](crate::Publisher::stamp_monotonic_clock)).
    /// A reader of the same boot then judges the message by `at_us`, which
    /// no time server moves, and not by `at_unix_ms`. A header without it
    /// is judged by the wall clock, as every header was before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clock: Option<String>,
}

/// What a reader's clocks say when it judges a header: its wall clock, its
/// CLOCK_MONOTONIC, and the id of its boot, if it knows one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Now<'a> {
    pub unix_ms: u64,
    pub monotonic_us: u64,
    pub boot_id: Option<&'a str>,
}

/// Which clock a header is judged by, for a reader at `now`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StampClock {
    /// `at_us` is this boot's CLOCK_MONOTONIC.
    Monotonic,
    /// `at_us` is the monotonic clock of another boot: nothing it says can
    /// be measured here, and the value has expired.
    OtherBoot,
    /// The header says no clock, or the reader knows no boot of its own:
    /// `at_unix_ms`, as before there was a `clock`.
    Wall,
}

const GRACE_US: u64 = EXPIRY_GRACE_MS * 1_000;

impl Header {
    /// Which clock this header is judged by at `now`.
    pub fn stamp_clock(&self, now: &Now<'_>) -> StampClock {
        match (self.clock.as_deref(), now.boot_id) {
            (Some(clock), Some(boot)) if clock == boot => StampClock::Monotonic,
            (Some(_), Some(_)) => StampClock::OtherBoot,
            _ => StampClock::Wall,
        }
    }

    /// Whether the value is no longer to be had at `now`, by the clock its
    /// header calls for. A this-boot stamp has expired once the monotonic
    /// clock is past `at_us` and its lifespan by more than
    /// [`EXPIRY_GRACE_MS`], and is refused outright when it is more than
    /// that far ahead of it. A stamp from another boot has always expired.
    /// One without a clock is judged as [`Self::expired_at`] judges it.
    pub fn expired_by(&self, now: &Now<'_>) -> bool {
        match self.stamp_clock(now) {
            StampClock::Monotonic => {
                self.ahead_of(now)
                    || self.expires_ms.is_some_and(|lasting| {
                        now.monotonic_us
                            > self
                                .at_us
                                .saturating_add(lasting.saturating_mul(1_000))
                                .saturating_add(GRACE_US)
                    })
            }
            StampClock::OtherBoot => true,
            StampClock::Wall => self.expired_at(now.unix_ms),
        }
    }

    /// Whether the stamp is more than [`EXPIRY_GRACE_MS`] ahead of `now`, by
    /// the clock its header calls for. Never so for a stamp of another boot,
    /// which [`Self::expired_by`] refuses already.
    pub fn ahead_of(&self, now: &Now<'_>) -> bool {
        match self.stamp_clock(now) {
            StampClock::Monotonic => self.at_us > now.monotonic_us.saturating_add(GRACE_US),
            StampClock::OtherBoot => false,
            StampClock::Wall => self.at_unix_ms > now.unix_ms.saturating_add(EXPIRY_GRACE_MS),
        }
    }

    /// Whether the value had outlived `expires_ms` at `now_unix_ms`, by more
    /// than [`EXPIRY_GRACE_MS`]. A message that names no limit never has.
    pub fn expired_at(&self, now_unix_ms: u64) -> bool {
        self.expires_ms.is_some_and(|lasting| {
            now_unix_ms
                > self
                    .at_unix_ms
                    .saturating_add(lasting)
                    .saturating_add(EXPIRY_GRACE_MS)
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub header: Header,
    pub payload: Vec<u8>,
    /// How many messages from this same publisher instance were sent
    /// between the last one this subscriber got and this one.
    pub gap: u64,
    /// The publisher of this name is not the instance heard before: it
    /// began again, and its count with it.
    pub publisher_restarted: bool,
    /// The value had outlived its `expires_ms` when it was read. Only a
    /// topic's kept value is ever returned so (`Bus::latest`,
    /// `Subscriber::latest`): it is the last thing known, and the caller is
    /// told that it is old. `Subscriber::recv` passes an expired message
    /// over and counts it.
    pub expired: bool,
}

impl Message {
    /// The payload as JSON.
    pub fn json(&self) -> io::Result<serde_json::Value> {
        self.parse()
    }

    /// The payload as a `T`, when it is JSON.
    pub fn parse<T: DeserializeOwned>(&self) -> io::Result<T> {
        if self.header.enc != JSON {
            return Err(invalid(format!(
                "the payload is {}, not json",
                self.header.enc
            )));
        }
        serde_json::from_slice(&self.payload).map_err(io::Error::from)
    }
}

fn invalid(why: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why)
}

/// Whether `id` can be a header's `clock`: what a kernel boot id is made
/// of, and short.
pub(crate) fn clock_id_is_valid(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

/// The wall clock, in milliseconds since 1970.
pub(crate) fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// The header and the payload of what arrived on `topic`, or why it is not
/// a message of that topic.
pub(crate) fn split<'a>(topic: &str, datagram: &'a [u8]) -> io::Result<(Header, &'a [u8])> {
    if datagram.len() > MOST_BYTES {
        return Err(invalid(format!("more than {MOST_BYTES} bytes")));
    }
    let Some(end) = datagram.iter().position(|&byte| byte == b'\n') else {
        return Err(invalid("no line end after the header".into()));
    };
    let header: Header = serde_json::from_slice(&datagram[..end])
        .map_err(|error| invalid(format!("header: {error}")))?;
    if header.topic != topic {
        return Err(invalid(format!("a message of topic {:?}", header.topic)));
    }
    let boot = header.boot.as_bytes();
    if boot.len() != 16
        || !boot
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return Err(invalid("boot is not 16 hex characters".into()));
    }
    if header.seq == 0 {
        return Err(invalid("seq is 0".into()));
    }
    if header
        .clock
        .as_deref()
        .is_some_and(|clock| !clock_id_is_valid(clock))
    {
        return Err(invalid("clock is not a boot id".into()));
    }
    name::source(&header.source)
        .and_then(|()| name::encoding(&header.enc))
        .and_then(|()| header.type_name.as_deref().map_or(Ok(()), name::type_name))
        .map_err(|error| invalid(error.to_string()))?;
    Ok((header, &datagram[end + 1..]))
}

/// What a topic's `latest` file holds, if there is one.
pub(crate) fn kept(path: &Path, topic: &str) -> io::Result<Option<(Header, Vec<u8>)>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    // A file that is not one of ours could be any size.
    let mut bytes = Vec::new();
    file.take(MOST_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    let (header, payload) =
        split(topic, &bytes).map_err(|error| invalid(format!("{}: {error}", path.display())))?;
    Ok(Some((header, payload.to_vec())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> Header {
        Header {
            topic: "servos".into(),
            kind: Kind::Latest,
            enc: JSON.into(),
            type_name: None,
            source: "body".into(),
            boot: "9f3a6c1e04b7d25a".into(),
            seq: 412,
            at_us: 98_123_456,
            at_unix_ms: 1_791_878_400_123,
            expires_ms: Some(500),
            clock: None,
        }
    }

    fn datagram(header: &Header, payload: &[u8]) -> Vec<u8> {
        let mut bytes = serde_json::to_vec(header).unwrap();
        bytes.push(b'\n');
        bytes.extend_from_slice(payload);
        bytes
    }

    #[test]
    fn the_header_is_one_line_of_json_with_its_fields_in_a_fixed_order() {
        let mut header = header();
        assert_eq!(
            serde_json::to_string(&header).unwrap(),
            "{\"topic\":\"servos\",\"kind\":\"latest\",\"enc\":\"json\",\"source\":\"body\",\
             \"boot\":\"9f3a6c1e04b7d25a\",\"seq\":412,\"at_us\":98123456,\
             \"at_unix_ms\":1791878400123,\"expires_ms\":500}"
        );
        // A value that does not expire says nothing about it.
        header.expires_ms = None;
        header.kind = Kind::Event;
        let text = serde_json::to_string(&header).unwrap();
        assert!(text.contains("\"kind\":\"event\"") && !text.contains("expires_ms"));
        assert_eq!(serde_json::from_str::<Header>(&text).unwrap(), header);
        // A publisher that declared the topic says what the payload is,
        // after how to read it. One that did not says nothing: a header
        // written before there were declarations reads as it always did.
        assert!(!text.contains("type"));
        header.type_name = Some("lamp_msgs/JointState".into());
        let text = serde_json::to_string(&header).unwrap();
        assert!(
            text.contains("\"enc\":\"json\",\"type\":\"lamp_msgs/JointState\",\"source\""),
            "{text}"
        );
        assert_eq!(serde_json::from_str::<Header>(&text).unwrap(), header);
    }

    #[test]
    fn a_message_has_expired_fifty_milliseconds_after_its_time_and_never_without_one() {
        let mut header = header();
        // Sent at ...123, lasting 500: good to ...623, handed over to ...673.
        let sent = header.at_unix_ms;
        assert!(!header.expired_at(0));
        assert!(!header.expired_at(sent));
        assert!(!header.expired_at(sent + 500));
        assert!(!header.expired_at(sent + 500 + EXPIRY_GRACE_MS));
        assert!(header.expired_at(sent + 500 + EXPIRY_GRACE_MS + 1));
        assert!(header.expired_at(u64::MAX));
        header.expires_ms = Some(0);
        assert!(!header.expired_at(sent + EXPIRY_GRACE_MS));
        assert!(header.expired_at(sent + EXPIRY_GRACE_MS + 1));
        // A limit too large to add is no limit.
        header.expires_ms = Some(u64::MAX);
        assert!(!header.expired_at(u64::MAX));
        header.expires_ms = None;
        assert!(!header.expired_at(u64::MAX));
    }

    /// A header as a build before `clock` reads and writes it.
    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct OldHeader {
        topic: String,
        kind: Kind,
        enc: String,
        #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
        type_name: Option<String>,
        source: String,
        boot: String,
        seq: u64,
        at_us: u64,
        at_unix_ms: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expires_ms: Option<u64>,
    }

    const BOOT: &str = "6a1c2b3d-5e6f-4a7b-8c9d-0e1f2a3b4c5d";

    #[test]
    fn a_clock_goes_last_and_old_and_new_headers_read_each_other() {
        // Without a clock the bytes are what every build before wrote.
        let plain = header();
        let old_text = serde_json::to_string(&plain).unwrap();
        assert!(!old_text.contains("clock"));
        let old: OldHeader = serde_json::from_str(&old_text).unwrap();
        assert_eq!(serde_json::to_string(&old).unwrap(), old_text);
        // An old header reads as one with no clock.
        assert_eq!(serde_json::from_str::<Header>(&old_text).unwrap(), plain);
        // A new header says its clock last, and an old reader passes it
        // over and reads the rest as it was.
        let mut stamped = header();
        stamped.clock = Some(BOOT.into());
        let new_text = serde_json::to_string(&stamped).unwrap();
        assert_eq!(
            new_text,
            format!("{},\"clock\":\"{BOOT}\"}}", &old_text[..old_text.len() - 1])
        );
        let read_old: OldHeader = serde_json::from_str(&new_text).unwrap();
        assert_eq!(read_old, old);
        assert_eq!(serde_json::from_str::<Header>(&new_text).unwrap(), stamped);
        // Through a datagram both ways.
        let (got, _) = split("servos", &datagram(&stamped, b"{}")).unwrap();
        assert_eq!(got, stamped);
        let (got, _) = split("servos", format!("{old_text}\n{{}}").as_bytes()).unwrap();
        assert_eq!(got.clock, None);
        // A clock that cannot be a boot id is no message.
        for bad in ["", "a b", "x/y", &"f".repeat(65)] {
            stamped.clock = Some(bad.into());
            let error = split("servos", &datagram(&stamped, b"{}")).unwrap_err();
            assert!(error.to_string().contains("clock"), "{bad:?}");
        }
    }

    #[test]
    fn a_this_boot_stamp_is_judged_by_the_monotonic_clock_another_boot_always_expired() {
        let mut header = header();
        header.clock = Some(BOOT.into());
        let sent = header.at_us;
        let at = |monotonic_us: u64, unix_ms: u64, boot: Option<&'static str>| Now {
            unix_ms,
            monotonic_us,
            boot_id: boot,
        };
        // Any wall clock: a step of 20 s either way changes nothing.
        for wall in [
            0,
            header.at_unix_ms - 20_000,
            header.at_unix_ms + 20_000,
            u64::MAX,
        ] {
            let now = |monotonic| at(monotonic, wall, Some(BOOT));
            assert_eq!(header.stamp_clock(&now(sent)), StampClock::Monotonic);
            assert!(!header.expired_by(&now(sent)));
            assert!(!header.expired_by(&now(sent + 550_000)));
            assert!(header.expired_by(&now(sent + 550_001)));
            // Ahead by the grace is in time; by more is refused.
            assert!(!header.ahead_of(&now(sent - 50_000)));
            assert!(!header.expired_by(&now(sent - 50_000)));
            assert!(header.ahead_of(&now(sent - 50_001)));
            assert!(header.expired_by(&now(sent - 50_001)));
        }
        // Without a lifespan it never expires, but is still refused ahead.
        header.expires_ms = None;
        assert!(!header.expired_by(&at(u64::MAX, 0, Some(BOOT))));
        assert!(header.expired_by(&at(0, 0, Some(BOOT))));
        header.expires_ms = Some(500);
        // Another boot's: expired whatever the numbers.
        let other = at(sent, header.at_unix_ms, Some("0f1e2d3c-4b5a"));
        assert_eq!(header.stamp_clock(&other), StampClock::OtherBoot);
        assert!(header.expired_by(&other) && !header.ahead_of(&other));
        // A reader that knows no boot, and a header with no clock: the wall.
        let unknown = at(u64::MAX, header.at_unix_ms + 550, None);
        assert_eq!(header.stamp_clock(&unknown), StampClock::Wall);
        assert!(!header.expired_by(&unknown));
        header.clock = None;
        let wall = |unix_ms| at(sent, unix_ms, Some(BOOT));
        assert_eq!(header.stamp_clock(&wall(0)), StampClock::Wall);
        assert!(!header.expired_by(&wall(header.at_unix_ms + 550)));
        assert!(header.expired_by(&wall(header.at_unix_ms + 551)));
        assert!(!header.ahead_of(&wall(header.at_unix_ms - 50)));
        assert!(header.ahead_of(&wall(header.at_unix_ms - 51)));
    }

    #[test]
    fn a_datagram_splits_at_the_first_line_end_and_the_payload_is_untouched() {
        let header = header();
        // The payload has line ends, a NUL and bytes that are not text.
        let payload = [b'\n', 0, 0xff, b'\n', 7];
        let bytes = datagram(&header, &payload);
        let (got, rest) = split("servos", &bytes).unwrap();
        assert_eq!(got, header);
        assert_eq!(rest, payload);
        // No payload at all is a message too.
        let bare = datagram(&header, b"");
        assert!(split("servos", &bare).unwrap().1.is_empty());
        // A field this version does not know is passed over.
        let extended =
            b"{\"topic\":\"servos\",\"kind\":\"event\",\"enc\":\"none\",\"source\":\"x\",\
            \"boot\":\"0123456789abcdef\",\"seq\":1,\"at_us\":0,\"at_unix_ms\":0,\"later\":[1]}\n";
        assert_eq!(split("servos", extended).unwrap().0.seq, 1);
    }

    #[test]
    fn what_is_not_a_message_of_the_topic_is_refused_with_the_reason() {
        let good = header();
        let reason = |bytes: &[u8]| split("servos", bytes).unwrap_err().to_string();
        assert!(reason(b"").contains("no line end"));
        assert!(reason(b"hello").contains("no line end"));
        assert!(reason(&serde_json::to_vec(&good).unwrap()).contains("no line end"));
        assert!(reason(b"hello\n").contains("header"));
        assert!(reason(b"{}\n").contains("header"));
        assert!(reason(b"\xff\xfe\n").contains("header"));
        let changed = |change: fn(&mut Header)| {
            let mut header = header();
            change(&mut header);
            reason(&datagram(&header, b"{}"))
        };
        assert!(changed(|header| header.topic = "mic2".into()).contains("topic \"mic2\""));
        assert!(changed(|header| header.seq = 0).contains("seq is 0"));
        assert!(changed(|header| header.boot = "9F3A6C1E04B7D25A".into()).contains("boot"));
        assert!(changed(|header| header.boot = "9f3a".into()).contains("boot"));
        assert!(changed(|header| header.source = "Lamp Body".into()).contains("publisher name"));
        assert!(changed(|header| header.enc = String::new()).contains("encoding"));
        assert!(changed(|header| header.type_name = Some("joint state".into())).contains("type "));
        let mut large = datagram(&good, b"");
        large.resize(MOST_BYTES + 1, b' ');
        assert!(reason(&large).contains("more than 60000 bytes"));
        large.truncate(MOST_BYTES);
        assert!(split("servos", &large).is_ok());
    }

    #[test]
    fn a_json_payload_reads_as_a_value_or_as_a_type_and_other_payloads_do_not() {
        #[derive(Deserialize, Debug, PartialEq)]
        struct Pose {
            pitch: u16,
            yaw: u16,
        }
        let mut message = Message {
            header: header(),
            payload: b"{\"pitch\":2048,\"yaw\":1990}".to_vec(),
            gap: 0,
            publisher_restarted: false,
            expired: false,
        };
        assert_eq!(message.json().unwrap()["yaw"], 1990);
        assert_eq!(
            message.parse::<Pose>().unwrap(),
            Pose {
                pitch: 2048,
                yaw: 1990
            }
        );
        message.payload = b"{\"pitch\":".to_vec();
        assert!(message.json().is_err());
        message.header.enc = "pcm_s16le_16000_mono".into();
        message.payload = b"{}".to_vec();
        let error = message.json().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("pcm_s16le_16000_mono, not json"));
    }
}
