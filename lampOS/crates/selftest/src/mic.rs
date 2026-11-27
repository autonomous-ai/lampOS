//! The voice microphone (Jieli): a second of its blocks on `mic2/audio`,
//! as `lamp-voice` gives them after its echo canceller, and their level;
//! whether it is on USB now (its sound card, `device_micro2`), and how many
//! times it fell off USB since boot, from the kernel's log. Only listened to
//! and read: the self-test never opens a microphone, which is the voice's
//! and behind its privacy switch.
use crate::{Check, kernel};
use lamp_bus::Bus;
use lamp_msgs::{Audio, PartResult, Verdict};
use std::time::{Duration, Instant};

/// The topic the voice microphone is heard on.
pub const TOPIC: &str = Audio::TOPICS[1];
/// How much of it is listened to.
pub const LISTEN: Duration = Duration::from_secs(1);
/// With nothing in this long, nothing is coming.
pub const FIRST_WITHIN: Duration = Duration::from_secs(2);
/// A microphone at or under this level gives nothing.
pub const SILENT_DB: f32 = crate::listen::SILENT_DB;

/// The voice microphone's sound card, by the name the lamp's udev rule
/// gives it.
pub const CARD: &str = "device_micro2";
/// Where the kernel lists its sound cards.
pub const SOUND: &str = "/sys/class/sound";

/// The voice microphone on USB: whether its card is there now, whether
/// this lamp has a Jieli at all, its port, and how often the kernel saw it
/// go.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Usb {
    /// Its sound card is there now.
    pub present: bool,
    /// A Jieli is this lamp's voice microphone: its card is there, or the
    /// kernel found one this boot. On an image whose mic is another
    /// (ReSpeaker, XVF3800), `device_micro2` is only an ALSA alias and no
    /// card has that name (the reviewer on #1304), so its absence says
    /// nothing there.
    pub jieli: bool,
    /// Its USB port (`3-1.1.1`), from its card now or from the kernel's
    /// record of a Jieli device being found there.
    pub port: Option<String>,
    /// `USB disconnect` records for that port in the kernel's log; `None`
    /// if the log could not be read or no port is known.
    pub drops: Option<usize>,
    /// The log reaches back to boot: its first record is the boot's first,
    /// and the scan reached its end without any overwrite. Otherwise the
    /// observed drops are "at least".
    pub since_boot: bool,
    /// The kernel's log was drained, or at least one record was observed.
    pub log_read: bool,
    /// Why the scan ended, absent when the log could not be opened. A partial
    /// scan never makes an otherwise passing microphone check a clean pass.
    pub scan_stop: Option<kernel::ScanStop>,
    /// The USB hub the mic hangs from, when the kernel saw it go: on
    /// lamp-4ace at 17:12 on 17 Oct the inner hub `3-1.1` dropped twice and
    /// took the mic, the speaker, the body's servo port and the camera with
    /// it (the first builder, #705 17:31).
    pub hub: Option<Hub>,
}

/// A hub's drops, and what went with them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hub {
    pub port: String,
    pub drops: usize,
    /// What dropped within [`WITH_HUB_US`] of one of its drops, by the name
    /// the kernel's records give each port, in port order.
    pub took: Vec<String>,
    /// How many of the mic's own drops came with the hub's.
    pub mic_with_it: usize,
}

/// A port dropped this close to its hub's drop went with it.
pub const WITH_HUB_US: u64 = 1_000_000;

/// A port's name, short: "the body's servo port" is "servo port".
fn short(name: &str) -> String {
    match name {
        "the body's servo port" => "servo port".to_owned(),
        // The C-Media speakerphone is the room mic too: both, short (the
        // reviewer on #1356).
        "the speaker with the room mic" => "speaker/room mic".to_owned(),
        name => name.strip_prefix("the ").unwrap_or(name).to_owned(),
    }
}

/// "a", "a and b", "a, b and c".
fn listed(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

impl Usb {
    /// Drops, as said: "2 times since boot", or "at least 2 times" when
    /// the kernel's log no longer reaches back to boot.
    fn times(&self, drops: usize) -> String {
        let times = if drops == 1 { "time" } else { "times" };
        if self.since_boot {
            format!("{drops} {times} since boot")
        } else if self
            .scan_stop
            .is_some_and(|stop| stop != kernel::ScanStop::Complete)
        {
            format!("at least {drops} {times} since boot (the kernel log scan was incomplete)")
        } else {
            format!(
                "at least {drops} {times} since boot (the kernel's log no longer reaches back to boot)"
            )
        }
    }

    /// The hub's drops in one line, for a person: "the lamp's USB hub
    /// dropped 2 times since boot; the voice mic, the body's servo port,
    /// the speaker with the room mic and the camera went with it". `None`
    /// when it never did. Short enough for `lamp/health`'s line: "at
    /// least" alone says the log no longer reaches back to boot.
    pub fn hub_said(&self) -> Option<String> {
        let (mut said, hub) = self.hub_dropped()?;
        if !hub.took.is_empty() {
            said.push_str(&format!("; {} went with it", listed(&hub.took)));
        }
        Some(said)
    }

    /// The same, short enough to stand before a failed part in
    /// `lamp/health`'s line: "the lamp's USB hub dropped 2 times since
    /// boot, with the voice mic, servo port, speaker/room mic and camera".
    pub fn hub_said_short(&self) -> Option<String> {
        let (mut said, hub) = self.hub_dropped()?;
        if !hub.took.is_empty() {
            let took: Vec<String> = hub.took.iter().map(|name| short(name)).collect();
            said.push_str(&format!(", with the {}", listed(&took)));
        }
        Some(said)
    }

    fn hub_dropped(&self) -> Option<(String, &Hub)> {
        let hub = self.hub.as_ref().filter(|hub| hub.drops > 0)?;
        let times = if hub.drops == 1 { "time" } else { "times" };
        let at_least = if self.since_boot { "" } else { "at least " };
        let said = format!(
            "the lamp's USB hub dropped {at_least}{} {times} since boot",
            hub.drops
        );
        Some((said, hub))
    }

    /// The mic's own drops, apart from those that came with its hub's. An
    /// incomplete suffix may contain a matching hub event, so subtracting two
    /// partial counts cannot establish an "alone" lower bound.
    fn alone_said(&self) -> Option<String> {
        if self
            .scan_stop
            .is_some_and(|stop| stop != kernel::ScanStop::Complete)
        {
            return None;
        }
        let with_hub = self.hub.as_ref().map_or(0, |hub| hub.mic_with_it);
        let alone = self.drops?.saturating_sub(with_hub);
        (alone > 0).then(|| {
            if with_hub > 0 {
                format!("the voice mic alone dropped {}", self.times(alone))
            } else {
                format!("off USB {}", self.times(alone))
            }
        })
    }
}

/// The USB port a sound card's device path sits on: the last path
/// component before its interface (`…/3-1.1.1/3-1.1.1:1.1` gives `3-1.1.1`).
pub fn port_of(device: &str) -> Option<String> {
    let interface = device.rsplit('/').next()?;
    let (port, _) = interface.split_once(':')?;
    (!port.is_empty() && port.contains('-')).then(|| port.to_owned())
}

/// The kernel's USB records, counted: the ports a Jieli device was found
/// on, and each port's disconnects. A record is `pri,seq,usec,flags;text`,
/// and the text `usb PORT: Manufacturer: …` or `usb PORT: USB disconnect, …`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsbRecords {
    pub jieli_ports: Vec<String>,
    pub disconnects: Vec<String>,
    /// Each disconnect with when it came, in microseconds since boot.
    pub disconnected_at: Vec<(String, u64)>,
    /// What each port is, by the kernel's own records of it.
    pub names: Vec<(String, String)>,
    /// Records read, and the first one's sequence (the boot's first is 0).
    pub records: usize,
    pub first_sequence: Option<u64>,
}

impl UsbRecords {
    pub fn count(&mut self, record: &str) {
        if record.starts_with(' ') {
            return;
        }
        let (prefix, text) = record.split_once(';').unwrap_or(("", record));
        if self.records == 0 {
            self.first_sequence = prefix.split(',').nth(1).and_then(|s| s.parse().ok());
        }
        self.records += 1;
        let at_us = prefix
            .split(',')
            .nth(2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        // `cdc_acm 3-1.1.2:1.0: ttyACM0: USB ACM device`: the body's servo
        // port, the only serial device on the lamp's hub.
        if let Some(rest) = text.strip_prefix("cdc_acm ")
            && let Some((interface, said)) = rest.split_once(": ")
            && said.starts_with("ttyACM")
            && let Some((port, _)) = interface.split_once(':')
        {
            self.name(port, "the body's servo port");
            return;
        }
        let Some(rest) = text.strip_prefix("usb ") else {
            return;
        };
        let Some((port, said)) = rest.split_once(": ") else {
            return;
        };
        if said.starts_with("USB disconnect") {
            self.disconnects.push(port.to_owned());
            self.disconnected_at.push((port.to_owned(), at_us));
        } else if said.starts_with("Manufacturer: Jieli") {
            if !self.jieli_ports.iter().any(|p| p == port) {
                self.jieli_ports.push(port.to_owned());
            }
            self.name(port, "the voice mic");
        } else if said.starts_with("Manufacturer: C-Media") {
            // The C-Media speakerphone is the room mic too (lamp-ears' mic1).
            self.name(port, "the speaker with the room mic");
        } else if said.starts_with("Product: ") && said.to_lowercase().contains("camera") {
            self.name(port, "the camera");
        }
    }

    fn name(&mut self, port: &str, what: &str) {
        self.names.retain(|(named, _)| named != port);
        self.names.push((port.to_owned(), what.to_owned()));
    }

    /// The hub `hub`: its drops, what went with them, and how many of
    /// `mic`'s drops were among them.
    pub fn hub(&self, hub: &str, mic: &str) -> Hub {
        let drops: Vec<u64> = self
            .disconnected_at
            .iter()
            .filter(|(port, _)| port == hub)
            .map(|(_, at)| *at)
            .collect();
        let near = |at: u64| drops.iter().any(|drop| drop.abs_diff(at) <= WITH_HUB_US);
        let child = format!("{hub}.");
        let mut ports: Vec<&str> = self
            .disconnected_at
            .iter()
            .filter(|(port, at)| port.starts_with(&child) && near(*at))
            .map(|(port, _)| port.as_str())
            .collect();
        ports.sort_unstable();
        ports.dedup();
        let took = ports
            .iter()
            .map(|port| {
                self.names
                    .iter()
                    .find(|(named, _)| named == port)
                    .map_or_else(|| (*port).to_owned(), |(_, what)| what.clone())
            })
            .collect();
        let mic_with_it = self
            .disconnected_at
            .iter()
            .filter(|(port, at)| port == mic && near(*at))
            .count();
        Hub {
            port: hub.to_owned(),
            drops: drops.len(),
            took,
            mic_with_it,
        }
    }

    /// Disconnects on `port`.
    pub fn drops(&self, port: &str) -> usize {
        self.disconnects.iter().filter(|p| *p == port).count()
    }
}

/// Reads whether the card is there and its port (from `sound`), and the
/// drops since boot (from the kernel's log at `kmsg`).
pub fn usb(sound: &std::path::Path, kmsg: &str, deadline: Instant) -> Usb {
    let card_port = std::fs::read_dir(sound).ok().and_then(|cards| {
        cards.flatten().find_map(|card| {
            let id = std::fs::read_to_string(card.path().join("id")).ok()?;
            (id.trim() == CARD).then(|| {
                std::fs::canonicalize(card.path().join("device"))
                    .ok()
                    .and_then(|device| port_of(&device.to_string_lossy()))
            })
        })
    });
    let mut records = UsbRecords::default();
    let scan = kernel::each_record(kmsg, deadline, |line| records.count(line)).ok();
    usb_from_records(card_port, &records, scan)
}

/// Fold a complete or partial scan without throwing away observed drops or
/// device evidence when a later read fails or reaches a bound.
fn usb_from_records(
    card_port: Option<Option<String>>,
    records: &UsbRecords,
    scan: Option<kernel::Scan>,
) -> Usb {
    let present = card_port.is_some();
    let read = scan.is_some_and(|scan| scan.complete() || scan.records > 0);
    let port = card_port
        .flatten()
        .or_else(|| records.jieli_ports.last().cloned());
    let drops = match (&port, read) {
        (Some(port), true) => Some(records.drops(port)),
        _ => None,
    };
    // The mic's parent hub: `3-1.1.1` hangs from `3-1.1`.
    let hub = match (&port, read) {
        (Some(port), true) => port.rsplit_once('.').map(|(hub, _)| records.hub(hub, port)),
        _ => None,
    };
    Usb {
        present,
        jieli: present || !records.jieli_ports.is_empty(),
        port,
        drops,
        since_boot: scan.is_some_and(|scan| scan.complete() && !scan.overwritten)
            && records.first_sequence == Some(0),
        log_read: read,
        scan_stop: scan.map(|scan| scan.stop),
        hub,
    }
}

/// What a second of listening came to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Heard {
    pub blocks: usize,
    /// Blocks missing between those heard, by their sequence.
    pub lost: u64,
    pub sum_squares: f64,
    pub samples: usize,
}

impl Heard {
    /// Adds one message's payload: the header line, then S16LE samples.
    pub fn add(&mut self, payload: &[u8]) -> Result<(), String> {
        let (header, samples) = Audio::split(payload).map_err(|e| e.to_string())?;
        if header.sample_format != Audio::S16LE {
            return Err(format!("samples are {}, not S16LE", header.sample_format));
        }
        self.blocks += 1;
        for pair in samples.chunks_exact(2) {
            let sample = f64::from(i16::from_le_bytes([pair[0], pair[1]]));
            self.sum_squares += sample * sample;
            self.samples += 1;
        }
        Ok(())
    }

    /// The level of everything heard, in dBFS.
    pub fn level_db(&self) -> f32 {
        if self.samples == 0 || self.sum_squares == 0.0 {
            return -120.0;
        }
        let rms = (self.sum_squares / self.samples as f64).sqrt() / 32_768.0;
        (20.0 * rms.log10()) as f32
    }
}

/// Judges what was heard, and says the microphone's USB. `declared`:
/// whether the voice ever said it would publish this microphone. Its card
/// gone with nothing heard fails it: the voice cannot open it (#1178, on
/// lamp-4ace twice on 17 Oct). Drops since boot are only said, never a
/// fail by themselves (the first builder, 17 Oct 13:18).
pub fn voice_mic(declared: bool, heard: &Result<Heard, String>, usb: Option<&Usb>) -> PartResult {
    let mut result = judged(declared, heard, usb);
    if let Some(usb) = usb {
        if let Some(stop) = usb.scan_stop {
            result = result
                .with("usb_log_scan", stop.code())
                .with("usb_log_order", "oldest_first");
            if stop != kernel::ScanStop::Complete {
                if result.verdict == Verdict::Pass {
                    result.verdict = Verdict::NotChecked;
                }
                let note = format!("the kernel log scan was incomplete ({})", stop.code());
                result.reason = if result.reason.is_empty() {
                    note
                } else {
                    format!("{}; {note}", result.reason)
                };
            }
        }
        if let Some(port) = &usb.port {
            result = result.with("usb_port", port);
        }
        if !usb.jieli {
            // No Jieli on this lamp, or none the log still holds: nothing
            // to say of its USB.
            let why = if usb
                .scan_stop
                .is_some_and(|stop| stop != kernel::ScanStop::Complete)
            {
                "unknown: no Jieli card here, and the kernel log scan was incomplete"
            } else if !usb.log_read {
                "unknown: no Jieli card here, and the kernel's log could not be read (it needs CAP_SYSLOG)"
            } else if !usb.since_boot {
                "unknown: no Jieli card here, and the kernel's log no longer reaches back to boot"
            } else {
                "unknown: no Jieli card here, and none in the kernel's log"
            };
            return result.with("on_usb", why);
        }
        result = result.with("on_usb", usb.present);
        result = match usb.drops {
            Some(drops) if usb.since_boot => result.with("usb_drops", drops),
            Some(drops) => result.with("usb_drops", format!("at least {drops}")),
            None => result.with(
                "usb_drops",
                if usb
                    .scan_stop
                    .is_some_and(|stop| stop != kernel::ScanStop::Complete)
                {
                    "unknown: the kernel log scan was incomplete, or the port is not known"
                } else {
                    "unknown: the kernel's log could not be read (it needs CAP_SYSLOG), or the port is not known"
                },
            ),
        };
        if let Some(hub) = &usb.hub {
            result = result.with("usb_hub", &hub.port).with(
                "usb_hub_drops",
                if usb.since_boot {
                    hub.drops.to_string()
                } else {
                    format!("at least {}", hub.drops)
                },
            );
        }
        // The hub first: when it drops, the mic goes with everything else
        // on it, and that is one thing, not a mic problem (the first
        // builder, #705 17:31). Then the mic's own drops, apart from those.
        // A fail says its own drops already, and `lamp-health` adds the
        // hub's line from `usb_hub_said`, so a fail's reason leaves it out.
        let mut notes = Vec::new();
        if let Some(said) = usb.hub_said() {
            result = result.with("usb_hub_said", &said);
            if let Some(short) = usb.hub_said_short() {
                result = result.with("usb_hub_said_short", short);
            }
            if result.verdict != Verdict::Fail {
                notes.push(said);
            }
        }
        if let Some(alone) = usb.alone_said()
            && result.verdict != Verdict::Fail
        {
            notes.push(alone);
        }
        for note in notes {
            result.reason = if result.reason.is_empty() {
                note
            } else {
                format!("{}; {note}", result.reason)
            };
        }
    }
    result
}

fn judged(declared: bool, heard: &Result<Heard, String>, usb: Option<&Usb>) -> PartResult {
    let part = "voice_mic";
    let heard = match heard {
        Err(why) => {
            return PartResult::new(
                part,
                Verdict::Fail,
                &format!("{TOPIC} cannot be read: {why}"),
            );
        }
        Ok(heard) => heard,
    };
    if heard.blocks == 0 {
        // Gone only where a Jieli is the voice mic: on another image
        // `device_micro2` is no card at all.
        if let Some(usb) = usb.filter(|usb| usb.jieli && !usb.present) {
            let drops = usb
                .alone_said()
                .map_or(String::new(), |said| format!(", {said}"));
            return PartResult::new(
                part,
                Verdict::Fail,
                &format!("not on USB now: no sound card {CARD}{drops}"),
            );
        }
        let why = if declared {
            "no sound for 2 s: the microphone is closed (privacy switch) or lamp-voice stopped giving it"
        } else {
            "nothing was ever said on mic2/audio: lamp-voice gives it only with LAMP_BUS=on and LAMP_BUS_AUDIO=on"
        };
        return PartResult::new(part, Verdict::NotChecked, why);
    }
    let level = heard.level_db();
    let result = if level <= SILENT_DB {
        PartResult::new(
            part,
            Verdict::Fail,
            &format!("silent at {level:.0} dBFS: the microphone gives nothing"),
        )
    } else {
        PartResult::new(part, Verdict::Pass, "")
    };
    result
        .with("level_db", format!("{level:.1}"))
        .with("blocks", heard.blocks)
        .with("lost", heard.lost)
}

/// Listens to the voice microphone for a second.
pub struct VoiceMic<'a> {
    pub bus: &'a Bus,
}

impl VoiceMic<'_> {
    fn listen(&self, deadline: Instant) -> Result<Heard, String> {
        let mut hears = self
            .bus
            .subscribe_typed(TOPIC, "selftest", Audio::TYPE)
            .map_err(|e| e.to_string())?;
        let begun = Instant::now();
        let mut heard = Heard::default();
        let mut last: Option<u64> = None;
        let mut first_at: Option<Instant> = None;
        loop {
            let now = Instant::now();
            let until = match first_at {
                Some(at) => at + LISTEN,
                None => begun + FIRST_WITHIN,
            }
            .min(deadline);
            if now >= until {
                break;
            }
            let Some(message) = hears.recv(until - now).map_err(|e| e.to_string())? else {
                continue;
            };
            if message.expired {
                continue;
            }
            first_at.get_or_insert_with(Instant::now);
            if let Ok((header, _)) = Audio::split(&message.payload) {
                if let Some(before) = last {
                    heard.lost += header.sequence.saturating_sub(before + 1);
                }
                last = Some(header.sequence);
            }
            heard.add(&message.payload)?;
        }
        Ok(heard)
    }
}

impl Check for VoiceMic<'_> {
    fn part(&self) -> &str {
        "voice_mic"
    }

    fn run(&mut self, deadline: Instant) -> PartResult {
        let declared = self.bus.declaration(TOPIC).ok().flatten().is_some();
        let heard = self.listen(deadline);
        let usb = cfg!(target_os = "linux")
            .then(|| usb(std::path::Path::new(SOUND), kernel::KMSG, deadline));
        voice_mic(declared, &heard, usb.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_incomplete_kernel_scan_keeps_usb_drops_and_real_microphone_faults() {
        let start = Instant::now();
        let mut records = UsbRecords::default();
        let mut first = true;
        let scan = kernel::scan_records(
            |buffer| {
                if std::mem::take(&mut first) {
                    let bytes = b"6,0,0,-;Booting\n6,1,1,-;usb 3-1.1.1: Manufacturer: Jieli\n6,2,2,-;usb 3-1.1: USB disconnect\n6,3,3,-;usb 3-1.1.1: USB disconnect\n";
                    buffer[..bytes.len()].copy_from_slice(bytes);
                    Ok(bytes.len())
                } else {
                    Err(std::io::Error::other("PRIVATE_CANARY"))
                }
            },
            start + kernel::SCAN_BUDGET,
            || start,
            |line| records.count(line),
        );
        assert_eq!(scan.stop, kernel::ScanStop::ReadError);
        let gone = usb_from_records(None, &records, Some(scan));
        assert!(gone.jieli && gone.log_read && !gone.since_boot);
        assert_eq!(gone.drops, Some(1));
        assert_eq!(gone.hub.as_ref().map(|hub| hub.drops), Some(1));
        let failed = voice_mic(true, &Ok(Heard::default()), Some(&gone));
        assert_eq!(failed.verdict, Verdict::Fail);
        assert!(failed.reason.contains("not on USB now"));
        assert!(
            failed
                .values
                .iter()
                .any(|value| value.key == "usb_drops" && value.value == "at least 1")
        );
        assert!(
            !serde_json::to_string(&failed)
                .unwrap()
                .contains("PRIVATE_CANARY")
        );

        let present = usb_from_records(Some(Some("3-1.1.1".into())), &records, Some(scan));
        let mut heard = Heard::default();
        heard.add(&block(1, 300)).unwrap();
        let partial = voice_mic(true, &Ok(heard), Some(&present));
        assert_eq!(partial.verdict, Verdict::NotChecked);
        assert!(partial.reason.contains("incomplete (read_error)"));
        assert!(
            partial
                .values
                .iter()
                .any(|value| value.key == "blocks" && value.value == "1")
        );
        assert!(
            partial
                .values
                .iter()
                .any(|value| value.key == "usb_hub_drops" && value.value == "at least 1")
        );

        let mut silent = Heard::default();
        silent.add(&block(1, 0)).unwrap();
        assert_eq!(
            voice_mic(true, &Ok(silent), Some(&present)).verdict,
            Verdict::Fail
        );
    }

    #[test]
    fn a_partial_suffix_cannot_establish_that_a_microphone_dropped_alone() {
        // The second hub drop arrives after its microphone drop. Cutting
        // before it makes the observed total minus observed associations one,
        // although the complete input proves both mic drops accompanied a hub.
        let lines: [&[u8]; 6] = [
            b"6,0,0,-;Booting\n",
            b"6,1,1,-;usb 3-1.1.1: Manufacturer: Jieli\n",
            b"6,2,2000000,-;usb 3-1.1: USB disconnect\n",
            b"6,3,2010000,-;usb 3-1.1.1: USB disconnect\n",
            b"6,4,5000000,-;usb 3-1.1.1: USB disconnect\n",
            b"6,5,5010000,-;usb 3-1.1: USB disconnect\n",
        ];
        for complete in [false, true] {
            let start = Instant::now();
            let now = std::cell::Cell::new(start);
            let mut next = 0;
            let mut records = UsbRecords::default();
            let scan = kernel::scan_records(
                |buffer| {
                    let Some(line) = lines.get(next) else {
                        return Err(std::io::ErrorKind::WouldBlock.into());
                    };
                    next += 1;
                    buffer[..line.len()].copy_from_slice(line);
                    Ok(line.len())
                },
                start + kernel::SCAN_BUDGET,
                || now.get(),
                |line| {
                    records.count(line);
                    if !complete && records.records == 5 {
                        now.set(start + kernel::SCAN_BUDGET);
                    }
                },
            );
            assert_eq!(
                scan.stop,
                if complete {
                    kernel::ScanStop::Complete
                } else {
                    kernel::ScanStop::Deadline
                }
            );
            let present = usb_from_records(Some(Some("3-1.1.1".into())), &records, Some(scan));
            assert_eq!(present.drops, Some(2));
            let hub = present.hub.as_ref().unwrap();
            assert_eq!(hub.drops, if complete { 2 } else { 1 });
            assert_eq!(hub.mic_with_it, if complete { 2 } else { 1 });
            assert!(present.alone_said().is_none());

            let mut heard = Heard::default();
            heard.add(&block(1, 300)).unwrap();
            let running = voice_mic(true, &Ok(heard), Some(&present));
            assert_eq!(
                running.verdict,
                if complete {
                    Verdict::Pass
                } else {
                    Verdict::NotChecked
                }
            );
            let gone = Usb {
                present: false,
                ..present
            };
            let missing = voice_mic(true, &Ok(Heard::default()), Some(&gone));
            assert_eq!(missing.verdict, Verdict::Fail);
            for result in [running, missing] {
                assert!(!result.reason.contains("alone"));
                let value = |key: &str| {
                    result
                        .values
                        .iter()
                        .find(|value| value.key == key)
                        .map(|value| value.value.as_str())
                };
                assert_eq!(
                    value("usb_drops"),
                    Some(if complete { "2" } else { "at least 2" })
                );
                assert_eq!(
                    value("usb_hub_drops"),
                    Some(if complete { "2" } else { "at least 1" })
                );
            }
        }
    }

    #[test]
    fn an_expired_kernel_scan_has_no_drop_count_or_claim_of_a_full_boot() {
        let start = Instant::now();
        let scan = kernel::scan_records(
            |_| panic!("no read after deadline"),
            start,
            || start,
            |_| panic!("no record after deadline"),
        );
        let usb = usb_from_records(
            Some(Some("3-1.1.1".into())),
            &UsbRecords::default(),
            Some(scan),
        );
        assert!(usb.present && usb.jieli);
        assert!(!usb.log_read && !usb.since_boot);
        assert_eq!(usb.drops, None);
        assert_eq!(usb.hub, None);
        let mut heard = Heard::default();
        heard.add(&block(1, 300)).unwrap();
        let result = voice_mic(true, &Ok(heard), Some(&usb));
        assert_eq!(result.verdict, Verdict::NotChecked);
        assert!(result.reason.contains("incomplete (deadline)"));
        assert!(
            result
                .values
                .iter()
                .any(|value| value.key == "usb_drops" && value.value.starts_with("unknown"))
        );
    }

    fn block(sequence: u64, amplitude: i16) -> Vec<u8> {
        let samples: Vec<u8> = (0..320)
            .flat_map(|n| (if n % 2 == 0 { amplitude } else { -amplitude }).to_le_bytes())
            .collect();
        Audio {
            sample_rate: 16_000,
            channels: 1,
            sample_format: Audio::S16LE.into(),
            sequence,
        }
        .block(&samples)
        .unwrap()
    }

    #[test]
    fn a_quiet_room_passes_with_its_level_and_digital_silence_fails() {
        let mut room = Heard::default();
        for n in 0..50 {
            room.add(&block(n, 30)).unwrap();
        }
        let result = voice_mic(true, &Ok(room), None);
        assert_eq!(result.verdict, Verdict::Pass, "{result:?}");
        assert!(
            result
                .values
                .iter()
                .any(|v| v.key == "level_db" && v.value == "-60.8"),
            "{result:?}"
        );
        let mut dead = Heard::default();
        dead.add(&block(0, 0)).unwrap();
        let silent = voice_mic(true, &Ok(dead), None);
        assert_eq!(silent.verdict, Verdict::Fail);
        assert!(silent.reason.contains("silent"));
    }

    #[test]
    fn no_sound_is_not_checked_and_says_whether_it_was_ever_given() {
        let closed = voice_mic(true, &Ok(Heard::default()), None);
        assert_eq!(closed.verdict, Verdict::NotChecked);
        assert!(closed.reason.contains("privacy"));
        let never = voice_mic(false, &Ok(Heard::default()), None);
        assert!(never.reason.contains("LAMP_BUS_AUDIO"), "{}", never.reason);
        assert_eq!(
            voice_mic(true, &Err("gone".into()), None).verdict,
            Verdict::Fail
        );
    }

    #[test]
    fn a_second_of_blocks_is_heard_from_the_bus_with_what_was_lost() {
        let root = std::path::PathBuf::from(format!("/tmp/ls-mic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut voice = bus
            .publisher_declared(TOPIC, &Audio::declaration(), "voice")
            .unwrap();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopping = stop.clone();
        let speaker = std::thread::spawn(move || {
            let mut sequence = 0;
            while !stopping.load(std::sync::atomic::Ordering::Relaxed) {
                // Every tenth block is lost on the way.
                if sequence % 10 != 9 {
                    voice
                        .publish_bytes(sequence, Audio::ENCODING, &block(sequence, 300))
                        .unwrap();
                }
                sequence += 1;
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        let result = VoiceMic { bus: &bus }.run(Instant::now() + Duration::from_secs(10));
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        speaker.join().unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(result.verdict, Verdict::Pass, "{result:?}");
        let value = |key: &str| {
            result
                .values
                .iter()
                .find(|v| v.key == key)
                .map(|v| v.value.parse::<u64>().unwrap())
                .unwrap()
        };
        assert!(value("blocks") >= 30, "{result:?}");
        assert!(value("lost") >= 3, "{result:?}");
    }

    #[test]
    fn a_cards_port_is_the_usb_path_before_its_interface() {
        let device = "/sys/devices/platform/soc@3000000/4200000.ehci1-controller/usb3/3-1/3-1.1/3-1.1.1/3-1.1.1:1.1";
        assert_eq!(port_of(device).as_deref(), Some("3-1.1.1"));
        assert_eq!(
            port_of("/sys/devices/platform/soc@3000000/soc@3000000:i2s4_mach"),
            None
        );
    }

    /// lamp-4ace on 17 Oct (#1178): the Jieli found at boot, then off USB
    /// twice. A disconnect on another port is not its own.
    #[test]
    fn the_kernels_records_give_the_jieli_port_and_its_disconnects() {
        let mut records = UsbRecords::default();
        for line in [
            "6,410,2100000,-;usb 3-1.1.1: new full-speed USB device number 4 using ehci-platform",
            "6,411,2200000,-;usb 3-1.1.1: Manufacturer: Jieli Technology",
            "6,412,2300000,-;usb 3-1.1.3: Manufacturer: C-Media Electronics Inc.",
            "6,9001,44960000000,-;usb 3-1.1.1: USB disconnect, device number 4",
            "6,9020,44965000000,-;usb 3-1.1.1: Manufacturer: Jieli Technology",
            "6,9500,46146000000,-;usb 3-1.1.1: USB disconnect, device number 9",
            "6,9600,46200000000,-;usb 3-1.1.3: USB disconnect, device number 5",
            " SUBSYSTEM=usb",
        ] {
            records.count(line);
        }
        assert_eq!(records.jieli_ports, vec!["3-1.1.1".to_owned()]);
        assert_eq!(records.drops("3-1.1.1"), 2);
        assert_eq!(records.drops("3-1.1.3"), 1);
    }

    /// Read from a sysfs and a kernel log laid out as lamp-75c9's: the
    /// card there gives the port; with the card gone, the kernel's Jieli
    /// record still names it.
    #[cfg(target_os = "linux")]
    #[test]
    fn its_card_and_drops_are_read_from_sysfs_and_the_kernels_log() {
        let root = std::env::temp_dir().join(format!("lamp-selftest-usb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let device = root.join("devices/usb3/3-1/3-1.1/3-1.1.1/3-1.1.1:1.1");
        std::fs::create_dir_all(&device).unwrap();
        let sound = root.join("sound");
        for (card, id) in [("card2", "device_cmedia"), ("card4", CARD)] {
            std::fs::create_dir_all(sound.join(card)).unwrap();
            std::fs::write(sound.join(card).join("id"), format!("{id}\n")).unwrap();
        }
        std::os::unix::fs::symlink(&device, sound.join("card4/device")).unwrap();
        let kmsg = root.join("kmsg");
        std::fs::write(
            &kmsg,
            "6,0,0,-;Booting\n6,1,1,-;usb 3-1.1.1: Manufacturer: Jieli Technology\n6,2,2,-;usb 3-1.1.1: USB disconnect, device number 4\n",
        )
        .unwrap();
        let kmsg = kmsg.to_str().unwrap();
        let there = usb(&sound, kmsg, Instant::now() + Duration::from_secs(1));
        assert_eq!(
            there,
            Usb {
                present: true,
                jieli: true,
                port: Some("3-1.1.1".into()),
                drops: Some(1),
                since_boot: true,
                log_read: true,
                scan_stop: Some(kernel::ScanStop::Complete),
                hub: Some(Hub {
                    port: "3-1.1".into(),
                    ..Hub::default()
                }),
            }
        );
        std::fs::remove_dir_all(sound.join("card4")).unwrap();
        let gone = usb(&sound, kmsg, Instant::now() + Duration::from_secs(1));
        assert_eq!(
            (gone.present, gone.port.as_deref(), gone.drops),
            (false, Some("3-1.1.1"), Some(1))
        );
        assert!(gone.jieli && gone.since_boot);
        let unread = usb(
            &sound,
            "/nonexistent/kmsg",
            Instant::now() + Duration::from_secs(1),
        );
        assert_eq!(
            (unread.present, unread.jieli, unread.drops),
            (false, false, None)
        );
        // A ring that no longer starts at the boot's first record: the
        // drops are "at least".
        let wrapped = root.join("wrapped");
        std::fs::write(
            &wrapped,
            "6,5000,9,-;usb 3-1.1.1: Manufacturer: Jieli Technology\n6,5001,10,-;usb 3-1.1.1: USB disconnect, device number 9\n",
        )
        .unwrap();
        let late = usb(
            &sound,
            wrapped.to_str().unwrap(),
            Instant::now() + Duration::from_secs(1),
        );
        assert_eq!(
            (late.jieli, late.drops, late.since_boot),
            (true, Some(1), false)
        );
        // An image whose voice mic is no Jieli: no card by that name, and
        // none in the kernel's log.
        let other = root.join("other");
        std::fs::write(
            &other,
            "6,0,0,-;Booting\n6,1,1,-;usb 3-1.1.1: Manufacturer: Seeed\n",
        )
        .unwrap();
        let none = usb(
            &sound,
            other.to_str().unwrap(),
            Instant::now() + Duration::from_secs(1),
        );
        assert_eq!(
            (none.present, none.jieli, none.port, none.drops),
            (false, false, None, None)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// lamp-4ace on 17 Oct at 17:12 (the first builder, #705 17:31): the
    /// inner hub `3-1.1` dropped twice, and the mic, the body's servo port,
    /// the speaker and the camera went with it each time. Once more the mic
    /// dropped alone. Said as the hub, then the mic's own drop.
    #[test]
    fn a_whole_hub_dropping_is_said_as_the_hub_with_what_went_with_it() {
        let mut records = UsbRecords::default();
        for line in [
            "6,0,0,-;Booting Linux",
            "6,410,2100000,-;usb 3-1.1.1: Manufacturer: Jieli Technology",
            "6,411,2110000,-;usb 3-1.1.3: Manufacturer: C-Media Electronics Inc.",
            "6,412,2120000,-;usb 3-1.1.4: Product: USB_Camera",
            "6,413,2130000,-;cdc_acm 3-1.1.2:1.0: ttyACM0: USB ACM device",
            "6,9000,62649000000,-;usb 3-1.1: USB disconnect, device number 3",
            "6,9001,62649010000,-;usb 3-1.1.1: USB disconnect, device number 4",
            "6,9002,62649020000,-;usb 3-1.1.2: USB disconnect, device number 5",
            "6,9003,62649030000,-;usb 3-1.1.3: USB disconnect, device number 6",
            "6,9004,62649040000,-;usb 3-1.1.4: USB disconnect, device number 7",
            // A port that only starts with the hub's name is not under it.
            "6,9005,62649050000,-;usb 3-1.10: USB disconnect, device number 2",
            "6,9100,62656000000,-;usb 3-1.1: USB disconnect, device number 8",
            "6,9101,62656010000,-;usb 3-1.1.1: USB disconnect, device number 9",
            "6,9102,62656020000,-;usb 3-1.1.2: USB disconnect, device number 10",
            "6,9103,62656030000,-;usb 3-1.1.3: USB disconnect, device number 11",
            "6,9104,62656040000,-;usb 3-1.1.4: USB disconnect, device number 12",
            "6,9500,70000000000,-;usb 3-1.1.1: USB disconnect, device number 13",
        ] {
            records.count(line);
        }
        let hub = records.hub("3-1.1", "3-1.1.1");
        assert_eq!(
            hub,
            Hub {
                port: "3-1.1".into(),
                drops: 2,
                took: [
                    "the voice mic",
                    "the body's servo port",
                    "the speaker with the room mic",
                    "the camera"
                ]
                .map(String::from)
                .to_vec(),
                mic_with_it: 2,
            }
        );
        let usb = Usb {
            present: true,
            jieli: true,
            port: Some("3-1.1.1".into()),
            drops: Some(records.drops("3-1.1.1")),
            since_boot: true,
            log_read: true,
            scan_stop: Some(kernel::ScanStop::Complete),
            hub: Some(hub),
        };
        let mut room = Heard::default();
        for sequence in 0..50 {
            room.add(&block(sequence, 300)).unwrap();
        }
        let heard = voice_mic(true, &Ok(room), Some(&usb));
        assert_eq!(heard.verdict, Verdict::Pass);
        assert_eq!(
            heard.reason,
            "the lamp's USB hub dropped 2 times since boot; the voice mic, the body's servo port, \
             the speaker with the room mic and the camera went with it; \
             the voice mic alone dropped 1 time since boot"
        );
        let value = |key: &str| {
            heard
                .values
                .iter()
                .find(|v| v.key == key)
                .map(|v| v.value.clone())
        };
        assert_eq!(value("usb_hub").as_deref(), Some("3-1.1"));
        assert_eq!(value("usb_hub_drops").as_deref(), Some("2"));
        assert!(
            value("usb_hub_said")
                .unwrap()
                .starts_with("the lamp's USB hub dropped 2 times")
        );
        assert_eq!(
            value("usb_hub_said_short").as_deref(),
            Some(
                "the lamp's USB hub dropped 2 times since boot, with the voice mic, servo port, \
                 speaker/room mic and camera"
            )
        );
        // A hub that never dropped says nothing of itself.
        let calm = Usb {
            hub: Some(Hub {
                port: "3-1.1".into(),
                ..Hub::default()
            }),
            drops: Some(0),
            ..usb.clone()
        };
        assert!(calm.hub_said().is_none());
        // The kernel's log wrapped: "at least", and no more, so the line
        // fits `lamp/health`'s.
        let wrapped = Usb {
            since_boot: false,
            ..usb.clone()
        };
        assert_eq!(
            wrapped.hub_said().unwrap(),
            "the lamp's USB hub dropped at least 2 times since boot; the voice mic, \
             the body's servo port, the speaker with the room mic and the camera went with it"
        );
        // The mic gone after the hub's drops: its reason says only its own
        // drop, and leaves the hub's line to `usb_hub_said`.
        let gone = Usb {
            present: false,
            ..usb
        };
        let off = voice_mic(true, &Ok(Heard::default()), Some(&gone));
        assert_eq!(off.verdict, Verdict::Fail);
        assert_eq!(
            off.reason,
            format!(
                "not on USB now: no sound card {CARD}, the voice mic alone dropped 1 time since boot"
            )
        );
        assert!(off.values.iter().any(|v| v.key == "usb_hub_said"));
    }

    /// Off USB now and nothing heard: a fail, with its drops. On USB, heard
    /// and dropped twice: a pass that says so. Drops never fail it alone.
    #[test]
    fn a_microphone_off_usb_fails_and_its_drops_are_said() {
        let gone = Usb {
            present: false,
            jieli: true,
            port: Some("3-1.1.1".into()),
            drops: Some(2),
            since_boot: true,
            log_read: true,
            scan_stop: Some(kernel::ScanStop::Complete),
            hub: None,
        };
        let off = voice_mic(true, &Ok(Heard::default()), Some(&gone));
        assert_eq!(off.verdict, Verdict::Fail);
        assert_eq!(
            off.reason,
            "not on USB now: no sound card device_micro2, off USB 2 times since boot"
        );
        let mut room = Heard::default();
        for sequence in 0..50 {
            room.add(&block(sequence, 300)).unwrap();
        }
        let back = Usb {
            present: true,
            ..gone.clone()
        };
        let heard = voice_mic(true, &Ok(room.clone()), Some(&back));
        assert_eq!(heard.verdict, Verdict::Pass);
        assert_eq!(heard.reason, "off USB 2 times since boot");
        let value = |result: &PartResult, key: &str| {
            result
                .values
                .iter()
                .find(|v| v.key == key)
                .map(|v| v.value.clone())
        };
        assert_eq!(value(&heard, "usb_drops").as_deref(), Some("2"));
        assert_eq!(value(&heard, "usb_port").as_deref(), Some("3-1.1.1"));
        assert_eq!(value(&heard, "on_usb").as_deref(), Some("true"));
        let steady = voice_mic(
            true,
            &Ok(room),
            Some(&Usb {
                drops: Some(0),
                ..back
            }),
        );
        assert_eq!(
            (steady.verdict, steady.reason.as_str()),
            (Verdict::Pass, "")
        );
        // On USB, the voice not giving it: still not checked, as before.
        let quiet = voice_mic(
            true,
            &Ok(Heard::default()),
            Some(&Usb {
                present: true,
                jieli: true,
                port: None,
                drops: None,
                since_boot: true,
                log_read: true,
                scan_stop: Some(kernel::ScanStop::Complete),
                hub: None,
            }),
        );
        assert_eq!(quiet.verdict, Verdict::NotChecked);
    }

    /// The reviewer's P3s on #1304. Another image's voice mic, muted: no
    /// `device_micro2` card and no Jieli in the log is no fail, and nothing
    /// is said of USB. A log that no longer reaches back to boot says "at
    /// least". With no port known, the drops are unknown, never 0.
    #[test]
    fn only_a_jieli_lamp_fails_off_usb_and_drops_are_said_as_far_as_the_log_reaches() {
        let value = |result: &PartResult, key: &str| {
            result
                .values
                .iter()
                .find(|v| v.key == key)
                .map(|v| v.value.clone())
        };
        let read = Usb {
            since_boot: true,
            log_read: true,
            hub: None,
            ..Usb::default()
        };
        let other = voice_mic(true, &Ok(Heard::default()), Some(&read));
        assert_eq!(other.verdict, Verdict::NotChecked, "{other:?}");
        assert!(value(&other, "usb_drops").is_none());
        assert_eq!(
            value(&other, "on_usb").as_deref(),
            Some("unknown: no Jieli card here, and none in the kernel's log")
        );
        // By hand, without CAP_SYSLOG, or with the ring wrapped past the
        // Jieli's record: never a fail, and the words say which (the
        // reviewer on #1308).
        let by_hand = voice_mic(true, &Ok(Heard::default()), Some(&Usb::default()));
        assert_eq!(by_hand.verdict, Verdict::NotChecked);
        assert!(
            value(&by_hand, "on_usb")
                .unwrap()
                .contains("could not be read")
        );
        let wrapped_past = Usb {
            log_read: true,
            hub: None,
            ..Usb::default()
        };
        let late = voice_mic(true, &Ok(Heard::default()), Some(&wrapped_past));
        assert!(
            value(&late, "on_usb")
                .unwrap()
                .contains("no longer reaches back to boot")
        );
        let wrapped = Usb {
            present: false,
            jieli: true,
            port: Some("3-1.1.1".into()),
            drops: Some(2),
            since_boot: false,
            log_read: true,
            scan_stop: Some(kernel::ScanStop::Complete),
            hub: None,
        };
        let off = voice_mic(true, &Ok(Heard::default()), Some(&wrapped));
        assert_eq!(off.verdict, Verdict::Fail);
        assert_eq!(
            off.reason,
            "not on USB now: no sound card device_micro2, off USB at least 2 times since boot (the kernel's log no longer reaches back to boot)"
        );
        assert_eq!(value(&off, "usb_drops").as_deref(), Some("at least 2"));
        let portless = Usb {
            port: None,
            drops: None,
            ..wrapped
        };
        let off = voice_mic(true, &Ok(Heard::default()), Some(&portless));
        assert_eq!(off.reason, "not on USB now: no sound card device_micro2");
        assert!(value(&off, "usb_drops").unwrap().starts_with("unknown"));
    }
}
