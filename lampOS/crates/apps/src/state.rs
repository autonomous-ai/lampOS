//! The helper's marks, root's own state: one empty file per app and mark.
//! A mark outlives a crash, so the sweep can finish what was cut.
use crate::name::app_name_ok;
use std::{fs, io, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// An install or an update was begun.
    Installing,
    /// A remove was begun: written first, cleared last.
    Removing,
    /// The owner stopped it: an update never starts it again.
    Stopped,
}

impl Mark {
    const ALL: [Mark; 3] = [Mark::Installing, Mark::Removing, Mark::Stopped];

    fn suffix(self) -> &'static str {
        match self {
            Mark::Installing => "installing",
            Mark::Removing => "removing",
            Mark::Stopped => "stopped",
        }
    }
}

fn file(state: &Path, name: &str, mark: Mark) -> io::Result<std::path::PathBuf> {
    if !app_name_ok(name) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not an app name",
        ));
    }
    Ok(state.join(format!("{name}.{}", mark.suffix())))
}

pub fn set(state: &Path, name: &str, mark: Mark) -> io::Result<()> {
    let path = file(state, name, mark)?;
    fs::create_dir_all(state)?;
    fs::File::create(&path)?.sync_all()?;
    fs::File::open(state)?.sync_all()
}

/// Clearing a mark that is not there is fine.
pub fn clear(state: &Path, name: &str, mark: Mark) -> io::Result<()> {
    match fs::remove_file(file(state, name, mark)?) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

pub fn has(state: &Path, name: &str, mark: Mark) -> bool {
    file(state, name, mark)
        .and_then(fs::symlink_metadata)
        .is_ok()
}

/// Every app with any mark, with its marks, by name; files that are not
/// ours are passed over.
pub fn marked(state: &Path) -> Vec<(String, Vec<Mark>)> {
    let mut names: Vec<String> = Vec::new();
    if let Ok(entries) = fs::read_dir(state) {
        for entry in entries.flatten() {
            let file = entry.file_name().to_string_lossy().into_owned();
            if let Some((name, _)) = file.rsplit_once('.')
                && app_name_ok(name)
                && !names.iter().any(|n| n == name)
            {
                names.push(name.to_owned());
            }
        }
    }
    names.sort();
    names
        .into_iter()
        .filter_map(|name| {
            let marks: Vec<Mark> = Mark::ALL
                .into_iter()
                .filter(|mark| has(state, &name, *mark))
                .collect();
            (!marks.is_empty()).then_some((name, marks))
        })
        .collect()
}
