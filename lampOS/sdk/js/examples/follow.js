// A desk camera's helper: while someone sits facing the lamp, it looks at them;
// when they leave, it lets go and the lamp's own life comes back. Needs the
// `look` grant. A hand on the head or a conversation takes the body back, and
// this waits for the next arrival.
import { Lamp, LampError } from 'lampos';

const sleep = (s) => new Promise((r) => setTimeout(r, s * 1000));
const lamp = await Lamp.connect();
let here = false;
await lamp.on('person.arrived', () => { here = true; });
await lamp.on('person.left', (event) => { if (!event.count) here = false; });

for (;;) {
  while (!here) await sleep(1);
  try {
    await lamp.withLease(['body'], async (lease) => {
      while (here && lease.held) {
        await lamp.look('face');
        await sleep(2); // look again now and then, not on and on
      }
    }, { why: 'looking at the person at the desk' });
  } catch (refused) {
    if (!(refused instanceof LampError)) throw refused;
    console.log(`not now: ${refused.message}`);
    await sleep(10);
  }
}
