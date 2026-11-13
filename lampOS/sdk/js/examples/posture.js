// A posture nudge: when someone has sat at the desk for a long stretch, the lamp
// makes one slow small tilt of its head, once, and says nothing.
//
// The stretch starts at an arrival and ends when the person leaves; after
// `SIT_AFTER_S` seconds (default 50 minutes) of it, the lamp tilts and counts the
// nudge in today's note (`posture-<day>`, by this program's own clock). One nudge
// per stretch: a lamp that fidgets every few minutes is a metronome. A refused
// tilt (the lamp is busy with a conversation, private, paused) is simply let go.
//
// Needs the `look` (arrivals), `body` (the tilt) and `notes` grants. Try it on the
// virtual lamp: `SIT_AFTER_S=20 node posture.js`, then `lamp-sim people 0 DIR`,
// wait about 5 s, `lamp-sim people 1 DIR` (an arrival), and wait 20 s.
import { Lamp, LampError } from 'lampos';

const sitAfterMs = Number(process.env.SIT_AFTER_S ?? 50 * 60) * 1000;
// Degrees from the standing pose: a small slow tip of the head and back.
const TILT = [{ roll: 3, pitch: 1 }, { roll: -2 }];

// The day key by this program's own clock: an event's `at` is UTC.
const today = () => {
  const now = new Date();
  return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`;
};

const lamp = await Lamp.connect();
let sitting = null; // the timer for this stretch, null while nobody is there

const nudge = async () => {
  sitting = null;
  try {
    await lamp.withLease(['body'], async () => {
      await lamp.movePoses(TILT, { holdMs: 600, wait: true });
    }, { why: 'a posture nudge' });
    const key = `posture-${today()}`;
    const count = (await lamp.notesGet(key, 0)) + 1;
    await lamp.notesSet(key, count);
    console.log(`nudge ${count} today`);
  } catch (refused) {
    if (refused instanceof LampError) console.log(`not now: ${refused.message}`); // busy, private, paused, not standing...
    else throw refused;
  }
};

await lamp.on('person.arrived', () => {
  if (sitting === null) sitting = setTimeout(nudge, sitAfterMs);
});
await lamp.on('person.left', () => {
  clearTimeout(sitting);
  sitting = null;
});
console.log(`waiting for a stretch of ${Math.round(sitAfterMs / 60000)} min; Ctrl-C to stop`);
await lamp.closed();
