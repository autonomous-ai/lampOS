package discordrelay

import (
	"encoding/json"
	"errors"
	"io/fs"
	"log/slog"
	"os"
	"path/filepath"
)

// joinedFile keeps the joined threads next to config.json, so answering in a
// thread without an @mention survives a restart or an update.
const joinedFile = "discord_threads.json"

// loadJoined reads the saved thread list and remembers where to save it. A missing
// or unreadable file starts an empty list.
func (r *Relay) loadJoined(path string) {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.joinedPath = path
	data, err := os.ReadFile(path)
	if err != nil {
		if !errors.Is(err, fs.ErrNotExist) {
			slog.Warn("discord relay: joined threads not read", "component", "discord-relay", "error", err)
		}
		return
	}
	var ids []string
	if err := json.Unmarshal(data, &ids); err != nil {
		slog.Warn("discord relay: joined threads file ignored", "component", "discord-relay", "error", err)
		return
	}
	if len(ids) > maxJoinedThreads {
		ids = ids[len(ids)-maxJoinedThreads:]
	}
	r.joined = ids
}

// saveJoinedLocked writes the thread list atomically. r.mu must be held.
func (r *Relay) saveJoinedLocked() {
	if r.joinedPath == "" {
		return
	}
	data, err := json.Marshal(r.joined)
	if err == nil {
		tmp := r.joinedPath + ".tmp"
		if err = os.WriteFile(tmp, data, 0o600); err == nil {
			err = os.Rename(tmp, r.joinedPath)
		}
	}
	if err != nil {
		slog.Warn("discord relay: joined threads not saved", "component", "discord-relay", "path", filepath.Base(r.joinedPath), "error", err)
	}
}
