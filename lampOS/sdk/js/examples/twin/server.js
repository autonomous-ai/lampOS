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
import { createServer } from 'node:http';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { WebSocketServer } from 'ws';
import { Keyring, Lamp, LampError, model } from 'lampos';
import { FakeLamp } from 'lampos/testing';
import { newToken, originOk, tokenOk } from './guard.js';

const here = (path) => fileURLToPath(new URL(path, import.meta.url));
const options = parse(process.argv.slice(2));
const port = Number(options.port ?? 8737);
const token = newToken();
const glbPath = options.glb ?? here('../../../../../robots/lamp/hardware/cad/glb/lamp.glb');
if (!existsSync(glbPath) || readFileSync(glbPath).toString('latin1', 0, 4) !== 'glTF') {
  console.error(`${glbPath}: no rigged CAD here (a clone needs \`git lfs pull\`); give --glb PATH`);
  process.exit(2);
}
const NAMES = model.joints.map((joint) => joint.name);
const DEG_A_COUNT = 360 / 4096;

const { lamp, sim, fakeLamp } = options.fake ? await fakeBody() : { lamp: await Lamp.connect() };
const pages = new Set();
const tell = (message) => {
  const text = JSON.stringify(message);
  for (const page of pages) page.send(text);
};
let last = null;
// `--ring-file PATH`: the virtual desktop lamp's ring pixels (lamp-sim's DIR/ring.json, 32 `#rrggbb`),
// put in the light's `shows.pixels` until a lamp says them itself: the twin then draws the ring pixel
// by pixel. It reads a file beside the lamp, not the lamp's door; it goes with the field it stands in for.
const ringFile = options['ring-file'];
const withRing = (state) => {
  if (!ringFile) return state;
  try {
    const { pixels } = JSON.parse(readFileSync(ringFile, 'utf8'));
    if (Array.isArray(pixels) && !state.lamp?.light?.shows?.pixels) {
      return { ...state, lamp: { ...state.lamp, light: { ...state.lamp?.light, shows: { ...state.lamp?.light?.shows, pixels } } } };
    }
  } catch { /* no ring file yet, or half written: the one colour shows */ }
  return state;
};
await lamp.on('body.state', (raw) => {
  const state = withRing(raw);
  last = state;
  tell({ type: 'state', state });
});
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

const files = {
  '/': ['text/html; charset=utf-8', () => readFileSync(here('./index.html'))],
  '/lamp.glb': ['model/gltf-binary', () => readFileSync(glbPath)],
  '/model.json': ['application/json', () => JSON.stringify(model)],
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

const server = createServer((request, response) => {
  if (!tokenOk(request.url, token)) {
    response.writeHead(403).end('the twin needs the URL it printed, with its token\n');
    return;
  }
  const pathname = new URL(request.url, 'http://localhost').pathname;
  if (pathname === '/camera.jpg') {
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
  response.writeHead(200, { 'content-type': file[0], 'cache-control': 'no-store' }).end(file[1]());
});
const wss = new WebSocketServer({ noServer: true });
server.on('upgrade', (request, socket, head) => {
  const path = new URL(request.url, 'http://localhost').pathname;
  if (path !== '/live' || !tokenOk(request.url, token) || !originOk(request.headers.origin, port)) {
    socket.end('HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n');
    return;
  }
  wss.handleUpgrade(request, socket, head, (page) => wss.emit('connection', page, request));
});
wss.on('connection', (page) => {
  pages.add(page);
  if (last) page.send(JSON.stringify({ type: 'state', state: last }));
  page.on('message', (data) => {
    let message;
    try {
      message = JSON.parse(data);
    } catch {
      return;
    }
    if (message.type === 'drag') drag(message.joint, Number(message.deg));
    if (message.type === 'let_go') release();
  });
  page.on('close', () => pages.delete(page));
});
server.listen(port, '127.0.0.1', () => console.log(`the twin: http://localhost:${port}/?t=${token}`));
for (const signal of ['SIGINT', 'SIGTERM']) {
  process.on(signal, async () => {
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
    if (key === 'fake') out.fake = true;
    else out[key] = args[++i];
  }
  return out;
}

// A fake lamp with every grant, paired in a keyring of its own, and a body
// that eases each joint to where it was last sent, said 25 times a second
// as lamp-control's `body.state` says it.
async function fakeBody() {
  const fakeLamp = await FakeLamp.start({ name: 'Fake lamp (twin)', grants: ['body', 'light', 'speak', 'look', 'camera'] });
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
    fakeLamp.emit('body.state', {
      joints,
      lamp: { light: { on: true, colour, level: 3 }, talking: false, private: false },
    });
  }, 40);
  return { lamp, sim, fakeLamp };
}
