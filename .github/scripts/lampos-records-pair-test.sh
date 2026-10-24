#!/usr/bin/env bash
# Offline CI boundary controls only. No artifact, Cargo, systemd or device runs.
set -euo pipefail
DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
TMP=$(mktemp -d "${TMPDIR:-/tmp}/records-pair-ci-offline.XXXXXXXX")
trap 'rm -rf -- "$TMP"' EXIT
JQ=(jq -L "$DIR")
S='include "lampos-records-pair"; '
COUNT=0
pass() { COUNT=$((COUNT+1)); printf 'ok %s - %s\n' "$COUNT" "$1"; }
accept() { local name=$1 filter=$2 file=$3; "${JQ[@]}" -e "$S$filter" "$file" > "$TMP/out"; pass "$name"; }
reject() {
  local name=$1 filter=$2 file=$3
  if "${JQ[@]}" -e "$S$filter" "$file" > "$TMP/out" 2> "$TMP/error"; then printf 'unexpected acceptance: %s\n' "$name" >&2; exit 1; fi
  pass "$name"
}
"${JQ[@]}" -n "${S}reduce tests[] as \$name ({}; .[\$name+\".json\"]={
  version:1,test:\$name,result:\"passed\",expected_pair:[6408,6449],
  artifacts:expected_artifacts(\"/scratch/pinned-artifacts\"),architecture:\"aarch64\",native_root_systemd:true,
  error:null,cleanup_error:null,cleanup_confirmed:true,no_guard_override:true,
  signature_trust_verified:false,full_rotation_or_device_acceptance:false,
  observations:[{synthetic_offline_control:true}],commands:[{reaped:true,group_absent:true}]
})" > "$TMP/receipts.json"
accept 'all three bound successful terminal cases' 'terminal("/scratch/pinned-artifacts")|length==3' "$TMP/receipts.json"
mutate_receipt() {
  local name=$1 expression=$2
  "${JQ[@]}" "$S$expression" "$TMP/receipts.json" > "$TMP/mutated.json"
  reject "$name" 'terminal("/scratch/pinned-artifacts")' "$TMP/mutated.json"
}
mutate_receipt 'missing third case cannot pass' 'del(.[tests[2]+".json"])'
mutate_receipt 'wrong case label cannot pass' '.[tests[0]+".json"].test=tests[1]'
mutate_receipt 'preflight failure is not a successful case' '.[tests[0]+".json"].result="failed"'
mutate_receipt 'internal cleanup failure cannot pass' '.[tests[0]+".json"].cleanup_confirmed=false'
mutate_receipt 'cleanup error cannot hide behind a true flag' '.[tests[0]+".json"].cleanup_error="unconfirmed"'
mutate_receipt 'wrong release digest cannot pass' '.[tests[0]+".json"].artifacts[0].sha256=("0"*64)'
mutate_receipt 'wrong artifact path cannot pass' '.[tests[1]+".json"].artifacts[0].path="/unbound/lamp-records"'
mutate_receipt 'duplicate artifact does not stand for the fourth payload' '.[tests[0]+".json"].artifacts[3]=.[tests[0]+".json"].artifacts[0]'
mutate_receipt 'wrong source commit cannot pass' '.[tests[0]+".json"].artifacts[1].source=("0"*40)'
mutate_receipt 'wrong architecture cannot pass' '.[tests[0]+".json"].architecture="x86_64"'
mutate_receipt 'guard override cannot pass' '.[tests[0]+".json"].no_guard_override=false'
mutate_receipt 'unreaped command cannot pass' '.[tests[0]+".json"].commands[0].reaped=false'
mutate_receipt 'live process group cannot pass' '.[tests[0]+".json"].commands[0].group_absent=false'
mutate_receipt 'compacted cleanup failure marker cannot pass' '.[tests[0]+".json"].commands=[{unconfirmed_command_cleanup:true}]'
mutate_receipt 'empty observations cannot pass' '.[tests[0]+".json"].observations=[]'
mutate_receipt 'unproved signature acceptance cannot pass' '.[tests[0]+".json"].signature_trust_verified=true'

# The actual collector invokes this same shell action after a passing command.
mkdir "$TMP/evidence" "$TMP/evidence/fixture-json"
"${JQ[@]}" -n "${S}{count:3,ignored_names:tests}" > "$TMP/evidence/test-inventory.json"
printf '{"exact_three_nonzero_summary":true}\n' > "$TMP/evidence/test-selection.json"
while IFS= read -r name; do
  jq --arg name "$name" '.[$name+".json"]' "$TMP/receipts.json" > "$TMP/evidence/fixture-json/$name.json"
done < <("${JQ[@]}" -nr "${S}tests[]")
bash "$DIR/lampos-records-pair.sh" verify-cases --evidence "$TMP/evidence" --artifacts /scratch/pinned-artifacts
pass 'actual terminal driver accepts the exact three filenames'
name=$("${JQ[@]}" -nr "${S}tests[0]")
cp "$TMP/evidence/fixture-json/$name.json" "$TMP/saved-case.json"
printf '\n{}\n' >> "$TMP/evidence/fixture-json/$name.json"
if bash "$DIR/lampos-records-pair.sh" verify-cases --evidence "$TMP/evidence" --artifacts /scratch/pinned-artifacts > "$TMP/out" 2> "$TMP/error"; then exit 1; fi
pass 'actual terminal driver refuses multiple JSON documents in one case'
cp "$TMP/saved-case.json" "$TMP/evidence/fixture-json/$name.json"
jq '.test="other_case"' "$TMP/saved-case.json" > "$TMP/evidence/fixture-json/$name.json"
if bash "$DIR/lampos-records-pair.sh" verify-cases --evidence "$TMP/evidence" --artifacts /scratch/pinned-artifacts > "$TMP/out" 2> "$TMP/error"; then exit 1; fi
pass 'actual terminal driver binds filename to case identity'
cp "$TMP/saved-case.json" "$TMP/evidence/fixture-json/$name.json"
printf '{"exact_three_nonzero_summary":false}\n' > "$TMP/evidence/test-selection.json"
if bash "$DIR/lampos-records-pair.sh" verify-cases --evidence "$TMP/evidence" --artifacts /scratch/pinned-artifacts > "$TMP/out" 2> "$TMP/error"; then exit 1; fi
pass 'actual terminal driver rejects a nonpassing/zero-selection summary'

printf '{"version":1,"units":["lamp-records-pair-123-old-0.service"]}\n' > "$TMP/ledger.json"
accept 'exact owned ledger' ledger "$TMP/ledger.json"
for units in '["hal.service"]' '["lamp-records-pair-123-old-0.service","os-server.service"]' '["lamp-records-pair-123-old-0.service","lamp-records-pair-123-old-0.service"]'; do
  jq -n --argjson units "$units" '{version:1,units:$units}' > "$TMP/mutated.json"
  reject 'unrelated or duplicate ownership refused before any manager command' ledger "$TMP/mutated.json"
done
printf '{"Id":"lamp-records-pair-123-old-0.service","LoadState":"loaded","ActiveState":"inactive","MainPID":"0","ControlPID":"0","Job":"","ControlGroup":"/system.slice/lamp-records-pair-123-old-0.service"}\n' > "$TMP/unit.json"
accept 'inactive manager snapshot requires both zero PIDs and no job' 'unit_fields("lamp-records-pair-123-old-0.service")|inactive' "$TMP/unit.json"
for change in '.ControlPID="123"' '.Job="42"' 'del(.ControlPID)' '.Id="hal.service"' '.ControlGroup="/system.slice/hal.service"'; do
  jq "$change" "$TMP/unit.json" > "$TMP/mutated.json"
  reject 'incomplete or other-owned manager snapshot cannot prove exit' 'unit_fields("lamp-records-pair-123-old-0.service")|inactive' "$TMP/mutated.json"
done

printf '%s\n' '{"controllers":true,"absent":false,"members":"","events":"populated 0\nfrozen 0\n"}' > "$TMP/group.json"
accept 'unified v2 empty parent and descendants' empty_v2_group "$TMP/group.json"
printf '{"controllers":true,"absent":true}\n' > "$TMP/group-absent.json"
accept 'exact absent group on verified unified v2' empty_v2_group "$TMP/group-absent.json"
for change in '.controllers=false' 'del(.events)' '.events="populated 1\nfrozen 0\n"' '.events="populated 0\npopulated 0\n"' '.members="123\n"'; do
  jq "$change" "$TMP/group.json" > "$TMP/mutated.json"
  reject 'v1 missing events live descendants or members cannot prove cleanup' empty_v2_group "$TMP/mutated.json"
done
jq '.controllers=false' "$TMP/group-absent.json" > "$TMP/mutated.json"
reject 'absence does not bypass unified-v2 requirement' empty_v2_group "$TMP/mutated.json"

"${JQ[@]}" -n "${S}pins[\"6408\"] as \$p | {schema:1,channel:\"lamp-voice\",sequence:6408,commit:\$p.source,
 files_url:\"https://github.com/autonomous-ai/lampOS/releases/download/lampos-lamp-voice-6408/\",
 files:[{name:\"lamp-records\",sha256:\$p[\"lamp-records\"].sha256,size:\$p[\"lamp-records\"].size},
 {name:\"lamp-voice\",sha256:\$p[\"lamp-voice\"].sha256,size:\$p[\"lamp-voice\"].size}]}" > "$TMP/manifest.json"
accept 'decoded manifest binds fixed source and both payloads' 'manifest("6408")' "$TMP/manifest.json"
for change in '.commit=("0"*40)' '.sequence=6449' '.files += [.files[0]]' '.files[1].sha256=("0"*64)'; do
  jq "$change" "$TMP/manifest.json" > "$TMP/mutated.json"
  reject 'rehashed wrong decoded binding refused' 'manifest("6408")' "$TMP/mutated.json"
done

jq -n '[{reason:"compiler-artifact",target:{name:"release_pair_systemd",kind:["test"],src_path:"/scratch/checkout/lampOS/crates/records/tests/release_pair_systemd.rs"},profile:{test:true},
 executable:"/scratch/checkout/lampOS/target/debug/deps/release_pair_systemd-0123",filenames:["/scratch/checkout/lampOS/target/debug/deps/release_pair_systemd-0123"]},
 {reason:"build-finished",success:true}]' > "$TMP/cargo.json"
accept 'current single successful Cargo test artifact' 'selected("/scratch/checkout")' "$TMP/cargo.json"
for change in '.[1].success=false' '.[0].profile.test=false' '.[0].target.src_path="/other/test.rs"' '.[0].executable="/other/test"' '.[1]=.[0]' '.[0:1]' '[.[0],.[0],.[1]]'; do
  jq "$change" "$TMP/cargo.json" > "$TMP/mutated.json"
  reject 'missing duplicate failed or wrong-source artifact refused' 'selected("/scratch/checkout")' "$TMP/mutated.json"
done
printf 'offline controls: %s passed; 0 failed; no Cargo, network, release executable or systemd execution\n' "$COUNT"
