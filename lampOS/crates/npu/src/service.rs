//! The service's rules, apart from its socket: who may ask how often, what
//! the board's heat and the lamp's privacy allow, when the model is fetched,
//! loaded and let go, and what each `pose` is answered. Its clock is passed
//! in, and its NPU, its Store and its threads are given to it, so the tests
//! play all of them.
use crate::{
    engine::{Backend, LoadError, Net},
    head::{self, Found, PoseHead},
    image::{self, Region},
    model::{Kept, ModelManifest, Source, Store},
};
use lamp_msgs::HeatLevel;
use lamp_update::fetch::Channel;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// The op this build answers.
pub const POSE: &str = "pose";
/// The largest JPEG taken, in bytes.
pub const MOST_JPEG_BYTES: usize = 512 * 1024;
/// A person scored under this is no person: the answer's `pose` is null.
pub const LEAST_SCORE: f32 = 0.2;
/// A client's crop around the person it last showed is kept this long:
/// a frame later than that is looked at whole.
pub const TRACK_FOR: Duration = Duration::from_millis(1500);
/// How long `preparing` tells a client to wait before it asks again.
pub const PREPARING_RETRY: Duration = Duration::from_secs(2);

/// What the service is set to.
#[derive(Clone, Debug)]
pub struct Config {
    /// The NPU's target, as a model's manifest names it.
    pub board: String,
    /// Where models come from; none, nothing is fetched.
    pub source: Option<Source>,
    /// A loaded model unused this long is let go.
    pub idle: Duration,
    /// One client's shortest time between two asks, at ok, warm and hot.
    pub each: [Duration; 3],
    /// All clients' shortest time between two runs, at ok, warm and hot.
    pub all: [Duration; 3],
    /// The first wait after a failed fetch, doubled each time up to `most_retry`.
    pub first_retry: Duration,
    pub most_retry: Duration,
}

impl Config {
    pub fn new(board: &str, source: Option<Source>) -> Self {
        Self {
            board: board.to_owned(),
            source,
            idle: Duration::from_secs(180),
            // 15, 5 and 1 a second for one client.
            each: [
                Duration::from_millis(66),
                Duration::from_millis(200),
                Duration::from_secs(1),
            ],
            // 20, 6 and 2 a second for all of them: at about 20 ms a run
            // the NPU stays more than half free for the eyes.
            all: [
                Duration::from_millis(50),
                Duration::from_millis(166),
                Duration::from_millis(500),
            ],
            first_retry: Duration::from_secs(30),
            most_retry: Duration::from_secs(30 * 60),
        }
    }
}

/// What the lamp is like at an ask.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Conditions {
    pub private: bool,
    pub heat: HeatLevel,
}

fn heat_at(heat: HeatLevel) -> usize {
    match heat {
        HeatLevel::Ok => 0,
        HeatLevel::Warm => 1,
        HeatLevel::Hot => 2,
    }
}

/// Why an ask was not answered with a pose: its wire `code`, its words and
/// when to ask again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused {
    pub code: &'static str,
    pub why: String,
    pub retry_ms: Option<u64>,
}

impl Refused {
    fn new(code: &'static str, why: impl Into<String>, retry: Option<Duration>) -> Self {
        Self {
            code,
            why: why.into(),
            retry_ms: retry.map(|wait| wait.as_millis().max(1) as u64),
        }
    }

    pub fn json(&self) -> Value {
        let mut answer = json!({"ok": false, "code": self.code, "why": self.why});
        if let Some(retry_ms) = self.retry_ms {
            answer["retry_ms"] = json!(retry_ms);
        }
        answer
    }
}

/// What the lamp has of the model.
#[derive(Clone, Debug)]
enum Model {
    /// The disk not looked at yet.
    Unknown,
    /// None kept, none being fetched.
    Absent,
    Preparing,
    Kept(Kept),
    /// A fetch or a check failed; fetched again from `retry_at`.
    Unavailable {
        why: String,
        retry_at: Instant,
        wait: Duration,
    },
    /// No NPU here: nothing to retry.
    Unsupported(String),
}

/// Where a client's person was last seen, for the next frame's crop.
struct Track {
    region: Region,
    width: usize,
    height: usize,
    at: Instant,
}

struct Loaded {
    net: Box<dyn Net>,
    head: Box<dyn PoseHead>,
    manifest: ModelManifest,
    used: Instant,
}

/// The Store's keys, read again for every fetch and load.
pub type Keys = Box<dyn Fn() -> Result<Vec<[u8; 32]>, String> + Send + Sync>;
/// A channel to the Store, made for each fetch.
pub type Channels = Box<dyn Fn() -> Result<Box<dyn Channel + Send>, String> + Send + Sync>;
/// Runs a fetch off the asker's thread.
pub type Spawn = Box<dyn Fn(Box<dyn FnOnce() + Send>) + Send + Sync>;

pub struct Service {
    config: Config,
    backend: Box<dyn Backend>,
    store: Store,
    keys: Keys,
    channels: Channels,
    spawn: Spawn,
    model: Mutex<Model>,
    /// The one net: a run holds this lock, so runs go one at a time.
    net: Mutex<Option<Loaded>>,
    asked: Mutex<(HashMap<String, Instant>, Option<Instant>)>,
    /// Each client's crop: forgotten after [`TRACK_FOR`], when the lamp is
    /// private and when the model is let go. Taken after `net`.
    tracks: Mutex<HashMap<String, Track>>,
}

impl Service {
    pub fn new(
        config: Config,
        backend: Box<dyn Backend>,
        store: Store,
        keys: Keys,
        channels: Channels,
        spawn: Spawn,
    ) -> Arc<Self> {
        Arc::new(Self {
            config,
            backend,
            store,
            keys,
            channels,
            spawn,
            model: Mutex::new(Model::Unknown),
            net: Mutex::new(None),
            asked: Mutex::new((HashMap::new(), None)),
            tracks: Mutex::new(HashMap::new()),
        })
    }

    /// Whether a client may ask now, at this heat: `Err` with the refusal.
    fn admit(&self, client: &str, heat: HeatLevel, now: Instant) -> Result<(), Refused> {
        let at = heat_at(heat);
        let mut asked = self.asked.lock().expect("asked");
        let (each, all) = (self.config.each[at], self.config.all[at]);
        if let Some(last) = asked.0.get(client)
            && now < *last + each
        {
            return Err(Refused::new(
                "rate",
                format!("at most one pose each {} ms now", each.as_millis()),
                Some(*last + each - now),
            ));
        }
        if let Some(last) = asked.1
            && now < last + all
        {
            return Err(Refused::new(
                "busy",
                "the NPU is busy with other asks",
                Some(last + all - now),
            ));
        }
        // A bounded table: clients are a few, and one not heard from for
        // the longest wait is forgotten.
        let longest = self.config.each[2];
        asked
            .0
            .retain(|_, last| now.saturating_duration_since(*last) < longest);
        asked.0.insert(client.to_owned(), now);
        asked.1 = Some(now);
        Ok(())
    }

    /// Starts a fetch if the model is not kept and none is going: the
    /// first ask, or a pose-using app installed. What the model is now.
    pub fn prepare(self: &Arc<Self>, now: Instant) -> Result<Kept, Refused> {
        let mut model = self.model.lock().expect("model");
        if matches!(*model, Model::Unknown) {
            *model = match (self.keys)() {
                Err(why) => Model::Unavailable {
                    why,
                    retry_at: now + self.config.first_retry,
                    wait: self.config.first_retry,
                },
                Ok(keys) => match self.store.load(&keys, POSE, &self.config.board) {
                    Ok(Some(kept)) => Model::Kept(kept),
                    Ok(None) => Model::Absent,
                    Err(why) => Model::Unavailable {
                        why,
                        retry_at: now,
                        wait: self.config.first_retry,
                    },
                },
            };
        }
        match &*model {
            Model::Kept(kept) => return Ok(kept.clone()),
            Model::Preparing => {
                return Err(Refused::new(
                    "preparing",
                    "the pose model is being fetched",
                    Some(PREPARING_RETRY),
                ));
            }
            Model::Unsupported(why) => return Err(Refused::new("unavailable", why, None)),
            Model::Unavailable { why, retry_at, .. } if now < *retry_at => {
                return Err(Refused::new(
                    "unavailable",
                    format!("the pose model is unavailable: {why}"),
                    Some(*retry_at - now),
                ));
            }
            Model::Unknown | Model::Absent | Model::Unavailable { .. } => {}
        }
        let Some(source) = self.config.source.clone() else {
            return Err(Refused::new(
                "unavailable",
                "no model source is set on this lamp",
                None,
            ));
        };
        let wait = match &*model {
            Model::Unavailable { wait, .. } => *wait,
            _ => self.config.first_retry,
        };
        *model = Model::Preparing;
        drop(model);
        let service = self.clone();
        (self.spawn)(Box::new(move || service.fetch(&source, wait)));
        Err(Refused::new(
            "preparing",
            "the pose model is being fetched",
            Some(PREPARING_RETRY),
        ))
    }

    /// One fetch, on its own thread; `wait` is what a failure waits.
    fn fetch(&self, source: &Source, wait: Duration) {
        let fetched = (|| {
            let keys = (self.keys)()?;
            let mut channel = (self.channels)()?;
            self.store
                .fetch(channel.as_mut(), source, &keys, POSE, &self.config.board)
        })();
        let mut model = self.model.lock().expect("model");
        *model = match fetched {
            Ok(kept) => {
                eprintln!(
                    "[lamp-npu] model {} ({}) kept",
                    kept.manifest.id, kept.manifest.licence
                );
                Model::Kept(kept)
            }
            Err(why) => {
                eprintln!(
                    "[lamp-npu] fetch failed, again in {} s: {why}",
                    wait.as_secs()
                );
                Model::Unavailable {
                    why,
                    retry_at: Instant::now() + wait,
                    wait: (wait * 2).min(self.config.most_retry),
                }
            }
        };
    }

    /// A model that failed at its load: let go and, but for no NPU at
    /// all, fetched again after a wait.
    fn failed(&self, now: Instant, error: LoadError) -> Refused {
        let mut model = self.model.lock().expect("model");
        match error {
            LoadError::Unsupported(why) => {
                *model = Model::Unsupported(why.clone());
                Refused::new("unavailable", why, None)
            }
            LoadError::Failed(why) => {
                self.store.remove(POSE);
                *model = Model::Unavailable {
                    why: why.clone(),
                    retry_at: now + self.config.first_retry,
                    wait: self.config.first_retry * 2,
                };
                Refused::new(
                    "unavailable",
                    format!("the pose model is unavailable: {why}"),
                    Some(self.config.first_retry),
                )
            }
        }
    }

    /// The kept model failed its check at its load (and was removed):
    /// unavailable, and fetched again after the first wait.
    fn lost(&self, now: Instant, why: String) -> Refused {
        eprintln!("[lamp-npu] {why}");
        let mut model = self.model.lock().expect("model");
        *model = Model::Unavailable {
            why: why.clone(),
            retry_at: now + self.config.first_retry,
            wait: self.config.first_retry * 2,
        };
        Refused::new(
            "unavailable",
            format!("the pose model is unavailable: {why}"),
            Some(self.config.first_retry),
        )
    }

    /// `client`'s `pose` of `jpeg`. `conditions` is read at the ask and
    /// again once the run is done: a lamp gone private meanwhile gets no
    /// answer but `private`.
    pub fn pose(
        self: &Arc<Self>,
        client: &str,
        jpeg: &[u8],
        now: Instant,
        conditions: &dyn Fn() -> Conditions,
    ) -> Result<Value, Refused> {
        let started = Instant::now();
        let first = conditions();
        if first.private {
            return Err(self.private());
        }
        if jpeg.is_empty() || jpeg.len() > MOST_JPEG_BYTES {
            return Err(Refused::new(
                "bad_request",
                format!("jpeg: 1 to {MOST_JPEG_BYTES} bytes"),
                None,
            ));
        }
        self.admit(client, first.heat, now)?;
        let kept = self.prepare(now)?;
        let mut net = self.net.lock().expect("net");
        if net.is_none() {
            // Checked again as it is loaded: never a file changed since.
            let keys = (self.keys)().map_err(|why| Refused::new("unavailable", why, None))?;
            let checked = match self.store.load(&keys, POSE, &self.config.board) {
                Ok(Some(checked)) if checked == kept => checked,
                Ok(Some(_)) | Ok(None) => {
                    return Err(self.lost(now, "the kept model is gone".to_owned()));
                }
                // Removed by the check: never run, fetched again later.
                Err(why) => return Err(self.lost(now, why)),
            };
            let Some(head) = head::named(&checked.manifest.head) else {
                return Err(self.failed(
                    now,
                    LoadError::Failed(format!("head {:?}", checked.manifest.head)),
                ));
            };
            match self.backend.load(&checked.path) {
                Ok(loaded) => {
                    eprintln!("[lamp-npu] model {} loaded", checked.manifest.id);
                    *net = Some(Loaded {
                        net: loaded,
                        head,
                        manifest: checked.manifest,
                        used: now,
                    });
                }
                Err(error) => {
                    drop(net);
                    return Err(self.failed(now, error));
                }
            }
        }
        let loaded = net.as_mut().expect("loaded above");
        loaded.used = now;
        let input = loaded.net.input();
        let decode_at = Instant::now();
        let picture = image::decode(jpeg).map_err(|why| Refused::new("bad_request", why, None))?;
        let (width, height) = (picture.width, picture.height);
        let region = self
            .tracked(client, width, height, now)
            .unwrap_or_else(|| Region::whole(width, height, input));
        let frame = image::crop(&picture, region, input);
        let decoded_at = Instant::now();
        let (outputs, run) = loaded
            .net
            .run(&frame)
            .map_err(|why| Refused::new("unavailable", why, Some(PREPARING_RETRY)))?;
        let ran_at = Instant::now();
        let mut found = loaded
            .head
            .decode(&outputs, input)
            .map_err(|why| Refused::new("unavailable", why, None))?;
        // In the picture's pixels, as they are: the crop is made of them.
        for k in &mut found.keypoints {
            (k.x, k.y) = region.back(input, width, height, k.x, k.y);
        }
        let next = loaded.head.next(&found, width, height);
        self.track(client, next, width, height, now);
        let model_id = loaded.manifest.id.clone();
        drop(net);
        if conditions().private {
            return Err(self.private());
        }
        let ms = |from: Instant, to: Instant| round2((to - from).as_secs_f64() * 1e3);
        let pose = (found.score >= LEAST_SCORE).then(|| answer(&found, width, height));
        let done = Instant::now();
        Ok(json!({
            "ok": true,
            "pose": pose,
            "width": width,
            "height": height,
            "model": model_id,
            "ms": {
                "decode": ms(decode_at, decoded_at),
                "npu": round2(run.npu + run.input),
                "post": ms(ran_at, done),
                "total": ms(started, done),
            },
        }))
    }

    /// Lets a model unused for the idle time go. Whether one is loaded.
    pub fn tick(&self, now: Instant) -> bool {
        let mut net = self.net.lock().expect("net");
        if net
            .as_ref()
            .is_some_and(|loaded| now.saturating_duration_since(loaded.used) >= self.config.idle)
        {
            eprintln!(
                "[lamp-npu] model let go after {} s idle",
                self.config.idle.as_secs()
            );
            *net = None;
            self.tracks.lock().expect("tracks").clear();
        }
        net.is_some()
    }

    /// `client`'s crop for a picture this size, if still fresh.
    fn tracked(&self, client: &str, width: usize, height: usize, now: Instant) -> Option<Region> {
        let tracks = self.tracks.lock().expect("tracks");
        let track = tracks.get(client)?;
        (track.width == width
            && track.height == height
            && now.saturating_duration_since(track.at) < TRACK_FOR)
            .then_some(track.region)
    }

    /// Keeps `client`'s next crop, or forgets it for the whole picture.
    fn track(&self, client: &str, next: Option<Region>, width: usize, height: usize, now: Instant) {
        let mut tracks = self.tracks.lock().expect("tracks");
        tracks.retain(|_, track| now.saturating_duration_since(track.at) < TRACK_FOR);
        match next {
            Some(region) => {
                tracks.insert(
                    client.to_owned(),
                    Track {
                        region,
                        width,
                        height,
                        at: now,
                    },
                );
            }
            None => {
                tracks.remove(client);
            }
        }
    }

    /// Refuses as private, and forgets where anyone was.
    fn private(&self) -> Refused {
        self.tracks.lock().expect("tracks").clear();
        Refused::new("private", "the lamp is private", None)
    }

    /// What the service has, for `status`.
    pub fn status(&self) -> Value {
        let loaded = self.net.lock().expect("net").is_some();
        let model = self.model.lock().expect("model").clone();
        let (state, kept, why) = match model {
            Model::Unknown | Model::Absent => ("absent", None, None),
            Model::Preparing => ("preparing", None, None),
            Model::Kept(kept) => ("ready", Some(kept.manifest), None),
            Model::Unavailable { why, .. } | Model::Unsupported(why) => {
                ("unavailable", None, Some(why))
            }
        };
        let mut status = json!({
            "ok": true,
            "ops": [POSE],
            "board": self.config.board,
            "pose": {"state": state, "loaded": loaded},
        });
        if let Some(manifest) = kept {
            status["pose"]["model"] = json!(manifest.id);
            status["pose"]["head"] = json!(manifest.head);
            status["pose"]["licence"] = json!(manifest.licence);
        }
        if let Some(why) = why {
            status["pose"]["why"] = json!(why);
        }
        status
    }
}

/// The answer's `pose`: keypoints kept on the picture.
fn answer(found: &Found, width: usize, height: usize) -> Value {
    let keypoints: Vec<Value> = found
        .keypoints
        .iter()
        .map(|k| {
            let x = k.x.clamp(0.0, width as f32);
            let y = k.y.clamp(0.0, height as f32);
            json!({"x": round2(f64::from(x)), "y": round2(f64::from(y)), "score": round3(k.score)})
        })
        .collect();
    json!({"score": round3(found.score), "keypoints": keypoints})
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn round3(value: f32) -> f64 {
    (f64::from(value) * 1000.0).round() / 1000.0
}
