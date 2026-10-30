use super::*;
use crate::decide::{Answer, Asked, Question};

/// A judge that answers every question with `yes`, as the model `name`,
/// in `took`; or fails, unreachable.
struct Scripted {
    name: &'static str,
    yes: Option<f32>,
    took: Duration,
    states: Vec<String>,
}

impl Decider for Scripted {
    fn ask(
        &mut self,
        state: &str,
        questions: &[(String, Question)],
        _deadline: Duration,
    ) -> Result<Asked, Failed> {
        self.states.push(state.to_owned());
        let Some(yes) = self.yes else {
            return Err(Failed::Unreachable {
                during: "connecting",
                kind: std::io::ErrorKind::ConnectionRefused,
            });
        };
        Ok(Asked {
            answers: questions
                .iter()
                .map(|(name, _)| (name.clone(), Answer::Noul(yes)))
                .collect(),
            model: format!("{}-20261014", self.name),
            took: self.took,
            usage: None,
            connection: None,
        })
    }

    fn name(&self) -> &str {
        self.name
    }
}

fn judge(name: &'static str, yes: Option<f32>, took_ms: u64) -> Scripted {
    Scripted {
        name,
        yes,
        took: Duration::from_millis(took_ms),
        states: Vec::new(),
    }
}

fn context(heard: &str, room: Room) -> Context {
    Context {
        heard: heard.into(),
        room,
        lamp_is_speaking: false,
        lamp_last_said: None,
        seconds_since_lamp_spoke: None,
        name_heard: None,
        like_asker: None,
    }
}

fn logged(id: &str, at: u64, heard: &str, room: Room) -> Logged {
    Logged {
        id: id.into(),
        at_unix_ms: at,
        context: context(heard, room),
        choice: "answer".into(),
        probability: 0.9,
        by: "typesafe/jev-1.13-20260917".into(),
        follows_lamp: None,
    }
}

/// The day's files are read as they are written: attention lines only, with
/// the line's number, and a line that is not one is counted, not fatal.
#[test]
fn the_logged_attention_decisions_are_read_with_their_place_in_the_day() {
    let dir = std::env::temp_dir().join(format!("policy-replay-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let attention = |at: u64, heard: &str| {
        json!({"at_unix_ms": at, "about": "attention", "shadow": true,
               "context": {"heard": heard, "room": "busy", "lamp_is_speaking": false,
                           "lamp_last_said": null, "seconds_since_lamp_spoke": null},
               "judgement": {"probability": 0.26, "choice": "stay_quiet", "from_rule": false},
               "by": "typesafe/jev-1.13-20260917"})
        .to_string()
    };
    let lines = [
        attention(1, "Hey Lamp, what is the weather like in Hanoi today?"),
        json!({"at_unix_ms": 2, "about": "progress", "context": {}}).to_string(),
        "{\"at_unix_ms\":3,\"about\":\"attention\"".to_owned(),
        json!({"at_unix_ms": 4, "about": "attention", "judgement": {}}).to_string(),
        attention(5, "Voglio mangiare sushi."),
    ];
    // A line cut inside a character, as a write the power took leaves one
    // (the reviewer on #1510): not text, counted unread, and the lines
    // after it still read.
    let mut file = lines.join("\n").into_bytes();
    let cut = attention(6, "Lamp ơi");
    let inside = cut.find('ơ').unwrap() + 1;
    file.push(b'\n');
    file.extend_from_slice(&cut.as_bytes()[..inside]);
    file.push(b'\n');
    file.extend_from_slice(attention(7, "Lamp, warmer.").as_bytes());
    fs::write(dir.join("decisions-2026-10-15.jsonl"), file).unwrap();
    let (read, unread) = read(&dir, &["2026-10-14".into(), "2026-10-15".into()]).unwrap();
    fs::remove_dir_all(&dir).unwrap();
    assert_eq!(unread, 3);
    let ids: Vec<&str> = read.iter().map(|logged| logged.id.as_str()).collect();
    assert_eq!(ids, ["2026-10-15#1", "2026-10-15#5", "2026-10-15#7"]);
    assert_eq!(read[0].context.room, Room::Busy);
    assert_eq!(read[1].choice, "stay_quiet");
    assert_eq!(read[1].context.name_heard, None);
}

/// What was heard is kept as kinds: how many words, the lamp named, and
/// likely another language.
#[test]
fn what_was_heard_is_told_as_kinds() {
    let kinds_of = |heard: &str| {
        let kinds = kinds(heard);
        (kinds.words, kinds.names_lamp, kinds.other_language)
    };
    assert_eq!(
        kinds_of("Hey Lamp, how much is one hundred dollars in euros?"),
        (10, true, false)
    );
    assert_eq!(kinds_of("Voglio mangiare sushi."), (3, false, true));
    assert_eq!(kinds_of("¿Qué?"), (1, false, true));
    assert_eq!(kinds_of("Lamp ơi, what time is it?"), (6, true, false));
    assert_eq!(kinds_of("Lamp ơi, mấy giờ rồi?"), (5, true, true));
    assert_eq!(kinds_of("Hey man, lunch?"), (3, true, false));
    assert_eq!(kinds_of("The plan is ready."), (4, true, false));
    assert_eq!(kinds_of("Sounds great."), (2, false, false));
    assert!(!attention::names_the_lamp("lampshade"));
}

/// Each model is asked exactly the text that was sent on the day, and its
/// answers are made a choice by the same weights.
#[test]
fn each_model_is_asked_what_was_sent_and_chooses_by_the_same_weights() {
    let decision = logged("2026-10-15#1", 1, "Hey Lamp, what time is it?", Room::Quiet);
    let mut sure = judge("typesafe/jev-1.14", Some(0.95), 410);
    match ask(&mut sure, &decision, Duration::from_secs(5)) {
        Replayed::Chose {
            by,
            choice,
            took_ms,
            answers,
            ..
        } => {
            assert_eq!(by, "typesafe/jev-1.14-20261014");
            assert_eq!(took_ms, 410);
            assert_eq!(answers.len(), 7);
            assert!(answers.values().all(|yes| *yes == 0.95));
            let answers = attention::Answers::of(
                &attention::questions()
                    .into_iter()
                    .map(|(name, _)| (name, Answer::Noul(0.95)))
                    .collect(),
            )
            .unwrap();
            assert_eq!(
                choice,
                attention::combine(&answers, &decision.context)
                    .choice
                    .as_str()
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(sure.states, [attention::state_text(&decision.context)]);
    let mut gone = judge("typesafe/jev-1.13", None, 0);
    assert_eq!(
        ask(&mut gone, &decision, Duration::from_secs(5)),
        Replayed::Failed("unreachable")
    );
}

/// A line names no word that was heard.
#[test]
fn a_line_holds_no_words() {
    let decision = logged("2026-10-15#139", 7, "Voglio mangiare sushi.", Room::Quiet);
    let line = line(
        &decision,
        Some(Should::StayQuiet),
        &[
            (
                "typesafe/jev-1.13".into(),
                Replayed::Chose {
                    by: "typesafe/jev-1.13-20260917".into(),
                    choice: "stay_quiet".into(),
                    probability: 0.26043,
                    took_ms: 531,
                    answers: [("calls_lamp".to_owned(), 0.05)].into(),
                },
            ),
            ("typesafe/jev-1.14".into(), Replayed::Failed("timed_out")),
        ],
    );
    let text = line.to_string();
    for word in ["Voglio", "mangiare", "sushi"] {
        assert!(!text.contains(word), "{text}");
    }
    assert_eq!(line["should"], "stay_quiet");
    assert_eq!(line["other_language"], true);
    assert_eq!(line["judges"][0]["probability"], 0.2604);
    assert_eq!(line["judges"][0]["answers"], json!({"calls_lamp": 0.05}));
    assert_eq!(line["judges"][1]["failed"], "timed_out");
    assert_eq!(line["name_heard"], Value::Null);
}

/// The counts: answered, the time's p50 and p95, what each chose, how often
/// it did what should have happened, overall and for named requests, a busy
/// room and another language; and how often the two chose alike.
#[test]
fn the_summary_counts_each_model_and_each_two_alike() {
    let models = ["old".to_owned(), "new".to_owned()];
    let chose = |choice: &str, took_ms: u64| Replayed::Chose {
        by: "x".into(),
        choice: choice.into(),
        probability: 0.5,
        took_ms,
        answers: BTreeMap::new(),
    };
    let lines = [
        // A named request in a busy room: the old model left it, the new answered.
        line(
            &logged("d#1", 1, "Hey Lamp, what is the weather?", Room::Busy),
            Some(Should::Answer),
            &[
                (models[0].clone(), chose("stay_quiet", 600)),
                (models[1].clone(), chose("answer", 500)),
            ],
        ),
        // Two people talking: both left it.
        line(
            &logged("d#2", 2, "Are you coming to lunch?", Room::Busy),
            Some(Should::StayQuiet),
            &[
                (models[0].clone(), chose("stay_quiet", 700)),
                (models[1].clone(), chose("stay_quiet", 900)),
            ],
        ),
        // Another language, nobody's label; the new model failed.
        line(
            &logged("d#3", 3, "Voglio mangiare sushi.", Room::Quiet),
            None,
            &[
                (models[0].clone(), chose("look_up", 800)),
                (models[1].clone(), Replayed::Failed("timed_out")),
            ],
        ),
    ];
    let summary = summary(&lines, &models);
    let old = &summary["models"]["old"];
    let new = &summary["models"]["new"];
    assert_eq!(old["answered"], 3);
    assert_eq!(new["answered"], 2);
    assert_eq!(new["failed"]["timed_out"], 1);
    assert_eq!(old["took_ms"], json!({"p50": 700, "p95": 800, "max": 800}));
    assert_eq!(new["took_ms"], json!({"p50": 500, "p95": 900, "max": 900}));
    assert_eq!(old["right"], json!({"labelled": 2, "right": 1}));
    assert_eq!(new["right"], json!({"labelled": 2, "right": 2}));
    assert_eq!(old["right_named"], json!({"labelled": 1, "right": 0}));
    assert_eq!(new["right_named"], json!({"labelled": 1, "right": 1}));
    assert_eq!(new["right_busy"], json!({"labelled": 2, "right": 2}));
    assert_eq!(old["chose"], json!({"look_up": 1, "stay_quiet": 2}));
    assert_eq!(old["same_as_logged"], json!({"n": 3, "same": 0}));
    assert_eq!(
        summary["pairs"][0],
        json!({"models": ["old", "new"], "both_answered": 2, "same_choice": 1, "same_side": 1})
    );
    assert!(!summary.to_string().contains("lunch"));
}

#[test]
fn labels_are_read_by_the_moment_and_refused_when_not_one_of_the_two() {
    let read = labels(
        "{\"at_unix_ms\": 5, \"should\": \"answer\"}\n\n{\"at_unix_ms\": 9, \"should\": \"stay_quiet\"}\n",
    )
    .unwrap();
    assert_eq!(read[&5], Should::Answer);
    assert_eq!(read[&9], Should::StayQuiet);
    assert!(labels("{\"at_unix_ms\": 5, \"should\": \"look_up\"}").is_err());
    assert!(labels("{\"should\": \"answer\"}").is_err());
    assert!(labels("not json").is_err());
}

/// A run is labelled after it is seen and counted again, asking nothing.
#[test]
fn an_earlier_run_is_counted_again_with_labels() {
    let models = ["old".to_owned(), "new".to_owned()];
    let chose = |choice: &str| Replayed::Chose {
        by: "x".into(),
        choice: choice.into(),
        probability: 0.5,
        took_ms: 100,
        answers: BTreeMap::new(),
    };
    let lines = [
        line(
            &logged("d#1", 11, "Hey Lamp, what is the weather?", Room::Busy),
            None,
            &[
                (models[0].clone(), chose("stay_quiet")),
                (models[1].clone(), chose("answer")),
            ],
        ),
        line(
            &logged("d#2", 12, "Are you coming to lunch?", Room::Busy),
            None,
            &[
                (models[0].clone(), chose("stay_quiet")),
                (models[1].clone(), chose("answer")),
            ],
        ),
    ];
    let mut text: String = lines.iter().map(|line| format!("{line}\n")).collect();
    text.push_str(&format!("{}\n", summary(&lines, &models)));
    let labels = labels("{\"at_unix_ms\": 11, \"should\": \"answer\"}").unwrap();
    let (again, found) = relabelled(&text, &labels).unwrap();
    assert_eq!(found, models);
    assert_eq!(again.len(), 2);
    assert_eq!(again[0]["should"], "answer");
    assert_eq!(again[1]["should"], Value::Null);
    let counted = summary(&again, &found);
    assert_eq!(
        counted["models"]["new"]["right"],
        json!({"labelled": 1, "right": 1})
    );
    assert_eq!(
        counted["models"]["old"]["right"],
        json!({"labelled": 1, "right": 0})
    );
    assert!(relabelled("{\"status\": \"other\"}", &labels).is_err());
}

/// The picture lines counted: sent or why not, the two askings, and each
/// sign beside the eyes' own, a label taken for the nearest line within 3 s.
#[test]
fn the_picture_lines_are_counted_against_labels_by_the_nearest_moment() {
    let sent = |at: u64, without: &str, with: &str, faces: f64, facing: u64| {
        json!({"at_unix_ms": at, "about": "attention_picture",
               "picture": {"sent": true, "width": 640, "height": 480},
               "seen": {"faces": 1, "near": 1, "facing": facing, "age_ms": 40},
               "without": {"choice": without, "took_ms": 600},
               "with": {"choice": with, "took_ms": 1_400,
                        "answers": {"picture_faces_lamp": faces, "picture_away": 0.1}}})
    };
    let lines = [
        // A named request at the lamp: the picture's asking got it right.
        sent(10_000, "stay_quiet", "answer", 0.9, 1),
        // Two people talking: both left it.
        sent(20_000, "stay_quiet", "stay_quiet", 0.2, 0),
        json!({"at_unix_ms": 30_000, "about": "attention_picture",
               "picture": {"sent": false, "why_not": "no_frame"}}),
        // Nobody labelled this one.
        sent(90_000, "answer", "answer", 0.9, 1),
    ];
    let labels = labels(
        "{\"at_unix_ms\": 11500, \"should\": \"answer\"}\n{\"at_unix_ms\": 19000, \"should\": \"stay_quiet\"}\n{\"at_unix_ms\": 50000, \"should\": \"answer\"}",
    )
    .unwrap();
    let summary = pictures_summary(&lines, &labels);
    assert_eq!(summary["questions"], 4);
    assert_eq!(summary["picture_sent"], 3);
    assert_eq!(summary["why_not"], json!({"no_frame": 1}));
    assert_eq!(
        summary["without"]["right"],
        json!({"labelled": 2, "right": 1})
    );
    assert_eq!(summary["with"]["right"], json!({"labelled": 2, "right": 2}));
    assert_eq!(
        summary["with"]["took_ms"],
        json!({"p50": 1400, "p95": 1400, "max": 1400})
    );
    assert_eq!(
        summary["signs"]["picture_faces_lamp"],
        json!({"labelled": 2, "right": 2, "yes": 1})
    );
    assert_eq!(
        summary["signs"]["eyes_facing"],
        json!({"labelled": 2, "right": 2, "yes": 1})
    );
    assert_eq!(
        summary["signs"]["picture_away"],
        json!({"labelled": 2, "right": 1, "yes": 0})
    );
}

/// The first builder on #871: on an empty desk the questions that take a
/// person for granted are answered near chance, so they are counted again
/// only where the picture had a person in it; and whether the picture's
/// person and the eyes' faces agree is counted line by line.
#[test]
fn the_signs_of_a_person_are_counted_again_only_where_the_picture_had_one() {
    let line = |at: u64, person: f64, faces_lamp: f64, talking: f64, faces: u64| {
        json!({"at_unix_ms": at, "about": "attention_picture",
               "picture": {"sent": true, "delivery": "answered", "withdrawn": false},
               "seen": {"faces": faces, "near": faces, "facing": faces, "age_ms": 40},
               "without": {"choice": "answer", "took_ms": 500},
               "with": {"choice": "answer", "took_ms": 600,
                        "answers": {"picture_person": person, "picture_faces_lamp": faces_lamp,
                                    "picture_talking_to_another": talking}}})
    };
    let lines = [
        // Someone at the lamp, turned to it.
        line(10_000, 0.9, 0.9, 0.2, 1),
        // The empty desk: "two people talking", near chance.
        line(20_000, 0.1, 0.3, 0.69, 0),
        // Two people talking, the eyes seeing no face.
        line(30_000, 0.8, 0.2, 0.7, 0),
    ];
    let labels = labels(
        "{\"at_unix_ms\": 10000, \"should\": \"answer\"}\n{\"at_unix_ms\": 20000, \"should\": \"stay_quiet\"}\n{\"at_unix_ms\": 30000, \"should\": \"stay_quiet\"}",
    )
    .unwrap();
    let summary = pictures_summary(&lines, &labels);
    assert_eq!(
        summary["signs"]["picture_talking_to_another"],
        json!({"labelled": 3, "right": 3, "yes": 2})
    );
    assert_eq!(
        summary["signs_given_a_person"]["picture_talking_to_another"],
        json!({"labelled": 2, "right": 2, "yes": 1})
    );
    assert_eq!(
        summary["signs_given_a_person"]["picture_faces_lamp"],
        json!({"labelled": 2, "right": 2, "yes": 1})
    );
    assert!(
        summary["signs_given_a_person"]
            .get("picture_away")
            .is_none()
    );
    assert_eq!(
        summary["picture_person"],
        json!({"lines": 3, "yes": 2, "eyes_saw_a_face": 1, "agree": 2})
    );
}

/// The reviewer on #873: a picture that went counts as sent whatever came
/// after, with how it was delivered and whether the asking was withdrawn
/// after; and the two askings are counted right on the same lines, a line
/// where either made no choice being in neither's right, with why in its
/// `failed`.
#[test]
fn a_picture_that_went_is_counted_and_both_askings_are_counted_on_the_same_lines() {
    let line = |at: u64, picture: Value, without: Value, with: Value| {
        json!({"at_unix_ms": at, "about": "attention_picture", "picture": picture,
               "without": without, "with": with})
    };
    let answered = json!({"sent": true, "delivery": "answered", "withdrawn": false});
    let chose = |choice: &str| json!({"choice": choice, "took_ms": 500});
    let lines = [
        line(
            10_000,
            answered.clone(),
            chose("stay_quiet"),
            chose("answer"),
        ),
        // The picture went, its connection closed, and the next try was
        // withdrawn: sent, and the picture's asking failed. The asking
        // without the picture chose, but is not counted here either.
        line(
            20_000,
            json!({"sent": true, "delivery": "uncertain", "withdrawn": true}),
            chose("answer"),
            json!({"failed": "withdrawn"}),
        ),
        // The asking without the picture failed: neither is counted.
        line(
            30_000,
            answered.clone(),
            json!({"failed": "timed_out"}),
            chose("answer"),
        ),
        line(
            40_000,
            json!({"sent": false, "why_not": "not_written"}),
            Value::Null,
            json!({"failed": "unreachable"}),
        ),
    ];
    let labels = labels(
        "{\"at_unix_ms\": 10000, \"should\": \"answer\"}\n{\"at_unix_ms\": 20000, \"should\": \"answer\"}\n{\"at_unix_ms\": 30000, \"should\": \"answer\"}",
    )
    .unwrap();
    let summary = pictures_summary(&lines, &labels);
    assert_eq!(summary["picture_sent"], 3);
    assert_eq!(summary["delivery"], json!({"answered": 2, "uncertain": 1}));
    assert_eq!(summary["withdrawn_after_sending"], 1);
    assert_eq!(summary["why_not"], json!({"not_written": 1}));
    assert_eq!(
        summary["without"]["right"],
        json!({"labelled": 1, "right": 0})
    );
    assert_eq!(summary["with"]["right"], json!({"labelled": 1, "right": 1}));
    assert_eq!(summary["with"]["failed"], json!({"withdrawn": 1}));
    assert_eq!(summary["without"]["failed"], json!({"timed_out": 1}));
    assert_eq!(
        (
            &summary["without"]["answered"],
            &summary["with"]["answered"]
        ),
        (&json!(2), &json!(2))
    );
}

/// The reviewer on #873: one label near two lines labels only the nearer,
/// so a decision nobody labelled is not counted as labelled; a label nearer
/// a line with no frame stays with that line; a label equally near two is
/// given to neither; two labels that disagree on one line are both dropped.
#[test]
fn each_label_is_given_once_to_the_nearest_line() {
    let sent = |at: u64, with: &str| {
        json!({"at_unix_ms": at, "about": "attention_picture",
               "picture": {"sent": true},
               "seen": {"facing": 1},
               "without": {"choice": with, "took_ms": 500},
               "with": {"choice": with, "took_ms": 900, "answers": {"picture_faces_lamp": 0.9}}})
    };
    let none = |at: u64| {
        json!({"at_unix_ms": at, "about": "attention_picture",
               "picture": {"sent": false, "why_not": "no_frame"}})
    };
    let lines = [
        sent(9_000, "answer"),
        sent(12_000, "stay_quiet"),
        none(30_000),
        sent(32_000, "answer"),
        sent(50_000, "answer"),
        sent(52_000, "answer"),
        sent(70_000, "answer"),
    ];
    let labels = labels(
        "{\"at_unix_ms\": 10000, \"should\": \"answer\"}\n\
         {\"at_unix_ms\": 30500, \"should\": \"answer\"}\n\
         {\"at_unix_ms\": 51000, \"should\": \"answer\"}\n\
         {\"at_unix_ms\": 69900, \"should\": \"answer\"}\n\
         {\"at_unix_ms\": 70100, \"should\": \"stay_quiet\"}\n\
         {\"at_unix_ms\": 90000, \"should\": \"answer\"}",
    )
    .unwrap();
    let summary = pictures_summary(&lines, &labels);
    // Only the line at 9 s is labelled: 12 s is not, 32 s is not (the label
    // went to the frameless line at 30 s), 50 and 52 s tie, and 70 s has two
    // labels that disagree.
    assert_eq!(summary["with"]["right"], json!({"labelled": 1, "right": 1}));
    assert_eq!(summary["signs"]["picture_faces_lamp"]["labelled"], 1);
    assert_eq!(summary["signs"]["eyes_facing"]["labelled"], 1);
    assert_eq!(
        summary["labels"],
        json!({"given": 6, "matched": 2, "unmatched": 1, "ambiguous": 1, "conflicting": 2})
    );
}

/// The reviewer on #873: two labels at one moment that disagree are refused,
/// in either order; the same label twice is one.
#[test]
fn labels_that_disagree_at_one_moment_are_refused_and_a_repeat_is_one() {
    let answer = "{\"at_unix_ms\": 10000, \"should\": \"answer\"}";
    let quiet = "{\"at_unix_ms\": 10000, \"should\": \"stay_quiet\"}";
    for text in [format!("{answer}\n{quiet}"), format!("{quiet}\n{answer}")] {
        let why = labels(&text).unwrap_err();
        assert!(why.starts_with("label line 2 says"), "{why}");
    }
    let repeated = labels(&format!("{answer}\n{answer}")).unwrap();
    assert_eq!(repeated.len(), 1);
    let lines = [json!({"at_unix_ms": 10_000, "about": "attention_picture",
                        "picture": {"sent": true}, "seen": {"facing": 1},
                        "with": {"choice": "answer", "took_ms": 900}})];
    assert_eq!(
        pictures_summary(&lines, &repeated)["labels"],
        json!({"given": 1, "matched": 1, "unmatched": 0, "ambiguous": 0, "conflicting": 0})
    );
}

fn decided(context: Context, choice: &str, by: &str) -> Logged {
    Logged {
        id: "2026-10-18#1".into(),
        at_unix_ms: 1,
        context,
        choice: choice.into(),
        probability: 0.5,
        by: by.into(),
        follows_lamp: None,
    }
}

/// #705, Ten's step 2 on the Policy's wait: each model decision beside the
/// rule's on the same context, by kind. The rule's own decisions are left
/// out, a look up counts as taking the turn, and a kind is certain only
/// with enough decisions and no disagreement.
#[test]
fn the_rule_and_the_model_are_counted_side_by_side_by_kind() {
    let called = |score: f32| Context {
        name_heard: Some(score),
        ..context("hey lamp, warmer", Room::Busy)
    };
    let busy = context("we should go", Room::Busy);
    let quiet = context("make it brighter", Room::Quiet);
    let mut logged = Vec::new();
    // Called plainly in a busy room: both take it, three times.
    for _ in 0..3 {
        logged.push(decided(called(0.995), "answer", "typesafe/jev-1.13"));
    }
    // Busy, not called: the rule stays quiet; the model once answers, once
    // looks up, once stays quiet.
    logged.push(decided(busy.clone(), "answer", "typesafe/jev-1.13"));
    logged.push(decided(busy.clone(), "look_up", "typesafe/jev-1.13"));
    logged.push(decided(busy.clone(), "stay_quiet", "typesafe/jev-1.13"));
    // Quiet room: the rule takes it; the model stays quiet once.
    logged.push(decided(quiet.clone(), "stay_quiet", "typesafe/jev-1.13"));
    // The rule's own decision is not a model's.
    logged.push(decided(busy.clone(), "stay_quiet", "rule"));
    // Fourth and the reviewer on #1510: a model's `stay_quiet` following the
    // lamp (follows_lamp 0.89) runs the lamp's calls, where the rule's plain
    // `stay_quiet` holds them: a disagreement, not an agreement. Below the
    // bar it is the plain one.
    let after_the_lamp = context("thanks, now a bit louder", Room::Unknown);
    logged.push(Logged {
        follows_lamp: Some(0.89),
        ..decided(after_the_lamp.clone(), "stay_quiet", "typesafe/jev-1.13")
    });
    logged.push(Logged {
        follows_lamp: Some(0.31),
        ..decided(after_the_lamp.clone(), "stay_quiet", "typesafe/jev-1.13")
    });
    let kinds = agreement(&logged);
    assert_eq!(
        kinds[&Kind::of(&called(0.995))],
        Agreement {
            decisions: 3,
            agreed: 3,
            ..Agreement::default()
        }
    );
    assert_eq!(Kind::of(&called(0.995)).name, ">=0.99");
    assert_eq!(Kind::of(&called(0.95)).name, "0.9-0.99");
    assert_eq!(
        kinds[&Kind::of(&busy)],
        Agreement {
            decisions: 3,
            agreed: 1,
            rule_took_model_quiet: 0,
            rule_quiet_model_took: 2,
            model_quiet_following: 0,
        }
    );
    assert_eq!(
        kinds[&Kind::of(&after_the_lamp)],
        Agreement {
            decisions: 2,
            agreed: 1,
            rule_took_model_quiet: 0,
            rule_quiet_model_took: 1,
            model_quiet_following: 1,
        }
    );
    assert_eq!(
        kinds[&Kind::of(&quiet)],
        Agreement {
            decisions: 1,
            agreed: 0,
            rule_took_model_quiet: 1,
            rule_quiet_model_took: 0,
            model_quiet_following: 0,
        }
    );
    let printed = agreement_summary(&kinds, 3);
    assert!(
        printed.contains("busy false false >=0.99 | 3 3 0 0 0"),
        "{printed}"
    );
    assert!(
        printed.contains("no disagreement: 1 of 4 kinds, 3 of 9 decisions"),
        "{printed}"
    );
    assert!(printed.contains("up to 63 % is still likely"), "{printed}");
    assert!(!printed.contains("hey lamp") && !printed.contains("brighter"));
}

/// The reviewer on #1510: a picture line cut inside a character (a write
/// the power took) is passed over, and the lines after it still read.
#[test]
fn a_picture_line_cut_inside_a_character_does_not_end_the_run() {
    // Not pictures' `policy-pictures-`: the crate's unit tests are one process.
    let dir = std::env::temp_dir().join(format!("policy-replay-cut-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let picture = |at: u64, heard: &str| {
        json!({"at_unix_ms": at, "about": "attention_picture", "context": {"heard": heard}})
            .to_string()
    };
    let cut = picture(2, "Lamp ơi");
    let inside = cut.find('ơ').unwrap() + 1;
    let mut file = picture(1, "Lamp, warmer.").into_bytes();
    file.push(b'\n');
    file.extend_from_slice(&cut.as_bytes()[..inside]);
    file.push(b'\n');
    file.extend_from_slice(picture(3, "Lamp, cooler.").as_bytes());
    fs::write(dir.join("pictures-2026-10-15.jsonl"), file).unwrap();
    let read = read_pictures(&dir, &["2026-10-15".into()]).unwrap();
    fs::remove_dir_all(&dir).unwrap();
    let ats: Vec<u64> = read
        .iter()
        .map(|line| line["at_unix_ms"].as_u64().unwrap())
        .collect();
    assert_eq!(ats, [1, 3]);
}

/// `--rule-decides` takes kinds as `--agreement` prints them, and nothing
/// else.
#[test]
fn a_kind_reads_back_as_printed() {
    let called = Context {
        name_heard: Some(0.995),
        ..context("hey lamp", Room::Busy)
    };
    for kind in [Kind::of(&called), Kind::of(&context("so", Room::Quiet))] {
        assert_eq!(Kind::parse(&kind.to_string()), Some(kind));
    }
    assert_eq!(Kind::of(&called).to_string(), "busy false false >=0.99");
    for refused in [
        "",
        "busy false false",
        "busy no false none",
        "loud false false none",
        "busy false false 0.99",
        "busy false false none extra",
    ] {
        assert_eq!(Kind::parse(refused), None, "{refused:?}");
    }
}

/// `--agreement --from/--until`: a UTC moment as milliseconds since 1970,
/// the same as `date -u -d … +%s`, and nothing else.
#[test]
fn a_utc_moment_reads_as_milliseconds() {
    assert_eq!(utc_ms("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(utc_ms("2026-10-18T11:00:00Z"), Some(1_792_321_200_000));
    assert_eq!(utc_ms("2026-10-19T00:00:00Z"), Some(1_792_368_000_000));
    assert_eq!(utc_ms("2024-02-29T23:59:59Z"), Some(1_709_251_199_000));
    assert!(utc_ms("2026-10-31T00:00:00Z").is_some());
    for refused in [
        "",
        "2026-10-18",
        "2026-10-18T11:00:00",
        "2026-10-18T11:00Z",
        "2026-13-18T11:00:00Z",
        "2026-10-18T24:00:00Z",
        "2026-10-18T11:00:00+07:00",
        "26-10-18T11:00:00Z",
        "2026-02-29T00:00:00Z",
        "2026-02-30T00:00:00Z",
        "2026-09-31T00:00:00Z",
    ] {
        assert_eq!(utc_ms(refused), None, "{refused:?}");
    }
}

/// The window keeps `from` and drops `until`, so a night's run and the
/// next never count one decision twice.
#[test]
fn a_window_keeps_its_start_and_drops_its_end() {
    let at = |ms: u64| Logged {
        at_unix_ms: ms,
        ..decided(context("x", Room::Busy), "answer", "typesafe/jev-1.13")
    };
    let all = || vec![at(9), at(10), at(15), at(20), at(21)];
    let times = |lines: &[Logged]| lines.iter().map(|line| line.at_unix_ms).collect::<Vec<_>>();
    let mut lines = all();
    within(&mut lines, Some(10), Some(20));
    assert_eq!(times(&lines), [10, 15]);
    let mut lines = all();
    within(&mut lines, None, Some(20));
    assert_eq!(times(&lines), [9, 10, 15]);
    let mut lines = all();
    within(&mut lines, Some(20), None);
    assert_eq!(times(&lines), [20, 21]);
    let mut lines = all();
    within(&mut lines, None, None);
    assert_eq!(times(&lines), [9, 10, 15, 20, 21]);
}

/// A judge whose connection a probe steers, by Jev's own rules: an asking
/// after `forget` makes its own connection; `ready` makes one ahead only
/// while none is kept (a kept one has 600 s here, far over the 6 s `ready`
/// leaves be); otherwise the kept one. What it does and what the probe
/// waits go into one record, in order.
struct Steered {
    done: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
    kept: bool,
    next: Option<Connection>,
}

impl Decider for Steered {
    fn ask(
        &mut self,
        _state: &str,
        questions: &[(String, Question)],
        _deadline: Duration,
    ) -> Result<Asked, Failed> {
        let on = self.next.take().unwrap_or(Connection::Kept);
        self.done.borrow_mut().push(format!("ask on {on:?}"));
        self.kept = true;
        Ok(Asked {
            answers: questions
                .iter()
                .map(|(name, _)| (name.clone(), Answer::Noul(0.5)))
                .collect(),
            model: "steered".into(),
            took: Duration::from_millis(500),
            usage: None,
            connection: Some(on),
        })
    }
    fn ready(&mut self, within: Duration) {
        self.done
            .borrow_mut()
            .push(format!("ready within {}", within.as_secs()));
        if !self.kept {
            self.next = Some(Connection::Ahead);
        }
    }
    fn name(&self) -> &str {
        "steered"
    }
}

impl Connections for Steered {
    fn forget(&mut self) {
        self.done.borrow_mut().push("forget".into());
        self.kept = false;
        self.next = Some(Connection::New);
    }
    fn keep_answered_for(&mut self, keep: Duration) {
        self.done
            .borrow_mut()
            .push(format!("keep {}", keep.as_secs()));
    }
}

/// #1522: the probe asks the one logged question seven ways, in order and
/// waiting as each says, keeping a connection 600 s so the last kept asking
/// tests the other end; one line each as it is made, with what it was sent
/// on and how long it took. The whole record is checked, so a probe that
/// skipped its `forget`, or waited before `ready` rather than after, would
/// show (the reviewer on #1630).
#[test]
fn the_connection_probe_asks_seven_ways_with_its_waits() {
    let done = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut judge = Steered {
        done: done.clone(),
        kept: false,
        next: None,
    };
    let question = logged(
        "2026-10-18#518",
        1,
        "Can you make your light a little warmer?",
        Room::Busy,
    );
    let mut lines = Vec::new();
    let waits = done.clone();
    probe_connections(
        &mut judge,
        &question,
        Duration::from_secs(10),
        1,
        &mut |wait| waits.borrow_mut().push(format!("wait {}", wait.as_secs())),
        &mut |line| lines.push(line.clone()),
    );
    assert_eq!(
        *done.borrow(),
        [
            "keep 600",
            "wait 300",
            "forget",
            "ask on New",
            "wait 0",
            "ask on Kept",
            "wait 0",
            "forget",
            "ask on New",
            "wait 0",
            "forget",
            "ready within 2",
            "wait 2",
            "ask on Ahead",
            "wait 140",
            "ask on Kept",
            "wait 300",
            "ask on Kept",
            "wait 300",
            "forget",
            "ready within 2",
            "wait 2",
            "ask on Ahead",
        ]
    );
    let probes: Vec<(&str, &str)> = lines
        .iter()
        .map(|line| {
            (
                line["probe"].as_str().unwrap(),
                line["on"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        probes,
        [
            ("fresh_after_quiet", "new"),
            ("kept_at_once", "kept"),
            ("fresh_at_once", "new"),
            ("ahead_at_once", "ahead"),
            ("kept_after_140s", "kept"),
            ("kept_after_300s", "kept"),
            ("ahead_after_quiet", "ahead"),
        ]
    );
    assert!(
        lines
            .iter()
            .all(|line| line["took_ms"] == 500 && line["round"] == 1)
    );
    // No words in any line.
    assert!(
        lines
            .iter()
            .all(|line| !line.to_string().contains("warmer"))
    );
}

/// The reviewer on #1630: the probe's question is the newest one a model
/// decided, never one the rule decided alone, whose words stayed on the
/// lamp; none if the rule decided them all.
#[test]
fn the_probe_asks_only_what_went_to_a_judge_on_the_day() {
    let mut asked = logged("2026-10-18#1", 1, "one", Room::Busy);
    asked.by = "typesafe/jev-1.13-20260917".into();
    let mut ruled = logged("2026-10-18#2", 2, "two", Room::Busy);
    ruled.by = "rule".into();
    let both = [asked.clone(), ruled.clone()];
    assert_eq!(
        newest_asked(&both).map(|line| line.id.as_str()),
        Some("2026-10-18#1")
    );
    assert!(newest_asked(&[ruled]).is_none());
    assert!(newest_asked(&[]).is_none());
}
