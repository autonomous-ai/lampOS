//! The owner's language (#4473 (a), the voice side): English, Vietnamese,
//! Japanese, Chinese or Korean, from one line in the file [`FILE_ENVIRONMENT`] names
//! (the product's `/etc/lampOS/setup/language`). A missing file, an empty
//! one, `en` or a value this build does not know (said in the journal)
//! is English, and nothing below changes: the speech-to-text's language, the
//! character, the voice and its model stay as they are today.
//!
//! For `vi`, `ja`, `zh` or `ko`:
//! - speech to text is told the language (over `stt_language`), and Flux,
//!   English only, is left for Nova-3 by its own rule;
//! - the character's "Always speak English" becomes the chosen language's
//!   ([`instruction`]); the call name stays "Lamp";
//! - ElevenLabs speaks with [`SPEECH_MODEL`], which speaks all four, in the
//!   language's voice from V1's table ([`Language::voice`]); Korean keeps the
//!   voice's own model, English's, in a voice of its own (Kanna,
//!   [`Language::speech_model`]);
//! - the English-only gates follow it: a cut-in with no English word is no
//!   fragment, a turn heard in another script is asked again (#926) only in
//!   a Latin-script language (Vietnamese), and the stop stream listens only
//!   in a language it has words for (Vietnamese, #2939), in that language;
//! - at start, a language other than the last one announced is said once in
//!   its own words ([`announce`]), and recorded in [`SAID_PATH`].
//!
//! Read at every voice start: the processes read it as they start, so a
//! change applies at the voice's next start.
use std::{ffi::OsString, fs, io::Read, path::Path};

/// The absolute path of the language file, and nothing else.
pub const FILE_ENVIRONMENT: &str = "LAMP_LANGUAGE_FILE";

/// The language last announced, one code a line; absent is English.
pub const SAID_PATH: &str = "/var/lib/lampOS/voice/language-said";

/// The ElevenLabs model for Vietnamese, Japanese and Chinese
/// (`docs/second-voice.md`, "The owner's language").
pub const SPEECH_MODEL: &str = "eleven_flash_v2_5";

/// The longest file taken: a code and a newline, with room to spare.
const FILE_MOST: u64 = 64;

/// The character's own rule this replaces, in both built-in characters and
/// `deploy/character/lamp.txt`.
const ENGLISH_RULE: &str = "Always speak English";

/// The words said when the lamp goes back to English.
pub const ENGLISH_CHANGED: &str = "Okay, I'll speak English from now on.";

/// A language other than English.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Vietnamese,
    Japanese,
    Chinese,
    Korean,
}

impl Language {
    /// The setting's text: `None` for English (`en`, empty), or why not.
    pub fn parse(text: &str) -> Result<Option<Self>, String> {
        match text.trim() {
            "" | "en" => Ok(None),
            "vi" => Ok(Some(Self::Vietnamese)),
            "ja" => Ok(Some(Self::Japanese)),
            "zh" => Ok(Some(Self::Chinese)),
            "ko" => Ok(Some(Self::Korean)),
            other => Err(format!(
                "the language {:?} is not one this lamp speaks (en, vi, ja, zh, ko): English",
                other.chars().take(16).collect::<String>()
            )),
        }
    }

    /// The setting's own code.
    pub fn code(self) -> &'static str {
        match self {
            Self::Vietnamese => "vi",
            Self::Japanese => "ja",
            Self::Chinese => "zh",
            Self::Korean => "ko",
        }
    }

    /// Deepgram's `language` for it.
    pub fn stt(self) -> &'static str {
        self.code()
    }

    /// The character's rule, as [`ENGLISH_RULE`] is worded.
    fn rule(self) -> &'static str {
        match self {
            Self::Vietnamese => "Always speak Vietnamese",
            Self::Japanese => "Always speak Japanese",
            Self::Chinese => "Always speak Mandarin Chinese, in simplified characters",
            Self::Korean => "Always speak Korean",
        }
    }

    /// Its voice, by its name in `net::tts`'s table: V1's first
    /// (`system/domain/device.go`'s `elevenLabsVoicesByLang`) for vi, ja and
    /// zh. V1 had no Korean voice: Kanna is a Korean native speaker's pick
    /// from a listening test (#4473, 30 Oct).
    pub fn voice(self) -> &'static str {
        match self {
            Self::Vietnamese => "Ngan",
            Self::Japanese => "Shizuka",
            Self::Chinese => "Amy",
            Self::Korean => "Kanna",
        }
    }

    /// The ElevenLabs model it is spoken with: [`SPEECH_MODEL`], or `None`
    /// to keep the voice's own, English's (the product's
    /// `eleven_v4_turbo`), with the speech tags English gets. Korean keeps
    /// it: [`SPEECH_MODEL`] drops the pitch at the end of a Korean sentence,
    /// and v4 ends it naturally (the same listening test).
    pub fn speech_model(self) -> Option<&'static str> {
        (self != Self::Korean).then_some(SPEECH_MODEL)
    }

    /// Whether the stop stream has words for it: #2939's Vietnamese
    /// backchannels. Without them it is not listened to.
    pub fn has_stop_words(self) -> bool {
        self == Self::Vietnamese
    }

    /// Whether it is written in the Latin script, as English is: #926's
    /// check for a turn heard in another script still holds for it.
    pub fn latin_script(self) -> bool {
        self == Self::Vietnamese
    }

    /// Said once when the lamp starts speaking it.
    pub fn changed(self) -> &'static str {
        match self {
            Self::Vietnamese => "Từ giờ mình sẽ nói tiếng Việt nhé.",
            Self::Japanese => "これからは日本語で話しますね。",
            Self::Chinese => "好的，从现在开始我说中文。",
            Self::Korean => "이제부터 한국어로 말할게요.",
        }
    }
}

/// The product's failure notices' English words (`docs/failure-notices.md`,
/// "The product's recordings"): the keys of [`FIXED_LINES`] for the lines
/// the notices release holds. An English lamp plays the release's files,
/// never these.
pub const PROVIDER_UNAVAILABLE: &str =
    "Sorry, I can't reach the internet right now. I'll be back soon.";
pub const BACK_ONLINE: &str = "I'm back online.";
pub const VOICE_SERVICE_UNREACHABLE: &str =
    "Sorry, I'm having trouble thinking right now. Give me a moment.";
pub const WIFI_LOST: &str = "I can't reach any Wi-Fi. If you changed your Wi-Fi's name or password, push a paperclip into the hole marked reset on the back of my base and hold it for five seconds. That erases me and starts setup again.";
pub const WIFI_RECOVERING: &str =
    "I can't reach my Wi-Fi, so I've opened setup. Set my Wi-Fi again in the app.";

pub const SIGNED_OUT: &str = "I've been signed out. Please set me up again in the app.";

/// The lamp's fixed lines (#4473 (c)), by their English words, in
/// Vietnamese, Japanese, Chinese and Korean, in that order: said from the
/// speech cache in the owner's language, English where a line is not kept.
const FIXED_LINES: &[(&str, [&str; 4])] = &[
    (
        lamp_msgs::GOING_TO_SLEEP,
        [
            "Mình đi ngủ đây. Khi đèn chuyển sang màu xanh lá thì rút điện cho mình nhé.",
            "おやすみなさい。ライトが緑になったら、プラグを抜いてね。",
            "我要睡觉了。等灯变绿了，就可以拔掉电源了。",
            "이제 잘게요. 불이 초록색으로 바뀌면 플러그를 뽑아 주세요.",
        ],
    ),
    (
        lamp_msgs::SLEEP_LINE,
        [
            "Mình ngủ đây. Chạm vào đầu mình để đánh thức nhé.",
            "おやすみ。頭をタップしたら起きるよ。",
            "我去睡了。拍拍我的头就能叫醒我。",
            "잘게요. 머리를 톡 치면 깨어날게요.",
        ],
    ),
    (
        PROVIDER_UNAVAILABLE,
        [
            "Xin lỗi, giờ mình không kết nối được internet. Mình sẽ quay lại sớm thôi.",
            "ごめんね、今インターネットにつながらないんだ。すぐ戻るね。",
            "抱歉，我现在连不上网。很快就回来。",
            "미안해요, 지금 인터넷에 연결할 수 없어요. 곧 돌아올게요.",
        ],
    ),
    (
        BACK_ONLINE,
        [
            "Mình kết nối lại rồi nhé.",
            "またつながったよ。",
            "我又连上网了。",
            "다시 연결됐어요.",
        ],
    ),
    (
        VOICE_SERVICE_UNREACHABLE,
        [
            "Xin lỗi, mình đang hơi chậm một chút. Đợi mình một chút nhé.",
            "ごめんね、今うまく考えられないんだ。ちょっと待ってね。",
            "抱歉，我现在脑子有点转不过来。稍等我一下。",
            "미안해요, 지금 생각이 잘 안 나요. 잠깐만 기다려 주세요.",
        ],
    ),
    (
        WIFI_LOST,
        [
            "Mình không kết nối được Wi-Fi nào cả. Nếu bạn đã đổi tên hoặc mật khẩu Wi-Fi, hãy dùng kẹp giấy ấn vào lỗ có chữ reset ở mặt sau đế đèn và giữ năm giây. Mình sẽ được đưa về cài đặt ban đầu để bạn cài lại từ đầu.",
            "Wi-Fiにつながらないよ。Wi-Fiの名前かパスワードを変えたなら、台座の裏にある「reset」の穴にクリップを差し込んで、5秒間押し続けてね。データが消えて、最初から設定し直せるよ。",
            "我连不上任何Wi-Fi。如果你改了Wi-Fi的名称或密码，请用回形针按住我底座背面标着reset的小孔五秒钟。这样会清除我的数据，重新开始设置。",
            "와이파이에 연결할 수 없어요. 와이파이 이름이나 비밀번호를 바꿨다면, 받침 뒷면의 reset 구멍에 클립을 넣고 5초 동안 눌러 주세요. 그러면 제가 초기화되고 설정이 처음부터 다시 시작돼요.",
        ],
    ),
    (
        WIFI_RECOVERING,
        [
            "Mình không kết nối được Wi-Fi nên đã mở chế độ cài đặt. Bạn cài lại Wi-Fi cho mình trong ứng dụng nhé.",
            "Wi-Fiにつながらないから、設定モードにしたよ。アプリでWi-Fiをもう一度設定してね。",
            "我连不上Wi-Fi，已经打开了设置。请在App里重新帮我设置Wi-Fi。",
            "와이파이에 연결할 수 없어서 설정 모드를 열었어요. 앱에서 와이파이를 다시 설정해 주세요.",
        ],
    ),
    (
        SIGNED_OUT,
        [
            "Mình đã bị đăng xuất. Bạn cài đặt lại cho mình trong ứng dụng nhé.",
            "サインアウトされちゃった。アプリでもう一度設定してね。",
            "我已经退出登录了。请在App里重新帮我设置。",
            "로그아웃됐어요. 앱에서 다시 설정해 주세요.",
        ],
    ),
    // The second route's own words before a look and a search (Thirteen's
    // P2 on #4707): never English in a turn in the owner's language.
    (
        crate::second_provider::LOOKING,
        [
            "Để mình xem nào.",
            "ちょっと見てみるね。",
            "我看一下。",
            "잠깐 볼게요.",
        ],
    ),
    (
        crate::second_provider::CHECKING,
        [
            "Để mình kiểm tra nhé.",
            "ちょっと調べてみるね。",
            "我查一下。",
            "잠깐 확인해 볼게요.",
        ],
    ),
    // Its down lines (`LAMP_SECOND_DOWN_LINE`) and a look that never began
    // (`LAMP_SECOND_LOOK`), both on in the product: said in the owner's
    // language too, never English to an owner who chose another (#4473).
    (
        crate::second_provider::DOWN_LINE,
        [
            "Xin lỗi, giờ mình đang nghĩ hơi chậm. Lát nữa hỏi lại mình nhé.",
            "ごめんね、今ちょっと考えるのが遅くなってるんだ。少ししたらもう一度聞いてね。",
            "抱歉，我现在反应有点慢。过一会儿再问我吧。",
            "미안해요, 지금 생각이 좀 느려요. 조금 있다가 다시 물어봐 주세요.",
        ],
    ),
    (
        crate::second_provider::DOWN_LINE_AGAIN,
        [
            "Vẫn còn chậm, xin lỗi nhé.",
            "まだ遅いんだ、ごめんね。",
            "还是有点慢，抱歉。",
            "아직 느려요, 미안해요.",
        ],
    ),
    (
        crate::second_provider::DOWN_LINE_CANNOT,
        [
            "Xin lỗi, giờ mình chưa trả lời được.",
            "ごめんね、今は答えられないんだ。",
            "抱歉，我现在没法回答。",
            "미안해요, 지금은 대답할 수 없어요.",
        ],
    ),
    (
        crate::second_provider::COULD_NOT_LOOK,
        [
            "Mình không nhìn rõ được.",
            "よく見えなかったよ。",
            "我没看清楚。",
            "잘 안 보였어요.",
        ],
    ),
    // Its answer to a stop said over it (`LAMP_SPOKEN_STOP_ENDS`, on).
    (
        crate::spoken_stop::LINE,
        [
            "Được rồi, mình dừng ở đây nhé.",
            "わかった、ここでやめるね。",
            "好的，我就说到这儿。",
            "알겠어요, 여기서 멈출게요.",
        ],
    ),
    // What it says when a line by name gets no words twice
    // (`LAMP_NO_WORDS_AGAIN`, off in the product; Tests' P3 on #4723).
    (
        crate::second_provider::NO_WORDS_LINE,
        [
            "Xin lỗi, mình bị mất lời rồi. Bạn hỏi lại mình được không?",
            "ごめんね、言葉が出てこなかった。もう一度聞いてくれる？",
            "抱歉，我刚才一下子说不出话了。你能再问我一次吗？",
            "미안해요, 말이 막혔어요. 다시 물어봐 줄래요?",
        ],
    ),
    // The first stand's ask and its "here I go", sent to the say service as
    // text (Fourth's finding on #4710). In another language than English it
    // asks for the touch alone, never the spoken yes.
    (
        lamp_body::first_stand::ASK_TOUCH_ONLY,
        [
            "Mình muốn vươn vai lần đầu tiên. Khi nào bạn sẵn sàng thì đặt tay lên đầu mình nhé.",
            "はじめて体を伸ばしてみたいな。準備ができたら、頭に手をのせてね。",
            "我想第一次伸个懒腰。你准备好了，就把手放在我头上吧。",
            "처음으로 기지개를 켜 보고 싶어요. 준비되면 제 머리에 손을 올려 주세요.",
        ],
    ),
    (
        lamp_body::first_stand::GOING,
        [
            "Được rồi, mình bắt đầu đây.",
            "よし、いくよ。",
            "好，我开始啦。",
            "좋아요, 시작할게요.",
        ],
    ),
];

impl Language {
    /// Its place in each of [`FIXED_LINES`]' rows.
    fn column(self) -> usize {
        match self {
            Self::Vietnamese => 0,
            Self::Japanese => 1,
            Self::Chinese => 2,
            Self::Korean => 3,
        }
    }

    /// The fixed line whose English words are `english`, in this language;
    /// none for a line that is not one of [`FIXED_LINES`].
    pub fn fixed(self, english: &str) -> Option<&'static str> {
        FIXED_LINES
            .iter()
            .find(|(line, _)| *line == english)
            .map(|(_, words)| words[self.column()])
    }

    /// Every fixed line in this language, for the speech cache to warm.
    pub fn fixed_lines(self) -> impl Iterator<Item = &'static str> {
        FIXED_LINES
            .iter()
            .map(move |(_, words)| words[self.column()])
    }
}

/// `english`, a fixed line, as `language` says it: English as it is.
pub fn fixed_in(language: Option<Language>, english: &'static str) -> &'static str {
    language
        .and_then(|language| language.fixed(english))
        .unwrap_or(english)
}

/// The language in `text`; an unknown value is said and taken as English.
fn from_text(text: &str) -> Option<Language> {
    Language::parse(text).unwrap_or_else(|why| {
        eprintln!("lamp-voice: {why}");
        None
    })
}

/// The language the file at `path` holds; missing or unreadable is English.
pub fn read(path: &Path) -> Option<Language> {
    let mut text = String::new();
    match fs::File::open(path).and_then(|file| file.take(FILE_MOST).read_to_string(&mut text)) {
        Ok(_) => from_text(&text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            eprintln!("lamp-voice: the language file could not be read ({error}): English");
            None
        }
    }
}

/// The language the file `var` names holds; none named is English.
pub fn from_var(var: impl Fn(&str) -> Option<OsString>) -> Option<Language> {
    var(FILE_ENVIRONMENT)
        .filter(|path| Path::new(path).is_absolute())
        .and_then(|path| read(Path::new(&path)))
}

/// [`from_var`] from this process's environment.
pub fn from_environment() -> Option<Language> {
    from_var(|name| std::env::var_os(name))
}

/// [`from_environment`], read once for this process: for checks made on
/// every turn.
pub fn at_start() -> Option<Language> {
    static CHOSEN: std::sync::OnceLock<Option<Language>> = std::sync::OnceLock::new();
    *CHOSEN.get_or_init(from_environment)
}

/// `config` heard in `language`: its speech to text told the language,
/// over `stt_language`; English leaves it as it is.
pub fn heard_in(
    mut config: crate::second_config::SecondConfig,
    language: Option<Language>,
) -> crate::second_config::SecondConfig {
    if let Some(language) = language {
        config.stt_language = Some(language.stt().to_owned());
    }
    config
}

/// The second route's file for the coordinator's own streams, the stop
/// words' and the wait words', heard in the owner's language
/// ([`heard_in`]).
pub fn hearing_config_from_env() -> std::io::Result<Option<crate::second_config::SecondConfig>> {
    Ok(crate::second_config::load_from_env()?.map(|config| heard_in(config, from_environment())))
}

/// The character told to speak `language`; English leaves it as it is.
pub fn instruction(character: String, language: Option<Language>) -> String {
    let Some(language) = language else {
        return character;
    };
    if character.contains(ENGLISH_RULE) {
        character.replacen(ENGLISH_RULE, language.rule(), 1)
    } else {
        format!(
            "{character}\n\n{}, whatever language you hear.",
            language.rule()
        )
    }
}

/// The code and the words owed when `chosen` is not the language last
/// announced (`said`, absent is English): none when they agree.
fn owed(chosen: Option<Language>, said: Option<&str>) -> Option<(&'static str, &'static str)> {
    let code = chosen.map_or("en", Language::code);
    let said = said.map(str::trim).filter(|said| !said.is_empty());
    if said.unwrap_or("en") == code {
        return None;
    }
    Some((code, chosen.map_or(ENGLISH_CHANGED, Language::changed)))
}

/// The words owed at start with the record at `said`.
fn owed_at(chosen: Option<Language>, said: &Path) -> Option<(&'static str, &'static str)> {
    owed(chosen, fs::read_to_string(said).ok().as_deref())
}

/// `code` recorded as announced, replacing the record whole.
fn record(said: &Path, code: &str) -> std::io::Result<()> {
    let placing = said.with_extension("placing");
    fs::write(&placing, format!("{code}\n"))?;
    fs::rename(&placing, said)
}

const SKILL: &str = "language";
/// The owner just chose it in the app: said after a short quiet, as a
/// reminder is.
const PRIORITY: u8 = crate::say::URGENT_FROM;
const EXPIRES_MS: u64 = 600_000;

/// Says the change once, through the say service, when the file
/// [`FILE_ENVIRONMENT`] names holds a language other than the one last
/// announced; it is recorded only once spoken, so a change not said is
/// said at the next start. An English lamp never changed has nothing owed.
pub fn announce(asks: crate::say::OwnAsks) {
    let said = Path::new(SAID_PATH);
    let Some((code, words)) = owed_at(from_environment(), said) else {
        return;
    };
    let answered = asks.text(SKILL, words, PRIORITY, EXPIRES_MS);
    let _ = std::thread::Builder::new()
        .name("lamp-voice-language".into())
        .spawn(move || {
            let outcome = loop {
                match answered.recv() {
                    Ok(crate::say::Answer::Done(outcome)) => break outcome,
                    Ok(crate::say::Answer::Begun(_)) => {}
                    Err(_) => {
                        break crate::say::Outcome::Dropped("the voice has gone".into());
                    }
                }
            };
            if outcome == crate::say::Outcome::Spoken
                && let Err(error) = record(Path::new(SAID_PATH), code)
            {
                eprintln!("lamp-voice: the language {code} was said but not recorded: {error}");
            }
            eprintln!(
                "lamp-voice: the language {code} announced: {}",
                outcome.line()
            );
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lamp-language-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn chosen(dir: &Path, text: Option<&str>) -> Option<Language> {
        let file = dir.join("language");
        let _ = fs::remove_file(&file);
        if let Some(text) = text {
            fs::write(&file, text).unwrap();
        }
        from_var(|name| (name == FILE_ENVIRONMENT).then(|| file.clone().into()))
    }

    #[test]
    fn english_an_absent_or_empty_file_and_an_unknown_value_are_english() {
        let dir = scratch("english");
        for text in [
            None,
            Some(""),
            Some("en\n"),
            Some("  "),
            Some("fr"),
            Some("JA"),
        ] {
            assert_eq!(chosen(&dir, text), None, "{text:?}");
        }
        assert_eq!(from_var(|_| None), None);
        // Only an absolute path is read.
        assert_eq!(from_var(|_| Some("language".into())), None);
    }

    #[test]
    fn vi_ja_zh_and_ko_are_read() {
        let dir = scratch("read");
        assert_eq!(chosen(&dir, Some("vi\n")), Some(Language::Vietnamese));
        assert_eq!(chosen(&dir, Some("ja\n")), Some(Language::Japanese));
        assert_eq!(chosen(&dir, Some("zh")), Some(Language::Chinese));
        assert_eq!(chosen(&dir, Some(" ko ")), Some(Language::Korean));
    }

    #[test]
    fn english_leaves_the_character_byte_for_byte() {
        for character in [
            crate::config::CHAT_INSTRUCTION,
            crate::config::CAMERA_CHAT_INSTRUCTION,
            include_str!("../../../deploy/character/lamp.txt"),
        ] {
            assert_eq!(instruction(character.to_owned(), None), character);
        }
    }

    #[test]
    fn each_language_replaces_the_english_rule() {
        for (language, rule) in [
            (
                Language::Vietnamese,
                "Always speak Vietnamese, whatever language you hear",
            ),
            (
                Language::Japanese,
                "Always speak Japanese, whatever language you hear",
            ),
            (
                Language::Chinese,
                "Always speak Mandarin Chinese, in simplified characters, whatever language you hear",
            ),
            (
                Language::Korean,
                "Always speak Korean, whatever language you hear",
            ),
        ] {
            for character in [
                crate::config::CHAT_INSTRUCTION,
                include_str!("../../../deploy/character/lamp.txt"),
            ] {
                let told = instruction(character.to_owned(), Some(language));
                assert!(told.contains(rule), "{language:?}");
                assert!(!told.contains(ENGLISH_RULE), "{language:?}");
                assert!(told.starts_with("You are Lamp"), "{language:?}");
            }
            // A character without the rule is told it at its end.
            assert!(
                instruction("Be kind.".into(), Some(language))
                    .ends_with(&format!("{}, whatever language you hear.", language.rule()))
            );
        }
    }

    /// The stop and wait words' streams hear the owner's language; English
    /// leaves the file's.
    #[test]
    fn the_coordinators_streams_hear_the_owners_language() {
        let dir = scratch("heard");
        let key = dir.join("llm.key");
        fs::write(&key, "secret\n").unwrap();
        let config = dir.join("second.json");
        fs::write(
            &config,
            format!(
                r#"{{"llm_base_url":"https://llm.example.invalid/v1","llm_model":"m","llm_credential_file":"{}","stt_language":"vi"}}"#,
                key.display()
            ),
        )
        .unwrap();
        for path in [&key, &config] {
            fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
        }
        let route = || crate::second_config::load_from_path(&config).unwrap();
        assert_eq!(heard_in(route(), None), route());
        for language in [
            Language::Vietnamese,
            Language::Japanese,
            Language::Chinese,
            Language::Korean,
        ] {
            let heard = heard_in(route(), Some(language));
            assert_eq!(heard.stt_language.as_deref(), Some(language.code()));
        }
    }

    /// #4473 (c): every fixed line has its own words in each language, none
    /// left English; English keeps the English, byte for byte.
    #[test]
    fn each_fixed_line_has_its_words_in_each_language() {
        assert_eq!(FIXED_LINES.len(), 18);
        for (english, _) in FIXED_LINES {
            assert_eq!(fixed_in(None, english), *english);
        }
        assert_eq!(fixed_in(Some(Language::Japanese), "Hello."), "Hello.");
        for language in [
            Language::Vietnamese,
            Language::Japanese,
            Language::Chinese,
            Language::Korean,
        ] {
            let lines: Vec<_> = language.fixed_lines().collect();
            assert_eq!(lines.len(), FIXED_LINES.len());
            for ((english, _), words) in FIXED_LINES.iter().zip(&lines) {
                assert!(!words.trim().is_empty(), "{language:?}");
                assert_ne!(words, english, "{language:?}");
                assert_eq!(language.fixed(english), Some(*words));
                assert_eq!(fixed_in(Some(language), english), *words);
            }
            assert_eq!(language.fixed("Hello."), None);
        }
        assert_eq!(
            Language::Vietnamese.fixed(lamp_msgs::SLEEP_LINE),
            Some("Mình ngủ đây. Chạm vào đầu mình để đánh thức nhé.")
        );
        // The second route's look and search lines (#4707's P2).
        assert_eq!(
            fixed_in(Some(Language::Japanese), crate::second_provider::CHECKING),
            "ちょっと調べてみるね。"
        );
        assert_eq!(
            fixed_in(None, crate::second_provider::LOOKING),
            crate::second_provider::LOOKING
        );
        // The first stand's ask and "here I go" (#4710).
        assert_eq!(
            fixed_in(Some(Language::Korean), lamp_body::first_stand::GOING),
            "좋아요, 시작할게요."
        );
        assert_eq!(
            fixed_in(None, lamp_body::first_stand::ASK_TOUCH_ONLY),
            lamp_body::first_stand::ASK_TOUCH_ONLY
        );
    }

    /// The first stand reads the voice's language file, and takes a language
    /// as not English exactly where the voice does.
    #[test]
    fn the_first_stand_reads_the_language_as_the_voice_does() {
        use lamp_body::first_stand::{LANGUAGE_FILE, speaks_other_than_english};
        assert_eq!(LANGUAGE_FILE, FILE_ENVIRONMENT);
        for text in ["", "en", "vi", "ja\n", "zh", " ko ", "fr", "JA"] {
            assert_eq!(
                speaks_other_than_english(Some(text)),
                matches!(Language::parse(text), Ok(Some(_))),
                "{text:?}"
            );
        }
    }

    #[test]
    fn the_change_is_said_once() {
        let dir = scratch("said");
        let said = dir.join("language-said");
        // An English lamp never changed owes nothing, and writes nothing.
        assert_eq!(owed_at(None, &said), None);
        let owed = owed_at(Some(Language::Japanese), &said);
        assert_eq!(owed, Some(("ja", "これからは日本語で話しますね。")));
        record(&said, "ja").unwrap();
        assert_eq!(owed_at(Some(Language::Japanese), &said), None);
        // Another language, then back to English, each said once.
        assert_eq!(owed_at(Some(Language::Korean), &said).unwrap().0, "ko");
        record(&said, "ko").unwrap();
        assert_eq!(
            owed_at(Some(Language::Vietnamese), &said),
            Some(("vi", "Từ giờ mình sẽ nói tiếng Việt nhé."))
        );
        record(&said, "vi").unwrap();
        assert_eq!(owed_at(None, &said), Some(("en", ENGLISH_CHANGED)));
        record(&said, "en").unwrap();
        assert_eq!(owed_at(None, &said), None);
        assert_eq!(fs::read_to_string(&said).unwrap(), "en\n");
    }
}
