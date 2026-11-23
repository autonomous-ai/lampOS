#!/bin/sh
# Mac only, no lamp, no cargo: sh stage.sh <#8400 head sha>
# Stages the two builds and the test key in stage/:
#   B  = #8400's head: its CI ARM64 artifact lampos-arm64-<sha> (gh run download), checked
#        against the artifact's SHA256SUMS, the non-development binaries kept as CI's publisher
#        keeps them (deploy/package-binaries.json at <sha>), and deploy/ + skills/ at <sha>
#        (git archive) for its manifest. Sequence = that ARM64 run's number, as the runbook.
#   A  = build 14976 (969ff46eb2, 20 Nov): its published release lampos-lamp-voice-14976 (its CI
#        artifact has expired; the release holds the same retained binaries, units and manifest).
#        Its production-signed envelope is checked with the production public key, every asset
#        against the manifest, then files_url is pointed at the rig's folder (re-signed later).
#   key = a fresh Ed25519 test key (openssl), private half kept in stage/key/ on this Mac only.
# Refuses unless <sha> is the PR's head now and its ARM64 release job is green, and unless A and
# B are the pair this row needs (A's updater level >= what B asks; A's PRODUCT lacks
# lamp-npu.service and B's has it). Writes stage/STAGE.env. Never reaches a lamp or the rig.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=autonomous-ai/lampOS; PR=8400
A_SEQ=14976; A_COMMIT=969ff46eb28c80b934ec9cc8b512e7a1bc0d4465
PROD_PUB='37n0Gidw/NXHyNa+PYB5yjGE3UaDR4zFWogoiE97rJU='   # LAMPOS_UPDATE_PUBLIC_KEY, fingerprint 54213f8d3aa2f241
PROD_FP=54213f8d3aa2f241
SRC=${SRC:?set SRC to a lampOS clone with both commits (14976 and the PR head)}      # any clone with both commits; read with git show/archive only
K=/var/tmp/el-8400                                        # the kit's folder in the rig
SHA=${1:-}
case "$SHA" in *[!0-9a-f]*|"") echo "usage: sh stage.sh <40-hex head sha>"; exit 2;; esac
[ ${#SHA} = 40 ] || { echo "usage: sh stage.sh <40-hex head sha>"; exit 2; }
die() { echo "stage: REFUSE: $*"; exit 1; }
ST=$HERE/stage

head=$(gh pr view $PR --repo $REPO --json headRefOid --jq .headRefOid) || exit 1
[ "$head" = "$SHA" ] || die "the PR's head is $head, not $SHA (read the new head, then sh stage.sh $head)"
git -C "$SRC" cat-file -e "$SHA^{commit}" 2>/dev/null || die "$SRC lacks $SHA (git -C $SRC fetch origin)"
git -C "$SRC" cat-file -e "$A_COMMIT^{commit}" 2>/dev/null || die "$SRC lacks $A_COMMIT"

# The pair, from the code at each commit.
prod() { git -C "$SRC" show "$1:lampOS/crates/update/src/install.rs" | awk '/^pub const PRODUCT: /{f=1} f&&/^\];/{exit} f' \
  | grep -oE 'deploy/product/[^"]+' | sed 's#deploy/product/##' | sort -u; }
level() { git -C "$SRC" show "$1:lampOS/crates/update/src/manifest.rs" | sed -n 's/^pub const UPDATER: u64 = \([0-9]*\);$/\1/p'; }
A_UPDATER=$(level $A_COMMIT); B_UPDATER=$(level "$SHA")
[ -n "$A_UPDATER" ] && [ -n "$B_UPDATER" ] || die "cannot read UPDATER at the two commits"
[ "$B_UPDATER" -le "$A_UPDATER" ] || die "B's updater is $B_UPDATER, above A's $A_UPDATER: A would climb, not switch (#7018's path not run)"
prod $A_COMMIT > "$HERE/.prodA"; prod "$SHA" > "$HERE/.prodB"
ADDS=$(comm -13 "$HERE/.prodA" "$HERE/.prodB" | tr '\n' ' '); rm -f "$HERE/.prodA" "$HERE/.prodB"
echo "updater levels: A $A_UPDATER, B $B_UPDATER; units in B's install::PRODUCT and not A's: $ADDS"
case " $ADDS " in *" lamp-npu.service "*) ;; *) die "lamp-npu.service is not in B's PRODUCT and absent from A's";; esac
git -C "$SRC" show "$SHA:lampOS/crates/update/src/round.rs" | grep -q 'which the lamp runs, adds {}: the updater that switched to it did not carry them (#7018)' \
  || die "B's round.rs lacks #7018's line"

# B: the artifact.
art=$(gh api "repos/$REPO/actions/artifacts?name=lampos-arm64-$SHA&per_page=5" \
  --jq '.artifacts | map(select(.expired|not)) | sort_by(.created_at) | last | "\(.id) \(.workflow_run.id)"')
case "$art" in ""|"null null") die "no unexpired artifact lampos-arm64-$SHA (CI still running?)";; esac
run=${art#* }
B_SEQ=$(gh run view "$run" --repo $REPO --json number --jq .number) || exit 1
gh run view "$run" --repo $REPO --json jobs --jq '.jobs[] | select(.name|test("ARM64")) | "  \(.name): \(.status) \(.conclusion)"' > "$HERE/jobs.txt"
cat "$HERE/jobs.txt"
grep -qE 'build and test on Linux ARM64 \(release\): completed success' "$HERE/jobs.txt" || die "the ARM64 release job is not green"
[ "$B_SEQ" -gt "$A_SEQ" ] || die "run number $B_SEQ is not above $A_SEQ"
echo "B: artifact ${art% *}, run $run, sequence (run number) $B_SEQ"

rm -rf "$ST"; mkdir -p "$ST/art" "$ST/chanA" "$ST/chanB" "$ST/srcB" "$ST/key"
gh run download "$run" --repo $REPO -n "lampos-arm64-$SHA" -D "$ST/art" || exit 1
sums=$(find "$ST/art" -type f -name SHA256SUMS | head -1)
[ -n "$sums" ] || die "no SHA256SUMS in the artifact"
(cd "$(dirname "$sums")" && shasum -a 256 -c SHA256SUMS > "$ST/art-check.txt" 2>&1) || { tail "$ST/art-check.txt"; die "the artifact does not match its SHA256SUMS"; }
git -C "$SRC" show "$SHA:lampOS/deploy/package-binaries.json" > "$ST/package-binaries.json"
python3 -I - "$(dirname "$sums")" "$ST/chanB" "$ST/package-binaries.json" <<'EOF' || exit 1
import hashlib, json, os, re, shutil, sys
src, out, cls = sys.argv[1], sys.argv[2], json.load(open(sys.argv[3]))
member = {n: g for g in ("core", "optional", "provisioning", "development") for n in cls[g]}
names = sorted(n for n in os.listdir(src) if re.fullmatch(r"lamp-[a-z0-9-]{1,39}", n))
loose = [n for n in names if n not in member]
if loose: sys.exit("stage: REFUSE: unclassified binaries " + " ".join(loose))
kept = [n for n in names if member[n] != "development"]
for need in ("lamp-update", "lamp-npu", "lamp-apps-manage"):
    if need not in kept: sys.exit("stage: REFUSE: the artifact lacks " + need)
lines = []
for n in kept:
    shutil.copyfile(os.path.join(src, n), os.path.join(out, n)); os.chmod(os.path.join(out, n), 0o755)
    lines.append("%s  %s\n" % (hashlib.sha256(open(os.path.join(out, n), "rb").read()).hexdigest(), n))
open(os.path.join(out, "SHA256SUMS"), "w").write("".join(lines))
print("B: %d binaries kept (non-development), %d left out" % (len(kept), len(names) - len(kept)))
EOF
file "$ST/chanB/lamp-update" | grep -q 'ARM aarch64' || die "B's lamp-update is not aarch64"
strings -a "$ST/chanB/lamp-update" | grep -q 'did not carry them (#7018)' || die "B's lamp-update lacks #7018's line"
git -C "$SRC" archive "$SHA" lampOS/deploy lampOS/skills | tar -x -C "$ST/srcB" || die "git archive of B's deploy/skills failed"
rm -rf "$ST/art"

# A: the release, checked with the production key, then pointed at the rig's folder.
gh release download "lampos-lamp-voice-$A_SEQ" --repo $REPO -D "$ST/chanA" || die "gh release download of $A_SEQ failed"
printf '%s\n' "$PROD_PUB" > "$ST/prod.pub"
python3 -I - "$ST/chanA" "$PROD_PUB" "$PROD_FP" "$A_SEQ" "$A_COMMIT" "file://$K/chanA/" <<'EOF' || exit 1
import base64, hashlib, json, os, re, subprocess, sys, tempfile
d, pub, pfp, seq, commit, url = sys.argv[1:]
raw = base64.b64decode(pub)
if hashlib.sha256(raw).hexdigest()[:16] != pfp: sys.exit("stage: REFUSE: the production key's fingerprint is not " + pfp)
env = json.load(open(os.path.join(d, "lampos-manifest.json")))
man = base64.b64decode(env["manifest"]); sig = base64.b64decode(env["signature"])
if env["key"] != pfp: sys.exit("stage: REFUSE: A's release is signed with " + env["key"])
with tempfile.TemporaryDirectory() as t:
    pem = os.path.join(t, "p.pem"); open(pem, "w").write("-----BEGIN PUBLIC KEY-----\n%s\n-----END PUBLIC KEY-----\n" % base64.b64encode(bytes.fromhex("302a300506032b6570032100") + raw).decode())
    open(os.path.join(t, "m"), "wb").write(man); open(os.path.join(t, "s"), "wb").write(sig)
    ok = subprocess.run(["openssl", "pkeyutl", "-verify", "-pubin", "-inkey", pem, "-rawin", "-in", os.path.join(t, "m"), "-sigfile", os.path.join(t, "s")], capture_output=True)
    if ok.returncode != 0: sys.exit("stage: REFUSE: A's envelope does not verify with the production key: " + ok.stdout.decode() + ok.stderr.decode())
if open(os.path.join(d, "manifest.json"), "rb").read() != man: sys.exit("stage: REFUSE: A's manifest.json is not the signed one")
m = json.loads(man)
if m["sequence"] != int(seq) or m["commit"] != commit: sys.exit("stage: REFUSE: A's manifest is %s %s" % (m["sequence"], m["commit"]))
for e in m["files"] + m["units"]:
    name = e["name"] if "name" in e else "%s.%s" % (e["lamp"], e["unit"])
    b = open(os.path.join(d, name), "rb").read()
    if len(b) != e["size"] or hashlib.sha256(b).hexdigest() != e["sha256"]: sys.exit("stage: REFUSE: A's " + name + " differs from its manifest")
for e in m["files"]:
    os.chmod(os.path.join(d, e["name"]), 0o755)
old = '"files_url": %s' % json.dumps(m["files_url"])
if man.decode().count(old) != 1: sys.exit("stage: REFUSE: A's files_url is not found once")
new = man.decode().replace(old, '"files_url": %s' % json.dumps(url)).encode()
os.rename(os.path.join(d, "lampos-manifest.json"), os.path.join(d, "lampos-manifest.production.json"))
open(os.path.join(d, "manifest.json"), "wb").write(new)
print("A: release %s (%s) verified with key %s; %d files, %d units; files_url now %s" % (seq, commit[:10], pfp, len(m["files"]), len(m["units"]), url))
EOF
grep -q 'lamp-npu' "$ST/chanA/manifest.json" && die "A's manifest names lamp-npu"

# The test key: never the production one, never on the rig in its private half.
(umask 077; openssl genpkey -algorithm ed25519 -outform DER | base64 > "$ST/key/test.key") || die "openssl genpkey failed"
base64 -d -i "$ST/key/test.key" | openssl pkey -inform DER -pubout -outform DER | tail -c 32 | base64 > "$ST/test.pub"
TEST_FP=$(python3 -I -c "import base64,hashlib,sys;print(hashlib.sha256(base64.b64decode(open(sys.argv[1]).read().strip())).hexdigest()[:16])" "$ST/test.pub")
[ ${#TEST_FP} = 16 ] && [ "$TEST_FP" != "$PROD_FP" ] || die "test key fingerprint $TEST_FP"

cat > "$ST/STAGE.env" <<EOF
B_SHA=$SHA
B_SEQ=$B_SEQ
B_RUN=$run
B_UPDATER=$B_UPDATER
A_SEQ=$A_SEQ
A_COMMIT=$A_COMMIT
A_UPDATER=$A_UPDATER
TEST_FP=$TEST_FP
ADDS="$ADDS"
EOF
n=$(find "$ST" -name '._*' | wc -l | tr -d ' ')
echo "staged: A $A_SEQ (updater $A_UPDATER), B $B_SEQ = #$PR at $(echo "$SHA" | cut -c1-10) (updater $B_UPDATER), test key $TEST_FP; ._ files: $n; $(du -sh "$ST" | cut -f1)"
echo "next: sh run.sh go   (or the steps one by one: up push make sign install snap leg1 leg2 judge putback)"
