package hermes

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"

	"go.autonomous.ai/os/system/domain"
)

const (
	destDiscordA = "[discord] Message from owner [id:1]:\nwhat is on my calendar?\n[via:discord]"
	destDiscordB = "[discord] Message from owner [id:1]:\nsummarise the news\n[via:discord]"
	destVoice    = "[user] turn on the light"
	destAmbient  = "[ambient] so then I told her we should leave early"
)

func TestManagedSameDestination(t *testing.T) {
	chat := func(id, text string) managedChat {
		return managedChat{runID: id, body: streamRequest{Input: text, Conversation: "c"}, source: "user"}
	}
	turn := func(requests ...managedChat) *managedTurn { return &managedTurn{requests: requests} }
	cases := []struct {
		name   string
		active *managedTurn
		in     managedChat
		want   bool
	}{
		{"voice into voice", turn(chat("a", destVoice)), chat("b", destAmbient), true},
		{"voice into merged voice", turn(chat("a", destVoice), chat("b", destAmbient)), chat("c", destVoice), true},
		{"ambient into discord", turn(chat("d", destDiscordA)), chat("v", destAmbient), false},
		{"voice into discord", turn(chat("d", destDiscordA)), chat("v", destVoice), false},
		{"discord into voice", turn(chat("v", destVoice)), chat("d", destDiscordA), false},
		{"discord into merged voice", turn(chat("v", destVoice), chat("w", destAmbient)), chat("d", destDiscordA), false},
		{"discord into discord", turn(chat("a", destDiscordA)), chat("b", destDiscordB), false},
		{"same discord text again", turn(chat("a", destDiscordA)), chat("b", destDiscordA), false},
		{"image request into voice", turn(chat("v", destVoice)), managedChat{runID: "i", body: streamRequest{Input: []inputMessage{{Role: "user"}}}, source: "user"}, true},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if got := managedSameDestination(c.active, c.in); got != c.want {
				t.Fatalf("managedSameDestination = %v, want %v", got, c.want)
			}
		})
	}
}

// destRuns is a fake native Hermes server that numbers its runs, records admissions and
// steers, and finishes each run only when the test releases it.
type destRuns struct {
	mu      sync.Mutex
	created []string
	steers  int
	release map[string]chan struct{}
	stopped map[string]bool
	admit   chan string
}

func newDestRuns() *destRuns {
	return &destRuns{release: map[string]chan struct{}{}, stopped: map[string]bool{}, admit: make(chan string, 8)}
}

func (d *destRuns) gate(id string) chan struct{} {
	d.mu.Lock()
	defer d.mu.Unlock()
	if d.release[id] == nil {
		d.release[id] = make(chan struct{})
	}
	return d.release[id]
}

func (d *destRuns) steerCount() int {
	d.mu.Lock()
	defer d.mu.Unlock()
	return d.steers
}

func (d *destRuns) handler(t *testing.T) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		path := r.URL.Path
		switch {
		case r.Method == http.MethodPost && path == "/v1/runs":
			raw, _ := io.ReadAll(r.Body)
			d.mu.Lock()
			d.created = append(d.created, string(raw))
			id := fmt.Sprintf("run-%d", len(d.created))
			d.mu.Unlock()
			d.admit <- id
			fmt.Fprintf(w, `{"run_id":%q}`, id)
		case strings.HasSuffix(path, "/steer"):
			d.mu.Lock()
			d.steers++
			d.mu.Unlock()
			fmt.Fprint(w, `{"accepted":true}`)
		case strings.HasSuffix(path, "/stop"):
			d.mu.Lock()
			d.stopped[strings.TrimSuffix(strings.TrimPrefix(path, "/v1/runs/"), "/stop")] = true
			d.mu.Unlock()
			fmt.Fprint(w, `{"accepted":true}`)
		case strings.HasSuffix(path, "/events"):
			id := strings.TrimSuffix(strings.TrimPrefix(path, "/v1/runs/"), "/events")
			w.Header().Set("Content-Type", "text/event-stream")
			w.WriteHeader(200)
			w.(http.Flusher).Flush()
			select {
			case <-d.gate(id):
			case <-r.Context().Done():
				return
			}
			fmt.Fprintf(w, "data: {\"event\":\"run.completed\",\"output\":\"answer-%s\"}\n\n", id)
		default:
			d.mu.Lock()
			stopped := d.stopped[strings.TrimPrefix(path, "/v1/runs/")]
			d.mu.Unlock()
			if stopped {
				fmt.Fprint(w, `{"status":"cancelled","session_id":"session"}`)
				return
			}
			fmt.Fprint(w, `{"status":"running","session_id":"session"}`)
		}
	})
}

type destEvents struct {
	mu     sync.Mutex
	finals map[string]string
	ends   map[string]string
	order  []string
}

func (e *destEvents) handle(_ context.Context, ev domain.WSEvent) error {
	var p map[string]any
	_ = json.Unmarshal(ev.Payload, &p)
	id, _ := p["runId"].(string)
	e.mu.Lock()
	defer e.mu.Unlock()
	if ev.Event == "chat" && p["state"] == "final" {
		e.finals[id], _ = p["message"].(string)
	}
	if d, _ := p["data"].(map[string]any); d != nil {
		if phase, _ := d["phase"].(string); phase == "end" || phase == "error" {
			e.ends[id] = phase
			e.order = append(e.order, id)
		}
	}
	return nil
}

func (e *destEvents) waitEnds(t *testing.T, ids ...string) {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		e.mu.Lock()
		n := 0
		for _, id := range ids {
			if e.ends[id] != "" {
				n++
			}
		}
		e.mu.Unlock()
		if n == len(ids) {
			return
		}
		time.Sleep(5 * time.Millisecond)
	}
	t.Fatalf("runs %v did not all end", ids)
}

func waitAdmit(t *testing.T, d *destRuns) string {
	t.Helper()
	select {
	case id := <-d.admit:
		return id
	case <-time.After(5 * time.Second):
		t.Fatal("no run was created")
		return ""
	}
}

// A request must not be merged into an active turn that answers to another place: it
// waits, runs as its own native run after the active one ends, and each run's final
// reply stays with its own request.
func TestManagedDestinationKeepsTurnsApart(t *testing.T) {
	cases := []struct {
		name         string
		first, later string
		firstSilent  bool
		laterSilent  bool
		laterWeb     bool
	}{
		{name: "ambient waits for discord turn", first: destDiscordA, later: destAmbient, firstSilent: true},
		{name: "voice waits for discord turn", first: destDiscordA, later: destVoice, firstSilent: true},
		{name: "web chat waits for discord turn", first: destDiscordA, later: destVoice, firstSilent: true, laterWeb: true},
		{name: "discord waits for voice turn", first: destVoice, later: destDiscordA, laterSilent: true},
		{name: "discord waits for ambient turn", first: destAmbient, later: destDiscordA, laterSilent: true},
		{name: "discord channel B waits for discord channel A", first: destDiscordA, later: destDiscordB, firstSilent: true, laterSilent: true},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			oldURL := BaseURL
			d := newDestRuns()
			server := httptest.NewServer(d.handler(t))
			BaseURL = server.URL
			ctx, cancel := context.WithCancel(context.Background())
			ev := &destEvents{finals: map[string]string{}, ends: map[string]string{}}
			s := &HermesService{httpClient: server.Client(), runtimeCtx: ctx, silentRuns: map[string]bool{}, webChatRuns: map[string]bool{}, handler: ev.handle}
			defer func() {
				cancel()
				for _, id := range []string{"run-1", "run-2"} {
					select {
					case <-d.gate(id):
					default:
						close(d.gate(id))
					}
				}
				server.Close()
				BaseURL = oldURL
			}()
			s.inFlightStreams.Store(2)
			if c.firstSilent {
				s.MarkSilentRun("first")
			}
			if c.laterSilent {
				s.MarkSilentRun("later")
			}
			if c.laterWeb {
				s.MarkWebChatRun("later")
			}

			s.enqueueManagedRun("first", streamRequest{Input: c.first, Conversation: "conv"}, "user")
			if id := waitAdmit(t, d); id != "run-1" {
				t.Fatalf("first run id %s", id)
			}
			s.enqueueManagedRun("later", streamRequest{Input: c.later, Conversation: "conv"}, "user")

			// The later request must not be steered and must not start while the first runs.
			select {
			case id := <-d.admit:
				t.Fatalf("second request started run %s while the first was still active", id)
			case <-time.After(300 * time.Millisecond):
			}
			if n := d.steerCount(); n != 0 {
				t.Fatalf("request was merged into the active turn (%d steers)", n)
			}

			close(d.gate("run-1"))
			ev.waitEnds(t, "first")
			if id := waitAdmit(t, d); id != "run-2" {
				t.Fatalf("waiting request run id %s", id)
			}
			close(d.gate("run-2"))
			ev.waitEnds(t, "later")

			ev.mu.Lock()
			defer ev.mu.Unlock()
			if ev.finals["first"] != "answer-run-1" || ev.finals["later"] != "answer-run-2" {
				t.Fatalf("replies crossed: %v", ev.finals)
			}
			if len(ev.order) != 2 || ev.order[0] != "first" || ev.order[1] != "later" {
				t.Fatalf("requests did not complete in order: %v", ev.order)
			}
			if n := d.steerCount(); n != 0 {
				t.Fatalf("steered %d times", n)
			}
		})
	}
}

// Speech arriving while a turn from the speaker runs still merges as before.
func TestManagedDestinationVoiceStillMergesIntoVoice(t *testing.T) {
	oldURL := BaseURL
	d := newDestRuns()
	server := httptest.NewServer(d.handler(t))
	BaseURL = server.URL
	ctx, cancel := context.WithCancel(context.Background())
	ev := &destEvents{finals: map[string]string{}, ends: map[string]string{}}
	s := &HermesService{httpClient: server.Client(), runtimeCtx: ctx, silentRuns: map[string]bool{}, webChatRuns: map[string]bool{}, handler: ev.handle}
	defer func() {
		cancel()
		select {
		case <-d.gate("run-1"):
		default:
			close(d.gate("run-1"))
		}
		server.Close()
		BaseURL = oldURL
	}()
	s.inFlightStreams.Store(2)
	s.enqueueManagedRun("first", streamRequest{Input: destVoice, Conversation: "conv"}, "user")
	waitAdmit(t, d)
	s.enqueueManagedRun("later", streamRequest{Input: destAmbient, Conversation: "conv"}, "user")
	deadline := time.Now().Add(3 * time.Second)
	for d.steerCount() == 0 && time.Now().Before(deadline) {
		time.Sleep(5 * time.Millisecond)
	}
	if d.steerCount() != 1 {
		t.Fatalf("speech was not merged into the speech turn (%d steers)", d.steerCount())
	}
	close(d.gate("run-1"))
	ev.waitEnds(t, "first", "later")
}

// A Discord turn that fails releases whatever waits behind it, and the failed run still ends
// (the handler finishes the Discord origin and its typing signal on that lifecycle error).
func TestManagedDestinationFailedDiscordTurnReleasesWaiter(t *testing.T) {
	oldURL := BaseURL
	d := newDestRuns()
	mux := http.NewServeMux()
	inner := d.handler(t)
	mux.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		if r.Method == http.MethodGet && r.URL.Path == "/v1/runs/run-1/events" {
			select {
			case <-d.gate("run-1"):
			case <-r.Context().Done():
				return
			}
			w.Header().Set("Content-Type", "text/event-stream")
			w.WriteHeader(200)
			fmt.Fprint(w, "data: {\"event\":\"run.failed\",\"error\":\"model unavailable\"}\n\n")
			return
		}
		inner.ServeHTTP(w, r)
	})
	server := httptest.NewServer(mux)
	BaseURL = server.URL
	ctx, cancel := context.WithCancel(context.Background())
	ev := &destEvents{finals: map[string]string{}, ends: map[string]string{}}
	s := &HermesService{httpClient: server.Client(), runtimeCtx: ctx, silentRuns: map[string]bool{}, webChatRuns: map[string]bool{}, handler: ev.handle}
	defer func() {
		cancel()
		select {
		case <-d.gate("run-2"):
		default:
			close(d.gate("run-2"))
		}
		server.Close()
		BaseURL = oldURL
	}()
	s.inFlightStreams.Store(2)
	s.MarkSilentRun("discord")
	s.enqueueManagedRun("discord", streamRequest{Input: destDiscordA, Conversation: "conv"}, "user")
	waitAdmit(t, d)
	s.enqueueManagedRun("voice", streamRequest{Input: destVoice, Conversation: "conv"}, "user")
	time.Sleep(100 * time.Millisecond)
	close(d.gate("run-1"))
	ev.waitEnds(t, "discord")
	ev.mu.Lock()
	phase := ev.ends["discord"]
	ev.mu.Unlock()
	if phase != "error" {
		t.Fatalf("failed discord turn ended with %q, want error", phase)
	}
	waitAdmit(t, d)
	close(d.gate("run-2"))
	ev.waitEnds(t, "voice")
	ev.mu.Lock()
	defer ev.mu.Unlock()
	if ev.finals["voice"] != "answer-run-2" {
		t.Fatalf("waiting request got %q", ev.finals["voice"])
	}
	if s.inFlightStreams.Load() != 0 {
		t.Fatalf("%d requests still counted as in flight", s.inFlightStreams.Load())
	}
}

// Cancelling the runtime while a Discord turn is active and a request waits ends both, so
// nothing is left typing or counted as busy.
func TestManagedDestinationCancelReleasesActiveAndWaiting(t *testing.T) {
	oldURL := BaseURL
	d := newDestRuns()
	server := httptest.NewServer(d.handler(t))
	BaseURL = server.URL
	ctx, cancel := context.WithCancel(context.Background())
	ev := &destEvents{finals: map[string]string{}, ends: map[string]string{}}
	s := &HermesService{httpClient: server.Client(), runtimeCtx: ctx, silentRuns: map[string]bool{}, webChatRuns: map[string]bool{}, handler: ev.handle}
	defer func() { server.Close(); BaseURL = oldURL }()
	s.inFlightStreams.Store(2)
	s.MarkSilentRun("discord")
	s.enqueueManagedRun("discord", streamRequest{Input: destDiscordA, Conversation: "conv"}, "user")
	waitAdmit(t, d)
	s.enqueueManagedRun("voice", streamRequest{Input: destAmbient, Conversation: "conv"}, "user")
	time.Sleep(100 * time.Millisecond)
	cancel()
	ev.waitEnds(t, "discord", "voice")
	deadline := time.Now().Add(3 * time.Second)
	for s.inFlightStreams.Load() != 0 && time.Now().Before(deadline) {
		time.Sleep(5 * time.Millisecond)
	}
	if s.inFlightStreams.Load() != 0 {
		t.Fatalf("%d requests still counted as in flight", s.inFlightStreams.Load())
	}
}
