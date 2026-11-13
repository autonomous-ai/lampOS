// The camera-by-pose reference (examples/twin/camera-warp.js): what the lamp's camera would see of a picture
// taken from its first pose, from its pose now. Facing it, the same picture; a little off, the picture moved;
// far off, black.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { model } from '../src/index.js';
import { angleBetween, cameraFrame, fadeAt, focal, frameFromForward, headRotation, sourcePixel, warp } from '../examples/twin/camera-warp.js';

// A picture whose pixel colour says where it is: red = x, green = y.
const picture = (width = 64, height = 48) => {
  const data = new Uint8ClampedArray(width * height * 4);
  for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) { const at = (y * width + x) * 4; data.set([Math.round((x / (width - 1)) * 255), Math.round((y / (height - 1)) * 255), 128, 255], at); }
  return { width, height, data };
};
const pixel = (p, x, y) => Array.from(p.data.slice((y * p.width + x) * 4, (y * p.width + x) * 4 + 4));
const STAND = { shoulder: 49.5, elbow: 46.1, pitch: 0, roll: 0, yaw: 0 };
const frame = (pose) => cameraFrame(model, { ...STAND, ...pose });

test('the same pose gives the same picture', () => {
  const src = picture();
  const out = warp(src, frame({}), frame({}), { fov: 90 });
  for (const [x, y] of [[0, 0], [10, 20], [32, 24], [63, 47]]) assert.deepEqual(pixel(out, x, y), pixel(src, x, y));
});

test('the head turned a little sees the picture moved the other way, the middle of it still in view', () => {
  const src = picture();
  const [reference, now] = [frame({}), frame({ yaw: 10 })];
  const turned = angleBetween(reference, now);
  assert.ok(turned > 5 && turned < 12, `the head turned ${turned} degrees`);
  const out = warp(src, reference, now, { fov: 90 });
  // The picture's middle column is not in the middle of what is seen now: it moved sideways.
  const middle = pixel(out, 32, 24);
  assert.notDeepEqual(middle, pixel(src, 32, 24));
  assert.equal(middle[3], 255);
  // The rows stay level (a yaw does not tilt the horizon): the green channel at a fixed row is unchanged in the middle.
  assert.ok(Math.abs(middle[1] - pixel(src, 32, 24)[1]) < 12);
});

test('turned far enough the picture is gone: black', () => {
  const src = picture();
  const [reference, now] = [frameFromForward([0, 0, 1]), frameFromForward([Math.sin(2.1), 0, Math.cos(2.1)])]; // 120 degrees round
  assert.ok(Math.abs(angleBetween(reference, now) - 120.3) < 0.5);
  const out = warp(src, reference, now, { fov: 90 });
  for (let i = 0; i < out.data.length; i += 4) assert.deepEqual(Array.from(out.data.slice(i, i + 4)), [0, 0, 0, 255]);
});

test('thirty degrees off the picture is still there but moved, and dimmer only past 60', () => {
  const src = picture();
  const reference = frameFromForward([0, 0, 1]);
  const now = frameFromForward([Math.sin(Math.PI / 6), 0, Math.cos(Math.PI / 6)]);
  const out = warp(src, reference, now, { fov: 128 });
  let seen = 0;
  for (let i = 0; i < out.data.length; i += 4) if (out.data[i] + out.data[i + 1] > 0) seen++;
  assert.ok(seen > 100 && seen < out.width * out.height, `${seen} lit pixels: some of the picture, not all`);
  const at75 = warp(src, reference, frameFromForward([Math.sin(1.3), 0, Math.cos(1.3)]), { fov: 160 });
  const lit = (p) => Array.from({ length: p.data.length / 4 }, (_, i) => p.data[i * 4]).reduce((a, b) => Math.max(a, b), 0);
  assert.ok(lit(at75) <= 255 * 0.5 + 1, 'past 60 degrees it is dimmed');
});

test('the fade is 1 up to 60 degrees, falls to 0 at 90 and does not snap', () => {
  assert.equal(fadeAt(0), 1);
  assert.equal(fadeAt(60), 1);
  assert.equal(fadeAt(75), 0.5);
  assert.equal(fadeAt(90), 0);
  assert.equal(fadeAt(150), 0);
  assert.equal(fadeAt(10, { from: 5, to: 15 }), 0.5);
});

test('the head turns about the world up for a yaw, whatever its other joints', () => {
  for (const base of [{}, { shoulder: 20 }, { elbow: 70, pitch: 2 }]) {
    const a = cameraFrame(model, { ...STAND, ...base });
    const b = cameraFrame(model, { ...STAND, ...base, yaw: 30 });
    // The camera's height above the horizon does not change when the base turns.
    assert.ok(Math.abs(a.forward[1] - b.forward[1]) < 1e-2, JSON.stringify(base));
    // A turn of 30 degrees about the up axis moves a direction by at most 30 degrees (less when it points up or down).
    assert.ok(angleBetween(a, b) <= 30.5, `${angleBetween(a, b)} degrees for ${JSON.stringify(base)}`);
  }
  const q = headRotation(model, STAND);
  assert.ok(Math.abs(Math.hypot(...q) - 1) < 1e-3, 'a rotation stays a unit quaternion (the model keeps four decimals)');
});

test('where an output pixel comes from, as fixed vectors another implementation can be held to', () => {
  const [w, h, fov] = [320, 180, 128];
  const f = focal(w, fov);
  assert.ok(Math.abs(f - 78.0) < 0.1, `focal ${f}`);
  const reference = frameFromForward([0, 0, 1]);
  // The same pose: every pixel comes from itself.
  for (const [x, y] of [[0, 0], [160, 90], [319, 179], [40, 120]]) {
    const [sx, sy] = sourcePixel(w, h, f, reference, reference, x, y);
    assert.ok(Math.abs(sx - x) < 1e-6 && Math.abs(sy - y) < 1e-6);
  }
  // The camera looking along the world z has its right at the world -x (y up). Turned 30 degrees toward +x it has turned to its LEFT:
  // its middle pixel looks 30 degrees left of the reference's axis, so it comes from f * tan(30) pixels left of the source's middle.
  const now = frameFromForward([Math.sin(Math.PI / 6), 0, Math.cos(Math.PI / 6)]);
  const middle = sourcePixel(w, h, f, reference, now, 159, 89);
  assert.ok(middle && Math.abs(middle[0] - (w / 2 - 0.5 - f * Math.tan(Math.PI / 6))) < 0.7, JSON.stringify(middle));
  assert.ok(Math.abs(middle[1] - (h / 2 - 0.5)) < 1);
  // Far to the side of a wide lens the ray leaves the source: black.
  assert.equal(sourcePixel(w, h, f, reference, frameFromForward([1, 0, 0.05]), 0, 90), null);
});
