#!/usr/bin/env bash
# Signed OTA metadata helpers; source from release scripts. Keep the signing key outside the repo.

# Signing is opt-in, but a partial configuration is always an error.
ota_metadata_signing_preflight() {
  if [ -z "${OTA_SIGNING_PRIVATE_KEY:-}" ] && [ -z "${OTA_SIGNING_KEY_ID:-}" ]; then
    [ "${OTA_METADATA_SIGNED_ONLY:-0}" != 1 ] || { echo 'ERROR: signed-only metadata requires signing credentials' >&2; return 1; }
    return 0
  fi
  [ -n "${OTA_SIGNING_PRIVATE_KEY:-}" ] && [ -n "${OTA_SIGNING_KEY_ID:-}" ] \
    || { echo 'ERROR: configure both OTA_SIGNING_PRIVATE_KEY and OTA_SIGNING_KEY_ID' >&2; return 1; }
  [ -r "$OTA_SIGNING_PRIVATE_KEY" ] || { echo 'ERROR: cannot read OTA_SIGNING_PRIVATE_KEY' >&2; return 1; }
  openssl pkey -in "$OTA_SIGNING_PRIVATE_KEY" -passin pass: -pubout -text_pub -noout 2>/dev/null | grep -q 'ED25519' \
    || { echo 'ERROR: OTA_SIGNING_PRIVATE_KEY must be a valid Ed25519 private key' >&2; return 1; }
}

# Never turn an existing signed feed into unsigned metadata implicitly.
ota_metadata_preserve_trust() {
  if [ -z "${OTA_SIGNING_PRIVATE_KEY:-}" ] && \
      jq -e 'has("signed") or .format == "autonomous-ota/v1"' "$1" >/dev/null; then
    echo 'ERROR: existing signed OTA feed requires signing credentials; refusing unsigned downgrade' >&2
    return 1
  fi
}

ota_base64_decode() {
  base64 --decode 2>/dev/null || base64 -D
}

# ota_metadata_unpack <envelope> <payload>
ota_metadata_unpack() {
  local envelope="$1" payload="$2"
  if jq -er '(.signed // .) | .format == "autonomous-ota/v1" and .signature.algorithm == "ed25519" and (.payload | type == "string")' "$envelope" >/dev/null; then
    jq -r '(.signed // .).payload' "$envelope" | ota_base64_decode >"$payload" \
      || { echo "ERROR: decode signed OTA metadata payload" >&2; return 1; }
  else
    cp "$envelope" "$payload"
  fi
  jq -e . "$payload" >/dev/null || { echo "ERROR: OTA metadata payload is not JSON" >&2; return 1; }
}

# ota_metadata_sign <payload> <envelope>
# OTA_SIGNING_PRIVATE_KEY: PEM Ed25519 key path; OTA_SIGNING_KEY_ID is a label only.
ota_metadata_sign() {
  local payload="$1" envelope="$2" signature payload_b64 signature_b64
  if [ -z "${OTA_SIGNING_PRIVATE_KEY:-}" ]; then
    echo "WARN: OTA signing key is not configured; publishing legacy unsigned metadata" >&2
    cp "$payload" "$envelope"
    return 0
  fi
  : "${OTA_SIGNING_KEY_ID:?OTA_SIGNING_KEY_ID is required when OTA_SIGNING_PRIVATE_KEY is set}"
  [ -r "$OTA_SIGNING_PRIVATE_KEY" ] || { echo "ERROR: cannot read OTA_SIGNING_PRIVATE_KEY" >&2; return 1; }

  signature=$(mktemp)
  trap 'rm -f "$signature"' RETURN
  openssl pkeyutl -sign -rawin -inkey "$OTA_SIGNING_PRIVATE_KEY" -in "$payload" -out "$signature" \
    || { echo "ERROR: sign OTA metadata" >&2; return 1; }
  payload_b64=$(base64 <"$payload" | tr -d '\n')
  signature_b64=$(base64 <"$signature" | tr -d '\n')
    # Top-level entries are a compat copy for legacy workers; OTA_METADATA_SIGNED_ONLY=1
    # drops it so unmigrated devices stop updating rather than trust unsigned data.
  local base='.'
  if [ "${OTA_METADATA_SIGNED_ONLY:-0}" = "1" ]; then
    base='{}'
  fi
  jq --arg payload "$payload_b64" --arg signature "$signature_b64" --arg keyID "$OTA_SIGNING_KEY_ID" \
    "$base"' + {signed:{format:"autonomous-ota/v1", payload:$payload, signature:{algorithm:"ed25519", key_id:$keyID, value:$signature}}}' \
    "$payload" >"$envelope"
}

# ota_artifact_sha256 <file>
ota_artifact_sha256() {
  shasum -a 256 "$1" | awk '{print $1}'
}

# Read one immutable GCS generation, then publish only if it is still current.
# A competing publisher must be retried from a fresh read; never overwrite its
# promotion or activation changes with an older snapshot.
ota_metadata_fetch() {
  local url="$1" destination="$2" metadata generation
  OTA_METADATA_SOURCE=''
  OTA_METADATA_GENERATION=''
  metadata=$(gsutil stat "$url") || return 1
  generation=$(printf '%s\n' "$metadata" | awk '$1 == "Generation:" {print $2}')
  [[ "$generation" =~ ^[0-9]+$ ]] || { echo 'ERROR: missing GCS metadata generation' >&2; return 1; }
  gsutil cp "${url}#${generation}" "$destination" || return 1
  OTA_METADATA_SOURCE="$url"
  OTA_METADATA_GENERATION="$generation"
}

ota_metadata_publish() {
  local source="$1" url="$2"
  [[ "${OTA_METADATA_SOURCE:-}" == "$url" && "${OTA_METADATA_GENERATION:-}" =~ ^[0-9]+$ ]] \
    || { echo 'ERROR: metadata was not read successfully; refusing publication' >&2; return 1; }
  gsutil -h "x-goog-if-generation-match:${OTA_METADATA_GENERATION}" \
    -h 'Content-Type:application/json' -h 'Cache-Control:no-cache, no-store, must-revalidate' \
    cp "$source" "$url" \
    || { echo 'ERROR: metadata publication failed or a concurrent release changed the feed; retry from a fresh read' >&2; return 1; }
}
