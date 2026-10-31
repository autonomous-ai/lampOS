//! The walk in the owner's language (#4473 (b), `docs/onboarding.md`, "The
//! owner's language"). English never comes here: its rules and lines are
//! the walk's own, byte for byte. For Vietnamese, Japanese, Chinese and
//! Korean ([`crate::language::Language`]):
//! - what the person says is read by the same rules English is read by
//!   (a yes, a no, the hello's answer, a question that is no answer, the
//!   quiet hours), from each language's own words ([`Lexicon`]): whole words
//!   in Vietnamese, the start of a word in Korean, whose endings join the
//!   word, and anywhere in the line in Japanese and Chinese, which have no
//!   spaces;
//! - the walk's fixed lines keep their English note around them, the
//!   instruction to the model, and each quoted line in it is said in the
//!   language ([`said`], [`LINES`]). The call name stays "Lamp", written so.
//!
//! A quoted line not in [`LINES`] (an edited skill file) stays English.
use crate::language::Language;

/// What a line says, as the walk's rules read it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Reading {
    /// A yes, with no no word.
    pub(super) agrees: bool,
    /// A no word.
    pub(super) refuses: bool,
    /// A name told.
    pub(super) names_someone: bool,
    /// A yes beside a greeting, with no no.
    pub(super) hello_yes: bool,
    /// About the hello, with a yes or a no in it.
    pub(super) about_hello: bool,
    /// A greeting.
    pub(super) greets: bool,
    /// Asking to hear it again.
    pub(super) again: bool,
    /// A question mark.
    pub(super) asks: bool,
    /// No words at all.
    pub(super) empty: bool,
    /// Hours named, in any words.
    pub(super) names_hours: bool,
    /// An answer about the hours to keep quiet: a time, or none.
    pub(super) quiet_hours: bool,
    /// A short line's strict yes or no: never one that puts it off or asks.
    pub(super) yes_or_no: Option<bool>,
    /// A maybe, a filler or a "let me think": no answer yet (English has
    /// none here; its own rules keep such lines out).
    pub(super) puts_off: bool,
    /// Once the hello is asked, this line's yes or no answers it: in
    /// English a short line or one opening with a firm yes or no (#4480); in
    /// the owner's language a short, strict yes or no (#4506).
    pub(super) answers_hello: bool,
    /// The line asks the hello back or puts it off: never its answer.
    pub(super) holds_hello: bool,
}

/// How a language's words are found in a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Found {
    /// Whole words, or whole runs of words (Vietnamese).
    Whole,
    /// At the start of a word: Korean's endings join the word ("밤에").
    WordStart,
    /// Anywhere: Japanese and Chinese have no spaces.
    Anywhere,
}

/// One language's words for the walk's rules, each list as its English
/// counterpart in `onboarding.rs` is used.
struct Lexicon {
    found: Found,
    /// [`super::YES`]: a yes for `agrees`.
    yes: &'static [&'static str],
    /// [`super::NO`].
    no: &'static [&'static str],
    /// [`super::YES_PHRASES`]: a yes that holds a no word.
    yes_phrases: &'static [&'static str],
    /// `yes_or_no`'s yes: no word that is also a polite refusal. A whole
    /// word in Vietnamese and Korean; in Japanese and Chinese only where it
    /// opens the line.
    strict_yes: &'static [&'static str],
    /// A maybe, a filler, a "wait" or a "but": the line is no answer, to the
    /// consents or the hello, whatever yes it holds (Reviewer-three's P1 on
    /// #4506).
    put_off: &'static [&'static str],
    /// What may go with a bare yes and nothing else: a politeness ending,
    /// "please" or "thanks" ([`Lexicon::bare_yes`]).
    polite: &'static [&'static str],
    /// What makes a line a question without its mark, found as written in
    /// the plain line: zh 吗, 呢; a ja か ending a phrase; a ko 까요, 나요.
    question: &'static [&'static str],
    /// `yes_or_no`'s no.
    strict_no: &'static [&'static str],
    /// The words of a line about the hello ("hello", "hi", "greet").
    hello: &'static [&'static str],
    /// A greeting, longest first: taken out before a yes or a time is read.
    greet: &'static [&'static str],
    /// Asking to hear it again: taken out before a yes or a no is read.
    again: &'static [&'static str],
    /// Words that hold a no word but are none: taken out first.
    ignore: &'static [&'static str],
    /// `names_a_time`'s words: a part of the day, "after", "before", and an
    /// hour with its own "o'clock" (ten o'clock, 十時, 十点, 열 시).
    when: &'static [&'static str],
    /// `names_hours`' "quiet", "hour", "hours".
    quiet: &'static [&'static str],
    /// What tells a name: "my name is", "call me", "I'm".
    name_marks: &'static [&'static str],
    /// [`super::NOT_A_NAME`] and [`super::NOT_A_NAME_ALONE`].
    not_a_name: &'static [&'static str],
    /// The most words (letters in Japanese and Chinese) of a strict yes or no.
    short_answer: usize,
    /// The most words (letters) of a name said alone.
    short_name: usize,
}

const VIETNAMESE: Lexicon = Lexicon {
    found: Found::Whole,
    yes: &[
        "có",
        "vâng",
        "dạ",
        "ừ",
        "ờ",
        "được",
        "ok",
        "okay",
        "oke",
        "đồng ý",
        "đúng",
        "chắc chắn",
        "tất nhiên",
        "ổn",
    ],
    no: &["không", "ko", "đừng", "chưa", "khỏi"],
    yes_phrases: &[
        "không sao",
        "không vấn đề gì",
        "không phiền",
        "cứ tự nhiên",
        "sao cũng được",
    ],
    strict_yes: &[
        "có",
        "vâng",
        "dạ",
        "ừ",
        "được",
        "đồng ý",
        "đúng",
        "chắc chắn",
    ],
    strict_no: &["không", "ko", "đừng", "sai"],
    put_off: &[
        "có lẽ",
        "có thể",
        "có khi",
        // "Probably"; "chắc chắn" ("surely") is taken out before it is read.
        "chắc",
        "để mình nghĩ",
        "để tôi nghĩ",
        "để em nghĩ",
        "để xem",
        "chờ",
        "đợi",
        "chưa biết",
        "không biết",
        "nhưng",
        "ừm",
        "ờm",
        "ơ",
        // #4480's "later", "slower", "check", "explain".
        "lát nữa",
        "để sau",
        "từ từ",
        "nói chậm",
        "kiểm tra",
        "giải thích",
    ],
    polite: &["ạ", "nhé", "nha", "rồi", "làm ơn", "cảm ơn"],
    question: &[],
    hello: &["chào", "lời chào"],
    greet: &[
        "chào buổi sáng",
        "chào buổi chiều",
        "chào buổi tối",
        "xin chào",
        "chào",
        "alo",
        "hello",
        "hi",
    ],
    again: &[
        "lặp lại",
        "nói lại",
        "gì cơ",
        "cái gì",
        "nói gì",
        "sao cơ",
        "chưa nghe",
        "không nghe",
        "nghe không rõ",
    ],
    ignore: &["xin lỗi"],
    when: &[
        "sáng",
        "trưa",
        "chiều",
        "tối",
        "đêm",
        "khuya",
        "nửa đêm",
        "sau",
        "trước",
        "hai giờ",
        "ba giờ",
        "bốn giờ",
        "năm giờ",
        "sáu giờ",
        "bảy giờ",
        "tám giờ",
        "chín giờ",
        "mười giờ",
        "mười một giờ",
        "mười hai giờ",
    ],
    quiet: &["yên lặng", "im lặng", "yên tĩnh", "giờ", "tiếng"],
    name_marks: &[
        "tên",
        "gọi tôi",
        "gọi mình",
        "gọi em",
        "gọi anh",
        "gọi chị",
        "tôi là",
        "mình là",
        "em là",
    ],
    not_a_name: &[
        "khỏe",
        "đây",
        "sẵn sàng",
        "bận",
        "mệt",
        "lamp",
        "ừm",
        "à",
        "hả",
    ],
    // Vietnamese words are syllables: "không được đâu" is three.
    short_answer: 6,
    short_name: 3,
};

const JAPANESE: Lexicon = Lexicon {
    found: Found::Anywhere,
    yes: &[
        "はい",
        "うん",
        "ええ",
        "いいよ",
        "いいですよ",
        "どうぞ",
        "オッケー",
        "ok",
        "もちろん",
        "お願い",
        "そうです",
        "了解",
        "わかりました",
    ],
    no: &[
        "いいえ",
        "いや",
        "だめ",
        "ダメ",
        "ない",
        "結構です",
        "やめて",
        "しないで",
        "嫌",
        "やだ",
    ],
    yes_phrases: &[
        "問題ない",
        "問題ありません",
        "構わない",
        "かまわない",
        "構いません",
        "かまいません",
        "気にしない",
    ],
    // "大丈夫です" and "いいです" are as often a polite no: never a yes here.
    strict_yes: &[
        "はい",
        "うん",
        "ええ",
        "いいよ",
        "いいですよ",
        "もちろん",
        "そうです",
    ],
    put_off: &[
        "ちょっと",
        "待って",
        "考え",
        "わから",
        "かな",
        "ええと",
        "えーと",
        "えっと",
        "そうですね",
        "でも",
        "けど",
        "かも",
        "まだ",
        "うーん",
        "ふうん",
        "たぶん",
        "多分",
        "あとで",
        "後で",
        "ゆっくり",
        "確認",
        "説明",
    ],
    polite: &["お願いします", "お願い", "どうぞ", "です", "よ", "ね"],
    question: &["ですか ", "ますか ", "でしょうか ", "かな "],
    strict_no: &[
        "いいえ",
        "いや",
        "だめ",
        "ダメ",
        "嫌",
        "やだ",
        "違",
        "結構です",
    ],
    hello: &["あいさつ", "挨拶", "こんにちは"],
    greet: &[
        "こんにちは",
        "こんばんは",
        "おはよう",
        "やあ",
        "もしもし",
        "ハロー",
        "hello",
    ],
    again: &[
        "もう一度",
        "もう一回",
        "もういちど",
        "なんて",
        "何て",
        "聞こえない",
        "聞こえなかった",
        "聞き取れない",
        "聞き取れなかった",
        "繰り返",
    ],
    ignore: &[],
    when: &[
        "夜",
        "朝",
        "午前",
        "午後",
        "夕方",
        "昼",
        "深夜",
        "真夜中",
        "以降",
        "以前",
        "過ぎ",
        "二時",
        "三時",
        "四時",
        "五時",
        "六時",
        "七時",
        "八時",
        "九時",
        "十時",
        "十一時",
        "十二時",
    ],
    quiet: &["静か", "時間", "音"],
    name_marks: &[
        "と呼んで",
        "って呼んで",
        "名前は",
        "私は",
        "わたしは",
        "僕は",
        "俺は",
    ],
    not_a_name: &[
        "元気",
        "大丈夫",
        "ここ",
        "すみません",
        "ごめん",
        "忙し",
        "疲れ",
        "lamp",
        "えっと",
        "うーん",
        "あの",
    ],
    short_answer: 12,
    short_name: 6,
};

const CHINESE: Lexicon = Lexicon {
    found: Found::Anywhere,
    yes: &[
        "是的", "是啊", "好", "可以", "行啊", "行吧", "对", "当然", "同意", "ok",
    ],
    no: &["不", "没有", "别", "没"],
    yes_phrases: &[
        "没问题",
        "没关系",
        "没事",
        "不介意",
        "无所谓",
        "不错",
        "没错",
    ],
    strict_yes: &["是的", "是啊", "好", "可以", "对", "当然"],
    put_off: &[
        "想想",
        "想一下",
        "好像",
        "等",
        "不知道",
        "对了",
        "但是",
        "可是",
        "也许",
        "或许",
        "可能",
        "不过",
        "再说",
        "还没",
        "嗯",
        "呃",
        "以后再",
        "稍后",
        "晚点",
        "慢点",
        "解释",
        "一会儿",
    ],
    polite: &["请", "谢谢", "的", "啊", "呀", "哦"],
    question: &["吗", "呢"],
    strict_no: &["不", "没有", "别", "错"],
    hello: &["打招呼", "招呼", "问好", "你好"],
    greet: &[
        "早上好",
        "下午好",
        "晚上好",
        "你好",
        "您好",
        "早安",
        "晚安",
        "嗨",
        "哈喽",
        "hello",
    ],
    again: &[
        "再说",
        "重复",
        "什么",
        "没听清",
        "没听到",
        "听不清",
        "再一遍",
        "啥",
    ],
    ignore: &["不好意思", "不客气"],
    when: &[
        "晚上",
        "早上",
        "上午",
        "下午",
        "中午",
        "凌晨",
        "半夜",
        "午夜",
        "夜里",
        "以后",
        "之后",
        "以前",
        "之前",
        "两点",
        "二点",
        "三点",
        "四点",
        "五点",
        "六点",
        "七点",
        "八点",
        "九点",
        "十点",
        "十一点",
        "十二点",
        "点钟",
    ],
    quiet: &["安静", "时间", "小时", "时段"],
    name_marks: &["我叫", "叫我", "我是", "名字"],
    not_a_name: &[
        "很好",
        "这里",
        "对不起",
        "抱歉",
        "忙",
        "累",
        "lamp",
        "嗯",
        "呃",
        "那个",
    ],
    short_answer: 12,
    short_name: 6,
};

const KOREAN: Lexicon = Lexicon {
    found: Found::WordStart,
    yes: &[
        "네",
        "예",
        "응",
        // A whole word: "그래요" may be only "I heard you", asked again.
        "그래 ",
        "좋아",
        "좋습니다",
        "물론",
        "맞아",
        "맞습니다",
        "오케이",
        "ok",
        "okay",
        "알겠",
        "돼요",
        "됩니다",
        "그럼요",
        "넵",
        "넹",
        "그러세요",
        "당연",
        "허락",
    ],
    // Hangul is matched by the syllable: "아니" is no "아닙니다", "안 돼" no
    // "안 됩니다".
    no: &[
        "아니",
        "아닙니다",
        "아뇨",
        "싫",
        "안 돼",
        "안돼",
        "안 됩니다",
        "안됩니다",
        "안 해",
        "하지 마",
        "하지마",
        "하지 말아",
        "않",
        "없",
        "말아",
    ],
    yes_phrases: &["상관없", "문제없", "문제 없"],
    // "괜찮아요" is as often a polite no: never a yes here.
    // Whole words only: "그래서요" and "예를 들면" hold no yes.
    strict_yes: &[
        "네",
        "예",
        "응",
        "좋아",
        "좋아요",
        "맞아",
        "맞아요",
        "맞습니다",
        "물론",
        "물론이죠",
        "물론이에요",
        "넵",
        "네네",
        "예예",
        "넹",
        "그럼요",
        "그러세요",
        "당연하죠",
        "당연하지",
        "좋습니다",
        "오케이",
        "허락할게요",
    ],
    put_off: &[
        "나중에",
        "천천히",
        "설명",
        "확인",
        "잠시",
        "그래서",
        "글쎄",
        "잠깐",
        "생각",
        "예를",
        "근데",
        "하지만",
        "아마",
        "그런데",
        "그렇지만",
        "그래도",
        "어쩌면",
        "혹시",
        "이따",
        "다음에",
        "모르겠",
        "아직",
        "다만",
        // A whole word: "음악" and "음성" are no "음".
        "음 ",
    ],
    polite: &[
        "부탁해요",
        "부탁합니다",
        "부탁드려요",
        "주세요",
        "감사합니다",
        "고마워요",
        // A permission after a yes ("네, 그렇게 하세요"), longest first, as
        // each is taken out whole.
        "그렇게 하세요",
        "그러세요",
        "해도 돼요",
        "하세요",
        "돼요",
        "됩니다",
        "요",
    ],
    question: &[
        "까요 ",
        "나요 ",
        "니까 ",
        "인가요 ",
        "건가요 ",
        "는가요 ",
        "래요 ",
        "죠 ",
        "지요 ",
    ],
    strict_no: &[
        "아니",
        "아닙니다",
        "아뇨",
        "싫",
        "안 돼",
        "안돼",
        "안 됩니다",
        "안됩니다",
        "안 해",
        "하지 마",
        "하지마",
        "하지 말아",
        "말아",
        "틀려",
    ],
    hello: &["인사", "안녕"],
    greet: &[
        "좋은 아침",
        "좋은 저녁",
        "안녕",
        "여보세요",
        "반가워",
        "반갑",
        "하이",
        "헬로",
        "hello",
        "hi",
    ],
    again: &[
        "다시",
        "뭐",
        "못 들었",
        "잘 안 들",
        "안 들려",
        "안 들렸",
        "못 알아들",
        "한 번 더",
    ],
    ignore: &[],
    when: &[
        "밤",
        "아침",
        "저녁",
        "오후",
        "오전",
        "새벽",
        "낮",
        "자정",
        "정오",
        "이후",
        "이전",
        "전에",
        "후에",
        // "One o'clock" too: unlike Chinese 一点 ("a little"), "한 시" is
        // only an hour.
        "한 시",
        "두 시",
        "세 시",
        "네 시",
        "다섯 시",
        "여섯 시",
        "일곱 시",
        "여덟 시",
        "아홉 시",
        "열 시",
        "열한 시",
        "열두 시",
        "한시",
        "두시",
        "세시",
        "네시",
        "다섯시",
        "여섯시",
        "일곱시",
        "여덟시",
        "아홉시",
        "열시",
        "열한시",
        "열두시",
    ],
    quiet: &["조용", "시간"],
    name_marks: &["이름", "불러", "저는", "나는"],
    // "음" and "어" whole words: "음악" and "어진" are no filler.
    not_a_name: &[
        "괜찮",
        "여기",
        "미안",
        "죄송",
        "바빠",
        "바쁘",
        "피곤",
        "lamp",
        "음 ",
        "어 ",
        "저기",
        "잠시",
        "잠깐",
        "글쎄",
        "나중",
        "아직",
        "그냥",
        "몰라",
        "모르겠",
        "램프",
        "헤이",
    ],
    short_answer: 4,
    short_name: 2,
};

fn lexicon(language: Language) -> &'static Lexicon {
    match language {
        Language::Vietnamese => &VIETNAMESE,
        Language::Japanese => &JAPANESE,
        Language::Chinese => &CHINESE,
        Language::Korean => &KOREAN,
    }
}

/// A line lower case, each mark a space, one space between words and one
/// at each end, so a word is found by its spaces.
fn plain(text: &str) -> String {
    let mut line = String::from(" ");
    for word in text
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|word| !word.is_empty())
    {
        line.push_str(&word.to_lowercase());
        line.push(' ');
    }
    line
}

impl Lexicon {
    fn has(&self, line: &str, word: &str) -> bool {
        match self.found {
            Found::Whole => line.contains(&format!(" {word} ")),
            Found::WordStart => line.contains(&format!(" {word}")),
            Found::Anywhere => line.contains(word),
        }
    }

    fn any(&self, line: &str, words: &[&str]) -> bool {
        words.iter().any(|word| self.has(line, word))
    }

    /// A yes word in the line. A one-syllable Korean yes is a whole word,
    /// alone or with its "요" ("네요"), since a name can start with it:
    /// "예린" is no "예" (a Korean customer's review of #4506, A7).
    fn any_yes(&self, line: &str, words: &[&str]) -> bool {
        words.iter().any(|word| {
            if self.found == Found::WordStart && word.chars().count() == 1 {
                line.contains(&format!(" {word} ")) || line.contains(&format!(" {word}요 "))
            } else {
                self.has(line, word)
            }
        })
    }

    /// `line` with `words` taken out, and whether any was there.
    fn without(&self, line: &str, words: &[&str]) -> (String, bool) {
        let mut rest = line.to_owned();
        let mut found = false;
        for word in words {
            let (whole, by) = match self.found {
                Found::Whole => (format!(" {word} "), "  "),
                Found::WordStart => (format!(" {word}"), " "),
                Found::Anywhere => ((*word).to_owned(), " "),
            };
            while rest.contains(&whole) {
                rest = rest.replacen(&whole, by, 1);
                found = true;
            }
        }
        (rest, found)
    }

    /// Words in the line, or letters where there are no spaces.
    fn size(&self, line: &str) -> usize {
        match self.found {
            Found::Anywhere => line.chars().filter(|c| c.is_alphanumeric()).count(),
            _ => line.split_whitespace().count(),
        }
    }

    /// A bare yes (Reviewer-three's P1 on #4506): the line is only strict
    /// yes words (or a yes phrase) and [`Lexicon::polite`] words, nothing
    /// else, so "Có khi", "はい、後で" or "네, 그런데요" is none.
    fn bare_yes(&self, line: &str) -> bool {
        let mut yes: Vec<&str> = self
            .strict_yes
            .iter()
            .chain(self.yes_phrases)
            .copied()
            .collect();
        yes.sort_by_key(|word| std::cmp::Reverse(word.len()));
        let mut said = false;
        let mut rest = line.to_owned();
        for (words, is_yes) in [(&yes[..], true), (self.polite, false)] {
            for word in words {
                // Whole words, even in Korean: "그래서요" is no "그래".
                let whole = match self.found {
                    Found::Anywhere => (*word).to_owned(),
                    _ => format!(" {word} "),
                };
                while rest.contains(&whole) {
                    rest = rest.replacen(&whole, "  ", 1);
                    said |= is_yes;
                }
            }
        }
        said && rest.trim().is_empty()
    }

    fn read(&self, text: &str) -> Reading {
        let line = plain(text);
        // A bare yes ending as a question may ("물론이죠") is no question.
        let asks = text.contains('?')
            || text.contains('？')
            || (self.question.iter().any(|mark| line.contains(mark)) && !self.bare_yes(&line));
        // A put-off inside a longer yes is none: vi "chắc chắn" ("surely")
        // holds "chắc" ("probably").
        let longer: Vec<&str> = self
            .strict_yes
            .iter()
            .copied()
            .filter(|word| word.contains(' '))
            .collect();
        let puts_off = self.any(&self.without(&line, &longer).0, self.put_off);
        let greets = self.any(&line, self.greet);
        let again = self.any(&line, self.again);
        // What is left to hold a yes or a no.
        let (line_no_again, _) = self.without(&line, self.again);
        let (answer, _) = self.without(&line_no_again, self.ignore);
        let (rest, phrase) = self.without(&answer, self.yes_phrases);
        let (ungreeted, _) = self.without(&rest, self.greet);
        let refuses = self.any(&rest, self.no);
        // A yes asked back ("可以吗", "いいですか") is no consent either.
        let agrees =
            !asks && !puts_off && (phrase || self.any_yes(&ungreeted, self.yes)) && !refuses;
        let size = self.size(&line);
        let (timed, _) = self.without(&line, self.greet);
        let names_a_time = self.any(&timed, self.when) || timed.chars().any(|c| c.is_numeric());
        let yes_or_no = (size > 0 && size <= self.short_answer && !asks && !puts_off)
            .then(|| {
                if self.any(&rest, self.strict_no) {
                    Some(false)
                } else if self.bare_yes(&line) {
                    Some(true)
                } else {
                    None
                }
            })
            .flatten();
        // A Korean put-off starts its word, so it is never in a name
        // ("잠시만요"); in Japanese and Chinese one can sit inside one (かな).
        let names_someone = !asks
            && !greets
            && !again
            && !refuses
            && (!puts_off || self.found != Found::WordStart)
            && !self.any_yes(&ungreeted, self.yes)
            && !self.any_yes(&ungreeted, self.strict_yes)
            && !self.any(&line, self.not_a_name)
            && (self.any(&line, self.name_marks) || (1..=self.short_name).contains(&size));
        Reading {
            agrees,
            refuses,
            names_someone,
            // The hello's yes is the consent's: a bare yes.
            hello_yes: yes_or_no == Some(true),
            about_hello: self.any(&line, self.hello) && yes_or_no.is_some(),
            greets,
            again,
            asks,
            empty: size == 0,
            names_hours: names_a_time || self.any(&line, self.quiet),
            quiet_hours: names_a_time || self.any(&answer, self.no),
            yes_or_no,
            puts_off,
            answers_hello: yes_or_no.is_some(),
            holds_hello: asks || again || puts_off,
        }
    }
}

/// `text` read in `language`.
pub(super) fn read(language: Language, text: &str) -> Reading {
    lexicon(language).read(text)
}

/// One fixed line in English and in each language, in that order: vi, ja,
/// zh, ko. A `{slot}` is filled before the line is said, in any language.
type Line = (&'static str, [&'static str; 4]);

/// Every quoted line of the walk's notes: the skill's (`skills/onboarding/
/// skill.toml`) and the code's (`skill.rs`).
#[rustfmt::skip]
pub(super) const LINES: &[Line] = &[
    ("Let's get set up first! It'll only take a few minutes. Please follow my instructions step-by-step, and once we're done, you can ask me anything. Sit where you usually sit and look at me. I'll tell you when I see you.", [
        "Mình cùng cài đặt trước nhé! Chỉ mất vài phút thôi. Bạn làm theo từng bước mình hướng dẫn, xong rồi thì hỏi mình gì cũng được. Bạn ngồi chỗ quen thuộc và nhìn về phía mình. Khi thấy bạn, mình sẽ báo.",
        "まずは設定をしましょう！数分で終わります。私の案内に沿って一つずつ進めてください。終わったら、何でも聞いてくださいね。いつも座る場所に座って、私の方を見てください。見えたらお知らせします。",
        "我们先来设置一下吧！只要几分钟。请按我的提示一步一步来，完成后你可以问我任何问题。请坐在你平时坐的地方，看着我。看到你时我会告诉你。",
        "먼저 설정부터 시작할게요! 몇 분이면 끝나요. 제 안내에 따라 하나씩 해 주세요. 다 끝나면 무엇이든 물어보셔도 돼요. 평소 앉는 자리에 앉아 저를 바라봐 주세요. 얼굴이 보이면 알려 드릴게요.",
    ]),
    ("Sit where you usually sit and look at me. I'll tell you when I see you.", [
        "Bạn ngồi chỗ quen thuộc và nhìn về phía mình. Khi thấy bạn, mình sẽ báo.",
        "いつも座る場所に座って、私の方を見てください。見えたらお知らせします。",
        "请坐在你平时坐的地方，看着我。看到你时我会告诉你。",
        "평소 앉는 자리에 앉아 저를 바라봐 주세요. 얼굴이 보이면 알려 드릴게요.",
    ]),
    ("Okay, we'll skip where you sit for now.", [
        "Được rồi, tạm bỏ qua phần chỗ ngồi nhé.",
        "わかりました。座る場所は、いったん飛ばしますね。",
        "好的，座位这一步我们先跳过。",
        "알겠어요. 앉는 자리는 일단 건너뛸게요.",
    ]),
    ("One more thing: sit where you usually sit and look at me. I'll tell you when I see you.", [
        "Còn một việc nữa: bạn ngồi chỗ quen thuộc và nhìn về phía mình. Khi thấy bạn, mình sẽ báo.",
        "もう一つだけ。いつも座る場所に座って、私の方を見てください。見えたらお知らせします。",
        "还有一件事：请坐在你平时坐的地方，看着我。看到你时我会告诉你。",
        "아, 아직 안 한 게 하나 있어요. 평소 앉는 자리에 앉아 저를 바라봐 주세요. 얼굴이 보이면 알려 드릴게요.",
    ]),
    ("What should I call you?", [
        "Mình nên gọi bạn là gì?",
        "何とお呼びすればいいですか？",
        "我该怎么称呼你？",
        "어떻게 불러 드릴까요?",
    ]),
    ("May I remember your face, so I know you next time?", [
        "Mình nhớ khuôn mặt bạn được không, để lần sau nhận ra bạn?",
        "次に会ったときわかるように、お顔を覚えてもいいですか？",
        "我可以记住你的脸吗？这样下次我就能认出你。",
        "다음에 알아볼 수 있게 얼굴을 기억해도 될까요?",
    ]),
    ("That's fine. You can tell me your name any time.", [
        "Không sao đâu. Lúc nào bạn cũng có thể cho mình biết tên.",
        "大丈夫です。お名前はいつでも教えてくださいね。",
        "没关系。你随时可以告诉我你的名字。",
        "괜찮아요. 이름은 언제든 알려 주세요.",
    ]),
    ("Earlier we didn't get to your name. Let's try it now.", [
        "Lúc nãy mình chưa kịp hỏi tên bạn. Giờ mình thử nhé.",
        "さっきはお名前を聞けませんでした。今やってみましょう。",
        "刚才我们还没问到你的名字。现在试试吧。",
        "아까는 이름을 여쭤보지 못했어요. 지금 여쭤볼게요.",
    ]),
    ("Now I'll learn how you call me. When I stop talking, say 'Hey Lamp' from your seat, the way you'd get my attention. I'll ask until I've heard you three times.", [
        "Giờ mình sẽ học cách bạn gọi mình. Khi mình nói xong, bạn ngồi tại chỗ và nói 'Hey Lamp', như khi muốn gọi mình. Mình sẽ nhờ bạn cho đến khi nghe được ba lần.",
        "次は、呼びかけ方を覚えます。私が話し終えたら、座ったまま、呼びかけるように「Hey Lamp」と言ってください。三回聞こえるまでお願いします。",
        "现在我来学习你怎么叫我。我说完后，请坐在原位说“Hey Lamp”，就像平时叫我那样。我会一直请你说，直到听到三次。",
        "이제 저를 부르는 목소리를 익혀 볼게요. 제 말이 끝나면 평소처럼 앉은 자리에서 'Hey Lamp'라고 불러 주세요. 제가 세 번 들을 때까지 다시 부탁드릴게요.",
    ]),
    ("Earlier we skipped how you call me. Let's try it now.", [
        "Lúc nãy mình bỏ qua phần bạn gọi mình. Giờ mình thử nhé.",
        "さっきは呼びかけ方を飛ばしました。今やってみましょう。",
        "刚才我们跳过了你怎么叫我。现在试试吧。",
        "아까 저를 부르는 단계를 건너뛰었어요. 지금 해 볼게요.",
    ]),
    ("Okay, we'll skip how you call me for now.", [
        "Được rồi, tạm bỏ qua phần bạn gọi mình nhé.",
        "わかりました。呼びかけ方は、いったん飛ばしますね。",
        "好的，叫我这一步我们先跳过。",
        "알겠어요. 부르는 단계는 일단 건너뛸게요.",
    ]),
    ("OK, next step. Tap the top of my head once, then let go.", [
        "Được rồi, bước tiếp theo. Bạn chạm nhẹ lên đỉnh đầu mình một lần, rồi buông tay ra.",
        "では、次です。私の頭のてっぺんを一回軽くたたいて、手を離してください。",
        "好，下一步。轻点一下我的头顶，然后松手。",
        "좋아요. 다음 단계예요. 제 머리 맨 위를 한 번 톡 두드리고 손을 떼 주세요.",
    ]),
    ("One more thing: tap the top of my head once, then let go.", [
        "Còn một việc nữa: bạn chạm nhẹ lên đỉnh đầu mình một lần, rồi buông tay ra.",
        "もう一つだけ。私の頭のてっぺんを一回軽くたたいて、手を離してください。",
        "还有一件事：轻点一下我的头顶，然后松手。",
        "아, 아직 안 한 게 하나 있어요. 제 머리 맨 위를 한 번 톡 두드리고 손을 떼 주세요.",
    ]),
    ("Earlier we skipped the tap on my head. Let's try it now.", [
        "Lúc nãy mình bỏ qua phần chạm đầu. Giờ mình thử nhé.",
        "さっきは頭をたたくところを飛ばしました。今やってみましょう。",
        "刚才我们跳过了轻点头顶。现在试试吧。",
        "아까 제 머리를 톡 두드리는 단계를 건너뛰었어요. 지금 해 볼게요.",
    ]),
    ("Okay, we'll skip the touch for now.", [
        "Được rồi, tạm bỏ qua phần chạm nhé.",
        "わかりました。タッチは、いったん飛ばしますね。",
        "好的，触摸这一步我们先跳过。",
        "알겠어요. 터치는 일단 건너뛸게요.",
    ]),
    ("Find my privacy switch {location}. Close it, then open it again.", [
        "Bạn tìm công tắc riêng tư của mình ({location}). Gạt đóng lại, rồi mở ra lại.",
        "プライバシースイッチを探してください（{location}）。一度閉じてから、また開けてください。",
        "请找到我的隐私开关（{location}）。把它关上，再打开。",
        "제 프라이버시 스위치를 찾아 주세요({location}). 밀어서 닫았다가 다시 열어 주세요.",
    ]),
    ("Earlier we skipped my privacy switch. Let's try it now.", [
        "Lúc nãy mình bỏ qua công tắc riêng tư. Giờ mình thử nhé.",
        "さっきはプライバシースイッチを飛ばしました。今やってみましょう。",
        "刚才我们跳过了隐私开关。现在试试吧。",
        "아까 프라이버시 스위치를 건너뛰었어요. 지금 해 볼게요.",
    ]),
    ("Hold my base with one hand. With the other, tilt my head gently to one side. Let go when I say so.", [
        "Một tay giữ đế của mình. Tay kia nhẹ nhàng nghiêng đầu mình sang một bên. Khi mình bảo thì buông tay.",
        "片手で私の台座を押さえて、もう片方の手で頭をそっと横に傾けてください。私が言ったら手を離してください。",
        "一只手扶住我的底座，另一只手轻轻把我的头歪向一边。我说好了再松手。",
        "한 손으로 제 받침대를 잡고, 다른 손으로 제 머리를 한쪽으로 살짝 기울여 주세요. 제가 손을 떼라고 하면 떼 주세요.",
    ]),
    ("Earlier we skipped my head giving way. Let's try it now.", [
        "Lúc nãy mình bỏ qua phần đầu mình đi theo tay bạn. Giờ mình thử nhé.",
        "さっきは頭を動かすところを飛ばしました。今やってみましょう。",
        "刚才我们跳过了让我的头顺着你的手动。现在试试吧。",
        "아까 제 머리가 손을 따라 움직이는지 확인하는 단계를 건너뛰었어요. 지금 해 볼게요.",
    ]),
    ("Okay, we'll skip my head giving way for now.", [
        "Được rồi, tạm bỏ qua phần đầu mình đi theo tay bạn nhé.",
        "わかりました。頭を動かすところは、いったん飛ばしますね。",
        "好的，让我的头顺着你的手动这一步我们先跳过。",
        "알겠어요. 머리를 움직이는 단계는 일단 건너뛸게요.",
    ]),
    ("May I notice music and laughter in the room? Nothing is recorded. Just say yes or no.", [
        "Mình để ý tiếng nhạc và tiếng cười trong phòng được không? Không có gì bị ghi lại đâu. Bạn chỉ cần nói có hoặc không.",
        "部屋の音楽や笑い声に気づくようにしてもいいですか？録音はしません。はいか、いいえで答えてください。",
        "我可以留意房间里的音乐和笑声吗？不会录下任何东西。请说好或者不好。",
        "방 안에서 음악이나 웃음소리가 나는지 알아봐도 될까요? 아무것도 녹음하지 않아요. '네' 또는 '아니요'로 대답해 주세요.",
    ]),
    ("Sorry, was that a yes or a no?", [
        "Xin lỗi, vậy là có hay không?",
        "すみません、はいですか、いいえですか？",
        "不好意思，是好还是不好？",
        "죄송해요. '네'라고 하셨나요, '아니요'라고 하셨나요?",
    ]),
    ("Got it, I'll notice music and laughter.", [
        "Rồi, mình sẽ để ý tiếng nhạc và tiếng cười.",
        "わかりました。音楽や笑い声に気づくようにしますね。",
        "好的，我会留意音乐和笑声。",
        "알겠어요. 음악이나 웃음소리가 나는지 알아볼게요.",
    ]),
    ("Okay, I won't.", [
        "Được rồi, mình sẽ không làm vậy.",
        "わかりました、しませんね。",
        "好的，我不会的。",
        "알겠어요, 안 할게요.",
    ]),
    ("May I notice when people look happy or upset, so I can answer more warmly? A yes covers everyone at this lamp. I never say it aloud and keep no pictures. Just say yes or no.", [
        "Mình để ý khi mọi người trông vui hay buồn được không, để trả lời ấm áp hơn? Đồng ý là áp dụng cho mọi người ở đây. Mình không bao giờ nói ra và không lưu ảnh nào. Bạn chỉ cần nói có hoặc không.",
        "もっと温かく応えられるように、人がうれしそうか、つらそうかに気づいてもいいですか？はいと答えると、ここにいる全員が対象になります。口に出すことはなく、写真も残しません。はいか、いいえで答えてください。",
        "我可以留意大家看起来开心还是难过吗？这样我能回应得更贴心。同意的话，对我身边的每个人都适用。我从不说出来，也不保存任何照片。请说好或者不好。",
        "사람들이 기뻐 보이는지 속상해 보이는지 알아봐도 될까요? 그러면 더 다정하게 대답할 수 있어요. '네'라고 하시면 여기 계신 모든 분께 적용돼요. 제가 알아챈 감정은 소리 내어 말하지 않고, 사진도 저장하지 않아요. '네' 또는 '아니요'로 대답해 주세요.",
    ]),
    ("Got it, I'll notice how people look.", [
        "Rồi, mình sẽ để ý vẻ mặt mọi người.",
        "わかりました。みなさんの表情に気づくようにしますね。",
        "好的，我会留意大家的表情。",
        "알겠어요. 사람들의 표정을 살펴볼게요.",
    ]),
    ("Are there hours when I should keep quiet? For example, 'after ten at night', or 'no'.", [
        "Có khung giờ nào mình nên giữ yên lặng không? Ví dụ: 'sau mười giờ tối', hoặc 'không'.",
        "静かにしていてほしい時間はありますか？たとえば「夜十時以降」や「ない」などです。",
        "有没有需要我保持安静的时间？比如“晚上十点以后”，或者“没有”。",
        "제가 조용히 해야 하는 시간이 있나요? 예를 들어 '밤 열 시 이후'라고 하시거나, 없으면 '없어요'라고 말씀해 주세요.",
    ]),
    ("Earlier we didn't get to your preferences. Let's do them now.", [
        "Lúc nãy mình chưa kịp hỏi bạn thích thế nào. Giờ mình làm nhé.",
        "さっきはお好みの設定まで進めませんでした。今やりましょう。",
        "刚才我们还没来得及设置你的偏好。现在来吧。",
        "아까는 원하시는 설정을 여쭤보지 못했어요. 지금 해 볼게요.",
    ]),
    ("I can't see your face yet. Sit where you usually sit, and look at me.", [
        "Mình chưa thấy mặt bạn. Bạn ngồi chỗ quen thuộc và nhìn về phía mình nhé.",
        "まだお顔が見えません。いつも座る場所に座って、私の方を見てください。",
        "我还看不到你的脸。请坐在你平时坐的地方，看着我。",
        "아직 얼굴이 안 보여요. 평소에 앉는 자리에 앉아서 저를 봐 주세요.",
    ]),
    ("You're a little far. Move me closer to you, or stay as you are.", [
        "Bạn hơi xa. Bạn dời mình lại gần hơn, hoặc cứ giữ nguyên cũng được.",
        "少し遠いです。私をもう少し近くに動かすか、そのままでも大丈夫です。",
        "你离得有点远。把我挪近一点，或者保持现在这样也行。",
        "조금 멀리 계시네요. 저를 더 가까이 옮겨 주셔도 되고, 지금 그대로 계셔도 돼요.",
    ]),
    ("I can see you at the edge of my view. Turn my head toward you until I'm facing you, or stay as you are.", [
        "Mình thấy bạn ở rìa tầm nhìn. Bạn xoay đầu mình về phía bạn cho tới khi mình nhìn thẳng vào bạn, hoặc cứ giữ nguyên cũng được.",
        "視界のはしに見えています。私の頭を、正面になるまであなたの方に向けてください。そのままでも大丈夫です。",
        "我在视野边缘看到你了。把我的头转向你，直到我正对着你，或者保持现在这样也行。",
        "제 시야 가장자리에 보여요. 제 머리를 돌려 저와 마주 보게 해 주시거나, 그대로 계셔도 돼요.",
    ]),
    ("Got it, I can see you well there.", [
        "Rồi, ở đó mình thấy bạn rõ lắm.",
        "はい、そこならよく見えます。",
        "好的，在那里我能看清你。",
        "좋아요. 그 자리에서는 얼굴이 잘 보여요.",
    ]),
    ("I still can't see you well there. That's fine, you can turn me toward your seat later.", [
        "Ở đó mình vẫn chưa thấy rõ bạn. Không sao, lát nữa bạn xoay mình về phía chỗ ngồi cũng được.",
        "そこだとまだよく見えません。大丈夫です、あとで私を席の方に向けてください。",
        "在那里我还是看不清你。没关系，你之后可以把我转向你的座位。",
        "거기서는 아직 잘 안 보여요. 괜찮아요, 나중에 저를 자리 쪽으로 돌려 주세요.",
    ]),
    ("Sorry, what should I call you?", [
        "Xin lỗi, mình nên gọi bạn là gì?",
        "すみません、何とお呼びすればいいですか？",
        "不好意思，我该怎么称呼你？",
        "죄송해요, 어떻게 불러 드릴까요?",
    ]),
    ("I didn't feel that. Touch the very top of my head with your fingertip, once.", [
        "Mình chưa cảm thấy. Bạn dùng đầu ngón tay chạm lên chính giữa đỉnh đầu mình một lần nhé.",
        "感じませんでした。指先で私の頭のいちばん上を一回さわってください。",
        "我没感觉到。用指尖碰一下我头顶最上面，一次就好。",
        "방금은 터치가 느껴지지 않았어요. 손끝으로 제 머리 맨 위를 한 번 톡 건드려 주세요.",
    ]),
    ("Got it, I felt that.", [
        "Rồi, mình cảm thấy rồi.",
        "はい、感じました。",
        "好的，我感觉到了。",
        "좋아요, 느꼈어요.",
    ]),
    ("I didn't feel a tap this time. That's fine, you can redo the setup later. Just ask me to redo the setup.", [
        "Lần này mình chưa cảm thấy cái chạm. Không sao, bạn có thể cài đặt lại sau. Chỉ cần bảo mình cài đặt lại.",
        "今回はタッチを感じませんでした。大丈夫です、設定はあとでやり直せます。「設定をやり直して」と言ってください。",
        "这次我没感觉到触碰。没关系，你可以稍后重新设置。只要让我重新设置就行。",
        "이번에는 터치를 못 느꼈어요. 괜찮아요. 설정은 나중에 다시 하실 수 있어요. 다시 설정하고 싶을 땐 저에게 말씀해 주세요.",
    ]),
    ("Got it, the switch closes my microphones.", [
        "Rồi, công tắc đó tắt micrô của mình.",
        "はい、スイッチでマイクが閉じます。",
        "好的，这个开关会关闭我的麦克风。",
        "좋아요. 스위치를 닫으면 제 마이크가 차단돼요.",
    ]),
    ("Got it, the switch closes my microphones and my camera.", [
        "Rồi, công tắc đó tắt micrô và camera của mình.",
        "はい、スイッチでマイクとカメラが閉じます。",
        "好的，这个开关会关闭我的麦克风和摄像头。",
        "좋아요. 스위치를 닫으면 제 마이크와 카메라가 차단돼요.",
    ]),
    ("The switch check didn't finish. You can try it again later by redoing the setup. Just ask me to redo the setup.", [
        "Phần kiểm tra công tắc chưa xong. Bạn có thể thử lại sau bằng cách cài đặt lại. Chỉ cần bảo mình cài đặt lại.",
        "スイッチの確認が終わりませんでした。あとで設定をやり直せば、もう一度試せます。「設定をやり直して」と言ってください。",
        "开关检查没有完成。你可以稍后重新设置再试一次。只要让我重新设置就行。",
        "스위치 확인을 끝내지 못했어요. 나중에 설정을 다시 하면 이 단계도 다시 해 볼 수 있어요. 다시 설정하고 싶을 땐 저에게 말씀해 주세요.",
    ]),
    ("Got it. Say 'Hey Lamp' once more.", [
        "Rồi. Bạn nói 'Hey Lamp' thêm lần nữa nhé.",
        "はい。もう一度「Hey Lamp」と言ってください。",
        "好的。再说一次“Hey Lamp”。",
        "좋아요. 'Hey Lamp'라고 한 번 더 말해 주세요.",
    ]),
    ("I didn't catch that. Say 'Hey Lamp' again, from your seat.", [
        "Mình chưa nghe rõ. Bạn ngồi tại chỗ nói lại 'Hey Lamp' nhé.",
        "聞き取れませんでした。座ったまま、もう一度「Hey Lamp」と言ってください。",
        "我没听清。请坐在原位再说一次“Hey Lamp”。",
        "잘 못 들었어요. 자리에서 다시 'Hey Lamp'라고 말해 주세요.",
    ]),
    ("Got it, that's three.", [
        "Rồi, đủ ba lần rồi.",
        "はい、これで三回です。",
        "好的，三次了。",
        "좋아요, 세 번 들었어요.",
    ]),
    ("I couldn't learn how you call me this time. That's fine, you can redo the setup later. Just ask me to redo the setup.", [
        "Lần này mình chưa học được cách bạn gọi mình. Không sao, bạn có thể cài đặt lại sau. Chỉ cần bảo mình cài đặt lại.",
        "今回は呼びかけ方を覚えられませんでした。大丈夫です、設定はあとでやり直せます。「設定をやり直して」と言ってください。",
        "这次我没学会你怎么叫我。没关系，你可以稍后重新设置。只要让我重新设置就行。",
        "이번에는 저를 부르는 목소리를 익히지 못했어요. 괜찮아요. 나중에 설정을 다시 하실 수 있어요. 다시 설정하고 싶을 땐 저에게 말씀해 주세요.",
    ]),
    ("Now say any sentence to me, the way you'd talk to me.", [
        "Giờ bạn nói với mình một câu bất kỳ, như lúc bình thường nói chuyện với mình.",
        "では、ふだん話しかけるように、何か一言話しかけてください。",
        "现在随便对我说一句话，就像平时跟我说话那样。",
        "이제 평소 저에게 이야기하듯 아무 문장이나 하나 말해 주세요.",
    ]),
    ("Say another sentence from your seat, a little louder.", [
        "Bạn ngồi tại chỗ nói thêm một câu nữa, to hơn một chút nhé.",
        "座ったまま、もう少し大きな声でもう一言話してください。",
        "请坐在原位再说一句，声音大一点。",
        "앉은 자리에서 조금 더 큰 목소리로 다른 문장 하나를 말해 주세요.",
    ]),
    ("Come a little closer and say another sentence.", [
        "Bạn lại gần hơn một chút và nói thêm một câu nữa nhé.",
        "もう少し近づいて、もう一言話してください。",
        "请再靠近一点，再说一句。",
        "조금 더 가까이 오셔서 다른 문장 하나를 말해 주세요.",
    ]),
    ("Say another sentence from your seat.", [
        "Bạn ngồi tại chỗ nói thêm một câu nữa nhé.",
        "座ったまま、もう一言話してください。",
        "请坐在原位再说一句。",
        "앉은 자리에서 다른 문장 하나를 말해 주세요.",
    ]),
    ("Got it, I hear you well from there.", [
        "Rồi, từ chỗ đó mình nghe bạn rõ lắm.",
        "はい、そこからよく聞こえます。",
        "好的，在那里我能听清你。",
        "좋아요, 거기서 잘 들려요.",
    ]),
    ("I couldn't hear you well enough this time. I'll get used to your voice as we talk.", [
        "Lần này mình chưa nghe bạn đủ rõ. Mình sẽ quen dần với giọng bạn khi mình trò chuyện.",
        "今回は十分に聞き取れませんでした。話しているうちに、あなたの声に慣れていきますね。",
        "这次我没能听清楚。我们多聊聊，我会慢慢熟悉你的声音。",
        "이번에는 말씀을 충분히 또렷하게 듣지 못했어요. 앞으로 대화하면서 목소리에 익숙해질게요.",
    ]),
    ("I didn't feel my head move. Hold my base with one hand. With the other, tilt my head gently to one side. Let go when I say so.", [
        "Mình chưa thấy đầu mình chuyển động. Một tay giữ đế của mình. Tay kia nhẹ nhàng nghiêng đầu mình sang một bên. Khi mình bảo thì buông tay.",
        "頭が動いたのを感じませんでした。片手で私の台座を押さえて、もう片方の手で頭をそっと横に傾けてください。私が言ったら手を離してください。",
        "我没感觉到头在动。一只手扶住我的底座，另一只手轻轻把我的头歪向一边。我说好了再松手。",
        "제 머리가 움직이는 걸 느끼지 못했어요. 한 손으로 제 받침대를 잡고, 다른 손으로 제 머리를 한쪽으로 살짝 기울여 주세요. 제가 손을 떼라고 하면 떼 주세요.",
    ]),
    ("That's it, now let go.", [
        "Đúng rồi, giờ buông tay ra nhé.",
        "そうです、では手を離してください。",
        "对，现在松手吧。",
        "바로 그거예요. 이제 손을 떼 주세요.",
    ]),
    ("Hold my base with one hand. With the other, tilt my head gently to one side, then let go.", [
        "Một tay giữ đế của mình. Tay kia nhẹ nhàng nghiêng đầu mình sang một bên, rồi buông tay.",
        "片手で私の台座を押さえて、もう片方の手で頭をそっと横に傾けてから、手を離してください。",
        "一只手扶住我的底座，另一只手轻轻把我的头歪向一边，然后松手。",
        "한 손으로 제 받침대를 잡고, 다른 손으로 제 머리를 한쪽으로 살짝 기울인 다음 손을 떼 주세요.",
    ]),
    ("A little more, then let go.", [
        "Thêm chút nữa, rồi buông tay nhé.",
        "もう少しだけ。それから手を離してください。",
        "再多一点，然后松手。",
        "조금만 더 기울인 다음 손을 떼 주세요.",
    ]),
    ("No problem, we'll try that one later.", [
        "Không sao, mình sẽ thử lại phần đó sau nhé.",
        "大丈夫です、それはあとでやりましょう。",
        "没问题，这一步我们稍后再试。",
        "괜찮아요, 그건 나중에 해 볼게요.",
    ]),
    ("Got it, my head moved with your hand.", [
        "Rồi, đầu mình đã đi theo tay bạn.",
        "はい、手に合わせて頭が動きました。",
        "好的，我的头跟着你的手动了。",
        "좋아요, 손을 따라 머리가 움직였어요.",
    ]),
    ("I didn't feel my head move this time. That's fine, you can redo the setup later. Just ask me to redo the setup.", [
        "Lần này mình chưa thấy đầu mình chuyển động. Không sao, bạn có thể cài đặt lại sau. Chỉ cần bảo mình cài đặt lại.",
        "今回は頭が動いたのを感じませんでした。大丈夫です、設定はあとでやり直せます。「設定をやり直して」と言ってください。",
        "这次我没感觉到头在动。没关系，你可以稍后重新设置。只要让我重新设置就行。",
        "이번에는 제 머리가 움직이는 걸 느끼지 못했어요. 괜찮아요. 나중에 설정을 다시 하실 수 있어요. 다시 설정하고 싶을 땐 저에게 말씀해 주세요.",
    ]),
    ("Did I get that right, {name}?", [
        "{name}, mình nghe đúng chưa?",
        "{name}さん、で合っていますか？",
        "{name}，我没听错吧？",
        "{name} 님이라고 들었는데, 맞나요?",
    ]),
    ("May I say hello when you sit down?", [
        "Khi bạn ngồi xuống, mình chào bạn được không?",
        "座ったときに、あいさつしてもいいですか？",
        "你坐下的时候，我可以跟你打个招呼吗？",
        "자리에 앉으시면 인사해도 될까요?",
    ]),
    ("Got it, that's everything. You can ask me to redo the setup any time.", [
        "Rồi, vậy là xong hết. Lúc nào bạn cũng có thể bảo mình cài đặt lại.",
        "はい、これで全部です。設定はいつでもやり直せるので、言ってくださいね。",
        "好的，全部完成了。你随时可以让我重新设置。",
        "좋아요. 이걸로 설정이 모두 끝났어요. 다시 설정하고 싶으시면 언제든 말씀해 주세요.",
    ]),
    ("That's fine, you can redo the setup later. Just ask me to redo the setup.", [
        "Không sao, bạn có thể cài đặt lại sau. Chỉ cần bảo mình cài đặt lại.",
        "大丈夫です、設定はあとでやり直せます。「設定をやり直して」と言ってください。",
        "没关系，你可以稍后重新设置。只要让我重新设置就行。",
        "괜찮아요. 나중에 다시 설정하실 수 있어요. 다시 설정하고 싶을 땐 저에게 말씀해 주세요.",
    ]),
];

fn column(language: Language) -> usize {
    match language {
        Language::Vietnamese => 0,
        Language::Japanese => 1,
        Language::Chinese => 2,
        Language::Korean => 3,
    }
}

/// One quoted line in `language`: a line of [`LINES`], its one slot filled
/// as it was; anything else as it is.
fn line_in(quoted: &str, language: Language) -> Option<String> {
    LINES.iter().find_map(|(english, said)| {
        let said = said[column(language)];
        let Some((before, rest)) = english.split_once('{') else {
            return (quoted == *english).then(|| said.to_owned());
        };
        let (slot, after) = rest.split_once('}')?;
        let filled = quoted
            .strip_prefix(before)?
            .strip_suffix(after)
            .filter(|_| quoted.len() >= before.len() + after.len())?;
        Some(said.replace(&format!("{{{slot}}}"), filled))
    })
}

/// A note of the walk as said in `language`: its instruction as it is,
/// each quoted line in the language. English is the note itself.
pub(super) fn said(note: String, language: Option<Language>) -> String {
    let Some(language) = language else {
        return note;
    };
    note.split('"')
        .enumerate()
        .map(|(at, part)| {
            if at % 2 == 1
                && let Some(line) = line_in(part, language)
            {
                line
            } else {
                part.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Language; 4] = [
        Language::Vietnamese,
        Language::Japanese,
        Language::Chinese,
        Language::Korean,
    ];

    /// Every note the walk can say: the shipped skill's, and the code's.
    fn notes() -> Vec<String> {
        fn strings(value: &toml::Value, into: &mut Vec<String>) {
            match value {
                toml::Value::String(text) if text.starts_with('(') => into.push(text.clone()),
                toml::Value::Table(table) => table.values().for_each(|v| strings(v, into)),
                toml::Value::Array(list) => list.iter().for_each(|v| strings(v, into)),
                _ => {}
            }
        }
        let file: toml::Table =
            toml::from_str(include_str!("../../../../skills/onboarding/skill.toml")).unwrap();
        let mut notes = Vec::new();
        strings(&toml::Value::Table(file), &mut notes);
        notes.extend(
            [
                super::super::skill::PREFERENCES_HELLO,
                super::super::skill::PREFERENCES_DONE,
                super::super::skill::FACE_EXPRESSIONS_YES,
                super::super::skill::FACE_EXPRESSIONS_NO,
                super::super::skill::LATER,
                super::super::skill::LAST_LATER,
            ]
            .map(str::to_owned),
        );
        assert!(notes.len() > 60, "{}", notes.len());
        notes
    }

    /// Each quoted line of a note that is a sentence, not a step's name.
    fn sentences(note: &str) -> Vec<&str> {
        note.split('"')
            .skip(1)
            .step_by(2)
            .filter(|part| part.contains(' '))
            .collect()
    }

    /// Each fixed line comes out in each language, inside the same note: the
    /// instruction as it is, its quoted lines translated, and the note
    /// still fits one written turn and still reads as a fixed line.
    #[test]
    fn every_fixed_line_is_said_in_each_language() {
        for note in notes() {
            assert_eq!(said(note.clone(), None), note);
            for language in ALL {
                let told = said(note.clone(), Some(language));
                assert!(told.len() <= lamp_gemini::wire::MAX_NOTE_BYTES, "{told}");
                assert_eq!(
                    told.matches('"').count(),
                    note.matches('"').count(),
                    "{told}"
                );
                for english in sentences(&note) {
                    assert!(!told.contains(english), "{language:?}: {english}");
                }
                assert_eq!(
                    super::super::fixed_words(&told).is_some(),
                    super::super::fixed_words(&note).is_some(),
                    "{told}"
                );
            }
        }
    }

    #[test]
    fn the_call_name_stays_lamp_and_no_line_breaks_its_quotes() {
        for (english, said) in LINES {
            for line in said {
                assert!(!line.contains('"'), "{line}");
                assert!(!line.contains("ランプ"), "{line}");
                assert_eq!(line.contains("Hey Lamp"), english.contains("Hey Lamp"));
                assert_eq!(line.matches('{').count(), english.matches('{').count());
            }
        }
    }

    #[test]
    fn a_slot_is_filled_in_the_language() {
        let privacy = "(Say only this, nothing before or after it: \"Find my privacy switch on my back. Close it, then open it again.\" Do not explain it, claim success or ask a tool to mark privacy done.)";
        let told = said(privacy.into(), Some(Language::Japanese));
        assert!(told.contains("プライバシースイッチを探してください（on my back）。"));
        assert!(
            told.ends_with(
                "\" Do not explain it, claim success or ask a tool to mark privacy done.)"
            )
        );
        let confirm =
            "(Otherwise ask them, in a few words: \"Did I get that right, Linh?\" Nothing more.)";
        assert_eq!(
            said(confirm.into(), Some(Language::Vietnamese)),
            "(Otherwise ask them, in a few words: \"Linh, mình nghe đúng chưa?\" Nothing more.)"
        );
    }

    /// An unknown language is English: nothing here changes.
    #[test]
    fn an_unknown_language_falls_back_to_english() {
        let chosen = Language::parse("fr").ok().flatten();
        assert_eq!(chosen, None);
        let note = super::super::skill::PREFERENCES_HELLO.to_owned();
        assert_eq!(said(note.clone(), chosen), note);
    }

    /// Lines in one language: a plain yes, a plain no, a question, the
    /// hello's yes, a quiet-hours answer naming a time, a greeting.
    struct Said {
        yes: &'static [&'static str],
        no: &'static [&'static str],
        questions: &'static [&'static str],
        hello_yes: &'static [&'static str],
        times: &'static [&'static str],
        greetings: &'static [&'static str],
        names: &'static [&'static str],
    }

    fn said_in(language: Language) -> Said {
        match language {
            Language::Vietnamese => Said {
                yes: &["Có.", "Vâng.", "Được, không sao.", "Ừ, đúng rồi"],
                no: &["Không.", "Không, cảm ơn.", "Đừng nhé."],
                questions: &["Giờ yên lặng là gì?", "Bạn nói gì cơ?", "Nghĩa là sao?"],
                hello_yes: &["Có, làm ơn.", "Được ạ."],
                times: &[
                    "Sau mười giờ tối.",
                    "Từ 10 giờ đêm đến 6 giờ sáng.",
                    "sau mười giờ tối",
                ],
                greetings: &["Xin chào Lamp.", "Chào buổi tối."],
                names: &["Linh", "Tên mình là Linh."],
            },
            Language::Japanese => Said {
                yes: &["はい。", "うん、いいよ。", "はい、問題ないです。"],
                no: &["いいえ。", "いや、やめてください。", "だめです。"],
                questions: &[
                    "静かな時間って何？",
                    "もう一度言ってもらえますか",
                    "何時から？",
                ],
                hello_yes: &["はい、お願いします。", "いいですよ。"],
                times: &["夜十時以降。", "夜10時から朝6時まで。", "夜十時以降"],
                greetings: &["こんにちは、Lamp。", "こんばんは。"],
                names: &["さくら", "名前はさくらです。"],
            },
            Language::Chinese => Said {
                yes: &["好的。", "可以。", "是的，没问题。"],
                no: &["不。", "不要。", "不用了，谢谢。"],
                questions: &["安静时间是什么？", "你说什么", "什么时候？"],
                hello_yes: &["好的，可以。", "可以啊。"],
                times: &["晚上十点以后。", "晚上10点到早上6点。", "晚上十点以后"],
                greetings: &["你好，Lamp。", "晚上好。"],
                names: &["小明", "我叫小明。"],
            },
            Language::Korean => Said {
                yes: &["네.", "네, 좋아요.", "물론이죠."],
                no: &["아니요.", "아니요, 괜찮아요.", "싫어요."],
                questions: &["조용한 시간이 뭐예요?", "다시 말해 주세요", "뭐라고요"],
                hello_yes: &["네, 부탁해요.", "좋아요."],
                times: &[
                    "밤 열 시 이후요.",
                    "밤 10시부터 아침 6시까지.",
                    "밤 열 시 이후",
                ],
                greetings: &["안녕하세요, Lamp.", "좋은 아침이에요."],
                names: &["민수", "제 이름은 민수예요."],
            },
        }
    }

    #[test]
    fn each_language_reads_a_plain_yes_and_a_plain_no() {
        for language in ALL {
            let lines = said_in(language);
            for line in lines.yes {
                let read = read(language, line);
                assert_eq!(read.yes_or_no, Some(true), "{language:?}: {line}");
                assert!(read.agrees && !read.refuses, "{language:?}: {line}");
            }
            for line in lines.no {
                let read = read(language, line);
                assert_eq!(read.yes_or_no, Some(false), "{language:?}: {line}");
                assert!(read.refuses && !read.agrees, "{language:?}: {line}");
                // A no answers the quiet hours: none.
                assert!(read.quiet_hours, "{language:?}: {line}");
            }
        }
    }

    #[test]
    fn each_language_takes_a_question_as_no_answer() {
        for language in ALL {
            for line in said_in(language).questions {
                let read = read(language, line);
                assert!(read.asks || read.again, "{language:?}: {line}");
                assert!(!read.names_someone, "{language:?}: {line}");
            }
        }
        // A full-width mark is a question too.
        assert!(read(Language::Japanese, "本当？").asks);
        assert!(read(Language::Chinese, "真的？").asks);
        // The question's own words name no time.
        assert!(!read(Language::Vietnamese, "Giờ yên lặng là gì?").quiet_hours);
        assert!(!read(Language::Japanese, "静かな時間って何？").quiet_hours);
        assert!(!read(Language::Chinese, "安静时间是什么？").quiet_hours);
        assert!(!read(Language::Korean, "조용한 시간이 뭐예요?").quiet_hours);
    }

    #[test]
    fn each_language_answers_the_hello_and_greets() {
        for language in ALL {
            let lines = said_in(language);
            for line in lines.hello_yes {
                let read = read(language, line);
                assert!(read.hello_yes && !read.names_hours, "{language:?}: {line}");
            }
            for line in lines.greetings {
                let read = read(language, line);
                assert!(read.greets, "{language:?}: {line}");
                // A greeting is neither the hello's yes nor a time.
                assert!(!read.hello_yes && !read.quiet_hours, "{language:?}: {line}");
            }
        }
    }

    #[test]
    fn each_language_takes_a_quiet_hours_answer_naming_a_time() {
        for language in ALL {
            for line in said_in(language).times {
                let read = read(language, line);
                assert!(read.quiet_hours && read.names_hours, "{language:?}: {line}");
                assert!(
                    !read.asks && !read.again && !read.greets,
                    "{language:?}: {line}"
                );
            }
        }
    }

    /// Reviewer-three's P1 and Fourth's P2 on #4506: a maybe, a filler, a
    /// "let me think" or a question without its mark is never a consent and
    /// never the hello's answer, whatever yes word it holds; a plain no
    /// still is one.
    #[test]
    fn a_maybe_a_filler_or_a_question_never_consents() {
        for (language, lines) in [
            (
                Language::Vietnamese,
                &[
                    "Có lẽ",
                    "Có lẽ vậy",
                    "Có thể",
                    "Được rồi, để mình nghĩ đã",
                    "Ừ, để mình nghĩ đã",
                    "Để sau nhé",
                    "Được, nhưng giải thích thêm đã",
                    "Có khi",
                    "Chắc là được",
                    "Vâng, để sau nhé",
                    "Có, có khi",
                    "Có, chắc vậy",
                    "Được, chưa chắc",
                    "Có, nhưng để sau",
                    "Vâng, lát nữa",
                    "Có, không biết nữa",
                ][..],
            ),
            (
                Language::Japanese,
                &[
                    "そうですね",
                    "ええと",
                    "ええと、わからない",
                    "ふうん",
                    "はい、でもちょっと待って",
                    "はい、ちょっと考えさせて",
                    "うーん",
                    "ちょっと待って",
                    "いいですか",
                    "あとでね",
                    "はい、説明してください",
                    "はい、わかんない",
                    "はい、後で",
                    "はい、たぶん",
                    "うん、多分",
                    "はい、かも",
                    "はい、でも",
                    "はい、いいけど",
                    "はい、あとで",
                    "はい、まだ",
                    "はい、わからない",
                ][..],
            ),
            (
                Language::Chinese,
                &[
                    "可以吗",
                    "好吗",
                    "对了",
                    "好像是吧",
                    "好吧，我想想",
                    "嗯，好像是",
                    "让我想一下",
                    "等一下",
                    "稍后再说",
                    "好的，你先解释一下",
                    "好，或许吧",
                    "好，以后再说",
                    "好，也许",
                    "好的，可能吧",
                    "可以，但是",
                    "好，不过",
                    "好，等一下",
                    "好，再说吧",
                    "好，不知道",
                    "好，还没",
                ][..],
            ),
            (
                Language::Korean,
                &[
                    "그래서요",
                    "예를 들면요",
                    "그래요",
                    "음, 글쎄요",
                    "잠깐만요",
                    "생각해 볼게요",
                    "나중에요",
                    "네, 설명해 주세요",
                    "네, 그런데요",
                    "네, 어쩌면요",
                    "네, 나중에요",
                    "네, 그렇지만요",
                    "네, 그래도요",
                    "네, 혹시요",
                    "네, 이따가요",
                    "네, 다음에요",
                    "네, 모르겠어요",
                    "네, 아직요",
                    "네, 다만요",
                    "네, 음",
                    "됩니까",
                    "그런 건가요",
                    "할래요",
                    "그렇죠",
                ][..],
            ),
        ] {
            for line in lines {
                let read = read(language, line);
                // Neither the consent's yes nor any answer to the hello,
                // nor the yes that ends "who you are" (a Korean customer's
                // review of #4506, A1, and #4768's P1 in each language).
                assert_eq!(read.yes_or_no, None, "{language:?}: {line}");
                assert!(!read.agrees, "{language:?}: {line}");
                assert!(!read.hello_yes && !read.about_hello, "{language:?}: {line}");
            }
        }
        for (language, no) in [
            (Language::Vietnamese, "Không"),
            (Language::Japanese, "いいえ"),
            (Language::Chinese, "不要"),
            (Language::Korean, "아니요"),
        ] {
            assert_eq!(read(language, no).yes_or_no, Some(false), "{language:?}");
        }
        // A plain yes still agrees.
        for (language, lines) in [
            (
                Language::Vietnamese,
                &[
                    "Có",
                    "Được",
                    "Chắc chắn",
                    "Chắc chắn rồi",
                    "Được mà",
                    "Có mà",
                ][..],
            ),
            (Language::Japanese, &["はい", "うん", "もちろん"][..]),
            (Language::Chinese, &["好的", "可以", "是的", "当然"][..]),
            (Language::Korean, &["네", "좋아요"][..]),
        ] {
            for line in lines {
                assert!(read(language, line).agrees, "{language:?}: {line}");
            }
        }
    }

    /// A bare yes with a politeness ending, "please" or "thanks" is still a
    /// bare yes: it consents and answers the hello.
    #[test]
    fn a_bare_yes_with_please_or_politeness_consents() {
        for (language, lines) in [
            (
                Language::Vietnamese,
                &["Vâng ạ", "Có, làm ơn", "Được nhé", "Dạ vâng ạ"][..],
            ),
            (
                Language::Japanese,
                &[
                    "はい、お願いします",
                    "うん、いいよ",
                    "ええ、どうぞ",
                    "はい、そうです",
                ][..],
            ),
            (
                Language::Chinese,
                &["好的，谢谢", "可以，请", "对的", "是的呀"][..],
            ),
            (
                Language::Korean,
                &[
                    "네, 부탁해요",
                    "네, 좋아요",
                    "예, 감사합니다",
                    "네, 주세요",
                    // A Korean customer's review of #4506, A2.
                    "넵",
                    "네네",
                    "예예",
                    "넹",
                    "그럼요",
                    "당연하죠",
                    "당연하지",
                    "좋습니다",
                    "오케이",
                    "허락할게요",
                    "물론이죠",
                    "네, 그렇게 하세요",
                    "네, 그러세요",
                    "네, 하세요",
                    "네, 해도 돼요",
                    "네, 돼요",
                    "네, 됩니다",
                ][..],
            ),
        ] {
            for line in lines {
                let read = read(language, line);
                assert_eq!(read.yes_or_no, Some(true), "{language:?}: {line}");
                assert!(read.answers_hello && read.hello_yes, "{language:?}: {line}");
                assert!(!read.holds_hello, "{language:?}: {line}");
            }
        }
    }

    /// A Korean customer's review of #4506: what a Korean speaker means.
    #[test]
    fn korean_reads_as_a_korean_speaker_means_it() {
        let ko = |line| read(Language::Korean, line);
        // A2: maybe "no thanks" or "I heard you": asked again.
        for line in ["괜찮아요", "괜찮습니다", "알겠어요", "알겠습니다", "그래요"]
        {
            assert_eq!(ko(line).yes_or_no, None, "{line}");
        }
        assert!(!ko("그래요").agrees && ko("그래").agrees);
        // A5: a question without its mark; a bare yes ending so is none.
        for line in [
            "됩니까",
            "그런 건가요",
            "할래요",
            "그렇죠",
            "그렇지요",
            "맞는가요",
        ] {
            assert!(ko(line).asks, "{line}");
        }
        assert!(!ko("물론이죠").asks);
        // A3: a refusal, by its own syllables.
        for line in [
            "아닙니다",
            "안 됩니다",
            "하지 마세요",
            "하지 말아 주세요",
            "하지마",
            "안 해요",
            "말아 주세요",
        ] {
            let read = ko(line);
            assert_eq!(read.yes_or_no, Some(false), "{line}");
            assert!(read.refuses && !read.agrees, "{line}");
        }
        // A4: "음악" and "음성" are no "음".
        for line in ["네, 음악", "음성 좋아요"] {
            assert!(!ko(line).puts_off, "{line}");
        }
        assert!(ko("음").puts_off);
        // A6: a filler is no name.
        for line in [
            "잠시만요",
            "글쎄요",
            "나중에요",
            "그냥요",
            "몰라요",
            "램프",
            "헤이",
            "어쩌면요",
            "어",
        ] {
            assert!(!ko(line).names_someone, "{line}");
        }
        // A4 and A7: a name may start with a yes or a filler syllable.
        for line in ["예린", "저는 예린이에요", "예준", "응수", "음석", "어진"] {
            let read = ko(line);
            assert!(read.names_someone && !read.agrees, "{line}");
        }
        assert!(ko("네요").agrees && !ko("네요").names_someone);
        // A8: asking to hear it again.
        for line in [
            "안 들려요",
            "안 들렸어요",
            "못 알아들었어요",
            "한 번 더요",
            "다시 한 번",
        ] {
            let read = ko(line);
            assert!(read.again && read.holds_hello, "{line}");
            assert_eq!(read.yes_or_no, None, "{line}");
        }
        // A9: one o'clock is an hour.
        for line in ["새벽 한 시까지", "한시 이후"] {
            assert!(ko(line).quiet_hours, "{line}");
        }
        // A11: a greeting.
        for line in ["반가워요", "반갑습니다"] {
            let read = ko(line);
            assert!(read.greets && !read.names_someone, "{line}");
        }
    }

    #[test]
    fn each_language_hears_a_name() {
        for language in ALL {
            let lines = said_in(language);
            for line in lines.names {
                assert!(read(language, line).names_someone, "{language:?}: {line}");
            }
            for line in lines.yes.iter().chain(lines.no).chain(lines.greetings) {
                assert!(!read(language, line).names_someone, "{language:?}: {line}");
            }
        }
    }
}
