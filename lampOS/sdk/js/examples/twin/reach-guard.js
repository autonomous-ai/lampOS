// The try-it page's guard for the arm and the head: a goal is checked against the reach map (tools/reach, `query.py --export`)
// before it is sent. The page only: the body's own limits are untouched and still the last word.
//
// The map says, for a shoulder and an elbow, which head pitches are safe: the lamp's own anti-tip rule and the CAD's parts not
// touching each other or the table (its masses are V1's estimates until measured on a lamp, its geometry lamp-4ace's). Poses are
// in the map's counts: the arm from its folded pose (deg 0 is folded) and the head's pitch from its home (up is fewer counts).
// Yaw and roll are not in the map and pass as they are.
//
// A goal inside the safe poses is sent as it is, or held at the edge of the safe range, said in words; from a pose the map calls
// unsafe (the folded pose is one: the head rests on its arm) a goal is sent only if it is no further from safe than the lamp is now,
// so the guard never traps a lamp where it is.
import { readFileSync } from 'node:fs';

export const DEG_A_COUNT = 360 / 4096;
// The map's lamp: lamp-4ace's folded arm and its head's home (counts), from which a pose's degrees are laid out.
const FOLDED = { shoulder: 2048, elbow: 2047 };
const HEAD_HOME = 1772;
export const MAPPED = ['shoulder', 'elbow', 'pitch'];

export function loadReachMap(path) {
  const map = JSON.parse(readFileSync(path, 'utf8'));
  const { shoulder, elbow, pitch } = map.axes ?? {};
  if (![shoulder, elbow, pitch].every((axis) => Array.isArray(axis) && axis.length > 1) || !Array.isArray(map.safe_pitch_runs)) throw new Error(`${path}: not a reach map`);
  if (map.safe_pitch_runs.length !== shoulder.length || map.safe_pitch_runs.some((row) => row.length !== elbow.length)) throw new Error(`${path}: the runs do not fit the axes`);
  return map;
}

/** The map's counts for the joints' degrees `{ shoulder, elbow, pitch }`. */
export const countsOf = (deg) => ({
  shoulder: FOLDED.shoulder + deg.shoulder / DEG_A_COUNT,
  elbow: FOLDED.elbow + deg.elbow / DEG_A_COUNT,
  pitch: HEAD_HOME - deg.pitch / DEG_A_COUNT,
});
const degOf = (joint, counts) => (joint === 'pitch' ? (HEAD_HOME - counts) * DEG_A_COUNT : (counts - FOLDED[joint]) * DEG_A_COUNT);

const nearest = (axis, value) => {
  let at = 0;
  for (let i = 1; i < axis.length; i++) if (Math.abs(axis[i] - value) < Math.abs(axis[at] - value)) at = i;
  return at;
};
const inRuns = (runs, pitch) => runs.some(([low, high]) => pitch >= low && pitch <= high);

/** Whether the pose (counts) is safe on the map, and still is `shrink` counts away on every axis. */
export function isSafe(map, pose, shrink = 0) {
  const at = (p) => map.safe_pitch_runs[nearest(map.axes.shoulder, p.shoulder)][nearest(map.axes.elbow, p.elbow)];
  const around = [pose];
  if (shrink > 0) for (const joint of MAPPED) for (const sign of [-1, 1]) around.push({ ...pose, [joint]: pose[joint] + sign * shrink });
  return around.every((p) => p.pitch >= map.axes.pitch[0] && p.pitch <= map.axes.pitch.at(-1) && inRuns(at(p), p.pitch));
}

/** How many counts (the most on any axis) the pose is from a safe grid pose: 0 on a safe one. */
export function distance(map, pose) {
  if (isSafe(map, pose, 0)) return 0;
  const { shoulder, elbow } = map.axes;
  let best = Infinity;
  for (let i = 0; i < shoulder.length; i++) {
    for (let j = 0; j < elbow.length; j++) {
      const base = Math.max(Math.abs(shoulder[i] - pose.shoulder), Math.abs(elbow[j] - pose.elbow));
      if (base >= best) continue;
      for (const [low, high] of map.safe_pitch_runs[i][j]) best = Math.min(best, Math.max(base, pose.pitch < low ? low - pose.pitch : pose.pitch > high ? pose.pitch - high : 0));
    }
  }
  return best;
}

/** How many counts `joint` alone must move, the others held, to reach a safe pose: 0 on a safe one, Infinity when no value of it is safe here. */
export function lineDistance(map, pose, joint) {
  if (isSafe(map, pose, 0)) return 0;
  if (joint === 'pitch') {
    const runs = map.safe_pitch_runs[nearest(map.axes.shoulder, pose.shoulder)][nearest(map.axes.elbow, pose.elbow)];
    return Math.min(Infinity, ...runs.map(([low, high]) => (pose.pitch < low ? low - pose.pitch : pose.pitch - high)));
  }
  return Math.min(Infinity, ...map.axes[joint].filter((v) => isSafe(map, { ...pose, [joint]: v }, 0)).map((v) => Math.abs(v - pose[joint])));
}

/** The counts `joint` may go to with the others held at `pose`: the run of safe poses that holds `pose`, `shrink` counts in from each end; null when the pose is not safe. */
export function limits(map, pose, joint, shrink = 0) {
  if (!isSafe(map, pose, 0)) return null;
  let [low, high] = [pose[joint], pose[joint]];
  if (joint === 'pitch') {
    // The run of safe pitches at this shoulder and elbow that holds the pitch: the same ends wherever in it the head is.
    const runs = map.safe_pitch_runs[nearest(map.axes.shoulder, pose.shoulder)][nearest(map.axes.elbow, pose.elbow)];
    [low, high] = runs.find(([a, b]) => pose.pitch >= a && pose.pitch <= b);
  } else {
    // Along the shoulder's or the elbow's grid from the point the pose is nearest: the ends are grid points, the same wherever in the run the joint is.
    const axis = map.axes[joint];
    const safeAt = (value) => isSafe(map, { ...pose, [joint]: value }, 0);
    let i = nearest(axis, pose[joint]);
    if (!safeAt(axis[i])) return [pose[joint], pose[joint]];
    let [lo, hi] = [i, i];
    while (lo > 0 && safeAt(axis[lo - 1])) lo--;
    while (hi < axis.length - 1 && safeAt(axis[hi + 1])) hi++;
    [low, high] = [Math.min(axis[lo], pose[joint]), Math.max(axis[hi], pose[joint])];
  }
  // The shrink takes counts off each end, never past the pose itself.
  return [Math.min(pose[joint], low + shrink), Math.max(pose[joint], high - shrink)];
}

/**
 * The degrees to send for `joint` when asked for `deg`: `{ deg, note }`, `note` set when the goal was changed.
 * `joints` is body.state's `joints` (each `{ deg, goal_deg? }`); the other joints are taken at their goal, else where they are.
 */
export function restrict(map, joints, joint, deg, { shrink = 48 } = {}) {
  if (!MAPPED.includes(joint) || !Number.isFinite(deg)) return { deg };
  const here = {};
  const goals = {};
  for (const name of MAPPED) {
    const j = joints?.[name];
    if (!j || !Number.isFinite(j.deg)) return { deg }; // the lamp has not said its pose: nothing to check against
    here[name] = j.deg;
    goals[name] = Number.isFinite(j.goal_deg) ? j.goal_deg : j.deg;
  }
  const now = countsOf(here);
  const held = countsOf({ ...goals, [joint]: here[joint] });
  const goal = countsOf({ ...goals, [joint]: deg });
  if (isSafe(map, held, 0)) {
    const range = limits(map, held, joint, shrink);
    const [a, b] = [degOf(joint, range[0]), degOf(joint, range[1])].sort((x, y) => x - y);
    if (deg >= a && deg <= b) return { deg };
    // Rounded toward the inside, so the held value is itself inside the range.
    const clamped = deg > b ? Math.floor(b * 10) / 10 : Math.ceil(a * 10) / 10;
    return { deg: clamped, note: `${joint} ${deg} is past what the other joints allow here: held at ${clamped}` };
  }
  // The pose is not safe now: only a goal no further from safe than the lamp is. One joint's own way to a safe pose is
  // compared when it has one, else all the joints' together.
  const [fromGoal, fromHere] = [lineDistance(map, goal, joint), lineDistance(map, { ...held, [joint]: now[joint] }, joint)];
  const closer = Number.isFinite(fromGoal) && Number.isFinite(fromHere) ? fromGoal <= fromHere : distance(map, goal) <= distance(map, now);
  if (closer) return { deg };
  return { deg: Math.round(here[joint] * 10) / 10, note: `${joint} ${deg} would take the lamp further from its safe poses: not sent` };
}

/** `restrict` for a set of targets (the `joints` op): each one in turn, the others as they are asked for. */
export function restrictAll(map, joints, targets, options) {
  const out = {};
  const notes = [];
  const asked = Object.fromEntries(Object.entries(joints ?? {}).map(([k, v]) => [k, { ...v, goal_deg: targets[k] ?? v.goal_deg }]));
  for (const [joint, deg] of Object.entries(targets)) {
    const result = restrict(map, asked, joint, deg, options);
    out[joint] = result.deg;
    if (result.note) notes.push(result.note);
    if (asked[joint]) asked[joint] = { ...asked[joint], goal_deg: result.deg };
  }
  return { targets: out, notes };
}

/**
 * What the map allows each of shoulder, elbow and pitch now, in degrees, for the page to show beside its slider:
 * `{ joint: [low, high] }` (the others held at their goals, `shrink` counts in from each end), or `{ joint: null }` when the lamp is in a pose the
 * map calls unsafe (there only a move toward safe is sent). `null` when the lamp has not said its pose.
 */
export function rangesDeg(map, joints, { shrink = 48 } = {}) {
  const goals = {};
  for (const name of MAPPED) {
    const j = joints?.[name];
    if (!j || !Number.isFinite(j.deg)) return null;
    goals[name] = Number.isFinite(j.goal_deg) ? j.goal_deg : j.deg;
  }
  const pose = countsOf(goals);
  const out = {};
  for (const joint of MAPPED) {
    const range = limits(map, pose, joint, shrink);
    if (!range) { out[joint] = null; continue; }
    // Rounded inward, so each end shown is itself allowed.
    const [a, b] = [degOf(joint, range[0]), degOf(joint, range[1])].sort((x, y) => x - y);
    out[joint] = [Math.ceil(a * 10) / 10, Math.floor(b * 10) / 10];
  }
  return out;
}
