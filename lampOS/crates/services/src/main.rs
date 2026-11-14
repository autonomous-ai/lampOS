//! `lamp-services [--enable] [--store DIR] [--run DIR]`: the person's linked
//! services, on the admin socket (`<run>/admin.sock`, lamp-cloud only) and
//! the voice's tools folder (`<run>/tools/`, lamp-voice only). Off unless
//! `LAMP_SERVICES=on` (a drop-in on its unit, as in lamp-cloud's and the
//! voice's) or `--enable`: off, it opens no socket and exits at once, so its
//! unit stays inactive. `lamp-services --pdf-text` is the service's own
//! child for one PDF's text (`mail::pdf`), never started by hand.
//! `lamp-services mcp` is the lamp's agent's MCP server over stdio
//! (`mcp_server`, `LAMP_AGENT_SERVICES`), started by Hermes in a skill's
//! run: it holds no service token and reaches the services only through
//! lamp-control's control socket with the run's one-run token.
use lamp_services::{
    CALENDAR_EDITS_ENVIRONMENT, DRAFT_EDITS_ENVIRONMENT, ENVIRONMENT, LINKED_TOOLS_ENVIRONMENT,
    SKILL_READS_ENVIRONMENT, SKILL_TASKS_ENVIRONMENT, WRITE_ENVIRONMENT,
    mail::{self, Gmail},
    mcp_server,
    net::Https,
    on,
    refresh::Backend,
    serve, skill_secrets, skill_writes, store, tools,
};
use std::{
    fs, io,
    os::unix::{fs::PermissionsExt, net::UnixListener},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
};

const USAGE: &str =
    "usage: lamp-services [--enable] [--store DIR] [--run DIR] (LAMP_SERVICES=on is --enable)";

fn say(line: &str) {
    eprintln!("lamp-services: {line}");
}

/// A socket at `path`, a stale one removed first, then opened to its
/// group (`lampos`, the unit's): the peer check tells the asker apart.
fn bind(path: &Path) -> io::Result<UnixListener> {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o660))?;
    Ok(listener)
}

fn main() -> ExitCode {
    // A PDF attachment's parse, in a child of the service (`mail::pdf`):
    // nothing else of the service is started.
    if std::env::args().nth(1).as_deref() == Some(mail::pdf::CHILD_ARG) {
        return mail::pdf::child();
    }
    // The lamp's agent's MCP server, on standard input and output.
    if std::env::args().nth(1).as_deref() == Some(mcp_server::ARG) {
        if std::env::args().nth(2).is_some() {
            say(
                "usage: lamp-services mcp (LAMP_CONTROL_SOCKET and LAMP_CONTROL_TOKEN, a skill run's)",
            );
            return ExitCode::from(2);
        }
        let link = mcp_server::Link::from_environment();
        return match mcp_server::serve(&link, io::stdin().lock(), io::stdout().lock()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                say(&format!("mcp: {}", error.kind()));
                ExitCode::FAILURE
            }
        };
    }
    let mut enable = on(std::env::var(ENVIRONMENT).ok().as_deref());
    let mut store = PathBuf::from(store::DIR);
    let mut run = PathBuf::from("/run/lampos-services");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--enable" => enable = true,
            "--store" | "--run" => {
                let Some(value) = args.next() else {
                    say(USAGE);
                    return ExitCode::from(2);
                };
                if arg == "--store" {
                    store = value.into();
                } else {
                    run = value.into();
                }
            }
            _ => {
                say(USAGE);
                return ExitCode::from(2);
            }
        }
    }
    if !enable {
        say("off: LAMP_SERVICES=on or --enable starts it; no socket opened");
        return ExitCode::SUCCESS;
    }
    // The backend, for refresh only, as lamp-cloud reads it.
    let backend = std::env::var_os("CREDENTIALS_DIRECTORY")
        .map(PathBuf::from)
        .and_then(|credentials| Backend::load(&credentials));
    if backend.is_none() {
        say("no backend link: OAuth links are used until they expire, never renewed");
    }
    let write_on = on(std::env::var(WRITE_ENVIRONMENT).ok().as_deref());
    say(if write_on {
        "writes on: each sent only on a spoken send"
    } else {
        "writes off: reads only"
    });
    // A made skill's reads (#5288): lamp-skills may call, for service_read.
    let skill_reads = on(std::env::var(SKILL_READS_ENVIRONMENT).ok().as_deref());
    if skill_reads {
        say("a made skill's reads on: lamp-skills may read, never write");
    }
    // A made skill's approved write (#6066): lamp-skills may send it.
    let skill_writes = on(std::env::var(skill_writes::ENVIRONMENT).ok().as_deref());
    if skill_writes {
        say("a made skill's writes on: lamp-skills may send each write the owner said yes to");
    }
    // A skill's run on the lamp's Hermes (#6432): lamp-control may call, for
    // a read or a write prepared, under the run's grants.
    let skill_tasks = on(std::env::var(SKILL_TASKS_ENVIRONMENT).ok().as_deref());
    if skill_tasks {
        say(
            "a skill run's services on: lamp-control may read, or prepare a write for a spoken yes",
        );
    }
    // Changing and cancelling a calendar event by its id, a write too.
    let calendar_edits = write_on && on(std::env::var(CALENDAR_EDITS_ENVIRONMENT).ok().as_deref());
    if calendar_edits {
        say("calendar edits on: an event changed or cancelled only on a spoken send");
    }
    // Changing and deleting a saved Gmail draft by its id, a write too.
    let draft_edits = write_on && on(std::env::var(DRAFT_EDITS_ENVIRONMENT).ok().as_deref());
    if draft_edits {
        say("Gmail draft edits on: a saved draft changed or deleted only on a spoken yes");
    }
    // The voice's tools name only the linked services (#6218).
    let linked_tools = on(std::env::var(LINKED_TOOLS_ENVIRONMENT).ok().as_deref());
    if linked_tools {
        say("linked tools on: the voice is offered only the linked services");
    }
    // A made skill's fixed write sent by its run on the lamp's agent (13 Nov).
    let skill_write_agent = on(std::env::var(skill_writes::AGENT_ENVIRONMENT)
        .ok()
        .as_deref());
    if skill_write_agent {
        say(
            "a skill's fixed write through the lamp's agent on: its run sends it once, as the owner said yes to it",
        );
    }
    // An uploaded skill's keys given in the app (LAMP_SKILL_SECRETS):
    // kept here, and asked for by lamp-tasks for a run.
    let skill_secrets =
        skill_secrets::on(std::env::var(skill_secrets::ENVIRONMENT).ok().as_deref());
    if skill_secrets {
        say("a skill's keys on: kept from the app, given to its run by lamp-tasks alone");
    }
    let services = serve::Services::new(store::Store(store), Https, Gmail, backend, write_on)
        .with_skill_reads(skill_reads)
        .with_skill_writes(skill_writes)
        .with_skill_tasks(skill_tasks)
        .with_skill_write_agent(skill_write_agent)
        .with_calendar_edits(calendar_edits)
        .with_draft_edits(draft_edits)
        .with_skill_secrets(skill_secrets);
    let tools_dir = run.join("tools");
    let edits = tools::Edits {
        calendar: calendar_edits,
        drafts: draft_edits,
    };
    let opened = (|| -> io::Result<(UnixListener, UnixListener)> {
        fs::create_dir_all(&tools_dir)?;
        fs::set_permissions(&tools_dir, fs::Permissions::from_mode(0o755))?;
        let admin = bind(&run.join("admin.sock"))?;
        let calls = bind(&tools_dir.join(tools::CALL_SOCKET))?;
        tools::write_for(&tools_dir, edits)?;
        Ok((admin, calls))
    })();
    let (admin, calls) = match opened {
        Ok(listeners) => listeners,
        Err(error) => {
            say(&format!("its sockets could not be opened: {error}"));
            return ExitCode::FAILURE;
        }
    };
    // Over the whole catalog written above, once the folder is there.
    let services = Arc::new(if linked_tools {
        services.with_linked_tools(tools_dir.clone(), edits)
    } else {
        services
    });
    say(&format!("serving {}", run.display()));
    let admin_services = services.clone();
    std::thread::spawn(move || serve::serve(admin, admin_services, serve::Door::Admin));
    serve::serve(calls, services, serve::Door::Tools);
    ExitCode::FAILURE
}
