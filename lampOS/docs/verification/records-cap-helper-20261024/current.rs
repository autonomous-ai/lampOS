use std::{fs, io::Read, path::Path};
fn plain_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file())
}
fn group_text(path: &Path, limit: u64) -> Option<String> {
    if !plain_file(path) {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > limit {
        return None;
    }
    String::from_utf8(bytes).ok()
}

#[test]
fn exact_byte_caps_are_independent_of_parsing() {
    use std::os::unix::fs::DirBuilderExt;
    let group = std::env::temp_dir().join(format!("fourth-records-cap-helper-{}", std::process::id()));
    fs::DirBuilder::new().mode(0o700).create(&group).unwrap();
    for (name, limit) in [("cgroup.events", 4096), ("cgroup.procs", 65_536)] {
        let path = group.join(name);
        let at_limit = " ".repeat(limit);
        fs::write(&path, &at_limit).unwrap();
        assert_eq!(group_text(&path, limit as u64).as_deref(), Some(at_limit.as_str()));
        fs::write(&path, format!("{at_limit} ")).unwrap();
        assert!(group_text(&path, limit as u64).is_none());
    }
    fs::remove_dir_all(group).unwrap();
}
