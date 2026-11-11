// The try-it app's buttons, as data: ONE entry per SDK call (CONTROLS) or per thing that happens in the room
// (STIMULI). The twin's server runs an entry (`run`, `argv`); the page draws it from `describe()` (label, fields,
// group), so a new SDK feature is a new entry here and nothing else in panel.js or server.js. Every entry is
// run by test/try.test.js with its default values, so none can break unseen.
//
// An entry: `{ id, group, label, fields, run }`; a field: `{ name, label, type, ... }` with type
//   'select' (`options`, `default`), 'number' (`min`, `max`, `integer`, `default`) or 'text' (`max`, `default`);
// `optional: true` lets a field be left empty. `run(ctx, values)` makes the SDK call and returns the call as the
// log says it; `ctx` is `{ lamp, release, listenFor }` (`release()` gives back the twin's own slider lease first).
// A refusal comes back as the lamp's own words; a value outside its field is refused here, before any call.
import { fileURLToPath } from 'node:url';
import { LampError, schema } from 'lampos';

const q = JSON.stringify;
// The one skill the app may put on the lamp: a folder of its own with one recorded move, never one the page names.
const TRY_SKILL = fileURLToPath(new URL('./skills/try-wave', import.meta.url));
const refuse = (why) => new LampError(why, { code: 'bad_request' });

export const COLOURS = [...schema.colours];
export const GESTURES = [...schema.gestures];
export const PATTERNS = [...schema.patterns];
export const FEELINGS = ['glad', 'lively', 'low'];

// The values of `fields` from `args`: a default for a missing one, each within its limits, else `bad_request`.
export function checked(fields, args = {}) {
  const out = {};
  for (const field of fields) {
    let value = args[field.name] ?? field.default;
    if ((value === undefined || value === '') && field.optional) continue;
    if (field.type === 'select') {
      if (!field.options.includes(value)) throw refuse(`${field.name}: one of ${field.options.join(', ')}`);
    } else if (field.type === 'number') {
      value = Number(value);
      if (!Number.isFinite(value) || value < field.min || value > field.max) throw refuse(`${field.name}: a number from ${field.min} to ${field.max}`);
      if (field.integer) value = Math.round(value);
    } else if (field.type === 'text') {
      if (typeof value !== 'string' || !value.trim() || value.length > field.max) throw refuse(`${field.name}: 1 to ${field.max} characters`);
    } else throw refuse(`${field.name}: no such field type`);
    out[field.name] = value;
  }
  return out;
}

const select = (name, options, def, extra = {}) => ({ name, label: name, type: 'select', options, default: def ?? options[0], ...extra });
const number = (name, min, max, def, extra = {}) => ({ name, label: name, type: 'number', min, max, default: def, ...extra });
const text = (name, max, def, extra = {}) => ({ name, label: name, type: 'text', max, default: def, ...extra });

export const CONTROLS = [
  { id: 'gesture', group: 'Body', label: 'Gesture', fields: [select('name', GESTURES, 'nod')],
    run: async ({ lamp }, a) => { await lamp.gesture(a.name); return `lamp.gesture(${q(a.name)})`; } },
  { id: 'express', group: 'Body', label: 'Express', fields: [select('feeling', FEELINGS)],
    run: async ({ lamp }, a) => { await lamp.express(a.feeling); return `lamp.express(${q(a.feeling)})`; } },
  { id: 'look', group: 'Body', label: 'Look', fields: [number('yaw', -180, 180, 20), number('pitch', -90, 90, 0)],
    run: async ({ lamp, release }, a) => {
      const at = { yaw: a.yaw, pitch: a.pitch };
      await release();
      await lamp.withLease(['body'], () => lamp.look(at, { wait: true }));
      return `lamp.withLease(['body'], () => lamp.look(${q(at)}, { wait: true }))`;
    } },
  { id: 'move', group: 'Body', label: 'Move poses', fields: [],
    run: async ({ lamp, release }) => {
      const poses = [{ pitch: 12, yaw: -15 }, { pitch: -6, yaw: 15 }, { pitch: 8, yaw: 0 }];
      // A move needs the body lease at the door, as a look does.
      await release();
      await lamp.withLease(['body'], () => lamp.movePoses(poses, { wait: true }));
      return `lamp.withLease(['body'], () => lamp.movePoses(${q(poses)}, { wait: true }))`;
    } },
  { id: 'stop', group: 'Body', label: 'Stop', fields: [], run: async ({ lamp }) => { await lamp.stop(); return 'lamp.stop()'; } },
  // The owner's language the lamp said in hello: a read, so the log says the code it got.
  { id: 'language', group: 'Voice', label: 'Language', fields: [], run: async ({ lamp }) => `lamp.language // ${q(lamp.language)}` },
  { id: 'light', group: 'Light', label: 'Light',
    fields: [select('colour', COLOURS, 'warm'), number('level', 1, 5, 3, { integer: true }), select('pattern', ['', ...PATTERNS], '', { optional: true }), number('for_ms', 1, 30_000, 10_000, { integer: true })],
    run: async ({ lamp }, a) => {
      await lamp.light(a.colour, a.level, a.for_ms, a.pattern);
      return `lamp.light(${q(a.colour)}, ${a.level}, ${a.for_ms}${a.pattern ? `, ${q(a.pattern)}` : ''})`;
    } },
  { id: 'light_off', group: 'Light', label: 'Light off', fields: [number('for_ms', 1, 30_000, 10_000, { integer: true })],
    run: async ({ lamp }, a) => { await lamp.lightOff({ forMs: a.for_ms }); return `lamp.lightOff({ forMs: ${a.for_ms} })`; } },
  { id: 'say', group: 'Voice', label: 'Say', fields: [text('text', 200, 'Hello, I am the virtual lamp.')],
    run: async ({ lamp }, a) => { await lamp.say(a.text); return `lamp.say(${q(a.text)})`; } },
  // The lamp's own clip, through the voice: no lease (`play` is the recorded move, not a sound).
  { id: 'sound', group: 'Voice', label: 'Sound (hum)', fields: [], run: async ({ lamp }) => { await lamp.sound('hum'); return "lamp.sound('hum')"; } },
  { id: 'skill_push', group: 'Skills', label: 'Push the try-wave skill', fields: [],
    run: async ({ lamp }) => {
      const skill = await lamp.pushSkill(TRY_SKILL);
      return `lamp.pushSkill('skills/try-wave')  // ${skill.held ? `held: ${skill.held}` : skill.on ? 'on' : 'off'}`;
    } },
  { id: 'skills', group: 'Skills', label: 'List my skills', fields: [],
    run: async ({ lamp }) => {
      const kept = await lamp.skills();
      return `lamp.skills()  // ${kept.map((k) => `${k.name} ${k.version}${k.on ? '' : ' (off)'}`).join(', ') || 'none'}`;
    } },
  { id: 'play', group: 'Skills', label: 'Play a recorded move', fields: [select('name', ['small-nod'])],
    run: async ({ lamp, release }, a) => {
      // `play` needs the body lease at the door, as a look does.
      await release();
      await lamp.withLease(['body'], () => lamp.play(a.name, { wait: true }));
      return `lamp.withLease(['body'], () => lamp.play(${q(a.name)}, { wait: true }))`;
    } },
  { id: 'skill_remove', group: 'Skills', label: 'Remove the try-wave skill', fields: [],
    run: async ({ lamp }) => { await lamp.removeSkill('try-wave'); return "lamp.removeSkill('try-wave')"; } },
];

// The world, not the SDK: `lamp-sim <verb> ... DIR` writes what the lamp's stand-ins read. `argv` is an array,
// never a shell string; the lamp's folder is added last.
export const STIMULI = [
  { id: 'arrive', group: 'People', label: 'Someone arrives', fields: [], argv: () => ['people', '1'] },
  { id: 'leave', group: 'People', label: 'Everyone leaves', fields: [], argv: () => ['people', '0'] },
  { id: 'people', group: 'People', label: 'Set people', fields: [number('n', 0, 5, 1, { integer: true })], argv: (a) => ['people', String(a.n)] },
  { id: 'hear', group: 'Sound', label: 'Hear it', fields: [text('text', 120, 'good night lamp')], argv: (a) => ['hear', a.text], listensFor: (a) => a.text },
  { id: 'sound', group: 'Sound', label: 'Make the sound', fields: [select('kind', ['music', 'laughter', 'door', 'footsteps'])], argv: (a) => ['sound', a.kind] },
  // A held hand is ONE pet (a real lamp tells one per stroke): a touch within `gapMs` of the last is the same stroke.
  { id: 'touch', group: 'Touch', label: 'Touch', fields: [select('kind', ['tap', 'pet'])], argv: (a) => ['touch', a.kind], gapMs: 800 },
  { id: 'hand', group: 'Touch', label: 'A hand', fields: [], argv: () => ['hand'] },
  { id: 'air', group: 'Air', label: 'Air', fields: [select('kind', ['stuffy', 'clear'])],
    argv: (a) => ['air', ...(a.kind === 'stuffy' ? ['co2_ppm=1600', 'pm2_5=45'] : ['co2_ppm=450', 'pm2_5=5'])] },
];

// What the page needs to draw the buttons: the entries without their code.
export const describe = () => ({
  controls: CONTROLS.map(({ id, group, label, fields }) => ({ id, group, label, fields })),
  stimuli: STIMULI.map(({ id, group, label, fields }) => ({ id, group, label, fields })),
});

export const findControl = (id) => CONTROLS.find((c) => c.id === id);
export const findStimulus = (id) => STIMULI.find((s) => s.id === id);
