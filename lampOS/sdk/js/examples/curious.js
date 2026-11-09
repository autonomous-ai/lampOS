// A curious look of your own, from three poses in degrees: up a little, then a
// little left and a little right, and back. The SDK eases it under the body's
// caps; a pose the body can't make is refused here, before anything is sent.
import { Lamp, LampError } from 'lampos';

// Degrees from the standing pose: pitch up, yaw right. The body holds each
// joint to its own bounds (`await lamp.get('body')`).
const CURIOUS = [{ pitch: 3 }, { pitch: 2, yaw: -3 }, { pitch: 2, yaw: 3 }];

const lamp = await Lamp.connect();
try {
  await lamp.withLease(['body'], async () => {
    const done = await lamp.movePoses(CURIOUS, { holdMs: 400, wait: true });
    console.log('looked:', done);
  }, { why: 'a curious look' });
} catch (refused) {
  if (refused instanceof LampError) console.log(`not now: ${refused.message}`); // busy, private, not standing...
  else throw refused;
} finally {
  await lamp.close();
}
