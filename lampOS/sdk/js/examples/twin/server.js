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
import { checked, cleanScratch, cleanScratchNotes, describe, findControl, findStimulus, logIt } from './controls.js';
import { chooseLan, hostOk, newToken, originOk, parseUrl, tokenOk } from './guard.js';

const here = (path) => fileURLToPath(new URL(path, import.meta.url));
const options = parse(process.argv.slice(2));
const port = Number(options.port ?? 8737);
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

const { lamp, sim, fakeLamp } = options.fake ? await fakeBody() : real ? { lamp: await Lamp.connect(undefined, { keyring: realKeyring }) } : { lamp: await Lamp.connect() };
const runFlags = real ? { camera: Boolean(options.camera), microphone: Boolean(options.microphone) } : undefined;
const realHost = real ? realKeyring.pairing()?.host ?? '' : '';
const pages = new Set();
const tell = (message) => {
  const text = JSON.stringify(message);
  for (const page of pages) page.send(text);
};
let last = null;
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
const withRing = (state) => {
  if (!ringFile) return state;
  const ring = ringShows();
  if (!ring) return state;
  const shows = state.lamp?.light?.shows ?? {};
  // What the lamp says itself stays; the ring's file fills only what is missing.
  return { ...state, lamp: { ...state.lamp, light: { ...state.lamp?.light, shows: { ...ring, ...shows } } } };
};
let lastRaw = null;
await lamp.on('body.state', (raw) => {
  lastRaw = raw;
  const state = withRing(raw);
  last = state;
  tell({ type: 'state', state });
});
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
console.log(`body.state subscribed on ${options.fake ? 'a simulated body' : 'the paired lamp'}`);

// One lease at a time, taken at the first drag and given back 3 s after the last.
let lease = null;
let letGo = null;
async function drag(joint, deg) {
  if (!NAMES.includes(joint) || !Number.isFinite(deg)) return;
  try {
    if (!lease) {
      lease = lamp.lease('body');
      await lease.take();
      tell({ type: 'said', text: `holding the body (lease ${lease.id})` });
    }
    clearTimeout(letGo);
    letGo = setTimeout(release, 3_000);
    const target = Math.max(-180, Math.min(180, deg));
    await lamp.joints(lease, 300, { [joint]: Number(target.toFixed(1)) });
    sim?.aim(joint, deg);
  } catch (error) {
    if (!(error instanceof LampError)) throw error;
    tell({ type: 'said', text: `refused: ${error.code}: ${error.message}` });
    if (error.code === 'no_lease') lease = null;
  }
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
      // A row the page draws off (not granted, or without --camera/--microphone on a real lamp) is refused here too, on a raw call;
      // and a row that moves the real lamp waits for this page's yes to "this moves the real lamp".
      const off = describe(mode, { grants: lamp.grants, flags: runFlags }).controls.find((c) => c.id === control.id)?.disabled;
      if (off) throw refuse(`n/a: ${off}`);
      if (real && control.moves && !page?.movesOk) throw refuse('this moves the real lamp: say yes to "this moves the real lamp" on the page first');
      // A picture goes to the page that pressed, as one message to it: not through `tell`, never logged, never kept.
      const show = (jpeg) => page?.send(JSON.stringify({ type: 'picture', id, jpeg: Buffer.from(jpeg).toString('base64') }));
      const call = await control.run({ lamp, release, listenFor, show, mode, audio: audioTo(page, id) }, checked(control.fields, args, mode));
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
  page.on('message', (data) => {
    let message;
    try {
      message = JSON.parse(data);
    } catch {
      return;
    }
    // On a real lamp the sliders move the real body only after the page said yes to "this moves the real lamp", and with the grant.
    if (message.type === 'moves_ok' && real) page.movesOk = true;
    if (message.type === 'drag' && (!real || (page.movesOk && lamp.grants.includes('body')))) drag(message.joint, Number(message.deg));
    if (message.type === 'let_go') release();
    if (message.type === 'camera_on') cameraAskedAt = Date.now();
    if (message.type === 'hear') hearing(page, message);
    if (tryPanel && (message.type === 'call' || message.type === 'stim') && typeof message.name === 'string') press(message, page);
  });
  page.on('close', () => pages.delete(page));
});
// The end of a real run: this run's scratch clip and notes come off the lamp (nothing else is touched), then the connection
// closes. try.js sends SIGTERM; a second signal ends it at once.
if (real) {
  let signals = 0;
  let ending = false;
  const bounded = (work) => Promise.race([work.catch((error) => console.error(`try: could not clean up on the lamp: ${error?.message ?? error}`)), new Promise((resolve) => setTimeout(resolve, 3000))]);
  const end = async () => {
    signals += 1;
    // A repeat signal (a second Ctrl-C, a parent's SIGTERM after a terminal's SIGINT) must not cut the cleanup short: only a
    // third one ends at once, and the cleanup itself is bounded (about 12 s).
    if (signals >= 3) process.exit(1);
    if (ending) return;
    ending = true;
    setTimeout(() => process.exit(1), 15_000).unref();
    await bounded(release());
    await bounded(cleanScratch(lamp));
    await bounded(cleanScratchNotes(lamp));
    await bounded(lamp.close());
    process.exit(0);
  };
  process.on('SIGTERM', end);
  process.on('SIGINT', end);
  // try.js started this process in a group of its own, so a terminal's Ctrl-C reaches try.js alone; if try.js dies without a
  // word (SIGKILL), this one cleans up and ends too, rather than keeping the lamp connected.
  const parent = process.ppid;
  setInterval(() => { if (process.ppid !== parent) end(); }, 2000).unref();
}
// Always on 127.0.0.1. With --lan also on the one address it prints (a second server that hands its requests and
// upgrades to the first), never on every interface: a second network card or a VPN address does not get the twin.
// No network address found: loopback only, said.
server.listen(port, '127.0.0.1', () => {
  console.log(`the twin: http://localhost:${port}/?t=${token}`);
  if (lan && !lanIp) console.log('no network address found: the twin is served on this computer only (127.0.0.1)');
  if (!lanIp) return;
  const network = createServer();
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
  const goal = { ...at, shoulder: 35, elbow: -60 };
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
