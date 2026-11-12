// The controller app's pre-flight: presses every row of a running try-it page with its defaults, one by one, and writes
// what happened as a markdown table: PASS (the lamp took it), FAIL (it refused or did not answer: the words are the
// evidence), REFUSED (as designed: the lamp did not grant what the row takes, drawn off by the page or answered `no_grant`
// under this pairing: the reason is in the lamp's words) or n/a (not on this build, the run's flag is off, the turn was over,
// skipped by the person). A REFUSED does not fail the run: the exit code is 1 only for a FAIL. The same file runs on the virtual
// lamp and on a real one.
//
//   node examples/twin/preflight.js --url 'http://HOST:PORT/?t=TOKEN' [--out FILE] [--yes-moves] [--only call:ROW,call:ROW]
//
// `--only` presses just those rows (ids as the table writes them, in the page's order; any other id is an error, exit 2) and says
// `only: ...` in the table's header: to rerun a few rows on a lamp. Cleanup is unchanged.
//
// It speaks the page's own websocket with the URL's token, so it can do what a person at the page can and no more. In a
// real-lamp run a row that moves the body, or that shows the room (camera, microphone), is asked about first on the
// terminal (y/N) unless `--yes-moves` says yes to all of them; a "no" is n/a, not FAIL. It ends with this run's clip and notes
// taken off (the rows' own removes) and the volume put back: its level and mute are read before the first row and set back at the very end, both said in the table's header. The URL, with its token,
// is never printed or written; neither is a pairing code or pin (it never sees them).
import { createInterface } from 'node:readline';
import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { WebSocket } from 'ws';

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
// `work` or `value` after `ms`, whichever comes first; the timer is cleared either way. (A race against a bare sleep leaves its
// timer running: 49 rows leave 49 of them, and the process lives on until the longest ends.)
const within = (work, ms, value) => {
  let timer;
  return Promise.race([work, new Promise((resolve) => { timer = setTimeout(() => resolve(value), ms); })]).finally(() => clearTimeout(timer));
};
const cell = (text) => String(text ?? '').replace(/\|/g, '\\|').replace(/\s+/g, ' ').trim();

// What the result cell says: PASS, FAIL, REFUSED, n/a, or a PASS that the page's own claim did not bear out: the events the row said it
// would show and that were not seen are named, nothing more (whether a lamp or a sim should have brought it is for the reader).
export function resultText(row) {
  if (row.result !== 'PASS' || !row.missing?.length) return row.result;
  return `PASS (expected event not seen: ${row.missing.join(', ')})`;
}

const volumeText = ({ level, muted }) => `${level}${muted ? ' (muted)' : ''}`;

// The markdown of a run: the counts, then one line per row in the page's order.
export function tableOf({ lamp, mode, rows, at, only, volume }) {
  const count = (result) => rows.filter((r) => r.result === result).length;
  const unseen = rows.filter((r) => r.result === 'PASS' && r.missing?.length).length;
  return [
    `# Controller pre-flight: ${cell(lamp.name || lamp.id)} (${cell(lamp.id)}, ${mode}) ${at}`, '',
    ...(only ? [`only: ${only.join(', ')}`, ''] : []),
    ...(volume ? [`volume: ${volumeText(volume.before)} before the first row; ${volume.after ? `${volumeText(volume.after)} after the last${volume.after.level === volume.before.level && volume.after.muted === volume.before.muted ? ' (put back)' : ' (NOT put back)'}` : 'not read again'}`, ''] : []),
    `${count('PASS')} PASS${unseen ? ` (${unseen} with an expected event not seen)` : ''}, ${count('FAIL')} FAIL, ${count('REFUSED')} refused as designed, ${count('n/a')} n/a, of ${rows.length} rows`, '',
    '| row | result | expected events | evidence |', '| --- | --- | --- | --- |',
    ...rows.map((r) => `| ${cell(r.id)} | ${cell(resultText(r))} | ${cell(r.expected)} | ${cell(r.evidence)} |`), '',
  ].join('\n');
}

// One y/N question on the terminal; no terminal means no.
export function askYesNo(question, { input = process.stdin, output = process.stderr } = {}) {
  if (!input.isTTY) return Promise.resolve(false);
  return new Promise((resolve) => {
    const reader = createInterface({ input, output });
    reader.question(`${question} [y/N] `, (answer) => { reader.close(); resolve(/^y(es)?$/i.test(answer.trim())); });
  });
}

// The page's reason for a row it draws off because of a grant (describe() in controls.js): the only kind of "off" that is REFUSED.
const GRANT_REASON = /^the lamp did not grant /;
// Whether `answer` is the lamp's `no_grant` for a grant this pairing lacks. `grants` are the page's (hello's); a page that
// does not say them leaves any `no_grant` as a refusal by design.
export function grantRefusal(answer, grants) {
  const text = String(answer ?? '');
  if (!/^no_grant:|not granted/.test(text)) return false;
  if (!Array.isArray(grants)) return true;
  // The lamp names what it lacks after `not granted:`, one grant or a list.
  // (A line may go on after the names: `not granted: camera (the app does not grant it yet: ...)`.)
  const named = (/not granted: ([a-z_]+(?:, ?[a-z_]+)*)/.exec(text)?.[1] ?? '').split(/,\s*/).filter(Boolean);
  return named.length === 0 || named.some((grant) => !grants.includes(grant));
}

// The lamp's words for an op or a field its build does not know (lamp-control's parser: serde's `unknown variant ...` for an
// op, `unknown field ...` for a field of a newer SDK, `not routed yet` for a feature the build has not wired): not a failure of
// the row, a lamp from before the feature.
// A part the build has not wired yet says `not routed yet` (a `play` on lamp-sim, whose lamp-control waits for client packages).
export const NOT_ON_THIS_BUILD = /^(bad_request: .*(unknown variant|unknown field)|refused: not routed yet)/s;
export const notOnBuild = (answer) => NOT_ON_THIS_BUILD.test(String(answer ?? ''));

// `turn_end` after `turn_start`: the lamp's turn is often over already, and it says so in its own words; that is the turn
// being over, not the row failing.
export const TURN_ALREADY_OVER = /^(refused: spoken|busy: not this computer's turn)/;
// And the other way round: pressed right after `turn_start`, a real lamp says the turn has not begun; it begins a moment later.
export const TURN_NOT_BEGUN = /^busy: this computer's turn has not begun yet/;

// The events a row said it would show (`shows`), against the ones heard since the press: 'seen', or 'NOT SEEN: ...'. A
// real lamp's events can come seconds after the press (a person.left after its delay), so they are waited for.
export function expectedEvents(shows, heard) {
  if (!shows?.length) return '';
  const missing = shows.filter((name) => !heard.includes(name));
  return missing.length ? `NOT SEEN: ${missing.join(', ')}` : 'seen';
}

// Runs the pre-flight against the page at `url`. `log` gets one short line per row (never the url).
export async function runPreflight({ url, signal, yesMoves = false, ask = askYesNo, log = () => {}, settleMs = 2000, expectMs = 6000, retryMs = 8000, retryEveryMs = 1000, only, answerMs = 40_000, now = () => new Date().toISOString() }) {
  const page = new URL(url);
  const token = page.searchParams.get('t');
  if (!token) throw new Error('the url has no ?t= token');
  const base = `http://${page.host}`;
  const table = await (await fetch(`${base}/try.json?t=${encodeURIComponent(token)}`)).json();
  const socket = new WebSocket(`ws://${page.host}/live?t=${encodeURIComponent(token)}`, { headers: { origin: base } });
  const heard = [];
  const waiting = new Map();
  socket.on('message', (data) => {
    let message;
    try { message = JSON.parse(data); } catch { return; }
    if (message.type === 'result') waiting.get(message.id)?.(message);
    else if (message.type === 'event') heard.push({ event: message.event, at: Date.now() });
    else if (message.type === 'picture') heard.push({ event: `picture (${Buffer.from(message.jpeg ?? '', 'base64').length} bytes)`, at: Date.now() });
  });
  await new Promise((resolve, reject) => { socket.once('open', resolve); socket.once('error', reject); });
  const rows = [];
  let id = 0;
  const real = table.mode === 'real';
  let movesTold = false;
  // `only`: ids like call:say, in the page's order; one the page does not have is an error, said before anything is pressed.
  const known = [...table.controls.map((c) => `call:${c.id}`), ...table.stimuli.map((s) => `stim:${s.id}`)];
  if (only) {
    const unknown = only.filter((name) => !known.includes(name));
    if (unknown.length) { socket.close(); throw new Error(`--only: no such row ${unknown.join(', ')} (rows: ${known.join(', ')})`); }
  }
  // One row pressed for its answer (not a row of the table): `{ ok, call, answer }`, or null when it is not on this page or off.
  const quietly = async (name, args = {}) => {
    const entry = table.controls.find((c) => c.id === name);
    if (!entry || entry.disabled || socket.readyState !== WebSocket.OPEN) return null;
    id += 1;
    const mine = id;
    const answer = new Promise((resolve) => waiting.set(mine, resolve));
    socket.send(JSON.stringify({ type: 'call', id: mine, name, args }));
    const result = await within(answer, 8000, null);
    waiting.delete(mine);
    return result;
  };
  // The lamp's level and mute, as `get volume` answers them (`{"volume":{"level":N,"muted":B}}` after the arrow in its line).
  const volumeNow = async () => {
    const result = await quietly('get', { state: 'volume' });
    try {
      const said = JSON.parse(String(result?.call ?? '').split(' -> ').slice(1).join(' -> '));
      const volume = said.volume ?? said;
      return typeof volume.level === 'number' ? { level: volume.level, muted: Boolean(volume.muted) } : null;
    } catch { return null; }
  };
  // The person's own volume, read BEFORE any row is pressed, and put back at the very end (also after a Ctrl-C).
  const volumeBefore = only && !only.some((name) => /volume/.test(name)) ? null : await volumeNow();
  let volumeAfter = null;
  try {
    for (const [kind, list] of [['call', table.controls], ['stim', table.stimuli]]) {
      for (const entry of list) {
        if (signal?.aborted) break;
        const rowId = `${kind === 'call' ? 'call' : 'stim'}:${entry.id}`;
        if (only && !only.includes(rowId)) continue;
        if (entry.disabled) {
          // Off because the lamp did not grant what the row takes: refused as designed. Off for another reason (the run's flag):
          // n/a.
          const refused = GRANT_REASON.test(entry.disabled);
          rows.push({ id: rowId, result: refused ? 'REFUSED' : 'n/a', evidence: entry.disabled });
          log(`${rowId}: ${refused ? 'REFUSED' : 'n/a'}`);
          continue;
        }
        // A real lamp: what moves it or shows the room is the person's call, row by row.
        const touches = entry.moves || entry.needs?.some((g) => g === 'camera' || g === 'microphone');
        if (real && touches && !yesMoves && !(await ask(`Press ${rowId} (${entry.label}) on the REAL lamp?`))) {
          rows.push({ id: rowId, result: 'n/a', evidence: 'skipped: not confirmed by the person' });
          log(`${rowId}: skipped`);
          continue;
        }
        // The person's yes (the question just answered, or --yes-moves) is the page's "this moves the real lamp": the server holds
        // a move until it has been told, once per connection.
        if (real && entry.moves && !movesTold) { socket.send(JSON.stringify({ type: 'moves_ok' })); movesTold = true; }
        id += 1;
        const mine = id;
        const since = Date.now();
        const answer = new Promise((resolve) => waiting.set(mine, resolve));
        socket.send(JSON.stringify({ type: kind, id: mine, name: entry.id, args: {} }));
        let result = await within(answer, answerMs, { ok: false, answer: `no answer in ${answerMs / 1000} s` });
        waiting.delete(mine);
        // Only the sequence is waited for, never the row: `turn_end` is pressed again every second while the lamp says its turn has
        // not begun, for up to 8 s.
        let neverBegan = false;
        if (entry.id === 'turn_end') {
          const until = Date.now() + retryMs;
          while (!result.ok && TURN_NOT_BEGUN.test(String(result.answer)) && Date.now() < until) {
            await sleep(retryEveryMs);
            id += 1;
            const again = id;
            const next = new Promise((resolve) => waiting.set(again, resolve));
            socket.send(JSON.stringify({ type: 'call', id: again, name: entry.id, args: {} }));
            result = await within(next, answerMs, { ok: false, answer: `no answer in ${answerMs / 1000} s` });
            waiting.delete(again);
          }
          neverBegan = !result.ok && TURN_NOT_BEGUN.test(String(result.answer));
        }
        // The events it said it would show are waited for (up to expectMs after the press), the rest settle for settleMs.
        const heardNow = () => heard.filter((h) => h.at >= since).map((h) => h.event);
        const shows = entry.shows ?? [];
        if (shows.length) {
          while (Date.now() - since < expectMs && !shows.every((name) => heardNow().includes(name))) await sleep(100);
          await sleep(Math.min(settleMs, 300));
        } else await sleep(settleMs);
        const events = [...new Set(heardNow())];
        // A lamp from before the feature says so in its parser's words: n/a, with the words beside it.
        const missing = !result.ok && notOnBuild(result.answer);
        const over = !result.ok && entry.id === 'turn_end' && (TURN_ALREADY_OVER.test(String(result.answer)) || neverBegan);
        // The lamp refusing for a grant this pairing does not hold (`no_grant`, naming it) is the design working.
        const refusedForGrant = !result.ok && !missing && !over && grantRefusal(result.answer, table.grants);
        const verdict = result.ok ? 'PASS' : refusedForGrant ? 'REFUSED' : missing || over ? 'n/a' : 'FAIL';
        const said = missing ? `not on this lamp's build (the lamp said: ${String(result.answer).slice(0, 200)})`
          : over ? `${neverBegan ? 'the turn never began' : "the lamp's turn was over before end was pressed"} (the lamp said: ${result.answer})`
            : `${result.call ?? entry.id}${result.ok ? '' : ` -> ${result.answer}`}`;
        rows.push({ id: rowId, result: verdict, missing: verdict === 'PASS' ? shows.filter((name) => !events.includes(name)) : [], expected: expectedEvents(shows, events), evidence: [said, events.length ? `events: ${events.join(', ')}` : ''].filter(Boolean).join('; ') });
        log(`${rowId}: ${verdict}`);
      }
    }
  } finally {
    // However it ends (a finished run, a Ctrl-C), this run's own clip and note come off the lamp: the rows that remove them are
    // pressed again (they touch only what this run made, and say so when none is kept); then the page's own end does the rest.
    for (const name of ['sound_remove', 'notes_delete']) {
      const entry = table.controls.find((c) => c.id === name);
      if (!entry || entry.disabled || socket.readyState !== WebSocket.OPEN) continue;
      id += 1;
      const mine = id;
      const answer = new Promise((resolve) => waiting.set(mine, resolve));
      socket.send(JSON.stringify({ type: 'call', id: mine, name, args: {} }));
      await within(answer, 5000, null);
    }
    // The volume the rows left is put back to what it was before the first row (only what differs is pressed).
    if (volumeBefore) {
      let found = await volumeNow();
      if (found && found.level !== volumeBefore.level) { await quietly('volume_to', { to: volumeBefore.level }); found = await volumeNow(); }
      if (found && found.muted !== volumeBefore.muted) { await quietly('volume_mute', { mute: volumeBefore.muted ? 'on' : 'off' }); found = await volumeNow(); }
      volumeAfter = found;
    }
    socket.close();
  }
  const volume = volumeBefore ? { before: volumeBefore, after: volumeAfter } : null;
  return { markdown: tableOf({ lamp: table.lamp, mode: table.mode, rows, at: now(), only, volume }), rows, mode: table.mode, volume };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const get = (name) => { const k = args.indexOf(name); return k >= 0 ? args[k + 1] : undefined; };
  const url = get('--url');
  if (!url || args.some((a) => a.startsWith('--') && !['--url', '--out', '--yes-moves', '--only'].includes(a))) {
    console.error("usage: node examples/twin/preflight.js --url 'http://HOST:PORT/?t=TOKEN' [--out FILE] [--yes-moves] [--only call:ROW,call:ROW]");
    process.exit(2);
  }
  // A Ctrl-C stops pressing rows, still cleans this run's scratch off the lamp, and still writes what was done.
  const stopper = new AbortController();
  for (const signalName of ['SIGINT', 'SIGTERM']) process.on(signalName, () => stopper.abort());
  try {
    const { markdown, rows } = await runPreflight({ url, signal: stopper.signal, yesMoves: args.includes('--yes-moves'), only: get('--only')?.split(',').map((x) => x.trim()).filter(Boolean), log: (line) => console.error(line) });
    if (get('--out')) writeFileSync(get('--out'), markdown); else console.log(markdown);
    process.exit(stopper.signal.aborted ? 130 : rows.some((r) => r.result === 'FAIL') ? 1 : 0);
  } catch (error) {
    console.error(`preflight: ${String(error.message).replaceAll(url, '<the url>')}`);
    process.exit(2);
  }
}
