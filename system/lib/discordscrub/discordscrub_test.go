package discordscrub

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func decode(t *testing.T, s string) any {
	t.Helper()
	var v any
	if err := json.Unmarshal([]byte(s), &v); err != nil {
		t.Fatal(err)
	}
	return v
}

func TestRedactShapes(t *testing.T) {
	cases := []struct {
		name, in string
		changed  bool
		gone     []string
		kept     []string
	}{
		{"channels.discord deleted", `{"channels":{"discord":{"token":"S","enabled":true},"telegram":{"botToken":"T"}}}`, true, []string{"S", "discord"}, []string{"T"}},
		{"bare discord object", `{"discord":{"botToken":"S","enabled":true,"guild":"g"},"gateway":1}`, true, []string{"S"}, []string{"guild", "gateway"}},
		{"nested under plugins", `{"plugins":{"x":{"discord":{"app_token":"S"}}}}`, true, []string{"S"}, nil},
		{"flat keys", `{"discord_bot_token":"S","DISCORD_APP_TOKEN":"S","other":1}`, true, []string{"S"}, []string{"other"}},
		{"in array", `{"items":[{"discord":{"token":"S"}}]}`, true, []string{"S"}, nil},
		{"empty flat key untouched", `{"discord_bot_token":""}`, false, nil, nil},
		{"nothing discord", `{"channels":{"telegram":{"botToken":"T"}}}`, false, nil, []string{"T"}},
		{"already disabled", `{"discord":{"enabled":false}}`, false, nil, nil},
	}
	for _, c := range cases {
		v := decode(t, c.in)
		if got := Redact(v); got != c.changed {
			t.Errorf("%s: changed = %v, want %v", c.name, got, c.changed)
		}
		b, _ := json.Marshal(v)
		for _, g := range c.gone {
			if strings.Contains(string(b), g) {
				t.Errorf("%s: %q still present in %s", c.name, g, b)
			}
		}
		for _, k := range c.kept {
			if !strings.Contains(string(b), k) {
				t.Errorf("%s: %q lost from %s", c.name, k, b)
			}
		}
	}
}

func TestScrubFile(t *testing.T) {
	dir := t.TempDir()
	p := filepath.Join(dir, "x.json")
	if err := os.WriteFile(p, []byte(`{"channels":{"discord":{"token":"S"}},"keep":1}`), 0640); err != nil {
		t.Fatal(err)
	}
	changed, err := ScrubFile(p)
	if err != nil || !changed {
		t.Fatalf("ScrubFile = %v, %v", changed, err)
	}
	b, _ := os.ReadFile(p)
	if strings.Contains(string(b), "S\"") || !strings.Contains(string(b), "keep") {
		t.Errorf("content = %s", b)
	}
	if info, _ := os.Stat(p); info.Mode().Perm() != 0640 {
		t.Errorf("mode = %v, want 0640", info.Mode().Perm())
	}
	if changed, err := ScrubFile(p); err != nil || changed {
		t.Errorf("second pass = %v, %v", changed, err)
	}
	if changed, err := ScrubFile(filepath.Join(dir, "missing.json")); err != nil || changed {
		t.Errorf("missing file = %v, %v", changed, err)
	}
	bad := filepath.Join(dir, "bad.json")
	_ = os.WriteFile(bad, []byte("not json"), 0600)
	if _, err := ScrubFile(bad); err == nil {
		t.Error("a non-JSON file must be reported")
	}
	if b, _ := os.ReadFile(bad); string(b) != "not json" {
		t.Error("a non-JSON file must be left alone")
	}
}
