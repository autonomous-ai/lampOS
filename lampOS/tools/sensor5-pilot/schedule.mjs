import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';

export const sha256 = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
export function buildSchedule(protocol) {
  const wanted = ['P01:tune:P02', 'P02:tune:P01', 'P03:test:P04', 'P04:test:P03'];
  if (JSON.stringify(protocol.participants.map(p => `${p.alias}:${p.split}:${p.playback_speaker}`)) !== JSON.stringify(wanted)
      || JSON.stringify(protocol.distances_m) !== '[0.6,1,1.5]'
      || JSON.stringify(protocol.lighting) !== '["daylight","lamplight"]'
      || protocol.paired_repeats_per_condition !== 5 || protocol.clip_seconds !== 10
      || protocol.decision_midpoint_us !== 5000000 || protocol.playback_seconds_per_person !== 30) {
    throw new Error('The approved person split and capture counts are frozen; use a separately reviewed protocol for changes.');
  }
  const rows = [];
  for (const [personIndex, person] of protocol.participants.entries()) {
    for (const [lightIndex, lighting] of protocol.lighting.entries()) {
      for (const [distanceIndex, distance_m] of protocol.distances_m.entries()) {
        for (let repeat = 1; repeat <= 5; repeat++) {
          const order = (personIndex + lightIndex + distanceIndex + repeat) % 2 ? ['A', 'B'] : ['B', 'A'];
          const pair_id = `${person.alias}-${lightIndex}-${distanceIndex}-${repeat}`;
          for (const intended_class of order) rows.push({
            clip_id: `S5-${String(rows.length + 1).padStart(4, '0')}`,
            participant: person.alias, split: person.split, intended_class,
            distance_m, lighting, repeat, pair_id, seconds: 10, decision_midpoint_us: 5000000,
            playback_speaker: person.playback_speaker,
            playback_source_id: `S5-SOURCE-${person.playback_speaker}`,
            playback_start_us: ((repeat - 1) % 3) * 10000000,
            truth: 'unlabelled', recording: 'not_started'
          });
        }
      }
    }
  }
  if (rows.length !== 240 || new Set(rows.map(r => r.clip_id)).size !== 240) throw new Error('Invalid schedule');
  return rows;
}

export function consentTemplate(alias) {
  if (!/^P0[1-4]$/.test(alias)) throw new Error('Unknown participant alias');
  return `Participant alias: ${alias}\nConsent accepted: no\n\nI voluntarily agree to recording my face and voice for the sensor 5 speaking-face pilot, including use of my 30-second video as local background playback, and indefinite retention of original recordings, derivatives and labels on encrypted local team storage accessible to the team lead, Reviewer and Builder #3. No identifiable media or private labels will be uploaded to GitHub. I may decline or stop further capture; already captured evidence is retained under this agreed protocol.\n\nOnly after the participant has read and agreed, replace the acceptance line with: Consent accepted: yes\nParticipant confirmation/signature (local only):\nConfirmed at UTC:\nOperator alias:\n`;
}

export function createPlan(out, protocolPath) {
  const bytes = fs.readFileSync(protocolPath);
  const protocol = JSON.parse(bytes);
  const rows = buildSchedule(protocol);
  fs.mkdirSync(out, { mode: 0o700 });
  const write = (name, content) => fs.writeFileSync(path.join(out, name), content, { flag: 'wx', mode: 0o600 });
  write('protocol.json', bytes);
  write('schedule.jsonl', rows.map(r => JSON.stringify(r)).join('\n') + '\n');
  write('split.json', JSON.stringify({ schema_version: 1, protocol_sha256: sha256(bytes),
    fixed_aliases: protocol.participants, real_person_mapping: 'REQUIRED_LOCALLY_BEFORE_CAPTURE',
    tune: { A: 60, B: 60 }, test: { A: 60, B: 60 } }, null, 2) + '\n');
  write('people-local-template.json', JSON.stringify({ people: protocol.participants.map(p => ({
    alias: p.alias, split: p.split, distinct_person_local_reference: '', consent_record: `${p.alias}-consent.txt`
  })), instructions: 'Complete with four distinct consenting people BEFORE capture; keep this map local; never reassign using scores.' }, null, 2) + '\n');
  for (const p of protocol.participants) write(`${p.alias}-consent.txt`, consentTemplate(p.alias));
  write('README.md', 'This is an unrecorded Monday capture plan. The 240 main clips total 40 minutes; four separately recorded 30-second source videos bring planned retained footage to 42 minutes. The schedule is immutable and includes intended staging only, never ground truth. Complete four distinct local identities and consent records before capture; keep the original unfilled plan, then freeze a separately hashed signed registry. Each actual take uses a fresh output directory; all failures remain. Source playback speakers stay within their own tune/test half. No command in this plan opens hardware or starts playback. See the checked-in operator guide for live privacy prerequisites and label export.\n');
  const records = fs.readdirSync(out).sort().map(name => `${sha256(fs.readFileSync(path.join(out, name)))}  ${name}`);
  write('SHA256SUMS', records.join('\n') + '\n');
  return { directory: out, clips: rows.length, protocol_sha256: sha256(bytes),
    schedule_sha256: sha256(fs.readFileSync(path.join(out, 'schedule.jsonl'))), recorded: false };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    if (process.argv.length !== 4 || process.argv[2] !== '--out' || !path.isAbsolute(process.argv[3])) {
      throw new Error('Usage: node schedule.mjs --out /absolute/new/local-plan-directory (creates a plan only)');
    }
    console.log(JSON.stringify(createPlan(process.argv[3], new URL('./protocol.json', import.meta.url)), null, 2));
  } catch (error) { console.error(error.message); process.exitCode = 2; }
}
