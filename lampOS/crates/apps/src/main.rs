//! `lamp-apps-manage asked`: one request line on standard input (the connection
//! systemd hands over), one answer line on standard output.
//! `lamp-apps-manage start-all`: the boot start. Both run only with `LAMP_STORE_APPS=on`
//! in their own environment (a list excepted).
use lamp_apps::{
    ask::{MOST_LINE, Parts, answer, answer_door, build_sequence},
    client::AdminYes,
    effects::RealEffects,
    launcher,
    layout::Layout,
    lock::Running,
    manage::start_all,
    peer::{NOT_AN_ASKER, READ_WITHIN, door_ok, peer_of, peer_ok, set_read_timeout},
    real::{SkillsFolder, uid_in},
    skills_reader::SkillsReader,
    sweep::sweep,
};
use serde_json::json;
use std::{io::BufRead, io::Read, process::ExitCode, time::Duration};

const SWITCH: &str = "LAMP_STORE_APPS";
const SKILLS: &str = "/var/lib/lampOS/skills";
/// lamp-update's state, where lamp-cloud reads the build from too.
const BUILD: &str = "/var/lib/lampos-update/state.json";

fn on() -> bool {
    std::env::var(SWITCH).as_deref() == Ok("on")
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let layout = Layout::product();
    let mut skills = SkillsFolder {
        skills: SKILLS.into(),
    };
    match args.first().map(String::as_str) {
        Some("asked") => {
            // Only lamp-cloud and the voice, always, and lamp-control for a paired computer's
            // pushed apps alone: by their cgroup and their user, before a byte is read. There is
            // no switch, flag or variable that skips this.
            let owner = std::fs::read_to_string("/etc/passwd")
                .ok()
                .and_then(|p| uid_in(&p, "lampos"))
                .unwrap_or(u32::MAX);
            let peer = peer_of(std::io::stdin());
            let asker = peer
                .as_ref()
                .is_ok_and(|peer| peer_ok(peer.uid, owner, &peer.cgroup));
            let door = peer
                .as_ref()
                .is_ok_and(|peer| door_ok(peer.uid, owner, &peer.cgroup));
            if !asker && !door {
                println!("{}", json!({"refused": NOT_AN_ASKER}));
                return ExitCode::FAILURE;
            }
            // A silent caller must not hold the shared lock for good: reads time out.
            if let Err(why) = set_read_timeout(std::io::stdin(), READ_WITHIN) {
                println!("{}", json!({"refused": format!("no read timeout: {why}")}));
                return ExitCode::FAILURE;
            }
            let Ok(_alive) = Running::shared(&layout.state, Duration::from_secs(4)) else {
                println!("{}", json!({"refused": lamp_apps::lock::BUSY}));
                return ExitCode::FAILURE;
            };
            let mut line = String::new();
            if std::io::stdin()
                .lock()
                .take(MOST_LINE)
                .read_line(&mut line)
                .is_err()
            {
                println!("{}", json!({"refused": "no request read"}));
                return ExitCode::FAILURE;
            }
            let build = std::fs::read_to_string(BUILD)
                .ok()
                .and_then(|t| build_sequence(&t));
            // The Store's keys as the product has them: the build's, else the factory's.
            let reader = SkillsReader {
                keys: lamp_skills::package::store_keys(
                    std::path::Path::new(SKILLS),
                    std::path::Path::new(lamp_skills::package::STORE_KEY),
                )
                .unwrap_or_default(),
                // An app's tools file (#8386): read only with LAMP_APP_TOOLS on.
                tools: lamp_skills::app_package::tools_on(),
            };
            let mut fx = RealEffects;
            let mut yes = AdminYes::product();
            let mut parts = Parts {
                layout: &layout,
                fx: &mut fx,
                skills: &mut skills,
                yes: &mut yes,
                reader: &reader,
                staged_owner: owner,
                build,
            };
            if on()
                && let Err(why) = launcher::ensure(&layout)
            {
                println!("{}", json!({"refused": format!("the launcher: {why}")}));
                return ExitCode::FAILURE;
            }
            // lamp-control asks only the door's verbs, and they are asked by nobody else.
            let said = if door {
                answer_door(on(), &line, &mut parts)
            } else {
                answer(on(), &line, &mut parts)
            };
            println!("{said}");
            ExitCode::SUCCESS
        }
        Some("folders") => {
            // The apps' three folders, empty (lamp-apps-folders.service): no argument, no file, no environment, no
            // socket and no switch. Anything after the name is refused before a thing is made.
            if args.len() != 1 {
                eprintln!("lamp-apps-manage: folders takes no argument");
                return ExitCode::from(2);
            }
            match lamp_apps::folders::make(&layout, lamp_apps::folders::give_to_lampos) {
                Ok(said) => {
                    for line in said {
                        eprintln!("lamp-apps-manage: folders: {line}");
                    }
                    ExitCode::SUCCESS
                }
                Err(why) => {
                    eprintln!("lamp-apps-manage: folders: {why}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("run") => {
            // What an app's unit starts: the name checked, then the fixed launcher (exec: this program is gone).
            use std::os::unix::process::CommandExt as _;
            let [_, name] = args.as_slice() else {
                eprintln!("lamp-apps-manage: run takes exactly an app's name");
                return ExitCode::from(78);
            };
            let argv = match lamp_apps::run::argv(&layout, name) {
                Ok(argv) => argv,
                Err(why) => {
                    eprintln!("lamp-apps-manage: run: {why}");
                    return ExitCode::from(78);
                }
            };
            let error = std::process::Command::new(&argv[0]).args(&argv[1..]).exec();
            eprintln!("lamp-apps-manage: run: {}: {error}", argv[0]);
            ExitCode::from(78)
        }
        Some("start-all") => {
            if !on() {
                eprintln!("lamp-apps-manage: start-all: {SWITCH} is off");
                return ExitCode::SUCCESS;
            }
            if let Err(why) = launcher::ensure(&layout) {
                eprintln!("lamp-apps-manage: the launcher: {why}");
                return ExitCode::FAILURE;
            }
            let mut fx = RealEffects;
            // A cut remove ends with a word to lamp-control, which `Type=simple` may not have
            // bound yet at boot: wait for its socket (30 s at most) only when one is waiting.
            if lamp_apps::state::marked(&layout.state)
                .iter()
                .any(|(_, marks)| marks.contains(&lamp_apps::state::Mark::Removing))
                && !lamp_apps::client::wait_for_path(
                    std::path::Path::new(lamp_apps::client::ADMIN),
                    30,
                    || std::thread::sleep(std::time::Duration::from_secs(1)),
                )
            {
                eprintln!(
                    "lamp-apps-manage: start-all: lamp-control's admin socket did not appear in 30 s; a cut remove stays cut"
                );
            }
            // The sweep only with no other instance alive, and the lock is let go before the
            // boot start: it is held for the sweep alone.
            match Running::alone(&layout.state) {
                Ok(Some(alone)) => {
                    let report = sweep(&layout, &mut fx, &mut skills);
                    drop(alone);
                    for error in &report.errors {
                        eprintln!(
                            "lamp-apps-manage: sweep: {error} (a cut remove not finished here is resumed at the next boot, or when the owner asks for it again)"
                        );
                    }
                }
                Ok(None) => eprintln!(
                    "lamp-apps-manage: the sweep is skipped: another instance is alive (a cut install's mark stays until a later start)"
                ),
                Err(why) => eprintln!("lamp-apps-manage: the sweep is skipped: {why}"),
            }
            let (started, errors) = start_all(&layout, &mut fx);
            eprintln!("lamp-apps-manage: started {started:?}");
            for error in &errors {
                eprintln!("lamp-apps-manage: start-all: {error}");
            }
            // An app that will not start is logged and never fails this unit: a round that restarts it judges the
            // OS's own units, and one app (a template not yet placed, a crashing app) must not put an update back
            // (0c89, 20 Nov 16:04, #8115). The next boot, or the owner's Start, tries it again.
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("usage: lamp-apps-manage asked | start-all | folders | run NAME");
            ExitCode::from(2)
        }
    }
}
