#!/bin/zsh
# Split ROW kit's prep (copied from the #8156 round kit's prep-local.zsh, t8138 -> t8156), Mac only, touches no lamp.
# Makes the two local channels the lamp's own rounds read, the
# #7922/#7956 method on lamp-75c9 (#7920 16:08): the PR's ARM64 artifact published as `lampos-publish.py` does it
# (`lamp-update manifest --binaries --deploy --skills --models`, then `sign`), but signed with a throwaway key here.
#   ch1, build SEQ1 (stable+1): the PR build, every non-development binary of the artifact, units and skills from the
#        artifact's own TREE (the PR's merge with lamp-voice that CI built), files_url file:///var/lib/lampos-t8156/ch1/.
#   ch2, build SEQ2 (stable+2): stable's signed manifest bytes with only `sequence` changed (a round never takes a
#        lower sequence: the #7123 kit's round 3 was refused "13894 is older than 13895"); files_url stays stable's
#        own GitHub release, so the lamp fetches stable's real files. Back to stable by a real round, no record edit.
# Low sequences on purpose: the lamp ends at SEQ2 and refuses any promote at or below it (updates.md "only advance").
# #8156: LOCAL_ART must be a FULL build of the merged tree (every lamp-* binary stable ships); a lamp-tasks +
# lamp-update only folder is refused by the inventory step below.
# usage: LOCAL_ART=<local build dir> TREE=<built tree> HEAD_SHA=<#8156 head> HOSTU=<host lamp-update> GITDIR=<repo holding TREE>
#        MERGE=<merge commit that was built> EXPECT_STABLE=<seq> [HARNESS_WANT=<ver>] zsh prep-local.zsh
set -eu
K=${0:a:h}; OUT=$K/out; SEC=${PREP_KEY_DIR:-$K/secret}   # a throwaway signing key made here, never committed (.gitignore)
: ${HEAD_SHA:?HEAD_SHA=<#8156 head sha>} ${HOSTU:?HOSTU=<host build of lamp-update at HEAD_SHA, e.g. cargo build -p lamp-update in its own target dir>}
GITDIR=${GITDIR:?GITDIR=<the worktree holding TREE and HEAD_SHA>}
HARNESS_WANT=${HARNESS_WANT:-0.6.75}   # the merge 30f022bf58 (tree 2a89d31e5e) pins Harness 0.6.75 (product.rs VERSION, read 21 Nov)
REPO=autonomous-ai/lampOS
STABLE_URL=https://github.com/$REPO/releases/download/lampos-stable/lampos-manifest.json
FILES_URL=file:///var/lib/lampos-t8156/ch1/
[[ -x $HOSTU ]] || { echo "STOP: $HOSTU not executable"; exit 2; }
# 1-2 (local variant, prep-local.zsh): LOCAL_ART=<arm64-local/out/<name>> is a local Docker ARM64 build of TREE, the
# merge of HEAD_SHA into origin/lamp-voice made by hand (same tree CI builds for a PR into lamp-voice). No gh download,
# no lamp-voice search. Say "local build" in any lamp report (the #8156 ROW says it).
: ${LOCAL_ART:?LOCAL_ART=<dir of the local ARM64 build>} ${TREE:?TREE=<tree sha of the merge that was built>}
RUN_ID=local; RUN_NUMBER=local; BASE=${BASE:-local}
ART=${LOCAL_ART:a}
[[ -d $ART ]] || { echo "STOP: $ART is not a directory"; exit 2; }
(cd $ART && shasum -a 256 -c --quiet SHA256SUMS) || { echo "STOP: local build SHA256SUMS mismatch"; exit 2; }
if [[ -f $ART/TREE ]]; then [[ $(tr -d '\n' < $ART/TREE) == $TREE ]] || { echo "STOP: $ART/TREE differs from TREE=$TREE"; exit 2; }; fi
echo "artifact: local build $ART, tree $TREE"
file $ART/lamp-update | grep -q 'ARM aarch64' || { echo "STOP: artifact lamp-update is not aarch64"; exit 2; }
git -C $GITDIR cat-file -e $TREE || { echo "STOP: tree $TREE not in $GITDIR"; exit 2; }
git -C $GITDIR cat-file -e $HEAD_SHA || { echo "STOP: $HEAD_SHA not fetched in $GITDIR"; exit 2; }
# #8156: the built tree is a merge commit's tree that has HEAD_SHA as a parent (the PR head merged into lamp-voice).
MERGE=${MERGE:?MERGE=<the merge commit that was built>}
[[ $(git -C $GITDIR rev-parse "$MERGE^{tree}") == $TREE ]] || { echo "STOP: $MERGE's tree is not $TREE"; exit 2; }
git -C $GITDIR rev-list --parents -n1 $MERGE | tr ' ' '\n' | tail -n +2 | grep -qx $HEAD_SHA || { echo "STOP: $HEAD_SHA is not a parent of $MERGE"; exit 2; }
echo "merge: ${MERGE[1,10]} = $HEAD_SHA merged (tree ${TREE[1,10]})"
SRC=$K/src/$HEAD_SHA; rm -rf $SRC; mkdir -p $SRC
git -C $GITDIR archive $TREE lampOS/deploy lampOS/skills lampOS/crates/update/src/harness_install/product.rs | tar -x -C $SRC
# 3. The pin this build carries must be the one the ROW claims.
PIN_VER=$(sed -n 's/^pub const VERSION: &str = "\(.*\)";/\1/p' $SRC/lampOS/crates/update/src/harness_install/product.rs)
PIN_SHA=$(awk '/^pub const MANIFEST_SHA256/{getline; gsub(/[ ";]/,""); print; exit}' $SRC/lampOS/crates/update/src/harness_install/product.rs)
[[ $PIN_VER == $HARNESS_WANT ]] || { echo "STOP: the build pins Harness $PIN_VER, not $HARNESS_WANT"; exit 2; }
echo "pin: Harness $PIN_VER, manifest sha $PIN_SHA"
# 4. Stable now. #8156: no stale default: the live feed is read here and reported (15203 when this kit was written,
# 21 Nov ~04:3x); EXPECT_STABLE=<seq> makes a different live value a STOP. A promote run is checked only when PROMOTE_RUN is given.
if [[ -n ${PROMOTE_RUN:-} ]]; then pr=$(gh run view $PROMOTE_RUN --repo $REPO --json status,conclusion -q '.status+" "+.conclusion')
  [[ $pr == "completed success" ]] || { echo "STOP: promote run $PROMOTE_RUN is '$pr'"; exit 2; }; fi
mkdir -p $K/stable; curl -fsSL -o $K/stable/env.json $STABLE_URL
LIVE=$(python3 -I -c 'import base64,json,sys; print(json.loads(base64.b64decode(json.load(open(sys.argv[1]))["manifest"]))["sequence"])' $K/stable/env.json) \
  || { echo "STOP: the stable feed does not decode"; exit 2; }
echo "stable feed (live, $(date '+%F %T')): $LIVE"
EXPECT_STABLE=${EXPECT_STABLE:?EXPECT_STABLE=<stable sequence> (no default)}
[[ $LIVE == $EXPECT_STABLE ]] || { echo "STOP: the stable feed is $LIVE, not EXPECT_STABLE=$EXPECT_STABLE"; exit 2; }
rm -rf $OUT; mkdir -p $OUT/ch1 $OUT/ch2
eval $(python3 -I - $K/stable/env.json $ART $SRC/lampOS/deploy/package-binaries.json $OUT/ch1 <<'EOF'
import base64, hashlib, json, re, shutil, sys, os
env, art, cls, ch1 = sys.argv[1:5]
m = json.loads(base64.b64decode(json.load(open(env))["manifest"]))
f = {x["name"]: x for x in m["files"]}
groups = json.load(open(cls)); member = {n: g for g in ("core", "optional", "provisioning", "development") for n in groups[g]}
names = [n for n in sorted(os.listdir(art)) if re.fullmatch(r"lamp-[a-z0-9-]{1,39}", n)]
assert all(n in member for n in names), "unclassified artifact binary"
kept = [n for n in names if member[n] != "development"]
assert "lamp-update" in kept and "lamp-tasks" in kept
missing = sorted(n for n in f if re.fullmatch(r"lamp-[a-z0-9-]{1,39}", n) and n not in kept and n != "lamp-product-skills")
assert not missing, "not a full build: stable ships %s, the artifact lacks them" % ", ".join(missing)
for n in kept: shutil.copy2(os.path.join(art, n), os.path.join(ch1, n)); os.chmod(os.path.join(ch1, n), 0o755)
sha = lambda p: hashlib.sha256(open(p, "rb").read()).hexdigest()
print("STABLE_SEQ=%d STABLE_COMMIT=%s STABLE_UPD_SHA=%s STABLE_TASKS_SHA=%s PR_UPD_SHA=%s PR_TASKS_SHA=%s NBIN=%d" % (
    m["sequence"], m["commit"], f["lamp-update"]["sha256"], f["lamp-tasks"]["sha256"],
    sha(os.path.join(ch1, "lamp-update")), sha(os.path.join(ch1, "lamp-tasks")), len(kept)))
EOF
)
SEQ1=${SEQ1:-$((STABLE_SEQ + 1))}; SEQ2=${SEQ2:-$((STABLE_SEQ + 2))}
echo "stable $STABLE_SEQ (${STABLE_COMMIT[1,10]}); ch1 = build $SEQ1, ch2 = build $SEQ2; $NBIN binaries"
[[ $PR_UPD_SHA != $STABLE_UPD_SHA ]] || { echo "STOP: the PR's lamp-update equals stable's: no self-switch, no new pin"; exit 2; }
# 5. ch1: the manifest as publish makes it (units, skills bundle and settings written into the folder).
$HOSTU manifest --binaries $OUT/ch1 --deploy $SRC/lampOS/deploy --skills $SRC/lampOS/skills --channel lamp-voice \
  --sequence $SEQ1 --commit $HEAD_SHA --files-url $FILES_URL --models $SRC/lampOS/deploy/product/models.json
# 6. ch2: stable's manifest bytes, sequence only.
python3 -I - $K/stable/env.json $OUT/ch2/manifest.json $STABLE_SEQ $SEQ2 <<'EOF'
import base64, json, sys
env, out, old, new = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
raw = base64.b64decode(json.load(open(env))["manifest"]).decode(); m = json.loads(raw)
a = '"sequence": %d,' % old; assert raw.count(a) == 1, "sequence text not found once"
t = raw.replace(a, '"sequence": %d,' % new); j = json.loads(t)
assert {k for k in m if m[k] != j.get(k)} == {"sequence"}
open(out, "w").write(t)
EOF
# 7. Sign both with a throwaway key (0600, never printed, never on the lamp) and verify as the publisher does.
mkdir -p $SEC; chmod 700 $SEC; umask 077
[[ -s $SEC/test.key ]] || openssl genpkey -algorithm ed25519 -outform DER | base64 > $SEC/test.key
umask 022
$HOSTU public-key --key-file $SEC/test.key > $OUT/test.pub
for c in ch1 ch2; do
  n=$([[ $c == ch1 ]] && echo $SEQ1 || echo $SEQ2)
  $HOSTU sign --key-file $SEC/test.key --manifest $OUT/$c/manifest.json --out $OUT/$c/lampos-manifest.json
  RT=$(mktemp -d); mkdir -p $RT/var/lib/lampos-update
  print -r -- "{\"installed\": {\"sequence\": $n, \"commit\": \"$(printf '0%.0s' {1..40})\"}, \"previous\": null, \"refused\": [], \"made\": []}" > $RT/var/lib/lampos-update/state.json
  ans=$($HOSTU check --channel-url file://$OUT/$c/ --channel lamp-voice --key-file $OUT/test.pub --root $RT --lamp publish-check --apply off 2>/dev/null | tail -1)
  rm -rf $RT; [[ $ans == "up to date: build $n" ]] || { echo "STOP: $c does not verify: $ans"; exit 3; }
  echo "$c verified: $ans"
  rm -f $OUT/$c/manifest.json
done
# The ch1 manifest must name exactly the binaries copied, with their shas (publish's own inventory check).
python3 -I - $OUT/ch1 <<'EOF'
import base64, hashlib, json, os, sys
d = sys.argv[1]; m = json.loads(base64.b64decode(json.load(open(os.path.join(d, "lampos-manifest.json")))["manifest"]))
for e in m["files"] + m["units"]:
    n = e.get("name") or "%s.%s" % (e["lamp"], e["unit"]); p = os.path.join(d, n)
    assert os.path.isfile(p), "ch1 lacks " + n
    assert hashlib.sha256(open(p, "rb").read()).hexdigest() == e["sha256"], "sha differs: " + n
print("ch1 inventory: %d files, %d units, all present with their sha256" % (len(m["files"]), len(m["units"])))
EOF
(cd $OUT && find test.pub ch1 ch2 -type f | sort | xargs shasum -a 256 > SHAS)
cat > $OUT/plan.env <<EOF
HEAD_SHA=$HEAD_SHA
RUN_ID=$RUN_ID
RUN_NUMBER=$RUN_NUMBER
TREE=$TREE
BASE=$BASE
STABLE_SEQ=$STABLE_SEQ
STABLE_COMMIT=$STABLE_COMMIT
STABLE_UPD_SHA=$STABLE_UPD_SHA
STABLE_TASKS_SHA=$STABLE_TASKS_SHA
PR_UPD_SHA=$PR_UPD_SHA
PR_TASKS_SHA=$PR_TASKS_SHA
SEQ1=$SEQ1
SEQ2=$SEQ2
HARNESS_WANT=$HARNESS_WANT
PIN_SHA=$PIN_SHA
EOF
echo "test.pub $(shasum -a 256 $OUT/test.pub | cut -c1-12); ch1 envelope $(shasum -a 256 $OUT/ch1/lampos-manifest.json | cut -c1-12); ch2 envelope $(shasum -a 256 $OUT/ch2/lampos-manifest.json | cut -c1-12); $(du -sh $OUT | cut -f1)"
echo "PREP OK"
