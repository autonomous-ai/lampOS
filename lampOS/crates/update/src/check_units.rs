//! `lamp-update check-units`: does a lamp run exactly the product's units?
//! (#806's B8, the first builder on #705, 17 Oct 16:24: "that's how we know
//! the owner is testing what customers get".)
//!
//! In: what `systemctl cat 'lamp-*.service' 'lamp-*.timer' 'lampos*'` printed
//! on the lamp, saved as a file, and the product's units: this program's own
//! (`install::PRODUCT`, as `deploy/product/` was at the commit it was built
//! from) or a `deploy/product/` folder given with `--product`.
//!
//! Install writes each product unit whole, byte for byte, into
//! `/etc/systemd/system/` ([`install::UNITS`]): no unit has an `@…@`
//! placeholder (`install::tests::no_product_unit_names_a_lamp`), so there is
//! nothing to expand and a unit is compared as it is.
//!
//! Out, for each product unit: `same`, `missing` (the lamp printed none), or
//! each line that differs, and, for one that leaves a switch off that the
//! product's sets `on` (an older build's base unit, #3733), that switch by
//! name. Then every drop-in (a `.d/` file: on a product
//! lamp each one is a finding, and one that turns a switch its unit sets
//! `on` off says so, as install does: crate::switches), every unit the lamp
//! printed that the product has no file for, and every product unit read
//! from somewhere other than
//! `/etc/systemd/system/`. All the same and nothing else is a pass.
//!
//! A unit of the image's own that updates carry (`manifest::IMAGE_FILES`,
//! #1369), such as `lamp-cmedia-speaker.service`, is not "not in the
//! product": it is the image's, compared with the variant's own when one is
//! given (`--image robots/lamp --variant NAME`), and said only otherwise.
use crate::{install, manifest};
use std::{collections::BTreeMap, fs, path::Path};

/// One file `systemctl cat` printed: its path and its text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Printed {
    pub path: String,
    pub text: String,
}

/// The suffixes of what `systemctl cat` prints: units' and drop-ins'.
const SUFFIXES: [&str; 11] = [
    ".service",
    ".timer",
    ".target",
    ".slice",
    ".path",
    ".socket",
    ".mount",
    ".automount",
    ".swap",
    ".scope",
    ".conf",
];

/// A line that may be one `systemctl cat` puts before each file: `# ` and
/// the file's absolute path, nothing after it. A unit's own comment that
/// begins with a path (`# /etc/lampOS/body/lamp.env as …`) has words after
/// it, so it is not one.
fn header(line: &str) -> Option<&str> {
    let path = line.strip_prefix("# ")?;
    (path.starts_with('/')
        && !path.contains(char::is_whitespace)
        && SUFFIXES.iter().any(|suffix| path.ends_with(suffix)))
    .then_some(path)
}

/// What `systemctl cat` printed: its files, in order, and its notices.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Parsed {
    pub files: Vec<Printed>,
    /// `# Unit NAME is masked.`, and `No files found for NAME.` when its
    /// error was saved too: the unit's name, and whether it was masked.
    pub notices: Vec<(String, bool)>,
}

/// The files in `systemctl cat`'s output, in order, and its notices.
///
/// A header is taken only where `systemctl` puts one: on the first line or
/// after an empty line, and only for a path not printed before (systemctl
/// prints each file once). So a unit whose own text holds a line `# PATH`,
/// even its own path, as lamp-4ace's `lamp-skills.service` begins, keeps that
/// line as text, and is compared whole (the reviewer on #1346). What comes
/// before the first header is not a file. `systemctl` puts an empty line
/// between files, so each file's trailing empty lines are dropped, here and
/// in what it is compared with.
pub fn parse(output: &str) -> Parsed {
    let mut parsed = Parsed::default();
    let mut after_empty = true;
    for line in output.lines() {
        if let Some(name) = line
            .strip_prefix("# Unit ")
            .and_then(|rest| rest.strip_suffix(" is masked."))
        {
            parsed.notices.push((name.to_owned(), true));
        } else if let Some(name) = line
            .strip_prefix("No files found for ")
            .and_then(|rest| rest.strip_suffix('.'))
        {
            parsed.notices.push((name.to_owned(), false));
        } else if let Some(path) = header(line)
            .filter(|path| after_empty && !parsed.files.iter().any(|file| file.path == *path))
        {
            parsed.files.push(Printed {
                path: path.to_owned(),
                text: String::new(),
            });
        } else if let Some(file) = parsed.files.last_mut() {
            file.text.push_str(line);
            file.text.push('\n');
        }
        after_empty = line.trim().is_empty();
    }
    for file in &mut parsed.files {
        file.text = trimmed(&file.text);
    }
    parsed
}

/// `text` without its trailing empty lines, ending in one newline.
fn trimmed(text: &str) -> String {
    let mut lines: Vec<&str> = text.lines().collect();
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    lines.iter().map(|line| format!("{line}\n")).collect()
}

/// The product's units, by name: this program's own.
pub fn embedded() -> BTreeMap<String, String> {
    install::PRODUCT
        .iter()
        .filter(|(name, _)| !name.starts_with('/'))
        .map(|(name, text)| ((*name).to_owned(), (*text).to_owned()))
        .collect()
}

/// The product's units, by name, from a `deploy/product/` folder: every file
/// with a unit's suffix (not `.conf`, which is tmpfiles' there).
pub fn from_folder(folder: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut units = BTreeMap::new();
    let entries = fs::read_dir(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("{}: {error}", folder.display()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".conf") && SUFFIXES.iter().any(|suffix| name.ends_with(suffix)) {
            let text = fs::read_to_string(entry.path())
                .map_err(|error| format!("{}: {error}", entry.path().display()))?;
            units.insert(name, text);
        }
    }
    if units.is_empty() {
        return Err(format!("{} holds no unit", folder.display()));
    }
    Ok(units)
}

/// One product unit's verdict.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Same,
    Missing,
    /// The lines that differ: `-` the product's, `+` the lamp's.
    Differs(Vec<String>),
}

/// What the check found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub units: BTreeMap<String, Verdict>,
    /// Drop-ins, with their text.
    pub drop_ins: Vec<Printed>,
    /// For a drop-in of a product unit that turns one of its switches off
    /// (crate::switches), or a product unit whose text differs and does not
    /// set one on: its path, and how.
    pub switches_off: Vec<(String, String)>,
    /// Units the lamp printed that the product has no file for.
    pub extra: Vec<String>,
    /// Product units the lamp read from another place than install's.
    pub elsewhere: Vec<String>,
    /// Units systemd says are masked.
    pub masked: Vec<String>,
    /// The image's own units the lamp printed: compared with the
    /// variant's, or none when no image was given.
    pub image: BTreeMap<String, Option<Verdict>>,
}

impl Report {
    /// Everything the same, and nothing else found.
    pub fn passes(&self) -> bool {
        self.units.values().all(|verdict| *verdict == Verdict::Same)
            && self.drop_ins.is_empty()
            && self.extra.is_empty()
            && self.elsewhere.is_empty()
            && self.masked.is_empty()
            && self
                .image
                .values()
                .all(|verdict| matches!(verdict, None | Some(Verdict::Same)))
    }

    /// The report as printed, one line a finding.
    pub fn said(&self) -> String {
        let mut out = String::new();
        for (name, verdict) in &self.units {
            match verdict {
                Verdict::Same => out.push_str(&format!("{name}: same\n")),
                Verdict::Missing => out.push_str(&format!("{name}: missing on the lamp\n")),
                Verdict::Differs(lines) => {
                    out.push_str(&format!("{name}: differs\n"));
                    for line in lines {
                        out.push_str(&format!("  {line}\n"));
                    }
                    let base =
                        |path: &&String| name_of(path) == name.as_str() && !path.contains(".d/");
                    for (path, how) in self.switches_off.iter().filter(|(p, _)| base(&p)) {
                        out.push_str(&format!(
                            "{}\n",
                            crate::switches::said(&format!("unit {path}"), how)
                        ));
                    }
                }
            }
        }
        for drop_in in &self.drop_ins {
            out.push_str(&format!("drop-in: {}\n", drop_in.path));
            for line in drop_in.text.lines() {
                out.push_str(&format!("  {line}\n"));
            }
            for (_, how) in self.switches_off.iter().filter(|(p, _)| *p == drop_in.path) {
                out.push_str(&format!(
                    "{}\n",
                    crate::switches::said(&format!("drop-in {}", drop_in.path), how)
                ));
            }
        }
        for (name, verdict) in &self.image {
            match verdict {
                None => out.push_str(&format!(
                    "the image's: {name} (carried by updates; not compared without --image)\n"
                )),
                Some(Verdict::Same) => out.push_str(&format!("the image's: {name}: same\n")),
                Some(Verdict::Missing) => {}
                Some(Verdict::Differs(lines)) => {
                    out.push_str(&format!("the image's: {name}: differs\n"));
                    for line in lines {
                        out.push_str(&format!("  {line}\n"));
                    }
                }
            }
        }
        for name in &self.extra {
            out.push_str(&format!("not in the product: {name}\n"));
        }
        for path in &self.elsewhere {
            out.push_str(&format!("not where install puts it: {path}\n"));
        }
        for name in &self.masked {
            out.push_str(&format!("masked: {name}\n"));
        }
        out.push_str(if self.passes() {
            "the lamp runs exactly the product's units\n"
        } else {
            "the lamp does not run exactly the product's units\n"
        });
        out
    }
}

/// The lines that differ between `product` and `lamp`, in order: a line
/// only the product has as `- line`, one only the lamp has as `+ line`.
fn differing(product: &str, lamp: &str) -> Vec<String> {
    let a: Vec<&str> = product.lines().collect();
    let b: Vec<&str> = lamp.lines().collect();
    // The longest common subsequence, by table: units are a few hundred
    // lines at most.
    let mut table = vec![vec![0u32; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            table[i][j] = if a[i] == b[j] {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }
    let (mut i, mut j, mut lines) = (0, 0, Vec::new());
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            i += 1;
            j += 1;
        } else if i < a.len() && (j == b.len() || table[i + 1][j] >= table[i][j + 1]) {
            lines.push(format!("- {}", shown(a[i])));
            i += 1;
        } else {
            lines.push(format!("+ {}", shown(b[j])));
            j += 1;
        }
    }
    lines
}

/// A line as said: one that ends in white space says so, or a change of
/// trailing spaces alone would print two lines that look the same.
fn shown(line: &str) -> String {
    if line.len() == line.trim_end().len() {
        line.to_owned()
    } else {
        format!("{line}(ends in white space)")
    }
}

/// The file name of `path`.
fn name_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The product's unit `name`, or for an instance (`lamp-x@1.service`) its
/// template's (`lamp-x@.service`).
fn product_unit<'a>(product: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
    if let Some(text) = product.get(name) {
        return Some(text);
    }
    let (head, rest) = name.split_once('@')?;
    let suffix = rest.rsplit_once('.')?.1;
    product
        .get(&format!("{head}@.{suffix}"))
        .map(String::as_str)
}

/// The image's own units a build carries, by name.
pub fn image_unit_names() -> Vec<&'static str> {
    manifest::IMAGE_FILES
        .iter()
        .filter_map(|path| path.strip_prefix(install::UNITS))
        .filter(|name| !name.contains('/'))
        .collect()
}

/// The image's own units for `variant`, by name, from its files.
pub fn image_units(image: &[manifest::ImageFile], variant: &str) -> BTreeMap<String, String> {
    image
        .iter()
        .filter(|file| file.variant == variant)
        .filter_map(|file| {
            let name = file.path.strip_prefix(install::UNITS)?;
            (!name.contains('/')).then(|| (name.to_owned(), file.text.clone()))
        })
        .collect()
}

/// Compares what the lamp printed with the product's units.
pub fn check(product: &BTreeMap<String, String>, parsed: &Parsed) -> Report {
    check_with_image(product, None, parsed)
}

/// [`check`], with the image's own units of the lamp's variant compared too.
pub fn check_with_image(
    product: &BTreeMap<String, String>,
    image: Option<&BTreeMap<String, String>>,
    parsed: &Parsed,
) -> Report {
    // A masked unit is a finding; "no files found" for a unit the product
    // has is already `missing`, and for one it hasn't (the implicit
    // `lampos.slice` that `'lampos*'` matches) is nothing.
    let mut report = Report {
        masked: parsed
            .notices
            .iter()
            .filter(|(_, masked)| *masked)
            .map(|(name, _)| name.clone())
            .collect(),
        ..Report::default()
    };
    let mut seen: BTreeMap<&str, &Printed> = BTreeMap::new();
    // Each product unit's drop-ins in the order printed (systemd's), read
    // together: a switch a later drop-in sets back `on` is on.
    let mut drop_ins_of: BTreeMap<&str, Vec<(String, crate::switches::Assignments)>> =
        BTreeMap::new();
    for file in &parsed.files {
        let parent = file.path.rsplit('/').nth(1).unwrap_or_default();
        if let Some(unit) = parent.strip_suffix(".d") {
            if product_unit(product, unit).is_some() {
                drop_ins_of
                    .entry(unit)
                    .or_default()
                    .push((file.path.clone(), crate::switches::environment(&file.text)));
            }
            report.drop_ins.push(file.clone());
            continue;
        }
        let name = name_of(&file.path);
        if !product.contains_key(name) && image_unit_names().contains(&name) {
            let verdict = image.map(|image| match image.get(name) {
                None => Verdict::Differs(vec![format!("+ (the variant's image has no {name})")]),
                Some(text) => {
                    let lines = differing(&trimmed(text), &file.text);
                    if lines.is_empty() {
                        Verdict::Same
                    } else {
                        Verdict::Differs(lines)
                    }
                }
            });
            report.image.insert(name.to_owned(), verdict);
            continue;
        }
        if !product.contains_key(name) {
            report.extra.push(name.to_owned());
            continue;
        }
        if file.path != format!("{}{name}", install::UNITS) {
            report.elsewhere.push(file.path.clone());
        }
        seen.insert(name, file);
    }
    let mut stale: BTreeMap<&str, &Printed> = BTreeMap::new();
    for (name, text) in product {
        let verdict = match seen.get(name.as_str()) {
            None => Verdict::Missing,
            Some(file) => {
                let lines = differing(&trimmed(text), &file.text);
                if lines.is_empty() {
                    Verdict::Same
                } else {
                    // An older build's base unit (lamp-c0a4's
                    // lamp-cloud.service of 19 Oct, #3733) is read with its
                    // drop-ins below, for a switch it leaves off.
                    stale.insert(name.as_str(), *file);
                    Verdict::Differs(lines)
                }
            }
        };
        report.units.insert(name.clone(), verdict);
    }
    // Each product unit's switches, from its stale base unit when the lamp's
    // differs, then its drop-ins: a switch is said only if it ends off.
    for (unit, drop_ins) in drop_ins_of {
        let Some(text) = product_unit(product, unit) else {
            continue;
        };
        let switches = crate::switches::switches(text);
        let base = stale
            .remove(unit)
            .map(|file| (file.path.as_str(), crate::switches::environment(&file.text)));
        report.switches_off.extend(crate::switches::merged_off_from(
            &switches,
            base.as_ref().map(|(path, found)| (*path, found)),
            &drop_ins,
        ));
    }
    for (name, file) in stale {
        let switches = crate::switches::switches(&product[name]);
        let found = crate::switches::environment(&file.text);
        report.switches_off.extend(crate::switches::merged_off_from(
            &switches,
            Some((file.path.as_str(), &found)),
            &[],
        ));
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn product() -> BTreeMap<String, String> {
        BTreeMap::from([
            (
                "lamp-a.service".to_owned(),
                "[Unit]\nDescription=a\n\n# /etc/lampOS/a.env is read by lamp-a.service\n[Service]\nExecStart=/opt/a\n"
                    .to_owned(),
            ),
            (
                "lamp-b.service".to_owned(),
                "[Service]\nEnvironment=LAMP_X=on\nExecStart=/opt/b\n".to_owned(),
            ),
            ("lamp-c.timer".to_owned(), "[Timer]\nOnBootSec=5min\n".to_owned()),
        ])
    }

    /// What `systemctl cat` prints for a lamp with one unit the same (a), one
    /// whose `Environment=` line was changed (b), its drop-in, and a unit the
    /// product doesn't have; c is missing.
    const PRINTED: &str = "\
# /etc/systemd/system/lamp-a.service
[Unit]
Description=a

# /etc/lampOS/a.env is read by lamp-a.service
[Service]
ExecStart=/opt/a

# /etc/systemd/system/lamp-b.service
[Service]
Environment=LAMP_X=off
ExecStart=/opt/b

# /etc/systemd/system/lamp-b.service.d/office.conf
[Service]
Environment=LAMP_OFFICE=on

# /etc/systemd/system/lamp-extra.service
[Service]
ExecStart=/opt/extra
";

    #[test]
    fn systemctl_cat_is_read_file_by_file_and_a_comment_with_a_path_is_no_file() {
        let files = parse(PRINTED).files;
        let paths: Vec<&str> = files.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "/etc/systemd/system/lamp-a.service",
                "/etc/systemd/system/lamp-b.service",
                "/etc/systemd/system/lamp-b.service.d/office.conf",
                "/etc/systemd/system/lamp-extra.service",
            ]
        );
        assert!(
            files[0]
                .text
                .contains("# /etc/lampOS/a.env is read by lamp-a.service\n")
        );
        assert!(
            files[0].text.ends_with("ExecStart=/opt/a\n"),
            "{:?}",
            files[0].text
        );
    }

    #[test]
    fn same_changed_drop_in_extra_and_missing_are_each_said_and_fail_the_check() {
        let report = check(&product(), &parse(PRINTED));
        assert_eq!(report.units["lamp-a.service"], Verdict::Same);
        assert_eq!(
            report.units["lamp-b.service"],
            Verdict::Differs(vec![
                "- Environment=LAMP_X=on".to_owned(),
                "+ Environment=LAMP_X=off".to_owned(),
            ])
        );
        assert_eq!(report.units["lamp-c.timer"], Verdict::Missing);
        assert_eq!(report.drop_ins.len(), 1);
        assert_eq!(
            report.drop_ins[0].path,
            "/etc/systemd/system/lamp-b.service.d/office.conf"
        );
        assert_eq!(report.extra, ["lamp-extra.service"]);
        assert!(!report.passes());
        let said = report.said();
        for line in [
            "lamp-a.service: same",
            "lamp-b.service: differs",
            "  - Environment=LAMP_X=on",
            "  + Environment=LAMP_X=off",
            r#"unit /etc/systemd/system/lamp-b.service turns LAMP_X off ("off"); the product sets it on"#,
            "lamp-c.timer: missing on the lamp",
            "drop-in: /etc/systemd/system/lamp-b.service.d/office.conf",
            "  Environment=LAMP_OFFICE=on",
            "not in the product: lamp-extra.service",
            "the lamp does not run exactly the product's units",
        ] {
            assert!(said.lines().any(|said| said == line), "{line}\n{said}");
        }
    }

    /// Every unit the same, from install's place, with nothing else: a pass.
    /// A unit read from another place is a finding even when its text is the
    /// same.
    #[test]
    fn exactly_the_product_passes_and_a_unit_from_elsewhere_does_not() {
        let units = product();
        let printed: String = units
            .iter()
            .map(|(name, text)| format!("# /etc/systemd/system/{name}\n{text}\n"))
            .collect();
        let report = check(&units, &parse(&printed));
        assert!(report.passes(), "{}", report.said());
        assert!(
            report
                .said()
                .ends_with("the lamp runs exactly the product's units\n")
        );
        let moved = printed.replace(
            "# /etc/systemd/system/lamp-c.timer",
            "# /usr/lib/systemd/system/lamp-c.timer",
        );
        let report = check(&units, &parse(&moved));
        assert_eq!(report.units["lamp-c.timer"], Verdict::Same);
        assert_eq!(report.elsewhere, ["/usr/lib/systemd/system/lamp-c.timer"]);
        assert!(!report.passes());
    }

    /// The reviewer on #1346: a unit whose own text holds a line with its own
    /// path (lamp-4ace's `lamp-skills.service` begins so) is one file, read
    /// whole, so a wrong line before it is never hidden.
    #[test]
    fn a_units_own_path_line_is_its_text_and_hides_nothing() {
        let units = BTreeMap::from([(
            "lamp-x.service".to_owned(),
            "# /etc/systemd/system/lamp-x.service\n[Service]\nExecStart=/opt/x\n".to_owned(),
        )]);
        let printed = "\
# /etc/systemd/system/lamp-x.service
[Service]
Environment=LAMP_EVIL=on
# /etc/systemd/system/lamp-x.service
[Service]
ExecStart=/opt/x
";
        let parsed = parse(printed);
        assert_eq!(parsed.files.len(), 1);
        let report = check(&units, &parsed);
        assert_eq!(
            report.units["lamp-x.service"],
            Verdict::Differs(vec![
                "+ [Service]".to_owned(),
                "+ Environment=LAMP_EVIL=on".to_owned(),
            ])
        );
        assert!(!report.passes());
        // The product's own file printed alone, its first line its own path:
        // the same.
        let alone = format!(
            "# /etc/systemd/system/lamp-x.service\n{}",
            units["lamp-x.service"]
        );
        assert!(check(&units, &parse(&alone)).passes());
    }

    /// systemd's notices: a masked unit is a finding; "No files found" for
    /// the implicit `lampos.slice` that `'lampos*'` matches (on stderr, saved
    /// only with `2>&1`) is nothing, and is not taken into the unit before
    /// it. A changed trailing space says so.
    #[test]
    fn a_masked_unit_fails_an_implicit_slice_does_not_and_white_space_shows() {
        let units = product();
        let mut printed: String = units
            .iter()
            .map(|(name, text)| format!("# /etc/systemd/system/{name}\n{text}\n"))
            .collect();
        printed.push_str("No files found for lampos.slice.\n");
        let report = check(&units, &parse(&printed));
        assert!(report.passes(), "{}", report.said());
        let masked = format!("{printed}# Unit lamp-d.service is masked.\n");
        let report = check(&units, &parse(&masked));
        assert_eq!(report.masked, ["lamp-d.service"]);
        assert!(report.said().contains("masked: lamp-d.service\n"));
        assert!(!report.passes());
        let spaced = printed.replace("ExecStart=/opt/b\n", "ExecStart=/opt/b \n");
        assert_eq!(
            check(&units, &parse(&spaced)).units["lamp-b.service"],
            Verdict::Differs(vec![
                "- ExecStart=/opt/b".to_owned(),
                "+ ExecStart=/opt/b (ends in white space)".to_owned(),
            ])
        );
    }

    /// The real product, printed as install writes it, passes: the embedded
    /// units need no expansion, and every one is found.
    #[test]
    fn the_product_as_install_writes_it_passes() {
        let units = embedded();
        assert!(units.contains_key("lamp-voice.service") && units.contains_key("lampos.target"));
        assert!(units.keys().all(|name| !name.starts_with('/')));
        let printed: String = units
            .iter()
            .map(|(name, text)| format!("# {}{name}\n{text}\n", install::UNITS))
            .collect();
        let report = check(&units, &parse(&printed));
        assert!(report.passes(), "{}", report.said());
        assert_eq!(report.units.len(), units.len());
    }
    /// A drop-in that turns its unit's `on` switch off says so in its
    /// finding, by name and value; one that sets other variables does not,
    /// and a later drop-in that sets the switch back on leaves it on, so
    /// nothing is said (the Reviewer-lead's P3 on #3728). The real
    /// product's lamp-cloud, and a template's instance, too.
    #[test]
    fn a_drop_in_that_turns_a_product_switch_off_says_so() {
        let units = product();
        let mut printed: String = units
            .iter()
            .map(|(name, text)| format!("# /etc/systemd/system/{name}\n{text}\n"))
            .collect();
        printed.push_str(
            "# /etc/systemd/system/lamp-b.service.d/old.conf\n[Service]\nEnvironment=LAMP_OFFICE=on \"LAMP_X=off\"\n\n\
             # /etc/systemd/system/lamp-b.service.d/other.conf\n[Service]\nEnvironment=LAMP_Z=on LAMP_Y=off\n",
        );
        let report = check(&units, &parse(&printed));
        assert_eq!(
            report.switches_off,
            [(
                "/etc/systemd/system/lamp-b.service.d/old.conf".to_owned(),
                r#"turns LAMP_X off ("off")"#.to_owned()
            )]
        );
        let said = report.said();
        let line = r#"drop-in /etc/systemd/system/lamp-b.service.d/old.conf turns LAMP_X off ("off"); the product sets it on"#;
        assert_eq!(
            said.lines().filter(|said| *said == line).count(),
            1,
            "{said}"
        );
        assert_eq!(said.matches("the product sets it on").count(), 1, "{said}");
        assert!(!report.passes());
        // A later drop-in sets it back on: on, and not said.
        let back = format!(
            "{printed}\n# /etc/systemd/system/lamp-b.service.d/zz-on.conf\n[Service]\nEnvironment=LAMP_X=on\n"
        );
        assert!(check(&units, &parse(&back)).switches_off.is_empty());
        // The office extras (PRINTED's office.conf) turn nothing off; only
        // lamp-b's own changed unit is named.
        assert_eq!(
            check(&units, &parse(PRINTED)).switches_off,
            [(
                "/etc/systemd/system/lamp-b.service".to_owned(),
                r#"turns LAMP_X off ("off")"#.to_owned()
            )]
        );

        let units = embedded();
        let printed = "\
# /etc/systemd/system/lamp-cloud.service.d/old.conf
[Service]
Environment=LAMP_CLOUD=off
";
        let said = check(&units, &parse(printed)).said();
        assert!(
            said.contains("drop-in /etc/systemd/system/lamp-cloud.service.d/old.conf turns LAMP_CLOUD off (\"off\"); the product sets it on\n"),
            "{said}"
        );

        // An instance's drop-in is read against its template; an empty
        // `Environment=` clears the template's switch.
        let units = BTreeMap::from([(
            "lamp-t@.service".to_owned(),
            "[Service]\nEnvironment=LAMP_T=on\n".to_owned(),
        )]);
        let printed =
            "# /etc/systemd/system/lamp-t@one.service.d/x.conf\n[Service]\nEnvironment=\n";
        assert_eq!(
            check(&units, &parse(printed)).switches_off,
            [(
                "/etc/systemd/system/lamp-t@one.service.d/x.conf".to_owned(),
                "turns LAMP_T off (an empty Environment= clears it)".to_owned()
            )]
        );
    }

    /// lamp-c0a4 after an install to stable (#3733): its base
    /// lamp-cloud.service was 19 Oct's, which sets no `LAMP_CLOUD=on`, with
    /// no drop-in. It differs, the check fails, and the switch is named.
    #[test]
    fn a_base_unit_of_19_oct_that_leaves_lamp_cloud_off_is_named() {
        let units = embedded();
        let old = include_str!("../tests/fixtures/units-19-oct/lamp-cloud.service");
        let printed = format!("# /etc/systemd/system/lamp-cloud.service\n{old}");
        let report = check(&units, &parse(&printed));
        assert!(matches!(
            report.units["lamp-cloud.service"],
            Verdict::Differs(_)
        ));
        assert!(!report.passes());
        assert!(report.drop_ins.is_empty());
        let said = report.said();
        let line = "unit /etc/systemd/system/lamp-cloud.service leaves LAMP_CLOUD off; the product sets it on";
        assert_eq!(
            said.lines().filter(|said| *said == line).count(),
            1,
            "{said}"
        );
        // The way that unit was meant to be used: a drop-in turns it on, and
        // systemd runs it on, so LAMP_CLOUD is not named (the Reviewer-lead's
        // P3 on #3846); the unit still differs.
        let with_on = format!(
            "{printed}\n# /etc/systemd/system/lamp-cloud.service.d/cloud.conf\n[Service]\nEnvironment=LAMP_CLOUD=on\n"
        );
        let report = check(&units, &parse(&with_on));
        assert!(matches!(
            report.units["lamp-cloud.service"],
            Verdict::Differs(_)
        ));
        assert!(
            !report
                .switches_off
                .iter()
                .any(|(_, how)| how.contains("LAMP_CLOUD ")),
            "{:?}",
            report.switches_off
        );
        // The build's own text names nothing.
        let (_, now) = install::PRODUCT
            .iter()
            .find(|(name, _)| *name == "lamp-cloud.service")
            .unwrap();
        let printed = format!("# /etc/systemd/system/lamp-cloud.service\n{now}");
        let report = check(&units, &parse(&printed));
        assert_eq!(report.units["lamp-cloud.service"], Verdict::Same);
        assert!(report.switches_off.is_empty());
    }

    const SPEAKER_PRINTED: &str = "\
# /etc/systemd/system/lamp-cmedia-speaker.service
[Service]
ExecStart=/usr/bin/amixer -q -c device_cmedia sset Speaker 9

# /etc/systemd/system/lamp-cmedia-speaker.service.d/30-local-gain.conf
[Service]
ExecStart=
";

    #[test]
    fn the_images_unit_is_the_images_not_extra_and_compared_with_its_variant() {
        let parsed = parse(SPEAKER_PRINTED);
        let report = check(&BTreeMap::new(), &parsed);
        assert!(report.extra.is_empty(), "{report:?}");
        assert_eq!(report.image["lamp-cmedia-speaker.service"], None);
        assert!(
            report
                .said()
                .contains("the image's: lamp-cmedia-speaker.service (carried")
        );
        // The drop-in a person made is still said.
        assert_eq!(report.drop_ins.len(), 1);
        let standard = BTreeMap::from([(
            "lamp-cmedia-speaker.service".to_owned(),
            "[Service]\nExecStart=/usr/bin/amixer -q -c device_cmedia sset Speaker 9\n".to_owned(),
        )]);
        let report = check_with_image(&BTreeMap::new(), Some(&standard), &parsed);
        assert_eq!(
            report.image["lamp-cmedia-speaker.service"],
            Some(Verdict::Same)
        );
        let pro = BTreeMap::from([(
            "lamp-cmedia-speaker.service".to_owned(),
            "[Service]\nExecStart=/usr/bin/amixer -q -c device_cmedia sset Speaker 6\n".to_owned(),
        )]);
        let report = check_with_image(&BTreeMap::new(), Some(&pro), &parsed);
        assert!(matches!(
            &report.image["lamp-cmedia-speaker.service"],
            Some(Verdict::Differs(lines)) if lines.len() == 2
        ));
        assert!(!report.passes());
        assert_eq!(
            image_unit_names(),
            [
                "lamp-cmedia-speaker.service",
                "respeaker-lite-softvol.service"
            ]
        );
    }
}
