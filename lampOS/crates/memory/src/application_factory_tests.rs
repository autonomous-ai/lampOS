//! The factory is selected only at the original optional-reporting boundary.
//! Real files make startup return before any forever-serving loop is entered.
use super::*;
use std::{
    cell::Cell,
    fs,
    os::unix::fs::{DirBuilderExt, FileTypeExt},
    sync::atomic::{AtomicU64, Ordering},
};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = PathBuf::from(format!(
            "/tmp/maf-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }

    fn arguments(&self, days: bool, bus: bool) -> Arguments {
        Arguments {
            dir: self.0.join("store"),
            socket: self.0.join("m.sock"),
            keep_days: 30,
            utc_offset_minutes: 0,
            zone: None,
            memory_days: days,
            bus_root: bus.then(|| self.0.join("b")),
            forget: None,
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn no_bus_root_never_invokes_the_factory_for_either_backend() {
    for days in [false, true] {
        let scratch = Scratch::new();
        let arguments = scratch.arguments(days, false);
        fs::write(&arguments.dir, b"not a store directory").unwrap();
        let called = Cell::new(0);
        let result = run(&arguments, |_, _| {
            called.set(called.get() + 1);
            Err(io::Error::other("factory must not be selected"))
        });
        assert!(result.is_err());
        assert_eq!(called.get(), 0);
        assert_eq!(fs::read(&arguments.dir).unwrap(), b"not a store directory");
        assert!(!scratch.0.join("b").exists());
        assert!(!scratch.0.join("ready").exists());
        assert!(!arguments.socket.exists());
    }
}

#[test]
fn a_socket_bind_failure_precedes_the_factory_and_store_for_either_backend() {
    for days in [false, true] {
        let scratch = Scratch::new();
        let arguments = scratch.arguments(days, true);
        fs::write(&arguments.socket, b"not a socket").unwrap();
        let ready = scratch.0.join("ready");
        fs::write(&ready, b"old readiness").unwrap();
        let called = Cell::new(0);
        let result = run(&arguments, |_, _| {
            called.set(called.get() + 1);
            Err(io::Error::other("factory must follow socket ownership"))
        });
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(called.get(), 0);
        assert_eq!(fs::read(&arguments.socket).unwrap(), b"not a socket");
        assert_eq!(fs::read(&ready).unwrap(), b"old readiness");
        assert!(!arguments.dir.exists());
        assert!(!scratch.0.join("b").exists());
    }
}

#[test]
fn the_selected_factory_gets_the_original_root_and_source_once_after_socket_bind() {
    for days in [false, true] {
        let scratch = Scratch::new();
        let arguments = scratch.arguments(days, true);
        fs::write(&arguments.dir, b"not a store directory").unwrap();
        let ready = scratch.0.join("ready");
        fs::write(&ready, b"old readiness").unwrap();
        let called = Cell::new(0);
        let once = String::from("owned factory input");
        let result = run(&arguments, |root, source| {
            // Moving this input proves the shared body accepts FnOnce.
            drop(once);
            called.set(called.get() + 1);
            assert_eq!(root, scratch.0.join("b"));
            match source {
                CountSource::Days => assert!(days),
                CountSource::Legacy(dir) => {
                    assert!(!days);
                    assert_eq!(dir, arguments.dir);
                }
            }
            assert!(
                fs::symlink_metadata(&arguments.socket)
                    .unwrap()
                    .file_type()
                    .is_socket()
            );
            assert!(!ready.exists());
            Err(io::Error::other("selected factory declined to start"))
        });
        let error = result.unwrap_err();
        assert_ne!(error.to_string(), "selected factory declined to start");
        assert_eq!(called.get(), 1);
        assert_eq!(fs::read(&arguments.dir).unwrap(), b"not a store directory");
        assert!(!scratch.0.join("b").exists());
        assert!(!ready.exists());
        assert!(!arguments.socket.exists());
    }
}
