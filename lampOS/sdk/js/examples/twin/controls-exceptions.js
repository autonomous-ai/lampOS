// The ops and events of control.schema.json that no entry of controls.js names yet, ONE list, each with its reason.
// test/controls-cover.test.js fails for any op or event that is in neither the entries nor here, and for a line here
// whose op or event an entry now names (delete the line when the row is written). Reasons: 'row not written yet'
// (the SDK has the method), 'SDK internal' (the client does it itself), or what keeps a row from being made.
const YET = 'row not written yet';
export const COVERAGE_EXCEPTIONS = [
  { kind: 'op', name: 'pair', reason: 'SDK internal: Lamp.pair runs it once, before any row' },
  { kind: 'op', name: 'hello', reason: 'SDK internal: Lamp.connect sends it; its answer is lamp.language and the grants' },
  { kind: 'op', name: 'subscribe', reason: 'SDK internal: lamp.on() sends it for the event log' },
  { kind: 'op', name: 'unsubscribe', reason: 'SDK internal: lamp.off() sends it' },
  { kind: 'op', name: 'get', reason: `${YET} (lamp.get, air, device, bodyInfo)` },
  { kind: 'op', name: 'joints', reason: `${YET} (lamp.joints, under a body lease)` },
  { kind: 'op', name: 'volume', reason: `${YET} (lamp.volume)` },
  { kind: 'op', name: 'listen.set', reason: `${YET} (lamp.setListening)` },
  { kind: 'op', name: 'lease.renew', reason: `${YET} (a Lease renews itself inside withLease; no row asks for it alone)` },
  { kind: 'op', name: 'service.read', reason: 'the virtual lamp has no service to read (needs a <service>-read grant and lamp-services)' },
  { kind: 'op', name: 'service.write', reason: 'the virtual lamp has no service to write (needs a <service>-write grant and lamp-services)' },
  { kind: 'event', name: 'person.stood_up', reason: 'lamp-sim has no verb for a person standing up yet' },
  { kind: 'event', name: 'person.facing', reason: 'lamp-sim has no verb for a person facing the lamp yet' },
  { kind: 'event', name: 'name.heard', reason: 'lamp-sim has no name.heard verb yet (L1 plans a canned one)' },
  { kind: 'event', name: 'button', reason: 'lamp-sim has no button verb' },
  { kind: 'event', name: 'volume.changed', reason: `${YET}: the volume row is the one that shows it` },
  { kind: 'event', name: 'body.state', reason: 'drawn by the twin itself, not logged (twelve a second)' },
  { kind: 'event', name: 'time', reason: 'noise: not logged, and nothing a button makes' },
];
