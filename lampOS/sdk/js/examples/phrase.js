// Say "good night lamp" and a script of yours runs: here, a dim warm light and a
// nod. The lamp tells this computer only which phrase it heard, never what else
// was said. Needs the `hear` grant.
import { Lamp } from 'lampos';

const lamp = await Lamp.connect();
await lamp.on('phrase.heard', async () => {
  await lamp.light('warm', 2, 30_000, 'breathe'); // a breath needs level 2 or up
  await lamp.gesture('nod');
}, { phrases: ['good night lamp'] });
await lamp.closed();
