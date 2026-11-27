//! The walk Lamp leads by voice (#8871), off unless
//! `LAMP_ONBOARDING_WALK_V2=on`: after "how you sound", the person tries one
//! skill by voice ("turn blue") and makes one ("remind me to drink water
//! every 30 minutes"). Wi-Fi is counted as the walk's first step, done by
//! the time Lamp speaks, so every opener starts "Step n of 7: ". Every
//! line is short (the owner: one sentence an opener, about 12 words), and
//! the walk ends with the Store.
//!
//! Both new steps are the walk's own, in code: the skill file's `order`
//! stays as it is, since an older build refuses a step it doesn't know
//! there (`skill::Definition`, `deny_unknown_fields`). With the switch on
//! they are put after "how you sound" in the order Voice snapshots
//! ([`extend`]), and the provider worker reads the same order
//! ([`super::installed_order`]).
//!
//! Each new step ends done only by evidence the worker sees, never a
//! report: a light call that ran from the person's own line for "try a
//! skill", and a skill `make_skill` made and put on from their own line for
//! "make a skill" ([`tool_ran`], a mailbox line the runner takes only while
//! it leads that step). The model may only skip them.
//!
//! A person who asks where the walk is ("which step are we on?", "how many
//! more?", "is setup done?") hears the runner's own fixed line, from its
//! real state ([`StateLines`]), with the step's ask in it: the second
//! route's worker says it as is, no chat asked ([`state_words`]). The model
//! is never told the steps.
use super::{Step, skill::StepWords};
use crate::language::Language;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::Path;

/// The switch, in the voice's unit: `on` exactly.
pub const ENVIRONMENT: &str = "LAMP_ONBOARDING_WALK_V2";
/// The two steps' ids, as the progress file and the app name them.
pub const TRY_SKILL: &str = "try_skill";
pub const MAKE_SKILL: &str = "make_skill";
/// The new steps, in their order.
pub(crate) const STEPS: [Step; 2] = [Step::TrySkill, Step::MakeSkill];
/// The walk's six steps after Wi-Fi, as the app counts them.
pub(crate) const WALK: [Step; 6] = [
    Step::WhereYouSit,
    Step::WhoYouAre,
    Step::Touch,
    Step::HowYouSound,
    Step::TrySkill,
    Step::MakeSkill,
];
/// The quiet after an ask before its one reminder (#8871: 10 s).
pub(crate) const REMIND_QUIET_US: u64 = 10_000_000;

/// A step that, tried and not done, is offered again later like one
/// nobody tried (#8871): every walk step but where you sit, which is never
/// later.
pub(crate) fn again_later(step: Step) -> bool {
    WALK.contains(&step) && step != Step::WhereYouSit
}

/// Whether the switch reads on.
pub fn switched(value: Option<&str>) -> bool {
    value == Some("on")
}

/// The switch in this process's environment.
pub fn on() -> bool {
    #[cfg(test)]
    if ON_FOR_TEST.with(std::cell::Cell::get) {
        return true;
    }
    switched(std::env::var(ENVIRONMENT).ok().as_deref())
}

#[cfg(test)]
thread_local! {
    static ON_FOR_TEST: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The switch on for this test's thread only.
#[cfg(test)]
pub(crate) fn set_on_for_test(on: bool) {
    ON_FOR_TEST.with(|cell| cell.set(on));
}

/// `order` with the two new steps right after "how you sound" (at the end
/// when it isn't there), each once.
pub(crate) fn with_steps(order: &[Step]) -> Vec<Step> {
    let mut order: Vec<Step> = order
        .iter()
        .copied()
        .filter(|step| !STEPS.contains(step))
        .collect();
    let at = order
        .iter()
        .position(|step| *step == Step::HowYouSound)
        .map_or(order.len(), |at| at + 1);
    for (offset, step) in STEPS.into_iter().enumerate() {
        order.insert(at + offset, step);
    }
    order
}

/// The definition Voice snapshots, with the new steps and their words.
pub(crate) fn extend(definition: &mut super::skill::Definition) {
    definition.order = with_steps(&definition.order);
    for step in STEPS {
        definition.steps.insert(step, words(step));
    }
    // The owner's short lines (#8871: one sentence an opener, about 12
    // words at most), in place of the file's, in memory only.
    for (step, first) in [(Step::WhereYouSit, SIT_FIRST), (Step::Touch, TOUCH_FIRST)] {
        if let Some(words) = definition.steps.get_mut(&step) {
            words.first = first.to_owned();
            words.reoffer = first.to_owned();
        }
    }
    definition.words.sit_done = SIT_DONE.to_owned();
    definition.words.touch_done = Some(TOUCH_DONE.to_owned());
    definition.words.later = Some(LATER.to_owned());
}

// The owner's short lines for the file's steps (#8871).
pub(crate) const SIT_FIRST: &str = "(Say only this, nothing before or after it: \"Sit where you usually do and look at me, so I always know where to find you.\")";
pub(crate) const SIT_DONE: &str = "(Say only this, nothing before or after it: \"There you are.\")";
pub(crate) const TOUCH_FIRST: &str = "(Say only this, nothing before or after it: \"Pet my head. That's how you can say hi or hush me without a word.\")";
pub(crate) const TOUCH_DONE: &str = "(Say only this, nothing before or after it: \"Mmm, nice.\")";
/// The face question (#8871), asked in place of `skill::FACE_ASK`: a
/// question the worker's setup save still takes a yes to
/// (`known_faces::asks_for_the_face`).
pub(crate) const FACE_ASK: &str = "(Say only this, nothing before or after it: \"Can I remember your face? Then I'll know it's you, not a guest. It stays on this lamp.\")";
pub(crate) const LATER: &str =
    "(Say only this, nothing before or after it: \"We'll do this later.\")";

// "Try a skill". The ask is a note for the model, so its answer to "turn
// blue" knows why; the rest are the walk's own lines.
pub(crate) const TRY_FIRST: &str = "(Nobody has spoken. Say exactly this: \"Say 'Hey Lamp, turn blue.' See how easy asking is.\" When they ask for a light, in any words, set it at once with your light functions. Never call setup_step for this, except skipped if they ask to skip it: the lamp sees the light change itself.)";
pub(crate) const TRY_REMIND: &str =
    "(Say only this, nothing before or after it: \"Just say: turn blue.\")";
pub(crate) const TRY_DONE: &str = "(Say only this, nothing before or after it: \"See? Just ask.\")";
const TRY_SKIPPED: &str =
    "(Say only this, nothing before or after it: \"Okay, we'll skip trying a skill for now.\")";
const TRY_LATER: &str = "(Say only this, nothing before or after it: \"Earlier we skipped trying a skill. Let's try it now.\")";

// "Make a skill".
pub(crate) const MAKE_FIRST: &str = "(Nobody has spoken. Say exactly this: \"Teach me something you'd like, for example 'remind me to drink water every 30 minutes'.\" Any skill they ask for counts, in their own words: call make_skill with their words. If what they ask is too vague to make, ask them one short question about what it should do and when. If they ask for ideas, say only: \"Or: turn the light warm at 8 pm, or tell me to stand up every hour.\" Never call setup_step for this, except skipped if they ask to skip it.)";
pub(crate) const MAKE_IDEAS: &str = "(Say only this, nothing before or after it: \"Or: turn the light warm at 8 pm, or tell me to stand up every hour.\")";
/// The read-back, the walk's own once the skill is on: `{skill}` is its
/// name and what it does ([`read_back`]).
pub(crate) const MAKE_DONE: &str = "(Say only this, nothing before or after it: \"I made {skill}. Change it any time in the app.\")";
const MAKE_SKIPPED: &str =
    "(Say only this, nothing before or after it: \"Okay, we'll skip making a skill for now.\")";
const MAKE_LATER: &str = "(Say only this, nothing before or after it: \"Earlier we skipped making a skill. Let's try it now.\")";

/// What the model is told in place of a make's own `next` while the walk
/// leads "make a skill": the walk reads it back itself.
pub(crate) const MAKE_READ_BACK_NEXT: &str =
    "The lamp reads this skill back itself now. Say nothing more.";
/// What it is told for a make refused while the walk leads it: one simpler
/// example, never a list.
pub(crate) const MAKE_SIMPLER_NEXT: &str = "Say only this: \"Let's start with something simpler, like: remind me to drink water every 30 minutes.\"";

/// Every fixed line and note of the new steps, for the language tests.
#[cfg(test)]
pub(crate) const NOTES: [&str; 16] = [
    SIT_FIRST,
    SIT_DONE,
    TOUCH_FIRST,
    TOUCH_DONE,
    FACE_ASK,
    LATER,
    TRY_FIRST,
    TRY_REMIND,
    TRY_DONE,
    TRY_SKIPPED,
    TRY_LATER,
    MAKE_FIRST,
    MAKE_IDEAS,
    MAKE_SKIPPED,
    MAKE_LATER,
    DONE_STORE,
];

fn words(step: Step) -> StepWords {
    let (first, skipped, later) = match step {
        Step::TrySkill => (TRY_FIRST, TRY_SKIPPED, TRY_LATER),
        _ => (MAKE_FIRST, MAKE_SKIPPED, MAKE_LATER),
    };
    StepWords {
        first: first.to_owned(),
        reoffer: first.to_owned(),
        skipped: Some(skipped.to_owned()),
        later_reoffer: Some(later.to_owned()),
        nudge: None,
    }
}

/// Whether `said`, a `redo_setup` step, names the whole walk (#8871):
/// "whole", "all", "everything", "from the start", "full", "setup".
pub(crate) fn whole_walk(said: &str) -> bool {
    let said = said.to_lowercase();
    let words: Vec<&str> = said
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    let has = |word: &str| words.contains(&word);
    has("whole")
        || has("all")
        || has("everything")
        || has("full")
        || has("entire")
        || said.contains("from the start")
        || said.contains("from scratch")
        || words == ["setup"]
        || words == ["set", "up"]
        || has("tất")
        || said.contains("từ đầu")
}

/// How long a refused `redo_setup` with no step stays the last one: a
/// second within it is the whole walk ([`refused_again`]).
const REFUSED_FOR: std::time::Duration = std::time::Duration::from_secs(120);

/// A `redo_setup` with no step known now (#8871): `true` when one was
/// refused within [`REFUSED_FOR`], so it is never refused twice in a row;
/// `false` the first time, kept for the next.
pub(crate) fn refused_again() -> bool {
    let again = |last: &mut Option<std::time::Instant>| {
        let now = std::time::Instant::now();
        if last.is_some_and(|at| now.duration_since(at) <= REFUSED_FOR) {
            *last = None;
            return true;
        }
        *last = Some(now);
        false
    };
    // Each test's thread its own; the worker's process one.
    #[cfg(test)]
    {
        thread_local! {
            static LAST: std::cell::Cell<Option<std::time::Instant>> = const { std::cell::Cell::new(None) };
        }
        LAST.with(|cell| {
            let mut last = cell.get();
            let told = again(&mut last);
            cell.set(last);
            told
        })
    }
    #[cfg(not(test))]
    {
        static LAST: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
        again(&mut LAST.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
    }
}

/// The reminder said once after a quiet ask (#8871: silence for 10 s, the
/// ask again more simply; for "make a skill", other simple ideas).
pub(crate) fn remind(step: Step) -> Option<&'static str> {
    match step {
        Step::TrySkill => Some(TRY_REMIND),
        Step::MakeSkill => Some(MAKE_IDEAS),
        _ => None,
    }
}

/// The light functions whose call from the person's line tries a skill:
/// lamp-actions' (`light_colour`, `light_on`, ...) and the light scenes.
pub(crate) fn is_light_call(name: &str) -> bool {
    name.starts_with("light_")
}

/// The most bytes of a made skill's name and description read back.
const MOST_NAME: usize = 64;
const MOST_DOES: usize = 240;

/// `text` cut to `most` bytes at a character, with no `"` (a fixed line
/// can't hold one) and no line breaks.
fn clean(text: &str, most: usize) -> String {
    let text: String = text
        .chars()
        .map(|c| if c == '"' || c.is_control() { ' ' } else { c })
        .collect();
    let mut text = text.trim().to_owned();
    while text.len() > most {
        text.pop();
    }
    text.trim().to_owned()
}

/// The read-back's `{skill}`, one sentence: the name and what it does
/// ("drink water: it reminds you to drink water every 30 minutes" in
/// English; elsewhere "drink water: ...", which no English word joins).
pub(crate) fn read_back(name: &str, does: &str, language: Option<Language>) -> Option<String> {
    let name = clean(&crate::made_skill::spoken_name(name), MOST_NAME);
    let does = clean(does, MOST_DOES);
    let does = does.trim_end_matches(['.', '!', '?']).trim();
    if name.is_empty() {
        return None;
    }
    let skill = match (does.is_empty(), language) {
        (true, _) => name,
        (false, None) => format!("{name}: it {}", crate::made_skill::lowered_first(does)),
        (false, Some(_)) => format!("{name}: {does}"),
    };
    Some(MAKE_DONE.replace("{skill}", &skill))
}

const DONE_OPENING: &str = "(Say only this, nothing before or after it: \"All set! ";
const STORE: &str = "Thanks for your patience. Find more skills in the Store, in the app.";
#[cfg(test)]
const DONE_STORE: &str = "(Say only this, nothing before or after it: \"All set! Thanks for your patience. Find more skills in the Store, in the app.\")";

/// The most characters of a name the done line says (#8871: the name only
/// if it is short to say), one word.
const MOST_NAME_SAID: usize = 10;

/// Whether `name` is short enough for the done line.
pub(super) fn short_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && name.chars().count() <= MOST_NAME_SAID
        && !name.contains(char::is_whitespace)
}

/// The Store sentence in `language`.
fn store_sentence(language: Option<Language>) -> &'static str {
    match language {
        None => STORE,
        Some(Language::Vietnamese) => {
            "Cảm ơn bạn đã kiên nhẫn. Mở app để tìm thêm kỹ năng trong Store nhé."
        }
        Some(Language::Japanese) => {
            "お付き合いありがとうございました。アプリのStoreで、もっとスキルを見つけてください。"
        }
        Some(Language::Chinese) => "谢谢你的耐心。在 App 的 Store 里找更多技能吧。",
        Some(Language::Korean) => "기다려 줘서 고마워요. 앱의 Store에서 스킬을 더 찾아보세요.",
        Some(Language::French) => {
            "Merci de votre patience. Trouvez d'autres compétences dans le Store, dans l'app."
        }
    }
}

/// The walk's done line (#8871): "All set! Find more skills in the Store,
/// in the app.", kept in English, the name put in when it is said and short
/// (`in_language::with_name`).
pub(crate) fn done_line() -> String {
    format!("{DONE_OPENING}{STORE}\")")
}

/// The English done line's rest (`rest`, after "All set! "), said in
/// `language`: none for words no done line says.
pub(super) fn done_rest_in(rest: &str, language: Language) -> Option<String> {
    (rest == STORE).then(|| store_sentence(Some(language)).to_owned())
}

/// "All set!" in `language`, with the name when the walk knows it.
pub(super) fn all_set(language: Language, name: Option<&str>) -> String {
    match (language, name) {
        (Language::Vietnamese, None) => "Xong rồi! ".into(),
        (Language::Vietnamese, Some(name)) => format!("Xong rồi, {name}! "),
        (Language::Japanese, None) => "準備完了です！".into(),
        (Language::Japanese, Some(name)) => format!("{name}さん、準備完了です！"),
        (Language::Chinese, None) => "都好了！".into(),
        (Language::Chinese, Some(name)) => format!("{name}，都好了！"),
        (Language::Korean, None) => "다 됐어요! ".into(),
        (Language::Korean, Some(name)) => format!("{name}님, 다 됐어요! "),
        (Language::French, None) => "C'est prêt ! ".into(),
        (Language::French, Some(name)) => format!("C'est prêt, {name} ! "),
    }
}

/// A step's name as the person hears it (the owner's, #8871: "Call Lamp"
/// for how you sound; the others as their step).
fn step_name(step: Step, language: Option<Language>) -> &'static str {
    let names: [&str; 6] = match language {
        None => [
            "Where you sit",
            "Who you are",
            "Touch",
            "Call Lamp",
            "Try a skill",
            "Make a skill",
        ],
        Some(Language::Vietnamese) => [
            "Chỗ bạn ngồi",
            "Bạn là ai",
            "Chạm",
            "Gọi Lamp",
            "Thử một kỹ năng",
            "Tạo một kỹ năng",
        ],
        Some(Language::Japanese) => [
            "座る場所",
            "あなたのこと",
            "タッチ",
            "Lampを呼ぶ",
            "スキルを試す",
            "スキルを作る",
        ],
        Some(Language::Chinese) => [
            "你坐的位置",
            "你是谁",
            "触摸",
            "呼叫 Lamp",
            "试一个技能",
            "做一个技能",
        ],
        Some(Language::Korean) => [
            "앉는 자리",
            "당신은 누구",
            "터치",
            "Lamp 부르기",
            "스킬 써 보기",
            "스킬 만들기",
        ],
        Some(Language::French) => [
            "Votre place",
            "Qui vous êtes",
            "Toucher",
            "Appeler Lamp",
            "Essayer une compétence",
            "Créer une compétence",
        ],
    };
    match step {
        Step::WhereYouSit => names[0],
        Step::WhoYouAre => names[1],
        Step::Touch => names[2],
        Step::HowYouSound => names[3],
        Step::TrySkill => names[4],
        Step::MakeSkill => names[5],
        // Never in the walk with this switch: the plain id.
        other => other.name(),
    }
}

/// What to do now in a step, one sentence, for the state lines.
fn step_ask(step: Step, language: Option<Language>) -> &'static str {
    let asks: [&str; 6] = match language {
        None => [
            "sit where you usually do and look at me.",
            "tell me what to call you.",
            "pet my head.",
            "say 'Hey Lamp' when I stop talking.",
            "say 'Hey Lamp, turn blue.'",
            "say 'remind me to drink water every 30 minutes'.",
        ],
        Some(Language::Vietnamese) => [
            "ngồi chỗ quen thuộc và nhìn mình.",
            "cho mình biết nên gọi bạn là gì.",
            "vuốt đầu mình.",
            "nói 'Hey Lamp' khi mình nói xong.",
            "nói 'đổi sang màu xanh dương'.",
            "nói 'nhắc tôi uống nước mỗi 30 phút'.",
        ],
        Some(Language::Japanese) => [
            "いつも座る場所に座って、私の方を見てください。",
            "なんとお呼びすればいいか教えてください。",
            "私の頭をなでてください。",
            "私が話し終えたら「Hey Lamp」と言ってください。",
            "「青にして」と言ってください。",
            "「30分ごとに水を飲むよう知らせて」と言ってください。",
        ],
        Some(Language::Chinese) => [
            "坐在你平时坐的地方，看着我。",
            "告诉我该怎么称呼你。",
            "摸摸我的头。",
            "等我说完，说'Hey Lamp'。",
            "说'变成蓝色'。",
            "说'每 30 分钟提醒我喝水'。",
        ],
        Some(Language::Korean) => [
            "평소 앉는 자리에 앉아 저를 바라봐 주세요.",
            "어떻게 불러 드리면 될지 알려 주세요.",
            "제 머리를 쓰다듬어 주세요.",
            "제 말이 끝나면 'Hey Lamp'라고 말해 주세요.",
            "'파란색으로 바꿔 줘'라고 말해 주세요.",
            "'30분마다 물 마시라고 알려 줘'라고 말해 주세요.",
        ],
        Some(Language::French) => [
            "asseyez-vous là où vous vous asseyez d'habitude, et regardez-moi.",
            "dites-moi comment vous appeler.",
            "caressez-moi la tête.",
            "dites « Hey Lamp » quand j'ai fini de parler.",
            "dites « passe au bleu ».",
            "dites « rappelle-moi de boire de l'eau toutes les 30 minutes ».",
        ],
    };
    match step {
        Step::WhereYouSit => asks[0],
        Step::WhoYouAre => asks[1],
        Step::Touch => asks[2],
        Step::HowYouSound => asks[3],
        Step::TrySkill => asks[4],
        Step::MakeSkill => asks[5],
        _ => "",
    }
}

const EN_COUNT: [&str; 9] = [
    "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
];

/// `count` as an English word, its first letter as `upper` says.
fn english(count: usize, upper: bool) -> String {
    let word = EN_COUNT
        .get(count.wrapping_sub(1))
        .map_or_else(|| count.to_string(), |word| (*word).to_owned());
    if upper {
        let mut chars = word.chars();
        chars
            .next()
            .map(|first| first.to_uppercase().chain(chars).collect())
            .unwrap_or_default()
    } else {
        word
    }
}

/// About how long `steps` more take: 30 s each, a minute at least.
pub(crate) fn minutes(steps: usize) -> usize {
    steps.div_ceil(2).max(1)
}

/// What a person may ask of where the walk stands (#8871).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Asked {
    /// "Which step are we on?", "What were we doing?", "Where are we?"
    Which,
    /// "How many more?", "How long?"
    HowMany,
    /// "Is setup done?", "Are we finished?"
    Done,
}

/// The three state lines, the walk's own fixed lines from its real state,
/// said as they are. Sent to the second route's worker with the walk's
/// state (`ProviderInput::WalkUnderWay`'s `state`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateLines {
    pub which: String,
    pub how_many: String,
    pub done: String,
    /// The step's ask waits for the person's answer now: the walk's own
    /// conversation (#8871, the first builder's change 3), so their plain
    /// answer counts with no name and no face turned for as long as it
    /// waits. False while the walk is paused, left or between asks.
    #[serde(default)]
    pub waits: bool,
}

/// Whether a person's line now is an answer to the walk's waiting ask
/// (#8871): with the switch on, while [`StateLines::waits`], and not
/// another voice than the conversation's (a room voice is no answer).
pub(crate) fn holds_answer(other_voice: bool) -> bool {
    !other_voice && on() && state().is_some_and(|lines| lines.waits)
}

/// The state lines for `step`, at place `at` of `of` (Wi-Fi counted as the
/// first), in `language`.
pub(crate) fn state_lines(
    step: Step,
    at: usize,
    of: usize,
    language: Option<Language>,
) -> StateLines {
    let name = step_name(step, language);
    let ask = step_ask(step, language);
    let left = of.saturating_sub(at) + 1;
    let mins = minutes(left);
    let (which, how_many, done) = match language {
        None => {
            let how_long = if mins == 1 {
                "about a minute".to_owned()
            } else {
                format!("about {} minutes", english(mins, false))
            };
            let how_many = if left == 1 {
                format!("Just this one, about a minute. Right now, {name}: {ask}")
            } else {
                format!(
                    "{} more, {how_long}. Right now, {name}: {ask}",
                    english(left, true)
                )
            };
            (
                format!("We're on step {at} of {of}, {name}: {ask}"),
                how_many,
                format!("Almost! Wi-Fi's done, and we're at step {at} of {of}, {name}: {ask}"),
            )
        }
        Some(Language::Vietnamese) => (
            format!("Mình đang ở bước {at} trên {of}, {name}: {ask}"),
            format!("Còn {left} bước nữa, khoảng {mins} phút. Bây giờ là {name}: {ask}"),
            format!("Gần xong rồi! Wi-Fi đã xong, giờ mình ở bước {at} trên {of}, {name}: {ask}"),
        ),
        Some(Language::Japanese) => (
            format!("いまはステップ{at}/{of}、{name}です。{ask}"),
            format!("あと{left}ステップ、{mins}分ほどです。いまは{name}：{ask}"),
            format!("もう少しです！Wi-Fiは完了、いまはステップ{at}/{of}、{name}です。{ask}"),
        ),
        Some(Language::Chinese) => (
            format!("我们在第{at}步，共{of}步，{name}：{ask}"),
            format!("还有{left}步，大约{mins}分钟。现在是{name}：{ask}"),
            format!("快好了！Wi-Fi 已完成，我们在第{at}步，共{of}步，{name}：{ask}"),
        ),
        Some(Language::Korean) => (
            format!("지금 {of}단계 중 {at}단계예요: {name}. {ask}"),
            format!("{left}단계 남았어요, {mins}분 정도요. 지금은 {name}: {ask}"),
            format!("거의 다 됐어요! Wi-Fi는 끝났고, 지금 {of}단계 중 {at}단계예요: {name}. {ask}"),
        ),
        Some(Language::French) => {
            let steps = if left == 1 { "étape" } else { "étapes" };
            let minute = if mins == 1 { "minute" } else { "minutes" };
            (
                format!("Nous en sommes à l'étape {at} sur {of}, {name} : {ask}"),
                format!(
                    "Encore {left} {steps}, environ {mins} {minute}. Pour l'instant, {name} : {ask}"
                ),
                format!(
                    "Presque ! Le Wi-Fi est fait, et nous en sommes à l'étape {at} sur {of}, {name} : {ask}"
                ),
            )
        }
    };
    let own = |words: String| super::said_as_is(&words).unwrap_or_default();
    StateLines {
        which: own(which),
        how_many: own(how_many),
        done: own(done),
        waits: false,
    }
}

impl StateLines {
    /// The line for `asked`, as a fixed own line.
    pub(crate) fn line(&self, asked: Asked) -> &str {
        match asked {
            Asked::Which => &self.which,
            Asked::HowMany => &self.how_many,
            Asked::Done => &self.done,
        }
    }
}

/// Lowercased words with no punctuation, joined by one space, padded so a
/// phrase matches only whole words.
fn plain(text: &str) -> String {
    let words: Vec<String> = text
        .to_lowercase()
        .replace(['\u{2019}', '`'], "'")
        .split(|c: char| c.is_whitespace() || (c.is_ascii_punctuation() && c != '\''))
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect();
    format!(" {} ", words.join(" "))
}

const WHICH: [&str; 9] = [
    "which step",
    "what step",
    "where are we at",
    "where are we now",
    "where were we",
    "what were we doing",
    "what are we doing now",
    "bước mấy",
    "bước nào",
];
/// Asked alone, the whole line: "Where are we?" (not "where are we going").
const WHICH_ALONE: [&str; 2] = ["where are we", "what are we doing"];
const HOW_MANY: [&str; 8] = [
    "how many more",
    "how many steps",
    "how many left",
    "how much longer",
    "how long will this",
    "how long is this",
    "còn mấy bước",
    "còn bao lâu",
];
const DONE: [&str; 10] = [
    "is setup done",
    "is the setup done",
    "is onboarding done",
    "is the onboarding done",
    "is setup finished",
    "are we done",
    "are we finished",
    "is it finished",
    "xong chưa",
    "đã xong chưa",
];

/// What `text` asks of where the walk stands, if it asks that: a few plain
/// phrases (#8871), in English and Vietnamese. Anything else is the model's.
pub(crate) fn state_asked(text: &str) -> Option<Asked> {
    let text = plain(text);
    let has = |phrases: &[&str]| {
        phrases
            .iter()
            .any(|phrase| text.contains(&format!(" {phrase} ")))
    };
    if has(&HOW_MANY) {
        Some(Asked::HowMany)
    } else if has(&DONE) {
        Some(Asked::Done)
    } else if has(&WHICH)
        || WHICH_ALONE
            .iter()
            .any(|phrase| text == format!(" {phrase} "))
    {
        Some(Asked::Which)
    } else {
        None
    }
}

/// The state lines as the walk set them last, in this process: the
/// coordinator's, told to the second route's worker with each line.
#[cfg(not(test))]
static STATE: std::sync::Mutex<Option<StateLines>> = std::sync::Mutex::new(None);
#[cfg(test)]
thread_local! {
    static STATE: std::cell::RefCell<Option<StateLines>> = const { std::cell::RefCell::new(None) };
}

pub(crate) fn set_state(lines: Option<StateLines>) {
    #[cfg(not(test))]
    if let Ok(mut state) = STATE.lock() {
        *state = lines;
    }
    #[cfg(test)]
    STATE.with(|state| *state.borrow_mut() = lines);
}

pub(crate) fn state() -> Option<StateLines> {
    #[cfg(not(test))]
    return STATE.lock().ok()?.clone();
    #[cfg(test)]
    STATE.with(|state| state.borrow().clone())
}

/// The words a person's line `said` is answered with while the walk is
/// under way, when it asks where the walk stands: the walk's own line, said
/// as it is, no chat asked.
pub(crate) fn state_words(said: &str) -> Option<String> {
    let asked = state_asked(said)?;
    let lines = state()?;
    super::own_words(lines.line(asked)).map(str::to_owned)
}

/// The step the walk leads now, as its file at `file` shows: the first of
/// `order` with no outcome; or, past the walk, a `later` step offered again
/// in the last 10 minutes. None without the file, the order or the switch.
pub(crate) fn led_now(file: &Path, order: Option<&[Step]>) -> Option<Step> {
    let order = order?;
    let progress: Value = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
    let steps = progress["steps"].as_object()?;
    if let Some(step) = order.iter().find(|step| !steps.contains_key(step.name())) {
        return Some(*step);
    }
    let now_s = super::unix_ms() / 1_000;
    order.iter().copied().find(|step| {
        let kept = &steps[step.name()];
        kept["later"] == true
            && kept["later_offers"]
                .as_u64()
                .is_some_and(|offers| offers > 0)
            && kept["offered_at_unix_s"]
                .as_u64()
                .is_some_and(|at| now_s.saturating_sub(at) < 600)
    })
}

/// A call the provider worker ran (#8871), with the switch on: a light call
/// or a skill made and put on, from the person's own line (`heard`), is the
/// new steps' evidence, posted to the runner's mailbox, which takes it only
/// while it leads that step. While the walk leads "make a skill", the
/// make's answer to the model is changed: a skill made is read back by the
/// walk itself, and one refused gets one simpler example.
pub(crate) fn tool_ran(
    file: &Path,
    order: Option<&[Step]>,
    name: &str,
    heard: bool,
    told: &mut Value,
) {
    if !heard || told.get("error").is_some() && name != crate::made_skill::MAKE {
        return;
    }
    if is_light_call(name) && told.get("error").is_none() {
        let line = json!({"step": TRY_SKILL, "outcome": "done", "by": name});
        if let Err(error) = super::post(file, &line) {
            eprintln!(
                "lamp-voice: onboarding: a light call, not noted for \"try a skill\": {error}"
            );
        }
        return;
    }
    if name != crate::made_skill::MAKE {
        return;
    }
    let leading = led_now(file, order) == Some(Step::MakeSkill);
    if let Some(made) = told.get("made").and_then(Value::as_str) {
        if told["on"] != true {
            return;
        }
        let line = json!({
            "step": MAKE_SKILL,
            "outcome": "done",
            "by": crate::made_skill::MAKE,
            "made": clean(made, MOST_NAME),
            "does": clean(told["description"].as_str().unwrap_or_default(), MOST_DOES),
        });
        match super::post(file, &line) {
            Ok(()) if leading => told["next"] = json!(MAKE_READ_BACK_NEXT),
            Ok(()) => {}
            Err(error) => {
                eprintln!(
                    "lamp-voice: onboarding: a skill made, not noted for \"make a skill\": {error}"
                );
            }
        }
    } else if leading && told.get("error").is_some() && told.get("next").is_some() {
        told["next"] = json!(MAKE_SIMPLER_NEXT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_steps_come_after_how_you_sound_once() {
        let order = [
            Step::WhereYouSit,
            Step::WhoYouAre,
            Step::Touch,
            Step::HowYouSound,
        ];
        let walk = with_steps(&order);
        assert_eq!(
            walk,
            [
                Step::WhereYouSit,
                Step::WhoYouAre,
                Step::Touch,
                Step::HowYouSound,
                Step::TrySkill,
                Step::MakeSkill
            ]
        );
        assert_eq!(with_steps(&walk), walk);
        assert_eq!(
            with_steps(&[Step::WhereYouSit, Step::WhoYouAre]),
            [
                Step::WhereYouSit,
                Step::WhoYouAre,
                Step::TrySkill,
                Step::MakeSkill
            ]
        );
    }

    #[test]
    fn the_done_line_is_short_with_a_short_name_only() {
        assert_eq!(done_line(), DONE_STORE);
        assert_eq!(
            super::super::in_language::with_name(&done_line(), Some("Mia")),
            "(Say only this, nothing before or after it: \"All set, Mia! Thanks for your patience. Find more skills in the Store, in the app.\")"
        );
        for long in ["Maximiliana", "Anna Maria"] {
            assert_eq!(
                super::super::in_language::with_name(&done_line(), Some(long)),
                DONE_STORE
            );
        }
        for language in [
            Language::Vietnamese,
            Language::Japanese,
            Language::Chinese,
            Language::Korean,
            Language::French,
        ] {
            let said = done_rest_in(STORE, language).unwrap();
            assert_ne!(said, STORE);
            assert!(!said.contains('"'), "{said}");
        }
    }

    #[test]
    fn the_state_lines_count_wifi_as_the_first_of_seven() {
        let lines = state_lines(Step::HowYouSound, 5, 7, None);
        assert_eq!(
            super::super::own_words(&lines.which),
            Some("We're on step 5 of 7, Call Lamp: say 'Hey Lamp' when I stop talking.")
        );
        assert_eq!(
            super::super::own_words(&lines.how_many),
            Some(
                "Three more, about two minutes. Right now, Call Lamp: say 'Hey Lamp' when I stop talking."
            )
        );
        assert_eq!(
            super::super::own_words(&lines.done),
            Some(
                "Almost! Wi-Fi's done, and we're at step 5 of 7, Call Lamp: say 'Hey Lamp' when I stop talking."
            )
        );
        let last = state_lines(Step::MakeSkill, 7, 7, None);
        assert!(last.how_many.contains("Just this one, about a minute."));
        for language in [
            Language::Vietnamese,
            Language::Japanese,
            Language::Chinese,
            Language::Korean,
            Language::French,
        ] {
            for step in [
                Step::WhereYouSit,
                Step::WhoYouAre,
                Step::Touch,
                Step::HowYouSound,
                Step::TrySkill,
                Step::MakeSkill,
            ] {
                let lines = state_lines(step, 3, 7, Some(language));
                for line in [&lines.which, &lines.how_many, &lines.done] {
                    let words = super::super::own_words(line).expect("a fixed own line");
                    assert!(!words.contains("Call Lamp"), "{words}");
                    assert!(words.len() <= lamp_gemini::wire::MAX_NOTE_BYTES);
                }
            }
        }
    }

    #[test]
    fn a_state_question_is_read_from_plain_phrases_only() {
        assert_eq!(state_asked("Which step are we on?"), Some(Asked::Which));
        assert_eq!(state_asked("Wait, what were we doing?"), Some(Asked::Which));
        assert_eq!(state_asked("How many more?"), Some(Asked::HowMany));
        assert_eq!(state_asked("how much longer"), Some(Asked::HowMany));
        assert_eq!(state_asked("Is setup done?"), Some(Asked::Done));
        assert_eq!(state_asked("Are we finished?"), Some(Asked::Done));
        assert_eq!(state_asked("Còn mấy bước?"), Some(Asked::HowMany));
        assert_eq!(state_asked("Turn blue."), None);
        assert_eq!(state_asked("I'm done."), None);
        assert_eq!(state_asked("Where are we?"), Some(Asked::Which));
        assert_eq!(state_asked("Where are we going for lunch?"), None);
        assert_eq!(state_asked("Remind me how long the pasta cooks"), None);
    }

    #[test]
    fn a_state_question_is_answered_with_the_walks_own_line_only_while_it_is_set() {
        set_state(None);
        assert_eq!(state_words("Which step are we on?"), None);
        set_state(Some(state_lines(Step::Touch, 4, 7, None)));
        assert_eq!(
            state_words("Which step are we on?").as_deref(),
            Some("We're on step 4 of 7, Touch: pet my head.")
        );
        assert_eq!(state_words("What's the weather?"), None);
        set_state(None);
    }

    /// lamp-9adf at 16443: "redo the whole setup, from the start" was
    /// refused twice. Words for the whole walk are it, and a second ask
    /// with no step is never refused.
    #[test]
    fn the_whole_setup_is_redone_by_its_words_and_never_refused_twice() {
        for said in ["whole", "the whole setup", "all steps", "everything", "from the start", "setup", "lại từ đầu"] {
            assert!(whole_walk(said), "{said}");
        }
        for said in ["touch", "who_you_are", "how you sound"] {
            assert!(!whole_walk(said), "{said}");
        }
        assert!(!refused_again());
        assert!(refused_again());
        assert!(!refused_again());
    }

    /// The first builder's change 3: while the walk's ask waits, the
    /// person's plain answer is taken, however late; not another voice's,
    /// not while paused, not with the switch off.
    #[test]
    fn a_waiting_ask_holds_the_answer_window_open() {
        set_on_for_test(true);
        let waiting = StateLines {
            waits: true,
            ..state_lines(Step::WhoYouAre, 3, 7, None)
        };
        set_state(Some(waiting.clone()));
        assert!(holds_answer(false));
        assert!(!holds_answer(true), "a room voice is no answer");
        set_state(Some(StateLines {
            waits: false,
            ..waiting.clone()
        }));
        assert!(!holds_answer(false), "paused or between asks");
        set_state(Some(waiting));
        set_on_for_test(false);
        assert!(!holds_answer(false), "switch off");
        set_state(None);
    }

    #[test]
    fn the_read_back_names_the_skill_and_what_it_does() {
        assert_eq!(
            read_back(
                "drink-water",
                "Reminds you to drink water every 30 minutes.",
                None
            )
            .as_deref(),
            Some(
                "(Say only this, nothing before or after it: \"I made drink water: it reminds you to drink water every 30 minutes. Change it any time in the app.\")"
            )
        );
        let quoted = read_back("say \"hi\"", "Says \"hi\".", None).unwrap();
        assert!(super::super::fixed_words(&quoted).is_some(), "{quoted}");
        assert_eq!(read_back("", "Does it.", None), None);
    }

    /// A progress file with `steps`, in a folder of its own for `name`.
    fn walk_file(name: &str, steps: Value) -> ((), std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "walk-v2-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("progress.json");
        std::fs::write(&file, json!({ "steps": steps }).to_string()).unwrap();
        ((), file)
    }

    const ORDER: [Step; 6] = [
        Step::WhereYouSit,
        Step::WhoYouAre,
        Step::Touch,
        Step::HowYouSound,
        Step::TrySkill,
        Step::MakeSkill,
    ];

    #[test]
    fn only_a_heard_light_call_or_a_skill_put_on_is_evidence() {
        let ended = json!({"outcome":"done","at_unix_s":1});
        let (_dir, file) = walk_file(
            "evidence",
            json!({
                "where_you_sit": ended, "who_you_are": ended, "touch": ended,
                "how_you_sound": ended, "try_skill": ended
            }),
        );
        let mailbox = super::super::mailbox_of(&file);
        let read = || std::fs::read_to_string(&mailbox).unwrap_or_default();
        // A lamp's own turn, and a light call that failed: nothing.
        tool_ran(
            &file,
            Some(&ORDER),
            "light_colour",
            false,
            &mut json!({"ok":true}),
        );
        tool_ran(
            &file,
            Some(&ORDER),
            "light_colour",
            true,
            &mut json!({"error":"no"}),
        );
        assert!(read().is_empty());
        tool_ran(
            &file,
            Some(&ORDER),
            "light_colour",
            true,
            &mut json!({"ok":true}),
        );
        assert!(read().contains("\"by\":\"light_colour\""), "{}", read());
        // A make: noted, and read back by the walk while it leads it.
        let mut made = json!({"made":"drink-water","on":true,
            "description":"Reminds you to drink water.","next":"I made drink water."});
        tool_ran(
            &file,
            Some(&ORDER),
            crate::made_skill::MAKE,
            true,
            &mut made,
        );
        assert_eq!(made["next"], MAKE_READ_BACK_NEXT);
        assert!(read().contains("\"made\":\"drink-water\""), "{}", read());
        // Made but not on: not done.
        let before = read();
        let mut off = json!({"made":"x","on":false,"next":"I made it but couldn't put it on."});
        tool_ran(&file, Some(&ORDER), crate::made_skill::MAKE, true, &mut off);
        assert_eq!(read(), before);
        assert_ne!(off["next"], MAKE_READ_BACK_NEXT);
        // Refused while led: one simpler example.
        let mut refused = json!({"error":"not made: too big","next":"Tell them why."});
        tool_ran(
            &file,
            Some(&ORDER),
            crate::made_skill::MAKE,
            true,
            &mut refused,
        );
        assert_eq!(refused["next"], MAKE_SIMPLER_NEXT);
    }

    #[test]
    fn a_make_outside_the_step_keeps_its_own_words() {
        let ended = json!({"outcome":"done","at_unix_s":1});
        let (_dir, file) = walk_file(
            "outside",
            json!({
                "where_you_sit": ended, "who_you_are": ended, "touch": ended,
                "how_you_sound": ended, "try_skill": ended, "make_skill": ended
            }),
        );
        let mut made = json!({"made":"drink-water","on":true,"next":"I made drink water."});
        tool_ran(
            &file,
            Some(&ORDER),
            crate::made_skill::MAKE,
            true,
            &mut made,
        );
        assert_eq!(made["next"], "I made drink water.");
        let mut refused = json!({"error":"no","next":"Tell them why."});
        tool_ran(
            &file,
            Some(&ORDER),
            crate::made_skill::MAKE,
            true,
            &mut refused,
        );
        assert_eq!(refused["next"], "Tell them why.");
    }
}
