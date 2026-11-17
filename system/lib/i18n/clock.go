package i18n

import (
	"fmt"
	"time"
)

// TimeOfDay answers "what time is it?" for t in the active language.
func TimeOfDay(t time.Time) string {
	return TimeOfDayIn(t, Lang())
}

// TimeOfDayIn is TimeOfDay for an explicit language; unknown languages use English.
func TimeOfDayIn(t time.Time, lang string) string {
	h, m := t.Hour(), t.Minute()
	h12 := h % 12
	if h12 == 0 {
		h12 = 12
	}
	pm := h >= 12
	switch NormalizeLang(lang) {
	case LangVI:
		if m == 0 {
			return fmt.Sprintf("Bây giờ là %d giờ.", h)
		}
		return fmt.Sprintf("Bây giờ là %d giờ %d phút.", h, m)
	case LangJA:
		period := pick(pm, "午後", "午前")
		if m == 0 {
			return fmt.Sprintf("今は%s%d時です。", period, h12)
		}
		return fmt.Sprintf("今は%s%d時%d分です。", period, h12, m)
	case LangKO:
		period := pick(pm, "오후", "오전")
		if m == 0 {
			return fmt.Sprintf("지금은 %s %d시예요.", period, h12)
		}
		return fmt.Sprintf("지금은 %s %d시 %d분이에요.", period, h12, m)
	case LangZhCN, LangZh, LangZhHans:
		period := pick(pm, "下午", "上午")
		if m == 0 {
			return fmt.Sprintf("现在是%s%d点整。", period, h12)
		}
		return fmt.Sprintf("现在是%s%d点%d分。", period, h12, m)
	case LangZhTW, LangZhHant:
		period := pick(pm, "下午", "上午")
		if m == 0 {
			return fmt.Sprintf("現在是%s%d點整。", period, h12)
		}
		return fmt.Sprintf("現在是%s%d點%d分。", period, h12, m)
	case LangAR:
		return fmt.Sprintf("الساعة الآن %d:%02d %s.", h12, m, pick(pm, "مساءً", "صباحاً"))
	default:
		return fmt.Sprintf("It's %s.", t.Format("3:04 PM"))
	}
}

func pick(cond bool, yes, no string) string {
	if cond {
		return yes
	}
	return no
}
