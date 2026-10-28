// Hand signs: hold an open hand up to the lamp and its head follows it; take
// the hand away, or close it, and the lamp lets go and its own life comes
// back. Needs the `body` and `look` grants. A hand on the head or a
// conversation takes the body back at once; this then waits for the next
// open hand.
import { Lamp, LampError } from 'lampos';

const sleep = (s) => new Promise((r) => setTimeout(r, s * 1000));
const lamp = await Lamp.connect();
// The lamp tells a hand by change, and `seen: false` once it's gone.
let shown = false;
await lamp.on('hand', (event) => { shown = event.seen && event.open; });

for (;;) {
  while (!shown) await sleep(0.2);
  try {
    const following = await lamp.followHand({ why: 'following the hand it is shown' }).start();
    while (shown && following.following) await sleep(0.1);
    const ended = following.following ? await following.stop() : await following.wait();
    console.log(`let go: ${ended.why === 'stopped' ? 'the hand dropped' : ended.why}`);
    await following.stop();
  } catch (refused) {
    if (!(refused instanceof LampError)) throw refused;
    console.log(`not now: ${refused.message}`);
    await sleep(10);
  }
}
