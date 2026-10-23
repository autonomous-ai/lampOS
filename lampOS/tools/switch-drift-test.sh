#!/usr/bin/env bash
# Tests for switch-drift.sh on a fixed running environment and a made-up
# unit: a missing, an extra and a different switch, a secret and a path that
# are never printed but still compared, names, passwords and free text
# shown only as <set> on either side, a number shown, quoted words several to a line, a
# bare Environment= clearing what came before, and UnsetEnvironment=.
# Run: lampOS/tools/switch-drift-test.sh
set -uo pipefail
DRIFT=$(cd "$(dirname "$0")" && pwd)/switch-drift.sh
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
failures=0
check() {
  if eval "$2"; then echo "ok: $1"; else echo "FAIL: $1"; failures=$((failures + 1)); fi
}
cat >"$T/environ" <<'EOF'
PATH=/usr/bin
LAMP_BUS=on
LAMP_SAY=on
LAMP_VOICE_ROUTE=gemini
LAMP_EXTRA_ONLY=on
LAMP_API_KEY=sk-running-secret-0000
LAMP_BUS_ROOT=/run/lampos-bus
LAMP_ENDPOINT=user@example.test
LAMP_SAME_SECRET_TOKEN=abc123
LAMP_GONE=on
LAMP_SECOND_KEY_TERMS=Mai,Bình,Lan Anh
LAMP_WIFI_PASSWORD=hunter22
LAMP_SUDO_PASS=s3cret
LAMP_GREETING=Hello there
LAMP_SCALE=0.75
EOF
cat >"$T/unit" <<'EOF'
[Service]
Environment=LAMP_OLD=cleared
Environment=
# Environment=LAMP_COMMENTED=on
Environment=LAMP_BUS=on "LAMP_SAY=on"
Environment='LAMP_VOICE_ROUTE=second' LAMP_MISSING_ONE=on
Environment=LAMP_API_KEY=sk-product-secret-1111
Environment=LAMP_BUS_ROOT=/run/lampos-bus-other LAMP_ENDPOINT=user@example.test
Environment=LAMP_SAME_SECRET_TOKEN=abc123
Environment=LAMP_GONE=on LAMP_GONE_TOO=off
UnsetEnvironment=LAMP_GONE LAMP_GONE_TOO=off
Environment="LAMP_SECOND_KEY_TERMS=Mai,Lan Anh" LAMP_WIFI_PASSWORD=hunter23
Environment=LAMP_SUDO_PASS=s3cret LAMP_GREETING=Hi LAMP_SCALE=0.5
EOF
"$DRIFT" --env-file "$T/environ" --against-file "$T/unit" >"$T/out" 2>&1; status=$?
check "it compares and exits 0 whatever the drift" '[[ $status == 0 ]]'
check "the header names both sides" 'head -1 "$T/out" | grep -q "^switch-drift: .*environ against .*unit$"'
check "missing: in the unit, not running" 'grep -A1 "^missing" "$T/out" | grep -qx "  LAMP_MISSING_ONE=on"'
check "extra: running, not in the unit (UnsetEnvironment= dropped LAMP_GONE)" '[[ $(sed -n "/^extra/,/^different/p" "$T/out" | grep -c "^  ") == 2 ]] && grep -qx "  LAMP_EXTRA_ONLY=on" "$T/out" && grep -qx "  LAMP_GONE=on" "$T/out"'
check "different: running vs reference, quoted words read" 'grep -qx "  LAMP_VOICE_ROUTE: gemini vs second" "$T/out"'
check "a differing secret and path say <set>, never their value" 'grep -qx "  LAMP_API_KEY: <set> vs <set>" "$T/out" && grep -qx "  LAMP_BUS_ROOT: <set> vs <set>" "$T/out"'
check "no secret, path or address is printed" '! grep -qE "sk-|/run|example.test|abc123" "$T/out"'
check "equal secrets and addresses are not different" '! grep -qE "LAMP_SAME_SECRET_TOKEN|LAMP_ENDPOINT" "$T/out"'
check "a bare Environment= clears, a comment is no line" '! grep -qE "LAMP_OLD|LAMP_COMMENTED" "$T/out"'
check "names, a password and free text say <set>, still compared" 'grep -qx "  LAMP_SECOND_KEY_TERMS: <set> vs <set>" "$T/out" && grep -qx "  LAMP_WIFI_PASSWORD: <set> vs <set>" "$T/out" && grep -qx "  LAMP_GREETING: <set> vs <set>" "$T/out"'
check "an equal sudo pass is not different" '! grep -q LAMP_SUDO_PASS "$T/out"'
check "a number is a switch's shape and is shown" 'grep -qx "  LAMP_SCALE: 0.75 vs 0.5" "$T/out"'
check "no name, password or free text is printed" '! grep -qE "Mai|Bình|Lan Anh|hunter2|s3cret|Hello|Hi$" "$T/out"'
echo LAMP_BUS=on >"$T/bare"
"$DRIFT" --env-file "$T/bare" --against-file "$T/environ" >"$T/out2" 2>&1
check "the reference side follows the same rule" 'grep -qx "  LAMP_SECOND_KEY_TERMS=<set>" "$T/out2" && grep -qx "  LAMP_WIFI_PASSWORD=<set>" "$T/out2" && grep -qx "  LAMP_SUDO_PASS=<set>" "$T/out2" && ! grep -qE "Bình|hunter22|s3cret" "$T/out2"'
check "the counts" 'tail -1 "$T/out" | grep -qx "missing 1, extra 2, different 7"'
grep '^LAMP_' "$T/environ" >"$T/same"
"$DRIFT" --env-file "$T/environ" --against-file "$T/same" >"$T/out" 2>&1
check "an env file as the reference, no drift" '[[ $(grep -c "^  (none)$" "$T/out") == 3 ]] && tail -1 "$T/out" | grep -qx "missing 0, extra 0, different 0"'
"$DRIFT" >/dev/null 2>&1; status=$?
check "no lamp is a usage error" '[[ $status == 2 ]]'
"$DRIFT" --env-file "$T/nothing" >/dev/null 2>&1; status=$?
check "an unreadable env file is an error" '[[ $status == 1 ]]'
echo "$failures failed"
[[ $failures == 0 ]]
