#!/bin/zsh
# Exit 0 if the lamp has neither heard nor said anything for SECONDS (default 45); 1 otherwise. For anything that
# restarts the voice or cuts its link: never under someone who is talking to it. Since 17 Oct 12:3x it reads the voice's
# own session events (at_us is CLOCK_MONOTONIC): lamp-4ace's journal file had gone "truncated", journalctl returned no
# lines, and the old journal count read every moment as quiet. A voice started less than SECONDS ago also counts as busy.
# Exit 2 if the events could not be read (or LAMP_HOST is not set): callers treat that as not quiet. argv: [SECONDS]
N=$("${0:A:h}/lamp_ssh" "sudo -n python3 - ${1:-45}" <<'PY'
import glob, json, os, sys, time
window = float(sys.argv[1]) * 1e6
root = "/var/lib/lampOS/voice/sessions"
sessions = sorted(glob.glob(root + "/*/metadata"), key=os.path.getmtime)
if not sessions:
    print("unreadable"); sys.exit()
busy_kinds = {"provider_audio_enqueued", "speaker_first_write", "input_candidate", "provider_input_started",
              "transcript", "duplex_hearing", "local_endpoint", "answer_kept"}
now = time.monotonic() * 1e6
latest, started = 0, None
for path in glob.glob(sessions[-1] + "/events-*.jsonl"):
    with open(path, errors="replace") as f:
        for line in f:
            if '"kind":"run_start"' in line or '"kind":"service_segment"' in line or any('"kind":"%s"' % k in line for k in busy_kinds):
                try:
                    e = json.loads(line)
                except ValueError:
                    continue
                at = e.get("at_us", 0)
                if e.get("kind") in ("run_start", "service_segment"):
                    started = at if started is None else min(started, at)
                else:
                    latest = max(latest, at)
recent = latest > now - window or (started is not None and started > now - window)
print(1 if recent else 0)
PY
)
[ "$N" = "0" ] && exit 0
[ "$N" = "1" ] && exit 1
exit 2
