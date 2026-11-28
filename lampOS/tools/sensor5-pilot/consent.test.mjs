import test from 'node:test';
import assert from 'node:assert/strict';
import { consentRecord } from './consent.mjs';
import { consentTemplate } from './schedule.mjs';
const date = '2026-11-30T02:00:00Z';
const signed = consentTemplate('P01').replace('Consent accepted: no', 'Consent accepted: yes')
  .replace('Participant confirmation/signature (local only):', 'Participant confirmation/signature (local only): SYNTHETIC TEST ONLY');
test('signed record binds statement, participant and operator', () => {
  const r = consentRecord(signed, 'P01', 'O01', date);
  assert.equal(r.accepted, true); assert.equal(r.participant_alias, 'P01');
  assert.equal(r.operator_alias, 'O01'); assert.equal(r.consent_text_sha256.length, 64);
});
test('blank templates, wrong alias, duplicate acceptance and missing confirmation fail', () => {
  for (const [text, alias] of [[consentTemplate('P01'), 'P01'], [signed, 'P02'],
    [signed + '\nConsent accepted: yes\n', 'P01'], [signed.replace('SYNTHETIC TEST ONLY', ''), 'P01']]) {
    assert.throws(() => consentRecord(text, alias, 'O01', date));
  }
});
test('unknown role and invalid acceptance date fail', () => {
  assert.throws(() => consentRecord(signed, 'P01', 'P02', date));
  assert.throws(() => consentRecord(signed, 'P01', 'O01', 'Monday'));
});
