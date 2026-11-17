use super::*;
use lamp_msgs::DeviceStatus::{Error, Ok as Fine, Refused};

const WITHIN: Duration = Duration::from_millis(50);

/// A cloud of the test's own: the owner's desk and bottles, answered by
/// `req_id`, with what each call asked kept.
struct Fake {
    rows: Vec<DeviceRow>,
    /// A state by device id.
    states: Vec<(&'static str, DeviceStatus, DeviceState)>,
    /// The list answers this, in place of the rows.
    list: Option<NoAnswer>,
    /// A state asked answers `unlisted`, this many times.
    unlisted: usize,
    asked: Vec<DeviceAsk>,
}

fn row(id: &str, device_type: DeviceType) -> DeviceRow {
    DeviceRow {
        device_id: id.into(),
        device_type,
        name: id.into(),
        commands: vec![],
    }
}

fn fake(states: Vec<(&'static str, DeviceStatus, DeviceState)>) -> Fake {
    Fake {
        rows: vec![
            row("desk-1", DeviceType::Desk),
            row("hyd-1", DeviceType::Hydrate),
            row("hyd-2", DeviceType::Hydrate),
        ],
        states,
        list: None,
        unlisted: 0,
        asked: Vec::new(),
    }
}

impl Link for Fake {
    fn ask(&mut self, asks: Vec<DevicesAsk>, _within: Duration) -> Vec<DevicesAnswer> {
        let mut answers = Vec::new();
        for ask in asks {
            self.asked.push(ask.ask.clone());
            let answer = match &ask.ask {
                DeviceAsk::List => match self.list {
                    Some(why) => DeviceAnswer::None { why },
                    None => DeviceAnswer::List {
                        devices: self.rows.clone(),
                    },
                },
                DeviceAsk::State { device_id } if self.unlisted > 0 => {
                    let _ = device_id;
                    self.unlisted -= 1;
                    DeviceAnswer::None {
                        why: NoAnswer::Unlisted,
                    }
                }
                DeviceAsk::State { device_id } => {
                    let Some((_, status, state)) =
                        self.states.iter().find(|(id, ..)| id == device_id)
                    else {
                        continue;
                    };
                    DeviceAnswer::State {
                        device_id: device_id.clone(),
                        status: *status,
                        state: state.clone(),
                    }
                }
                DeviceAsk::Command { .. } => panic!("a read never commands"),
            };
            answers.push(DevicesAnswer {
                req_id: ask.req_id,
                answer,
            });
        }
        answers
    }
}

fn desk(height: Option<u16>, online: Option<bool>) -> DeviceState {
    DeviceState {
        height_cm: height,
        online,
        ..DeviceState::default()
    }
}

fn bottle(today: Option<u32>, goal: Option<u32>) -> DeviceState {
    DeviceState {
        today_ml: today,
        goal_ml: goal,
        sips_today: Some(5),
        online: Some(true),
        ..DeviceState::default()
    }
}

fn say(told: &(Value, bool)) -> &str {
    told.0["say"]
        .as_str()
        .or_else(|| told.0["error"].as_str())
        .unwrap()
}

#[test]
fn the_desks_height_is_said_as_the_cloud_gave_it() {
    let mut cloud = fake(vec![("desk-1", Fine, desk(Some(112), Some(true)))]);
    let told = run(&mut cloud, &mut Cache::default(), Read::Desk, WITHIN);
    assert!(told.1);
    assert_eq!(say(&told), "Your desk is at 112 centimetres.");
    assert_eq!(told.0["height_cm"], 112);
}

#[test]
fn a_height_the_cloud_has_not_got_is_never_made_up() {
    for (online, line) in [
        (Some(false), DESK_OFFLINE),
        (Some(true), HEIGHT_UNKNOWN),
        (None, HEIGHT_UNKNOWN),
    ] {
        let mut cloud = fake(vec![("desk-1", Fine, desk(None, online))]);
        let told = run(&mut cloud, &mut Cache::default(), Read::Desk, WITHIN);
        assert_eq!(say(&told), line, "{online:?}");
        assert!(told.0.get("height_cm").is_none());
    }
}

#[test]
fn a_desk_the_cloud_refuses_or_fails_on_is_said_unreached() {
    for status in [Refused, Error] {
        let mut cloud = fake(vec![("desk-1", status, desk(Some(100), Some(true)))]);
        let told = run(&mut cloud, &mut Cache::default(), Read::Desk, WITHIN);
        assert!(!told.1);
        assert_eq!(say(&told), DESK_UNREACHED);
        assert!(
            told.0.get("height_cm").is_none(),
            "no reading from a refusal"
        );
    }
    // Nothing answered the state at all.
    let mut cloud = fake(vec![]);
    let told = run(&mut cloud, &mut Cache::default(), Read::Desk, WITHIN);
    assert_eq!(say(&told), DESK_UNREACHED);
}

#[test]
fn the_water_total_is_the_bottle_drunk_from_most_and_in_litres_past_a_thousand() {
    let mut cloud = fake(vec![
        ("hyd-1", Fine, bottle(Some(200), Some(2000))),
        ("hyd-2", Fine, bottle(Some(1200), Some(2000))),
    ]);
    let told = run(&mut cloud, &mut Cache::default(), Read::Water, WITHIN);
    assert!(told.1);
    assert_eq!(say(&told), "You've had 1.2 litres today, of 2 litres.");
    assert_eq!(told.0["today_ml"], 1200);
    // Both bottles were asked together after one list.
    assert_eq!(cloud.asked.len(), 3);
}

#[test]
fn a_bottle_with_no_reading_yet_is_said_not_known() {
    let mut cloud = fake(vec![
        ("hyd-1", Fine, bottle(None, None)),
        ("hyd-2", Fine, bottle(None, None)),
    ]);
    let told = run(&mut cloud, &mut Cache::default(), Read::Water, WITHIN);
    assert_eq!(say(&told), WATER_UNKNOWN);
    let mut cloud = fake(vec![
        ("hyd-1", Error, bottle(Some(5), None)),
        ("hyd-2", Refused, bottle(Some(5), None)),
    ]);
    let told = run(&mut cloud, &mut Cache::default(), Read::Water, WITHIN);
    assert_eq!(say(&told), BOTTLE_UNREACHED);
}

#[test]
fn an_owner_with_no_such_device_is_told_so_and_nothing_is_asked_of_one() {
    let mut cloud = fake(vec![]);
    cloud
        .rows
        .retain(|row| row.device_type == DeviceType::Hydrate);
    let told = run(&mut cloud, &mut Cache::default(), Read::Desk, WITHIN);
    assert_eq!(say(&told), NO_DESK);
    assert_eq!(cloud.asked, vec![DeviceAsk::List]);
    let mut cloud = fake(vec![]);
    cloud.rows.retain(|row| row.device_type == DeviceType::Desk);
    let told = run(&mut cloud, &mut Cache::default(), Read::Water, WITHIN);
    assert_eq!(say(&told), NO_BOTTLE);
}

#[test]
fn no_answer_to_the_list_is_said_plainly_and_away_says_it_is_offline() {
    for (why, line) in [
        (
            NoAnswer::Timeout,
            "I couldn't reach your devices right now.",
        ),
        (NoAnswer::Busy, "I couldn't reach your devices right now."),
        (NoAnswer::Away, AWAY),
    ] {
        let mut cloud = fake(vec![]);
        cloud.list = Some(why);
        let told = run(&mut cloud, &mut Cache::default(), Read::Desk, WITHIN);
        assert!(!told.1);
        assert_eq!(say(&told), line, "{why:?}");
    }
    // Nobody on the bus at all.
    struct Silent;
    impl Link for Silent {
        fn ask(&mut self, _: Vec<DevicesAsk>, _: Duration) -> Vec<DevicesAnswer> {
            Vec::new()
        }
    }
    let told = run(&mut Silent, &mut Cache::default(), Read::Water, WITHIN);
    assert!(!told.1);
}

#[test]
fn a_repeated_read_is_one_round_trip_and_a_lamp_cloud_restart_asks_the_list_again() {
    let mut cache = Cache::default();
    let mut cloud = fake(vec![("desk-1", Fine, desk(Some(90), Some(true)))]);
    run(&mut cloud, &mut cache, Read::Desk, WITHIN);
    cloud.asked.clear();
    let told = run(&mut cloud, &mut cache, Read::Desk, WITHIN);
    assert_eq!(say(&told), "Your desk is at 90 centimetres.");
    assert_eq!(
        cloud.asked.len(),
        1,
        "the kept list: only the state is asked"
    );
    // lamp-cloud restarted and does not know the desk: the list is read again.
    cloud.asked.clear();
    cloud.unlisted = 1;
    let told = run(&mut cloud, &mut cache, Read::Desk, WITHIN);
    assert_eq!(say(&told), "Your desk is at 90 centimetres.");
    assert_eq!(cloud.asked[1], DeviceAsk::List);
    assert_eq!(cloud.asked.len(), 3);
}

#[test]
fn volumes_are_said_the_way_a_person_says_them() {
    for (ml, said) in [
        (0, "0 millilitres"),
        (250, "250 millilitres"),
        (999, "999 millilitres"),
        (1000, "1 litre"),
        (1200, "1.2 litres"),
        (1949, "1.9 litres"),
        (2000, "2 litres"),
    ] {
        assert_eq!(volume(ml), said);
    }
}

#[test]
fn the_functions_exist_only_with_the_switch_on_exactly_and_take_no_argument() {
    for value in [None, Some(""), Some("off"), Some("ON"), Some("1")] {
        assert!(arguments(value).is_empty(), "{value:?}");
    }
    assert_eq!(arguments(Some("on")), vec![FLAG.to_owned()]);
    let tools = tools();
    assert_eq!(
        tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        [DESK, WATER]
    );
    assert!(tools.iter().all(|tool| tool.parameters.is_none()));
    assert_eq!(read_of(DESK), Some(Read::Desk));
    assert_eq!(read_of(WATER), Some(Read::Water));
    assert_eq!(read_of("desk_move"), None);
}

/// The bus side, against a part of the test's own standing in for
/// `lamp-cloud`: asks by `req_id` come back by `req_id`.
#[test]
fn over_a_real_bus_the_asks_go_out_and_the_answers_come_back_by_req_id() {
    let root = std::path::PathBuf::from(format!("/tmp/vdt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let cloud_bus = lamp_bus::Bus::new(&root).unwrap();
    let mut asks = cloud_bus
        .subscribe_typed(DevicesAsk::TOPIC, "cloud", DevicesAsk::TYPE)
        .unwrap();
    let mut out = cloud_bus
        .publisher_declared(DevicesAnswer::TOPIC, &DevicesAnswer::declaration(), "cloud")
        .unwrap();
    let standing_in = std::thread::spawn(move || {
        let mut served = 0;
        while served < 2 {
            if let Ok(Some(message)) = asks.recv(Duration::from_secs(5)) {
                assert_eq!(message.header.source, "voice");
                let ask: DevicesAsk = message.parse().unwrap();
                let answer = match ask.ask {
                    DeviceAsk::List => DeviceAnswer::List {
                        devices: vec![row("desk-1", DeviceType::Desk)],
                    },
                    DeviceAsk::State { device_id } => DeviceAnswer::State {
                        device_id,
                        status: DeviceStatus::Ok,
                        state: desk(Some(105), Some(true)),
                    },
                    DeviceAsk::Command { .. } => panic!("a read never commands"),
                };
                out.publish_json(
                    lamp_ipc::monotonic_us(),
                    &DevicesAnswer {
                        req_id: ask.req_id,
                        answer,
                    },
                )
                .unwrap();
                served += 1;
            }
        }
    });
    let mut bus = Bus(Some(lamp_bus::Bus::new(&root).unwrap()));
    let began = Instant::now();
    let told = run(
        &mut bus,
        &mut Cache::default(),
        Read::Desk,
        Duration::from_secs(5),
    );
    assert_eq!(say(&told), "Your desk is at 105 centimetres.");
    // The bus's own cost, with an answerer that does nothing: a list and a
    // state, two round trips (the target, a second, is for the cloud's legs).
    assert!(
        began.elapsed() < Duration::from_secs(2),
        "{:?}",
        began.elapsed()
    );
    standing_in.join().unwrap();
    let _ = std::fs::remove_dir_all(&root);
}

/// The switch reaches the model's tools and the route: declared with it on,
/// nobody's with it off, and a skill of the same name is the voice's to
/// drop.
#[test]
fn the_switch_declares_the_reads_and_routes_them_and_off_offers_nothing() {
    use crate::tools::{Route, Settings, declared, route, take, voices_own};
    let plain = Settings::default();
    assert!(
        declared(&plain)
            .iter()
            .all(|tool| !is_tool(tool.name.as_str()))
    );
    assert_eq!(route(&plain, DESK), Route::Nobody);
    assert_eq!(route(&plain, WATER), Route::Nobody);
    let flags = arguments(Some("on"));
    let (on, rest) = take(&flags).unwrap();
    assert!(on.devices && rest.is_empty());
    let names: Vec<String> = declared(&on).into_iter().map(|tool| tool.name).collect();
    assert!(names.contains(&DESK.to_owned()) && names.contains(&WATER.to_owned()));
    assert_eq!(route(&on, DESK), Route::Devices(Read::Desk));
    assert_eq!(route(&on, WATER), Route::Devices(Read::Water));
    assert!(voices_own(DESK) && voices_own(WATER));
}

/// Fourteen's P2 on #7647: one deadline for the whole call, not one a round.
#[test]
fn one_deadline_is_shared_by_every_round_of_a_call() {
    struct Slow(Vec<Duration>);
    impl Link for Slow {
        fn ask(&mut self, asks: Vec<DevicesAsk>, within: Duration) -> Vec<DevicesAnswer> {
            self.0.push(within);
            std::thread::sleep(Duration::from_millis(60));
            asks.into_iter()
                .map(|ask| DevicesAnswer {
                    answer: match ask.ask {
                        DeviceAsk::List => DeviceAnswer::List {
                            devices: vec![row("desk-1", DeviceType::Desk)],
                        },
                        _ => DeviceAnswer::None {
                            why: NoAnswer::Timeout,
                        },
                    },
                    req_id: ask.req_id,
                })
                .collect()
        }
    }
    let mut link = Slow(Vec::new());
    let began = Instant::now();
    let told = run(
        &mut link,
        &mut Cache::default(),
        Read::Desk,
        Duration::from_millis(100),
    );
    assert!(!told.1);
    assert_eq!(link.0.len(), 2, "a list, then a state: {:?}", link.0);
    assert!(link.0[0] <= Duration::from_millis(100));
    assert!(
        link.0[1] <= Duration::from_millis(45),
        "the state gets what the list left: {:?}",
        link.0
    );
    assert!(
        began.elapsed() < Duration::from_millis(250),
        "{:?}",
        began.elapsed()
    );
}

/// Fourteen's second P2: a kept list with no desk is not said as current.
#[test]
fn a_kept_list_with_no_desk_is_read_fresh_before_saying_not_found() {
    let mut cache = Cache::default();
    let mut cloud = fake(vec![("desk-1", Fine, desk(Some(101), Some(true)))]);
    cloud
        .rows
        .retain(|row| row.device_type == DeviceType::Hydrate);
    let told = run(&mut cloud, &mut cache, Read::Desk, WITHIN);
    assert_eq!(say(&told), NO_DESK);
    // The desk is paired in the app: the next ask finds it at once.
    cloud.rows.push(row("desk-1", DeviceType::Desk));
    cloud.asked.clear();
    let told = run(&mut cloud, &mut cache, Read::Desk, WITHIN);
    assert_eq!(say(&told), "Your desk is at 101 centimetres.");
    assert_eq!(cloud.asked[0], DeviceAsk::List, "a fresh list first");
}

/// The bus side skips an answer for another `req_id` and ends at its own
/// deadline.
#[test]
fn the_bus_skips_other_req_ids_and_gives_up_at_its_deadline() {
    let root = std::path::PathBuf::from(format!("/tmp/vdt-skip-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let cloud_bus = lamp_bus::Bus::new(&root).unwrap();
    let mut asks = cloud_bus
        .subscribe_typed(DevicesAsk::TOPIC, "cloud", DevicesAsk::TYPE)
        .unwrap();
    let mut out = cloud_bus
        .publisher_declared(DevicesAnswer::TOPIC, &DevicesAnswer::declaration(), "cloud")
        .unwrap();
    let standing_in = std::thread::spawn(move || {
        if let Ok(Some(_)) = asks.recv(Duration::from_secs(5)) {
            // An answer for somebody else's ask, and never the right one.
            out.publish_json(
                lamp_ipc::monotonic_us(),
                &DevicesAnswer {
                    req_id: "somebody-else".into(),
                    answer: DeviceAnswer::None {
                        why: NoAnswer::Timeout,
                    },
                },
            )
            .unwrap();
        }
    });
    let mut bus = Bus(Some(lamp_bus::Bus::new(&root).unwrap()));
    let began = Instant::now();
    let told = run(
        &mut bus,
        &mut Cache::default(),
        Read::Desk,
        Duration::from_millis(600),
    );
    assert!(!told.1);
    assert!(
        began.elapsed() < Duration::from_millis(1500),
        "{:?}",
        began.elapsed()
    );
    standing_in.join().unwrap();
    let _ = std::fs::remove_dir_all(&root);
}
