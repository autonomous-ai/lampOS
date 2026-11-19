// The lamp's digital twin: a page that draws the lamp in 3D from its model
// and moves with `body.state`, the ring in the light's colour; dragging a
// joint's slider sends `joints` under a body lease, and the real lamp follows.
//
//   node examples/twin/server.js            # the paired lamp (LAMPOS_HOME)
//   node examples/twin/server.js --fake     # no lamp: a simulated body
//   open the URL it prints: http://localhost:8737/?t=TOKEN
//
// Options: --port N (8737), --ring-file PATH (see below), --glb PATH (the repository's rigged CAD,
// robots/lamp/hardware/cad/glb/lamp.glb). A slider is a joint's degrees from
// its home, as `body.state` and `get('body')` count them and `joints` takes
// them; today the lamp takes `yaw` and `pitch`, and refuses `roll`,
// `shoulder` and `elbow` until the body's own goal stream (control.md).
//
// The page is served on this computer only (127.0.0.1), and only with the
// token in the URL it prints: any website open in the browser could otherwise
// reach localhost and drive the lamp (guard.js). The lamp's door is spoken to
// through the SDK, so the page never holds the computer's key.
import { execFile } from 'node:child_process';
import { createServer } from 'node:http';
import { networkInterfaces } from 'node:os';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { WebSocketServer } from 'ws';
import { Keyring, Lamp, LampError, SPEAKER_EVENTS, model, schema } from 'lampos';
import { FakeLamp } from 'lampos/testing';
import { limitsOf } from './reach.js';
import { linked } from './link.js';
import { checked, cleanScratch, cleanScratchNotes, cleanScratchSkill, describe, findControl, findStimulus, logIt, stopRainbow } from './controls.js';
import { endHandler, watchParent } from './twin-end.js';
import { chooseLan, hostOk, newToken, originOk, parseUrl, tokenOk } from './guard.js';
import { loadReachMap, rangesDeg, restrict, restrictAll } from './reach-guard.js';

const here = (path) => fileURLToPath(new URL(path, import.meta.url));
const options = parse(process.argv.slice(2));
const port = Number(options.port ?? 8737);
// `--reach-map PATH`: the page checks every goal for the arm and the head against this reach map (tools/reach, `query.py --export`;
// examples/twin/reach-map-4ace.json is one) before it sends it. The page only: the lamp's own limits are unchanged. `--reach-shrink N`
// takes N counts (default 48: the 3D grid overshot the exact march by 40 at the stand) off each end of what the map allows.
// `--reach-map A.json,B.json` loads several maps (the first is the one in use; the page can switch, each says what masses it was judged with).
const reachMaps = options['reach-map'] ? options['reach-map'].split(',').map((path) => ({ map: loadReachMap(path), name: path.split('/').pop().replace(/\.json$/, '') })) : null;
let reachAt = 0;
const reachMapNow = () => reachMaps?.[reachAt]?.map ?? null;
const reachShrink = Number(options['reach-shrink'] ?? 48);
const token = newToken();
// `--lan`: the page is served on this computer's network address too (not only 127.0.0.1), still only with the
// token in the URL. `--stim-dir DIR` (with `--lamp-sim PATH`) is the try-it app: a panel that drives the lamp
// through this SDK connection, stimuli buttons that run `lamp-sim <verb> ... DIR`, and a log of the lamp's events.
const lan = Boolean(options.lan);
const stimDir = options['stim-dir'];
// `--real`: a real lamp, paired in the keyring that `LAMP_TRY_KEYRING` names (set by try.js, read once here, never printed):
// the same panel with no stimuli (they are lamp-sim verbs), a banner, and rows that say why they cannot run. `--camera` /
// `--microphone` are the person's yes to those rows, as in try.js.
const real = Boolean(options.real);
const tryPanel = Boolean(stimDir || real);
const mode = real ? 'real' : options.fake ? 'fake' : 'lamp-sim';
// Only try.js gives it; started alone, this would fall to the SDK's default keyring (the person's own pairings): refused.
if (real && !process.env.LAMP_TRY_KEYRING) {
  console.error('try: a real run takes its keyring from try.js --real (LAMP_TRY_KEYRING): start it with try.js --real');
  process.exit(2);
}
const realKeyring = real ? new Keyring(process.env.LAMP_TRY_KEYRING) : undefined;
delete process.env.LAMP_TRY_KEYRING;
const lampSimBin = options['lamp-sim'] ?? 'lamp-sim';
const glbPath = options.glb ?? here('../../../../../robots/lamp/hardware/cad/glb/lamp.glb');
if (!existsSync(glbPath) || readFileSync(glbPath).toString('latin1', 0, 4) !== 'glTF') {
  console.error(`${glbPath}: no rigged CAD here (a clone needs \`git lfs pull\`); give --glb PATH`);
  process.exit(2);
}
const NAMES = model.joints.map((joint) => joint.name);
const DEG_A_COUNT = 360 / 4096;

// The one word the page server says of the lamp's connection, to its own terminal and to every page once `tell` exists (below).
let announce = (text) => console.error(`try-it: ${text}`);
// A real lamp's connection is kept (link.js): it ends with a reason said, is made again, and what the page listens to is listened to again.
const { lamp, sim, fakeLamp } = options.fake ? await fakeBody() : real
  ? { lamp: await linked(() => Lamp.connect(undefined, { keyring: realKeyring }), { say: (text) => announce(text), onUp: () => reconnected(), onEnd: () => process.kill(process.pid, 'SIGTERM') }) }
  : { lamp: await Lamp.connect() };
const runFlags = real ? { camera: Boolean(options.camera), microphone: Boolean(options.microphone) } : undefined;
const realHost = real ? realKeyring.pairing()?.host ?? '' : '';
const pages = new Set();
const tell = (message) => {
  const text = JSON.stringify(message);
  for (const page of pages) page.send(text);
};
let last = null;
announce = (text) => { console.error(`try-it: ${text}`); tell({ type: 'said', text }); };
// `--ring-file PATH`: the virtual desktop lamp's ring (lamp-sim's DIR/ring.json: `on`, `colour`, `level`, 32 `#rrggbb` pixels as drawn
// at that moment, and the `pattern` while one moves), put in the light's `shows` for what the lamp does not say itself: the twin then
// draws the ring pixel by pixel, moving. It reads a file beside the lamp, not the lamp's door; it goes with the field it stands in
// for. lamp-sim rewrites the file as the pattern steps (about 25 times a second); a change is read within 100 ms and pushed to the
// pages as a state of its own, so the picture moves between the lamp's own states.
const ringFile = options['ring-file'];
const ringShows = () => {
  try {
    const ring = JSON.parse(readFileSync(ringFile, 'utf8'));
    if (!Array.isArray(ring.pixels)) return null;
    return { pixels: ring.pixels, ...(typeof ring.on === 'boolean' ? { on: ring.on } : {}), ...(typeof ring.colour === 'string' ? { colour: ring.colour } : {}), ...(Number.isInteger(ring.level) ? { level: ring.level } : {}), ...(typeof ring.pattern === 'string' ? { pattern: ring.pattern } : {}) };
  } catch { return null; /* no ring file yet, or half written: the one colour shows */ }
};
// With a reach map, every state carries what the map allows each joint now, for the page to draw beside the sliders.
const withReach = (state) => (reachMaps ? { ...state, reach: { on: true, virtual: options.fake === true, shrink: reachShrink, name: reachMaps[reachAt].name, label: reachMaps[reachAt].map.label ?? '', choices: reachMaps.map((m) => ({ name: m.name, label: m.map.label ?? m.name })), ranges: rangesDeg(reachMapNow(), state.joints, { shrink: reachShrink }) } } : state);
const withRing = (state) => withReach(withRingShows(state));
const withRingShows = (state) => {
  if (!ringFile) return state;
  const ring = ringShows();
  if (!ring) return state;
  const shows = state.lamp?.light?.shows ?? {};
  // What the lamp says itself stays; the ring's file fills only what is missing.
  return { ...state, lamp: { ...state.lamp, light: { ...state.lamp?.light, shows: { ...ring, ...shows } } } };
};
let lastRaw = null;
// The body's state is drawn only when the pairing holds the body grant: the lamp refuses `body.state` without it (`no_grant`), and a
// real run paired for speech alone must still start, with the twin drawn at rest. (It used to die here with a stack.)
const bodyGrant = lamp.grants.includes('body');
if (bodyGrant) {
  await lamp.on('body.state', (raw) => {
    lastRaw = raw;
    const state = withRing(raw);
    last = state;
    tell({ type: 'state', state });
  });
}
if (ringFile) {
  let before = '';
  setInterval(() => {
    let text = '';
    try { text = readFileSync(ringFile, 'utf8'); } catch { return; }
    if (text === before) return;
    before = text;
    if (!lastRaw) return;
    last = withRing(lastRaw);
    tell({ type: 'state', state: last });
  }, 100).unref();
}
// What the body will do with each joint (`get body`'s min_deg / max_deg), read at the start and again when it changes (standing or
// folded moves the head's reach): the page draws the reach and warns past it, and never blocks (reach.js). A lamp that says none: none.
let limits = null;
async function readLimits() {
  if (!bodyGrant) return;
  try {
    const next = limitsOf(await lamp.bodyInfo());
    if (JSON.stringify(next) === JSON.stringify(limits)) return;
    limits = next;
    tell({ type: 'limits', joints: next });
  } catch { /* an older lamp, or the body not heard from yet: no band, as before */ }
}
if (bodyGrant) {
  readLimits();
  setInterval(readLimits, 5_000).unref();
}
console.log(bodyGrant ? `body.state subscribed on ${options.fake ? 'a simulated body' : 'the paired lamp'}` : 'no body grant: the twin draws the lamp at rest and its body rows are drawn off');

// One lease at a time, taken at the first drag and given back HOLD_MS after the last (the lease is taken for 30 s; the lamp's own pose
// comes back when it is given back, so a pose set with a slider stays this long). A `let_go` message gives it back at once.
const HOLD_MS = 25_000;
let lease = null;
let letGo = null;
// The page's STOP (`stop_all`): from then on no ask goes to the lamp until a page says `resume`.
let halted = false;
// The lamp ended a connection and this one is new: the lamp ended the leases of the old one (docs/control.md), so none is held, and the
// limits are asked again.
function reconnected() {
  clearTimeout(letGo);
  const heldBefore = lease !== null;
  lease = null;
  readLimits();
  // A STOP's hold is the old connection's lease: with it gone the body is the lamp's again (its arm eases back to the stand), said at once.
  if (halted && heldBefore) announce('the lamp\'s connection was made again while STOPPED: the hold ended with the old connection, the body is the lamp\'s now');
}
async function drag(joint, deg, final = false) {
  if (halted || !NAMES.includes(joint) || !Number.isFinite(deg)) return;
  try {
    if (!lease) {
      lease = lamp.lease('body');
      await lease.take();
      tell({ type: 'said', text: `holding the body (lease ${lease.id})` });
    }
    if (halted) return; // a STOP came while the lease was being taken: nothing goes out under it
    clearTimeout(letGo);
    letGo = setTimeout(release, HOLD_MS);
    let target = Math.max(-180, Math.min(180, deg));
    if (reachMapNow()) {
      const checked = restrict(reachMapNow(), lastRaw?.joints, joint, target, { shrink: reachShrink });
      if (checked.note) tell({ type: 'said', text: `reach map: ${checked.note}` });
      target = checked.deg;
    }
    // The virtual lamp's own arm (and roll) is this page's simulation: the lamp's `joints` op takes only yaw and pitch today.
    if (options.fake && !['yaw', 'pitch'].includes(joint)) { sim?.aim(joint, target); return; }
    // The lamp's answer to the move, said to the pages (the body's `done`: reached, `clamped`, or `dropped` with its why): the slider
    // only asks, and the body may cut or refuse what it asks (roll is held to a count either way on a product lamp). Only the page's
    // last ask of a drag (`final`, the slider let go) waits for it, so a drag leaves no wait behind per step; a lamp that sends no `done`
    // is not waited on for more than 6 s.
    if (halted) return;
    const mine = (asks[joint] = (asks[joint] ?? 0) + 1);
    const pending = lamp.joints(lease, 300, { [joint]: Number(target.toFixed(1)) }, { wait: final });
    sim?.aim(joint, target);
    const done = await (final ? Promise.race([pending, new Promise((resolve) => setTimeout(resolve, 6_000).unref())]) : pending);
    if (final && done?.how && mine === asks[joint]) {
      tell({ type: 'joint_done', joint, how: done.how, why: done.why, clamped: done.clamped === true, ...(typeof done[joint] === 'number' ? { [joint]: done[joint] } : {}) });
    }
  } catch (error) {
    if (!(error instanceof LampError)) throw error;
    tell({ type: 'said', text: `refused: ${error.code}: ${error.message}` });
    if (error.code === 'no_lease') lease = null;
  }
}
const asks = {};
// STOP, from the page's big button: at once no more asks are sent from this page, the rainbow ends and the client's own actions end
// (`lamp.stop()`: its light, a hand follow, a say), and the arm and head are HELD where they are now: the body lease is kept (the SDK
// renews it; the body's own cap is 10 minutes) and one last ask names the present pose of the shoulder, the elbow and the pitch, so the
// body holds them there instead of finishing the move it was on. Letting the lease go is a second, deliberate press (`give_back`): the
// body then keeps torque on and eases the arm and head back to the stand (lease.rs `Lease::end`, body.rs `GESTURE_STEP` 21 counts a 20 ms
// step = up to about 1,050 counts a second at the follower; the servo's own bound is 800 to 1,000 counts a second in the product unit,
// so up to about 70 to 90 degrees a second, from wherever the arm is). STOP NEVER TAKES THE BODY: it holds only what this page held (the
// lease it had, or one `lamp.stop()` ended with its own follow, taken again once); a page with no lease of its own ends its sends and
// actions and leaves the body to the lamp, else a stopped web page would stiffen the lamp for the lease's 10 minutes. Each step is bounded and failures are said, never thrown.
async function stopAll() {
  halted = true;
  clearTimeout(letGo);
  tell({ type: 'stopped', text: 'STOP: this page sends nothing more to the lamp; holding the arm where it is now…' });
  const bound = (what, work) => Promise.race([
    work.then(() => null, (error) => `${what}: ${error?.message ?? error}`),
    new Promise((resolve) => setTimeout(() => resolve(`${what}: no answer in 3 s`), 3_000).unref()),
  ]);
  const problems = (await Promise.all([bound('rainbow', stopRainbow()), bound('lamp.stop', lamp.stop())])).filter(Boolean);
  let held = 'the body lease is kept but the present pose is not known, so the last ask stands';
  const here = Object.fromEntries(['shoulder', 'elbow', 'pitch'].map((name) => [name, lastRaw?.joints?.[name]?.deg]).filter(([, deg]) => Number.isFinite(deg)));
  if (lease && Object.keys(here).length) {
    const asked = Object.fromEntries(Object.entries(here).map(([name, deg]) => [name, Number(deg.toFixed(1))]));
    let error = await bound('hold', lamp.joints(lease, 300, asked));
    // `lamp.stop()` may have ended the lease with the follow it ends: take it again once, so the hold is not lost to that.
    if (error && /no_lease|lease/.test(error)) {
      try { lease = lamp.lease('body'); await lease.take(); error = await bound('hold', lamp.joints(lease, 300, asked)); } catch (again) { lease = null; error = `body lease: ${again?.message ?? again}`; }
    }
    if (error && lease && asked.pitch !== undefined) { error = await bound('hold', lamp.joints(lease, 300, { pitch: asked.pitch })); if (!error) held = 'only the head is held (the lamp refused the arm\'s hold)'; }
    if (!error && held.startsWith('the body')) held = `held at ${Object.entries(asked).map(([name, deg]) => `${name} ${deg}`).join(', ')}`;
    if (error) problems.push(error);
  } else if (!lease) held = 'this page held no body lease, so the body is left to the lamp';
  tell({ type: 'stopped', done: true, hold: Boolean(lease), text: `STOPPED (${held}).${problems.length ? ` But: ${problems.join('; ')}.` : ''}${lease ? ' Letting go is a second press: the arm then eases back to its stand at the body\'s own pace, up to about 70 to 90 degrees a second.' : ''}` });
}
// The second press: the body lease is given back; the body eases the arm and head back to the stand (not held, not limp).
async function giveBack() {
  halted = true;
  await release();
  tell({ type: 'stopped', done: true, hold: false, text: 'LET GO: the body is given back; the arm and head are easing back to the stand now.' });
}
async function release() {
  // A following the panel started (a row that follows the hand) holds the body lease too: let it go, so every row
  // that needs the body (a look, a move) finds it free.
  await lamp.following?.stop().catch(() => {});
  const held = lease;
  lease = null;
  if (!held) return;
  try {
    await held.release();
    tell({ type: 'said', text: 'the body given back' });
  } catch (error) {
    if (!(error instanceof LampError)) throw error;
  }
}

// The try-it app (`--stim-dir`): every button is one SDK call (CONTROLS) or one `lamp-sim` stimulus (STIMULI), both
// in controls.js, and the log says the call and the lamp's own answer, a refusal in its words.
const refuse = (why) => new LampError(why, { code: 'bad_request' });
const said = (id, call, ok, answer) => tell({ type: 'result', id, call, ok, answer });
// Sound for the listener (the try-it page's "Hear the lamp in this page"): a page that turned it on (`page.hear`, off on every
// connection, never remembered) gets the WAV a row made for it, as one message to that page alone as a picture goes, never
// logged and never kept. It is this app making the sound, not the lamp's voice reaching the browser. Never the microphone's
// sound: no `microphone.audio` frame is read here (the stream row only counts), so none can reach a page.
const hearing = (page, message) => {
  const on = message.on === true;
  page.hear = on;
  page.send(JSON.stringify({ type: 'hear', on, language: lamp.info?.language ?? lamp.language }));
};
const audioTo = (page, id) => (what, wav) => {
  if (!page?.hear || page.readyState !== 1) return 0;
  page.send(JSON.stringify({ type: 'audio', id, what, wav: Buffer.from(wav).toString('base64') }));
  return wav.length;
};
let stimulating = Promise.resolve();
const lastStimulus = new Map();
async function press(message, page) {
  const { id, name, args = {} } = message;
  try {
    if (message.type === 'call') {
      const control = findControl(name);
      if (!control) throw refuse(`no button ${name}`);
      if (halted && control.id !== 'stop') throw refuse('stopped: press Resume on the red bar first');
      // A row the page draws off (not granted, or without --camera/--microphone on a real lamp) is refused here too, on a raw call;
      // and a row that moves the real lamp waits for this page's yes to "this moves the real lamp".
      const off = describe(mode, { grants: lamp.grants, flags: runFlags }).controls.find((c) => c.id === control.id)?.disabled;
      if (off) throw refuse(`n/a: ${off}`);
      if (real && control.moves && !page?.movesOk) throw refuse('this moves the real lamp: say yes to "this moves the real lamp" on the page first');
      // A picture goes to the page that pressed, as one message to it: not through `tell`, never logged, never kept.
      const show = (jpeg) => page?.send(JSON.stringify({ type: 'picture', id, jpeg: Buffer.from(jpeg).toString('base64') }));
      const call = await control.run({ lamp, release, listenFor, show, mode, audio: audioTo(page, id), reach: reachMaps && ((targets) => { const r = restrictAll(reachMapNow(), lastRaw?.joints, targets, { shrink: reachShrink }); for (const note of r.notes) tell({ type: 'said', text: `reach map: ${note}` }); return r.targets; }) }, checked(control.fields, args, mode));
      said(id, call, true, 'taken');
    } else {
      const stimulus = real ? undefined : findStimulus(name);
      if (!stimulus) throw refuse(real ? 'no stimuli on a real lamp: they are lamp-sim verbs' : `no stimulus ${name}`);
      const values = checked(stimulus.fields, args);
      const argv = [...stimulus.argv(values), stimDir];
      // The same stroke (a held hand, a repeated press) is one touch: nothing is forked for the repeats.
      const now = Date.now();
      if (stimulus.gapMs && now - (lastStimulus.get(stimulus.id) ?? 0) < stimulus.gapMs) {
        said(id, `lamp-sim ${argv.slice(0, -1).join(' ')}`, true, `ignored: the same stroke (one ${stimulus.id} per ${stimulus.gapMs / 1000} s)`);
        return;
      }
      lastStimulus.set(stimulus.id, now);
      if (stimulus.listensFor) await listenFor(stimulus.listensFor(values));
      const [verb, ...rest] = argv;
      // One stimulus at a time: a button pressed often cannot fork lamp-sim without end.
      const run = stimulating.then(() => new Promise((resolve, reject) => execFile(lampSimBin, argv, { timeout: 10_000 }, (error, stdout, stderr) => (error ? reject(new Error((stderr || error.message).trim().split('\n')[0])) : resolve(stdout)))));
      stimulating = run.catch(() => {});
      await run;
      said(id, `lamp-sim ${verb} ${rest.slice(0, -1).join(' ')}`.trim(), true, 'done');
    }
  } catch (error) {
    // The SDK's refusals already say their code first ("bad_request: ...").
    const why = error instanceof LampError ? (error.message.startsWith(`${error.code}:`) ? error.message : `${error.code}: ${error.message}`) : `failed: ${error.message}`;
    said(id, name, false, why);
  }
}
// The lamp's events, said in the log (not `body.state`: twelve a second, and the twin draws it; `time`: noise);
// a busy kind (a hand, a sound) at most five times a second.
const quiet = new Map();
// A phrase the lamp is to hear is asked of it first (`phrase.heard` comes only for phrases a program listed).
const listening = new Set();
async function listenFor(phrase) {
  const key = phrase.trim().toLowerCase();
  if (listening.has(key)) return;
  await lamp.on('phrase.heard', (frame) => tell({ type: 'event', event: 'phrase.heard', frame: { phrase: frame.phrase, ...(frame.confidence === undefined ? {} : { confidence: frame.confidence }) } }), { phrases: [phrase] });
  listening.add(key);
}
if (tryPanel) {
  if (stimDir) listenFor('good night lamp').catch((error) => console.log(`not listening for a phrase: ${error.message}`));
  for (const event of [...schema.events.filter((e) => e !== 'body.state' && e !== 'time' && e !== 'phrase.heard'), 'lease', 'done', ...SPEAKER_EVENTS]) {
    lamp.on(event, (frame) => {
      if (!logIt(event, Date.now(), quiet)) return;
      const { ev: _ev, at: _at, ...rest } = frame ?? {};
      tell({ type: 'event', event, frame: rest });
    }).catch((error) => console.log(`not subscribed to ${event}: ${error.code ?? ''} ${error.message}`));
  }
}

const files = {
  '/': ['text/html; charset=utf-8', () => readFileSync(here('./index.html'))],
  '/lamp.glb': ['model/gltf-binary', () => readFileSync(glbPath)],
  '/model.json': ['application/json', () => JSON.stringify(model)],
  '/reach.js': ['text/javascript; charset=utf-8', () => readFileSync(here('./reach.js'))],
  // `lamp` and `mode` name the checklist's key: the lamp's own id and name from hello, and what this page is driving; `grants`
  // are hello's; each row says if it is `disabled` (describe). `camera`/`microphone` say the picture or the sound may show.
  ...(tryPanel ? { '/try.json': ['application/json', () => JSON.stringify({
    ...describe(mode, { grants: lamp.grants, flags: runFlags }),
    camera: lamp.grants.includes('camera') && (!real || Boolean(options.camera)),
    microphone: lamp.grants.includes('microphone') && (!real || Boolean(options.microphone)),
    lamp: { id: String(lamp.info?.id ?? 'lamp'), name: String(lamp.info?.name ?? '') }, mode, grants: [...lamp.grants], host: realHost,
  })] } : {}),
  ...(tryPanel ? { '/panel.js': ['text/javascript; charset=utf-8', () => readFileSync(here('./panel.js'))] } : {}),
  ...(tryPanel ? { '/checklist.js': ['text/javascript; charset=utf-8', () => readFileSync(here('./checklist.js'))] } : {}),
};
// three.js, vendored (examples/twin/vendor/three, three@0.160.0, MIT), so the page works with no internet.
// A page's import map cannot carry the `?t=` token, so exactly these files are served without it, from this
// fixed list (never a path built from the request): library code only, nothing of the lamp's.
const vendored = {
  '/vendor/three/three.module.min.js': 'text/javascript; charset=utf-8',
  '/vendor/three/jsm/loaders/GLTFLoader.js': 'text/javascript; charset=utf-8',
  '/vendor/three/jsm/controls/OrbitControls.js': 'text/javascript; charset=utf-8',
  '/vendor/three/jsm/utils/BufferGeometryUtils.js': 'text/javascript; charset=utf-8',
  '/vendor/three/LICENSE': 'text/plain; charset=utf-8',
};
// What the lamp's camera sees now, for the page's "what the lamp sees" panel: the lamp's own newest
// picture through the SDK (the `camera` grant), so the lamp's rules hold (never while private, the ring's
// mark shown). One at a time, and the last one again if asked within 700 ms.
let seeing = null;
let seen = { at: 0, jpeg: null };
function snapshot() {
  if (Date.now() - seen.at < 700 && seen.jpeg) return Promise.resolve(seen.jpeg);
  seeing ??= lamp.cameraSnapshot().then((jpeg) => { seen = { at: Date.now(), jpeg }; return jpeg; }).finally(() => { seeing = null; });
  return seeing;
}

// With --lan the one network address the twin prints; a Host or an Origin is accepted only if it is that or loopback.
const lanChoice = lan ? chooseLan(networkInterfaces(), options['lan-address']) : undefined;
if (options['lan-address'] !== undefined && !lan) {
  console.error('--lan-address goes with --lan');
  process.exit(2);
}
if (lanChoice?.error && options['lan-address'] !== undefined) {
  console.error(`${lanChoice.error}; this computer's addresses: ${lanChoice.own.map(({ name, address }) => `${address} (${name})`).join(', ') || 'none'}`);
  process.exit(2);
}
const lanIp = lanChoice?.address;
const lanHost = lanIp ? `${lanIp}:${port}` : undefined;
let cameraAskedAt = 0;
// A page that works for seconds between two requests (it parses a 22 MB lamp model, slowly on a loaded computer) reuses its
// connection; Node closes an idle one after 5 s (6 s on Node 24), and a request that races that close is aborted in the browser
// ("Failed to fetch", net::ERR_ABORTED, no status: the buttons' import was lost that way). The twin keeps an idle connection
// open for two minutes instead (the headers timeout above it, as Node asks).
const IDLE_MS = 120_000;
const server = createServer((request, response) => {
  if (lan && !hostOk(request.headers.host, port, lanHost)) {
    response.writeHead(403).end('not the address the twin printed\n');
    return;
  }
  // The target parsed once, here: one that is not a URL is a bad request (never a throw), before anything else.
  const parsed = parseUrl(request.url);
  if (!parsed) {
    response.writeHead(400).end('bad request\n');
    return;
  }
  const pathname = parsed.pathname;
  if (Object.hasOwn(vendored, pathname) && (request.method === 'GET' || request.method === 'HEAD')) {
    response.writeHead(200, { 'content-type': vendored[pathname], 'cache-control': 'public, max-age=3600' }).end(readFileSync(here(`.${pathname}`)));
    return;
  }
  if (!tokenOk(request.url, token)) {
    response.writeHead(403).end('the twin needs the URL it printed, with its token\n');
    return;
  }
  if (pathname === '/camera.jpg' && real && !options.camera) {
    response.writeHead(403, { 'x-reason': encodeURIComponent('start the run with --camera') }).end();
    return;
  }
  if (pathname === '/camera.jpg') {
    // On the network the room's picture is given only while a page that has the camera on says so (a token alone,
    // with no page showing the camera, gets none).
    if (lan && Date.now() - cameraAskedAt > 3_000) {
      response.writeHead(403, { 'x-reason': encodeURIComponent('the camera is shown only while a page has it on') }).end();
      return;
    }
    snapshot().then(
      (jpeg) => response.writeHead(200, { 'content-type': 'image/jpeg', 'cache-control': 'no-store' }).end(jpeg),
      (why) => response.writeHead(503, { 'content-type': 'text/plain', 'x-reason': encodeURIComponent(String(why.message ?? why)) }).end(),
    );
    return;
  }
  const file = files[pathname];
  if (!file) {
    response.writeHead(404).end();
    return;
  }
  response.writeHead(200, { 'content-type': file[0], 'cache-control': 'no-store', 'referrer-policy': 'no-referrer' }).end(file[1]());
});
// A request the HTTP parser itself refuses (a broken line, a header too long) ends that one connection and nothing
// else; on both servers, the loopback one and the network address's.
const dropBadClient = (error, socket) => {
  if (socket.writable) socket.end('HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n');
  socket.destroy();
};
server.on('clientError', dropBadClient);
const wss = new WebSocketServer({ noServer: true, maxPayload: 16 * 1024 });
server.on('upgrade', (request, socket, head) => {
  const path = parseUrl(request.url)?.pathname;
  if (path !== '/live' || !tokenOk(request.url, token) || !originOk(request.headers.origin, port, lanHost) || (lan && !hostOk(request.headers.host, port, lanHost))) {
    socket.end('HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n');
    return;
  }
  wss.handleUpgrade(request, socket, head, (page) => wss.emit('connection', page, request));
});
wss.on('connection', (page) => {
  pages.add(page);
  // A frame over the cap (or a broken one) closes that page's socket; it must not end the twin.
  page.on('error', () => page.terminate());
  if (last) page.send(JSON.stringify({ type: 'state', state: last }));
  if (limits) page.send(JSON.stringify({ type: 'limits', joints: limits }));
  page.on('message', (data) => {
    let message;
    try {
      message = JSON.parse(data);
    } catch {
      return;
    }
    // On a real lamp the sliders move the real body only after the page said yes to "this moves the real lamp", and with the grant.
    if (message.type === 'moves_ok' && real) page.movesOk = true;
    // The page's choice among the loaded reach maps (which masses the limits are judged with): for every page, the next state carries it.
    if (message.type === 'reach_map' && reachMaps) {
      const at = reachMaps.findIndex((m) => m.name === message.name);
      if (at >= 0 && at !== reachAt) {
        reachAt = at;
        tell({ type: 'said', text: `reach map: now ${reachMaps[at].map.label || reachMaps[at].name}` });
        if (lastRaw) { last = withRing(lastRaw); tell({ type: 'state', state: last }); }
      }
    }
    if (message.type === 'drag' && (!real || (page.movesOk && lamp.grants.includes('body')))) drag(message.joint, Number(message.deg), message.final === true);
    if (message.type === 'let_go') release();
    if (message.type === 'stop_all') stopAll();
    if (message.type === 'give_back') giveBack();
    if (message.type === 'resume' && halted) { halted = false; tell({ type: 'resumed', text: 'resumed: the sliders send again' }); }
    if (message.type === 'camera_on') cameraAskedAt = Date.now();
    if (message.type === 'hear') hearing(page, message);
    if (tryPanel && (message.type === 'call' || message.type === 'stim') && typeof message.name === 'string') press(message, page);
  });
  // The endless rainbow goes with the last page: nobody is left to switch it off.
  page.on('close', () => { pages.delete(page); if (pages.size === 0) stopRainbow().catch(() => {}); });
});
// The end of a real run: this run's scratch clip and notes come off the lamp (nothing else is touched), then the connection
// closes. try.js sends SIGTERM; a second signal ends it at once.
if (real) {
  // Six steps, each bounded at 3 s, are 18 s at worst; the limit is that plus 5 s, so a lamp that hangs on every step still ends 0.
  const STEP_MS = 3000;
  const STEPS = 6;
  const bounded = (work) => Promise.race([work.catch((error) => console.error(`try: could not clean up on the lamp: ${error?.message ?? error}`)), new Promise((resolve) => setTimeout(resolve, STEP_MS))]);
  const end = endHandler({
    mostMs: STEPS * STEP_MS + 5000,
    exit: (code) => process.exit(code),
    cleanup: async () => {
      await bounded(stopRainbow());
      await bounded(release());
      await bounded(cleanScratch(lamp));
      // Notes are listed only on a pairing that has the grant: without it the lamp answers no_grant, and there is nothing of ours to remove.
      if (lamp.grants.includes('notes')) await bounded(cleanScratchNotes(lamp));
      await bounded(cleanScratchSkill(lamp));
      await bounded(lamp.close());
    },
  });
  process.on('SIGTERM', end);
  process.on('SIGINT', end);
  // try.js started this process in a group of its own, so a terminal's Ctrl-C reaches try.js alone; if try.js dies without a
  // word (SIGKILL), this one cleans up and ends too, rather than keeping the lamp connected.
  watchParent({ parent: process.ppid, end });
}
// Always on 127.0.0.1. With --lan also on the one address it prints (a second server that hands its requests and
// upgrades to the first), never on every interface: a second network card or a VPN address does not get the twin.
// No network address found: loopback only, said.
server.keepAliveTimeout = IDLE_MS;
server.headersTimeout = IDLE_MS + 5000;
server.listen(port, '127.0.0.1', () => {
  console.log(`the twin: http://localhost:${port}/?t=${token}`);
  if (lan && !lanIp) console.log('no network address found: the twin is served on this computer only (127.0.0.1)');
  if (!lanIp) return;
  const network = createServer();
  network.keepAliveTimeout = IDLE_MS;
  network.headersTimeout = IDLE_MS + 5000;
  network.on('clientError', dropBadClient);
  network.on('request', (...handed) => server.emit('request', ...handed));
  network.on('upgrade', (...handed) => server.emit('upgrade', ...handed));
  network.listen(port, lanIp, () => {
    console.log(`on your network: http://${lanIp}:${port}/?t=${token}`);
    if (lanChoice.others.length) console.log(`other addresses (one is served; pick with --lan-address IP): ${lanChoice.others.map(({ name, address }) => `${address} (${name})`).join(', ')}`);
  });
});
for (const signal of ['SIGINT', 'SIGTERM']) {
  process.on(signal, async () => {
    // A real run has its own end (above): this lamp's scratch is cleaned first.
    if (real) return;
    await release();
    await lamp.close();
    await fakeLamp?.stop();
    process.exit(0);
  });
}

function parse(args) {
  const out = {};
  for (let i = 0; i < args.length; i++) {
    const key = args[i].replace(/^--/, '');
    if (['fake', 'lan', 'real', 'camera', 'microphone'].includes(key)) out[key] = true;
    else out[key] = args[++i];
  }
  return out;
}

// A fake lamp with every grant, paired in a keyring of its own, and a body
// that eases each joint to where it was last sent, said 25 times a second
// as lamp-control's `body.state` says it.
async function fakeBody() {
  const fakeLamp = await FakeLamp.start({ name: 'Fake lamp (twin)', grants: ['body', 'light', 'speak', 'look', 'notes', 'camera', 'skills', 'speaker'] });
  // A keyring of its own, so the fake's pairing never sits beside a real lamp's.
  const folder = mkdtempSync(join(tmpdir(), 'lampos-twin-'));
  process.on('exit', () => rmSync(folder, { recursive: true, force: true }));
  const keyring = new Keyring(folder);
  await Lamp.pair(fakeLamp.code, fakeLamp.host, { pin: fakeLamp.pin, port: fakeLamp.port, keyring });
  const lamp = await Lamp.connect(undefined, { keyring });
  const at = Object.fromEntries(NAMES.map((name) => [name, 0]));
  // `--fake-arm SHOULDER,ELBOW` (degrees from the folded arm) starts the simulated arm there: the stand is about 49.5,46.1.
  const [armShoulder, armElbow] = (options['fake-arm'] ?? '35,-60').split(',').map(Number);
  if (![armShoulder, armElbow].every(Number.isFinite)) throw new Error('--fake-arm takes SHOULDER,ELBOW in degrees');
  Object.assign(at, { shoulder: armShoulder, elbow: armElbow });
  const goal = { ...at };
  const sim = { aim: (joint, deg) => (goal[joint] = deg) };
  const colours = ['warm', 'orange', 'pink', 'purple', 'blue', 'cyan', 'green', 'yellow'];
  let tick = 0;
  setInterval(() => {
    const joints = {};
    for (const name of NAMES) {
      const step = Math.max(-4, Math.min(4, goal[name] - at[name]));
      at[name] += step;
      joints[name] = {
        deg: Number(at[name].toFixed(1)),
        // A home at the servo's middle; the head looks up with fewer counts.
        counts: Math.round((name === 'pitch' ? -at[name] : at[name]) / DEG_A_COUNT + 2048),
        goal_deg: Number(goal[name].toFixed(1)),
        load: Number((0.05 + Math.abs(step) / 20).toFixed(2)),
        moving: Math.abs(step) * 25 >= 5,
        temp_c: 40,
      };
    }
    // A new colour every 4 s, as a skill's light might.
    const colour = colours[Math.floor(tick++ / 100) % colours.length];
    // A computer's own pixels, while the fake lamp holds them, as the real body.state carries them.
    const shows = fakeLamp.lightState()?.shows;
    fakeLamp.emit('body.state', {
      joints,
      lamp: { light: { on: true, colour, level: 3, ...(shows?.pixels ? { shows } : {}) }, talking: false, private: false },
    });
  }, 40);
  return { lamp, sim, fakeLamp };
}
