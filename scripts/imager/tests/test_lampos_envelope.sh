#!/bin/bash
# Real Ed25519 signatures; no downloads, image mounts, root or installer runs.
set -euo pipefail
export LC_ALL=C
umask 077
test_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
helper="$test_dir/../lib/verify_lampos_envelope.sh"
work=$(mktemp -d)
passed=0
failed=0
finish() {
  local status=$?
  if [ "$status" -eq 0 ] && [ "$failed" -eq 0 ]; then
    rm -rf -- "$work"
  else
    printf 'Test files and failure logs retained at %s\n' "$work" >&2
  fi
}
trap finish EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

pass() {
  printf 'PASS: %s\n' "$1"
  passed=$((passed + 1))
}
fail() {
  printf 'FAIL: %s\n' "$1" >&2
  failed=$((failed + 1))
}

make_key() {
  local name=$1
  openssl genpkey -algorithm ED25519 -out "$work/$name.pem"
  openssl pkey -in "$work/$name.pem" -pubout -outform DER -out "$work/$name.der"
  tail -c 32 "$work/$name.der" > "$work/$name.raw"
  openssl base64 -A -in "$work/$name.raw" -out "$work/$name.pub"
}

make_envelope() {
  local manifest=$1 private_key=$2 key_id=$3 destination=$4
  openssl pkeyutl -sign -rawin -inkey "$private_key" -in "$manifest" \
    -out "$work/signature.raw"
  openssl base64 -A -in "$manifest" -out "$work/manifest.base64"
  openssl base64 -A -in "$work/signature.raw" -out "$work/signature.base64"
  jq -n --arg key "$key_id" --rawfile manifest "$work/manifest.base64" \
    --rawfile signature "$work/signature.base64" \
    '{format:"lampos-update/1", key:$key, manifest:$manifest, signature:$signature}' \
    > "$destination"
}

accept() {
  local name=$1 key=$2 envelope=$3 expected=$4
  printf 'previous output must be replaced only on success\n' > "$work/output"
  if bash "$helper" "$key" "$envelope" "$work/output" > "$work/$name.log" 2>&1 &&
    cmp -s "$expected" "$work/output"; then
    pass "$name"
  else
    fail "$name"
  fi
}

reject() {
  local name=$1 key=$2 envelope=$3
  local expected
  case "$name" in
    changed_manifest_bytes|removed_manifest_newlines|attacker_key_relabeled_as_trusted|tampered_signature)
      expected="the signature does not match the manifest" ;;
    wrong_trusted_key|wrong_key_fingerprint) expected="the envelope names another public key" ;;
    wrong_envelope_format) expected="the envelope format is not lampos-update/1" ;;
    truncated_signature) expected="the signature is not 64 bytes" ;;
    missing_base64_padding) expected="the signature is not canonical base64" ;;
    manifest_base64_whitespace|malformed_base64) expected="the manifest is not canonical base64" ;;
    truncated_public_key) expected="the public key is not 32 bytes" ;;
    malformed_public_key|noncanonical_public_key_padding_bits) expected="the public key is not canonical base64" ;;
    malformed_envelope_json|unknown_envelope_field|nonstring_envelope_field|multiple_envelope_roots|duplicate_envelope_field)
      expected="the envelope must contain exactly one of each of its four string fields" ;;
    signed_duplicate_manifest_object|signed_duplicate_nested_field) expected="the manifest contains duplicate fields" ;;
    signed_multiple_manifest_roots|signed_malformed_manifest) expected="the manifest is not one JSON object" ;;
    oversized_signed_manifest) expected="the manifest is too large" ;;
    oversized_envelope) expected="the envelope is too large" ;;
    oversized_public_key_text) expected="the public key is too large" ;;
    *) printf 'No expected refusal for %s\n' "$name" >&2; exit 2 ;;
  esac
  printf 'previous output must survive refusal byte for byte\n' > "$work/output"
  cp "$work/output" "$work/previous"
  if bash "$helper" "$key" "$envelope" "$work/output" > "$work/$name.log" 2>&1; then
    fail "$name (accepted)"
  elif ! cmp -s "$work/previous" "$work/output"; then
    fail "$name (changed previous output)"
  elif ! grep -Fq "$expected" "$work/$name.log"; then
    fail "$name (did not reach the intended refusal)"
  else
    pass "$name"
  fi
}

make_key trusted
make_key other
fingerprint=$(openssl dgst -sha256 -r "$work/trusted.raw" | cut -c 1-16)
# This helper authenticates bytes; the installer owns manifest schema checks.
# Multiple nested objects and arrays exercise duplicate-key traversal too.
printf '%s\n\n' '{"schema":1,"channel":"lamp-voice","sequence":1,"files":[{"name":"lamp-update","bytes":1},{"name":"lamp-voice","bytes":2}],"nested":{"first":{"value":1},"second":{"value":2}},"empty":{}}' \
  > "$work/manifest"
make_envelope "$work/manifest" "$work/trusted.pem" "$fingerprint" "$work/valid.json"
accept exact_bytes_and_trailing_newlines "$work/trusted.pub" "$work/valid.json" "$work/manifest"

# The updater trims all Unicode White_Space around the trusted key.
printf '\t\302\240' > "$work/spaced.pub"
cat "$work/trusted.pub" >> "$work/spaced.pub"
printf '\r\n\343\200\200' >> "$work/spaced.pub"
accept public_key_outer_whitespace "$work/spaced.pub" "$work/valid.json" "$work/manifest"

# One whitespace byte changes the signed message, even though JSON is equivalent.
cp "$work/manifest" "$work/tampered"
printf ' ' >> "$work/tampered"
openssl base64 -A -in "$work/tampered" -out "$work/tampered.base64"
jq --rawfile manifest "$work/tampered.base64" '.manifest = $manifest' \
  "$work/valid.json" > "$work/tampered.json"
reject changed_manifest_bytes "$work/trusted.pub" "$work/tampered.json"

# Removing trailing newlines must not accidentally recover the signed message.
tr -d '\n' < "$work/manifest" > "$work/stripped"
openssl base64 -A -in "$work/stripped" -out "$work/stripped.base64"
jq --rawfile manifest "$work/stripped.base64" '.manifest = $manifest' \
  "$work/valid.json" > "$work/stripped.json"
reject removed_manifest_newlines "$work/trusted.pub" "$work/stripped.json"
reject wrong_trusted_key "$work/other.pub" "$work/valid.json"

make_envelope "$work/manifest" "$work/other.pem" "$fingerprint" "$work/relabeled.json"
reject attacker_key_relabeled_as_trusted "$work/trusted.pub" "$work/relabeled.json"
jq '.key = "0000000000000000"' "$work/valid.json" > "$work/wrong-id.json"
reject wrong_key_fingerprint "$work/trusted.pub" "$work/wrong-id.json"
jq '.format = "lampos-update/2"' "$work/valid.json" > "$work/wrong-format.json"
reject wrong_envelope_format "$work/trusted.pub" "$work/wrong-format.json"

dd if=/dev/zero bs=64 count=1 2>/dev/null > "$work/bad-signature.raw"
openssl base64 -A -in "$work/bad-signature.raw" -out "$work/bad-signature.base64"
jq --rawfile signature "$work/bad-signature.base64" '.signature = $signature' \
  "$work/valid.json" > "$work/bad-signature.json"
reject tampered_signature "$work/trusted.pub" "$work/bad-signature.json"
head -c 63 "$work/signature.raw" > "$work/short-signature.raw"
openssl base64 -A -in "$work/short-signature.raw" -out "$work/short-signature.base64"
jq --rawfile signature "$work/short-signature.base64" '.signature = $signature' \
  "$work/valid.json" > "$work/short-signature.json"
reject truncated_signature "$work/trusted.pub" "$work/short-signature.json"
jq '.signature |= rtrimstr("=")' "$work/valid.json" > "$work/unpadded.json"
reject missing_base64_padding "$work/trusted.pub" "$work/unpadded.json"
jq '.manifest += "\n"' "$work/valid.json" > "$work/wrapped-base64.json"
reject manifest_base64_whitespace "$work/trusted.pub" "$work/wrapped-base64.json"
jq '.manifest = "!!!!"' "$work/valid.json" > "$work/bad-base64.json"
reject malformed_base64 "$work/trusted.pub" "$work/bad-base64.json"

head -c 31 "$work/trusted.raw" > "$work/short-key.raw"
openssl base64 -A -in "$work/short-key.raw" -out "$work/short.pub"
reject truncated_public_key "$work/short.pub" "$work/valid.json"
printf 'not a base64 public key\n' > "$work/malformed.pub"
reject malformed_public_key "$work/malformed.pub" "$work/valid.json"
# A changed unused padding bit can decode to the same bytes in OpenSSL.
jq -Rrj '
  . as $key | "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/" as $alphabet |
  ($alphabet | index($key[42:43])) as $index |
  $key[0:42] + $alphabet[$index + 1:$index + 2] + "="
' "$work/trusted.pub" > "$work/noncanonical.pub"
reject noncanonical_public_key_padding_bits "$work/noncanonical.pub" "$work/valid.json"

printf '{"format":' > "$work/malformed.json"
reject malformed_envelope_json "$work/trusted.pub" "$work/malformed.json"
jq '.extra = true' "$work/valid.json" > "$work/unknown.json"
reject unknown_envelope_field "$work/trusted.pub" "$work/unknown.json"
jq '.signature = null' "$work/valid.json" > "$work/null.json"
reject nonstring_envelope_field "$work/trusted.pub" "$work/null.json"
cat "$work/valid.json" "$work/valid.json" > "$work/two-roots.json"
reject multiple_envelope_roots "$work/trusted.pub" "$work/two-roots.json"
# The final key repeats an earlier field using an escaped JSON spelling.
sed '$d' "$work/valid.json" > "$work/duplicate.json"
printf ', "k\\u0065y": "%s"}\n' "$fingerprint" >> "$work/duplicate.json"
reject duplicate_envelope_field "$work/trusted.pub" "$work/duplicate.json"

printf '{"first":{"a":1},"first":{"b":2}}\n' > "$work/duplicate-manifest"
make_envelope "$work/duplicate-manifest" "$work/trusted.pem" "$fingerprint" \
  "$work/duplicate-manifest.json"
reject signed_duplicate_manifest_object "$work/trusted.pub" "$work/duplicate-manifest.json"
printf '{"items":[{"value":1,"value":2}]}\n' > "$work/duplicate-nested"
make_envelope "$work/duplicate-nested" "$work/trusted.pem" "$fingerprint" \
  "$work/duplicate-nested.json"
reject signed_duplicate_nested_field "$work/trusted.pub" "$work/duplicate-nested.json"
printf '{"schema":1}\n{"schema":1}\n' > "$work/multiple-manifests"
make_envelope "$work/multiple-manifests" "$work/trusted.pem" "$fingerprint" \
  "$work/multiple-manifests.json"
reject signed_multiple_manifest_roots "$work/trusted.pub" "$work/multiple-manifests.json"
printf '{"schema":\n' > "$work/broken-manifest"
make_envelope "$work/broken-manifest" "$work/trusted.pem" "$fingerprint" \
  "$work/broken-manifest.json"
reject signed_malformed_manifest "$work/trusted.pub" "$work/broken-manifest.json"

# Exercise the exact decoded bound with a signed JSON object, then one extra byte.
printf '{"pad":"' > "$work/limit-manifest"
dd if=/dev/zero bs=262133 count=1 2>/dev/null | tr '\000' x >> "$work/limit-manifest"
printf '"}\n' >> "$work/limit-manifest"
[ "$(wc -c < "$work/limit-manifest")" -eq 262144 ]
make_envelope "$work/limit-manifest" "$work/trusted.pem" "$fingerprint" "$work/limit.json"
accept manifest_size_limit "$work/trusted.pub" "$work/limit.json" "$work/limit-manifest"
printf '\n' >> "$work/limit-manifest"
make_envelope "$work/limit-manifest" "$work/trusted.pem" "$fingerprint" "$work/too-big.json"
reject oversized_signed_manifest "$work/trusted.pub" "$work/too-big.json"

cp "$work/valid.json" "$work/limit-envelope.json"
envelope_padding=$((524288 - $(wc -c < "$work/limit-envelope.json")))
dd if=/dev/zero bs="$envelope_padding" count=1 2>/dev/null | tr '\000' ' ' >> "$work/limit-envelope.json"
accept envelope_size_limit "$work/trusted.pub" "$work/limit-envelope.json" "$work/manifest"
cp "$work/valid.json" "$work/large-envelope.json"
dd if=/dev/zero bs=524289 count=1 2>/dev/null | tr '\000' ' ' >> "$work/large-envelope.json"
reject oversized_envelope "$work/trusted.pub" "$work/large-envelope.json"
cp "$work/trusted.pub" "$work/large.pub"
dd if=/dev/zero bs=4097 count=1 2>/dev/null | tr '\000' ' ' >> "$work/large.pub"
reject oversized_public_key_text "$work/large.pub" "$work/valid.json"

rm -f "$work/output"
if bash "$helper" "$work/trusted.pub" "$work/tampered.json" "$work/output" \
  > "$work/no-output-on-failure.log" 2>&1; then
  fail "no_output_on_failure (accepted)"
elif [ -e "$work/output" ]; then
  fail "no_output_on_failure (created output)"
elif ! grep -Fq 'the signature does not match the manifest' "$work/no-output-on-failure.log"; then
  fail "no_output_on_failure (did not reach signature refusal)"
else
  pass no_output_on_failure
fi

printf 'Tests: %s passed, %s failed\n' "$passed" "$failed"
[ "$failed" -eq 0 ]
