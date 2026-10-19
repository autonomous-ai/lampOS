//! A lamp with many skills (#1668): every installed skill reached through one
//! function, `use_skill`, instead of one function each. A session may be given
//! at most 32 functions (`lamp_gemini::MAX_TOOLS`); the voice's own and the
//! actions part's are 30 of them, so a third skill's function would be
//! dropped. With `LAMP_USE_SKILL=on` a session is given `use_skill`, whose
//! `skill` names one of the installed skills, and its instruction lists each
//! skill in one line, under "Your skills" ([`section`]). A call is made the
//! skill's own call when it is run ([`translate`]), so the skill answers it
//! as before. Off unless set.
//!
//! A skill takes at most one choice (a word from its list) and one yes-or-no
//! (`lamp_skills::tools_file`), so `use_skill` takes `choice` and `flag`, and
//! each is given the name the skill's own parameter has.
use lamp_gemini::{Tool, ToolCall};
use serde_json::{Value, json};

/// The function.
pub const NAME: &str = "use_skill";
/// Where the coordinator reads whether skills are reached through it. `on`
/// only.
pub const ENVIRONMENT: &str = "LAMP_USE_SKILL";
/// The provider worker's flag for it.
pub const FLAG: &str = "--use-skill";

/// The worker's flag for what the environment says: `on` only.
pub fn arguments(value: Option<&str>) -> Vec<String> {
    if value == Some("on") {
        vec![FLAG.to_owned()]
    } else {
        Vec::new()
    }
}
/// The most the skills' lines may take of the instruction: one past it is
/// listed by its name alone, and one past even that is left out of the
/// lines (it can still be named, if the model knows it).
pub const SECTION_MOST_BYTES: usize = 4_096;

/// Of `skills`, those `use_skill` offers: as many as its parameters can
/// name within the link's bound (`lamp_gemini::MAX_TOOL_JSON_BYTES`), so a
/// lamp with more never loses the function, and every skill with it (the
/// reviewer on #1696). The rest are counted on the setup line.
pub fn offered(skills: &[Tool]) -> &[Tool] {
    let fits = |count: usize| {
        built(&skills[..count])
            .parameters
            .is_some_and(|parameters| {
                parameters.to_string().len() <= lamp_gemini::MAX_TOOL_JSON_BYTES
            })
    };
    let mut count = skills.len();
    while count > 0 && !fits(count) {
        count -= 1;
    }
    &skills[..count]
}

/// `use_skill`, for `skills` (the skills' own functions, as their tools file
/// says them), naming those it can ([`offered`]), or none when there is no
/// skill.
pub fn function(skills: &[Tool]) -> Option<Tool> {
    let offered = offered(skills);
    (!offered.is_empty()).then(|| built(offered))
}

fn built(skills: &[Tool]) -> Tool {
    let names: Vec<&str> = skills.iter().map(|skill| skill.name.as_str()).collect();
    Tool {
        name: NAME.to_owned(),
        description: "Use one of your skills, listed under \"Your skills\" in your instructions, each with when to use it. Call this when what someone asks is what a skill's line says: give the skill's name, and its choice and its yes-or-no only when its line names them. Then do what its answer tells you.".to_owned(),
        parameters: Some(json!({
            "type": "object",
            "properties": {
                "skill": {
                    "type": "string",
                    "enum": names,
                    "description": "The skill's name, as its line under \"Your skills\" gives it."
                },
                "choice": {
                    "type": "string",
                    "description": "The skill's choice: one of the words its line lists. Leave it out when its line names none, or the person gave none."
                },
                "flag": {
                    "type": "boolean",
                    "description": "The skill's yes-or-no, as its line says. Leave it out when its line names none."
                }
            },
            "required": ["skill"]
        })),
        silent: false,
    }
}

/// The instruction's lines for `skills`: one each, its name, when to use it,
/// and its choice and yes-or-no, within [`SECTION_MOST_BYTES`]. Empty when
/// there is no skill.
pub fn section(skills: &[Tool]) -> String {
    lines(skills).0
}

/// Of `skills`' lines ([`section`]), the skills named alone and those left
/// out, past [`SECTION_MOST_BYTES`]: for the setup line (the reviewer on
/// #1696).
pub fn cut(skills: &[Tool]) -> (Vec<String>, Vec<String>) {
    let (_, named, left) = lines(skills);
    (named, left)
}

fn lines(skills: &[Tool]) -> (String, Vec<String>, Vec<String>) {
    let (mut named_alone, mut left_out) = (Vec::new(), Vec::new());
    if skills.is_empty() {
        return (String::new(), named_alone, left_out);
    }
    let mut told = String::from(
        "\n\nYour skills\nEach is used through use_skill, by its name, when what someone asks is what its line says:\n",
    );
    for skill in skills {
        let whole = format!("- {}\n", line(skill));
        let named = format!("- {}\n", skill.name);
        if told.len() + whole.len() <= SECTION_MOST_BYTES {
            told.push_str(&whole);
        } else if told.len() + named.len() <= SECTION_MOST_BYTES {
            told.push_str(&named);
            named_alone.push(skill.name.clone());
        } else {
            left_out.push(skill.name.clone());
        }
    }
    (told, named_alone, left_out)
}

/// One skill's line: its name and description, then its choice and its
/// yes-or-no, each with what its own parameter says of it.
fn line(skill: &Tool) -> String {
    let mut line = format!("{}: {}", skill.name, skill.description.trim());
    if let Some((name, words, about)) = choice(skill) {
        line.push_str(&format!(
            " Its choice is {name}, one of {}{}.",
            words.join(", "),
            about
                .map(|about| format!(": {}", about.trim().trim_end_matches('.')))
                .unwrap_or_default()
        ));
    }
    if let Some((name, about)) = flag(skill) {
        line.push_str(&format!(
            " Its yes-or-no is {name}{}.",
            about
                .map(|about| format!(": {}", about.trim().trim_end_matches('.')))
                .unwrap_or_default()
        ));
    }
    line.replace('\n', " ")
}

/// The skill's choice: its parameter's name, its words, what it says of it.
fn choice(skill: &Tool) -> Option<(&str, Vec<&str>, Option<&str>)> {
    properties(skill).find_map(|(name, schema)| {
        let words: Vec<&str> = schema
            .get("enum")?
            .as_array()?
            .iter()
            .filter_map(Value::as_str)
            .collect();
        Some((
            name.as_str(),
            words,
            schema.get("description").and_then(Value::as_str),
        ))
    })
}

/// The skill's yes-or-no: its parameter's name, and what it says of it.
fn flag(skill: &Tool) -> Option<(&str, Option<&str>)> {
    properties(skill).find_map(|(name, schema)| {
        (schema.get("type").and_then(Value::as_str) == Some("boolean")).then(|| {
            (
                name.as_str(),
                schema.get("description").and_then(Value::as_str),
            )
        })
    })
}

fn properties(skill: &Tool) -> impl Iterator<Item = (&String, &Value)> {
    skill
        .parameters
        .as_ref()
        .and_then(|parameters| parameters.get("properties"))
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
}

/// `call`, a call of `use_skill`, as the call of the skill it names, with its
/// choice and yes-or-no under the skill's own parameters' names, for the
/// skill to run as when it is called itself. What the model is told
/// instead, when it names no installed skill or a word not in the skill's
/// choice. A choice or yes-or-no the skill does not take is left out, as
/// the skill would refuse it.
pub fn translate(call: &ToolCall, skills: &[Tool]) -> Result<ToolCall, Value> {
    let named = call.args.get("skill").and_then(Value::as_str);
    let Some(skill) = named.and_then(|named| skills.iter().find(|skill| skill.name == named))
    else {
        return Err(json!({
            "error": format!(
                "There is no skill named {}. Your skills are {}.",
                named.unwrap_or("that"),
                skills
                    .iter()
                    .map(|skill| skill.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }));
    };
    let choice_of = choice(skill);
    let flag_of = flag(skill);
    // The skill's own parameters' names are taken too, as its line says
    // them ("Its choice is style"); any other key is told, never dropped
    // (the reviewer on #1696).
    let known = |key: &str| {
        matches!(key, "skill" | "choice" | "flag")
            || choice_of.as_ref().is_some_and(|(name, _, _)| *name == key)
            || flag_of.as_ref().is_some_and(|(name, _)| *name == key)
    };
    if let Some(stray) = call
        .args
        .as_object()
        .and_then(|args| args.keys().find(|key| !known(key)))
    {
        // Only what the skill takes, so the model is not sent after a
        // choice it then is told the skill has none of (the reviewer on
        // #1706).
        let takes = match (choice_of.is_some(), flag_of.is_some()) {
            (true, true) => format!(
                "give {}'s choice as choice and its yes-or-no as flag",
                skill.name
            ),
            (true, false) => format!("give {}'s choice as choice", skill.name),
            (false, true) => format!("give {}'s yes-or-no as flag", skill.name),
            (false, false) => format!("{} takes only its name", skill.name),
        };
        return Err(json!({
            "error": format!("use_skill takes skill, choice and flag, not {stray}: {takes}.")
        }));
    }
    let given_choice = call.args.get("choice").or_else(|| {
        choice_of
            .as_ref()
            .and_then(|(name, _, _)| call.args.get(*name))
    });
    let given_flag = call
        .args
        .get("flag")
        .or_else(|| flag_of.as_ref().and_then(|(name, _)| call.args.get(*name)))
        .and_then(Value::as_bool);
    if choice_of.is_none()
        && given_choice
            .and_then(Value::as_str)
            .is_some_and(|word| !word.is_empty())
    {
        return Err(json!({"error": format!("{} takes no choice.", skill.name)}));
    }
    if flag_of.is_none() && given_flag == Some(true) {
        return Err(json!({"error": format!("{} takes no yes-or-no.", skill.name)}));
    }
    let mut args = serde_json::Map::new();
    if let (Some(given), Some((name, words, _))) = (given_choice, choice_of) {
        match given.as_str().filter(|given| words.contains(given)) {
            Some(word) => {
                args.insert(name.to_owned(), json!(word));
            }
            None => {
                return Err(json!({
                    "error": format!(
                        "{}'s choice is one of {}.",
                        skill.name,
                        words.join(", ")
                    )
                }));
            }
        }
    }
    if let (Some(given), Some((name, _))) = (given_flag, flag_of) {
        args.insert(name.to_owned(), json!(given));
    }
    Ok(ToolCall {
        id: call.id.clone(),
        name: skill.name.clone(),
        args: Value::Object(args),
    })
}

/// A name for each skill `use_skill` reaches, beside its own, so a session
/// given it is told apart from one given other skills
/// (`crate::session_tools::names`).
pub fn reached(tool: &Tool) -> Vec<String> {
    if tool.name != NAME {
        return Vec::new();
    }
    tool.parameters
        .as_ref()
        .and_then(|parameters| parameters.pointer("/properties/skill/enum"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|skill| format!("{NAME}:{skill}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(name: &str, description: &str, parameters: Option<Value>) -> Tool {
        Tool {
            name: name.into(),
            description: description.into(),
            parameters,
            silent: false,
        }
    }

    /// Dance, Desk tutor and its marks, as `lamp_skills::tools_file` writes
    /// them.
    fn installed() -> Vec<Tool> {
        vec![
            skill(
                "dance",
                "When someone asks the lamp to dance.",
                Some(json!({"type":"object","properties":{
                    "style":{"type":"string","enum":["calm","upbeat","jazz"],"description":"The mood of the dance."},
                    "long":{"type":"boolean","description":"True when they asked for a long dance."}}})),
            ),
            skill(
                "check_my_work",
                "Call when someone asks the lamp to check their work.",
                None,
            ),
            skill(
                "show_verdict",
                "Call only after check_my_work.",
                Some(json!({"type":"object","properties":{
                    "verdict":{"type":"string","enum":["right","wrong"],"description":"Always give one."}}})),
            ),
        ]
    }

    fn call(args: Value) -> ToolCall {
        ToolCall {
            id: "c1".into(),
            name: NAME.into(),
            args,
        }
    }

    #[test]
    fn one_function_names_every_skill_and_the_link_takes_it() {
        assert!(function(&[]).is_none());
        let told = function(&installed()).unwrap();
        assert!(told.validate().is_ok());
        assert_eq!(
            told.parameters.as_ref().unwrap()["properties"]["skill"]["enum"],
            json!(["dance", "check_my_work", "show_verdict"])
        );
        assert_eq!(
            reached(&told),
            [
                "use_skill:dance",
                "use_skill:check_my_work",
                "use_skill:show_verdict"
            ]
        );
        // Sixty skills of the longest names still fit the link's bound.
        let many: Vec<Tool> = (0..60)
            .map(|n| skill(&format!("{n:0>48}").replacen('0', "a", 1), "A skill.", None))
            .collect();
        assert!(function(&many).unwrap().validate().is_ok());
    }

    #[test]
    fn the_instruction_lists_each_skill_with_its_choice_and_its_yes_or_no() {
        assert_eq!(section(&[]), "");
        let told = section(&installed());
        assert!(told.starts_with("\n\nYour skills\n"), "{told}");
        assert!(told.contains(
            "- dance: When someone asks the lamp to dance. Its choice is style, one of calm, upbeat, jazz: The mood of the dance. Its yes-or-no is long: True when they asked for a long dance.\n"
        ), "{told}");
        assert!(
            told.contains(
                "- check_my_work: Call when someone asks the lamp to check their work.\n"
            )
        );
        assert!(told.contains("- show_verdict: Call only after check_my_work. Its choice is verdict, one of right, wrong: Always give one.\n"));
    }

    /// Past the section's bound a skill is named alone, and past even that
    /// it is left out of the lines.
    #[test]
    fn the_skills_lines_keep_within_their_bound() {
        let long = "x".repeat(1_000);
        let many: Vec<Tool> = (0..400)
            .map(|n| skill(&format!("skill_{n}"), &long, None))
            .collect();
        let told = section(&many);
        assert!(told.len() <= SECTION_MOST_BYTES, "{}", told.len());
        assert!(told.contains("- skill_0: x"));
        assert!(
            told.contains("- skill_4\n"),
            "named alone once the lines are full"
        );
        assert!(!told.contains("- skill_399"), "left out past the bound");
    }

    #[test]
    fn a_call_becomes_the_skills_own_with_its_parameters_names() {
        let skills = installed();
        let dance = translate(
            &call(json!({"skill":"dance","choice":"jazz","flag":true})),
            &skills,
        )
        .unwrap();
        assert_eq!(dance.id, "c1");
        assert_eq!(dance.name, "dance");
        assert_eq!(dance.args, json!({"style":"jazz","long":true}));
        let check = translate(&call(json!({"skill":"check_my_work"})), &skills).unwrap();
        assert_eq!(
            (check.name.as_str(), check.args),
            ("check_my_work", json!({}))
        );
        let verdict = translate(
            &call(json!({"skill":"show_verdict","choice":"wrong"})),
            &skills,
        )
        .unwrap();
        assert_eq!(verdict.args, json!({"verdict":"wrong"}));
    }

    /// The reviewer on #1696: the switch is read as `on` only.
    #[test]
    fn the_switch_is_on_only_for_on() {
        assert!(arguments(None).is_empty());
        assert!(arguments(Some("yes")).is_empty());
        assert!(arguments(Some("off")).is_empty());
        assert_eq!(arguments(Some("on")), [FLAG]);
    }

    /// The reviewer on #1696: the skill's own parameters' names are taken
    /// as `choice` and `flag` are, and anything else, or a choice or a yes
    /// to a skill that takes none, is told, not dropped.
    #[test]
    fn a_skills_own_parameter_names_are_taken_and_anything_else_is_told() {
        let skills = installed();
        let dance = translate(
            &call(json!({"skill":"dance","style":"jazz","long":true})),
            &skills,
        )
        .unwrap();
        assert_eq!(dance.args, json!({"style":"jazz","long":true}));
        let stray = translate(&call(json!({"skill":"dance","mood":"jazz"})), &skills).unwrap_err();
        assert_eq!(
            stray["error"],
            "use_skill takes skill, choice and flag, not mood: give dance's choice as choice and its yes-or-no as flag."
        );
        let bare =
            translate(&call(json!({"skill":"check_my_work","page":1})), &skills).unwrap_err();
        assert_eq!(
            bare["error"],
            "use_skill takes skill, choice and flag, not page: check_my_work takes only its name."
        );
        let word =
            translate(&call(json!({"skill":"show_verdict","mood":"x"})), &skills).unwrap_err();
        assert_eq!(
            word["error"],
            "use_skill takes skill, choice and flag, not mood: give show_verdict's choice as choice."
        );
        let no_choice = translate(
            &call(json!({"skill":"check_my_work","choice":"jazz"})),
            &skills,
        )
        .unwrap_err();
        assert_eq!(no_choice["error"], "check_my_work takes no choice.");
        let no_flag = translate(
            &call(json!({"skill":"show_verdict","choice":"right","flag":true})),
            &skills,
        )
        .unwrap_err();
        assert_eq!(no_flag["error"], "show_verdict takes no yes-or-no.");
        // An empty choice or a "no" to a skill that takes none is nothing.
        let check = translate(
            &call(json!({"skill":"check_my_work","choice":"","flag":false})),
            &skills,
        )
        .unwrap();
        assert_eq!(check.args, json!({}));
    }

    /// The reviewer on #1696: past the link's bound `use_skill` names as
    /// many skills as fit, and keeps its place; the lines say which are
    /// named alone and which are left out.
    #[test]
    fn past_its_bounds_use_skill_keeps_what_fits_and_says_what_it_cut() {
        let many: Vec<Tool> = (0..120)
            .map(|n| skill(&format!("{n:0>48}").replacen('0', "a", 1), "A skill.", None))
            .collect();
        let offered = offered(&many);
        assert!(
            offered.len() > 60 && offered.len() < 120,
            "{}",
            offered.len()
        );
        let told = function(&many).unwrap();
        assert!(told.validate().is_ok());
        assert_eq!(reached(&told).len(), offered.len());
        let long = "x".repeat(1_000);
        let wordy: Vec<Tool> = (0..400)
            .map(|n| skill(&format!("skill_{n}"), &long, None))
            .collect();
        let (named, left) = cut(&wordy);
        assert_eq!(named.first().map(String::as_str), Some("skill_3"));
        assert!(left.contains(&"skill_399".to_owned()));
        assert_eq!(named.len() + left.len() + 3, 400);
        assert_eq!(cut(&installed()), (Vec::new(), Vec::new()));
    }

    #[test]
    fn a_call_naming_no_skill_or_a_word_not_in_its_choice_is_told_so() {
        let skills = installed();
        let none = translate(&call(json!({"skill":"juggle"})), &skills).unwrap_err();
        assert_eq!(
            none["error"],
            "There is no skill named juggle. Your skills are dance, check_my_work, show_verdict."
        );
        let unnamed = translate(&call(json!({})), &skills).unwrap_err();
        assert!(
            unnamed["error"]
                .as_str()
                .unwrap()
                .starts_with("There is no skill named that.")
        );
        let word =
            translate(&call(json!({"skill":"dance","choice":"tango"})), &skills).unwrap_err();
        assert_eq!(
            word["error"],
            "dance's choice is one of calm, upbeat, jazz."
        );
    }
}
