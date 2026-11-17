import { useSyncExternalStore } from "react";

export const LANG = {
  EN: "en",
  VI: "vi",
  JA: "ja",
  KO: "ko",
  AR: "ar",
  ZH_CN: "zh-CN",
  ZH_TW: "zh-TW",
} as const;

export type Lang = (typeof LANG)[keyof typeof LANG];

const FALLBACK_LANG: Lang = LANG.EN;

// Normalises an STT language code or alias onto a canonical Lang (unknown -> English).
export function normalizeLang(code: string | undefined | null): Lang {
  if (!code) return FALLBACK_LANG;
  const c = code.trim().toLowerCase().replaceAll("_", "-");
  if (c === "ja" || c.startsWith("ja-")) return LANG.JA;
  if (c === "ko" || c.startsWith("ko-")) return LANG.KO;
  if (c === "ar" || c.startsWith("ar-")) return LANG.AR;
  if (c === "vi" || c.startsWith("vi-")) return LANG.VI;
  if (c === "zh-tw" || c === "zh-hant" || c.startsWith("zh-hant-") || c === "zh-hk" || c === "zh-mo") return LANG.ZH_TW;
  if (c === "zh" || c === "zh-cn" || c === "zh-hans" || c.startsWith("zh-hans-") || c.startsWith("zh-")) return LANG.ZH_CN;
  if (c === "en" || c.startsWith("en-")) return LANG.EN;
  return FALLBACK_LANG;
}

let active: Lang = FALLBACK_LANG;
const listeners = new Set<() => void>();

// Sets the active language from a raw or alias code; no-op when unchanged.
export function setLanguage(code: string | undefined | null): void {
  const next = normalizeLang(code);
  if (next === active) return;
  active = next;
  for (const l of listeners) l();
}

// getLanguage returns the active canonical language.
export function getLanguage(): Lang {
  return active;
}

function subscribe(cb: () => void): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

type Catalogue = Record<string, Partial<Record<Lang, string>> & { en: string }>;

const strings: Catalogue = {
  "chat.empty.title": {
    en: "Chat with Assistant",
    ja: "アシスタントとチャット",
    ko: "어시스턴트와 채팅",
    ar: "الدردشة مع المساعد",
    vi: "Trò chuyện với Assistant",
    "zh-CN": "与助手聊天",
    "zh-TW": "與助手聊天",
  },
  "chat.empty.subtitle": {
    en: "Ask anything, or try one of these",
    ja: "何でも聞いてみてください。または、次の例をお試しください",
    ko: "무엇이든 물어보거나 아래 예시를 눌러 보세요",
    ar: "اسأل أي شيء، أو جرّب أحد هذه الاقتراحات",
    vi: "Hỏi bất cứ điều gì, hoặc thử một gợi ý",
    "zh-CN": "随便问，或试试以下建议",
    "zh-TW": "隨便問，或試試以下建議",
  },

  "chat.suggest.music": {
    en: "Play a relaxing song",
    ja: "リラックスできる曲をかけて",
    ko: "편안한 노래 틀어 줘",
    ar: "شغّل أغنية هادئة",
    vi: "Mở một bài nhạc thư giãn",
    "zh-CN": "播放一首轻松的歌",
    "zh-TW": "播放一首輕鬆的歌",
  },
  "chat.suggest.howAreYou": {
    en: "How are you today?",
    ja: "今日は元気？",
    ko: "오늘 기분 어때?",
    ar: "كيف حالك اليوم؟",
    vi: "Hôm nay bạn thế nào?",
    "zh-CN": "你今天怎么样？",
    "zh-TW": "你今天好嗎？",
  },
  "chat.suggest.warmLight": {
    en: "Set a warm light mood",
    ja: "暖かい色の照明にして",
    ko: "따뜻한 조명으로 바꿔 줘",
    ar: "اجعل الإضاءة دافئة",
    vi: "Chỉnh ánh sáng ấm áp",
    "zh-CN": "调成温暖的灯光",
    "zh-TW": "調成溫暖的燈光",
  },
  "chat.suggest.whatCanYouDo": {
    en: "What can you do?",
    ja: "どんなことができる？",
    ko: "뭘 할 수 있어?",
    ar: "ماذا تستطيع أن تفعل؟",
    vi: "Bạn làm được gì?",
    "zh-CN": "你能做什么？",
    "zh-TW": "你能做什麼？",
  },

  "chat.status.thinking": {
    en: "Assistant is thinking…",
    ja: "アシスタントが考えています…",
    ko: "어시스턴트가 생각하는 중…",
    ar: "المساعد يفكر…",
    vi: "Assistant đang suy nghĩ…",
    "zh-CN": "助手正在思考…",
    "zh-TW": "助手正在思考…",
  },
  "chat.status.online": {
    en: "Assistant · online",
    ja: "アシスタント · オンライン",
    ko: "어시스턴트 · 온라인",
    ar: "المساعد · متصل",
    vi: "Assistant · trực tuyến",
    "zh-CN": "助手 · 在线",
    "zh-TW": "助手 · 在線",
  },

  "chat.time.now": {
    en: "now",
    ja: "たった今",
    ko: "방금",
    ar: "الآن",
    vi: "vừa xong",
    "zh-CN": "刚刚",
    "zh-TW": "剛剛",
  },
  "chat.time.minutes": {
    en: "{n}m",
    ja: "{n}分前",
    ko: "{n}분 전",
    ar: "قبل {n} د",
    vi: "{n} phút",
    "zh-CN": "{n}分钟前",
    "zh-TW": "{n}分鐘前",
  },
  "chat.time.hours": {
    en: "{n}h",
    ja: "{n}時間前",
    ko: "{n}시간 전",
    ar: "قبل {n} س",
    vi: "{n} giờ",
    "zh-CN": "{n}小时前",
    "zh-TW": "{n}小時前",
  },
  "chat.time.yesterday": {
    en: "yesterday",
    ja: "昨日",
    ko: "어제",
    ar: "أمس",
    vi: "hôm qua",
    "zh-CN": "昨天",
    "zh-TW": "昨天",
  },
  "chat.time.days": {
    en: "{n}d",
    ja: "{n}日前",
    ko: "{n}일 전",
    ar: "قبل {n} ي",
    vi: "{n} ngày",
    "zh-CN": "{n}天前",
    "zh-TW": "{n}天前",
  },
};

// Looks up `key` in the active language, falling back to English, then to the key; `params` fills `{name}` placeholders.
export function t(key: string, params?: Record<string, string | number>, lang?: Lang): string {
  const entry = strings[key];
  if (!entry) return key;
  const l = lang ?? active;
  let out = entry[l] ?? entry.en;
  if (params) {
    for (const k in params) out = out.replace(`{${k}}`, String(params[k]));
  }
  return out;
}

// Returns a t() bound to the active language; re-renders when it changes.
export function useT(): (key: string, params?: Record<string, string | number>) => string {
  const lang = useSyncExternalStore(subscribe, getLanguage, getLanguage);
  return (key, params) => t(key, params, lang);
}
