# Current preparation audit, 23 October 2026 03:28 +07

The publisher/workflow, three publication test suites and three updater
entry points remain byte-identical from `17167b1ba` through `b8a9148bd` to
audited integration `c5450ea97` (nine files). The [latest source delta](../verification/release-preflight-20261023/source-delta-c5450ea97.json)
records the before/after hashes from `b8a9148bd`. The delivered preparation
below remains current; no additional publisher or missing host preflight
implementation was found. This was a source audit, not a rerun of those
suites or a real release. Configuration metadata below retains its earlier
00:21 observation time; it was not fetched again for this source audit.
#3012's process/CLI checks have now merged through train #3078; they still
provide no attended physical-cut, real-systemd or quiet/Body acceptance.

[Configuration metadata](../verification/release-preflight-20261023/metadata.json)
was read at 00:21 +07. The repository-variable API returned zero variables.
The publication environment and its secret-name listing returned HTTP 404:
absence cannot be distinguished from inaccessible configuration. No signing
key value was read. Organization/environment variable inheritance and the
effective enablement value remain unestablished. No setting was changed.

The existing publisher fixes its channel to `lamp-voice`; it has no trial
channel option. First must approve an isolated route and monotonic
restoration plan for the deliberately bad release, rather than assuming the
normal publisher is a rehearsal switch. Before execution, First/owner still
supplies the attended #2810 result, lamp/person/Body window, trusted real
key/public-key fingerprint, protected environment and activation/bootstrap
decision, selected successful push artifact, installed/published/good/bad/
restoration sequences, persistent trial configuration and evidence steps.
There is no source-only substitute for those inputs. No publication, SSH or
physical acceptance was performed in this audit.

---

**Current delivery, 22 October 2026, source `17167b1ba`:** the workflow and
publisher described below now exist. The original proposal is retained as
historical design, not a request to implement another publisher. Current
sources are [ARM64 workflow](https://github.com/autonomous-ai/lampOS/blob/17167b1ba/.github/workflows/lampos-arm64.yml),
[publisher](https://github.com/autonomous-ai/lampOS/blob/17167b1ba/.github/scripts/lampos-publish.py),
and [artifact exporter](https://github.com/autonomous-ai/lampOS/blob/17167b1ba/.github/scripts/lampos-publish-artifact.py).

- Signing is conditional on a successful `lamp-voice` **push** run in the
  public repository and `LAMPOS_UPDATE_PUBLISH=on`. A PR or manual run cannot
  enter the publication job. Eligible signing builds skip cache restore;
  the exporter derives current targets from Cargo build evidence instead of
  a directory glob. The publisher verifies the exact checksum inventory,
  classifies retained binaries and uses the existing updater's manifest,
  signing and verification commands. No duplicate helper is needed.
- The job names `lampos-update-publish`, but the owner still must configure
  its actual branch protection and provision the real environment key plus
  trusted installed public half. A named environment alone is not protection.
  This source reading does not inspect a secret or establish its presence.
- Initial publication requires explicit bootstrap. Later candidates must be
  newer than the verified current sequence; immutable-tag collisions and a
  changed channel are refused. Promotion remains a multi-step public-release
  operation with retained restoration limits, not an atomic channel swap.
- Current `.config/nextest.toml` sets `profile.pr.default-filter = 'all()'`;
  the workflow's comments saying PRs skip realtime are stale. Scope filters
  and process-harness exclusions still apply. Read the actual run/selection,
  not those comments, when choosing the release gate.
- Real signed internet download, quiet switch, bad-release recovery and the
  attended Body run remain **unexecuted by Second**. #2810's attended unplug
  remains first. First/owner must provide the approved lamp/person window,
  isolated bad-release route, real-key/publication activation and monotonic
  restoration plan. No activation, publication or device action was made.

---

# Internet release preparation for #1812

Proposed preparation, inspected at `34aa8f18c13ed6f12877a60c04bc769330b3ad69`. Second can prepare one reviewable workflow/helper change and its offline acceptance cases after Music. First decides when publication and the real key leave the deferral recorded in [#1812](https://github.com/autonomous-ai/lampOS/issues/1812). No key, release, workflow edit or device operation was made for this plan. The separate whole-lamp/version row stays with Tests/Migrate under #1680.

## Reuse what exists

`lamp-update manifest`, `sign`, `public-key`, `check`, the Ed25519 verifier, file/HTTPS fetcher, quiet switch and rollback already exist. `docs/updates.md` proposes the release layout and a CI job, but that job is absent from actual ARM64 CI. The existing `publish-skills.yml` publishes a different feed on `main`; it is not the lampOS update publisher. No second updater or signing format is needed.

## Prepare before publication is enabled

1. **Bind input to the tested build.** Consume `lampos-arm64-<github.sha>` from the same successful `lamp-voice` **push** run, on an ARM64 runner. Record repository, workflow, run ID, attempt, run number, event, ref and checkout SHA. Checkout `deploy/` and `skills/` at that SHA. A PR artifact is not eligible: its name uses the PR head while its bytes come from the tested merge commit. Require `SHA256SUMS`, verify it before executing the artifact's `lamp-update`, and retain it unchanged as CI evidence. `publish::make_with` only checks that file when present, so the wrapper must require it.
2. **Build the existing layout.** Run the artifact's own `lamp-update manifest` with the matching `--deploy`, mandatory `--skills`, `--channel lamp-voice`, `--sequence <run_number>`, `--commit <sha>` and immutable release `--files-url`. It creates `manifest.json`, the product-skills bundle and `<lamp>.<unit>` assets. Inventory the exact assets named by `files` and `units`, plus the manifest, envelope and CI checksums; don't rebuild binaries. Do not silently add models/settings/image flags: their wider qualification belongs to #1680.
3. **Sign and verify without a release.** A future offline rehearsal can use a disposable test key in a temporary directory, calling the existing `sign` and verifier. The envelope remains `lampos-update/1`, with Ed25519 over the exact manifest bytes. For verified sequence/commit inspection, reuse `manifest::open`; the CLI has no standalone inspection command, so a small helper around that API is needed if the workflow cannot use `check` against its disposable root. Do not duplicate cryptography or trust a decoded field before verification.
4. **Separate preparation from promotion.** Preparation writes a local staged release and a receipt only. The later publishing step creates `lampos-lamp-voice-<N>` as a non-latest prerelease containing every staged asset; only after completeness and hashes match may it replace the single envelope asset on `lampos-lamp-voice`. The signed `files_url` always points to the immutable build release. The updater already follows HTTPS redirects without a GitHub token; its `--channel-url` is the channel folder ending in `/`, and it appends `lampos-manifest.json`.
5. **Keep the channel monotonic.** Serialize publication in the proposed `lampos-publish-lamp-voice` group and compare against the verified current head immediately before promotion. Require candidate sequence greater than head; a lower or equal candidate leaves head unchanged. Serialization alone does not establish completion order. Keep the existing workflow's run-number namespace; retries retain the same number. Start with the documented refusal of an existing immutable build tag. Retry/resume and retention can be decided separately; do not add automatic deletion to the first publication slice.

## Offline acceptance for that preparation change

Use the current updater's fixture/channel interfaces and temporary roots, not systemd or a lamp. Check one package through manifest -> test-key seal -> verify -> staged asset inventory. Add wrapper cases for a wrong run/SHA, missing or changed checksums, incomplete release assets, an older finishing run, an existing tag, and a failed upload that must not move the head. Keep existing updater tests for wrong key/changed signature, bad file checksum, no network, quiet switching, rollback and replay refusal. Run the affected Rust gates if a helper changes that crate; run workflow/script checks for the wrapper. No external test or real signing secret is necessary to complete preparation.

The existing HTTPS tests cover redirect framing, size bounds, truncation and rejection of a downgrade to HTTP. The ignored GitHub-fetch test is a later network check; an offline pass does not prove a published release is downloadable by a lamp.

## Deferred boundary and lamp acceptance

The owner supplies the real Ed25519 PKCS#8 key through `LAMPOS_UPDATE_KEY`, and its public half reaches `/etc/lampOS/update/key.pub` through the trusted install/setup path. The publisher reads the secret only in the publication job, writes it outside the artifact directory with mode 0600, removes it on success or failure, and records only its fingerprint. Missing real credentials fail publication; there is no test-key fallback.

First owns publication enablement and the attended lamp-4ace run. Use a dedicated rehearsal channel for the deliberately bad signed build, with its exact release/sequence and restore procedure reviewed first. Under the normal `serve` service, show an internet fetch and verification, waiting while the voice is busy, a quiet good update, a 60 s health watch, and automatic recovery from a bad release. Include the body only with someone at the arm. Record the tested commit, release/manifest/binary hashes, key fingerprint, voice interruption and recovery times, plus before/after units, links, records, keys and switches. Restore trial channel overrides and the original lamp state with the same checks. #737's local 15-part proof excluded the body; #1788's 107 s / ~693 s recovery was updater-only and restored its temporary unit. Neither completes these checks.

First still needs to settle: the publication deferral versus today's priority; whether the release gate is each successful push (today's ARM64 `pr` profile skips realtime tests) or a full run, and any matching Clippy requirement; and the first trusted key/sequence plus isolated lamp rehearsal channel. Preparing the disabled path does not settle those decisions.

## Source references

- [.github/workflows/lampos-arm64.yml](https://github.com/autonomous-ai/lampOS/blob/34aa8f18c13ed6f12877a60c04bc769330b3ad69/.github/workflows/lampos-arm64.yml)
- [.github/workflows/publish-skills.yml](https://github.com/autonomous-ai/lampOS/blob/34aa8f18c13ed6f12877a60c04bc769330b3ad69/.github/workflows/publish-skills.yml)
- [lampOS/crates/update/src/publish.rs](https://github.com/autonomous-ai/lampOS/blob/34aa8f18c13ed6f12877a60c04bc769330b3ad69/lampOS/crates/update/src/publish.rs)
- [lampOS/crates/update/src/main.rs](https://github.com/autonomous-ai/lampOS/blob/34aa8f18c13ed6f12877a60c04bc769330b3ad69/lampOS/crates/update/src/main.rs)
- [lampOS/crates/update/src/manifest.rs](https://github.com/autonomous-ai/lampOS/blob/34aa8f18c13ed6f12877a60c04bc769330b3ad69/lampOS/crates/update/src/manifest.rs)
- [lampOS/crates/update/src/fetch.rs](https://github.com/autonomous-ai/lampOS/blob/34aa8f18c13ed6f12877a60c04bc769330b3ad69/lampOS/crates/update/src/fetch.rs)
- [lampOS/crates/update/src/round.rs](https://github.com/autonomous-ai/lampOS/blob/34aa8f18c13ed6f12877a60c04bc769330b3ad69/lampOS/crates/update/src/round.rs)
- [lampOS/crates/update/tests/round.rs](https://github.com/autonomous-ai/lampOS/blob/34aa8f18c13ed6f12877a60c04bc769330b3ad69/lampOS/crates/update/tests/round.rs)
- [lampOS/docs/updates.md](https://github.com/autonomous-ai/lampOS/blob/34aa8f18c13ed6f12877a60c04bc769330b3ad69/lampOS/docs/updates.md)
- [Local parts proof, #737](https://github.com/autonomous-ai/lampOS/issues/737#issuecomment-6259361314); [self-updater proof, #1788](https://github.com/autonomous-ai/lampOS/pull/1788#issuecomment-6279387052).
