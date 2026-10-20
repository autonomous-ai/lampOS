#!/usr/bin/env bash
# Tool calls' times before and after an install (the first builder on #1722,
# 20 Oct 12:11, for #2169, #1903 and #1924): from the lamp's journal, for each
# kind of call the voice made in each window, how long the voice waited for its
# answer, and for a call `lamp-actions` answered, how long `lamp-actions` took
# and the rest. A table of journal times only: call names and milliseconds,
# never a word heard or said. Run it on the lamp:
#
#   lampOS/tools/call-times.sh BEFORE_FROM BEFORE_TO AFTER_FROM AFTER_TO
#
# Each time as `journalctl --since` takes it ("2026-10-20 13:40:00", "13:40").
# JOURNALCTL names another journalctl: a test's stand-in, or from a Mac
# `JOURNALCTL="ssh lamp-4ace journalctl"`, with times that have no spaces:
# "13:04:21" for today, or "@1760940261" (seconds since 1970) for any day.
# The lamp's journalctl doesn't take the ISO "2026-10-20T13:04:21" (the first
# builder on #1722, 20 Oct 13:06).
#
# The lines it reads:
#   lamp-voice second: request N asked for NAME; WHOM answered in M ms; ...
#   lamp-voice provider: request N asked for NAME; WHOM answered in M ms; ...
#     (voice/src/tools.rs and provider_worker.rs; "did not answer" counts apart)
#   lamp-actions: NAME: done in M ms   (actions/src/server.rs)
# A skill's call is the voice's `use_skill`, or the skill's own tool, answered
# by "the executive" as `lamp-actions`' are; `lamp-skills` keeps no time of
# its own, so only the voice's wait is shown for it.
#
# The rows, each "median (fastest–slowest), n calls" in ms:
#   waited         the voice, from asking to the answer;
#   lamp-actions   `lamp-actions`' own "done in" for that call, matched to the
#                  voice's line by its name within a second;
#   the rest       the voice's wait less `lamp-actions`' time: reaching the part,
#                  waiting to be taken (#2169's accept) and the answer back.
set -euo pipefail

JOURNALCTL=${JOURNALCTL:-journalctl}
[[ $# -eq 4 ]] || {
  echo "usage: call-times.sh BEFORE_FROM BEFORE_TO AFTER_FROM AFTER_TO" >&2
  exit 2
}
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT

# The calls of one window, a line each: NAME<TAB>MEASURE<TAB>MS.
calls() { # calls FROM TO
  # shellcheck disable=SC2086 # JOURNALCTL may be a command with its words.
  $JOURNALCTL --no-pager --quiet --output=short-unix \
    --unit lamp-voice --unit lamp-actions --since "$1" --until "$2" |
    awk '
    function after(text, mark) { return substr(text, index(text, mark) + length(mark)) }
    # The voice: the call, who answered and how long it waited.
    /lamp-voice (second|provider): request [0-9]+ asked for [A-Za-z0-9_]+; / {
      rest = after($0, " asked for ")
      name = substr(rest, 1, index(rest, ";") - 1)
      rest = after(rest, "; ")
      answered = index(rest, " answered in ") > 0
      mark = answered ? " answered in " : " did not answer in "
      if (!index(rest, mark)) next
      whom = substr(rest, 1, index(rest, mark) - 1)
      ms = after(rest, mark) + 0
      n++; vt[n] = $1 + 0; vname[n] = name; vms[n] = ms; vwhom[n] = whom; vok[n] = answered
      next
    }
    # lamp-actions: the call and how long it took itself.
    /lamp-actions: [A-Za-z0-9_]+: [a-z_ ]+ in [0-9]+ ms/ {
      rest = after($0, "lamp-actions: ")
      name = substr(rest, 1, index(rest, ":") - 1)
      ms = after(rest, " in ") + 0
      m++; at[m] = $1 + 0; aname[m] = name; ams[m] = ms
      next
    }
    END {
      for (i = 1; i <= n; i++) {
        if (!vok[i]) { print vname[i] "\tnot answered\t" vms[i]; continue }
        print vname[i] "\twaited\t" vms[i]
        if (vwhom[i] != "the executive") continue
        best = 0
        for (j = 1; j <= m; j++) {
          if (used[j] || aname[j] != vname[i]) continue
          gap = at[j] - vt[i]; if (gap < 0) gap = -gap
          if (gap <= 1.0 && (!best || gap < bestgap)) { best = j; bestgap = gap }
        }
        if (!best) continue
        used[best] = 1
        print vname[i] "\tlamp-actions\t" ams[best]
        rest = vms[i] - ams[best]; if (rest < 0) rest = 0
        print vname[i] "\tthe rest\t" rest
      }
    }'
}

# "median (fastest–slowest), n calls" of the numbers on stdin, or "none".
summary() {
  sort -n | awk '{ v[++n] = $1 }
    END {
      if (!n) { print "none"; exit }
      median = (n % 2) ? v[(n + 1) / 2] : (v[n / 2] + v[n / 2 + 1]) / 2
      printf "%g (%g–%g), %d call%s\n", median, v[1], v[n], n, (n == 1 ? "" : "s")
    }'
}

calls "$1" "$2" >"$T/before"
calls "$3" "$4" >"$T/after"
echo "| call | | before ($1 to $2) | after ($3 to $4) |"
echo "|---|---|---|---|"
# Each call's rows in the order above, the calls by name.
cut -f1,2 "$T/before" "$T/after" | sort -u |
  awk -F'\t' '{ rank = ($2 == "waited") ? 1 : ($2 == "lamp-actions") ? 2 : ($2 == "the rest") ? 3 : 4
    print $1 "\t" rank "\t" $2 }' | sort -t$'\t' -k1,1 -k2,2n | cut -f1,3 |
  while IFS=$'\t' read -r name measure; do
    cell() { awk -F'\t' -v n="$name" -v m="$measure" '$1 == n && $2 == m { print $3 }' "$1" | summary; }
    echo "| \`$name\` | $measure | $(cell "$T/before") | $(cell "$T/after") |"
  done
