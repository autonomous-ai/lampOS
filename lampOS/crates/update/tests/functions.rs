//! The functions a product lamp's voice tells each session of, with every
//! product skill installed. A session takes at most `lamp_gemini::MAX_TOOLS`
//! (32). Past it the bound cuts by a declared order, never a core skill's
//! function (the owner's rule, on #2614): on the chat route the search's
//! fetch, the time elsewhere, the skills' not a core skill's, then the
//! search; with the lamp's own and core skills' alone over, the voice says
//! so on an error line (the coordinator, 19 Oct: lamp-4ace sat at exactly
//! 32, and switching onboarding on there pushed one of the actions part's
//! out). So the
//! product's count is pinned here, worked out by each part's own code: the
//! voice's own from the product voice unit's environment, the actions
//! part's whole catalog (its sound, its light and Schedule's), and the tools
//! of `install::PRODUCT_SKILLS` as install writes them and `lamp-skills`
//! writes their tools (those of `install::PRODUCT_SKILLS_OFF` are off).
use lamp_update::install;
use std::process::Command;

const VOICE: &str = include_str!("../../../deploy/product/lamp-voice.service");
/// Set only for the run in a process of its own: the folder of the
/// catalogs it reads, there as the product's `LAMP_ACTIONS_DIR`.
const CATALOGS: &str = "LAMP_TEST_PRODUCT_CATALOGS";
/// The test run in a process of its own.
const VOICE_SIDE: &str = "the_product_voice_s_functions_in_a_process_of_their_own";
/// How that process says each function given, and each left out: found
/// anywhere in a line, as the test runner may begin the first with the
/// test's name.
const DECLARED: &str = "declared: ";
const LEFT_OUT: &str = "left out: ";
/// Each function a chat request of the product's voice route is given.
const CHAT: &str = "chat: ";

/// The lamp's own functions a product lamp's chat request offers now, on the
/// product's route (`LAMP_VOICE_ROUTE=second`, bound
/// `lamp_voice::second_provider::MAX_TOOLS`): the voice's own 19 (with
/// onboarding's two and memory's `forget_everything` since 25 Oct, and
/// reports' `reports_stop` and `reports_state` since #2862), the
/// actions part's 17 and the skills' 1 (Dance's `dance`), before the search
/// and the look. The test prints this split. A change to it
/// is a change to what every session is told: say in its pull request what
/// is left out past the bound, if anything.
const PRODUCT_FUNCTIONS: usize = 37;

/// Every `Environment=` of a voice unit's `[Service]`.
fn voice_environment(unit: &str) -> Vec<(String, String)> {
    let mut inside = false;
    let mut set = Vec::new();
    for line in unit.lines().map(str::trim) {
        if line.starts_with('[') {
            inside = line == "[Service]";
        } else if inside && let Some(pair) = line.strip_prefix("Environment=") {
            let (name, value) = pair.split_once('=').unwrap();
            set.push((name.to_owned(), value.to_owned()));
        }
    }
    set
}

/// The voice's side, run in a process of its own by
/// [`a_product_lamp_declares_every_function_it_has_within_the_bound`] with
/// the product voice unit's environment and no other `LAMP_` setting, as
/// the voice's provider worker works it out: its flags from that
/// environment, its memory started (the rules' functions are told only
/// then), a session's functions, and what the link would leave out of
/// them. Does nothing in any other run.
#[test]
fn the_product_voice_s_functions_in_a_process_of_their_own() {
    if std::env::var_os(CATALOGS).is_none() {
        return;
    }
    let flags = lamp_voice::tools::arguments_from_environment();
    let (settings, rest) = lamp_voice::tools::take(&flags).unwrap();
    assert!(rest.is_empty(), "{rest:?}");
    if let Some(socket) = &settings.memory {
        lamp_voice::remember::start(socket.clone(), None);
    }
    let (kept, left_out) = lamp_voice::tools::admitted(lamp_voice::tools::declared(&settings));
    for tool in kept {
        println!("{DECLARED}{}", tool.name);
    }
    for why in left_out {
        println!("{LEFT_OUT}{why}");
    }
    // The product's route (LAMP_VOICE_ROUTE=second): its chat requests add
    // the web search's functions, LAMP_SECOND_SEARCH=deepseek's two.
    let search =
        match std::env::var(lamp_voice::second_provider::worker::SEARCH_ENVIRONMENT).as_deref() {
            Ok("deepseek") => lamp_voice::second_provider::net::search::Search::deepseek("test")
                .unwrap()
                .declarations(),
            _ => Vec::new(),
        };
    // A spoken turn's look, as the worker offers it: with a full look and
    // the eyes' look file (#2905); the product sets both (#2791's product half).
    let look = lamp_voice::sight::full_look_file().is_some()
        && std::env::var_os(lamp_voice::sight::LOOK_ENVIRONMENT).is_some();
    for tool in lamp_voice::second_provider::worker::chat_tools(&settings, &search, look) {
        println!("{CHAT}{}", tool.name);
    }
}

/// What the product voice declares, run in a process of its own with the
/// product voice unit's environment and `extra` on top: the actions part's
/// whole catalog, the product skills' tools, and the names given and left
/// out by the link.
struct Product {
    executive: Vec<lamp_actions::Function>,
    skill_tools: Vec<String>,
    declared: Vec<String>,
    left_out: Vec<String>,
    chat: Vec<String>,
}

fn product_with(extra: &[(&str, &str)]) -> Product {
    let folder = tempfile::tempdir().unwrap();
    // The actions part's catalog with all it offers: sound and light, both
    // there, and Schedule's (the product's LAMP_SCHEDULE_TOOLS=on).
    let mut executive = lamp_actions::catalog(true, true);
    executive.extend(lamp_actions::schedule::catalog());
    std::fs::write(
        folder.path().join(lamp_voice::tools::CATALOG_FILE),
        lamp_actions::catalog_file(&executive),
    )
    .unwrap();
    // The skills' tools beside it, as `lamp-skills --tools-dir` writes them.
    // The skills as install writes them, so those it installs off are off.
    let installed = tempfile::tempdir().unwrap();
    for (path, text) in install::SETTINGS {
        if let Some(file) = path.strip_prefix("/var/lib/lampOS/skills/") {
            let at = installed.path().join(file);
            std::fs::create_dir_all(at.parent().unwrap()).unwrap();
            std::fs::write(at, text).unwrap();
        }
    }
    let skills: Vec<lamp_skills::Skill> = install::PRODUCT_SKILLS
        .iter()
        .map(|name| lamp_skills::load(&installed.path().join(name)).unwrap())
        // The product leaves LAMP_UNPLUG unset: mirror the runtime catalog
        // filter even though this fixture loads the installed files directly.
        .filter(|skill| !skill.prepares_unplug())
        .collect();
    let beside = folder.path().join(lamp_voice::tools::SKILLS_DIR);
    std::fs::create_dir(&beside).unwrap();
    std::fs::write(
        beside.join(lamp_voice::tools::CATALOG_FILE),
        lamp_skills::tools_file(&skills).to_string(),
    )
    .unwrap();

    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args([VOICE_SIDE, "--exact", "--nocapture", "--test-threads=1"]);
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("LAMP_") {
            command.env_remove(name);
        }
    }
    for (name, value) in voice_environment(VOICE) {
        if name == "LAMP_ACTIONS_DIR" {
            command.env(name, folder.path());
        } else {
            command.env(name, value);
        }
    }
    for (name, value) in extra {
        command.env(name, value);
    }
    let output = command.env(CATALOGS, folder.path()).output().unwrap();
    let said = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{said}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let found = |mark: &str| -> Vec<String> {
        said.lines()
            .filter_map(|line| line.split_once(mark).map(|(_, rest)| rest.to_owned()))
            .collect()
    };
    Product {
        executive,
        // A disabled skill's tool is never written (lamp_skills::tools_file).
        skill_tools: skills
            .iter()
            .filter(|skill| skill.enabled)
            .filter_map(|skill| skill.tool.clone())
            .collect(),
        declared: found(DECLARED),
        left_out: found(LEFT_OUT),
        chat: found(CHAT),
    }
}

/// Every function the actions part and the product skills offer reaches a
/// product lamp's chat requests, on the product's route, none left out by
/// order or by the link, and the lamp's own are [`PRODUCT_FUNCTIONS`],
/// within that route's bound. Live's 32 (a per-lamp drop-in, not the
/// product's route) no longer bounds them since #3430: there the cut takes
/// the last past 32 and says so on the setup line.
#[test]
fn a_product_lamp_declares_every_function_it_has_within_the_bound() {
    let Product {
        executive,
        skill_tools,
        chat,
        left_out,
        ..
    } = product_with(&[]);
    assert!(left_out.is_empty(), "left out by the link: {left_out:?}");
    for name in executive
        .iter()
        .map(|function| function.name)
        .chain(skill_tools.iter().map(String::as_str))
    {
        assert!(
            chat.iter().any(|told| told == name),
            "{name} is left out of a product lamp's chat requests, past {}: {chat:?}",
            lamp_voice::second_provider::MAX_TOOLS
        );
    }
    let routes_own = [
        lamp_voice::second_provider::net::search::NAME,
        lamp_voice::second_provider::net::search::FETCH,
        lamp_voice::second_provider::LOOK_CALL,
    ];
    let lamps: Vec<&String> = chat
        .iter()
        .filter(|name| !routes_own.contains(&name.as_str()))
        .collect();
    println!(
        "A product lamp's chat request offers {} of the lamp's own functions (of at most {} in all): the voice's own {}, the actions part's {}, the skills' {} ({})",
        lamps.len(),
        lamp_voice::second_provider::MAX_TOOLS,
        lamps.len() - executive.len() - skill_tools.len(),
        executive.len(),
        skill_tools.len(),
        skill_tools.join(", ")
    );
    assert!(chat.len() <= lamp_voice::second_provider::MAX_TOOLS);
    assert_eq!(lamps.len(), PRODUCT_FUNCTIONS, "{chat:?}");
    // The product's voice has `LAMP_REPORTS=on` (#2862): the person can stop
    // reports and ask about them on the product's route.
    for name in [
        lamp_voice::reports_stop::NAME,
        lamp_voice::reports_stop::STATE_NAME,
    ] {
        assert!(chat.iter().any(|told| told == name), "{name}: {chat:?}");
    }
}

/// A product lamp's chat request (lamp-0c89, 21 Oct: "Lamp. Please dance
/// for me." answered "I've no legs to dance with" with no call, 32 tools
/// sent): the 31 above and the search, and Dance's `dance` among them.
/// Within the second route's own bound
/// (`lamp_voice::second_provider::MAX_TOOLS`), not Live's, nothing is left
/// out: the search's fetch is there too (lamp-4ace, 22 Oct, where Live's
/// bound left out the search itself).
#[test]
fn a_product_chat_request_offers_dance_at_the_bound() {
    let Product {
        declared,
        chat,
        skill_tools,
        ..
    } = product_with(&[]);
    assert!(
        chat.len() <= lamp_voice::second_provider::MAX_TOOLS,
        "{chat:?}"
    );
    assert!(
        chat.iter()
            .any(|name| name == lamp_voice::second_provider::net::search::FETCH),
        "{chat:?}"
    );
    assert_eq!(skill_tools, ["dance"]);
    assert!(chat.iter().any(|name| name == "dance"), "{chat:?}");
    assert!(
        chat.iter()
            .any(|name| name == lamp_voice::second_provider::net::search::NAME),
        "{chat:?}"
    );
    // The product's full look (#2791's product half): a spoken turn's look.
    assert!(
        chat.iter()
            .any(|name| name == lamp_voice::second_provider::LOOK_CALL),
        "{chat:?}"
    );
    // A Live session's functions are all in the chat request too; the chat
    // request has the room Live's 32 doesn't (#3430).
    for name in &declared {
        assert!(chat.contains(name), "{name}: {chat:?}");
    }
}

/// The reviewer's P3 on #1906: with `LAMP_REPORTS=on` the product voice
/// declares its two reports functions too, `reports_stop` and
/// `reports_state`, before the actions part's and the skills'. Then the
/// voice's own 15, the actions part's 17 and Dance's are 33, and the chat
/// request's search makes 35. By the owner's rule (on #2614) Dance's, a
/// core skill's function, is never the one cut, and by the first builder's
/// on #2644 neither are the actions part's nor `reports_stop`: the chat
/// request leaves out the search's fetch, the time elsewhere and
/// `reports_state`, a Live session `reports_state` alone, with no error
/// line, and `dance` stays in both. Since the second route's own bound
/// (22 Oct) its chat request leaves none of them out; a Live session still
/// leaves out `reports_state`.
#[test]
fn with_reports_on_dance_stays_at_the_bound() {
    let Product {
        executive,
        skill_tools,
        declared,
        left_out,
        chat,
        ..
    } = product_with(&[(lamp_voice::reports_stop::ENVIRONMENT, "on")]);
    assert_eq!(skill_tools, ["dance"]);
    assert!(
        chat.len() > lamp_gemini::MAX_TOOLS && chat.len() <= lamp_voice::second_provider::MAX_TOOLS,
        "{chat:?}"
    );
    assert!(chat.iter().any(|told| told == "dance"), "{chat:?}");
    for kept in [
        lamp_voice::second_provider::net::search::FETCH,
        lamp_voice::reports_stop::STATE_NAME,
    ] {
        assert!(chat.iter().any(|told| told == kept), "{kept}: {chat:?}");
    }
    for name in [
        lamp_voice::reports_stop::NAME,
        lamp_voice::second_provider::net::search::NAME,
    ]
    .into_iter()
    .chain(executive.iter().map(|function| function.name))
    {
        assert!(chat.iter().any(|told| told == name), "{name}: {chat:?}");
    }

    assert!(left_out.is_empty(), "left out by the link: {left_out:?}");
    assert_eq!(declared.len(), lamp_gemini::MAX_TOOLS, "{declared:?}");
    assert!(declared.iter().any(|told| told == "dance"), "{declared:?}");
    assert!(
        !declared
            .iter()
            .any(|told| told == lamp_voice::reports_stop::STATE_NAME),
        "{declared:?}"
    );
    // On Live, a per-lamp drop-in, `reports_stop` and Dance's are never the
    // ones cut. Past 32 the actions part's last are taken out before the cut
    // there, and the error line names only what the cut took (here
    // `reports_state`): a Live limit, not the product's. The product's route
    // offers every one of them (above, #3430).
    assert!(
        declared
            .iter()
            .any(|told| told == lamp_voice::reports_stop::NAME),
        "{declared:?}"
    );
}

/// Read the actual generic unit through the voice's flag parser and
/// declaration path, in a child so no test shares its environment.
#[test]
fn the_generic_voice_declares_the_clock_and_receives_time_and_session_context() {
    const CHILD: &str = "LAMP_TEST_GENERIC_CLOCK";
    const TEST: &str = "the_generic_voice_declares_the_clock_and_receives_time_and_session_context";
    if std::env::var_os(CHILD).is_some() {
        let flags = lamp_voice::tools::arguments_from_environment();
        let (settings, rest) = lamp_voice::tools::take(&flags).unwrap();
        assert!(rest.is_empty(), "{rest:?}");
        assert!(settings.decline && settings.clock && settings.places);
        let declared = lamp_voice::tools::declared(&settings);
        let clock = declared
            .iter()
            .find(|tool| tool.name == lamp_voice::tools::THE_TIME)
            .expect("the generic voice offers no clock function");
        assert!(clock.parameters.as_ref().unwrap()["properties"]["place"].is_object());
        assert_eq!(
            lamp_voice::tools::route(&settings, lamp_voice::tools::THE_TIME),
            lamp_voice::tools::Route::Clock
        );
        // The real opening-note path either names the local time or says
        // the kernel clock is unset; neither needs a product-only file.
        let note = lamp_voice::local_time::told_here(settings.clock, settings.places)
            .expect("the generic voice has no time context");
        assert!(note.contains("The time"), "{note}");
        let kernel = std::env::var(lamp_voice::local_time::KERNEL_CLOCK_ENVIRONMENT).unwrap();
        assert!(lamp_voice::local_time::unset(Some(&kernel), Some(false)));
        let session = lamp_voice::session_fact::arguments_from_environment();
        let (enabled, rest) = lamp_voice::session_fact::take(&session);
        assert!(enabled && rest.is_empty());
        assert!(lamp_gemini::wire::encode_context(lamp_voice::session_fact::FACT).is_ok());
        return;
    }
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args([TEST, "--exact", "--nocapture", "--test-threads=1"]);
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("LAMP_") {
            command.env_remove(name);
        }
    }
    for (name, value) in voice_environment(include_str!("../../../deploy/units/lamp-voice.service"))
    {
        command.env(name, value);
    }
    let output = command.env(CHILD, "1").output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
