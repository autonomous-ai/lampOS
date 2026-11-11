// A move, as lamp-body's `moves::check` holds it (crates/body/src/moves.rs),
// and a move made from poses (`posesToFrames`, `Lamp.movePoses`). The Python
// SDK's lampos/moves.py has the same.
import { LampError } from './errors.js';

export const MOVE_STEP_MS = [40, 1000];
export const MOVE_LASTS_MS = [1500, 10000];
// A computer's move: 2 frames at the least (lamp-control's protocol.rs
// FRAMES_MIN); its most are the body's check's (10 s at a 40 ms step, 251).
export const MOVE_FRAMES_MIN = 2;
export const MOVE_FARTHEST = [200, 200, 250, 350, 350];
export const MOVE_FASTEST = [900, 1300, 1000, 800, 800];

// Rust's `{:.0}`, as the body prints a speed: a half rounds to the even neighbour.
function roundHalfEven(value) {
  const floor = Math.floor(value);
  const part = value - floor;
  if (part !== 0.5) return Math.round(value);
  return floor % 2 === 0 ? floor : floor + 1;
}

// Why the body refuses a move of these steps and frames, in its words, at its
// first problem; null for one it plays.
export function moveWhy(stepMs, frames) {
  if (stepMs < MOVE_STEP_MS[0] || stepMs > MOVE_STEP_MS[1]) return `step_ms ${stepMs} is not from 40 to 1000`;
  const lasts = Math.max(frames.length - 1, 0) * stepMs;
  if (lasts < MOVE_LASTS_MS[0] || lasts > MOVE_LASTS_MS[1]) return `it lasts ${lasts} ms, not 1500 to 10000`;
  const rest = (frame) => Array.isArray(frame) && frame.length === 5 && frame.every((count) => count === 0);
  if (!frames.length || !rest(frames[0]) || !rest(frames.at(-1))) return 'it does not begin and end at nothing';
  const step = stepMs / 1000;
  for (let at = 1; at < frames.length; at++) {
    for (let joint = 0; joint < 5; joint++) {
      const after = frames[at][joint];
      if (Math.abs(after) > MOVE_FARTHEST[joint]) return `frame ${at} takes joint ${joint} ${after} counts, farther than ${MOVE_FARTHEST[joint]}${joint === 2 ? ", this lamp's yaw reach (--reach)" : ''}`;
      const speed = Math.abs(after - frames[at - 1][joint]) / step;
      if (speed > MOVE_FASTEST[joint] + 2 / step) {
        return `frame ${at} asks joint ${joint} for ${roundHalfEven(speed)} counts a second, faster than ${MOVE_FASTEST[joint]}`;
      }
    }
  }
  return null;
}


// A move from poses: the joints by name, in a frame's order, and whether a
// joint's degrees grow with its counts. The head looks up with fewer counts,
// so up is positive for `pitch`, as `get` `body` and `joints` count it
// (lamp-control's body_info.rs).
export const POSE_JOINTS = Object.freeze(['pitch', 'roll', 'yaw', 'shoulder', 'elbow']);
const POSE_SIGN = [-1, 1, 1, 1, 1];
export const COUNTS_PER_DEGREE = 4096 / 360;
// The step a move from poses is sent at, in ms.
export const POSES_STEP_MS = 50;
// How much of each joint's fastest a pose-to-pose ease asks at its peak: the
// rest is room for rounding and for the body's own smoothing.
const POSES_SPEED_SHARE = 0.8;
// The shortest ease from one pose to the next, in ms: quicker reads as a jerk.
const POSES_SHORTEST_EASE_MS = 400;

const ceilSteps = (ms, stepMs) => Math.max(1, Math.ceil(ms / stepMs - 1e-9));
// Python's round(), as lampos/moves.py rounds counts: a half to the even neighbour.
const roundEven = (value) => (Math.abs(value % 1) === 0.5 ? 2 * Math.round(value / 2) : Math.round(value));
const g = (value) => String(Number(value.toPrecision(6)));

// A `move`'s frames that go from the standing pose through each of `poses` in
// turn and back, eased in and out, each pose held `holdMs`. A pose names the
// joints it turns, in degrees from the standing pose: `pitch` (up is
// positive), `roll`, `yaw` (right is positive), `shoulder` and `elbow` (up is
// positive); one it leaves out is at the standing pose. Each ease takes as long
// as its widest turn needs at the body's speeds, at least 0.4 s, and a move
// shorter than 1.5 s is slowed to 1.5 s. Throws a LampError (code 'bad_request'), before anything
// is sent, for an unknown joint, one farther than a move may take it, or poses
// that would take more than 10 s. The same frames as Python's
// `poses_to_frames`; the body still holds each joint to its own bounds.
export function posesToFrames(poses, { stepMs = POSES_STEP_MS, holdMs = 400 } = {}) {
  if (!Number.isInteger(stepMs) || stepMs < MOVE_STEP_MS[0] || stepMs > MOVE_STEP_MS[1]) {
    throw new LampError(`stepMs ${stepMs} is not from ${MOVE_STEP_MS[0]} to ${MOVE_STEP_MS[1]}`, { code: 'bad_request' });
  }
  if (!Number.isInteger(holdMs) || holdMs < 0 || holdMs > MOVE_LASTS_MS[1]) {
    throw new LampError(`holdMs must be a whole number from 0 to ${MOVE_LASTS_MS[1]}`, { code: 'bad_request' });
  }
  if (!Array.isArray(poses) || poses.length === 0) throw new LampError('poses: at least one pose', { code: 'bad_request' });
  const points = [[0, 0, 0, 0, 0]];
  poses.forEach((pose, index) => {
    const at = index + 1;
    if (pose === null || typeof pose !== 'object' || Array.isArray(pose) || Object.keys(pose).length === 0) {
      throw new LampError(`pose ${at}: name at least one of ${POSE_JOINTS.join(', ')}, in degrees`, { code: 'bad_request' });
    }
    const point = [0, 0, 0, 0, 0];
    for (const [name, degrees] of Object.entries(pose)) {
      const joint = POSE_JOINTS.indexOf(name);
      if (joint < 0) throw new LampError(`pose ${at}: '${name}' is not a joint: ${POSE_JOINTS.join(', ')}`, { code: 'bad_request' });
      if (typeof degrees !== 'number' || !Number.isFinite(degrees)) throw new LampError(`pose ${at}: ${name} must be a number of degrees`, { code: 'bad_request' });
      const counts = roundEven(POSE_SIGN[joint] * degrees * COUNTS_PER_DEGREE);
      if (Math.abs(counts) > MOVE_FARTHEST[joint]) {
        const reach = (MOVE_FARTHEST[joint] / COUNTS_PER_DEGREE).toFixed(1);
        throw new LampError(`pose ${at}: ${name} ${g(degrees)}° is farther than a move may take it, ${reach}° either way`, { code: 'bad_request' });
      }
      point[joint] = counts + 0;
    }
    points.push(point);
  });
  points.push([0, 0, 0, 0, 0]);
  // Each ease's steps: a half-cosine peaks at pi/2 times its mean speed.
  let eases = [];
  for (let index = 1; index < points.length; index++) {
    let ms = POSES_SHORTEST_EASE_MS;
    for (let joint = 0; joint < 5; joint++) {
      const fastest = MOVE_FASTEST[joint] * POSES_SPEED_SHARE;
      ms = Math.max(ms, (Math.PI / 2) * Math.abs(points[index][joint] - points[index - 1][joint]) / fastest * 1000);
    }
    eases.push(ceilSteps(ms, stepMs));
  }
  const holdSteps = roundEven(holdMs / stepMs);
  const held = holdSteps * poses.length;
  const shortest = ceilSteps(MOVE_LASTS_MS[0], stepMs);
  const sum = (list) => list.reduce((a, b) => a + b, 0);
  if (sum(eases) + held < shortest) {
    // Too quick for a move: slow every ease alike, never speed one up.
    const stretch = (shortest - held) / sum(eases);
    eases = eases.map((steps) => ceilSteps(steps * stretch * stepMs, stepMs));
  }
  const lasts = (sum(eases) + held) * stepMs;
  if (lasts > MOVE_LASTS_MS[1]) {
    throw new LampError(`these poses take ${lasts} ms at the body's speeds, longer than a move's ${MOVE_LASTS_MS[1]}: fewer or nearer poses, or a shorter holdMs`, { code: 'bad_request' });
  }
  const frames = [[0, 0, 0, 0, 0]];
  eases.forEach((steps, index) => {
    const before = points[index];
    const after = points[index + 1];
    for (let k = 1; k <= steps; k++) {
      const share = (1 - Math.cos((Math.PI * k) / steps)) / 2;
      frames.push(before.map((a, joint) => roundEven(a + (after[joint] - a) * share) + 0));
    }
    if (index < poses.length) for (let n = 0; n < holdSteps; n++) frames.push([...after]);
  });
  const why = moveWhy(stepMs, frames);
  if (why !== null) throw new LampError(`these poses make a move the body would refuse: ${why}`, { code: 'bad_request' });
  return frames;
}
