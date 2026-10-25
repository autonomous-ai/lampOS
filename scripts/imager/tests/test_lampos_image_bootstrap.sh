#!/bin/bash
# Test the actual image Phase 4 using local command mocks and ephemeral keys.
# No updater is executed, downloaded, chrooted or installed on the host.
set -euo pipefail
SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
REPO=$(cd "${SCRIPT_DIR}/../../.." && pwd -P)
ROOT=$(mktemp -d "${TMPDIR:-/tmp}/lampos-image-bootstrap-shell.XXXXXX")
ROOT=$(cd "$ROOT" && pwd -P)
chmod 0700 "$ROOT"
printf 'Bootstrap integration evidence: %s\n' "$ROOT"
HELPER="${REPO}/scripts/imager/lib/verify_lampos_envelope.sh"
[ -f "$HELPER" ] || { echo 'Verifier is not ready; no test result claimed' >&2; exit 1; }
fail() { echo "FAIL: $*" >&2; exit 1; }
sha() { openssl dgst -sha256 -r "$1" | cut -c1-64; }
extract_phase() {
  # Preserve each source line exactly; only the owned /work binding changes later.
  sed -n '/^# Phase 4:/,/^# Phase 5:/{ /^# Phase 5:/!p; }' "$1"
}
extract_phase "$REPO/scripts/imager/build-orangepi-v2.sh" > "$ROOT/phase4-current.sh"
grep -q '^# Phase 4:' "$ROOT/phase4-current.sh" || fail 'Phase 4 was not found'
VERSIONS=(current)
if [ -n "${LAMPOS_BOOTSTRAP_BASELINE_REF:-}" ]; then
  git -C "$REPO" show "${LAMPOS_BOOTSTRAP_BASELINE_REF}:scripts/imager/build-orangepi-v2.sh" > "$ROOT/baseline-source.sh"
  extract_phase "$ROOT/baseline-source.sh" > "$ROOT/phase4-baseline.sh"
  grep -q '^# Phase 4:' "$ROOT/phase4-baseline.sh" || fail 'baseline Phase 4 was not found'
  VERSIONS+=(baseline)
fi
openssl genpkey -algorithm ED25519 -out "$ROOT/ephemeral-key.pem"
openssl pkey -in "$ROOT/ephemeral-key.pem" -pubout -outform DER -out "$ROOT/key.der"
tail -c 32 "$ROOT/key.der" > "$ROOT/key.raw"
KEY_ID=$(sha "$ROOT/key.raw" | cut -c1-16)
openssl base64 -A -in "$ROOT/key.raw" > "$ROOT/key.pub"
printf '\n' >> "$ROOT/key.pub"

# The shared mock rejects unexpected commands, arguments and paths. Its only
# executable input is this authored shell; downloaded fixture bytes never run.
cat > "$ROOT/mock.sh" <<'MOCK'
#!/bin/bash
set -euo pipefail
name=${0##*/}
die() { echo "MOCK_ERROR: $*" >&2; exit 97; }
owned() {
  case "$1" in "$BOOT_CASE"/*) ;; *) die "path outside case: $1" ;; esac
  case "$1" in */../*|*/..|*/./*|*/.) die "noncanonical path: $1" ;; esac
}
event() { jq -cn --arg command "$name" "$@" >> "$BOOT_CASE/events.jsonl"; }
decode() { jq -rj '.manifest' "$1" | openssl base64 -d -A; }
case "$name" in
  curl)
    [ "$#" -eq 4 ] && [ "$1" = -fsSL ] && [ "$2" = -o ] || die 'unexpected curl arguments'
    target=$3; url=$4; owned "$target"
    event --arg url "$url" --arg target "$target" '{command:$command,url:$url,target:$target}'
    case "$url" in
      https://fixture.invalid/channel/lampos-manifest.json) cp "$BOOT_CASE/$BOOT_ENVELOPE" "$target" ;;
      https://fixture.invalid/build-A/lamp-update)
        if [ "$BOOT_SCENARIO" = tampered ]; then cp "$BOOT_CASE/tampered" "$target"; else cp "$BOOT_CASE/payload" "$target"; fi ;;
      https://fixture.invalid/attacker/lamp-update) cp "$BOOT_CASE/attacker-payload" "$target" ;;
      *) die "unexpected fetch: $url" ;;
    esac ;;
  install)
    [ "$#" -eq 5 ] && [ "$1" = -D ] && [ "$2" = -m ] && [ "$3" = 0644 ] || die 'unexpected install arguments'
    owned "$4"; owned "$5"; mkdir -p "$(dirname "$5")"; cp "$4" "$5"; chmod 0644 "$5"
    event --arg target "$5" '{command:$command,target:$target}' ;;
  tee)
    [ "$#" -eq 1 ] || die 'unexpected tee arguments'
    owned "$1"; cat > "$1"; cat "$1"
    event --arg target "$1" '{command:$command,target:$target}' ;;
  chroot)
    [ "$#" -eq 11 ] || die 'unexpected chroot argument count'
    [ "$1" = "$BOOT_CASE/mnt" ] && [ "$2" = /var/tmp/lampos-channel/lamp-update ] && [ "$3" = install ] \
      && [ "$4" = --channel-url ] && [ "$6" = --channel ] && [ "$7" = lamp-voice ] \
      && [ "$8" = --key-file ] && [ "$9" = /etc/lampOS/update/key.pub ] \
      && [ "${10}" = --setup ] && [ "${11}" = on ] || die 'unexpected chroot arguments'
    mnt=$1; url=$5; stage="$mnt/var/tmp/lampos-channel"; owned "$mnt"
    case "$url" in
      file:///var/tmp/lampos-channel/) decode "$stage/lampos-manifest.json" > "$BOOT_CASE/selected.json" ;;
      https://fixture.invalid/channel/) decode "$BOOT_CASE/envelope-B.json" > "$BOOT_CASE/selected.json" ;;
      *) die "unexpected install URL: $url" ;;
    esac
    expected="$BOOT_CASE/payload"
    [ "$BOOT_SCENARIO" != attacker ] || expected="$BOOT_CASE/attacker-payload"
    actual_hash=$(openssl dgst -sha256 -r "$stage/lamp-update" | cut -c1-64)
    expected_hash=$(openssl dgst -sha256 -r "$expected" | cut -c1-64)
    [ "$actual_hash" = "$expected_hash" ] || die 'unexpected updater bytes at execution boundary'
    count=0
    if [ -f "$BOOT_CASE/events.jsonl" ]; then count=$(jq -s '[.[] | select(.command == "chroot")] | length' "$BOOT_CASE/events.jsonl"); fi
    event --arg url "$url" --slurpfile selected "$BOOT_CASE/selected.json" --argjson call "$((count + 1))" \
      '{command:$command,url:$url,sequence:$selected[0].sequence,commit:$selected[0].commit,call:$call}'
    if [ "$BOOT_SCENARIO" = wrong-record ]; then decode "$BOOT_CASE/envelope-B.json" > "$BOOT_CASE/recorded.json"; else cp "$BOOT_CASE/selected.json" "$BOOT_CASE/recorded.json"; fi
    mkdir -p "$mnt/var/lib/lampos-update"
    jq '{installed:{sequence:.sequence,commit:.commit},systemd_owed:false}' "$BOOT_CASE/recorded.json" > "$mnt/var/lib/lampos-update/state.json"
    for file in etc/lampOS/setup/serve.json etc/lampOS/setup/reset-plan.json opt/lampOS/setup/current/lamp-setup opt/lampOS/update/current/lamp-update; do
      mkdir -p "$(dirname "$mnt/$file")"; printf 'synthetic fixture\n' > "$mnt/$file"
    done
    wants="$mnt/etc/systemd/system/multi-user.target.wants"; mkdir -p "$wants"
    for unit in lamp-setup.service lampos-setup-done.path lamp-update.service; do
      [ -L "$wants/$unit" ] || ln -s "/fixture-only/$unit" "$wants/$unit"
    done
    if [ "$BOOT_SCENARIO" = mutable-retry ] && [ "$count" -eq 0 ]; then printf 'model could not be had\n'; fi ;;
  *) die "unexpected command: $name" ;;
esac
MOCK
chmod 0700 "$ROOT/mock.sh"
make_envelope() {
  local label=$1 sequence=$2 payload=$3 files_url=$4 commit
  if [ "$label" = A ]; then commit=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa; else commit=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb; fi
  jq -cn --arg commit "$commit" --argjson sequence "$sequence" --arg url "$files_url" \
    --arg hash "$(sha "$payload")" --argjson bytes "$(wc -c < "$payload" | tr -d ' ')" \
    '{schema:1,channel:"lamp-voice",sequence:$sequence,commit:$commit,files_url:$url,files:[{name:"lamp-update",sha256:$hash,size:$bytes}],units:[]}' \
    > "$BOOT_CASE/manifest-$label.json"
  openssl pkeyutl -sign -rawin -inkey "$ROOT/ephemeral-key.pem" -in "$BOOT_CASE/manifest-$label.json" -out "$BOOT_CASE/signature-$label.raw"
  openssl base64 -A -in "$BOOT_CASE/manifest-$label.json" > "$BOOT_CASE/manifest-$label.base64"
  openssl base64 -A -in "$BOOT_CASE/signature-$label.raw" > "$BOOT_CASE/signature-$label.base64"
  jq -cn --arg key "$KEY_ID" --rawfile manifest "$BOOT_CASE/manifest-$label.base64" --rawfile signature "$BOOT_CASE/signature-$label.base64" \
    '{format:"lampos-update/1",key:$key,manifest:$manifest,signature:$signature}' > "$BOOT_CASE/envelope-$label.json"
}
passed=0
for version in "${VERSIONS[@]}"; do
  for scenario in attacker mutable-retry wrong-record tampered local; do
    BOOT_CASE="$ROOT/$version-$scenario"; mkdir -m 0700 "$BOOT_CASE"
    mkdir "$BOOT_CASE/input" "$BOOT_CASE/mnt" "$BOOT_CASE/work" "$BOOT_CASE/bin" "$BOOT_CASE/lib"
    BOOT_SCENARIO=$scenario; BOOT_ENVELOPE=envelope-A.json
    export BOOT_CASE BOOT_SCENARIO BOOT_ENVELOPE
    cp "$ROOT/key.pub" "$BOOT_CASE/input/key.pub"
    printf 'fixture updater bytes; this file must never execute\n' > "$BOOT_CASE/payload"
    cp "$BOOT_CASE/payload" "$BOOT_CASE/tampered"; printf TAMPERED >> "$BOOT_CASE/tampered"
    printf 'attacker-controlled updater; never execute\n' > "$BOOT_CASE/attacker-payload"
    make_envelope A 101 "$BOOT_CASE/payload" https://fixture.invalid/build-A/
    make_envelope B 102 "$BOOT_CASE/payload" https://fixture.invalid/build-B/
    if [ "$scenario" = attacker ]; then
      jq --arg hash "$(sha "$BOOT_CASE/attacker-payload")" --argjson bytes "$(wc -c < "$BOOT_CASE/attacker-payload" | tr -d ' ')" \
        '.files_url="https://fixture.invalid/attacker/" | .files[0].sha256=$hash | .files[0].size=$bytes' \
        "$BOOT_CASE/manifest-A.json" > "$BOOT_CASE/substituted.json"
      openssl base64 -A -in "$BOOT_CASE/substituted.json" > "$BOOT_CASE/substituted.base64"
      jq --rawfile manifest "$BOOT_CASE/substituted.base64" '.manifest=$manifest' "$BOOT_CASE/envelope-A.json" > "$BOOT_CASE/envelope-ATTACK.json"
      BOOT_ENVELOPE=envelope-ATTACK.json; export BOOT_ENVELOPE
    fi
    if [ "$scenario" = local ]; then
      mkdir "$BOOT_CASE/input/channel"
      cp "$BOOT_CASE/envelope-A.json" "$BOOT_CASE/input/channel/lampos-manifest.json"
      cp "$BOOT_CASE/payload" "$BOOT_CASE/input/channel/lamp-update"
    fi
    if [ "$scenario" = mutable-retry ]; then
      mkdir "$BOOT_CASE/input/models"; printf '{"models":[]}\n' > "$BOOT_CASE/input/models.json"
    fi
    for command in curl chroot install tee; do cp "$ROOT/mock.sh" "$BOOT_CASE/bin/$command"; done
    if ! command -v sha256sum >/dev/null 2>&1; then
      printf '#!/bin/sh\nexec shasum -a 256 "$@"\n' > "$BOOT_CASE/bin/sha256sum"; chmod 0700 "$BOOT_CASE/bin/sha256sum"
    fi
    ln -s "$HELPER" "$BOOT_CASE/lib/verify_lampos_envelope.sh"
    {
      printf 'set -euo pipefail\nlog() { printf "%%s\\n" "$*"; }\nerr() { printf "%%s\\n" "$*" >&2; exit 1; }\n'
      # Harness binding only: direct fixed /work paths into this owned case.
      sed "s|/work/|$BOOT_CASE/work/|g" "$ROOT/phase4-$version.sh"
    } > "$BOOT_CASE/driver.sh"
    status=0
    V2_INPUT="$BOOT_CASE/input" MNT="$BOOT_CASE/mnt" CHROOT_CHANNEL=/var/tmp/lampos-channel \
      LAMPOS_CHANNEL=lamp-voice LAMPOS_CHANNEL_URL=https://fixture.invalid/channel/ \
      PATH="$BOOT_CASE/bin:$PATH" bash "$BOOT_CASE/driver.sh" \
      > "$BOOT_CASE/stdout.txt" 2> "$BOOT_CASE/stderr.txt" || status=$?
    cat "$BOOT_CASE/stdout.txt" "$BOOT_CASE/stderr.txt" > "$BOOT_CASE/diagnostic.txt"
    ! grep -Eq 'MOCK_ERROR|command not found|No such file or directory|syntax error' "$BOOT_CASE/diagnostic.txt" \
      || fail "$version/$scenario: harness failure (see $BOOT_CASE/diagnostic.txt)"
    [ -f "$BOOT_CASE/events.jsonl" ] || fail "$version/$scenario: no mock events"
    calls=$(jq -s '[.[] | select(.command == "chroot")] | length' "$BOOT_CASE/events.jsonl")
    if [ "$version" = current ] && [ "$scenario" = attacker ]; then
      [ "$status" -ne 0 ] && [ "$calls" -eq 0 ] || fail 'attacker reached execution boundary'
      jq -se '[.[] | select(.command=="curl") | .url] == ["https://fixture.invalid/channel/lampos-manifest.json"]' "$BOOT_CASE/events.jsonl" >/dev/null || fail 'attacker payload was fetched'
      grep -q 'signature does not match' "$BOOT_CASE/diagnostic.txt" || fail 'attacker refusal was not signature verification'
    elif [ "$scenario" = tampered ]; then
      [ "$status" -ne 0 ] && [ "$calls" -eq 0 ] && grep -q 'SHA-256 differs' "$BOOT_CASE/diagnostic.txt" || fail "$version: payload tamper refusal missing"
    elif [ "$version" = current ] && [ "$scenario" = wrong-record ]; then
      [ "$status" -ne 0 ] && [ "$calls" -eq 1 ] && grep -q 'different build' "$BOOT_CASE/diagnostic.txt" || fail 'wrong recorded build accepted'
    else
      [ "$status" -eq 0 ] || fail "$version/$scenario: unexpected exit $status (see $BOOT_CASE/diagnostic.txt)"
      expected_calls=1; [ "$scenario" != mutable-retry ] || expected_calls=2
      [ "$calls" -eq "$expected_calls" ] || fail "$version/$scenario: wrong install count"
      if [ "$scenario" = mutable-retry ]; then
        expected_sequence=101; [ "$version" != baseline ] || expected_sequence=102
        jq -se --argjson sequence "$expected_sequence" '[.[] | select(.command=="chroot")] | all(.sequence==$sequence)' "$BOOT_CASE/events.jsonl" >/dev/null || fail "$version: wrong retry build"
      fi
      if [ "$scenario" = local ]; then
        jq -se 'all(.[]; .command!="curl") and ([.[] | select(.command=="chroot")][0].sequence==101)' "$BOOT_CASE/events.jsonl" >/dev/null || fail "$version: local channel fetched remote bytes"
      fi
      if [ "$version" = baseline ] && [ "$scenario" = attacker ]; then
        jq -se 'any(.[]; .command=="curl" and .url=="https://fixture.invalid/attacker/lamp-update")' "$BOOT_CASE/events.jsonl" >/dev/null || fail 'baseline did not reach attacker payload'
      fi
    fi
    jq -cn --arg version "$version" --arg scenario "$scenario" --argjson exit "$status" --argjson calls "$calls" \
      '{version:$version,scenario:$scenario,exit:$exit,install_calls:$calls,expected_observation:true}' >> "$ROOT/results.jsonl"
    passed=$((passed + 1)); printf 'PASS %s/%s: exit=%s, installs=%s\n' "$version" "$scenario" "$status" "$calls"
  done
done
printf '%s behavioral observations passed; evidence retained at %s\n' "$passed" "$ROOT"
