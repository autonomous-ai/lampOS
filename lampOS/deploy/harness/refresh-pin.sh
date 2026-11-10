#!/usr/bin/env bash
# Moves the build's own Harness pin to the newest Harness on its CDN (#5957).
#
# The pin is crates/update/src/harness_install/product.rs: byte-for-byte
# copies, in this folder, of Harness's CLI manifest, its installer and the
# four runtime manifests the installer reads, each compiled in with its
# SHA-256 (#5229). This script downloads all six again from the URLs
# product.rs says they were copied from (ORIGIN, INSTALLER_ORIGIN, each
# MANIFESTS origin) into a fresh temporary folder and checks them. When the
# CLI manifest's `cli.version` is newer than product.rs's VERSION, it writes
# the new copies here under the existing names' pattern (the version in the
# name; the installer by its SHA-256's first 8 hex digits), removes the old
# ones, and rewrites product.rs's file names, SHA-256s and VERSION. When it
# is not newer, nothing is changed.
#
# A move made this way is a pull request (.github/workflows/lampos-harness-pin.yml,
# daily): it merges only after a lamp row on 52e6 or c0a4, at most one pin
# move to stable a week (the first builder's decision on #5957).
#
# Usage: refresh-pin.sh [--check] [--summary FILE]
#   --check         download and check, say what would change, write nothing
#                   under lampOS/ (exit 0 either way)
#   --summary FILE  also write a Markdown table of the old and new copies,
#                   their URLs and SHA-256s, to FILE (when newer)
#
# No network beyond the six URLs. Needs bash, curl and python3.
set -euo pipefail

mode="write"
summary=""
while [ $# -gt 0 ]; do
  case "$1" in
    --check) mode=check ;;
    --summary)
      [ $# -ge 2 ] || { echo "refresh-pin: --summary needs a file" >&2; exit 2; }
      summary="$2"
      shift
      ;;
    -h | --help)
      sed -n '2,/^set -euo/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "refresh-pin: unknown argument: $1" >&2
      exit 2
      ;;
  esac
  shift
done

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
lampos="$(cd "$here/../.." && pwd)"
product="$lampos/crates/update/src/harness_install/product.rs"
install_rs="$lampos/crates/update/src/install.rs"
for file in "$product" "$install_rs"; do
  [ -f "$file" ] || { echo "refresh-pin: $file not found" >&2; exit 1; }
done

work="$(mktemp -d "${TMPDIR:-/tmp}/harness-pin.XXXXXX")"
trap 'rm -rf "$work"' EXIT

# What product.rs pins, one line each: key, the copy's file name, origin URL.
python3 -I - "$product" "$install_rs" >"$work/plan.tsv" <<'PY'
import re, sys
product = open(sys.argv[1], encoding="utf-8").read()
install = open(sys.argv[2], encoding="utf-8").read()
def one(pattern, text, what):
    found = re.findall(pattern, text, re.S)
    if len(found) != 1:
        sys.exit(f"refresh-pin: product.rs: {what} found {len(found)} times, not once")
    return found[0]
inc = r'include_bytes!\("\.\./\.\./\.\./\.\./deploy/harness/([^"/]+)"\)'
origin = one(r'pub const ORIGIN: &str =\s*"([^"]+)";', product, "ORIGIN")
installer_url = one(r'pub const HARNESS_INSTALLER: &str = "([^"]+)";', install, "HARNESS_INSTALLER")
one(r'pub const INSTALLER_ORIGIN: &str = (crate::install::HARNESS_INSTALLER);', product, "INSTALLER_ORIGIN")
print("cli", one(r"pub const MANIFEST: &\[u8\] = " + inc, product, "MANIFEST"), origin, sep="\t")
print("installer", one(r"pub const INSTALLER: &\[u8\] = " + inc, product, "INSTALLER"), installer_url, sep="\t")
entries = re.findall(
    r'Manifest \{\s*env: "([A-Z_]+)",\s*path: "[^"]+",\s*bytes: ' + inc
    + r',\s*sha256: "[0-9a-f]{64}",\s*origin: "([^"]+)",\s*\}', product)
if len(entries) != 4:
    sys.exit(f"refresh-pin: product.rs: {len(entries)} MANIFESTS entries read, not 4")
for env, name, url in entries:
    print(env, name, url, sep="\t")
PY

# Download each, over https only, into the temporary folder.
while IFS=$'\t' read -r key _name url; do
  case "$url" in
    https://*) ;;
    *) echo "refresh-pin: $key's origin is not https: $url" >&2; exit 1 ;;
  esac
  curl -fsSL --proto '=https' --tlsv1.2 --retry 3 --max-time 120 -o "$work/$key" "$url" \
    || { echo "refresh-pin: could not download $url" >&2; exit 1; }
done <"$work/plan.tsv"

# Check them, decide, and (unless --check) write the new pin.
python3 -I - "$mode" "$work" "$here" "$product" "$summary" <<'PY'
import hashlib, json, os, re, sys

mode, work, folder, product_path, summary = sys.argv[1:6]
product = open(product_path, encoding="utf-8").read()
plan = [line.split("\t") for line in open(os.path.join(work, "plan.tsv"), encoding="utf-8").read().splitlines()]

def fail(message):
    sys.exit(f"refresh-pin: {message}")

def sha(data):
    return hashlib.sha256(data).hexdigest()

def plain(version):
    """a.b.c, digits only, as product::version_of reads it."""
    parts = version.split(".")
    if len(parts) != 3 or not all(p.isdigit() for p in parts):
        return None
    return tuple(int(p) for p in parts)

def versions(value, depth=0):
    """Every string `version` in a manifest, at most two objects down."""
    found = set()
    if isinstance(value, dict) and depth <= 2:
        for key, inner in value.items():
            if key == "version" and isinstance(inner, str):
                found.add(inner)
            else:
                found |= versions(inner, depth + 1)
    return found

def manifest(data, what):
    try:
        value = json.loads(data)
    except ValueError as error:
        fail(f"{what} is not JSON: {error}")
    if not isinstance(value, dict) or not value:
        fail(f"{what} is not a JSON object")
    return value

def version_of(data, what):
    found = versions(manifest(data, what))
    if len(found) != 1:
        fail(f"{what} names {sorted(found)} as its version, not one")
    version = found.pop()
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*", version):
        fail(f"{what}'s version {version!r} cannot name a file")
    return version

old_version = re.findall(r'pub const VERSION: &str = "([^"]+)";', product)
if len(old_version) != 1 or not plain(old_version[0]):
    fail("product.rs: VERSION not read as a.b.c")
old_version = old_version[0]

rows = []
for key, old_name, url in plan:
    old_bytes = open(os.path.join(folder, old_name), "rb").read()
    new_bytes = open(os.path.join(work, key), "rb").read()
    old_sha, new_sha = sha(old_bytes), sha(new_bytes)
    if product.count(old_sha) != 1:
        fail(f"{old_name}'s SHA-256 {old_sha} is not named once in product.rs: the pin is already out of step")
    if key == "installer":
        if not new_bytes.startswith(b"#!"):
            fail(f"{url} is not a shell script")
        if not re.fullmatch(r"cli-install-[0-9a-f]{8}\.sh", old_name):
            fail(f"the installer's copy {old_name} is not named cli-install-<8 hex>.sh")
        new_name = f"cli-install-{new_sha[:8]}.sh"
        old_v = new_v = None
    else:
        old_v = version_of(old_bytes, old_name)
        new_v = version_of(new_bytes, url)
        if old_name.count(old_v) != 1:
            fail(f"{old_name} does not name its version {old_v} once")
        prefix, suffix = old_name.split(old_v)
        new_name = prefix + new_v + suffix
    rows.append(dict(key=key, url=url, old_name=old_name, new_name=new_name,
                     old_sha=old_sha, new_sha=new_sha, old_v=old_v, new_v=new_v, data=new_bytes))

cli = next(r for r in rows if r["key"] == "cli")
new_version = cli["new_v"]
value = manifest(cli["data"], cli["url"])
entry = value.get("cli")
if not isinstance(entry, dict) or entry.get("version") != new_version or not plain(new_version):
    fail(f"{cli['url']}: cli.version {new_version!r} is not a.b.c")
for part in ("cli", "notify"):
    item = entry.get(part)
    if not isinstance(item, dict) or not re.fullmatch(r"[0-9a-f]{64}", str(item.get("sha256", ""))):
        fail(f"{cli['url']}: cli.{part} has no sha256")
want = f"https://cdn.autonomous.ai/harness/cli/{new_version}/cli.js"
if entry["cli"].get("url") != want:
    fail(f"{cli['url']}: cli.cli.url is {entry['cli'].get('url')!r}, not {want}")

if plain(new_version) <= plain(old_version):
    print(f"refresh-pin: Harness {new_version} on the CDN is not newer than the pin {old_version}: nothing changed")
    sys.exit(0)

# The installer must read exactly the manifests the pin copies, each from its
# variable with its origin as the default, so a pinned install fetches none.
installer = next(r for r in rows if r["key"] == "installer")["data"].decode("utf-8", "replace")
read = set(re.findall(r'\$\{(HARNESS_[A-Z_]+_URL):-https://', installer))
pinned = {"HARNESS_METADATA_URL" if r["key"] == "cli" else r["key"]: r["url"]
          for r in rows if r["key"] != "installer"}
if read != set(pinned):
    fail(f"the new installer reads {sorted(read)}, the pin copies {sorted(pinned)}: product.rs needs a hand change")
for env, url in pinned.items():
    if "${%s:-%s}" % (env, url) not in installer:
        fail(f"the new installer does not read {env} with {url} as its default")
if '[ "${HARNESS_INSTALL_ATTENDED:-}" = 1 ]' not in installer:
    fail("the new installer no longer asks HARNESS_INSTALL_ATTENDED before its unattended step")
names = [r["new_name"] for r in rows]
if len(set(names)) != len(names):
    fail(f"two new copies share a name: {names}")

print(f"refresh-pin: Harness {old_version} -> {new_version}")
for r in rows:
    same = "unchanged" if r["old_sha"] == r["new_sha"] else "changed"
    print(f"  {r['old_name']} -> {r['new_name']} ({same}) sha256 {r['new_sha']}  {r['url']}")

if summary:
    with open(summary, "w", encoding="utf-8") as out:
        out.write(f"Harness {old_version} → {new_version}\n\n")
        out.write("| copy | old | new | new SHA-256 | from |\n|---|---|---|---|---|\n")
        for r in rows:
            out.write(f"| {r['key']} | `{r['old_name']}` | `{r['new_name']}` | `{r['new_sha']}` | {r['url']} |\n")

if mode == "check":
    print("refresh-pin: --check: nothing written")
    sys.exit(0)

text = product
for r in rows:
    old_inc = f'deploy/harness/{r["old_name"]}")'
    if text.count(old_inc) != 1:
        fail(f"product.rs does not include {r['old_name']} once")
    text = text.replace(old_inc, f'deploy/harness/{r["new_name"]}")')
    text = text.replace(r["old_sha"], r["new_sha"])
text = text.replace(f'pub const VERSION: &str = "{old_version}";', f'pub const VERSION: &str = "{new_version}";')
# Old copies go first, and only those no new copy is named after, so a new
# name equal to another row's old name is never removed once written.
for name in {r["old_name"] for r in rows} - set(names):
    os.remove(os.path.join(folder, name))
for r in rows:
    with open(os.path.join(folder, r["new_name"]), "wb") as out:
        out.write(r["data"])
for r in rows:
    path = os.path.join(folder, r["new_name"])
    if not os.path.isfile(path) or sha(open(path, "rb").read()) != r["new_sha"]:
        fail(f"{r['new_name']} is not the copy just written")
with open(product_path, "w", encoding="utf-8") as out:
    out.write(text)
print(f"refresh-pin: wrote the {new_version} pin to {os.path.basename(product_path)} and deploy/harness/")
PY
