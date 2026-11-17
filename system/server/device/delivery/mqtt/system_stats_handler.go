package mqtthandler

import (
	"context"
	"log/slog"
	"os"
	"os/exec"
	"regexp"
	"runtime"
	"strconv"
	"strings"
	"syscall"
	"time"

	"go.autonomous.ai/os/system/domain"
	"go.autonomous.ai/os/system/lib/hal"
)

// System stats probes. Every probe is bounded so a hung journal or a dead HAL
// cannot stall the reply.
const (
	statsProbeTimeout = 3 * time.Second
	statsJournalLines = 300 // journal lines scanned for errors
	statsMaxErrLines  = 15  // error lines returned
	statsMaxLineLen   = 300 // bytes kept per returned line
	statsMaxOOMLines  = 3
)

// HAL logs what users say, and a log value can span lines, so a line is never matched
// by keyword or by a loose prefix. Only fixed forms are returned, each anchored
// end to end so it cannot carry free text: systemd's own exit/restart messages for the
// unit, traceback locations (file, line, function), a missing module's name, and
// exception type names with the message dropped.
var (
	reUnitLine = regexp.MustCompile(`^hal\.service: (Main process exited, code=[a-z]+, status=[0-9]+/[A-Za-z0-9]+` +
		`|Failed with result '[a-z-]+'\.?` +
		`|Scheduled restart job, restart counter is at [0-9]+\.?)$`)
	reTraceFrame = regexp.MustCompile(`^ +File "[^"]+", line [0-9]+, in [A-Za-z0-9_<>]+$`)
	reNoModule   = regexp.MustCompile(`^(ModuleNotFoundError|ImportError): No module named '[A-Za-z0-9_.]+'$`)
	reExcType    = regexp.MustCompile(`^([A-Za-z_][A-Za-z0-9_.]*(Error|Exception))(:|$)`)
)

// handleSystemStats returns host load and the state of the HAL service, so a
// device whose HAL is down can be diagnosed without a shell on it.
func (h *DeviceMQTTHandler) handleSystemStats(env domain.MQTTDataCommand) error {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	data := probeStats(ctx)
	slog.Info("system.stats", "component", "mqtt",
		"load1", data.CPU.Load1, "mem_available_mb", data.Mem.AvailableMB,
		"disk_percent", data.Disk.UsedPercent, "hal_state", data.HAL.ActiveState,
		"hal_health", data.HAL.Health)
	return h.publishDataResult(env.Kind, "success", "", data)
}

// probeStats gathers the whole snapshot; each part degrades to zero values on error.
func probeStats(ctx context.Context) domain.MQTTStatsData {
	out := domain.MQTTStatsData{
		CPU:   readCPUStats(),
		Mem:   readMemStats(),
		Disk:  readDiskStats("/"),
		TempC: readTempC(),
		HAL:   probeHALStats(ctx),
	}
	out.KernelOOM = readKernelOOM(ctx)
	return out
}

func readCPUStats() domain.MQTTCPUStats {
	out := domain.MQTTCPUStats{Cores: runtime.NumCPU()}
	if data, err := os.ReadFile("/proc/loadavg"); err == nil {
		out.Load1, out.Load5, out.Load15 = parseLoadavg(string(data))
	}
	return out
}

// parseLoadavg reads the three load averages from /proc/loadavg content.
func parseLoadavg(content string) (l1, l5, l15 float64) {
	f := strings.Fields(content)
	if len(f) < 3 {
		return 0, 0, 0
	}
	l1, _ = strconv.ParseFloat(f[0], 64)
	l5, _ = strconv.ParseFloat(f[1], 64)
	l15, _ = strconv.ParseFloat(f[2], 64)
	return l1, l5, l15
}

func readMemStats() domain.MQTTMemStats {
	data, err := os.ReadFile("/proc/meminfo")
	if err != nil {
		return domain.MQTTMemStats{}
	}
	return parseMeminfo(string(data))
}

// parseMeminfo turns /proc/meminfo content (kB) into MB figures.
func parseMeminfo(content string) domain.MQTTMemStats {
	var total, avail, swapTotal, swapFree int64
	for _, line := range strings.Split(content, "\n") {
		f := strings.Fields(line)
		if len(f) < 2 {
			continue
		}
		v, err := strconv.ParseInt(f[1], 10, 64)
		if err != nil {
			continue
		}
		switch f[0] {
		case "MemTotal:":
			total = v
		case "MemAvailable:":
			avail = v
		case "SwapTotal:":
			swapTotal = v
		case "SwapFree:":
			swapFree = v
		}
	}
	out := domain.MQTTMemStats{
		TotalMB:     total / 1024,
		AvailableMB: avail / 1024,
		SwapTotalMB: swapTotal / 1024,
		SwapUsedMB:  (swapTotal - swapFree) / 1024,
	}
	if total > 0 {
		out.UsedPercent = float64(total-avail) / float64(total) * 100
	}
	return out
}

func readDiskStats(path string) domain.MQTTDiskStats {
	var st syscall.Statfs_t
	if err := syscall.Statfs(path, &st); err != nil {
		return domain.MQTTDiskStats{}
	}
	total := st.Blocks * uint64(st.Bsize)
	used := total - st.Bavail*uint64(st.Bsize)
	out := domain.MQTTDiskStats{
		TotalMB: int64(total / (1024 * 1024)),
		UsedMB:  int64(used / (1024 * 1024)),
	}
	if total > 0 {
		out.UsedPercent = float64(used) / float64(total) * 100
	}
	return out
}

func readTempC() float64 {
	data, err := os.ReadFile("/sys/class/thermal/thermal_zone0/temp")
	if err != nil {
		return 0
	}
	milli, _ := strconv.Atoi(strings.TrimSpace(string(data)))
	return float64(milli) / 1000.0
}

// probeHALStats reads the hal unit's state, probes /health, and collects recent
// error lines from its journal.
func probeHALStats(ctx context.Context) domain.MQTTHALStats {
	out := domain.MQTTHALStats{Health: "ok"}

	showCtx, cancel := context.WithTimeout(ctx, statsProbeTimeout)
	defer cancel()
	if b, err := exec.CommandContext(showCtx, "systemctl", "show", "hal",
		"--property=ActiveState,SubState,Result,ExecMainStatus,NRestarts").Output(); err == nil {
		props := parseSystemctlShow(string(b))
		out.ActiveState = props["ActiveState"]
		out.SubState = props["SubState"]
		out.Result = props["Result"]
		out.ExecMainStatus = props["ExecMainStatus"]
		out.Restarts, _ = strconv.Atoi(props["NRestarts"])
	}

	healthCtx, cancelHealth := context.WithTimeout(ctx, statsProbeTimeout)
	defer cancelHealth()
	if _, err := hal.GetHealthContext(healthCtx); err != nil {
		out.Health = err.Error()
	}

	jCtx, cancelJ := context.WithTimeout(ctx, statsProbeTimeout)
	defer cancelJ()
	if b, err := exec.CommandContext(jCtx, "journalctl", "-u", "hal", "--no-pager", "-o", "cat",
		"-n", strconv.Itoa(statsJournalLines)).Output(); err == nil {
		out.RecentErrors = errorLines(string(b), statsMaxErrLines, statsMaxLineLen)
	}
	return out
}

// parseSystemctlShow reads "Key=Value" lines.
func parseSystemctlShow(content string) map[string]string {
	props := map[string]string{}
	for _, line := range strings.Split(content, "\n") {
		if k, v, ok := strings.Cut(strings.TrimSpace(line), "="); ok {
			props[k] = v
		}
	}
	return props
}

// errorLines keeps the newest limit structural error lines (see the patterns above),
// oldest first, each cut to maxLen bytes. Exception messages are dropped.
func errorLines(journal string, limit, maxLen int) []string {
	lines := strings.Split(journal, "\n")
	var picked []string
	for i := len(lines) - 1; i >= 0 && len(picked) < limit; i-- {
		line, ok := structuralLine(strings.TrimRight(lines[i], "\r"))
		if !ok {
			continue
		}
		if len(line) > maxLen {
			line = line[:maxLen]
		}
		picked = append(picked, line)
	}
	for i, j := 0, len(picked)-1; i < j; i, j = i+1, j-1 {
		picked[i], picked[j] = picked[j], picked[i]
	}
	return picked
}

// structuralLine returns the part of a journal line that is safe to report, or false.
func structuralLine(line string) (string, bool) {
	switch {
	case reUnitLine.MatchString(line), reTraceFrame.MatchString(line), reNoModule.MatchString(line):
		return line, true
	}
	if m := reExcType.FindStringSubmatch(line); m != nil {
		return m[1] + ":", true
	}
	return "", false
}

// readKernelOOM returns the last kernel lines about the out-of-memory killer.
func readKernelOOM(ctx context.Context) []string {
	ctx, cancel := context.WithTimeout(ctx, statsProbeTimeout)
	defer cancel()
	b, err := exec.CommandContext(ctx, "journalctl", "-k", "--no-pager", "-o", "short-iso",
		"-n", "2000").Output()
	if err != nil {
		return nil
	}
	var hits []string
	for _, line := range strings.Split(string(b), "\n") {
		l := strings.ToLower(line)
		if strings.Contains(l, "out of memory") || strings.Contains(l, "oom-kill") || strings.Contains(l, "oom_reaper") {
			hits = append(hits, strings.TrimSpace(line))
		}
	}
	if len(hits) > statsMaxOOMLines {
		hits = hits[len(hits)-statsMaxOOMLines:]
	}
	return hits
}
