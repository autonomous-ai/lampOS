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
use std::collections::BTreeSet;

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
/// The most the skills' lines may take of the instruction. Past it a core
/// skill's line is kept whole first, every other skill is given a short
/// line ([`SHORT_MOST_BYTES`]), then one past that is named alone, and one
/// past even that is left out of the lines (it can still be named, if the
/// model knows it).
pub const SECTION_MOST_BYTES: usize = 4_096;
/// The most of a short line's description: its first sentence, cut at a
/// word's end past this.
pub const SHORT_MOST_BYTES: usize = 200;

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
        description: "Use one of your skills, listed under \"Your skills\" in your instructions, each with when to use it. Call this when what someone asks is what a skill's line says: give the skill's name, and its choice and its yes-or-no only when its line names them. The call runs the skill: then do what its answer tells you, and don't call it again for the same ask unless its answer tells you to.".to_owned(),
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

/// The key of the note a skill's answer through `use_skill` carries ([`ran`]).
pub const RAN: &str = "ran";

/// A skill's answer through `use_skill`, told that this call has already
/// run it (#2826: on lamp-9adf on 6 Nov, told by `dance` to "Say one short,
/// playful line that you are going to dance", the model called `use_skill`
/// for `dance` again in the next round, which lamp-skills refused as under
/// way, and said a stray line; called by its own name, `dance` ran once).
/// An error (lamp-skills sends every refusal, "I'm still doing that."
/// among them, as `error`), or an answer that is not an object, is left as
/// it is. The caller gives none to a skill whose tools file gives it a
/// search (`crate::tools::search_walk`): a search is several calls of its
/// skill in one turn, each answer telling the model to call again with the
/// next place (look-for-things' `[search]`, a reader's P2 on #5505).
pub fn ran(skill: &str, told: &mut Value) {
    if let Some(answer) = told.as_object_mut()
        && !answer.contains_key("error")
    {
        answer.insert(
            RAN.to_owned(),
            Value::String(format!(
                "This call has already run {skill}. Do not call use_skill for {skill} again in this turn."
            )),
        );
    }
}

/// For tests: `told` as the model is told it, through `use_skill` when
/// `wrapped` ([`ran`]), else as it is.
#[cfg(test)]
pub(crate) fn told_through(wrapped: bool, skill: &str, mut told: Value) -> Value {
    if wrapped {
        ran(skill, &mut told);
    }
    told
}

/// The instruction's lines for `skills`: one each, its name, when to use it,
/// and its choice and yes-or-no, within [`SECTION_MOST_BYTES`], in
/// `skills`' order. Past the bound the lines of `core`'s skills (the tools
/// file's `core`, `crate::tools::core_skills`) are kept whole first, and
/// every other skill is given a short line ([`short_line`]), each made
/// whole again, in order, while there is room. Empty when there is no
/// skill.
pub fn section(skills: &[Tool], core: &BTreeSet<String>) -> String {
    lines(skills, core).0
}

/// Of `skills`' lines ([`section`]), past [`SECTION_MOST_BYTES`]: the
/// skills given a short line, those named alone and those left out, for
/// the setup line (the reviewer on #1696).
pub fn cut(skills: &[Tool], core: &BTreeSet<String>) -> Cut {
    lines(skills, core).1
}

/// The skills whose lines [`section`] made less than whole.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cut {
    pub shortened: Vec<String>,
    pub named_alone: Vec<String>,
    pub left_out: Vec<String>,
}

impl Cut {
    /// Every line was whole.
    pub fn is_empty(&self) -> bool {
        self.shortened.is_empty() && self.named_alone.is_empty() && self.left_out.is_empty()
    }
}

const HEAD: &str = "\n\nYour skills\nEach is used through use_skill, by its name, when what someone asks is what its line says:\n";

fn lines(skills: &[Tool], core: &BTreeSet<String>) -> (String, Cut) {
    let mut cut = Cut::default();
    if skills.is_empty() {
        return (String::new(), cut);
    }
    let whole: Vec<String> = skills
        .iter()
        .map(|skill| format!("- {}\n", line(skill)))
        .collect();
    if HEAD.len() + whole.iter().map(String::len).sum::<usize>() <= SECTION_MOST_BYTES {
        return (HEAD.to_owned() + &whole.concat(), cut);
    }
    let short: Vec<String> = skills
        .iter()
        .map(|skill| format!("- {}\n", short_line(skill)))
        .collect();
    let named: Vec<String> = skills
        .iter()
        .map(|skill| format!("- {}\n", skill.name))
        .collect();
    // Which line each skill is given, `None` for none: the core skills'
    // first, whole while it fits; then the others', short while it fits;
    // then, in order, the others made whole again while there is room.
    let mut given: Vec<Option<&String>> = vec![None; skills.len()];
    let mut used = HEAD.len();
    let is_core = |index: usize| core.contains(&skills[index].name);
    for core_first in [true, false] {
        for index in (0..skills.len()).filter(|index| is_core(*index) == core_first) {
            let tried = if core_first {
                [&whole[index], &short[index], &named[index]]
            } else {
                [&short[index], &named[index], &named[index]]
            };
            if let Some(fits) = tried
                .into_iter()
                .find(|line| used + line.len() <= SECTION_MOST_BYTES)
            {
                used += fits.len();
                given[index] = Some(fits);
            }
        }
    }
    for index in (0..skills.len()).filter(|index| !is_core(*index)) {
        if given[index] == Some(&short[index])
            && used - short[index].len() + whole[index].len() <= SECTION_MOST_BYTES
        {
            used = used - short[index].len() + whole[index].len();
            given[index] = Some(&whole[index]);
        }
    }
    let mut told = HEAD.to_owned();
    for (index, skill) in skills.iter().enumerate() {
        let Some(line) = given[index] else {
            cut.left_out.push(skill.name.clone());
            continue;
        };
        told.push_str(line);
        if *line == whole[index] {
            // Whole: nothing cut.
        } else if *line == short[index] {
            cut.shortened.push(skill.name.clone());
        } else {
            cut.named_alone.push(skill.name.clone());
        }
    }
    (told, cut)
}

/// A skill's short line, past the bound ([`section`]): its name and its
/// description's first sentence, at most [`SHORT_MOST_BYTES`] of it
/// ([`first_sentence`]), then its choice's name and words and its
/// yes-or-no's name, without what they say of them, so a call can still
/// give them.
fn short_line(skill: &Tool) -> String {
    let mut line = format!(
        "{}: {}",
        skill.name,
        first_sentence(skill.description.trim(), SHORT_MOST_BYTES)
    );
    if let Some((name, words, _)) = choice(skill) {
        line.push_str(&format!(
            " Its choice is {name}, one of {}.",
            words.join(", ")
        ));
    }
    if let Some((name, _)) = flag(skill) {
        line.push_str(&format!(" Its yes-or-no is {name}."));
    }
    line.replace('\n', " ")
}

/// `text`'s first sentence (to a '.', '!' or '?' before a space or the
/// end), whole when it is at most `most` bytes; past that, cut at the last
/// word's end, on a char's boundary, and ended "…", within `most` bytes.
fn first_sentence(text: &str, most: usize) -> String {
    const MORE: &str = "…";
    let end = text
        .char_indices()
        .find(|&(at, c)| {
            matches!(c, '.' | '!' | '?')
                && text[at + c.len_utf8()..]
                    .chars()
                    .next()
                    .is_none_or(char::is_whitespace)
        })
        .map_or(text.len(), |(at, c)| at + c.len_utf8());
    let sentence = &text[..end];
    if sentence.len() <= most {
        return sentence.to_owned();
    }
    let within = &sentence[..sentence.floor_char_boundary(most.saturating_sub(MORE.len()))];
    // Its last word whole, or left out when the cut is inside it.
    let words = if sentence[within.len()..].starts_with(char::is_whitespace) {
        within
    } else {
        within
            .rfind(char::is_whitespace)
            .map_or(within, |space| &within[..space])
    };
    format!(
        "{}{MORE}",
        words.trim_end_matches([',', ';', ':', '-', ' '])
    )
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

    /// #2826: a skill's answer through `use_skill` says the call already
    /// ran it, so the model does not call it again in the turn's next round;
    /// a refusal (lamp-skills' "its run is under way") and an answer that is
    /// not an object are left as they are. The function says so too.
    #[test]
    fn an_answer_through_use_skill_says_the_skill_already_ran() {
        let mut told = json!({"next": "Say one short, playful line that you are going to dance."});
        ran("dance", &mut told);
        assert_eq!(
            told[RAN],
            "This call has already run dance. Do not call use_skill for dance again in this turn."
        );
        assert_eq!(
            told["next"],
            "Say one short, playful line that you are going to dance."
        );
        // lamp-skills' refusal of a call while its run is under way, as its
        // `refuse` writes it (`{"error": why}`, with its default `still`).
        let mut refused = json!({"error": "I'm still doing that."});
        ran("dance", &mut refused);
        assert_eq!(refused, json!({"error": "I'm still doing that."}));
        let mut plain = json!("Danced.");
        ran("dance", &mut plain);
        assert_eq!(plain, json!("Danced."));
        let tool = function(&installed()).unwrap();
        assert!(
            tool.description
                .ends_with("don't call it again for the same ask unless its answer tells you to."),
            "{}",
            tool.description
        );
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
        assert_eq!(section(&[], &none()), "");
        let told = section(&installed(), &none());
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

    fn none() -> BTreeSet<String> {
        BTreeSet::new()
    }

    /// Past the section's bound a core skill's line is kept whole, every
    /// other skill is given a short line, past that one is named alone, and
    /// past even that it is left out of the lines.
    #[test]
    fn the_skills_lines_keep_within_their_bound() {
        let long = "x".repeat(1_000);
        let mut many: Vec<Tool> = (0..399)
            .map(|n| skill(&format!("skill_{n}"), &long, None))
            .collect();
        let core_line = "y".repeat(900);
        many.push(skill("skill_399", &core_line, None));
        let core = BTreeSet::from(["skill_399".to_owned()]);
        let told = section(&many, &core);
        assert!(told.len() <= SECTION_MOST_BYTES, "{}", told.len());
        assert!(
            told.contains(&format!("- skill_399: {core_line}\n")),
            "a core skill's line whole, though it comes last"
        );
        let short = format!("- skill_0: {}…\n", "x".repeat(SHORT_MOST_BYTES - 3));
        assert!(told.contains(&short), "{told}");
        assert!(
            told.contains("- skill_14\n"),
            "named alone once the short lines are full"
        );
        assert!(!told.contains("- skill_398"), "left out past the bound");
        let cut = cut(&many, &core);
        assert_eq!(cut.shortened.len(), 14, "{cut:?}");
        assert_eq!(cut.shortened.first().map(String::as_str), Some("skill_0"));
        assert_eq!(
            cut.named_alone.first().map(String::as_str),
            Some("skill_14")
        );
        assert_eq!(
            cut.shortened.len() + cut.named_alone.len() + cut.left_out.len(),
            399
        );
    }

    /// The product's skills and the Skills Store's nine packages, as
    /// `lamp_skills::tools_file` wrote them on lamp-voice (90525c3717), the
    /// product's light skills installed off (`PRODUCT_SKILLS_OFF`) and
    /// prepare-unplug left out (`LAMP_UNPLUG` off).
    fn store_installed() -> (Vec<Tool>, BTreeSet<String>) {
        let file: Value =
            serde_json::from_str(include_str!("../fixtures/use-skill-store-tools.json")).unwrap();
        let tools: Vec<Tool> = serde_json::from_value(file["tools"].clone()).unwrap();
        let core: BTreeSet<String> = serde_json::from_value(file["core"].clone()).unwrap();
        (tools, core)
    }

    /// The product's own skills fit whole: their lines are as before, byte
    /// for byte (each whole line, in the tools file's order).
    #[test]
    fn the_products_own_skills_lines_are_whole_as_before() {
        let (all, core) = store_installed();
        let product: Vec<Tool> = all
            .into_iter()
            .filter(|tool| core.contains(&tool.name))
            .collect();
        assert_eq!(product.len(), 4);
        let told = section(&product, &core);
        let before: String = product
            .iter()
            .map(|skill| format!("- {}\n", line(skill)))
            .collect();
        assert_eq!(told, format!("{HEAD}{before}"));
        assert!(told.len() < SECTION_MOST_BYTES, "{}", told.len());
        assert!(cut(&product, &core).is_empty());
        // As without a core skill named: what fits is not reordered.
        assert_eq!(section(&product, &none()), told);
    }

    /// The Store audit on lamp-voice 90525c3717: with the nine Store
    /// packages installed beside the product's skills, `set_pose` and
    /// `wellbeing_nudges` (core) were named alone, and `study_buddy` and
    /// `who_was_here` too. Now every core skill's line is whole, every
    /// skill has at least a short line, and the section keeps its bound.
    #[test]
    fn with_the_stores_skills_the_core_lines_stay_whole_and_none_is_named_alone() {
        let (all, core) = store_installed();
        assert_eq!(all.len(), 12);
        let told = section(&all, &core);
        assert!(told.len() <= SECTION_MOST_BYTES, "{}", told.len());
        for skill in &all {
            if core.contains(&skill.name) {
                assert!(
                    told.contains(&format!("- {}\n", line(skill))),
                    "{} whole: {told}",
                    skill.name
                );
            } else {
                assert!(
                    told.contains(&format!("- {}: ", skill.name)),
                    "{} has a line: {told}",
                    skill.name
                );
            }
        }
        for name in ["set_pose", "wellbeing_nudges"] {
            assert!(core.contains(name), "{name}");
        }
        let cut = cut(&all, &core);
        assert!(
            cut.named_alone.is_empty() && cut.left_out.is_empty(),
            "{cut:?}"
        );
        assert!(!cut.shortened.is_empty(), "{cut:?}");
        for name in &cut.shortened {
            assert!(!core.contains(name), "{name}");
        }
        // Still in the tools file's order.
        let at = |name: &str| told.find(&format!("- {name}:")).unwrap();
        assert!(at("dance") < at("focus_timer") && at("focus_timer") < at("who_was_here"));
    }

    /// A short line is its description's first sentence, cut past
    /// `SHORT_MOST_BYTES` at a word's end on a char's boundary.
    #[test]
    fn a_short_line_is_the_first_sentence_cut_at_a_words_end() {
        assert_eq!(
            first_sentence("Call when asked. Then more.", 200),
            "Call when asked."
        );
        assert_eq!(
            first_sentence("Say \"where are my keys?\", or so. More.", 200),
            "Say \"where are my keys?\", or so."
        );
        assert_eq!(first_sentence("No end at all", 200), "No end at all");
        // Multi-byte: "đèn" is 5 bytes, "ơi" 3; a cut never splits a char.
        let text = "Bật đèn ơi ".repeat(30);
        for most in 10..=200 {
            let short = first_sentence(&text, most);
            assert!(short.len() <= most, "{most}: {short}");
            assert!(short.ends_with('…'), "{most}: {short}");
            let words = short.trim_end_matches('…');
            assert!(text.starts_with(words), "{most}: {short}");
            assert!(
                text[words.len()..].starts_with(' '),
                "a word's end at {most}: {short}"
            );
        }
        // One word longer than the bound: cut on a char's boundary.
        let one = "ơ".repeat(100);
        let short = first_sentence(&one, 50);
        assert!(short.len() <= 50 && short.starts_with("ơơ"), "{short}");
        // The short line keeps the choice's words and the yes-or-no's name.
        let dance = &installed()[0];
        assert_eq!(
            short_line(dance),
            "dance: When someone asks the lamp to dance. Its choice is style, one of calm, upbeat, jazz. Its yes-or-no is long."
        );
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
        let cut_of = cut(&wordy, &none());
        assert_eq!(
            cut_of.shortened.first().map(String::as_str),
            Some("skill_0")
        );
        assert!(cut_of.left_out.contains(&"skill_399".to_owned()));
        assert_eq!(
            cut_of.shortened.len() + cut_of.named_alone.len() + cut_of.left_out.len(),
            400
        );
        assert!(cut(&installed(), &none()).is_empty());
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
