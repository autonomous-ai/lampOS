//! The NPU service with a fake NPU, a fake Store and its own clock: the
//! first builder's tests on #7415 (a bad signature and a wrong sha256 each
//! leave the model unloaded and answer `unavailable`; `preparing` until the
//! model is ready; no fetch on a lamp with no pose app), and the rest of
//! its rules.
use base64::{Engine, engine::general_purpose::STANDARD};
use lamp_msgs::HeatLevel;
use lamp_npu::{
    BOARD,
    engine::{Fake, Input, LoadError, Tensor},
    head,
    model::{self, MANIFEST, SIGNATURE, Source, Store},
    serve,
    service::{Conditions, Config, Service},
};
use lamp_update::fetch::{Channel, Fetch};
use ring::{rand::SystemRandom, signature::Ed25519KeyPair, signature::KeyPair};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::Write,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const BASE: &str = "https://store.example/npu";
const HOST: &str = "store.example";

/// The Store: files by URL, and every URL asked.
#[derive(Clone, Default)]
struct Shelf {
    files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    asked: Arc<Mutex<Vec<String>>>,
}

impl Shelf {
    fn put(&self, name: &str, bytes: &[u8]) {
        self.files
            .lock()
            .unwrap()
            .insert(format!("{BASE}/pose/{BOARD}/{name}"), bytes.to_vec());
    }
    fn asked(&self) -> usize {
        self.asked.lock().unwrap().len()
    }
}

impl Channel for Shelf {
    fn fetch(&mut self, url: &str, most: u64, out: &mut dyn Write) -> Result<u64, Fetch> {
        self.asked.lock().unwrap().push(url.to_owned());
        let files = self.files.lock().unwrap();
        let bytes = files.get(url).ok_or(Fetch::Missing)?;
        if bytes.len() as u64 > most {
            return Err(Fetch::Failed("too large".to_owned()));
        }
        out.write_all(bytes).unwrap();
        Ok(bytes.len() as u64)
    }
}

/// A Store key: its private half as `lamp-npu pack` takes it, its public.
fn store_key() -> (String, [u8; 32]) {
    let document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    let pair = Ed25519KeyPair::from_pkcs8(document.as_ref()).unwrap();
    let public: [u8; 32] = pair.public_key().as_ref().try_into().unwrap();
    (STANDARD.encode(document.as_ref()), public)
}

/// MoveNet's four maps, every keypoint at `(y, x)` normalised.
fn movenet_output(y: f32, x: f32, score: f32) -> Vec<Tensor> {
    head::movenet_maps(&[(y, x, score); 17], 48)
}

const SQUARE: Input = Input {
    width: 192,
    height: 192,
    planar: false,
};

fn jpeg(width: u16, height: u16) -> Vec<u8> {
    let mut out = Vec::new();
    let rgb = vec![90u8; usize::from(width) * usize::from(height) * 3];
    jpeg_encoder::Encoder::new(&mut out, 80)
        .encode(&rgb, width, height, jpeg_encoder::ColorType::Rgb)
        .unwrap();
    out
}

/// The fetches queued, run when a test says.
type Jobs = Arc<Mutex<Vec<Box<dyn FnOnce() + Send>>>>;

struct Rig {
    service: Arc<Service>,
    shelf: Shelf,
    fake: Fake,
    jobs: Jobs,
    state: tempfile::TempDir,
    key: String,
}

impl Rig {
    fn new(fake: Fake, source: bool) -> Self {
        let (key, public) = store_key();
        Self::with_keys(fake, source, key, vec![public])
    }

    fn with_keys(fake: Fake, source: bool, key: String, keys: Vec<[u8; 32]>) -> Self {
        let shelf = Shelf::default();
        let state = tempfile::tempdir().unwrap();
        let jobs: Jobs = Arc::default();
        let queued = jobs.clone();
        let channel = shelf.clone();
        let source = source.then(|| Source::new(BASE, HOST).unwrap());
        let service = Service::new(
            Config::new(BOARD, source),
            Box::new(fake.clone()),
            Store::new(state.path().join("models")),
            Box::new(move || Ok(keys.clone())),
            Box::new(move || Ok(Box::new(channel.clone()) as Box<dyn Channel + Send>)),
            Box::new(move |job| queued.lock().unwrap().push(job)),
        );
        Self {
            service,
            shelf,
            fake,
            jobs,
            state,
            key,
        }
    }

    /// The Store serves a model signed with `key`.
    fn shelve(&self, key: &str, file: &[u8], head: &str) -> model::ModelManifest {
        let manifest = model::manifest_for(
            "pose.nb",
            file,
            "movenet-lightning-4",
            "pose",
            head,
            BOARD,
            "Apache-2.0",
        );
        let (bytes, signature) = model::pack(key, &manifest).unwrap();
        self.shelf.put(MANIFEST, &bytes);
        self.shelf.put(SIGNATURE, &signature);
        self.shelf.put("pose.nb", file);
        manifest
    }

    fn run_jobs(&self) -> usize {
        let jobs: Vec<_> = self.jobs.lock().unwrap().drain(..).collect();
        let n = jobs.len();
        for job in jobs {
            job();
        }
        n
    }

    fn pose(&self, client: &str, at: Instant, conditions: Conditions) -> Value {
        let line = json!({"op": "pose", "client": client, "jpeg": STANDARD.encode(jpeg(640, 480))});
        serve::answer(&self.service, &line.to_string(), at, &move || conditions)
    }

    fn kept_files(&self) -> Vec<String> {
        let mut found = Vec::new();
        walk(&self.state.path().join("models"), &mut found);
        found
    }
}

fn walk(dir: &Path, found: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.path().is_dir() {
            walk(&entry.path(), found);
        } else {
            found.push(entry.path().display().to_string());
        }
    }
}

const OPEN: Conditions = Conditions {
    private: false,
    heat: HeatLevel::Ok,
};

fn fake_movenet() -> Fake {
    Fake::new(SQUARE, movenet_output(0.5, 0.25, 0.9))
}

#[test]
fn preparing_until_the_model_is_fetched_then_a_pose_in_the_pictures_pixels() {
    let rig = Rig::new(fake_movenet(), true);
    rig.shelve(&rig.key.clone(), b"a model's bytes", "movenet");
    let t = Instant::now();
    let first = rig.pose("c_1", t, OPEN);
    assert_eq!(first["code"], "preparing", "{first}");
    assert_eq!(first["retry_ms"], 2000);
    // Still fetching: preparing again, and no second fetch started.
    let again = rig.pose("c_1", t + Duration::from_secs(1), OPEN);
    assert_eq!(again["code"], "preparing");
    assert_eq!(rig.run_jobs(), 1);
    assert_eq!(rig.shelf.asked(), 3);
    let answer = rig.pose("c_1", t + Duration::from_secs(2), OPEN);
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(
        (answer["width"].clone(), answer["height"].clone()),
        (json!(640), json!(480))
    );
    assert_eq!(answer["model"], "movenet-lightning-4");
    let pose = &answer["pose"];
    assert_eq!(pose["keypoints"].as_array().unwrap().len(), 17);
    // (0.25, 0.5) of a 192 square that holds the whole 640x480 picture
    // from row 24: x 0.25 x 640 = 160, y (96 - 24) / 0.3 = 240.
    let nose = &pose["keypoints"][0];
    assert!((nose["x"].as_f64().unwrap() - 160.0).abs() < 0.5, "{nose}");
    assert!((nose["y"].as_f64().unwrap() - 240.0).abs() < 0.5, "{nose}");
    assert_eq!(nose["score"], 0.9);
    assert_eq!(pose["score"], 0.9);
    for part in ["decode", "npu", "post", "total"] {
        assert!(answer["ms"][part].is_number(), "{part}");
    }
    let status = serve::answer(&rig.service, r#"{"op":"status"}"#, t, &|| OPEN);
    assert_eq!(status["pose"]["state"], "ready");
    assert_eq!(status["pose"]["licence"], "Apache-2.0");
    assert_eq!(status["pose"]["loaded"], true);
}

#[test]
fn a_bad_signature_leaves_nothing_and_answers_unavailable() {
    let rig = Rig::new(fake_movenet(), true);
    // Signed with a key that is not the Store's.
    let (other, _) = store_key();
    rig.shelve(&other, b"a model's bytes", "movenet");
    let t = Instant::now();
    assert_eq!(rig.pose("c_1", t, OPEN)["code"], "preparing");
    rig.run_jobs();
    let answer = rig.pose("c_1", t + Duration::from_secs(1), OPEN);
    assert_eq!(answer["code"], "unavailable", "{answer}");
    assert!(answer["retry_ms"].as_u64().unwrap() > 0);
    assert!(
        answer["why"].as_str().unwrap().contains("signed with key"),
        "{answer}"
    );
    // The model file itself was never even fetched, and nothing was kept.
    assert_eq!(rig.shelf.asked(), 2);
    assert!(rig.kept_files().is_empty(), "{:?}", rig.kept_files());
    assert_eq!(rig.fake.loads.load(std::sync::atomic::Ordering::SeqCst), 0);
    // After the wait it is fetched again, and fails again, waiting longer.
    let later = t + Duration::from_secs(31);
    assert_eq!(rig.pose("c_1", later, OPEN)["code"], "preparing");
    rig.run_jobs();
    assert_eq!(
        rig.pose("c_1", later + Duration::from_secs(1), OPEN)["code"],
        "unavailable"
    );
}

#[test]
fn a_wrong_sha256_leaves_nothing_and_answers_unavailable() {
    let rig = Rig::new(fake_movenet(), true);
    rig.shelve(&rig.key.clone(), b"a model's bytes", "movenet");
    // The file swapped after it was signed, the same size.
    rig.shelf.put("pose.nb", b"another's bytes");
    let t = Instant::now();
    assert_eq!(rig.pose("c_1", t, OPEN)["code"], "preparing");
    rig.run_jobs();
    let answer = rig.pose("c_1", t + Duration::from_secs(1), OPEN);
    assert_eq!(answer["code"], "unavailable", "{answer}");
    assert!(
        answer["why"].as_str().unwrap().contains("sha256"),
        "{answer}"
    );
    assert!(rig.kept_files().is_empty(), "{:?}", rig.kept_files());
    assert_eq!(rig.fake.loads.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn a_kept_model_changed_on_the_disk_is_removed_and_never_run() {
    let rig = Rig::new(fake_movenet(), true);
    rig.shelve(&rig.key.clone(), b"a model's bytes", "movenet");
    let t = Instant::now();
    rig.pose("c_1", t, OPEN);
    rig.run_jobs();
    let kept = rig.state.path().join("models/pose/pose.nb");
    std::fs::write(&kept, b"tampered bytes!").unwrap();
    let answer = rig.pose("c_1", t + Duration::from_secs(1), OPEN);
    assert_eq!(answer["code"], "unavailable", "{answer}");
    assert!(!kept.exists());
    assert_eq!(rig.fake.runs.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn a_kept_model_is_loaded_on_the_first_ask_and_let_go_when_idle() {
    let rig = Rig::new(fake_movenet(), true);
    rig.shelve(&rig.key.clone(), b"a model's bytes", "movenet");
    let t = Instant::now();
    // A pose-using app installed: fetched before any ask.
    let apps = tempfile::tempdir().unwrap();
    assert!(!serve::apps_tick(&rig.service, apps.path(), t));
    assert_eq!(rig.run_jobs(), 0);
    let coach = apps.path().join("coach/current");
    std::fs::create_dir_all(&coach).unwrap();
    std::fs::write(
        coach.join("lamp-skill.json"),
        r#"{"grants":["camera"],"npu":["pose"]}"#,
    )
    .unwrap();
    assert!(serve::apps_tick(&rig.service, apps.path(), t));
    assert_eq!(rig.run_jobs(), 1);
    // Fetched, not loaded: the NPU is opened at the first ask.
    assert_eq!(rig.fake.loads.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(rig.pose("c_1", t, OPEN)["ok"], true);
    assert_eq!(
        rig.pose("c_1", t + Duration::from_secs(1), OPEN)["ok"],
        true
    );
    assert_eq!(rig.fake.loads.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(rig.service.tick(t + Duration::from_secs(100)));
    assert!(!rig.service.tick(t + Duration::from_secs(182)));
    assert_eq!(
        rig.pose("c_1", t + Duration::from_secs(183), OPEN)["ok"],
        true
    );
    assert_eq!(rig.fake.loads.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[test]
fn no_fetch_on_a_lamp_with_no_pose_app_until_one_is_asked() {
    let rig = Rig::new(fake_movenet(), true);
    rig.shelve(&rig.key.clone(), b"a model's bytes", "movenet");
    let apps = tempfile::tempdir().unwrap();
    let clock = apps.path().join("clock/current");
    std::fs::create_dir_all(&clock).unwrap();
    std::fs::write(
        clock.join("lamp-skill.json"),
        r#"{"grants":["camera","light"]}"#,
    )
    .unwrap();
    let t = Instant::now();
    for minute in 0..5 {
        serve::apps_tick(
            &rig.service,
            apps.path(),
            t + Duration::from_secs(60 * minute),
        );
    }
    assert_eq!(rig.run_jobs(), 0);
    assert_eq!(rig.shelf.asked(), 0);
    let status = serve::answer(&rig.service, r#"{"op":"status"}"#, t, &|| OPEN);
    assert_eq!(status["pose"]["state"], "absent");
}

#[test]
fn private_rate_and_heat() {
    let rig = Rig::new(fake_movenet(), true);
    rig.shelve(&rig.key.clone(), b"a model's bytes", "movenet");
    let t = Instant::now();
    rig.pose("c_1", t, OPEN);
    rig.run_jobs();
    let private = Conditions {
        private: true,
        heat: HeatLevel::Ok,
    };
    let at = t + Duration::from_secs(10);
    assert_eq!(rig.pose("c_1", at, private)["code"], "private");
    assert_eq!(rig.pose("c_1", at, OPEN)["ok"], true);
    // 15 a second for one client.
    let soon = rig.pose("c_1", at + Duration::from_millis(30), OPEN);
    assert_eq!(soon["code"], "rate", "{soon}");
    assert_eq!(soon["retry_ms"], 36);
    // Another client: the NPU's own share, 20 a second for all.
    assert_eq!(
        rig.pose("c_2", at + Duration::from_millis(30), OPEN)["code"],
        "busy"
    );
    assert_eq!(
        rig.pose("c_2", at + Duration::from_millis(60), OPEN)["ok"],
        true
    );
    // Hot: one a second each.
    let hot = Conditions {
        private: false,
        heat: HeatLevel::Hot,
    };
    let later = at + Duration::from_secs(5);
    assert_eq!(rig.pose("c_1", later, hot)["ok"], true);
    assert_eq!(
        rig.pose("c_1", later + Duration::from_millis(600), hot)["code"],
        "rate"
    );
    assert_eq!(
        rig.pose("c_1", later + Duration::from_millis(1000), hot)["ok"],
        true
    );
    // A lamp gone private during the run: no pose.
    let flips = Arc::new(Mutex::new(0));
    let counted = flips.clone();
    let turning = move || {
        let mut n = counted.lock().unwrap();
        *n += 1;
        Conditions {
            private: *n > 1,
            heat: HeatLevel::Ok,
        }
    };
    let line = json!({"op": "pose", "client": "c_3", "jpeg": STANDARD.encode(jpeg(64, 48))});
    let answer = serve::answer(
        &rig.service,
        &line.to_string(),
        later + Duration::from_secs(9),
        &turning,
    );
    assert_eq!(answer["code"], "private", "{answer}");
}

#[test]
fn no_npu_no_source_and_bad_asks() {
    let mut fake = fake_movenet();
    fake.refuse = Some(LoadError::Unsupported("this lamp has no NPU".to_owned()));
    let rig = Rig::new(fake, true);
    rig.shelve(&rig.key.clone(), b"a model's bytes", "movenet");
    let t = Instant::now();
    rig.pose("c_1", t, OPEN);
    rig.run_jobs();
    let answer = rig.pose("c_1", t + Duration::from_secs(1), OPEN);
    assert_eq!(answer["code"], "unavailable");
    assert_eq!(answer["why"], "this lamp has no NPU");
    assert!(answer.get("retry_ms").is_none());
    // No source set: unavailable, and nothing asked of anyone.
    let none = Rig::new(fake_movenet(), false);
    let answer = none.pose("c_1", t, OPEN);
    assert_eq!(answer["code"], "unavailable", "{answer}");
    assert_eq!(none.run_jobs(), 0);
    // Not a JPEG, no client, another op.
    for line in [
        json!({"op": "pose", "client": "c_1", "jpeg": STANDARD.encode(b"not a jpeg")}),
        json!({"op": "pose", "jpeg": STANDARD.encode(jpeg(8, 8))}),
        json!({"op": "pose", "client": "c_1", "jpeg": "%%%"}),
        json!({"op": "faces"}),
    ] {
        let answer = serve::answer(&none.service, &line.to_string(), t, &|| OPEN);
        assert_eq!(answer["ok"], false, "{line}");
    }
}

#[test]
fn a_model_for_another_head_or_board_is_refused_before_its_file() {
    let rig = Rig::new(fake_movenet(), true);
    rig.shelve(&rig.key.clone(), b"a model's bytes", "rtmpose");
    let t = Instant::now();
    rig.pose("c_1", t, OPEN);
    rig.run_jobs();
    let answer = rig.pose("c_1", t + Duration::from_secs(1), OPEN);
    assert!(answer["why"].as_str().unwrap().contains("head"), "{answer}");
    assert_eq!(rig.shelf.asked(), 2);
}

#[test]
fn models_come_only_over_https_from_the_stores_host() {
    assert!(Source::new("https://store.example/npu/", HOST).is_ok());
    assert_eq!(
        Source::new(BASE, HOST)
            .unwrap()
            .url("pose", BOARD, MANIFEST),
        format!("https://store.example/npu/pose/{BOARD}/lamp-model.json")
    );
    for base in [
        "http://store.example/npu",
        "file:///tmp/npu",
        "https://other.example/npu",
        "https://user@store.example/npu",
        "https://store.example:8443/npu",
    ] {
        assert!(Source::new(base, HOST).is_err(), "{base}");
    }
}

#[test]
fn a_yolov8_model_answers_the_same_shape() {
    // A 64x32 frame: the three strides' maps, all quiet but one anchor.
    let input = Input {
        width: 64,
        height: 32,
        planar: false,
    };
    let mut outputs = Vec::new();
    for stride in [8usize, 16, 32] {
        let anchors = (64 / stride) * (32 / stride);
        let mut class = vec![-9f32; anchors];
        if stride == 8 {
            class[0] = 4.0;
        }
        outputs.push(Tensor::from_f32(
            vec![1, 64, anchors],
            &vec![0.0; 64 * anchors],
        ));
        outputs.push(Tensor::from_f32(vec![1, 1, anchors], &class));
        outputs.push(Tensor::from_f32(
            vec![1, 51, anchors],
            &vec![0.0; 51 * anchors],
        ));
    }
    let rig = Rig::new(Fake::new(input, outputs), true);
    rig.shelve(&rig.key.clone(), b"a model's bytes", "yolov8-pose");
    let t = Instant::now();
    rig.pose("c_1", t, OPEN);
    rig.run_jobs();
    let answer = rig.pose("c_1", t + Duration::from_secs(1), OPEN);
    assert_eq!(answer["ok"], true, "{answer}");
    let pose = &answer["pose"];
    assert_eq!(pose["keypoints"].as_array().unwrap().len(), 17);
    assert_eq!(pose["keypoints"][0].as_object().unwrap().len(), 3);
    assert!(pose["score"].as_f64().unwrap() > 0.98);
}

/// A person standing in the middle of the frame: shoulders at 0.4, hips
/// at 0.6, the rest about them, all seen.
fn standing() -> Fake {
    let mut points = [(0.5, 0.5, 0.9); 17];
    points[5] = (0.4, 0.4, 0.9);
    points[6] = (0.4, 0.6, 0.9);
    points[11] = (0.6, 0.45, 0.9);
    points[12] = (0.6, 0.55, 0.9);
    Fake::new(SQUARE, head::movenet_maps(&points, 48))
}

fn left_shoulder(answer: &Value) -> (f64, f64) {
    let k = &answer["pose"]["keypoints"][5];
    (k["x"].as_f64().unwrap(), k["y"].as_f64().unwrap())
}

#[test]
fn a_clients_next_frame_is_cropped_around_its_person_until_it_goes_stale() {
    let rig = Rig::new(standing(), true);
    rig.shelve(&rig.key.clone(), b"a model's bytes", "movenet");
    let t = Instant::now();
    rig.pose("c_1", t, OPEN);
    rig.run_jobs();
    // The first frame whole: (0.4, 0.4) of the square is x 256, y 176.
    let first = rig.pose("c_1", t + Duration::from_secs(1), OPEN);
    let whole = left_shoulder(&first);
    assert!(
        (whole.0 - 256.0).abs() < 0.5 && (whole.1 - 176.0).abs() < 0.5,
        "{first}"
    );
    // The next one is cropped around the person found: the fake NPU sees
    // the same, so it maps back inside the crop, nearer the hips.
    let second = rig.pose("c_1", t + Duration::from_millis(1100), OPEN);
    let cropped = left_shoulder(&second);
    assert!((cropped.0 - whole.0).abs() > 5.0, "{whole:?} {cropped:?}");
    // Another client's first frame is whole.
    let other = rig.pose("c_2", t + Duration::from_millis(1200), OPEN);
    assert_eq!(left_shoulder(&other), whole);
    // Stale after 1.5 s: whole again.
    let late = rig.pose("c_1", t + Duration::from_secs(3), OPEN);
    assert_eq!(left_shoulder(&late), whole);
    // A private lamp forgets every crop.
    rig.pose("c_1", t + Duration::from_millis(3100), OPEN);
    let private = Conditions {
        private: true,
        heat: HeatLevel::Ok,
    };
    assert_eq!(
        rig.pose("c_1", t + Duration::from_millis(3200), private)["code"],
        "private"
    );
    let after = rig.pose("c_1", t + Duration::from_millis(3300), OPEN);
    assert_eq!(left_shoulder(&after), whole);
}
