//! The spike's four cases (#6071): a normal script, an endless loop stopped
//! by its fuel or its time, a memory blow-up refused, and an unknown host
//! call refused before it runs.
use lamp_skill_code::{Call, Limits, Refused, Value, check, run};
use std::time::{Duration, Instant};

#[test]
fn a_normal_script_gives_its_plan_in_order() {
    let script = r#"
        fn greeting(n) { if n > 2 { "Welcome back, again." } else { "Welcome back." } }
        if event.someone {
            light("warm white");
            say(greeting(event.times));
        }
    "#;
    let event = [("someone", Value::Bool(true)), ("times", Value::Int(3))];
    let plan = run(script, &event, &Limits::default()).expect("runs");
    assert_eq!(
        plan.calls,
        [
            Call::Light("warm white".into()),
            Call::Say("Welcome back, again.".into())
        ]
    );
    assert!(plan.operations > 0 && plan.operations < 1_000, "{plan:?}");
    let uses = check(script, &Limits::default()).expect("checks");
    assert_eq!(uses.host.into_iter().collect::<Vec<_>>(), ["light", "say"]);
}

#[test]
fn nothing_is_made_when_the_trigger_says_no() {
    let plan = run(
        r#"if event.someone { light("cyan"); }"#,
        &[("someone", Value::Bool(false))],
        &Limits::default(),
    )
    .expect("runs");
    assert!(plan.calls.is_empty());
}

#[test]
fn an_endless_loop_runs_out_of_fuel_and_makes_nothing() {
    let began = Instant::now();
    let refused = run(r#"light("red"); loop { }"#, &[], &Limits::default()).unwrap_err();
    assert_eq!(refused, Refused::OutOfFuel);
    assert_eq!(refused.plainly(), "its code didn't finish in time");
    // 100,000 operations: well under the 50 ms wall time on the Mac.
    assert!(
        began.elapsed() < Duration::from_millis(500),
        "{:?}",
        began.elapsed()
    );
}

#[test]
fn an_endless_loop_with_fuel_to_spare_is_stopped_by_its_wall_time() {
    let limits = Limits {
        operations: u64::MAX,
        wall: Duration::from_millis(20),
        ..Limits::default()
    };
    let began = Instant::now();
    let refused = run("let i = 0; loop { i += 1; }", &[], &limits).unwrap_err();
    let took = began.elapsed();
    assert_eq!(refused, Refused::OutOfTime);
    assert!(took >= Duration::from_millis(20), "{took:?}");
    assert!(took < Duration::from_millis(500), "{took:?}");
}

#[test]
fn a_string_that_doubles_is_refused_at_its_limit() {
    let refused = run(r#"let s = "x"; loop { s += s; }"#, &[], &Limits::default()).unwrap_err();
    assert!(matches!(refused, Refused::TooMuchMemory(_)), "{refused:?}");
    assert_eq!(refused.plainly(), "its code needed too much memory");
}

#[test]
fn an_array_that_grows_is_refused_at_its_limit() {
    let refused = run("let a = [1]; loop { a += a; }", &[], &Limits::default()).unwrap_err();
    assert!(matches!(refused, Refused::TooMuchMemory(_)), "{refused:?}");
}

#[test]
fn endless_recursion_is_refused_at_its_call_depth() {
    let refused = run(
        "fn down(n) { down(n + 1) } down(0);",
        &[],
        &Limits::default(),
    )
    .unwrap_err();
    assert!(matches!(refused, Refused::TooMuchMemory(_)), "{refused:?}");
}

#[test]
fn an_unknown_host_call_is_refused_before_it_runs() {
    // `send_text` comes after a light: a run would have recorded the light
    // first, but the check refuses the script whole, so nothing runs.
    let script = r#"light("red"); send_text("I'm home");"#;
    let refused = check(script, &Limits::default()).unwrap_err();
    assert_eq!(refused, Refused::CantYet("send_text".into()));
    assert_eq!(refused.plainly(), "the lamp can't do send_text yet");
    assert_eq!(run(script, &[], &Limits::default()).unwrap_err(), refused);
    // Inside a function of the script's own, and as a method, alike.
    for script in [
        r#"fn greet() { camera_snap() } greet();"#,
        r#"let s = "a"; s.camera_snap();"#,
    ] {
        assert_eq!(
            check(script, &Limits::default()).unwrap_err(),
            Refused::CantYet("camera_snap".into()),
            "{script}"
        );
    }
}

#[test]
fn a_call_by_a_computed_name_cannot_be_written() {
    for script in [
        r#"eval("light(\"red\")");"#,
        r#"let f = Fn("li" + "ght"); f.call("red");"#,
        r#"print("hi");"#,
    ] {
        assert!(
            matches!(
                check(script, &Limits::default()),
                Err(Refused::Unreadable(_))
            ),
            "{script}"
        );
    }
}

#[test]
fn a_plan_is_held_to_its_calls_and_one_say() {
    // A range for `for` needs a library the bare engine doesn't have yet
    // (docs/skill-code.md); a `while` counts as well.
    let refused = run(
        r#"let i = 0; while i < 20 { light("red"); i += 1; }"#,
        &[],
        &Limits::default(),
    );
    assert_eq!(refused.unwrap_err(), Refused::TooManyCalls);
    assert_eq!(
        run(r#"say("one"); say("two");"#, &[], &Limits::default()).unwrap_err(),
        Refused::TooManyCalls
    );
}

#[test]
fn a_host_function_refuses_what_its_step_would() {
    assert!(matches!(
        run(r#"say("   ");"#, &[], &Limits::default()),
        Err(Refused::BadInput(_))
    ));
    assert!(matches!(
        run(r#"light("<script>");"#, &[], &Limits::default()),
        Err(Refused::BadInput(_))
    ));
}

#[test]
fn a_source_over_its_size_is_refused_unread() {
    let script = format!("// {}\nlight(\"red\");", "x".repeat(9 * 1024));
    assert!(matches!(
        check(&script, &Limits::default()),
        Err(Refused::TooBig { .. })
    ));
}
