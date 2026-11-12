//! The sessions the lamp itself started or handed work to (#6274, the
//! first builder's ruling of 11 Nov): by machine, each one's id, its name
//! as last seen and when it was last used, never anything of its work.
//! `harness ps --all` lists only sessions open or stopped in the last seven
//! days, so a person who names an older one the lamp started ("open the
//! Christmas dates one") is found through this index, by its id
//! ([`crate::harness::Cli`]'s named lookup).
//!
//! Kept in one file of the tasks folder ([`crate::runtime::SESSION_INDEX_FILE`]
//! under `--dir`), which the lamp's reset wipes with the folder, written
//! whole beside it and renamed over it (mode 0600, never through a link),
//! and removed by the owner's "forget everything" (`forget_sessions`, on
//! the voice's route). Harness read as signed out lets the account
//! machines' sessions go and keeps those on this computer's own id, and a
//! read of the account's machines lets an unpaired machine's go
//! ([`forget_machines`]). A hand-over or look still running when the
//! forget comes may keep its one session again after it: that is a use
//! after the forget, as any later one is. Each change reads the file, changes it and writes it
//! again under one lock of the process, so the part's lanes never write
//! over each other. A file that can't be read, or isn't this shape, is read
//! as none, with a line.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
    sync::Mutex,
};

/// The most sessions kept, on every machine together: one more lets the
/// least lately used go.
pub const MOST_INDEXED: usize = 200;

/// The file is at most this long; a longer one is read as none. 200
/// entries of an id, a name of at most [`MOST_NAME_BYTES`] and a time fit
/// in about 70 KB.
const MOST_FILE_BYTES: u64 = 128 * 1024;

/// The most bytes of a name kept, cut at a character.
pub const MOST_NAME_BYTES: usize = 200;

/// The one lock every read-change-write of the file takes.
static FILE: Mutex<()> = Mutex::new(());

/// One session the lamp started or handed work to: its id, its name as
/// last seen (empty while none is known but the lamp's own), and when the
/// lamp last started, typed into, reopened or opened it (unix ms).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Indexed {
    pub id: String,
    pub name: String,
    pub last_used: u64,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexFile {
    version: u32,
    machines: BTreeMap<String, Vec<Indexed>>,
}

/// Every session kept, by machine id. None when the file is missing or
/// can't be read.
pub fn read(file: &Path) -> BTreeMap<String, Vec<Indexed>> {
    let _held = FILE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    load(file).machines
}

/// `id` on `machine` used at `now_ms`: kept, or its time moved on, with
/// `name` when one is given (a lamp-made name never is), else the one kept.
pub fn record(file: &Path, machine: &str, id: &str, name: Option<&str>, now_ms: u64) {
    change(file, |index| {
        let entries = index.machines.entry(machine.to_owned()).or_default();
        match entries.iter_mut().find(|entry| entry.id == id) {
            Some(entry) => {
                entry.last_used = entry.last_used.max(now_ms);
                if let Some(name) = name {
                    entry.name = cut(name);
                }
            }
            None => entries.push(Indexed {
                id: id.to_owned(),
                name: name.map(cut).unwrap_or_default(),
                last_used: now_ms,
            }),
        }
        bound(index);
        true
    });
}

/// The names a list of `machine` shows for sessions kept (`(id, name)`),
/// kept as their names; their times stay. Written only when one changed.
pub fn rename(file: &Path, machine: &str, names: &[(String, String)]) {
    if names.is_empty() {
        return;
    }
    change(file, |index| {
        let Some(entries) = index.machines.get_mut(machine) else {
            return false;
        };
        let mut changed = false;
        for (id, name) in names {
            let name = cut(name);
            if let Some(entry) = entries.iter_mut().find(|entry| entry.id == *id)
                && entry.name != name
            {
                entry.name = name;
                changed = true;
            }
        }
        changed
    });
}

/// `id` on `machine` is no longer there (Harness says so): let go.
pub fn drop_id(file: &Path, machine: &str, id: &str) {
    change(file, |index| {
        let Some(entries) = index.machines.get_mut(machine) else {
            return false;
        };
        let before = entries.len();
        entries.retain(|entry| entry.id != id);
        let changed = entries.len() != before;
        if entries.is_empty() {
            index.machines.remove(machine);
        }
        changed
    });
}

/// Harness signed out, or a machine unpaired (the P3s on #6447): the
/// sessions kept on machines `keep` refuses let go. Written only when one
/// went.
pub fn forget_machines(file: &Path, keep: impl Fn(&str) -> bool) {
    change(file, |index| {
        let before = index.machines.len();
        index.machines.retain(|machine, _| keep(machine));
        index.machines.len() != before
    });
}

/// The owner's "forget everything": the file removed. Whether there was
/// one.
pub fn forget(file: &Path) -> io::Result<bool> {
    let _held = FILE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let partial = partial_of(file)?;
    match fs::remove_file(&partial) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    match fs::remove_file(file) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    }
    if let Some(parent) = file.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(true)
}

/// `name` whole, or cut at a character to [`MOST_NAME_BYTES`].
fn cut(name: &str) -> String {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.len() <= MOST_NAME_BYTES {
        return name;
    }
    let mut end = MOST_NAME_BYTES;
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    name[..end].to_owned()
}

/// At most [`MOST_INDEXED`]: the least lately used let go.
fn bound(index: &mut IndexFile) {
    loop {
        let count: usize = index.machines.values().map(Vec::len).sum();
        if count <= MOST_INDEXED {
            return;
        }
        let oldest = index
            .machines
            .iter()
            .flat_map(|(machine, entries)| {
                entries
                    .iter()
                    .map(move |entry| (entry.last_used, machine.clone(), entry.id.clone()))
            })
            .min();
        let Some((_, machine, id)) = oldest else {
            return;
        };
        if let Some(entries) = index.machines.get_mut(&machine) {
            entries.retain(|entry| entry.id != id);
            if entries.is_empty() {
                index.machines.remove(&machine);
            }
        }
    }
}

/// Read, changed by `changed` (which says whether it changed anything),
/// and written whole when it did. A failed write is said, and nothing else.
fn change(file: &Path, changed: impl FnOnce(&mut IndexFile) -> bool) {
    let _held = FILE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut index = load(file);
    if !changed(&mut index) {
        return;
    }
    index.version = 1;
    if let Err(error) = write(file, &index) {
        eprintln!("tasks: the sessions the lamp started could not be kept ({error})");
    }
}

fn load(file: &Path) -> IndexFile {
    match try_load(file) {
        Ok(index) => index,
        Err(error) if error.kind() == io::ErrorKind::NotFound => IndexFile::default(),
        Err(error) => {
            eprintln!(
                "tasks: the sessions the lamp started could not be read ({error}); none are taken as kept"
            );
            IndexFile::default()
        }
    }
}

fn try_load(file: &Path) -> io::Result<IndexFile> {
    let metadata = fs::symlink_metadata(file)?;
    if !metadata.is_file() || metadata.len() > MOST_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not a small file",
        ));
    }
    let index: IndexFile = serde_json::from_slice(&fs::read(file)?)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "not the index"))?;
    let count: usize = index.machines.values().map(Vec::len).sum();
    if index.version != 1 || count > MOST_INDEXED {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not version 1"));
    }
    Ok(index)
}

fn partial_of(file: &Path) -> io::Result<std::path::PathBuf> {
    let name = file
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?;
    let mut partial = name.to_os_string();
    partial.push(".partial");
    Ok(file.with_file_name(partial))
}

/// Written whole beside `file`, then renamed over it, as the folders seen
/// are (`harness::NewFolder`).
fn write(file: &Path, index: &IndexFile) -> io::Result<()> {
    let bytes = serde_json::to_vec(index).map_err(io::Error::other)?;
    let partial = partial_of(file)?;
    let mut out = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
        .open(&partial)?;
    out.write_all(&bytes)?;
    out.sync_all()?;
    drop(out);
    fs::rename(&partial, file)?;
    if let Some(parent) = file.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}
