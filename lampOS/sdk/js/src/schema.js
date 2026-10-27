// The v1 protocol's names, read from its schema (docs/control.schema.json).
// The package carries its own copy; a test keeps it equal to the repository's.
import { readFileSync } from 'node:fs';

export const VERSION = 1;
export const PORT = 7731;
export const SERVICE = 'lampos';

export const schema = JSON.parse(readFileSync(new URL('./control.schema.json', import.meta.url), 'utf8'));
const defs = schema.$defs;
const named = (ref) => defs[ref.$ref.split('/').pop()];

export const ops = defs.request.oneOf.map((ref) => named(ref).properties.op.const);
export const events = defs.event_name.enum;
export const states = defs.state_name.enum;
export const gestures = defs.gesture.enum;
export const colours = defs.colour.enum;
export const patterns = defs.pattern.enum;
export const grants = defs.grant.enum;
export const parts = defs.part.enum;
export const errorCodes = defs.error.properties.code.enum;
