//! The SDKs' fake lamps tell the same `hand` events as watch.rs for the same
//! `humans/hands/tracked` track (sdk/python/src/lampos/testing.py
//! `hand_events`, sdk/js/src/testing.js `handEvents`, and their tests'
//! `FIXED_TRACK` and `FIXED_EVENTS`): so a program tried on a fake lamp
//! hears what a lamp says.

use lamp_control::watch::{Out, Watch};
use serde_json::{Value, json};

/// The 250 ms check the door runs (`HAND_CHECK_US` in the fakes).
const CHECK_US: u64 = 250_000;

#[test]
fn the_fakes_hand_events_are_watch_rs_own() {
    // Across, closed from the 31st, then gone: the SDK tests' FIXED_TRACK.
    let track: Vec<(u64, Value)> = (0..40u64)
        .map(|n| {
            let hand = json!({"x": 0.1 + n as f64 * 0.02, "y": 0.5, "size": 0.125, "open": n < 30, "confidence": 0.875});
            (n * 66_667, hand)
        })
        .collect();
    let mut watch = Watch::new();
    let mut said = Vec::new();
    let mut keep = |at: u64, out: Vec<Out>| {
        for out in out {
            if let Out::Event(_, event) = out {
                said.push(json!([at, event]));
            }
        }
    };
    let mut check = CHECK_US;
    for (t, hand) in &track {
        while check < *t {
            keep(check, watch.lapse(check));
            check += CHECK_US;
        }
        keep(*t, watch.take("humans/hands/tracked", hand, *t));
    }
    for _ in 0..8 {
        keep(check, watch.lapse(check));
        check += CHECK_US;
    }
    let fixed: Value = serde_json::from_str(r#"[[0,{"seen":true,"x":0.1,"y":0.5,"size":0.13,"open":true,"confidence":0.88}],[250000,{"seen":true,"x":0.16,"y":0.5,"size":0.13,"open":true,"confidence":0.88}],[500000,{"seen":true,"x":0.24,"y":0.5,"size":0.13,"open":true,"confidence":0.88}],[750000,{"seen":true,"x":0.32,"y":0.5,"size":0.13,"open":true,"confidence":0.88}],[1000000,{"seen":true,"x":0.38,"y":0.5,"size":0.13,"open":true,"confidence":0.88}],[1250000,{"seen":true,"x":0.46,"y":0.5,"size":0.13,"open":true,"confidence":0.88}],[1500000,{"seen":true,"x":0.54,"y":0.5,"size":0.13,"open":true,"confidence":0.88}],[1750000,{"seen":true,"x":0.62,"y":0.5,"size":0.13,"open":true,"confidence":0.88}],[2000000,{"seen":true,"x":0.68,"y":0.5,"size":0.13,"open":true,"confidence":0.88}],[2250000,{"seen":true,"x":0.76,"y":0.5,"size":0.13,"open":false,"confidence":0.88}],[2500000,{"seen":true,"x":0.84,"y":0.5,"size":0.13,"open":false,"confidence":0.88}],[3750000,{"seen":false}]]"#).unwrap();
    assert_eq!(Value::Array(said), fixed);
}
