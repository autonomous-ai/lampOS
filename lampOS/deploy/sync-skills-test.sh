#!/usr/bin/env bash
# Tests for sync-skills.sh with no lamp: folders standing for /var/lib/lampOS/skills and
# lamp-skills' state, and stand-ins for ssh, sshpass and sudo for --lamp. Runs on bash 3.2.
# Run: lampOS/deploy/sync-skills-test.sh
set -uo pipefail

SYNC=$(cd "$(dirname "$0")" && pwd)/sync-skills.sh
REPO=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
failures=0
check() {
  if eval "$2"; then echo "ok: $1"; else echo "FAIL: $1"; failures=$((failures + 1)); fi
}
mode() { stat -c %a "$1" 2>/dev/null || stat -f %Lp "$1"; }
D=$T/skills
S=$T/state
mkdir -p "$S"
sync() { "$SYNC" --dir "$D" --state-dir "$S" "$@" 2>&1; }
OFF_LINE=$'# Installed off until its lamp check (lamp-update\'s install::PRODUCT_SKILLS_OFF).\nenabled = false'

# A blank lamp, dry run: everything to add, nothing written.
out=$(sync)
check "a dry run lists startle and reminders to add" '[[ $out == *"add: startle/skill.toml"* && $out == *"add: reminders/skill.toml"* ]]'
check "a dry run writes nothing" '[[ ! -e $D ]]'
check "a dry run says so" '[[ $out == *"dry run: nothing written"* ]]'

# Applied: every product skill's files, no tests, the off line on off skills, 0644 files.
out=$(sync --apply)
check "apply writes the product skills" '[[ -f $D/startle/skill.toml && -f $D/reminders/SKILL.md && -f $D/welcome-back/lights/warm_glow.toml ]]'
check "a written file is the repo's own" 'cmp -s "$D/reminders/skill.toml" "$REPO/lampOS/skills/reminders/skill.toml"'
check "no tests folder is written" '[[ -z $(find "$D" -path "*/tests*") ]]'
check "a skill not in the product is not written" '[[ ! -e $D/desk-tutor && ! -e $D/guard-mode ]]'
check "an off skill gets the off line above its skill.toml" '[[ $(head -n 2 "$D/light-out/skill.toml") == "$OFF_LINE" ]]'
check "an off skill is otherwise its own file" 'cmp -s <(tail -n +3 "$D/light-out/skill.toml") "$REPO/lampOS/skills/light-out/skill.toml"'
check "bedtime-morning is written off by its own file" 'cmp -s "$D/bedtime-morning/skill.toml" "$REPO/lampOS/skills/bedtime-morning/skill.toml"'
check "files are 0644 and folders 0755" '[[ $(mode "$D/startle/skill.toml") == 644 && $(mode "$D/startle") == 755 ]]'
check "nothing is left unpacked beside the folder" '[[ -z $(find "$T" -maxdepth 1 -name ".sync-skills.*") ]]'

# Again: nothing to do.
out=$(sync)
check "a second run finds nothing to add or change" '[[ $out == *"0 to add, 0 to change"* ]]'

# A changed file is put back; the person's own skill and a stray file are never touched.
echo "changed here" >>"$D/reminders/SKILL.md"
mkdir -p "$D/my-own-skill" && echo mine >"$D/my-own-skill/skill.toml"
echo mine >"$D/startle/notes.md"
out=$(sync)
check "a changed file is listed as a change" '[[ $out == *"change: reminders/SKILL.md"* ]]'
check "a stray file in a product folder is listed as left" '[[ $out == *"left: startle/notes.md"* ]]'
check "a skill the person installed is not listed" '[[ $out != *my-own-skill* ]]'
sync --apply >/dev/null
check "apply puts the changed file back" 'cmp -s "$D/reminders/SKILL.md" "$REPO/lampOS/skills/reminders/SKILL.md"'
check "apply never deletes a stray file" '[[ $(cat "$D/startle/notes.md") == mine ]]'
check "apply never touches a skill the person installed" '[[ $(cat "$D/my-own-skill/skill.toml") == mine ]]'

# An off skill (the off line) turned on on the lamp stays on.
tail -n +3 "$D/music-sway/skill.toml" >"$T/on" && cp "$T/on" "$D/music-sway/skill.toml"
out=$(sync --apply)
check "an off skill turned on is said kept on" '[[ $out == *"kept on: music-sway"* ]]'
check "an off skill turned on is written without the off line" 'cmp -s "$D/music-sway/skill.toml" "$REPO/lampOS/skills/music-sway/skill.toml"'

# Bedtime and Morning, off by its own enabled = false (Jarvis's P2): kept on when `on` names it,
# or when the lamp's file turns it on; off again when the lamp turned it off again.
echo bedtime-morning >"$S/on"
out=$(sync --apply)
check "a skill named in on is said kept on" '[[ $out == *"kept on: bedtime-morning (named in"* ]]'
check "a skill named in on is written turned on" 'grep -q "^enabled = true" "$D/bedtime-morning/skill.toml"'
check "turned on differs from the build's file only in enabled" 'diff <(grep -v "^enabled" "$D/bedtime-morning/skill.toml") <(grep -v "^enabled" "$REPO/lampOS/skills/bedtime-morning/skill.toml") >/dev/null'
rm "$S/on"
out=$(sync)
check "a skill whose lamp file turns it on stays on without on" '[[ $out == *"kept on: bedtime-morning (its skill.toml"* && $out == *"0 to change"* ]]'
sed "s/^enabled = true/enabled = false/" "$D/bedtime-morning/skill.toml" >"$T/off" && cp "$T/off" "$D/bedtime-morning/skill.toml"
out=$(sync)
check "a skill the lamp turned off again stays off" '[[ $out != *"kept on: bedtime-morning"* && $out == *"0 to change"* ]]'
# Reviewer-two's P2: with `on` still naming it, a file turned off again (and not the build's) wins.
echo bedtime-morning >"$S/on"
{ echo "# turned off again here"; cat "$REPO/lampOS/skills/bedtime-morning/skill.toml"; } >"$T/off"
cp "$T/off" "$D/bedtime-morning/skill.toml"
out=$(sync --apply)
check "a file turned off again wins over on, and is said" '[[ $out == *"left off: bedtime-morning"* && $out != *"kept on: bedtime-morning"* ]]'
check "a file turned off again is left as it is" 'cmp -s "$D/bedtime-morning/skill.toml" "$T/off"'
check "a file turned off again is not listed as a stray" '[[ $out != *"left: bedtime-morning/skill.toml"* ]]'
# With `on` naming it and the lamp's file the build's own, or missing, it is kept on.
cp "$REPO/lampOS/skills/bedtime-morning/skill.toml" "$D/bedtime-morning/skill.toml"
out=$(sync --apply)
check "on with the build's own file keeps it on" '[[ $out == *"kept on: bedtime-morning (named in"* ]] && grep -q "^enabled = true" "$D/bedtime-morning/skill.toml"'
rm "$D/bedtime-morning/skill.toml"
out=$(sync --apply)
check "on with the file missing keeps it on" '[[ $out == *"kept on: bedtime-morning (named in"* ]] && grep -q "^enabled = true" "$D/bedtime-morning/skill.toml"'
rm "$S/on"

# An older build: --rev reads that build's lists (startle is not a product skill before 06478cdc2).
out=$("$SYNC" --rev 06478cdc2^ --dir "$T/older" --state-dir "$S" 2>&1)
check "--rev takes that build's PRODUCT_SKILLS" '[[ $out != *"startle/"* && $out == *"add: reminders/skill.toml"* ]]'
"$SYNC" --rev no-such-rev --dir "$T/older" --state-dir "$S" >/dev/null 2>&1; code=$?
check "an unknown --rev fails and writes nothing" '[[ $code -ne 0 && ! -e $T/older ]]'

# --lamp: over stand-in ssh and sudo, password from LAMP_SUDO on sudo's stdin only.
mkdir -p "$T/bin" "$T/tmp"
printf '%s\n' '#!/usr/bin/env bash' 'while [[ $1 == -o ]]; do shift 2; done' \
  'echo "ssh $1" >>"$FAKE_LOG"; shift' 'exec bash -c "$*"' >"$T/bin/ssh"
printf '%s\n' '#!/usr/bin/env bash' \
  'if [[ $1 == -S ]]; then read -r password; echo "sudo password: $password" >>"$FAKE_LOG"; shift 3' \
  'elif [[ $1 == -n ]]; then echo "sudo -n" >>"$FAKE_LOG"; shift; fi' 'exec "$@"' >"$T/bin/sudo"
printf '%s\n' '#!/usr/bin/env bash' 'echo "sshpass $1" >>"$FAKE_LOG"; shift' 'exec "$@"' >"$T/bin/sshpass"
chmod +x "$T/bin/ssh" "$T/bin/sudo" "$T/bin/sshpass"
L=$T/lamp-skills
export FAKE_LOG=$T/log
: >"$FAKE_LOG"
out=$(PATH=$T/bin:$PATH "$SYNC" --lamp pi@lamp --dir relative 2>&1); code=$?
check "a relative --dir is refused" '[[ $code -eq 2 ]]'
out=$(PATH=$T/bin:$PATH TMPDIR=$T/tmp LAMP_SUDO=pw "$SYNC" --apply --lamp pi@lamp --dir "$L" --state-dir "$S" 2>&1 </dev/null); code=$?
check "--lamp --apply writes the lamp's folder" '[[ $code -eq 0 && -f $L/startle/skill.toml ]]'
check "--lamp says where it wrote" '[[ $out == *"in $L on pi@lamp"* ]]'
check "--lamp --apply leaves nothing in either temp folder" '[[ -z $(ls -A "$T/tmp") ]]'
check "--lamp goes over ssh to the target" 'grep -q "^ssh pi@lamp" "$FAKE_LOG"'
check "LAMP_SUDO reaches sudo on its stdin" 'grep -q "^sudo password: pw" "$FAKE_LOG"'
check "the password is not in any command line" '! grep -E "^ssh .*pw" "$FAKE_LOG" >/dev/null'

# A refused sudo (Fourteen's P3) writes nothing and leaves no copy on the lamp.
printf '%s\n' '#!/usr/bin/env bash' 'echo "sudo refused" >>"$FAKE_LOG"; exit 1' >"$T/bin/sudo"
rm -rf "$L"
out=$(PATH=$T/bin:$PATH TMPDIR=$T/tmp "$SYNC" --apply --lamp pi@lamp --dir "$L" --state-dir "$S" 2>&1 </dev/null); code=$?
check "a refused sudo fails and writes nothing" '[[ $code -ne 0 && ! -e $L/startle ]]'
check "a refused sudo leaves no copy behind" '[[ -z $(ls -A "$T/tmp") ]]'

# LAMP_SSH replaces the ssh command (Jarvis's P3: sshpass for a lamp reached by password).
out=$(PATH=$T/bin:$PATH LAMP_SSH="sshpass -e ssh" "$SYNC" --lamp pi@lamp --dir "$L" --state-dir "$S" 2>&1 </dev/null)
check "LAMP_SSH replaces the ssh command" 'grep -q "^sshpass -e" "$FAKE_LOG"'

echo
if [[ $failures -eq 0 ]]; then echo "all passed"; else echo "$failures failed"; exit 1; fi
