// Start a turn with a question: the lamp asks it, the person answers the lamp.
// The person's words stay on the lamp; this computer hears only how the turn
// ended. Needs the speak and hear grants.
import { Lamp, LampError } from 'lampos';

const lamp = await Lamp.connect();
try {
  const done = await lamp.startTurn("Shall we go over today's list?", { wait: true });
  console.log(`the turn ended: ${done.how} ${done.why ?? ''}`.trim());
} catch (refused) {
  if (refused instanceof LampError) console.log(`not now: ${refused.message}`); // quiet hours, private, no grant...
  else throw refused;
} finally {
  await lamp.close();
}
