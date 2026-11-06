package hermes

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"syscall"

	"go.autonomous.ai/os/system/lib/discordscrub"
)

// Discord messages reach the device through the cloud relay, so hermes-gateway
// never runs a Discord adapter and its .env carries no DISCORD_* settings.

// ScrubDiscordEnv removes every DISCORD_* line from <home>/.env. A missing file
// is not an error. Reports whether the file was rewritten (the caller restarts
// the gateway when it was).
func ScrubDiscordEnv(home string) (bool, error) {
	path := filepath.Join(home, ".env")
	raw, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return false, nil
		}
		return false, fmt.Errorf("read hermes env: %w", err)
	}
	lines := strings.Split(string(raw), "\n")
	kept := lines[:0:0]
	changed := false
	for _, l := range lines {
		if strings.HasPrefix(strings.TrimSpace(l), "DISCORD_") {
			changed = true
			continue
		}
		kept = append(kept, l)
	}
	if !changed {
		return false, nil
	}
	info, statErr := os.Stat(path)
	mode := os.FileMode(0600)
	if statErr == nil {
		mode = info.Mode().Perm()
	}
	if err := os.WriteFile(path, []byte(strings.Join(kept, "\n")), mode); err != nil {
		return false, fmt.Errorf("write hermes env: %w", err)
	}
	return true, nil
}

// ScrubDiscordMigrationArchive removes Discord settings from the JSON files hermes
// archives when it imports from openclaw (<home>/migration/openclaw/<stamp>/archive/*.json).
// Files that are not JSON are left alone. Reports whether any file was rewritten.
func ScrubDiscordMigrationArchive(home string) (bool, error) {
	paths, err := filepath.Glob(filepath.Join(home, "migration", "openclaw", "*", "archive", "*.json"))
	if err != nil {
		return false, err
	}
	changedAny := false
	var errs []error
	for _, p := range paths {
		changed, err := discordscrub.ScrubFile(p)
		if err != nil {
			errs = append(errs, err)
			continue
		}
		changedAny = changedAny || changed
	}
	return changedAny, errors.Join(errs...)
}

// discordStateFiles are the files hermes-gateway's Discord adapter writes about the
// servers it talked to (registered slash commands, the capability cache). Nothing
// reads them once Discord runs through the relay.
var discordStateFiles = []string{
	filepath.Join("gateway", "discord_command_sync_state.json"),
	filepath.Join("cache", "discord_capabilities.json"),
}

// channelDirectoryFile is the channel list hermes-gateway keeps under its home: one list
// of channels per platform, under "platforms".
const channelDirectoryFile = "channel_directory.json"

// RemoveDiscordState deletes the Discord adapter's state files under home and drops the
// Discord entries from the channel directory. Missing files are not an error. Reports
// whether anything was changed.
func RemoveDiscordState(home string) (bool, error) {
	removed := false
	var errs []error
	for _, rel := range discordStateFiles {
		err := os.Remove(filepath.Join(home, rel))
		switch {
		case err == nil:
			removed = true
		case !os.IsNotExist(err):
			errs = append(errs, err)
		}
	}
	changed, err := ScrubDiscordChannelDirectory(home)
	if err != nil {
		errs = append(errs, err)
	}
	return removed || changed, errors.Join(errs...)
}

// ScrubDiscordChannelDirectory removes the stale Discord entries from
// <home>/channel_directory.json and keeps every other platform as it is. A missing file,
// a file that is not a JSON object, and a file without Discord entries are left alone and
// are not errors, so the call is safe on every boot. The file is replaced atomically with
// its mode and owner kept. Reports whether the file was rewritten.
func ScrubDiscordChannelDirectory(home string) (bool, error) {
	path := filepath.Join(home, channelDirectoryFile)
	raw, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return false, nil
		}
		return false, fmt.Errorf("read hermes channel directory: %w", err)
	}
	var doc map[string]json.RawMessage
	if err := json.Unmarshal(raw, &doc); err != nil || doc == nil {
		return false, nil
	}
	changed := false
	if _, ok := doc["discord"]; ok {
		delete(doc, "discord")
		changed = true
	}
	if rawPlatforms, ok := doc["platforms"]; ok {
		var platforms map[string]json.RawMessage
		if json.Unmarshal(rawPlatforms, &platforms) == nil && platforms != nil {
			if _, ok := platforms["discord"]; ok {
				delete(platforms, "discord")
				out, err := marshalNoEscape(platforms)
				if err != nil {
					return false, fmt.Errorf("encode hermes channel directory: %w", err)
				}
				doc["platforms"] = out
				changed = true
			}
		}
	}
	if !changed {
		return false, nil
	}
	out, err := marshalNoEscape(doc)
	if err != nil {
		return false, fmt.Errorf("encode hermes channel directory: %w", err)
	}
	if err := replaceFileKeepingOwner(path, append(out, '\n')); err != nil {
		return false, fmt.Errorf("write hermes channel directory: %w", err)
	}
	return true, nil
}

// marshalNoEscape encodes v indented with two spaces, leaving non-ASCII text and
// <, >, & as they are.
func marshalNoEscape(v any) ([]byte, error) {
	var buf bytes.Buffer
	enc := json.NewEncoder(&buf)
	enc.SetEscapeHTML(false)
	enc.SetIndent("", "  ")
	if err := enc.Encode(v); err != nil {
		return nil, err
	}
	return bytes.TrimRight(buf.Bytes(), "\n"), nil
}

// replaceFileKeepingOwner writes data to a temporary file beside path and renames it over
// path, so a reader never sees a partial file. The file keeps the mode of the one it
// replaces; its owner is kept too when the process may set it (hermes runs as another
// user than os-server on some devices and must still be able to rewrite the file).
func replaceFileKeepingOwner(path string, data []byte) error {
	info, err := os.Stat(path)
	if err != nil {
		return err
	}
	tmp, err := os.CreateTemp(filepath.Dir(path), "."+filepath.Base(path)+".tmp-*")
	if err != nil {
		return err
	}
	tmpPath := tmp.Name()
	cleanup := func() { _ = os.Remove(tmpPath) }
	if _, err := tmp.Write(data); err != nil {
		_ = tmp.Close()
		cleanup()
		return err
	}
	if err := tmp.Chmod(info.Mode().Perm()); err != nil {
		_ = tmp.Close()
		cleanup()
		return err
	}
	if st, ok := info.Sys().(*syscall.Stat_t); ok {
		// Best effort: only a privileged process may hand the file to another user.
		_ = tmp.Chown(int(st.Uid), int(st.Gid))
	}
	if err := tmp.Sync(); err != nil {
		_ = tmp.Close()
		cleanup()
		return err
	}
	if err := tmp.Close(); err != nil {
		cleanup()
		return err
	}
	if err := os.Rename(tmpPath, path); err != nil {
		cleanup()
		return err
	}
	return nil
}
