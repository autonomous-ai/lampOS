// What the real body will do with a joint's slider, as the lamp itself says it: `get body`'s `min_deg` and `max_deg` per joint
// (docs/control.md, "The body's limits"). The page WARNS with these (a band, an amber thumb, a ghost of what was asked beside the
// pose the body will take); it never blocks a send: the lamp's own answer is the rule, and a lamp that says no limits is drawn as
// before. Loaded by index.html and by the server's tests.

const finite = (x) => typeof x === 'number' && Number.isFinite(x);

// `{ name: { min, max } }` for each joint of a `bodyInfo()` answer that says both bounds (a joint the body has not measured says none).
export function limitsOf(info) {
  const out = {};
  for (const [name, joint] of Object.entries(info?.joints ?? {})) {
    if (finite(joint?.min_deg) && finite(joint?.max_deg) && joint.min_deg <= joint.max_deg) out[name] = { min: joint.min_deg, max: joint.max_deg };
  }
  return out;
}

// What the body will do with `deg`: held inside its reach. No limit known: as asked.
export const clampTo = (limit, deg) => (limit && finite(deg) ? Math.min(limit.max, Math.max(limit.min, deg)) : deg);

// Whether the body will cut `deg` (more than `slack` degrees past its reach).
export const pastReach = (limit, deg, slack = 0.05) => Boolean(limit) && finite(deg) && (deg < limit.min - slack || deg > limit.max + slack);

const deg = (x) => `${(Math.round(x * 10) / 10).toFixed(1)}°`;

// The line under a joint's slider: its real reach, and what the body will do with what was asked.
export function reachText(limit, asked) {
  if (!limit) return '';
  const reach = `the lamp reaches ${deg(limit.min)} to ${deg(limit.max)}`;
  return pastReach(limit, asked) ? `${reach}: it will cut ${deg(asked)} to ${deg(clampTo(limit, asked))}` : reach;
}

// The lamp's own words for a joint's `done` (its `how`, and its `why`; `clamped` and the angles it reached, as the body says them).
export function doneWords(done, joint) {
  const how = done?.how;
  const said = done?.why ? `: ${done.why}` : '';
  if (how === 'dropped') return `the lamp did not take it${said}`;
  if (how === 'cut_short') return `the lamp was cut short${said}`;
  const reached = finite(done?.[joint]) ? ` (reached ${deg(done[joint])})` : '';
  if (how === 'finished') return done.clamped ? `the lamp took it, cut by the body${reached}${said}` : `the lamp took it${reached}`;
  return '';
}
