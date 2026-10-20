//! Explicit MPR121 register policy. Synchronous I/O belongs in the isolated worker.
//!
//! No profile is a default or a calibration claim. See `docs/controls.md`.

use serde::{Deserialize, Serialize};
use std::{fmt, io, time::Duration};

const STATUS: u8 = 0x00;
/// Filtered data, two bytes per electrode (10 bits, low byte first), then one
/// baseline byte per electrode (its upper 8 of 10 bits), ending at 0x2a.
const FILTERED: u8 = 0x04;
const SIGNALS_LEN: usize = 0x2b - FILTERED as usize;
const CDC: u8 = 0x5f;
const CHARGE_LEN: usize = 0x72 - CDC as usize;
const CONFIG_START: u8 = 0x2b;
const CONFIG_END: u8 = 0x7f;
const CONFIG_LEN: usize = (CONFIG_END - CONFIG_START + 1) as usize;
/// A served read checks one of this many equal parts of the static
/// configuration, in turn, so the whole of it every this many reads (at
/// most 250 ms at the slowest `every_ms`). Read whole every sample, its 85
/// bytes were about 1,000 bit-clocks: ~9.7 ms at the 100 kHz the air
/// sensor needs on the same bus, past the strip's 5 ms pace; a part is
/// ~3.5 ms of wire time there, worked out from bit-clocks (the first
/// builder, #705 17 Oct 19:04).
pub const CONFIG_PARTS: usize = 5;
const CONFIG_PART_LEN: usize = CONFIG_LEN / CONFIG_PARTS;
const _: () = assert!(CONFIG_LEN.is_multiple_of(CONFIG_PARTS));
const ECR: u8 = 0x5e;
const RESET: u8 = 0x80;
const RESET_WAIT_US: u64 = 1_000;
const MIN_SETTLE_US: u64 = 100_000;
const MAX_SETTLE_US: u64 = 5_000_000;

/// `pads` selects 3..12 unique working electrodes in physical order. Sparse
/// orders must ascend or descend; complete orders retain arbitrary permutations.
/// This is a software selection, not an electrical scan mask: ECR still enables
/// all twelve electrodes. Nothing is opened by deserialization or validation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub bus: u8,
    pub address: u16,
    pub every_ms: u64,
    pub pads: Vec<u8>,
    pub profile: Profile,
}

/// Explicit electrical settings. CDC units are microamps; CDT values are the
/// register codes: 1=0.5us, 2=1us, ..., 7=32us. Per-pad zero inherits the global.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub touch_thresholds: [u8; 12],
    pub release_thresholds: [u8; 12],
    pub baseline: Baseline,
    /// First-filter sample count: 6, 10, 18 or 34.
    pub ffi: u8,
    /// Second-filter sample count: 4, 6, 10 or 18.
    pub sfi: u8,
    /// Electrode sampling interval: 1, 2, 4, 8, 16, 32, 64 or 128 ms.
    pub esi_ms: u8,
    pub global_cdc: u8,
    pub global_cdt: u8,
    pub cdc: [u8; 12],
    pub cdt: [u8; 12],
    /// Encoded 0..7; N requires N+1 consecutive detections.
    pub debounce_touch: u8,
    pub debounce_release: u8,
    /// 0x8c..0x8f: all twelve electrodes, proximity off, baseline CL=10.
    pub ecr: u8,
    pub autoconfig: AutoConfig,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Baseline {
    pub rising: Filter,
    pub falling: Filter,
    pub touched: TouchFilter,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Filter {
    pub mhd: u8,
    pub nhd: u8,
    pub ncl: u8,
    pub fdl: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TouchFilter {
    /// Zero deliberately disables the touched baseline adjustment, as in V1.
    pub nhd: u8,
    pub ncl: u8,
    pub fdl: u8,
}

/// FFI and baseline initialization bits are derived from the matching profile;
/// hardware retry and unused interrupt bits remain zero. No host retry exists.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum AutoConfig {
    // An empty struct variant, so a newer field beside `mode` is read past
    // and named, as in any struct of the pins file (#1680).
    Disabled {},
    Enabled {
        upper_limit: u8,
        lower_limit: u8,
        target: u8,
        reconfigure: bool,
        skip_cdt_search: bool,
    },
}

impl Config {
    pub fn check(&self) -> Result<(), String> {
        if !(0x5a..=0x5d).contains(&self.address) {
            return Err("MPR121 address must be 0x5a..0x5d".into());
        }
        if !(5..=50).contains(&self.every_ms) {
            return Err("MPR121 every_ms must be 5..50".into());
        }
        // Share the recognizer's geometry boundary so a valid electrical
        // configuration cannot hand it an ambiguous sparse physical order.
        crate::base::strip::StripRecognizer::new(&self.pads)
            .map_err(|error| format!("MPR121 {error}"))?;
        self.profile.check()
    }

    /// Conservative sensing wait, excluding the initial 1ms reset wait and I/O.
    /// This is a chosen startup bound, not a silicon calibration-complete flag.
    pub fn settle_us(&self) -> Result<u64, String> {
        self.check()?;
        Ok(self.profile.settle_unchecked())
    }
}

impl Profile {
    pub fn check(&self) -> Result<(), String> {
        if self
            .touch_thresholds
            .iter()
            .zip(self.release_thresholds)
            .any(|(&touch, release)| release >= touch)
        {
            return Err("MPR121 thresholds require release < touch for each electrode".into());
        }
        for filter in [&self.baseline.rising, &self.baseline.falling] {
            if !(1..=63).contains(&filter.mhd) || !(1..=63).contains(&filter.nhd) {
                return Err("MPR121 rising/falling MHD and NHD must be 1..63".into());
            }
        }
        if self.baseline.touched.nhd > 63 {
            return Err("MPR121 touched NHD must be 0..63".into());
        }
        if ![6, 10, 18, 34].contains(&self.ffi)
            || ![4, 6, 10, 18].contains(&self.sfi)
            || ![1, 2, 4, 8, 16, 32, 64, 128].contains(&self.esi_ms)
        {
            return Err("MPR121 FFI, SFI or ESI is not an encoded hardware setting".into());
        }
        if !(1..=63).contains(&self.global_cdc) || self.cdc.iter().any(|&cdc| cdc > 63) {
            return Err("MPR121 global CDC must be 1..63; per-electrode CDC must be 0..63".into());
        }
        if !(1..=7).contains(&self.global_cdt) || self.cdt.iter().any(|&cdt| cdt > 7) {
            return Err("MPR121 global CDT must be 1..7; per-electrode CDT must be 0..7".into());
        }
        if self.debounce_touch > 7 || self.debounce_release > 7 {
            return Err("MPR121 touch/release debounce must be 0..7".into());
        }
        if !(0x8c..=0x8f).contains(&self.ecr) {
            return Err("MPR121 ECR must enable all12, proximity off, baseline CL=10".into());
        }
        if let AutoConfig::Enabled {
            upper_limit,
            lower_limit,
            target,
            ..
        } = self.autoconfig
            && !(0 < lower_limit
                && lower_limit < target
                && target < upper_limit
                && upper_limit < 255)
        {
            return Err("MPR121 autoconfig requires 0 < lower < target < upper < 255".into());
        }
        if self.settle_unchecked() > MAX_SETTLE_US {
            return Err("MPR121 profile requires more than the 5s startup sensing bound".into());
        }
        Ok(())
    }

    fn settle_unchecked(&self) -> u64 {
        // Charge+discharge is 2*CDT per FFI sample and enabled electrode. The
        // scan overrides ESI when longer. An automatic CDT search can choose
        // any valid CDT, so do not assume the captured/initial code survives.
        let max_cdt = if self.searches_cdt() {
            7
        } else {
            self.cdt
                .iter()
                .map(|&cdt| if cdt == 0 { self.global_cdt } else { cdt })
                .max()
                .unwrap_or(self.global_cdt)
        };
        let scan_us = u64::from(self.ffi) * 12 * (1_u64 << (max_cdt - 1));
        let update_us = scan_us.max(u64::from(self.esi_ms) * 1_000) * u64::from(self.sfi);
        // One initial baseline result, then N+1 touch/release decisions.
        let results = u64::from(self.debounce_touch.max(self.debounce_release)) + 2;
        MIN_SETTLE_US.max(update_us * results)
    }

    fn searches_cdt(&self) -> bool {
        matches!(
            self.autoconfig,
            AutoConfig::Enabled {
                skip_cdt_search: false,
                ..
            }
        )
    }

    fn registers(&self) -> [u8; CONFIG_LEN] {
        let mut bytes = [0; CONFIG_LEN];
        let mut set = |register: u8, value| bytes[usize::from(register - CONFIG_START)] = value;
        let baseline = &self.baseline;
        for (register, filter) in [(0x2b, &baseline.rising), (0x2f, &baseline.falling)] {
            set(register, filter.mhd);
            set(register + 1, filter.nhd);
            set(register + 2, filter.ncl);
            set(register + 3, filter.fdl);
        }
        set(0x33, baseline.touched.nhd);
        set(0x34, baseline.touched.ncl);
        set(0x35, baseline.touched.fdl);
        for electrode in 0_u8..12 {
            set(
                0x41 + 2 * electrode,
                self.touch_thresholds[electrode as usize],
            );
            set(
                0x42 + 2 * electrode,
                self.release_thresholds[electrode as usize],
            );
            set(0x5f + electrode, self.cdc[electrode as usize]);
        }
        for pair in 0_u8..6 {
            let index = usize::from(pair) * 2;
            set(0x6c + pair, self.cdt[index] | self.cdt[index + 1] << 4);
        }
        let ffi = match self.ffi {
            6 => 0,
            10 => 1,
            18 => 2,
            34 => 3,
            _ => unreachable!("profile checked before register encoding"),
        };
        let sfi = match self.sfi {
            4 => 0,
            6 => 1,
            10 => 2,
            18 => 3,
            _ => unreachable!("profile checked before register encoding"),
        };
        set(0x5b, self.debounce_release << 4 | self.debounce_touch);
        set(0x5c, ffi << 6 | self.global_cdc);
        set(
            0x5d,
            self.global_cdt << 5 | sfi << 3 | self.esi_ms.trailing_zeros() as u8,
        );
        set(ECR, self.ecr);
        if let AutoConfig::Enabled {
            upper_limit,
            lower_limit,
            target,
            reconfigure,
            skip_cdt_search,
        } = self.autoconfig
        {
            set(
                0x7b,
                ffi << 6 | ((self.ecr >> 6) << 2) | u8::from(reconfigure) << 1 | 1,
            );
            set(0x7c, u8::from(skip_cdt_search) << 7);
            set(0x7d, upper_limit);
            set(0x7e, lower_limit);
            set(0x7f, target);
        }
        bytes
    }
}

/// One reading of what the chip compares with its thresholds, for every
/// electrode: the 10-bit filtered signal and the 10-bit baseline. A finger
/// lowers the signal, so `baseline - filtered` (the delta) rises with touch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signals {
    pub touched: u16,
    pub filtered: [u16; 12],
    pub baseline: [u16; 12],
}

impl Signals {
    pub fn delta(&self, electrode: usize) -> i32 {
        i32::from(self.baseline[electrode]) - i32::from(self.filtered[electrode])
    }

    fn parse(touched: u16, bytes: &[u8; SIGNALS_LEN]) -> Self {
        let mut filtered = [0; 12];
        let mut baseline = [0; 12];
        for electrode in 0..12 {
            filtered[electrode] =
                u16::from_le_bytes([bytes[2 * electrode], bytes[2 * electrode + 1]]) & 0x3ff;
            baseline[electrode] = u16::from(bytes[26 + electrode]) << 2;
        }
        Self {
            touched,
            filtered,
            baseline,
        }
    }
}

/// The charge current (µA) and time code each electrode uses now, as
/// auto-configuration left them. Zero means it inherits the global value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Charge {
    pub cdc: [u8; 12],
    pub cdt: [u8; 12],
}

/// Implementations must fill the complete read buffer or return an error, and
/// must report short transfers as errors. The maximum requested read is 85B.
pub trait RegisterIo {
    fn read(&mut self, register: u8, bytes: &mut [u8]) -> io::Result<()>;
    fn write(&mut self, register: u8, value: u8) -> io::Result<()>;
}

pub trait Delay {
    fn wait_us(&mut self, microseconds: u64);
}

impl<F: FnMut(u64)> Delay for F {
    fn wait_us(&mut self, microseconds: u64) {
        self(microseconds);
    }
}

pub struct ThreadDelay;

impl Delay for ThreadDelay {
    fn wait_us(&mut self, microseconds: u64) {
        std::thread::sleep(Duration::from_micros(microseconds));
    }
}

#[derive(Debug)]
pub enum Error {
    InvalidConfig(String),
    Io(io::Error),
    RegisterMismatch {
        register: u8,
        expected: u8,
        actual: u8,
    },
    OverCurrent,
    OutOfRange {
        electrodes: u16,
        proximity: bool,
    },
    AutoConfigurationFailed,
    AutoReconfigurationFailed,
    UnexpectedStatus,
    Unavailable,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(reason) => formatter.write_str(reason),
            Self::Io(error) => write!(formatter, "MPR121 I/O failed ({:?})", error.kind()),
            Self::RegisterMismatch {
                register,
                expected,
                actual,
            } => write!(
                formatter,
                "MPR121 register 0x{register:02x} mismatch: expected 0x{expected:02x}, got 0x{actual:02x}"
            ),
            Self::OverCurrent => formatter.write_str("MPR121 overcurrent; sensing unavailable"),
            Self::OutOfRange {
                electrodes,
                proximity,
            } => write!(
                formatter,
                "MPR121 out of range: electrodes=0x{electrodes:03x}, proximity={proximity}"
            ),
            Self::AutoConfigurationFailed => formatter.write_str("MPR121 autoconfiguration failed"),
            Self::AutoReconfigurationFailed => {
                formatter.write_str("MPR121 autoreconfiguration failed")
            }
            Self::UnexpectedStatus => formatter.write_str("MPR121 unexpected status bits"),
            Self::Unavailable => formatter.write_str("MPR121 unavailable after a prior failure"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub struct Device<T: RegisterIo> {
    io: T,
    expected: [u8; CONFIG_LEN],
    selected: u16,
    check_cdc: bool,
    check_cdt: bool,
    failed: bool,
    /// The part of the static configuration the next read checks.
    part: usize,
}

impl<T: RegisterIo> Device<T> {
    pub fn initialize(io: T, config: Config, delay: &mut impl Delay) -> Result<Self, Error> {
        config.check().map_err(Error::InvalidConfig)?;
        let mut device = Self {
            io,
            expected: config.profile.registers(),
            selected: config.pads.iter().fold(0, |mask, pad| mask | (1 << pad)),
            check_cdc: matches!(config.profile.autoconfig, AutoConfig::Disabled {}),
            check_cdt: !config.profile.searches_cdt(),
            failed: false,
            part: 0,
        };
        // A known active overcurrent must not be erased by a convenient reset.
        let mut status = [0; 4];
        device.io.read(STATUS, &mut status)?;
        if status[1] & 0x80 != 0 {
            return Err(Error::OverCurrent);
        }
        device.io.write(RESET, 0x63)?;
        delay.wait_us(RESET_WAIT_US);
        device.io.write(ECR, 0)?;
        let mut reset = [0; CONFIG_LEN];
        reset[usize::from(0x5c - CONFIG_START)] = 0x10;
        reset[usize::from(0x5d - CONFIG_START)] = 0x24;
        device.verify_registers(&reset, true, true)?;
        // All control writes happen in verified Stop. Proximity stays disabled.
        // GPIO is unused; reset/readback establishes its disabled configuration.
        for register in CONFIG_START..=CONFIG_END {
            if register != ECR && !(0x73..=0x7a).contains(&register) {
                device.io.write(
                    register,
                    device.expected[usize::from(register - CONFIG_START)],
                )?;
            }
        }
        let mut stopped = device.expected;
        stopped[usize::from(ECR - CONFIG_START)] = 0;
        device.verify_registers(&stopped, true, true)?;
        device.io.write(ECR, config.profile.ecr)?;
        delay.wait_us(config.profile.settle_unchecked());
        // Never publish the startup validation sample. The worker takes a new
        // acquisition timestamp for its first read after this method returns.
        device.read()?;
        Ok(device)
    }

    /// Return the complete twelve-pad raw mask, including excluded electrodes.
    /// Selected-electrode OOR and every global/configuration/I/O fault are
    /// terminal for this object; subsequent calls perform no I/O.
    pub fn read(&mut self) -> Result<u16, Error> {
        if self.failed {
            return Err(Error::Unavailable);
        }
        let result = self.read_validated();
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    /// The touch mask with every electrode's signal and baseline, checked like
    /// [`Self::read`]. For calibration only: the served path reads the mask.
    pub fn read_signals(&mut self) -> Result<Signals, Error> {
        if self.failed {
            return Err(Error::Unavailable);
        }
        let result = self.read_validated().and_then(|touched| {
            let mut bytes = [0; SIGNALS_LEN];
            self.io.read(FILTERED, &mut bytes)?;
            Ok(Signals::parse(touched, &bytes))
        });
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    /// The per-electrode charge settings the chip holds now.
    pub fn charge(&mut self) -> Result<Charge, Error> {
        if self.failed {
            return Err(Error::Unavailable);
        }
        let mut bytes = [0; CHARGE_LEN];
        if let Err(error) = self.io.read(CDC, &mut bytes) {
            self.failed = true;
            return Err(error.into());
        }
        let mut charge = Charge {
            cdc: [0; 12],
            cdt: [0; 12],
        };
        for electrode in 0..12 {
            charge.cdc[electrode] = bytes[electrode] & 0x3f;
            let pair = bytes[usize::from(0x6c - CDC) + electrode / 2];
            charge.cdt[electrode] = if electrode % 2 == 0 { pair } else { pair >> 4 } & 0x07;
        }
        Ok(charge)
    }

    fn read_validated(&mut self) -> Result<u16, Error> {
        let mut status = [0; 4];
        self.io.read(STATUS, &mut status)?;
        check_status(status, self.selected)?;
        let expected = self.expected;
        let part = self.part;
        self.part = (part + 1) % CONFIG_PARTS;
        self.verify_part(&expected, part, self.check_cdc, self.check_cdt)?;
        // Check faults again after the static readback. The final ECR check,
        // every read, catches a reset between static readback and status,
        // and a reset at any time: it stops the electrodes.
        self.io.read(STATUS, &mut status)?;
        check_status(status, self.selected)?;
        let mut ecr = [0];
        self.io.read(ECR, &mut ecr)?;
        compare(ECR, self.expected[usize::from(ECR - CONFIG_START)], ecr[0])?;
        Ok(u16::from_le_bytes([status[0], status[1]]) & 0x0fff)
    }

    fn verify_registers(
        &mut self,
        expected: &[u8; CONFIG_LEN],
        check_cdc: bool,
        check_cdt: bool,
    ) -> Result<(), Error> {
        self.verify_range(expected, 0, CONFIG_LEN, check_cdc, check_cdt)
    }

    /// Part `part` of [`CONFIG_PARTS`].
    fn verify_part(
        &mut self,
        expected: &[u8; CONFIG_LEN],
        part: usize,
        check_cdc: bool,
        check_cdt: bool,
    ) -> Result<(), Error> {
        self.verify_range(
            expected,
            part * CONFIG_PART_LEN,
            CONFIG_PART_LEN,
            check_cdc,
            check_cdt,
        )
    }

    /// `len` registers from `CONFIG_START + from`, read at once and compared.
    fn verify_range(
        &mut self,
        expected: &[u8; CONFIG_LEN],
        from: usize,
        len: usize,
        check_cdc: bool,
        check_cdt: bool,
    ) -> Result<(), Error> {
        let mut actual = [0; CONFIG_LEN];
        let first = CONFIG_START + from as u8;
        self.io.read(first, &mut actual[from..from + len])?;
        for register in first..first + len as u8 {
            if (!check_cdc && (0x5f..=0x6a).contains(&register))
                || (!check_cdt && (0x6c..=0x71).contains(&register))
                // GPIO data and its set/clear/toggle aliases are not static.
                || register == 0x75
                || (0x78..=0x7a).contains(&register)
            {
                continue;
            }
            let index = usize::from(register - CONFIG_START);
            compare(register, expected[index], actual[index])?;
        }
        Ok(())
    }
}

fn compare(register: u8, expected: u8, actual: u8) -> Result<(), Error> {
    if expected != actual {
        return Err(Error::RegisterMismatch {
            register,
            expected,
            actual,
        });
    }
    Ok(())
}

fn check_status(status: [u8; 4], selected: u16) -> Result<(), Error> {
    if status[1] & 0x80 != 0 {
        return Err(Error::OverCurrent);
    }
    // Datasheet Rev4 (2013), section 5.12: ACFF is bit7 and ARFF is bit6.
    // The older AN3889 Rev1 table reverses these labels. Neither summary flag
    // is scoped to the selected pads: excluding a faulty electrode does not
    // excuse a failed auto-configuration or auto-reconfiguration cycle.
    if status[3] & 0x80 != 0 {
        return Err(Error::AutoConfigurationFailed);
    }
    if status[3] & 0x40 != 0 {
        return Err(Error::AutoReconfigurationFailed);
    }
    // Only individual electrode OOR bits follow the explicit working set.
    // ECR cannot skip interior electrodes, so those channels are still scanned
    // and can still trigger the global failures checked above. Proximity stays
    // disabled; its OOR bit must never be hidden by software pad selection.
    let range = u16::from_le_bytes([status[2], status[3]]) & (selected | 0x1000);
    if range != 0 {
        return Err(Error::OutOfRange {
            electrodes: range & 0x0fff,
            proximity: range & 0x1000 != 0,
        });
    }
    // Proximity is disabled. Reserved bits must not quietly become releases.
    if status[1] & 0x70 != 0 || status[3] & 0x20 != 0 {
        return Err(Error::UnexpectedStatus);
    }
    Ok(())
}

/// Linux transport only. No force-address, scan, global timeout/retry or bus
/// speed mutation. Whole-device exclusion and process lifetime belong to worker.
#[cfg(target_os = "linux")]
pub struct LinuxIo(i2cdev::linux::LinuxI2CDevice);

#[cfg(target_os = "linux")]
impl LinuxIo {
    pub fn open(config: &Config) -> io::Result<Self> {
        config
            .check()
            .map_err(|reason| io::Error::new(io::ErrorKind::InvalidInput, reason))?;
        i2cdev::linux::LinuxI2CDevice::new(format!("/dev/i2c-{}", config.bus), config.address)
            .map(Self)
            .map_err(Into::into)
    }
}

#[cfg(target_os = "linux")]
impl RegisterIo for LinuxIo {
    fn read(&mut self, register: u8, bytes: &mut [u8]) -> io::Result<()> {
        use i2cdev::core::{I2CMessage, I2CTransfer};
        use i2cdev::linux::LinuxI2CMessage;
        if bytes.is_empty() || bytes.len() > CONFIG_LEN {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid MPR121 transfer length",
            ));
        }
        let pointer = [register];
        let mut messages = [
            LinuxI2CMessage::write(&pointer),
            LinuxI2CMessage::read(bytes),
        ];
        let transferred = self.0.transfer(&mut messages).map_err(io::Error::from)?;
        check_transfer(transferred, 2)
    }

    fn write(&mut self, register: u8, value: u8) -> io::Result<()> {
        use i2cdev::core::{I2CMessage, I2CTransfer};
        use i2cdev::linux::LinuxI2CMessage;
        let bytes = [register, value];
        let mut messages = [LinuxI2CMessage::write(&bytes)];
        let transferred = self.0.transfer(&mut messages).map_err(io::Error::from)?;
        check_transfer(transferred, 1)
    }
}

#[cfg(any(target_os = "linux", test))]
fn check_transfer(transferred: u32, expected: u32) -> io::Result<()> {
    if transferred == expected {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "incomplete MPR121 I2C transaction",
        ))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn transport_requires_exact_message_count() {
        for expected in [1, 2] {
            for actual in 0..=3 {
                assert_eq!(
                    super::check_transfer(actual, expected).is_ok(),
                    actual == expected
                );
            }
        }
    }
}
