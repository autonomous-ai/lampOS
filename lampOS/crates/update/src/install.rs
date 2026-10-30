//! `install`: lampOS on a blank lamp, from the newest signed build of its
//! channel (docs/install.md, task #811).
//!
//! On a lamp with the base image and nothing of lampOS, it installs every
//! part of the product (`deploy/product/`, carried in this program): the
//! build's binary of each, in `/opt/lampOS/<part>/<sha256>/` with the link
//! `current` to it; the product units, the target, the slices and the
//! timer; the tmpfiles of the bus and the locks; the parts' users; and the
//! lamp's zone. It enables the lamp and starts nothing: the lamp starts
//! once setup has written its marker (#806).
//!
//! Run again, it changes nothing that is as it should be, and says so.
//! It never writes what is one lamp's own (`/etc/lampOS/body/lamp.env`) or
//! what setup writes, and leaves a unit's other drop-ins alone: only the
//! reserved `exit-note.conf` is always install's. Store's `store.conf` is
//! installed as a setting and keeps a lamp's edits. A product setting
//! changed on the lamp since an install wrote it is the lamp's own too, and
//! is kept ([`settings`]). A kept drop-in or setting that turns a product
//! switch off (`LAMP_CLOUD=off`) is kept as well, and named
//! ([`switches_off`]).
use crate::{
    fetch::{Channel, Fetch},
    manifest::{self, Manifest},
    records::{Build, Records, State, replace_link, sync_parent, write_whole},
    round::{Hashing, file_sha256, filling},
    unit,
    world::{JOURNAL_WITHIN, RESTART_WITHIN, query},
};
use ring::digest;
use std::{
    fs,
    io::{self, Read},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

/// Every unit of the product, by name, as `deploy/product/` has it at the
/// commit this program was built from.
pub const PRODUCT: &[(&str, &str)] = &[
    (
        "lampos.target",
        include_str!("../../../deploy/product/lampos.target"),
    ),
    (
        "lampos-voice.slice",
        include_str!("../../../deploy/product/lampos-voice.slice"),
    ),
    (
        "lampos-parts.slice",
        include_str!("../../../deploy/product/lampos-parts.slice"),
    ),
    (
        "lamp-voice.service",
        include_str!("../../../deploy/product/lamp-voice.service"),
    ),
    (
        "lamp-body.service",
        include_str!("../../../deploy/product/lamp-body.service"),
    ),
    (
        "lamp-eyes.service",
        include_str!("../../../deploy/product/lamp-eyes.service"),
    ),
    (
        "lamp-name.service",
        include_str!("../../../deploy/product/lamp-name.service"),
    ),
    (
        "lamp-expression.service",
        include_str!("../../../deploy/product/lamp-expression.service"),
    ),
    (
        "lamp-actions.service",
        include_str!("../../../deploy/product/lamp-actions.service"),
    ),
    (
        "lamp-memory.service",
        include_str!("../../../deploy/product/lamp-memory.service"),
    ),
    (
        "lamp-policy.service",
        include_str!("../../../deploy/product/lamp-policy.service"),
    ),
    // The lamp on the company broker, for the Autonomous app (#1396); off
    // unless set.
    (
        "lamp-cloud.service",
        include_str!("../../../deploy/product/lamp-cloud.service"),
    ),
    (
        "lamp-health.service",
        include_str!("../../../deploy/product/lamp-health.service"),
    ),
    (
        "lamp-controls.service",
        include_str!("../../../deploy/product/lamp-controls.service"),
    ),
    (
        "lamp-ears.service",
        include_str!("../../../deploy/product/lamp-ears.service"),
    ),
    (
        "lamp-schedule.service",
        include_str!("../../../deploy/product/lamp-schedule.service"),
    ),
    (
        "lamp-air.service",
        include_str!("../../../deploy/product/lamp-air.service"),
    ),
    // The air's bus at 100 kHz, set as root before the air and the controls
    // start; the same binary as the air's.
    (
        "lamp-air-clock.service",
        include_str!("../../../deploy/product/lamp-air-clock.service"),
    ),
    (
        "lamp-tasks.service",
        include_str!("../../../deploy/product/lamp-tasks.service"),
    ),
    // The add-ons' judge (#2530); off unless set.
    (
        "lamp-addons.service",
        include_str!("../../../deploy/product/lamp-addons.service"),
    ),
    // The lamp's inner state (#1670); off unless set.
    (
        "lamp-mood.service",
        include_str!("../../../deploy/product/lamp-mood.service"),
    ),
    // The developers' door (#3504, docs/control.md); off unless the owner's
    // setting writes LAMP_CONTROL=on.
    (
        "lamp-control.service",
        include_str!("../../../deploy/product/lamp-control.service"),
    ),
    (
        "lamp-skills.service",
        include_str!("../../../deploy/product/lamp-skills.service"),
    ),
    // The skills' manage helper (#1722, #2414): root's, started by its
    // socket for each connection, so the socket is enabled and the template
    // never is. lamp-cloud's skills.remove and a made skill's spoken yes
    // (#1685) ask it.
    (
        "lamp-skills-manage.socket",
        include_str!("../../../deploy/product/lamp-skills-manage.socket"),
    ),
    (
        "lamp-skills-manage@.service",
        include_str!("../../../deploy/product/lamp-skills-manage@.service"),
    ),
    // The eyes' camera helper (#2892): root's, started by its socket for
    // each ask, so the socket is enabled and the template never is.
    // lamp-eyes asks it after a camera stops opening.
    (
        "lamp-eyes-camera.socket",
        include_str!("../../../deploy/product/lamp-eyes-camera.socket"),
    ),
    (
        "lamp-eyes-camera@.service",
        include_str!("../../../deploy/product/lamp-eyes-camera@.service"),
    ),
    (
        "lamp-records.service",
        include_str!("../../../deploy/product/lamp-records.service"),
    ),
    (
        "lamp-records.timer",
        include_str!("../../../deploy/product/lamp-records.timer"),
    ),
    (
        "lamp-update.service",
        include_str!("../../../deploy/product/lamp-update.service"),
    ),
    // lamp-update's fallback, from its fixed path: started by lamp-update's
    // failure and by its timer (#1680, crate::own).
    (
        "lamp-update-back.service",
        include_str!("../../../deploy/product/lamp-update-back.service"),
    ),
    (
        "lamp-update-back.timer",
        include_str!("../../../deploy/product/lamp-update-back.timer"),
    ),
    // The build's settings and tmpfiles, written at a round's switch from
    // the same fixed path, and started only by a round (#1680, crate::apply).
    (
        "lamp-update-apply.service",
        include_str!("../../../deploy/product/lamp-update-apply.service"),
    ),
    // The passive self-test, five minutes after the lamp starts and every
    // six hours (#812). Its service is started by its timer alone.
    (
        "lamp-selftest.service",
        include_str!("../../../deploy/product/lamp-selftest.service"),
    ),
    (
        "lamp-selftest.timer",
        include_str!("../../../deploy/product/lamp-selftest.timer"),
    ),
    // Setup at first boot, its reset, and the start of the lamp once it is
    // set up (#806, #811).
    (
        "lamp-setup.service",
        include_str!("../../../deploy/product/lamp-setup.service"),
    ),
    (
        "lamp-setup-reset.service",
        include_str!("../../../deploy/product/lamp-setup-reset.service"),
    ),
    (
        "lampos-setup-done.path",
        include_str!("../../../deploy/product/lampos-setup-done.path"),
    ),
    // Root's helpers for what the app changes after setup, without a reset
    // (#1959, #2853): the names' and zone's, and the Wi-Fi's with its boot
    // half, which puts the old network back after a cut change. Each socket
    // is enabled and its template never, as the skills' manage helper's.
    (
        "lamp-setup-change.socket",
        include_str!("../../../deploy/product/lamp-setup-change.socket"),
    ),
    (
        "lamp-setup-change@.service",
        include_str!("../../../deploy/product/lamp-setup-change@.service"),
    ),
    (
        "lamp-setup-wifi.socket",
        include_str!("../../../deploy/product/lamp-setup-wifi.socket"),
    ),
    (
        "lamp-setup-wifi@.service",
        include_str!("../../../deploy/product/lamp-setup-wifi@.service"),
    ),
    (
        "lamp-setup-wifi-restore.service",
        include_str!("../../../deploy/product/lamp-setup-wifi-restore.service"),
    ),
    // The waking face, from power-on until the voice takes the ring
    // (#1681); off unless `LAMP_WAKING_FACE=on`.
    (
        "lamp-waking.service",
        include_str!("../../../deploy/product/lamp-waking.service"),
    ),
    // Setup's hotspot again when a set-up lamp's saved network is gone
    // (#3020); off unless `LAMP_WIFI_RECOVERY=on`.
    (
        "lamp-setup-recover.service",
        include_str!("../../../deploy/product/lamp-setup-recover.service"),
    ),
];

/// The fixed services whose stops Cloud may report. The install fixture
/// holds this list equal to `lamp_cloud::reports::UNITS`, including the
/// helpers installed by setup. No prefix drop-in reaches unlisted units.
pub const EXIT_NOTE_UNITS: &[&str] = &[
    "lamp-actions.service",
    "lamp-addons.service",
    "lamp-air-clock.service",
    "lamp-air.service",
    "lamp-body.service",
    "lamp-cloud.service",
    "lamp-control.service",
    "lamp-controls.service",
    "lamp-ears.service",
    "lamp-expression.service",
    "lamp-eyes-camera@.service",
    "lamp-eyes.service",
    "lamp-health.service",
    "lamp-keeper-restart@.service",
    "lamp-memory.service",
    "lamp-mood.service",
    "lamp-name.service",
    "lamp-policy.service",
    "lamp-records.service",
    "lamp-schedule.service",
    "lamp-selftest.service",
    "lamp-setup-change@.service",
    "lamp-setup-keys@.service",
    "lamp-setup-recover.service",
    "lamp-setup-reset.service",
    "lamp-setup-wifi-restore.service",
    "lamp-setup-wifi@.service",
    "lamp-setup.service",
    "lamp-skills-manage@.service",
    "lamp-skills.service",
    "lamp-tasks.service",
    "lamp-update-apply.service",
    "lamp-update-back.service",
    "lamp-update.service",
    "lamp-voice.service",
    "lamp-waking.service",
];
/// The existing recorder command: local bounded notes, never a cloud send.
pub const EXIT_NOTE: &str = include_str!("../../../deploy/product/exit-note.conf");

/// The skills every lamp receives: each a folder of `lampOS/skills/`, put in
/// `/var/lib/lampOS/skills/` whole but for its `tests/` ([`SETTINGS`]).
/// Every core skill is one (the owner: core skills are always installed;
/// `tests/install.rs` holds the two lists equal). Twenty keep their folders'
/// own default: nineteen enabled, and Bedtime and Morning disabled by its own
/// `enabled = false` until its lamp comparison (#1672); those of
/// [`PRODUCT_SKILLS_OFF`] are installed disabled until their lamp checks,
/// so none of their functions reach a session.
/// Every behaviour that has a skill runs as the skill (the owner, 19 Oct):
/// the voice's own hello is off in `lamp-voice.service`, and its reminders
/// and task news wait for the skill's ask (`LAMP_SCHEDULE_SPEAK=skill`,
/// `LAMP_TASK_NEWS=skill`). Prepare-to-unplug is installed but hidden and
/// inactive unless `LAMP_UNPLUG=on`.
pub const PRODUCT_SKILLS: &[&str] = &[
    "welcome-back",
    "reminders",
    "task-news",
    "onboarding",
    "stretch-break",
    "stuffy-air",
    "dance",
    "bedtime-morning",
    // When the lamp sleeps at night (#940): Expression's built-in trigger,
    // moved into a skill. On, as the built-in sleep was.
    "night-sleep",
    // A failed part shown as a creature a little unwell (#1681): row 8 on
    // lamp-52e6 with the lead, 20 Oct 11:35, read it as unwell, not broken.
    "not-feeling-well",
    "prepare-unplug",
    // The startle (#2822): Expression's glance at a sudden sound, its move
    // read from this skill's file when Expression starts.
    "startle",
    // Music sway and watch you leave (#2834): core skills on in the product,
    // each reading what the product's units turn on (the ears' sound
    // classes behind the privacy route, the eyes' activity).
    "watch-you-leave",
    "music-sway",
    // The wellbeing nudges (#1685), each saying one short line since the
    // owner's 25 Oct "talk to the user": water, the eyes, standing up and
    // winding down, with Stretch break's budget of one an hour between them.
    // Their one function, `wellbeing_nudges` (their `[stop]`), is a product
    // session's 32nd of its 32.
    "sip-of-water",
    "rest-your-eyes",
    "stand-up",
    "wind-down",
    // A pet on the head (#3997, Part of #4133): Head pat's line, now and
    // then, and its wiggle at every pet, on `controls/pet`, which the
    // product's lamp-controls says (`LAMP_HEAD_PET=on`); the design owner's
    // yes after the lamp check on lamp-06c1 (29 Oct).
    "head-pat",
    "head-pat-wiggle",
    // Installed disabled ([`PRODUCT_SKILLS_OFF`]).
    "light-out",
    "light-scenes",
    "point-light",
    "light-show",
    "smoky-air",
    "dusty-air",
    "feelings",
];

/// The product skills installed disabled, each with why: install and a
/// round's bundle write [`OFF_LINE`] above the folder's own `skill.toml`,
/// so the folder in `lampOS/skills/` (and a lamp run from it) keeps its own
/// default and its cases. A skill leaves this list once its lamp check has
/// passed; until then a person or a trial turns it on on the lamp itself.
pub const PRODUCT_SKILLS_OFF: &[(&str, &str)] = &[
    (
        "light-out",
        "it reads `room/presence`, which the eyes say only with `LAMP_PRESENCE=on` (#1677), off \
         until the light going out and coming back has been seen on a lamp: someone reading face \
         down at the desk must never lose their light",
    ),
    // #1679's three core skills, built and not yet tried on a lamp: each
    // comes with the product once lamp-4ace has played it with a person.
    (
        "light-scenes",
        "not yet tried on a lamp (#1679), and its function would put a product session past its \
         32 without `LAMP_USE_SKILL=on`, which the product doesn't run",
    ),
    (
        "point-light",
        "not yet tried on a lamp (#1679): its turn needs a lamp-body with `toward` and the lamp's \
         `--gaze`, and its function would put a product session past its 32 without \
         `LAMP_USE_SKILL=on`",
    ),
    (
        "light-show",
        "not yet tried on a lamp (#1679), and its function would put a product session past its \
         32 without `LAMP_USE_SKILL=on`, which the product doesn't run",
    ),
    // #1684's two core skills, merged off (the first builder, #1722): a
    // trial puts them on by hand. Each comes with the product once the four
    // real traces are recorded on a lamp with an air sensor and its
    // provisional numbers are set again from them.
    (
        "smoky-air",
        "not measured for false alarms yet (#1684): its numbers wait for real traces of kettle \
         steam, toast, a candle blown out and a match, recorded on a lamp with an air sensor and \
         replayed",
    ),
    (
        "dusty-air",
        "not measured for false alarms yet (#1684): its hour at 125 ug/m3 waits for the same \
         traces and an office's own days, replayed",
    ),
    // #3691's core skill, built and not yet seen on a lamp.
    (
        "feelings",
        "not yet seen on a lamp (#3691): it reads `mood/state`'s `felt`, which Mood says only with \
         `LAMP_MOOD=on` (and a line's tone with `LAMP_MOOD_TONE=on`), both trial-only; each \
         feeling's move and light wait for an observer's verdict with the video face",
    ),
];

/// The line written above the `skill.toml` of each of
/// [`PRODUCT_SKILLS_OFF`] on a lamp: top-level, so before any table.
macro_rules! off_line {
    () => {
        "# Installed off until its lamp check (lamp-update's install::PRODUCT_SKILLS_OFF).\nenabled = false\n"
    };
}
pub const OFF_LINE: &str = off_line!();

/// The skills of `lampOS/skills/` a lamp does not get yet, each with why.
/// A product session's functions are 31 of the 32 it takes
/// (`tests/functions.rs`): the two tools of Desk tutor and its marks would
/// put the last of them past it, left out without a word.
pub const NOT_PRODUCT_SKILLS: &[(&str, &str)] = &[
    // An installable skill (#3504): its owner chooses it; not in the
    // product's install until it has been tried on a lamp.
    (
        "comfort-break",
        "an installable skill, not yet tried on a lamp (#3504): V1's bathroom-break reminder, \
         which its owner chooses; a played check on a lamp comes first",
    ),
    (
        "desk-tutor",
        "with its marks its two functions would be 33 of a Live session's 32; the full look it \
         reads handwriting from (`LAMP_LOOK_FULL=on` with `lamp-eyes --full-look`) is in the \
         product since #2791's product half",
    ),
    (
        "desk-tutor-marks",
        "it is called only after Desk tutor's look, and Desk tutor isn't in the product",
    ),
    (
        "guard-mode",
        "an installable skill (#1685), not yet tried on a lamp: it needs `room/presence` \
         (`LAMP_PRESENCE=on`), and its texts reach the phone only with `LAMP_MESSAGES=on` in \
         `lamp-cloud`, which carries `messages/out` up as unasked news",
    ),
    (
        "laugh-glance",
        "PARKED (#1669): dropped, the owner never asked to listen for other people laughing \
         (the lead, #1711, 21 Oct); kept, and off by being listed here",
    ),
    // An installable skill (#1685): its owner puts it on; not in the
    // product's install until it has been tried on a lamp.
    (
        "look-for-things",
        "an installable skill, not yet tried on a lamp (#1685): its turns need a lamp-body with \
         `toward` and the lamp's `--gaze`, and its function would put a product session past its \
         32 without `LAMP_USE_SKILL=on`",
    ),
    (
        "meal-check",
        "an installable skill, not yet tried on a lamp (#3504): V1's meal reminder, which its \
         owner chooses; a played check on a lamp comes first",
    ),
    (
        "sit-back",
        "an installable skill, not yet tried on a lamp (#3694): V1's posture reminder, which its \
         owner chooses; a played check on a lamp with a person comes first",
    ),
    (
        "who-was-here",
        "an installable skill, not yet tried on a lamp (#1685): it reads `room/presence`, which the \
         eyes say only with `LAMP_PRESENCE=on`, and its function would put a product session past \
         its 32 without `LAMP_USE_SKILL=on`",
    ),
];

/// One file of a product skill, `lampOS/skills/SKILL/FILE`, and where
/// `lamp-skills` reads it on a lamp: `/var/lib/lampOS/skills/SKILL/FILE`.
macro_rules! skill_file {
    ($skill:literal, $file:literal) => {
        (
            concat!("/var/lib/lampOS/skills/", $skill, "/", $file),
            include_str!(concat!("../../../skills/", $skill, "/", $file)),
        )
    };
}
/// A [`PRODUCT_SKILLS_OFF`] skill's `skill.toml`, with [`OFF_LINE`] above it.
macro_rules! skill_file_off {
    ($skill:literal) => {
        (
            concat!("/var/lib/lampOS/skills/", $skill, "/skill.toml"),
            concat!(
                off_line!(),
                include_str!(concat!("../../../skills/", $skill, "/skill.toml"))
            ),
        )
    };
}

/// The product's settings that no build carries, and where each goes:
/// in `/etc/lampOS/<part>/`, readable by the parts' user (docs/install.md),
/// and the product skills' files.
pub const SETTINGS: &[(&str, &str)] = &[
    (
        "/etc/lampOS/expression/expression.json",
        include_str!("../../../deploy/product/settings/expression.json"),
    ),
    (
        "/etc/lampOS/controls/controls.json",
        include_str!("../../../deploy/product/settings/controls.json"),
    ),
    (
        "/etc/lampOS/controls/controls-pins.json",
        include_str!("../../../deploy/product/settings/controls-pins.json"),
    ),
    // Records' bound on the voice's sessions, without which its timer runs
    // nothing and they grow without end (Twelve on lamp-cb94, the first
    // builder on #705, 17 Oct): 2.0 GB, as on lamp-4ace, and two weeks.
    (
        "/etc/lampOS/records.json",
        include_str!("../../../deploy/product/settings/records.json"),
    ),
    // The product skills ([`PRODUCT_SKILLS`]), every file but their tests,
    // which stay in the repository; `lamp-skills` reloads the folder by
    // itself. Welcome back, the hello on arrival, came first (#811).
    skill_file!("welcome-back", "skill.toml"),
    skill_file!("welcome-back", "SKILL.md"),
    skill_file!("welcome-back", "lights/warm_glow.toml"),
    skill_file!("reminders", "skill.toml"),
    skill_file!("reminders", "SKILL.md"),
    skill_file!("task-news", "skill.toml"),
    skill_file!("task-news", "SKILL.md"),
    skill_file!("onboarding", "skill.toml"),
    skill_file!("onboarding", "SKILL.md"),
    skill_file!("stretch-break", "skill.toml"),
    skill_file!("stretch-break", "SKILL.md"),
    skill_file!("stuffy-air", "skill.toml"),
    skill_file!("stuffy-air", "SKILL.md"),
    skill_file!("dance", "skill.toml"),
    skill_file!("dance", "SKILL.md"),
    skill_file!("bedtime-morning", "skill.toml"),
    skill_file!("bedtime-morning", "SKILL.md"),
    skill_file!("night-sleep", "skill.toml"),
    skill_file!("night-sleep", "SKILL.md"),
    skill_file!("not-feeling-well", "skill.toml"),
    skill_file!("not-feeling-well", "SKILL.md"),
    // Core skill #1674 is installed but not advertised until LAMP_UNPLUG=on.
    skill_file!("prepare-unplug", "skill.toml"),
    skill_file!("prepare-unplug", "SKILL.md"),
    skill_file!("startle", "skill.toml"),
    skill_file!("startle", "SKILL.md"),
    // Installed disabled (PRODUCT_SKILLS_OFF).
    skill_file_off!("light-out"),
    skill_file!("light-out", "SKILL.md"),
    skill_file!("watch-you-leave", "skill.toml"),
    skill_file!("watch-you-leave", "SKILL.md"),
    skill_file!("watch-you-leave", "moves/watch-left.json"),
    skill_file!("watch-you-leave", "moves/watch-right.json"),
    skill_file!("sip-of-water", "skill.toml"),
    skill_file!("sip-of-water", "SKILL.md"),
    skill_file!("rest-your-eyes", "skill.toml"),
    skill_file!("rest-your-eyes", "SKILL.md"),
    skill_file!("stand-up", "skill.toml"),
    skill_file!("stand-up", "SKILL.md"),
    skill_file!("wind-down", "skill.toml"),
    skill_file!("wind-down", "SKILL.md"),
    skill_file!("head-pat", "skill.toml"),
    skill_file!("head-pat", "SKILL.md"),
    skill_file!("head-pat", "README.md"),
    skill_file!("head-pat-wiggle", "skill.toml"),
    skill_file!("head-pat-wiggle", "SKILL.md"),
    skill_file!("head-pat-wiggle", "README.md"),
    skill_file!("head-pat-wiggle", "moves/pat-wiggle.json"),
    skill_file!("music-sway", "skill.toml"),
    skill_file!("music-sway", "SKILL.md"),
    skill_file!("music-sway", "moves/sway.json"),
    skill_file_off!("light-scenes"),
    skill_file!("light-scenes", "SKILL.md"),
    skill_file_off!("point-light"),
    skill_file!("point-light", "SKILL.md"),
    skill_file_off!("light-show"),
    skill_file!("light-show", "SKILL.md"),
    skill_file!("light-show", "shows/calm.toml"),
    skill_file!("light-show", "shows/music.toml"),
    skill_file!("light-show", "shows/party.toml"),
    skill_file_off!("smoky-air"),
    skill_file!("smoky-air", "SKILL.md"),
    skill_file_off!("dusty-air"),
    skill_file!("dusty-air", "SKILL.md"),
    skill_file_off!("feelings"),
    skill_file!("feelings", "SKILL.md"),
    skill_file!("feelings", "moves/glad-lift.json"),
    skill_file!("feelings", "moves/perk.json"),
    skill_file!("feelings", "moves/low-droop.json"),
];

/// Setup's own page (docs/setup.md, "On a lamp"), written only with
/// `install --setup on`: with it, the reset plan ([`RESET_PLAN`]) and no
/// marker, `lamp-setup.service` starts the hotspot at boot. Off unless set,
/// so rerunning install to upgrade the updater on a lamp set up by hand
/// never puts it into setup (#811).
pub const SETUP_SETTINGS: &[(&str, &str)] = &[(
    "/etc/lampOS/setup/serve.json",
    include_str!("../../setup/fixtures/serve-own-page.json"),
)];
/// Setup's reset plan, at [`manifest::RESET_PLAN`]: what the 5 s reset
/// wipes. The product's and never the person's (#806): install writes it
/// with `--setup on`, and over any plan a lamp has, the old one kept in the
/// records first; a lamp with none and no `--setup on` (set up by hand)
/// gets none, since with setup's marker a plan arms the reset. A build
/// carries it too (`reset_plan`, crate::apply).
pub const RESET_PLAN: &str = include_str!("../../setup/fixtures/reset-plan-product.json");
/// Every SHA-256 of `crates/setup/fixtures/reset-plan-product.json` in git
/// (`git log -p --follow` on it), the newest first: a lamp's plan that is
/// one of these is a plan an earlier build shipped, said as such when the
/// build's is written over it. Add the new one first whenever the fixture
/// changes; never remove one.
pub const RESET_PLAN_SHIPPED: &[&str] = &[
    "48e84ee08af2ee5ac35a249e0407a3fa14dac67dcd7e5be0daa50dcc843bd5e2", // the owner's developer setting (lamp-control's drop-in) forgotten on reset
    "e8d1da28131f4a5da8271730b6dc85bdd32f574f0c1792287fa1cf923c42ca52", // lamp-control's state (paired computers) forgotten on reset
    "cf1adcb3075bff001ae625c90ddab1e956a25c9b3bcf633ce07c1228737d0adc", // #4333: the owner's place beside the name
    "f82d89481380f2a6851ebc09ffa400056494f55c2ff6552d8b58fed2742ebd6b", // #2948 carried onto lamp-voice 23 Oct: head touch on (#711)
    "ad3fd11185bdf206f8622fef2c6d9bafac4efb7742e65b1c7df11de383cc83e6", // combined people temporaries and face expressions, 23 Oct
    "52645fae3f61d823ae08b920f5af8efa179cc47a8dd3ba96af29910e42ab16d1", // #3072 (people temporaries), 22 Oct
    "a681f484a5e1cd404f5117fb0efb90eca93ae9bbd1862e6f447bed05e0986a24", // #2947 (face-expressions.json), 22 Oct
    "52045bc1c42014eab6454a0fd21ee3be7f75d2ece76af35e455f0a4ebe6c6890", // #1362 carried onto #2754's, 22 Oct: the voice's mute record emptied
    "7c24a8315ceb8fa6ff9d119370989f30cdad6d9390610ab67a607a99729cc5d8", // #2754 (glance.log), 22 Oct
    "881c5fd156474fcd8484e41f77e34f952f69aa85c6d71f3e33ceb0d7bfb9bd9d", // 64cefadfc, 21 Oct
    "60be2ab39e759a6161d249710e5babfec042c944baa62450779ed7a4478eea22", // d06e9d53e, 21 Oct
    "d5d911cdd1adb34395127e6d02c2936c826b13fa9f0fc34cf5bc1fc1d92254e4", // ce05f063e, 21 Oct
    "4845d51735c6f369184f2d82fc7fb4d4922cd69460796ca699c8b5d267959120", // c4fd23ac2, 20 Oct
    "de872e78cb60d724ce35d49ebff92be6cfba6caf5e38824c1e5d06cca60e7506", // ef6a2b9a7, 20 Oct: lamp-0c89's
    "dd6c5fd360b6ba7b60c8e4d6330d992babf25e5fd21ac0d01d925dfc7cf20a7f", // 4de994a4d, 20 Oct
    "67ed9881a61b82847fdf42a461cd9bbec4e84fb13dbca62c500b089a61abff4f", // 6b7e02655, 19 Oct
    "e1db47e6dd266a5bbad77fba42ae5e1a4bd7c3091fa5bbc5e6c12c4c5061f4ac", // ff05ecea9, 19 Oct
    "2bd3145d083626b82e88b067c37dd55dc1e701012c1447fdd5ccc05a93fb65d1", // 2e754384b, 19 Oct
    "5dff6f81202528430637a63818bb7f159a912fb4c83dcde6386d56ab9ac4e3d4", // 4de8ebe1c, 19 Oct: lamp-06c1's
    "6cce76f0751c44a53d4b35965a8c4819a46ff3dd562aa5483c928d23e37c0229", // 1f2bba3e0, 18 Oct
    "199913e2f9a49ba4865c574046ba76c14b046fa19c7d6cde7fb81169a9e03d92", // c11cadbbd, 18 Oct
    "5b77f93a80c524b593f685a8a1885db64c5a89a17bb4e5d7e9b8abdf4c3a58be", // a805cdc98, 18 Oct
    "dea03e06a79e48659ede0b0cbb2be6dd8be165bcb15e368f5bb8140ce2ffbc05", // 7183d2219, 18 Oct
    "dd547cb194c666b0d89c0010570d0c3588c054f9fcbceff395f6ebb6ec7ff061", // ac1f106bb, 17 Oct
    "12377352390c46a4b610a13565ea27846b14e01176b449adf5788c56f7e519bc", // 572fe829e, 16 Oct
    "b3a24f7b04d5431fc18c76e91b5ef14d86fbf9b40f85a97e542dc4ab7b6f2632", // b1d9bb2e5, 16 Oct
    "8dfd3d03d80f06a852be4b92e4d803633d2776177c8e8e0ed349a9e8700d4f78", // 3a6e28adf, 16 Oct
    "0fa5b5f7d1e8d4e9a6452505a7cb4a2f3a1e283453eebb0e14344dafc84668a8", // 10596b41a, 16 Oct
    "e9c7404b207eb40b66813011220e3021deceb969ec7c939006577669ccad959c", // 831e92295, 16 Oct
];

/// The reset plan as a build carries it (`reset_plan`, #806).
pub fn reset_plan_setting() -> manifest::Setting {
    manifest::Setting::new(manifest::RESET_PLAN, RESET_PLAN)
}
/// Models no build carries (#811): where each goes, where it is fetched from,
/// and its size and SHA-256 (`deploy/product/models.json`): one release of
/// ours, with the name part's own `name.onnx`, the one the lamps run.
pub const MODELS: &str = include_str!("../../../deploy/product/models.json");

#[derive(serde::Deserialize)]
struct Models {
    models: Vec<Model>,
}

/// One model of [`MODELS`]: the manifest's own, which a build may carry.
pub use crate::manifest::{Archive, Model};

/// The models of [`MODELS`].
pub fn models() -> Result<Vec<Model>, String> {
    models_from(MODELS).map_err(|error| format!("deploy/product/models.json: {error}"))
}

/// The models of a file in `models.json`'s form.
pub fn models_from(text: &str) -> Result<Vec<Model>, String> {
    serde_json::from_str::<Models>(text)
        .map(|list| list.models)
        .map_err(|error| error.to_string())
}

/// Store preparation belongs to install, not a channel's settings manifest:
/// these paths are outside the updater's setting allowlist. The signing key
/// is never a default, and changes to these two files are the lamp's own.
pub const STORE_SETTINGS: &[(&str, &str)] = &[
    // Store preparation only: both readers stay off until the shared switch
    // is explicitly on and the separate factory-provisioned key is valid.
    (
        "/etc/lampOS/skills/store.env",
        include_str!("../../../deploy/product/settings/store.env"),
    ),
    (
        "/etc/systemd/system/lamp-skills-manage@.service.d/store.conf",
        include_str!("../../../deploy/product/settings/store.conf"),
    ),
];

/// The product's settings as a build carries them (#1680, crate::apply):
/// every one of [`SETTINGS`] and the tmpfiles, 0644. Never setup's, the
/// voice's service, the keys or the zone, which are the lamp's; nor the
/// product skills' files, which a round places from the build's
/// `lamp-product-skills` (crate::product_skills) and the skills guard judges.
pub fn product_settings() -> Vec<manifest::Setting> {
    let skills = format!("{}/", crate::product_skills::FOLDER);
    SETTINGS
        .iter()
        .filter(|(path, _)| !path.starts_with(&skills))
        .chain([&(TMPFILES_PATH, TMPFILES)])
        .map(|(path, text)| manifest::Setting::new(path, text))
        .collect()
}

/// Where the repository keeps the Skills Store's public key a build ships
/// (#3692), under `lamp-update manifest`'s `--deploy` (`lampOS/deploy`).
/// Not in the repository until the Store's keeper gives its key; a build
/// made without it carries none and leaves a lamp's key as it is.
pub const STORE_KEY_SOURCE: &str = "product/settings/var/lib/lampOS/skills/store-key.pub";
/// Where a build writes it: lamp-skills' `package::BUILD_STORE_KEY` in the
/// product's skills folder, a place a settings writer from build 7071 on
/// takes (`manifest::SETTING_FOLDERS`), so no newer `lamp-update-back` is
/// needed for it.
pub const STORE_KEY_SETTING: &str = "/var/lib/lampOS/skills/store-key.pub";

/// The Store's key as a build carries it, from [`STORE_KEY_SOURCE`] under
/// `deploy`, 0644: none when the repository has no such file, and refused
/// unless it is one or two keys, one a line, as a lamp reads it
/// (`manifest::read_public_keys`), so a build never ships a key file every
/// lamp would refuse.
pub fn store_key_setting(deploy: &Path) -> Result<Option<manifest::Setting>, String> {
    let source = deploy.join(STORE_KEY_SOURCE);
    let text = match fs::read_to_string(&source) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: {error}", source.display())),
    };
    manifest::read_public_keys(&text).map_err(|why| format!("{}: {why}", source.display()))?;
    Ok(Some(manifest::Setting::new(STORE_KEY_SETTING, &text)))
}

/// `/etc/tmpfiles.d/lampos.conf`: the bus root and the locks.
pub const TMPFILES: &str = include_str!("../../../deploy/product/lampos.conf");
pub const TMPFILES_PATH: &str = "/etc/tmpfiles.d/lampos.conf";
/// The lamp's zone, for the parts that take it (docs/install.md).
pub const ZONE_PATH: &str = "/etc/lampOS/zone.env";
/// The voice's service: V1's route, the campaign-api proxy with the user's
/// key. Private, as the voice requires (`read_private`: the process user's,
/// 0600), kept up to date while it is install's own.
pub const PROVIDER_PATH: &str = "/etc/lampOS/voice/provider.json";
pub const PROVIDER: &str = include_str!("../../../deploy/product/settings/voice-provider.json");
/// The second route's services, the lamps' voice since 19 Oct (#1712): the
/// relay, every service on the same key as `provider.json`'s
/// ([`CREDENTIAL_PATH`]). Private and kept as `provider.json` is: up to date
/// while it is install's own, and a lamp's own (lamp-4ace's) kept.
pub const SECOND_PATH: &str = "/etc/lampOS/voice/second.json";
/// Where the voice's unit names it: the round places it when the build's
/// voice unit does (`round::Round::place_second`).
pub const SECOND_CONFIG_ENV: &str = "LAMP_VOICE_SECOND_CONFIG";
pub const SECOND: &str = include_str!("../../../deploy/product/settings/second.json");
/// The voice's key, as `provider.json` names it. It is the user's (the owner on
/// #806, 2026-10-16 16:13): setup takes it from the account page or the app,
/// and a reset wipes it. On a lamp that ran V1, install copies V1's
/// `llm_api_key` once, into a lamp that never had one.
pub const CREDENTIAL_PATH: &str = "/etc/lampOS/voice/credential";
/// The Policy's judge key: the same key, for V1's route to Jev
/// (`deploy/product/lamp-policy.service`; the first builder on #811, 13:09:
/// provision it like the voice's).
pub const JEV_KEY_PATH: &str = "/etc/lampOS/policy/jev.key";
/// What each key path is recorded as when the records were set aside: once
/// written, with no hash any file can match. So the rule for a key removed
/// since it was written holds on every later run, not only on the run that
/// found the damage, which saves records with no key in them (Third on
/// #1263). A key setup writes later is kept as the lamp's own.
pub const SET_ASIDE: &str = "unknown: the records were set aside";
/// The key paths [`SET_ASIDE`] marks: the voice's and the Policy's.
pub const KEY_PATHS: [&str; 2] = [CREDENTIAL_PATH, JEV_KEY_PATH];

/// Whether the records in `folder` were set aside: by this run's read
/// (`existed`, and now gone), or by an earlier run that could not save what
/// it marked, which left no `state.json` and a `state.json.corrupt-*` beside
/// it (the reviewer on #1263). Either way, what was written before is not
/// known.
fn records_lost(folder: &Path, existed: bool) -> bool {
    if folder.join("state.json").exists() {
        return false;
    }
    existed
        || fs::read_dir(folder).is_ok_and(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("state.json.corrupt-")
            })
        })
}
pub const V1_CONFIG: &str = "/root/config/config.json";
/// Where lamp-update keeps its records on a lamp.
pub const RECORDS: &str = "/var/lib/lampos-update/";
pub const UNITS: &str = "/etc/systemd/system/";

/// The parts' user, and memory's own.
pub const USER: &str = "lampos";
pub const MEMORY_USER: &str = "lampos-memory";
/// The groups `lampos` is put in, each only if the image has it.
pub const GROUPS: &[&str] = &["audio", "video", "dialout", "i2c", "gpio"];

/// The Harness program, as the tasks unit names it, and the home it runs
/// in: the tasks part's state folder (`deploy/product/lamp-tasks.service`).
pub const HARNESS: &str = "/usr/local/bin/harness";
pub const HARNESS_HOME: &str = "/var/lib/lampos-tasks";
/// Harness's own installer, the address its header names as where it is
/// published (openharness `cli/scripts/install.sh`). Per user, with no
/// root and no question asked; what it fetches is checked against Harness's
/// sha256 metadata (#1676, the lead on #1711).
pub const HARNESS_INSTALLER: &str = "https://cdn.autonomous.ai/harness/cli/install.sh";
/// The oldest Harness that signs a lamp in by a spoken code (`harness login
/// --code`, Harness 0.4.23, openharness#1410; `lamp-tasks serve
/// --sign-in-by-code`, #1676). `harness-on` brings an older one it put there
/// up to the version Harness's own installer ships.
pub const HARNESS_LEAST: (u32, u32, u32) = (0, 4, 23);

/// The version `harness version` prints ("0.4.23", or with a `v` or a word
/// before it): its first `a.b.c`.
pub fn harness_version_of(text: &str) -> Option<(u32, u32, u32)> {
    text.split_whitespace().find_map(|word| {
        let mut parts = word.trim_start_matches('v').split('.');
        let number = |part: Option<&str>| part?.parse::<u32>().ok();
        let version = (number(parts.next())?, number(parts.next())?, {
            // "23-beta.1" is 23.
            let patch = parts.next()?;
            let digits: String = patch.chars().take_while(char::is_ascii_digit).collect();
            digits.parse::<u32>().ok()?
        });
        Some(version)
    })
}

fn shown_version(version: Option<(u32, u32, u32)>) -> String {
    version.map_or_else(
        || "of a version it doesn't say".to_owned(),
        |(major, minor, patch)| format!("{major}.{minor}.{patch}"),
    )
}

/// The part that hands work to Harness, started again once Harness is on.
const TASKS_UNIT: &str = "lamp-tasks.service";
/// What says Harness was set to start at boot: written in lamp-update's
/// records only after `harness service install` succeeded. Not the files
/// the CLI leaves: it writes its unit before registering it, and keeps it
/// when a later step fails (the reviewer on #934), so they prove nothing.
const HARNESS_DONE: &str = "harness-at-boot";

/// Binaries a build carries that live in another part's folder: the voice
/// starts its turn-end worker from its own (docs/endpoint-detection.md). A
/// build without one is refused whole.
const BESIDE: &[(&str, &[&str])] = &[("lamp-voice", &["lamp-turn-end"])];

/// Tools put beside a part when the build carries them, and left out, said
/// once, when it does not: the factory's steps run the body's from the
/// body's folder (`deploy/factory/steps.sh`, #2189), and a build from before
/// they were made (19 Oct) must still install, as a lamp put back on one
/// does (the reviewer on #2249).
const BESIDE_IF_CARRIED: &[(&str, &[&str])] =
    &[("lamp-body", &["lamp-body-measure", "lamp-body-mood"])];

/// Whether a list of units holds the Wi-Fi helper only with its boot half
/// listed and enabled (the first builder's P3 4 on #2617).
#[cfg(test)]
fn wifi_helper_whole(listed: &[&str], enabled: &[String]) -> bool {
    const RESTORE: &str = "lamp-setup-wifi-restore.service";
    let helper = listed
        .iter()
        .any(|name| matches!(*name, "lamp-setup-wifi.socket" | "lamp-setup-wifi@.service"));
    !helper || (listed.contains(&RESTORE) && enabled.iter().any(|name| name == RESTORE))
}

/// Services started by a timer of their own, and never enabled themselves.
/// A template (`NAME@.service`) is started by its socket, never enabled.
const TIMED: &[&str] = &[
    "lamp-records.service",
    "lamp-selftest.service",
    "lamp-update-back.service",
];

/// Services a round starts, and never enabled.
const STARTED_BY_A_ROUND: &[&str] = &[crate::apply::UNIT];

/// The units enabled: the target, and what starts outside it.
pub(crate) fn enabled() -> Vec<String> {
    PRODUCT
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| {
            name.ends_with(".service")
                && !name.contains('@')
                && !TIMED.contains(name)
                && !STARTED_BY_A_ROUND.contains(name)
        })
        .chain([
            "lampos.target",
            "lamp-records.timer",
            "lamp-selftest.timer",
            "lamp-update-back.timer",
            "lampos-setup-done.path",
            "lamp-skills-manage.socket",
            "lamp-setup-change.socket",
            "lamp-setup-wifi.socket",
            "lamp-eyes-camera.socket",
        ])
        .map(str::to_owned)
        .collect()
}

/// What a product unit starts: `/opt/lampOS/<part>/current/<binary>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    /// The part's folder: `/opt/lampOS/voice/`.
    pub folder: String,
    /// `lamp-voice`.
    pub binary: String,
}

/// The part a unit's first `ExecStart=` starts through its `current` link.
pub fn part(text: &str) -> Option<Part> {
    unit::linked(text).map(|(folder, binary)| Part { folder, binary })
}

/// Every part of the product, in the order of `PRODUCT`, each once: setup,
/// its reset and the waking face start the same binary.
pub fn parts() -> Vec<Part> {
    let mut parts: Vec<Part> = Vec::new();
    for part in PRODUCT.iter().filter_map(|(_, text)| part(text)) {
        if !parts.contains(&part) {
            parts.push(part);
        }
    }
    parts
}

/// What `install` asks of the machine, behind a trait so that the tests
/// install onto a folder.
pub trait Host {
    /// A system user with its own group, no login and no home, in `groups`.
    fn add_user(&mut self, name: &str, groups: &[String]) -> Result<(), String>;
    /// `systemctl daemon-reload`, then `systemctl enable` each unit.
    fn enable(&mut self, units: &[String]) -> Result<(), String>;
    /// `systemd-tmpfiles --create` of one file.
    fn tmpfiles(&mut self, path: &str) -> Result<(), String>;
    /// The zone's offset from UTC now, in minutes.
    fn utc_offset_minutes(&mut self, zone: &str) -> Result<i32, String>;
    /// Harness at boot for `user` (#811, the first builder's recipe of
    /// 01:56): `home` made the user's own, `loginctl enable-linger`, then
    /// the Harness CLI's own `harness service install` as `user` with that
    /// `HOME`. Never a unit of this program's writing.
    fn harness_at_boot(&mut self, user: &str, home: &str) -> Result<(), String>;
    /// Undoes [`Host::harness_at_boot`] (`lamp-update harness-off`): as
    /// `user` with that `HOME`, the Harness CLI's own `harness service
    /// uninstall`, then `harness stop`, since the uninstall starts the daemon
    /// again the usual way (Harness 0.4.17, Nine on #705); then `loginctl
    /// disable-linger`. Then what is left of it, a line each: the service
    /// Harness still has, or a daemon still running as `user`. Never stopped
    /// here: stopping Harness is Harness's own.
    fn harness_off(&mut self, user: &str, home: &str) -> Result<Vec<String>, String>;
    /// Harness on the lamp for `user` (`lamp-update harness-on`, #1676):
    /// `home` made the user's own, Harness's own installer run as `user`
    /// with that `HOME`, then [`HARNESS`] linked to the launcher it wrote.
    /// Never a copy of Harness of this program's own.
    fn harness_on(&mut self, user: &str, home: &str) -> Result<(), String>;
    /// What `harness version` prints, as `user` with that `HOME` (#1676).
    fn harness_version(&mut self, user: &str, home: &str) -> Result<String, String>;
    /// `systemctl restart` of one unit.
    fn restart(&mut self, unit: &str) -> Result<(), String>;
    /// The sha256 of the `lamp-update` running this install, as hex: install
    /// writes the units it carries itself ([`PRODUCT`]), so one that is not
    /// the build's own is said ([`not_the_builds_updater`], #3733).
    fn running_sha256(&mut self) -> Result<String, String>;
    /// Whether this run was handed over to already ([`HANDED_OVER`] set): it
    /// hands over no further (#3733).
    fn handed_over(&mut self) -> bool;
    /// Runs `exe`, the build's own lamp-update, in this process's place with
    /// the same arguments and [`HANDED_OVER`] set, so it writes its own
    /// units (#3733). Returns only when that could not be done, with why.
    fn hand_over(&mut self, exe: &Path) -> String;
}

/// Set on the lamp-update an install hands over to ([`Host::hand_over`]):
/// it hands over no further, whatever it finds.
pub const HANDED_OVER: &str = "LAMP_UPDATE_HANDED_OVER";

/// The machine itself: only with the root `/`.
pub struct System;

impl Host for System {
    fn add_user(&mut self, name: &str, groups: &[String]) -> Result<(), String> {
        let mut command = Command::new("useradd");
        command.args(["--system", "--user-group", "--no-create-home"]);
        command.args(["--home-dir", "/nonexistent", "--shell", "/usr/sbin/nologin"]);
        if !groups.is_empty() {
            command.args(["--groups", &groups.join(",")]);
        }
        run(command.arg(name))
    }

    fn enable(&mut self, units: &[String]) -> Result<(), String> {
        run(Command::new("systemctl").arg("daemon-reload"))?;
        run(Command::new("systemctl").arg("enable").args(units))
    }

    fn tmpfiles(&mut self, path: &str) -> Result<(), String> {
        run_within(
            Command::new("systemd-tmpfiles").args(["--create", path]),
            JOURNAL_WITHIN,
        )
    }

    fn harness_at_boot(&mut self, user: &str, home: &str) -> Result<(), String> {
        harness_at_boot_with(&mut output_of, Path::new("/"), user, home)
    }

    fn harness_off(&mut self, user: &str, home: &str) -> Result<Vec<String>, String> {
        harness_off_with(&mut output_of, user, home)
    }

    fn harness_on(&mut self, user: &str, home: &str) -> Result<(), String> {
        harness_on_with(&mut output_of, user, home)
    }

    fn harness_version(&mut self, user: &str, home: &str) -> Result<String, String> {
        let mut quick = |argv: &[String]| output_of_within(argv, VERSION_WITHIN);
        let uid = uid_of(&mut quick, user)?;
        quick(&harness_argv(user, home, &uid, &["version"]))
    }

    fn restart(&mut self, unit: &str) -> Result<(), String> {
        run(Command::new("systemctl").args(["restart", unit]))
    }

    fn running_sha256(&mut self) -> Result<String, String> {
        // `/proc/self/exe` opens the file this process runs even after a
        // newer one took its path; elsewhere, the path it was started from.
        let proc_exe = Path::new("/proc/self/exe");
        let path = if proc_exe.exists() {
            proc_exe.to_path_buf()
        } else {
            std::env::current_exe().map_err(|error| error.to_string())?
        };
        file_sha256(&path).map_err(|error| format!("{}: {error}", path.display()))
    }

    fn handed_over(&mut self) -> bool {
        std::env::var_os(HANDED_OVER).is_some()
    }

    fn hand_over(&mut self, exe: &Path) -> String {
        use std::os::unix::process::CommandExt;
        // The records' lock is opened close-on-exec, so it is let go here
        // and the build's own lamp-update takes it again.
        let error = Command::new(exe)
            .args(std::env::args_os().skip(1))
            .env(HANDED_OVER, "1")
            .exec();
        format!("{}: {error}", exe.display())
    }

    fn utc_offset_minutes(&mut self, zone: &str) -> Result<i32, String> {
        // A failed `date` prints no offset, as before; only one that cannot
        // start, or runs past the bound, says so.
        let stdout = match query::output_within(
            Command::new("date").arg("+%z").env("TZ", zone),
            Instant::now() + JOURNAL_WITHIN,
        ) {
            Ok(stdout) => stdout,
            Err(error) if error.kind() == io::ErrorKind::Other => Vec::new(),
            Err(error) => return Err(shown_error("date +%z", JOURNAL_WITHIN, &error)),
        };
        offset_minutes(String::from_utf8_lossy(&stdout).trim())
            .ok_or_else(|| format!("date gave no offset for {zone}"))
    }
}

/// The Harness CLI as `user`, with `home` as its `HOME` and the user's
/// own runtime folder (`/run/user/<uid>`), which its per-user service needs.
fn harness_argv(user: &str, home: &str, uid: &str, args: &[&str]) -> Vec<String> {
    let mut argv: Vec<String> = ["runuser", "-u", user, "--", "env"]
        .map(str::to_owned)
        .into();
    argv.push(format!("HOME={home}"));
    argv.push(format!("XDG_RUNTIME_DIR=/run/user/{uid}"));
    argv.push(HARNESS.to_owned());
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    argv
}

/// `user`'s id, from `id -u`, through `run`.
fn uid_of(run: &mut Runner, user: &str) -> Result<String, String> {
    let uid = run(&["id".to_owned(), "-u".to_owned(), user.to_owned()])?
        .trim()
        .to_owned();
    if uid.is_empty() || !uid.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("id -u {user} gave no user id"));
    }
    Ok(uid)
}

/// One program and its arguments, run to its end: what it printed when it
/// ended with 0, else why not. The real one is [`output_of`]; a test's logs
/// and fails on demand.
pub type Runner<'a> = dyn FnMut(&[String]) -> Result<String, String> + 'a;

/// The longest `harness version` may take: a cold start under `runuser` on
/// a busy lamp. One past it is not known, and the Harness there is kept.
pub const VERSION_WITHIN: Duration = Duration::from_secs(30);

/// The longest Harness's installer (`curl … | sh`) may take: it fetches over
/// the lamp's own network, which can be slow.
pub const INSTALLER_WITHIN: Duration = Duration::from_secs(15 * 60);

/// Every step through it gets [`RESTART_WITHIN`], but Harness's installer,
/// which fetches, gets [`INSTALLER_WITHIN`].
fn output_of(argv: &[String]) -> Result<String, String> {
    let fetches = argv
        .last()
        .is_some_and(|last| last.contains(HARNESS_INSTALLER));
    output_of_within(
        argv,
        if fetches {
            INSTALLER_WITHIN
        } else {
            RESTART_WITHIN
        },
    )
}

fn output_of_within(argv: &[String], within: Duration) -> Result<String, String> {
    let shown = argv.join(" ");
    query::output_and_stderr_within(
        Command::new(&argv[0]).args(&argv[1..]),
        Instant::now() + within,
    )
    .map(|stdout| String::from_utf8_lossy(&stdout).into_owned())
    .map_err(|error| shown_error(&shown, within, &error))
}

/// A bounded command's error as `"{shown}: …"`: its own trimmed stderr (or
/// exit status) when it failed, that it ran past `within` (its group then
/// killed), or why it could not run.
/// What an install command that ran past its bound says, and how
/// [`timed_out`] knows it.
const DID_NOT_END: &str = "did not end within";

/// Whether `why` is a command that ran past its bound: not known, rather
/// than failed.
pub fn timed_out(why: &str) -> bool {
    why.contains(DID_NOT_END)
}

fn shown_error(shown: &str, within: Duration, error: &io::Error) -> String {
    if error.kind() == io::ErrorKind::TimedOut {
        format!(
            "{shown}: {DID_NOT_END} {} s: it ran on, or a child it left held its output",
            within.as_secs()
        )
    } else {
        format!("{shown}: {error}")
    }
}

/// Where `user`'s own user manager is given `home` as its `HOME` (#1676):
/// a drop-in for `user@<uid>.service` alone. `lampos` has no home of its
/// own (`/nonexistent`), so without it the manager looks for user units
/// under `/nonexistent/.config/systemd/user`, never where Harness writes
/// its unit under `home` ("Unit file harnessd.service does not exist", on
/// lamp-75c9 on 19 Oct). The parts that run as `lampos` keep their `HOME`:
/// only this manager, which runs nothing but Harness, gets it.
pub fn home_drop_in(uid: &str) -> String {
    format!("/etc/systemd/system/user@{uid}.service.d/lamp-home.conf")
}

/// [`Host::harness_at_boot`]'s steps, each through `run`, under `root` (`/`
/// on the lamp): `id -u`; `home` made `user`'s own (0700); the manager's
/// `HOME` drop-in ([`home_drop_in`]) written, `systemctl daemon-reload`,
/// and `systemctl try-restart` of `user@<uid>.service`, so a manager
/// already running (its linger on before) takes it, and one not running
/// is left alone; `loginctl enable-linger`, which starts it with that
/// `HOME`; then the Harness CLI's own `harness service install` as `user`
/// with that `HOME`. Shown on lamp-75c9 on 19 Oct: the manager keeps the
/// drop-in's `HOME`, and Harness's service then installs and runs. A
/// failing step stops it there.
pub fn harness_at_boot_with(
    run: &mut Runner,
    root: &Path,
    user: &str,
    home: &str,
) -> Result<(), String> {
    let uid = uid_of(run, user)?;
    run(&["install", "-d", "-o", user, "-g", user, "-m", "0700", home].map(str::to_owned))?;
    let drop_in = root.join(home_drop_in(&uid).trim_start_matches('/'));
    let folder = drop_in.parent().expect("the drop-in is in a folder");
    std::fs::create_dir_all(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    write_whole(
        &drop_in,
        format!("[Service]\nEnvironment=HOME={home}\n").as_bytes(),
        0o644,
    )
    .map_err(|error| format!("{}: {error}", drop_in.display()))?;
    run(&["systemctl", "daemon-reload"].map(str::to_owned))?;
    run(&[
        "systemctl".to_owned(),
        "try-restart".to_owned(),
        format!("user@{uid}.service"),
    ])?;
    // A user manager for the user from boot, with no login: the daemon's
    // user unit needs it, a system user as any other.
    run(&["loginctl", "enable-linger", user].map(str::to_owned))?;
    run(&harness_argv(user, home, &uid, &["service", "install"]))?;
    Ok(())
}

/// [`Host::harness_off`]'s steps, each through `run`, in this order: `id -u`,
/// then as `user` `harness service uninstall` and `harness stop` (with no
/// daemon running it says so and ends with 0), then `harness service status
/// --json` while the user's manager is still there, then `loginctl
/// disable-linger`, which ends it, then the manager's `HOME` drop-in
/// removed ([`home_drop_in`]) and `systemctl daemon-reload`, then `pgrep -u
/// <user> -f harnessd`. A
/// failing step stops it there. The status not answering, and what it and
/// `pgrep` still find, are the lines returned.
pub fn harness_off_with(run: &mut Runner, user: &str, home: &str) -> Result<Vec<String>, String> {
    let uid = uid_of(run, user)?;
    run(&harness_argv(user, home, &uid, &["service", "uninstall"]))?;
    run(&harness_argv(user, home, &uid, &["stop"]))?;
    let mut left = match run(&harness_argv(
        user,
        home,
        &uid,
        &["service", "status", "--json"],
    )) {
        Ok(json) => service_left(&json),
        Err(_) => vec!["`harness service status --json` did not answer".to_owned()],
    };
    run(&["loginctl", "disable-linger", user].map(str::to_owned))?;
    // The manager's `HOME` goes with it ([`home_drop_in`]).
    run(&["rm".to_owned(), "-f".to_owned(), home_drop_in(&uid)])?;
    run(&["systemctl", "daemon-reload"].map(str::to_owned))?;
    // `pgrep` ends with 1 when it finds nothing: then there is no list.
    if let Ok(found) = run(&["pgrep", "-u", user, "-f", "harnessd"].map(str::to_owned)) {
        let pids: Vec<&str> = found.split_whitespace().collect();
        if !pids.is_empty() {
            left.push(format!(
                "harnessd still runs as {user}: pid {}",
                pids.join(", ")
            ));
        }
    }
    Ok(left)
}

/// [`Host::harness_on`]'s steps, each through `run`, in this order: `id -u`;
/// `home` made `user`'s own, 0700; as `user`, with `home` as its `HOME`, its
/// runtime folder, a plain `PATH` and `SHELL=/bin/sh` (so the installer's one
/// PATH line goes in `home`'s `.profile`), Harness's installer fetched and
/// run (`curl … | sh`, as Harness says to install it); its launcher checked;
/// then [`HARNESS`] linked to it. A failing step stops it there, before the
/// link, so a lamp is never left with a link to nothing.
pub fn harness_on_with(run: &mut Runner, user: &str, home: &str) -> Result<(), String> {
    let uid = uid_of(run, user)?;
    run(&["install", "-d", "-o", user, "-g", user, "-m", "0700", home].map(str::to_owned))?;
    let mut installer: Vec<String> = ["runuser", "-u", user, "--", "env"]
        .map(str::to_owned)
        .into();
    installer.extend([
        format!("HOME={home}"),
        format!("XDG_RUNTIME_DIR=/run/user/{uid}"),
        "PATH=/usr/local/bin:/usr/bin:/bin".to_owned(),
        "SHELL=/bin/sh".to_owned(),
        "sh".to_owned(),
        "-c".to_owned(),
        format!("curl -fsSL --proto =https --tlsv1.2 {HARNESS_INSTALLER} | sh"),
    ]);
    run(&installer)?;
    let launcher = format!("{home}/.local/bin/harness");
    run(&["test".to_owned(), "-x".to_owned(), launcher.clone()])?;
    run(&[
        "ln".to_owned(),
        "-sfn".to_owned(),
        launcher,
        HARNESS.to_owned(),
    ])?;
    Ok(())
}

/// What `harness service status --json` still says of Harness's service:
/// nothing when it is not installed, registered or running, else one line.
/// Its fields are the CLI's own (`cli/src/lib/serviceCommand.ts`).
pub fn service_left(json: &str) -> Vec<String> {
    let Ok(status) = serde_json::from_str::<serde_json::Value>(json) else {
        return vec!["`harness service status --json` said nothing readable".to_owned()];
    };
    let flag = |name: &str| status[name].as_bool() == Some(true);
    if !flag("installed") && !flag("registered") && !flag("running") {
        return Vec::new();
    }
    let pid = status["pid"]
        .as_u64()
        .map_or(String::new(), |pid| format!(", pid {pid}"));
    vec![format!(
        "Harness still has its service: installed {}, registered {}, running {}{pid}",
        flag("installed"),
        flag("registered"),
        flag("running")
    )]
}

/// `command` run to its end within [`RESTART_WITHIN`]: `systemctl`
/// restarts, reloads and enables, and `useradd`.
fn run(command: &mut Command) -> Result<(), String> {
    run_within(command, RESTART_WITHIN)
}

fn run_within(command: &mut Command, within: Duration) -> Result<(), String> {
    let shown = format!("{command:?}");
    query::output_and_stderr_within(command, Instant::now() + within)
        .map(drop)
        .map_err(|error| shown_error(&shown, within, &error))
}

/// `+0700` or `-0330` as minutes.
pub fn offset_minutes(text: &str) -> Option<i32> {
    let (sign, digits) = match text.as_bytes().first()? {
        b'+' => (1, &text[1..]),
        b'-' => (-1, &text[1..]),
        _ => return None,
    };
    if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let hours: i32 = digits[..2].parse().ok()?;
    let minutes: i32 = digits[2..].parse().ok()?;
    Some(sign * (hours * 60 + minutes))
}

pub struct Install {
    /// The filesystem the lamp's paths are under: `/` on a lamp.
    pub root: PathBuf,
    pub channel: String,
    pub channel_url: String,
    /// The image's public key.
    pub key: [u8; 32],
    /// Where lamp-update keeps its records: `/var/lib/lampos-update/`.
    pub records: String,
    /// `--setup on`: also write setup's settings ([`SETUP_SETTINGS`]).
    pub setup: bool,
    /// The models to place where missing: [`models`] on a lamp.
    pub models: Vec<Model>,
}

impl Install {
    fn on(&self, path: &str) -> PathBuf {
        self.root
            .join(path.trim_start_matches('/').trim_end_matches('/'))
    }
}

/// What an install did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    pub sequence: u64,
    /// One line for each thing written or made; empty when nothing was.
    pub changed: Vec<String>,
}

/// The start of every error from an install that ended before writing its
/// units (#3733): the lamp keeps the units it had, which may be an older
/// build's, and `check-units` names each one that differs.
pub const UNITS_NOT_WRITTEN: &str = "units not written";

/// Installs the newest build of the channel and the product around it.
pub fn install(
    config: &Install,
    channel: &mut dyn Channel,
    host: &mut dyn Host,
    say: &mut dyn FnMut(&str),
) -> Result<Installed, String> {
    // Everything before the units, in one place: a run that ends there (no
    // build, the records locked, a refused head reader, a user, a part's
    // file or link) wrote no unit, and its error says so, so a lamp left
    // with an older build's units is told why (#3733, lamp-c0a4's
    // lamp-cloud.service of 19 Oct).
    let Early {
        manifest,
        records,
        lock: _lock,
        mut state,
        records_lost,
        mut changed,
        made,
        own,
    } = before_units(config, channel, host, say)
        .map_err(|why| format!("{UNITS_NOT_WRITTEN}: {why}"))?;

    // The units below are this program's own (`PRODUCT`), not the build's.
    // A lamp-update known to be another than the build's own hands over to
    // the build's, placed above, to write the build's units (#3733); one
    // that cannot is said, and install goes on with its own.
    let running = host.running_sha256();
    if let Some(line) = not_the_builds_updater(&manifest, running.clone()) {
        if let (Ok(_), Some(own)) = (&running, &own) {
            if host.handed_over() {
                // The loop guard: the channel moved on between the two runs.
                say(&format!(
                    "this lamp-update was handed this install already, so it writes \
                     its own units over build {}'s binaries",
                    manifest.sequence
                ));
            } else {
                say(&format!(
                    "handing over to build {}'s own lamp-update, {own}, to write its units",
                    manifest.sequence
                ));
                let why = host.hand_over(&config.on(own));
                say(&format!("could not hand over: {why}"));
            }
        }
        say(&line);
    }

    // The units, their exit-note drop-ins and the tmpfiles. systemd must be
    // told of any of them written, so that is recorded before the first is:
    // a run stopped anywhere after leaves the debt for the next (the reviewer on #815,
    // 19:17 and 19:32).
    let for_systemd: Vec<(String, &[u8])> = PRODUCT
        .iter()
        .map(|(name, text)| (format!("{UNITS}{name}"), text.as_bytes()))
        .chain(EXIT_NOTE_UNITS.iter().map(|name| {
            (
                format!("{UNITS}{name}.d/exit-note.conf"),
                EXIT_NOTE.as_bytes(),
            )
        }))
        .chain([(TMPFILES_PATH.to_owned(), TMPFILES.as_bytes())])
        .collect();
    if !state.systemd_owed
        && for_systemd
            .iter()
            .any(|(path, bytes)| fs::read(config.on(path)).map_or(true, |now| now != *bytes))
    {
        state.systemd_owed = true;
        save(&records, &state).map_err(|why| format!("{UNITS_NOT_WRITTEN}: {why}"))?;
    }
    // Each file is written whole; one that fails leaves those before it
    // new and the rest as they were.
    for (path, bytes) in &for_systemd {
        put(config, path, bytes, 0o644, &mut changed)
            .map_err(|why| format!("units not all written: {why}"))?;
    }
    // The tmpfiles are install's, recorded as the settings are, so a
    // build's newer tmpfiles reach this lamp (#1680, crate::apply).
    state.settings.insert(
        TMPFILES_PATH.to_owned(),
        manifest::sha256_hex(TMPFILES.as_bytes()),
    );
    // A core skill turned on on this lamp stays on (the first builder on
    // #2585, 22:24): its skill.toml is written turned on.
    let mut texts: Vec<String> = SETTINGS
        .iter()
        .map(|(_, text)| (*text).to_owned())
        .collect();
    crate::product_skills::keep_turned_on(
        &config.root,
        SETTINGS.iter().map(|(path, _)| *path).zip(texts.iter_mut()),
        &state.settings,
        &state.settings_writing,
        true,
        say,
    )
    .map_err(|error| format!("the product skills cannot be read: {error}"))?;
    let mut defaults: Vec<(&str, &str)> = SETTINGS
        .iter()
        .zip(&texts)
        .map(|((path, _), text)| (*path, text.as_str()))
        .collect();
    defaults.extend_from_slice(STORE_SETTINGS);
    if config.setup {
        defaults.extend_from_slice(SETUP_SETTINGS);
    }
    place_models(
        config,
        channel,
        &mut state,
        &mut changed,
        say,
        &mut |state| save(&records, state),
    )?;
    settings(
        config,
        &defaults,
        &mut state,
        &mut changed,
        say,
        &mut |state| save(&records, state),
    )?;
    own_pins_follow_build(config, &defaults, &mut changed, say)?;
    // The units and settings are written; what is kept beside them may
    // still turn a product switch off. Said, never changed.
    for line in switches_off(config, &defaults) {
        say(&line);
    }
    reset_plan(
        config,
        manifest.sequence,
        &mut state,
        &mut changed,
        say,
        &mut |state| save(&records, state),
    )?;
    // The voice's service and the user's key, private. Each is install's
    // while it is still what an install wrote, so a newer provider, or a key
    // V1 rotated in its config, reaches the lamp; one setup wrote, or one
    // changed on the lamp, is kept (the reviewer, thirteen, on #1050). The key
    // is the user's (the owner on #806, 16:13): V1's is copied only into a lamp
    // that never had one from install. One install wrote that is now gone was
    // removed on purpose (a reset wipes it with the person's data), and V1's
    // config, which may still hold the previous owner's, never puts it back.
    let key = device_key(config);
    let mut private: Vec<(&str, &str)> = vec![(PROVIDER_PATH, PROVIDER), (SECOND_PATH, SECOND)];
    let mut marked = false;
    for (path, part) in [(CREDENTIAL_PATH, "the voice"), (JEV_KEY_PATH, "the Policy")] {
        let there = config.on(path).exists();
        let had = state.settings.contains_key(path);
        // Every key path, the ones there too: a key there now is removed by
        // a later reset, and must not come back then. One there that is
        // V1's gets its real hash below; any other is kept as the lamp's own
        // (the reviewer on #1263).
        if records_lost {
            state.settings.insert(path.to_owned(), SET_ASIDE.to_owned());
            marked = true;
        }
        match &key {
            Some(key) if there || (!had && !records_lost) => private.push((path, key)),
            _ if there => {}
            Some(_) if records_lost => say(&format!(
                "{path} is missing and the records were set aside: V1's key is not copied; setup takes the user's"
            )),
            Some(_) => say(&format!(
                "{path} was removed since install wrote it (a reset?): V1's key is not put back; setup takes the user's"
            )),
            None => say(&format!(
                "no key: {part} cannot reach its service until setup takes the user's into {path} (none in {V1_CONFIG})"
            )),
        }
    }
    if marked {
        save(&records, &state)?;
    }
    let wrote = changed.len();
    settings_with_mode(
        config,
        &private,
        0o600,
        &mut state,
        &mut changed,
        say,
        &mut |state| save(&records, state),
    )?;
    for path in [CREDENTIAL_PATH, JEV_KEY_PATH] {
        if changed[wrote..]
            .iter()
            .any(|line| line == &format!("wrote {path}"))
        {
            say(&format!("V1's key was taken from {V1_CONFIG} into {path}"));
        }
    }
    // Made 0755 here on a blank lamp. One made closed before (lamp-4ace's is
    // root's alone) is not opened: credentials may be below it. Said instead.
    if let Ok(meta) = fs::metadata(config.on("/etc/lampOS"))
        && meta.permissions().mode() & 0o001 == 0
    {
        say(&format!(
            "/etc/lampOS is closed to other users ({:o}): the parts run as {USER} cannot read their settings in it",
            meta.permissions().mode() & 0o777
        ));
    }
    // Written when missing, and again when the lamp's saved zone is another
    // (#820): a lamp set up before setup wrote this file kept the image's
    // zone here, which schedule and skills read, while memory and Expression
    // follow /etc/localtime. A lamp naming no zone keeps the file it has.
    let saved = saved_zone(config);
    let written = fs::read_to_string(config.on(ZONE_PATH)).ok();
    let written_zone = written.as_deref().and_then(|text| {
        text.lines()
            .find_map(|line| line.strip_prefix("LAMP_ZONE="))
            .map(str::trim)
    });
    let differs = written.is_some()
        && saved
            .as_deref()
            .is_some_and(|zone| written_zone != Some(zone));
    // Only to a zone the lamp has: a wrong /etc/timezone must not replace a
    // good file, or schedule would not start on it (the reviewer on #1380).
    let stale = differs
        && saved
            .as_deref()
            .is_some_and(|zone| zone_file_known(config, zone));
    if differs && !stale {
        say(&format!(
            "kept {ZONE_PATH}: the lamp names {} as its zone, which is not a zone in /usr/share/zoneinfo",
            saved.as_deref().unwrap_or_default()
        ));
    }
    if written.is_none() || stale {
        let zone = saved.unwrap_or_else(|| "UTC".to_owned());
        let offset = host.utc_offset_minutes(&zone)?;
        if stale {
            say(&format!(
                "{ZONE_PATH} follows the zone this lamp was set up in, {zone}"
            ));
        }
        let text = format!(
            "# The lamp's zone, for schedule and skills (docs/install.md).\n\
             # Written by lamp-update install from the system's zone; setup writes it again.\n\
             LAMP_ZONE={zone}\nLAMP_UTC_OFFSET_MINUTES={offset}\n"
        );
        put(config, ZONE_PATH, text.as_bytes(), 0o644, &mut changed)?;
    }
    if !config.on("/etc/lampOS/body/lamp.env").exists() {
        say(
            "the body waits for this lamp's own poses in /etc/lampOS/body/lamp.env (docs/install.md)",
        );
    }

    let before = state.clone();
    let build = Build {
        sequence: manifest.sequence,
        commit: manifest.commit.clone(),
    };
    // The folders made are this program's: a round removes one once
    // neither the link nor the build before names it.
    let new_folders: Vec<String> = made
        .into_iter()
        .filter(|folder| !state.made.contains(folder))
        .collect();
    if state.installed.as_ref() != Some(&build) {
        state.previous = state.installed.replace(build);
    }
    state.made.extend(new_folders);
    // Everything on disk is recorded before systemd is asked anything,
    // with what is owed to it: a run whose enable fails leaves the debt,
    // and the next run pays it though it writes nothing (the reviewer on
    // #815, 19:17).
    state.systemd_owed |= !changed.is_empty();
    if state != before {
        save(&records, &state)?;
    }

    // Started by nothing: enabled, for the next power-up after setup.
    if state.systemd_owed {
        host.tmpfiles(TMPFILES_PATH)?;
        host.enable(&enabled())?;
        changed.push(format!("enabled {}", enabled().join(" ")));
        state.systemd_owed = false;
        save(&records, &state)?;
    }
    // Harness at boot, once it is on the lamp; not having it is said, not
    // a failure: it is installed by its own installer (lamp-tasks.service).
    if !config.on(HARNESS).exists() {
        say(&format!(
            "Harness is not at {HARNESS}: it is not set to start at boot"
        ));
    } else {
        let done = config.on(&config.records).join(HARNESS_DONE);
        if !done.exists() {
            // A failure leaves nothing recorded: the next install asks again.
            host.harness_at_boot(USER, HARNESS_HOME)?;
            write_whole(&done, format!("{USER} {HARNESS_HOME}\n").as_bytes(), 0o644)
                .map_err(|error| format!("{}: {error}", done.display()))?;
            changed.push(format!("Harness starts at boot as {USER}"));
        }
    }
    for line in &changed {
        say(line);
    }
    Ok(Installed {
        sequence: manifest.sequence,
        changed,
    })
}

/// The line install says when the `lamp-update` running it, by its sha256
/// (`running`), is not the build's own `lamp-update` (#3733): the units it
/// writes are the ones it carries, so an older lamp-update installing a
/// newer build leaves the new binaries under its own older units, as on
/// lamp-c0a4 on 27 Oct. None when they are the same file. A running file
/// that cannot be read says only that it could not be checked. The
/// advice names the build's own folder, never `current`, which on a lamp
/// held below the build's updater is the older lamp-update itself.
pub fn not_the_builds_updater(
    manifest: &Manifest,
    running: Result<String, String>,
) -> Option<String> {
    let sequence = manifest.sequence;
    // Before the units the build is known to carry its lamp-update; one
    // without it has nothing to check against, so says nothing.
    let builds = manifest
        .files
        .iter()
        .find(|file| file.name == crate::own::BINARY)?;
    let running = match running {
        Ok(sha256) => sha256,
        Err(why) => {
            return Some(format!(
                "this lamp-update could not be checked against build {sequence}'s: {why}"
            ));
        }
    };
    if builds.sha256.eq_ignore_ascii_case(&running) {
        return None;
    }
    let short = |sha256: &str| sha256.chars().take(12).collect::<String>();
    Some(format!(
        "this lamp-update (sha256 {}) is not build {sequence}'s ({}): the units written are \
         this lamp-update's own, not build {sequence}'s; run build {sequence}'s own, \
         {}{}/{} install, to write its units",
        short(&running),
        short(&builds.sha256),
        crate::own::FOLDER,
        builds.sha256.to_ascii_lowercase(),
        crate::own::BINARY,
    ))
}

/// What install has when it comes to the units: the build, the records
/// and their lock, held to the end, the state, and what it changed.
struct Early {
    manifest: Manifest,
    records: Records,
    lock: crate::records::Lock,
    state: State,
    records_lost: bool,
    changed: Vec<String>,
    /// The build folders this run made.
    made: Vec<String>,
    /// The build's own lamp-update, placed: `/opt/lampOS/update/<build>/lamp-update`.
    own: Option<String>,
}

/// Install up to its units: the build, the lock, the head reader's
/// preflight, the state, the users, every part's build and link, and
/// lamp-update-back. Nothing here writes a unit.
fn before_units(
    config: &Install,
    channel: &mut dyn Channel,
    host: &mut dyn Host,
    say: &mut dyn FnMut(&str),
) -> Result<Early, String> {
    let manifest = newest(config, channel)?;
    let records = Records::new(config.on(&config.records));
    let lock = records
        .lock()
        .map_err(|error| format!("another lamp-update is running: {error}"))?;
    // The base units below replace the installed ones, but local drop-ins
    // survive. Refuse an unknown or conflicting head reader before any
    // state migration, user, binary link, unit or pins write.
    crate::head_touch::preflight_install(&config.root, &config.on(UNITS), PRODUCT, SETTINGS)?;
    // The records as `Records::state` reads them: an unreadable file stops
    // install rather than being written over, and a corrupt one is set
    // aside and said. Set aside, it is not known what install wrote before,
    // so V1's key is not copied (below): a reset may have wiped the user's
    // key, and V1's config may still hold the previous owner's (#1094).
    let state_file = config.on(&config.records).join("state.json");
    let existed = state_file.exists();
    let state = records
        .state(say)
        .map_err(|error| format!("the state cannot be read: {error}"))?;
    let records_lost = records_lost(&config.on(&config.records), existed);
    let mut changed = Vec::new();

    // The users first: a folder or a part made below may name them.
    let groups = existing_groups(config, GROUPS);
    for missing in GROUPS
        .iter()
        .filter(|group| !groups.iter().any(|g| g == *group))
    {
        say(&format!(
            "the image has no group {missing}; {USER} is not put in it"
        ));
    }
    for (user, groups) in [(USER, groups), (MEMORY_USER, vec![USER.to_owned()])] {
        if !has_user(config, user) {
            host.add_user(user, &groups)?;
            changed.push(format!("made the user {user}"));
        }
    }

    // Every part's build beside nothing, then its link.
    let mut wanted = Vec::new();
    for part in parts() {
        let mut names = vec![part.binary.clone()];
        if let Some((_, beside)) = BESIDE.iter().find(|(binary, _)| *binary == part.binary) {
            names.extend(beside.iter().map(|name| (*name).to_owned()));
        }
        if let Some((_, tools)) = BESIDE_IF_CARRIED
            .iter()
            .find(|(binary, _)| *binary == part.binary)
        {
            for tool in *tools {
                if manifest.files.iter().any(|file| file.name == *tool) {
                    names.push((*tool).to_owned());
                } else {
                    say(&format!(
                        "build {} carries no {tool}: it is not put beside {}",
                        manifest.sequence, part.binary
                    ));
                }
            }
        }
        let files: Vec<&manifest::File> = names
            .iter()
            .map(|name| {
                manifest
                    .files
                    .iter()
                    .find(|file| &file.name == name)
                    .ok_or_else(|| format!("build {} carries no {name}", manifest.sequence))
            })
            .collect::<Result<_, _>>()?;
        wanted.push((part, files));
    }
    // Every folder whole before any link moves: a file that fails leaves
    // the folders made so far beside, unnamed by any link, for the next run.
    let mut builds = Vec::new();
    let mut made = Vec::new();
    for (part, files) in &wanted {
        let before = changed.len();
        let build = place(config, &manifest, channel, part, files, &mut changed)?;
        if changed.len() > before {
            made.push(unit::folder(&part.folder, &build));
        }
        builds.push(build);
    }
    for ((part, _), build) in wanted.iter().zip(&builds) {
        link(config, part, build, &mut changed)?;
    }
    // lamp-update-back, a copy of this build's lamp-update at a fixed path:
    // written here alone, never by a round, which cannot even write it, so
    // a lamp-update that cannot start is still put back (#1680).
    let mut own = None;
    if let Some(((part, _), build)) = wanted
        .iter()
        .zip(&builds)
        .find(|((part, _), _)| part.binary == crate::own::BINARY)
    {
        let placed = format!("{}{}", unit::folder(&part.folder, build), part.binary);
        let from = config.on(&placed);
        let bytes = fs::read(&from).map_err(|error| format!("{}: {error}", from.display()))?;
        put(config, crate::own::BACK, &bytes, 0o755, &mut changed)?;
        own = Some(placed);
    }

    Ok(Early {
        manifest,
        records,
        lock,
        state,
        records_lost,
        changed,
        made,
        own,
    })
}

fn save(records: &Records, state: &State) -> Result<(), String> {
    records
        .save_state(state)
        .map_err(|error| format!("the state cannot be written: {error}"))
}

/// Every kept drop-in of a product unit (all but the reserved
/// `exit-note.conf`), and every environment file the unit and those
/// drop-ins name, read together as systemd merges them, where one of the
/// unit's switches ends up off (crate::switches): said by install, by the
/// file that left it off, and left as it is. A lamp from its
/// builder days with `Environment=LAMP_CLOUD=off` in a drop-in would
/// otherwise come up off the cloud after an install to stable with nothing
/// said. A file of `defaults` is said as a setting, edited when it is not
/// the default's bytes. A file that cannot be read is passed over.
pub fn switches_off(config: &Install, defaults: &[(&str, &str)]) -> Vec<String> {
    use crate::switches::{environment, environment_file, environment_files, merged_off, said};
    let mut lines = Vec::new();
    for (name, text) in PRODUCT {
        let switches = crate::switches::switches(text);
        if switches.is_empty() {
            continue;
        }
        let folder = format!("{UNITS}{name}.d/");
        let mut drop_ins: Vec<String> = fs::read_dir(config.on(&folder))
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .filter(|file| file.ends_with(".conf") && file != "exit-note.conf")
            .collect();
        drop_ins.sort();
        // Read as systemd merges them (the Reviewer-lead's P3 on #3728): the
        // drop-ins in name order, then the environment files the unit and
        // those drop-ins name, which win over `Environment=`. Only a switch
        // still off at the end is said, by the file that left it off.
        let mut sources = Vec::new();
        let (_, mut files) = environment_files(text);
        for file in drop_ins {
            let path = format!("{folder}{file}");
            let Ok(kept) = fs::read_to_string(config.on(&path)) else {
                continue;
            };
            sources.push((format!("kept drop-in {path}"), environment(&kept)));
            let (cleared, named) = environment_files(&kept);
            if cleared {
                files.clear();
            }
            files.extend(named);
        }
        // A file named twice is read twice; its last read is the one that
        // counts, so it is kept at its last place and said once.
        let mut once: Vec<String> = Vec::new();
        for path in files.into_iter().rev() {
            if !once.contains(&path) {
                once.insert(0, path);
            }
        }
        for path in once {
            let Ok(kept) = fs::read_to_string(config.on(&path)) else {
                continue;
            };
            let what = match defaults.iter().find(|(setting, _)| *setting == path) {
                Some((_, default)) if *default != kept => "edited setting",
                Some(_) => "setting",
                None => "environment file",
            };
            sources.push((
                format!("{what} {path}, read by {name},"),
                environment_file(&kept),
            ));
        }
        for (by, how) in merged_off(&switches, &sources) {
            lines.push(said(&by, &how));
        }
    }
    lines
}

/// The product's settings, from `defaults` (path, bytes): each written
/// where it is missing, or where it is still exactly what an install wrote
/// before (by its SHA-256 in the state's `settings`, or in
/// `settings_writing` when a run stopped between writing it and recording
/// it), so a newer build's default reaches a lamp that never changed it.
/// One that differs from all of these was changed on the lamp, or was
/// there before any install (a lamp's own pins, its calibration): it is
/// kept, and said, and its record is left as it was. Rerunning install is
/// also how the updater upgrades itself, so this holds at every run (the
/// reviewer on #815).
///
/// What is about to be written is recorded through `persist` before the
/// first write, and what was written after the last, so that no stop in
/// between leaves a file of install's that the next run takes for the
/// lamp's.
pub fn settings(
    config: &Install,
    defaults: &[(&str, &str)],
    state: &mut State,
    changed: &mut Vec<String>,
    say: &mut dyn FnMut(&str),
    persist: &mut dyn FnMut(&State) -> Result<(), String>,
) -> Result<(), String> {
    settings_with_mode(config, defaults, 0o644, state, changed, say, persist)
}

/// Where the product's units read Controls' pins.
const PINS_PATH: &str = "/etc/lampOS/controls/controls-pins.json";

/// Where they read Controls' settings, which name the head pads too.
const CONTROLS_PATH: &str = "/etc/lampOS/controls/controls.json";

/// The head pads every product pins file named before #1798: a guess, right
/// on no unit known since (#2182), which the factory step never wrote.
const GUESSED_HEAD_PADS: &str = r#"[
    {"name": "head-1", "line": 37, "active_low": false, "pull": "down"},
    {"name": "head-2", "line": 96, "active_low": false, "pull": "down"}
]"#;

/// A lamp's own pins file, which [`settings`] keeps (the factory's head
/// pads, a calibration), follows the build where install wrote the part
/// it changes:
/// - it gains the build's `privacy` section when it has none. The
///   product's units select the privacy route (`LAMP_PRIVACY_CONTROLS=on`),
///   and without the section Controls has no privacy source, so Voice and
///   Ears never open a microphone (the reviewer's P1 on #2143);
/// - its head pads, while still the old guess ([`GUESSED_HEAD_PADS`], with
///   no factory baseline), become the build's: every line a pad is wired
///   to on some unit (#1798). On lamp-52e6 and lamp-cb94 the guess read no
///   touch at all. A lamp's own Controls settings naming the old two pads
///   name the build's then too, so a Controls from before #1798, which
///   wants both files to name the same pads, still starts if it is put
///   back;
/// - its `legacy_head` goes when the build's pins have none: the product's
///   voice and Expression read the head from `controls/touch` (#711), and a
///   pat must not also be the old engine's `Pet` beside them (the reviewer
///   on #1176).
///
/// Only those keys change; everything else is kept, each file's mode too.
/// A file already like the build, a build without them, a file that isn't
/// there (the default was just written) or one that isn't a JSON object is
/// left, the last said.
fn own_pins_follow_build(
    config: &Install,
    defaults: &[(&str, &str)],
    changed: &mut Vec<String>,
    say: &mut dyn FnMut(&str),
) -> Result<(), String> {
    let Some(default) = defaults
        .iter()
        .find(|(path, _)| *path == PINS_PATH)
        .and_then(|(_, text)| serde_json::from_str::<serde_json::Value>(text).ok())
    else {
        return Ok(());
    };
    let Some(mut pins) = own_object(config, PINS_PATH, say)? else {
        return Ok(());
    };
    let mut done = Vec::new();
    if let Some(section) = default.get("privacy")
        && !pins.contains_key("privacy")
    {
        pins.insert("privacy".into(), section.clone());
        done.push("added the privacy section to");
    }
    let guessed: serde_json::Value =
        serde_json::from_str(GUESSED_HEAD_PADS).expect("the old guess is JSON");
    let pads = default.get("head_pads").filter(|pads| **pads != guessed);
    if let Some(pads) = pads
        && pins.get("head_pads") == Some(&guessed)
        && !pins.contains_key("head_rest")
    {
        pins.insert("head_pads".into(), pads.clone());
        done.push("put the build's head pads, every line a pad is wired to, in");
    }
    if default
        .get("legacy_head")
        .is_none_or(|legacy| *legacy == false)
        && pins.get("legacy_head") == Some(&serde_json::Value::Bool(true))
    {
        pins.remove("legacy_head");
        done.push("took the old engine's head (legacy_head) out of");
    }
    // The pins name the build's pads now, whether install wrote them just
    // before or this did.
    let names = pads
        .filter(|pads| pins.get("head_pads") == Some(*pads))
        .and_then(|pads| pads.as_array())
        .map(|pads| pads.iter().map(|pad| pad["name"].clone()).collect());
    if !done.is_empty() {
        write_object(config, PINS_PATH, pins, changed)?;
        for what in done {
            say(&format!(
                "{what} {PINS_PATH}: this lamp's own pins are kept otherwise"
            ));
        }
    }
    if let Some(names) = names
        && let Some(mut controls) = own_object(config, CONTROLS_PATH, say)?
        && controls.get("head_pads") == Some(&serde_json::json!(["head-1", "head-2"]))
    {
        controls.insert("head_pads".into(), serde_json::Value::Array(names));
        write_object(config, CONTROLS_PATH, controls, changed)?;
        say(&format!(
            "named the same head pads in {CONTROLS_PATH}: this lamp's own settings are kept otherwise"
        ));
    }
    Ok(())
}

/// The JSON object at `path` on this lamp: none for a file that isn't
/// there, and none, said, for one that isn't an object.
fn own_object(
    config: &Install,
    path: &str,
    say: &mut dyn FnMut(&str),
) -> Result<Option<serde_json::Map<String, serde_json::Value>>, String> {
    let text = match fs::read_to_string(config.on(path)) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{path}: {error}")),
    };
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(serde_json::Value::Object(object)) => Ok(Some(object)),
        _ => {
            say(&format!(
                "{path} is not a JSON object: it is not changed, and Controls will say so"
            ));
            Ok(None)
        }
    }
}

/// `object` written whole over the file at `path`, keeping its mode.
fn write_object(
    config: &Install,
    path: &str,
    object: serde_json::Map<String, serde_json::Value>,
    changed: &mut Vec<String>,
) -> Result<(), String> {
    let at = config.on(path);
    let mode = fs::metadata(&at)
        .map(|meta| meta.permissions().mode() & 0o777)
        .map_err(|error| format!("{path}: {error}"))?;
    let mut bytes = serde_json::to_vec_pretty(&serde_json::Value::Object(object))
        .map_err(|error| format!("{path}: {error}"))?;
    bytes.push(b'\n');
    write_whole(&at, &bytes, mode).map_err(|error| format!("{path}: {error}"))?;
    changed.push(format!("wrote {path}"));
    Ok(())
}

/// [`settings`], written with `mode`: the voice's service and the device
/// key are private (0600), as the voice requires.
pub fn settings_with_mode(
    config: &Install,
    defaults: &[(&str, &str)],
    mode: u32,
    state: &mut State,
    changed: &mut Vec<String>,
    say: &mut dyn FnMut(&str),
    persist: &mut dyn FnMut(&State) -> Result<(), String>,
) -> Result<(), String> {
    let before = state.clone();
    let mut writes = Vec::new();
    let skills = format!("{}/", crate::product_skills::FOLDER);
    for (path, text) in defaults {
        let default = manifest::sha256_hex(text.as_bytes());
        let now = match fs::read(config.on(path)) {
            Ok(bytes) => Some(manifest::sha256_hex(&bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(format!("{path}: {error}")),
        };
        let installs = |hash: &String| {
            state.settings.get(*path) == Some(hash)
                || state.settings_writing.get(*path) == Some(hash)
        };
        match now {
            Some(now) if now == default => {
                // Its bytes are install's, so its mode is too: a default
                // written closed by something before install (lamp-cb94's,
                // 0600) is opened, or the parts run as `lampos` never read it.
                set_mode(config, path, text.as_bytes(), mode, changed)?;
                state.settings.insert((*path).to_owned(), default);
            }
            // A product skill's file is the build's, never the lamp's own
            // (#1722): written over below, as a round puts it back.
            Some(now) if !installs(&now) && !path.starts_with(&skills) => {
                say(&format!(
                    "kept {path}: this lamp's own, not a default an install wrote; the build's default is not written"
                ));
                // The lamp's own is not install's to open: said instead,
                // and only said, so a file gone since it was read stops
                // nothing (the reviewer on #1293).
                if mode & 0o004 != 0
                    && let Ok(meta) = fs::metadata(config.on(path))
                    && meta.permissions().mode() & 0o004 == 0
                {
                    say(&format!(
                        "{path} is closed to other users ({:o}): the parts run as {USER} cannot read it",
                        meta.permissions().mode() & 0o777
                    ));
                }
            }
            _ => {
                if let Some(now) = &now
                    && !installs(now)
                {
                    // Only a product skill's file gets here changed: said
                    // as a round's put-back is (the reviewer on #2585).
                    say(&format!(
                        "put back {path}: changed on this lamp; a core skill's files are the build's"
                    ));
                }
                // A file that is install's only by an earlier run's intent
                // (written, then that run failed or stopped) is settled as
                // install's now, before this run's intent takes the
                // intent's place: a second failure must not lose it (the
                // reviewer on #815, 21:42).
                if let Some(now) = now {
                    state.settings.insert((*path).to_owned(), now);
                }
                writes.push((*path, *text, default));
            }
        }
    }
    if !writes.is_empty() {
        // A stopped run must reload a Store drop-in even if the next run
        // finds its bytes already correct. Record the debt before writing.
        state.systemd_owed |= writes.iter().any(|(path, _, _)| path.starts_with(UNITS));
        for (path, _, default) in &writes {
            state
                .settings_writing
                .insert((*path).to_owned(), default.clone());
        }
        persist(state)?;
        for (path, text, default) in writes {
            put(config, path, text.as_bytes(), mode, changed)?;
            state.settings.insert(path.to_owned(), default);
        }
    }
    // Every setting is settled: nothing is being written any more.
    state.settings_writing.clear();
    if *state != before {
        persist(state)?;
    }
    Ok(())
}

/// Setup's reset plan ([`RESET_PLAN`], #806): written with `--setup on`
/// where missing, and over any other plan the lamp has, never kept as the
/// lamp's own. The old one is kept in the records first
/// (crate::apply::keep_reset_plan) unless it is the plan recorded. A lamp
/// with no plan and no `--setup on` gets none. Recorded as install's, its
/// intent first, as [`settings_with_mode`] does.
fn reset_plan(
    config: &Install,
    sequence: u64,
    state: &mut State,
    changed: &mut Vec<String>,
    say: &mut dyn FnMut(&str),
    persist: &mut dyn FnMut(&State) -> Result<(), String>,
) -> Result<(), String> {
    let path = manifest::RESET_PLAN;
    let new = manifest::sha256_hex(RESET_PLAN.as_bytes());
    let now = match fs::read(config.on(path)) {
        Ok(bytes) => Some(manifest::sha256_hex(&bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("{path}: {error}")),
    };
    match now {
        None if !config.setup => return Ok(()),
        Some(now) if now == new => {
            set_mode(config, path, RESET_PLAN.as_bytes(), 0o644, changed)?;
        }
        _ => {
            let was = state
                .settings
                .get(path)
                .or_else(|| state.settings_writing.get(path))
                .cloned();
            crate::apply::keep_reset_plan(
                &config.root,
                &config.on(&config.records),
                sequence,
                was.as_ref(),
                say,
            )
            .map_err(|error| format!("{path}: {error}"))?;
            state.settings_writing.insert(path.to_owned(), new.clone());
            persist(state)?;
            put(config, path, RESET_PLAN.as_bytes(), 0o644, changed)?;
            state.settings_writing.remove(path);
        }
    }
    if state.settings.get(path) != Some(&new) || state.settings_writing.contains_key(path) {
        state.settings.insert(path.to_owned(), new);
        state.settings_writing.remove(path);
        persist(state)?;
    }
    Ok(())
}

/// The channel's newest build, checked with the image's key.
fn newest(config: &Install, channel: &mut dyn Channel) -> Result<Manifest, String> {
    let url = format!("{}{}", config.channel_url, manifest::ENVELOPE);
    let mut envelope = Vec::new();
    match channel.fetch(&url, manifest::MOST_ENVELOPE_BYTES, &mut envelope) {
        Ok(_) => {}
        Err(Fetch::Missing) => return Err("the channel has published no build".into()),
        Err(error) => return Err(error.to_string()),
    }
    let (manifest, _) = manifest::open(&envelope, &config.key).map_err(|why| why.0)?;
    if manifest.channel != config.channel {
        return Err(format!(
            "the build is of channel {}, not {}",
            manifest.channel, config.channel
        ));
    }
    Ok(manifest)
}

/// The part's build folder, filled if it is not there whole: named by the
/// binary's SHA-256, or, when a folder of that name holds other versions of
/// what goes beside it, by the SHA-256 of their list (as a round names it).
fn place(
    config: &Install,
    manifest: &Manifest,
    channel: &mut dyn Channel,
    part: &Part,
    files: &[&manifest::File],
    changed: &mut Vec<String>,
) -> Result<String, String> {
    let mut lines: Vec<String> = files
        .iter()
        .map(|file| format!("{}  {}\n", file.sha256, file.name))
        .collect();
    lines.sort();
    let names = [
        files[0].sha256.clone(),
        manifest::sha256_hex(lines.concat().as_bytes()),
    ];
    for (index, name) in names.iter().enumerate() {
        let folder = config.on(&format!("{}{name}", part.folder));
        if !folder.exists() {
            fill(manifest, channel, &folder, files)?;
            changed.push(format!("installed {}{name}/", part.folder));
            return Ok(name.clone());
        }
        if holds(&folder, files) {
            return Ok(name.clone());
        }
        if files.len() == 1 || index == 1 {
            return Err(format!(
                "{} is there and does not hold what build {} has; it is left as it is",
                folder.display(),
                manifest.sequence
            ));
        }
    }
    unreachable!("the second name returns")
}

fn holds(folder: &Path, files: &[&manifest::File]) -> bool {
    files
        .iter()
        .all(|file| file_sha256(&folder.join(&file.name)).is_ok_and(|sha| sha == file.sha256))
}

/// One folder of a build, filled under its temporary name and renamed only
/// when every file is whole and matches.
fn fill(
    manifest: &Manifest,
    channel: &mut dyn Channel,
    folder: &Path,
    files: &[&manifest::File],
) -> Result<(), String> {
    let failed = |error: io::Error| format!("{}: {error}", folder.display());
    let temporary = filling(folder);
    if let Some(parent) = folder.parent() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o755)
            .create(parent)
            .map_err(failed)?;
    }
    if temporary.exists() {
        fs::remove_dir_all(&temporary).map_err(failed)?;
    }
    fs::DirBuilder::new()
        .mode(0o755)
        .create(&temporary)
        .map_err(failed)?;
    let result = (|| {
        for file in files {
            let url = format!("{}{}", manifest.files_url, file.name);
            let mut out = fs::File::create(temporary.join(&file.name)).map_err(failed)?;
            let mut hashing = Hashing {
                file: &mut out,
                context: digest::Context::new(&digest::SHA256),
            };
            let fetched = channel.fetch(&url, file.size, &mut hashing);
            let sha = manifest::hex(hashing.context.finish().as_ref());
            match fetched {
                Ok(size) if size == file.size && sha == file.sha256 => {}
                Ok(_) => return Err(format!("{} does not match its SHA-256", file.name)),
                Err(error) => return Err(format!("{}: {error}", file.name)),
            }
            out.set_permissions(fs::Permissions::from_mode(0o755))
                .and_then(|()| out.sync_all())
                .map_err(failed)?;
        }
        fs::rename(&temporary, folder)
            .and_then(|()| sync_parent(folder))
            .map_err(failed)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&temporary);
    }
    result
}

/// `current` pointing at the build, replaced whole if it points elsewhere.
fn link(
    config: &Install,
    part: &Part,
    build: &str,
    changed: &mut Vec<String>,
) -> Result<(), String> {
    let path = unit::current(&part.folder);
    let current = config.on(&path);
    if fs::read_link(&current).is_ok_and(|target| target == Path::new(build)) {
        return Ok(());
    }
    replace_link(&current, build).map_err(|error| format!("{path}: {error}"))?;
    changed.push(format!("{path} -> {build}"));
    Ok(())
}

/// A file written whole, only if it is not already exactly this.
fn put(
    config: &Install,
    path: &str,
    bytes: &[u8],
    mode: u32,
    changed: &mut Vec<String>,
) -> Result<(), String> {
    let at = config.on(path);
    if fs::read(&at).is_ok_and(|now| now == bytes) {
        return set_mode(config, path, bytes, mode, changed);
    }
    let failed = |error: io::Error| format!("{path}: {error}");
    if let Some(parent) = at.parent() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o755)
            .create(parent)
            .map_err(failed)?;
    }
    write_whole(&at, bytes, mode).map_err(failed)?;
    changed.push(format!("wrote {path}"));
    Ok(())
}

/// Gives install's file at `path` `mode` when it has another and still
/// holds exactly `bytes`: the bytes alone do not make it install's as
/// written (the first builder on #705, 12:13). It is opened once, never
/// through a link, and what is read and what is changed are that one open
/// file, so nothing swapped in at the path between is touched (the
/// reviewer on #1293). A link is left as it is: its target is not
/// install's to open or close (Nine on #1293).
fn set_mode(
    config: &Install,
    path: &str,
    bytes: &[u8],
    mode: u32,
    changed: &mut Vec<String>,
) -> Result<(), String> {
    let failed = |error: io::Error| format!("{path}: {error}");
    let flags = rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK;
    let mut file = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(flags.bits() as i32)
        .open(config.on(path))
    {
        Ok(file) => file,
        Err(error) if error.raw_os_error() == Some(rustix::io::Errno::LOOP.raw_os_error()) => {
            return Ok(());
        }
        Err(error) => return Err(failed(error)),
    };
    let meta = file.metadata().map_err(failed)?;
    if !meta.is_file() {
        return Ok(());
    }
    let mut now_bytes = Vec::new();
    file.read_to_end(&mut now_bytes).map_err(failed)?;
    if now_bytes != bytes {
        return Ok(());
    }
    let now = meta.permissions().mode() & 0o7777;
    if now != mode {
        file.set_permissions(fs::Permissions::from_mode(mode))
            .map_err(failed)?;
        changed.push(format!("made {path} {mode:o}, was {now:o}"));
    }
    Ok(())
}

/// V1's device key (`llm_api_key` in its config), when it is one line of
/// printable text a header can carry. Never said or logged.
fn device_key(config: &Install) -> Option<String> {
    let text = fs::read_to_string(config.on(V1_CONFIG)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let key = value.get("llm_api_key")?.as_str()?;
    (!key.is_empty() && key.len() <= 4096 && key.bytes().all(|b| b.is_ascii_graphic()))
        .then(|| key.to_owned())
}

/// `lamp-update harness-off`: undoes what [`install`] did for Harness at
/// boot, and nothing else (#811; the first builder's yes on #705, 18 Oct
/// 11:1x). With install's record of it, [`Host::harness_off`] runs, and the
/// record goes last, so a failure keeps it and a later run tries again.
/// Without the record it changes nothing. Harness itself
/// ([`HARNESS`]), the person's sign-in under [`HARNESS_HOME`] (a reset's to
/// wipe) and the `lampos` user stay. What Harness still has is returned and
/// said; it is never stopped here. An install afterwards sets it up again.
pub fn harness_off(
    root: &Path,
    records: &str,
    host: &mut dyn Host,
    say: &mut dyn FnMut(&str),
) -> Result<Vec<String>, String> {
    let on = |path: &str| root.join(path.trim_start_matches('/').trim_end_matches('/'));
    let folder = on(records);
    let done = folder.join(HARNESS_DONE);
    if !done.exists() {
        say("Harness was not set to start at boot by install: nothing to undo");
        return Ok(Vec::new());
    }
    let _lock = Records::new(folder)
        .lock()
        .map_err(|error| format!("another lamp-update is running: {error}"))?;
    // Again under the lock, which never waits: a run that checked just
    // before another finished finds it done.
    if !done.exists() {
        say("Harness was not set to start at boot by install: nothing to undo");
        return Ok(Vec::new());
    }
    if !on(HARNESS).exists() {
        return Err(format!(
            "Harness is not at {HARNESS}: its service cannot be undone, and {} is kept",
            done.display()
        ));
    }
    let left = host.harness_off(USER, HARNESS_HOME)?;
    std::fs::remove_file(&done).map_err(|error| format!("{}: {error}", done.display()))?;
    for line in &left {
        say(line);
    }
    Ok(left)
}

/// `lamp-update harness-on` (#1676 B5): Harness on a lamp lampOS is
/// installed on, as the person's computer has it. Without Harness at
/// [`HARNESS`], [`Host::harness_on`] puts it there with Harness's own
/// installer. One already there is kept as it is, unless it is older than
/// [`HARNESS_LEAST`] and is the one `harness-on` put there (the link to the
/// launcher in [`HARNESS_HOME`]): Harness's own installer is run again, to
/// the version it ships, and `harness version` read before and after (the
/// first builder at 09:46 on #1676). A failed or still too old install, or
/// an older Harness put there otherwise, is kept as it is and said as
/// `harness_too_old`; the rest goes on. Then Harness at boot, as
/// [`install`] sets it, with its record, if not yet set; then the tasks part
/// is started again, so it hands over with Harness, but only when something
/// changed. Signing in is the person's, and not done here. Run again, it
/// changes nothing. What changed is returned.
pub fn harness_on(
    root: &Path,
    records: &str,
    host: &mut dyn Host,
    say: &mut dyn FnMut(&str),
) -> Result<Vec<String>, String> {
    let on = |path: &str| root.join(path.trim_start_matches('/').trim_end_matches('/'));
    let folder = on(records);
    if !folder.is_dir() {
        return Err(format!(
            "{} is not there: lampOS is not installed, so Harness has no user to run as",
            folder.display()
        ));
    }
    let _lock = Records::new(folder.clone())
        .lock()
        .map_err(|error| format!("another lamp-update is running: {error}"))?;
    let mut changed = Vec::new();
    let link = on(HARNESS);
    if fs::symlink_metadata(&link).is_ok() {
        let launcher = format!("{HARNESS_HOME}/.local/bin/harness");
        let ours = fs::read_link(&link).is_ok_and(|to| to == Path::new(&launcher));
        let least = shown_version(Some(HARNESS_LEAST));
        let first = host.harness_version(USER, HARNESS_HOME);
        match first.as_deref().ok().and_then(harness_version_of) {
            // Too slow to say is not known, not missing: never installed
            // over, and asked again on the next run.
            _ if first.as_ref().is_err_and(|why| timed_out(why)) => say(&format!(
                "Harness at {HARNESS} is kept as it is: its version could not be read in time ({}); asked again on the next run",
                first.as_ref().unwrap_err()
            )),
            Some(found) if found >= HARNESS_LEAST => say(&format!(
                "Harness {} is already at {HARNESS}: kept as it is",
                shown_version(Some(found))
            )),
            found if ours => {
                let was = shown_version(found);
                let installed = host.harness_on(USER, HARNESS_HOME);
                let after = host.harness_version(USER, HARNESS_HOME);
                let now = after.as_deref().ok().and_then(harness_version_of);
                match (installed, now) {
                    (Ok(()), None) if after.as_ref().is_err_and(|why| timed_out(why)) => {
                        say(&format!(
                            "Harness's own installer ran, from {was}; its version could not be read in time ({}); asked again on the next run",
                            after.as_ref().unwrap_err()
                        ))
                    }
                    (Ok(()), Some(now)) if now >= HARNESS_LEAST => changed.push(format!(
                        "Harness updated from {was} to {} by its own installer, for signing in by a spoken code",
                        shown_version(Some(now))
                    )),
                    (Ok(()), now) => say(&format!(
                        "harness_too_old: Harness is {} after its own installer; signing in by a spoken code needs {least} or later",
                        shown_version(now)
                    )),
                    (Err(error), now) => say(&format!(
                        "harness_too_old: Harness's own installer failed ({error}); Harness {} is kept; signing in by a spoken code needs {least} or later",
                        shown_version(now)
                    )),
                }
            }
            found => say(&format!(
                "harness_too_old: Harness {} at {HARNESS} wasn't put there by harness-on, so it is kept as it is; signing in by a spoken code needs {least} or later",
                shown_version(found)
            )),
        }
    } else {
        host.harness_on(USER, HARNESS_HOME)?;
        changed.push(format!(
            "Harness installed for {USER} by its own installer, at {HARNESS}"
        ));
    }
    let done = folder.join(HARNESS_DONE);
    if !done.exists() {
        // A failure leaves nothing recorded: the next run asks again.
        host.harness_at_boot(USER, HARNESS_HOME)?;
        write_whole(&done, format!("{USER} {HARNESS_HOME}\n").as_bytes(), 0o644)
            .map_err(|error| format!("{}: {error}", done.display()))?;
        changed.push(format!("Harness starts at boot as {USER}"));
    }
    if !changed.is_empty() {
        host.restart(TASKS_UNIT)?;
        changed.push(format!("{TASKS_UNIT} started again"));
    }
    for line in &changed {
        say(line);
    }
    Ok(changed)
}

/// Each model missing on the lamp, fetched and checked by size and SHA-256
/// under a temporary name, then renamed into place. One that cannot be had
/// is said, and the install goes on: its part cannot start until it is
/// there, and the next install tries again. A file already there that is not
/// the pinned one is the lamp's own (lamp-52e6's own `name.onnx`): kept, and
/// said.
fn place_models(
    config: &Install,
    channel: &mut dyn Channel,
    state: &mut State,
    changed: &mut Vec<String>,
    say: &mut dyn FnMut(&str),
    persist: &mut dyn FnMut(&State) -> Result<(), String>,
) -> Result<(), String> {
    let before = state.clone();
    for model in &config.models {
        let at = config.on(&model.path);
        match file_sha256(&at) {
            Ok(sha) if sha == model.sha256 => {
                state.settings.insert(model.path.clone(), sha);
                continue;
            }
            // An earlier pin this program placed (its SHA-256 recorded as
            // the settings' are): replaced, since its part checks the new
            // pin and would refuse it (the reviewer, thirteen, on #1052).
            Ok(sha) if state.settings.get(&model.path) == Some(&sha) => {}
            Ok(_) => {
                say(&format!("kept {}: it is not the pinned model", model.path));
                continue;
            }
            Err(_) => {}
        }
        match fetch_model(channel, model, &at) {
            Ok(()) => {
                state
                    .settings
                    .insert(model.path.clone(), model.sha256.clone());
                changed.push(format!("fetched {}", model.path));
            }
            Err(why) => say(&format!(
                "{} could not be had ({why}); its part cannot start until it is there",
                model.path
            )),
        }
    }
    if *state != before {
        persist(state)?;
    }
    Ok(())
}

/// Where [`MODELS`] places the voice's recorded failure lines on a product
/// lamp: the folder the product voice unit's `--failure-notice-dir` names
/// (docs/failure-notices.md, #1844).
pub const NOTICES_PRODUCT: &str = "/opt/lampOS/voice/notices";
/// Where `lamp-keeper prepare` places the same lines on a lamp moved from V1,
/// the folder the generic voice unit names (#2943): a moved lamp gets no
/// install, its package carries no notices, and the package's packager
/// takes every path under `/opt/lampOS` for one it carries.
pub const NOTICES_MOVED: &str = "/var/lib/lampOS/voice/notices";

/// [`MODELS`]' recorded failure lines placed in [`NOTICES_MOVED`] under
/// `root` (`/` on a lamp), by [`place_moved_notices_of`].
pub fn place_moved_notices(
    root: &Path,
    channel: &mut dyn Channel,
    say: &mut dyn FnMut(&str),
) -> Vec<String> {
    match models() {
        Ok(models) => place_moved_notices_of(&models, root, channel, say),
        Err(why) => {
            say(&why);
            Vec::new()
        }
    }
}

/// Each of `models` under [`NOTICES_PRODUCT`] placed under the same name in
/// [`NOTICES_MOVED`], only where it is missing, by install's own fetch and
/// size and SHA-256 check ([`fetch_model`]). One there at its pin is left;
/// one that isn't is kept and said, with how to take the pin: nothing records
/// what prepare wrote, so a line [`MODELS`] has since re-pinned is not told from
/// the lamp's own (the Reviewer-lead's and Fourteen's P3 on #3189). One that cannot be had
/// is said and the rest go on: never a refusal, since the voice plays its
/// sigh for a line it cannot load. Returns the paths written.
pub fn place_moved_notices_of(
    models: &[Model],
    root: &Path,
    channel: &mut dyn Channel,
    say: &mut dyn FnMut(&str),
) -> Vec<String> {
    let product = format!("{NOTICES_PRODUCT}/");
    let mut wrote = Vec::new();
    for model in models {
        let Some(name) = model.path.strip_prefix(&product) else {
            continue;
        };
        let path = format!("{NOTICES_MOVED}/{name}");
        let at = root.join(path.trim_start_matches('/'));
        match file_sha256(&at) {
            Ok(sha) if sha == model.sha256 => continue,
            Ok(_) => {
                say(&format!(
                    "kept {path}: it is not the pinned line (an older pin or the lamp's own); remove it and run prepare again to take the pin"
                ));
                continue;
            }
            Err(_) => {}
        }
        let placed = Model {
            path: path.clone(),
            ..model.clone()
        };
        match fetch_model(channel, &placed, &at) {
            Ok(()) => wrote.push(path),
            Err(why) => say(&format!(
                "{path} could not be had ({why}); the voice sighs in its place until it is there"
            )),
        }
    }
    wrote
}

/// A model fetched and checked to `at`, whole or not at all. A channel out
/// of reach is said as [`Fetch::Unreachable`], so a round tries again; any
/// other failure is [`Fetch::Failed`].
pub(crate) fn fetch_model(
    channel: &mut dyn Channel,
    model: &Model,
    at: &Path,
) -> Result<(), Fetch> {
    let failed = |why: String| Fetch::Failed(why);
    let parent = at.parent().ok_or_else(|| failed("no folder".into()))?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o755)
        .create(parent)
        .map_err(|error| failed(error.to_string()))?;
    let name = at
        .file_name()
        .ok_or_else(|| failed("no name".into()))?
        .to_string_lossy();
    let temporary = parent.join(format!(".{name}.fetching"));
    let archive = parent.join(format!(".{name}.archive"));
    let result = (|| {
        match &model.archive {
            None => fetch_checked(channel, &model.url, model.bytes, &model.sha256, &temporary)?,
            Some(pinned) => {
                fetch_checked(channel, &model.url, pinned.bytes, &pinned.sha256, &archive)?;
                let out =
                    fs::File::create(&temporary).map_err(|error| failed(error.to_string()))?;
                let status = Command::new("tar")
                    .arg("-xzOf")
                    .arg(&archive)
                    .arg(&pinned.member)
                    .stdout(out)
                    .status()
                    .map_err(|error| failed(format!("tar: {error}")))?;
                if !status.success() {
                    return Err(failed(format!("tar could not take {} out", pinned.member)));
                }
                let sha = file_sha256(&temporary).map_err(|error| failed(error.to_string()))?;
                let size = fs::metadata(&temporary)
                    .map_err(|error| failed(error.to_string()))?
                    .len();
                if size != model.bytes || sha != model.sha256 {
                    return Err(failed(format!(
                        "{} does not match its SHA-256",
                        pinned.member
                    )));
                }
            }
        }
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o644))
            .and_then(|()| fs::File::open(&temporary)?.sync_all())
            .and_then(|()| fs::rename(&temporary, at))
            .and_then(|()| sync_parent(at))
            .map_err(|error| failed(error.to_string()))
    })();
    let _ = fs::remove_file(&archive);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// The file at `url` into `to`, only if it is `bytes` long with `sha256`.
fn fetch_checked(
    channel: &mut dyn Channel,
    url: &str,
    bytes: u64,
    sha256: &str,
    to: &Path,
) -> Result<(), Fetch> {
    let mut out = fs::File::create(to).map_err(|error| Fetch::Failed(error.to_string()))?;
    let mut hashing = Hashing {
        file: &mut out,
        context: digest::Context::new(&digest::SHA256),
    };
    let fetched = channel.fetch(url, bytes, &mut hashing);
    let sha = manifest::hex(hashing.context.finish().as_ref());
    match fetched {
        Ok(size) if size == bytes && sha == sha256 => Ok(()),
        Ok(_) => Err(Fetch::Failed("it does not match its SHA-256".into())),
        Err(Fetch::Missing) => Err(Fetch::Failed(format!("{url} is not there"))),
        Err(Fetch::Unreachable(why)) => Err(Fetch::Unreachable(why)),
        Err(error) => Err(Fetch::Failed(format!("{error:?}"))),
    }
}

/// V1's key where V1 still owns the lamp, for `lamp-keeper prepare`
/// (docs/keeper.md): [`CREDENTIAL_PATH`] and [`JEV_KEY_PATH`] written by
/// [`settings_with_mode`], as [`install`] writes them (0600, whole), and
/// recorded in lamp-update's records (`records` under `root`) the same way,
/// so install and prepare share one rule. Only a file that is missing is
/// written, and only into a lamp that never had one from install or
/// prepare: one recorded and now gone was removed on purpose (a reset wipes
/// the user's key, #806), and V1's config, which may still hold the previous
/// owner's, never puts it back. The records are read as install reads them:
/// a corrupt file is set aside and said, and V1's key is then not copied;
/// one that cannot be read is said, and nothing is written. `root` is `/` on
/// a lamp. Returns the paths written; the key is never in what it returns or
/// says.
pub fn provision_device_key(
    root: &Path,
    records: &str,
    say: &mut dyn FnMut(&str),
) -> Result<Vec<&'static str>, String> {
    let config = Install {
        root: root.to_owned(),
        channel: String::new(),
        channel_url: String::new(),
        key: [0; 32],
        records: records.to_owned(),
        setup: false,
        models: Vec::new(),
    };
    let records = Records::new(config.on(&config.records));
    let _lock = records
        .lock()
        .map_err(|error| format!("another lamp-update is running: {error}"))?;
    // The records as install reads them (`Records::state`, #1094's review):
    // a corrupt file is set aside and said, and then what was written
    // before is not known, so V1's key is not copied (a reset may have wiped
    // the user's). One that cannot be read is said and left alone, nothing
    // written: a refusal would refuse every lamp-os update.
    let state_file = config.on(&config.records).join("state.json");
    let existed = state_file.exists();
    let mut state = match records.state(say) {
        Ok(state) => state,
        Err(error) => {
            say(&format!(
                "the records cannot be read ({error}): V1's key is not written; install or setup sees to it"
            ));
            return Ok(Vec::new());
        }
    };
    let records_lost = records_lost(&config.on(&config.records), existed);
    let mut wanted = Vec::new();
    for path in [CREDENTIAL_PATH, JEV_KEY_PATH] {
        // Every key path is marked, the ones there too: a key there now is
        // removed by a later reset, and must not come back then (the
        // reviewer on #1263).
        if records_lost {
            state.settings.insert(path.to_owned(), SET_ASIDE.to_owned());
        }
        if fs::symlink_metadata(config.on(path)).is_ok() {
            continue;
        }
        if records_lost {
            say(&format!(
                "{path} is missing and the records were set aside: V1's key is not copied; setup takes the user's"
            ));
        } else if state.settings.contains_key(path) {
            say(&format!(
                "{path} was removed since it was written (a reset?): V1's key is not put back; setup takes the user's"
            ));
        } else {
            wanted.push(path);
        }
    }
    if records_lost {
        save(&records, &state)?;
    }
    if wanted.is_empty() {
        return Ok(wanted);
    }
    // No usable key: said, and left to setup, as install does; prepare runs
    // before every lamp-os update, so refusing here would refuse them all.
    let Some(key) = device_key(&config) else {
        for path in wanted {
            say(&format!(
                "no key: nothing written to {path} until setup takes the user's (none in {V1_CONFIG})"
            ));
        }
        return Ok(Vec::new());
    };
    let private: Vec<(&str, &str)> = wanted.iter().map(|path| (*path, key.as_str())).collect();
    let mut changed = Vec::new();
    settings_with_mode(
        &config,
        &private,
        0o600,
        &mut state,
        &mut changed,
        say,
        &mut |state| save(&records, state),
    )?;
    Ok(wanted)
}

fn has_user(config: &Install, name: &str) -> bool {
    fs::read_to_string(config.on("/etc/passwd")).is_ok_and(|text| {
        text.lines()
            .any(|line| line.split(':').next() == Some(name))
    })
}

fn existing_groups(config: &Install, wanted: &[&str]) -> Vec<String> {
    let text = fs::read_to_string(config.on("/etc/group")).unwrap_or_default();
    wanted
        .iter()
        .filter(|group| {
            text.lines()
                .any(|line| line.split(':').next() == Some(**group))
        })
        .map(|group| (*group).to_owned())
        .collect()
}

/// Whether `zone` is a zone file in the lamp's zone data, as setup checks a
/// zone before it saves one (`zone_known` in `crates/setup`): a plain name
/// whose file, links followed, stays inside `/usr/share/zoneinfo` and is a
/// `TZif` file.
fn zone_file_known(config: &Install, zone: &str) -> bool {
    if zone.is_empty()
        || zone.len() > 64
        || !zone
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
        || !zone
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/_+-".contains(&b))
    {
        return false;
    }
    let zoneinfo = config.on("/usr/share/zoneinfo");
    let (Ok(root), Ok(file)) = (zoneinfo.canonicalize(), zoneinfo.join(zone).canonicalize()) else {
        return false;
    };
    let mut magic = [0u8; 4];
    file.starts_with(&root)
        && fs::File::open(&file)
            .and_then(|mut opened| io::Read::read_exact(&mut opened, &mut magic))
            .is_ok()
        && &magic == b"TZif"
}

/// The zone the lamp names: `/etc/timezone`, which setup writes with the
/// person's zone, else where `/etc/localtime` links. None when neither says.
fn saved_zone(config: &Install) -> Option<String> {
    if let Ok(text) = fs::read_to_string(config.on("/etc/timezone")) {
        let zone = text.trim();
        if !zone.is_empty() {
            return Some(zone.to_owned());
        }
    }
    fs::read_link(config.on("/etc/localtime"))
        .ok()
        .and_then(|target| {
            let target = target.to_string_lossy().into_owned();
            target
                .split_once("zoneinfo/")
                .map(|(_, zone)| zone.to_owned())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first builder's P3 4 on #2617: install never lists the Wi-Fi
    /// helper (`lamp-setup-wifi.socket`, `lamp-setup-wifi@.service`)
    /// without its boot half, `lamp-setup-wifi-restore.service`, enabled:
    /// a change a power cut interrupted would boot on an unconfirmed
    /// network.
    #[test]
    fn the_wifi_helper_is_never_listed_without_its_restore_at_boot() {
        assert!(wifi_helper_whole(
            &PRODUCT.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
            &enabled()
        ));
        let helper = ["lamp-setup-wifi.socket", "lamp-setup-wifi@.service"];
        let restore = "lamp-setup-wifi-restore.service";
        assert!(!wifi_helper_whole(&helper, &[]));
        assert!(!wifi_helper_whole(&helper[..1], &[]));
        let both = [helper[0], helper[1], restore];
        assert!(!wifi_helper_whole(&both, &[]));
        assert!(wifi_helper_whole(&both, &[restore.to_owned()]));
        assert!(wifi_helper_whole(&[], &[]));
    }

    /// #2189: every program the factory's steps run from a part's `current`
    /// folder (`*_BIN=${*_BIN:-/opt/lampOS/<part>/current/<binary>}`) is
    /// one install puts there: the part's own binary, or one beside it.
    /// Read from steps.sh, so a step that needs a new tool fails here first.
    #[test]
    fn every_tool_the_factory_steps_run_is_installed_where_they_look() {
        const STEPS: &str = include_str!("../../../deploy/factory/steps.sh");
        let wanted: Vec<(String, String)> = STEPS
            .lines()
            .filter_map(|line| {
                let default = line.split_once(":-")?.1.strip_suffix('}')?;
                let (folder, binary) = default.rsplit_once("/current/")?;
                folder
                    .starts_with("/opt/lampOS/")
                    .then(|| (format!("{folder}/"), binary.to_owned()))
            })
            .collect();
        assert!(
            wanted
                .iter()
                .any(|(_, binary)| binary == "lamp-body-measure"),
            "{wanted:?}"
        );
        let parts = parts();
        for (folder, binary) in &wanted {
            let part = parts
                .iter()
                .find(|part| &part.folder == folder)
                .unwrap_or_else(|| panic!("no part installs {folder}"));
            let beside = BESIDE
                .iter()
                .chain(BESIDE_IF_CARRIED)
                .filter(|(owner, _)| *owner == part.binary)
                .flat_map(|(_, beside)| beside.iter())
                .any(|name| name == binary);
            assert!(
                &part.binary == binary || beside,
                "steps.sh runs {folder}current/{binary}, which install does not put there"
            );
        }
    }

    #[test]
    fn every_product_part_starts_through_its_current_link() {
        let parts = parts();
        // lamp-update-back starts from its fixed path, which no round
        // switches (#1680).
        let linked = |name: &&str| {
            name.ends_with(".service")
                && *name != "lamp-update-back.service"
                && *name != crate::apply::UNIT
        };
        for (name, text) in PRODUCT.iter().filter(|(name, _)| linked(name)) {
            assert!(part(text).is_some(), "{name} starts through a current link");
        }
        // Setup, its reset, the waking face, its recovery (#3020) and
        // setup's three helpers (the names', the Wi-Fi's and its boot
        // restore) share one binary, and so do the air and its bus clock,
        // the skills and their manage helper, and the eyes and their camera
        // helper (#2892); every other service is a part.
        let services = PRODUCT.iter().filter(|(name, _)| linked(name)).count();
        assert_eq!(parts.len(), services - 9, "every product service is a part");
        for part in &parts {
            let name = part.binary.trim_start_matches("lamp-");
            assert_eq!(part.folder, format!("/opt/lampOS/{name}/"), "{part:?}");
        }
    }

    /// #1722, #2414: the skills' manage helper is installed, its socket
    /// enabled and its template never (systemd starts one for each
    /// connection), so the app's remove and a made skill's yes reach it.
    #[test]
    fn the_skills_manage_socket_is_enabled_and_its_template_is_not() {
        assert!(
            PRODUCT
                .iter()
                .any(|(name, _)| *name == "lamp-skills-manage@.service")
        );
        assert!(
            enabled()
                .iter()
                .any(|name| name == "lamp-skills-manage.socket")
        );
        assert!(
            !enabled().iter().any(|name| name.contains('@')),
            "{:?}",
            enabled()
        );
    }

    /// #2853: every unit in `deploy/product/` is in the install list, so a
    /// lamp gets it with an update and no hand step, or is named here with
    /// why it is not yet; every socket installed is enabled.
    #[test]
    fn every_deploy_product_unit_is_installed_or_named_left_out() {
        // Left out until their lamp check: the keeper's restart helper
        // (#2193) and the own keys' helper (#2094).
        const LEFT_OUT: &[&str] = &[
            "lamp-keeper-restart.socket",
            "lamp-keeper-restart@.service",
            "lamp-setup-keys.socket",
            "lamp-setup-keys@.service",
        ];
        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/product");
        let listed: Vec<&str> = PRODUCT.iter().map(|(name, _)| *name).collect();
        let mut missing = Vec::new();
        for entry in fs::read_dir(&folder).unwrap() {
            let name = entry.unwrap().file_name().to_string_lossy().into_owned();
            let unit = [
                ".service", ".socket", ".timer", ".target", ".slice", ".path",
            ]
            .iter()
            .any(|suffix| name.ends_with(suffix));
            if unit && !listed.contains(&name.as_str()) && !LEFT_OUT.contains(&name.as_str()) {
                missing.push(name);
            }
        }
        assert!(
            missing.is_empty(),
            "in deploy/product but not installed: {missing:?}"
        );
        for name in LEFT_OUT {
            assert!(
                !listed.contains(name),
                "{name} is installed: take it off LEFT_OUT"
            );
            // A renamed or removed unit leaves no stale entry that passes.
            assert!(
                folder.join(name).is_file(),
                "{name} is not in deploy/product: take it off LEFT_OUT"
            );
        }
        // The Wi-Fi restore runs at boot; the helpers' templates start
        // only from their sockets.
        let on = enabled();
        assert!(
            on.iter()
                .any(|name| name == "lamp-setup-wifi-restore.service"),
            "{on:?}"
        );
        for template in ["lamp-setup-change@.service", "lamp-setup-wifi@.service"] {
            assert!(listed.contains(&template), "{template} is not installed");
            assert!(!on.iter().any(|name| name == template), "{on:?}");
        }
        for socket in listed.iter().filter(|name| name.ends_with(".socket")) {
            assert!(
                enabled().iter().any(|name| name == socket),
                "{socket} is not enabled"
            );
        }
    }

    /// #2892: the eyes' camera helper is installed, its socket enabled and
    /// its template never, and it names the same camera as the eyes, which
    /// ask it; #4074: both on any port, since it differs by
    /// unit (`3-1.1.4` on lamp-4ace, `3-1.3.4` on lamp-a63e).
    #[test]
    fn the_eyes_camera_helper_socket_is_enabled_and_names_the_eyes_camera() {
        let unit = |name: &str| {
            PRODUCT
                .iter()
                .find(|(unit, _)| *unit == name)
                .map(|(_, text)| *text)
                .unwrap()
        };
        assert!(
            enabled()
                .iter()
                .any(|name| name == "lamp-eyes-camera.socket")
        );
        assert!(!enabled().iter().any(|name| name.contains('@')));
        let usb = |text: &str| {
            let at = text.find("--usb ").unwrap() + "--usb ".len();
            text[at..].split_whitespace().next().unwrap().to_owned()
        };
        // The same camera: the helper's ids are the eyes'.
        assert_eq!(
            usb(unit("lamp-eyes-camera@.service")),
            usb(unit("lamp-eyes.service"))
        );
        assert!(
            unit("lamp-eyes.service")
                .contains("--camera-helper /run/lampos-eyes-camera/camera.sock")
        );
        assert!(
            unit("lamp-eyes-camera.socket")
                .contains("ListenStream=/run/lampos-eyes-camera/camera.sock")
        );
        assert!(unit("lamp-eyes-camera@.service").contains("lamp-eyes reauthorize-camera --usb"));
        // #4074: the eyes and their helper name the product's camera by its
        // ids, no port and no node: every lamp-eyes since 20 Oct (#2190)
        // finds the one camera with the ids on any port at each open, and
        // every channel's helper since stable 8250 (#4077) does the same.
        assert_eq!(usb(unit("lamp-eyes.service")), "01da:5875");
        assert!(!unit("lamp-eyes.service").contains("--device"));
        assert_eq!(usb(unit("lamp-eyes-camera@.service")), "01da:5875");
        for name in ["lamp-eyes.service", "lamp-eyes-camera@.service"] {
            assert_eq!(unit(name).matches("--usb ").count(), 1, "{name}");
        }
    }

    /// #1815: the cloud is enabled with the lamp, so a Lamp 2 set up from
    /// the app is online at the next power-up too, not only after
    /// `cloud-on.sh`.
    #[test]
    fn the_cloud_is_enabled_with_the_lamp() {
        assert!(enabled().iter().any(|name| name == "lamp-cloud.service"));
        assert!(
            enabled()
                .iter()
                .any(|name| name == "lampos-setup-done.path")
        );
    }

    /// #812: the passive self-test is started by its timer alone, which the
    /// lamp's target wants, as the records' check is by its own.
    #[test]
    fn a_timed_service_is_never_enabled_and_its_timer_is() {
        let enabled = enabled();
        for timed in TIMED {
            assert!(!enabled.iter().any(|name| name == timed), "{timed}");
            let timer = timed.replace(".service", ".timer");
            assert!(enabled.contains(&timer), "{timer}");
            assert!(PRODUCT.iter().any(|(name, _)| *name == timer), "{timer}");
        }
        let (_, timer) = PRODUCT
            .iter()
            .find(|(name, _)| *name == "lamp-selftest.timer")
            .unwrap();
        assert!(timer.contains("WantedBy=lampos.target"), "{timer}");
        let (_, service) = PRODUCT
            .iter()
            .find(|(name, _)| *name == "lamp-selftest.service")
            .unwrap();
        assert!(!service.contains("[Install]"), "{service}");
        assert!(service.contains("--occasion first-boot"), "{service}");
    }

    #[test]
    fn no_product_unit_names_a_lamp() {
        for (name, text) in PRODUCT {
            // What systemd reads; a comment may say where a value was measured.
            let settings: String = text
                .lines()
                .filter(|line| !line.trim_start().starts_with('#'))
                .collect();
            assert!(!settings.contains("ConditionHost"), "{name}");
            // A template's placeholder is @UPPER_CASE@; the camera's USB path has an @ too.
            let placeholder = settings.split('@').skip(1).step_by(2).any(|piece| {
                !piece.is_empty() && piece.bytes().all(|b| b.is_ascii_uppercase() || b == b'_')
            });
            assert!(!placeholder, "{name} has a placeholder");
            assert!(!settings.contains("lamp-4ace"), "{name} names a lamp");
            assert!(!settings.contains("lamp-52e6"), "{name} names a lamp");
            assert!(!settings.contains("/home/"), "{name} names a home");
            assert!(
                crate::unit::program(text).is_none(),
                "{name} names a build's folder"
            );
        }
    }

    #[test]
    fn a_unit_through_current_is_read() {
        let text = "[Service]\nExecStart=-/opt/lampOS/body/current/lamp-body --x\n";
        assert_eq!(
            part(text),
            Some(Part {
                folder: "/opt/lampOS/body/".into(),
                binary: "lamp-body".into()
            })
        );
        for text in [
            "ExecStart=/opt/lampOS/body/lamp-body\n",
            "ExecStart=/opt/lampOS/body/latest/lamp-body\n",
            "ExecStart=/opt/lampOS/a/b/current/lamp-body\n",
            "ExecStart=/opt/lampOS/body/current/sh\n",
        ] {
            assert_eq!(part(text), None, "{text}");
        }
    }

    #[test]
    fn offsets_are_read() {
        assert_eq!(offset_minutes("+0700"), Some(420));
        assert_eq!(offset_minutes("-0330"), Some(-210));
        assert_eq!(offset_minutes("+0000"), Some(0));
        assert_eq!(offset_minutes("0700"), None);
        assert_eq!(offset_minutes("+07"), None);
    }

    fn sh(script: &str) -> Vec<String> {
        ["sh", "-c", script].map(str::to_owned).into()
    }

    /// Install's own commands are bounded: one that stalls, or ends while a
    /// child it left holds its pipes, is an error within the bound naming
    /// the command, never a hang. The four waits run at once (#1806), each
    /// timed on its own thread: one after another they took 4 s.
    #[test]
    fn install_commands_never_wait_past_their_bound() {
        let second = Duration::from_secs(1);
        let checks: Vec<_> = ["sleep 30", "sleep 30 & echo ok"]
            .into_iter()
            .flat_map(|script| {
                let output = std::thread::spawn(move || {
                    let started = Instant::now();
                    let why = output_of_within(&sh(script), second).unwrap_err();
                    assert!(started.elapsed() < Duration::from_secs(3), "{script}");
                    assert_eq!(
                        why,
                        format!(
                            "sh -c {script}: did not end within 1 s: it ran on, or a child it left held its output"
                        )
                    );
                });
                let run = std::thread::spawn(move || {
                    let started = Instant::now();
                    let why =
                        run_within(Command::new("sh").args(["-c", script]), second).unwrap_err();
                    assert!(started.elapsed() < Duration::from_secs(3), "{script}");
                    assert!(why.contains("did not end within 1 s"), "{why}");
                    assert!(why.starts_with("\"sh\" \"-c\""), "{why}");
                });
                [output, run]
            })
            .collect();
        for check in checks {
            check.join().unwrap();
        }
    }

    #[test]
    fn install_commands_say_their_stderr_or_give_their_stdout() {
        assert_eq!(
            output_of_within(&sh("echo '  no such unit  ' >&2; exit 1"), JOURNAL_WITHIN),
            Err("sh -c echo '  no such unit  ' >&2; exit 1: no such unit".to_owned())
        );
        let failing = run_within(
            Command::new("sh").args(["-c", "echo ' bad ' >&2; exit 1"]),
            JOURNAL_WITHIN,
        );
        assert_eq!(
            failing,
            Err("\"sh\" \"-c\" \"echo ' bad ' >&2; exit 1\": bad".to_owned())
        );
        assert_eq!(
            output_of_within(&sh("echo ok"), JOURNAL_WITHIN),
            Ok("ok\n".to_owned())
        );
        assert_eq!(
            run_within(Command::new("sh").args(["-c", "echo ok"]), JOURNAL_WITHIN),
            Ok(())
        );
    }
}
