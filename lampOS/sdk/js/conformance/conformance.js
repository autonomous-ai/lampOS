// Black-box conformance: the same steps, through the public SDK only, against
// any lamp (a real one, the virtual desktop lamp, or the SDK's own fake), with
// output reduced to what a program can see: whether each step was taken, the
// refusal's code, the reply's field names and how long it took. A step that
// differs between two lamps is a finding about one of them.
//
//   node conformance/conformance.js                 # the paired lamp (LAMPOS_HOME)
//   node conformance/conformance.js --fake          # the SDK's own fake lamp
//   node conformance/conformance.js --compare       # the paired lamp against the fake
//   node conformance/conformance.js --json          # the table as JSON
//
// A step needing a grant the lamp did not give is skipped, not failed. The
// `ms` column is shown and never compared: timing is a separate step. A step
// named in EXPECTED differs by design (below) and is not counted.
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Keyring, Lamp, LampError } from '../src/index.js';
import { FakeLamp } from '../src/testing.js';

const STEPS = [
  ['get lamp', null, (lamp) => lamp.get('lamp')],
  ['get body', 'body', async (lamp) => { const body = await lamp.bodyInfo(); return { known: body.known, joints: Object.keys(body.joints ?? {}).sort() }; }],
  ['light green', 'light', (lamp) => lamp.light('green', 3, 1500)],
  ['get light', 'light', (lamp) => lamp.get('light')],
  ['light bad colour', 'light', (lamp) => lamp.light('chartreuse', 3, 1000)],
  ['light bad level', 'light', (lamp) => lamp.light('green', 99, 1000)],
  ['gesture nod', 'body', (lamp) => lamp.gesture('nod', { wait: true })],
  ['gesture bad name', 'body', (lamp) => lamp.gesture('NOT A GESTURE!')],
  ['look without a lease', 'body', (lamp) => lamp.look({ yaw: 5, pitch: 0 })],
  ['look under a lease', 'body', (lamp) => lamp.withLease(['body'], () => lamp.look({ yaw: 3, pitch: 0 }, { wait: true }))],
  ['say', 'speak', (lamp) => lamp.say('hello')],
  ['stop', null, (lamp) => lamp.stop()],
];

// A look on a resting lamp ends `cut_short` on a real body (the SDK README
// says so), where the SDK's fake finishes it: a known difference, not a finding.
export const EXPECTED = new Set(['look under a lease']);

export async function run(lamp) {
  const rows = [];
  for (const [name, grant, action] of STEPS) {
    if (grant && !lamp.grants.includes(grant)) { rows.push({ name, result: 'skipped', ms: 0 }); continue; }
    const started = Date.now();
    try {
      const reply = await action(lamp);
      rows.push({ name, result: 'ok', fields: reply && typeof reply === 'object' ? Object.keys(reply).filter((k) => !['re', 'ev'].includes(k)).sort() : [], ms: Date.now() - started });
    } catch (refused) {
      if (!(refused instanceof LampError)) throw refused;
      rows.push({ name, result: refused.code ?? 'refused', ms: Date.now() - started });
    }
  }
  return rows;
}

async function onFake() {
  const folder = mkdtempSync(join(tmpdir(), 'conformance-'));
  const fake = await FakeLamp.start({ grants: ['body', 'light', 'speak', 'look'] });
  try {
    const keyring = new Keyring(folder);
    await Lamp.pair(fake.code, fake.host, { pin: fake.pin, port: fake.port, name: 'conformance', keyring });
    const lamp = await Lamp.connect(undefined, { keyring });
    try { return await run(lamp); } finally { await lamp.close(); }
  } finally { await fake.stop(); rmSync(folder, { recursive: true, force: true }); }
}

async function onPaired() {
  const lamp = await Lamp.connect();
  try { return await run(lamp); } finally { await lamp.close(); }
}

const show = (rows) => rows.map((r) => `${r.name.padEnd(22)} ${r.result.padEnd(12)} ${(r.fields ?? []).join(',').padEnd(28)} ${String(r.ms).padStart(6)} ms`).join('\n');

// A step differs when it was taken in one and refused in the other, or refused with another code, or answered with other fields.
export function differences(left, right) {
  return left.flatMap((a, i) => {
    const b = right[i];
    const same = EXPECTED.has(a.name) || (a.result === b.result && (a.fields ?? []).join() === (b.fields ?? []).join());
    return same ? [] : [`${a.name}: ${a.result} ${(a.fields ?? []).join(',')} / ${b.result} ${(b.fields ?? []).join(',')}`];
  });
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  if (args.includes('--compare')) {
    const [paired, fake] = [await onPaired(), await onFake()];
    console.log(`paired lamp:\n${show(paired)}\n\nthe SDK's fake:\n${show(fake)}\n`);
    const different = differences(paired, fake);
    console.log(different.length ? `${different.length} step(s) differ:\n${different.join('\n')}` : 'no step differs');
    process.exit(different.length ? 1 : 0);
  }
  const rows = args.includes('--fake') ? await onFake() : await onPaired();
  console.log(args.includes('--json') ? JSON.stringify(rows, null, 1) : show(rows));
}
