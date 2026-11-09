// The camera: save its newest picture, then watch a short stream and save
// the last frame. Needs a pairing with the `camera` grant; the lamp refuses
// both while it is private or its camera is closed.
//
//     node examples/camera.js [seconds]    # 3 by default
import { writeFileSync } from 'node:fs';
import { Lamp, LampError } from 'lampos';

const seconds = Number(process.argv[2] ?? 3);

// A fast stream's frame carries the JPEG as `data` (a Buffer); otherwise it
// is the eyes' newest frame, as `jpeg` in base64.
const jpegOf = (frame) => frame.data ?? Buffer.from(frame.jpeg, 'base64');

const refused = (step, error) => {
  if (!(error instanceof LampError)) throw error;
  console.log(`${step}: refused, ${error.message}`); // no `camera` grant, private, camera closed...
  return 1;
};

async function main(lamp) {
  let picture;
  try {
    picture = await lamp.cameraSnapshot();
  } catch (error) {
    return refused('snapshot', error);
  }
  writeFileSync('snapshot.jpg', picture);
  console.log(`snapshot: ${picture.length} bytes, saved as snapshot.jpg`);

  const frames = [];
  let ended;
  const endedOnce = new Promise((resolve) => { ended = resolve; });
  await lamp.on('camera.frame', (frame) => frames.push(frame));
  await lamp.on('camera.ended', () => ended(true));
  let answer;
  try {
    answer = await lamp.cameraStream(true, 5);
  } catch (error) {
    return refused('stream', error);
  }
  console.log(`stream: on, fast=${answer.fast ?? false}`);
  let timer;
  const byLamp = await Promise.race([endedOnce, new Promise((resolve) => { timer = setTimeout(resolve, seconds * 1000, false); })]);
  clearTimeout(timer);
  if (byLamp) console.log('stream: the lamp ended it (private, or the camera closed)');
  else await lamp.cameraStream(false);
  if (!frames.length) {
    console.log('stream: no frame came');
    return 1;
  }
  writeFileSync('last-frame.jpg', jpegOf(frames.at(-1)));
  console.log(`stream: ${frames.length} frames in ${seconds} s, the last saved as last-frame.jpg`);
  return 0;
}

const lamp = await Lamp.connect();
try {
  process.exitCode = await main(lamp);
} finally {
  await lamp.close();
}
