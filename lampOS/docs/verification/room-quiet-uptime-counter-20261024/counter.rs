use lamp_voice::room_told::{Kept, Room, RoomTold};

fn release_seed(now_us: u64) -> Room {
    let mut room = RoomTold::new();
    room.count_only_talk(true);
    room.count_held_talk(true);
    let first = now_us - 400_000_000;
    let second = now_us - 90_000_000;
    room.at(first);
    let actual = room.at(second);
    println!("now_us={now_us} first={first} second={second} elapsed={} room={actual:?}", second.saturating_sub(first));
    actual
}

#[test]
fn original_quiet_assertion_at_300_seconds() {
    assert_eq!(release_seed(300_000_000), Room::Quiet);
}

#[test]
fn release_arithmetic_characterizes_the_interval() {
    for (now, expected) in [
        (89_999_999, Room::Quiet), (90_000_000, Room::Unknown),
        (300_000_000, Room::Unknown), (399_999_999, Room::Unknown),
        (400_000_000, Room::Quiet), (500_000_000, Room::Quiet),
    ] {
        assert_eq!(release_seed(now), expected);
    }
}

#[test]
fn quiet_precondition_is_restorable_without_future_clock_values() {
    for now_us in [0, 89_999_999, 90_000_000, 300_000_000, 400_000_000] {
        let mut room = RoomTold::new();
        room.count_only_talk(true);
        room.count_held_talk(true);
        let kept = Kept {
            boot: "fixture".to_owned(), at_us: now_us, room: Room::Quiet,
            quiet_since_us: Some(now_us), watching_since_us: Some(now_us),
            last_talk_us: None, last_sound_us: now_us,
        };
        assert_eq!(room.take_back(&kept, "fixture", now_us), Ok(0));
        assert_eq!(room.at(now_us), Room::Quiet);
    }
}
