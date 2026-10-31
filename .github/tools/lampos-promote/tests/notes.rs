//! The stable notes' "Built but not on yet" part (#3976): which rows are
//! listed, from what the product turns on, and where the part goes.
use lampos_promote::notes::{
    self, DEVELOPERS, HEADING, ON_UNSET, PLAIN, PLAIN_MOST, Waiting, placed, plain_lines, section,
    turned_on, waiting,
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
| LAMP_BY_DEFAULT | `off` to turn it off; unset, on. On by its code | on (unset) | on (unset) | until |
| LAMP_OFF_UNSET | `on`; unset, off. Off by its code | — | off (unset) | until |

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

## In plain words

| Switch | What a person would notice |
|---|---|
| `LAMP_WAITS` | The lamp answers you when you wait for it. |
| LAMP_IS_ON | On already, so never listed. |
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
            "LAMP_OFF_UNSET",
            "--flag-off"
        ]
    );
}

#[test]
fn a_rows_words_are_its_first_sentence_after_its_values_and_bounded() {
    let found = waiting(PAGE, &turned_on(&product()));
    assert_eq!(found[0].words, "Answers a person who waits.");
    assert_eq!(found[0].part, "The voice: hearing, turns and the ring");
    assert_eq!(found[5].part, "The voice's own flags");
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
    assert!(words.ends_with("word…"), "cut at a whole word: {words}");
    // A cut inside a code span ends before it, so no backtick is left open
    // (Third's P3 on #4400).
    let spanned = format!(
        "| LAMP_SPAN | `on`; off. {}the file (`lamp-near-call --from the chair of the person`), and more | — | — | until |",
        "word ".repeat(22)
    );
    let page = format!(
        "## The other parts\n\n| Switch | Values | a | b | Counted as |\n|---|---|---|---|---|\n{spanned}\n"
    );
    let words = &waiting(&page, &BTreeMap::new())[0].words;
    assert_eq!(words.matches('`').count() % 2, 0, "{words}");
    assert!(words.ends_with("the file…"), "{words}");
}

#[test]
fn the_part_says_how_many_and_folds_the_list_by_table() {
    let found = waiting(PAGE, &turned_on(&product()));
    let part = section(&found);
    assert!(part.starts_with(&format!(
        "{HEADING}\n6 features are in this build but off on every lamp"
    )));
    assert!(part.contains("<details><summary>The 6, by part</summary>"));
    // Once in the plain list and once in the developers' fold.
    assert_eq!(
        part.matches("**The voice: hearing, turns and the ring**")
            .count(),
        2
    );
    assert!(part.contains("- `--flag-off`: Not passed."));
    assert!(part.trim_end().ends_with("</details>"));
    assert_eq!(part.matches("<details>").count(), 2, "{part}");
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
        plain: None,
        product: None,
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

/// Second on #3976 (30 Oct 02:0x): a switch on by its code's default, which
/// no product file sets, says so in its "Product unit" cell as exactly
/// [`ON_UNSET`] and is not listed. Any other "(unset)" cell is read as the
/// files say.
#[test]
fn a_switch_on_by_its_codes_default_is_not_listed() {
    let found = waiting(PAGE, &turned_on(&product()));
    assert!(!found.iter().any(|w| w.switch == "LAMP_BY_DEFAULT"));
    let off = found.iter().find(|w| w.switch == "LAMP_OFF_UNSET").unwrap();
    assert_eq!(off.product.as_deref(), Some("off (unset)"));
    // Only the exact cell: another wording is listed as the files say.
    let page = format!(
        "## The other parts\n\n| Switch | Values | Product unit | Counted as |\n|---|---|---|---|\n\
         | LAMP_A | `on`. A | {ON_UNSET} | until |\n\
         | LAMP_B | `on`. B | on (unset, #1) | until |\n"
    );
    let listed: Vec<String> = waiting(&page, &BTreeMap::new())
        .into_iter()
        .map(|w| w.switch)
        .collect();
    assert_eq!(listed, ["LAMP_B"]);
}

/// The first builder on #3976 (29 Oct 23:4x): the list says what a person
/// would notice, from the page's plain-words table, with no switch name; the
/// name and the row's words are folded under "For developers". A switch with
/// no plain line is listed in its row's words.
#[test]
fn the_list_is_in_plain_words_and_the_names_and_rows_are_for_developers() {
    assert_eq!(
        plain_lines(PAGE).get("LAMP_WAITS").map(String::as_str),
        Some("The lamp answers you when you wait for it.")
    );
    let found = waiting(PAGE, &turned_on(&product()));
    assert_eq!(
        found[0].plain.as_deref(),
        Some("The lamp answers you when you wait for it.")
    );
    assert_eq!(found[1].plain, None, "LAMP_SOON has no plain line");
    let part = section(&found);
    let (people, developers) = part
        .split_once("<details><summary>For developers")
        .expect("a developers fold");
    assert!(people.contains("- The lamp answers you when you wait for it.\n"));
    assert!(!people.contains("LAMP_WAITS"), "no switch name for people");
    assert!(
        people.contains("- Proposed now.\n"),
        "the row's words without one"
    );
    assert!(developers.contains("- `LAMP_WAITS`: Answers a person who waits.\n"));
    assert!(developers.contains("- `LAMP_SOON`: Proposed now.\n"));
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
        assert_eq!(
            listed.words.matches('`').count() % 2,
            0,
            "{}: {}",
            listed.switch,
            listed.words
        );
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

/// The real page: `LAMP_FORGET_ASKED`, on unless set to `off`, is not listed
/// (Fourth on #4413), and no listed switch's "Product unit" cell says it is
/// on unset in other words, so a second on-by-default switch can't slip in
/// (Second on #3976, 30 Oct 02:0x).
#[test]
fn on_this_repository_a_switch_on_by_default_is_not_listed() {
    let lamp_os = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../lampOS");
    let page = std::fs::read_to_string(lamp_os.join("docs/switches.md")).unwrap();
    let on = turned_on(&notes::product_files(&lamp_os).unwrap());
    let found = waiting(&page, &on);
    assert!(!found.iter().any(|w| w.switch == "LAMP_FORGET_ASKED"));
    for w in &found {
        if let Some(product) = w.product.as_deref().filter(|p| p.contains("(unset)")) {
            assert!(
                !product.starts_with("on"),
                "{} is listed, but its Product unit cell says {product:?}: write it as exactly {ON_UNSET:?} if it is on by default",
                w.switch
            );
        }
    }
}

/// The real page and units: a file in `deploy/product` that install never
/// places turns nothing on, so `LAMP_OWN_KEYS`, set only in the own-keys
/// drop-in a test lamp gets by hand, is listed as not on yet.
#[test]
fn on_this_repository_a_file_install_never_places_turns_nothing_on() {
    let lamp_os = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../lampOS");
    let page = std::fs::read_to_string(lamp_os.join("docs/switches.md")).unwrap();
    assert!(lamp_os.join("deploy/product/own-keys-voice.conf").is_file());
    let files = notes::product_files(&lamp_os).unwrap();
    for left_out in [
        "own-keys-voice.conf",
        "lamp-keeper-restart.socket",
        "lamp-setup-keys@.service",
    ] {
        assert!(!files.contains_key(left_out), "{left_out} is read");
    }
    assert!(files.contains_key("exit-note.conf") && files.contains_key("store.conf"));
    let on = turned_on(&files);
    assert!(!on.contains_key("LAMP_OWN_KEYS"));
    assert!(
        waiting(&page, &on)
            .iter()
            .any(|w| w.switch == "LAMP_OWN_KEYS")
    );
}

/// A file counts as placed only by the name install places it under: a hand
/// drop-in holding an installed unit's text word for word is not read
/// (Second's P3 on #4666).
#[test]
fn a_file_with_an_installed_text_under_another_name_is_not_read() {
    let lamp_os = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../lampOS");
    let voice = std::fs::read_to_string(lamp_os.join("deploy/product/lamp-voice.service")).unwrap();
    let copy = tempfile::tempdir().unwrap();
    let product = copy.path().join("deploy/product");
    std::fs::create_dir_all(product.join("settings")).unwrap();
    std::fs::write(product.join("lamp-voice.service"), &voice).unwrap();
    std::fs::write(product.join("hand-voice.conf"), &voice).unwrap();
    let files = notes::product_files(copy.path()).unwrap();
    assert!(files.contains_key("lamp-voice.service"));
    assert!(!files.contains_key("hand-voice.conf"));
}

/// The real page and units: no switch a product file turns on is counted
/// `until` or `now`, which say it waits off for a check, so the page's
/// counts of what waits are what's off (Reviewer-two's P3 on #4662).
#[test]
fn on_this_repository_no_switch_the_product_turns_on_is_counted_waiting() {
    let lamp_os = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../lampOS");
    let page = std::fs::read_to_string(lamp_os.join("docs/switches.md")).unwrap();
    let on = turned_on(&notes::product_files(&lamp_os).unwrap());
    let counted_waiting: Vec<String> = waiting(&page, &BTreeMap::new())
        .into_iter()
        .filter_map(|w| {
            on.get(&w.switch)
                .map(|file| format!("{} ({file})", w.switch))
        })
        .collect();
    assert!(
        counted_waiting.is_empty(),
        "counted `until` or `now` in docs/switches.md, but on in the product; count them `on`:\n{}",
        counted_waiting.join("\n")
    );
}

/// The real page and units: each row is counted as the product has it. A
/// row counted `off` is one no product file turns on, and one counted `on`
/// is turned on by a product file or on by its code's default
/// ([`ON_UNSET`]). On 31 Oct `LAMP_STALLS` was counted `off` with the
/// product unit turning it on since 17 Oct, and `LAMP_DECLINE_SILENT` `on`
/// with no unit setting it (#4735).
#[test]
fn on_this_repository_each_switch_is_counted_as_the_product_has_it() {
    let lamp_os = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../lampOS");
    let page = std::fs::read_to_string(lamp_os.join("docs/switches.md")).unwrap();
    let on = turned_on(&notes::product_files(&lamp_os).unwrap());
    let rows = notes::rows(&page);
    assert!(rows.len() > 300, "only {} rows read", rows.len());
    let wrong: Vec<String> = rows
        .iter()
        .filter_map(|row| match (row.counted.as_str(), on.get(&row.switch)) {
            ("off", Some(file)) => Some(format!("{}: counted `off`, but {file} turns it on", row.switch)),
            ("on", None) if row.product.as_deref() != Some(ON_UNSET) => Some(format!(
                "{}: counted `on`, but no product file turns it on and its Product unit cell isn't {ON_UNSET:?}",
                row.switch
            )),
            _ => None,
        })
        .collect();
    assert!(
        wrong.is_empty(),
        "docs/switches.md's Counted as, against the product files:\n{}",
        wrong.join("\n")
    );
}

/// The real page: every switch the notes list has a plain line in its
/// [`PLAIN`] table, one short sentence a person can read, with no switch
/// name, flag or code (the first builder on #3976, 29 Oct 23:4x).
#[test]
fn on_this_repository_each_listed_switch_has_a_plain_line() {
    let lamp_os = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../lampOS");
    let page = std::fs::read_to_string(lamp_os.join("docs/switches.md")).unwrap();
    assert!(page.contains(&format!("\n{PLAIN}\n")));
    // Each row of the table is exactly two cells: a `|` inside a line would
    // split it and keep only its start (Second's P3 on #4413).
    let table = page.split(&format!("\n{PLAIN}\n")).nth(1).unwrap();
    let table = table.split("\n## ").next().unwrap();
    for row in table.lines().filter(|line| line.starts_with('|')) {
        assert_eq!(
            row.trim().trim_matches('|').split('|').count(),
            2,
            "a row of {PLAIN} with other than two cells: {row}"
        );
    }
    let on = turned_on(&notes::product_files(&lamp_os).unwrap());
    let missing: Vec<String> = waiting(&page, &on)
        .into_iter()
        .filter_map(|w| match &w.plain {
            None => Some(format!("{}: no plain line", w.switch)),
            Some(line)
                if line.chars().count() > PLAIN_MOST
                    || !line.ends_with('.')
                    || line.contains('`')
                    || line.contains("LAMP_")
                    || line.contains(" --") =>
            {
                Some(format!("{}: {line}", w.switch))
            }
            Some(_) => None,
        })
        .collect();
    assert!(
        missing.is_empty(),
        "add or fix in docs/switches.md's {PLAIN}:\n{}",
        missing.join("\n")
    );
}
