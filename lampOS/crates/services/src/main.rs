//! `lamp-services [--enable] [--store DIR] [--run DIR]`: the person's linked
//! services, on the admin socket (`<run>/admin.sock`, lamp-cloud only) and
//! the voice's tools folder (`<run>/tools/`, lamp-voice only). Off unless
//! `LAMP_SERVICES=on` (a drop-in on its unit, as in lamp-cloud's and the
//! voice's) or `--enable`: off, it opens no socket and exits at once, so its
//! unit stays inactive. `lamp-services --pdf-text` is the service's own
//! child for one PDF's text (`mail::pdf`), never started by hand.
use lamp_services::{
    CALENDAR_EDITS_ENVIRONMENT, DRAFT_EDITS_ENVIRONMENT, ENVIRONMENT, SKILL_READS_ENVIRONMENT,
    WRITE_ENVIRONMENT,
    mail::{self, Gmail},
    net::Https,
    on,
    refresh::Backend,
    serve, skill_writes, store, tools,
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
    let services = Arc::new(
        serve::Services::new(store::Store(store), Https, Gmail, backend, write_on)
            .with_skill_reads(skill_reads)
            .with_skill_writes(skill_writes)
            .with_calendar_edits(calendar_edits)
            .with_draft_edits(draft_edits),
    );
    let tools_dir = run.join("tools");
    let opened = (|| -> io::Result<(UnixListener, UnixListener)> {
        fs::create_dir_all(&tools_dir)?;
        fs::set_permissions(&tools_dir, fs::Permissions::from_mode(0o755))?;
        let admin = bind(&run.join("admin.sock"))?;
        let calls = bind(&tools_dir.join(tools::CALL_SOCKET))?;
        tools::write_for(
            &tools_dir,
            tools::Edits {
                calendar: calendar_edits,
                drafts: draft_edits,
            },
        )?;
        Ok((admin, calls))
    })();
    let (admin, calls) = match opened {
        Ok(listeners) => listeners,
        Err(error) => {
            say(&format!("its sockets could not be opened: {error}"));
            return ExitCode::FAILURE;
        }
    };
    say(&format!("serving {}", run.display()));
    let admin_services = services.clone();
    std::thread::spawn(move || serve::serve(admin, admin_services, serve::Door::Admin));
    serve::serve(calls, services, serve::Door::Tools);
    ExitCode::FAILURE
}
