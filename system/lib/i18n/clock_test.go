package i18n

import (
	"testing"
	"time"
)

func TestTimeOfDayIn(t *testing.T) {
	afternoon := time.Date(2026, 11, 13, 15, 4, 0, 0, time.UTC)
	morningHour := time.Date(2026, 11, 13, 9, 0, 0, 0, time.UTC)
	midnight := time.Date(2026, 11, 13, 0, 30, 0, 0, time.UTC)
	cases := []struct {
		lang string
		at   time.Time
		want string
	}{
		{"", afternoon, "It's 3:04 PM."},
		{"en", midnight, "It's 12:30 AM."},
		{"fr", afternoon, "It's 3:04 PM."},
		{"vi", afternoon, "Bây giờ là 15 giờ 4 phút."},
		{"vi", morningHour, "Bây giờ là 9 giờ."},
		{"ja", afternoon, "今は午後3時4分です。"},
		{"ja-JP", morningHour, "今は午前9時です。"},
		{"ko", afternoon, "지금은 오후 3시 4분이에요."},
		{"ko-KR", morningHour, "지금은 오전 9시예요."},
		{"ko", midnight, "지금은 오전 12시 30분이에요."},
		{"zh-CN", afternoon, "现在是下午3点4分。"},
		{"zh-CN", morningHour, "现在是上午9点整。"},
		{"zh-TW", afternoon, "現在是下午3點4分。"},
		{"ar", afternoon, "الساعة الآن 3:04 مساءً."},
		{"ar-SA", morningHour, "الساعة الآن 9:00 صباحاً."},
	}
	for _, tc := range cases {
		if got := TimeOfDayIn(tc.at, tc.lang); got != tc.want {
			t.Errorf("TimeOfDayIn(%s, %q) = %q, want %q", tc.at.Format("15:04"), tc.lang, got, tc.want)
		}
	}
}
