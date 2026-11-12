package hal

import "net/http"

// SyncSleepMode asks HAL to apply the saved sleep_mode toggle: with it off, a
// sleeping device wakes quietly. HAL reads the toggle from config.json itself.
func SyncSleepMode() error {
	var out struct {
		SleepMode bool `json:"sleep_mode"`
		Woke      bool `json:"woke"`
	}
	return doJSON(http.MethodPost, "/system/sleep-mode-sync", nil, &out)
}
