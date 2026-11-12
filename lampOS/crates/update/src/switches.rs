//! The product's switches, and a kept file that turns one off.
//!
//! A product switch is a variable a product unit sets `on` in its own
//! `[Service]` (`Environment=LAMP_CLOUD=on` in `lamp-cloud.service`): taken
//! from the units themselves, so a new switch is covered with no list to
//! keep. Install rewrites the units but keeps a unit's other drop-ins, and
//! a setting edited on the lamp (docs/install.md), so a drop-in from a
//! lamp's builder days with `Environment=LAMP_CLOUD=off`, or an edited
//! `EnvironmentFile=` saying so, leaves a part quietly off after an install
//! to stable. Install and `check-units` name each one ([`merged_off`]);
//! neither changes it.

/// What one file's `Environment=` lines (or an environment file's lines)
/// leave set: the last value of each variable, and whether an empty
/// `Environment=` cleared what came before it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Assignments {
    pub cleared: bool,
    pub set: Vec<(String, String)>,
}

impl Assignments {
    fn assign(&mut self, name: &str, value: &str) {
        self.set.retain(|(set, _)| set != name);
        self.set.push((name.to_owned(), value.to_owned()));
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.set
            .iter()
            .find(|(set, _)| set == name)
            .map(|(_, value)| value.as_str())
    }
}

/// A unit file's lines as systemd reads them: a line ending in `\` goes on
/// into the next, comments (`#`, `;`) are dropped, each with its section.
fn lines(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut section = String::new();
    let mut pending = String::new();
    for raw in text.lines() {
        let line = raw.trim();
        if pending.is_empty() && (line.starts_with('#') || line.starts_with(';')) {
            continue;
        }
        if let Some(head) = line.strip_suffix('\\') {
            pending.push_str(head);
            pending.push(' ');
            continue;
        }
        pending.push_str(line);
        let whole = std::mem::take(&mut pending);
        let whole = whole.trim();
        if whole.starts_with('[') && whole.ends_with(']') {
            section = whole[1..whole.len() - 1].to_owned();
        } else if !whole.is_empty() {
            out.push((section.clone(), whole.to_owned()));
        }
    }
    out
}

/// The values of every `KEY=` in `[Service]`, in order, each trimmed.
fn service_values<'a>(lines: &'a [(String, String)], key: &str) -> Vec<&'a str> {
    lines
        .iter()
        .filter(|(section, _)| section == "Service")
        .filter_map(|(_, line)| {
            let (name, value) = line.split_once('=')?;
            (name.trim() == key).then_some(value.trim())
        })
        .collect()
}

/// `value` split into words as systemd splits `Environment=`: on white
/// space outside quotes, with `"…"` and `'…'` unquoted anywhere in a word
/// and `\` taking the next character as it is.
fn words(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (_, '\\') => {
                if let Some(next) = chars.next() {
                    word.push(next);
                }
                started = true;
            }
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => word.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            (None, c) => {
                word.push(c);
                started = true;
            }
        }
    }
    if started {
        out.push(word);
    }
    out
}

/// What a unit file or drop-in's `[Service]` `Environment=` lines set.
pub fn environment(text: &str) -> Assignments {
    let mut found = Assignments::default();
    for value in service_values(&lines(text), "Environment") {
        if value.is_empty() {
            found = Assignments {
                cleared: true,
                set: Vec::new(),
            };
            continue;
        }
        for word in words(value) {
            if let Some((name, value)) = word.split_once('=') {
                found.assign(name, value);
            }
        }
    }
    found
}

/// The `EnvironmentFile=` paths a unit file or drop-in names, without the
/// `-` that lets one be missing, and whether an empty one cleared the
/// unit's list. A path with a `%` specifier is left out: it names a file
/// only systemd can work out.
pub fn environment_files(text: &str) -> (bool, Vec<String>) {
    let mut cleared = false;
    let mut paths = Vec::new();
    for value in service_values(&lines(text), "EnvironmentFile") {
        if value.is_empty() {
            cleared = true;
            paths.clear();
            continue;
        }
        let path = value.strip_prefix('-').unwrap_or(value).trim();
        if path.starts_with('/') && !path.contains('%') {
            paths.push(path.to_owned());
        }
    }
    (cleared, paths)
}

/// What an environment file sets: `KEY=VALUE` lines, `#` and `;` comments
/// dropped, a value in quotes unquoted.
pub fn environment_file(text: &str) -> Assignments {
    let mut found = Assignments::default();
    for line in text.lines().map(str::trim) {
        if line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        if let Some((name, value)) = line.split_once('=') {
            let value = value.trim();
            let value = words(value).concat();
            found.assign(name.trim(), &value);
        }
    }
    found
}

/// The product unit's switches: each variable its `[Service]` sets `on`.
pub fn switches(unit: &str) -> Vec<String> {
    environment(unit)
        .set
        .into_iter()
        .filter(|(_, value)| value == "on")
        .map(|(name, _)| name)
        .collect()
}

/// Which of `switches` end up off once every file that sets them is read in
/// systemd's order (`sources`: the unit's kept drop-ins in name order, then
/// its environment files, which win over `Environment=`), each with the file
/// that left it off and how (`turns LAMP_CLOUD off ("off")`). A drop-in's `off` that
/// a later drop-in or file sets back `on` is not said: the switch is on.
pub fn merged_off(switches: &[String], sources: &[(String, Assignments)]) -> Vec<(String, String)> {
    switches
        .iter()
        .filter_map(|name| {
            // The product unit sets every switch `on`: a switch no source
            // sets or clears stays on.
            let (value, by) = last(name, sources)?;
            let by = by.to_owned();
            match value {
                Some("on") => None,
                Some(value) => Some((by, format!("turns {name} off ({value:?})"))),
                None => Some((
                    by,
                    format!("turns {name} off (an empty Environment= clears it)"),
                )),
            }
        })
        .collect()
}

/// The last of `sources` that sets `name` or clears every variable: its
/// value (`None`: cleared) and the source.
fn last<'a>(
    name: &str,
    sources: &'a [(String, Assignments)],
) -> Option<(Option<&'a str>, &'a str)> {
    let mut found = None;
    for (what, assignments) in sources {
        if let Some(set) = assignments.get(name) {
            found = Some((Some(set), what.as_str()));
        } else if assignments.cleared {
            found = Some((None, what.as_str()));
        }
    }
    found
}

/// What `name` ends up as once every one of `sources` is read in systemd's
/// order: none when no source sets it or the last one clears it.
pub fn effective<'a>(name: &str, sources: &'a [(String, Assignments)]) -> Option<&'a str> {
    last(name, sources)?.0
}

/// The line said for `what` (`kept drop-in PATH`) turning a switch off.
pub fn said(what: &str, how: &str) -> String {
    format!("{what} {how}; the product sets it on")
}

/// One of lamp-update's own switches (#6607), as `LAMP_APP_ENGINES` and
/// `LAMP_SKILL_TASKS` are read: set in its environment (`env`), `on`
/// exactly and any other value off, so the environment always wins; not set
/// there at all, the switch file every build carries
/// (`manifest::UPDATE_ENV`, `/etc/lampOS/update.env`) under `root`, read by
/// `crate::models::file_says_on`'s rule; missing or garbled, off. No round
/// rewrites lamp-update's own unit, so a switch only in that unit never
/// reaches a lamp already in a home: every lamp-update switch is read here.
pub fn update_switch(root: &std::path::Path, switch: &str, env: Option<&std::ffi::OsStr>) -> bool {
    match env {
        Some(value) => value == "on",
        None => crate::models::lamp_file_says_on(root, crate::manifest::UPDATE_ENV, switch),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One file read alone: how it leaves each switch off.
    fn turned_off(switches: &[String], found: &Assignments) -> Vec<String> {
        merged_off(switches, &[("f".to_owned(), found.clone())])
            .into_iter()
            .map(|(_, how)| how)
            .collect()
    }

    #[test]
    fn a_later_drop_in_or_file_that_sets_it_back_on_leaves_it_on() {
        let switches = ["LAMP_CLOUD".to_owned()];
        let old = environment("[Service]\nEnvironment=LAMP_CLOUD=off\n");
        let fix = environment("[Service]\nEnvironment=LAMP_CLOUD=on\n");
        let file_on = environment_file("LAMP_CLOUD=on\n");
        let file_off = environment_file("LAMP_CLOUD=off\n");
        let cleared = environment("[Service]\nEnvironment=\n");
        let source = |what: &str, found: &Assignments| (what.to_owned(), found.clone());
        assert!(
            merged_off(&switches, &[source("10-old", &old), source("20-fix", &fix)]).is_empty()
        );
        assert!(
            merged_off(
                &switches,
                &[source("old", &old), source("store.env", &file_on)]
            )
            .is_empty()
        );
        assert_eq!(
            merged_off(&switches, &[source("10-fix", &fix), source("20-old", &old)]),
            [(
                "20-old".to_owned(),
                r#"turns LAMP_CLOUD off ("off")"#.to_owned()
            )]
        );
        assert_eq!(
            merged_off(
                &switches,
                &[source("fix", &fix), source("store.env", &file_off)]
            ),
            [(
                "store.env".to_owned(),
                r#"turns LAMP_CLOUD off ("off")"#.to_owned()
            )]
        );
        assert_eq!(
            merged_off(&switches, &[source("fix", &fix), source("clear", &cleared)]),
            [(
                "clear".to_owned(),
                "turns LAMP_CLOUD off (an empty Environment= clears it)".to_owned()
            )]
        );
        assert!(merged_off(&switches, &[]).is_empty(), "the unit's own on");
    }

    #[test]
    fn the_product_switches_are_its_on_lines_and_lamp_cloud_is_one() {
        let (_, cloud) = crate::install::PRODUCT
            .iter()
            .find(|(name, _)| *name == "lamp-cloud.service")
            .unwrap();
        let found = switches(cloud);
        assert!(found.iter().any(|name| name == "LAMP_CLOUD"), "{found:?}");
        // On in the product since 2 Nov: a switch install guards.
        assert!(found.iter().any(|name| name == "LAMP_SKILLS_STORE"));
        // On in the product's eyes: whether anyone is at the lamp, a voice
        // counted only when said to it.
        let (_, eyes) = crate::install::PRODUCT
            .iter()
            .find(|(name, _)| *name == "lamp-eyes.service")
            .unwrap();
        assert!(
            switches(eyes).iter().any(|found| found == "LAMP_PRESENCE"),
            "{eyes}"
        );
        assert!(
            eyes.lines()
                .any(|line| line == "Environment=LAMP_PRESENCE_VOICE=addressed")
        );
        // On in the product: a made skill sent to the Store from the app.
        assert!(
            found.iter().any(|name| name == "LAMP_SKILLS_SHARE"),
            "{found:?}"
        );
        // On in the product's voice: a skill made by the lamp's own model.
        let (_, voice) = crate::install::PRODUCT
            .iter()
            .find(|(name, _)| *name == "lamp-voice.service")
            .unwrap();
        let voice = switches(voice);
        assert!(voice.iter().any(|name| name == "LAMP_MAKE_SKILL"));
        // On in the product's voice: only faces at the desk are people at
        // the lamp (the a0ae name-forget report, 2 Nov).
        assert!(voice.iter().any(|name| name == "LAMP_FACES_AT_DESK_ONLY"));
        // A commented example line is no switch.
        let (_, addons) = crate::install::PRODUCT
            .iter()
            .find(|(name, _)| *name == "lamp-addons.service")
            .unwrap();
        assert!(!switches(addons).iter().any(|name| name == "LAMP_ADDONS"));
        // Linked services (Queue #4710): on in all three units, and sends
        // on in lamp-services', a switch install guards; the unit passes no
        // `--enable`, so a drop-in's `LAMP_SERVICES=off` still stops it.
        let (_, services) = crate::install::PRODUCT
            .iter()
            .find(|(name, _)| *name == "lamp-services.service")
            .unwrap();
        let services_switches = switches(services);
        assert!(services_switches.iter().any(|name| name == "LAMP_SERVICES"));
        assert!(
            services_switches
                .iter()
                .any(|name| name == "LAMP_SERVICES_WRITE")
        );
        for unit in ["lamp-cloud.service", "lamp-voice.service"] {
            let (_, text) = crate::install::PRODUCT
                .iter()
                .find(|(name, _)| *name == unit)
                .unwrap();
            assert!(
                switches(text).iter().any(|name| name == "LAMP_SERVICES"),
                "{unit}"
            );
        }
        assert!(
            services
                .lines()
                .filter(|line| line.starts_with("ExecStart="))
                .all(|line| !line.contains("--enable")),
            "{services}"
        );
    }

    #[test]
    fn several_on_a_line_quoted_continued_and_cleared_are_read_as_systemd_does() {
        let text = "[Unit]\nEnvironment=LAMP_UNIT=off\n[Service]\n\
                    Environment=LAMP_A=on \"LAMP_CLOUD=off\" LAMP_B='x y'\n\
                    Environment=LAMP_C=\"o\"n \\\n  LAMP_D=off\n# Environment=LAMP_E=off\n";
        let found = environment(text);
        assert_eq!(found.get("LAMP_CLOUD"), Some("off"));
        assert_eq!(found.get("LAMP_B"), Some("x y"));
        assert_eq!(found.get("LAMP_C"), Some("on"));
        assert_eq!(found.get("LAMP_D"), Some("off"));
        assert_eq!(found.get("LAMP_E"), None);
        assert_eq!(found.get("LAMP_UNIT"), None, "only [Service] sets it");
        let switches = ["LAMP_CLOUD", "LAMP_C", "LAMP_E"].map(str::to_owned);
        assert_eq!(
            turned_off(&switches, &found),
            [r#"turns LAMP_CLOUD off ("off")"#]
        );
        let cleared =
            environment("[Service]\nEnvironment=LAMP_C=on\nEnvironment=\nEnvironment=LAMP_E=on\n");
        assert_eq!(
            turned_off(&switches, &cleared),
            [
                "turns LAMP_CLOUD off (an empty Environment= clears it)",
                "turns LAMP_C off (an empty Environment= clears it)"
            ]
        );
        // The last of one file wins; set back on, it is not said.
        let back = environment("[Service]\nEnvironment=LAMP_CLOUD=\nEnvironment=LAMP_CLOUD=on\n");
        assert!(turned_off(&switches, &back).is_empty());
        let empty = environment("[Service]\nEnvironment=LAMP_CLOUD=\n");
        assert_eq!(
            turned_off(&switches, &empty),
            [r#"turns LAMP_CLOUD off ("")"#]
        );
    }

    #[test]
    fn environment_files_and_their_lines() {
        let (cleared, paths) = environment_files(
            "[Service]\nEnvironmentFile=-/etc/a.env\nEnvironmentFile=/etc/%i.env\nEnvironmentFile=/etc/b.env\n",
        );
        assert!(!cleared);
        assert_eq!(paths, ["/etc/a.env", "/etc/b.env"]);
        let (cleared, paths) =
            environment_files("[Service]\nEnvironmentFile=/etc/a.env\nEnvironmentFile=\n");
        assert!(cleared && paths.is_empty());
        let found = environment_file("# LAMP_X=off\nLAMP_CLOUD = \"off\"\nexport LAMP_Y=on\n");
        assert_eq!(found.get("LAMP_CLOUD"), Some("off"));
        assert_eq!(found.get("LAMP_Y"), Some("on"));
        assert_eq!(found.get("LAMP_X"), None);
    }

    /// #6607: a lamp whose lamp-update unit lacks `LAMP_APP_ENGINES` and
    /// `LAMP_SKILL_TASKS` (c0a4's, dated 5 Nov) takes both from the carried
    /// file the product ships; the environment set wins; no file is off.
    #[test]
    fn lamp_updates_switches_come_from_the_carried_file_the_environment_winning() {
        use std::ffi::OsStr;
        let product = include_bytes!("../../../deploy/product/settings/update.env");
        let engines = crate::engines::SWITCH;
        let tasks = crate::hermes_install::TASKS_SWITCH;
        assert!(crate::models::file_says_on(product, engines));
        assert!(crate::models::file_says_on(product, tasks));

        let root = tempfile::tempdir().unwrap();
        // No file, nothing set: off.
        assert!(!update_switch(root.path(), engines, None));
        assert!(!update_switch(root.path(), tasks, None));
        // Set in the environment, it decides with no file.
        assert!(update_switch(root.path(), engines, Some(OsStr::new("on"))));

        let at = root
            .path()
            .join(crate::manifest::UPDATE_ENV.trim_start_matches('/'));
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(&at, product).unwrap();
        // The unit lacks the lines: the carried file turns both on.
        assert!(update_switch(root.path(), engines, None));
        assert!(update_switch(root.path(), tasks, None));
        // The environment set always wins over the file.
        assert!(!update_switch(
            root.path(),
            engines,
            Some(OsStr::new("off"))
        ));
        assert!(!update_switch(root.path(), tasks, Some(OsStr::new(""))));
        // A switch the file doesn't name stays off.
        assert!(!update_switch(root.path(), "LAMP_NOT_A_SWITCH", None));
        // The file saying off, or garbled: off.
        std::fs::write(&at, b"LAMP_APP_ENGINES=off\nLAMP_SKILL_TASKS=on\n").unwrap();
        assert!(!update_switch(root.path(), engines, None));
        assert!(update_switch(root.path(), tasks, None));
        std::fs::write(&at, b"LAMP_APP_ENGINES=on\ngarbage\n").unwrap();
        assert!(!update_switch(root.path(), engines, None));
    }
}
