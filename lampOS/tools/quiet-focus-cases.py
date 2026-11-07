"""Writes Quiet focus's test cases in the format `lamp-bus record` writes
(see skills/quiet-focus/tests/README.md): not recorded on a lamp.

Usage, from lampOS/: python3 -I tools/quiet-focus-cases.py skills/quiet-focus/tests
"""
import base64
import json
import os
import sys

BASE_UNIX_MS = 1792368000000
SAY = (
    "They have just looked up after being quietly focused on their work for a good while. "
    "In one short, warm line of a few words, the way a friend would, say you noticed and it's "
    "nicely done. Only that: ask nothing of them and suggest nothing to do, and say nothing "
    "about their body or looks, the time, or health."
)
SITTER = {"confidence": 0.9, "id": "a1", "xmax": 0.6, "xmin": 0.3, "ymax": 0.6, "ymin": 0.2}
OTHER = {"confidence": 0.9, "id": "b1", "xmax": 0.2, "xmin": 0.05, "ymax": 0.5, "ymin": 0.3}


def payload(value):
    text = json.dumps(value, separators=(",", ":"), sort_keys=True)
    return base64.b64encode(text.encode()).decode()


def line(topic, typ, source, boot, seq, at_us, value, expires_ms=None):
    header = {
        "topic": topic,
        "kind": "latest",
        "enc": "json",
        "type": typ,
        "source": source,
        "boot": boot,
        "seq": seq,
        "at_us": at_us,
        "at_unix_ms": BASE_UNIX_MS + at_us // 1000,
    }
    if expires_ms is not None:
        header["expires_ms"] = expires_ms
    return json.dumps({"header": header, "payload_b64": payload(value)}, separators=(",", ":"))


def state_at(spans, second):
    for start, end, state in spans:
        if start * 60 <= second < end * 60:
            return state
    return None


def write_case(root, name, comment, clock, spans, end_min, expects=(), others=(),
               dense=(), voice=(("resting", 0),), call_tool=None):
    """`spans`: (from_min, to_min, state), state at_desk, head_down or away.
    `others`: (from_min, to_min) a second face in view. `dense`: (from_min,
    to_min) faces every 1.5 s. `voice`: (state, at_min)."""
    folder = os.path.join(root, name)
    os.makedirs(folder, exist_ok=True)
    end_s = int(end_min * 60)

    # room/activity: every 10 s while someone sits, every 30 s while away.
    lines = [json.dumps({"topic": "room/activity", "declaration": {
        "type": "lamp_msgs/RoomActivity", "version": 1, "kind": "latest",
        "period_ms": 1000, "lifespan_ms": 3000}})]
    seq = 0
    sitting_from = None
    state_from = None
    last_state = None
    second = 0
    while second <= end_s:
        state = state_at(spans, second) or "away"
        if state != last_state:
            state_from = second
            if state == "away":
                sitting_from = None
            elif sitting_from is None:
                sitting_from = second
            last_state = state
        sitting = 0 if state == "away" else second - sitting_from
        value = {
            "baseline": state != "away" and sitting >= 120,
            "for_s": second - state_from,
            "sitting_s": sitting,
            "state": state,
        }
        seq += 1
        lines.append(line("room/activity", "lamp_msgs/RoomActivity", "eyes",
                          "5d0c2a7e91b34f86", seq, second * 1_000_000, value, 3000))
        second += 30 if state == "away" else 10
    with open(os.path.join(folder, "room_activity.jsonl"), "w") as out:
        out.write("\n".join(lines) + "\n")

    # humans/faces/tracked: every 30 s, every 1.5 s inside `dense`.
    lines = [json.dumps({"topic": "humans/faces/tracked", "declaration": {
        "type": "lamp_msgs/Faces", "version": 1, "kind": "latest"}})]
    seq = 0
    tenth = 0  # in tenths of a second
    while tenth <= end_s * 10:
        second = tenth / 10
        state = state_at(spans, second) or "away"
        faces = [] if state == "away" else [SITTER]
        if state != "away" and any(a * 60 <= second < b * 60 for a, b in others):
            faces = [SITTER, OTHER]
        seq += 1
        lines.append(line("humans/faces/tracked", "lamp_msgs/Faces", "lamp_bus_cli",
                          "5f1e2d3c4b5a6978", seq, tenth * 100_000, {"faces": faces}))
        inside = any(a * 600 <= tenth < b * 600 for a, b in dense)
        tenth += 15 if inside else 300
    with open(os.path.join(folder, "humans_faces_tracked.jsonl"), "w") as out:
        out.write("\n".join(lines) + "\n")

    lines = [json.dumps({"topic": "voice/state", "declaration": {
        "type": "lamp_msgs/VoiceState", "version": 1, "kind": "latest"}})]
    for seq, (state, at_min) in enumerate(voice, start=1):
        lines.append(line("voice/state", "lamp_msgs/VoiceState", "lamp_bus_cli",
                          "6a7b8c9d0e1f2a3b", seq, int(at_min * 60) * 1_000_000,
                          {"state": state}))
    with open(os.path.join(folder, "voice_state.jsonl"), "w") as out:
        out.write("\n".join(lines) + "\n")

    toml = [f"# {comment}", f'clock = "{clock}"']
    if not expects:
        toml.append("none = true")
    if call_tool:
        toml += ["", "[[call_tool]]", 'name = "wellbeing_nudges"', f'at = "{call_tool}"',
                 'args = { how = "off" }']
    for at in expects:
        toml += ["", "[[expect]]", f'at = "{at}m"', 'within = "1m"', 'call = "body.gaze"',
                 'args = { look_at = "face" }', "", "[[expect]]", f'at = "{at}m"',
                 'within = "1m"', 'call = "say"', f'args = {{ note = "{SAY}" }}']
    with open(os.path.join(folder, "expect.toml"), "w") as out:
        out.write("\n".join(toml) + "\n")



def main():
    root = sys.argv[1]
    up_at_50 = [(0, 3, "at_desk"), (3, 50, "head_down"), (50, 52, "at_desk"),
                (52, 60, "head_down")]
    write_case(root, "a-word-when-they-look-up",
               "10:00, head down from 3, due at 48 and nothing said, looks up at 50: "
               "the word then, once, and head down again.",
               "10:00", up_at_50, 61, expects=[50], dense=[(45, 56)])
    write_case(root, "nothing-while-they-stay-head-down",
               "10:00, head down from 3 to 60, due at 48: they look up only at 60, past "
               "its 10 minutes: nothing.",
               "10:00", [(0, 3, "at_desk"), (3, 60, "head_down"), (60, 62, "at_desk"),
                         (62, 70, "head_down")], 70, dense=[(45, 65)])
    write_case(root, "a-short-look-up-is-let-be",
               "10:00, head down from 3, a minute looking up at 25, head down again, looks "
               "up at 50: no word at 25, the word at 50 (the minute under the 2-minute gap).",
               "10:00", [(0, 3, "at_desk"), (3, 25, "head_down"), (25, 26, "at_desk"),
                         (26, 50, "head_down"), (50, 52, "at_desk"), (52, 55, "head_down")],
               56, expects=[50], dense=[(45, 54)])
    write_case(root, "a-short-focus-is-let-be",
               "10:00, head down from 3 to 40, then sat up: nothing.",
               "10:00", [(0, 3, "at_desk"), (3, 40, "head_down"), (40, 60, "at_desk")], 60,
               dense=[(38, 60)])
    write_case(root, "a-long-look-up-begins-it-again",
               "10:00, head down 3 to 30, three minutes looking up, head down 33 to 50, "
               "looks up at 50: nothing (past the 2-minute gap, 17 minutes after it).",
               "10:00", [(0, 3, "at_desk"), (3, 30, "head_down"), (30, 33, "at_desk"),
                         (33, 50, "head_down"), (50, 55, "at_desk"), (55, 70, "head_down")],
               70, dense=[(45, 56)])
    write_case(root, "held-while-someone-else-is-in-view",
               "10:00, due at 48, looks up at 50 with a second face in view from 49 to 52: "
               "held, said at 53 once they have been gone a minute.",
               "10:00", [(0, 3, "at_desk"), (3, 50, "head_down"), (50, 56, "at_desk"),
                         (56, 60, "head_down")],
               61, expects=[53], others=[(49, 52)], dense=[(45, 58)])
    write_case(root, "let-go-while-someone-else-stays",
               "10:00, due at 48, looks up at 50 with a second face in view from 49 to 70: "
               "nothing (its 5-minute wait, and the 10 minutes after the focus, run out).",
               "10:00", [(0, 3, "at_desk"), (3, 50, "head_down"), (50, 60, "at_desk"),
                         (60, 70, "head_down")],
               71, others=[(49, 70)], dense=[(45, 70)])
    write_case(root, "held-while-they-talk",
               "10:00, due at 48, looks up at 50 into a conversation to 52:30: held, said "
               "by 53 once the voice has rested.",
               "10:00", [(0, 3, "at_desk"), (3, 50, "head_down"), (50, 56, "at_desk"),
                         (56, 60, "head_down")],
               61, expects=[53], dense=[(45, 58)],
               voice=(("resting", 0), ("speaking", 49.5), ("listening", 50.5),
                      ("resting", 52.5)))
    write_case(root, "once-a-sitting",
               "10:00, the word at 50, ten minutes looking up, head down again from 60 to "
               "110, looks up at 110: the word only once (cooldown, inside the sitting's "
               "3 hours).",
               "10:00", [(0, 3, "at_desk"), (3, 50, "head_down"), (50, 60, "at_desk"),
                         (60, 110, "head_down"), (110, 112, "at_desk"),
                         (112, 115, "head_down")],
               116, expects=[50], dense=[(45, 56), (103, 114)])
    sitting = lambda at: [(at, at + 3, "at_desk"), (at + 3, at + 50, "head_down"),
                          (at + 50, at + 52, "at_desk"), (at + 52, at + 55, "head_down")]
    write_case(root, "twice-a-day-at-most",
               "09:00, three sittings of 55 minutes, head down from 3, looking up at 50, "
               "each 3 h 50 after the last: the word at 50 and 280, not at 510 "
               "(most_a_day = 2).",
               "09:00", sitting(0) + sitting(230) + sitting(460),
               516, expects=[50, 280], dense=[(45, 54), (275, 284), (505, 514)])
    write_case(root, "quiet-in-the-evening",
               "19:30, head down from 3, looks up at 50: due at 20:18, nothing ([active] "
               "09:00 to 20:00).",
               "19:30", up_at_50, 61, dense=[(45, 56)])
    write_case(root, "off-when-asked",
               "10:00, the wellbeing nudges turned off a minute in, then head down from 3, "
               "looks up at 50: nothing ([stop]).",
               "10:00", up_at_50, 61, dense=[(45, 56)], call_tool="1m")
    write_case(root, "quiet-at-night",
               "22:00, head down from 3, looks up at 50: nothing ([quiet] 22:00 to 08:00).",
               "22:00", up_at_50, 61, dense=[(45, 56)])


if __name__ == "__main__":
    main()
