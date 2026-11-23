# lamp, root sh (common first; then LEG=). READ ONLY: the live mute must be off before a clip plays. Prints MUTE OFF OK |
# MUTE FAIL <why>.
cd /
L=$(mute_live); echo "$L"
if echo "$L" | grep -q 'physical_muted=True'; then echo "MUTE FAIL: the physical slide is closed"
elif echo "$L" | grep -qE 'VOICE_MUTE muted=False .*age_s=[0-4]\.'; then echo "MUTE OFF OK"
else echo "MUTE FAIL: voice/mute is not a fresh unmuted value"; fi
