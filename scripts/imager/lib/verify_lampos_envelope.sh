#!/bin/bash
# Authenticate bootstrap bytes before an image builder trusts their download URL
# or hash. Keep this wire format and its bounds aligned with update/manifest.rs.
set -euo pipefail
export LC_ALL=C
umask 077

fail() {
  printf 'ERROR: lampOS envelope: %s\n' "$*" >&2
  exit 1
}

[ "$#" -eq 3 ] || fail "usage: $0 KEY_PUB_FILE ENVELOPE_JSON VERIFIED_MANIFEST_FILE"
key_file=$1
envelope_file=$2
manifest_file=$3
[ -f "$key_file" ] || fail "the public key is not a regular file"
[ -f "$envelope_file" ] || fail "the envelope is not a regular file"
[ ! -d "$manifest_file" ] || fail "the output is a directory"

# A temporary file beside the destination makes the final rename atomic.
# No unauthenticated bytes are ever written to the destination.
output_dir=$(dirname -- "$manifest_file")
work=$(mktemp -d "$output_dir/.lampos-envelope.XXXXXX")
trap 'rm -rf -- "$work"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

bounded_copy() {
  local source=$1 destination=$2 limit=$3 label=$4
  # Read at most limit + 1 bytes even if a source grows while it is read.
  head -c "$((limit + 1))" "$source" > "$destination" ||
    fail "cannot read $label"
  [ "$(wc -c < "$destination")" -le "$limit" ] ||
    fail "$label is too large"
}

# The updater permits 512 KiB envelopes and 256 KiB decoded manifests.
# The public-key text has an additional 4 KiB input bound; after trimming
# whitespace it must be the same canonical base64 32-byte key as the updater.
bounded_copy "$key_file" "$work/key.text" 4096 "the public key"
bounded_copy "$envelope_file" "$work/envelope.json" 524288 "the envelope"

# jq normally collapses duplicate object keys. Streaming exposes every field,
# including escaped spellings, so reject ambiguity before reading any values.
jq -n -e --stream '
  [inputs | select(length == 2)] as $fields |
  ($fields | length) == 4 and
  all($fields[];
    (.[0] | length) == 1 and
    (.[0][0] | type) == "string" and
    (.[1] | type) == "string") and
  ([$fields[][0][0]] | sort) == ["format", "key", "manifest", "signature"]
' "$work/envelope.json" > /dev/null 2>&1 ||
  fail "the envelope must contain exactly one of each of its four string fields"
jq -e '.format == "lampos-update/1"' "$work/envelope.json" > /dev/null ||
  fail "the envelope format is not lampos-update/1"

# Rust str::trim uses the Unicode White_Space characters, not just spaces.
jq -Rrsj '
  gsub("^[\u0009-\u000d\u0020\u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]+|[\u0009-\u000d\u0020\u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]+$"; "")
' "$work/key.text" > "$work/key.base64"
jq -rj '.manifest' "$work/envelope.json" > "$work/manifest.base64"
jq -rj '.signature' "$work/envelope.json" > "$work/signature.base64"

decode_standard() {
  local encoded=$1 decoded=$2 label=$3
  openssl base64 -d -A -in "$encoded" -out "$decoded" ||
    fail "cannot decode $label"
  # Re-encoding rejects bad alphabet, whitespace, missing padding and unused
  # nonzero padding bits even when OpenSSL accepts them. Rust STANDARD is strict.
  openssl base64 -A -in "$decoded" -out "$decoded.canonical"
  cmp -s "$encoded" "$decoded.canonical" ||
    fail "$label is not canonical base64"
}

decode_standard "$work/key.base64" "$work/key.raw" "the public key"
[ "$(wc -c < "$work/key.raw")" -eq 32 ] || fail "the public key is not 32 bytes"
decode_standard "$work/signature.base64" "$work/signature.raw" "the signature"
[ "$(wc -c < "$work/signature.raw")" -eq 64 ] || fail "the signature is not 64 bytes"
decode_standard "$work/manifest.base64" "$work/manifest.raw" "the manifest"
[ "$(wc -c < "$work/manifest.raw")" -le 262144 ] || fail "the manifest is too large"

fingerprint=$(openssl dgst -sha256 -r "$work/key.raw" | cut -c 1-16)
jq -e --arg fingerprint "$fingerprint" '.key == $fingerprint' \
  "$work/envelope.json" > /dev/null || fail "the envelope names another public key"

# DER SubjectPublicKeyInfo for Ed25519 is this fixed prefix and the raw key.
printf '\060\052\060\005\006\003\053\145\160\003\041\000' > "$work/key.der"
cat "$work/key.raw" >> "$work/key.der"
openssl pkeyutl -verify -pubin -keyform DER -inkey "$work/key.der" \
  -rawin -in "$work/manifest.raw" -sigfile "$work/signature.raw" \
  > /dev/null 2>&1 || fail "the signature does not match the manifest"

# Authentication does not replace the installer's schema and inventory checks.
# Require one unambiguous JSON object for the builder's jq readers. In a jq
# stream a one-element event closes its parent container. Remember each newly
# entered path so duplicate object-valued fields cannot hide behind disjoint
# child fields. Array indexes are distinct paths and need no special case.
jq -n -e '[inputs] | length == 1 and (.[0] | type) == "object"' \
  "$work/manifest.raw" > /dev/null 2>&1 || fail "the manifest is not one JSON object"
jq -n --stream -e '
  reduce inputs as $event ({open: [], seen: {}};
    if ($event | length) == 1 then
      .open = $event[0][0:-2]
    else
      $event[0] as $path |
      (.open | length) as $depth |
      reduce range($depth + 1; ($path | length) + 1) as $length (.;
        ($path[0:$length] | tojson) as $name |
        if .seen[$name] then error("duplicate manifest field")
        else .seen[$name] = true end
      ) |
      .open = $path[0:-1]
    end
  ) | true
' "$work/manifest.raw" > /dev/null 2>&1 || fail "the manifest contains duplicate fields"

# Never reserialize the signed bytes or put them in command substitution:
# either would change bytes, including trailing newlines.
chmod 0644 "$work/manifest.raw"
[ ! -d "$manifest_file" ] || fail "the output is a directory"
mv -f -- "$work/manifest.raw" "$manifest_file"
