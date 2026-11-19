// What the real body will do with a joint's slider, as the lamp itself says it: `get body`'s `min_deg` and `max_deg` per joint
// (docs/control.md, "The body's limits"). The page WARNS with these (a band, an amber thumb, a ghost of what was asked beside the
// pose the body will take); it never blocks a send: the lamp's own answer is the rule, and a lamp that says no limits is drawn as
// before. Loaded by index.html and by the server's tests.

const finite = (x) => typeof x === 'number' && Number.isFinite(x);

const COUNT_DEG = 360 / 4096;
const ARM = ['shoulder', 'elbow'];

// `{ name: { min, max, pose? } }` for each joint of a `bodyInfo()` answer that says both bounds (a joint the body has not measured says none).
// `min`/`max` are the servo's span. `pose` is what a holder's pose can ask of the arm today on a lamp that stands: the body cuts the shoulder's
// and the elbow's target to the counts between its fold and its stand (`crates/body/src/body.rs`, the holder's pose: no margin), so nothing above
// the stand is taken ("not yet": #7513 part 4). In degrees from the fold, as `get body` counts them.
export function limitsOf(info) {
  const out = {};
  for (const [name, joint] of Object.entries(info?.joints ?? {})) {
    if (!(finite(joint?.min_deg) && finite(joint?.max_deg) && joint.min_deg <= joint.max_deg)) continue;
    out[name] = { min: joint.min_deg, max: joint.max_deg };
    if (info?.stands === true && ARM.includes(name) && finite(joint.folded_counts) && finite(joint.stand_counts)) {
      const stand = (joint.stand_counts - joint.folded_counts) * COUNT_DEG;
      out[name].pose = { min: Math.min(0, stand), max: Math.max(0, stand) };
    }
  }
  return out;
}

// What the body takes of a joint: its span, inside the pose range where it says one.
export function reachOf(limit) {
  if (!limit) return null;
  return limit.pose ? { min: Math.max(limit.min, limit.pose.min), max: Math.min(limit.max, limit.pose.max) } : { min: limit.min, max: limit.max };
}

// What the body will do with `deg`: held inside its reach. No limit known: as asked.
export const clampTo = (limit, deg) => { const reach = reachOf(limit); return reach && finite(deg) ? Math.min(reach.max, Math.max(reach.min, deg)) : deg; };

// Whether the body will cut `deg` (more than `slack` degrees past its reach).
export const pastReach = (limit, deg, slack = 0.05) => { const reach = reachOf(limit); return Boolean(reach) && finite(deg) && (deg < reach.min - slack || deg > reach.max + slack); };

// Whether `deg` is above the stand where the body takes nothing yet (an arm joint on a standing lamp).
export const aboveStand = (limit, deg, slack = 0.05) => Boolean(limit?.pose) && finite(deg) && deg > limit.pose.max + slack;

const deg = (x) => `${(Math.round(x * 10) / 10).toFixed(1)}°`;

// The line under a joint's slider: its real reach, and what the body will do with what was asked.
export function reachText(limit, asked) {
  if (!limit) return '';
  const own = reachOf(limit);
  const reach = limit.pose ? `the body takes ${deg(own.min)} to ${deg(own.max)} (up to its stand; above it is not yet)` : `the lamp reaches ${deg(limit.min)} to ${deg(limit.max)}`;
  if (aboveStand(limit, asked)) return `${reach}: ${deg(asked)} is above the stand, it will be cut to ${deg(clampTo(limit, asked))}`;
  return pastReach(limit, asked) ? `${reach}: it will cut ${deg(asked)} to ${deg(clampTo(limit, asked))}` : reach;
}

// The lamp's own words for a joint's `done` (its `how`, and its `why`; `clamped` and the angles it reached, as the body says them).
export function doneWords(done, joint, limit) {
  const how = done?.how;
  const said = done?.why ? `: ${done.why}` : '';
  if (how === 'dropped') return `the lamp did not take it${said}`;
  if (how === 'cut_short') return `the lamp was cut short${said}`;
  const reached = finite(done?.[joint]) ? ` (reached ${deg(done[joint])})` : '';
  const atStand = limit?.pose && finite(done?.[joint]) && Math.abs(done[joint] - limit.pose.max) <= 2.5 ? ' (at its stand: above it is not yet)' : '';
  if (how === 'finished') return done.clamped ? `the lamp took it, cut by the body${reached}${atStand}${said}` : `the lamp took it${reached}`;
  return '';
}
