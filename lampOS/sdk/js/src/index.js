// lampos: program a lampOS lamp from your own computer (docs/control.md).
export { Following, Lamp, Lease } from './client.js';
export { LampError, NotPaired, WrongLamp, byCode, fromReply } from './errors.js';
export { Keyring } from './keys.js';
export { find } from './discovery.js';
export { fake } from './fake.js';
export * as schema from './schema.js';
