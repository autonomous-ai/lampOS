// The try-it page's reach guard (examples/twin/reach-guard.js): a goal for the arm or the head checked against the reach map
// (reach-map-4ace.json, from tools/reach) before it is sent; the page only.
import assert from 'node:assert/strict';
import { existsSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { WebSocket } from 'ws';
import { CONTROLS, checked } from '../examples/twin/controls.js';
import { countsOf, distance, isSafe, limits, loadReachMap, rangesDeg, restrict, restrictAll } from '../examples/twin/reach-guard.js';
import { startTwin } from './twin-start.js';

// The page's own clamp, taken out of index.html between its markers (as try-panel-load.test.js does for the loader).
const pageHtml = readFileSync(new URL('../examples/twin/index.html', import.meta.url), 'utf8');
const { clampToRange } = new Function(`${/\/\/ <reach-clamp>\n([\s\S]*?)\/\/ <\/reach-clamp>/.exec(pageHtml)[1]}\nreturn { clampToRange };`)();

const mapFile = fileURLToPath(new URL('../examples/twin/reach-map-4ace.json', import.meta.url));
const heavyFile = fileURLToPath(new URL('../examples/twin/reach-map-4ace-base-heavy.json', import.meta.url));
const map = loadReachMap(mapFile);
const heavy = loadReachMap(heavyFile);
// body.state's joints, degrees from home: standing (49.5 / 46.1 are the arm's stand angles of the model), folded, head level.
const standing = { shoulder: { deg: 49.5 }, elbow: { deg: 46.1 }, pitch: { deg: 0 }, yaw: { deg: 0 } };
const folded = { shoulder: { deg: 0 }, elbow: { deg: 0 }, pitch: { deg: -10 }, yaw: { deg: 0 } };

test('a map that is not one, or does not fit its axes, is refused at load', () => {
  const dir = mkdtempSync(join(tmpdir(), 'reach-'));
  const bad = (name, value) => { const p = join(dir, name); writeFileSync(p, JSON.stringify(value)); return p; };
  assert.throws(() => loadReachMap(bad('a.json', {})), /not a reach map/);
  const good = JSON.parse(readFileSync(mapFile, 'utf8'));
  assert.throws(() => loadReachMap(bad('b.json', { ...good, safe_pitch_runs: good.safe_pitch_runs.slice(1) })), /do not fit/);
  assert.equal(loadReachMap(mapFile).axes.shoulder.length, 46);
});

test('the map reads as the study did: the stand is safe, the folded pose is not, the head tips far down standing and little up', () => {
  const stand = countsOf({ shoulder: 49.5, elbow: 46.1, pitch: 0 });
  assert.ok(isSafe(map, stand, 0));
  assert.equal(distance(map, stand), 0);
  const fold = countsOf({ shoulder: 0, elbow: 0, pitch: -10 });
  assert.ok(!isSafe(map, fold, 0));
  assert.ok(distance(map, fold) > 0);
  // The head's pitch at the stand: about 77 deg down, about 27 up (README.md), in counts the grid's own edges.
  const [low, high] = limits(map, stand, 'pitch', 0);
  assert.ok((high - stand.pitch) * (360 / 4096) > 70, `${high}`);
  assert.ok((stand.pitch - low) * (360 / 4096) > 20, `${low}`);
});

test('a goal inside the safe poses goes as asked; one past them is held at the edge, with the reason', () => {
  assert.deepEqual(restrict(map, standing, 'pitch', -40), { deg: -40 });
  const up = restrict(map, standing, 'pitch', 60);
  assert.ok(up.deg < 60 && up.deg > 15, JSON.stringify(up));
  assert.match(up.note, /pitch 60 is past what the other joints allow here: held at /);
  const down = restrict(map, standing, 'pitch', -89);
  assert.ok(down.deg > -89 && down.deg < -65, JSON.stringify(down));
  // The edge keeps a margin of counts off the map's own edge: a larger shrink holds earlier.
  assert.ok(restrict(map, standing, 'pitch', 60, { shrink: 96 }).deg < up.deg);
  assert.ok(restrict(map, standing, 'pitch', 60, { shrink: 0 }).deg > up.deg);
  // The held value is itself safe, and a goal a hair past it is held there again.
  assert.deepEqual(restrict(map, standing, 'pitch', up.deg), { deg: up.deg });
});

test('the other joints count: the same head pitch is held with the arm in another pose', () => {
  const halfUp = { shoulder: { deg: 24 }, elbow: { deg: 23 }, pitch: { deg: 0 }, yaw: { deg: 0 } };
  const a = restrict(map, standing, 'pitch', -60);
  const b = restrict(map, halfUp, 'pitch', -60);
  assert.deepEqual(a, { deg: -60 });
  assert.ok(b.deg > -60, `half way up the head may not tip that far: ${JSON.stringify(b)}`);
});

test('from a pose the map calls unsafe the guard lets the lamp move toward safe and not away, and never traps it', () => {
  // Folded: the head rests on its arm. Raising the arm is the way out; tipping the head further down is not.
  assert.deepEqual(restrict(map, folded, 'shoulder', 30), { deg: 30 });
  const worse = restrict(map, folded, 'pitch', -14);
  assert.equal(worse.deg, -10);
  assert.match(worse.note, /further from its safe poses: not sent/);
  assert.deepEqual(restrict(map, folded, 'pitch', -4), { deg: -4 });
  // The elbow's goal that is the arm's own fold is not refused at the stand either.
  assert.deepEqual(restrict(map, standing, 'elbow', 40), { deg: 40 });
});

test('a joint the map does not hold, a number that is not one, or a lamp that has not said its pose passes as it is', () => {
  assert.deepEqual(restrict(map, standing, 'yaw', 170), { deg: 170 });
  assert.deepEqual(restrict(map, standing, 'roll', 3), { deg: 3 });
  assert.deepEqual(restrict(map, standing, 'pitch', Number.NaN).note, undefined);
  assert.deepEqual(restrict(map, undefined, 'pitch', 80), { deg: 80 });
  assert.deepEqual(restrict(map, { shoulder: { deg: 1 } }, 'pitch', 80), { deg: 80 });
});

test('a set of goals is checked together: yaw as asked, pitch held, one note', () => {
  const out = restrictAll(map, standing, { yaw: 25, pitch: 70 });
  assert.equal(out.targets.yaw, 25);
  assert.ok(out.targets.pitch < 70);
  assert.equal(out.notes.length, 1);
  assert.deepEqual(restrictAll(map, standing, { yaw: 10, pitch: 5 }), { targets: { yaw: 10, pitch: 5 }, notes: [] });
});

test('the ranges the page shows are the ones the guard holds to, and none where the pose is unsafe', () => {
  const r = rangesDeg(map, standing);
  assert.deepEqual(Object.keys(r), ['shoulder', 'elbow', 'pitch']);
  assert.ok(r.pitch[0] < -70 && r.pitch[0] > -80 && r.pitch[1] > 15 && r.pitch[1] < 27, JSON.stringify(r));
  // A goal at either end is sent as it is, and one a hair past is held at it.
  assert.deepEqual(restrict(map, standing, 'pitch', r.pitch[0]), { deg: r.pitch[0] });
  assert.ok(restrict(map, standing, 'pitch', r.pitch[0] - 3).deg >= r.pitch[0] - 0.1);
  // Other joints move the range, and the goals (not where the joint is now) are what it is read at.
  const lower = rangesDeg(map, { ...standing, shoulder: { deg: 24, goal_deg: 24 }, elbow: { deg: 23, goal_deg: 23 } });
  assert.notDeepEqual(lower.pitch, r.pitch);
  // Folded: every joint's range is none.
  assert.deepEqual(rangesDeg(map, folded), { shoulder: null, elbow: null, pitch: null });
  assert.equal(rangesDeg(map, undefined), null);
  assert.equal(rangesDeg(map, { shoulder: { deg: 1 } }), null);
});

test('a joint\'s range is the same wherever in it the joint is, so the page\'s shading does not drift as it moves', () => {
  const at = (joint, deg) => rangesDeg(map, { ...standing, [joint]: { deg, goal_deg: deg } });
  for (const deg of [-60, -30, 0, 10, 20]) assert.deepEqual(at('pitch', deg).pitch, at('pitch', 0).pitch, `pitch at ${deg}`);
  for (const deg of [44, 46.1, 52, 60]) assert.deepEqual(at('elbow', deg).elbow, at('elbow', 46.1).elbow, `elbow at ${deg}`);
  for (const deg of [40, 49.5, 60]) assert.deepEqual(at('shoulder', deg).shoulder, at('shoulder', 49.5).shoulder, `shoulder at ${deg}`);
});

test('the joints row sends what the guard returns, and says it', async () => {
  const control = CONTROLS.find((c) => c.id === 'joints');
  const sent = [];
  const lamp = { withLease: async (_scope, run) => run('lease'), joints: async (_lease, ms, targets) => { sent.push([ms, targets]); } };
  const said = await control.run({ lamp, release: async () => {}, reach: (t) => restrictAll(map, standing, t).targets }, checked(control.fields, { yaw: 5, pitch: 70, within_ms: 800 }));
  assert.equal(sent.length, 1);
  assert.equal(sent[0][1].yaw, 5);
  assert.ok(sent[0][1].pitch < 70);
  assert.match(said, /joints\(lease, 800, \{"yaw":5,"pitch":\d+(\.\d+)?\}/);
  // Without a map the row is as it was.
  sent.length = 0;
  await control.run({ lamp, release: async () => {} }, checked(control.fields, { yaw: 5, pitch: 70, within_ms: 800 }));
  assert.deepEqual(sent, [[800, { yaw: 5, pitch: 70 }]]);
});

const glb = new URL('../../../../robots/lamp/hardware/cad/glb/lamp.glb', import.meta.url);
const hasCad = existsSync(glb) && readFileSync(glb).toString('latin1', 0, 4) === 'glTF';

test('the running twin, standing: the head goes far down as far as the map allows and no further, with the reason', { skip: !hasCad && 'lamp.glb is not here (Git LFS not pulled)' }, async (t) => {
  const server = fileURLToPath(new URL('../examples/twin/server.js', import.meta.url));
  const { port, token } = await startTwin(t, server, (p) => ['--fake', '--port', String(p), '--reach-map', mapFile, '--fake-arm', '49.5,46.1']);
  const ws = new WebSocket(`ws://127.0.0.1:${port}/live?t=${token}`, { origin: `http://127.0.0.1:${port}` });
  t.after(() => ws.close());
  const said = [];
  let pose = null;
  let reach = null;
  ws.on('message', (data) => {
    const m = JSON.parse(data);
    if (m.type === 'said') said.push(m.text);
    if (m.type === 'state') { pose = m.state?.joints ?? pose; reach = m.state?.reach ?? reach; }
  });
  await new Promise((resolve, reject) => { ws.once('open', resolve); ws.once('error', reject); });
  const until = async (ok, ms = 8000) => { const end = Date.now() + ms; while (Date.now() < end) { if (ok()) return true; await new Promise((r) => setTimeout(r, 50)); } return false; };
  assert.ok(await until(() => pose?.pitch), 'the twin says a pose');
  assert.equal(pose.shoulder.deg, 49.5);
  assert.ok(await until(() => reach), 'the state carries the reach map');
  assert.equal(reach.virtual, true);
  assert.equal(reach.on, true);
  assert.ok(reach.ranges.pitch[0] < -70 && reach.ranges.pitch[1] > 15, JSON.stringify(reach.ranges));
  // 70 deg down (up is positive): inside what the map allows standing, so as asked.
  ws.send(JSON.stringify({ type: 'drag', joint: 'pitch', deg: -70 }));
  assert.ok(await until(() => pose.pitch.goal_deg === -70), `the head goes 70 deg down: ${pose.pitch.goal_deg} ${said.join(' | ')}`);
  // 89 deg down: past it, held at the edge, said.
  said.length = 0;
  ws.send(JSON.stringify({ type: 'drag', joint: 'pitch', deg: -89 }));
  assert.ok(await until(() => said.some((s) => /reach map: pitch -89 is past what the other joints allow here: held at -7\d/.test(s))), said.join(' | '));
  const held = Number(/held at (-?[\d.]+)/.exec(said.find((s) => /reach map: pitch -89/.test(s)))[1]);
  assert.ok(held <= -70 && held > -80, `${held}`);
  assert.ok(await until(() => pose.pitch.goal_deg === held), `the head goes to the held ${held}, not ${pose.pitch.goal_deg}`);
  // A yaw is not in the map and goes as asked.
  ws.send(JSON.stringify({ type: 'drag', joint: 'yaw', deg: 25 }));
  assert.ok(await until(() => pose.yaw.goal_deg === 25));
});

test('the page holds a slider inside the range the map allows: past either end it stops at the end; no range, as it is', () => {
  assert.equal(clampToRange(-90, [-71.3, 21.5]), -71.3);
  assert.equal(clampToRange(40, [-71.3, 21.5]), 21.5);
  assert.equal(clampToRange(5, [-71.3, 21.5]), 5);
  assert.equal(clampToRange(-71.3, [-71.3, 21.5]), -71.3);
  assert.equal(clampToRange(33, null), 33);
  assert.equal(clampToRange(33, undefined), 33);
  assert.equal(clampToRange(33, [Number.NaN, 5]), 33);
});

test('the running virtual lamp moves its arm, and the others\' ranges follow: nothing is sent outside what the map allows', { skip: !hasCad && 'lamp.glb is not here (Git LFS not pulled)' }, async (t) => {
  const server = fileURLToPath(new URL('../examples/twin/server.js', import.meta.url));
  const { port, token } = await startTwin(t, server, (p) => ['--fake', '--port', String(p), '--reach-map', mapFile, '--fake-arm', '49.5,46.1']);
  const ws = new WebSocket(`ws://127.0.0.1:${port}/live?t=${token}`, { origin: `http://127.0.0.1:${port}` });
  t.after(() => ws.close());
  const said = [];
  let pose = null;
  let reach = null;
  ws.on('message', (data) => {
    const m = JSON.parse(data);
    if (m.type === 'said') said.push(m.text);
    if (m.type === 'state') { pose = m.state?.joints ?? pose; reach = m.state?.reach ?? reach; }
  });
  await new Promise((resolve, reject) => { ws.once('open', resolve); ws.once('error', reject); });
  const until = async (ok, ms = 8000) => { const end = Date.now() + ms; while (Date.now() < end) { if (ok()) return true; await new Promise((r) => setTimeout(r, 50)); } return false; };
  const drag = (joint, deg) => ws.send(JSON.stringify({ type: 'drag', joint, deg }));
  assert.ok(await until(() => reach?.ranges?.pitch));
  const standingPitch = [...reach.ranges.pitch];
  // The arm moves (the virtual lamp simulates it) and pitch's range changes with it.
  // Toward half way up, a step at a time (each joint is held where the others still allow it).
  for (const [joint, deg] of [['elbow', 32], ['shoulder', 34], ['elbow', 24], ['shoulder', 26], ['elbow', 20]]) {
    drag(joint, deg);
    await new Promise((r) => setTimeout(r, 1500));
  }
  assert.ok(pose.shoulder.goal_deg < 40 && pose.elbow.goal_deg < 30, `the arm came down: ${pose.shoulder.goal_deg} ${pose.elbow.goal_deg} ${said.join(' | ')}`);
  assert.ok(await until(() => reach.ranges.pitch && reach.ranges.pitch[0] > standingPitch[0] + 10), `pitch's range follows the arm: ${JSON.stringify(standingPitch)} -> ${JSON.stringify(reach.ranges.pitch)}`);
  // Tip the head to the far end of what is allowed now, then ask the shoulder past what that head allows: it stops at its edge.
  drag('pitch', reach.ranges.pitch[0]);
  assert.ok(await until(() => pose.pitch.goal_deg === reach.ranges.pitch[0] || Math.abs(pose.pitch.goal_deg - reach.ranges.pitch[0]) < 0.2));
  const [low, high] = reach.ranges.shoulder;
  said.length = 0;
  drag('shoulder', high + 25);
  assert.ok(await until(() => said.some((s) => /reach map: shoulder/.test(s))), said.join(' | '));
  assert.ok(await until(() => pose.shoulder.goal_deg <= high + 0.1), `${pose.shoulder.goal_deg} past ${high}`);
  assert.ok(pose.shoulder.goal_deg >= low - 0.1);
  // After every move the head is still inside its own range: nothing the page sent left the safe poses.
  assert.ok(await until(() => reach.ranges.pitch && pose.pitch.goal_deg >= reach.ranges.pitch[0] - 0.3 && pose.pitch.goal_deg <= reach.ranges.pitch[1] + 0.3), `${pose.pitch.goal_deg} in ${JSON.stringify(reach.ranges.pitch)}`);
});

test('the base-heavy map is plainly an estimate, opens the shoulder where tipping stops binding, and still stops at contacts', () => {
  assert.match(map.label, /^V1 masses/);
  assert.match(heavy.label, /^ESTIMATE, not measured: base 6\.0 kg/);
  const [v1, wide] = [rangesDeg(map, standing), rangesDeg(heavy, standing)];
  // Tipping no longer closes the arm: the shoulder and the elbow reach far past the V1 ends.
  assert.ok(wide.shoulder[0] < v1.shoulder[0] - 15 && wide.shoulder[1] > v1.shoulder[1] + 20, `${JSON.stringify(v1.shoulder)} ${JSON.stringify(wide.shoulder)}`);
  assert.ok(wide.elbow[1] > v1.elbow[1] + 10);
  // The head still stops at the shade meeting the arm: tipping never set that end, so it is the same.
  assert.ok(Math.abs(wide.pitch[0] - v1.pitch[0]) < 3, `${JSON.stringify(v1.pitch)} ${JSON.stringify(wide.pitch)}`);
  // And the ends are the map's own grid ends (a servo's span, a contact): never past the grid.
  const grid = (joint, counts) => counts.map((c) => (joint === 'pitch' ? (1772 - c) : (c - (joint === 'shoulder' ? 2048 : 2047))) * (360 / 4096)).sort((a, b) => a - b);
  for (const joint of ['shoulder', 'elbow', 'pitch']) {
    const ends = [heavy.axes[joint][0], heavy.axes[joint].at(-1)];
    const [lo, hi] = grid(joint, ends);
    assert.ok(wide[joint][0] >= lo - 0.2 && wide[joint][1] <= hi + 0.2, `${joint} ${JSON.stringify(wide[joint])} inside ${lo}..${hi}`);
  }
  // Goals past a contact are still held with the heavy map: the head cannot go past -77 standing.
  assert.ok(restrict(heavy, standing, 'pitch', -89).deg >= -78);
});

test('the running virtual lamp offers both maps, says which masses the limits come from, and switches', { skip: !hasCad && 'lamp.glb is not here (Git LFS not pulled)' }, async (t) => {
  const server = fileURLToPath(new URL('../examples/twin/server.js', import.meta.url));
  const { port, token } = await startTwin(t, server, (p) => ['--fake', '--port', String(p), '--reach-map', `${mapFile},${heavyFile}`, '--fake-arm', '49.5,46.1']);
  const ws = new WebSocket(`ws://127.0.0.1:${port}/live?t=${token}`, { origin: `http://127.0.0.1:${port}` });
  t.after(() => ws.close());
  let reach = null;
  ws.on('message', (data) => { const m = JSON.parse(data); if (m.type === 'state') reach = m.state?.reach ?? reach; });
  await new Promise((resolve, reject) => { ws.once('open', resolve); ws.once('error', reject); });
  const until = async (ok, ms = 8000) => { const end = Date.now() + ms; while (Date.now() < end) { if (ok()) return true; await new Promise((r) => setTimeout(r, 50)); } return false; };
  assert.ok(await until(() => reach?.ranges?.shoulder));
  assert.equal(reach.choices.length, 2);
  assert.equal(reach.name, 'reach-map-4ace');
  assert.match(reach.label, /^V1 masses/);
  const narrow = [...reach.ranges.shoulder];
  ws.send(JSON.stringify({ type: 'reach_map', name: 'reach-map-4ace-base-heavy' }));
  assert.ok(await until(() => reach.name === 'reach-map-4ace-base-heavy'));
  assert.match(reach.label, /^ESTIMATE, not measured/);
  assert.ok(await until(() => reach.ranges.shoulder[0] < narrow[0] - 15), `${JSON.stringify(narrow)} -> ${JSON.stringify(reach.ranges.shoulder)}`);
  // An unknown name changes nothing.
  ws.send(JSON.stringify({ type: 'reach_map', name: 'nope' }));
  await new Promise((r) => setTimeout(r, 300));
  assert.equal(reach.name, 'reach-map-4ace-base-heavy');
});
