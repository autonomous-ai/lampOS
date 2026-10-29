//! The stable notes' "Built but not on yet" part (#3976): which rows are
//! listed, from what the product turns on, and where the part goes.
use lampos_promote::notes::{
    self, DEVELOPERS, HEADING, Waiting, placed, section, turned_on, waiting,
};
use std::{collections::BTreeMap, path::Path};

const PAGE: &str = "\
# Switches

## How to read it

| Switch | Values and default | Counted as |
|---|---|---|
| LAMP_NOT_A_TABLE | `on`; off. Never read: this isn't a switch table | until |

## The voice: hearing, turns, the ring and the backup voice

| Switch | Values and default | lamp-4ace (repo unit) | Product unit | Counted as |
|---|---|---|---|---|
| LAMP_WAITS | `on`; off. Answers a person who waits. More words after | — | — | until |
| LAMP_IS_ON | `on`; off. On in the product | on | on | until |
| LAMP_DIAGNOSTIC | `on`; off. For debugging only | — | — | off |
| LAMP_SOON | `on`; off. Proposed now | — | — | now |
| LAMP_SAID_ON_BUT_NOT | `on`; off. The cell says on, the units don't | on | on | until |
| LAMP_SET_OFF | `on`; off. Set to off in a unit | off | off | until |

## The voice's own flags (`lamp-voice serve`)

| Switch | Values and default | lamp-4ace (repo unit) | Product unit | Counted as |
|---|---|---|---|---|
| `--flag-on` | a number; none. Passed in ExecStart | yes | yes | until |
| `--flag-off` | a number; none. Not passed | — | — | until |

## The other parts: body, eyes, expression, tasks and controls

| Switch | Values and default | lamp-4ace (repo unit) | Product unit | Counted as |
|---|---|---|---|---|
| LAMP_STORE | `on`; off. From a settings file | — | on | until |
| LAMP_WAITS | `on`; off. The same switch again, listed once | — | — | until |
";

fn product() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "lamp-voice.service".to_owned(),
            "[Service]\n\
             ExecStart=/opt/lamp-voice serve --flag-on 3\n\
             Environment=LAMP_IS_ON=on\n\
             Environment=LAMP_SET_OFF=off\n\
             # Environment=LAMP_SAID_ON_BUT_NOT=on\n"
                .to_owned(),
        ),
        (
            "store.env".to_owned(),
            "# a comment\nLAMP_STORE=on\n".to_owned(),
        ),
    ])
}

#[test]
fn a_row_counted_until_or_now_that_no_product_file_turns_on_is_listed_once() {
    let on = turned_on(&product());
    assert!(on.contains_key("LAMP_IS_ON") && on.contains_key("--flag-on"));
    assert!(on.contains_key("LAMP_STORE"), "a settings .env file counts");
    assert!(!on.contains_key("LAMP_SET_OFF"), "a value of off is off");
    assert!(
        !on.contains_key("LAMP_SAID_ON_BUT_NOT"),
        "a commented line is not read"
    );
    let found = waiting(PAGE, &on);
    let listed: Vec<&str> = found.iter().map(|w| w.switch.as_str()).collect();
    // The units decide, not the page's Product unit cell; off and setting
    // rows, and rows outside the four tables, are never listed.
    assert_eq!(
        listed,
        [
            "LAMP_WAITS",
            "LAMP_SOON",
            "LAMP_SAID_ON_BUT_NOT",
            "LAMP_SET_OFF",
            "--flag-off"
        ]
    );
}

#[test]
fn a_rows_words_are_its_first_sentence_after_its_values_and_bounded() {
    let found = waiting(PAGE, &turned_on(&product()));
    assert_eq!(found[0].words, "Answers a person who waits.");
    assert_eq!(found[0].part, "The voice: hearing, turns and the ring");
    assert_eq!(found[4].part, "The voice's own flags");
    let long = format!(
        "| LAMP_LONG | `on`; off. {} | — | — | until |",
        "word ".repeat(80)
    );
    let page = format!(
        "## The other parts\n\n| Switch | Values | a | b | Counted as |\n|---|---|---|---|---|\n{long}\n"
    );
    let words = &waiting(&page, &BTreeMap::new())[0].words;
    assert!(
        words.ends_with('…') && words.chars().count() <= 141,
        "{words}"
    );
}

#[test]
fn the_part_says_how_many_and_folds_the_list_by_table() {
    let found = waiting(PAGE, &turned_on(&product()));
    let part = section(&found);
    assert!(part.starts_with(&format!(
        "{HEADING}\n5 features are in this build but off on every lamp"
    )));
    assert!(part.contains("<details><summary>The 5, by part</summary>"));
    assert_eq!(
        part.matches("**The voice: hearing, turns and the ring**")
            .count(),
        1
    );
    assert!(part.contains("- `--flag-off`: Not passed."));
    assert!(part.trim_end().ends_with("</details>"));
    assert_eq!(
        section(&[]),
        format!("{HEADING}\nNothing: every feature waiting for a lamp check is on.\n")
    );
}

#[test]
fn the_part_replaces_its_own_or_goes_before_the_developers_and_nothing_else_moves() {
    let part = section(&[Waiting {
        part: "The other parts",
        switch: "LAMP_X".to_owned(),
        words: "X.".to_owned(),
    }]);
    let hand = "## lampOS build 8652\n\nThe notes in plain words.\n";
    let developers = format!("{DEVELOPERS}\nMerged since 8600:\n- #1 one\n");
    // First time: between the plain words and the developers part.
    let once = placed(&format!("{hand}\n{developers}"), &part);
    assert_eq!(once, format!("{hand}\n{part}\n{developers}"));
    // Again: replaced in place, the rest byte for byte.
    let other = section(&[]);
    let twice = placed(&once, &other);
    assert_eq!(twice, format!("{hand}\n{other}\n{developers}"));
    assert_eq!(
        placed(&twice, &other),
        twice,
        "the same part twice changes nothing"
    );
    // A known-issues part written by hand after it is kept.
    let known = "### Known issues\n- one\n";
    let with_known = format!("{hand}\n{part}\n{known}");
    assert_eq!(
        placed(&with_known, &other),
        format!("{hand}\n{other}\n{known}")
    );
    // No developers part: at the end; an empty body: the part alone.
    assert_eq!(placed(hand, &part), format!("{hand}\n{part}"));
    assert_eq!(placed("", &part), part);
}

/// The real page and units: every switch listed is on in no product file,
/// one the product turns on is never listed, and the page's tables are read.
#[test]
fn on_this_repository_each_listed_switch_is_off_in_every_product_file() {
    let lamp_os = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../lampOS");
    let page = std::fs::read_to_string(lamp_os.join("docs/switches.md")).unwrap();
    let files = notes::product_files(&lamp_os).unwrap();
    assert!(files.contains_key("lamp-voice.service") && files.contains_key("store.env"));
    let on = turned_on(&files);
    assert_eq!(
        on.get("LAMP_ATTENTION").map(String::as_str),
        Some("lamp-voice.service")
    );
    let found = waiting(&page, &on);
    assert!(found.len() > 10, "{}", found.len());
    for listed in &found {
        assert!(!on.contains_key(&listed.switch), "{} is on", listed.switch);
        for (name, text) in &files {
            for line in text.lines().filter(|l| !l.trim_start().starts_with('#')) {
                assert!(
                    !line.contains(&format!("{}=on", listed.switch)),
                    "{name} turns {} on",
                    listed.switch
                );
            }
        }
    }
    assert!(!found.iter().any(|w| w.switch == "LAMP_ATTENTION"));
}
