//! `lamp-cloud` reads setup's account where the product's setup writes it,
//! and in the form setup writes it: the two had drifted to different paths
//! once (Fourth on #1404), and a lamp set up with the app would never have
//! come online.
use lamp_cloud::account::{self, Account};
use lamp_setup::serve::{Config, Files, Save, SetupRequest, backend_link};
use std::path::PathBuf;

const UNIT: &str = include_str!("../../../deploy/product/lamp-cloud.service");

/// The product's setup settings, as `lamp-update`'s install writes them.
fn product_setup() -> Config {
    serde_json::from_str(include_str!("../../setup/fixtures/serve-own-page.json")).unwrap()
}

#[test]
fn the_unit_reads_the_account_where_the_products_setup_writes_it() {
    let setup = product_setup();
    let backend = setup.backend.display().to_string();
    assert!(
        UNIT.lines()
            .any(|line| line == format!("LoadCredential=account:{backend}")),
        "the unit's account is not setup's {backend}"
    );
    assert!(
        UNIT.lines()
            .any(|line| line == format!("ConditionPathExists={backend}")),
        "the unit does not wait for setup's {backend}"
    );
    let marker = setup.marker.display().to_string();
    assert!(
        UNIT.lines()
            .any(|line| line == format!("ConditionPathExists={marker}")),
        "the unit does not wait for setup's marker {marker}"
    );
    // The reset removes it with the rest of the person's data.
    let plan: serde_json::Value =
        serde_json::from_str(include_str!("../../setup/fixtures/reset-plan-product.json")).unwrap();
    assert!(
        plan.to_string().contains(&format!("\"{backend}\"")),
        "{plan}"
    );
}

#[test]
fn what_setup_writes_from_the_apps_request_reads_as_a_complete_account() {
    let dir = PathBuf::from(format!("/tmp/lcs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut setup = product_setup();
    setup.backend = dir.join("backend.json");
    // The V1 app's request, the port as the number it sends.
    let request: SetupRequest = serde_json::from_value(serde_json::json!({
        "ssid": "Home", "password": "password1",
        "telegram_bot_token": "none", "telegram_user_id": "none",
        "device_id": "dev-42", "llm_base_url": "https://gateway.example/v1",
        "llm_api_key": "AIza-key_1", "llm_model": "auto",
        "mqtt_endpoint": "broker.example", "mqtt_port": 1883,
        "mqtt_username": "lamp-dev-42", "mqtt_password": "s3cret pass",
        "fa_channel": "fa/dev-42", "fd_channel": "fd/dev-42"
    }))
    .unwrap();
    let link = backend_link(&request).unwrap();
    Files(&setup).backend(&link).unwrap();
    let read = account::kept(&setup.backend).expect("setup's file reads as an account");
    assert_eq!(
        read,
        Account {
            device_id: "dev-42".into(),
            mqtt_endpoint: "broker.example".into(),
            mqtt_port: 1883,
            mqtt_username: "lamp-dev-42".into(),
            mqtt_password: "s3cret pass".into(),
            fa_channel: "fa/dev-42".into(),
            fd_channel: "fd/dev-42".into(),
            llm_base_url: "https://gateway.example/v1".into(),
        }
    );
    assert!(read.complete());
    assert_eq!(read.client_id(), "os-server-dev-42");
    let _ = std::fs::remove_dir_all(dir);
}
