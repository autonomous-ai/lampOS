package mqtthandler

import (
	"strings"
	"testing"
)

func TestParseLoadavg(t *testing.T) {
	l1, l5, l15 := parseLoadavg("1.50 0.75 0.25 2/345 6789\n")
	if l1 != 1.5 || l5 != 0.75 || l15 != 0.25 {
		t.Fatalf("got %v %v %v", l1, l5, l15)
	}
	if a, b, c := parseLoadavg("garbage"); a != 0 || b != 0 || c != 0 {
		t.Fatalf("short input should give zeros, got %v %v %v", a, b, c)
	}
}

func TestParseMeminfo(t *testing.T) {
	got := parseMeminfo("MemTotal:        8192000 kB\nMemFree: 100 kB\nMemAvailable:    2048000 kB\nSwapTotal:       1024000 kB\nSwapFree:         512000 kB\n")
	if got.TotalMB != 8000 || got.AvailableMB != 2000 || got.SwapTotalMB != 1000 || got.SwapUsedMB != 500 {
		t.Fatalf("unexpected %+v", got)
	}
	if got.UsedPercent < 74.9 || got.UsedPercent > 75.1 {
		t.Fatalf("used percent %v, want 75", got.UsedPercent)
	}
	if parseMeminfo("").TotalMB != 0 {
		t.Fatal("empty input should give zeros")
	}
}

func TestParseSystemctlShow(t *testing.T) {
	p := parseSystemctlShow("ActiveState=failed\nSubState=failed\nResult=exit-code\nExecMainStatus=1\nNRestarts=4\n")
	if p["ActiveState"] != "failed" || p["Result"] != "exit-code" || p["NRestarts"] != "4" {
		t.Fatalf("unexpected %v", p)
	}
}

func TestErrorLinesKeepsNewestOldestFirst(t *testing.T) {
	journal := strings.Join([]string{
		"hal.service: Main process exited, code=exited, status=1/FAILURE",
		"hal.service: Failed with result 'exit-code'.",
		`  File "/opt/hal/server.py", line 40, in main`,
		"info all good",
		"Traceback (most recent call last):",
		"ModuleNotFoundError: No module named 'torch'",
	}, "\n")
	got := errorLines(journal, 2, 300)
	want := []string{`  File "/opt/hal/server.py", line 40, in main`, "ModuleNotFoundError: No module named 'torch'"}
	if len(got) != 2 || got[0] != want[0] || got[1] != want[1] {
		t.Fatalf("got %q, want %q", got, want)
	}
}

func TestErrorLinesNeverReturnsUserText(t *testing.T) {
	journal := strings.Join([]string{
		"INFO user said: turn on the lights, error in my heart",
		"WARNING could not parse 'play the failed experiment'",
		"ERROR bad value from user text",
		"Harness voice mode unavailable: asked about the fatal crash",
		"ValueError: invalid literal for int() with base 10: 'my secret'",
		"KeyError",
		"hal.service: I heard 'call mum' and then something private",
		"ImportError: cannot import name 'my_secret' from 'private_thing'",
		"ModuleNotFoundError: No module named 'x'; the user said hello",
	}, "\n")
	got := errorLines(journal, 10, 300)
	want := []string{"ValueError:", "KeyError:", "ImportError:", "ModuleNotFoundError:"}
	if len(got) != len(want) {
		t.Fatalf("got %q, want %q", got, want)
	}
	for i := range want {
		if got[i] != want[i] {
			t.Fatalf("got %q, want %q", got, want)
		}
	}
	for _, l := range got {
		for _, bad := range []string{"secret", "lights", "experiment", "heart", "crash"} {
			if strings.Contains(l, bad) {
				t.Fatalf("user text leaked in %q", l)
			}
		}
	}
}

func TestErrorLinesCutsLengthAndHandlesEmpty(t *testing.T) {
	got := errorLines(`  File "/`+strings.Repeat("x", 500)+`", line 1, in f`, 5, 50)
	if len(got) != 1 || len(got[0]) != 50 {
		t.Fatalf("got %q", got)
	}
	if errorLines("", 5, 50) != nil {
		t.Fatal("empty journal should give no lines")
	}
}
