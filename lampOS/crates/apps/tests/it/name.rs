use lamp_apps::name::{app_name_ok, net_unit, plain_unit, user_name};

#[test]
fn names_follow_nines_rule() {
    for good in [
        "a",
        "english-lessons",
        "x9",
        "a-",
        "abcdefghijklmnopqrstuvwx",
    ] {
        assert!(app_name_ok(good), "{good}");
    }
    for bad in [
        "",
        "9a",
        "-a",
        "A",
        "english_lessons",
        "english.lessons",
        "a b",
        "a/b",
        "a\n",
        "..",
        "a=b",
        "abcdefghijklmnopqrstuvwxy",
        "é",
        "a\0",
    ] {
        assert!(!app_name_ok(bad), "{bad:?}");
    }
}

#[test]
fn the_longest_name_still_makes_a_user_of_32() {
    let longest = "a".repeat(24);
    assert_eq!(user_name(&longest).len(), 32);
}

#[test]
fn the_two_unit_variants_are_named_by_the_instance() {
    assert_eq!(plain_unit("x"), "lampapp@x.service");
    assert_eq!(net_unit("x"), "lampapp-net@x.service");
}
