#!/bin/sh
# In the rig: `sh mkchan.sh make` writes build B's manifest with B's own lamp-update, as CI's
# publisher does (lampos-publish.py prepare: manifest --binaries --deploy --skills --models);
# `sh mkchan.sh check` reads both signed channels back. Signing itself is run from the Mac
# (run.sh sign), the test key piped on stdin, never written here.
set -u
. /var/tmp/el-8400/lamp/common.sh
rig_only
A=$K/chanA; B=$K/chanB
case "${1:-}" in
make)
  [ -x "$B/lamp-update" ] || die "no $B/lamp-update"
  rm -f "$B/manifest.json" "$B/lampos-manifest.json"
  models=""
  [ "${MODELS:-1}" = 1 ] && models="--models $K/srcB/lampOS/deploy/product/models.json"
  # shellcheck disable=SC2086
  "$B/lamp-update" manifest --binaries "$B" --deploy "$K/srcB/lampOS/deploy" --skills "$K/srcB/lampOS/skills" \
    --channel lamp-voice --sequence "$B_SEQ" --commit "$B_SHA" --files-url "file://$B/" $models \
    || die "lamp-update manifest failed"
  python3 -I - "$A/manifest.json" "$B/manifest.json" "$A_UPDATER" "$B_SEQ" "$A_SEQ" <<'EOF' || exit 1
import json, sys
a = json.load(open(sys.argv[1])); b = json.load(open(sys.argv[2]))
level, bseq, aseq = int(sys.argv[3]), int(sys.argv[4]), int(sys.argv[5])
def product(m): return {u["unit"] for u in m["units"] if u["lamp"] == "product"}
adds = sorted(product(b) - product(a))
print("el-8400: B manifest: sequence %s, min_updater %s, %d files, %d units, models %d" % (
    b["sequence"], b.get("min_updater"), len(b["files"]), len(b["units"]), len(b.get("models", []))))
print("el-8400: product units in B's manifest that A's lacks: " + ", ".join(adds))
if b["sequence"] != bseq or a["sequence"] != aseq or bseq <= aseq:
    sys.exit("el-8400: STOP: sequences A %s B %s are not A < B as staged" % (a["sequence"], b["sequence"]))
if (b.get("min_updater") or 1) > level:
    sys.exit("el-8400: STOP: B asks updater %s, above A's %s: A would climb and B's updater switch with its own PRODUCT; #7018's path is not run" % (b.get("min_updater"), level))
if "lamp-npu.service" not in adds:
    sys.exit("el-8400: STOP: lamp-npu.service is not a unit B adds over A")
EOF
  ;;
check)
  for c in A B; do
    d=$K/chan$c
    python3 -I - "$d/lampos-manifest.json" "$d/manifest.json" "$TEST_FP" "file://$d/" <<'EOF' || exit 1
import base64, json, sys
env = json.load(open(sys.argv[1])); raw = open(sys.argv[2], "rb").read()
ok = (env["format"] == "lampos-update/1" and env["key"] == sys.argv[3]
      and base64.b64decode(env["manifest"]) == raw and json.loads(raw)["files_url"] == sys.argv[4])
print("el-8400: %s: key %s, files_url %s: %s" % (sys.argv[1], env["key"], json.loads(raw)["files_url"], "OK" if ok else "BAD"))
sys.exit(0 if ok else 1)
EOF
  done
  [ "$(fp "$K/test.pub")" = "$TEST_FP" ] || die "test.pub's fingerprint is not $TEST_FP"
  [ "$TEST_FP" != "$PROD_FP" ] || die "the test key is the production key"
  say "CHANNELS-OK: A $A_SEQ and B $B_SEQ, both signed with the test key $TEST_FP"
  ;;
*) echo "usage: sh mkchan.sh make|check"; exit 2;;
esac
