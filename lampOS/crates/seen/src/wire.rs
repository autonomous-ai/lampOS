//! V1's WebSocket messages to the perception service, as types only (#6543
//! phase 1; no socket). From `hal/drivers/sensing/perceptions/processors/`
//! on `main`: `motion.py` (activity, task `action`, at
//! `/ws/hal/api/dl/action-analysis/ws`) and `pose.py` (task `pose`, at
//! `/ws/hal/api/dl/pose-estimation/ws`).
//!
//! A session opens with the key exchange ([`crate::crypto`]); then every
//! message each way is encrypted, and these are their plaintexts:
//!
//! | Lamp sends | Server answers |
//! |---|---|
//! | activity only: `{"type":"config","task":"action","whitelist":[..]\|null,"threshold":0.3,"person_detection_enabled":true,"person_min_area_ratio":0.25}` | any JSON (V1 reads none of it) |
//! | `{"type":"heartbeat","task":T}`, at most once a minute | `{"status":"ok"}` |
//! | `{"type":"frame","task":T,"frame_b64":JPEG}` | activity: `{"detected_classes":[{"class_name","conf"}..]}`; pose: `{"pose_2d","pose_3d","ergo"}` or `{"error"}` |
//!
//! The server buffers activity's frames into its clips; the lamp sends one
//! frame a message. V1 wrote these with `json.dumps`, which puts a space
//! after each `,` and `:`; the server parses JSON, so serde's compact form
//! is the same message.
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Which service a session is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Task {
    /// Activity recognition (V1's `motion.py`).
    Action,
    /// Pose and its RULA score (V1's `pose.py`).
    Pose,
}

/// V1's activity defaults (`hal/config.py`): the confidence a class needs,
/// and whether the server crops to a person who fills at least this share
/// of the picture.
pub const ACTION_THRESHOLD: f64 = 0.3;
pub const PERSON_DETECTION: bool = true;
pub const PERSON_MIN_AREA_RATIO: f64 = 0.25;
/// V1's heartbeat interval (`DL_HEARTBEAT_INTERVAL_S`).
pub const HEARTBEAT_EVERY_S: u64 = 60;
/// V1's wait for any answer before the session is dropped
/// (`DL_WS_RECV_TIMEOUT_S`). V1's heartbeat waited with no bound
/// (`motion.py:208`); a port bounds it too.
pub const ANSWER_WITHIN_S: u64 = 15;

/// V1's activity whitelist, line for line and in its order
/// (`processors/resources/white_list.txt` on `main`), plus one class beyond
/// V1's list: "eating chips", after "eating carrots", the nearest Kinetics
/// class to a snack, added after the 13 Nov lamp run gave no eating label
/// for one. The list reaches only this session: the server
/// (`core/perception/action/session.py`, `_post_config_update`) builds each
/// session's class mask from the whitelist that session sent, and a V1
/// lamp sends its own at its session's start, so V1 lamps keep their 30.
/// The config object itself is shared (`perception.py`, `create_session`)
/// and the threshold is read from it live, so the other values stay V1's.
pub const WHITELIST: [&str; 31] = [
    "drinking",
    "drinking beer",
    "drinking shots",
    "tasting beer",
    "celebrating",
    "clapping",
    "applauding",
    "stretching arm",
    "stretching leg",
    "tasting food",
    "dining",
    "eating burger",
    "eating cake",
    "eating carrots",
    "eating chips",
    "eating doughnuts",
    "eating hotdog",
    "eating ice cream",
    "eating spaghetti",
    "eating watermelon",
    "using computer",
    "writing",
    "texting",
    "reading book",
    "reading newspaper",
    "drawing",
    "playing controller",
    "laughing",
    "crying",
    "yawning",
    "singing",
];

/// Activity's config as V1 sends it: the whitelist (V1's and one more
/// class) and V1's defaults.
pub fn v1_action_config() -> Sent {
    Sent::action_config(Some(
        WHITELIST.iter().map(|label| (*label).to_owned()).collect(),
    ))
}

/// What the lamp sends, after the key exchange, before encryption.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Sent {
    /// Activity only, once a session.
    Config {
        task: Task,
        whitelist: Option<Vec<String>>,
        threshold: f64,
        person_detection_enabled: bool,
        person_min_area_ratio: f64,
    },
    Heartbeat {
        task: Task,
    },
    Frame {
        task: Task,
        /// One JPEG, standard Base64.
        frame_b64: String,
    },
}

impl Sent {
    /// Activity's config with V1's defaults.
    pub fn action_config(whitelist: Option<Vec<String>>) -> Self {
        Sent::Config {
            task: Task::Action,
            whitelist,
            threshold: ACTION_THRESHOLD,
            person_detection_enabled: PERSON_DETECTION,
            person_min_area_ratio: PERSON_MIN_AREA_RATIO,
        }
    }
}

/// A heartbeat's answer: `ok` is all V1 checks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub status: String,
}

impl Status {
    pub fn ok(&self) -> bool {
        self.status == "ok"
    }
}

/// One activity class the server found.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Detected {
    pub class_name: String,
    pub conf: f64,
}

/// Activity's answer to a frame. Absent classes are none.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ActionAnswer {
    #[serde(default)]
    pub detected_classes: Vec<Detected>,
}

impl ActionAnswer {
    /// The classes, most confident first, as V1 sorted them.
    pub fn by_confidence(&self) -> Vec<Detected> {
        let mut classes = self.detected_classes.clone();
        classes.sort_by(|a, b| b.conf.total_cmp(&a.conf));
        classes
    }
}

/// Pose's RULA score for the frame (`ergo`). Each side's detail is kept as
/// sent; V1 read `score` and `risk_level`, and each side's `score` and
/// `body_scores`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Ergo {
    #[serde(default)]
    pub score: i64,
    #[serde(default)]
    pub risk_level: i64,
    #[serde(default)]
    pub left: Option<Value>,
    #[serde(default)]
    pub right: Option<Value>,
}

/// Pose's answer to a frame: the points and the score, or an error.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PoseAnswer {
    #[serde(default)]
    pub pose_2d: Option<Value>,
    #[serde(default)]
    pub pose_3d: Option<Value>,
    #[serde(default)]
    pub ergo: Option<Ergo>,
    #[serde(default)]
    pub error: Option<String>,
}

/// A label's group, as V1's `ACTIVITY_GROUP` (`motion.py`) has it, for the
/// labels the skills will read in phase 2 (drink, eat, break) and the rest
/// V1 grouped. None for a label V1 did not group.
pub fn group(label: &str) -> Option<&'static str> {
    Some(match label {
        "drinking" | "drinking beer" | "drinking shots" | "tasting beer" => "drink",
        "stretching arm" | "stretching leg" => "break",
        "celebrating" | "clapping" | "applauding" => "celebrate",
        "tasting food" | "dining" | "eating burger" | "eating cake" | "eating carrots"
        | "eating chips" | "eating doughnuts" | "eating hotdog" | "eating ice cream"
        | "eating spaghetti" | "eating watermelon" => "eat",
        "using computer" | "writing" | "texting" | "reading book" | "reading newspaper"
        | "reading" | "drawing" | "playing controller" => "sedentary",
        "yawning" => "tired",
        "laughing" | "crying" | "singing" => "emotional",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Each sent message is V1's `json.dumps` dict, key for key and in its
    /// order, whitespace aside.
    #[test]
    fn sent_messages_are_v1s() {
        let config = serde_json::to_string(&Sent::action_config(None)).unwrap();
        assert_eq!(
            config,
            r#"{"type":"config","task":"action","whitelist":null,"threshold":0.3,"person_detection_enabled":true,"person_min_area_ratio":0.25}"#
        );
        let listed = Sent::action_config(Some(vec!["drinking".into(), "dining".into()]));
        assert_eq!(
            serde_json::to_value(&listed).unwrap()["whitelist"],
            json!(["drinking", "dining"])
        );
        assert_eq!(
            serde_json::to_string(&Sent::Heartbeat { task: Task::Pose }).unwrap(),
            r#"{"type":"heartbeat","task":"pose"}"#
        );
        assert_eq!(
            serde_json::to_string(&Sent::Frame {
                task: Task::Action,
                frame_b64: "/9j/AA==".into()
            })
            .unwrap(),
            r#"{"type":"frame","task":"action","frame_b64":"/9j/AA=="}"#
        );
        // V1's own text (json.dumps) reads back as the same message.
        let v1: Sent = serde_json::from_str(r#"{"type": "heartbeat", "task": "action"}"#).unwrap();
        assert_eq!(v1, Sent::Heartbeat { task: Task::Action });
    }

    #[test]
    fn answers_are_read_as_v1_reads_them() {
        let status: Status = serde_json::from_str(r#"{"status": "ok"}"#).unwrap();
        assert!(status.ok());
        let action: ActionAnswer = serde_json::from_str(
            r#"{"detected_classes": [{"class_name": "dining", "conf": 0.41}, {"class_name": "drinking", "conf": 0.77}], "extra": 1}"#,
        )
        .unwrap();
        let sorted = action.by_confidence();
        assert_eq!(sorted[0].class_name, "drinking");
        assert_eq!(sorted[1].class_name, "dining");
        let none: ActionAnswer = serde_json::from_str("{}").unwrap();
        assert!(none.detected_classes.is_empty());

        let pose: PoseAnswer = serde_json::from_str(
            r#"{"pose_2d": {"joints": [[1, 2]], "confs": [0.9]}, "pose_3d": null, "ergo": {"score": 5, "risk_level": 3, "left": {"score": 5}, "right": {"score": 4}}}"#,
        )
        .unwrap();
        let ergo = pose.ergo.unwrap();
        assert_eq!((ergo.score, ergo.risk_level), (5, 3));
        assert_eq!(ergo.right.unwrap()["score"], 4);
        assert!(pose.error.is_none());
        let failed: PoseAnswer = serde_json::from_str(r#"{"error": "no person"}"#).unwrap();
        assert_eq!(failed.error.as_deref(), Some("no person"));
        assert!(failed.ergo.is_none());
    }

    #[test]
    fn labels_group_as_v1s() {
        assert_eq!(group("drinking"), Some("drink"));
        assert_eq!(group("tasting beer"), Some("drink"));
        assert_eq!(group("eating burger"), Some("eat"));
        assert_eq!(group("dining"), Some("eat"));
        // The one class beyond V1's list.
        assert_eq!(group("eating chips"), Some("eat"));
        assert_eq!(group("stretching leg"), Some("break"));
        assert_eq!(group("yawning"), Some("tired"));
        assert_eq!(group("reading"), Some("sedentary"));
        assert_eq!(group("juggling"), None);
        // Every whitelisted label is one V1 grouped.
        assert!(WHITELIST.iter().all(|label| group(label).is_some()));
    }

    #[test]
    fn the_config_is_v1s_exactly() {
        let config = serde_json::to_value(v1_action_config()).unwrap();
        assert_eq!(config["whitelist"].as_array().unwrap().len(), 31);
        assert_eq!(config["whitelist"][0], "drinking");
        assert_eq!(config["whitelist"][13], "eating carrots");
        assert_eq!(config["whitelist"][14], "eating chips");
        assert_eq!(config["whitelist"][30], "singing");
        assert_eq!(config["threshold"], 0.3);
        assert_eq!(config["person_detection_enabled"], true);
        assert_eq!(config["person_min_area_ratio"], 0.25);
    }
}
