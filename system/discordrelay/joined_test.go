package discordrelay

import (
	"os"
	"path/filepath"
	"slices"
	"testing"
)

func TestJoinedThreadsSurviveRestart(t *testing.T) {
	path := filepath.Join(t.TempDir(), joinedFile)

	first := New(nil, nil)
	first.loadJoined(path)
	first.joinThread("111")
	first.joinThread("222")
	first.joinThread("111")

	second := New(nil, nil)
	second.loadJoined(path)
	if !slices.Equal(second.joined, []string{"111", "222"}) {
		t.Fatalf("joined after restart = %v", second.joined)
	}
}

func TestJoinedThreadsBadFileStartsEmpty(t *testing.T) {
	path := filepath.Join(t.TempDir(), joinedFile)
	if err := os.WriteFile(path, []byte("{not json"), 0o600); err != nil {
		t.Fatal(err)
	}
	r := New(nil, nil)
	r.loadJoined(path)
	if len(r.joined) != 0 {
		t.Fatalf("joined = %v", r.joined)
	}
	r.joinThread("333")
	again := New(nil, nil)
	again.loadJoined(path)
	if !slices.Equal(again.joined, []string{"333"}) {
		t.Fatalf("joined after rewrite = %v", again.joined)
	}
}

func TestJoinedThreadsCapOnLoad(t *testing.T) {
	path := filepath.Join(t.TempDir(), joinedFile)
	r := New(nil, nil)
	r.loadJoined(path)
	for i := 0; i < maxJoinedThreads+5; i++ {
		r.joinThread(string(rune('a'+i%26)) + string(rune('0'+i/26)))
	}
	again := New(nil, nil)
	again.loadJoined(path)
	if len(again.joined) != maxJoinedThreads {
		t.Fatalf("len = %d", len(again.joined))
	}
}
