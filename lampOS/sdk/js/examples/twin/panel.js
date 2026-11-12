// The try-it panel (served only with --stim-dir), drawn from the table in controls.js (the server sends it as
// /try.json): one row per SDK call (a button and its fields), one row per thing that happens in the room, and the
// lamp's own events. Nothing here is written per feature: a new SDK call is a new entry in controls.js.
const send = (message) => window.twinSend?.(message);
let counter = 0;
// The row each press came from, for a picture it asks to show (answered to this page only, under that row).
const rowOf = new Map();
const el = (tag, props = {}, ...children) => {
  const node = Object.assign(document.createElement(tag), props);
  for (const child of children) node.append(child);
  return node;
};
document.head.append(el('style', { textContent: `
  #panel { margin-top: 10px; font-size: 13px; }
  #panel h2 { font-size: 13px; margin: 12px 0 6px; color: var(--soft); text-transform: uppercase; letter-spacing: .04em; }
  #panel h3 { font-size: 12px; margin: 8px 0 2px; color: var(--soft); font-weight: 600; }
  #panel .row { display: flex; flex-wrap: wrap; gap: 6px; margin: 4px 0; align-items: center; }
  #panel .row label { display: inline-flex; gap: 4px; align-items: center; color: var(--soft); }
  #panel button { cursor: pointer; }
  #panel input[type=text] { flex: 1; min-width: 120px; }
  #panel input[type=number] { width: 64px; }
  #panel .mark { font-size: 11px; padding: 1px 6px; opacity: .7; } #panel .on-pass { background: #2e7d32; color: #fff; opacity: 1; } #panel .on-fail { background: #c62828; color: #fff; opacity: 1; }
  #panel .note { max-width: 160px; font-size: 11px; }
  #panel .world button { border-style: dashed; }
  #panel .why { flex-basis: 100%; font-size: 11px; color: #b26a00; margin: -2px 0 2px 2px; }
  #panel button:disabled { opacity: .5; cursor: not-allowed; }
  #panel .hint { flex-basis: 100%; font-size: 11px; color: var(--soft); margin: -2px 0 2px 2px; }
  #panel img.shot { flex-basis: 100%; max-width: 320px; border-radius: 6px; border: 1px solid var(--line); }
  #events { height: 220px; overflow: auto; font: 12px/1.35 ui-monospace, monospace; border: 1px solid var(--line); border-radius: 6px; padding: 6px; white-space: pre-wrap; }
  #events .ok { color: var(--accent); } #events .no { color: #d9534f; } #events .ev { color: var(--soft); }
` }));

// The lamp's own names and the table of buttons, from the server.
const table = await (await fetch(`/try.json${location.search}`)).json();
// The checklist (checklist.js, asked for with the page's own query: the token), kept in this origin's localStorage.
const { keyFor, openChecklist, exportChecklist, rowsOf, tickable } = await import(`/checklist.js${location.search}`);
const rows = rowsOf(table);
const checklist = openChecklist({ storage: (() => { try { return window.localStorage; } catch { return null; } })(), key: keyFor(table.lamp.id, table.mode), rowIds: tickable(rows) });
const ticked = el('span', { id: 'ticked' });
const refresh = [];
const showCount = () => { const { ticked: n, of } = checklist.count(); ticked.textContent = `${n} of ${of} ticked`; };

// One input for a field, with its limits and default.
function input(field) {
  if (field.type === 'select') {
    const node = el('select');
    for (const option of field.options) node.append(el('option', { value: option, textContent: option === '' ? '(none)' : option }));
    node.value = field.default ?? field.options[0];
    return node;
  }
  if (field.type === 'number') return el('input', { type: 'number', min: field.min, max: field.max, value: field.default ?? field.min, step: field.integer ? 1 : 'any' });
  return el('input', { type: 'text', value: field.default ?? '' });
}

// The checklist's controls of one row: PASS, FAIL (pressing the one set clears it) and a note.
function marks(id) {
  const pass = el('button', { textContent: 'PASS', className: 'mark' });
  const fail = el('button', { textContent: 'FAIL', className: 'mark' });
  const note = el('input', { type: 'text', placeholder: 'note', className: 'note', maxLength: 500 });
  const draw = () => {
    const mark = checklist.get(id);
    pass.className = `mark${mark?.result === 'pass' ? ' on-pass' : ''}`;
    fail.className = `mark${mark?.result === 'fail' ? ' on-fail' : ''}`;
    note.value = mark?.note ?? '';
    showCount();
  };
  const set = (result) => { checklist.set(id, { result: checklist.get(id)?.result === result ? null : result, note: note.value }); draw(); };
  pass.onclick = () => set('pass');
  fail.onclick = () => set('fail');
  note.onchange = () => { checklist.set(id, { result: checklist.get(id)?.result ?? null, note: note.value }); draw(); };
  refresh.push(draw);
  draw();
  return [pass, fail, note];
}

// The download of the record: JSON and a markdown table, made here from the rows and the marks, nothing else.
function download(name, type, text) {
  const link = el('a', { href: URL.createObjectURL(new Blob([text], { type })), download: name });
  // In the document while it is clicked: some browsers ignore a click on a link that is not.
  document.body.append(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(link.href), 1000);
}
const exportBar = el('div', { className: 'row' }, ticked,
  el('button', { textContent: 'Export JSON', onclick: () => download('try-checklist.json', 'application/json', exportChecklist({ lamp: table.lamp, mode: table.mode, rows, marks: checklist.marks() }).json) }),
  el('button', { textContent: 'Export table', onclick: () => download('try-checklist.md', 'text/markdown', exportChecklist({ lamp: table.lamp, mode: table.mode, rows, marks: checklist.marks() }).markdown) }),
  el('button', { textContent: 'Clear ticks', onclick: () => { if (window.confirm('Clear every tick and note for this lamp?')) { checklist.clear(); refresh.forEach((draw) => draw()); } } }));

// A row: the fields (named), then the button, which sends the entry's id and the values.
function row(entry, kind) {
  const inputs = entry.fields.map((field) => [field, input(field)]);
  const values = () => Object.fromEntries(inputs.map(([field, node]) => [field.name, field.type === 'number' ? Number(node.value) : node.value]));
  const children = inputs.map(([field, node]) => (field.type === 'text' ? node : el('label', {}, field.label ?? field.name, node)));
  const node = el('div', { className: 'row' });
  children.push(el('button', {
    textContent: entry.label,
    disabled: Boolean(entry.disabled),
    onclick: () => {
      rowOf.set(counter + 1, node);
      send({ type: kind, id: ++counter, name: entry.id, args: values() });
    },
  }));
  // What the row does that its label does not say, under it (textContent: never markup).
  if (typeof entry.hint === 'string') children.push(el('div', { className: 'hint', textContent: entry.hint }));
  // A row the lamp or the run cannot do: drawn, off, with its reason, and no PASS/FAIL (it is n/a in the export).
  if (entry.disabled) {
    node.append(...children, el('div', { className: 'why', textContent: `n/a: ${entry.disabled}` }));
    return node;
  }
  node.append(...children, ...marks(`${kind === 'stim' ? 'stim' : 'call'}:${entry.id}`));
  return node;
}

// The entries of one list, under a small heading for each group in the order they first appear.
function section(entries, kind) {
  const nodes = [];
  let group;
  for (const entry of entries) {
    if (entry.group !== group) { group = entry.group; nodes.push(el('h3', { textContent: group })); }
    nodes.push(row(entry, kind));
  }
  return nodes;
}

const events = el('div', { id: 'events' });
const say = (cls, text) => {
  events.prepend(el('div', { className: cls, textContent: `${new Date().toLocaleTimeString()} ${text}` }));
  while (events.childElementCount > 200) events.lastElementChild.remove();
};

const panel = el('section', { id: 'panel' },
  el('h2', { textContent: 'The lamp, through the SDK' }),
  el('p', { className: 'sub', textContent: `Checklist for ${table.lamp.name || table.lamp.id} (${table.mode}): PASS or FAIL each row, add a note, then export. Ticks stay in this browser for this lamp; the export is the record.` }),
  exportBar,
  ...section(table.controls, 'call'),
  el('div', { className: 'world' },
    el('h2', { textContent: 'The room (not the SDK)' }),
    el('p', { className: 'sub', textContent: 'The room starts empty. The lamp calls someone an arrival only after 5 s with nobody there.' }),
    ...section(table.stimuli, 'stim')),
  el('h2', { textContent: 'What the lamp says' }),
  el('p', { className: 'sub', textContent: table.camera || table.microphone
    ? `${[table.camera && 'Camera', table.microphone && 'Microphone'].filter(Boolean).join(' and ')} ON for this run: the page can show the room to anyone on your network with this link (plain HTTP). The lamp sees the room only while it stands up (press a gesture first); folded down its picture is black.`
    : 'Camera and microphone are off: the lamp sees and hears only what the buttons above make. The log shows each line the lamp says out loud (lamp.said, the speech grant).' }),
  events,
);
// The panel first, the joint sliders (still there) folded away below it.
const joints = document.getElementById('joints');
const fold = el('details', {}, el('summary', { textContent: 'Joints (drag a slider to move the body)' }));
joints.before(panel);
panel.after(fold);
fold.append(joints);

// A picture the lamp gave for a press: under its row, replacing the last, in memory only (a blob URL), never logged.
function showPicture({ id, jpeg }) {
  const node = rowOf.get(id);
  if (!node || typeof jpeg !== 'string') return;
  let image = node.querySelector('img.shot');
  if (!image) { image = el('img', { className: 'shot', alt: 'what the lamp sent' }); node.append(image); }
  const bytes = Uint8Array.from(atob(jpeg), (c) => c.charCodeAt(0));
  const old = image.src;
  image.src = URL.createObjectURL(new Blob([bytes], { type: 'image/jpeg' }));
  if (old.startsWith('blob:')) URL.revokeObjectURL(old);
}

// Hear the lamp in this page (F3): off on every load and never remembered; one click makes the page's one AudioContext (a browser
// needs a click before any sound), plays a 100 ms test blip and tells the server. What it plays is made by this app: a hum that is a
// stand-in, a clip's own audio, and the lamp's own words spoken by the browser's voice. A reconnect puts it back to off.
const hearing = (() => {
  const available = table.mode !== 'real';
  let on = false;
  let context = null;
  let language = 'en';
  const button = el('button', { id: 'hear', disabled: !available });
  const note = el('div', { className: 'hint', textContent: available
    ? 'Sound here is made by this page, not by the lamp. Off by default; the microphone is never played.'
    : 'A real lamp speaks through its own speaker.' });
  const draw = () => { button.textContent = available ? `Hear the lamp in this page: ${on ? 'on' : 'off'}` : 'Hear the lamp in this page: a real lamp speaks through its own speaker'; };
  const set = (value) => { on = value && available; if (!on) window.speechSynthesis?.cancel(); draw(); };
  const blip = () => {
    const oscillator = context.createOscillator();
    const gain = context.createGain();
    gain.gain.value = 0.03;
    oscillator.frequency.value = 880;
    oscillator.connect(gain).connect(context.destination);
    oscillator.start();
    oscillator.stop(context.currentTime + 0.1);
  };
  button.onclick = async () => {
    if (!available) return;
    if (on) { set(false); send({ type: 'hear', on: false }); return; }
    try {
      const Context = window.AudioContext ?? window.webkitAudioContext;
      if (!Context) throw new Error('this browser has no audio');
      context ??= new Context();
      await context.resume();
      blip();
      // A phone's browser speaks only after a gesture: one silent line, now, lets the first lamp.said be heard.
      if (typeof window.SpeechSynthesisUtterance === 'function' && window.speechSynthesis) {
        const unlock = new window.SpeechSynthesisUtterance(' ');
        unlock.volume = 0;
        window.speechSynthesis.speak(unlock);
      }
    } catch (error) {
      say('no', `no sound in this page: ${error?.message ?? error}`);
      return;
    }
    set(true);
    send({ type: 'hear', on: true });
  };
  draw();
  return {
    node: el('div', { className: 'row', id: 'hear-box' }, button, note),
    get on() { return on; },
    off: () => set(false),
    // The lamp's language (a code like en, vi or pt-BR), said in the server's answer to the switch.
    language: (value) => { if (typeof value === 'string' && /^[a-z]{2,3}(-[A-Za-z0-9]{2,8})*$/.test(value)) language = value; },
    // A WAV the server made for this page, played once (the bytes stay in memory only).
    async play({ what, wav }) {
      if (!on || !context || typeof wav !== 'string') return;
      try {
        const bytes = Uint8Array.from(atob(wav), (c) => c.charCodeAt(0));
        // Decoding takes the bytes' buffer from us: the size is read first.
        const size = bytes.length;
        const buffer = await context.decodeAudioData(bytes.buffer);
        const source = context.createBufferSource();
        source.buffer = buffer;
        source.connect(context.destination);
        source.start();
        say('ev', `playing ${what} here (${size.toLocaleString('en-US')} bytes)`);
      } catch (error) {
        say('no', `could not play ${what} here: ${error?.message ?? error}`);
      }
    },
    // The lamp's own words, in the browser's voice and the lamp's language.
    speak(text) {
      if (!on || typeof text !== 'string' || !text) return;
      const synth = window.speechSynthesis;
      if (!synth || typeof window.SpeechSynthesisUtterance !== 'function') { say('no', "this browser has no speech voice: the line is only in the log"); return; }
      const utterance = new window.SpeechSynthesisUtterance(text);
      utterance.lang = language;
      synth.speak(utterance);
    },
  };
})();
// Above the log it controls (the page has one column here).
events.before(hearing.node);
window.addEventListener('twin-open', () => hearing.off());

window.addEventListener('twin-message', ({ detail }) => {
  if (detail.type === 'picture') showPicture(detail);
  if (detail.type === 'audio') hearing.play(detail);
  if (detail.type === 'hear') hearing.language(detail.language);
  if (detail.type === 'hear' && !detail.on) { hearing.off(); if (detail.why) say('no', `no sound here: ${detail.why}`); }
  if (detail.type === 'event' && detail.event === 'lamp.said') hearing.speak(detail.frame?.text);
  if (detail.type === 'result') say(detail.ok ? 'ok' : 'no', `${detail.call} -> ${detail.answer}`);
  if (detail.type === 'event') say('ev', `event ${detail.event} ${JSON.stringify(detail.frame)}`);
});
