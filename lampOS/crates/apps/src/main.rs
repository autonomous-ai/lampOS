//! `lamp-apps asked`: one request line on standard input (the connection
//! systemd hands over), one answer line on standard output.
//! `lamp-apps start-all`: the boot start. Both run only with `LAMP_STORE_APPS=on`
//! in their own environment (a list excepted).
use lamp_apps::{
    ask::{MOST_LINE, Parts, answer, build_sequence},
    client::AdminYes,
    effects::RealEffects,
    layout::Layout,
    lock::Running,
    manage::start_all,
    peer::{NOT_AN_ASKER, READ_WITHIN, peer_of, peer_ok, set_read_timeout},
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
            // Only lamp-cloud and the voice, always: by their cgroup and their user, before a
            // byte is read. There is no switch, flag or variable that skips this.
            let owner = std::fs::read_to_string("/etc/passwd")
                .ok()
                .and_then(|p| uid_in(&p, "lampos"))
                .unwrap_or(u32::MAX);
            let allowed =
                peer_of(std::io::stdin()).is_ok_and(|peer| peer_ok(peer.uid, owner, &peer.cgroup));
            if !allowed {
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
            println!("{}", answer(on(), &line, &mut parts));
            ExitCode::SUCCESS
        }
        Some("start-all") => {
            if !on() {
                eprintln!("lamp-apps: start-all: {SWITCH} is off");
                return ExitCode::SUCCESS;
            }
            let mut fx = RealEffects;
            // The sweep only with no other instance alive, and the lock is let go before the
            // boot start: it is held for the sweep alone.
            match Running::alone(&layout.state) {
                Ok(Some(alone)) => {
                    let report = sweep(&layout, &mut fx, &mut skills);
                    drop(alone);
                    for error in &report.errors {
                        eprintln!("lamp-apps-manage: sweep: {error}");
                    }
                }
                Ok(None) => eprintln!(
                    "lamp-apps-manage: the sweep is skipped: another instance is alive (a cut install's mark stays until a later start)"
                ),
                Err(why) => eprintln!("lamp-apps-manage: the sweep is skipped: {why}"),
            }
            let (started, errors) = start_all(&layout, &mut fx);
            eprintln!("lamp-apps: started {started:?}");
            for error in &errors {
                eprintln!("lamp-apps: start-all: {error}");
            }
            if errors.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        _ => {
            eprintln!("usage: lamp-apps asked | start-all");
            ExitCode::from(2)
        }
    }
}
