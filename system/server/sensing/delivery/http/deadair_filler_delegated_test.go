package http

import (
	"testing"

	"go.autonomous.ai/os/system/lib/i18n"
)

// The realtime model usually hands off without speaking, so a delegated turn
// arms the continuation filler at turn start like any other voice turn.
func TestDelegatedRunArmsAtTurnStart(t *testing.T) {
	fm := NewFillerManager()
	id := "delegated-turn"
	fm.MarkDelegatedVoiceRun(id, "")
	fm.OnTurnStart(id)
	t.Cleanup(func() { fm.Cancel(id) })
	run := fm.runs[id]
	if run == nil || run.timer == nil {
		t.Fatal("delegated turn must arm a filler at turn start")
	}
}

func TestDelegatedRunHardwareToolCancelsFiller(t *testing.T) {
	fm := NewFillerManager()
	id := "delegated-hw-turn"
	fm.MarkDelegatedVoiceRun(id, "")
	fm.OnTurnStart(id)
	t.Cleanup(func() { fm.Cancel(id) })
	run := fm.runs[id]
	fm.OnToolStart(id, "http://127.0.0.1:5001/emotion", "curl")
	if run.timer != nil {
		t.Fatal("a hardware reaction is the acknowledgement; the pending filler is cancelled")
	}
}

func TestPlainVoiceRunStillArmsAtTurnStart(t *testing.T) {
	fm := NewFillerManager()
	id := "plain-turn"
	fm.MarkVoiceRun(id, "")
	fm.OnTurnStart(id)
	t.Cleanup(func() { fm.Cancel(id) })
	if fm.runs[id].timer == nil {
		t.Fatal("non-delegated voice turns keep the FillerDelay arm")
	}
}

// Continuations play in pool order: "please wait" first, "still working" second.
func TestContinuationFillersEscalateInOrder(t *testing.T) {
	pool := i18n.FillerContinuation(i18n.Lang())
	if len(pool) < 2 {
		t.Fatalf("continuation pool needs two phrases, got %v", pool)
	}
	for i := 0; i < 5; i++ {
		if got := pickFiller(1, ""); got != pool[0] {
			t.Fatalf("first continuation = %q, want %q", got, pool[0])
		}
		if got := pickFiller(2, pool[0]); got != pool[1] {
			t.Fatalf("second continuation = %q, want %q", got, pool[1])
		}
	}
}
