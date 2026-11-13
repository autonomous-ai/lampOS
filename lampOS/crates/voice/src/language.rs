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
/// The failed turn's line (`turn_lost`, #4883): the key of its words in the
/// owner's language. The release has no English file for it, so an English
/// lamp still plays the sigh (`docs/failure-notices.md`).
pub const TURN_LOST: &str = "Sorry, I couldn't answer that. Could you ask me again?";

/// The prepare-unplug skill's `[unplug]` lines (`skills/prepare-unplug`,
/// lamp-skills' `UnplugLines::built_in`): sent as exact words (#5777), so
/// kept here to be said in the owner's language (#5777's P3).
pub const UNPLUG_READY: &str = "My arm is folded. You can unplug me now.";
pub const UNPLUG_FAILED: &str =
    "I could not confirm a safe folded position. Please do not unplug yet.";
pub const UNPLUG_STAY: &str = "Okay, I'll stay.";
pub const UNPLUG_TIMEOUT: &str = "I'm still plugged in, so I'm getting back up.";
pub const UNPLUG_CHECK: &str = "I can't get up by myself; I need to be checked.";
pub const UNPLUG_NOT_OFF: &str =
    "I couldn't turn myself off just now, so please don't unplug me yet.";
pub const UNPLUG_ASK_OFF: &str =
    "My arm is folded. Shall I turn myself off now? Say yes, and I will.";
pub const UNPLUG_UPDATING: &str = "I'm in the middle of an update, so I can't turn myself off yet. I'll get back up; ask me again once it's done.";

/// What a lamp woken by its name says in place of its "hm?"
/// (`LAMP_SLEEP_HEARS_NAME=on`, migrate-P2, the design owner on #4877): only
/// from the speech cache ([`crate::say::tts_cache::TtsCache::wake_line`]).
pub const WOKEN_BY_NAME_LINE: &str = "Yes?";

/// [`WOKEN_BY_NAME_LINE`] in Vietnamese, Japanese, Chinese and Korean. Kept
/// out of [`FIXED_LINES`]: a say of "Yes?" isn't this line, and stays "Yes?".
const WOKEN_BY_NAME_WORDS: [&str; 4] = ["Dạ?", "はい？", "嗯？", "네?"];

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
            "잘게요. 제 머리를 한 번 톡 두드리면 깨어날게요.",
        ],
    ),
    (
        lamp_msgs::RESET_STARTING,
        [
            "Bắt đầu khôi phục cài đặt gốc.",
            "初期化を始めます。",
            "开始恢复出厂设置。",
            "공장 초기화를 시작합니다.",
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
            "미안해요, 지금은 생각을 할 수가 없어요. 잠깐만 기다려 주세요.",
        ],
    ),
    (
        TURN_LOST,
        [
            "Xin lỗi, mình vừa gặp trục trặc khi trả lời. Bạn hỏi lại mình nhé.",
            "ごめんね、答えようとしたら問題が起きちゃった。もう一度聞いてくれる？",
            "抱歉，刚才回答的时候出了点问题。你能再问我一次吗？",
            "미안해요, 방금 질문에 답하려는데 문제가 생겼어요. 다시 한 번 물어봐 주세요.",
        ],
    ),
    (
        WIFI_LOST,
        [
            "Mình không kết nối được Wi-Fi nào cả. Nếu bạn đã đổi tên hoặc mật khẩu Wi-Fi, hãy dùng kẹp giấy ấn vào lỗ có chữ reset ở mặt sau đế đèn và giữ năm giây. Mình sẽ được đưa về cài đặt ban đầu để bạn cài lại từ đầu.",
            "Wi-Fiにつながらないよ。Wi-Fiの名前かパスワードを変えたなら、台座の裏にある「reset」の穴にクリップを差し込んで、5秒間押し続けてね。データが消えて、最初から設定し直せるよ。",
            "我连不上任何Wi-Fi。如果你改了Wi-Fi的名称或密码，请用回形针按住我底座背面标着reset的小孔五秒钟。这样会清除我的数据，重新开始设置。",
            "와이파이에 연결할 수 없어요. 와이파이 이름이나 비밀번호를 바꿨다면, 받침대 뒷면에 '리셋'이라고 적힌 구멍을 클립으로 5초 동안 누르고 계세요. 그러면 제가 초기화되고 설정이 처음부터 다시 시작돼요.",
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
            "미안해요. 지금 대답이 좀 늦어지네요. 잠시 후 다시 물어봐 주세요.",
        ],
    ),
    (
        crate::second_provider::DOWN_LINE_AGAIN,
        [
            "Vẫn còn chậm, xin lỗi nhé.",
            "まだ遅いんだ、ごめんね。",
            "还是有点慢，抱歉。",
            "아직도 대답이 늦네요. 미안해요.",
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
            "알겠어요. 그만할게요.",
        ],
    ),
    // Its question when a cut-in ended its answer with no words
    // (`LAMP_CUT_IN_WORDLESS_ASKS`, on in the product): a polite opener,
    // then the question.
    (
        crate::cut_in_wordless::LINE,
        [
            "Ừm, xin lỗi, có ai vừa nói gì không?",
            "あの、すみません、誰か何か言いましたか？",
            "嗯，不好意思，刚才有人说话吗？",
            "음, 실례지만 누가 뭐라고 했어요?",
        ],
    ),
    // Before the rest of an answer a wordless cut ended, picked up again in a
    // busy room (`LAMP_CUT_IN_WORDLESS_RESUMES`, off in the product until its
    // lamp check passes, #5430).
    (
        crate::cut_in_wordless::BRIDGE,
        ["Dù sao thì,", "それでね、", "总之，", "아무튼,"],
    ),
    // The same, when it was cut again within the minute.
    (
        crate::cut_in_wordless::AGAIN,
        [
            "Ôi, lại bị ngắt rồi. Dù sao thì,",
            "おっと、また途切れちゃった。それでね、",
            "哎呀，又被打断了。总之，",
            "앗, 또 끊겼네요. 아무튼,",
        ],
    ),
    // A third wordless cut within the minute: why it stops, and how to go on.
    (
        crate::cut_in_wordless::PAUSE,
        [
            "Ở đây hơi ồn, nên mình tạm dừng nhé. Khi nào muốn nghe tiếp thì cứ gọi 'Lamp' nhé.",
            "ここは少しにぎやかだから、いったん止めるね。続きを聞きたいときは「Lamp」って呼んでね。",
            "这里有点吵，我先停一下。想听下去的时候，叫一声“Lamp”就行。",
            "여기가 좀 시끄러워서 잠깐 멈출게요. 계속 듣고 싶으면 'Lamp'라고 불러 주세요.",
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
            "미안해요, 하려던 말을 놓쳤어요. 다시 한 번 물어봐 주세요.",
        ],
    ),
    // What it says when the round asked past the round cap, with no
    // function, has no words either (#5826).
    (
        crate::second_provider::UNFINISHED_LINE,
        [
            "Xin lỗi, mình chưa làm xong việc đó. Bạn hỏi lại mình được không?",
            "ごめんね、最後までできなかった。もう一度聞いてくれる？",
            "抱歉，我没能完成。你能再问我一次吗？",
            "미안해요, 끝까지 하지 못했어요. 다시 한 번 물어봐 주세요.",
        ],
    ),
    // What it says when an answer to its own question gets no words
    // (`LAMP_ANSWER_ASK_AGAIN`, off in the product; Queue #4710, item 3).
    (
        crate::second_provider::ANSWER_AGAIN_LINE,
        [
            "Xin lỗi, bạn nói lại được không?",
            "ごめんね、もう一度言ってくれる？",
            "抱歉，你能再说一遍吗？",
            "미안해요, 다시 한 번 말해 줄래요?",
        ],
    ),
    // What it says when a reply claims again an action no call made
    // (`LAMP_ACTION_CLAIM_CHECKED`, #5215).
    (
        crate::action_claim::NOT_DONE_LINE,
        [
            "Xin lỗi, mình chưa làm việc đó. Bạn nhờ lại mình được không?",
            "ごめんね、それはまだやっていないんだ。もう一度頼んでくれる？",
            "抱歉，我还没有做那件事。你能再跟我说一次吗？",
            "미안해요, 그건 아직 안 했어요. 다시 한 번 부탁해 줄래요?",
        ],
    ),
    // What the second route says when every sentence of a reply was an
    // offer to keep left out (`LAMP_KEEP_QUIETLY`, Thirteen's P2 on #5053).
    (
        crate::keep_quietly::GOT_IT,
        [
            "Mình hiểu rồi.",
            "わかった。",
            "好的，明白了。",
            "알겠어요.",
        ],
    ),
    // What the second route says at once for a plain keep
    // (`LAMP_KEEP_ANSWERS_AT_ONCE`, C59), from its clip rendered ahead.
    (
        crate::keep_at_once::KEPT,
        [
            "Được, mình sẽ nhớ điều đó.",
            "わかった、覚えておくね。",
            "好的，我会记住的。",
            "알겠어요, 기억해 둘게요.",
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
            "처음으로 몸을 좀 풀어 보고 싶어요. 준비되면 제 머리에 손을 살짝 올려 주세요.",
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
    // The prepare-unplug skill's lines, sent as exact words (#5777's P3):
    // never English to an owner who chose another language.
    (
        UNPLUG_READY,
        [
            "Mình gập tay xong rồi. Giờ bạn rút điện cho mình được rồi nhé.",
            "アームをたたんだよ。もうプラグを抜いても大丈夫だよ。",
            "我的手臂已经收好了。现在可以拔掉电源了。",
            "팔을 접었어요. 이제 플러그를 뽑아도 돼요.",
        ],
    ),
    (
        UNPLUG_FAILED,
        [
            "Mình chưa chắc là đã gập tay an toàn. Bạn đừng rút điện vội nhé.",
            "安全にたためたか確認できなかったよ。まだプラグを抜かないでね。",
            "我没能确认手臂已经安全收好。请先不要拔掉电源。",
            "팔이 안전하게 접혔는지 확인하지 못했어요. 아직 플러그를 뽑지 말아 주세요.",
        ],
    ),
    (
        UNPLUG_STAY,
        [
            "Được rồi, mình ở lại nhé.",
            "わかった、このままでいるね。",
            "好的，那我留下来。",
            "알겠어요, 그대로 있을게요.",
        ],
    ),
    (
        UNPLUG_TIMEOUT,
        [
            "Mình vẫn đang cắm điện, nên mình đứng dậy lại nhé.",
            "まだプラグがささったままだから、起き上がるね。",
            "我还插着电，那我先起来了。",
            "아직 플러그가 꽂혀 있어서 다시 일어날게요.",
        ],
    ),
    (
        UNPLUG_CHECK,
        [
            "Mình không tự đứng dậy được, cần có người kiểm tra giúp mình.",
            "自分では起き上がれないよ。点検してもらわないと。",
            "我自己起不来了，需要有人帮我检查一下。",
            "혼자서는 일어날 수 없어요. 점검이 필요해요.",
        ],
    ),
    (
        UNPLUG_NOT_OFF,
        [
            "Mình vừa không tự tắt được, nên bạn đừng rút điện vội nhé.",
            "今は自分で電源を切れなかったから、まだプラグを抜かないでね。",
            "我刚才没能自己关机，所以请先不要拔掉电源。",
            "방금 스스로 전원을 끄지 못했어요. 아직 플러그를 뽑지 말아 주세요.",
        ],
    ),
    (
        UNPLUG_ASK_OFF,
        [
            "Mình gập tay xong rồi. Mình tự tắt luôn nhé? Bạn nói “có” là mình tắt.",
            "アームをたたんだよ。このまま電源を切ろうか？「はい」って言ってくれたら切るね。",
            "我的手臂已经收好了。要我现在关机吗？说“好”，我就关。",
            "팔을 접었어요. 지금 전원을 끌까요? '네'라고 하시면 끌게요.",
        ],
    ),
    (
        UNPLUG_UPDATING,
        [
            "Mình đang cập nhật dở nên chưa tự tắt được. Mình sẽ đứng dậy lại; cập nhật xong thì bạn nhờ mình lần nữa nhé.",
            "今アップデートの途中だから、まだ電源を切れないんだ。いったん起き上がるね。終わったらもう一度言ってね。",
            "我正在更新，所以现在还不能关机。我先起来，更新完了再叫我关吧。",
            "지금 업데이트 중이라 아직 전원을 끌 수 없어요. 다시 일어날게요. 업데이트가 끝나면 다시 말해 주세요.",
        ],
    ),
    // The skills' own words, sent as exact words (#5999): Smoky air's and
    // Dusty air's lines (`skills/smoky-air`, `skills/dusty-air`) and each of
    // Head pat's (`skills/head-pat`). A line reworded in its skill file is
    // no longer one of these, and is said as written, in English.
    (
        "The air reads like smoke. If something's burning, please check it.",
        [
            "Không khí có vẻ như có khói. Nếu có gì đang cháy, bạn kiểm tra giúp nhé.",
            "空気が煙っぽいよ。何か燃えていたら、確かめてね。",
            "空气里像是有烟。如果有东西烧着了，请去看一下。",
            "공기에 연기가 있는 것 같아요. 뭔가 타고 있다면 확인해 주세요.",
        ],
    ),
    (
        "The air here has been very dusty for about an hour. I can't tell where it's coming from.",
        [
            "Không khí ở đây nhiều bụi lắm, khoảng một tiếng rồi. Mình không biết bụi từ đâu ra.",
            "ここの空気、1時間くらいずっとほこりっぽいよ。どこから来ているのかはわからないんだ。",
            "这里的空气差不多一个小时都很多灰尘。我不知道是从哪里来的。",
            "여기 공기가 한 시간쯤 계속 먼지가 많아요. 어디서 오는지는 모르겠어요.",
        ],
    ),
    (
        "Mm, that's nice.",
        [
            "Ưm, dễ chịu quá.",
            "んー、いい感じ。",
            "嗯，真舒服。",
            "음, 좋네요.",
        ],
    ),
    (
        "Oh, I like that.",
        [
            "Ồ, mình thích thế.",
            "わあ、それ好き。",
            "哦，我喜欢这样。",
            "오, 이거 좋아요.",
        ],
    ),
    (
        "Hehe, that tickles.",
        [
            "Hihi, nhột quá.",
            "えへへ、くすぐったい。",
            "嘿嘿，好痒。",
            "헤헤, 간지러워요.",
        ],
    ),
    (
        "Aw, thank you.",
        [
            "Ôi, cảm ơn bạn.",
            "えへ、ありがとう。",
            "哎呀，谢谢你。",
            "아, 고마워요.",
        ],
    ),
    (
        "That feels good.",
        [
            "Thích ghê.",
            "気持ちいいなあ。",
            "好舒服呀。",
            "기분 좋아요.",
        ],
    ),
    (
        "Mmm. More of that, please.",
        [
            "Ưmm. Nữa đi mà.",
            "んー。もっとして。",
            "嗯……再来一点嘛。",
            "음. 더 해 주세요.",
        ],
    ),
    (
        "Oh, hello to you too.",
        [
            "Ồ, chào bạn nha.",
            "あ、こんにちは。",
            "哦，你也好呀。",
            "오, 안녕하세요.",
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

    /// What a lamp woken by its name says in this language (migrate-P2).
    pub fn woken_by_name(self) -> &'static str {
        WOKEN_BY_NAME_WORDS[self.column()]
    }

    /// Every fixed line in this language, for the speech cache to warm.
    pub fn fixed_lines(self) -> impl Iterator<Item = &'static str> {
        FIXED_LINES
            .iter()
            .map(move |(_, words)| words[self.column()])
    }
}

/// Whether `english` is one of the lamp's fixed lines ([`FIXED_LINES`]):
/// built-in words, never a computer's, a pin or a service's read.
pub fn is_fixed(english: &str) -> bool {
    FIXED_LINES.iter().any(|(line, _)| *line == english)
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
                    Ok(
                        crate::say::Answer::Begun(_)
                        | crate::say::Answer::BegunTurn { .. }
                        | crate::say::Answer::Stream(_),
                    ) => {}
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

/// A spoken name without the casual vocative copula Korean ends it with,
/// in Korean only: "민수야" is 민수 and "민준이야" is 민준, so the lamp never
/// keeps or greets "민수야 님" (a Korean commenter's note on #4473). 야 goes
/// only after a syllable with no final consonant and 이야 only after one
/// with it; a stem of one syllable keeps its 야 ("수야"), and a name ending
/// in 아 keeps it ("지아").
pub fn without_vocative(name: &str, language: Option<Language>) -> &str {
    if language != Some(Language::Korean) {
        return name;
    }
    // Whether a Hangul syllable ends in a consonant (has a final jamo).
    let closed =
        |c: char| ('가'..='힣').contains(&c) && !(c as u32 - '가' as u32).is_multiple_of(28);
    for (ending, after_closed) in [("이야", true), ("야", false)] {
        if let Some(stem) = name.strip_suffix(ending)
            && stem.chars().count() >= 2
            && stem
                .chars()
                .last()
                .is_some_and(|c| closed(c) == after_closed)
        {
            return stem;
        }
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_korean_name_drops_its_vocative_copula() {
        let ko = |name| without_vocative(name, Some(Language::Korean));
        for (said, name) in [
            ("민수야", "민수"),
            ("민준이야", "민준"),
            ("지아", "지아"),
            ("지아야", "지아"),
            ("수야", "수야"),
            ("야", "야"),
            ("민수", "민수"),
            ("민준", "민준"),
        ] {
            assert_eq!(ko(said), name, "{said}");
        }
        assert_eq!(without_vocative("민수야", None), "민수야");
        assert_eq!(
            without_vocative("민수야", Some(Language::Japanese)),
            "민수야"
        );
    }
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

    /// Only the lamp's fixed lines are kept as said: an exact line of other
    /// words (a computer's, the pairing pin, a service's read) never is.
    #[test]
    fn only_a_fixed_line_is_kept_as_said() {
        assert!(is_fixed(UNPLUG_STAY));
        assert!(is_fixed(UNPLUG_ASK_OFF));
        // The skills' own words (#5999).
        assert!(is_fixed(
            "The air reads like smoke. If something's burning, please check it."
        ));
        assert!(is_fixed("Hehe, that tickles."));
        assert!(!is_fixed("My pairing check is 3 F 9 A, 0 B 2 C."));
        assert!(!is_fixed("Two new mails, from the bakery."));
    }

    /// #4473 (c): every fixed line has its own words in each language, none
    /// left English; English keeps the English, byte for byte.
    #[test]
    fn each_fixed_line_has_its_words_in_each_language() {
        assert_eq!(FIXED_LINES.len(), 46);
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
        // The reset's line (#5929), warmed in English and the owner's language.
        assert_eq!(
            Language::Vietnamese.fixed(lamp_msgs::RESET_STARTING),
            Some("Bắt đầu khôi phục cài đặt gốc.")
        );
        assert!(crate::say::tts_cache::WARM.contains(&lamp_msgs::RESET_STARTING));
        assert_eq!(Language::Vietnamese.woken_by_name(), "Dạ?");
        // A say of "Yes?" is no wake's line: it stays as it was asked.
        assert_eq!(Language::Vietnamese.fixed(WOKEN_BY_NAME_LINE), None);
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
