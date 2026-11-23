// The twin's "what the real body will do" (examples/twin/reach.js and the server's side of it): the lamp's own limits as bands and
// warnings, never a block. The page itself is a browser's; its logic is these functions, and the server must send every ask as made.
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { WebSocket } from 'ws';
import { aboveStand, clampTo, doneWords, limitsOf, pastReach, reachOf, reachText } from '../examples/twin/reach.js';
import { startTwin } from './twin-start.js';

// `get body` as a product 75c9 says it: roll held to a count either way, pitch 13 up and about 1 down, yaw 22 either way.
const INFO = { joints: {
  roll: { min_deg: -0.1, max_deg: 0.1 }, pitch: { min_deg: -1.3, max_deg: 13.2 }, yaw: { min_deg: -22, max_deg: 22 },
  elbow: { min_deg: null, max_deg: null }, shoulder: {},
} };

test('the limits are read from body_info, a joint that says none has none', () => {
  assert.deepEqual(limitsOf(INFO), { roll: { min: -0.1, max: 0.1 }, pitch: { min: -1.3, max: 13.2 }, yaw: { min: -22, max: 22 } });
  assert.deepEqual(limitsOf({}), {});
  assert.deepEqual(limitsOf(undefined), {});
  assert.deepEqual(limitsOf({ joints: { yaw: { min_deg: 5, max_deg: -5 } } }), {}, 'a reach the wrong way round is no reach');
});

test('the pose the body will take is the ask held inside its reach, and no limit means as asked', () => {
  const { roll, pitch } = limitsOf(INFO);
  assert.equal(clampTo(roll, 10), 0.1);
  assert.equal(clampTo(roll, -90), -0.1);
  assert.equal(clampTo(pitch, 5), 5);
  assert.equal(clampTo(pitch, -20), -1.3);
  assert.equal(clampTo(undefined, 77), 77);
  assert.equal(clampTo(roll, undefined), undefined);
});

test('an ask is past the reach only by more than the slack, and the line under the slider says what the body will do', () => {
  const { roll } = limitsOf(INFO);
  assert.equal(pastReach(roll, 0.12), false);
  assert.equal(pastReach(roll, 0.2), true);
  assert.equal(pastReach(undefined, 500), false);
  assert.equal(reachText(roll, 0), 'the lamp reaches -0.1° to 0.1°');
  assert.equal(reachText(roll, 10), 'the lamp reaches -0.1° to 0.1°: it will cut 10.0° to 0.1°');
  assert.equal(reachText(undefined, 10), '');
});

test('the lamp\'s own words for a move: reached, cut, dropped with its why, or cut short', () => {
  assert.equal(doneWords({ how: 'finished', clamped: false, roll: 0 }, 'roll'), 'the lamp took it (reached 0.0°)');
  assert.equal(doneWords({ how: 'finished', clamped: true, roll: 0.1 }, 'roll'), 'the lamp took it, cut by the body (reached 0.1°)');
  assert.equal(doneWords({ how: 'dropped', why: 'refused by the body: not standing' }, 'roll'), 'the lamp did not take it: refused by the body: not standing');
  assert.equal(doneWords({ how: 'cut_short', why: 'a hand on the head' }, 'yaw'), 'the lamp was cut short: a hand on the head');
  assert.equal(doneWords({}, 'yaw'), '');
});

// The server end, on the twin with its simulated body (limits of the fake lamp: yaw +-3.5): it tells each page the limits, sends every
// ask as made (the page warns and never blocks: a clamp here would show as the reached angle), and says the lamp's answer to a drag.
test('the twin tells the page the lamp\'s limits, sends a drag past them as asked, and says what the lamp answered', async (t) => {
  const folder = mkdtempSync(join(tmpdir(), 'reach-'));
  t.after(() => rmSync(folder, { recursive: true, force: true }));
  const glb = join(folder, 'lamp.glb');
  writeFileSync(glb, Buffer.from('glTF\x02\x00\x00\x00\x0c\x00\x00\x00'));
  const server = fileURLToPath(new URL('../examples/twin/server.js', import.meta.url));
  const { port, token } = await startTwin(t, server, (p) => ['--fake', '--port', String(p), '--glb', glb]);
  const messages = [];
  const page = new WebSocket(`ws://127.0.0.1:${port}/live?t=${token}`, { headers: { origin: `http://127.0.0.1:${port}` } });
  page.on('message', (data) => messages.push(JSON.parse(data)));
  await new Promise((resolve, reject) => { page.on('open', resolve); page.on('error', reject); });
  t.after(() => page.close());
  const until = async (find, what) => {
    for (let i = 0; i < 100; i += 1) { const found = messages.find(find); if (found) return found; await new Promise((r) => setTimeout(r, 50)); }
    assert.fail(`never heard: ${what}: ${JSON.stringify(messages.slice(-5))}`);
  };
  const limits = await until((m) => m.type === 'limits', 'the limits');
  assert.deepEqual(limits.joints.yaw, { min: -3.5, max: 3.5 });
  // Past the reach: sent as asked, so the lamp (here the fake, which does not cut) says it reached 50.
  page.send(JSON.stringify({ type: 'drag', joint: 'yaw', deg: 50, final: true }));
  const done = await until((m) => m.type === 'joint_done' && m.joint === 'yaw', "the lamp's answer to the drag");
  assert.equal(done.how, 'finished');
  assert.equal(done.yaw, 50, 'the ask went to the lamp whole: the page does not clamp it');
  // A drag step (not the last) asks the lamp and says nothing more.
  const before = messages.filter((m) => m.type === 'joint_done').length;
  page.send(JSON.stringify({ type: 'drag', joint: 'yaw', deg: 10 }));
  await new Promise((r) => setTimeout(r, 300));
  assert.equal(messages.filter((m) => m.type === 'joint_done').length, before);
});

// `get body` as lamp-75c9 said it standing (10 Nov..19 Nov): the servos' spans are wide, but a holder's pose is cut to the counts between the
// fold and the stand (body.rs, the holder's pose), so nothing above the stand is taken.
const STANDING = { stands: true, joints: {
  shoulder: { counts: 2620, deg: 47.8, folded_counts: 2076, stand_counts: 2639, min_deg: -36.9, max_deg: 86.4 },
  elbow: { counts: 2535, deg: 43.1, folded_counts: 2045, stand_counts: 2570, min_deg: -36.9, max_deg: 83.1 },
  pitch: { counts: 1789, deg: 0.2, folded_counts: null, stand_counts: null, min_deg: -1.3, max_deg: 4.5 },
} };

test('on a standing lamp an arm joint takes only the fold to the stand: above it is not yet, said and shaded', () => {
  const limits = limitsOf(STANDING);
  assert.ok(Math.abs(limits.shoulder.pose.max - 49.5) < 0.05 && limits.shoulder.pose.min === 0, 'the shoulder: 0 to 49.5');
  assert.ok(Math.abs(limits.elbow.pose.max - 46.1) < 0.05, 'the elbow: 0 to 46.1');
  assert.equal(limits.pitch.pose, undefined, 'the head has no stand pose');
  assert.deepEqual(reachOf(limits.pitch), { min: -1.3, max: 4.5 });
  assert.ok(Math.abs(reachOf(limits.shoulder).max - 49.5) < 0.05 && reachOf(limits.shoulder).min === 0, 'what the body takes is not the servo span');
  assert.ok(Math.abs(clampTo(limits.shoulder, 51.8) - 49.5) < 0.05, 'an ask above the stand is cut to the stand');
  assert.equal(clampTo(limits.shoulder, 44.8), 44.8, 'below the stand it is taken as asked');
  assert.equal(pastReach(limits.shoulder, 51.8), true);
  assert.equal(aboveStand(limits.shoulder, 51.8), true);
  assert.equal(aboveStand(limits.shoulder, 44.8), false);
  assert.equal(aboveStand(limits.pitch, 3), false);
  assert.match(reachText(limits.shoulder, 51.8), /above the stand, it will be cut to 49\.5°/);
  assert.match(reachText(limits.shoulder, 44.8), /takes 0\.0° to 49\.5° \(up to its stand; above it is not yet\)$/);
  // The lamp's own word after the let-go ask names the stand when it cut the ask there.
  assert.match(doneWords({ how: 'finished', clamped: true, shoulder: 48 }, 'shoulder', limits.shoulder), /cut by the body \(reached 48\.0°\) \(at its stand: above it is not yet\)/);
  assert.doesNotMatch(doneWords({ how: 'finished', clamped: true, shoulder: 20 }, 'shoulder', limits.shoulder), /stand/);
  // A lamp that does not say it stands, or a joint without its counts, has no pose range: as before.
  assert.equal(limitsOf({ ...STANDING, stands: false }).shoulder.pose, undefined);
  assert.equal(limitsOf({ stands: true, joints: { shoulder: { min_deg: -1, max_deg: 9 } } }).shoulder.pose, undefined);
});

test('the try-it page draws the not-yet region and reads the body\'s reach through reach.js', async () => {
  const { readFileSync } = await import('node:fs');
  const html = readFileSync(new URL('../examples/twin/index.html', import.meta.url), 'utf8');
  assert.match(html, /class="notyet"/);
  assert.match(html, /bodyReach\.reachOf\(limit\)/);
  assert.match(html, /doneWords\(data, data\.joint, limits\[data\.joint\]\)/);
  assert.match(html, /limit\.pose\.stand >= 0 \? \[limit\.pose\.stand, 180\] : \[-180, limit\.pose\.stand\]/, 'the region is drawn on the side of the stand away from the fold');
  assert.match(html, /id="holdnote"/);
});

test('a joint that counts down from its fold has its stand below the fold: past it, away from the fold, is not yet', () => {
  const down = limitsOf({ stands: true, joints: { shoulder: { folded_counts: 2076, stand_counts: 1537, min_deg: -90, max_deg: 40 } } }).shoulder;
  assert.ok(Math.abs(down.pose.stand + 47.4) < 0.1 && down.pose.max === 0 && Math.abs(down.pose.min + 47.4) < 0.1);
  assert.equal(aboveStand(down, -50), true, 'further from the fold than the stand');
  assert.equal(aboveStand(down, -30), false);
  assert.equal(aboveStand(down, 10), false, 'the fold side of the fold is the servo\'s business, not "above the stand"');
  assert.ok(Math.abs(clampTo(down, -60) + 47.4) < 0.1);
  assert.match(reachText(down, -50), /is above the stand, it will be cut to -47\.4°/);
});
