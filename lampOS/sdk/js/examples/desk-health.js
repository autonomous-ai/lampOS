// Desk health: keeps count of how long the air at the desk was stuffy today,
// and asks for a window once, not every few seconds.
//
// When the air turns stuffy this waits two minutes (a breath of a room is not
// a problem), then glows a soft orange and says one short line; when the air
// clears it puts its own light out with `lightOff` and says nothing. The
// minutes of stuffy air are kept in a note per day, so a restart does not
// forget them. Needs the `air`, `light`, `speak` and `notes` grants. Try it on
// the virtual lamp: `lamp-sim air co2_ppm=1500`, then `lamp-sim air co2_ppm=600`.
import { Lamp, LampError } from 'lampos';

const waitMs = Number(process.env.DESK_STUFFY_AFTER_S ?? 120) * 1000; // stuffy this long before a word
const renewMs = 25_000; // a light is held 30 s at most, so it is sent again before that
const stuffy = ['stuffy', 'very_stuffy'];

// The day key by this program's own clock: an event's `at` is UTC.
const today = () => {
  const now = new Date();
  return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`;
};

const lamp = await Lamp.connect();
let since = null; // when the air turned stuffy, null while it is not
let nudged = false; // said once per stuffy spell
let glowing = false;
let backoff = false; // the last light was refused: wait a renew, not a tick
let wake = () => {};

// Add a spell of stuffy air to today's note.
const keep = async (seconds) => {
  const key = `stuffy-${today()}`;
  const total = (await lamp.notesGet(key, 0)) + seconds / 60;
  await lamp.notesSet(key, Math.round(total * 10) / 10);
  console.log(`today the air was stuffy ${Math.round(total * 10) / 10} min`);
};

await lamp.on('air.changed', async (event) => {
  if (stuffy.includes(event.feel)) {
    if (since === null) {
      since = Date.now();
      console.log(`air: ${event.co2_ppm ?? '?'} ppm, stuffy, waiting ${waitMs / 1000} s`);
    }
  } else if (since !== null) {
    const spell = (Date.now() - since) / 1000;
    since = null;
    try {
      await keep(spell);
    } catch (refused) {
      if (refused instanceof LampError) console.log(`not now: ${refused.message}`); // notes paused, over its limits...
      else throw refused;
    }
  }
  wake(); // an event wakes the loop, so a clear air puts the light out at once
});
console.log('watching the air; Ctrl-C to stop');

while (true) {
  // Wake for an event, for the end of the wait, or to renew the light.
  let left = since === null ? renewMs : waitMs - (Date.now() - since);
  if (backoff) [left, backoff] = [renewMs, false];
  await new Promise((resolve) => {
    const timer = setTimeout(resolve, Math.max(100, Math.min(renewMs, left)));
    wake = () => { clearTimeout(timer); resolve(); };
  });
  try {
    if (since !== null && Date.now() - since >= waitMs) {
      if (!nudged) {
        nudged = true;
        await lamp.say('It is getting stuffy in here. A window would help.');
      }
      await lamp.light('orange', 2, 30_000, 'breathe');
      glowing = true;
    } else if (since === null && glowing) {
      await lamp.lightOff(); // only the light this program put on
      glowing = false;
      nudged = false;
    }
  } catch (refused) {
    if (!(refused instanceof LampError)) throw refused;
    console.log(`not now: ${refused.message}`); // private, paused, asleep...
    backoff = true;
  }
}
