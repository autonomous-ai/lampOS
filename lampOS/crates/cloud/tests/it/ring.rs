//! The app's preview against the voice's own ring and its arbitration (the
//! reviewer on #1464): a light the person asks for at the lamp ends a
//! preview at once, and a drag that goes on after it stays given way, so
//! "make your light blue" is never answered with a light the ring does not
//! show.
use lamp_bus::Bus;
use lamp_cloud::answer::Lamp;
use lamp_cloud::lamp::{Places, Real, Ring, Texts, Wifi};
use lamp_msgs::{Light, LightOutranked};
use lamp_voice::ring_light::RingLight;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Run the ring's loop on the host's clock, as its worker does, until
/// `done`: not a fixed 300 ms (#1806).
fn settle(ring: &mut RingLight, mut done: impl FnMut(&RingLight) -> bool) {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        ring.tick(lamp_ipc::monotonic_us());
        if done(ring) {
            return;
        }
        assert!(Instant::now() < until, "not so in 5 s");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_light_asked_at_the_lamp_ends_the_apps_preview_and_a_drag_after_it_stays_given_way() {
    let root = PathBuf::from(format!("/tmp/lcr-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let bus_root = root.join("bus");
    let random = root.join("proc/sys/kernel/random");
    std::fs::create_dir_all(&bus_root).unwrap();
    std::fs::create_dir_all(&random).unwrap();
    std::fs::write(random.join("boot_id"), "boot-test\n").unwrap();
    let (mut ring, _) = RingLight::on(&bus_root).unwrap();
    // What the ring says of the app's light it shows nothing of: listened
    // to before its publisher is made, so none is missed.
    let mut outranked = Bus::new(&bus_root)
        .unwrap()
        .subscribe_typed(LightOutranked::TOPIC, "test", LightOutranked::TYPE)
        .unwrap();
    let mut given_way = 0;
    let mut told_given_way = move || {
        while let Some(message) = outranked.recv(Duration::ZERO).unwrap() {
            let told: LightOutranked = message.parse().unwrap();
            if (told.source.as_str(), told.by.as_str()) == ("cloud", LightOutranked::PERSON) {
                given_way += 1;
            }
        }
        given_way
    };
    ring.take_requests(&bus_root, "boot-test").unwrap();
    let mut lamp = Real {
        places: Places {
            actions: root.join("call.sock"),
            proc: root.join("proc"),
            sys: root.join("sys"),
            etc: root.join("etc"),
            run: root.join("run"),
        },
        bus: Some(Bus::new(&bus_root).unwrap()),
        device_id: "d-42".into(),
        build: String::new(),
        wifi: Wifi::default(),
        ring: Ring::default(),
        texts: Texts::default(),
        reports: None,
        skills: None,
        changes: None,
        photos: None,
        faces: None,
        schedule: None,
        update_page: false,
        restart: None,
        guard: None,
        make_skill: None,
        shutdown: None,
    };
    let mut voice = Bus::new(&bus_root)
        .unwrap()
        .publisher_declared(Light::TOPICS[0], &Light::declaration(), "actions")
        .unwrap();
    let pink = Light::new(true, "pink", 1).unwrap();
    let blue = Light::new(true, "blue", 3).unwrap();
    let purple = Light::new(true, "purple", 2).unwrap();

    // The app's drag shows its colour over the person's light.
    settle(&mut ring, |_| true);
    lamp.preview_light(&pink).unwrap();
    settle(&mut ring, |ring| ring.face() == &pink);
    assert_eq!(ring.face(), &pink);
    // "Lamp, make your light blue": shown at once, the preview given way.
    voice.publish_json(lamp_ipc::monotonic_us(), &blue).unwrap();
    settle(&mut ring, |ring| {
        (ring.now(), ring.face()) == (&blue, &blue)
    });
    assert_eq!((ring.now(), ring.face()), (&blue, &blue));
    // The drag goes on: still given way, the person's light shown. The
    // ring tells a request it shows nothing of once, and again for each
    // new one from its source while it stays given way, so the second
    // telling is the purple taken.
    lamp.preview_light(&purple).unwrap();
    settle(&mut ring, |_| told_given_way() >= 2);
    assert_eq!(ring.face(), &blue);
    let _ = std::fs::remove_dir_all(root);
}
