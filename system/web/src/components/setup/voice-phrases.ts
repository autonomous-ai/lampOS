import { normalizeLang } from "@/lib/i18n";

export const VOICE_PHRASES_BY_LANG: Record<string, string[]> = {
  en: [
    "Hi there, I'm enrolling my voice so you can recognize me when we talk.",
    "The quick brown fox jumps over the lazy dog near the bright morning window.",
    "Today is a great day to start something new, and I'm looking forward to it.",
  ],
  ja: [
    "こんにちは。会話するときに私だと分かるように、声を登録しています。",
    "今日は新しいことを始めるのにぴったりの日です。これからが楽しみです。",
    "朝に温かいコーヒーを飲むと、すっきりした気分で一日の仕事を始められます。",
  ],
  ko: [
    "안녕하세요. 대화할 때 저를 알아볼 수 있도록 목소리를 등록하고 있어요.",
    "오늘은 새로운 일을 시작하기에 딱 좋은 날이에요. 앞으로가 기대돼요.",
    "아침에 따뜻한 커피 한 잔을 마시면 상쾌한 기분으로 하루를 시작할 수 있어요.",
  ],
  ar: [
    "مرحباً، أنا أسجّل صوتي حتى تتعرّف عليّ عندما نتحدث.",
    "اليوم يوم رائع لبدء شيء جديد، وأنا متحمس لما هو قادم.",
    "عندما أشرب فنجان قهوة دافئاً في الصباح، أبدأ يومي بنشاط وراحة.",
  ],
  vi: [
    "Xin chào, mình đang đăng ký giọng nói để bạn nhận ra mình khi nói chuyện.",
    "Hôm nay là một ngày tuyệt vời, mình rất mong chờ những điều mới mẻ phía trước.",
    "Một cốc cà phê nóng vào buổi sáng giúp mình tỉnh táo và bắt đầu công việc tốt hơn.",
  ],
  "zh-CN": [
    "你好，我正在录入我的声音，这样你听到我说话就能认出我。",
    "今天天气不错，是开始新事情的好日子，我期待接下来的一切。",
    "早晨喝一杯热咖啡能让我精神焕发，更好地开始一天的工作。",
  ],
  "zh-TW": [
    "你好，我正在錄入我的聲音，這樣你聽到我說話就能認出我。",
    "今天天氣不錯，是開始新事情的好日子，我期待接下來的一切。",
    "早晨喝一杯熱咖啡能讓我精神煥發，更好地開始一天的工作。",
  ],
};

export const VOICE_INTRO_BY_LANG: Record<string, string> = {
  en: "Stand near the robot. When recording starts, read the 3 sentences in a normal voice. The robot's mic captures you — your laptop mic is not used.",
  ja: "ロボットの近くに立ってください。録音が始まったら、次の3つの文を普段どおりの声で読んでください。録音にはロボットのマイクを使用します。パソコンのマイクは使用しません。",
  ko: "로봇 가까이에 서 주세요. 녹음이 시작되면 아래 3문장을 평소 목소리로 읽어 주세요. 녹음은 로봇의 마이크로 하며, 노트북 마이크는 사용하지 않아요.",
  ar: "قف بالقرب من الروبوت. عندما يبدأ التسجيل، اقرأ الجمل الثلاث بصوتك العادي. يسجّل ميكروفون الروبوت صوتك، ولا يُستخدم ميكروفون الحاسوب.",
  vi: "Đứng gần robot. Khi bắt đầu ghi âm, đọc 3 câu sau với giọng bình thường. Mic của robot sẽ thu âm bạn — không dùng mic của máy tính.",
  "zh-CN": "站在机器人附近。开始录音后，用正常语速朗读这 3 句话。机器人的麦克风会录下你的声音 — 不使用电脑麦克风。",
  "zh-TW": "站在機器人附近。開始錄音後，用正常語速朗讀這 3 句話。機器人的麥克風會錄下你的聲音 — 不使用電腦麥克風。",
};

export const VOICE_DURATION_SEC = 15;

export function pickVoicePhrases(sttLanguage: string): string[] {
  return VOICE_PHRASES_BY_LANG[normalizeLang(sttLanguage)] ?? VOICE_PHRASES_BY_LANG.en;
}

export function pickVoiceIntro(sttLanguage: string): string {
  return VOICE_INTRO_BY_LANG[normalizeLang(sttLanguage)] ?? VOICE_INTRO_BY_LANG.en;
}
