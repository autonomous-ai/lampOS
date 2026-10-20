//! Every name `lamp-setup` subscribes to the bus under is one the bus takes
//! (#1959): `setup-change` was refused on lamp-0c89, and the change helper
//! then could never judge quiet. The bus's own check (`lamp_bus`'s private
//! `name::subscriber`, `[a-z][a-z0-9_]{0,31}`) is reached through a real
//! subscription, so the rule is never copied here.
use lamp_bus::Bus;
use std::{fs, io, path::PathBuf};

struct Root(PathBuf);

impl Root {
    fn new(what: &str) -> Self {
        // Short: a subscriber's socket path must fit a Unix socket address.
        let path = PathBuf::from("/tmp").join(format!("lsn-{what}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn every_subscriber_name_in_the_crate_is_one_the_bus_takes() {
    let root = Root::new("ok");
    let bus = Bus::new(&root.0).unwrap();
    for name in lamp_setup::SUBSCRIBERS {
        if let Err(error) = bus.subscribe("voice/state", name) {
            panic!("the bus refuses subscriber {name:?}: {error}");
        }
    }
}

#[test]
fn the_old_change_name_is_refused_by_the_bus() {
    let root = Root::new("old");
    let bus = Bus::new(&root.0).unwrap();
    let error = bus.subscribe("voice/state", "setup-change").err().unwrap();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{error}");
    assert!(error.to_string().contains("subscriber name"), "{error}");
}
