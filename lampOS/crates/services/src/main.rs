//! `lamp-services --enable [--store DIR] [--run DIR]`: the person's linked
//! services, on the admin socket (`<run>/admin.sock`, lamp-cloud only) and
//! the voice's tools folder (`<run>/tools/`, lamp-voice only). Without
//! `--enable` it opens no socket and exits: its unit passes it.
use lamp_services::{
    WRITE_ENVIRONMENT, mail::Gmail, net::Https, on, refresh::Backend, serve, store, tools,
};
use std::{
    fs, io,
    os::unix::{fs::PermissionsExt, net::UnixListener},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
};

const USAGE: &str = "usage: lamp-services --enable [--store DIR] [--run DIR]";

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
    let mut enable = false;
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
        say("off: started without --enable; no socket opened");
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
    let services = Arc::new(serve::Services::new(
        store::Store(store),
        Https,
        Gmail,
        backend,
        write_on,
    ));
    let tools_dir = run.join("tools");
    let opened = (|| -> io::Result<(UnixListener, UnixListener)> {
        fs::create_dir_all(&tools_dir)?;
        fs::set_permissions(&tools_dir, fs::Permissions::from_mode(0o755))?;
        let admin = bind(&run.join("admin.sock"))?;
        let calls = bind(&tools_dir.join(tools::CALL_SOCKET))?;
        tools::write(&tools_dir)?;
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
