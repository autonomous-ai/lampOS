// The try-it app's buttons, as data: ONE entry per SDK call (CONTROLS) or per thing that happens in the room
// (STIMULI). The twin's server runs an entry (`run`, `argv`); the page draws it from `describe()` (label, fields,
// group), so a new SDK feature is a new entry here and nothing else in panel.js or server.js. Every entry is
// run by test/try.test.js with its default values, so none can break unseen.
//
// An entry: `{ id, group, label, fields, run }`; a field: `{ name, label, type, ... }` with type
//   'select' (`options`, `default`), 'number' (`min`, `max`, `integer`, `default`) or 'text' (`max`, `default`);
// `optional: true` lets a field be left empty. `run(ctx, values)` makes the SDK call and returns the call as the
// log says it; `ctx` is `{ lamp, release, listenFor, show }` (`release()` gives back the twin's own slider lease first).
// `ops` are the control.schema.json ops the entry sends (the SDK's `withLease` sends `lease.take` and `lease.release`)
// and `shows` the events its action should bring up in the log; a stimulus has `shows` only. test/controls-cover.test.js
// fails for an op or an event that no entry names, unless controls-exceptions.js lists it with its reason.
// `needs` (the grants the row's calls take, as the lamp checks them): a row the lamp did not grant is DRAWN, its button off,
// with its reason beside it (`describe(mode, { grants, flags })` adds `disabled`), and the server refuses a press of it; a
// camera or microphone row also needs the run's `--camera` / `--microphone` in real mode. `moves: true` rows ask one confirm on a real lamp. `ctx.show(jpeg)` puts a picture on the page
// that pressed, under its row and nowhere else: not in the log, not on disk, not in the checklist's export.
// `hint` (optional, a string of at most 200 characters) is said under the row on the page, in `textContent`; `hintReal`,
// when the page drives a real lamp (`describe('real')`), replaces it. For a row with an effect the label doesn't say.
// `ctx.audio(what, wav)` sends a sound this app made to the page that pressed, if that page turned "Hear the lamp in this page" on, and
// returns the bytes sent (0 when it is off; undefined with no page): a stand-in hum or a clip's own audio, never the microphone's.
// A refusal comes back as the lamp's own words; a value outside its field is refused here, before any call.
import { randomBytes } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { LampError, schema } from 'lampos';

const q = JSON.stringify;
// The one skill the app may put on the lamp: a folder of its own with one recorded move, never one the page names.
const TRY_SKILL = fileURLToPath(new URL('./skills/try-wave', import.meta.url));
// A call and what it answered, for the log (one line: a long answer is cut).
const answered = (call, value) => `${call} -> ${q(value ?? null).slice(0, 300)}`;
const refuse = (why) => new LampError(why, { code: 'bad_request' });

export const COLOURS = [...schema.colours];
export const GESTURES = [...schema.gestures];
export const PATTERNS = [...schema.patterns];
export const FEELINGS = ['glad', 'lively', 'low'];

// 32 colours each: `(k, count) => '#rrggbb'`.
const hue = (h) => `#${[5, 3, 1].map((n) => { const k = (n + h * 6) % 6; return Math.round(255 * (1 - Math.max(0, Math.min(k, 4 - k, 1)))).toString(16).padStart(2, '0'); }).join('')}`;
export const PIXEL_PRESETS = {
  rainbow: (k, count) => hue(k / count),
  'half red, half blue': (k, count) => (k < count / 2 ? '#ff0000' : '#0000ff'),
  'one pixel': (k) => (k === 0 ? '#ffffff' : '#000000'),
};

// The values of `fields` from `args`: a default for a missing one, each within its limits, else `bad_request`.
export function checked(fields, args = {}, mode = 'lamp-sim') {
  const out = {};
  for (const field of fields) {
    let value = args[field.name] ?? (mode === 'real' && field.defaultReal !== undefined ? field.defaultReal : field.default);
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

// This run's one scratch clip: a name of its own (`try-` and six hex digits from this server's start), so a row never
// touches a clip that was on the lamp before; made by `sound_upload`, removed by `sound_remove` (and `cleanScratch`).
const SCRATCH_CLIP = `try-${randomBytes(3).toString('hex')}`;
let clipKept = false;
// In real mode a note is this run's own: its key carries this prefix, and nothing else is read, listed or deleted.
const NOTE_PREFIX = `try-${randomBytes(3).toString('hex')}`;
// The lamp takes at most 64 bytes of key; the run's prefix and its dot take some, so a real run's own text may be MOST_NOTE_TEXT.
const MOST_NOTE_TEXT = 64 - (NOTE_PREFIX.length + 1);
const noteText = (text) => {
  if (Buffer.byteLength(text) > MOST_NOTE_TEXT) throw refuse(`1 to ${MOST_NOTE_TEXT} bytes here (this run's own prefix takes ${NOTE_PREFIX.length + 1} of the lamp's 64)`);
  return text;
};
const noteKey = (ctx, key) => (ctx.mode === 'real' ? `${NOTE_PREFIX}.${noteText(key)}` : key);
// The names this app gives its own test clips: the only ones a row removes.
const TRY_CLIP = /^try-[0-9a-f]{6}$/;
// A quiet tone as the lamp takes a clip: 16-bit mono PCM at 24 kHz, peak 1000 (-30 dBFS, under the voice's -12 dBFS cap and
// its -30 dBFS RMS per 10 ms), made here, never read from a file.
export function quietTone(ms) {
  const frames = Math.round((24_000 * ms) / 1000);
  const wav = Buffer.alloc(44 + frames * 2);
  wav.write('RIFF', 0, 'ascii'); wav.writeUInt32LE(36 + frames * 2, 4); wav.write('WAVEfmt ', 8, 'ascii');
  wav.writeUInt32LE(16, 16); wav.writeUInt16LE(1, 20); wav.writeUInt16LE(1, 22); wav.writeUInt32LE(24_000, 24);
  wav.writeUInt32LE(48_000, 28); wav.writeUInt16LE(2, 32); wav.writeUInt16LE(16, 34); wav.write('data', 36, 'ascii'); wav.writeUInt32LE(frames * 2, 40);
  for (let k = 0; k < frames; k += 1) wav.writeInt16LE(Math.round(1000 * Math.sin((2 * Math.PI * 440 * k) / 24_000)), 44 + k * 2);
  return wav;
}
// The hum the page plays for `sound` (a stand-in: the lamp's real hum is made by its voice, not a file): two low tones, about
// 0.6 s, 16-bit mono PCM at 24 kHz, two tones of amplitude 500 each so the sum peaks at 1000 (-30 dBFS, as the quiet clip), faded in and out so it does not click.
export function humWav(ms = 600) {
  const frames = Math.round((24_000 * ms) / 1000);
  const wav = Buffer.alloc(44 + frames * 2);
  wav.write('RIFF', 0, 'ascii'); wav.writeUInt32LE(36 + frames * 2, 4); wav.write('WAVEfmt ', 8, 'ascii');
  wav.writeUInt32LE(16, 16); wav.writeUInt16LE(1, 20); wav.writeUInt16LE(1, 22); wav.writeUInt32LE(24_000, 24);
  wav.writeUInt32LE(48_000, 28); wav.writeUInt16LE(2, 32); wav.writeUInt16LE(16, 34); wav.write('data', 36, 'ascii'); wav.writeUInt32LE(frames * 2, 40);
  const fade = 720;
  for (let k = 0; k < frames; k += 1) {
    const edge = Math.min(1, k / fade, (frames - 1 - k) / fade);
    const tone = Math.sin((2 * Math.PI * 110 * k) / 24_000) + Math.sin((2 * Math.PI * 165 * k) / 24_000);
    wav.writeInt16LE(Math.round(500 * edge * tone), 44 + k * 2);
  }
  return wav;
}
// What a row says about the sound it made for this page (`ctx.audio` is the server's: the bytes sent, 0 when this page has not
// turned "Hear the lamp in this page" on, undefined where there is no page, as in a test of the row alone).
const heard = (sent, kind) => (sent === undefined ? '' : sent > 0 ? `  // audio: ${sent.toLocaleString('en-US')} bytes to this page${kind ? ` (${kind})` : ''}` : "  // no sound sent: turn on 'Hear the lamp in this page'");
// Takes this run's scratch clip off the lamp, if this run put it there (the end of a run, or the row).
export async function cleanScratch(lamp) {
  let removed = false;
  if (clipKept) { await lamp.removeSound(SCRATCH_CLIP); clipKept = false; removed = true; }
  return removed;
}
// The try-wave skill, if THIS run pushed it and did not remove it (inert, but it would stay on the owner's lamp).
let skillPushed = false;
export async function cleanScratchSkill(lamp) {
  if (!skillPushed) return false;
  await lamp.removeSkill('try-wave');
  skillPushed = false;
  return true;
}
// And the notes this run made on a real lamp (keys under its prefix), at the end of a run.
export async function cleanScratchNotes(lamp) {
  const { keys = [] } = await lamp.notesList(`${NOTE_PREFIX}.`);
  // Each key is `{ key, bytes }`; only keys under this run's prefix are ever asked for, and only those are deleted.
  for (const { key } of keys) if (key.startsWith(`${NOTE_PREFIX}.`)) await lamp.notesDelete(key);
  return keys.length;
}

// The held light of `hold_light` (its `stop()`), one at a time: this app has one lamp and one hand on it.
let held = null;

export const CONTROLS = [
  { id: 'gesture', group: 'Body', needs: ['body'], moves: true, ops: ['gesture'], shows: [], label: 'Gesture', fields: [select('name', GESTURES, 'nod')],
    run: async ({ lamp }, a) => { await lamp.gesture(a.name); return `lamp.gesture(${q(a.name)})`; } },
  { id: 'express', group: 'Body', needs: ['body', 'light'], moves: true, ops: ['express'], shows: [], label: 'Express', fields: [select('feeling', FEELINGS)],
    run: async ({ lamp }, a) => { await lamp.express(a.feeling); return `lamp.express(${q(a.feeling)})`; } },
  { id: 'look', group: 'Body', needs: ['body'], moves: true, ops: ['look', 'lease.take', 'lease.release'], shows: [], label: 'Look', fields: [number('yaw', -180, 180, 20), number('pitch', -90, 90, 0)],
    run: async ({ lamp, release }, a) => {
      const at = { yaw: a.yaw, pitch: a.pitch };
      await release();
      await lamp.withLease(['body'], () => lamp.look(at, { wait: true }));
      return `lamp.withLease(['body'], () => lamp.look(${q(at)}, { wait: true }))`;
    } },
  { id: 'move', group: 'Body', needs: ['body'], moves: true, ops: ['move', 'lease.take', 'lease.release'], shows: [], label: 'Move poses', fields: [],
    run: async ({ lamp, release }) => {
      const poses = [{ pitch: 12, yaw: -15 }, { pitch: -6, yaw: 15 }, { pitch: 8, yaw: 0 }];
      // A move needs the body lease at the door, as a look does.
      await release();
      await lamp.withLease(['body'], () => lamp.movePoses(poses, { wait: true }));
      return `lamp.withLease(['body'], () => lamp.movePoses(${q(poses)}, { wait: true }))`;
    } },
  // The head follows the hand the lamp sees until the lease runs out (`for_ms`) or Stop; the lease's end comes in the log.
  // No `shows` here: a `hand` event comes only while a hand is in view, which this row does not put there (the `hand` stimulus,
  // under Touch, does and claims it), so pressing this alone cannot be told to show one.
  { id: 'follow_hand', group: 'Body', needs: ['body', 'look'], moves: true, ops: ['look', 'lease.take', 'lease.release'], shows: [], label: 'Follow a hand', fields: [number('for_ms', 1000, 30_000, 8000, { integer: true })],
    run: async ({ lamp, release }, a) => {
      await release();
      await lamp.followHand({ forMs: a.for_ms }).start();
      return `lamp.followHand({ forMs: ${a.for_ms} }).start()`;
    } },
  { id: 'joints', group: 'Body', needs: ['body'], moves: true, ops: ['joints', 'lease.take', 'lease.release'], shows: [], label: 'Joints (yaw, pitch)', fields: [number('yaw', -180, 180, 10), number('pitch', -90, 90, 5), number('within_ms', 0, 5000, 1000, { integer: true })],
    run: async ({ lamp, release }, a) => {
      const targets = { yaw: a.yaw, pitch: a.pitch };
      await release();
      await lamp.withLease(['body'], (lease) => lamp.joints(lease, a.within_ms, targets, { wait: true }));
      return `lamp.withLease(['body'], (lease) => lamp.joints(lease, ${a.within_ms}, ${q(targets)}, { wait: true }))`;
    } },
  { id: 'stop', group: 'Body', ops: ['stop'], shows: [], label: 'Stop', fields: [], run: async ({ lamp }) => { await lamp.stop(); return 'lamp.stop()'; } },
  // The owner's language the lamp said in hello: a read, so the log says the code it got.
  { id: 'language', group: 'Voice', ops: [], shows: [], label: 'Language', fields: [], run: async ({ lamp }) => `lamp.language // ${q(lamp.language)}` },
  { id: 'light', group: 'Light', needs: ['light'], ops: ['light'], shows: [], label: 'Light',
    fields: [select('colour', COLOURS, 'warm'), number('level', 1, 5, 3, { integer: true }), select('pattern', ['', ...PATTERNS], '', { optional: true }), number('for_ms', 1, 30_000, 10_000, { integer: true })],
    run: async ({ lamp }, a) => {
      await lamp.light(a.colour, a.level, a.for_ms, a.pattern);
      return `lamp.light(${q(a.colour)}, ${a.level}, ${a.for_ms}${a.pattern ? `, ${q(a.pattern)}` : ''})`;
    } },
  // A computer's own colours on the ring, one per pixel: a few presets (the SDK takes any 32 `#rrggbb`).
  { id: 'light_pixels', group: 'Light', needs: ['light'], ops: ['light'], shows: [], label: 'Light pixels',
    fields: [select('preset', Object.keys(PIXEL_PRESETS), 'rainbow'), number('for_ms', 1, 30_000, 10_000, { integer: true })],
    run: async ({ lamp }, a) => {
      const count = lamp.ringPixels ?? 32;
      const pixels = Array.from({ length: count }, (_, k) => PIXEL_PRESETS[a.preset](k, count));
      await lamp.ring(pixels, { forMs: a.for_ms });
      return `lamp.ring(${q(a.preset)} // ${count} pixels, { forMs: ${a.for_ms} })`;
    } },
  { id: 'light_off', group: 'Light', needs: ['light'], ops: ['light'], shows: [], label: 'Light off', fields: [number('for_ms', 1, 30_000, 10_000, { integer: true })],
    run: async ({ lamp }, a) => { await lamp.lightOff({ forMs: a.for_ms }); return `lamp.lightOff({ forMs: ${a.for_ms} })`; } },
  // Notes: this computer's own small notes kept on the lamp (grant `notes`, no lease). The refusals (a bad key, a value over
  // 16 KiB, a second write at once) come back in the lamp's words.
  { id: 'notes_set', group: 'Notes', needs: ['notes'], ops: ['notes.set'], shows: [], label: 'Keep a note', fields: [text('key', 64, 'try'), text('value', 16_384, '{"hello": "world"}')],
    run: async (ctx, a) => {
      const { lamp } = ctx;
      let value;
      try { value = JSON.parse(a.value); } catch { throw refuse('value: JSON, such as {"hello": "world"} or "text" or 3'); }
      await lamp.notesSet(noteKey(ctx, a.key), value);
      return `lamp.notesSet(${q(noteKey(ctx, a.key))}, ${q(value)})`;
    } },
  { id: 'notes_get', group: 'Notes', needs: ['notes'], ops: ['notes.get'], shows: [], label: 'Read a note', fields: [text('key', 64, 'try')],
    run: async (ctx, a) => `lamp.notesGet(${q(noteKey(ctx, a.key))}) -> ${q((await ctx.lamp.notesGet(noteKey(ctx, a.key), null)) ?? null)}` },
  { id: 'notes_delete', group: 'Notes', needs: ['notes'], ops: ['notes.delete'], shows: [], label: 'Delete a note', fields: [text('key', 64, 'try')],
    run: async (ctx, a) => `lamp.notesDelete(${q(noteKey(ctx, a.key))}) -> ${q(await ctx.lamp.notesDelete(noteKey(ctx, a.key)))}` },
  { id: 'notes_list', group: 'Notes', needs: ['notes'], ops: ['notes.list'], shows: [], label: 'List the notes', fields: [text('prefix', 64, '', { optional: true })],
    run: async (ctx, a) => {
      const prefix = ctx.mode === 'real' ? `${NOTE_PREFIX}.${noteText(a.prefix ?? '')}` : (a.prefix ?? '');
      return `lamp.notesList(${q(prefix)}) -> ${q(await ctx.lamp.notesList(prefix))}`;
    } },
  // A light kept on past the 30 s one ask lasts: asked again at half the lamp's hold limit until stopped. One hold at a
  // time here (a new one ends the last); a later refusal ends it and is said on the server's log.
  { id: 'hold_light', group: 'Light', needs: ['light'], ops: ['light'], shows: [], label: 'Hold a light',
    fields: [select('colour', COLOURS, 'warm'), number('level', 1, 5, 2, { integer: true }), select('pattern', ['', ...PATTERNS], '', { optional: true })],
    run: async ({ lamp }, a) => {
      held?.stop();
      held = await lamp.holdLight(a.colour, a.level, a.pattern || undefined, { onError: (error) => { held = null; console.error(`try-it: the held light ended: ${error?.message ?? error}`); } });
      return `lamp.holdLight(${q(a.colour)}, ${a.level}${a.pattern ? `, ${q(a.pattern)}` : ''})`;
    } },
  { id: 'hold_light_stop', group: 'Light', ops: [], shows: [], label: 'Stop the held light', fields: [],
    run: async () => { const was = held !== null; held?.stop(); held = null; return `held.stop() (${was ? 'a held light ended' : 'none was held'})`; } },
  { id: 'say', group: 'Voice', needs: ['speak'], ops: ['say'], shows: ['lamp.said'], label: 'Say', fields: [text('text', 200, 'Hello, I am the virtual lamp.', { defaultReal: 'Hello from the try-it page.' })],
    hint: "spoken by your browser when 'Hear the lamp in this page' is on (the lamp's real voice sounds different; the stand-in voice may speak it too)", hintReal: "a real lamp says it through its own speaker",
    run: async ({ lamp }, a) => { await lamp.say(a.text); return `lamp.say(${q(a.text)})`; } },
  // The computer's own turn: the lamp's voice takes the prompt as if it were said to it; `done` follows, and
  // ending it gives the turn back (with none of this computer's, the lamp's own `busy` shows).
  { id: 'turn_start', group: 'Voice', needs: ['speak', 'hear'], ops: ['turn.start'], shows: [], label: 'Ask the lamp (start a turn)', fields: [text('prompt', 200, 'What can you do?')],
    hint: 'the virtual lamp has no model unless it runs with --real-voice, so otherwise it gives no spoken answer and the log says done; a real lamp answers aloud',
    hintReal: 'a real lamp answers aloud through its own voice; it may say lamp.changed when its turn starts',
    run: async ({ lamp }, a) => { await lamp.startTurn(a.prompt); return `lamp.startTurn(${q(a.prompt)})`; } },
  { id: 'turn_end', group: 'Voice', needs: ['speak'], ops: ['turn.end'], shows: [], label: 'End my turn', fields: [], run: async ({ lamp }) => { await lamp.endTurn(); return 'lamp.endTurn()'; } },
  { id: 'volume_to', group: 'Voice', needs: ['speak'], changes: "the lamp's volume", ops: ['volume'], shows: ['volume.changed'], hint: "sets the speaker volume to this level (0 to 100); it is not put back", hintReal: "sets the REAL lamp's volume to this level (0 to 100) and leaves it there", label: 'Volume to', fields: [number('to', 0, 100, 40, { integer: true })],
    run: async ({ lamp }, a) => answered(`lamp.volume({ to: ${a.to} })`, await lamp.volume({ to: a.to })) },
  { id: 'volume_step', group: 'Voice', needs: ['speak'], changes: "the lamp's volume", ops: ['volume'], shows: ['volume.changed'], hint: "steps the speaker volume once; it is not put back", hintReal: "steps the REAL lamp's volume once and leaves it there", label: 'Volume step', fields: [select('step', ['up', 'down'])],
    run: async ({ lamp }, a) => answered(`lamp.volume({ step: ${q(a.step)} })`, await lamp.volume({ step: a.step })) },
  { id: 'volume_mute', group: 'Voice', needs: ['speak'], changes: "the lamp's volume", ops: ['volume'], shows: [], hint: "mutes or unmutes the speaker; it is not put back", hintReal: "mutes or unmutes the REAL lamp's speaker and leaves it so", label: 'Mute', fields: [select('mute', ['on', 'off'], 'off')],
    run: async ({ lamp }, a) => answered(`lamp.volume({ mute: ${a.mute === 'on'} })`, await lamp.volume({ mute: a.mute === 'on' })) },
  // The person's own volume, so this row puts it back: it reads the level, takes the one step (or the mute), then
  // restores the level and the mute it found, even when the step is refused. Nothing is left changed.
  { id: 'volume', group: 'Voice', needs: ['speak'], ops: ['volume', 'get'], shows: ['volume.changed'], label: 'Volume (then back)',
    hint: 'steps the volume once and puts it back (level and mute)', fields: [select('how', ['step down', 'step up', 'mute'], 'step down')],
    run: async ({ lamp }, a) => {
      const before = (await lamp.get('volume')).volume;
      if (typeof before?.level !== 'number') throw refuse('the lamp did not say its volume (a virtual lamp from an older build does not): nothing was changed');
      const arg = a.how === 'mute' ? { mute: true } : { step: a.how === 'step up' ? 'up' : 'down' };
      let after;
      let back;
      try {
        after = await lamp.volume(arg);
      } finally {
        back = await lamp.volume({ to: before.level });
        if (before.muted) back = await lamp.volume({ mute: true });
        else if (back.muted) back = await lamp.volume({ mute: false });
      }
      return `lamp.volume(${q(arg)}) -> ${q(after)}; put back: ${q(back)} (was ${q({ level: before.level, muted: Boolean(before.muted) })})`;
    } },
  // Mute this computer's hearing and give it back: `false` holds the lamp's mute, `true` lifts it. Needs the hear grant.
  { id: 'listen', group: 'Voice', needs: ['hear'], ops: ['listen.set'], shows: [], label: 'Listening',
    hint: "the virtual lamp has no listening control: on is taken, off is refused here (a real lamp's off holds its mute until you press on)",
    hintReal: "off holds the real lamp's mute: it stays deaf to the room until you press on", fields: [select('listen', ['on', 'off'], 'on')],
    run: async ({ lamp }, a) => `lamp.setListening(${a.listen === 'on'}) -> ${q(await lamp.setListening(a.listen === 'on'))}` },
  // The lamp's own clip, through the voice: no lease (`play` is the recorded move, not a sound).
  { id: 'sound', group: 'Voice', needs: ['speak'], ops: ['sound'], shows: [], label: 'Sound (hum)', fields: [],
    hint: "a stand-in for the lamp's hum: this page plays its own quiet hum (the lamp's real hum is made by its voice)", hintReal: "a real lamp plays its own hum through its own speaker",
    run: async ({ lamp, audio }) => { await lamp.sound('hum'); return `lamp.sound('hum')${heard(audio?.('hum', humWav()), "a stand-in for the lamp's hum")}`; } },
  // Clips (grant `speak`): this computer's own, kept on the lamp. The rows touch ONLY this run's scratch clip: upload makes it,
  // the list shows this computer's clips (a read), remove takes away the one this run made and nothing else.
  { id: 'sound_upload', group: 'Voice', needs: ['speak'], ops: ['sound.upload'], shows: [], label: 'Keep a quiet clip', fields: [number('ms', 100, 2000, 300, { integer: true })],
    hint: 'this page plays the clip it keeps, its own audio', hintReal: 'a real lamp plays its clips through its own speaker',
    run: async ({ lamp, audio }, a) => {
      const clip = quietTone(a.ms);
      const kept = await lamp.uploadSound(SCRATCH_CLIP, clip);
      clipKept = true;
      return `lamp.uploadSound(${q(SCRATCH_CLIP)}, a ${a.ms} ms quiet tone) -> ${q(kept)}${heard(audio?.('clip', clip), 'the clip saved on the lamp')}`;
    } },
  { id: 'sound_list', group: 'Voice', needs: ['speak'], ops: ['sound.list'], shows: [], label: 'List my clips', fields: [],
    hint: 'clips named try-... are this app\'s test clips; remove them from the list',
    run: async ({ lamp }) => `lamp.sounds() -> ${q((await lamp.sounds()).map((c) => `${c.name} ${c.ms} ms${c.name === SCRATCH_CLIP ? ' (this run\'s)' : TRY_CLIP.test(c.name) ? ' (an earlier run\'s test clip)' : ''}`))}` },
  // Removes this run's clip, or one of this app's own earlier test clips named in `name` (`try-` and six hex digits): any
  // other name (`hum`, a clip of the person's) is refused here, before the lamp is asked.
  { id: 'sound_remove', group: 'Voice', needs: ['speak'], ops: ['sound.remove'], shows: [], label: 'Remove a test clip', fields: [text('name', 64, '', { optional: true })],
    hint: 'with no name, this run\'s clip; or a try-xxxxxx test clip from the list',
    run: async ({ lamp }, a) => {
      const name = a.name ?? SCRATCH_CLIP;
      if (!TRY_CLIP.test(name)) throw refuse(`name: only this app's test clips, try- and six hex digits (try-0a1b2c); nothing was asked of the lamp`);
      if (a.name === undefined && !clipKept) return `none of this run's clips is kept (nothing was removed; name an earlier test clip to remove it)`;
      const answer = await lamp.removeSound(name);
      if (name === SCRATCH_CLIP) clipKept = false;
      return `lamp.removeSound(${q(name)}) -> ${q(answer?.removed ?? answer)}`;
    } },
  // The camera and the microphone (their grants; one press, one use, nothing kept). The lamp's own rules hold: never while
  // it is private, the ring shows the mark, and the microphone only with one person in view (its refusal in its words).
  // A picture goes to the page that pressed, under the row, and is not logged; sound is only counted, never played or kept.
  { id: 'camera_snapshot', group: 'Camera and microphone', needs: ['camera'], ops: ['camera.snapshot'], shows: [], label: 'Camera: one picture', fields: [],
    run: async ({ lamp, show }) => {
      const jpeg = await lamp.cameraSnapshot();
      show?.(jpeg);
      return `lamp.cameraSnapshot() -> a JPEG of ${jpeg.length} bytes, on this page only`;
    } },
  { id: 'camera_stream', group: 'Camera and microphone', needs: ['camera'], ops: ['camera.stream'], shows: [], label: 'Camera: a short stream', fields: [number('seconds', 1, 10, 2, { integer: true }), number('fps', 1, 30, 2, { integer: true }), select('size', ['', '640x480', '1280x720'], '', { optional: true })],
    hint: "fps 1 to 30 (the lamp's limit). The virtual lamp's host camera gives about 6 pictures a second; size changes nothing here. seconds: how long this row keeps the stream on (at most 10)",
    hintReal: "fps 1 to 30 (the lamp's limit; its fast stream gives about 21 to 25). size: the fast stream's picture size. seconds: how long this row keeps the stream on (at most 10)",
    run: async ({ lamp, show }, a) => {
      let frames = 0;
      let last = null;
      let ended = null;
      const onFrame = (frame) => { frames += 1; last = frame.data ?? (frame.jpeg ? Buffer.from(frame.jpeg, 'base64') : last); };
      const onEnded = (frame) => { ended = frame?.why ?? 'ended'; };
      await lamp.on('camera.frame', onFrame);
      await lamp.on('camera.ended', onEnded);
      try {
        await lamp.cameraStream(true, a.fps, a.size || undefined);
        await new Promise((resolve) => setTimeout(resolve, a.seconds * 1000));
      } finally {
        await lamp.cameraStream(false).catch(() => {});
        lamp.off('camera.frame', onFrame);
        lamp.off('camera.ended', onEnded);
      }
      if (last) show?.(last);
      return `lamp.cameraStream(true, ${a.fps}${a.size ? `, ${q(a.size)}` : ''}) for ${a.seconds} s -> ${frames} frames${ended ? `, ended: ${ended}` : ''}; the last is on this page only`;
    } },
  { id: 'microphone_stream', group: 'Camera and microphone', needs: ['microphone'], ops: ['microphone.stream'], shows: [], label: 'Microphone: count the sound', fields: [number('seconds', 1, 10, 2, { integer: true })],
    run: async ({ lamp }, a) => {
      let blocks = 0;
      let bytes = 0;
      let lost = 0;
      let ended = null;
      const onAudio = (frame) => { blocks += 1; bytes += frame.data?.length ?? frame.bytes ?? 0; lost += frame.lost ?? 0; };
      const onEnded = (frame) => { ended = frame?.why ?? 'ended'; };
      await lamp.on('microphone.audio', onAudio);
      await lamp.on('microphone.ended', onEnded);
      try {
        await lamp.microphoneStream(true);
        await new Promise((resolve) => setTimeout(resolve, a.seconds * 1000));
      } finally {
        await lamp.microphoneStream(false).catch(() => {});
        lamp.off('microphone.audio', onAudio);
        lamp.off('microphone.ended', onEnded);
      }
      return `lamp.microphoneStream(true) for ${a.seconds} s -> ${blocks} blocks, ${bytes} bytes, ${lost} lost${ended ? `, ended: ${ended}` : ''} (counted, not played or kept)`;
    } },
  // The speaker (grant `speaker`, off unless the owner ticked it and the lamp serves it): a second of 440 Hz at -20 dBFS
  // through a speaker stream, in real time; the log shows its `speaker.stats` and `speaker.ended` events. Without the
  // grant, or while the lamp is private or muted, the lamp's refusal comes back in its words.
  { id: 'speaker_tone', group: 'Voice', needs: ['speaker'], ops: ['speaker.stream'], shows: [], label: 'Speaker tone (1 s of 440 Hz)', fields: [],
    run: async ({ lamp }) => {
      const stream = await lamp.speakerStream(16000);
      try {
        const samples = Buffer.alloc(16000 * 2);
        for (let n = 0; n < 16000; n += 1) samples.writeInt16LE(Math.round(3277 * Math.sin((2 * Math.PI * 440 * n) / 16000)), n * 2);
        const { ms, ended } = await stream.playPcm(samples);
        // Left open 2.5 s: the lamp says its `speaker.stats` (every 2 s) and then ends the idle stream (`speaker.ended`,
        // `idle`, after 2 s with no frame), so the log shows both.
        if (ended === null) await new Promise((resolve) => setTimeout(resolve, 2500));
        return `lamp.speakerStream(16000).playPcm(440 Hz, 1 s) // ${ended === null ? `${ms} ms sent` : `ended: ${ended}`}`;
      } finally {
        await stream.stop();
      }
    } },
  { id: 'skill_push', group: 'Skills', needs: ['skills'], ops: ['skill.push'], shows: [], label: 'Push the try-wave skill', fields: [],
    run: async ({ lamp }) => {
      const skill = await lamp.pushSkill(TRY_SKILL);
      skillPushed = true;
      return `lamp.pushSkill('skills/try-wave')  // ${skill.held ? `held: ${skill.held}` : skill.on ? 'on' : 'off'}`;
    } },
  { id: 'skills', group: 'Skills', needs: ['skills'], ops: ['skill.list'], shows: [], label: 'List my skills', fields: [],
    run: async ({ lamp }) => {
      const kept = await lamp.skills();
      return `lamp.skills()  // ${kept.map((k) => `${k.name} ${k.version}${k.on ? '' : ' (off)'}`).join(', ') || 'none'}`;
    } },
  { id: 'play', group: 'Skills', needs: ['body'], moves: true, ops: ['play', 'lease.take', 'lease.release'], shows: [], label: 'Play a recorded move', fields: [select('name', ['small-nod'])],
    hint: 'plays small-nod from the try-wave skill: push that skill first (the push needs the skills grant; the play needs the body alone)',
    run: async ({ lamp, release }, a) => {
      // `play` needs the body lease at the door, as a look does.
      await release();
      await lamp.withLease(['body'], () => lamp.play(a.name, { wait: true }));
      return `lamp.withLease(['body'], () => lamp.play(${q(a.name)}, { wait: true }))`;
    } },
  { id: 'skill_remove', group: 'Skills', needs: ['skills'], ops: ['skill.remove'], shows: [], label: 'Remove the try-wave skill', fields: [],
    run: async ({ lamp }) => { await lamp.removeSkill('try-wave'); skillPushed = false; return "lamp.removeSkill('try-wave')"; } },
  // What the lamp says of itself, by name (`lamp.get`), and the three named reads (the `air`, `device` and `body`
  // states): the answer is in the log; a `known: false` is the lamp having nothing to say yet.
  { id: 'get', group: 'Reads', ops: ['get'], shows: [], label: 'Read a state', fields: [select('state', [...schema.states], 'lamp')],
    run: async ({ lamp }, a) => answered(`lamp.get(${q(a.state)})`, await lamp.get(a.state)) },
  { id: 'air', group: 'Reads', needs: ['air'], ops: ['get'], shows: [], label: 'Air', fields: [], run: async ({ lamp }) => answered('lamp.air()', await lamp.air()) },
  { id: 'device', group: 'Reads', needs: ['device'], ops: ['get'], shows: [], label: 'Device', fields: [], run: async ({ lamp }) => answered('lamp.device()', await lamp.device()) },
  { id: 'body_info', group: 'Reads', needs: ['body'], ops: ['get'], shows: [], label: 'Body', fields: [], run: async ({ lamp }) => answered('lamp.bodyInfo()', await lamp.bodyInfo()) },
];

// The world, not the SDK: `lamp-sim <verb> ... DIR` writes what the lamp's stand-ins read. `argv` is an array,
// never a shell string; the lamp's folder is added last.
export const STIMULI = [
  { id: 'arrive', group: 'People', shows: ['person.arrived', 'person.facing'], label: 'Someone arrives', fields: [], argv: () => ['people', '1'] },
  { id: 'leave', group: 'People', shows: ['person.left'], label: 'Everyone leaves', fields: [], argv: () => ['people', '0'] },
  { id: 'people', group: 'People', shows: ['person.arrived'], label: 'Set people', fields: [number('n', 0, 5, 1, { integer: true })], argv: (a) => ['people', String(a.n)] },
  { id: 'hear', group: 'Sound', needs: ['hear'], shows: ['phrase.heard'], label: 'Hear it', fields: [text('text', 120, 'good night lamp')], argv: (a) => ['hear', a.text], listensFor: (a) => a.text },
  { id: 'sound', group: 'Sound', shows: ['sound.heard'], label: 'Make the sound', fields: [select('kind', ['music', 'laughter', 'door', 'footsteps'])], argv: (a) => ['sound', a.kind] },
  // A held hand is ONE pet (a real lamp tells one per stroke): a touch within `gapMs` of the last is the same stroke.
  { id: 'touch', group: 'Touch', shows: ['touch'], label: 'Touch', fields: [select('kind', ['tap', 'pet'])], argv: (a) => ['touch', a.kind], gapMs: 800 },
  { id: 'hand', group: 'Touch', shows: ['hand'], label: 'A hand', fields: [], argv: () => ['hand'] },
  { id: 'air', group: 'Air', shows: ['air.changed'], label: 'Air', fields: [select('kind', ['stuffy', 'clear'])],
    argv: (a) => ['air', ...(a.kind === 'stuffy' ? ['co2_ppm=1600', 'pm2_5=45'] : ['co2_ppm=450', 'pm2_5=5'])] },
];

// What the page needs to draw the buttons: the entries without their code.
// `grants` (the lamp's, from hello) and `flags` (`{ camera, microphone }`, the run's, only in real mode) make a row's
// `disabled`: why the lamp or the run cannot do it. Without `grants` the table is static, as before. In real mode there
// are no stimuli (they are lamp-sim verbs).
export function describe(mode = 'lamp-sim', { grants, flags } = {}) {
  const why = (entry) => {
    if (!grants) return undefined;
    const missing = (entry.needs ?? []).filter((g) => !grants.includes(g));
    if (missing.length) return `the lamp did not grant ${missing.join(', ')}`;
    const off = (entry.needs ?? []).filter((g) => flags && g in flags && !flags[g]);
    return off.length ? `start the run with ${off.map((g) => `--${g}`).join(' and ')}` : undefined;
  };
  const shown = (entry) => {
    const { id, group, label, fields, hint, hintReal, needs, moves, changes, shows } = entry;
    const text = (mode === 'real' && hintReal) || hint;
    const disabled = why(entry);
    // A field that says another default for a real lamp (`defaultReal`) shows that one there, and nothing of it otherwise.
    const shownFields = fields.map(({ defaultReal, ...field }) => (mode === 'real' && defaultReal !== undefined ? { ...field, default: defaultReal } : field));
    return { id, group, label, fields: shownFields, ...(needs?.length ? { needs } : {}), ...(moves ? { moves: true } : {}), ...(changes ? { changes } : {}), ...(shows?.length ? { shows } : {}), ...(text ? { hint: text } : {}), ...(disabled ? { disabled } : {}) };
  };
  return { controls: CONTROLS.map(shown), stimuli: mode === 'real' ? [] : STIMULI.map(shown) };
}

// Whether the event log shows this event now: every line the lamp says (lamp.said) is shown, however fast they come
// (a dropped line would be a line the page never saw), and so is every `done` (one frame per request, never a stream: a
// refusing one, how dropped why "refused: the lamp is speaking", thinned away behind another `done` read as a PASS on lamp-75c9);
// the others are thinned to one in 200 ms per event name.
export function logIt(event, now, quiet) {
  if (event === 'lamp.said' || event === 'done') return true;
  if (now - (quiet.get(event) ?? 0) < 200) return false;
  quiet.set(event, now);
  return true;
}

export const findControl = (id) => CONTROLS.find((c) => c.id === id);
export const findStimulus = (id) => STIMULI.find((s) => s.id === id);
