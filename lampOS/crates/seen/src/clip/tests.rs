use super::*;
use crate::activity::{FAST_EVERY_US, fastest_us};

const S: u64 = 1_000_000;
const NOW: u64 = 1_800_000_000;

fn clip(every_s: u64, until_unix_s: u64) -> Vec<u8> {
    format!(r#"{{"every_s":{every_s},"until_unix_s":{until_unix_s}}}"#).into_bytes()
}

/// With the switch on, the cloud's value in force and in 15-60 s is the
/// period.
#[test]
fn the_clouds_value_in_range_and_in_force_is_the_period() {
    let on = fastest_us(Some("on"));
    for every_s in [15, 30, 60] {
        assert_eq!(
            every_us(Some(&clip(every_s, NOW + 1)), NOW, on),
            every_s * S
        );
    }
}

/// Each fallback is a minute.
#[test]
fn every_fallback_is_a_minute() {
    let on = fastest_us(Some("on"));
    assert_eq!(every_us(None, NOW, on), EVERY_US, "no file");
    for (text, why) in [
        (b"".to_vec(), "empty"),
        (b"not json".to_vec(), "not JSON"),
        (br#"{"every_s":30}"#.to_vec(), "no until"),
        (br#"{"until_unix_s":1800000300}"#.to_vec(), "no every_s"),
        (
            br#"{"every_s":30.5,"until_unix_s":1800000300}"#.to_vec(),
            "not whole seconds",
        ),
        (
            br#"{"every_s":-30,"until_unix_s":1800000300}"#.to_vec(),
            "negative",
        ),
        (
            br#"{"every_s":"30","until_unix_s":1800000300}"#.to_vec(),
            "a string",
        ),
        (
            br#"{"every_s":30,"until_unix_s":1800000300,"x":1}"#.to_vec(),
            "another key",
        ),
        (br#"[30,1800000300]"#.to_vec(), "not an object"),
        (clip(14, NOW + 300), "under 15 s"),
        (clip(0, NOW + 300), "0 s"),
        (clip(61, NOW + 300), "over a minute"),
        (clip(u64::MAX, NOW + 300), "huge"),
        (clip(30, NOW), "at until"),
        (clip(30, NOW - 1), "past until"),
    ] {
        assert_eq!(every_us(Some(&text), NOW, on), EVERY_US, "{why}");
    }
}

/// The cloud's value never beats the switch: off, a minute whatever it says.
#[test]
fn the_clouds_value_never_beats_the_switch() {
    let off = fastest_us(None);
    for every_s in [15, 30, 59, 60] {
        assert_eq!(
            every_us(Some(&clip(every_s, NOW + 300)), NOW, off),
            EVERY_US
        );
    }
    assert_eq!(fastest_us(Some("on")), FAST_EVERY_US);
}

/// The heat pace multiplies the cloud's value and never makes it faster.
#[test]
fn the_heat_multiplies_the_clouds_value() {
    use crate::heat::Pace;
    use lamp_msgs::HeatLevel;
    let every = every_us(Some(&clip(30, NOW + 300)), NOW, fastest_us(Some("on")));
    assert_eq!(
        Pace::at_every(HeatLevel::Warm, true, every).burst_every_us,
        60 * S
    );
    assert_eq!(
        Pace::at_every(HeatLevel::Hot, true, every).burst_every_us,
        120 * S
    );
}

/// The file: read whole, a file over the bound or missing is a minute, and
/// a change is read again.
#[test]
fn the_file_is_read_and_a_large_or_missing_one_is_a_minute() {
    let folder = std::env::temp_dir().join(format!("lamp-seen-clip-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("seen-clip.json");
    let _ = std::fs::remove_file(&path);
    let mut kept = ClipIn::at(path.clone(), FAST_EVERY_US);
    assert_eq!(kept.every_us(NOW), EVERY_US, "missing");
    std::fs::write(&path, clip(20, NOW + 300)).unwrap();
    kept.read = None;
    assert_eq!(kept.every_us(NOW), 20 * S);
    assert_eq!(kept.every_us(NOW + 300), 20 * S, "held until the next read");
    kept.read = None;
    assert_eq!(kept.every_us(NOW + 300), EVERY_US, "lapsed");
    let mut large = clip(20, NOW + 300);
    large.resize(lamp_msgs::consent::MOST_BYTES as usize + 1, b' ');
    std::fs::write(&path, large).unwrap();
    kept.read = None;
    assert_eq!(kept.every_us(NOW), EVERY_US, "over the bound");
    std::fs::remove_dir_all(&folder).unwrap();
}
