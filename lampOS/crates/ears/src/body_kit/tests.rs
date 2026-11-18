use super::*;

/// A clip of `label` whose patches score `class` as given and nothing else.
fn clip(label: &str, class: BodyClass, scores: &[f32]) -> Clip {
    Clip {
        label: label.into(),
        patches: scores
            .iter()
            .map(|&score| {
                let mut patch = [0.0; 5];
                patch[slot(class)] = score;
                patch
            })
            .collect(),
    }
}

fn quiet_hour(spikes: &[(u64, f32)]) -> Vec<OfficeRow> {
    (0..7_200u64)
        .map(|n| {
            let end_us = 1_000_000 + n * 500_000;
            let mut scores = [0.0; 5];
            if let Some((_, score)) = spikes.iter().find(|(at, _)| *at == n) {
                scores[0] = *score;
            }
            OfficeRow {
                end_us,
                own: false,
                scores,
            }
        })
        .collect()
}

fn session() -> Vec<Clip> {
    let mut clips = Vec::new();
    // Ten coughs: nine peak over 0.5, one at 0.2.
    for n in 0..9 {
        clips.push(clip(
            "cough",
            BodyClass::Cough,
            &[0.1, 0.6 + n as f32 * 0.01, 0.4, 0.0],
        ));
    }
    clips.push(clip("cough", BodyClass::Cough, &[0.2, 0.1]));
    // A sneeze the model also hears as a cough.
    clips.push(Clip {
        label: "sneeze".into(),
        patches: vec![[0.7, 0.0, 0.9, 0.0, 0.0]],
    });
    // The others: speech peaking at 0.33 on cough, a thump at 0.1.
    clips.push(clip("speech", BodyClass::Cough, &[0.33, 0.2]));
    clips.push(clip("thump", BodyClass::Cough, &[0.1]));
    clips
}

/// #7685: the bar is the lowest 0.01 step over every other sound's peak,
/// with the person's own hits, the other body sounds over it, and the
/// office hour's alarms through the publisher's detector.
#[test]
fn the_bar_is_just_over_the_other_sounds_and_says_how_it_does() {
    let clips = session();
    let office = quiet_hour(&[(100, 0.9), (101, 0.9), (5_000, 0.2)]);
    let suggestions = suggest(&clips, Some(&office));
    let cough = &suggestions[0];
    assert_eq!(cough.class, BodyClass::Cough);
    let bar = cough.bar.unwrap();
    assert_eq!(bar.bar, 0.34);
    // The longest cough holds 0.6.., 0.4 over 70% of 0.34: two patches, so
    // the floor of a second.
    assert_eq!(bar.refractory_us, 1_000_000);
    assert_eq!(
        (cough.own, cough.hits, cough.cross, cough.others),
        (10, 9, 1, 0)
    );
    assert_eq!(cough.recall(), 0.9);
    assert_eq!(cough.precision(), 0.9);
    // One spike over two patches is told once; 0.2 is under the bar.
    assert_eq!(cough.office.unwrap().0, 1);
    assert!(cough.alarms_an_hour().unwrap() < 1.01);
    assert!(!cough.publishable(), "one an hour is not under one");
    let calmer = suggest(&clips, Some(&quiet_hour(&[(5_000, 0.2)])));
    assert!(calmer[0].publishable());
    // No office hour: not publishable, whatever the clips say.
    assert!(!suggest(&clips, None)[0].publishable());
    // Sneeze has one clip, over its bar (no other sound scores it).
    let sneeze = &suggestions[2];
    assert_eq!(
        (sneeze.bar.unwrap().bar, sneeze.hits, sneeze.own),
        (0.01, 1, 1)
    );
    // No clip of a sniff or a wheeze: no bar.
    assert!(suggestions[3].bar.is_none() && suggestions[4].bar.is_none());
    assert!(!suggestions[4].publishable());
}

#[test]
fn a_class_with_no_other_sounds_to_set_it_against_has_no_bar() {
    let clips = vec![clip("cough", BodyClass::Cough, &[0.9])];
    assert!(suggest(&clips, None)[0].bar.is_none());
}

/// The detector the kit counts with is the publisher's: nothing while the
/// lamp speaks, one sound over several patches once, again after re-arm and
/// refractory.
#[test]
fn office_alarms_count_as_the_lamp_would_tell() {
    let bar = Bar {
        bar: 0.5,
        refractory_us: 1_000_000,
    };
    let mut rows = quiet_hour(&[(10, 0.8), (11, 0.9), (12, 0.6), (20, 0.8)]);
    rows[30].scores[0] = 0.9;
    rows[30].own = true;
    let (told, seconds) = office_alarms(&rows, BodyClass::Cough, bar);
    assert_eq!(told, 2);
    assert_eq!(seconds, 7_199.0 * 0.5);
    // Another class's bar does not count it.
    assert_eq!(office_alarms(&rows, BodyClass::Sneeze, bar).0, 0);
}

#[test]
fn the_kits_files_read_back() {
    let scores = SoundScores {
        cough: 0.5,
        wheeze: 0.25,
        ..SoundScores::default()
    };
    let text = clip_csv(&[(0.97, scores), (1.47, SoundScores::default())]);
    assert!(text.starts_with(CLIP_HEADER));
    let back = Clip::parse("cough", &text).unwrap();
    assert_eq!(back.patches, [[0.5, 0.0, 0.0, 0.0, 0.25], [0.0; 5]]);
    assert_eq!(back.peak(BodyClass::Wheeze), 0.25);
    assert!(Clip::parse("cough", "0.5,1,2").is_err());
    let row = OfficeRow::of(9, OwnVoiceEvidence::Present, &scores);
    assert_eq!(OfficeRow::parse(&row.csv()).unwrap(), Some(row));
    assert_eq!(OfficeRow::parse(OFFICE_HEADER).unwrap(), None);
    assert!(OfficeRow::parse("1,unknown,0.1").is_err());
    assert_eq!(class_of("throat_clear"), Some(BodyClass::ThroatClear));
    assert_eq!(class_of("speech"), None);
    assert!(plain_label("lamp_voice") && !plain_label("a b") && !plain_label(""));
    assert!(
        OTHERS
            .iter()
            .all(|label| plain_label(label) && class_of(label).is_none())
    );
}

#[test]
fn the_report_gives_bars_only_for_classes_that_pass() {
    let clips = session();
    let suggestions = suggest(&clips, Some(&quiet_hour(&[])));
    let report = report(&clips, &suggestions);
    assert!(report.starts_with("13 clips: 11 of the person's body sounds, 2 other sounds\n"));
    assert!(
        report.contains("cough         0.34  1.0     9/10 0.90     0.90"),
        "{report}"
    );
    assert!(
        report.contains("(BodyClass::Cough, Some(Bar { bar: 0.34, refractory_us: 1000000 })),")
    );
    // A sneeze from one clip has recall 1 and passes: the numbers say so.
    assert!(report.contains("(BodyClass::Sniff, None),"));
    assert!(report.contains("(BodyClass::Wheeze, None),"));
}

/// With the pinned model at hand (`LAMP_EARS_CLASSES_TEST_MODEL`), a 3 s
/// clip is five patches scored with the body sounds; skipped otherwise.
#[test]
fn a_clip_is_scored_patch_by_patch_by_the_publishers_model() {
    let Some(path) = std::env::var_os("LAMP_EARS_CLASSES_TEST_MODEL") else {
        eprintln!("LAMP_EARS_CLASSES_TEST_MODEL unset: skipped");
        return;
    };
    let model = Model::load(std::path::Path::new(&path))
        .unwrap()
        .with_body_sounds(true);
    let pcm: Vec<f32> = (0..48_000).map(|i| 0.2 * (i as f32 * 0.17).sin()).collect();
    let patches = score_clip(&model, &Frontend::new(), &pcm).unwrap();
    assert_eq!(patches.len(), 5);
    assert!((patches[0].0 - 0.966).abs() < 0.001);
    for (_, scores) in patches {
        assert!(
            BodyClass::ALL
                .iter()
                .all(|&class| score_of(&scores, class) > 0.0)
        );
    }
}
