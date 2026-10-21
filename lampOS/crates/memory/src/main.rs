//! The lamp's memory: one process that keeps, on the lamp's own disk, each
//! exchange the lamp took part in and what it was asked to keep, and answers
//! on a local socket when the voice asks for something to be remembered,
//! recalled or forgotten. Its own process so that the lamp talks on, without
//! recalling earlier days, when this one has stopped.
//!
//! It runs until it is killed. The standard library cannot catch a signal
//! and this binary registers no signal handler, so SIGTERM ends it where it
//! stands. Nothing remembered is lost by that: a line is on the disk before
//! its request is answered. The socket file and `ready` are left behind, and
//! the next process clears them as it starts.

include!("application.rs");

fn main() -> ExitCode {
    entrypoint(Reporter::start)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(args: &[&str]) -> Result<Option<Arguments>, &'static str> {
        parse(args.iter().map(OsString::from))
    }

    #[test]
    fn the_memory_days_start_line_names_the_zone_only_when_one_is_given() {
        let start = |extra: &[&str]| {
            let mut args = vec!["--dir", "/m", "--socket", "/s", "--memory-days"];
            args.extend_from_slice(extra);
            days_start_line(&arguments(&args).unwrap().unwrap())
        };
        assert_eq!(
            start(&["--zone", "system"]),
            "started memory_days=true zone=system; the model adapter is not configured"
        );
        assert_eq!(
            start(&["--utc-offset-minutes", "420"]),
            "started memory_days=true; the model adapter is not configured"
        );
        assert_eq!(
            start(&[]),
            "started memory_days=true; the model adapter is not configured"
        );
    }

    #[test]
    fn the_two_paths_are_required_and_the_days_and_the_offset_have_defaults() {
        assert_eq!(
            arguments(&[
                "--dir",
                "/var/lib/lamp/memory",
                "--socket",
                "/run/lamp/memory.sock"
            ]),
            Ok(Some(Arguments {
                dir: "/var/lib/lamp/memory".into(),
                socket: "/run/lamp/memory.sock".into(),
                keep_days: 30,
                utc_offset_minutes: 0,
                zone: None,
                memory_days: false,
                bus_root: None,
                forget: None,
            }))
        );
        assert_eq!(
            arguments(&[
                "--utc-offset-minutes",
                "420",
                "--socket",
                "/run/m.sock",
                "--keep-days",
                "7",
                "--dir",
                "/m",
            ]),
            Ok(Some(Arguments {
                dir: "/m".into(),
                socket: "/run/m.sock".into(),
                keep_days: 7,
                utc_offset_minutes: 420,
                zone: None,
                memory_days: false,
                bus_root: None,
                forget: None,
            }))
        );
        let west = arguments(&[
            "--dir",
            "/m",
            "--socket",
            "/s",
            "--utc-offset-minutes",
            "-300",
        ]);
        assert_eq!(west.unwrap().unwrap().utc_offset_minutes, -300);
        // #820: a zone in place of the offset.
        let zoned = arguments(&["--dir", "/m", "--socket", "/s", "--zone", "system"]);
        let zoned = zoned.unwrap().unwrap();
        assert_eq!(zoned.zone.as_deref(), Some("system"));
        if std::path::Path::new("/etc/localtime").exists() {
            assert!(matches!(local(&zoned), Ok(LocalTime::System(_))));
        }
        let named = arguments(&[
            "--dir",
            "/m",
            "--socket",
            "/s",
            "--zone",
            "America/New_York",
        ]);
        assert!(matches!(
            local(&named.unwrap().unwrap()),
            Ok(LocalTime::Zone(_))
        ));
        let nowhere = arguments(&["--dir", "/m", "--socket", "/s", "--zone", "Nowhere/City"]);
        assert!(local(&nowhere.unwrap().unwrap()).is_err());
        assert!(matches!(
            local(
                &arguments(&["--dir", "/m", "--socket", "/s"])
                    .unwrap()
                    .unwrap()
            ),
            Ok(LocalTime::Offset(0))
        ));
        assert_eq!(arguments(&["--help"]), Ok(None));
        assert!(
            arguments(&["--dir", "/m", "--socket", "/s", "--memory-days"])
                .unwrap()
                .unwrap()
                .memory_days
        );
        assert!(
            arguments(&[
                "--dir",
                "/m",
                "--socket",
                "/s",
                "--memory-days",
                "--memory-days"
            ])
            .is_err()
        );
        assert!(arguments(&["--memory-days", "--help"]).is_err());
    }

    #[test]
    fn health_reporting_requires_one_explicit_absolute_bus_root() {
        let base = ["--dir", "/m", "--socket", "/s"];
        let with = |more: &[&str]| arguments(&[&base[..], more].concat());
        assert_eq!(with(&[]).unwrap().unwrap().bus_root, None);
        assert_eq!(
            with(&["--bus-root", "/b"]).unwrap().unwrap().bus_root,
            Some("/b".into())
        );
        for bad in [
            &["--bus-root"][..],
            &["--bus-root", "relative"],
            &["--bus-root", ""],
            &["--bus-root", "/b", "--bus-root", "/b"],
            &["--bus-root", "/b", "--help"],
        ] {
            assert!(with(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_forget_socket_is_off_unless_set_and_needs_its_group_and_an_absolute_path() {
        let base = ["--dir", "/m", "--socket", "/s"];
        let with = |more: &[&str]| arguments(&[&base[..], more].concat());
        assert_eq!(with(&[]).unwrap().unwrap().forget, None);
        assert_eq!(
            with(&[
                "--forget-socket",
                "/f/forget.sock",
                "--forget-group",
                "lamp-app"
            ])
            .unwrap()
            .unwrap()
            .forget,
            Some(("/f/forget.sock".into(), "lamp-app".into()))
        );
        for bad in [
            &["--forget-socket", "/f/forget.sock"][..],
            &["--forget-group", "lamp-app"],
            &[
                "--forget-socket",
                "forget.sock",
                "--forget-group",
                "lamp-app",
            ],
            &["--forget-socket", "/", "--forget-group", "lamp-app"],
            &["--forget-socket", "/s", "--forget-group", "lamp-app"],
            &["--forget-socket", "/f/a", "--forget-group", ""],
            &["--forget-socket", "/f/a", "--forget-group", "a:b"],
            &[
                "--forget-socket",
                "/f/a",
                "--forget-socket",
                "/f/a",
                "--forget-group",
                "g",
            ],
            &[
                "--forget-socket",
                "/f/a",
                "--forget-group",
                "g",
                "--forget-group",
                "g",
            ],
            &["--forget-socket"],
            &["--forget-group", "g", "--help"],
        ] {
            assert!(with(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_relative_path_an_unknown_option_or_a_bad_number_is_refused() {
        let paths = ["--dir", "/m", "--socket", "/s"];
        let with = |more: &[&str]| arguments(&[&paths[..], more].concat());
        assert!(with(&[]).is_ok());
        for bad in [
            &["--dir", "memory", "--socket", "/s"][..],
            &["--dir", "/m", "--socket", "m.sock"],
            &["--dir", "./m", "--socket", "/s"],
            &["--dir", "", "--socket", "/s"],
            &["--dir", "/m", "--socket", "/"],
            &["--dir", "/m"],
            &["--socket", "/s"],
            &[],
            &["--dir"],
            &["--dir", "/m", "--socket"],
            &["/m", "/s"],
            &["--help", "--dir", "/m"],
            &["--dir", "/m", "--help"],
        ] {
            assert!(arguments(bad).is_err(), "{bad:?}");
        }
        for bad in [
            &["--verbose"][..],
            &["--verbose", "yes"],
            &["-d", "/x"],
            &["--dir", "/again"],
            &["--socket", "/again"],
            &["--keep-days", "0"],
            &["--keep-days", "-3"],
            &["--keep-days", "thirty"],
            &["--keep-days", "3.5"],
            &["--keep-days", ""],
            &["--keep-days", "4294967296"],
            &["--keep-days", "3", "--keep-days", "4"],
            &["--utc-offset-minutes", "841"],
            &["--utc-offset-minutes", "-721"],
            &["--utc-offset-minutes", "+7h"],
            &["--utc-offset-minutes", "420", "--utc-offset-minutes", "420"],
            &["--zone"],
            &["--zone", ""],
            &["--zone", "system", "--zone", "system"],
            &["--zone", "system", "--utc-offset-minutes", "420"],
            &["--utc-offset-minutes", "0", "--zone", "UTC"],
            &["extra"],
        ] {
            assert!(with(bad).is_err(), "{bad:?}");
        }
        assert_eq!(with(&["--verbose"]), Err("unknown option"));
        assert!(with(&["--keep-days", "1", "--utc-offset-minutes", "840"]).is_ok());
        assert!(with(&["--keep-days", "4294967295", "--utc-offset-minutes", "-720"]).is_ok());
    }
}

#[cfg(test)]
#[path = "application_factory_tests.rs"]
mod factory_tests;
