// What the lamp's camera would see: a picture taken from where the lamp's head
// was first, seen again from where it is now. A camera that only turns (its
// centre stays) sees the same scene through a homography, H = K * R^-1 * K^-1,
// so the picture is moved, stretched and, past the edge of what the first
// picture holds, black. This is the reference for lamp-sim's own warp: pure
// functions, no browser and no lamp, so a test (and the Rust version) can be
// held against it on the same frames.
//
// The head's direction comes from the model's chain (`lamp.model.json`: each
// bone's rest rotation, axis, sign and zero) and a pose in degrees from the
// home, as `body.state` counts them. The camera looks along the head's own
// +y (the shade's axis); its horizon stays level (the head's roll is a degree
// or two). A picture is { width, height, data } with `data` RGBA bytes.

const norm = (v) => { const n = Math.hypot(...v); return v.map((x) => x / n); };
const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];

// Quaternions [x, y, z, w].
const qmul = (a, b) => [
  a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
  a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
  a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
  a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
];
const qaxis = (axis, rad) => { const [x, y, z] = norm(axis); const s = Math.sin(rad / 2); return [x * s, y * s, z * s, Math.cos(rad / 2)]; };
const qrotate = (q, v) => {
  const p = qmul(qmul(q, [v[0], v[1], v[2], 0]), [-q[0], -q[1], -q[2], q[3]]);
  return [p[0], p[1], p[2]];
};

/** The head's world rotation for a pose `{ yaw, shoulder, elbow, pitch, roll }` (degrees from home). */
export function headRotation(model, pose) {
  let q = [0, 0, 0, 1];
  for (const name of model.chain) {
    const joint = model.joints.find((j) => j.name === name);
    const turn = ((joint.sign ?? 1) * ((pose[name] ?? 0) - (joint.zero_deg ?? 0)) * Math.PI) / 180;
    q = qmul(qmul(q, joint.rest_quat), qaxis(joint.axis, turn));
  }
  return q;
}

/** A camera frame from where it looks: its right and down in the picture, the horizon level. */
export function frameFromForward(direction) {
  const forward = norm(direction);
  const worldUp = [0, 1, 0];
  // Looking straight up or down, the horizon is undefined: use the world's z as the helper.
  const right = norm(Math.abs(dot(forward, worldUp)) > 0.999 ? cross(forward, [0, 0, 1]) : cross(forward, worldUp));
  const up = cross(right, forward);
  return { forward, right, down: up.map((x) => -x) };
}

/** The camera's frame in the world for a pose: it looks along the head's +y. */
export const cameraFrame = (model, pose) => frameFromForward(qrotate(headRotation(model, pose), [0, 1, 0]));

/** The angle in degrees between two frames' directions. */
export const angleBetween = (a, b) => (Math.acos(Math.max(-1, Math.min(1, dot(a.forward, b.forward)))) * 180) / Math.PI;

/** 1 up to `from` degrees off, falling to 0 at `to` and beyond: the picture fades out, it does not snap. */
export const fadeAt = (degrees, { from = 60, to = 90 } = {}) => (degrees <= from ? 1 : degrees >= to ? 0 : (to - degrees) / (to - from));

/** Where output pixel (x, y) comes from in the source picture, as [sx, sy] (fractional, pixel centres at .0), or null: black. */
export function sourcePixel(width, height, f, reference, now, x, y) {
  const ray = [(x + 0.5 - width / 2) / f, (y + 0.5 - height / 2) / f, 1];
  // The ray in the world, then in the reference camera's frame.
  const world = [0, 1, 2].map((k) => now.right[k] * ray[0] + now.down[k] * ray[1] + now.forward[k] * ray[2]);
  const inRef = [dot(world, reference.right), dot(world, reference.down), dot(world, reference.forward)];
  if (inRef[2] <= 1e-6) return null;
  const sx = (inRef[0] / inRef[2]) * f + width / 2 - 0.5;
  const sy = (inRef[1] / inRef[2]) * f + height / 2 - 0.5;
  return sx < 0 || sy < 0 || sx > width - 1 || sy > height - 1 ? null : [sx, sy];
}

export const focal = (size, fovDeg) => (size / 2) / Math.tan((fovDeg * Math.PI) / 360);

/**
 * The picture seen from `now` that was taken from `reference` (both camera frames), as the lamp's
 * camera of `fov` degrees across would see it: for each output pixel the ray, in the reference
 * camera, and the source pixel it lands on (bilinear); black where it falls outside the source,
 * behind the camera, or past the fade.
 */
export function warp(source, reference, now, { fov = 128, fade = {} } = {}) {
  const { width, height } = source;
  const f = focal(width, fov);
  const out = { width, height, data: new Uint8ClampedArray(width * height * 4) };
  const gain = fadeAt(angleBetween(reference, now), fade);
  if (gain === 0) { for (let i = 3; i < out.data.length; i += 4) out.data[i] = 255; return out; }
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const at = (y * width + x) * 4;
      out.data[at + 3] = 255;
      const from = sourcePixel(width, height, f, reference, now, x, y);
      if (!from) continue;
      const [sx, sy] = from;
      const [x0, y0] = [Math.floor(sx), Math.floor(sy)];
      const [x1, y1] = [Math.min(x0 + 1, width - 1), Math.min(y0 + 1, height - 1)];
      const [tx, ty] = [sx - x0, sy - y0];
      for (let c = 0; c < 3; c++) {
        const p = (px, py) => source.data[(py * width + px) * 4 + c];
        out.data[at + c] = gain * ((p(x0, y0) * (1 - tx) + p(x1, y0) * tx) * (1 - ty) + (p(x0, y1) * (1 - tx) + p(x1, y1) * tx) * ty);
      }
    }
  }
  return out;
}
