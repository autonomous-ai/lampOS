// The try-it panel (served only with --stim-dir): buttons that drive the lamp through the SDK, buttons that
// act on the room (lamp-sim's stimuli), and the lamp's own events. The twin's server does the calls; each
// button is one SDK call or one stimulus, and the log says which, and the lamp's answer.
const send = (message) => window.twinSend?.(message);
let counter = 0;
const el = (tag, props = {}, ...children) => {
  const node = Object.assign(document.createElement(tag), props);
  for (const child of children) node.append(child);
  return node;
};
const style = el('style', { textContent: `
  #panel { margin-top: 10px; font-size: 13px; }
  #panel h2 { font-size: 13px; margin: 12px 0 6px; color: var(--soft); text-transform: uppercase; letter-spacing: .04em; }
  #panel .row { display: flex; flex-wrap: wrap; gap: 6px; margin: 4px 0; align-items: center; }
  #panel button { cursor: pointer; }
  #panel input[type=text] { flex: 1; min-width: 120px; }
  #panel input[type=number] { width: 64px; }
  #panel .world button { border-style: dashed; }
  #events { height: 220px; overflow: auto; font: 12px/1.35 ui-monospace, monospace; border: 1px solid var(--line); border-radius: 6px; padding: 6px; white-space: pre-wrap; }
  #events .ok { color: var(--accent); } #events .no { color: #d9534f; } #events .ev { color: var(--soft); }
` });
document.head.append(style);

const button = (label, kind, name, args = () => ({})) => el('button', { textContent: label, onclick: () => send({ type: kind, id: ++counter, name, args: args() }) });
const select = (options, value) => { const s = el('select'); for (const o of options) s.append(el('option', { value: o, textContent: o })); if (value) s.value = value; return s; };

// The lamp's own names (the SDK's schema), from the server.
const names = await (await fetch(`/try.json${location.search}`)).json();
const gesture = select(names.gestures);
const colour = select(names.colours, 'warm');
const level = el('input', { type: 'number', min: 1, max: 5, value: 3 });
const pattern = select(['', ...names.patterns]);
const words = el('input', { type: 'text', value: 'Hello, I am the virtual lamp.' });
const yaw = el('input', { type: 'number', min: -180, max: 180, value: 20 });
const pitch = el('input', { type: 'number', min: -90, max: 90, value: 0 });
const feeling = select(['glad', 'lively', 'low']);
const phrase = el('input', { type: 'text', value: 'good night lamp' });
const people = el('input', { type: 'number', min: 0, max: 5, value: 1 });
const soundKind = select(['music', 'laughter', 'door', 'footsteps']);

const events = el('div', { id: 'events' });
const say = (cls, text) => {
  events.prepend(el('div', { className: cls, textContent: `${new Date().toLocaleTimeString()} ${text}` }));
  while (events.childElementCount > 200) events.lastElementChild.remove();
};

const panel = el('section', { id: 'panel' },
  el('h2', { textContent: 'The lamp, through the SDK' }),
  el('div', { className: 'row' }, gesture, button('Gesture', 'call', 'gesture', () => ({ name: gesture.value })), feeling, button('Express', 'call', 'express', () => ({ feeling: feeling.value }))),
  el('div', { className: 'row' }, colour, el('span', { textContent: 'level' }), level, pattern,
    button('Light', 'call', 'light', () => ({ colour: colour.value, level: Number(level.value), pattern: pattern.value || undefined })),
    button('Light off', 'call', 'light_off')),
  el('div', { className: 'row' }, words, button('Say', 'call', 'say', () => ({ text: words.value }))),
  el('div', { className: 'row' }, el('span', { textContent: 'yaw' }), yaw, el('span', { textContent: 'pitch' }), pitch,
    button('Look', 'call', 'look', () => ({ yaw: Number(yaw.value), pitch: Number(pitch.value) })),
    button('Move poses', 'call', 'move'), button('Sound', 'call', 'sound'), button('Stop', 'call', 'stop')),
  el('div', { className: 'world' },
    el('h2', { textContent: 'The room (not the SDK)' }),
    el('p', { className: 'sub', textContent: 'The room starts empty. The lamp calls someone an arrival only after 5 s with nobody there.' }),
    el('div', { className: 'row' }, el('span', { textContent: 'people' }), people, button('Set people', 'stim', 'people', () => ({ n: Number(people.value) })),
      button('Someone arrives', 'stim', 'people', () => ({ n: 1 })), button('Everyone leaves', 'stim', 'people', () => ({ n: 0 }))),
    el('div', { className: 'row' }, phrase, button('Hear it', 'stim', 'hear', () => ({ text: phrase.value }))),
    el('div', { className: 'row' }, button('Tap', 'stim', 'touch', () => ({ kind: 'tap' })), button('Pet', 'stim', 'touch', () => ({ kind: 'pet' })), button('A hand', 'stim', 'hand'),
      soundKind, button('Make the sound', 'stim', 'sound', () => ({ kind: soundKind.value }))),
    el('div', { className: 'row' }, button('Stuffy air', 'stim', 'air', () => ({ kind: 'stuffy' })), button('Clear air', 'stim', 'air', () => ({ kind: 'clear' })))),
  el('h2', { textContent: 'What the lamp says' }),
  el('p', { className: 'sub', textContent: 'Camera and microphone are off: the lamp sees and hears only what the buttons above make.' }),
  events,
);
// The panel first, the joint sliders (still there) folded away below it.
const joints = document.getElementById('joints');
const fold = el('details', {}, el('summary', { textContent: 'Joints (drag a slider to move the body)' }));
joints.before(panel);
panel.after(fold);
fold.append(joints);

window.addEventListener('twin-message', ({ detail }) => {
  if (detail.type === 'result') say(detail.ok ? 'ok' : 'no', `${detail.call} -> ${detail.answer}`);
  if (detail.type === 'event') say('ev', `event ${detail.event} ${JSON.stringify(detail.frame)}`);
});
