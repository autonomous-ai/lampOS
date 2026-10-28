//! A part's unit file, as far as an update reads and changes it: the
//! program its `ExecStart=` starts, in the folder named by the build.
//!
//! A part is installed as `/opt/lampOS/<folder>/<sha256>/<binary>` and its
//! unit names that path (docs/joining.md, "Installing"). The 64 hex
//! characters are the build. Changing a part's build is writing another
//! build in their place, everywhere the unit names that folder: a part's
//! files beside its binary (the controls' settings) are named through the
//! same folder.
//!
//! A product part (docs/install.md) is started through a link instead:
//! `/opt/lampOS/<folder>/current/<binary>`, `current` naming the build's
//! folder. Changing its build is replacing the link; its unit stays as it is.

mod restart_order;
pub(crate) use restart_order::start_order;

/// Where every part is installed.
pub const INSTALLED: &str = "/opt/lampOS/";
/// The link a product part is started through.
pub const CURRENT: &str = "current";

/// The program a unit starts, when it is a part installed by build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    /// The folder of its builds: `/opt/lampOS/voice/`, or on lamp-4ace
    /// `/opt/lampOS/releases/` for the voice.
    pub builds: String,
    /// The build: 64 lowercase hex characters.
    pub build: String,
    /// The binary's name: `lamp-voice`.
    pub binary: String,
    /// Started through `current`, which names the build: a switch replaces
    /// the link and leaves the unit as it is.
    pub linked: bool,
}

impl Program {
    /// The build's folder, as the unit names it: `/opt/lampOS/voice/<sha256>/`.
    pub fn folder(&self) -> String {
        folder(&self.builds, &self.build)
    }
}

/// A build's folder: `<builds><build>/`.
pub fn folder(builds: &str, build: &str) -> String {
    format!("{builds}{build}/")
}

/// Whether a path is a build's folder: `/opt/lampOS/<folder>/<64 hex>/`.
pub fn is_build_folder(path: &str) -> bool {
    path.strip_prefix(INSTALLED)
        .and_then(|rest| rest.strip_suffix('/'))
        .and_then(|rest| rest.split_once('/'))
        .is_some_and(|(folder, build)| {
            crate::manifest::is_token(folder)
                && build.len() == 64
                && crate::manifest::is_lower_hex(build)
        })
}

/// The program of the unit's first `ExecStart=`, if it is
/// `/opt/lampOS/<one folder>/<64 hex>/lamp-<name>`. Anything else is a
/// unit this program does not update.
pub fn program(text: &str) -> Option<Program> {
    let line = text
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("ExecStart="))?;
    // systemd's prefixes: - @ : + ! say how to run it, not what.
    let command = line.trim_start_matches(['-', '@', ':', '+', '!']);
    let path = command.split_whitespace().next()?;
    let rest = path.strip_prefix(INSTALLED)?;
    let mut pieces = rest.split('/');
    let (folder, build, binary) = (pieces.next()?, pieces.next()?, pieces.next()?);
    if pieces.next().is_some()
        || !crate::manifest::is_token(folder)
        || build.len() != 64
        || !crate::manifest::is_lower_hex(build)
        || !crate::manifest::is_binary_name(binary)
    {
        return None;
    }
    Some(Program {
        builds: format!("{INSTALLED}{folder}/"),
        build: build.to_owned(),
        binary: binary.to_owned(),
        linked: false,
    })
}

/// The folder of its builds and its binary, when the unit's first
/// `ExecStart=` is `/opt/lampOS/<one folder>/current/lamp-<name>`.
pub fn linked(text: &str) -> Option<(String, String)> {
    let line = text
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("ExecStart="))?;
    let command = line.trim_start_matches(['-', '@', ':', '+', '!']);
    let rest = command.split_whitespace().next()?.strip_prefix(INSTALLED)?;
    let mut pieces = rest.split('/');
    let (folder, current, binary) = (pieces.next()?, pieces.next()?, pieces.next()?);
    (pieces.next().is_none()
        && current == CURRENT
        && crate::manifest::is_token(folder)
        && crate::manifest::is_binary_name(binary))
    .then(|| (format!("{INSTALLED}{folder}/"), binary.to_owned()))
}

/// The program of a linked unit on a lamp: the build is where its
/// `current` link points, which must be a build's name (64 hex).
pub fn linked_program(text: &str, root: &std::path::Path) -> Option<Program> {
    let (builds, binary) = linked(text)?;
    let link = root.join(builds.trim_start_matches('/')).join(CURRENT);
    let target = std::fs::read_link(link).ok()?;
    let build = target.to_str()?;
    (build.len() == 64 && crate::manifest::is_lower_hex(build)).then(|| Program {
        builds,
        build: build.to_owned(),
        binary,
        linked: true,
    })
}

/// A part's `current` link, as the lamp names it: `/opt/lampOS/voice/current`.
pub fn current(builds: &str) -> String {
    format!("{builds}{CURRENT}")
}

/// The unit with its program's folder at `build` instead: every place it
/// names the old build's folder.
pub fn with_build(text: &str, program: &Program, build: &str) -> String {
    text.replace(&program.folder(), &folder(&program.builds, build))
}

/// What a unit says once its comments and blank lines are left out: two
/// units that differ only there are the same unit to systemd.
fn meaning(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim_end)
        .filter(|line| {
            let line = line.trim_start();
            !line.is_empty() && !line.starts_with('#') && !line.starts_with(';')
        })
        .collect()
}

/// Whether two units would make systemd do the same.
/// The last value a unit's `Environment=` lines give `name`, as systemd
/// reads them: assignments split by spaces, each perhaps in double quotes.
pub fn environment(text: &str, name: &str) -> Option<String> {
    let mut found = None;
    for line in text.lines().map(str::trim) {
        let Some(assignments) = line.strip_prefix("Environment=") else {
            continue;
        };
        let mut word = String::new();
        let mut quoted = false;
        let mut words = Vec::new();
        for c in assignments.chars() {
            match c {
                '"' => quoted = !quoted,
                ' ' if !quoted => words.push(std::mem::take(&mut word)),
                _ => word.push(c),
            }
        }
        words.push(word);
        for word in words {
            if let Some(value) = word
                .strip_prefix(name)
                .and_then(|rest| rest.strip_prefix('='))
            {
                found = Some(value.to_owned());
            }
        }
    }
    found
}

pub fn same(a: &str, b: &str) -> bool {
    meaning(a) == meaning(b)
}

/// The lines only in `before` (`- `) and only in `after` (`+ `), in order,
/// for a person to read before an update is applied.
pub fn differences(before: &str, after: &str) -> Vec<String> {
    let (before, after) = (meaning(before), meaning(after));
    let mut lines: Vec<String> = before
        .iter()
        .filter(|line| !after.contains(line))
        .map(|line| format!("- {line}"))
        .collect();
    lines.extend(
        after
            .iter()
            .filter(|line| !before.contains(line))
            .map(|line| format!("+ {line}")),
    );
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    /// #2396: the generic voice unit starts the light ring's worker with the
    /// product's ceiling, so a lamp moved from V1 lights its ring.
    #[test]
    fn the_generic_voice_runs_the_ring_as_the_product_does() {
        let exec = |unit: &str| {
            unit.lines()
                .find_map(|line| line.strip_prefix("ExecStart="))
                .unwrap()
                .to_owned()
        };
        let product = exec(include_str!("../../../deploy/product/lamp-voice.service"));
        let generic = exec(include_str!("../../../deploy/units/lamp-voice.service"));
        let ceiling = |exec: &str| {
            exec.split_whitespace()
                .skip_while(|word| *word != "--ring-channel-ceiling")
                .nth(1)
                .map(str::to_owned)
        };
        assert!(ceiling(&product).is_some(), "{product}");
        assert_eq!(ceiling(&generic), ceiling(&product), "{generic}");
    }

    /// #2943: the generic voice unit names the recorded failure lines in the
    /// product's language, from the folder `lamp-keeper prepare` fills
    /// (`install::NOTICES_MOVED`), so a lamp moved from V1 says why it can't
    /// answer rather than nothing. Not the product's folder under
    /// `/opt/lampOS`: the generic package's packager takes every path there
    /// for one it carries, and it carries no notices.
    #[test]
    fn the_generic_voice_says_its_failures_as_the_product_does() {
        let exec = |unit: &str| {
            unit.lines()
                .find_map(|line| line.strip_prefix("ExecStart="))
                .unwrap()
                .to_owned()
        };
        let product = exec(include_str!("../../../deploy/product/lamp-voice.service"));
        let generic = exec(include_str!("../../../deploy/units/lamp-voice.service"));
        let value = |exec: &str, flag: &str| {
            exec.split_whitespace()
                .skip_while(|word| *word != flag)
                .nth(1)
                .map(str::to_owned)
        };
        let language = "--failure-notice-language";
        assert!(value(&product, language).is_some(), "{product}");
        assert_eq!(value(&generic, language), value(&product, language));
        let folder = "--failure-notice-dir";
        assert_eq!(
            value(&product, folder).as_deref(),
            Some(crate::install::NOTICES_PRODUCT)
        );
        assert_eq!(
            value(&generic, folder).as_deref(),
            Some(crate::install::NOTICES_MOVED),
            "{generic}"
        );
    }

    /// The product's "who it answers" switches, and the bus they read the
    /// name and the faces from: the generic voice unit carries each, so a
    /// lamp moved from V1 answers only when spoken to (Migrate on #2602,
    /// 22 Oct). Not `LAMP_NAMES`: it needs the eyes' who socket, which the
    /// generic eyes unit does not serve.
    const WHO_IT_ANSWERS: [&str; 13] = [
        "LAMP_BUS",
        "LAMP_BUS_AUDIO",
        "LAMP_ATTENTION",
        "LAMP_ROOM_FACTS",
        "LAMP_SPOKEN_TO",
        "LAMP_NAME_IN_PASSING",
        "LAMP_NAME_NOTE",
        "LAMP_NAME_NOTE_BUSY",
        "LAMP_CUT_KEEPS_NAME",
        "LAMP_SHORT_SOUNDS",
        "LAMP_FAINT_HOLD_MS",
        "LAMP_WAIT_KEEPS_ANSWER",
        "LAMP_BUSY_ENDS_SOONER",
    ];

    /// The clock and opening context need no per-lamp provider file: use
    /// the same local clock, zone database and session fact as the product.
    const CLOCK_AND_SESSION: [&str; 5] = [
        "LAMP_TOOLS",
        "LAMP_CLOCK",
        "LAMP_TIME_PLACES",
        "LAMP_KERNEL_CLOCK",
        "LAMP_SESSION_FACT",
    ];

    #[test]
    fn the_generic_voice_has_the_product_clock_and_session_context() {
        let product = include_str!("../../../deploy/product/lamp-voice.service");
        let generic = include_str!("../../../deploy/units/lamp-voice.service");
        for name in CLOCK_AND_SESSION {
            let value = environment(product, name);
            assert_eq!(value.as_deref(), Some("on"), "{name}");
            assert_eq!(environment(generic, name), value, "{name}");
        }
    }

    #[test]
    fn the_generic_voice_answers_whom_the_product_does() {
        let product = include_str!("../../../deploy/product/lamp-voice.service");
        let generic = include_str!("../../../deploy/units/lamp-voice.service");
        for name in WHO_IT_ANSWERS {
            let value = environment(product, name);
            assert!(value.is_some(), "the product voice unit sets no {name}");
            assert_eq!(environment(generic, name), value, "{name}");
        }
        // Every switch attention.md names that the product voice sets is in the group above, or
        // the clock group or here, outside the attention group: a new attention switch can't be
        // forgotten (the Reviewer-lead's P3 on #2883). Head touch goes on with the product's
        // own pins and its try (#1362); a moved lamp's controls carry no head lines yet.
        // Reports decide nothing about whom the voice answers: a held line naming reports
        // gets one round for `reports_stop`, the product's only (#2862).
        const NOT_THE_GROUP: [&str; 7] = [
            "LAMP_FIRST_OUTPUT",
            "LAMP_HEAD_TOUCH",
            "LAMP_PROVIDER_PATIENCE",
            "LAMP_REPORTS",
            "LAMP_SECOND_LOOK",
            "LAMP_THREAD_FILE",
            "LAMP_VOICE_ROUTE",
        ];
        let attention = include_str!("../../../docs/attention.md");
        for line in product.lines() {
            let Some(name) = line
                .strip_prefix("Environment=")
                .and_then(|rest| rest.split('=').next())
            else {
                continue;
            };
            let named = attention.match_indices(name).any(|(at, _)| {
                !attention[at + name.len()..]
                    .starts_with(|c: char| c == '_' || c.is_ascii_alphanumeric())
                    && !attention[..at].ends_with(|c: char| c == '_' || c.is_ascii_alphanumeric())
            });
            if named {
                assert!(
                    WHO_IT_ANSWERS.contains(&name)
                        || CLOCK_AND_SESSION.contains(&name)
                        || NOT_THE_GROUP.contains(&name),
                    "{name} is named in attention.md and set in the product voice: add it to the group or leave it out on purpose"
                );
            }
        }
        // The faces the name note and the spoken-to rule read reach the bus only when the eyes publish.
        let product_eyes = include_str!("../../../deploy/product/lamp-eyes.service");
        let generic_eyes = include_str!("../../../deploy/units/lamp-eyes.service");
        assert_eq!(environment(product_eyes, "LAMP_BUS").as_deref(), Some("on"));
        assert_eq!(environment(generic_eyes, "LAMP_BUS").as_deref(), Some("on"));
    }

    /// Setup's product units a lamp moved from V1 does not get yet, and why
    /// (#2937). Every other product setup unit has a generic twin.
    const SETUP_LEFT_OUT: [(&str, &str); 9] = [
        (
            "lampos-setup-done.path",
            "main's adapter starts the target itself once setup writes the marker (#2977)",
        ),
        (
            "lamp-setup-wifi.socket",
            "the app's Wi-Fi change: main's adapter takes no socket units yet",
        ),
        ("lamp-setup-wifi@.service", "the socket's template"),
        (
            "lamp-setup-wifi-restore.service",
            "the Wi-Fi change's restore at boot, before the network; under /run it would come too late",
        ),
        (
            "lamp-setup-change.socket",
            "the app's settings change: main's adapter takes no socket units yet",
        ),
        ("lamp-setup-change@.service", "the socket's template"),
        (
            "lamp-setup-keys.socket",
            "own keys after setup: main's adapter takes no socket units yet",
        ),
        ("lamp-setup-keys@.service", "the socket's template"),
        (
            "lamp-setup-reset.service",
            "no 5 s reset on a moved lamp: its line 100 may be a head pad (the first builder, 22:47 on #2975)",
        ),
    ];

    /// #2937: a lamp moved from V1 joins a new Wi-Fi as a product lamp does.
    /// Each of setup's product units has a generic twin that runs the same
    /// command under the same conditions and hardening, or is left out above
    /// with why (the reset, which a moved lamp doesn't get). The
    /// generic controls run without `--touch`, so no 5 s hold ever reaches
    /// the reset on a moved lamp: its line 100 may be a head pad, which idles
    /// high as the button does (the first builder on #2975, after El's P1).
    #[test]
    fn the_generic_package_sets_up_and_resets_as_the_product_does() {
        let deploy = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy");
        let mut product_setup: Vec<String> = std::fs::read_dir(deploy.join("product"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| name.starts_with("lamp-setup") || name.starts_with("lampos-setup"))
            .collect();
        product_setup.sort();
        assert!(product_setup.contains(&"lamp-setup.service".to_owned()));
        let lines = |text: &str, key: &str| -> Vec<String> {
            text.lines()
                .filter(|line| line.starts_with(key))
                .map(str::to_owned)
                .collect()
        };
        // The command from the program on, so the build folder is not compared.
        let command = |text: &str| -> Vec<String> {
            lines(text, "ExecStart=")
                .iter()
                .map(|line| line.split_once("/lamp-setup ").unwrap().1.to_owned())
                .collect()
        };
        for name in &product_setup {
            let generic = deploy.join("units").join(name);
            if let Some((_, why)) = SETUP_LEFT_OUT.iter().find(|(left, _)| left == name) {
                assert!(
                    !generic.exists(),
                    "{name} is left out ({why}) but has a twin"
                );
                continue;
            }
            let product = std::fs::read_to_string(deploy.join("product").join(name)).unwrap();
            let generic = std::fs::read_to_string(&generic)
                .unwrap_or_else(|_| panic!("{name}: no generic twin and not left out"));
            assert_eq!(command(&generic), command(&product), "{name}");
            for key in [
                "ConditionPathExists=",
                "User=",
                "WantedBy=",
                "PartOf=",
                "ExecStopPost=",
            ] {
                assert_eq!(lines(&generic, key), lines(&product, key), "{name}: {key}");
            }
            assert!(
                generic.contains("/opt/lampOS/setup/@SETUP_SHA256@/lamp-setup "),
                "{name}"
            );
            // Recovery moves the radio as root: the twin keeps the
            // product's hardening and runtime folder (#3020, Fourteen's P2
            // on #2977: a moved lamp's only way to a new Wi-Fi).
            for key in [
                "RuntimeDirectory=",
                "NoNewPrivileges=",
                "ProtectSystem=",
                "ReadWritePaths=",
                "ProtectHome=",
                "PrivateTmp=",
                "RestrictAddressFamilies=",
            ] {
                assert_eq!(lines(&generic, key), lines(&product, key), "{name}: {key}");
            }
        }
        for (left, _) in SETUP_LEFT_OUT {
            assert!(
                product_setup.iter().any(|name| name == left),
                "{left} is named left out but is no product unit"
            );
        }

        let flags = |text: &str| -> Vec<String> {
            let exec = lines(text, "ExecStart=").pop().unwrap();
            ["--touch", "--enable"]
                .into_iter()
                .filter(|flag| exec.split_whitespace().any(|word| word == *flag))
                .map(str::to_owned)
                .collect()
        };
        let product_controls =
            std::fs::read_to_string(deploy.join("product/lamp-controls.service")).unwrap();
        let generic_controls =
            std::fs::read_to_string(deploy.join("units/lamp-controls.service")).unwrap();
        assert_eq!(flags(&product_controls), ["--touch", "--enable"]);
        assert!(flags(&generic_controls).is_empty(), "{generic_controls}");
    }

    #[test]
    fn lamp_4aces_own_units_are_read() {
        // As they are in the repository: what the lamp runs.
        let voice = program(include_str!("../../../deploy/lamp-4ace/lamp-voice.service")).unwrap();
        assert_eq!(voice.builds, "/opt/lampOS/releases/");
        assert_eq!(voice.binary, "lamp-voice");
        let controls = program(include_str!(
            "../../../deploy/lamp-4ace/lamp-controls.service"
        ))
        .unwrap();
        assert_eq!(controls.builds, "/opt/lampOS/controls/");
        assert_eq!(controls.binary, "lamp-controls");
        // A template's placeholder is not a build.
        assert_eq!(
            program(include_str!("../../../deploy/units/lamp-voice.service")),
            None
        );
    }

    #[test]
    fn a_new_build_is_written_everywhere_the_folder_is_named() {
        let text = format!(
            "[Service]\nExecStart=/opt/lampOS/controls/{A}/lamp-controls serve --config /opt/lampOS/controls/{A}/controls.json\n"
        );
        let program = program(&text).unwrap();
        assert_eq!(program.build, A);
        let changed = with_build(&text, &program, B);
        assert_eq!(changed, text.replace(A, B));
        assert_eq!(super::program(&changed).unwrap().build, B);
        assert!(is_build_folder(&program.folder()));
        assert!(!is_build_folder("/opt/lampOS/"));
        assert!(!is_build_folder(&format!("/opt/lampOS/../{A}/")));
        assert!(!is_build_folder(&format!("/etc/{A}/")));
    }

    #[test]
    fn only_a_part_installed_by_build_is_a_program() {
        for text in [
            "ExecStart=/usr/bin/amixer set Speaker 80%\n".to_owned(),
            format!("ExecStart=/opt/lampOS/voice/{A}/bash\n"),
            format!("ExecStart=/opt/lampOS/voice/{}/lamp-voice\n", &A[..60]),
            format!("ExecStart=/opt/lampOS/a/b/{A}/lamp-voice\n"),
            "[Service]\n".to_owned(),
        ] {
            assert_eq!(program(&text), None, "{text}");
        }
        let prefixed = format!("ExecStart=-/opt/lampOS/voice/{A}/lamp-voice serve\n");
        assert_eq!(program(&prefixed).unwrap().binary, "lamp-voice");
    }

    #[test]
    fn comments_do_not_make_a_unit_another() {
        let a = "# one\n[Service]\nExecStart=/x\n\n";
        let b = "[Service]\n# two\nExecStart=/x\n";
        assert!(same(a, b));
        assert!(!same(a, "[Service]\nExecStart=/x --on\n"));
        assert_eq!(
            differences(a, "[Service]\nExecStart=/x --on\n"),
            vec!["- ExecStart=/x", "+ ExecStart=/x --on"]
        );
    }
}
