"""A move, as lamp-body's `moves::check` holds it (crates/body/src/moves.rs),
with nothing to install: `lamp skill check` reads a package's moves with it,
and the fake lamp holds a computer's `move` to it, as lamp-control does
(protocol.rs, #4217)."""

from __future__ import annotations

import math

from .errors import BY_CODE

MOVE_STEP_MS = (40, 1_000)
# A computer's move: 2 frames at the least (lamp-control's protocol.rs
# FRAMES_MIN); its most are the body's check's (10 s at a 40 ms step, 251).
MOVE_FRAMES_MIN = 2
MOVE_LASTS_MS = (1_500, 10_000)
# Head pitch, head roll, base yaw, shoulder, elbow: counts from rest, and
# counts a second.
MOVE_FARTHEST = (200, 200, 250, 350, 350)
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
                reach = ", this lamp's yaw reach (--reach)" if joint == 2 else ""
                return f"frame {at} takes joint {joint} {after[joint]} counts, farther than {MOVE_FARTHEST[joint]}{reach}"
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
    if len(frames) < MOVE_FRAMES_MIN:
        return f"frames: at least {MOVE_FRAMES_MIN}"
    if frames[0] != [0] * 5 or frames[-1] != [0] * 5:
        return "frames: start and end at home, all five 0"
    why = move_why(step_ms, frames)
    return None if why is None else f"frames: a move this body cannot play: {why}"


# A move from poses (`poses_to_frames`, `Lamp.move_poses`): the joints by
# name, in a frame's order, and whether a joint's degrees grow with its
# counts. The head looks up with fewer counts, so up is positive for `pitch`,
# as `get` `body` and `joints` count it (lamp-control's body_info.rs).
POSE_JOINTS = ("pitch", "roll", "yaw", "shoulder", "elbow")
_POSE_SIGN = (-1, 1, 1, 1, 1)
COUNTS_PER_DEGREE = 4_096 / 360
# The step a move from poses is sent at, in ms.
POSES_STEP_MS = 50
# How much of each joint's fastest a pose-to-pose ease asks at its peak: the
# rest is room for rounding and for the body's own smoothing.
_POSES_SPEED_SHARE = 0.8
# The shortest ease from one pose to the next, in ms: quicker reads as a jerk.
_POSES_SHORTEST_EASE_MS = 400


def _ceil_steps(ms: float, step_ms: int) -> int:
    return max(1, math.ceil(ms / step_ms - 1e-9))


def poses_to_frames(poses: list[dict], *, step_ms: int = POSES_STEP_MS, hold_ms: int = 400) -> list[list[int]]:
    """A `move`'s frames that go from the standing pose through each of
    `poses` in turn and back, eased in and out, each pose held `hold_ms`.

    A pose names the joints it turns, in degrees from the standing pose:
    `pitch` (up is positive), `roll`, `yaw` (right is positive), `shoulder`
    and `elbow` (up is positive); a joint it leaves out is at the standing
    pose. Each ease takes as long as its widest turn needs at the body's
    speeds, at least 0.4 s, and a move shorter than 1.5 s is slowed to 1.5 s.

    Raises LampError (code `bad_request`), before anything is sent, for an unknown joint, a joint
    farther than a move may take it, or poses that would take more than 10 s.
    The frames pass the body's move check (`move_why`); the body still holds
    each joint to its own bounds (`get("body")`)."""
    if not MOVE_STEP_MS[0] <= step_ms <= MOVE_STEP_MS[1]:
        raise BY_CODE["bad_request"](f"step_ms {step_ms} is not from {MOVE_STEP_MS[0]} to {MOVE_STEP_MS[1]}")
    if isinstance(hold_ms, bool) or not isinstance(hold_ms, int) or not 0 <= hold_ms <= MOVE_LASTS_MS[1]:
        raise BY_CODE["bad_request"](f"hold_ms must be a whole number from 0 to {MOVE_LASTS_MS[1]}")
    if not isinstance(poses, (list, tuple)) or not poses:
        raise BY_CODE["bad_request"]("poses: at least one pose")
    points = [[0] * 5]
    for at, pose in enumerate(poses, start=1):
        if not isinstance(pose, dict) or not pose:
            raise BY_CODE["bad_request"](f"pose {at}: name at least one of {', '.join(POSE_JOINTS)}, in degrees")
        point = [0] * 5
        for name, degrees in pose.items():
            if name not in POSE_JOINTS:
                raise BY_CODE["bad_request"](f"pose {at}: {name!r} is not a joint: {', '.join(POSE_JOINTS)}")
            if isinstance(degrees, bool) or not isinstance(degrees, (int, float)) or not math.isfinite(degrees):
                raise BY_CODE["bad_request"](f"pose {at}: {name} must be a number of degrees")
            joint = POSE_JOINTS.index(name)
            counts = round(_POSE_SIGN[joint] * degrees * COUNTS_PER_DEGREE)
            if abs(counts) > MOVE_FARTHEST[joint]:
                reach = MOVE_FARTHEST[joint] / COUNTS_PER_DEGREE
                raise BY_CODE["bad_request"](f"pose {at}: {name} {degrees:g}° is farther than a move may take it, {reach:.1f}° either way")
            point[joint] = counts
        points.append(point)
    points.append([0] * 5)
    # Each ease's steps: a half-cosine peaks at pi/2 times its mean speed.
    eases = []
    for before, after in zip(points, points[1:]):
        ms = _POSES_SHORTEST_EASE_MS
        for joint in range(5):
            fastest = MOVE_FASTEST[joint] * _POSES_SPEED_SHARE
            ms = max(ms, math.pi / 2 * abs(after[joint] - before[joint]) / fastest * 1_000)
        eases.append(_ceil_steps(ms, step_ms))
    hold_steps = round(hold_ms / step_ms)
    held = hold_steps * len(poses)
    shortest = _ceil_steps(MOVE_LASTS_MS[0], step_ms)
    if sum(eases) + held < shortest:
        # Too quick for a move: slow every ease alike, never speed one up.
        stretch = (shortest - held) / sum(eases)
        eases = [_ceil_steps(steps * stretch * step_ms, step_ms) for steps in eases]
    lasts = (sum(eases) + held) * step_ms
    if lasts > MOVE_LASTS_MS[1]:
        raise BY_CODE["bad_request"](
            f"these poses take {lasts} ms at the body's speeds, longer than a move's {MOVE_LASTS_MS[1]}: fewer or nearer poses, or a shorter hold_ms"
        )
    frames = [[0] * 5]
    for index, steps in enumerate(eases):
        before, after = points[index], points[index + 1]
        for k in range(1, steps + 1):
            share = (1 - math.cos(math.pi * k / steps)) / 2
            frames.append([round(a + (b - a) * share) for a, b in zip(before, after)])
        if index < len(poses):
            frames.extend([list(after) for _ in range(hold_steps)])
    why = move_why(step_ms, frames)
    if why is not None:
        raise BY_CODE["bad_request"](f"these poses make a move the body would refuse: {why}")
    return frames
