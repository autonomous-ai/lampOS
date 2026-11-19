//! A saved turn's id on the account's history (#7771): one helper for the
//! voice's save (`LAMP_HISTORY_SEND`) and its forget (`LAMP_HISTORY_FORGET`),
//! so a forget names exactly the ids the save sent (the first builder's
//! 17:09 ruling on #7957).
//!
//! `<store>:<scope>:<exchange id>`: the Days memory store's own id (8
//! lowercase hex digits, made when the store is made and gone with it at a
//! reset, lamp-memory's `store` op), the memory scope (a known person's `p-`
//! id, or [`ROOM_SCOPE`]) and the exchange's id in that scope. A reset
//! lamp's new turns are then neither taken as already saved nor refused by
//! an old tombstone. At most 8 + 1 + 32 + 1 + 20 = 62 characters, within
//! the store's 64.

/// The room's scope in lamp-memory; its person key on the server is `""`.
pub const ROOM_SCOPE: &str = "everyone";

/// The id of exchange `exchange` in memory scope `scope` of store `store`.
pub fn turn_id(store: &str, scope: &str, exchange: u64) -> String {
    format!("{store}:{scope}:{exchange}")
}

/// Whether `store` is a store id as lamp-memory makes one: 8 lowercase hex
/// digits.
pub fn is_store_id(store: &str) -> bool {
    store.len() == 8
        && store
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turns_id_carries_the_store_the_scope_and_the_exchange() {
        assert_eq!(turn_id("0a1b2c3d", ROOM_SCOPE, 7), "0a1b2c3d:everyone:7");
        assert_eq!(
            turn_id("0a1b2c3d", "p-0123456789abcdef", 12),
            "0a1b2c3d:p-0123456789abcdef:12"
        );
        let longest = turn_id("ffffffff", &"a".repeat(32), u64::MAX);
        assert!(longest.len() <= 64, "{}", longest.len());
        assert!(is_store_id("0a1b2c3d"));
        for bad in ["", "0a1b2c3", "0a1b2c3d4", "0A1B2C3D", "0a1b2c3g"] {
            assert!(!is_store_id(bad), "{bad}");
        }
    }
}
