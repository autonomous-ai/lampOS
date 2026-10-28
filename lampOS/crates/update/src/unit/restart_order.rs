//! Startup ordering of managed unit files, in either direction of an update.
//! This reads declarations, not effective systemd state or service readiness.
use std::collections::{BTreeMap, BTreeSet};

/// Put prerequisites before callers; units without an ordering relation use
/// name order. Include unchanged managed units so a path through one still
/// orders two changed units. External units are not added or started.
pub(crate) fn start_order<'a>(
    units: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<Vec<String>, String> {
    let units: BTreeMap<&str, &str> = units.into_iter().collect();
    let mut needs: BTreeMap<&str, BTreeSet<&str>> =
        units.keys().map(|name| (*name, BTreeSet::new())).collect();
    for (name, text) in &units {
        for (before, dependency) in declarations(text).map_err(|why| format!("{name}: {why}"))? {
            let Some((dependency, _)) = units.get_key_value(dependency.as_str()) else {
                continue;
            };
            let (caller, prerequisite) = if before {
                (*dependency, *name)
            } else {
                (*name, *dependency)
            };
            needs.get_mut(caller).unwrap().insert(prerequisite);
        }
    }
    let mut ordered = Vec::with_capacity(needs.len());
    while !needs.is_empty() {
        let Some(name) = needs
            .iter()
            .find_map(|(name, dependencies)| dependencies.is_empty().then_some(*name))
        else {
            return Err(format!(
                "unit startup ordering has a cycle: {}",
                needs.keys().copied().collect::<Vec<_>>().join(", ")
            ));
        };
        needs.remove(name);
        ordered.push(name.to_owned());
        for dependencies in needs.values_mut() {
            dependencies.remove(name);
        }
    }
    Ok(ordered)
}

/// Repeated ordering directives accumulate, including an empty assignment.
/// Sockets= adds After= edges; Wants=/Requires=/PartOf= do not imply order.
fn declarations(text: &str) -> Result<Vec<(bool, String)>, String> {
    let mut section = String::new();
    let mut folded = String::new();
    let mut edges = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        if let Some(line) = line.strip_suffix('\\') {
            folded.push_str(line);
            folded.push(' ');
            continue;
        }
        folded.push_str(line);
        let line = std::mem::take(&mut folded);
        let line = line.trim();
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            section = name.to_owned();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let before = match (section.as_str(), key.trim()) {
            ("Unit", "After") | ("Service", "Sockets") => false,
            ("Unit", "Before") => true,
            _ => continue,
        };
        edges.extend(words(value)?.into_iter().map(|name| (before, name)));
    }
    if !folded.is_empty() {
        return Err("unit ordering ends with an unfinished continuation".into());
    }
    Ok(edges)
}

/// Managed lamp units have literal names. Refuse quoting, escapes or
/// specifiers rather than inventing an edge or silently losing one that
/// systemd could expand to another managed unit.
fn words(value: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut word = String::new();
    for character in value.chars() {
        match character {
            '\\' | '%' | '\'' | '"' => {
                return Err(
                    "unit ordering with quoting, escapes or specifiers is not supported".into(),
                );
            }
            character if character.is_whitespace() => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            character => word.push(character),
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_managed_services_have_a_valid_startup_graph() {
        let units: Vec<_> = crate::install::PRODUCT
            .iter()
            .copied()
            .filter(|(name, text)| {
                let product = |unit: &str| {
                    crate::install::PRODUCT
                        .iter()
                        .find(|(other, _)| *other == unit)
                        .map(|(_, text)| *text)
                };
                // A part's templates, sockets and timers join its graph.
                (*name != crate::round::OWN_UNIT
                    && crate::manifest::is_unit_name(name)
                    && crate::unit::linked(text).is_some())
                    || crate::companion::part(name, text, product).is_some()
            })
            .collect();
        assert!(
            units
                .iter()
                .any(|(name, _)| *name == "lamp-setup-wifi.socket")
        );
        assert!(
            units
                .iter()
                .any(|(name, _)| *name == crate::round::VOICE_UNIT)
        );
        assert_eq!(
            start_order(units.iter().copied()).unwrap().len(),
            units.len()
        );
    }

    #[test]
    fn repeated_and_folded_declarations_keep_each_edge() {
        let units = [
            (
                "a.service",
                "[Unit]\n After = b.service \\\n# a comment between continued lines\n c.service\nAfter=b.service\nAfter=\n",
            ),
            ("b.service", "[Unit]\nBefore=a.service\n"),
            ("c.service", "[Unit]\nBefore=b.service\n"),
        ];
        assert_eq!(
            start_order(units).unwrap(),
            ["c.service", "b.service", "a.service"]
        );
    }

    #[test]
    fn requirements_and_directives_in_other_sections_do_not_order_units() {
        let units = [
            (
                "a.service",
                "[Unit]\nWants=b.service\nRequires=b.service\nPartOf=b.service\nAfter=external.target\n[Service]\nAfter=b.service\n[Install]\nBefore=b.service\n",
            ),
            (
                "b.service",
                "[Unit]\n# Before=a.service\n; Before=a.service\n",
            ),
        ];
        assert_eq!(start_order(units).unwrap(), ["a.service", "b.service"]);
    }

    #[test]
    fn a_declared_socket_precedes_its_service() {
        assert_eq!(
            start_order([
                ("a.service", "[Service]\nSockets=z.socket\nSockets=\n"),
                ("z.socket", "[Socket]\nListenStream=/run/synthetic\n"),
            ])
            .unwrap(),
            ["z.socket", "a.service"]
        );
    }

    /// lamp-52e6's reboot, 27 Oct 16:37 (#3697): Controls was ordered after
    /// the voice, so systemd stopped it first and the voice, its privacy
    /// switch gone, ended with 1 and came back muted by the restart guard.
    /// The voice now starts after Controls, so it stops before it, in the
    /// product's units and the template alike, with no cycle.
    #[test]
    fn the_voice_starts_after_controls_so_it_stops_first() {
        for (voice, controls) in [
            (
                include_str!("../../../../deploy/product/lamp-voice.service"),
                include_str!("../../../../deploy/product/lamp-controls.service"),
            ),
            (
                include_str!("../../../../deploy/units/lamp-voice.service"),
                include_str!("../../../../deploy/units/lamp-controls.service"),
            ),
        ] {
            let order = start_order([
                ("lamp-voice.service", voice),
                ("lamp-controls.service", controls),
            ])
            .unwrap();
            assert_eq!(order, ["lamp-controls.service", "lamp-voice.service"]);
            assert!(
                voice.lines().any(
                    |line| line.starts_with("Wants=") && line.contains("lamp-controls.service")
                ),
                "{voice}"
            );
        }
    }

    #[test]
    fn a_path_through_an_unchanged_unit_still_orders_its_ends() {
        let order = start_order([
            ("a.service", "[Unit]\nAfter=b.service\n"),
            ("b.service", "[Unit]\nAfter=z.service\n"),
            ("z.service", ""),
        ])
        .unwrap();
        let changed: Vec<_> = order
            .iter()
            .filter(|name| name.as_str() != "b.service")
            .collect();
        assert_eq!(changed, [&"z.service".to_owned(), &"a.service".to_owned()]);
    }

    #[test]
    fn cycles_including_self_edges_are_refused() {
        for units in [
            vec![("a.service", "[Unit]\nAfter=a.service\n")],
            vec![
                ("a.service", "[Unit]\nAfter=b.service\n"),
                ("b.service", "[Unit]\nAfter=a.service\n"),
            ],
        ] {
            assert!(start_order(units).unwrap_err().contains("cycle"));
        }
    }

    #[test]
    fn unsupported_ordering_cannot_silently_lose_a_dependency() {
        for declaration in [
            "After=%n",
            "After=lamp-\\x6demory.service",
            "After='b.service",
            "After='b.service'",
            "After=b.service \\",
        ] {
            assert!(
                start_order([("a.service", format!("[Unit]\n{declaration}\n").as_str())]).is_err()
            );
        }
    }
}
