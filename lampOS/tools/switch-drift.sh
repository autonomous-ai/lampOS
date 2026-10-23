#!/usr/bin/env bash
# Which of a lamp's voice switches differ from the product's, or from another
# lamp's. Read-only on every lamp: it reads the running lamp-voice's
# environment and the updater's record of the build, and changes nothing.
#
#   switch-drift.sh LAMP [--against product|LAMP2]
#   switch-drift.sh --env-file FILE [--against-file FILE]     # offline
#
# The running side is `/proc/<pid>/environ` of lamp-voice's MainPID (through
# `sudo -n`), its LAMP_* names only. The product side is the `Environment=`
# and `UnsetEnvironment=` lines of deploy/product/lamp-voice.service at the
# lamp's own build: the `installed.commit` of the updater's
# /var/lib/lampos-update/state.json, read from this Git repository; when
# that build is unknown or not here, the working tree's unit, with a note.
# --against LAMP2 compares with a second lamp's running voice instead.
# --env-file stands for the running side (NAME=VALUE lines) and
# --against-file for the reference (a unit file, or NAME=VALUE lines).
#
# It prints the lists missing (in the reference, not running), extra
# (running, not in the reference) and different (running vs reference),
# sorted, then their counts. A value is printed only when it has a switch's
# shape: on, off, a number, or a short lower-case word (`[a-z][a-z0-9_-]`,
# at most 24 characters). Any other value, and any value whose name ends
# _URL, _KEY, _TOKEN, _SECRET, _FILE, _PATH, _DIR or _SOCKET or holds
# CREDENTIAL, PASS, PSK or KEY_TERMS, shows as `<set>`. Such values are
# compared by a checksum, so "different" still sees them. The rule is the
# same for the running side and the reference (unit, file or second lamp).
#
# Exit codes: 0 compared (whatever the drift), 1 the lamp could not be read
# (ssh failed, no lamp-voice running, sudo refused), 2 usage.
#
# Reaching the lamp (as lamp-lock.sh):
#   LAMP_SSH   the ssh command (default: ssh -n -o BatchMode=yes, a 15 s
#              connect timeout)
#   LAMP_USER  the lamp's user (default: orangepi)
set -uo pipefail

usage() { sed -n '6,7p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 2; }
HERE=$(cd "$(dirname "$0")/.." && pwd)
SSH=${LAMP_SSH:-ssh -n -o BatchMode=yes -o ConnectTimeout=15}
USER_AT=${LAMP_USER:-orangepi}
LAMP='' AGAINST=product ENV_FILE='' AGAINST_FILE=''
while [[ $# -gt 0 ]]; do
  case $1 in
    --against) AGAINST=${2:?}; shift 2 ;;
    --env-file) ENV_FILE=${2:?}; shift 2 ;;
    --against-file) AGAINST_FILE=${2:?}; shift 2 ;;
    -h|--help) usage ;;
    -*) usage ;;
    *) [[ -z $LAMP ]] || usage; LAMP=$1; shift ;;
  esac
done
[[ -n $LAMP || -n $ENV_FILE ]] || usage
for l in "$LAMP" "${AGAINST#product}"; do
  [[ -z $l || $l =~ ^lamp-[0-9a-z]+$ ]] || { echo "not a lamp name: $l" >&2; exit 2; }
done

die() { echo "switch-drift: $*" >&2; exit 1; }

# The running voice's LAMP_* lines on a lamp.
lamp_env() {
  # shellcheck disable=SC2016  # expanded on the lamp
  $SSH "$USER_AT@$1.local" 'p=$(systemctl show -p MainPID --value lamp-voice); [ "${p:-0}" -gt 0 ] || { echo "no lamp-voice running" >&2; exit 1; }; sudo -n cat "/proc/$p/environ" >/dev/null || exit 1; sudo -n cat "/proc/$p/environ" | tr "\0" "\n" | grep "^LAMP_"; exit 0' \
    || die "could not read lamp-voice's environment on $1"
}

# The build a lamp runs: the updater's installed commit, or nothing.
lamp_build() {
  $SSH "$USER_AT@$1.local" 'sudo -n cat /var/lib/lampos-update/state.json' 2>/dev/null \
    | tr -d ' \n' | grep -o '"installed":{[^}]*}' | grep -o '"commit":"[0-9a-f]*"' | cut -d'"' -f4
}

# NAME=VALUE lines from a unit's Environment= and UnsetEnvironment= lines:
# quoted words, several on a line, a bare `Environment=` clearing the list.
unit_env() {
  awk '
  function words(s, out,   n, i, c, q, w, started) {
    n = 0; q = ""; w = ""; started = 0
    for (i = 1; i <= length(s); i++) {
      c = substr(s, i, 1)
      if (q != "") {
        if (c == "\\" && i < length(s)) { i++; w = w substr(s, i, 1) }
        else if (c == q) q = ""
        else w = w c
      } else if (c == "\"" || c == "\047") { q = c; started = 1 }
      else if (c == " " || c == "\t") { if (started) { out[++n] = w; w = ""; started = 0 } }
      else { w = w c; started = 1 }
    }
    if (started) out[++n] = w
    return n
  }
  /^[ \t]*Environment=/ {
    s = substr($0, index($0, "=") + 1)
    if (s ~ /^[ \t]*$/) { for (k in env) delete env[k]; next }
    n = words(s, w)
    for (i = 1; i <= n; i++) if ((j = index(w[i], "=")) > 1) env[substr(w[i], 1, j - 1)] = substr(w[i], j + 1)
    next
  }
  /^[ \t]*UnsetEnvironment=/ {
    n = words(substr($0, index($0, "=") + 1), w)
    for (i = 1; i <= n; i++) unset[++u] = w[i]
  }
  END {
    # systemd applies UnsetEnvironment= after every assignment.
    for (i = 1; i <= u; i++) {
      if ((j = index(unset[i], "=")) > 1) { k = substr(unset[i], 1, j - 1); if (env[k] == substr(unset[i], j + 1)) delete env[k] }
      else delete env[unset[i]]
    }
    for (k in env) print k "=" env[k]
  }'
}

# A reference file is a unit when it has a section or an Environment= line.
file_env() {
  if grep -qE '^[[:space:]]*(\[[A-Za-z]+\]|Environment=)' "$1"; then unit_env <"$1"; else cat "$1"; fi
}

# NAME<tab>SHOWN<tab>KEY for each LAMP_ line, the last of a name kept: SHOWN
# is what may be printed, KEY what is compared.
# A value is shown only when its name is not a secret's and its shape is a
# switch's; free text (names, passwords) never is. C locale: [a-z] is ASCII.
secret() {
  case $1 in *_URL|*_KEY|*_TOKEN|*_SECRET|*_FILE|*_PATH|*_DIR|*_SOCKET|*CREDENTIAL*|*PASS*|*PSK*|*KEY_TERMS*) return 0 ;; esac
  local LC_ALL=C
  ! [[ $2 =~ ^(on|off|[0-9]+(\.[0-9]+)?|[a-z][a-z0-9_-]{0,23})$ ]]
}
norm() {
  local line name val
  while IFS= read -r line; do
    [[ $line == LAMP_*=* ]] || continue
    name=${line%%=*} val=${line#*=}
    if secret "$name" "$val"; then
      printf '%s\t<set>\t#%s\n' "$name" "$(printf '%s' "$val" | cksum | cut -d' ' -f1)"
    else
      printf '%s\t%s\t=%s\n' "$name" "$val" "$val"
    fi
  done | awk -F'\t' '{ v[$1] = $0 } END { for (k in v) print v[k] }' | sort
}

# The running side.
if [[ -n $ENV_FILE ]]; then
  [[ -r $ENV_FILE ]] || die "cannot read $ENV_FILE"
  RUN_LABEL=${LAMP:-$ENV_FILE} BUILD='' RUNNING=$(norm <"$ENV_FILE")
else
  RUN_LABEL=$LAMP
  ENV=$(lamp_env "$LAMP") || exit 1
  RUNNING=$(norm <<<"$ENV"); unset ENV
  BUILD=$(lamp_build "$LAMP")
fi

# The reference.
NOTE=''
if [[ -n $AGAINST_FILE ]]; then
  [[ -r $AGAINST_FILE ]] || die "cannot read $AGAINST_FILE"
  REF_LABEL=$AGAINST_FILE REF=$(file_env "$AGAINST_FILE" | norm)
elif [[ $AGAINST == product ]]; then
  UNIT=''
  [[ -n $BUILD ]] && UNIT=$(git -C "$HERE" show "$BUILD:./deploy/product/lamp-voice.service" 2>/dev/null)
  if [[ -n $UNIT ]]; then
    REF_LABEL="the product unit at ${BUILD:0:9}"
  else
    UNIT=$(cat "$HERE/deploy/product/lamp-voice.service") || die "no product unit"
    REF_LABEL="the product unit in the working tree"
    if [[ -n $BUILD ]]; then NOTE="note: build ${BUILD:0:9} is not in this repository (git fetch?); the working tree's unit is used"
    else NOTE="note: the lamp's build is unknown; the working tree's unit is used"; fi
  fi
  REF=$(unit_env <<<"$UNIT" | norm)
else
  REF_LABEL="$AGAINST (running)"
  ENV=$(lamp_env "$AGAINST") || exit 1
  REF=$(norm <<<"$ENV"); unset ENV
fi

echo "switch-drift: $RUN_LABEL${BUILD:+ (build ${BUILD:0:9})} against $REF_LABEL"
[[ -z $NOTE ]] || echo "$NOTE"
T=$(mktemp -d) || die "mktemp"
trap 'rm -rf "$T"' EXIT
printf '%s\n' "$RUNNING" | sed '/^$/d' >"$T/run"
printf '%s\n' "$REF" | sed '/^$/d' >"$T/ref"
awk -F'\t' -v out="$T" '
  FNR == NR { rs[$1] = $2; rk[$1] = $3; next }
  { fs[$1] = $2; fk[$1] = $3 }
  END {
    for (k in fs) if (!(k in rs)) print "missing\t" k "=" fs[k]
    for (k in rs) if (!(k in fs)) print "extra\t" k "=" rs[k]
    for (k in rs) if ((k in fs) && rk[k] != fk[k]) print "different\t" k ": " rs[k] " vs " fs[k]
  }' "$T/run" "$T/ref" | sort >"$T/all"
for list in missing extra different; do
  case $list in
    missing) echo "missing (in the reference, not running):" ;;
    extra) echo "extra (running, not in the reference):" ;;
    different) echo "different (running vs reference):" ;;
  esac
  lines=$(grep "^$list	" "$T/all" | cut -f2-)
  if [[ -n $lines ]]; then printf '%s\n' "$lines" | sed 's/^/  /'; else echo "  (none)"; fi
done
printf 'missing %d, extra %d, different %d\n' \
  "$(grep -c '^missing' "$T/all")" "$(grep -c '^extra' "$T/all")" "$(grep -c '^different' "$T/all")"
exit 0
