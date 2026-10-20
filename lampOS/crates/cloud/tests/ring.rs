//! The app's preview against the voice's own ring and its arbitration (the
//! reviewer on #1464): a light the person asks for at the lamp ends a
//! preview at once, and a drag that goes on after it stays given way, so
//! "make your light blue" is never answered with a light the ring does not
//! show.
use lamp_bus::Bus;
use lamp_cloud::answer::Lamp;
use lamp_cloud::lamp::{Places, Real, Ring, Texts, Wifi};
use lamp_msgs::Light;
use lamp_voice::ring_light::RingLight;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Run the ring's loop on the host's clock for a while, as its worker does.
fn settle(ring: &mut RingLight) {
    let until = Instant::now() + Duration::from_millis(300);
    while Instant::now() < until {
        ring.tick(lamp_ipc::monotonic_us());
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
    ring.take_requests(&bus_root, "boot-test").unwrap();
    let mut lamp = Real {
        places: Places {
            actions: root.join("call.sock"),
            proc: root.join("proc"),
            sys: root.join("sys"),
            etc: root.join("etc"),
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
    };
    let mut voice = Bus::new(&bus_root)
        .unwrap()
        .publisher_declared(Light::TOPICS[0], &Light::declaration(), "actions")
        .unwrap();
    let pink = Light::new(true, "pink", 1).unwrap();
    let blue = Light::new(true, "blue", 3).unwrap();
    let purple = Light::new(true, "purple", 2).unwrap();

    // The app's drag shows its colour over the person's light.
    settle(&mut ring);
    lamp.preview_light(&pink).unwrap();
    settle(&mut ring);
    assert_eq!(ring.face(), &pink);
    // "Lamp, make your light blue": shown at once, the preview given way.
    voice.publish_json(lamp_ipc::monotonic_us(), &blue).unwrap();
    settle(&mut ring);
    assert_eq!((ring.now(), ring.face()), (&blue, &blue));
    // The drag goes on: still given way, the person's light shown.
    lamp.preview_light(&purple).unwrap();
    settle(&mut ring);
    assert_eq!(ring.face(), &blue);
    let _ = std::fs::remove_dir_all(root);
}
