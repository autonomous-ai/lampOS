use lamp_apps::entry::{entry_env, entry_ok};

#[test]
fn a_dotted_module_path_is_an_entry() {
    for good in ["lamp_learn.app", "app", "a.b.c", "_x.y1", "a_b.c_d"] {
        assert!(entry_ok(good), "{good}");
    }
}

#[test]
fn nothing_else_is_an_entry() {
    for bad in [
        "",
        ".",
        "a.",
        ".a",
        "a..b",
        "..",
        "../x",
        "a/b",
        "a b",
        "a\n",
        "A.b",
        "1a",
        "a.1b",
        "a-b",
        "a=b",
        "a;b",
        "a$b",
        "é",
        "a\0b",
        "a.b.c.d.e.f.g.h.i",
    ] {
        assert!(!entry_ok(bad), "{bad:?}");
    }
    assert!(!entry_ok(&"a".repeat(129)));
}

#[test]
fn entry_env_is_exactly_one_line() {
    assert_eq!(
        entry_env("lamp_learn.app").unwrap(),
        "ENTRY=lamp_learn.app\n"
    );
    assert!(entry_env("a\nENTRY=b").is_err());
    assert!(entry_env("").is_err());
}
