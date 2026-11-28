import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';

export function consentRecord(text, participant, operator, acceptedAt) {
  if (!/^P0[1-4]$/.test(participant) || !/^O(?:0[1-9]|[1-9][0-9])$/.test(operator)) throw new Error('Use participant P01–P04 and operator O01–O99 aliases.');
  const lines = text.split(/\r?\n/);
  const exactly = wanted => lines.filter(line => line === wanted).length === 1;
  if (!exactly(`Participant alias: ${participant}`) || !exactly('Consent accepted: yes')
      || lines.some(line => /^Consent accepted:/.test(line) && line !== 'Consent accepted: yes')
      || !lines.some(line => /^Participant confirmation\/signature \(local only\):\s*\S/.test(line))) {
    throw new Error('The actual local signed/confirmed text must name this alias and accept consent; a blank template is not consent.');
  }
  if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,3})?Z$/.test(acceptedAt) || !Number.isFinite(Date.parse(acceptedAt))) throw new Error('Supply the actual acceptance time as UTC RFC3339.');
  return { schema_version: 1, participant_alias: participant, operator_alias: operator,
    accepted: true, scope: 'sensor5-monday-local-av', accepted_at_utc: acceptedAt,
    consent_text_sha256: crypto.createHash('sha256').update(text).digest('hex') };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const args = process.argv.slice(2);
    if (args.length !== 10) throw new Error('Usage: node consent.mjs --text LOCAL_SIGNED_TEXT --participant P01 --operator O01 --accepted-at UTC_RFC3339 --out NEW_LOCAL_JSON');
    const pairs = new Map();
    for (let i = 0; i < args.length; i += 2) {
      if (!['--text', '--participant', '--operator', '--accepted-at', '--out'].includes(args[i]) || pairs.has(args[i])) throw new Error('Unknown/duplicate argument');
      pairs.set(args[i], args[i + 1]);
    }
    const source = pairs.get('--text'), output = pairs.get('--out');
    if (!path.isAbsolute(source) || !path.isAbsolute(output) || fs.statSync(source).size > 65536) throw new Error('Use absolute local paths and a bounded consent record.');
    const record = consentRecord(fs.readFileSync(source, 'utf8'), pairs.get('--participant'), pairs.get('--operator'), pairs.get('--accepted-at'));
    const bytes = JSON.stringify(record, null, 2) + '\n';
    fs.writeFileSync(output, bytes, { flag: 'wx', mode: 0o600 });
    console.log(JSON.stringify({ output, sha256: crypto.createHash('sha256').update(bytes).digest('hex'),
      statement: 'Recorded supplied operator-confirmed consent; no identity verification or capture occurred.' }, null, 2));
  } catch (error) { console.error(error.message); process.exitCode = 2; }
}
