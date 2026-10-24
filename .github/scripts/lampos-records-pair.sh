#!/usr/bin/env bash
# CI-only published-pair driver. Actual release behavior lives in the Rust tests.
set -euo pipefail
export LC_ALL=C
SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
SELF="$SCRIPT_DIR/lampos-records-pair.sh"
SCHEMA='include "lampos-records-pair"; '
JQ=(jq -L "$SCRIPT_DIR")
fail() { printf '%s\n' "$*" >&2; exit 1; }
hash() { sha256sum -- "$1" | cut -d ' ' -f 1; }
regular() {
  test -f "$1" && test ! -L "$1" || fail "not a regular non-symlink file: $1"
  test "$(stat -c %h -- "$1")" = 1 || fail "linked file: $1"
  test "$(stat -c %s -- "$1")" -le "$2" || fail "file exceeds bound: $1"
}
elf() {
  local file=$1 out=$2
  timeout --kill-after=1s 5s readelf -h -l -- "$file" > "$out"
  grep -Eq 'Class:[[:space:]]+ELF64$' "$out"
  grep -Eq 'Data:[[:space:]]+2.s complement, little endian$' "$out"
  grep -Eq 'Machine:[[:space:]]+AArch64$' "$out"
  test "$(grep -c 'Requesting program interpreter:' "$out")" = 1
  grep -Fq '[Requesting program interpreter: /lib/ld-linux-aarch64.so.1]' "$out"
}
verify_payload() {
  local release=$1 part=$2 file=$3 expected size
  size=$("${JQ[@]}" -nr --arg r "$release" --arg p "$part" "${SCHEMA}pins[\$r][\$p].size")
  expected=$("${JQ[@]}" -nr --arg r "$release" --arg p "$part" "${SCHEMA}pins[\$r][\$p].sha256")
  regular "$file" "$size"
  test "$(stat -c %s -- "$file")" = "$size"
  test "$(hash "$file")" = "$expected"
}
download() {
  local url=$1 file=$2 maximum=$3
  test ! -e "$file" && test ! -L "$file"
  # head caps bytes even when an old curl receives no Content-Length. timeout
  # bounds the whole pipeline and owns its process group; pipefail keeps errors.
  timeout --signal=TERM --kill-after=5s 70s /bin/bash -c '
    set -euo pipefail
    /usr/bin/curl --disable --fail --location --silent --show-error \
      --proto =https --proto-redir =https --tlsv1.2 --max-redirs 5 \
      --connect-timeout 10 --max-time 60 --max-filesize "$3" "$1" \
      | /usr/bin/head -c "$2"
  ' pair-download "$url" "$((maximum+1))" "$maximum" > "$file" 2> "$file.curl.log"
  regular "$file" "$maximum"
}
fetch() {
  test "$(id -u)" -ne 0
  mkdir -m 700 -- "$ARTIFACTS"
  "${JQ[@]}" -n "${SCHEMA}pins" > "$EVIDENCE/pins.json"
  : > "$EVIDENCE/verified-assets.jsonl"
  local release part asset size sha url folder metadata decoded
  for release in 6408 6449; do
    folder="$ARTIFACTS/$release"; mkdir -m 700 -- "$folder"
    metadata="$EVIDENCE/release-$release.json"
    download "https://api.github.com/repos/autonomous-ai/lampOS/releases/tags/lampos-lamp-voice-$release" "$metadata" 524288
    jq -e --arg r "$release" '.tag_name==("lampos-lamp-voice-"+$r) and .draft==false' "$metadata" >/dev/null
    for part in lampos-manifest.json lamp-records lamp-voice; do
      IFS=$'\t' read -r asset size sha < <("${JQ[@]}" -nr --arg r "$release" --arg p "$part" "${SCHEMA}pins[\$r][\$p]|[.asset,.size,.sha256]|@tsv")
      url="https://github.com/autonomous-ai/lampOS/releases/download/lampos-lamp-voice-$release/$part"
      jq -e --argjson id "$asset" --arg n "$part" --argjson size "$size" --arg sha "$sha" --arg url "$url" \
        '[.assets[]|select(.id==$id or .name==$n)]|length==1 and .[0].id==$id and .[0].name==$n and .[0].size==$size and .[0].digest==("sha256:"+$sha) and .[0].browser_download_url==$url' "$metadata" >/dev/null
      download "$url" "$folder/$part" "$size"
      verify_payload "$release" "$part" "$folder/$part"
      if test "$part" = lampos-manifest.json; then
        jq -e 'keys==["format","key","manifest","signature"] and .format=="lampos-update/1" and .key=="54213f8d3aa2f241"' "$folder/$part" >/dev/null
        jq -er .signature "$folder/$part" | base64 --decode > "$EVIDENCE/signature-$release.bin"
        test "$(stat -c %s -- "$EVIDENCE/signature-$release.bin")" = 64
        # Only the length is evidence; do not upload redundant signature bytes.
        rm -- "$EVIDENCE/signature-$release.bin"
        decoded="$EVIDENCE/decoded-manifest-$release.json"
        jq -er .manifest "$folder/$part" | base64 --decode > "$decoded"
        regular "$decoded" 65536
        "${JQ[@]}" -e --arg r "$release" "${SCHEMA}manifest(\$r)" "$decoded" >/dev/null
      else
        elf "$folder/$part" "$EVIDENCE/elf-$release-$part.txt"
        chmod 0555 -- "$folder/$part"
        test -x "$folder/$part" && test "$(stat -c %a -- "$folder/$part")" = 555
      fi
      jq -n --argjson release "$release" --arg part "$part" --argjson asset "$asset" --argjson size "$size" --arg sha "$sha" \
        '{release:$release,part:$part,asset_id:$asset,size:$size,sha256:$sha,signature_trust_verified:false}' >> "$EVIDENCE/verified-assets.jsonl"
    done
  done
  jq -s 'if length==6 then {complete:true,assets:.,signature_trust_verified:false} else error("incomplete acquisitions") end' \
    "$EVIDENCE/verified-assets.jsonl" > "$EVIDENCE/artifact-verification.json"
}
select_executable() {
  regular "$BUILD_LOG" 134217728
  local executable
  executable=$("${JQ[@]}" -ser --arg root "$WORKSPACE" "${SCHEMA}selected(\$root)" "$BUILD_LOG")
  regular "$executable" 134217728
  test -x "$executable"
  elf "$executable" "$EVIDENCE/test-elf.txt"
  jq -n --arg path "$executable" --arg sha "$(hash "$executable")" --argjson size "$(stat -c %s -- "$executable")" \
    --arg source "$WORKSPACE/lampOS/crates/records/tests/release_pair_systemd.rs" --arg log "$(hash "$BUILD_LOG")" \
    '{path:$path,sha256:$sha,bytes:$size,source:$source,cargo_build_log_sha256:$log}' > "$EVIDENCE/test-executable.json"
}
source_receipt() {
  export GIT_OPTIONAL_LOCKS=0
  local prefix="$EVIDENCE/source-$PHASE" file
  git -C "$WORKSPACE" -c core.fileMode=true diff --exit-code --no-ext-diff --no-textconv HEAD -- > "$prefix.diff"
  git -C "$WORKSPACE" diff --cached --exit-code --no-ext-diff --no-textconv HEAD -- > "$prefix.index-diff"
  git -C "$WORKSPACE" ls-files -z > "$prefix.paths0"
  git -C "$WORKSPACE" ls-files --stage -z > "$prefix.index0"
  : > "$prefix.regular0"; : > "$prefix.links.sha256z"
  while IFS= read -r -d '' file; do
    if test -L "$WORKSPACE/$file"; then
      printf '%s\0%s\0' "$file" "$(readlink -n -- "$WORKSPACE/$file" | sha256sum | cut -d ' ' -f 1)" >> "$prefix.links.sha256z"
    elif test -f "$WORKSPACE/$file"; then
      printf '%s\0' "$file" >> "$prefix.regular0"
    else fail "unsupported tracked source kind: $file"; fi
  done < "$prefix.paths0"
  (cd -- "$WORKSPACE"; xargs -0 -r sha256sum --zero -- < "$prefix.regular0") > "$prefix.files.sha256z"
  (cd -- "$WORKSPACE"; xargs -0 -r stat --printf '%n\0%f\0%y\0' -- < "$prefix.paths0") > "$prefix.stat0"
  jq -n --arg head "$(git -C "$WORKSPACE" rev-parse HEAD)" --arg tree "$(git -C "$WORKSPACE" rev-parse 'HEAD^{tree}')" \
    --arg parents "$(git -C "$WORKSPACE" show -s --format=%P HEAD)" --arg bytes "$(hash "$prefix.files.sha256z")" \
    --arg links "$(hash "$prefix.links.sha256z")" --arg stat "$(hash "$prefix.stat0")" --arg paths "$(hash "$prefix.paths0")" \
    '{checkout:$head,tree:$tree,parents:($parents|split(" ")),file_hashes_sha256:$bytes,symlink_hashes_sha256:$links,modes_mtimes_sha256:$stat,paths_sha256:$paths}' > "$prefix.json"
  if test "$PHASE" = after; then
    cmp "$EVIDENCE/source-before.json" "$prefix.json"
    for file in paths0 index0 files.sha256z links.sha256z stat0; do cmp "$EVIDENCE/source-before.$file" "$prefix.$file"; done
  fi
}
run_cases() {
  test "$(id -u)" -ne 0
  [[ "$ROOT_OUTPUT" =~ ^/run/lamp-records-pair-ci-[0-9]+-[0-9]+$ ]]
  local executable expected release part side file code began
  executable=$(jq -er .path "$EVIDENCE/test-executable.json")
  [[ "$executable" =~ ^"$WORKSPACE/lampOS/target/debug/deps/release_pair_systemd-"[a-f0-9]+$ ]]
  regular "$executable" 134217728
  expected=$(jq -er .sha256 "$EVIDENCE/test-executable.json")
  test "$(hash "$executable")" = "$expected" && test -x "$executable"
  local envs=(PATH=/usr/bin:/bin LANG=C.UTF-8 LC_ALL=C.UTF-8 "LAMP_PAIR_OUTPUT=$ROOT_OUTPUT")
  for release in 6408 6449; do
    side=6408; if test "$release" = 6449; then side=CURRENT; fi
    for part in RECORDS VOICE; do
      file="$ARTIFACTS/$release/lamp-${part,,}"
      verify_payload "$release" "lamp-${part,,}" "$file"
      test -x "$file" && test "$(stat -c %a -- "$file")" = 555
      envs+=("LAMP_PAIR_${side}_${part}=$file")
    done
  done
  timeout --kill-after=1s 10s "$executable" --list --ignored --format terse > "$EVIDENCE/test-list.txt"
  "${JQ[@]}" -Rse "${SCHEMA}split(\"\\n\")|map(select(endswith(\": test\"))|rtrimstr(\": test\"))|must(sort==(tests|sort);\"ignored inventory differs\")|{count:length,ignored_names:.}" \
    "$EVIDENCE/test-list.txt" > "$EVIDENCE/test-inventory.json"
  printf '%s\n' "$executable --ignored --test-threads=1 --nocapture" > "$EVIDENCE/test-command.txt"
  began=$SECONDS
  set +e
  timeout --signal=TERM --kill-after=15s 720s /usr/bin/sudo --non-interactive /usr/bin/env -i "${envs[@]}" \
    /usr/bin/timeout --signal=TERM --kill-after=15s 600s "$executable" --ignored --test-threads=1 --nocapture \
    > "$EVIDENCE/tests.log" 2>&1
  code=$?
  set -e
  jq -n --argjson exit "$code" --argjson seconds "$((SECONDS-began))" '{exit:$exit,elapsed_seconds:$seconds}' > "$EVIDENCE/test-exit.json"
  local good=false
  if test "$code" = 0 && test "$(grep -c '^test result:' "$EVIDENCE/tests.log")" = 1 \
      && grep -Eq '^test result: ok\. 3 passed; 0 failed; 0 ignored; 0 measured; [0-9]+ filtered out;' "$EVIDENCE/tests.log"; then good=true; fi
  jq -n --argjson good "$good" '{exact_three_nonzero_summary:$good,case_receipts_verified:false}' > "$EVIDENCE/test-selection.json"
  test "$(hash "$executable")" = "$expected"
  test "$good" = true || fail "explicit pair command did not pass exactly three cases"
  # The always collector independently requires all three exact terminal JSON
  # receipts and internal/external cleanup; this summary is insufficient alone.
}
manager_show() {
  local unit=$1 prefix=$2 code
  set +e
  timeout --kill-after=1s 5s /usr/bin/systemctl --system show "$unit" \
    --property=Id,LoadState,ActiveState,MainPID,ControlPID,ControlGroup,Job --all --no-pager \
    > "$prefix.txt" 2> "$prefix.stderr"
  code=$?
  set -e
  test "$code" = 0 || test "$code" = 4 || return 1
  regular "$prefix.txt" 8192
  regular "$prefix.stderr" 8192
  "${JQ[@]}" -Rse --arg unit "$unit" "${SCHEMA}
    split(\"\\n\")|map(select(length>0))
    | must(all(.[];contains(\"=\"));\"malformed manager properties\")
    | map(index(\"=\") as \$at|{key:.[0:\$at],value:.[\$at+1:]}) as \$pairs
    | (\$pairs|from_entries)
    | must((keys|length)==(\$pairs|length);\"duplicate manager property\")
    | unit_fields(\$unit)" "$prefix.txt" > "$prefix.json" || return 1
}
cgroup_empty() {
  local unit=$1 group=$2 prefix=$3 dir path n=0
  test "$group" = '' || test "$group" = "/system.slice/$unit" || return 1
  test -d /sys/fs/cgroup && test ! -L /sys/fs/cgroup || return 1
  # This lane qualifies only unified v2. Empty parent cgroup.procs on v1
  # cannot prove that a descendant contains no process.
  test -f /sys/fs/cgroup/cgroup.controllers && test ! -L /sys/fs/cgroup/cgroup.controllers || return 1
  : > "$prefix.jsonl"
  for path in "/sys/fs/cgroup/system.slice/$unit"; do
    n=$((n+1))
    # All components are fixed kernel paths or a validated reserved unit name.
    for dir in /sys/fs/cgroup/system.slice "$path"; do
      test ! -L "$dir" || return 1
      test ! -e "$dir" || test -d "$dir" || return 1
    done
    if test ! -e "$path"; then
      "${JQ[@]}" -ne "${SCHEMA}{controllers:true,absent:true}|empty_v2_group" >/dev/null || return 1
      jq -n --arg path "$path" '{path:$path,unified_v2:true,absent:true,empty:true}' >> "$prefix.jsonl" || return 1
      continue
    fi
    test -d "$path" && test ! -L "$path/cgroup.procs" && test -f "$path/cgroup.procs" || return 1
    timeout --kill-after=1s 2s head -c 65537 -- "$path/cgroup.procs" > "$prefix.$n.procs" || return 1
    test "$(stat -c %s -- "$prefix.$n.procs")" -le 65536 || return 1
    # An inactive main process alone does not prove an empty owned service.
    test ! -s "$prefix.$n.procs" || return 1
    test ! -L "$path/cgroup.events" && test -f "$path/cgroup.events" || return 1
    timeout --kill-after=1s 2s head -c 4097 -- "$path/cgroup.events" > "$prefix.$n.events" || return 1
    test "$(stat -c %s -- "$prefix.$n.events")" -le 4096 || return 1
    "${JQ[@]}" -ne --rawfile members "$prefix.$n.procs" --rawfile events "$prefix.$n.events" \
      "${SCHEMA}{controllers:true,absent:false,members:\$members,events:\$events}|empty_v2_group" >/dev/null || return 1
    jq -n --arg path "$path" '{path:$path,unified_v2:true,absent:false,empty:true,populated:0}' >> "$prefix.jsonl" || return 1
  done
  jq -s '{empty:all(.[];.empty==true),probes:.}' "$prefix.jsonl" > "$prefix.json" || return 1
}
confirmed_unit() {
  local unit=$1 before=$2 prefix=$3 group
  "${JQ[@]}" -e "${SCHEMA}inactive" "$before.json" >/dev/null || return 1
  group=$(jq -er .ControlGroup "$before.json") || return 1
  cgroup_empty "$unit" "$group" "$prefix.group-before" || return 1
  manager_show "$unit" "$prefix.after" || return 1
  cmp -s "$before.json" "$prefix.after.json" || return 1
  cgroup_empty "$unit" "$group" "$prefix.group-after" || return 1
}
cleanup_unit() {
  [[ "$UNIT" =~ ^lamp-records-pair-[0-9]+-[a-z0-9-]+-[0-9]+\.service$ ]]
  "${JQ[@]}" -e --arg unit "$UNIT" "${SCHEMA}ledger|.units|index(\$unit)!=null" "$ROOT_OUTPUT/owned-units.json" >/dev/null
  local folder="$EVIDENCE/owned-cleanup/$UNIT" until n=0
  mkdir -m 755 -- "$folder"
  jq -n --arg unit "$UNIT" '{unit:$unit,exit_confirmed:false}' > "$folder/result.json"
  manager_show "$UNIT" "$folder/before"
  if ! confirmed_unit "$UNIT" "$folder/before" "$folder/initial"; then
    if ! timeout --signal=TERM --kill-after=1s 12s /usr/bin/systemctl --system stop "$UNIT" \
        > "$folder/stop.txt" 2>&1; then
      timeout --kill-after=1s 5s /usr/bin/systemctl --system kill --kill-who=all --signal=KILL "$UNIT" > "$folder/kill.txt" 2>&1 || true
      timeout --kill-after=1s 5s /usr/bin/systemctl --system stop --no-block "$UNIT" > "$folder/stop-queued.txt" 2>&1 || true
    fi
    until=$((SECONDS+8))
    while :; do
      n=$((n+1))
      manager_show "$UNIT" "$folder/poll-$n"
      if confirmed_unit "$UNIT" "$folder/poll-$n" "$folder/proof-$n"; then break; fi
      test "$SECONDS" -lt "$until" || fail "owned unit exit remains unconfirmed: $UNIT"
      # Runtime cleanup polling, not an orchestration watcher.
      sleep 0.2
    done
  fi
  jq -n --arg unit "$UNIT" '{unit:$unit,exit_confirmed:true,main_pid_zero:true,control_pid_zero:true,no_job:true,cgroup_empty:true}' > "$folder/result.json"
}
cleanup_owned() {
  test "$(id -u)" = 0
  local ledger="$ROOT_OUTPUT/owned-units.json" unit until remaining code
  if test ! -e "$ledger" && test ! -L "$ledger"; then
    jq -n '{complete:true,ledger_present:false,units:[],meaning:"Fixture must journal before start; no unit was recorded."}' > "$EVIDENCE/owned-unit-cleanup.json"
    return
  fi
  regular "$ledger" 16384
  test "$(stat -c '%u:%a' -- "$ledger")" = 0:600
  "${JQ[@]}" -e "${SCHEMA}ledger" "$ledger" > "$EVIDENCE/owned-unit-ledger.json"
  mkdir -m 755 -- "$EVIDENCE/owned-cleanup"
  : > "$EVIDENCE/owned-cleanup/units.jsonl"
  until=$((SECONDS+120))
  while IFS= read -r unit; do
    remaining=$((until-SECONDS))
    test "$remaining" -gt 0 || fail "owned cleanup total deadline exhausted"
    set +e
    timeout --signal=TERM --kill-after=1s "${remaining}s" /bin/bash "$SELF" cleanup-unit \
      --evidence "$EVIDENCE" --root-output "$ROOT_OUTPUT" --unit "$unit" \
      > "$EVIDENCE/owned-cleanup/$unit.log" 2>&1
    code=$?
    set -e
    if test -f "$EVIDENCE/owned-cleanup/$unit/result.json"; then
      jq --argjson exit "$code" '. + {driver_exit:$exit} | .exit_confirmed=(.exit_confirmed and $exit==0)' \
        "$EVIDENCE/owned-cleanup/$unit/result.json" >> "$EVIDENCE/owned-cleanup/units.jsonl"
    else
      jq -n --arg unit "$unit" --argjson exit "$code" '{unit:$unit,driver_exit:$exit,exit_confirmed:false}' >> "$EVIDENCE/owned-cleanup/units.jsonl"
    fi
    jq -s '{ledger_present:true,units:.,complete:all(.[];.exit_confirmed==true)}' \
      "$EVIDENCE/owned-cleanup/units.jsonl" > "$EVIDENCE/owned-unit-cleanup.json"
  done < <(jq -r '.units[]' "$EVIDENCE/owned-unit-ledger.json")
  # Also handles an empty, successfully parsed journal.
  jq -s '{ledger_present:true,units:.,complete:all(.[];.exit_confirmed==true)}' \
    "$EVIDENCE/owned-cleanup/units.jsonl" > "$EVIDENCE/owned-unit-cleanup.json"
  jq -e .complete "$EVIDENCE/owned-unit-cleanup.json" >/dev/null
}
verify_cases() {
  local name
  jq -e '.exact_three_nonzero_summary==true' "$EVIDENCE/test-selection.json" >/dev/null
  "${JQ[@]}" -e "${SCHEMA}.count==3 and (.ignored_names|sort)==(tests|sort)" "$EVIDENCE/test-inventory.json" >/dev/null
  : > "$EVIDENCE/terminal-entries.jsonl"
  while IFS= read -r name; do
    "${JQ[@]}" -se --arg name "$name" "${SCHEMA}must(length==1 and .[0].test==\$name;\"wrong terminal filename/identity\")
      | {key:(\$name+\".json\"),value:.[0]}" "$EVIDENCE/fixture-json/$name.json" >> "$EVIDENCE/terminal-entries.jsonl"
  done < <("${JQ[@]}" -nr "${SCHEMA}tests[]")
  "${JQ[@]}" -se --arg root "$ARTIFACTS" "${SCHEMA}from_entries|terminal(\$root)" \
    "$EVIDENCE/terminal-entries.jsonl" > "$EVIDENCE/validated-cases.json"
}
collect() {
  test "$(id -u)" = 0
  [[ "$ROOT_OUTPUT" =~ ^/run/lamp-records-pair-ci-[0-9]+-[0-9]+$ ]]
  test -d "$ROOT_OUTPUT" && test ! -L "$ROOT_OUTPUT"
  test "$(stat -c '%u:%a' -- "$ROOT_OUTPUT")" = 0:700
  local cleanup_code path name size total=0 count=0 failed=0 command_pass=false qualified=false
  set +e
  timeout --signal=TERM --kill-after=2s 140s /bin/bash "$SELF" cleanup \
    --evidence "$EVIDENCE" --root-output "$ROOT_OUTPUT" > "$EVIDENCE/owned-cleanup.log" 2>&1
  cleanup_code=$?
  set -e
  if test ! -f "$EVIDENCE/owned-unit-cleanup.json" || test "$cleanup_code" != 0 \
      || ! jq -e '.complete==true' "$EVIDENCE/owned-unit-cleanup.json" >/dev/null; then
    jq -n --argjson exit "$cleanup_code" '{complete:false,driver_exit:$exit,meaning:"Owned unit cleanup is unconfirmed; retain evidence."}' > "$EVIDENCE/cleanup-failure.json"
    failed=1
  fi
  mkdir -m 755 -- "$EVIDENCE/fixture-json"
  : > "$EVIDENCE/collected-files.jsonl"
  : > "$EVIDENCE/collection-errors.txt"
  # The root-only directory is owned by this fixture. Never traverse children
  # or upload a raw session/executable; only the exact receipt filenames count.
  shopt -s nullglob dotglob
  for path in "$ROOT_OUTPUT"/*; do
    count=$((count+1)); test "$count" -le 64 || { printf '%s\n' 'too many evidence entries' >> "$EVIDENCE/collection-errors.txt"; failed=1; break; }
    name=${path##*/}
    if ! "${JQ[@]}" -ne --arg n "$name" "${SCHEMA}(tests|map(.+\".json\")|index(\$n))!=null or \$n==\"owned-units.json\" or \$n==\"owned-units.pending.json\"" >/dev/null \
        || test ! -f "$path" || test -L "$path" || test "$(stat -c '%u:%a:%h' -- "$path")" != 0:600:1; then
      printf 'rejected evidence entry: %q\n' "$name" >> "$EVIDENCE/collection-errors.txt"; failed=1; continue
    fi
    size=$(stat -c %s -- "$path"); total=$((total+size))
    if test "$size" -gt 1048576 || test "$total" -gt 16777216; then
      printf '%s\n' 'evidence byte bound exceeded' >> "$EVIDENCE/collection-errors.txt"; failed=1; continue
    fi
    cp -- "$path" "$EVIDENCE/fixture-json/$name"
    chmod 0644 -- "$EVIDENCE/fixture-json/$name"
    if ! jq -e 'type=="object"' "$EVIDENCE/fixture-json/$name" >/dev/null; then failed=1; fi
    jq -n --arg n "$name" --argjson size "$size" --arg sha "$(hash "$EVIDENCE/fixture-json/$name")" \
      '{name:$n,bytes:$size,sha256:$sha}' >> "$EVIDENCE/collected-files.jsonl"
  done
  jq -s --argjson bytes "$total" '{files:.,bytes:$bytes,raw_session_files_or_binaries_uploaded:false}' \
    "$EVIDENCE/collected-files.jsonl" > "$EVIDENCE/collected.json"
  if test -f "$EVIDENCE/test-exit.json" && jq -e '.exit==0' "$EVIDENCE/test-exit.json" >/dev/null; then
    command_pass=true
    if /bin/bash "$SELF" verify-cases --evidence "$EVIDENCE" --artifacts "$ARTIFACTS" \
          > "$EVIDENCE/case-validation.stdout" 2> "$EVIDENCE/case-validation.stderr" \
        && test "$failed" = 0; then qualified=true; else failed=1; fi
  fi
  jq -n --argjson passed "$command_pass" --argjson qualified "$qualified" --argjson cleanup "$cleanup_code" \
    '{passing_test_command:$passed,qualified:$qualified,external_cleanup_exit:$cleanup,
      meaning:"Only exact successful case receipts qualify; failed or absent execution retains partial evidence without a pass."}' \
    > "$EVIDENCE/case-verification.json"
  # Root-created evidence must be readable by the ordinary upload action.
  find "$EVIDENCE/fixture-json" "$EVIDENCE/owned-cleanup" -type f -exec chmod 0644 {} + 2>/dev/null || true
  for path in "$EVIDENCE"/*.json "$EVIDENCE"/*.jsonl "$EVIDENCE"/*.txt "$EVIDENCE"/*.stderr "$EVIDENCE"/*.log; do chmod 0644 -- "$path"; done
  test "$failed" = 0 || fail "pair evidence or owned cleanup remains unconfirmed"
}
main() {
  local action=${1:?action required}; shift
  WORKSPACE= ARTIFACTS= EVIDENCE= BUILD_LOG= ROOT_OUTPUT= PHASE= UNIT=
  while test "$#" -gt 0; do
    case "$1" in
      --workspace) WORKSPACE=$2;; --artifacts) ARTIFACTS=$2;; --evidence) EVIDENCE=$2;;
      --build-log) BUILD_LOG=$2;; --root-output) ROOT_OUTPUT=$2;; --phase) PHASE=$2;; --unit) UNIT=$2;;
      *) fail "unknown argument: $1";;
    esac
    shift 2
  done
  test -d "$EVIDENCE" && test ! -L "$EVIDENCE" || fail 'create the ordinary evidence directory first'
  if test -n "$WORKSPACE"; then WORKSPACE=$(cd -- "$WORKSPACE" && pwd -P); fi
  if test -n "$ARTIFACTS"; then [[ "$ARTIFACTS" = /* ]] || fail 'absolute artifact path required'; fi
  case "$action" in
    fetch) fetch;; select) select_executable;;
    source) test "$PHASE" = before || test "$PHASE" = after; source_receipt;;
    run) run_cases;; cleanup) cleanup_owned;; cleanup-unit) cleanup_unit;; collect) collect;; verify-cases) verify_cases;;
    *) fail 'unknown action';;
  esac
}
if test "${BASH_SOURCE[0]}" = "$0"; then main "$@"; fi
