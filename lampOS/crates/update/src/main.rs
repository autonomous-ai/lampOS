//! `lamp-update`: a lamp keeps its lampOS current by itself
//! (docs/updates.md). On the lamp: `check` says what the newest build of
//! its channel would change, `once` does one round, `serve` one round
//! every so often. Where builds are made: `manifest` and `sign`.
use lamp_update::{
    Config, Outcome, World,
    fetch::Web,
    install::{self, Install, System},
    judge::UnitState,
    manifest, own, publish, round,
    world::{BusEars, Deaf, Ears, Monotonic, Statvfs, Systemctl, Systemd},
};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    time::Duration,
};

const USAGE: &str = "\
lamp-update check|once|serve --channel-url URL [--channel NAME] [--key-file PATH] [--lamp NAME]
                             [--apply on|off] [--every-s N] [--quiet-wait-s N] [--watch-s N]
                             [--keep-free-mb N] [--root DIR] [--bus-root DIR]
lamp-update install --channel-url URL [--channel NAME] [--key-file PATH] [--setup on|off]
lamp-update harness-off
lamp-update harness-on
lamp-update back
lamp-update apply
lamp-update writer-probe
lamp-update writer-refresh
lamp-update check-units --cat FILE [--product DIR] [--image DIR [--variant NAME]]
lamp-update manifest --binaries DIR --deploy DIR --skills DIR --channel NAME --sequence N --commit SHA --files-url URL
                     [--min-updater N] [--models FILE] [--settings on|off] [--image DIR]
lamp-update sign --key-file PRIVATE --manifest FILE --out FILE
lamp-update public-key --key-file PRIVATE
lamp-update --help

check   says what the newest build of the channel would change on this lamp, and
        writes nothing (with any --apply).
once    one round: with --apply on, fetches what changed, installs it beside the old
        build, switches at a quiet moment (nobody heard or answered for 45 s, or the
        lamp asleep or muted), watches each part for --watch-s (60) and puts the
        previous build back if one is not well. Without --apply on (the default) it is check.
serve   a round every --every-s (900), until stopped.
install lampOS on a blank lamp (docs/install.md): the newest build's binary of every
        product part, in its folder with its current link; the product units, tmpfiles,
        users and zone; enables the lamp and starts nothing. Run again, it changes
        nothing. As root, on the lamp itself.
harness-off undoes install's Harness at boot, and only that: as lampos, the Harness CLI's
        `harness service uninstall` and `harness stop`, then `loginctl disable-linger
        lampos`, then install's record of it. Harness itself, its sign-in and the
        lampos user stay; what Harness still has is said, never stopped. Without the
        record it changes nothing. As root, on the lamp itself.
harness-on puts Harness on a lamp lampOS is installed on: without Harness at
        /usr/local/bin/harness, Harness's own installer run as lampos (HOME
        /var/lib/lampos-tasks) and linked there; then Harness at boot as install sets
        it; then lamp-tasks started again. Signing in is the person's. Run again, it
        changes nothing. As root, on the lamp itself.
back    lamp-update-back (installed as /usr/local/lib/lampos/lamp-update-back, started by
        lamp-update.service's failure and its timer): when lamp-update switched to its own
        new build (LAMP_UPDATE_SELF=on) and that build has not confirmed itself, and
        lamp-update is not running or 10 minutes have passed, update/current names the
        old build again, the new one is refused, and lamp-update is started again.
        Otherwise it changes nothing. As root, on the lamp itself.
apply   lamp-update-apply (lamp-update-back apply, started by a round with
        LAMP_UPDATE_APPLY=on): the request the round staged in its records, done. It
        writes the build's settings and tmpfiles, each only where the lamp still has the
        build's before, keeps a file as it was before its first write, and puts them back
        when asked. Without a request it changes nothing. As root, on the lamp itself.
writer-probe prints one line, `lamp-update writer-probe 1 <sha256>`, the SHA-256 of the
        file it runs from, and changes nothing: a copy is run so before it is put at a
        fixed path (#3689).
writer-refresh puts this lamp-update at the settings writer's fixed path, the copy
        there kept as lamp-update-back.prev, once a copy beside it answers writer-probe.
        Run as root by lamp-update-writer.service, which a round starts; by hand only
        with no round at work (#3689).

  --channel-url   the channel's folder, https://.../ or file:///.../, ending in /
  --channel       the channel followed (lamp-voice)
  --key-file      the lamp's public key, base64 (/etc/lampOS/update/key.pub)
  --setup         install: on also writes setup's two settings, so a lamp without
                  setup's marker starts setup at boot (off)
  --lamp          this lamp's name, which of the build's lamp folders is its own
                  (the host name in ROOT/etc/hostname)
  --quiet-wait-s  how long a round waits for a quiet moment (3600)
  --keep-free-mb  room kept free on the disk beyond the new build (256)
  --root          the filesystem the lamp's paths are under (/), for tests
  --bus-root      the bus (LAMP_BUS_ROOT, or /run/lampos-bus)

check-units  says whether a lamp runs exactly the product's units (docs/install.md):
          --cat is what `systemctl cat 'lamp-*.service' 'lamp-*.timer' 'lampos*'`
          printed on it, saved as a file; the product is this program's own units, or
          the `deploy/product/` folder given with --product. Each product unit is
          `same`, `missing` or its differing lines; every drop-in, every unit the
          product has no file for, and every unit not in /etc/systemd/system/ is said.
          The image's own units that updates carry are the image's, not extra: with
          --image (robots/lamp) each is compared with --variant's (standard), else said.
          It ends with 0 when everything is the same and nothing else is found, 1 when
          not, and 2 for a wrong option or a file it cannot read.

manifest  writes DIR/manifest.json for the binaries in --binaries, the units of
          each lamp folder (one with a LAMP.md) in --deploy, copied beside them, and
          the product skills of --skills (lampOS/skills), written beside them as
          lamp-product-skills.
          --min-updater N: the oldest updater that can apply the build (left out:
          any). It must carry lamp-update, and N is at most this program's updater.
          --models FILE: the models in FILE (deploy/product/models.json's form) are
          carried, and N is then at least 2.
          --settings on: the product's settings and tmpfiles are carried, and N is
          then at least 3; with them setup's reset plan, and N is then at least 5.
          With --deploy/product present this is the default; --settings off
          explicitly leaves them out. Ordinary product services require N >= 6.
          --image DIR: the image's own files of DIR (robots/lamp: its rootfs and each
          variant's override), each listed in --deploy's image/shipped.json, are
          carried, and N is then at least 4.
sign      writes the signed envelope (lampos-manifest.json) for a manifest, with a
          private key: an unencrypted PKCS#8 Ed25519 key in base64.
public-key prints the public key of a private key, in base64: what a lamp is given.

check and once end with 0 when up to date, switched, or only saying what they would
do; 1 when refused, put back or failed; 3 when the channel cannot be reached (nothing
changed); 4 when no quiet moment came (nothing switched). A wrong option ends with 2.
";

fn usage() -> ExitCode {
    eprint!("{USAGE}");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let Some((command, rest)) = arguments.split_first() else {
        return usage();
    };
    let Some(options) = options(rest) else {
        return usage();
    };
    let result = match command.as_str() {
        "--help" => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        "check" | "once" | "serve" => return on_the_lamp(command, &options),
        "install" => install(&options),
        "harness-off" => harness_off(&options),
        "harness-on" => harness_on(&options),
        "back" => back(&options),
        "apply" => apply(&options),
        "writer-probe" => writer_probe(&options),
        "writer-refresh" => writer_refresh(&options),
        "check-units" => return check_units(&options),
        "manifest" => make_manifest(&options),
        "sign" => sign(&options),
        "public-key" => public_key(&options),
        _ => return usage(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("lamp-update: {why}");
            ExitCode::FAILURE
        }
    }
}

/// `lamp-update check-units`: 0 when the lamp runs exactly the product's
/// units, 1 when not, 2 for a wrong option or a file that cannot be read, so
/// a script tells a bad input from a lamp that differs (Twelve on #1346).
fn check_units(options: &BTreeMap<String, String>) -> ExitCode {
    use lamp_update::check_units;
    if options
        .keys()
        .any(|name| !["cat", "product", "image", "variant"].contains(&name.as_str()))
    {
        return usage();
    }
    let Some(cat) = options.get("cat") else {
        return usage();
    };
    let product = match options.get("product") {
        Some(folder) => match check_units::from_folder(Path::new(folder)) {
            Ok(units) => units,
            Err(why) => {
                eprintln!("lamp-update: {why}");
                return ExitCode::from(2);
            }
        },
        None => check_units::embedded(),
    };
    let printed = match fs::read_to_string(cat) {
        Ok(text) => check_units::parse(&text),
        Err(error) => {
            eprintln!("lamp-update: {cat}: {error}");
            return ExitCode::from(2);
        }
    };
    let image = match options.get("image") {
        None => None,
        Some(lamp) => {
            let shipped = lamp_update::image::shipped_from(lamp_update::image::SHIPPED)
                .and_then(|shipped| lamp_update::image::from_tree(Path::new(lamp), &shipped));
            match shipped {
                Ok(files) => Some(check_units::image_units(
                    &files,
                    options
                        .get("variant")
                        .map_or(lamp_update::image::STANDARD, String::as_str),
                )),
                Err(error) => {
                    eprintln!("lamp-update: {lamp}: {error}");
                    return ExitCode::from(2);
                }
            }
        }
    };
    let report = check_units::check_with_image(&product, image.as_ref(), &printed);
    print!("{}", report.said());
    if report.passes() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// `--name value` pairs, each name once.
fn options(words: &[String]) -> Option<BTreeMap<String, String>> {
    let mut found = BTreeMap::new();
    let mut words = words.iter();
    while let Some(name) = words.next() {
        let name = name.strip_prefix("--")?;
        let value = words.next()?;
        if found.insert(name.to_owned(), value.clone()).is_some() {
            return None;
        }
    }
    Some(found)
}

fn wanted<'a>(options: &'a BTreeMap<String, String>, name: &str) -> Result<&'a str, String> {
    options
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| format!("--{name} is needed"))
}

fn manifest_settings_on(deploy: &Path, choice: Option<&str>) -> Result<bool, String> {
    match choice {
        None => Ok(deploy.join(manifest::PRODUCT_LAMP).is_dir()),
        Some("on") => Ok(true),
        Some("off") => Ok(false),
        Some(_) => Err("--settings is on or off".into()),
    }
}

/// Only an absent switch may inherit the authenticated product default.
/// An explicit off, invalid value, or non-UTF8 value never enables it.
fn settings_switches(choice: Option<&std::ffi::OsStr>, self_switch: bool) -> (bool, bool) {
    (
        lamp_update::apply::on(choice.and_then(std::ffi::OsStr::to_str)),
        choice.is_none() && self_switch,
    )
}

fn make_manifest(options: &BTreeMap<String, String>) -> Result<(), String> {
    known(
        options,
        &[
            "binaries",
            "deploy",
            "skills",
            "channel",
            "sequence",
            "commit",
            "files-url",
            "min-updater",
            "models",
            "settings",
            "image",
        ],
    )?;
    let image = match options.get("image") {
        None => Vec::new(),
        Some(lamp) => {
            let deploy = Path::new(wanted(options, "deploy")?);
            let shipped_path = deploy.join("image/shipped.json");
            let shipped = std::fs::read_to_string(&shipped_path)
                .and_then(|text| lamp_update::image::shipped_from(&text))
                .map_err(|error| format!("{}: {error}", shipped_path.display()))?;
            lamp_update::image::from_tree(Path::new(lamp), &shipped)
                .map_err(|error| format!("{lamp}: {error}"))?
        }
    };
    // With the settings, setup's reset plan, in its own field (#806), and
    // the Skills Store's key when the repository has one (#3692).
    let deploy = Path::new(wanted(options, "deploy")?);
    let (settings, reset_plan) =
        if manifest_settings_on(deploy, options.get("settings").map(String::as_str))? {
            let mut settings = install::product_settings();
            settings.extend(install::store_key_setting(deploy)?);
            (settings, Some(install::reset_plan_setting()))
        } else {
            (Vec::new(), None)
        };
    let models = match options.get("models") {
        None => Vec::new(),
        Some(path) => {
            let text = std::fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?;
            lamp_update::install::models_from(&text).map_err(|error| format!("{path}: {error}"))?
        }
    };
    let min_updater = match options.get("min-updater") {
        None => None,
        Some(value) => Some(
            value
                .parse::<u64>()
                .ok()
                .filter(|level| *level >= 1)
                .ok_or("--min-updater is a whole number from 1")?,
        ),
    };
    let sequence = wanted(options, "sequence")?
        .parse()
        .map_err(|_| "--sequence is a whole number".to_owned())?;
    let made = publish::make_with(
        Path::new(wanted(options, "binaries")?),
        Path::new(wanted(options, "deploy")?),
        Path::new(wanted(options, "skills")?),
        wanted(options, "channel")?,
        sequence,
        wanted(options, "commit")?,
        wanted(options, "files-url")?,
        min_updater,
        models,
        settings,
        image,
        reset_plan,
    )
    .map_err(|error| error.to_string())?;
    println!(
        "manifest of build {} of {}: {} binaries, {} units, {} models, {} settings, {} of the image's files",
        made.sequence,
        made.channel,
        made.files.len(),
        made.units.len(),
        made.models.len(),
        made.settings.len(),
        made.image.len()
    );
    Ok(())
}

fn install(options: &BTreeMap<String, String>) -> Result<(), String> {
    known(options, &["channel-url", "channel", "key-file", "setup"])?;
    let setup = match options.get("setup").map(String::as_str) {
        None | Some("off") => false,
        Some("on") => true,
        Some(_) => return Err("--setup is on or off".into()),
    };
    let channel_url = wanted(options, "channel-url")?.to_owned();
    if !(channel_url.starts_with("https://") || channel_url.starts_with("file:///"))
        || !channel_url.ends_with('/')
    {
        return Err("--channel-url is an https:// or file:/// folder, ending in /".into());
    }
    let channel = options.get("channel").map_or("lamp-voice", String::as_str);
    if !manifest::is_token(channel) {
        return Err("--channel is a plain name".into());
    }
    // The image's key: a trust root never comes from the channel (#806).
    let key_file = options
        .get("key-file")
        .map_or("/etc/lampOS/update/key.pub", String::as_str);
    let key = fs::read_to_string(key_file).map_err(|error| format!("{key_file}: {error}"))?;
    let key = manifest::read_public_key(&key).map_err(|why| format!("{key_file}: {why}"))?;
    let config = Install {
        root: PathBuf::from("/"),
        channel: channel.to_owned(),
        channel_url,
        key,
        records: lamp_update::install::RECORDS.into(),
        setup,
        models: install::models()?,
    };
    let mut channel = Web::new().map_err(|error| format!("TLS cannot be set up: {error}"))?;
    let mut say = |line: &str| eprintln!("lamp-update: {line}");
    let done = install::install(&config, &mut channel, &mut System, &mut say)?;
    if done.changed.is_empty() {
        println!("build {} is installed; nothing changed", done.sequence);
    } else {
        println!(
            "build {} installed: {} changes; nothing started",
            done.sequence,
            done.changed.len()
        );
    }
    Ok(())
}

fn harness_off(options: &BTreeMap<String, String>) -> Result<(), String> {
    known(options, &[])?;
    let mut say = |line: &str| eprintln!("lamp-update: {line}");
    let left = install::harness_off(
        Path::new("/"),
        lamp_update::install::RECORDS,
        &mut System,
        &mut say,
    )?;
    if left.is_empty() {
        println!("Harness does not start at boot as lampos");
    } else {
        println!(
            "Harness does not start at boot as lampos; {} left, said above, is Harness's own to stop",
            left.len()
        );
    }
    Ok(())
}

fn harness_on(options: &BTreeMap<String, String>) -> Result<(), String> {
    known(options, &[])?;
    let mut say = |line: &str| eprintln!("lamp-update: {line}");
    let changed = install::harness_on(
        Path::new("/"),
        lamp_update::install::RECORDS,
        &mut System,
        &mut say,
    )?;
    if changed.is_empty() {
        println!("Harness was already on and starts at boot as lampos: nothing changed");
    } else {
        println!(
            "Harness is on and starts at boot as lampos; it is not signed in until the person signs it in"
        );
    }
    Ok(())
}

/// `lamp-update back`: lamp-update-back, at its fixed path (#1680).
fn back(options: &BTreeMap<String, String>) -> Result<(), String> {
    known(options, &[])?;
    let records = Path::new(lamp_update::install::RECORDS);
    let unit = lamp_update::round::OWN_UNIT;
    let state = Systemctl::default()
        .states(&[unit.to_owned()])?
        .into_iter()
        .find(|state| state.unit == unit)
        .unwrap_or_default();
    let mut say = |line: &str| eprintln!("lamp-update-back: {line}");
    let back = own::back(
        records,
        Path::new("/"),
        lamp_update::records::unix_ms(),
        &state,
        &mut say,
    )?;
    let busy = back == own::Back::Busy;
    match back {
        // A self.json set aside leaves lamp-update to be started too: it
        // may have failed on it.
        own::Back::PutBack(_) | own::Back::Unreadable(_) => {
            // Started again whatever brought it here: its start limit may
            // have been reached.
            let _ = Command::new("systemctl")
                .args(["reset-failed", unit])
                .status();
            let started = Command::new("systemctl")
                .args(["restart", "--no-block", unit])
                .status()
                .map_err(|error| format!("systemctl: {error}"))?;
            if !started.success() {
                return Err(format!("{unit} could not be started again"));
            }
        }
        own::Back::Busy => say("another lamp-update is at work; the timer comes again"),
        own::Back::Nothing | own::Back::Waiting => {}
    }
    // A `back` that ended well proves a refreshed writer (#3689): noted with
    // its own bytes, which a round reads before it removes the copy kept.
    // One that found the lock held ran no more than its start, which
    // proves nothing (Seventh's P3 on #4107).
    if !busy
        && let Ok(own_bytes) = lamp_update::fixed_copy::own_bytes()
        && let Err(error) = lamp_update::writer_swap::note_back(
            records,
            &own_bytes,
            lamp_update::records::unix_ms(),
        )
    {
        say(&format!("{}: {error}", lamp_update::writer_swap::BACK_OK));
    }
    Ok(())
}

/// `lamp-update writer-probe`: a copy of lamp-update says it starts here,
/// and which bytes it runs from, before it is put at a fixed path (#3689).
fn writer_probe(options: &BTreeMap<String, String>) -> Result<(), String> {
    known(options, &[])?;
    println!("{}", lamp_update::fixed_copy::own_answer()?);
    Ok(())
}

/// `lamp-update writer-refresh`: the running lamp-update put at the settings
/// writer's fixed path, the copy there kept (#3689). Run as root by
/// `lamp-update-writer.service`, which a round starts; by hand only when no
/// round is at work.
fn writer_refresh(options: &BTreeMap<String, String>) -> Result<(), String> {
    known(options, &[])?;
    let running = lamp_update::fixed_copy::own_bytes()?;
    let done = lamp_update::writer_swap::refresh(
        Path::new("/"),
        Path::new(lamp_update::install::RECORDS),
        &running,
        &lamp_update::records::unix_ms,
        &mut |path: &Path| lamp_update::fixed_copy::run_probe(path),
    )?;
    // A new writer is judged by its own `back` only: a failure systemd
    // still shows from the copy before is cleared (Seventh's P3 on #4107).
    if done.before.is_some() && !done.proven {
        let _ = Command::new("systemctl")
            .args(["reset-failed", lamp_update::writer_swap::BACK_UNIT])
            .status();
    }
    let build = &done.build[..12];
    match (&done.before, done.synced) {
        (None, _) => println!("the settings writer already is build {build}"),
        (Some(before), true) => println!(
            "the settings writer is build {build}, and build {} is kept at {}",
            &before[..12],
            lamp_update::writer_swap::PREV
        ),
        (Some(_), false) => {
            println!("the settings writer is build {build}, but its folder could not be synced")
        }
    }
    Ok(())
}

/// `lamp-update apply`: lamp-update-apply, from lamp-update-back's fixed
/// path (#1680).
fn apply(options: &BTreeMap<String, String>) -> Result<(), String> {
    known(options, &[])?;
    let key_file = "/etc/lampOS/update/key.pub";
    let key = fs::read_to_string(key_file).map_err(|error| format!("{key_file}: {error}"))?;
    let key = manifest::read_public_key(&key).map_err(|why| format!("{key_file}: {why}"))?;
    let mut say = |line: &str| eprintln!("lamp-update-apply: {line}");
    // Each command has passed lamp_update::image::guard in apply::run.
    let mut command = |parts: &[&str]| {
        let status = Command::new(parts[0])
            .args(&parts[1..])
            .status()
            .map_err(|error| format!("{}: {error}", parts[0]))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("{}: {status}", parts.join(" ")))
        }
    };
    lamp_update::apply::run(
        Path::new("/"),
        Path::new(lamp_update::install::RECORDS),
        &key,
        &mut command,
        &mut say,
    )
}

fn sign(options: &BTreeMap<String, String>) -> Result<(), String> {
    known(options, &["key-file", "manifest", "out"])?;
    let key =
        fs::read_to_string(wanted(options, "key-file")?).map_err(|error| error.to_string())?;
    let key = manifest::read_private_key(&key).map_err(|why| why.0)?;
    let bytes = fs::read(wanted(options, "manifest")?).map_err(|error| error.to_string())?;
    let envelope = manifest::seal(&bytes, &key).map_err(|why| why.0)?;
    fs::write(wanted(options, "out")?, envelope).map_err(|error| error.to_string())?;
    let public =
        manifest::read_public_key(&manifest::public_key_text(&key)).map_err(|why| why.0)?;
    println!("signed with key {}", manifest::fingerprint(&public));
    Ok(())
}

fn public_key(options: &BTreeMap<String, String>) -> Result<(), String> {
    known(options, &["key-file"])?;
    let key =
        fs::read_to_string(wanted(options, "key-file")?).map_err(|error| error.to_string())?;
    let key = manifest::read_private_key(&key).map_err(|why| why.0)?;
    println!("{}", manifest::public_key_text(&key));
    Ok(())
}

fn known(options: &BTreeMap<String, String>, names: &[&str]) -> Result<(), String> {
    match options.keys().find(|name| !names.contains(&name.as_str())) {
        Some(name) => Err(format!("--{name} is not an option here")),
        None => Ok(()),
    }
}

fn seconds(
    options: &BTreeMap<String, String>,
    name: &str,
    default: u64,
) -> Result<Duration, String> {
    match options.get(name) {
        None => Ok(Duration::from_secs(default)),
        Some(value) => value
            .parse::<u64>()
            .ok()
            .filter(|seconds| (1..=86_400).contains(seconds))
            .map(Duration::from_secs)
            .ok_or_else(|| format!("--{name} is 1 to 86400 seconds")),
    }
}

/// The configuration of a round on the lamp, from its options.
fn config(
    command: &str,
    options: &BTreeMap<String, String>,
) -> Result<(Config, Duration, PathBuf), String> {
    known(
        options,
        &[
            "channel-url",
            "channel",
            "key-file",
            "lamp",
            "apply",
            "every-s",
            "quiet-wait-s",
            "watch-s",
            "keep-free-mb",
            "root",
            "bus-root",
        ],
    )?;
    let root = PathBuf::from(options.get("root").map_or("/", String::as_str));
    let on = |path: &str| root.join(path.trim_start_matches('/'));
    let channel_url = wanted(options, "channel-url")?.to_owned();
    if !(channel_url.starts_with("https://") || channel_url.starts_with("file:///"))
        || !channel_url.ends_with('/')
    {
        return Err("--channel-url is an https:// or file:/// folder, ending in /".into());
    }
    let channel = options.get("channel").map_or("lamp-voice", String::as_str);
    if !manifest::is_token(channel) {
        return Err("--channel is a plain name".into());
    }
    let key_file = options
        .get("key-file")
        .cloned()
        .unwrap_or_else(|| on("/etc/lampOS/update/key.pub").display().to_string());
    let key = fs::read_to_string(&key_file).map_err(|error| format!("{key_file}: {error}"))?;
    let key = manifest::read_public_key(&key).map_err(|why| format!("{key_file}: {why}"))?;
    let lamp = match options.get("lamp") {
        Some(lamp) => lamp.clone(),
        None => fs::read_to_string(on("/etc/hostname"))
            .map(|name| name.trim().to_owned())
            .map_err(|error| format!("the host name cannot be read ({error}); give --lamp"))?,
    };
    if !manifest::is_token(&lamp) {
        return Err(format!("{lamp:?} is not a lamp's name"));
    }
    let apply = match options.get("apply").map(String::as_str) {
        None | Some("off") => false,
        Some("on") => command != "check",
        Some(_) => return Err("--apply is on or off".into()),
    };
    let keep_free_mb: u64 = match options.get("keep-free-mb") {
        None => 256,
        Some(value) => value
            .parse()
            .ok()
            .filter(|mb| *mb <= 1_000_000)
            .ok_or("--keep-free-mb is a whole number of MB")?,
    };
    let bus_root = options
        .get("bus-root")
        .cloned()
        .or_else(|| std::env::var(lamp_bus::ROOT_VARIABLE).ok())
        .unwrap_or_else(|| lamp_bus::DEFAULT_ROOT.to_owned());
    let running = running_folder(&root);
    let self_switch = own::on(std::env::var(own::SWITCH).ok().as_deref());
    let settings_env = std::env::var_os(lamp_update::apply::SWITCH);
    let (settings, product_settings) = settings_switches(settings_env.as_deref(), self_switch);
    let config = Config {
        root,
        units: "/etc/systemd/system/".into(),
        records: lamp_update::install::RECORDS.into(),
        channel: channel.to_owned(),
        channel_url,
        key,
        lamp,
        apply,
        quiet_wait: seconds(options, "quiet-wait-s", 3600)?,
        watch: seconds(options, "watch-s", 60)?,
        keep_free: keep_free_mb * 1_000_000,
        self_switch,
        running,
        updater: manifest::UPDATER,
        models: lamp_update::models::on(std::env::var(lamp_update::models::SWITCH).ok().as_deref()),
        settings,
        product_settings,
        image: lamp_update::image::on(std::env::var(lamp_update::image::SWITCH).ok().as_deref()),
        writer_from: lamp_update::fixed_copy::own_bytes()
            .ok()
            .map(lamp_update::writer_swap::Running::new),
    };
    Ok((
        config,
        seconds(options, "every-s", 900)?,
        PathBuf::from(bus_root),
    ))
}

/// The folder this binary runs from, as the lamp names it under `root`:
/// `/opt/lampOS/update/<sha256>/` on a product lamp.
fn running_folder(root: &Path) -> Option<String> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    let root = root.canonicalize().ok()?;
    let folder = exe.parent()?.strip_prefix(&root).ok()?;
    Some(format!("/{}/", folder.display()))
}

fn status(outcome: &Outcome) -> u8 {
    match outcome {
        Outcome::UpToDate(_)
        | Outcome::Switched(_)
        | Outcome::SelfSwitched { .. }
        | Outcome::Climbs { .. }
        | Outcome::Would(_)
        | Outcome::NothingPublished => 0,
        Outcome::Unreachable(_) => 3,
        Outcome::NotQuiet(_) => 4,
        Outcome::Refused(_)
        | Outcome::WaitsForModels(_)
        | Outcome::RolledBack { .. }
        | Outcome::Failed(_) => 1,
    }
}

fn on_the_lamp(command: &str, options: &BTreeMap<String, String>) -> ExitCode {
    let (config, every, bus_root) = match config(command, options) {
        Ok(found) => found,
        Err(why) => {
            eprintln!("lamp-update: {why}");
            return usage();
        }
    };
    let mut say = |line: &str| eprintln!("lamp-update: {line}");
    let mut channel = match Web::new() {
        Ok(web) => web,
        Err(error) => {
            eprintln!("lamp-update: TLS cannot be set up: {error}");
            return ExitCode::FAILURE;
        }
    };
    // Without --apply on nothing is waited for or watched: the bus is not
    // even subscribed to.
    let mut ears: Box<dyn Ears> = if config.apply {
        Box::new(BusEars::new(&bus_root, &mut say))
    } else {
        Box::new(Deaf)
    };
    let mut systemd = Systemctl::default();
    let clock = Monotonic::default();
    eprintln!(
        "lamp-update: {command} channel={} url={} lamp={} apply={} self={} key={}",
        config.channel,
        config.channel_url,
        config.lamp,
        if config.apply { "on" } else { "off" },
        if config.self_switch { "on" } else { "off" },
        manifest::fingerprint(&config.key)
    );
    if config.apply {
        // Phase 2 of a switch of its own: confirmed only once a dry round
        // has checked the channel's manifest. Until then no round acts;
        // lamp-update-back's deadline puts the old build back if it never is.
        let running = std::env::current_exe()
            .ok()
            .and_then(|exe| Some(exe.parent()?.file_name()?.to_string_lossy().into_owned()));
        let mut told = String::new();
        loop {
            let confirmed = {
                let mut world = World {
                    channel: &mut channel,
                    systemd: &mut systemd,
                    ears: ears.as_mut(),
                    disk: &Statvfs,
                    clock: &clock,
                    say: &mut say,
                    phase: &mut |_| {},
                };
                own::confirm(&config, &mut world, running.as_deref())
            };
            let why = match confirmed {
                Ok(own::Confirm::Confirmed) => {
                    eprintln!("lamp-update: confirmed: lamp-update runs its new build");
                    break;
                }
                Ok(own::Confirm::Nothing | own::Confirm::NotOwn) => break,
                Ok(own::Confirm::Unreadable(own::Unreadable::SetAside)) => break,
                Ok(own::Confirm::Unreadable(own::Unreadable::PutBack { .. })) => {
                    // This build is refused: the old one runs in its place.
                    let program = config.on(own::CURRENT).join(own::BINARY);
                    let error = Command::new(&program)
                        .args(std::env::args_os().skip(1))
                        .exec();
                    eprintln!(
                        "lamp-update: {} cannot be started: {error}",
                        program.display()
                    );
                    return ExitCode::FAILURE;
                }
                Ok(own::Confirm::NotProven(why)) => format!("not confirmed yet: {why}"),
                Ok(own::Confirm::Busy) => {
                    "not confirmed yet: another lamp-update is at work".into()
                }
                Err(why) => format!("not confirmed: {why}"),
            };
            if why != told {
                eprintln!("lamp-update: {why}");
                told = why;
            }
            let _ = ears.listen(Duration::from_secs(30));
        }
    }
    let mut last = String::new();
    // `update/status` (#2194): where each round is, and how the last ended,
    // only for a round that acts.
    let records = config.on(&config.records);
    let mut telling = config
        .apply
        .then(|| lamp_update::status::Telling::start(&bus_root, &records, &mut say));
    loop {
        let outcome = {
            let mut phase = |now: &'static str| {
                if let Some(telling) = &mut telling {
                    telling.phase(now);
                }
            };
            let mut world = World {
                channel: &mut channel,
                systemd: &mut systemd,
                ears: ears.as_mut(),
                disk: &Statvfs,
                clock: &clock,
                say: &mut say,
                phase: &mut phase,
            };
            round(&config, &mut world)
        };
        if let Some(telling) = &mut telling {
            let at_unix_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_millis() as u64);
            telling.ended(&outcome, at_unix_ms);
        }
        let text = outcome.to_string();
        if let Outcome::SelfSwitched { .. } | Outcome::Climbs { .. } = outcome {
            // Phase 1 done: the new build runs in this program's place,
            // with the same options. An exec that fails returns here.
            eprintln!("lamp-update: {text}");
            let program = config.on(own::CURRENT).join(own::BINARY);
            let error = Command::new(&program)
                .args(std::env::args_os().skip(1))
                .exec();
            eprintln!(
                "lamp-update: {} cannot be started: {error}",
                program.display()
            );
            let not_running = UnitState::default();
            let records = config.on(&config.records);
            if let Err(why) = own::back(&records, &config.root, u64::MAX, &not_running, &mut say) {
                eprintln!("lamp-update: its own build cannot be put back: {why}");
            }
            continue;
        }
        if command != "serve" {
            println!("{text}");
            return ExitCode::from(status(&outcome));
        }
        // A round that says what the one before said is not said again.
        if text != last {
            eprintln!("lamp-update: {text}");
            last = text;
        }
        // Between rounds the bus is still read, so that what the voice said
        // meanwhile does not wait in the sockets.
        let _ = ears.listen(every);
    }
}

#[cfg(test)]
mod product_settings_tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn manifest_defaults_follow_the_product_tree_but_explicit_choices_win() {
        let folder = tempfile::tempdir().unwrap();
        assert!(!manifest_settings_on(folder.path(), None).unwrap());
        assert!(manifest_settings_on(folder.path(), Some("on")).unwrap());
        std::fs::create_dir(folder.path().join(manifest::PRODUCT_LAMP)).unwrap();
        assert!(manifest_settings_on(folder.path(), None).unwrap());
        assert!(!manifest_settings_on(folder.path(), Some("off")).unwrap());
        assert!(manifest_settings_on(folder.path(), Some("on")).unwrap());
        assert!(manifest_settings_on(folder.path(), Some("auto")).is_err());
    }

    #[test]
    fn only_an_absent_apply_switch_and_self_switch_allow_the_product_default() {
        assert_eq!(settings_switches(None, true), (false, true));
        assert_eq!(settings_switches(None, false), (false, false));
        for self_switch in [false, true] {
            assert_eq!(
                settings_switches(Some(OsStr::new("on")), self_switch),
                (true, false)
            );
            for value in ["off", "", "invalid"] {
                assert_eq!(
                    settings_switches(Some(OsStr::new(value)), self_switch),
                    (false, false)
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_apply_value_is_present_and_never_enables_product_settings() {
        use std::os::unix::ffi::OsStrExt;
        assert_eq!(
            settings_switches(Some(OsStr::from_bytes(&[0xff])), true),
            (false, false)
        );
    }
}
