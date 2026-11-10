// The every-example test, in its own file so it has its own share of
// node --test's per-file timeout: it runs every example at once, and the
// longest (ring-walk.js) takes about 7 s by design.
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { Keyring, Lamp, posesToFrames, schema } from '../src/index.js';
import { FakeLamp, handTrack } from '../src/testing.js';

const folder = () => mkdtempSync(join(tmpdir(), 'lampos-test-'));

// Every example but the tour (which has its own test), run from sdk/js as the
// README runs it, against a fake lamp that grants everything: its arguments,
// the event it waits for (said once it has subscribed), the ops it exists to
// send with the fields that say what it asks (`sent` below; test_client.py's
// `_sent` the same),
// and the one refusal it may print: `look` at a face is not in this
// release (docs/control.md), so follow.js asks and is told so. A new example
// needs its row, so none is left untried. It is a smoke test: a long-running
// example is stopped once it has sent its ops, so it shows the example starts
// and asks what it is for, not that it ends well (a camera's saved file, say).
// One that saves files runs in the
// test's own folder instead (WRITES_FILES), so nothing is left in the
// repository. test_client.py has the same.
const WRITES_FILES = new Set(['camera.js']);
const EXAMPLE_RUNS = {
  'air.js': [[], 'air', [['get', { state: ['air'] }], ['light', { colour: 'orange', level: 2, pattern: 'breathe' }]]],
  'ask-the-person.js': [[], null, [['turn.start', { prompt: "Shall we go over today's list?" }]]],
  'camera.js': [['1'], null, ['camera.snapshot', ['camera.stream', { on: true, fps: 5 }]]],
  'ci-light.js': [['pass'], null, [['light', { colour: 'green', level: 3, pattern: 'breathe' }]]],
  // Its three poses, eased as the SDK eases them.
  'curious.js': [[], null, ['lease.take', ['move', { frames: posesToFrames([{ pitch: 3 }, { pitch: 2, yaw: -3 }, { pitch: 2, yaw: 3 }], { holdMs: 400 }) }]]],
  'follow.js': [[], 'arrived', ['lease.take', ['look', { at: 'face' }]], 'not now: refused: not in this release: look at a face or a name'],
  'follow-hand.js': [[], 'hand', ['lease.take', ['look', { at: 'hand' }]]],
  'heat-watch.js': [[], 'hot', [['light', { colour: 'orange', level: 2, pattern: 'pulse' }]]],
  'greet.js': [[], 'arrived', [['gesture', { name: 'greeting' }]]],
  'phrase.js': [[], 'phrase', [['light', { colour: 'warm', level: 2, pattern: 'breathe' }], ['gesture', { name: 'nod' }]]],
  'pomodoro.js': [[], null, [['light', { colour: 'warm', level: 2 }]]],
  'push-skill.js': [['../examples/hello-skill'], null, ['skill.push']],
  'touch.js': [[], 'tap', [['light', { colour: 'warm', level: 3 }]]],
  // Pixel 0 white, a green pixel walking, the whole ring blue, given back.
  'ring-walk.js': [['50'], null, [
    ['light', { pixels: (px) => Array.isArray(px) && px[0] === '#ffffff' && px.slice(1).every((c) => c === '#000000') }],
    ['light', { pixels: (px) => Array.isArray(px) && px.filter((c) => c === '#00ff00').length === 1 }],
    ['light', { rgb: '#0000ff', brightness: 50 }],
    'stop',
  ]],
};

// An op an example must send: its name, or [name, fields], where each field
// is the value the lamp must have been sent (compared as JSON) or a check of it.
const opWhat = (op) => (Array.isArray(op) ? `${op[0]} ${JSON.stringify(op[1], (k, v) => (typeof v === 'function' ? 'a check' : v))}` : op);
const sent = (fake, op) => {
  const [name, fields = {}] = Array.isArray(op) ? op : [op];
  return fake.asked(name).some((asked) => Object.entries(fields).every(([key, want]) =>
    (typeof want === 'function' ? want(asked[key]) : JSON.stringify(asked[key]) === JSON.stringify(want))));
};

test('every example runs against a fake lamp and sends what it is for', async () => {
  const { spawn } = await import('node:child_process');
  const { readdirSync } = await import('node:fs');
  const examples = new URL('../examples/', import.meta.url);
  const sdkJs = new URL('../', import.meta.url).pathname;
  assert.deepEqual(readdirSync(examples).filter((f) => f.endsWith('.js') && f !== 'tour.js').sort(), Object.keys(EXAMPLE_RUNS).sort());
  const nudges = {
    air: (fake) => fake.setAir({ pm2_5: 41, co2_ppm: 1350, feel: 'stuffy' }),
    arrived: (fake) => fake.emit('person.arrived', { count: 1, near: 0.3 }),
    // In real time, as the lamp tells it: the example looks every 0.2 s.
    hand: (fake) => fake.emitHandTrack(handTrack('wave', { seconds: 1 }), { realtime: true }),
    hot: (fake) => fake.emit('body.state', { joints: { yaw: { deg: 0, temp_c: 41 }, elbow: { deg: 30, temp_c: 66 } }, lamp: { light: 'off', talking: false, private: false } }),
    phrase: (fake) => fake.emit('phrase.heard', { phrase: 'good night lamp' }),
    tap: (fake) => fake.emit('touch', { kind: 'tap' }),
  };
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  await Promise.all(Object.entries(EXAMPLE_RUNS).map(async ([name, [args, nudge, ops, refusal]]) => {
    const fake = await FakeLamp.start({ grants: [...schema.grants] });
    try {
      const home = folder();
      await Lamp.pair(fake.code, fake.host, { pin: fake.pin, port: fake.port, keyring: new Keyring(home) });
      // Not execFileSync: the fake answers on this process's event loop.
      const child = spawn(process.execPath, [new URL(name, examples).pathname, ...args], { cwd: WRITES_FILES.has(name) ? home : sdkJs, env: { ...process.env, LAMPOS_HOME: home } });
      let out = '';
      child.stdout.on('data', (chunk) => { out += chunk; });
      child.stderr.on('data', (chunk) => { out += chunk; });
      const exited = new Promise((resolve) => child.on('close', (code, signal) => resolve({ code, signal })));
      let done = false;
      exited.then(() => { done = true; });
      try {
        const deadline = Date.now() + 15_000;
        if (nudge) {
          while (!fake.asked('subscribe').length && !done) {
            assert.ok(Date.now() < deadline, name);
            await sleep(10);
          }
          await nudges[nudge](fake);
        }
        while (!ops.every((op) => sent(fake, op)) && !done && Date.now() < deadline) await sleep(20);
        // A moment for what it prints after its last op, then it is stopped.
        if (!done) await sleep(500);
      } finally {
        // Stopped however this ends, so a failed check leaves no example
        // running (Reviewer-two's P3 on #5834).
        if (!done) child.kill('SIGTERM');
      }
      const { code, signal } = await exited;
      assert.ok(!/Error|at file:/.test(out.replace(refusal ?? '\0', '')), `${name}: ${out}`);
      assert.ok(refusal ? out.includes(refusal) : !out.includes('not now'), `${name}: ${out}`);
      assert.ok(code === 0 || signal === 'SIGTERM', `${name}: exit ${code} ${signal}: ${out}`);
      for (const op of ops) assert.ok(sent(fake, op), `${name} sent no ${opWhat(op)}: ${out}`);
    } finally {
      await fake.stop();
    }
  }));
});
