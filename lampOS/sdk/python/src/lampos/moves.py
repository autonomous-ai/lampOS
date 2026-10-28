"""A move, as lamp-body's `moves::check` holds it (crates/body/src/moves.rs),
with nothing to install: `lamp skill check` reads a package's moves with it,
and the fake lamp holds a computer's `move` to it, as lamp-control does
(protocol.rs, #4217)."""

from __future__ import annotations

MOVE_STEP_MS = (40, 1_000)
# A computer's move: 2 to 500 frames (lamp-control's protocol.rs FRAMES).
MOVE_FRAMES = (2, 500)
MOVE_LASTS_MS = (1_500, 10_000)
# Head pitch, head roll, base yaw, shoulder, elbow: counts from rest, and
# counts a second.
MOVE_FARTHEST = (200, 200, 150, 350, 350)
MOVE_FASTEST = (900.0, 1_300.0, 1_000.0, 800.0, 800.0)


def move_why(step_ms: int, frames: list[list[int]]) -> str | None:
    """Why lamp-body's `moves::check` refuses a move of these steps and frames,
    in its words, at its first problem; None for one it plays. lamp-control
    holds a computer's `move` to the same check (protocol.rs, #4217)."""
    if not MOVE_STEP_MS[0] <= step_ms <= MOVE_STEP_MS[1]:
        return f"step_ms {step_ms} is not from 40 to 1000"
    lasts = max(len(frames) - 1, 0) * step_ms
    if not MOVE_LASTS_MS[0] <= lasts <= MOVE_LASTS_MS[1]:
        return f"it lasts {lasts} ms, not 1500 to 10000"
    if not frames or frames[0] != [0] * 5 or frames[-1] != [0] * 5:
        return "it does not begin and end at nothing"
    step = step_ms / 1_000
    for at, (before, after) in enumerate(zip(frames, frames[1:]), start=1):
        for joint in range(5):
            if abs(after[joint]) > MOVE_FARTHEST[joint]:
                return f"frame {at} takes joint {joint} {after[joint]} counts, farther than {MOVE_FARTHEST[joint]}"
            speed = abs(after[joint] - before[joint]) / step
            if speed > MOVE_FASTEST[joint] + 2 / step:
                return f"frame {at} asks joint {joint} for {speed:.0f} counts a second, faster than {MOVE_FASTEST[joint]:.0f}"
    return None


def move_refusal(step_ms: int, frames: list[list[int]]) -> str | None:
    """lamp-control's refusal of a computer's `move` (protocol.rs), in its
    words, as `bad_request`: its own shape first, then the body's check;
    None for one it takes."""
    if not MOVE_STEP_MS[0] <= step_ms <= MOVE_STEP_MS[1]:
        return f"step_ms: from {MOVE_STEP_MS[0]} to {MOVE_STEP_MS[1]}"
    if not MOVE_FRAMES[0] <= len(frames) <= MOVE_FRAMES[1]:
        return f"frames: from {MOVE_FRAMES[0]} to {MOVE_FRAMES[1]}"
    if frames[0] != [0] * 5 or frames[-1] != [0] * 5:
        return "frames: start and end at home, all five 0"
    why = move_why(step_ms, frames)
    return None if why is None else f"frames: a move this body cannot play: {why}"
