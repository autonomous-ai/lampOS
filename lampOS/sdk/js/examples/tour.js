// A tour of everything a computer can do with a lamp, one step at a time.
//
// Each step runs only with the grant it needs, and a refusal is printed with
// the lamp's own words, then the tour goes on. Try it against `lamp fake
// --everything` first, then your own lamp.
import { Lamp, LampError } from 'lampos';

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function step(name, action) {
  try {
    console.log(`${name}: ${JSON.stringify(await action())}`);
  } catch (refused) {
    if (!(refused instanceof LampError)) throw refused;
    console.log(`${name}: refused, ${refused.message}`);
  }
}

const lamp = await Lamp.connect();
console.log(`connected to ${lamp.info.name} (${lamp.info.id})`);
console.log(`grants: ${lamp.grants.join(', ')}`);
console.log(`limits: ${JSON.stringify(lamp.limits)}`);

// What the lamp can tell this computer now.
const states = ['light', 'mood', 'lamp', ...(lamp.grants.includes('look') ? ['presence'] : [])];
await step('state', () => lamp.get(...states));

// Events, by edge: printed for the rest of the tour.
const events = ['touch', 'button', 'lamp.changed'];
if (lamp.grants.includes('look')) events.push('person.arrived', 'person.left', 'hand');
for (const event of events) await lamp.on(event, (said) => console.log(`  heard ${JSON.stringify(said)}`));

if (lamp.grants.includes('light')) {
  await step('light', () => lamp.light('warm', 3, 5000));
  await sleep(1000);
  await step('light, breathing', () => lamp.light('blue', 3, 5000, 'breathe'));
}

if (lamp.grants.includes('speak')) await step('say', () => lamp.say('Hello from the tour.'));

if (lamp.grants.includes('body')) {
  await step('gesture', () => lamp.gesture('nod'));
  await sleep(3000);
  if (lamp.grants.includes('light')) {
    await step('express', () => lamp.express('glad'));
    await sleep(3000);
  }
  await step('body lease', () => lamp.withLease(['body'], async (lease) => {
    await step('look ahead', () => lamp.look('ahead'));
    await step('look to the side', () => lamp.look({ yaw: 20, pitch: 5 }));
    await step('joints', () => lamp.joints(lease, 1000, { pitch: 5 }));
    return 'let go';
  }, { why: 'the SDK tour' }));
}

await step('stop', () => lamp.stop());
await sleep(2000);
await lamp.close();
