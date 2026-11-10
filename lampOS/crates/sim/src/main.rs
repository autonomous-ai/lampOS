//! `lamp-sim`: the virtual desktop lamp's hardware edge (docs/simulator-lamp.md).
//!
//! lamp-sim run DIR [--bin-dir BIN] [--listen ADDR] [--camera=always] [--microphone]
//!     Starts everything in DIR (short, private, outside a world-writable
//!     folder): the simulated servo bus, the real lamp-body on it, the ring,
//!     the voice and the volume, and the real lamp-control; prints a pairing
//!     code on demand with `lamp-sim code DIR`.
use lamp_sim::{
    camera::Camera, mic, people::People, pty::Port, ring::Ring, servo::Bus, stimuli::Stimuli, voice,
};
use std::{
    os::unix::{fs::symlink, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command},
    thread,
    time::Duration,
};

const USAGE: &str = "usage: lamp-sim people N DIR | hear TEXT DIR | hand DIR | air KEY=VALUE... DIR (a person in front of the lamp, by hand) | lamp-sim run DIR [--bin-dir BIN] [--listen ADDR] [--camera=always] [--microphone] | lamp-sim code DIR | lamp-sim servo DIR";

fn option(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|k| args.get(k + 1))
        .cloned()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(done) = stimulus(&args) {
        if let Err(error) = done {
            eprintln!("lamp-sim: {error}");
            std::process::exit(1);
        }
        return;
    }
    let dir = args.get(2).map(PathBuf::from);
    let result = match (args.get(1).map(String::as_str), dir) {
        (Some("servo"), Some(dir)) => servo(&dir),
        (Some("run"), Some(dir)) => run(&dir, &args),
        (Some("code"), Some(dir)) => code(&dir, &args),
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };
    if let Err(error) = result {
        eprintln!("lamp-sim: {error}");
        std::process::exit(1);
    }
}

/// `people N DIR`, `hear TEXT DIR`, `hand DIR`, `air KEY=VALUE... DIR`: what a
/// person in front of the lamp does, written as a file in the lamp's folder
/// for the running lamp to take. None if `args` is not one of these.
fn stimulus(args: &[String]) -> Option<std::io::Result<()>> {
    let (what, rest) = (args.get(1)?.as_str(), &args[2..]);
    let (file, text, dir) = match (what, rest) {
        ("people", [n, dir]) if n.parse::<u8>().is_ok() => ("people", n.clone(), dir),
        ("hear", [text, dir]) => ("hear", text.clone(), dir),
        ("hand", [dir]) => ("hand", "wave".to_owned(), dir),
        ("air", [words @ .., dir]) if !words.is_empty() => ("air", words.join(" "), dir),
        _ => return None,
    };
    if what == "air" && lamp_sim::stimuli::air_of(&text).is_none() {
        return Some(Err(std::io::Error::other(
            "air takes key=value words: pm1_0 pm2_5 pm4_0 pm10 humidity temperature co2_ppm",
        )));
    }
    Some(std::fs::write(Path::new(dir).join(file), text))
}

/// The servo bus alone, on a pty linked at `DIR/servo`.
fn servo(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut port = Port::open()?;
    let link = dir.join("servo");
    let _ = std::fs::remove_file(&link);
    symlink(&port.slave, &link)?;
    eprintln!(
        "lamp-sim: servo bus on {} ({})",
        link.display(),
        port.slave.display()
    );
    port.serve(&mut found_bus(), &link, || false)
}

/// Servos found folded at the counts of lamp-fc4b's measured home: base yaw,
/// shoulder, elbow, head roll, head pitch.
fn found_bus() -> Bus {
    Bus::new([2028, 2048, 1946, 2085, 1817], [(0, 4095); 5])
}

fn private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir.join("locks"))?;
    std::fs::create_dir_all(dir.join("state"))?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    std::fs::set_permissions(dir.join("locks"), std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn run(dir: &Path, args: &[String]) -> std::io::Result<()> {
    private_dir(dir)?;
    let bin = option(args, "--bin-dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(Path::to_path_buf))
                .unwrap_or_default()
        });
    let listen = option(args, "--listen").unwrap_or_else(|| "127.0.0.1:7731".to_owned());
    let bus_root = dir.join("bus");
    let _ = std::fs::remove_dir_all(&bus_root);
    // The servo bus, on its own thread.
    let mut port = Port::open()?;
    let link = dir.join("servo");
    let _ = std::fs::remove_file(&link);
    symlink(&port.slave, &link)?;
    let mut threads = vec![spawn_named("servo bus", move || {
        let _ = port.serve(&mut found_bus(), &link, || false);
    })];
    // The voice and the volume.
    let (say, act) = (dir.join("say.sock"), dir.join("act.sock"));
    {
        let say = say.clone();
        threads.push(spawn_named("say socket", move || {
            let _ = voice::serve_say(&say);
        }));
        let act = act.clone();
        threads.push(spawn_named("volume socket", move || {
            let _ = voice::serve_actions(&act);
        }));
    }
    // The ring, before lamp-control asks it anything.
    let bus = lamp_bus::Bus::new(&bus_root)?;
    // One boot id for the ring and lamp-control (macOS has no /proc).
    let clock_id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .map(|id| id.trim().to_owned())
        .ok()
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| {
            format!(
                "lamp-sim-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_nanos())
            )
        });
    let boot_id = clock_id.clone();
    let mut ring = Ring::on(&bus)?;
    let mut people = People::on(&bus, dir)?;
    let mut stimuli = Stimuli::on(&bus, dir)?;
    let view = dir.join("ring.json");
    let sight = dir.join("sight.jpg");
    let mut camera = Camera::new(&sight);
    // The host's camera opens only when asked for with `--camera=always`, and
    // then stays open for the lamp's whole life, as the eyes' does (the host's
    // camera light stays on). Each picture still goes only while the ring shows
    // the camera's mark. Opening only for a computer's mark needs lamp-control's
    // `--camera-opens-on-demand`, which comes separately.
    let camera_always = args.iter().any(|a| a == "--camera=always");
    if args.iter().any(|a| a == "--camera") {
        return Err(std::io::Error::other(
            "--camera needs lamp-control's --camera-opens-on-demand, not in this build: use --camera=always",
        ));
    }
    // The microphone the same way: only with --microphone. The ears' sound
    // still goes to a computer only while the ring shows its mark.
    let microphone_on = args.iter().any(|a| a == "--microphone");
    if microphone_on {
        let bus = lamp_bus::Bus::new(&bus_root)?;
        threads.push(spawn_named("microphone", move || {
            if let Err(error) = mic::run(&bus) {
                eprintln!("lamp-sim: the microphone ended: {error}");
            }
        }));
    }
    threads.push(spawn_named("ring", move || {
        let mut tick = 0u32;
        loop {
            if ring.step(&clock_id).is_err() {
                thread::sleep(Duration::from_millis(100));
            }
            camera.step(camera_always);
            if tick.is_multiple_of(5) {
                let _ = people.say();
            }
            let _ = stimuli.step();
            tick += 1;
            // Whole or not at all, for a reader (a viewer) that looks at any time.
            let temp = view.with_extension("json.tmp");
            if std::fs::write(&temp, ring.view().to_string()).is_ok() {
                let _ = std::fs::rename(&temp, &view);
            }
            thread::sleep(Duration::from_millis(40));
        }
    }));
    // The real body, then the real door.
    let mut children: Vec<Child> = Vec::new();
    let d = |name: &str| dir.join(name).display().to_string();
    let mut body = body_command(&bin, dir, &bus_root)?.spawn()?;
    thread::sleep(Duration::from_secs(1));
    children.push(
        watched(bin.join("lamp-control"))
            .env("LAMP_CONTROL", "on")
            .args([
                "serve",
                "--state-dir",
                &d("state"),
                "--listen",
                &listen,
                "--admin-socket",
                &d("a.sock"),
            ])
            .args([
                "--boot-id",
                &boot_id,
                "--name",
                "virtual",
                "--bus-root",
                &bus_root.display().to_string(),
            ])
            .args([
                "--mood-socket",
                &d("mood.sock"),
                "--lease-socket",
                &d("lease.sock"),
            ])
            .args([
                "--say-socket",
                &d("say.sock"),
                "--actions-socket",
                &d("act.sock"),
            ])
            .args(["--sight-file", &d("sight.jpg")])
            .args(["--microphone", if microphone_on { "on" } else { "off" }])
            .env("LAMP_BUS_AUDIO", "on")
            .process_group(0)
            .spawn()?,
    );
    eprintln!(
        "lamp-sim: up on {listen}; pair with: lamp-sim code {}",
        dir.display()
    );
    // The body is restarted when it ends (a busy laptop can make it miss a
    // servo reply and the real body then stops, as it should), with its why
    // said; more than five times in ten minutes, or the door or a part of lamp-sim
    // ending, ends the lamp.
    let mut restarts: Vec<std::time::Instant> = Vec::new();
    loop {
        thread::sleep(Duration::from_millis(500));
        let mut ended = false;
        if let Some(thread) = threads.iter().find(|t| t.is_finished()) {
            eprintln!(
                "lamp-sim: its {} thread ended (a panic, if nothing was said above)",
                thread.thread().name().unwrap_or("unnamed")
            );
            ended = true;
        }
        if let Ok(Some(status)) = body.try_wait() {
            eprintln!(
                "lamp-sim: lamp-body ended ({status}): {}",
                why_body_ended(dir)
            );
            restarts.retain(|at| at.elapsed() < Duration::from_secs(600));
            if restarts.len() >= 5 {
                eprintln!("lamp-sim: lamp-body ended five times in ten minutes: giving up");
                ended = true;
            } else {
                restarts.push(std::time::Instant::now());
                match body_command(&bin, dir, &bus_root).and_then(|mut c| c.spawn()) {
                    Ok(next) => {
                        body = next;
                        eprintln!("lamp-sim: lamp-body started again");
                    }
                    Err(error) => {
                        eprintln!("lamp-sim: lamp-body did not start again: {error}");
                        ended = true;
                    }
                }
            }
        }
        if children
            .iter_mut()
            .any(|c| matches!(c.try_wait(), Ok(Some(_))))
        {
            eprintln!("lamp-sim: lamp-control ended");
            ended = true;
        }
        if ended {
            // The parts run in their own process groups under a watcher shell:
            // end each group, not only the shell.
            children.push(body);
            for child in &mut children {
                if let Some(group) = rustix::process::Pid::from_raw(child.id() as i32) {
                    let _ =
                        rustix::process::kill_process_group(group, rustix::process::Signal::TERM);
                }
                let _ = child.wait();
            }
            return Err(std::io::Error::other("the lamp ended"));
        }
    }
}

/// The real `lamp-body` on the simulated servo bus; its exit reports go to
/// `DIR/body.log`, one after another.
fn body_command(bin: &Path, dir: &Path, bus_root: &Path) -> std::io::Result<Command> {
    let d = |name: &str| dir.join(name).display().to_string();
    let mut command = watched(bin.join("lamp-body"));
    command
        .env("LAMP_BUS", "on")
        .env("LAMP_BUS_ROOT", bus_root)
        .args([
            "--device",
            &d("servo"),
            "--unit-id",
            "lamp-sim",
            "--lock-directory",
            &d("locks"),
        ])
        .args([
            "--mood-socket",
            &d("mood.sock"),
            "--lease-socket",
            &d("lease.sock"),
        ])
        .args([
            "--reach",
            "150,15,1,250",
            "--speed",
            "300",
            "--servo-speed",
            "1000",
        ])
        .args([
            "--servo-acceleration",
            "60",
            "--stand-share",
            "100",
            "--stand-seconds",
            "6",
        ])
        .args([
            "--awake-seconds",
            "90",
            "--idle",
            "on",
            "--force-limit",
            "600",
        ])
        .args(["--home", "1817,2085,2028", "--folded-arm", "2048,1946"])
        .args([
            "--stand",
            "1772,2085,2028,2621,2558",
            "--pitch-window",
            "1740,2030",
            "--torque",
            "on",
        ]);
    command
        .stdout(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("body.log"))?,
        )
        .process_group(0);
    Ok(command)
}

/// Why the body ended, from the exit report it printed (`body.stop`), else
/// the last line it wrote.
fn why_body_ended(dir: &Path) -> String {
    let text = std::fs::read_to_string(dir.join("body.log")).unwrap_or_default();
    text.lines()
        .rev()
        .find_map(|line| {
            let report: serde_json::Value = serde_json::from_str(line).ok()?;
            Some(report["body"]["stop"].to_string())
        })
        .unwrap_or_else(|| "no exit report".to_owned())
}

/// A thread with a name, so that the lamp can say which one ended.
fn spawn_named(name: &str, work: impl FnOnce() + Send + 'static) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name(name.to_owned())
        .spawn(work)
        .expect("a thread")
}

/// A part of the lamp (`lamp-body`, `lamp-control`) under a shell that stops it
/// when lamp-sim ends, even killed: the parts never outlive their lamp.
fn watched(program: PathBuf) -> Command {
    let mut command = Command::new("sh");
    command
        .args([
            "-c",
            "\"$@\" & F=$!; \
             while kill -0 \"$OWNER\" 2>/dev/null && kill -0 $F 2>/dev/null; do sleep 1; done; \
             kill $F 2>/dev/null; wait $F 2>/dev/null",
            "lamp-part",
        ])
        .arg(program)
        .env("OWNER", std::process::id().to_string());
    command
}

/// A pairing code, from the running lamp-control's admin socket.
fn code(dir: &Path, args: &[String]) -> std::io::Result<()> {
    let bin = option(args, "--bin-dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(Path::to_path_buf))
                .unwrap_or_default()
        });
    let grants = option(args, "--grants").unwrap_or_else(|| "body,light,speak,look".to_owned());
    let status = watched(bin.join("lamp-control"))
        .args([
            "code",
            "--grants",
            &grants,
            "--admin-socket",
            &dir.join("a.sock").display().to_string(),
        ])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other("lamp-control did not give a code"))
    }
}
