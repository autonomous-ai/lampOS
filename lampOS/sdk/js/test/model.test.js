// The lamp's model: the packaged copy is the repository's, it is what the
// tool writes from the rigged CAD, and its joints are the protocol's.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { hardware, model, schema } from '../src/index.js';

const repoModel = new URL('../../../docs/lamp.model.json', import.meta.url);
const repoHardware = new URL('../../../docs/lamp.hardware.json', import.meta.url);
const glb = new URL('../../../../robots/lamp/hardware/cad/glb/lamp.glb', import.meta.url);

test('the packaged model is the repository\'s', (t) => {
  if (!existsSync(repoModel)) return t.skip('docs/lamp.model.json is not beside this copy');
  assert.deepEqual(JSON.parse(readFileSync(repoModel, 'utf8')), model);
});

test('the model is what the tool writes from the rigged CAD', (t) => {
  // A clone without Git LFS has a pointer file there, not the CAD.
  if (!existsSync(glb) || readFileSync(glb).toString('latin1', 0, 4) !== 'glTF') {
    return t.skip('lamp.glb is not here (Git LFS not pulled)');
  }
  const tool = fileURLToPath(new URL('../tools/lamp-model.js', import.meta.url));
  const written = execFileSync(process.execPath, [tool, fileURLToPath(glb)], { encoding: 'utf8' });
  assert.deepEqual(JSON.parse(written), model);
});

test('the model\'s joints are the joints op\'s, one chain from the base', () => {
  const targets = Object.keys(schema.schema.$defs.joints.properties.targets.properties).sort();
  assert.deepEqual(model.joints.map((joint) => joint.name).sort(), targets);
  assert.deepEqual([...model.chain].sort(), targets);
  assert.equal(model.joints.find((joint) => joint.name === model.chain[0]).parent, 'base');
  for (const joint of model.joints) {
    assert.equal(joint.axis.reduce((sum, v) => sum + Math.abs(v), 0), 1, `${joint.name}: one axis`);
    assert.equal(joint.origin_m.length, 3);
  }
  assert.equal(model.measured.origins, 'cad');
});

test('the packaged hardware is the repository\'s, and every part says its source', (t) => {
  if (!existsSync(repoHardware)) return t.skip('docs/lamp.hardware.json is not beside this copy');
  assert.deepEqual(JSON.parse(readFileSync(repoHardware, 'utf8')), hardware);
  for (const [part, facts] of Object.entries(hardware)) {
    if (facts && typeof facts === 'object' && !Array.isArray(facts)) assert.ok(facts.source, `${part} says no source`);
  }
  assert.deepEqual([...hardware.joints.names].sort(), model.joints.map((joint) => joint.name).sort());
});
