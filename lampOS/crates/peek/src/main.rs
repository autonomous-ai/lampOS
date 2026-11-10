//! `lamp-peek`: the read-only page on an office lamp (#2141, docs/peek.md).
//! Off unless `LAMP_PEEK=on` or `LAMP_PEEK=sessions-only`. Answers on `LAMP_PEEK_LISTEN` (127.0.0.1:8077
//! unless set); anywhere but this machine only with a token, from
//! `$CREDENTIALS_DIRECTORY/peek-token` (`LoadCredential=peek-token:...`).
use lamp_bus::Bus;
use lamp_peek::{
    DEFAULT_LISTEN, DEFAULT_SESSIONS, LISTEN, Mode, SESSIONS, SWITCH, TOKEN_CREDENTIAL,
    check_listen,
    serve::{self, Peek, Sources},
    units, watch,
};
use std::{
    env,
    net::SocketAddr,
    path::PathBuf,
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

/// The token from systemd's credentials, if it gave one.
fn token() -> Option<String> {
    let directory = env::var_os("CREDENTIALS_DIRECTORY")?;
    let text = std::fs::read_to_string(PathBuf::from(directory).join(TOKEN_CREDENTIAL)).ok()?;
    let token = text.trim();
    (!token.is_empty()).then(|| token.to_owned())
}

fn main() -> ExitCode {
    let Some(mode) = Mode::from_switch(env::var(SWITCH).ok().as_deref()) else {
        eprintln!("lamp-peek: off ({SWITCH} is neither on nor sessions-only)");
        return ExitCode::SUCCESS;
    };
    let listen = env::var(LISTEN).unwrap_or_else(|_| DEFAULT_LISTEN.to_owned());
    let Ok(listen) = listen.parse::<SocketAddr>() else {
        eprintln!("lamp-peek: {LISTEN}={listen} is not an address and port");
        return ExitCode::from(2);
    };
    let token = token();
    if let Err(why) = check_listen(listen, token.as_deref()) {
        eprintln!("lamp-peek: {why}");
        return ExitCode::from(2);
    }
    match run(mode, listen, token) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lamp-peek: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(mode: Mode, listen: SocketAddr, token: Option<String>) -> std::io::Result<()> {
    let sources = Sources::for_mode(mode, Bus::from_environment, || Arc::new(units::read))?;
    let stop = Arc::new(AtomicBool::new(false));
    let watching = match &sources {
        Sources::SessionsOnly => None,
        Sources::Full(full) => Some(
            thread::Builder::new()
                .name("lamp-peek-watch".into())
                .spawn({
                    let (bus, seen, stop) =
                        (full.bus.clone(), Arc::clone(&full.seen), Arc::clone(&stop));
                    move || watch::watch(&bus, &seen, &stop)
                })?,
        ),
    };
    let peek = Arc::new(Peek {
        sources,
        token,
        sessions: env::var_os(SESSIONS)
            .map_or_else(|| PathBuf::from(DEFAULT_SESSIONS), PathBuf::from),
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let served = runtime.block_on(async move {
        let mut stopped =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        let listener = tokio::net::TcpListener::bind(listen).await?;
        eprintln!("lamp-peek: read-only, on {listen}");
        axum::serve(listener, serve::router(peek))
            .with_graceful_shutdown(async move {
                tokio::select! {
                    _ = stopped.recv() => {}
                    _ = tokio::signal::ctrl_c() => {}
                }
                eprintln!("lamp-peek: asked to stop");
            })
            .await
    });
    stop.store(true, Ordering::Relaxed);
    let watched = watching.map_or(Ok(()), |watching| watching.join().unwrap_or(Ok(())));
    served.and(watched)
}
