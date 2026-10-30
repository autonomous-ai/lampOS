// The lamp's kinematic model (docs/lamp.model.json, written from the rigged
// CAD by tools/lamp-model.js): its joints under the protocol's names, the
// chain from the base out, each joint's origin, axis and reach, and what is
// measured and what is still the CAD's. The package carries its own copy; a
// test keeps it equal to the repository's.
import { readFileSync } from 'node:fs';

export const model = JSON.parse(readFileSync(new URL('./lamp.model.json', import.meta.url), 'utf8'));
// The lamp's parts as data (docs/lamp.hardware.json): board, servos, camera,
// microphones, speaker, light, touch, buttons, air and power, each with where
// its facts come from.
export const hardware = JSON.parse(readFileSync(new URL('./lamp.hardware.json', import.meta.url), 'utf8'));
