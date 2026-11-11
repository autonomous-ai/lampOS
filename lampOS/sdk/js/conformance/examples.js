// The SDK's examples against a running lamp, black box: each example runs as a
// program of its own, prints no error, and the lamp shows what it is for,
// seen through the SDK (a light's colour, a joint that moved). The fake lamp's
// own test (test/examples.test.js) checks what the example asked; a real or
// virtual lamp cannot be asked that, so this checks what the lamp did.
//
//   LAMPOS_HOME=<paired with the lamp> node conformance/examples.js [--sim DIR [--lamp-sim PATH]]
//
// `--sim DIR` is the virtual desktop lamp's folder (`lamp-sim run DIR`): the examples that
// wait for the room (someone arriving, a heard phrase, a hand, the air) make it happen with
// `lamp-sim people|hear|hand|air ... DIR` (`--lamp-sim PATH` names the binary; default
// `lamp-sim` on the PATH), and are skipped without it. The lamp tells that someone left
// about five seconds after the count drops, a debounce a real lamp has too.
import { spawn } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { Lamp } from '../src/index.js';

const examples = new URL('../examples/', import.meta.url);
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const colour = async (lamp) => (await lamp.get('light')).light?.colour;
const moved = (now, before) => Object.keys(now).some((joint) => Math.abs(now[joint] - before[joint]) > 3);
const counts = async (lamp) => Object.fromEntries(Object.entries((await lamp.bodyInfo()).joints ?? {}).map(([name, joint]) => [name, joint.counts]));

// name, arguments, what to do once it runs (`nudge`), and what the lamp must show (`shows(lamp, before, output)`:
// through the SDK, or in what the example printed when what it does is hard to see on the lamp).
export const RUNS = [
  { name: 'ci-light.js', args: ['pass'], shows: async (lamp) => (await colour(lamp)) === 'green' },
  { name: 'pomodoro.js', args: [], shows: async (lamp) => (await colour(lamp)) === 'warm' },
  { name: 'ring-walk.js', args: ['50'], shows: async () => true },
  { name: 'curious.js', args: [], needs: 'body', shows: async () => true },
  { name: 'ask-the-person.js', args: [], shows: async () => true },
  {
    name: 'greet.js', args: [], needs: 'sim',
    // Someone leaves, the lamp says so, someone arrives: the greeting moves the body.
    // (Waiting for the lamp to say so, not a fixed time: under load the debounce is longer.)
    nudge: async (sim, lamp) => {
      let left;
      const said = new Promise((resolve) => { left = resolve; });
      await lamp.on('person.left', () => left());
      await sim('people', '0');
      await Promise.race([said, sleep(20_000)]);
      await sim('people', '1');
    },
    shows: async (lamp, before) => moved(await counts(lamp), before),
  },
  {
    name: 'phrase.js', args: [], needs: 'sim',
    nudge: async (sim) => { await sim('hear', 'good night lamp'); },
    shows: async (lamp) => (await colour(lamp)) === 'warm',
  },
  {
    name: 'air.js', args: [], needs: 'sim',
    // The lamp tells the air by change: clean it first, so the stuffy reading is a change on every run.
    nudge: async (sim) => { await sim('air', 'pm2_5=5', 'co2_ppm=500'); await sleep(1_500); await sim('air', 'pm2_5=41', 'co2_ppm=1350'); },
    shows: async (lamp) => (await colour(lamp)) === 'orange',
  },
  {
    name: 'touch.js', args: [], needs: 'sim',
    // `lamp-sim touch tap` is the head's tap; the example says what it answered, and a tap lights the ring warm.
    nudge: async (sim) => { await sim('touch', 'tap'); },
    shows: async (lamp, before, out) => /tap/.test(out) && (await colour(lamp)) === 'warm',
  },
  {
    name: 'follow-hand.js', args: [], needs: 'sim',
    nudge: async (sim) => { await sim('hand'); },
    // The hand comes and goes; the example follows it and says it let go (it prints that when the lamp says the hand is gone).
    shows: async (lamp, before, out) => /let go/.test(out),
  },
];

// The examples with no row yet, and what each needs of the lamp first: the room's
// events a lamp-sim hook has to raise (air readings, a heard phrase, a hand), a
// camera, a skill folder. A new example must be in RUNS or here, so none is left untried.
export const NOT_YET = {
  'camera.js': 'the camera open (lamp-sim --camera)', 'follow.js': 'someone arriving, then a look at a face the lamp does not take yet', 'push-skill.js': 'a skill folder and skill push on the lamp', 'tour.js': 'a run of its own (conformance.js covers its steps)',
  'notes.js': 'a note kept across runs, then its count shown as the light (no black-box row yet; test/examples.test.js checks its asks)',
  'heat-watch.js': 'a joint over its limit (not tried against lamp-sim yet)', 'visitors.js': 'an arrival (lamp-sim people 0, a 5 s wait, people 1), then the phrase (lamp-sim hear); no black-box row yet', 'desk-health.js': 'stuffy air for a wait, then clear (a run against lamp-sim: DESK_STUFFY_AFTER_S short, lamp-sim air; no black-box row yet)',
};

async function run(lamp, { name, args, nudge, shows }, sim) {
  const before = await counts(lamp);
  const child = spawn(process.execPath, [fileURLToPath(new URL(name, examples)), ...args], { env: process.env });
  let out = '';
  child.stdout.on('data', (chunk) => { out += chunk; });
  child.stderr.on('data', (chunk) => { out += chunk; });
  let done = false;
  const exited = new Promise((resolve) => child.on('close', (code, signal) => { done = true; resolve({ code, signal }); }));
  await sleep(2_000);
  if (nudge) await nudge(sim, lamp);
  let shown = false;
  for (let waited = 0; waited < 12_000 && !shown; waited += 500) {
    shown = await shows(lamp, before, out).catch(() => false);
    if (!shown) await sleep(500);
  }
  if (!done) child.kill('SIGTERM');
  const { code, signal } = await exited;
  const error = /Error|at file:/.test(out);
  return { ok: shown && !error && (code === 0 || signal === 'SIGTERM'), shown, error, out: out.trim().split('\n').slice(0, 3).join(' / ').slice(0, 200) };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const option = (name) => { const at = process.argv.indexOf(name); return at > 0 ? process.argv[at + 1] : undefined; };
  const dir = option('--sim');
  const binary = option('--lamp-sim') ?? 'lamp-sim';
  // `sim('people', '0')`: `lamp-sim people 0 DIR`, said back if it fails.
  const sim = dir ? (...words) => new Promise((resolve, reject) => {
    const child = spawn(binary, [...words, dir], { stdio: 'ignore' });
    child.on('close', (code) => (code === 0 ? resolve() : reject(new Error(`lamp-sim ${words.join(' ')} failed: ${code}`))));
    child.on('error', reject);
  }) : undefined;
  const lamp = await Lamp.connect();
  let failed = 0;
  try {
    for (const entry of RUNS) {
      if (entry.needs === 'sim' && !sim) { console.log(`${entry.name.padEnd(20)} skipped (needs --sim DIR)`); continue; }
      if (entry.needs === 'body' && !lamp.grants.includes('body')) { console.log(`${entry.name.padEnd(20)} skipped (needs the body grant)`); continue; }
      const result = await run(lamp, entry, sim);
      if (!result.ok) failed++;
      console.log(`${entry.name.padEnd(20)} ${result.ok ? 'ok' : 'FAILED'}${result.ok ? '' : ` shown=${result.shown} error=${result.error} ${result.out}`}`);
    }
  } finally { await lamp.close(); }
  process.exit(failed ? 1 : 0);
}
