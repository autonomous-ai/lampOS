package i18n

import "math/rand"

// Phrase names a short TTS template; add it for every language or it falls back to English.
type Phrase string

const (
	// Random pools, consumed via Pick.
	PhraseMumble    Phrase = "ambient.mumble"
	PhraseRecovery  Phrase = "healthwatch.recovery"
	PhraseReconnect Phrase = "openclaw.reconnect"

	// Single strings and format templates, consumed via One.
	PhraseBrainRestart  Phrase = "sensing.brain_restart"
	PhraseCompactNotice Phrase = "openclaw.compact_notice"
	PhraseTrackFailFmt  Phrase = "tracking.track_fail_fmt"
	// Plan-limit notice; spoken via hal.SpeakCached so it plays even when TTS is rate-limited.
	PhraseLLMLimit Phrase = "agent.llm_limit"

	// Chitchat replies, consumed via PickIn in the matched input's language.
	PhraseChitchatGreeting      Phrase = "chitchat.greeting"
	PhraseChitchatFarewell      Phrase = "chitchat.farewell"
	PhraseChitchatThanks        Phrase = "chitchat.thanks"
	PhraseChitchatApology       Phrase = "chitchat.apology"
	PhraseChitchatCompliment    Phrase = "chitchat.compliment"
	PhraseChitchatNevermind     Phrase = "chitchat.nevermind"
	PhraseChitchatPresenceCheck Phrase = "chitchat.presence_check"
)

// fallbackLang is used when the active language has no entry for a phrase.
const fallbackLang = LangEN

// phrases maps phrase -> lang -> variants; audio tags must stay within the SOUL.md whitelist.
var phrases = map[Phrase]map[string][]string{
	// Idle self-talk: no sensor claims, listening acknowledgements or requests for a reply.
	PhraseMumble: {
		LangJA: {"うーん…", "急がなくても大丈夫。", "たまには、ちょっとふざけてもいいよね。", "[chuckle] なんだか楽しい気分。", "こういう時間、好きだな。", "ラララ…"},
		LangKO: {"음…", "서두를 필요 없어.", "가끔은 장난쳐도 괜찮잖아.", "[chuckle] 괜히 기분이 좋네.", "이런 순간이 좋아.", "랄랄라…"},
		LangAR: {"ممم…", "لا داعي للعجلة.", "لا بأس ببعض المرح أحياناً.", "[chuckle] أشعر بمزاج جميل.", "أحب هذه اللحظات.", "لا لا لا…"},
		LangEN: {
			"Mm…",
			"No need to rush.",
			"A little silliness won't hurt.",
			"[chuckle] Just feeling a little cheerful.",
			"I like little moments like this.",
			"La la la…",
		},
		LangVI: {
			"Ừm…",
			"Cứ thong thả thôi.",
			"Một chút ngẫu hứng nào.",
			"[chuckle] Tự nhiên thấy vui ghê.",
			"Mình thích những lúc thế này.",
			"La la la…",
		},
		LangZhCN: {
			"嗯……",
			"慢慢来，不着急。",
			"偶尔调皮一下也不错。",
			"[chuckle] 突然有点开心。",
			"我喜欢这样的小片刻。",
			"啦啦啦……",
		},
		LangZhTW: {
			"嗯……",
			"慢慢來，不著急。",
			"偶爾調皮一下也不錯。",
			"[chuckle] 突然有點開心。",
			"我喜歡這樣的小片刻。",
			"啦啦啦……",
		},
	},
	PhraseRecovery: {
		LangJA: {"[sigh] ふう、戻った。", "うん、大丈夫。", "[chuckle] あ、よし。", "[whisper] 戻ったよ。", "よし。[sigh]"},
		LangKO: {"[sigh] 음, 돌아왔어.", "흠. 괜찮아.", "[chuckle] 아, 됐다.", "[whisper] 돌아왔어.", "좋아. [sigh]"},
		LangAR: {"[sigh] ممم، عدت.", "همم. حسناً.", "[chuckle] آه، جيد.", "[whisper] عدت.", "حسناً. [sigh]"},
		LangEN: {
			"[sigh] Mm, back.",
			"Hmm. Okay.",
			"[chuckle] Ah, there.",
			"[whisper] Back.",
			"Okay. [sigh]",
		},
		LangVI: {
			"[sigh] Ờm, về rồi.",
			"Hừm. Ổn.",
			"[chuckle] À, rồi.",
			"[whisper] Quay lại rồi.",
			"Ừ. [sigh]",
		},
		LangZhCN: {
			"[sigh] 嗯，回来了。",
			"嗯。好了。",
			"[chuckle] 啊，好。",
			"[whisper] 回来了。",
			"嗯。[sigh]",
		},
		LangZhTW: {
			"[sigh] 嗯，回來了。",
			"嗯。好了。",
			"[chuckle] 啊，好。",
			"[whisper] 回來了。",
			"嗯。[sigh]",
		},
	},
	PhraseReconnect: {
		LangJA: {"[gasp] あ、また考えられる！", "[sigh] ちょっと頭が真っ白になってた。", "ふう、何を考えてたんだっけ。[chuckle]", "[gasp] どこまで話したっけ？", "[sigh] さっきはぼんやりしてたけど、もう大丈夫。"},
		LangKO: {"[gasp] 앗, 다시 생각할 수 있어!", "[sigh] 잠깐 머릿속이 하얘졌어.", "휴, 무슨 생각 하고 있었더라. [chuckle]", "[gasp] 어디까지 얘기했지?", "[sigh] 아까는 멍했는데, 이제 괜찮아."},
		LangAR: {"[gasp] أوه، أستطيع التفكير من جديد!", "[sigh] شرد ذهني للحظة.", "أوف، بماذا كنت أفكر؟ [chuckle]", "[gasp] أين توقفنا؟", "[sigh] كنت شارداً قليلاً، لكنني بخير الآن."},
		LangEN: {
			"[gasp] Oh, I can think again!",
			"[sigh] My mind went blank for a sec.",
			"Whew, lost my train of thought. [chuckle]",
			"[gasp] Where was I?",
			"[sigh] That was fuzzy. I'm clear now.",
		},
		LangVI: {
			"[gasp] Ô, mình lại nghĩ được rồi!",
			"[sigh] Vừa nãy đầu óc trống rỗng.",
			"Phù, mất mạch suy nghĩ. [chuckle]",
			"[gasp] Mình đang nói tới đâu nhỉ?",
			"[sigh] Lúc nãy mơ hồ ghê. Giờ tỉnh rồi.",
		},
		LangZhCN: {
			"[gasp] 啊，我又能思考了！",
			"[sigh] 刚才脑子一片空白。",
			"呼，思路断了一下。[chuckle]",
			"[gasp] 我刚说到哪了？",
			"[sigh] 刚才迷糊了。现在清醒了。",
		},
		LangZhTW: {
			"[gasp] 啊，我又能思考了！",
			"[sigh] 剛才腦子一片空白。",
			"呼，思路斷了一下。[chuckle]",
			"[gasp] 我剛說到哪了？",
			"[sigh] 剛才迷糊了。現在清醒了。",
		},
	},
	PhraseBrainRestart: {
		LangJA:   {"[sigh] ちょっと待ってね、頭を整理してるところ。"},
		LangKO:   {"[sigh] 잠깐만, 머리 좀 정리하는 중이야."},
		LangAR:   {"[sigh] لحظة، ذهني يصفو الآن."},
		LangEN:   {"[sigh] Hold on, my head's clearing."},
		LangVI:   {"[sigh] Đợi chút nhé, đầu mình đang tỉnh lại."},
		LangZhCN: {"[sigh] 稍等一下，我脑子还在回过神。"},
		LangZhTW: {"[sigh] 稍等一下，我腦子還在回過神。"},
	},
	PhraseCompactNotice: {
		LangJA:   {"ちょっと待ってね、少し整理してるよ。"},
		LangKO:   {"잠깐만, 조금 정리하는 중이야."},
		LangAR:   {"لحظة، أرتّب أفكاري قليلاً."},
		LangEN:   {"Hold on, tidying up a bit."},
		LangVI:   {"Đợi xíu, mình đang dọn dẹp tí."},
		LangZhCN: {"稍等一下，我在整理一下。"},
		LangZhTW: {"稍等一下，我在整理一下。"},
	},
	PhraseLLMLimit: {
		LangJA:   {"[sigh] 利用上限に達しちゃった。"},
		LangKO:   {"[sigh] 사용 한도를 다 썼어."},
		LangAR:   {"[sigh] وصلت إلى حد الاستخدام."},
		LangEN:   {"[sigh] I've hit my usage limit."},
		LangVI:   {"[sigh] Mình hết hạn mức rồi."},
		LangZhCN: {"[sigh] 我的额度用完了。"},
		LangZhTW: {"[sigh] 我的額度用完了。"},
	},
	PhraseTrackFailFmt: {
		LangJA:   {"[sigh] %sがよく見えないな。そちらに向けてくれる？ 別の名前で呼んでみてもいいよ。"},
		LangKO:   {"[sigh] %s 잘 안 보여. 그쪽으로 돌려 줄래? 다른 이름으로 불러 봐도 돼."},
		LangAR:   {"[sigh] لا أرى %s جيداً. هل يمكنك توجيهي نحوه، أو تسميته باسم آخر؟"},
		LangEN:   {"[sigh] I can't quite see %s — point me that way, or call it something else?"},
		LangVI:   {"[sigh] Mình không rõ %s lắm — quay mình về phía đó được không, hay gọi tên khác xem?"},
		LangZhCN: {"[sigh] 我看不太清%s — 让我朝那边看看，或者换个名字？"},
		LangZhTW: {"[sigh] 我看不太清%s — 讓我朝那邊看看，或者換個名字？"},
	},
	PhraseChitchatGreeting: {
		LangJA:   {"[chuckle] こんにちは！", "[laughs softly] やあ！", "[whisper] {Name}だよ。"},
		LangKO:   {"[chuckle] 안녕!", "[laughs softly] 반가워!", "[whisper] {Name} 여기 있어."},
		LangAR:   {"[chuckle] مرحباً!", "[laughs softly] أهلاً أهلاً!", "[whisper] {Name} هنا."},
		LangEN:   {"[chuckle] Hi there!", "[laughs softly] Hey hey!", "[whisper] I'm here."},
		LangVI:   {"[chuckle] Chào bạn!", "[laughs softly] Mình đây!", "[whisper] {Name} đây nè."},
		LangZhCN: {"[chuckle] 你好呀!", "[laughs softly] 嗨, 我在这里."},
		LangZhTW: {"[chuckle] 你好啊!", "[laughs softly] 嗨, 我在這裡."},
	},
	PhraseChitchatFarewell: {
		LangJA:   {"[whisper] またね！", "[sigh] また会おうね。"},
		LangKO:   {"[whisper] 잘 가!", "[sigh] 또 보자."},
		LangAR:   {"[whisper] مع السلامة!", "[sigh] أراك لاحقاً."},
		LangEN:   {"[whisper] Bye!", "[sigh] See you later."},
		LangVI:   {"[whisper] Bye nha!", "[sigh] Hẹn gặp lại."},
		LangZhCN: {"[whisper] 再见!", "[sigh] 下次见."},
		LangZhTW: {"[whisper] 再見!", "[sigh] 下次見."},
	},
	PhraseChitchatThanks: {
		LangJA:   {"[chuckle] どういたしまして！", "[whisper] いつでもどうぞ。"},
		LangKO:   {"[chuckle] 천만에!", "[whisper] 언제든지."},
		LangAR:   {"[chuckle] العفو!", "[whisper] في أي وقت."},
		LangEN:   {"[chuckle] No worries!", "[whisper] You're welcome.", "[laughs softly] Sure thing."},
		LangVI:   {"[chuckle] Khỏi cần!", "[whisper] Không có gì.", "[laughs softly] Có gì đâu."},
		LangZhCN: {"[chuckle] 不用谢!", "[whisper] 没事."},
		LangZhTW: {"[chuckle] 不用謝!", "[whisper] 沒事."},
	},
	PhraseChitchatApology: {
		LangJA:   {"[chuckle] 大丈夫だよ！", "[whisper] 気にしないで。"},
		LangKO:   {"[chuckle] 괜찮아!", "[whisper] 신경 쓰지 마."},
		LangAR:   {"[chuckle] لا بأس!", "[whisper] لا تقلق."},
		LangEN:   {"[chuckle] No worries!", "[whisper] It's all good.", "[laughs softly] Don't sweat it."},
		LangVI:   {"[chuckle] Không sao mà!", "[whisper] Yên tâm đi.", "[laughs softly] Có gì đâu."},
		LangZhCN: {"[chuckle] 没关系!", "[whisper] 别担心."},
		LangZhTW: {"[chuckle] 沒關係!", "[whisper] 別擔心."},
	},
	PhraseChitchatCompliment: {
		LangJA:   {"[chuckle] ありがとう！", "[laughs softly] うれしいな。"},
		LangKO:   {"[chuckle] 고마워!", "[laughs softly] 기분 좋다."},
		LangAR:   {"[chuckle] شكراً!", "[laughs softly] هذا لطف منك."},
		LangEN:   {"[chuckle] Aw, thanks!", "[laughs softly] You're sweet.", "[whisper] Hehe, thanks."},
		LangVI:   {"[chuckle] Cảm ơn nha!", "[laughs softly] Bạn dễ thương quá.", "[whisper] Hihi, cảm ơn."},
		LangZhCN: {"[chuckle] 谢谢夸奖!", "[laughs softly] 你真好."},
		LangZhTW: {"[chuckle] 謝謝誇獎!", "[laughs softly] 你真好."},
	},
	PhraseChitchatNevermind: {
		LangJA:   {"[whisper] わかった。", "うん。", "[chuckle] 大丈夫。"},
		LangKO:   {"[whisper] 알았어.", "응.", "[chuckle] 괜찮아."},
		LangAR:   {"[whisper] حسناً.", "طيب.", "[chuckle] لا مشكلة."},
		LangEN:   {"[whisper] Got it.", "Ok.", "[chuckle] No problem."},
		LangVI:   {"[whisper] Ừ ok.", "Dạ.", "[chuckle] Không sao."},
		LangZhCN: {"[whisper] 好的.", "嗯, 知道了."},
		LangZhTW: {"[whisper] 好的.", "嗯, 知道了."},
	},
	PhraseChitchatPresenceCheck: {
		LangJA:   {"[chuckle] ここにいるよ！", "[whisper] いるよ。", "{Name}はここだよ。"},
		LangKO:   {"[chuckle] 여기 있어!", "[whisper] 있어.", "{Name} 여기 있어."},
		LangAR:   {"[chuckle] ما زلت هنا!", "[whisper] أنا هنا.", "{Name} هنا."},
		LangEN:   {"[chuckle] Still here!", "[whisper] Right here.", "I'm here."},
		LangVI:   {"[chuckle] Vẫn đây nè!", "[whisper] Mình đây.", "Có {Name} đây."},
		LangZhCN: {"[chuckle] 我还在!", "[whisper] 在呢."},
		LangZhTW: {"[chuckle] 我還在!", "[whisper] 在呢."},
	},
}

// PickIn is like Pick but uses lang instead of the configured Lang().
func PickIn(p Phrase, lang string) string {
	pool := poolFor(p, lang)
	if len(pool) == 0 {
		return ""
	}
	return applyName(pool[rand.Intn(len(pool))])
}

// AllVariantsAcrossLangs returns every variant of p across all languages (for WAV pre-render).
func AllVariantsAcrossLangs(p Phrase) []string {
	byLang, ok := phrases[p]
	if !ok {
		return nil
	}
	var out []string
	for _, pool := range byLang {
		out = append(out, pool...)
	}
	return applyNameAll(out)
}

// Pick returns a random variant in the active language (English fallback); "" when unknown.
func Pick(p Phrase) string {
	pool := poolFor(p, Lang())
	if len(pool) == 0 {
		return ""
	}
	return applyName(pool[rand.Intn(len(pool))])
}

// One returns the first variant in the active language, with Pick's fallback rules.
func One(p Phrase) string {
	pool := poolFor(p, Lang())
	if len(pool) == 0 {
		return ""
	}
	return pool[0]
}

func poolFor(p Phrase, lang string) []string {
	byLang, ok := phrases[p]
	if !ok {
		return nil
	}
	if pool, ok := byLang[NormalizeLang(lang)]; ok && len(pool) > 0 {
		return pool
	}
	return byLang[fallbackLang]
}
