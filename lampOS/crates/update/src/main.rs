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
lamp-update harness-install
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
serve   a round every --every-s (900), until stopped; with --apply on, the app's
        Check for update (update/check) starts the next one sooner.
install lampOS on a blank lamp (docs/install.md): the newest build's binary of every
        product part, in its folder with its current link; the product units, tmpfiles,
        users and zone; enables the lamp and starts nothing. Run again, it changes
        nothing. As root, on the lamp itself.
region  the name and time servers for the region lamp-cloud keeps (docs/install.md, #6256):
        China's only on a lamp placed in China. As root; lamp-region.service runs it.
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
harness-install run by lamp-update-harness.service, which a `serve` round starts with
        LAMP_HARNESS_AT_ONBOARDING=on while Harness is missing (#5197): harness-on's
        install alone, through the manifest the round pinned if it named one (its sha256
        checked first), recorded for update/status; no Harness at boot, no daemon. Then
        Hermes after it (on by default on a product lamp, LAMP_HERMES_ON_LAMP=off
        turns it off), pinned, as lampos, only in its quiet window and watched by
        serve while it runs (#5978); with LAMP_HERMES_SKILLS (on by default on a
        product lamp), its namespace shows lamp-skills' socket (#6128); with
        LAMP_SKILL_TASKS=on, lamp-control's control socket alone (#6306). Then OpenClaw
        after Hermes (on by default on a product lamp, LAMP_OPENCLAW_ON_LAMP=off turns
        it off; off with Harness off): a pinned Node 24 and the pinned openclaw package,
        as lampos, in the same namespace and the same window, taking turns with Hermes,
        with its gateway lamp-openclaw-gateway.service on loopback, enabled and always
        on while the switch is (disabled and stopped with it off); with
        LAMP_OPENCLAW_SKILLS (on by default on a product lamp) its namespace shows
        lamp-skills' socket (#6226).
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
changed); 4 when no quiet moment came, or another lamp-update or setup's helper held the
lock (nothing switched). A wrong option ends with 2.
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
        "region" => region(&options),
        "harness-off" => harness_off(&options),
        "harness-on" => harness_on(&options),
        "harness-install" => harness_install(&options),
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

fn region(options: &BTreeMap<String, String>) -> Result<(), String> {
    known(options, &[])?;
    let mut say = |line: &str| eprintln!("lamp-update: {line}");
    let mut changed = Vec::new();
    install::region_servers(Path::new("/"), &mut System, &mut changed, &mut say)?;
    for line in &changed {
        println!("{line}");
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

/// `lamp-update harness-install`: Harness's one-shot (#5197), Hermes
/// after it (#5978), and OpenClaw after Hermes (#6226). Ends with 1 when Harness's install failed or another
/// held its lock; the records say why. Hermes's failure is its own record's.
fn harness_install(options: &BTreeMap<String, String>) -> Result<(), String> {
    known(options, &[])?;
    let mut say = |line: &str| eprintln!("lamp-update: {line}");
    lamp_update::hermes_install::one_shot(
        Path::new("/"),
        Path::new(lamp_update::install::RECORDS),
        &mut install::output_of,
        &mut lamp_update::records::unix_ms,
        &mut say,
    )
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
    let mut config = Config {
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
        // Not set at all: the switch file a round carries decides.
        models_file: std::env::var_os(lamp_update::models::SWITCH).is_none(),
        settings,
        product_settings,
        image: lamp_update::image::on(std::env::var(lamp_update::image::SWITCH).ok().as_deref()),
        // Set below: the product default needs the config's root and units.
        first_update: false,
        writer_from: lamp_update::fixed_copy::own_bytes()
            .ok()
            .map(lamp_update::writer_swap::Running::new),
    };
    // The first update after setup (#6284): on by this build's default on a
    // product lamp, since lamp-update's own unit is never carried by a round
    // (own_unit_line_tests) and so can't be given the switch on a lamp in a
    // home; LAMP_FIRST_UPDATE=off turns it off, and another lamp needs `on`.
    config.first_update = lamp_update::hermes_install::wanted(
        std::env::var(lamp_update::first_update::SWITCH)
            .ok()
            .as_deref(),
        config.apply
            && lamp_update::harness_install::product::product_lamp(
                &config.root,
                &config.on(&config.units),
            ),
        lamp_update::first_update::PRODUCT_DEFAULT,
    );
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
        | Outcome::Held { .. }
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
                    target: &mut |_| {},
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
    let records = config.on(&config.records);
    // The app's Check for update (#4153): read between rounds only by a
    // `serve` that acts, the one that says `update/status`.
    let mut app_check = (config.apply && command == "serve")
        .then(|| lamp_update::app_check::AppCheck::new(&bus_root, &mut say));
    // The first update after setup (#6284, LAMP_FIRST_UPDATE=on): only
    // the `serve` that acts, the one the app's check is read by.
    let mut first = app_check
        .is_some()
        .then(|| lamp_update::first_update::First::new(&config))
        .flatten();
    // Harness before the sign-in (#5197): only a `serve` that acts, with
    // its switch on.
    let harness = {
        use lamp_update::harness_install::{self as harness, product};
        let acts = config.apply && command == "serve";
        // A lamp installed as the product takes this build's own default
        // and pin when its environment sets none (#5229).
        let wanted = product::wanted(
            std::env::var(harness::SWITCH).ok().as_deref(),
            std::env::var(harness::MANIFEST_URL).ok().as_deref(),
            std::env::var(harness::MANIFEST_SHA256).ok().as_deref(),
            acts && product::product_lamp(&config.root, &config.on(&config.units)),
            product::PRODUCT_DEFAULT,
        );
        let on = acts && wanted.on;
        if acts {
            harness::at_start(&config.root, &records, on, lamp_update::records::unix_ms());
            if wanted.source == product::Source::Build {
                say(&format!(
                    "Harness before the sign-in: {} by this build's default on a product lamp",
                    if on { "on" } else { "off" }
                ));
            }
        }
        on.then_some(wanted.pin)
    };
    // lamp-update's own switches (#6607): set in its environment, or not set
    // there, from the carried /etc/lampOS/update.env (no round rewrites this
    // unit); skill tasks from the file only on a product lamp. Read here once,
    // and again after each round: a round that writes or changes the file
    // starts lamp-update again (below), so they take effect with no reboot.
    let switches_product = lamp_update::harness_install::product::product_lamp(
        &config.root,
        &config.on(&config.units),
    );
    let read_switches = || {
        lamp_update::switches::update_switches(&config.root, switches_product, |name| {
            std::env::var_os(name)
        })
    };
    let switches = read_switches();
    // Hermes on the lamp (#5978): only with Harness's install on too, as it
    // rides on Harness's one-shot.
    let hermes = {
        use lamp_update::{harness_install::product, hermes_install as hermes};
        let acts = config.apply && command == "serve";
        // On by this build's default on a product lamp (PRODUCT_DEFAULT true, the product owner's
        // decision, 10 Nov, with the first builder's schedule: crate::hermes_schedule);
        // LAMP_HERMES_ON_LAMP=off turns it off, and another lamp needs `on`.
        let product = acts && product::product_lamp(&config.root, &config.on(&config.units));
        let wanted = hermes::wanted(
            std::env::var(hermes::SWITCH).ok().as_deref(),
            product,
            hermes::PRODUCT_DEFAULT,
        );
        // Its own switch (#6128), on by this build's default on a product lamp
        // (SKILLS_PRODUCT_DEFAULT true): Hermes is shown lamp-skills' socket folder,
        // read-only; LAMP_HERMES_SKILLS=off turns it off, and another lamp needs `on`.
        let skills = hermes::wanted(
            std::env::var(hermes::SKILLS_SWITCH).ok().as_deref(),
            product,
            hermes::SKILLS_PRODUCT_DEFAULT,
        );
        // A skill's tasks (#6306): LAMP_SKILL_TASKS, the switch lamp-tasks reads,
        // as read above (#6607). On, Hermes is shown lamp-control's control socket
        // alone, read-only.
        let tasks = switches.tasks;
        let shown = hermes::Shown { skills, tasks };
        if acts && wanted && harness.is_none() {
            say("Hermes on the lamp: off, as Harness before the sign-in is off");
        }
        let on = acts && wanted && harness.is_some();
        if acts {
            hermes::at_start(
                &config.root,
                &records,
                on,
                shown,
                lamp_update::records::unix_ms(),
            );
        }
        // Off, a `serve` that acts still takes out what an earlier install
        // left for lampos to run (its wrapper and namespace script).
        acts.then_some((on, shown))
    };
    // OpenClaw on the lamp (#6226): as Hermes, beside it, on Harness's one-shot.
    let openclaw = {
        use lamp_update::{harness_install::product, openclaw_install as openclaw};
        let acts = config.apply && command == "serve";
        // On by this build's default on a product lamp (PRODUCT_DEFAULT true: beside Hermes,
        // the default engine, under Harness, #6498); LAMP_OPENCLAW_ON_LAMP=off turns it off, and
        // another lamp needs `on`. Its skills switch is read the same way.
        let product = acts && product::product_lamp(&config.root, &config.on(&config.units));
        let wanted = openclaw::wanted(
            std::env::var(openclaw::SWITCH).ok().as_deref(),
            product,
            openclaw::PRODUCT_DEFAULT,
        );
        let skills = openclaw::wanted(
            std::env::var(openclaw::SKILLS_SWITCH).ok().as_deref(),
            product,
            openclaw::SKILLS_PRODUCT_DEFAULT,
        );
        // A skill's runs on OpenClaw (#6685): LAMP_SKILL_DEFAULT_AGENT=on with
        // LAMP_SKILL_TASKS on; the namespace then also shows lamp-control's socket
        // alone and lamp-tasks' runs' token folder.
        let tasks = switches.tasks
            && openclaw::default_agent(
                std::env::var(openclaw::DEFAULT_AGENT_SWITCH)
                    .ok()
                    .as_deref(),
            );
        let shown = lamp_update::hermes_install::Shown { skills, tasks };
        if acts && wanted && harness.is_none() {
            say("OpenClaw on the lamp: off, as Harness before the sign-in is off");
        }
        let on = acts && wanted && harness.is_some();
        if acts {
            openclaw::at_start(
                &config.root,
                &records,
                on,
                shown,
                lamp_update::records::unix_ms(),
            );
        }
        acts.then_some((on, shown))
    };
    // Hermes's schedule (#5978): each install watched while it runs, the
    // voice heard from the start, so speech freezes it and a lost privacy
    // reading stops it; one watch for Hermes's and OpenClaw's both (#6226).
    let hermes_watch = (matches!(hermes, Some((true, _))) || matches!(openclaw, Some((true, _))))
        .then(|| lamp_update::hermes_schedule::Watcher::start(&bus_root, &records));
    // The lamp's agents for the app (#6240): LAMP_APP_ENGINES, as read above
    // (#6607). Hermes's state on update/status, and the app's tap heard beside
    // the app's checks.
    let engines = switches.engines.then(|| lamp_update::engines::Engines {
        hermes_on: matches!(hermes, Some((true, _))),
        frozen: hermes_watch
            .as_ref()
            .map(lamp_update::hermes_schedule::Watcher::frozen)
            .unwrap_or_default(),
        pending: Default::default(),
        // OpenClaw beside Hermes, said and installed as Hermes is (#6240,
        // V2's ask of 12 Nov): on as its install is (its switch, Harness's).
        openclaw_on: matches!(openclaw, Some((true, _))),
        openclaw_pending: Default::default(),
        harness: Default::default(),
    });
    if let Some(engines) = &engines {
        // Every other engine the lamp's Harness has (the lead's rule of 12
        // Nov: the list is Harness's, never a fixed two), looked at on a
        // thread of its own every five minutes (`HARNESS_LOOK_EVERY`), only by a `serve` that acts.
        if config.apply && command == "serve" {
            engines.look_at_harness_every(
                lamp_update::engines::HARNESS_LOOK_EVERY,
                lamp_update::engines::harness_run(),
                |line: &str| eprintln!("lamp-update: {line}"),
            );
        }
        app_check = app_check.map(|check| {
            check.with_engines(lamp_update::engines::Asks::new(
                &bus_root,
                &records,
                engines.clone(),
                &mut say,
            ))
        });
    }
    // One agent namespace for every engine Harness reports (#6510 Part 1,
    // step 1): only with lamp-update's own LAMP_AGENT_NS=on (no product unit
    // sets it), by a `serve` that acts, beside the look at Harness's engines
    // (LAMP_APP_ENGINES=on) that says which; the namespace shows what
    // Hermes's does (its skills and tasks switches).
    let agent_ns = {
        use lamp_update::agent_ns::rounds;
        let wanted = config.apply
            && command == "serve"
            && rounds::on(std::env::var(rounds::SWITCH).ok().as_deref());
        if wanted && engines.is_none() {
            say(&format!(
                "the agent namespace for every engine: off, as {} (the look at Harness's engines) is off",
                lamp_update::engines::SWITCH
            ));
        }
        if wanted && harness.is_none() {
            say("the agent namespace for every engine: off, as Harness before the sign-in is off");
        }
        match (wanted, &engines, hermes) {
            (true, Some(engines), Some((_, shown))) if harness.is_some() => {
                Some((engines.harness.clone(), shown))
            }
            _ => None,
        }
    };
    // `update/status` (#2194): where each round is, and how the last ended,
    // only for a round that acts; Harness's record looked at often only
    // with its switch on.
    // Held in a cell: the round's phase and target both tell it (#5473).
    // What the agent namespace's rounds last said, so a line that stays
    // true is said once (#6510).
    let mut agent_ns_said = lamp_update::agent_ns::rounds::Said::default();
    let telling = std::cell::RefCell::new(config.apply.then(|| {
        lamp_update::status::Telling::start(
            &bus_root,
            &records,
            harness.is_some(),
            engines.clone(),
            &mut say,
        )
    }));
    loop {
        let before_setup = lamp_update::first_update::before_setup(&config);
        if let Some(first) = &mut first {
            first.begin(lamp_update::records::unix_ms(), &mut say);
        }
        let outcome = {
            let mut phase = |now: &'static str| {
                if let Some(telling) = telling.borrow_mut().as_mut() {
                    telling.phase(now);
                }
            };
            let mut target = |build: &lamp_update::records::Build| {
                if let Some(telling) = telling.borrow_mut().as_mut() {
                    telling.target(build);
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
                target: &mut target,
            };
            round(&config, &mut world)
        };
        // Once the channel was reached, so the lamp has its network; never
        // before a switch of its own, whose new build does its own round.
        if let Some(pin) = &harness
            && !matches!(
                outcome,
                Outcome::Unreachable(_) | Outcome::SelfSwitched { .. } | Outcome::Climbs { .. }
            )
        {
            // A move to a newer pin waits while Harness is in use (#5957):
            // tasks/status on the bus, and Harness's own processes.
            lamp_update::harness_install::after_round_when(
                &config.root,
                &records,
                pin,
                &mut systemd,
                lamp_update::records::unix_ms(),
                &mut || {
                    lamp_update::harness_install::busy::in_use(
                        &config.root,
                        Some(bus_root.as_path()),
                    )
                },
                &mut say,
            );
            if let (Some(_), Some((true, shown))) = (&hermes_watch, hermes) {
                use lamp_update::{hermes_install, hermes_schedule};
                let now_unix_ms = lamp_update::records::unix_ms();
                let window = hermes_schedule::window(&hermes_schedule::Moment {
                    walk_under_way: lamp_update::walk::under_way(&config),
                })
                // The app's tap (#6240): a recent failure does not hold it.
                .map(|window| {
                    if engines
                        .as_ref()
                        .is_some_and(lamp_update::engines::Engines::pending)
                    {
                        hermes_schedule::Window::Asked
                    } else {
                        window
                    }
                });
                hermes_install::after_round(
                    &config.root,
                    &records,
                    shown,
                    &mut systemd,
                    now_unix_ms,
                    window,
                    &mut say,
                );
                if let Some(engines) = &engines {
                    engines.settle(&records);
                }
            }
            // OpenClaw after Hermes, in its own window by its own record's
            // last try; Hermes's install under way holds it to a later round.
            if let (Some(_), Some((true, shown))) = (&hermes_watch, openclaw) {
                use lamp_update::{hermes_schedule, openclaw_install};
                let now_unix_ms = lamp_update::records::unix_ms();
                let window = hermes_schedule::window(&hermes_schedule::Moment {
                    walk_under_way: lamp_update::walk::under_way(&config),
                })
                // The app's tap (#6240): a recent failure does not hold it.
                .map(|window| {
                    if engines
                        .as_ref()
                        .is_some_and(lamp_update::engines::Engines::openclaw_pending)
                    {
                        hermes_schedule::Window::Asked
                    } else {
                        window
                    }
                });
                openclaw_install::after_round(
                    &config.root,
                    &records,
                    shown,
                    &mut systemd,
                    now_unix_ms,
                    window,
                    &mut say,
                );
                if let Some(engines) = &engines {
                    engines.settle_openclaw(&records);
                }
            }
            // Every other engine Harness says is installed, after Hermes's
            // and OpenClaw's own installers (#6510): its wrapper, the
            // namespace script and its home, written again when not there.
            if let Some((look, shown)) = &agent_ns {
                let look = look.lock().unwrap_or_else(|e| e.into_inner()).clone();
                lamp_update::agent_ns::rounds::after_round(
                    &config.root,
                    &look,
                    *shown,
                    &mut agent_ns_said,
                    &mut lamp_update::agent_ns::rounds::lampos_ids,
                    &mut lamp_update::agent_ns::wrapper::login_finds,
                    &mut lamp_update::agent_ns::wrapper::login_programs,
                    &mut say,
                );
            }
        }
        if matches!(hermes, Some((false, _)))
            && !matches!(
                outcome,
                Outcome::Unreachable(_) | Outcome::SelfSwitched { .. } | Outcome::Climbs { .. }
            )
        {
            lamp_update::hermes_install::after_round_off(
                &config.root,
                &records,
                &mut systemd,
                &mut say,
            );
        }
        if matches!(openclaw, Some((false, _)))
            && !matches!(
                outcome,
                Outcome::Unreachable(_) | Outcome::SelfSwitched { .. } | Outcome::Climbs { .. }
            )
        {
            lamp_update::openclaw_install::after_round_off(
                &config.root,
                &records,
                &mut systemd,
                &mut say,
            );
        }
        let kept = lamp_update::first_update::kept(before_setup, &outcome);
        if let Some(telling) = telling.borrow_mut().as_mut() {
            if kept {
                let at_unix_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |since| since.as_millis() as u64);
                telling.ended(&outcome, at_unix_ms);
            } else {
                telling.let_go();
            }
        }
        // Before setup, a channel out of reach is asked again soon, not
        // after the whole interval: the lamp has no internet yet.
        let every = if kept {
            every
        } else {
            say(&format!(
                "setup is not done: the channel is out of reach, not told, asked again in {} s",
                lamp_update::first_update::BEFORE_SETUP_AGAIN.as_secs()
            ));
            every.min(lamp_update::first_update::BEFORE_SETUP_AGAIN)
        };
        if let Some(first) = &mut first {
            first.ended(&outcome, &mut say);
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
        // A round that wrote or changed /etc/lampOS/update.env (#6607): this
        // program starts again in its own place, with the same options, so
        // its switches take effect with no reboot. An exec that fails returns
        // here, and the switches wait for the next start.
        if lamp_update::switches::restart_for(switches, read_switches()) {
            eprintln!("lamp-update: {text}");
            eprintln!(
                "lamp-update: {} changed: lamp-update starts again",
                lamp_update::manifest::UPDATE_ENV
            );
            match std::env::current_exe() {
                Ok(program) => {
                    let error = Command::new(&program)
                        .args(std::env::args_os().skip(1))
                        .exec();
                    eprintln!(
                        "lamp-update: {} cannot be started: {error}",
                        program.display()
                    );
                }
                Err(error) => eprintln!("lamp-update: its own program is not known: {error}"),
            }
        }
        // A round that says what the one before said is not said again.
        if text != last {
            eprintln!("lamp-update: {text}");
            last = text;
        }
        // Between rounds the bus is still read, so that what the voice said
        // meanwhile does not wait in the sockets. A check from the app ends
        // the wait (#4153); one that came during the round is dropped.
        match (&mut app_check, &mut first) {
            (Some(app_check), Some(first)) => {
                use lamp_update::app_check::Woken;
                app_check.forget(&mut say);
                let mut due =
                    |say: &mut dyn FnMut(&str)| first.due(lamp_update::records::unix_ms(), say);
                match app_check.wait_or(ears.as_mut(), every, &mut say, &mut due) {
                    Woken::Asked => {
                        eprintln!("lamp-update: the app asked for a check: a round now");
                        // The owner's retry of a refused build (#6266) drops
                        // its refusal before the round, as without the first
                        // update (#6800).
                        app_check.retry_asked(
                            &lamp_update::records::Records::new(records.clone()),
                            &mut say,
                        );
                    }
                    Woken::Due | Woken::Waited => {}
                }
            }
            (Some(app_check), None) => {
                app_check.forget(&mut say);
                if app_check.wait(ears.as_mut(), every, &mut say) {
                    eprintln!("lamp-update: the app asked for a check: a round now");
                    // The owner's retry of a refused build (#6266) drops its
                    // refusal before the round.
                    app_check.retry_asked(
                        &lamp_update::records::Records::new(records.clone()),
                        &mut say,
                    );
                }
            }
            (None, _) => {
                let _ = ears.listen(every);
            }
        }
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

/// The updater's own unit is one no round switches (`round::OWN_UNIT`): a
/// lamp keeps the line its image installed, and every `lamp-update` it
/// climbs to is started by it. On the 7071 image (#3689) that is
/// `aa9ec9e27a`'s `deploy/product/lamp-update.service:26`. If this program
/// no longer took it, a climbed updater would end with 2 at each start,
/// unconfirmed, and `lamp-update-back` would put 7071's back at its
/// deadline: every 7071 lamp would climb and fall back, round after round.
#[cfg(test)]
mod own_unit_line_tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};

    const LINE_7071: &str = "/opt/lampOS/update/current/lamp-update serve \
        --channel-url https://github.com/autonomous-ai/lampOS/releases/download/lampos-stable/ \
        --channel lamp-voice --key-file /etc/lampOS/update/key.pub --apply on";
    const PRODUCT_UNIT: &str = include_str!("../../../deploy/product/lamp-update.service");

    /// The line's command and options, as this program reads them, on a
    /// lamp whose key and host name are under `root`.
    fn read(line: &str, root: &Path) -> Result<(String, Config), String> {
        let words: Vec<String> = line.split_whitespace().skip(1).map(String::from).collect();
        let (command, rest) = words.split_first().ok_or("no command")?;
        let mut options = options(rest).ok_or("not --name value pairs")?;
        // Its key is at an absolute path: the same file, under `root`.
        let key = options.get_mut("key-file").ok_or("no --key-file")?;
        *key = root.join(key.trim_start_matches('/')).display().to_string();
        options.insert("root".into(), root.display().to_string());
        let (config, _, _) = config(command, &options)?;
        Ok((command.clone(), config))
    }

    #[test]
    fn the_7071_images_own_unit_line_still_starts_this_updater() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("etc/lampOS/update")).unwrap();
        fs::write(
            root.path().join("etc/lampOS/update/key.pub"),
            format!("{}\n", STANDARD.encode([7_u8; 32])),
        )
        .unwrap();
        fs::write(root.path().join("etc/hostname"), "lamp-7071\n").unwrap();
        let product = PRODUCT_UNIT
            .lines()
            .find_map(|line| line.strip_prefix("ExecStart="))
            .unwrap();
        for line in [LINE_7071, product] {
            let (command, config) =
                read(line, root.path()).unwrap_or_else(|why| panic!("{line}: {why}"));
            assert_eq!(command, "serve", "{line}");
            assert_eq!(
                config.channel_url,
                "https://github.com/autonomous-ai/lampOS/releases/download/lampos-stable/",
                "{line}"
            );
            assert_eq!(config.channel, "lamp-voice", "{line}");
            assert_eq!(config.key, [7_u8; 32], "{line}");
            assert_eq!(config.lamp, "lamp-7071", "{line}");
            assert!(config.apply, "{line}");
        }
    }

    /// The sandbox lines of `aa9ec9e27a`'s `deploy/product/lamp-update.service`,
    /// the unit every 7071 lamp runs each round under.
    const SANDBOX_7071: [&str; 7] = [
        "User=root",
        "NoNewPrivileges=yes",
        "ProtectSystem=strict",
        "ReadWritePaths=/opt/lampOS /etc/systemd/system -/var/lib/lampOS/skills -/var/lib/lampos-skills-state -/run/lampos-bus",
        "ProtectHome=yes",
        "PrivateTmp=yes",
        "StateDirectory=lampos-update",
    ];

    /// Whether a round, run under `sandbox`'s lines, may write `path`.
    fn writable(sandbox: &[&str], path: &str) -> bool {
        let mut writes: Vec<String> = sandbox
            .iter()
            .filter_map(|line| line.strip_prefix("ReadWritePaths="))
            .flat_map(str::split_whitespace)
            .map(|at| at.trim_start_matches('-').to_owned())
            .collect();
        writes.extend(
            sandbox
                .iter()
                .filter_map(|line| line.strip_prefix("StateDirectory="))
                .map(|name| format!("/var/lib/{name}")),
        );
        let path = path.trim_end_matches('/');
        writes
            .iter()
            .any(|at| path == at || path.starts_with(&format!("{at}/")))
    }

    /// No round switches lamp-update's own unit, so the sandbox a round
    /// runs in on a lamp is the one its image installed, whatever this file
    /// says. A path a round needs to write that isn't in 7071's
    /// `ReadWritePaths` needs a one-shot unit of its own, as the settings
    /// writer's (`writer_unit`) and the apply's: widening this file's line
    /// never reaches a shipped lamp (Second's P2 on #4503).
    #[test]
    fn the_updaters_own_sandbox_is_still_the_7071_images() {
        assert_eq!(round::OWN_UNIT, "lamp-update.service");
        let sandbox: Vec<&str> = PRODUCT_UNIT
            .lines()
            .filter(|line| {
                SANDBOX_7071
                    .iter()
                    .any(|kept| line.split('=').next() == kept.split('=').next())
            })
            .collect();
        assert_eq!(sandbox, SANDBOX_7071);
    }

    /// What a round writes itself fits 7071's sandbox; what it writes
    /// through a one-shot unit of its own, and Harness's place, don't.
    #[test]
    fn a_round_writes_itself_only_where_the_7071_sandbox_lets_it() {
        use lamp_update::{added, install, manifest, own, product_skills, unit, writer_swap};
        for path in [
            install::RECORDS,
            install::UNITS,
            added::UNITS,
            own::FOLDER,
            own::CURRENT,
            manifest::MODELS_ROOT,
            manifest::STORE,
            unit::INSTALLED,
            product_skills::FOLDER,
            product_skills::ON,
            "/run/lampos-bus",
        ] {
            assert!(writable(&SANDBOX_7071, path), "{path}");
        }
        for path in [
            own::BACK,
            writer_swap::PREV,
            manifest::RESET_PLAN,
            install::ZONE_PATH,
            install::HARNESS,
            install::HARNESS_HOME,
        ] {
            assert!(!writable(&SANDBOX_7071, path), "{path}");
        }
    }
}
