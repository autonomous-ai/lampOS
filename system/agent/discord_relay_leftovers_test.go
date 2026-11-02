package agent

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"go.autonomous.ai/os/system/domain"
	"go.autonomous.ai/os/system/server/config"
)

const leftoverSecret = "fake-discord-secret-for-tests-0008"

// Earlier firmware (and the runtimes themselves) left copies of runtime config next
// to the live files: backups of openclaw.json and the archive hermes keeps when it
// imports from openclaw. Those must not keep Discord settings either.

type leftovers struct {
	openclawDir, hermesDir, picoDir string
	backups                         []string
	archives                        []string
	dbFiles                         map[string]string // path -> content that must stay untouched
}

func writeFile(t *testing.T, path, body string) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(body), 0600); err != nil {
		t.Fatal(err)
	}
}

func layoutLeftovers(t *testing.T) leftovers {
	t.Helper()
	root := t.TempDir()
	l := leftovers{
		openclawDir: filepath.Join(root, "oc"),
		hermesDir:   filepath.Join(root, "hermes"),
		picoDir:     filepath.Join(root, "pico"),
		dbFiles:     map[string]string{},
	}
	cfg := `{"meta":{"keep":"me"},"channels":{"telegram":{"enabled":true},"discord":{"enabled":true,"token":"` + leftoverSecret + `"}}}`
	writeFile(t, filepath.Join(l.openclawDir, "openclaw.json"), cfg)
	for _, n := range []string{"openclaw.json.bak", "openclaw.json.bak.1", "openclaw.json.last-good"} {
		p := filepath.Join(l.openclawDir, n)
		writeFile(t, p, cfg)
		l.backups = append(l.backups, p)
	}
	for _, a := range []struct{ ts, body string }{
		{"20261001T000000Z", cfg},
		{"20261002T000000Z", `{"discord":{"botToken":"` + leftoverSecret + `","enabled":true},"gateway":{"port":1}}`},
	} {
		p := filepath.Join(l.hermesDir, "migration", "openclaw", a.ts, "archive", "gateway-config.json")
		writeFile(t, p, a.body)
		l.archives = append(l.archives, p)
	}
	// Live databases and curator blobs are never rewritten.
	for _, rel := range []string{"state.db", filepath.Join(".curator_backups", "blobs", "abc123")} {
		p := filepath.Join(l.hermesDir, rel)
		writeFile(t, p, "binary-ish "+leftoverSecret)
		l.dbFiles[p] = "binary-ish " + leftoverSecret
	}
	p := filepath.Join(l.openclawDir, "state", "openclaw.sqlite")
	writeFile(t, p, "binary-ish "+leftoverSecret)
	l.dbFiles[p] = "binary-ish " + leftoverSecret

	return l
}

func runReconcile(l leftovers, gw domain.AgentGateway) {
	(&DiscordRelayReconcile{
		cfg: &config.Config{AgentRuntime: domain.AgentRuntimeHermes},
		gw:  gw, openclawDir: l.openclawDir, hermesHome: l.hermesDir, picoHome: l.picoDir,
	}).Reconcile()
}

func TestDiscordRelayReconcileScrubsConfigBackups(t *testing.T) {
	l := layoutLeftovers(t)
	runReconcile(l, &restartGateway{})

	for _, p := range l.backups {
		b, err := os.ReadFile(p)
		if err != nil {
			t.Fatal(err)
		}
		if strings.Contains(string(b), leftoverSecret) {
			t.Errorf("%s still holds the credential", filepath.Base(p))
		}
		if !strings.Contains(string(b), `"keep"`) || !strings.Contains(string(b), "telegram") {
			t.Errorf("%s lost unrelated settings: %s", filepath.Base(p), b)
		}
	}
}

func TestDiscordRelayReconcileScrubsMigrationArchive(t *testing.T) {
	l := layoutLeftovers(t)
	runReconcile(l, &restartGateway{})

	for _, p := range l.archives {
		b, err := os.ReadFile(p)
		if err != nil {
			t.Fatal(err)
		}
		if strings.Contains(string(b), leftoverSecret) {
			t.Errorf("%s still holds the credential: %s", p, b)
		}
	}
	b, _ := os.ReadFile(l.archives[1])
	if !strings.Contains(string(b), `"gateway"`) {
		t.Errorf("archive lost unrelated settings: %s", b)
	}
}

func TestDiscordRelayReconcileLeavesDatabasesAndBlobsAlone(t *testing.T) {
	l := layoutLeftovers(t)
	runReconcile(l, &restartGateway{})

	for p, want := range l.dbFiles {
		b, err := os.ReadFile(p)
		if err != nil {
			t.Fatal(err)
		}
		if string(b) != want {
			t.Errorf("%s was modified; live databases and backups are never rewritten", p)
		}
	}
}

func TestDiscordRelayReconcileLeftoversSecondPassIsQuiet(t *testing.T) {
	l := layoutLeftovers(t)
	gw := &restartGateway{}
	runReconcile(l, gw)
	first := gw.restarts
	runReconcile(l, gw)
	if gw.restarts != first {
		t.Errorf("second pass restarted the agent again (%d -> %d)", first, gw.restarts)
	}
}
