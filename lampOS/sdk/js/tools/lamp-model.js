#!/usr/bin/env node
// Writes the lamp's kinematic model (lamp.model.json) from the rigged CAD
// export, robots/lamp/hardware/cad/glb/lamp.glb: the armature's five bones
// are the body's five servos, and their rest transforms are the chain.
//
//   node tools/lamp-model.js PATH/TO/lamp.glb > src/lamp.model.json
//
// Each bone's axis is read from the CAD's own animation (the one component
// of its rotation that changes), its origin and rest rotation from its node.
// What is not in the CAD is said as such: each joint's zero (the servo's
// reading at the CAD's rest pose) and direction are "cad" until measured
// on a lamp (MEASURED below holds what has been), and its reach is the
// product's (`lamp-body --reach`).
import { readFileSync } from 'node:fs';

// The body's servos in their bus order (lamp-body's `bus_out::JOINTS`), the
// protocol's names for them, and each one's bone in lamp.glb.
// Zero and direction measured from a lamp's own numbers. Shoulder and elbow: the
// zero is the lamp-75c9's stand angle (stand counts less home counts, of 4096 a
// turn: 2639 - 2076 and 2570 - 2045), so the CAD's upright pose is the stood lamp
// and deg 0 the folded one; photos of the lamp standing agree with it (11 Nov: the
// stand-up is +541 / +488 counts, 47.5 / 42.9 deg, to the rest at 2617 / 2534).
// The elbow's sign -1 closes the arm as the real fold does (the angle between the
// two segments narrows by the 43 deg of counts). The shoulder's sign is +1: it was -1
// from the render, which drew the folded lamp with its lower arm standing and the shade
// through the base (64 head vertices inside it, 12 below the table); +1 lays the lower
// arm low over the base, as in a frame of lamp-75c9 folded by itself (18:05 and 18:19,
// 11 Nov: arms low and compact, head on top, shade clear of the base), and the CAD
// scan (tools/model-collisions.html) finds no collision at the fold, the stand or between.
// Both signs were judged from frames and counts, not yet from a measured angle.
const MEASURED = { shoulder: { sign: 1, zero_deg: 49.5 }, elbow: { sign: -1, zero_deg: 46.1 } };

const JOINTS = [
  { name: 'yaw', bus: 'base_yaw', servo: 1, bone: 'base_yaw', mesh: '1_base_yaw' },
  { name: 'shoulder', bus: 'shoulder', servo: 2, bone: 'base_pitch', mesh: '2_base_pitch' },
  { name: 'elbow', bus: 'elbow', servo: 3, bone: 'elbow_pitch', mesh: '3_elbow_pitch' },
  { name: 'roll', bus: 'head_roll', servo: 4, bone: 'wrist_roll', mesh: '4_wrist_roll' },
  { name: 'pitch', bus: 'head_pitch', servo: 5, bone: 'wrist_pitch', mesh: '5_wrist_pitch' },
];
// The product's reach around home, in counts (deploy/product/lamp-body.service,
// `--reach UP,DOWN,ROLL,YAW`): the head's pitch up and down, its roll, the base's
// yaw. The shoulder and the elbow are moved only between folded and the stand.
const REACH = { pitch: { up: 150, down: 15 }, roll: { either_way: 1 }, yaw: { either_way: 250 } };
const DEG_A_COUNT = 360 / 4096;

function glb(path) {
  const bytes = readFileSync(path);
  if (bytes.toString('latin1', 0, 4) !== 'glTF') throw new Error(`${path}: not a binary glTF`);
  const jsonLength = bytes.readUInt32LE(12);
  const json = JSON.parse(bytes.toString('utf8', 20, 20 + jsonLength));
  const binAt = 20 + jsonLength;
  const bin = bytes.subarray(binAt + 8, binAt + 8 + bytes.readUInt32LE(binAt));
  const accessor = (index) => {
    const a = json.accessors[index];
    const view = json.bufferViews[a.bufferView];
    const width = { SCALAR: 1, VEC3: 3, VEC4: 4 }[a.type];
    const at = (view.byteOffset ?? 0) + (a.byteOffset ?? 0);
    const floats = new Float32Array(bin.buffer.slice(bin.byteOffset + at, bin.byteOffset + at + a.count * width * 4));
    return Array.from({ length: a.count }, (_, i) => Array.from(floats.subarray(i * width, (i + 1) * width)));
  };
  return { json, accessor };
}

// q = rest * delta, so delta = conj(rest) * q; the axis is delta's largest
// component of x, y and z over the animation (quaternions as [x, y, z, w]).
function axisOf(rest, keys) {
  const conj = [-rest[0], -rest[1], -rest[2], rest[3]];
  const mul = (a, b) => [
    a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
    a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
    a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
    a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
  ];
  const spread = [0, 0, 0];
  for (const q of keys) {
    const delta = mul(conj, q);
    for (let i = 0; i < 3; i++) spread[i] = Math.max(spread[i], Math.abs(delta[i]));
  }
  const most = spread.indexOf(Math.max(...spread));
  return [0, 1, 2].map((i) => (i === most ? 1 : 0));
}

const round = (value, places) => Number(value.toFixed(places));

function model(path) {
  const { json, accessor } = glb(path);
  const nodes = json.nodes;
  const byName = new Map(nodes.map((node, index) => [node.name, index]));
  const parentOf = new Map();
  nodes.forEach((node, index) => (node.children ?? []).forEach((child) => parentOf.set(child, index)));
  const animation = json.animations?.[0];
  const rotations = new Map();
  for (const channel of animation?.channels ?? []) {
    if (channel.target.path !== 'rotation') continue;
    rotations.set(channel.target.node, accessor(animation.samplers[channel.sampler].output));
  }
  const bones = new Set(JOINTS.map((joint) => joint.bone));
  const joints = JOINTS.map((joint) => {
    const index = byName.get(joint.bone);
    if (index === undefined) throw new Error(`${path}: no bone ${joint.bone}`);
    const node = nodes[index];
    const rest = node.rotation ?? [0, 0, 0, 1];
    const parent = nodes[parentOf.get(index)]?.name;
    const keys = rotations.get(index);
    if (!keys) throw new Error(`${path}: ${joint.bone} has no rotation in the animation, so no axis`);
    const reach = REACH[joint.name];
    return {
      name: joint.name,
      bus: joint.bus,
      servo: joint.servo,
      bone: joint.bone,
      parent: bones.has(parent) ? JOINTS.find((j) => j.bone === parent).name : 'base',
      origin_m: (node.translation ?? [0, 0, 0]).map((v) => round(v, 4)),
      rest_quat: rest.map((v) => round(v, 4)),
      axis: axisOf(rest, keys),
      sign: MEASURED[joint.name]?.sign ?? 1,
      zero_deg: MEASURED[joint.name]?.zero_deg ?? 0,
      reach_counts: reach ?? null,
      reach_deg: reach
        ? Object.fromEntries(Object.entries(reach).map(([way, counts]) => [way, round(counts * DEG_A_COUNT, 1)]))
        : null,
      mesh: joint.mesh,
    };
  });
  // The chain from the base out, as the armature has it: the head pitches
  // before it rolls.
  const order = [];
  let at = 'base';
  while (order.length < joints.length) {
    const next = joints.find((joint) => joint.parent === at);
    if (!next) break;
    order.push(next.name);
    at = next.name;
  }
  return {
    model: 'lampos-lamp',
    version: 1,
    hardware: 'Lamp_09102026',
    source: 'robots/lamp/hardware/cad/glb/lamp.glb (from the Fusion design f3d/Lamp_09102026.f3d)',
    units: { length: 'm', angle: 'deg' },
    frame: 'glTF: y up, metres; each joint turns its bone about axis, in the bone\'s own frame after rest_quat',
    measured: {
      origins: 'cad',
      zero_and_sign: 'shoulder and elbow: zero_deg at lamp-75c9\'s stand angle (stand 2639 / 2570 counts less home 2076 / 2045: 49.5 / 46.1 deg), so the CAD\'s upright pose is the stood lamp and deg 0 the folded one; photos of lamp-75c9 standing agree with the zero. Signs: the elbow\'s -1 closes the arm as the real fold does; the shoulder\'s +1 (it was -1) lays the lower arm low over the base at the fold, as in frames of lamp-75c9 folded by itself on 11 Nov, and the CAD scan (tools/model-collisions.html) finds no collision at the fold, the stand or between; both signs judged from frames and counts, not yet from a measured angle. Another lamp\'s own stand angle differs by a few degrees. The others: cad, zero_deg is the joint\'s deg (body.state, degrees from its home) at the CAD\'s rest pose, sign +1, until measured on a lamp',
      reach: 'product: lamp-body --reach around each lamp\'s home; a lamp\'s home is its own (/etc/lampOS/body/lamp.env)',
    },
    chain: order,
    base_mesh: '0_base',
    joints,
  };
}

const path = process.argv[2];
if (!path) {
  process.stderr.write('usage: node tools/lamp-model.js PATH/TO/lamp.glb\n');
  process.exit(2);
}
process.stdout.write(`${JSON.stringify(model(path), null, 2)}\n`);
