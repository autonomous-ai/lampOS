use std::{collections::BTreeMap,fs,io::Read,path::{Path,PathBuf}};
fn group_empty_at(mount: &Path, unit: &str, reported: Option<&str>) -> bool {
    if !unit.starts_with("lamp-records-pair-")
        || !unit.ends_with(".service")
        || !unit
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
    {
        return false;
    }
    let expected = format!("/system.slice/{unit}");
    if reported.is_some_and(|group| !group.is_empty() && group != expected) {
        return false;
    }
    // An empty parent member list does not include child cgroups. Unified v2's
    // populated flag proves the whole subtree is empty before evidence removal.
    if !plain_directory(mount)
        || !plain_file(&mount.join("cgroup.controllers"))
        || !plain_directory(&mount.join("system.slice"))
    {
        return false;
    }
    let path = mount.join("system.slice").join(unit);
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return true,
        Ok(meta) if meta.is_dir() => {}
        _ => return false,
    }
    let Some(members) = group_text(&path.join("cgroup.procs"), 65_536) else {
        return false;
    };
    if !members.is_empty() {
        return false;
    }
    let Some(events) = group_text(&path.join("cgroup.events"), 4096) else {
        return false;
    };
    let mut fields = BTreeMap::new();
    for line in events.lines() {
        let mut words = line.split_whitespace();
        let (Some(key), Some(value), None) = (words.next(), words.next(), words.next()) else {
            return false;
        };
        if !key.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
            || !value.bytes().all(|b| b.is_ascii_digit())
            || fields.insert(key, value).is_some()
        {
            return false;
        }
    }
    fields.get("populated") == Some(&"0")
}
fn plain_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir())
}
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

#[cfg(test)]
mod cgroup_tests {
    use super::*;
    use std::{
        os::unix::fs::symlink,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT: AtomicU64 = AtomicU64::new(0);
    const UNIT: &str = "lamp-records-pair-1-control-0.service";
    struct Tree(PathBuf);
    impl Tree {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "records-cgroup-control-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            fs::create_dir(path.join("system.slice")).unwrap();
            fs::write(path.join("cgroup.controllers"), "cpu memory\n").unwrap();
            Self(path)
        }
        fn group(&self, members: &str, events: &str) -> PathBuf {
            let path = self.0.join("system.slice").join(UNIT);
            fs::create_dir(&path).unwrap();
            fs::write(path.join("cgroup.procs"), members).unwrap();
            fs::write(path.join("cgroup.events"), events).unwrap();
            path
        }
        fn empty(&self) -> bool {
            group_empty_at(&self.0, UNIT, Some(&format!("/system.slice/{UNIT}")))
        }
    }
    impl Drop for Tree {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    #[test]
    fn empty_parent_with_live_descendant_refuses_cleanup() {
        let tree = Tree::new();
        let group = tree.group("", "populated 1\nfrozen 0\n");
        fs::create_dir(group.join("child")).unwrap();
        fs::write(group.join("child/cgroup.procs"), "123\n").unwrap();
        assert!(!tree.empty());
        assert!(group.join("child/cgroup.procs").exists());
    }
    #[test]
    fn genuine_empty_subtree_and_absent_exact_group_allow_cleanup() {
        let tree = Tree::new();
        assert!(tree.empty());
        assert!(group_empty_at(&tree.0, UNIT, None));
        tree.group("", "populated 0\nfrozen 0\n");
        assert!(tree.empty());
        assert!(group_empty_at(&tree.0, UNIT, None));
        assert!(!group_empty_at(
            &tree.0,
            UNIT,
            Some("/system.slice/other.service")
        ));
    }
    #[test]
    fn missing_or_invalid_events_and_live_parent_refuse_cleanup() {
        let tree = Tree::new();
        let group = tree.group("", "populated 0\n");
        for events in [
            "",
            "frozen 0\n",
            "populated invalid\n",
            "populated 0\npopulated 0\n",
            "populated 0\npopulated 1\n",
            "populated 0\npopulated\t1\n",
            "populated 0\nfrozen invalid\n",
            "populated 0 extra\n",
        ] {
            fs::write(group.join("cgroup.events"), events).unwrap();
            assert!(!tree.empty(), "invalid events were accepted: {events:?}");
        }
        fs::remove_file(group.join("cgroup.events")).unwrap();
        assert!(!tree.empty());
        fs::write(group.join("cgroup.events"), "populated 0\n").unwrap();
        fs::write(group.join("cgroup.procs"), "123\n").unwrap();
        assert!(!tree.empty());
        fs::remove_file(group.join("cgroup.procs")).unwrap();
        assert!(!tree.empty());
    }
    #[test]
    fn absent_group_without_unified_v2_proof_refuses_cleanup() {
        let tree = Tree::new();
        fs::remove_file(tree.0.join("cgroup.controllers")).unwrap();
        assert!(!tree.empty());
    }
    #[test]
    fn symlinks_and_unexpected_group_paths_refuse_cleanup() {
        let tree = Tree::new();
        tree.group("", "populated 0\n");
        assert!(!group_empty_at(&tree.0, "../other.service", None));
        let group = tree.0.join("system.slice").join(UNIT);
        fs::rename(group.join("cgroup.events"), group.join("events-real")).unwrap();
        symlink("events-real", group.join("cgroup.events")).unwrap();
        assert!(!tree.empty());
        fs::remove_file(group.join("cgroup.events")).unwrap();
        fs::rename(group.join("events-real"), group.join("cgroup.events")).unwrap();
        fs::rename(&group, tree.0.join("saved")).unwrap();
        symlink("../saved", &group).unwrap();
        assert!(!tree.empty());
    }
    #[test]
    fn oversized_evidence_refuses_cleanup() {
        let tree = Tree::new();
        let group = tree.group("", &format!("populated 0\n{}", "x".repeat(4096)));
        assert!(!tree.empty());
        fs::write(group.join("cgroup.events"), "populated 0\n").unwrap();
        fs::write(group.join("cgroup.procs"), " ".repeat(65_537)).unwrap();
        assert!(!tree.empty());
    }
}
