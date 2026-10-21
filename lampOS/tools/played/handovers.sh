#!/bin/zsh
# #854/#903: in ONE session, stories asked of the lamp must be told by the lamp, and work asked of one of the staff
# must be handed over (offered, then declined with "No, not now." so nothing starts). Written 2026-10-16 02:55 after
# #903 told every story but refused both named hand-overs ("I can't write code or use your office computer").
# PLAYS SOUND IN THE ROOM. About 4 minutes. argv: LABEL. Prints each line and what the lamp said, then a tally:
# told = a story/poem said by the lamp; offered/handed = it named Claude Code or Codex and offered to start or
# hand it over ("I can't … ask Claude Code directly" is not a hand-over). Read the lines too: the tally is a guide.
cd "${0:A:h}" || exit 9
. ./settings.sh
L=$1
./scenario.py handovers-$L \
  "Daniel|Hey Lamp, tell me a story about a clever fox, at least eight sentences long.|40" \
  "Samantha|Hey Lamp, ask Claude Code to write a two-line poem about the rain.|9" \
  "Samantha|No, not now.|6" \
  "Daniel|Hey Lamp, tell me a story about a sleepy dragon, at least five sentences long.|24" \
  "Samantha|Hey Lamp, can you ask Claude Code to write a short poem about the sea?|9" \
  "Samantha|No, not now.|6" \
  "Karen|Hey Lamp, tell me a long story about a lighthouse keeper.|48" \
  "Samantha|Hey Lamp, ask Codex to write a haiku about coffee.|9" \
  "Samantha|No thanks, not now.|6" \
  "Samantha|Hey Lamp, write me a poem about the moon.|14" > /dev/null 2>&1
f=$(ls -t "$PLAYED_OUT"/scenarios/*handovers-$L*.txt | head -1)
/usr/bin/awk '
  / PLAY /{ if (line != "") print line " -> " said; line = substr($0, index($0, "PLAY") + 6, 80); said = ""; next }
  / LAMP /{ s = $0; sub(/.*LAMP +/, "", s); said = said " " s; next }
  END { print line " -> " said }' $f | cut -c1-230 | tee "$PLAYED_OUT"/scenarios/handovers-$L.tally.txt
/usr/bin/awk -F' -> ' '
  function offer(s) { return s ~ /(Claude|Codex)/ && s ~ /([Ss]tart|[Hh]and|[Pp]ass|[Gg]iv(e|ing)|get started|[Ss]hall I|[Ss]hould I|I.ll ask)/ && s !~ /can.t/ }
  /tell me a .*story|write me a poem/ { asked_lamp++; if (offer($2)) offered_own++; else if (length($2) > 40) told++ }
  /ask Claude Code|ask Codex|have Codex/ { asked_staff++; if (offer($2)) handed++ }
  END { printf "asked of the lamp %d: told %d, offered to the staff %d | asked of the staff %d: handed over %d\n", asked_lamp, told, offered_own, asked_staff, handed }' "$PLAYED_OUT"/scenarios/handovers-$L.tally.txt
