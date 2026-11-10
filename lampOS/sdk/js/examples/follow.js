// A desk camera's helper: while someone sits facing the lamp, it looks at them;
// when they leave, it lets go and the lamp's own life comes back. Needs the
// `look` grant. A hand on the head or a conversation takes the body back, and
// this waits for the next arrival.
//
// Needs the next build: a lamp-control before it (11091 and older) refuses
// `look` at a face ("not in this release: look at a face or a name",
// docs/control.md), so there this prints that refusal and tries again 10 s
// later. With it, each look is one turn toward the face in view; with none in
// view it ends `dropped`, "no face in view", and the lamp stays as it is.
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
