# Updates: a lamp keeps its lampOS current by itself

Task 45 ([#737](https://github.com/autonomous-ai/lampOS/issues/737), the
second builder, proposal `second-P5`). Until now a lamp took new lampOS
software only when the first builder copied binaries to it over SSH from
the office Mac (`~/lamp-4ace-v2-deploy-20261012/*_deploy.sh`). This is the
simplest version that works end to end ("wide first, deep later"):

- CI can sign and publish a tested push of `lamp-voice` as a GitHub
  release. Publication is disabled until the owner supplies the real key
  and explicitly enables the workflow below.
- `lamp-update` (`crates/update`), its own process with its own unit and
  records, learns of a newer build, fetches what changed, checks the
  signature and every SHA-256, installs each part beside the old one,
  switches at a quiet moment, watches each part for a minute, and puts the
  previous build back by itself if a part is not well.

**Apply is off by default on the command line.** Nothing changes on a lamp
until `lamp-update` is installed there, and then nothing is written until
`--apply on` is given: with `--apply off` (the command's default) it reads the
channel and says what it would do, and writes nothing, not even its own
records. A test shows it
(`without_apply_nothing_is_written_and_what_would_change_is_said`: every
file under the lamp's root byte for byte the same, and systemd not asked).
It publishes nothing on the bus at any setting, and with `--apply off` it
does not even subscribe. The product unit already passes `--apply on`.
The separate `LAMP_UPDATE_SELF`, `LAMP_UPDATE_MODELS` and `LAMP_UPDATE_IMAGE`
switches remain off unless set to `on`. Explicit `LAMP_UPDATE_APPLY=off`
also stays off; an absent apply switch can inherit the authenticated
managed-product default described below. The product unit sets
`LAMP_UPDATE_SELF` and `LAMP_UPDATE_APPLY` since 25 Oct; models and image
files remain separate. The lamp trials below do not enable them elsewhere.

## What exists and what we take

The unwired [private before-image bundle](writer-preimages.md) preserves bounded
ordinary captures as immutable, versioned bytes, with fixed-name durable
publication in an existing private directory. This adds no live write, apply
admission, restoration or helper bootstrap. Unknown semantics retain bytes
and refuse; content identity does not authenticate the caller.

The unwired [setting-file observer](writer-file-observation.md), a #3689
prerequisite, provides bounded read-only descriptor observations for a
future independent writer guard. It grants no path authority and performs
no settings write, helper refresh or restoration.

### V1's OTA, in the form of the V1 verdicts

Read for this task: `docs/bootstrap-ota.md` (sections 1, 2, 4 and 5) and
`system/bootstrap/{security.go,rollback.go}` in the parent repository, on
2026-10-14. These rows add to GO-3.4 to GO-3.7 of
[v1-verdicts.md](v1-verdicts.md); the sheets themselves were not re-read.

| Sheet | What it is, in a few words | Verdict | Why | Where it belongs in V2 | State in V2 |
|---|---|---|---|---|---|
| GO-3.5 | The signed feed: an `autonomous-ota/v1` envelope, Ed25519 over the exact payload bytes, the public key given at setup and never read from the feed, a key fingerprint (first 16 hex of its SHA-256) | `rebuild from the idea` | Right, and small. Taken: Ed25519 over the manifest's exact bytes, one envelope so the two are always read together, a local key, the fingerprint. Not taken: the unsigned "legacy" mode and the unsigned top-level copy kept for old workers (`security.go`: "Compatibility feeds keep legacy top-level entries"); here an unsigned build is never one. Added: a sequence that only goes up, which V1's envelope lacks, so an old signed manifest cannot be played back | `lamp-update` | built |
| GO-3.5 | Each cycle downloads a new updater script and runs it after `bash -n` (`updater_refresh.go`) | `ignore` | Code fetched and run is a second, unsigned path. `lamp-update` runs nothing it fetches but the parts' binaries, each named by its SHA-256 in a signed manifest | Nowhere | none |
| GO-3.4 | `min_version` floor beside `version`; `software-update KEY` over SSH ignores the floor | `ignore` | A channel replaces it: the lamp follows the newest build of one branch, and a build is tried by hand from a folder on the lamp (`--channel-url file:///...`), not by a second number | Nowhere | none |
| GO-3.4 | `rollback_versions`: the version a rollback removed is skipped by later polls until another is published | `rebuild from the idea` | Taken as "refused builds": a build put back is never tried again; the next one is | `lamp-update`, `state.json` | built |
| GO-3.4 | State written to a temporary file, synced, renamed; a corrupt state kept aside as `state.json.corrupt-<time>` and begun again, its key paths marked as once written (#1263) | `rebuild from the idea` | Taken as it is, for every record, with the folder synced too | `lamp-update`, `records.rs` | built |
| GO-3.4 | `componentInstalled`: a component the device does not have is not updated | `rebuild from the idea` | Taken: only parts with a unit installed on this lamp are updated; a binary the build carries and the lamp does not run is not fetched | `lamp-update` | built |
| GO-3.6 | Five safeguards "learned the hard way" (`bootstrap-ota.md`, "Invariants") | `take the cases` | (1) One run at a time: taken, `flock` on the records' `lock`. (2) A missing installation is a reinstall: taken, a unit whose folder is gone gets the build's binary afresh. (3) Publish with the live tree's mode: taken, folders 0755 and binaries 0755, never a temporary folder's 0700. (4) A virtualenv is not relocatable: nothing here is relocated; files are installed under their final folder's parent and renamed. (5) Failure feedback is temporary: no light at all (the ring is the voice's) | `lamp-update` | built, (5) not needed |
| GO-3.6 | A journal written and flushed before the live tree is replaced; recovery before any new update | `rebuild from the idea` | Taken: the journal and the units as they were are written before any unit changes; at the next start an unfinished switch is put back before anything else | `lamp-update` | built |
| GO-3.6 | Core binaries: a `--version` probe within 15 s, then a health URL and "the service remains active" | `rebuild from the idea` | The second half is taken: a part must stay up with one process for a minute and not say it is worse in `lamp/health`. No `--version` probe: the folder is named by the binary's SHA-256, which says more, and a probe would start a second copy of a part beside the one running | `lamp-update`, `judge.rs` | built |
| GO-3.7 | Ten progress phases; `/security` posture with the key's fingerprint and the last fetch | `later` | The log says each outcome, and the key's fingerprint is printed at start; a page and progress belong with the surround's pages | The surround | partly |
| GO-3.4 | LED feedback while updating (orange breath, green, red) | `ignore` | The ring is the voice's; an update at a quiet moment says nothing | Nowhere | none |

The safeguard the old updater lacked, by its own record: two runs 30 s
apart lost the live tree, staging and backup. Here a second
`lamp-update` is refused by the lock, the old folder is never touched,
and a new one only gets its name once it is whole.

### What the lamps run now, and what was kept

- Every part is installed as `/opt/lampOS/<folder>/<sha256>/<binary>`, its
  unit naming that path ([joining.md](joining.md), "Installing"). On
  lamp-4ace the voice is under `/opt/lampOS/releases/<sha256>/`, beside the
  `lamp-turn-end` worker it starts, its model and the ONNX Runtime library
  ([endpoint-detection.md](endpoint-detection.md), "Installation"); the
  controls keep their settings beside their binary
  (`deploy/lamp-4ace/lamp-controls.service`). Both are kept as they are:
  a new build goes in a new folder beside the old one, in the same parent.
- The rule for restarting the voice: whoever restarts it first checks that
  the lamp has neither heard nor said anything for 45 seconds
  ([brain-build.md](brain-build.md), "Restarting the voice"). The first
  builder's `quiet_gate.sh` is not in the repository; its rule is taken from
  that page, judged from what the voice says on the bus.
- [The surround](surround.md) has a fuller design: TUF through `tough`,
  whole release sets of voice, body and eyes, and a supervisor that closes
  the voice's admission and proves the body at rest before a switch. It is
  not used here because, end to end, it needs a TUF repository with its four
  roles kept signed, and the supervisor's proofs, neither of which exists.
  It stays the deeper design: once updates work end to end, its pieces can
  replace these one at a time.

## Publishing (built; activation is the owner's)

`.github/workflows/lampos-arm64.yml` has a `publish` job after the successful
`arm64` job. It uses `.github/scripts/lampos-publish.py` and the tested
artifact's own `lamp-update manifest`, `public-key`, `sign` and `check`.
There is no second updater or signing implementation. Publication is **off
by default**: the helper requires repository variable
**`LAMPOS_UPDATE_PUBLISH` to be exactly lowercase `on`**. Only a push to
`autonomous-ai/lampOS`'s `lamp-voice` can publish; pull requests and manual
runs cannot. [Actions string comparisons ignore case](https://docs.github.com/en/actions/reference/workflows-and-actions/expressions#operators),
so `ON` can enter the cold-build and protected publication job paths, but
the helper rejects it before executing an artifact binary.

**Where the artifact is built.** With repository variable `LAMPOS_STUDIO` set
to `on`, lamp-voice's `arm64` job may run on the internal Mac Studio's trusted
runner (`studio-trusted`) instead of GitHub-hosted: one VM, fresh for each
job and deleted after it, with no cache mounted, that takes only `lamp-voice`.
`.github/scripts/lampos-route.py` chooses, and falls back to GitHub-hosted on
any doubt and on every re-run: any attempt after the first runs the `arm64`
job on GitHub-hosted, whether it is "Re-run all jobs" or "Re-run failed
jobs" (which reuses `route`'s first answer). The build is cold either way. `publish` and
`promote-stable` always run on GitHub-hosted, so the signing key never
reaches the Studio. The key staying away does not keep the Studio out of the
release's trust: `publish` signs the bytes the Studio built, and its SHA-256
check covers only their transfer, not the build. So with `on`, the Studio
host, its hypervisor and its VM image are part of what the signature vouches
for, alongside GitHub; the team's devops administers them. The Studio's
pull-request pool is retired: no pull request runs on that host.

### Selecting a published build for the stable feed

The same ARM64 workflow accepts an optional `promote_sequence` manual input.
With it empty, ordinary builds and publication keep their existing behavior.
With it set, the workflow runs the separate Rust CI tool in
`.github/tools/lampos-promote`; it does not rebuild or sign the selected
product binaries. `full` and `promote_sequence` cannot be used together.

Promotion is disabled unless repository variable `LAMPOS_STABLE_PROMOTION`
is exactly `on`. The job uses the existing branch-restricted
`lampos-update-publish` environment and its `LAMPOS_UPDATE_PUBLIC_KEY`
variable: the base64 public half already provisioned to lamps, obtained
through the trusted provisioning path. It never derives trust from a release
asset and never reads the signing secret. Keep the switch off until the
environment protection and public key have been checked. No environment,
variable, key or lamp configuration is provisioned by this implementation.

An authorized operator selects an already published sequence, for example:

```sh
gh workflow run lampos-arm64.yml --repo autonomous-ai/lampOS \
  --ref lamp-voice -f promote_sequence=6819
```

The tool accepts only the public repository's `lamp-voice` manual workflow
context. It verifies the selected immutable release, its recorded original
ARM64 run and attempt, the successful build/publication jobs and source commit.
It uses `lamp_update::manifest` to verify the signature with the trusted
public key, schema and supported manifest fields, then checks the sequence,
commit, immutable payload URL, plain manifest bytes and declared asset hashes
and sizes. An unsupported manifest, updater level or enum value is refused.
External model manifests are also refused: the tool does not fetch model
URLs outside the selected release. Supported settings, image and reset-plan
contents remain signed inline and use the updater's validation. A missing or
ambiguous source, unsuccessful run, changed asset or unknown signing key
cannot update the feed.

**The stable release's notes (#3976).** After a promotion over a previous
stable, the job's next step (`.github/scripts/lampos-stable-notes.py`, never
a gate: `continue-on-error`) fills the `lampos-stable` release body's
`### For developers` part. It gives the source release, the runs, the commit
and the previous stable's, then lists the pull requests merged between the
two commits: each commit's merged pull requests into `lamp-voice`, with a
train opened into the pull requests its title names. GitHub also names open
or closed pull requests whose branch holds a landed commit; without
`merged_at` they aren't listed (#4211). If the body already starts
`## lampOS build <N>` for this build, written by hand, only that last part
is replaced. Otherwise it is still the previous build's, and the body
becomes a fresh header with the developers part, ready for the notes in
plain words. A build's own release body is never touched: the promote tool
reads its one line as provenance.

The step after it (`lampos-promote`'s `lampos-notes` binary,
`.github/tools/lampos-promote/src/notes.rs`, never a gate either) writes
the `### Built but not on yet` part. It lists each `switches.md` row counted
`until` or `now` whose switch no product file turns on, read from the
units themselves (`Environment=` lines, `ExecStart` flags and the settings'
`.env` files in `deploy/product`), not from the row's "Product unit" cell.
The one exception is a cell of exactly `on (unset)`: a switch on by its
code's default that no file sets, such as `LAMP_FORGET_ASKED`, which is not
listed.
Each switch is listed in plain words: its line in `switches.md`'s "In plain
words" table, what a person would notice with it on, with no switch name. A
switch with no line there falls back to its row's words. Every switch listed
has one, held by `on_this_repository_each_listed_switch_has_a_plain_line`
(the first builder on #3976). The part says how many, then gives the list
folded and grouped by the page's four tables. A second fold, "For
developers", gives each switch's name and its row's words to the end of their
first sentence (140 characters at most). The part goes between the notes in plain words and
`### For developers`, or replaces itself, and it is written only when the
body is already this build's. The same notes are kept on the build's own
release as an asset, `NOTES.md`, which no promotion reads (it checks only
the signed manifest's files). A release that can't take it says so in the
step, and the promotion stands.

The stable feed is
`https://github.com/autonomous-ai/lampOS/releases/download/lampos-stable/lampos-manifest.json`.
Its envelope is an exact copy of the selected release's signed bytes. The
signed channel stays `lamp-voice`, and `files_url` still names the original
immutable `lampos-lamp-voice-<sequence>` release. Stable is a selection feed,
not a new signature or sequence namespace. A lamp pointed at it still uses
`--channel lamp-voice`. The product unit (`deploy/product/lamp-update.service`)
and the V2 image's default (`scripts/imager/build-orangepi-v2.sh`) follow it
from the first customer lamps on (26 Oct): a lamp in a home gets a build only
once it is promoted here, never each lamp-voice publish, and a lamp the image
makes starts on the promoted build. Until a first promotion the feed is absent
and such a lamp keeps the build it has. This tool does not install anything on
a lamp.

The verified stable head can only advance. Repeating the same sequence and
identical envelope is a no-op; a lower sequence or different bytes at the
same sequence is refused. A corrective build needs a higher sequence.
Lamps already ahead of the selected build keep their updater's normal
anti-rollback rules; promotion cannot downgrade them.

The tool downloads and checks a staged envelope before moving the previous
canonical asset to a retained backup and renaming the candidate. Stage names
include the promotion run and attempt; no upload uses `--clobber`. The job
serializes stable promotions, rechecks the previous head before replacement,
and attempts restoration after a failed rename or confirmation, including a
lost response after a successful rename. Existing releases without a
canonical envelope require repair; the tool does not guess after a killed
runner. As with ordinary publication, the two renames are not atomic and a
lost runner can leave the feed temporarily absent. The lamp keeps its build;
the retained asset IDs and job receipt support repair. Other writers must
not update this alias outside the serialized job.

Offline Rust tests exercise verification, monotonic selection and failed
promotion/recovery with a disposable key and simulated GitHub responses.
The ARM64 workflow runs them; Linux Clippy also checks the standalone tool.
These tests do not establish a real stable promotion or lamp acceptance.

### The build and its identity

- The job checks out that run's `github.sha` and downloads
  `lampos-arm64-<github.sha>` from the same run. No artifact from another
  run, branch, pull request or manual input is accepted. The manifest's
  commit is that SHA and its sequence is `github.run_number`.
- A run eligible to publish under that exact event/ref/repository/variable
  gate builds cold: it skips cache restore and the step that restores
  cached source times. A separate step first refuses an existing
  `lampOS/target`, including a dangling symlink, then runs the unchanged
  source-times helper against the absent target. This only records fresh
  checkout times for the cache saved afterward; it cannot reuse a cached
  binary or timestamp record. The save keeps the upstream commit-keyed
  policy, with the same explicit key when restore produced no output.
  Pull requests, manual runs and publication-disabled pushes keep their
  existing cached behavior. This cold build is the reviewable default for
  signing candidates, not an activation or an owner configuration change;
  its extra compilation time belongs in the publication cadence decision.
- Artifact packaging uses the current workspace's `cargo metadata` and
  successful build's JSON `compiler-artifact` records, never a glob over
  the restored target cache. Every current `lamp-*` binary target and both
  selected examples must have a matching record and a nonempty, executable
  regular file at its expected release path. Missing targets, duplicate or
  conflicting export paths, and incomplete build logs refuse packaging.
  Cargo's `fresh: true` record is an intentional validated cache hit for a
  current target; an unrelated cached executable has no such record and is
  excluded. A future feature-gated binary must be enabled in this build or
  packaging fails rather than using an older executable with its name.
- The example asset `schedule_check` belongs to `lamp-schedule`, and
  `replay_cost` explicitly belongs to `lamp-expression` (the planner-cost
  tool documented in [expression.md](expression.md)). Both are built into
  `target/lampos-artifact-examples`, with separate successful build records.
  The workspace still builds the controls example also named `replay_cost`;
  its shared output cannot supply the CI artifact. The exporter creates
  a new artifact directory and hashes every copied file, so a retired or
  renamed cached binary cannot acquire a checksum under the current SHA.
  After artifact export/upload and before cache save, the job discards
  only `lampOS/target/lampos-artifact-examples`, under the same condition
  as cache save. The retained CI artifact remains complete, including both
  examples and their checksums. The isolated example target therefore
  rebuilds on later runs; it no longer increases the saved cache's size.
- Before executing the artifact, the helper requires a flat, regular-file
  inventory and a complete `SHA256SUMS`: no missing, extra, duplicate or
  mismatched file. This includes development binaries and CI examples
  that will not be published: an excluded file with a bad checksum still
  refuses the run before any artifact executable can run.
- The helper then strictly reads the same checkout's
  [`deploy/package-binaries.json`](../deploy/package-binaries.json), from
  [#2481](https://github.com/autonomous-ai/lampOS/pull/2481). It requires
  the four named lists, safe binary names, no duplicate membership and a
  classification for every artifact binary. Malformed JSON, unknown
  groups or a missing classification refuse publication; the `_comment`
  field cannot classify a binary. Listed optional tools absent from the
  artifact need not be published. Only `core`, `optional` and
  `provisioning` binaries are copied into private staging; `development`
  and both CI examples are excluded. The provisioning tools remain
  because the updater carries `lamp-body-measure` and `lamp-body-mood`
  beside Body when present. This is the packager's existing classification,
  not a second binary list. CI also retains its classification check
  immediately after the workspace build.
- Staging gets a new `SHA256SUMS` covering exactly those retained binaries.
  Every copied binary is checked against its original verified hash before
  `lamp-update`'s private copy is made executable. The original CI artifact,
  its full checksum list and its modes remain unchanged. The existing
  manifest command includes the retained binaries, product skills
  (`--skills lampOS/skills`, below), and supported units in each
  `deploy/<lamp>/` folder with a `LAMP.md`. Units are named `<lamp>.<unit>`
  and skills `lamp-product-skills`. These come from the same checkout;
  binaries are never rebuilt in the publication job.
- The immutable prerelease is `lampos-lamp-voice-<run number>`, targeted at
  the exact SHA and never marked Latest. It contains those retained
  binaries and their regenerated `SHA256SUMS`, generated units/skills,
  `manifest.json` and `lampos-manifest.json`. Development binaries and
  examples enter neither the manifest nor the release. Every release
  checksum has its target; generated units and skills are covered by the
  signed manifest. All manifest hashes and sizes must match the staged files,
  and every uploaded asset is downloaded and hashed again before the
  channel can change. Missing or extra assets refuse promotion.
- An existing immutable tag or release that isn't this build's refuses the
  run. A re-run over its own release, one a 5xx cut short included, goes on
  from where it stopped ("GitHub's 5xx and a re-run" under
  [Promotion and recovery](#promotion-and-recovery)). Nothing overwrites or
  prunes build releases or tags. Old backups and stages are deleted from the
  channel and stable releases only, after a verified switch ("The channel
  release stays under GitHub's cap", in the same section).

### Cadence, storage and retention

Once enabled, each successful eligible `lamp-voice` push attempts a new
release; there is no nightly batching or rollout percentage. The default
900-second poll observes the latest signed channel head, so a lamp can
skip intermediate builds. Applying still depends on its apply switches,
installed parts, quiet period and refusal/rollback checks; publication
does not promise that every lamp installs each merge within 15 minutes.

The Actions artifact expires after **seven days**. Immutable GitHub build
releases and their tags have **no automatic expiry or pruning** here, and
the publisher lists the release history on each attempt, so API work also
grows with retained history. Retained previous envelopes are pruned:
after a verified switch, the channel and stable releases each keep the newest
20 backups and lose the older ones and older sequences' stages, at most 250
a run (#4160, #4164; "The channel release stays under GitHub's cap" under
[Promotion and recovery](#promotion-and-recovery)). An unfinished upload of
this build's own release is deleted and uploaded again by a re-run (#4103);
one anywhere else refuses the run. The earlier proposed 30-build retention
limit for build releases is not implemented.

For a measured size baseline, [ARM64 run 39543873384](https://github.com/autonomous-ai/lampOS/actions/runs/39543873384)
at `f41a24c185` produced 49 CI files: **300,709,350 bytes** uncompressed,
**111,046,340 bytes** in the downloaded archive. Applying the merged
classification to that inventory retains **30 binaries totaling
214,974,504 bytes** (about 215 MB), omitting 16 development binaries and
two examples. That retained binary total excludes regenerated checksums,
units, skills and signed manifests; it is **not a complete release size**.
At 20–40 publications per day with that binary payload, retained binaries
alone add about **4.3–8.6 GB per day**, or **129–258 GB over 30 days**,
before those generated assets and envelope history. Actual totals must
be measured on the build selected for activation; the public host record
keeps the measured inventory identity.

### Publication authorization, the key and verified channel

The job names the GitHub environment **`lampos-update-publish`**. Before
provisioning the real key, the owner must create and protect that
environment: choose **Selected branches and tags**, add a **Branch** rule
for exactly **`lamp-voice`**, and allow no tag, wildcard or pull-request-ref
rule. Store the key only as that environment's **`LAMPOS_UPDATE_KEY`**
secret, with no repository or organization secret copy. A repository-wide
secret would be available to other workflows on builder branches. The
owner may also require their approval for each deployment through the
environment's required-reviewer policy. The workflow does not create
these protection rules; GitHub can automatically create an environment
without them when a job references a missing name. Keep publication off
until the owner has verified the environment's configuration. See
[GitHub's environment setup](https://docs.github.com/en/actions/how-tos/deploy/configure-and-manage-deployments/manage-environments).

The owner must also supply **`LAMPOS_RELEASE_TOKEN`** in that same protected
environment, with no repository or organization copy. Use an already authorized,
repository-limited GitHub App installation token or fine-grained access token
with **Contents: write** and **Workflows: write**. Keep its lifetime and renewal
under the owner's credential policy. At preparation, the metadata-only audit
found only the signing-key secret in this environment; it did not read any
secret value. This change creates no account, token, key or environment secret.

The built-in Actions `GITHUB_TOKEN` is sometimes refused when it creates the
release: `gh release create` gets HTTP 403, "Resource not accessible by
integration" (#3906). On 27 Oct it refused 7 of 9 publishes between 19:15 and
22:30 UTC, then none for 19; on 28 Oct it refused two more (`8a1cfa93a` and
`83c334356`). What triggers it is not established. Refused and published
commits differed from `main` in the same workflow files and came from the same
pusher, and commits already superseded on lamp-voice published fine. A token
with Workflows: write is not held to the built-in token's workflow rule
([release API](https://docs.github.com/en/rest/releases/releases#create-a-release),
[workflow scope](https://github.blog/changelog/2023-11-02-github-actions-enforcing-workflow-scope-when-creating-a-release/)),
which the built-in token can never be given. Keep `--target` equal to the
tested artifact's exact SHA. Retargeting to a newer commit would break
provenance and is not an authorization fix.

The publication step maps the protected release credential to `GH_TOKEN`;
the job's built-in token has only Contents: read. If `GH_TOKEN` is absent,
empty or whitespace, the helper refuses before any child command, artifact
preparation or signing with
`LAMPOS_RELEASE_TOKEN is absent from the protected publication environment`.
An ambient built-in token cannot substitute for it.

The owner makes the real Ed25519 PKCS#8 key outside the repository, puts
its base64 form in **`LAMPOS_UPDATE_KEY`**, and provisions its public
half (`lamp-update public-key --key-file FILE`) to lamps as
`/etc/lampOS/update/key.pub`. Keep **`LAMPOS_UPDATE_PUBLISH` as a repository
variable**: the build and publication gates must decide before the
environment starts. This PR provisions no environment, secret or variable.

Within this workflow, only the gated publication step references these
two environment secrets. The helper writes the signing key outside the upload
folder in a 0600 temporary file, removes it in `finally` after signing, and
excludes it from child-process environments and command arguments. It also
removes `GH_TOKEN`, `GITHUB_TOKEN`, `GH_ENTERPRISE_TOKEN` and
`GITHUB_ENTERPRISE_TOKEN` from its base child context. Only a `gh` child receives
the original publication `GH_TOKEN`; Git and the artifact's updater receive no
GitHub authorization tokens. The parent context stays unchanged, and diagnostic
redaction still uses the original secret values. Each child command has a
600-second timeout. Command timeouts and handled `SIGINT`/`SIGTERM`
unwind through the same cleanup, but cancellation can interrupt cleanup
before it finishes. `SIGKILL` or a lost runner cannot execute it, so the
ephemeral runner's disposal remains the final boundary. No real key is
created or included here.

Signing and verification use the existing `lampos-update/1` envelope:
Ed25519 over the exact manifest bytes, schema 1. The helper verifies both
its candidate and the existing head with the artifact's `lamp-update
check --apply off`, using a local file channel, the derived public key and
a disposable root. It seeds only that scratch root with the decoded
sequence: the updater checks the signature, schema and channel **before**
its installed-sequence equality shortcut. The helper requires the exact
`up to date: build N` result and an unchanged root, including file bytes,
modes, directories and symlink targets. No lamp, bus, unit or
manifest-named download is involved in this verification.

The channel release is `lampos-lamp-voice`. Its canonical asset remains
`lampos-manifest.json`, polled by lamps every 15 minutes at
`https://github.com/autonomous-ai/lampOS/releases/download/lampos-lamp-voice/lampos-manifest.json`.
A public release download needs no lamp token. The signed `files_url`
points to the immutable build release, so an envelope cannot pick up a
later build's binary.

A successful release/tag listing must establish the channel's state;
authentication, network, malformed API responses and unverifiable heads
all refuse publication. A missing channel requires the separate explicit
repository variable **`LAMPOS_UPDATE_BOOTSTRAP=on`**. Set it only for the
first channel, then turn it off. An existing tag without a release or a
release without its canonical envelope requires owner repair, even with
bootstrap enabled. Before activation, the owner must also check that the
workflow run-number namespace is greater than sequences already installed
on the intended lamps, including local trial sequences; this job cannot
infer device state. Renaming/replacing the workflow or resetting its
sequence namespace requires the same review.

For an existing verified head, the candidate sequence must be strictly
greater. The helper rechecks the head after the immutable upload and again
before promotion. A shared concurrency group prevents two instances of
this job from promoting together; the sequence guard is still necessary
because [Actions concurrency ordering](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-workflow-concurrency)
is not build order. Other publishers must remain disabled; GitHub offers
no compare-and-swap operation across release assets.

When a `gh` command fails, the helper immediately logs its fixed command label
and exit code with only the first stderr line, stripped of terminal escapes and
nonprintable characters and limited to 320 diagnostic characters. It redacts
configured secret values and recognized credential patterns, including JWT/JWE
forms with empty components, before truncation; this bounded summary is not a complete error transcript or a guarantee that
every unknown secret format is recognized. It does not echo command arguments
or stdout, and other command failures retain their generic message. The log
survives promotion recovery wrapping the error; it adds no publication retry.

### Promotion and recovery

The helper uploads the candidate envelope under the unique name
`lampos-manifest-staged-<run number>.json` on the channel release, then
downloads and hashes it. Preparation, immutable-build upload and staging
failures leave an existing canonical head untouched. It never uses
[`gh release upload --clobber`](https://cli.github.com/manual/gh_release_upload),
which can remove the old asset before a failed replacement upload.

After staging succeeds, [release-asset rename](https://docs.github.com/en/rest/releases/assets#update-a-release-asset)
retains the previous canonical asset as
`lampos-manifest-previous-<run number>-<asset ID>.json`, then renames the
candidate to `lampos-manifest.json` and downloads/verifies the new head.
**The two renames are not atomic.** A lamp may briefly find no canonical
envelope and leave its build unchanged. A promotion or confirmation
failure attempts to put the retained previous asset back, including when
an API response was lost after a rename actually happened. If restoration
also fails, the job fails with the release and retained asset IDs for
owner repair. An interrupted or externally changed channel can therefore
remain without a canonical envelope; the helper does not claim an atomic
replacement or guaranteed recovery from a lost runner. The old bytes are
retained.

**The channel release stays under GitHub's cap.** GitHub allows [at most
1000 assets on one release](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases),
and every promotion leaves a `lampos-manifest-previous-*` backup on
`lampos-lamp-voice`: 214 by 28 Oct, about 37 a day, so it would have been
full around 18 Nov. Once a switch is verified, the helper deletes the
backups beyond the newest 20 (`KEEP_BACKUPS`; this switch's own, and any of
a later sequence, are always kept) and the stages left by older sequences,
oldest first and 250 at most a run (`MOST_PRUNED`). It never deletes the
head, deletes nothing after a failed or put-back switch, and a cleanup that
fails is said in the log, leaving the build published and the rest for the
next run. The promote tool does the same on the stable feed's release
(`lampos-stable`) after a verified promotion
(`lampos-promote::KEEP_BACKUPS`, `MOST_PRUNED`), at its slower pace of a few
promotes a day.

**GitHub's 5xx and a re-run (#4103).** On 28 Oct GitHub answered 504 three
times in this job and in a promotion, each after the release was already
made, and a re-run was refused ("immutable release already exists"), so
that build could never be promoted. Now:

- **A read is asked again.** A `gh api` GET or an asset download that
  GitHub answers 500, 502, 503 or 504 is asked again after 5, 15 and 30 s,
  then refused. A 4xx, or any other failure, is never asked again.
  `lampos-promote` reads GitHub the same way (`github.rs`, `retried`).
- **A change is never repeated blind.** A 5xx on `gh release create` or on
  the stage's `gh release upload` can come after GitHub made the change, so
  the helper reads what is there and goes on only if it is there. The
  renames are not retried; their failure is put back as above.
- **A re-run goes on from where it stopped.** Re-running the failed job
  keeps the run's number, so the sequence is the same, and the job signs
  the same bytes again (Ed25519 is deterministic). If this sequence's release
  exists, it must carry this commit and every asset it has byte for byte,
  or the re-run is refused as before. One cut short inside `gh release
  create` (a 5xx between its uploads, in this run or an earlier one) gets
  only its missing assets, once the ones it has are checked, and then every
  asset is checked again. An upload GitHub left unfinished (an asset whose
  state isn't `uploaded`) still holds its name: if the name is one of this
  build's, it is deleted and uploaded with the rest. Anywhere else, an
  unfinished asset still refuses the run. A matching release is used as is, and its
  notes are changed to name this attempt (`attempt N`), since the promote
  tool reads the run's success from that attempt. A stage left by a failed
  switch is used again only if it holds these bytes. If the channel's head
  is already this build's envelope, the job finishes without touching it.
  A newer head refuses as stale, as before.

`min_updater` remains a signed level, not the build sequence:
`manifest::UPDATER` is 8 (2 reads models, 3 settings, 4 image, 5 reset plans,
6 product service units, 7 a part's templates, sockets and timers, 8 a
product unit the lamp lacks added). With its product deploy tree, this workflow now
carries ordinary service templates, allowlisted settings and the reset plan
by default. Models and image inputs remain separate; publication does not
enable their lamp switches. The manifest CLI can specify `--min-updater N`;
it refuses a level above the carried updater or a build without `lamp-update`.

### Verification and ownership

The workflow runs `.github/scripts/lampos-publish-test.py` with all
external commands simulated, and `.github/scripts/lampos-publish-cli-test.py`
against the just-built updater with a disposable test key, local files
and simulated GitHub. These cover checksums, upload inventory/readback,
sequence races, refusal paths, both promotion renames and failed
restoration, a 5xx on each read and after each change, and a re-run over
its own release, over a switched channel and over a release that differs;
the CLI rehearsal checks the actual publisher/verification
contract. They do not publish or use the real secret.

`.github/scripts/lampos-publish-artifact-test.py` also checks the artifact
inventory and checksum coverage. Its real offline Cargo regression builds
a binary, renames its current target while keeping the retired executable
in the cache, and proves the old file is excluded. It also checks current
`fresh: true` cache hits and the selected example's separate build output.

The [21 October host record](verification/update-release-host-20261021.json)
records the simulated failure tests, real updater CLI tests, and the
artifact provenance checks passing, plus the exact-checkout updater,
workflow lint and syntax checks. It preserves
commands and log hashes; native ARM64 results qualify their published head
separately.

This delivers #1812's host workflow and documentation scope under the
[lead's direction](https://github.com/autonomous-ai/lampOS/issues/1812#issuecomment-6305700703).
The owner decides when to provision the real key, resolve the sequence
namespace and bootstrap choice, and activate publication. Migrate owns the
wider update path afterward; the separate Tests/Migrate whole-lamp row
remains theirs. Local test-key part updates and updater-only self-recovery
shown below do not prove real-key internet delivery or an attended body
switch. Those lamp acceptance runs and restoration remain coordinated
with the lamp's owner after activation. No such run was performed by this
workflow change.

## On the lamp

### One round

`lamp-update once` is one round; `lamp-update serve` one every 15 minutes
(`--every-s`), reading the bus in between. With `--apply on`, `serve` also
reads the app's Check for update (`update/check`, #4153,
`crates/update/src/app_check.rs`) between rounds, looking once a second: a
check starts the next round at once instead of after the wait. It is the
same round, with the same lock and quiet moment; a check that came while a
round ran is dropped, never kept for after it, and at most one a minute
starts a round. With nobody saying one, the wait is as before
([cloud.md](cloud.md), "Check for update"). lamp-cloud is the one that
says it, but the bus keeps no list of who may say a topic, so any part on
the lamp's bus can start a round this way: still at most one a minute, and
each under the quiet rule. A round
(`crates/update/src/round.rs`):

1. **A switch cut short is put back.** If the journal of a switch is there
   (the program stopped, or the lamp lost power, after the units were
   written), every unit it names that differs from how it was is written
   back, and those recorded as running are restarted in the saved old
   units' startup order. Already-restored units and units that were inactive
   gain no restart. Completed product Setup is also left alone when its
   restored unit passes the lifecycle check in step 8; the journal still
   truthfully records whether it was active before the switch. The graph is
   checked before any restoration write: a
   cycle or unsupported ordering syntax leaves the journal and the existing
   recovery prefix in place for another attempt. A completed put-back
   refuses the build.
2. **The channel.** `lampos-manifest.json` is read from `--channel-url`.
   No network: `nothing changed: the channel cannot be reached: ...`, status
   3. Nothing published: `nothing changed: the channel has published no
   build`.
   The name lookup is given the connect time, 10 s, on a thread of its own
   (the system's lookup has no timeout): a resolver that never answers is
   "the channel cannot be reached", not a round held for good (#830). One
   lookup runs at a time: while one that ran over has not ended, a round
   starts none and says "an earlier name lookup has not ended".
3. **The build.** The envelope's key fingerprint must be the lamp's and
   its signature must match the manifest's bytes. Then, before anything
   else of the manifest is read, its `min_updater`: one above this
   updater's `UPDATER` refuses the whole build with nothing fetched or
   written, as `refused, nothing switched: build N needs updater M or
   newer, and this is updater U; ...` (in `log.jsonl` as `refused`). This
   happens in a round, in `check` and in `install` alike. Nothing is held
   against the build, so the next round asks again. With
   `LAMP_UPDATE_SELF=on`, a refused build that carries a `lamp-update`
   still gives the lamp that one, and only that one (below, "An updater too
   old for a build"). Because this comes
   before the schema check, a later schema's manifest that sets
   `min_updater` is refused for its updater, not for its schema. An
   updater from before the field refuses any manifest that carries it as
   an unknown field, also before anything changes, only less clearly.
   The manifest must be schema 1
   of the channel followed, every name plain (`lamp-<name>`, never a path),
   every hash 64 hex, every size within 256 MiB, and no field unknown. Its
   sequence must be greater than the one the lamp runs (equal: up to date;
   less: refused), and it must not be a build put back on this lamp before.
4. **What changes.** A part is a unit in `/etc/systemd/system` named
   `lamp-<name>.service`, a file and not a link, whose `ExecStart=` starts
   `/opt/lampOS/<folder>/<64 hex>/lamp-<name>` (a part that names its
   build, as on lamp-4ace and lamp-52e6), or
   `/opt/lampOS/<folder>/current/lamp-<name>` with `current` a link to a
   build's folder (a product part, [install.md](install.md): its build is
   where the link points). A lamp being moved to the product units has both
   for a while, and one round switches each its own way.
   lamp-update's own unit is never one. For each part whose binary the build
   carries:
   - **its unit**: a pinned part uses the build's copy from this lamp's
     own folder (`deploy/<lamp>/<unit>`, the lamp being `--lamp`, or its host
     name), else the lamp's own. Its new build folder is written everywhere
     that unit names the old folder (so the controls' `--config` follows too).
     A linked part prefers a host template through `current`, otherwise the
     product template. When a product candidate is carried, a legacy pinned
     host template is skipped with a journal line naming only the unit, host
     and installed `current` path. Without one, the existing host validation
     still applies. The chosen linked unit must name the same part folder and
     binary. What moves is its link, with any compatible unit change carried
     by the build.
   - **its folder's files**: every file beside its binary now goes to the
     new folder: the build's version of any binary the build carries (the
     voice's `lamp-turn-end`), the old file otherwise (a model, a library,
     settings), hard-linked, or copied on another disk. A folder holding
     anything but files is refused. A folder that is gone is installed
     afresh with the binary alone.
   - **its folder's name**: the binary's SHA-256, as now. If a folder of
     that name exists and holds other versions of the binaries beside it
     (the voice unchanged, its worker changed), the name is instead the
     SHA-256 of the list of the build's binaries in it, one
     `<sha256>  <name>` line each, sorted, as `SHA256SUMS` writes them.
   - A part whose folder and unit (comments and blank lines aside) are the
     same is left alone.

   **Restart order** comes from `[Unit]` `After=` and `Before=` in the
   candidate unit files (`unit/restart_order.rs`), with all managed installed
   services in the graph, including unchanged and inactive ones. This keeps
   a dependency path through an unchanged service without restarting it.
   Prerequisites come first; where several services can go next, name order
   breaks the tie. The old installed graph is also checked here, for
   rollback. A cycle in either graph refuses the build before installing
   or switching files. Ordering names must be literal: quoted names, escapes
   and specifiers are refused. Repeated directives accumulate; an empty one
   does not clear earlier edges, and continued lines are joined.
   `Wants=`, `Requires=` and `PartOf=` do not themselves establish order.
   This orders restart calls, not service readiness or health (step 9).

   **The setup walk under way (#3689, `walk.rs`):** a build with changes
   is not fetched while the person is being walked through onboarding.
   Under way means setup is done (`/var/lib/lampos-setup/done`) and the
   voice's onboarding file has no outcome for one of
   `lamp_msgs::ONBOARDING_STEPS`, the voice's nine step ids, the same since
   the 7071 image. The file is the one the installed `lamp-voice.service`
   names in `LAMP_ONBOARDING_FILE`, else
   `/var/lib/lampOS/voice/onboarding/progress.json`. A `later` step has an
   outcome, so a walk left for later is over. No file counts as under way
   when that unit sets `LAMP_ONBOARDING=on`, as the product's does: the
   7071 voice writes the file only at the first step's end, so the walk's
   hello and its wait for the person to sit have none (Tests' and Third's
   P2 on #4455). Without it, no file is no walk (onboarding off). A file
   that can't be read, or isn't the walk's, is no walk. The round then ends
   `not switched: the setup walk is under way: held, round N of 8` (status
   4), and the next round looks again. The walk is looked at again at each
   second of the switch's quiet wait (step 7), so a round begun before the
   walk's file (its first step, or before setup's `done`) is held there
   the same way, with its new folders installed beside and nothing
   switched (Tests' P2 on #4455). At most 8 rounds on a lamp, 2 hours
   at `serve`'s 900 s: the count is kept in `walk-held` in the records and
   never reset, so a walk left half done never holds updates for long, and
   a redo weeks later is held only for what is left of the 8. The app's
   Check for update starts a round at once (#4153), so each one during a
   walk spends a held round too. Counting rounds needs no clock, which a new lamp may
   set only after its setup. A count that can't be read or kept holds
   nothing. Only the voice's unit file itself is read, not its drop-ins
   (`lamp-voice.service.d/`, `/run/systemd/system`), so two kinds of lamp are held
   once, for up to the 8 rounds: one whose drop-in turns onboarding off,
   and a set-up lamp whose voice walks but never wrote its file (an office
   lamp that skipped its walk), on its first round with this build (Tests'
   P3 on #4455). **Why:** the first customer lamps ship reset, and their first
   update comes within the hour of their setup, while their first voice
   leads the walk. The quiet gate (step 7) keeps a switch off a sentence,
   but a walk waiting silently for the person reads as resting. A climb
   (lamp-update's own switch) is not held: it restarts only lamp-update.
5. **Room.** The disk `/opt/lampOS` is on must hold every file of every
   new folder, counted whole as if nothing could be linked, plus
   `--keep-free-mb` (256). Otherwise refused, before anything is written:
   `the disk has N MB free; the new build needs M MB beside the old one,
   and 256 MB are kept free`.
6. **Install beside.** Each new folder is filled as
   `.<name>.lamp-update` in the same parent, each fetched file hashed as it
   is written and compared with the manifest, synced; only when every
   folder is whole is each renamed to its name. A file that does not match
   (or does not come) removes every folder being filled: nothing is
   installed. The folders made are recorded. With `--apply off` the round
   stops before this step, and prints the plan with each unit's changed
   lines.
   **The skills still load** (the first builder on #1722), judged on the
   folder as the round will leave it. With the build's product skills to
   write (below, "The product's skills"), the lamp's skills folder is
   copied to `skills-staged/` in the records and the writes are made there
   only; without, the folder itself is judged. When the skills part changes
   folder or product skills are written, the build's `lamp-skills list --dir
   DIR` on that folder (the unit's `--dir`, else `/var/lib/lampOS/skills`)
   and the running one's on the lamp's folder are read, each killed after
   30 s, and the staged copy is removed. A skill that loads now and that the
   build would not load (a skill of the person's, or a file of the lamp's
   own, from before #1698's `[permissions]`, under a lamp-skills with its
   checks) refuses the build before the quiet moment, nothing written or
   switched: `build N's lamp-skills would not load these skills of DIR,
   which load now: mine (voice/heard is not in [permissions] topics); ...`.
   Every round tries again, and switches once they load. A skill both
   refuse holds nothing back. A running lamp-skills older than `list`
   (status 2: `list` and the checks came together) loaded every folder; a
   build's that cannot list the folder fails the round, nothing switched.
   Not run with `--apply off`.
7. **The quiet moment.** Before anything is switched: the voice says on
   the bus that it is resting (`voice/state`, said once a second, at most
   3 s old), nothing was heard or said (`voice/heard`, `voice/said`) and the
   voice was in no other state for 45 s, and the program has watched that
   long. Listening means it hears someone, so only resting is quiet. A
   lamp asleep or muted for privacy says no state, only its mute
   (`voice/mute`, once a second, at most 3 s old, #3770): muted, with the
   reason `sleep` or none, is quiet at once, since its microphones are
   closed and no turn can be cut (not while a fresh `voice/state` says a
   turn). The round plays no sound and shows nothing on the ring, so a
   sleeping lamp is switched silently; the voice comes back asleep from its
   saved mute record, and the watch (step 9) reads `lamp/health`, where
   it says it is well with its microphones off, not `voice/state`. The
   bus, not that record, is read: while an awake voice runs, its record
   holds the restart guard's mute. A mute restored
   by that guard after a fault reads on the bus as a mute with no reason
   and counts too: its microphones are closed as well. Without
   `LAMP_HEAD_TOUCH=on` and `--controls-mute` the voice says no
   `voice/mute`, and a muted voice there still cannot be judged. A
   voice that says nothing on the bus cannot be judged and is not switched;
   a voice whose unit is not running hears nobody and is no reason to wait.
   The wait says "waiting for a quiet moment: <why>" once, after its first
   one-second look at the bus, so the reason is what the voice said and
   not the empty gate's "says nothing" (each round's gate starts empty).
   No quiet moment within `--quiet-wait-s` (3600): the new folders stay
   installed beside, nothing is switched, status 4, and the next round tries
   again.
8. **Switch.** The units as they are, each moved link with the build it
   names, and the journal are written. The journal includes the validated
   old startup order of every changed unit, including inactive units,
   preserving dependency paths through unchanged units. Then the new units (a product part's
   only if the build changed it), `systemctl daemon-reload`, and each
   product part's `current` replaced whole: a new link made beside as
   `.current.lamp-update`, renamed over it, the folder synced. Quiet is asked once more: if
   someone spoke meanwhile, the old units go back and nothing is restarted.
   Then the build's product skills are written (planned again first, as the
   lamp may have changed a file while the round waited; each file's bytes
   before are in the journal, written with it). Each changed unit that was
   running is then restarted in the candidate graph's startup order, except
   the voice: when a running part before it restarts too, the voice is
   stopped first and started last (see "The voice stopped first" below). A
   unit that was not running is switched without being started.

   **Completed product Setup:** `lamp-setup.service` may still be serving
   the phone for its ten-minute grace period, or may have ended while the
   update fetched its files. Restarting it after `/var/lib/lampos-setup/done`
   exists stops that process, but the unit's condition prevents a new start.
   Leave it alone: its files and link update for the next setup, without a
   restart or a new watch. This exception requires the exact product unit
   declarations both before and after, the normal installed unit directory,
   a regular completion marker and no unknown unit fragment or drop-in.
   Only install's exact unit-local exit-note hook is allowed. A fresh bounded
   `systemctl show` query must also report the loaded canonical identity,
   exactly that one name, the canonical installed fragment and no drop-ins
   except that hook. Aliases (including loaded aliases no longer on disk),
   custom search paths, failed queries and incomplete metadata retain normal
   restart/watch. The manager is queried only after filesystem recognition,
   after reload, independently on both forward and recovery paths. Reads are
   nonblocking, refuse nonregular or symlink override inputs, and cap each
   file at 16 KiB and each drop-in directory at 128 entries. The ordinary
   merged-`/usr` library alias is checked through its destination. Missing
   completion, a custom unit, a condition reset or uncertain overrides
   retain the normal restart and watch. Completion is checked again after
   inspecting overrides and querying the manager, at selection time; this does not lock out a
   simultaneous reset. The earlier activity snapshot remains unchanged.

   **The voice stopped first (#3689, the first builder's yes on 30 Oct):**
   when the voice is running and a running part before it in startup order
   restarts too (lamp-controls, Memory, Ears, ...), the round reads queued
   speech and checks the 45 s quiet gate once more, then stops the voice
   with a plain `systemctl stop`, while lamp-controls is still up. That is a
   clean stop with nothing in flight, so no build's voice arms its restart
   guard (#960), not even one from before `1c88f2dbd` (#4280). Then the
   other parts restart in startup order, and the voice is started last, as
   the new voice. If no quiet moment arrives within `--quiet-wait-s`, the
   changes are put back and nothing has been restarted. The voice is marked
   restarted at its stop, so a part that fails later puts it back, and the
   old voice is started again. A sentence begun while a prerequisite
   restarts reaches no voice and doesn't hold the switch: the lamp isn't
   listening for those few seconds, inside a moment already judged quiet.
   A round with no other running part before the voice restarts the voice
   as before, with the quiet gate at its own boundary. It applies only when
   the voice itself changes: a round that leaves the voice as it is
   restarts the parts it reads under it, which a voice with #4280 survives.
   The 7071 lamps' first update always changes the voice (Fourth's P3 on
   #4431). A cut just after the voice's stop leaves it stopped, and the next
   start's recovery puts the old unit back and starts the old voice
   (`a_switch_cut_after_the_voice_was_stopped_first_starts_the_old_voice_again`).

   **Why:** the order before this restarted lamp-controls first. The old
   voice's run then closed on an unreadable privacy switch, its own stop
   came about 0.5 s later, and a voice from before train 347
   (`1c88f2dbd`) kept its restart guard's mute: the lamp came back muted
   until a head tap (lamp-06c1, 8474's round, 29 Oct). The voice's own
   release ([controls.md](controls.md), "A switch unreadable, then the
   stop") runs in the voice being stopped, so it can't help a lamp updating
   from a build without it. The ten customer lamps are on the 7071 image:
   their first home update climbs to the new updater first, so this
   updater's order is what runs their round (Tests' 7071 → 8670 run and
   trace on #3689, 29–30 Oct). Old voice and restarted new servers still
   need compatibility in both directions; restart order does not make the
   round atomic.
9. **Watch.** Every second for `--watch-s` (60): each restarted unit must
   not fail, stop, be in `auto-restart`, be started again by systemd
   (`NRestarts`) or change process (`MainPID`). At the end it must be
   running, and if it was in `lamp/health` before the switch it must be
   there again, not stale, and not in error unless it was before. A part
   that never reported is judged by systemd alone. A state query that fails
   (`systemctl show`, at most 5 s) is not known yet and is asked again until
   the watch's end; only one still failing then puts the build back
   ("systemd cannot be asked").
10. **Back.** A part not well: the journal's validated old startup order is
    used without rereading the installed dependency graph. Later changes or
    read failures in that graph cannot veto the saved restart order. Every
    changed unit is then written back as it was, reloaded, every moved link named
    its old build again (replaced whole, as at the switch), each product
    skill's file the switch wrote put back as it was (or removed, with any
    folder made for it if still empty; a file the lamp changed since it was
    written is left, and said), and those restarted are restarted again in
    that saved order. Already-restored and inactive members gain no restart.
    After successful file/link restoration, the same completed-product-Setup
    check avoids a spurious restart, including recovery from an older
    interrupted-switch journal. A reset that removes completion restores
    the ordinary restart rule.
    The saved order must name all journal units exactly once, before any
    already-restored prefix is excluded. A present malformed order keeps
    the journal and every restoration prefix intact and starts no new round.
    Older journals without the field reconstruct the old graph from saved
    declarations and unchanged installed units; an unreadable or cyclic
    graph retains the journal before any restoration write. Their journal
    is not rewritten to add an order, preserving later fields it may hold.
    Unless the voice is what failed, queued speech is read at its own
    restart boundary and the between-turn wait runs there (resting, nothing
    heard or said for 5 s; at once if its state cannot be seen). If that
    bounded wait expires, the voice is put back anyway and this is said.
    They are watched again, the outcome says how the previous build is,
    and the build is refused from then on.
    If installed units cannot be read during cleanup, all build folders
    and their `made` records are kept, with one journal line; unreadable
    inventory is never treated as proof that a live build is unused.
    **What a part not well said of itself goes into the outcome** (#1505):
    the unit that failed on the new build (read before it is put back), and
    the first unit found not well on the old one (the watch stops there),
    each gets how often systemd started it again since this round restarted
    it (`NRestarts`, which a restart asked for flushes) and its own last
    journal line since that restart: `journalctl -u <unit> --since @<the
    restart> -o json`, only entries of the unit's own processes
    (`_SYSTEMD_UNIT`, so not systemd's, systemd-coredump's or journald's
    lines about it), not its main process from before the restart (the old
    build's last words as it stopped), control characters gone, at most 200
    characters. The query uses a 5 s budget (`JOURNAL_WITHIN`) and a 1 MiB
    response cap. It supplies no line if the command fails, the budget
    expires, or the response is invalid UTF-8 or exceeds the cap.
    "build 812 put back:
    lamp-tasks.service ended and systemd is starting it again (systemd
    started it again once; its last line: "tasks: storage: Tasks storage is
    unavailable; …")". **Only in the outcome as printed**, to `lamp-update`'s
    own journal (`Outcome::told`): the refusal in `state.json` and the line
    in `log.jsonl` (`Outcome::kept`) hold the reason alone, since a part's
    line can carry what a person said and those files outlive the journal.
    Asked only of a unit found not well, never when systemd itself could not
    be asked; a round that switches asks nothing.
11. **After.** Switched or put back, the folders this program made that no
    unit or link names now, nor any unit or link as it was before the last
    switch, are removed: the lamp keeps its build and the one before. The
    folder `lamp-update.service` starts this program from (install's
    `update/current`) is kept too, since no round switches it (#1680), and
    so is the folder the running binary is in, when it is under
    `/opt/lampOS/update/`: a round started by hand has no unit of its own to
    name it, and after phase 1 of its own switch `update/current` names
    another while it still runs. A folder it did not make (every one installed by hand) is never removed.

### The product's skills

A round places the build's product skill folders from the channel, on by
default (#2014's P2, the first builder's 01:29 and 02:07 on #2014): the
updater a lamp runs is the one install left, and its baked skills
(`install::SETTINGS`) are that install's, so without this a lamp with
folders from before #1698 would refuse every build with #1698's
lamp-skills, for good. `crates/update/src/product_skills.rs`:

- **Carried:** `lamp-update manifest --skills lampOS/skills` writes
  `lamp-product-skills` beside the binaries: JSON (`lampos-skills/1`) of
  every file of each `install::PRODUCT_SKILLS` folder but its `tests/` and
  hidden files, each text, at most 1 MiB; the `skill.toml` of each of
  `install::PRODUCT_SKILLS_OFF` with `enabled = false` above it
  (`install::OFF_LINE`), as install writes it. It is listed in the manifest's
  `files` with its SHA-256 and size, as a binary is, so it is signed with
  the manifest. No field is added to the manifest: it denies unknown
  fields, so a new one would make every older updater refuse every build.
  An older updater fetches only its parts' binaries and passes this file
  over. A test holds the bundle of the repository, file for file, to what
  `install::SETTINGS` writes.
- **For which lamp:** one with a lamp-skills unit whose `--dir` is
  `/var/lib/lampOS/skills`, install's folder, from a build that carries the
  file. A lamp-skills reading a folder of the lamp's own (lamp-4ace's
  `/var/lib/lamp-skills`) is given nothing. The file is fetched in step 3's
  checks (a SHA-256 that doesn't match refuses the build), also with
  `--apply off`, which lists each file it would write or keep.
- **File by file**, from three: the build's before (its SHA-256 in
  `state.json`'s `settings`, the record install keeps, or one install was
  about to write), the build's now, and the lamp's now. Recorded only: one
  already the build's now. Written: every other. A core skill's files are
  the build's, not the person's (the first builder on #1722, 21 Oct), so one
  the lamp changed, or removed after it was recorded, is put back, and said
  (`put back PATH: changed on this lamp; a core skill's files are the
  build's`, or `removed on this lamp`); a put-back of the build puts the
  lamp's text back. Install writes over a changed core skill's file the
  same way and says `put back PATH: changed on this lamp`. **A core skill
  someone turned on stays on** (the first builder on #2585): an update
  changes what the build ships, not what was chosen on the lamp. For a
  `skill.toml` the build ships off, a lamp file that turns it on and is
  not the build's before (or is already this build turned on) puts its
  name in `/var/lib/lampos-skills-state/on`, beside `off` (one name a line,
  written whole beside and renamed, 0644, read never through a link). One
  the lamp turned off again takes it out. Every skill `on` names is then
  written turned on: install's off line taken out and a top-level
  `enabled = false` made `true`. A lamp with no `on` has none, as before;
  an updater from before never reads it. A name the build no longer
  carries leaves `on`, said once: `NAME is no longer a core skill of this
  build: its turn-on in /var/lib/lampos-skills-state/on is dropped`.
  `--apply off` writes no `on`. `lamp-update.service` has
  `-/var/lib/lampos-skills-state` in `ReadWritePaths` for it; a round that
  can't write it says so and keeps the skill on for that round. What #1913
  keeps is the person's otherwise: the skills they turned off
  (`/var/lib/lampos-skills-state/off`) and a skill's state there
  (`daily.json`), never read or written by a round; and a folder the build's
  bundle doesn't name (a skill the person installed), never read or written
  but by the guard's copy. A skill the build no longer carries is left as
  it is.
- **When:** judged staged before the quiet moment (step 6), written at the
  switch after the units and links and before any restart (step 8), put
  back with the units (step 10), and recorded in `settings` only once the
  switch is well. A switch cut short puts them back at the next start from
  the journal. A build that changes no part here but its product skills
  writes them without a quiet moment or a restart (lamp-skills reloads its
  folder by itself), putting back what it wrote if a file fails, and is the
  lamp's build.
- **The sandbox:** `lamp-update.service` has `-/var/lib/lampOS/skills` in
  `ReadWritePaths`. A round with files to write first makes and removes
  `.lamp-update-probe` there; one that can't (a unit from before) fails,
  nothing switched and the build not refused, so the next round tries
  again: `/var/lib/lampOS/skills cannot be written (...): lamp-update.service
  needs it in ReadWritePaths; nothing is switched`.

Tests (`crates/update/tests/round.rs`):
`a_round_puts_a_core_skills_files_back_and_keeps_the_persons_own`,
`the_guard_still_refuses_a_persons_skill_the_new_lamp_skills_will_not_load`,
`a_build_put_back_puts_the_product_skills_back`,
`product_skills_written_by_a_switch_cut_short_are_put_back_at_the_next_start`,
`a_build_of_only_new_product_skills_places_them_and_restarts_nothing`,
`product_skills_go_only_into_installs_folder_and_only_when_it_can_be_written`;
and `product_skills::tests`, `publish::tests`.

### The commands

```text
lamp-update check|once|serve --channel-url URL [--channel NAME] [--key-file PATH] [--lamp NAME]
                             [--apply on|off] [--every-s N] [--quiet-wait-s N] [--watch-s N]
                             [--keep-free-mb N] [--root DIR] [--bus-root DIR]
lamp-update install --channel-url URL [--channel NAME] [--key-file PATH] [--setup on|off]
lamp-update harness-off
lamp-update harness-on
lamp-update check-units --cat FILE [--product DIR]
lamp-update manifest --binaries DIR --deploy DIR --skills DIR --channel NAME --sequence N --commit SHA --files-url URL
                     [--min-updater N]
lamp-update sign --key-file PRIVATE --manifest FILE --out FILE
lamp-update public-key --key-file PRIVATE
lamp-update writer-probe
```

`writer-probe` prints one line, `lamp-update writer-probe 1 <sha256>`, the
SHA-256 of the file it runs from, and changes nothing. A copy of lamp-update
is run so before it is put at a fixed path (below, "A fixed copy, checked").

`harness-on` puts Harness on the lamp with Harness's own installer and sets
it at boot (#1676), `harness-off` undoes install's Harness at boot, and only that, and
`check-units` says whether a lamp runs exactly the product's units: both
in `docs/install.md` (step 9, and "Checking a lamp runs the product's
units").

`install` puts lampOS on a blank lamp from the newest build: every product
part through its `current` link, the product units, users and zone, the
product settings and every product skill (`/var/lib/lampOS/skills/`),
enabled and not started ([install.md](install.md)). A round writes
the units a build carries for this lamp by name. On a product-installed
lamp it also carries the product's own units (#3466, updater level 6, which
an older updater climbs to first): the new text of each `lamp-<name>.service`
already installed and started through its `current` link, so a switch in a
product unit comes over the air. At updater level 7 it carries a part's
templates, sockets and timers too, where the lamp has them (below, "A
part's templates, sockets and timers"). At level 8 it adds the product units
`install` puts on a lamp that the lamp lacks, a part new to it among them,
enabled as `install` enables them (below, "A part added over the air"). It
never carries the updater's own unit or the fixed helper units, and never
makes a user or a group. Those still come only with `lamp-update install`
run again (#3689). A round writes the product's settings and tmpfiles the build
carries (below, "Settings and tmpfiles") with `LAMP_UPDATE_APPLY=on` on any
install, or with no apply choice at all on a managed, self-updating product
install. With an apply choice of off or an invalid one, or with no choice on
an unmanaged install, they come only from `lamp-update install`. Product
skills don't depend on the apply switch: a round places the build's
`lamp-product-skills` bundle (above) whenever the installed `lamp-skills`
part's unit uses the product folder `/var/lib/lampOS/skills/` and the signed
build carries the bundle, within its size bound.

On a product-installed lamp, a legacy hash-pinned host template no longer
blocks the linked product template (#3796). It is authenticated and skipped
when a product candidate is carried, with a journal line naming only the unit,
host and installed link path. A host template through `current` still wins,
subject to the same folder/binary validation. Pinned installations retain
their host-specific handling. A by-hand `once` uses the lamp's host name unless
`--lamp` is given; no host-name override is needed to ignore legacy pinned
entries on linked parts. `check` never writes, whatever `--apply` says. `once`
and `check` end with
0 (up to date, switched, or only saying), 1 (refused, put back, failed), 3
(the channel cannot be reached), 4 (no quiet moment, or another lamp-update
or setup's helper held the lock, #4524); a wrong option with 2.
`serve` prints a round's outcome only when it differs from the last. The
defaults: channel `lamp-voice`, key `/etc/lampOS/update/key.pub`, lamp the
host name, bus `LAMP_BUS_ROOT` or `/run/lampos-bus`.

The unit is [`deploy/units/lamp-update.service`](../deploy/units/lamp-update.service),
and on a product lamp [`deploy/product/lamp-update.service`](../deploy/product/lamp-update.service):
root (it writes `/opt/lampOS` and `/etc/systemd/system` and restarts
units), `ProtectSystem=strict` with only those two, the product skills'
folder (`-/var/lib/lampOS/skills`), the bus and its records writable, and
on the product also lamp-skills' state (`-/var/lib/lampos-skills-state`).
It runs with `Nice=10` and idle I/O, and is not part of `lampos.target`.
No round switches this unit (`round::OWN_UNIT`), so a lamp's round runs
in the sandbox its image installed: on the shipped lamps, 7071's, which
has today's lines. What a round needs written outside it goes through a
one-shot unit of its own, as the settings writer's (below) and
`lamp-update-apply.service` do. Widening this line never reaches a lamp
already in a home. A test pins the product unit's sandbox to 7071's and
each folder a round writes itself to inside it (`own_unit_line_tests` in
`main.rs`, #4540).

### Its records

In `/var/lib/lampos-update/` (`records.rs`), each written whole:
`state.json` (the build the lamp runs and the one before, up to 32 builds
refused with why, the folders made, the SHA-256 of each product setting as
`install` last wrote it and of each it is about to write, and whether
systemd is still owed an install's `enable`), `journal.json` (only while a switch is
under way; for a product part it holds the link and the build it named, so
a switch cut short is put back by the link too, and each product skill's
file the switch writes as it was before), `skills-staged/` (only while a
round judges the skills folder as it would leave it), `previous/` (the units
before the last switch, and for each product part `<unit>.link`, one line:
the link and the build it named; the way back by hand), `log.jsonl` (one line per round that acted, past 1 MiB begun again
with one old file kept) and `lock`. Every line it says also goes to its
journal (`journalctl -u lamp-update`).

The records stay root's alone. What a part may know of them is said on the
bus instead: a round that acts says `update/status` (#2194,
`lamp_update::status`) as each phase begins (`checking`, `fetching`,
`switching`, `watching`, and `restoring` for a rollback's put-back) and
`idle` with its outcome's name and build when it ends, with the build
`state.json` says the lamp runs and no reason text. For a round that refused,
failed, rolled back, was unreachable or found no quiet moment, the end also
says where it stopped by a fixed name (`reason`: the last phase it said, or
`models` for `WaitsForModels`). An end says when its round began
(`started_at_unix_ms`, its `checking`), for #1683's update alerts. A round
that ends before its first phase, such as one whose lock is taken
(`round.rs`, the lock before `checking`), says neither. One whose lock is
taken by another holder (another lamp-update, or setup's helper restarting
a part under it, #4490) ends `not-quiet`, not `failed`: nothing was tried,
so no update alert says a failure, and the next round tries again. A lock
that can't be taken for any other reason is still `failed`. A new updater's confirmation of its own switch is a round's
end too, `self-confirmed` with the build its dry round found, so it is the
last result the new updater says when it starts telling. A round's end is logged
in `log.jsonl` with `"round": true` and its build, synced with its folder
(the root mount's `commit=600` would otherwise lose it to a power cut within
10 minutes; the log's other lines are not synced), and read back when
lamp-update starts, so the last result outlives a restart and its own
switch. It is said again every minute and lasts three, so a lamp-update that
died mid-round reads stale.
lamp-cloud answers the app's `update.status` from it ([cloud.md](cloud.md)).
A bus that cannot be used is said once, and the round goes on. The other
way, the only thing a part may ask of lamp-update is a round sooner
(`update/check`, #4153, above); a bus or topic that cannot be subscribed to
is said once, and `serve` waits as before.

To go back by hand to the build before the last switch:

```sh
sudo systemctl stop lamp-update
sudo cp /var/lib/lampos-update/previous/*.service /etc/systemd/system/
sudo systemctl daemon-reload
# each product part: point its link back, replaced whole
cat /var/lib/lampos-update/previous/*.link | while read -r link build; do
  sudo ln -sfn "$build" "$link.back" && sudo mv -T "$link.back" "$link"
done
sudo systemctl restart lamp-voice lamp-body   # each unit in previous/ that was running
```

Going back across #1798 on a lamp whose pins the factory step wrote: a
Controls from before it wants `controls.json` to name exactly the pins'
head pads, and install's `controls.json` now names four. Name the pins' own
pads in it (`"head_pads": ["head-1", "head-2"]` for two) before restarting
`lamp-controls`, or it won't start, and on the privacy route the lamp then
hears nothing (install.md).

### Reading a round from the journal

Each line is `lamp-update: <text>` in `journalctl -u lamp-update`. A round
ends with one line, its outcome as `round::Outcome` writes it. The same end
has a name in `log.jsonl` (`outcome`) and on `update/status` (`last`), from
`Outcome::kind`; **that name is never in the journal**, so a run sheet greps
the text below, not `self-switched` or `self-confirmed` (#4561's P2). `serve`
says an end only when its text differs from the last one it said in the same
run, so a lamp up to date for hours says `up to date: build N` once per run of
`serve` (again after a restart, a self-switch or a climb). A `once` or `check`
by hand prints its end on standard output with no `lamp-update: ` before it;
only a self-switch or a climb goes to the journal's form, before its `exec`.

| The journal line begins | Its name | Means |
|---|---|---|
| `up to date: build N` | `up-to-date` | nothing to do |
| `nothing changed: …` | `unreachable`, `nothing-published` | the channel couldn't be read, or has no build |
| `refused, nothing switched: …` | `refused` | the build isn't taken. `…: build N needs updater M or newer, and this is updater K; …` (`TooOld::why`) means **no climb**: `LAMP_UPDATE_SELF` off, the build not newer, its updater put back before, or its updater not this channel's (each ends `…; waiting for a newer updater`); or the build carries none (`…; it carries no lamp-update, so waiting for a build that does`) |
| `not switched: …` | `not-quiet` | tried again next round: `no quiet moment in S s: …`, `the setup walk is under way: held, round R of 8`, or `another lamp-update is at work` |
| `switched to build N; every part is well` | `switched` | the parts switched and the watch passed |
| `build N runs; lamp-update switches from its build A to B, which must confirm itself` | `self-switched` | a new updater, after the parts (or with none to change) |
| `build N needs updater M; lamp-update switches from its build A to B, which the build carries, and takes nothing else of it until that one has confirmed itself` | `self-switched` | a **climb**: only the updater the build carries is taken |
| `build N put back: …; the previous build is well`, or `…; the previous build is not well either: …` | `rolled-back` | the parts were put back (the watch, or a start that failed). The second ending, the old build failing its own check too, is the one to post at once; grep `build N put back:` for both |
| `failed: …` | `failed` | an error, said with why |

Inside a round: `switching to build N`, `waiting for a quiet moment: …`
(once per wait), and `putting build N back: …` before a `put back` end.
After a self-switch or a climb, the new updater says
`confirmed: lamp-update runs its new build` once its dry round passes
(`self-confirmed` in `log.jsonl`), or `not confirmed yet: …` and
`not confirmed: …`, each when its text changes, looking again every 30 s
(`main.rs`). A climb's parts switch follows the confirm in the same `serve`
run. A new updater that never confirms is put back by `lamp-update-back`,
in its own unit's journal (`journalctl -u lamp-update-back`, lines
`lamp-update-back: …`), not `-u lamp-update`.

### The second route's services in a round

A round places `/etc/lampOS/voice/second.json` when the build's voice unit
names it (`LAMP_VOICE_SECOND_CONFIG`), before any unit is written
(`Round::place_second`; #1940, the reviewer's P1 and the first builder at
00:11, 20 Oct). It places it as install does, with install's own writer
and records: 0600, from the round's own `install::SECOND`. A missing file,
or one still install's and not the build's, is written; a lamp's own is
kept. A round that cannot place it writes no unit and keeps the old one
(`Outcome::Failed`), so the running voice goes on.

A round runs the updater a lamp already has (below), so a lamp whose
updater predates this step gets the voice's new unit without the file. The
voice covers that one transition itself: as it starts, with
`LAMP_VOICE_ROUTE=second` at the product's path and no file there, it
places the same bytes (`second_config::place_product_if_missing`), and a
later install takes them as its own.

### lamp-update's own build on a product lamp

With `LAMP_UPDATE_SELF` off, a round never switches lamp-update itself
(`lamp-update.service` is never a part), on either kind of lamp. On a
product lamp its unit also runs through `/opt/lampOS/update/current/`. It
moves to its own newer build only when `lamp-update install` is run again,
which installs every product
part's newest build beside the old and moves each link, its own among them,
without restarting anything; the new updater runs from its next start.
Replacing the link under a running updater is safe on Linux: the program
already running stays mapped.

### lamp-update switching itself (`LAMP_UPDATE_SELF`, on for the product)

With `LAMP_UPDATE_SELF=on` in `lamp-update.service`'s environment, a round
also moves lamp-update to its build's own binary (`own.rs`, #1680). Off, or
absent, a round is exactly as above: no `self.json` is ever written, so the
confirmation and lamp-update-back find nothing to do. The product unit sets
it since 25 Oct, before the first customer lamps: without it a customer's
updater could change only by running install again on the lamp, so no later
build could carry it units or settings.

1. **Phase 1.** Once a round's parts run build N (the round switched to it,
   or found it up to date), and N's `lamp-update` is not the one
   `update/current` names, nor one refused before, the round installs it
   beside in `update/<sha256>/` (checked like any part's, with room for it
   and `--keep-free-mb` to spare), writes `self.json`, then moves
   `update/current`. The outcome is `SelfSwitched`, and `serve` (or `once`)
   `exec`s the new binary in its own place, with the same options: the same
   process, so systemd restarts nothing and the records' lock, opened
   close-on-exec, is let go. An `exec` that fails puts the link back at once.
2. **Phase 2.** The new binary, before its first round, confirms itself only
   once a dry round (nothing applied) has fetched the channel's manifest and
   checked its signature, and it has read `state.json` under the lock; then
   `self.json` goes. A binary that cannot reach the channel or refuses the
   manifest waits and tries again every 30 s, and doesn't confirm.
3. **Back.** `lamp-update-back` is a copy of the installed build's
   `lamp-update` at `/usr/local/lib/lampos/lamp-update-back`. It is outside
   `lamp-update.service`'s `ReadWritePaths`, so no round writes it itself.
   `install` writes it, and so does the writer's one-shot unit, which a round
   starts to refresh it from a confirmed lamp-update ("The settings writer
   refreshed", #4178). `lamp-update-back.service` runs it as
   `lamp-update-back back`. It is started by `lamp-update.service`'s
   `OnFailure=` and every 2 min by `lamp-update-back.timer`. When
   `self.json` is there and lamp-update is not `active`/`running`, or 10 min
   (`own::DEADLINE`) have passed since the link moved, it points
   `update/current` at the old build again, adds the new binary's SHA-256 to
   `self-refused` (never switched to again), removes `self.json`, logs a
   round's end, `self-put-back`, and runs `systemctl reset-failed` and
   `restart --no-block lamp-update`. Otherwise it ends, having changed
   nothing. So the old updater's `update/status` says `self-put-back` as the
   last result when it starts again (#1683's update alerts). The line comes
   after `self.json` is removed, so a cut between the two leaves the
   put-back done with no line, and `update/status` says the end before it.
   Logging first would log it twice when `back` runs again. The window is a
   few calls either way. An unreadable `self.json` it puts back logs the
   same. A helper copied from a build
   before this says it only in its journal, until a confirmed lamp-update
   refreshes it. The unit has
   `ConditionPathExists=/usr/local/lib/lampos/lamp-update-back`: on a lamp
   not installed since #1680 (a round adds only the product units of
   `install`'s list, never this helper, and never changes
   `lamp-update.service`), the timer
   and `OnFailure=` find the condition unmet and systemd skips the start,
   which is not a failure. lamp-health reads parts' reports on the bus, not
   systemd's unit states, so a skipped or failed lamp-update-back is no part
   in error either way.

**Recovery must be ready before handover** (#2700). With the self-update
switch on, an actual candidate is taken only when the fixed fallback is a
regular executable file, its service is loaded and unmasked, and its timer
is loaded, persistently enabled, active and waiting or running, with
`lamp-update-back.service` as its target. A timer enabled only at runtime
does not qualify: it would lose its enablement on reboot. A service reported
as `transient`, `linked-runtime`, `enabled-runtime` or `generated` is also
refused; these states do not establish a recovery service for the next boot.
The usual timer-started service is `static` and may be inactive. `install` enables
the timer but does not start it; before a self-update it must also have
started, normally during boot.

The check runs after unchanged or previously refused candidates are skipped
and before the updater is downloaded, `self.json` is written or its link
moves. It never installs recovery or starts/enables a unit. If recovery
cannot be established, a journal line says why and only self-update waits:
the parts' completed result is kept. A later round can retry the same
candidate when recovery is ready. The updater-first `min_updater` path uses
the same check and keeps its existing refusal until it can climb. With the
switch off there is no recovery query. The check observes the selected
systemd properties at handover; it does not verify the service fragment's
storage location, certify arbitrary unit overrides or protect against an
administrator changing recovery afterward. The `systemctl show` query uses
`output_within` with the existing 5 s `JOURNAL_WITHIN` query budget and a
1 MiB stdout limit. A failed, timed-out, invalid UTF-8 or oversized query
defers self-update just as unavailable recovery
does. It does not enable or start the recovery timer. With the switch on in
`lamp-cloud`'s environment as well, a health report says lamp-update waits
while the timer is not armed (`waits`, `cloud.md`, "Health and crash
reports").

The same output reader serves effective Setup identity, pre-restart MainPID
and last-journal-line queries. It returns the complete UTF-8 response or none,
never a truncated response read as successful state. Output is read
nonblocking; the leader remains unreaped while a descendant holds stdout or
a failed exit needs group cleanup. An unsuccessful group is stopped before
the leader is reaped, including descendants that closed their output. If
ownership can no longer be proved because another reaper consumed the child,
the query fails without signalling that identity. Successful completion does
not claim cleanup of detached descendants or descendants closing output.
The budget does not establish a hard bound on kernel spawn/reap or filesystem
I/O, cancel systemd jobs, or change stop/restart allowances. The separate
`Systemctl::states` read keeps its existing behavior in this change.

**Restarting after a partial self-switch** (#2810). The host fixtures in
`tests/round/self_cuts.rs` stop at five logical persisted prefixes: the
candidate is synced and recorded, `self.json` is saved before the link
moves, `current` names the new updater, confirmation has removed
`self.json`, and put-back is interrupted before or after restoring the old
link. Round and confirmation calls construct a fresh `World`; recovery
reads the files left by the prior operation. Distinct old and new shell
fixtures arrive through the signed install/build path; the tests execute
the selected updater and the unchanged fixed fallback, check the
pending/refused records, and finish the next round. They also keep parts
updates working with self-update off.
The cuts use existing journal callbacks and occupied atomic-file names;
they do not add a production hook or wait for a real timer. A separate
put-back subcase reconstructs the prefix after the new build's refusal is
persisted but before `self.json` is removed: it lets the real put-back
finish, then restores only the exact pending bytes captured beforehand.
Retry must clear that record without duplicating the refusal or changing
the selected old updater. This subcase does not interrupt execution at that
prefix. These are logical filesystem-prefix checks, not physical power-loss
or fsync durability proof, and the stubs do not exercise the daemon's `exec`
handover.

The builtin-only stubs have a test-harness startup watchdog of five seconds
from spawn's return to the first observed stdout byte. That byte starts one
absolute two-second deadline for both successful exit and stdout EOF; later
bytes do not renew it. This changes the fixture's old two-second-after-spawn
boundary, not a product startup or recovery deadline. A single owner polls
nonblocking stdout and child status at up to five-millisecond intervals; it
does not wait for a complete line or block reading after the leader exits.
The exact old/new payload (including its final newline), successful status,
file bytes/mode, links and recovery records are still checked. Output beyond
the former 128-byte capture bound fails explicitly; observed bytes, including
invalid UTF-8, remain in failure diagnostics.

Timeouts report the check number, caller, selected path, expected script hash,
PID, startup/finish phase, captured bytes, EOF/status and poll timing. A final
status read is diagnostic only and cannot clear a timeout. The caller retains
the direct child and its original kill/reap cleanup, whose results are logged
on failure. These byte-checked scripts create no descendants; they retain the
outer runner's inherited process group. The collector starts no thread and
cannot leave a reader join waiting on an open pipe. Separate controls own
held descriptors explicitly and exercise delayed first output, partial/full
output followed by a hang, held EOF, wrong or missing output, nonzero status
and read errors. This harness change does not establish the cause of, or
qualify, either historical failed Mac run (#1569/#3012). Runtime updater code
and recovery assertions are unchanged.

**Unclean process death at the write boundaries** (#2810). The separate
Linux `self-process-cuts` test uses the same signed install/round fixture in
a single-threaded driver. Its parent traces an owned child, stops immediately
after a successful directory `fsync`, checks the exact pending/link/refusal
prefix while the child is stopped, sends SIGKILL and requires signal 9 when
reaping it. There is no Rust unwinding or reconstructed pending file at a
cut. Seven stops cover the synced candidate (before its `made` record),
pending before link replacement, the new link, confirmation cleanup, the
restored old link, the refusal before pending cleanup, and put-back cleanup.
Another process then recovers from the same files and completes the next
round. The selected, retained old/new and fixed fallback executables are
checked separately. No runtime hook, quiet-window change or retry shortcut is
added. The record lock must be released by process death itself.

Run `cargo test -p lamp-update --test self-process-cuts -- --nocapture` on
Linux 5.3 or newer, little-endian ARM64 or x86-64, with ptrace permitted for
owned children (Podman needs its tracing capability/seccomp allowance).
The custom driver exposes one libtest-compatible named case to nextest and
runs before creating any threads; the Mac reports it ignored. Each traced
operation/recovery is bounded by 120 seconds, cleanup by five seconds. A
missing cut, unexpected signal, timeout or failed executable is a failure.
This checks the real library in killed processes with executable fixtures,
not installed-daemon exec, real systemd recovery, lost kernel caches or
physical flash durability. The attended unplug remains separate evidence.

**The real CLI exec and confirmation** (#2810). The Linux `self-cli-exec`
checker runs the built updater executable against a private signed local
channel. Distinct trailers outside its ELF segments give old/new identities
of the same implementation. The parent observes the old exec and then its
same-PID candidate exec, verifies executable hashes, start identity, argv,
environment and the real persisted pending/current/state prefix, then detaches.
The candidate's actual main must confirm exactly once; its next ordinary
round must remain at the installed sequence. A self-switch-off control exits
successfully at that same sequence but must be rejected for missing candidate
exec. One named nextest case contains both paths; Mac reports it ignored.
Run `cargo test -p lamp-update --test self-cli-exec -- --nocapture` on Linux
ARM64 or x86-64 with owned-child ptrace permitted. Child waits are bounded
by 30 seconds within a 120-second case budget, and failure cleanup by five
seconds. Systemctl is an exact fixture, not a real service. This does not
test distinct release versions, fixed-path fallback, timer expiry, production
quiet/health timing, cold boot or physical power loss.

**Two published updater versions recovering each other's work** (#2809).
The manual `release_pair` integration target uses the actual ARM64
`lamp-update` executables from releases 6408 and 6449, with pinned SHA-256
digests. It runs both directions: `release_6408_recovers_a_cut_written_by_release_6449`
and `release_6449_recovers_a_cut_written_by_release_6408`. The writing CLI
creates the journal and helper request itself. A controlled command boundary
stops it after settings and candidate units have changed; the fixture kills
that writer, then asks the opposite-version CLI to recover. Apply and Back
also cross the helper-version boundary. The fixture checks exact restored
files, modes, links and recorded hashes; it does not author the recovery
journal or call the current recovery library in place of the old executable.

These two cases are explicitly manual/ignored in an ordinary crate run:
their immutable external release artifacts and cached ARM64 container image
must already be present. They do not download a binary or pull an image.
Run them explicitly with the release files:

```sh
LAMP_COMPAT_6408=/path/to/release-6408/lamp-update \
LAMP_COMPAT_6449=/path/to/release-6449/lamp-update \
cargo +1.96.0 test -p lamp-update --test release_pair -- --ignored --nocapture --test-threads=1
```

Execution uses a disposable network-disabled container, without host mounts,
and a strict fake systemctl boundary. Fixture/result directories are retained
for inspection. This is distinct-version executable/process recovery evidence,
not a production systemd sandbox, release-signature acceptance, attended motion,
cold boot, lost kernel caches or flash durability. A named case only counts
as passed when its explicit run and cleanup succeed; the ordinary CI skip is
not release-pair qualification.

**Published reminder and task readers** (#2809). Schedule, Tasks and Voice
have separate ignored `release_pair` targets using the published 6408 and
6498 executables. A dedicated ARM64 PR job runs those explicit cases and
retains their exits, protocol exchanges and stopped-state snapshots. Schedule's
payload is byte-identical in the pair; Tasks and Voice have distinct hashes.
The fixtures distinguish reminder continuity, task restart fencing and delivery
ledger recovery from real speech, external task completion and physical reboot
acceptance. See the [pair's artifacts, scope and commands](verification/reminder-task-release-pair-20261024/README.md).

**Published Actions output-cap state** (#2809). The separate Actions
`release_pair` target runs 6408 → 6498 → 6408 and the reverse using the
actual pinned executables. It checks active restrictions, cancellation
tombstones, exact no-op/refusal persistence and a fresh replacement after
cancellation over private sockets and bus paths. Both published Actions
binaries are byte-identical. No mixer or output owner participates, so
durable acceptance and bus publication remain explicitly unconfirmed output.
The two cases are ignored in ordinary tests and require explicit native
execution; see the [cap-state verification record](verification/caps-state-release-pair-20261024/README.md)
for qualification status, retained evidence and limits.

**Published Settings store readers** (#2809). The explicit Settings
`release_pair` target uses actual pinned 6408/6498 servers for private desired
values, guarded mutations and retry receipts in both directions and back.
The ordinary current-CLI driver is separate from the released-artifact cases.
Unknown stored fields and unknown tagged values remain refused; this test-only
slice adds no forward-field policy or setting consumer. The
[scope and artifact contract](verification/settings-release-pair-20261024/README.md)
separates software store qualification from hardware, updater and power-loss
acceptance; a pass requires the dedicated official native run.

**Published Surround candidate readers** (#2809). The separate Surround
`release_pair` target stages three synthetic provider candidates through actual
Unix HTTP endpoints across 6408 → 6498 → 6408 and the reverse. The selected
published payloads are byte-identical. It checks manifest/payload hashes,
generated operation IDs and journal admission history, no automatic replay on
reopen, and the existing collection of an unselected superseded generation.
Unknown operations, authentication enum values and provider fields remain refused;
synthetic unknown candidate fields and manifest schema 2 refuse startup without
changing those bytes. This adds no runtime compatibility policy. The dedicated
ARM64 workflow uses a fresh network-none container with private tmpfs state;
ordinary tests use the current CLI. See the
[fixture scope and retained-evidence contract](../crates/surround/tests/release_pair/README.md).
No configuration is selected or applied, and no provider, device, updater or
physical power-loss acceptance is claimed.

**A good build cut off from the channel is put back.** The confirmation
needs the channel: a new lamp-update that cannot reach it for the 10 min of
the deadline, a network down or a router restarting, is put back and its
binary added to `self-refused`, though nothing was wrong with it. This is
on purpose (the first builder's rule on #1680: if the channel can't be
reached, wait for it and don't confirm; the deadline then puts the old one
back): an updater that has not shown it can fetch and check a signed
manifest is not trusted to be the one that updates the lamp. The parts'
own rounds go on with the old lamp-update. What recovers it is the next
release whose `lamp-update` binary differs (a new SHA-256 is not refused):
the lamp switches to that one as to any other. A later build with a
byte-for-byte identical `lamp-update` stays refused, so a lamp seen with
`self-refused` after an outage is waiting for a release that changes
lamp-update, not broken.

**A build put back across a reboot is tried once more** (#2810). When a
switch begins, the round notes the build and this boot's kernel id
(`/proc/sys/kernel/random/boot_id`) in `self-began`. When `lamp-update-back`
puts a build back, it notes the build and the boot it does so on in
`self-put-back`. A power cut or a reboot
in the switch's 10 minutes reads to `lamp-update-back` the same as a build
that can't run. On a lamp with no clock of its own, the clock's jump at the
next boot can also pass the deadline before the confirming round runs.
- **Tried once more:** a refused build whose switch began on one boot and
  was put back on another. An ordinary reboot later doesn't count: only the
  boot of the put-back does. The round moves it from `self-refused` to `self-retried`
  (retried first, so a cut between the two leaves it refused for good), and
  says so.
- **Final:** a build put back on the boot it began on (it failed, or hung
  past the deadline) stays refused, as before. So does one put back a second
  time, one whose boot is not known, or one put back by a
  `lamp-update-back` that notes no boot (7071's, until the settings writer
  is refreshed).
- **Not covered:** a stop by hand without a reboot.
- **Old updaters:** the rule is read by the lamp-update that runs. A lamp
  whose own updater is older (7071's) never retries, and still waits for a
  release that changes lamp-update.

**An updater too old for a build** (#1680). When a signed build's
`min_updater` is above this updater's `UPDATER`, the build names a
`lamp-update` (read leniently, before the schema check), and the build is
of the channel followed, newer than the one the lamp runs, and its updater
hasn't been refused before, phase 1 runs for that binary alone. No part's
file, unit or link is fetched or changed. The outcome is `Climbs` ("build N
needs updater M; lamp-update switches from its build ... to ..., which the
build carries, and takes nothing else of it until that one has confirmed
itself", `self-switched` in `log.jsonl`). `check` says `lamp-update first`.
The new binary confirms itself as in phase 2, and a dry round that finds
an updater it would take this way counts as proof that it can still
update. Then it reads the same build again and takes the rest. So a lamp
climbs one updater a round, however old it is. Without a carried
`lamp-update`, or with the switch off, the build is refused whole and
nothing is written.

**One lock.** There is no lamp-update timer: `serve` runs its rounds in one
process. Every round, `install`, the confirmation and lamp-update-back take
the same lock (`flock` on `/var/lib/lampos-update/lock`, not waiting). While
`self.json` waits, every round is refused before it reads the channel, so an
updater restarted in phase 2, or the old binary run by hand, never starts a
second round.

**`self.json` is a contract for good.** The lamp-update-back on a lamp is as
old as its install, and it must read what any later lamp-update writes. The
file is `{"version": 1, "old": "<64 hex>", "new": "<64 hex>", "at_unix_ms": N}`.
These four keep their meaning in every version. A later version may add
fields, and back reads any version and ignores fields it does not know. Back
does three things only: the link, the refusal and the restart. The deadline
is on the wall clock: on a lamp without a battery clock, a switch made
before NTP set the time may be put back early, or late, once it is set.

**A `self.json` that can't be read** (#1505) is a switch that failed, never
one confirmed. The builds are taken as known only when `update/current`
moved after the file was written (so it names the new build) and
`/opt/lampOS/update/` holds exactly one other build with a `lamp-update` in
it (the old one). Then the link goes back, the new build goes into
`self-refused`, and the file is renamed `self.json.corrupt-<ms>`: by the new
updater in its confirm step, which then execs the old one, or by
lamp-update-back on its usual gate, the deadline counted from the file's
mtime. Otherwise nothing is guessed: the file is renamed the same way with a
`self-set-aside` line in `log.jsonl`, the link is left, and rounds go on;
`serve` does so only after the dry round a confirmation needs. A round by
hand is refused while the file waits. Only a lamp-update-back installed from
this build on does its part.

### Models in the manifest (`LAMP_UPDATE_MODELS`, off)

A build can carry the models its parts read (`models.rs`, #1680): a
`models` list in the signed manifest, each entry in
`deploy/product/models.json`'s form (`path`, `url`, `bytes`, `sha256`, and
an optional `archive` for a file inside a pinned `.tgz`).
`lamp-update manifest --models deploy/product/models.json` writes it and
raises `min_updater` to 2 (`manifest::MODELS_UPDATER`), so an updater from
before the field climbs first. The current `UPDATER` is 9; a build with the product's units raises the floor to 9 (`manifest::ORDER_UPDATER`), past their service units (6), a part's templates, sockets and timers (7) and the units it adds (8): see "Every lamp switches in its build's order". A manifest
without models doesn't write the field and is byte for byte what it was.
A model's path must be under `/opt/lampOS/`, neither the store nor in it, nor in a
build's folder (a part with a 64-hex name) nor a `current` link itself, in
plain parts.

With `LAMP_UPDATE_MODELS=on` in `lamp-update.service`'s environment:

- **Two kinds of path.** A path with a `current` part
  (`/opt/lampOS/voice/current/smart-turn-v3.2-gpu.onnx`) belongs in that
  part's folder beside its binary. It is one of the new folder's files, so
  the part's link (or its unit) switches it. A folder whose binary is the
  same but whose model changed gets a new folder, named by its list, model
  lines included. Any other path (`/opt/lampOS/eyes/models/...`, the name's
  `/opt/lampOS/name/models/`) becomes a relative link into the store,
  `/opt/lampOS/models/<sha256>`. No part or unit changes for that: the eyes
  and the name open the same paths as before.
- **Only what changed is fetched.** A model whose file is already there
  with its SHA-256 (in the old folder, or at its path) isn't fetched, and a
  file install placed stays a file. A store entry already there and whole
  isn't fetched again. A file at the path that isn't install's pin (no
  record of it in `state.json`) is the lamp's own: it is kept, and said.
- **Room first.** Step 5 counts each new store entry (with its archive) on
  top of the new folders, before anything is fetched: `... needs M MB beside
  the old one (K MB of it models) ...`, refused and kept in `log.jsonl`.
- **Checked, all or none.** At step 6 each store entry is fetched under a
  temporary name and checked by size and SHA-256 before it gets its name. A
  mismatch removes what this round fetched and refuses the build, with
  nothing switched. A channel out of reach mid-fetch is `unreachable`, as
  for a build's file, and tried again next round.
- **Switched with the parts.** At the quiet moment, after the units and
  links, each changed path is pointed at its store entry, replaced whole. A
  file install placed is first kept in the store by its bytes. The journal
  holds each path as it was: put back in reverse after the parts' links on
  a failed switch, and at the next start after a cut. A file comes back as
  a file. Every path is tried even when one fails; then the parts are still
  restarted on the old build, but the journal is kept, nothing is pruned,
  and the next start tries again. A part whose unit names the path, or its
  folder as a folder, is restarted and watched with it, even when its own
  binary didn't change, and after a cut too when a path it reads isn't as
  it was.
- **Prune.** A store entry stays while the build the lamp runs or the one
  before it names it (`state.json`'s `models` and `models_before`), or a
  model path under `/opt/lampOS/` links to it now, so a `state.json` set
  aside as unreadable leaves no link pointing at nothing. The others go,
  and an empty store goes too.

Off, or absent: a build whose models are the lamp's is taken as today, its
models ignored. A file at a model's path that install didn't place is the
lamp's own, kept and said, as with the switch on. A build with any other
model whose file on the lamp has another SHA-256 is refused
(`... and LAMP_UPDATE_MODELS=on is not set`, `refused` in `log.jsonl`),
with nothing fetched or written, since its parts may need that model: the
rest isn't taken without it, which is what `min_updater` 2 is for. The
build waits for the switch. A new lamp-update's dry round counts only what
it is switched to apply, so on a lamp that switches itself it confirms on
such a build (`Outcome::WaitsForModels`), stays, and no later round climbs
again.

What the release lists today (`models-2026-10-16`, unpacked): the voice's
57.5 MB (turn-end model 32.4, ONNX Runtime 25.1, from a 10.3 MB `.tgz`),
the eyes' 38.9 MB (0.2 and 38.7) and the name's 3.0 MB (1.1, 1.3 and 0.6):
99.5 MB in all. With two builds kept, only a model that changed costs a
second copy. If every model changed at once, that would be 99.5 MB beside
the old ones, plus the archive while it is unpacked, plus
`--keep-free-mb`.

### Product service units and settings over the air

The publisher also carries the ordinary binary-backed services in
`deploy/product/`, under the reserved manifest lamp name `product`.
Their assets are `product.<unit>`, with the same signed SHA-256 and size
checks as lamp-specific units. A product carrier needs updater level 6;
an older updater first takes the carried updater through the existing
self-update and confirmation path. Manifests without product entries keep
their previous representation and behavior.

A generic entry applies only to an already installed service started
through its `current` link, with the same part folder and binary. An exact
`deploy/<lamp>/` entry through `current` takes precedence regardless of manifest
order. On a linked installation, a valid legacy hash-pinned host entry is skipped
when a product candidate is carried, before that precedence is decided; the
product entry can then apply. Without a product candidate, the previous host
validation remains. The skip line contains the unit, host and installed link path, never unit text, arguments
or environment values. Malformed or mismatched linked entries still reach the
existing refusal checks. Pinned installations keep their lamp-specific unit
handling. Without an eligible carried entry, the installed unit remains. A product
entry of level 6 never adds or enables a missing part (level 8 does, below,
"A part added over the air") and never rewrites local drop-ins. Unit text, binary links and settings use the existing quiet
switch, journal, health check and fallback; a failure restores the exact
previous unit and link before restarting the old part.

The carrier covers ordinary part services, and from level 7 their
templates, sockets and timers (next). It does not replace the updater's own
service, the fixed fallback/helper units, targets or slices. In particular, the install-time fallback
understands the updater's old link contract, not a backup of its service
text; changing that contract requires a separately compatible fallback.
Those exclusions are not claims of a complete systemd effective graph.

`lamp-update manifest` now includes product settings and the reset plan
by default when its deploy tree contains `product/`, as the release
publisher's tree does. `--settings off` explicitly omits them; a deploy
tree without a product folder keeps the old default. No key, credential,
zone or lamp calibration is added to the settings allowlist (the Skills
Store's public key goes in the skills folder it already takes; below).

Fresh product installs explicitly set `LAMP_UPDATE_APPLY=on`. An existing
product updater without that variable may apply a signed level-6 product
carrier's settings only when self-update is enabled and its regular base
service starts `update/current/lamp-update`. Explicit `off`, an unknown
value, an unmanaged updater or an older non-product manifest keeps the
old off behavior. This lets the existing settings helper operate after an
updater climb without rewriting the updater's own service. Its signed
settings request and fallback records keep their existing format.

These changes add no work to Voice's input path. The existing quiet checks
still gate actuator restarts; local fixtures use their fake clock and do
not establish device timing, systemd sandbox behavior or power-cut safety
on a shipped lamp.

### A part's templates, sockets and timers (#3689)

At updater level 7 (`manifest::COMPANIONS_UPDATER`) the publisher also
carries, under `product`, each `lamp-<x>@.service`, `lamp-<x>.socket` and
`lamp-<x>.timer` in `deploy/product/` that belongs to a part
(`crate::companion`). A template belongs to the part its `ExecStart=` starts
through that part's `current` link. A socket that accepts each connection
belongs to its template's part. A timer belongs to the part of the service
it starts. The keeper's template and socket start `/usr/local/bin/lamp-keeper`,
and `lamp-update-back.timer` starts the fixed settings writer, so neither is
carried. A socket that hands one listener to a running service is not
carried either: systemd refuses to start such a socket again while its
service runs. A lamp's own folder carries services only. A unit's file in
the channel is a release asset named `<lamp>.<unit>`, and an asset's name
can't hold `@`. So a manifest names a template `lamp-<x>.template.service`
(`manifest::manifest_name`), and a round gives it back its `@` on the lamp
(`Unit::on_lamp`). No part's service has a `.` in its name, so the two forms
never meet, and a manifest that names a unit with an `@` is refused. Such a
build has `min_updater` 7 at least (8 from "A part added over the air"), so
an updater of level 6 (7071's, after its climb) climbs again first, and the
manifest's other readers, `lamp-update-apply` among them, never read these
names.

A round takes one only where the lamp already has that file, a regular one,
belonging to a part installed here through its `current` link. The build's
must start the same unit, of the same part. One that would start another
part or another unit is refused before anything is written. One the lamp
lacks is not carried as a change: at level 8 it is added instead (next).
Each that changes is written with its part's switch, in the same journal and
under the same quiet checks. A template is reloaded and never restarted: an
instance a socket starts after the reload runs it, and one already running
finishes on the old text. A socket that was listening, or a timer that was
waiting, is restarted with the switch in startup order and watched as a
part is. A build put back, or a switch cut short and recovered at the next
start, writes each back as it was, with the rest.

Not shown: a real systemd restarting a socket while an instance of its
template runs, and a carried template on a 7071 lamp. The tests use the
round's fake systemd.

### Every lamp switches in its build's order (second-P1)

Every product build asks for updater level 9 (`manifest::ORDER_UPDATER`):
a round stops the voice first and starts it last (#4431, step 8). An
updater of level 8 or lower reads such a build as too new for it, takes
only the lamp-update it carries, and that one confirms itself and, in the
same `serve`, runs the switch in its own order: on lamp-9adf a confirmed
self ran its next round 2 s later. Without it, a lamp's own older updater
ran the switch, restarting lamp-controls before the voice, and a lamp on a
build from 28 Oct 17:16 to 29 Oct 09:59 (8237 to 8498: updater 8, from
before the voice's own guard release `1c88f2dbd`) came back muted until a
head tap. The ten customer lamps shipped on the stable of the day they
were set up at the factory (the lead, #1711, 30 Oct), which may be one of
those. The cost is the climb itself, about 20 s, on each lamp's next
update; the self-switch every build already makes moves from after the
parts to before them. A lamp that put a carried lamp-update back for good
(`own::refused_for_good`) now takes nothing from a build carrying that same
lamp-update, where before its own updater still switched the parts; that ends
at the next build whose lamp-update differs (Tests' P3 on #4582). A
development lamp on `deploy/units/lamp-update.service`, pinned with no
`LAMP_UPDATE_SELF`, refuses every product build from here ("needs updater 9
or newer") until it is installed again. The test:
`an_updater_of_level_eight_climbs_before_it_switches_any_part`.

### A part added over the air (#3689)

From updater level 8 (`manifest::ADDS_UPDATER`) a round adds a product unit
the lamp lacks (`crate::added`). Every build with the product's units asks
for at least level 8 (9 since second-P1, above), so an updater of level 7 climbs first. It adds only:

- **On a lamp installed as the product:** a managed updater (its own unit
  starts `update/current/lamp-update`), and every part started through its
  `current` link. A lamp of its own pinned units, as lamp-4ace is, is never
  given the product's.
- **What `install` puts on a lamp** (`install::PRODUCT`) and the build
  carries under `product`. So the keeper's helper and the own keys' helper,
  which `install` leaves out until their lamp checks, are not added either.
  A unit the lamp has as a link (a masked one) or anything but nothing is
  left as it is. Only `/etc/systemd/system` is looked in: a unit of the same
  name in `/lib/systemd/system` or `/run/systemd/system` would be shadowed by
  the added copy, and lampOS ships none there.
- **A unit whose users the lamp has:** its `User=`, `Group=` and
  `SupplementaryGroups=` (root needs none), read from `/etc/passwd` and
  `/etc/group` only. A user of another source (userdb, NSS) or a numeric
  one reads as missing, which fails safe. A round can't write
  `/etc/passwd`, so one that runs as a user or a group the lamp lacks is not
  added, and that is said. `lamp-update install` makes them.

A service of a part already installed here is added beside it, through the
same link. A part new to the lamp is installed afresh in the folder of its
binary's SHA-256, as a switch installs a build, and its `current` link is
made to name it. A part's template, socket or timer is added when its part
is here or added with it. Each is enabled only if `install` enables it
(`install::enabled`), and then as `systemctl enable` would: the links its
`[Install]` asks for (`WantedBy=` in `<target>.wants/`, `RequiredBy=` in
`<target>.requires/`) are written beside it before the reload. One already
there (an old enable) is left as it is, and stays on the way back too. An
`[Install]` with anything else (`Alias=`, `Also=`), or a template's, is
refused. As `install` leaves it, an added unit is enabled for the lamp's
next start and not started now. Some product units end at once when they
have nothing to do (the waking face without `LAMP_WAKING_FACE=on`, the
Wi-Fi restore, recovery). Started and watched, one of those would read as
stopped, and every build would be put back. The waking face would also
start mid-day.

All of it rides the switch's journal (`Switching::added`). A build put
back, or a switch cut short and recovered at the next start, stops each
added unit and removes its file, its links, an emptied `.wants` folder and
the `current` link it made. A new part's folder goes when nothing names it,
and then the folder of its builds (`/opt/lampOS/<part>/`) too, once empty.

Not shown: any of it on a real systemd or a 7071 lamp. A lamp imaged with
7071 lacks no product unit of today's `install` list, so the first build
that adds adds nothing there. A lamp installed before 22 Oct that runs a
managed updater lacks some (the waking face, the Wi-Fi restore and
recovery among them), and gets them enabled for its next start. A stop of
an added unit systemd never loaded (the switch cut before its reload) is
taken as done. The tests use the round's fake systemd.

### Settings and tmpfiles (`LAMP_UPDATE_APPLY`)


A build can carry the product's settings and tmpfiles (`apply.rs`,
#1680): a `settings` list in the signed manifest, each entry `path`,
`text` (inline), `sha256` and `mode` (`0644` or `0600`), or `path` and
`removed: true`. `lamp-update manifest --settings on` writes install's own
(`install::SETTINGS` and `lampos.conf`, `install::product_settings`), but
for the product skills' files under `/var/lib/lampOS/skills/`: those are
`lamp-product-skills`', placed by the round and judged by the skills
guard, so one file never has two writers at the switch. The one setting
there is the Skills Store's public key (#3692):
`deploy/product/settings/var/lib/lampOS/skills/store-key.pub`
(`install::STORE_KEY_SOURCE`), when the repository has it, is carried as
`/var/lib/lampOS/skills/store-key.pub` (`install::STORE_KEY_SETTING`),
0644, beside the skills and never in a skill's folder. With settings, `lamp-update manifest`
refuses a file that isn't one or two keys, one a line, as a lamp reads it
(`manifest::read_public_keys`). The repository holds the Store's
production key, fingerprint `8717ddcdf9ac22fb`, as the Store's keeper
posted it (`publish::tests::the_repositorys_store_key_is_the_stores_production_key`
pins it). To rotate, the file holds the new key and the old one, a line
each, for a build, then the new one alone; each round writes the build's
file over the one it wrote before
(`apply::tests::the_stores_key_is_written_0644_and_a_rotated_one_replaces_it`).
A build without the file carries no `removed` entry for it, so it leaves a
lamp's key as it is.
The path is one build 7071's fixed settings writer already takes, so a
lamp still on that `lamp-update-back` gets it in a round
(`publish::tests::the_stores_key_is_carried_only_when_the_repository_has_it`
pins the five places as 7071 has them). lamp-skills reads it before the
factory's `/etc/lampOS/skills/store-key.pub` ([skills.md](skills.md), "From
the Skills Store"), and the build's key wins (`package.rs`). So a test lamp
given the test environment's key with `--store-key` trusts only the
production key after any round on the channel, because every round writes the
file again. A test-environment Store check needs a lamp off the channel. Settings alone need
`min_updater` 3 (`manifest::SETTINGS_UPDATER`); `--settings on` also carries
the reset plan described below and therefore raises the floor to 5
(`manifest::RESET_PLAN_UPDATER`). The current `UPDATER` is 9; a build with the product's units raises the floor to 9 (`manifest::ORDER_UPDATER`), past their service units (6), a part's templates, sockets and timers (7) and the units it adds (8): see "Every lamp switches in its build's order".
A manifest without settings doesn't write the field. A path must be
`/etc/lampOS/records.json`, `/etc/tmpfiles.d/lampos.conf`, or a file in
plain parts under `/etc/lampOS/expression/`, `/etc/lampOS/controls/` or
`/var/lib/lampOS/skills/`; any other refuses the build. So a build never
writes a secret key, the voice's service, the zone, setup's answers,
`body/lamp.env`, `person/name`, the update key or the factory's Store key:
those are setup's, the person's or the lamp's. The only key a build may
write is the Skills Store's public key in the skills folder (above), which
is no secret. At most 128 settings and 160 KB of text.

With `LAMP_UPDATE_APPLY=on` in `lamp-update.service`'s environment:

- **Three-way.** For each setting: the build's before (its SHA-256 in
  `state.json`'s `settings`, as install or the last apply wrote it), the
  build's now (the manifest), and the lamp's now (the file). The lamp's now
  equal to the build's now is only recorded. Equal to the build's before,
  or never there and never recorded, it is written. Otherwise it is the
  lamp's own: kept and said (`kept PATH: this lamp's own`). One the lamp
  removed after it was recorded stays removed. A `removed` setting goes
  only where the file is still the build's before. Install records the
  `lampos.conf` it writes; on a lamp installed before that, a
  `lampos.conf` with no record that is one install ever shipped
  (`apply::TMPFILES_SHIPPED`) is the build's before, and any other is the
  lamp's own. The round looks at this at step 4. A build whose only change is its settings is switched too.
- **Root, small.** At the switch, after the journal and normally before any
  unit, link or model moves (the paired head-pins exception is below), the round stages `apply/request.json` and the
  build's envelope in its records and starts `lamp-update-apply.service`
  (`systemctl restart`, which waits for a one-shot). That runs
  `/usr/local/lib/lampos/lamp-update-back apply`, install's fixed copy,
  never `update/current`. It checks the envelope's signature again with
  `/etc/lampOS/update/key.pub` and the path rules above, reads the records
  from `state.json` and never writes it. It writes `apply/back.json` (each
  file's bytes and mode as they were, or absent, and its record) and syncs
  it before the first write, writes each file whole, then runs
  `systemd-tmpfiles --create /etc/tmpfiles.d/lampos.conf` if that file
  changed and is there, then writes `apply/done.json` (the records the state takes). The
  round checks the receipt's operation and build sequence against the values
  it actually asked for, before changing any setting record in `state.json`.
  It does not take that expected identity from a request the helper can
  rewrite. A mismatched or malformed receipt fails the apply and follows
  the existing recovery path. Additive fields and later versions keep their
  permanent read-through behavior; an unknown operation is still refused.
  This identity check does not authenticate the helper or validate the
  receipt's record paths or file contents. The round takes matching records
  into `state.json` and removes the request. Its unit:
  `ProtectSystem=strict`, writable only `/etc` (for the image's
  `/etc/asound.conf`, replaced by a rename beside it; what is written
  there is only what the path rules allow), `/var/lib/lampOS`, the records,
  `/run` (for the tmpfiles) and `/usr/local/libexec`, the update key
  read-only, no network.
- **Head pins follow their readers (#2948).** When the signed settings carry
  `/etc/lampOS/controls/controls-pins.json` and Controls actually reads that
  path, build-owned pins keep
  `legacy_head: true` with old Voice and Expression units. Only the complete
  new bundle (Voice `LAMP_HEAD_TOUCH=on` and `--controls-mute`, Expression
  `--head-touch`, and Body `LAMP_HEAD_TOUCH=on`) gets false. Controls,
  Expression and Voice's head reader
  must use the same effective bus root (explicit flag, then `LAMP_BUS_ROOT`,
  then the default). Voice's enabled privacy reader must share that root too.
  Body's sleep reader must also use that bus, with its empty or unset
  `LAMP_BUS_ROOT` selecting the default. Its known calibration files retain
  the literal `BODY_POSES` format described in [install](install.md);
  their existing contents are checked, so a file cannot silently disable or
  redirect Body's sleep reader. These per-lamp files remain excluded from
  manifest settings and are not rewritten by the check.
  Legacy head-off readers do not acquire a new Body requirement.
  The helper verifies the signed input first,
  derives only this compatibility bit, and records the hash of the actual
  written bytes. Local wiring is still kept by the three-way rule; an
  incompatible custom file, partial pair, or nonliteral/local override of
  the mode refuses the round rather than guessing. Harmless exit-note
  drop-ins do not block it. Controls' effective `--pins` path selects the
  file checked: build-local/custom active pins are kept byte-for-byte and
  checked against their readers, while an unused product pins file follows
  ordinary settings handling without imposing a head-reader restart.
  Settings-off preserves old pins; a unit change
  incompatible with those preserved pins is refused before writes.
  Local head-pair reads inspect the same descriptor they open without
  waiting for a FIFO writer. Regular files and regular-file symlinks keep
  their existing behavior; actual null-device masks remain empty without a
  device read, while other nonregular inputs refuse. Missing files and
  dangling links retain the previous absence behavior. This applies to
  units, drop-ins, known Body calibration and pins when read by this check,
  including the settings helper's projection and verification. It is not
  a deadline for regular-file storage or a new live-touch wait.
  The install tests keep the four refusal cases in owned children with a
  15-second setup bound and a 5-second post-ready bound, including FIFO
  reads without a writer. The five regular/symlink/missing/null-mask cases
  run full fresh installs with the ordinary suite bound and retain their
  file-preservation and head-pins migration assertions. Their old 30-second
  whole-install watchdog also counted durable binary, link and unit writes;
  it was not a preflight-liveness measurement. Product durability is unchanged.
  For a head-pins write, candidate units and links are placed after the
  journal but before apply, so the helper reads the units being installed.
  No service is restarted until the resulting pins match those units.
  An older fixed helper still writes the signed bytes unchanged: if that
  breaks the pair, the round puts everything back and names the mismatch.
  `lamp-update install` refreshes that fixed helper; a self-update alone
  does not. A cut after apply restores the pins and restarts their Controls
  reader even if its unit and binary never changed. This is update-time
  coordination, with no added work in the live touch path. Host regressions
  do not establish physical tap latency or flash durability.
- **Restarted with it.** A part whose unit names a written path, or its
  folder as a folder, is restarted and watched with the switch, as for a
  model: the controls for `controls.json`, the expression for
  `expression.json`. The skills reload by themselves, and the records'
  check reads its file at each run.
- **Put back.** A failed apply, a part not well after the switch, or a
  switch cut short (the journal's `settings`) asks `lamp-update-apply` to
  put back, after the units, links and models. It restores each file from
  `back.json` byte for byte, with its mode, or removes one the build added
  and the folders the apply made for it while they are empty, and its
  record, unless the lamp changed it again since (kept and said). A file
  already as it was (a put-back cut short and asked again) gets its record
  back too. A `back.json` of another build is never used. Then the
  tmpfiles are created again if they are there; nothing they made is
  removed. `back.json` stays as the way back by hand. With no `back.json`
  of this build the apply wrote nothing, so `lamp-update-apply` isn't
  asked at all: the build is put back and refused, and the journal
  cleared.
- **Not installed.** On a lamp installed before this, there is no
  `lamp-update-apply.service` or no `apply` in lamp-update-back: nothing is
  done, the round says `lamp-update-apply.service did nothing`, and the
  build is put back and refused, the journal cleared, since nothing was
  written. Running `lamp-update install` again
  installs both.

`back.json`, the request and `done.json` are read by a
lamp-update-back as old as the lamp's install, as `self.json` is: their
fields keep their meaning, a newer one may add fields, and each side
ignores what it doesn't know. So does a setting in the manifest.

The level-6 carrier doesn't refresh that fixed writer. A later round does,
from the lamp-update that runs (#3689 item 1, #4178): "The settings writer
refreshed", below. The new writer is proved by its own `back`, or the copy
before is put back, and no switch of lamp-update's own begins until it is
proved. [The #3689 settings-writer design](settings-writer-refresh.md) is
the other route, with an independent recovery guard. It wasn't built, and
its record foundation stays unwired.

Explicit off, an invalid value, or absence outside the managed product
carrier default leaves a build's settings unwritten. The round says which
(`build N's settings are not written (...): LAMP_UPDATE_APPLY=on is not
set`). The rest of the build is taken as today. Users and groups aren't
carried: a later step, when a build needs a new one.

### Setup's reset plan (#806)

The reset plan, `/etc/lampOS/setup/reset-plan.json`, is what the 5 s reset
wipes. It is the product's and never the person's: an old plan makes the
reset wipe less than the build's stores, so a person's own keys or display
name would survive a reset. Found on lamp-06c1 on 21 Oct: its plan was the
fixture of 19 Oct (`4de8ebe1c`, sha256 `5dff6f81…`), 10 stores short, kept
by install as "this lamp's own" and never carried by a round.

- **Carried.** `lamp-update manifest --settings on` also writes
  `reset_plan`, the fixture `crates/setup/fixtures/reset-plan-product.json`
  as one setting (`install::reset_plan_setting`), 0644, and raises
  `min_updater` to 5 (`manifest::RESET_PLAN_UPDATER`). `UPDATER` is 8. It
  is a field of its own, not in `settings`, for the image's reason: a
  lamp-update-back from before it refuses a setting path it doesn't know,
  and passes over a field it doesn't know. Its path must be the plan's,
  never removed, 0644, at most 16 KiB (`manifest::check_reset_plan`).
- **Always the build's.** With `LAMP_UPDATE_APPLY=on`, wherever the lamp
  has a plan, lamp-update-apply writes the build's over it, whatever it
  is (`apply::reset_plan_step`): a past build's, one placed by hand, one
  changed on the lamp. It is never kept as the lamp's own. The build's
  already there is only recorded. A lamp with no plan gets none: on a lamp
  set up by hand (lamp-4ace), with setup's marker, a plan would start
  `lamp-setup-reset.service` and arm the 5 s reset.
- **Backed up.** Before the first write, the old plan is copied whole into
  the records as `reset-plan.json.before-<build>` (`-<ms>` after it if one
  of another plan has that name), 0600, unless it is the plan recorded
  (a build wrote it, and `back.json` holds it). The journal line says
  whether it was a plan an earlier build shipped
  (`install::RESET_PLAN_SHIPPED`, every version of the fixture in git) or
  one no build shipped. `back.json` holds it as every setting, so a build
  put back puts the old plan back byte for byte.
- **Nothing restarted.** No part is restarted for it: the reset's watcher
  hands each reset to `lamp-setup reset-now --plan`, which reads the file
  again.
- **An older lamp-update-back** (installed before this) writes the rest of
  the build's settings and not the plan; the round goes on and says
  `/etc/lampOS/setup/reset-plan.json was not written: this lamp's
  lamp-update-back is older than it, so install again`.
- **Install** does the same at every run (`install::reset_plan`): with
  `--setup on` it writes the plan where there is none; with or without it,
  it writes the build's over any other, the old one kept in the records
  first, and records it. Without `--setup on`, a lamp with no plan gets
  none.

### The image's own files (`LAMP_UPDATE_IMAGE`, off)

The image lays down files of its own that `install` never placed
(`robots/lamp/rootfs/`, and `robots/lamp/overrides/<variant>/rootfs/`
over it): a fix to one reached only newly imaged lamps (#1369). A build can
carry them (`image.rs`, #1680 step 6): an `image` list in the signed
manifest, each entry `variant`, `path`, `text`, `sha256` and `shipped`.
Not in `settings`: install's fixed `lamp-update-back` refuses a setting
path it doesn't know, and ignores a field it doesn't know.

- **Which.** Only `manifest::IMAGE_FILES` (checked when the manifest is
  read, and again by lamp-update-apply before its first write, as on
  put-back; a build that names any other path, or one path as both a
  setting and an image file, writes nothing): `/etc/asound.conf`,
  ramlog's `/etc/default/orangepi-ramlog` (#2602; the base image's own
  file is listed as shipped too, so it is replaced, and a lamp's own
  `SIZE` is kept), `lamp-cmedia-speaker.service`, `respeaker-lite-softvol.service`,
  logrotate's `zz-lamp-ramlog.conf` and its `lamp-ramlog-postrotate`,
  ramlog's `zz-lamp-journal-keep.conf` and its `lamp-journal-keep` (#2602:
  the last 2 boots' journal on the eMMC; a daemon-reload, so the next
  shutdown keeps, and the boot after links), journald's `zz-lamp-journal.conf`, and the udev rules `90-respeaker-lite`,
  `91-pulseaudio-hal-ignore` and `99-lamp-device`. Never V1's HAL
  (`/opt/hal/.env`, `led-boot`/`led-shutdown`, `hal.service`'s drop-in),
  never the radio scripts (`device-ap-mode`/`device-sta-mode` are written
  by `build-orangepi.sh`, not files in the tree), and never a drop-in a
  lamp or a person made, such as lamp-4ace's
  `lamp-cmedia-speaker.service.d/30-local-gain.conf`.
- **Variants.** `lamp-update manifest --image robots/lamp` writes each
  variant merged as `apply-overrides.py` merges it: the variant's own file
  where it has one, else the standard one. On the lamp, the variant is
  `/etc/autonomous/hardware-profile` as the image wrote it (none, empty or
  `standard`: the standard lamp); one that isn't a plain name carries
  nothing. `min_updater` is then at least 4 (`manifest::IMAGE_UPDATER`).
- **Shipped.** `install` never recorded these files, so the build's before
  is also every SHA-256 the image has shipped at that path for that
  variant: `deploy/image/shipped.json`, from the history of `robots/lamp`.
  Publishing refuses a file whose SHA-256 it doesn't list, and a test
  (`image::tests`) fails the same way: change a file in `robots/lamp`, add
  its new SHA-256, never remove one. A lamp's file that is none of them
  and not what an update recorded is the lamp's own: kept and said.
- **Written** by `lamp-update-apply` with the settings, by the same rule,
  when the round's request says `image` (only with the switch on). Each
  unit (not a drop-in) is first staged in `/run/lampos-update-apply/` and
  checked with `systemd-analyze verify`; one systemd can't load fails the
  apply with nothing written. After the writes: `systemctl daemon-reload`
  for a unit or a unit's drop-in, `udevadm control --reload` for a rule
  (no trigger: a rule is used at the next plug or boot), and
  `lamp-cmedia-speaker.service` started once when it changed (a failure
  there is said: a lamp without the C-Media card). A put-back restores each
  file byte for byte and runs the same reloads.
- **journald is never restarted** or signalled by an update (the first
  builder on #1680, 19:48: restarting it broke every part's log pipe on
  15 Oct). Its drop-in is read at the next boot, and that is said. Every
  command lamp-update-apply runs passes `image::guard` first, which
  refuses any that names journald.
- **The voice, at a quiet moment.** `/etc/asound.conf` changed restarts
  the voice (`image::restarts`), which ALSA's configuration reaches only
  when it opens its devices. A round writes it only after the switch's
  quiet gate (`quiet.rs`: nobody heard or answered and the voice resting
  for 45 s), the gate every switch of the voice passes. With no quiet
  moment within the initial `--quiet-wait-s`, nothing is written or
  restarted, the round ends `not-quiet`, and the next round tries again.
  A lamp's own `asound.conf`, kept, restarts nothing.
- **Older lamp-update-back.** A lamp installed before this has a
  lamp-update-back that ignores `image`: nothing of it is written, the
  round says so (`this lamp's lamp-update-back is older than them, so
  install again`), and the rest of the build goes on. A part restarted
  only for a file not written (the voice for `asound.conf`) isn't
  restarted.
- **check-units.** The image's units (`lamp-cmedia-speaker.service`,
  `respeaker-lite-softvol.service`) are "the image's", not "not in the
  product". With `--image robots/lamp [--variant NAME]` each is compared
  with that variant's, and a difference fails the check.

Off, or absent: none of the image's files is written, neither they nor
`/etc/autonomous/hardware-profile` is read, and the round says so
(`build N's image files are not written: LAMP_UPDATE_IMAGE=on is not
set`). On, a profile or image file that can't be read is said, and the
rest of the build goes on without the image's files.

## What was measured, and where

On the development Mac (Intel, macOS 26.6, Rust 1.96.0), not on a lamp:

- `cargo test -p lamp-update -- --test-threads=1`: 29 unit tests and 15
  rounds on a lamp made of files (`tests/round.rs`), passed; one test
  against the network ignored.
- The rounds, with a channel, systemd, bus and clock of the test's own:
  a new build installed beside and switched (only the voice restarted,
  after 45 s of quiet and a 60 s watch); a unit the build carries carried,
  with the build written in; a bad signature and a bad checksum refused
  with nothing installed and systemd not asked; a part that dies put back
  (the units and `/opt/lampOS` byte for byte as before, the new folders
  removed) and its build not tried again; a part in error in the lamp's
  health put back after its minute; no network: nothing changes and it
  says so; the voice not switched while someone speaks every 30 s, then
  switched in the next quiet round; a disk too full refused before anything
  is written; without `--apply` nothing written, records included; an older
  build refused; a switch cut short put back at the next start; a new
  worker beside an unchanged voice in a folder named by its list; a folder
  made two builds ago removed and the one installed by hand kept; a part
  whose folder is gone installed afresh; a voice that is well put back only
  5 s after the last word; a part put back saying its restarts and last
  line in the outcome and never in `state.json` or `log.jsonl`; the old
  build not well either, the first unit not well's line after `after`; no
  line asked of a good switch, none said when there is none, and none asked
  when systemd itself cannot be asked; the journal line picked from
  `journalctl -o json` (the unit's own last, others' and the old main
  process's skipped, cleaned and cut); a command bounded in time and read
  whole however much it prints; the units' state query (`systemctl show`)
  bounded too, within 5 s (`JOURNAL_WITHIN`), group-safe and capped as the
  other queries (1 MiB stdout; stderr drained, 64 KiB kept): one that
  stalls, or ends while a child it left holds its pipes, is an error and
  that child is killed with its group; a failed query says its trimmed
  stderr. In the watch a failed query is asked again until the watch's
  deadline, putting back only if systemd still cannot be asked at the end.
  Install's own commands go through the same helper: its `systemctl`
  steps and Harness's service steps within 120 s (`RESTART_WITHIN`),
  Harness's installer, which fetches, within 15 min (`INSTALLER_WITHIN`),
  `systemd-tmpfiles` and `date +%z` within 5 s, `harness version` within
  30 s (`VERSION_WITHIN`; past it, the Harness there is kept as not
  known, never installed over); one that stalls or leaves a child holding its pipes is an
  error naming it and the bound, a failed one says its trimmed stderr.
- With product parts (task #811, `cargo test -p lamp-update --test round`,
  19 rounds): a lamp with both kinds switched in one round, the voice by its
  unit and the eyes by their link, the eyes' unit byte for byte the same; a
  product part that dies put back by its link, the lamp byte for byte as
  before; a link switch cut short put back at the next start; a product part
  keeping its build and the one before, the one two builds ago removed and
  the one installed by hand kept. With the link never moved on purpose,
  those four fail and the other fifteen pass.
- After install (#1680, `a_round_after_install_keeps_lamp_updates_own_folder`):
  a blank lamp given build 1 by `lamp-update install`, then a round that
  switches to build 2, keeps `update/current` naming a folder that is there.
  Without the fix the round removes that folder and the test fails.
- Settings and tmpfiles (#1680, `apply.rs` and `tests/round.rs`,
  `LAMP_UPDATE_APPLY` on unless said): a setting still the build's is
  written at the switch, before any part restarts, and only the part whose
  unit reads it is restarted; one the lamp changed or removed is kept; the
  tmpfiles are created once when `lampos.conf` changed; a part not well,
  and a switch cut short after the apply, put each file back byte for byte
  with its mode and its record; one changed again since is kept; another
  build's `back.json` is never used; an envelope signed with another key,
  or of another build, writes nothing; a lamp without lamp-update-apply puts
  the build back with nothing written, its journal cleared and the build
  refused; with the switch off nothing is written and that is said. With
  the put-back left out, the two put-back tests of `tests/round.rs` fail.
  From the review of #1913: a product lamp whose `lampos.conf` install wrote
  without a record takes the build's, and an unrecorded one of its own is
  kept; install records it; a put-back asked again after being cut short
  puts every record back; the tmpfiles are created only when they are
  there; a new skill's folder goes with it and one the person added to
  stays; a link left beside a temp file is replaced, never written
  through. Each fails with its fix taken out.
- Apply receipt identity (#3689, `apply::merge_for` and the parent-round
  control), qualified on 27 October on Apple Silicon
  (`aarch64-apple-darwin`, macOS 26.6.2, Rust 1.96.0), distinct from the
  historical Intel host named above. On base `e362659ce`, fmt and
  strict Update all-target Clippy passed; the full library passed
  **152 / 0 / 2 ignored**, and 65 selected settings, image, reset-plan,
  head-touch and product-carrier round tests passed **65 / 0**, with 132
  filtered out. Both wrong-identity library controls failed on the original
  production, as did the rewritten-request round control, which incorrectly
  returned `Switched(2)`. That old round stopped at its first operation case;
  the separate library test proved the sequence failure. All six new test
  functions pass with the fix, including both parent-round variants and
  later-version/additive-field acceptance. These local results do not cover
  the whole round suite, Linux/ARM64, a device, helper authentication or the
  frozen writer guard.
  A later gate on the same Apple Silicon host, on base `1fb93206e`, adds
  two controls that wrap the actual
  applier: a wrong Apply receipt still restores overwritten, removed and new
  settings, modes and records through real Back; a wrong Back receipt keeps
  the incomplete journal until a later idempotent Back is acknowledged,
  without applying the refused build again. Full library **155 / 0 / 2
  ignored** and the prior 65 plus those two round tests **67 / 0**, with 134
  filtered out, pass with fmt and strict Update all-target Clippy. These are
  filesystem/FakeSystemd fixtures, not a root one-shot, reboot or lamp trial;
  their results are separate from the earlier `e362659ce` qualification.
- Models in the manifest (#1680, `tests/round.rs`, `LAMP_UPDATE_MODELS`
  on unless said): a changed eyes model is fetched once, checked, and its
  path linked into the store with the eyes switched, install's file kept
  in the store; a changed voice model goes into a new voice folder though
  the binary is the same; unchanged models aren't fetched and install's
  file stays a file; a model whose bytes don't match refuses the build
  with the lamp byte for byte the same and nothing restarted; a part that
  dies puts the model back as install's file; a switch cut short puts it
  back at the next start; only a model changing restarts the eyes, and
  after three builds the store keeps the last two builds' models; 50 MB of
  models on a disk with 10 MB to spare is refused before any model URL is
  read, the number in `log.jsonl`; with the switch off a build with another
  model is refused and one with the lamp's models is taken as today. From
  the review of #1879: a model path that can't be put back leaves the
  others put back and the journal kept, a store entry still linked kept
  (`a_model_that_cannot_be_put_back_leaves_the_others_put_back_and_the_journal_kept`);
  a model-only switch cut short restarts the eyes
  (`a_model_only_switch_cut_short_restarts_the_part_that_reads_it`); a
  `state.json` set aside keeps the linked entries
  (`prune_keeps_what_a_model_path_links_to_when_the_state_was_set_aside`);
  a model fetch cut by the network is unreachable
  (`a_model_fetch_cut_by_the_network_is_unreachable_not_refused`); off and
  on both keep a lamp's own model
  (`with_the_switch_off_a_lamps_own_model_is_kept_as_with_it_on`); and with
  `self_switch` on and models off, the new updater confirms, the build waits
  with nothing written, and no round climbs again
  (`with_models_off_a_new_updater_confirms_and_the_build_waits_for_models`).
- lamp-update switching itself (#1680, `tests/round.rs`, on a lamp given
  build 1 by `install`, with `self_switch` on unless said): a new updater
  that confirms keeps its switch, and a later failure or the deadline leaves
  it; a new binary that exits at once (a real `#!/bin/sh` `exit 1`, run) is
  put back by `OnFailure=`'s run of back and refused; one that runs but isn't
  confirmed is waited for at 1 min and put back after 10; one that refuses
  every manifest (another key) or cannot reach the channel never confirms,
  and is put back by the deadline; while `self.json` waits a round is
  refused with the lamp byte for byte the same, and with the lock held
  elsewhere the confirmation, back and a round all do nothing; with the
  switch off the round switches the parts and lamp-update stays, and back
  and the confirmation write nothing; install writes back at its fixed path,
  0755, outside `ReadWritePaths`, and two self-switching rounds leave it as
  it was; back reads a `self.json` of version 5 with fields it doesn't know.
  Prune keeps the running binary's folder, both in a round by hand with no
  lamp-update unit and in one made by the old binary after its own switch.
  Without the fix both tests fail.
- Independent self-update recovery (#2700, `tests/round/self_recovery.rs`):
  a missing, non-executable, directory or symlink fallback defers only the
  updater; an unavailable recovery query also defers it, then a later ready
  round takes the same candidate. The `min_updater` path leaves all parts
  unchanged while recovery is unavailable. Off, unchanged and refused
  candidates do not query recovery. `world.rs` tests distinguish a running,
  persistently enabled timer from inactive or runtime-only enablement.
- Self-switch cut fixtures (#2810, `tests/round/self_cuts.rs`): six tests
  cover the five prefixes described above and the off switch. Recovery
  completes through confirmation or put-back and a subsequent round;
  interrupted put-back is retried without losing the runnable current
  updater or changing the fixed fallback. A separately reconstructed
  post-refusal prefix clears stale pending state without a duplicate
  refusal. These are logical-prefix checks, not a lamp-storage power-cut
  result.
- The real HTTPS path, once, by hand (the ignored test
  `a_release_file_from_github`): 646 bytes of this repository's
  `models/ambernet` release, through GitHub's redirect to
  `release-assets.githubusercontent.com`, and a missing asset read as
  missing. 0.88 s.
- `lamp-update manifest` on run 144's real artifact (`lamp-voice` at
  `bc3fccb54`) and this repository's `deploy/`: 31 binaries (146.95 MB), 12
  units (nine of lamp-4ace's, three of lamp-52e6's; `lamp-voice-pilot@` and
  the templates are not parts installed by build). `check` against a lamp
  made of lamp-4ace's units, with a key made for that check and thrown
  away, printed the plan for nine units and wrote nothing.

### Lamp trials: local channels signed with test keys

- **19 October 2026, 10:54–10:59 (+07), lamp-4ace:** the first builder's
  [parts update proof on #737](https://github.com/autonomous-ai/lampOS/issues/737#issuecomment-6259361314)
  ran `lamp-update once --apply on`. Build 3005 installed 15 parts beside
  the old ones, switched at a quiet moment and passed the health watch.
  Build 3006 deliberately failed the voice and was put back automatically.
  The body was excluded from the channel and kept its build.
- **20 October 2026, 05:54–06:11 (+07), lamp-4ace:**
  [the self-update proof on #1788](https://github.com/autonomous-ai/lampOS/pull/1788#issuecomment-6279387052)
  ran `run-1788.sh` v3b on merged commit `647f1ee63`. Cases A, B and C and
  restoration passed: a good updater confirmed; one that exited was put
  back 107 s after the restart; one that never confirmed was put back about
  693 s after its switch (the 600 s deadline plus a timer pass). The channel
  used the lamp's installed sequence, so only lamp-update switched: no
  product part or skill changed. Restoration checked the records, units,
  fallback binary and key against their originals, removed the trial's
  unit and drop-in, and left lamp-update inactive as before.

Both trials used local channels and test keys. They do not show a release
fetched over the internet with the real signing key, an update including
the body, or a combined update of models, settings and image files.

## On lamp-4ace: from one build of `lamp-voice` to the next

For the first builder, who owns the lamp. Before CI publishes, the channel
is a folder on the lamp, made on the office Mac from two runs' artifacts and
signed with a key the first builder makes there for this test (never
committed, never the owner's real key). `L` is the runbook's SSH alias.

1. **Room first.**

   ```sh
   L 'df -h / /opt/lampOS /var/tmp; sudo du -sh /opt/lampOS/* | sort -h | tail'
   ```

   A round needs the new folders whole (the voice's about 94 MB with its
   worker, model and library; every changed part together under about
   150 MB) and 256 MB to spare; the channel folder copied to `/var/tmp` is
   another 147 MB. If the disk is near its 10% warning, old folders in
   `/opt/lampOS/releases/` the units do not name are the first builder's to
   remove by hand: lamp-update never removes a folder it did not make.

2. **What the lamp runs, written down** (joining.md):

   ```sh
   L 'systemctl list-units "lamp*"; sha256sum /etc/systemd/system/lamp-*.service; ls /opt/lampOS/*/'
   ```

3. **A key, and the channel folder, on the Mac** (from `lampOS/`, a
   checkout of this branch; `B` is the run of the `lamp-voice` head to go
   to, built after the one installed):

   ```sh
   umask 077; openssl genpkey -algorithm ed25519 -outform DER | base64 > ~/lampos-update-test.key
   cargo run -q -p lamp-update -- public-key --key-file ~/lampos-update-test.key > ~/lampos-update-test.pub
   gh run download B -R autonomous-ai/lampOS -n lampos-arm64-<sha of B> -D ~/lampos-channel
   cargo run -q -p lamp-update -- manifest --binaries ~/lampos-channel --deploy deploy --skills skills \
     --channel lamp-voice --sequence <run number of B> --commit <sha of B> \
     --files-url file:///var/tmp/lampos-channel/
   cargo run -q -p lamp-update -- sign --key-file ~/lampos-update-test.key \
     --manifest ~/lampos-channel/manifest.json --out ~/lampos-channel/lampos-manifest.json
   ```

4. **lamp-update and the key on the lamp** (the binary from B's artifact,
   or from this branch's ARM64 run):

   ```sh
   scp -r ~/lampos-channel LAMP:/var/tmp/      # with the runbook's key options
   H=$(shasum -a 256 ~/lampos-channel/lamp-update | cut -c1-64)
   L "sudo install -D -m 0755 /var/tmp/lampos-channel/lamp-update /opt/lampOS/update/$H/lamp-update"
   L 'sudo mkdir -p /etc/lampOS/update && sudo tee /etc/lampOS/update/key.pub >/dev/null' < ~/lampos-update-test.pub
   L 'sudo sync'
   ```

5. **The dry run** (writes nothing):

   ```sh
   L "sudo /opt/lampOS/update/$H/lamp-update check --channel-url file:///var/tmp/lampos-channel/"
   ```

   It names each unit that would change, old folder to new, and every line
   of its unit file that would change. **Read those lines.** They come from
   `deploy/lamp-4ace/` in build B; where the repository's copy is not what
   the lamp runs, carrying it would undo what was set on the lamp (a switch
   turned on there and not in the repository). Make the two agree first.

6. **The minute with the switch off**: the unit from the template, with
   `@UNIT_ID@` `lamp-4ace`, `@UPDATE_SHA256@` `$H`, `@UPDATE_CHANNEL_URL@`
   `file:///var/tmp/lampos-channel/` and `@UPDATE_APPLY@` `off`; then
   `sudo systemctl start lamp-update`, a minute, and
   `journalctl -u lamp-update -o cat` says what it would do. Step 2's
   commands again: the same.

7. **One round, attended:**

   ```sh
   L "sudo /opt/lampOS/update/$H/lamp-update once --channel-url file:///var/tmp/lampos-channel/ --apply on; echo status \$?"
   L 'sudo tail -n 5 /var/lib/lampos-update/log.jsonl; lamp-health; sudo sync'
   ```

   It waits for 45 s of quiet (up to an hour), switches, watches a minute,
   and prints `switched to build N; every part is well`, or what it put
   back and why. Then speak to the lamp.

8. **A rollback on purpose** (optional, attended, a few seconds without a
   voice): a copy of the channel folder with `lamp-bus` copied over
   `lamp-voice` and `SHA256SUMS` removed (it no longer matches), the
   manifest made and signed again with a greater `--sequence`. Given the
   voice's arguments `lamp-bus` prints its usage and exits with 2, systemd
   starts it again, and the round must put the voice back within seconds.

9. **On:** `@UPDATE_APPLY@` `on`, and, once the CI job publishes,
   `@UPDATE_CHANNEL_URL@`
   `https://github.com/autonomous-ai/lampOS/releases/download/lampos-lamp-voice/`
   with the owner's public key in `key.pub`.

## What parts say to each other, one build apart

The parts of one build are signed together, but they are not switched
together. For a while, short or long, a part of the new build talks to a
part of the one before. The rule (the first builder on #3126, 23 Oct):
**any change to what one part says to another works with the previous
build on the other side, both ways.** That covers the bus, the sockets,
memory's protocol and the tool calls between crates.

- **An old reader ignores new fields.** A field or a value the next build
  adds is skipped by the build before, and it goes on as it did.
- **A new reader refused by an old server acts safely and says so.** It
  does not guess and does not act on half an answer. #3127 is the example:
  an older lamp-memory refuses `observations`, so the voice removes nothing
  and tells the person it couldn't forget.

Why the window can't be zero:

- **Signed together.** lamp-voice and lamp-memory are both `core`
  (`deploy/package-binaries.json:4-5`), and one envelope covers the whole
  manifest (`.github/scripts/lampos-publish.py:196-231`). Publishing is
  still off (`.github/workflows/lampos-arm64.yml:279`).
- **Not switched as one step.** Each `current` link is renamed in turn,
  and each active changed unit is restarted separately, in the candidate
  unit graph's startup order (step 8 above). Voice's declared prerequisites
  restart before it; old Voice can talk to new Memory while Voice waits
  for its own quiet boundary, up to `--quiet-wait-s` (3600 s by default).
  Dependents that declare themselves after Voice still restart after it.
- **Failures leave a mixed build.** Put-back uses the saved old startup
  graph and waits for a gap between turns at Voice's own restart boundary.
  New Voice can therefore talk to restored old servers during that wait.
  A failed restart, a power cut between renames, or a part that was not
  running (switched, not started) can also leave two versions in use.
- **Test lamps skew on purpose.** lamp-4ace's units pin each part's build,
  and `deploy.sh` swaps the voice alone (`deploy/lamp-4ace/RUNBOOK.md:51`).
  lamp-06c1 runs a hand-swapped voice on an older build
  (`deploy/lamp-06c1/LAMP.md:2`).

The app's Check for update (#4153) is one such change. An older
lamp-update never subscribes to `update/check`, so it ignores it and its
rounds go on every `--every-s`. A newer lamp-cloud beside it sees that
nobody reads the topic (no subscriber took the message) and refuses with
`lamp-update is not listening for a check`, so the app is never told a round
started when none will. An older lamp-cloud never says a check, and a newer
lamp-update waits as before. Even when one is taken, the app follows the
round on `update.status`, not on the check's reply.

Dependency order changes which side moves first; it does not remove the
compatibility rule. The updater reads the managed base service files:
drop-ins, aliases, symlinked units and dependency paths through external,
socket or target units are not resolved. Restart order is not a readiness
handshake or an atomic protocol cutover.

## Every part's saved data, one version ahead

An update can be put back, by a round, by `install` or by hand, after a
newer build has already written its data. On 18 Oct a PR check wrote
`told` into lamp-tasks' store, and the installed build refused it at
start (#1505). The rule since (#1505, the first builder at 14:02, 18 Oct):
**each part's store reads one release forward, and what can't be read is
set aside, never dropped.** An extra field or a new value written by the
next build is read; the unknowns are kept or dropped as the store says;
the part starts. A store it truly can't read (a real schema bump) is set
aside under a new name with a journal line, and the part starts without
it.

**And one version back** (the build row "Updates cover the whole lamp, V3 to
V5", 21 Oct): each store as its day's build wrote it is kept as a
fixture, and every later build's tests read it, as a restart in the same
boot and after a reboot. A shape change adds the new day's file and keeps
the old ones. Kept so far, each in its crate's `tests/fixtures/stores/`:
lamp-tasks (the state and the database around it, as SQL); lamp-schedule
(its reminders and timers, the same two, and the voice's delivery ledger,
which Schedule's code keeps in a store of its own); memory across days (its database,
as SQL); the eyes (their known people); lamp-update (its own state and
journal); the voice (its thread file, which outlives a restart in
`/run/lampos-voice`); and lamp-cloud's reports (setup's answer, the person's
own choice, the queue and the exit note a crash came from, and the login a
ping rotated, `rotated.json`); the controls (`controls.json`, the product's
pins file, read with `--touch`, and the pins file prepare gives a lamp moved
from V1, read without it); the skills turned off (`off`, read by lamp-skills at
a start and by lamp-cloud for the app); and lamp-records (a voice session
folder, taken as a session with nothing refused). The other parts follow, by
harm.

**Published Controls single-implementation replay check** (#2809): the
Controls `release_pair` target selects pinned 6408/6498
`calibrate-touch --replay` writers and opposite `replay --touch` readers,
both directions and back to the original reader. Both releases contain the
same Controls bytes, so this is not a distinct-implementation compatibility
check. Native ARM64 run 40075828326 passed both cases on `27b3695f0`, and
independent reads passed; later documentation-only heads retain that result
as historical evidence alongside their own current CI.
This qualifies only synthetic calibration/pins files, not physical touch,
real acquisition latency or whole-lamp update/rollback. The ordinary current
CLI test checks the fixture only; the two ignored cases are the actual pair.
[Scope and execution record](verification/controls-release-pair-20261024/README.md).

**Published routine-state readers** (#2809): the manual Skills `release_pair`
target covers daily claims and on/off preferences with the actual pinned 6408
and 6498 ARM64 executables, both directions and back to the original reader.
Private synthetic peers supply the triggers and helper requests. The
[scope and execution record](verification/routine-state-release-pair-20261024/README.md)
separates these four cases from ordinary-suite skips and from the still-open
rested-night, full-loader, installation and physical power-cut boundaries.

**Rested-night continuation** (#2809): two additional Skills cases keep one
unchanged synthetic skill through 6408 → 6498 → 6408 and the reverse. They
require the actual released writer's matched rest admission and persisted
night, the replacement reader's single morning stretch at a private fake
Body socket, and the original reader's suppression of the spent morning.
Fresh owner probes distinguish persisted history from resumed active rest.
Fixed local offsets map consecutive dates; no physical night is elapsed and
no timezone migration is qualified. The separate
[rested-state plan and gate record](verification/rested-state-release-pair-20261024/README.md)
records dedicated native six-case passes on its separate earlier heads;
the combined eight-case composition requires its own execution. Ordinary
suite skips establish no released-process result.

**Published local-package readers** (#2809): two additional ignored Skills
`release_pair` cases use actual 6408/6498 `pack`, `install` and `list` commands
in both directions. They check synthetic versioned installable folders,
disabled inventory/tool reservation, selected strict refusals and preservation
of the existing files after a refused replacement. The
[package scope and execution record](verification/package-state-release-pair-20261024/README.md)
records the earlier separate six-case pass and requires new evidence for
the combined eight-case composition. Inspecting the installer's
`.package.json` output does not execute Cloud's provenance reader; this slice
also excludes Store trust, the full product catalog, updater restoration and
physical output.

What follows is the audit for #1680 at `647f1ee63`, read from the code
(paths relative to `lampOS/`). The tasks, schedule, eyes and cloud rows
were also checked with temporary tests, since removed. "Unread" marks what
wasn't traced to the end.

**Risk:** **high**, a record one version ahead stops the part from
starting (it exits, and systemd gives up after its restarts) or wedges it;
**medium**, it silently drops or resets data, or a feature stays dead with
the data kept; **low**, it's set aside or skipped with a journal line, or
the file is never read back.

"Field" and "variant" say whether an unknown field
(`#[serde(deny_unknown_fields)]`) or an unknown enum value (no
`#[serde(other)]` or fallback) is refused.

### High

| Part | Store, path | Format | Version | Field | Variant | At start, on a record it can't read |
|---|---|---|---|---|---|---|
| controls | `/etc/lampOS/controls/controls.json`, `controls-pins.json` (update's settings write them, `install.rs:339`) | JSON | none | **read**, named once by path (`controls/src/forward.rs`, #1680). A key-like field, or one about privacy (`privacy`, `slide`, `mute`) or the reset (`reset`), is refused by name, whatever its value; the `privacy` section stays strict. The legacy reader's own pins file, kept beside its binary, stays strict at its top | refused: a bad known value (a new `pull`, a strip `mode`) | starts, with one journal line naming the fields. A file that doesn't parse, a bad known value, or an unknown privacy, reset or key field still refuses to start, told by serde's category, line and column only. Apply also restores the files with the build on going back (`update/src/apply.rs:14-20`); a person's own edit is kept and not restored |
| body | poses `lamp-body-measure` writes once, `/etc/lampOS/body/lamp.env` (`BODY_POSES`, passed as arguments) | env | none | **refused, on purpose: a safety rule, not an oversight.** An option from `BODY_POSES` this build does not read stops the body, and the journal names it and says the body stays still until the lamp is updated (`lamp-body.rs` `parse_with`) | an unknown on/off value refused | **refuses to start** (`StartLimitBurst=3`). The one store where "never stop the part" gives way to "never move unsure": a pose option a newer measure adds may be a bound the arm needs, as `--pitch-window` once was, and a rolled-back body that dropped it would move outside it (the first builder on #2071, 20 Oct 06:28). Tests: `lamp-body.rs` `a_newer_poses_files_option_still_stops_the_body`, `every_option_the_usage_names_is_known` |

### Medium

| Part | Store, path | Format | Version | Field | Variant | At start, on a record it can't read |
|---|---|---|---|---|---|---|
| tasks, schedule | the newer build's optional fields | JSON | — | read and dropped | — | tasks rewrites its state as it opens, so they go at once; schedule at its next save (`schedule/src/engine.rs:409`). As designed in #1505, but the data is lost on a roll forward |
| schedule (run by the voice) | reminder delivery ledger, `/var/lib/lampOS/voice/schedule-delivery/schedule.sqlite3` | SQLite + JSON | `user_version` 1; `schema` 1, higher **set aside** (`schedule/src/delivery.rs:562-605`) | top level read (`delivery.rs:397`); nested types refused (`delivery.rs:45-185`) | refused (`delivery.rs:128,164`) | a nested field or a new value without a bump, or a higher `user_version`: the reminder child exits, the voice logs "Schedule consumer failed" and runs on (`voice/src/reminders.rs:319-326`). Reminders stay unsaid on every restart; the data is kept |
| memory | old journal and kept files, imported once, `/var/lib/lampos-memory/<person>/journal-*.jsonl`, `kept.jsonl` | JSONL | none | read, dropped (`Exchange`, `Kept` take no `deny_unknown_fields`) | — | a line that doesn't read is only counted, then the file is deleted (`days.rs:228-295`, `308-324`): **dropped**. Not changed by #1680: an older build's files, not a newer one's, and a line that doesn't read is a cut or broken one; keeping it would keep a journal past `keep_days` and past a forget |
| memory | a tidy job's input and result (`jobs` table) | JSON in TEXT | none | refused (`tidy.rs:52-93`; `#[should_panic]` test `tidy.rs:395`) | refused (`Category`, `tidy.rs:44`) | not at start: that tidy fails "stored tidy input is invalid" (`days.rs:976`, `1127`, `1147`). The job clears a day after it was prepared, as every job does: `maintain` runs at start and before every request (`days.rs:344-347`, `days_protocol.rs:245`); until then it counts toward the 8 pending (`MAX_PENDING`) |
| eyes | forgets waiting, `/var/lib/lampos-eyes/set-aside/forgets-pending` | JSON | none | read | n/a | set aside as `forgets-pending.unreadable-<unix s>` with a line, never counted as records, and forgetting goes on; merged back at a start once it can be read (`who.rs` `pending`, #1680) |
| skills | skills the person turned off, `/var/lib/lampos-skills-state/off` (cloud reads it too, `cloud/src/skills.rs:81`) | text lines | none | n/a | n/a | a line that isn't a plain name is skipped with no line (`skills/src/lib.rs:3158-3170`) and gone at the next rewrite (`lib.rs:3178-3190`) |
| cloud | login a ping rotated, `/var/lib/lampos-cloud/rotated.json` | JSON | none | read (`cloud/src/account.rs:14`) | n/a | unreadable falls back to setup's account silently (`main.rs:124-128`); the next rotation overwrites it whole |
| records | voice sessions, `/var/lib/lampOS/voice/sessions` | folders | n/a | an extra file in a session is refused (`records/src/store.rs:22-28`, `394-396`) | n/a | that session is kept and reported and the run exits 1 (`run.rs:253-261`); a newer voice's sessions are never pruned |
| eyes | faces from photos, `<people>.from-photos` (only with `--photo-dir`, not in the product unit) | JSON | `version` 1; any other **deletes the file** (`eyes/src/photo.rs:195-209`) | read | n/a | dropped with a line. Off in the product |
| voice | person's light, `/var/lib/lampOS/light/person.json` (`LAMP_LIGHT_KEEP`, not set in the product) | JSON | none | read (`msgs/src/lib.rs:597`) | an unknown colour filtered (`ring_light.rs:170-175`) | dropped silently, overwritten at the next change. Off |
| voice | made skills index, `/var/lib/lampOS/voice/made-skills/made.json` (`LAMP_MAKE_SKILL`, on in the product since fourteen-P15) | JSON | none | `Value` | n/a | unreadable reads as `[]` (`made_skill.rs:510-515`) and is overwritten (`547-575`). On |
| tasks | mark of machine-written targets, `targets.json.from-machines` (only `--targets-from-machines`, off) | text | none | n/a | n/a | may overwrite a newer build's targets file whose hash is marked (`runtime.rs:1186-1220`). Off |

### Low

| Part | Store, path | Format | Version | Field | Variant | At start, on a record it can't read |
|---|---|---|---|---|---|---|
| memory | memory across days, `/var/lib/lampos-memory/memory-days.sqlite3` | SQLite | `user_version` 1; higher is **set aside** whole as `memory-days.sqlite3.newer-db<version>-<s>` (its sidecars too, if any outlived the close), never changed, with a journal line naming only the file, and the part starts with an empty store (`memory/src/days.rs:173-180`, `1266-1289`); health warns with `set_aside` on every start while the file is there (`days.rs:1331-1352`, `application.rs:199`, `health.rs`; #1680). Read before any setting is applied, so `journal_mode` never rewrites it. No code reads it back yet: a roll forward opens `memory-days.sqlite3`; whichever build brings `user_version` 2 reads `newer-db2-*` back | columns read by name; `CREATE … IF NOT EXISTS` | n/a | the part starts and serves. Side tables added since keep `user_version` 1 (`follow_ups`, `event_days`, `follow_up_offers`, and #2849's `speakers`, who said a room line, `days.sql`): a store made before one gets it on open, an older build passes it over, and that build's forgets and expiry still take a row with its entry by the foreign key (`tests/it/days_speakers.rs`). After a rollback, that build's `adopt` (the room's early words following a known face) keeps a `speakers` row on a line that is then someone's own: harmless, since `forget_said` reads only the room's lines and the row goes with its entry. The kept file's records are not read or searched here. Every forget (`forget`, `forget_last`, `forget_all`) removes every set-aside file whole, sidecars too, and health's warning clears; so does the usual pass once the file's `<s>` stamp is `keep_days` old (`days.rs:349-353`) (`days.rs:605-609`, `646`, `686`, `690-697`, `1294-1328`, `server.rs:262`; #2067); the 5 s reset wipes the whole folder (`setup/fixtures/reset-plan-product.json:5`) |
| memory | `category` of an entry | SQLite text | — | — | an unknown one reads as no category (`category_from`, `days.rs:1421-1431`): kept as stored, never read as another, given to a tidy as a previous observation, chosen as one, or superseded (`days.rs:1025`, `1067`; #1680) | the entry is kept and read; in a session note it is a plain fact |
| schedule | the store's container, `/var/lib/lampos-schedule/store/schedule.sqlite3` | SQLite | `user_version` 1; higher is **set aside** whole as `schedule.sqlite3.newer-db<version>-<s>`, never deleted, with a journal line, and the part starts with an empty store; read from the file's header before any SQLite connection, so a newer build's WAL database, or one with `-wal`/`-shm` beside it, is set aside with its sidecars, not refused, and a start after a crash between the sidecars' renames and the database's keeps the database under their `<s>` (`orphan_stamp`), but only when the database's mtime is no later than those sidecars' ctime and none of its own sidecars would land on a name already there; otherwise it takes a new stamp (`schedule/src/store.rs:172-287`, `454-463`); health says it (`set_aside`) until the file is gone (#1680) | n/a | n/a | the kept file's reminders are not sounded by this build. `user_version` 0 with tables, or a schema it doesn't expect at 1, still refuses to start (corruption, not a newer build) |
| schedule | reminders and timers in the same file | JSON | `schema` 1; higher is **set aside** as `schedule.sqlite3.newer-…`, its identity kept | read | a reminder with a value it doesn't know (e.g. a repeat `kind: "monthly"`), a field twice, a zone its tzdb doesn't know, or newer tzdb rules for one it knows (an offset that doesn't match, e.g. a daily 07:00 Asia/Ho_Chi_Minh stored at +08:00), at the same schema, is **unread**: skipped with a journal line naming only its id, kept in the store byte for byte and written back in id order, so the build that wrote it finds it again; counted on health (`unread`, a warning) and toward `MAX_ITEMS` (`engine/stored.rs:30-97`, `time.rs` `zone_rules_differ`, `engine.rs:153-185`; #1680) | the part starts and serves. An unread reminder is never sounded, snoozed or acknowledged here. A new stored value still needs a `schema` bump, now pinned by a test (`a_new_stored_value_comes_with_a_schema_bump`). The receipt alone is dropped with a line |
| schedule | the zone setup and install write, `/etc/lampOS/zone.env` (`LAMP_ZONE`) | env | none | n/a | a zone this build's tzdb doesn't know | falls back to the system's zone when it names one it knows; setup links `/etc/localtime` to the same name, so in practice to the fixed offset `LAMP_UTC_OFFSET_MINUTES` from the same file (`Etc/GMT+3` for -180): 07:00 rings at 07:00 at the lamp, an hour off at most across summer time. With no whole-hour offset stored, `Etc/UTC`, and a new reminder at a clock time with no zone of its own is refused (`bad_time`, with the reason), while one in minutes from now is set (`engine.rs:437-480`, `895-906`, `1017-1027`). A journal line and a health warning (`zone_asked`, `zone`; #1680) say it. Reminders a newer build set in that zone are unread here and stay silent, one-offs too, though their `unix_ms` is stored (row above) |
| tasks | the store's container, `/var/lib/lampos-tasks/store/tasks.sqlite3` | SQLite | `user_version` 1; higher is **set aside** whole as `tasks.sqlite3.newer-db<version>-<s>`, never deleted, with a journal line, and the part starts with an empty store (`tasks/src/store.rs:418-452`, `203-291`); health says it (`set_aside`) until the file is gone (#1680). `user_version` is read from the file's header first, with no SQLite connection (`store.rs:180-195`), so a newer database in WAL mode, or with `-wal`/`-shm` beside it, is set aside with its sidecars (renamed `<kept>-wal`, …) byte for byte, not refused; the sidecars go first, and a start after a crash between those renames and the database's keeps the database under its sidecars' `<s>` (`orphan_stamp`), so they stay paired, but only when the database's mtime is no later than those sidecars' ctime and none of its own sidecars would land on a name already there; a stale leftover from an older crash, or a complete earlier set-aside, gets a new stamp; a version bumped only in a `-wal` not yet checkpointed isn't seen and is refused, as a version-1 WAL database is. Every set-aside file is kept: a lamp that flips versions keeps one per flip, each up to 32 MiB, health names only the newest, and nothing in this build removes them. The schema has one version and no additive history, so nothing higher is read in place | an extra table, index or trigger at version 1 is refused (`store.rs:300-318`) | n/a | the kept file's tasks are not run, watched, told or closed by this build; their harnesses are left as they are. `user_version` 0 with tables, or a schema it doesn't expect at 1, still refuses to start (corruption, not a newer build) |
| tasks | task state in the same file (`state.snapshot`) | JSON | `schema` 2 (oldest 1); higher is **set aside** as `tasks.sqlite3.newer-<schema>-<s>`, with a journal and health line | read on `State`, `Task`, `Dispatch` (#1512); a field on `Target`, `Launch`, `Observation` without a bump makes that task unread (below) | a task with a value it doesn't know (e.g. `dispatch.state: "later"`) at the same schema is **unread**: skipped with a journal line naming only its id, kept in the store byte for byte and written back in id order, so the build that wrote it finds it again; counted on health (`unread`, a warning) and toward `MAX_TASKS` (`engine.rs:441-482`, `380-413`; #1680) | the part starts and serves. An unread task is left alone, mid-run or not: never invalidated, launched, watched, told or closed here. A new stored value still needs a `schema` bump, now pinned by a test (`a_new_stored_value_comes_with_a_schema_bump`) |
| tasks | targets, `/var/lib/lampos-tasks/targets.json` | JSON | `version` 1 | refused, read as "newer" | refused, read as "newer" (`runtime.rs:255-279`) | runs with hand-overs off, journal and health say so (`main.rs:132-150`) |
| tasks | computer names, `targets.names.json` (only `--set-up-later`) | JSON | `version` 1 | refused (`runtime.rs:992`) | n/a | no names, one line (`runtime.rs:1275-1283`) |
| eyes | known faces, `/var/lib/lampos-eyes/people.json` | JSON | `version` 1; higher sets the whole file aside in `set-aside/` (`eyes/src/who.rs:426-437`, `1153-1168`) | read, dropped on save (`who.rs:295`, `309`) | n/a | a person it can't read is set aside alone, one count-only line (`who.rs:441-478`). Nothing reads `set-aside/` back, so a version 2 leaves the lamp knowing nobody |
| eyes | its own set-aside records, `set-aside/people-*.json` | JSON | — | `Value` | n/a | not loaded; counted on health (`who.rs:1222-1228`) |
| skills | installed skills, `/var/lib/lampOS/skills/<name>/` (`skill.toml`, `SKILL.md`) | TOML + Markdown | the skill's own semver only | refused (`skills/src/lib.rs:125`, ~20 nested types) | refused (`lib.rs:663-664`) | that skill is "not loaded: …" and the rest load (`main.rs:687-690`, `lib.rs:2828-2836`). Its functions are gone, core skills included |
| voice | Live provider settings (install writes them), `/etc/lampOS/voice/provider.json` | JSON | none | read past, named once in the journal: `lamp-voice: <file>: fields this build does not read, ignored: …` (`voice/src/config.rs:70`, `90`, #1680); a field named as a key's (`*_key`, `token`, `*_secret`, `*_password`) is refused by name, since the file never holds a key (`config.rs:80`) | refused: a known field's bad value is a broken config, not a newer one (`Authentication`, `ReconnectContext`, `SilentResponse`, `config.rs:120-153`) | a file that doesn't read, or a known field with a bad value, still **refuses to start**: the voice can't run without it. The line names the file and serde's reason (`config.rs:226`) |
| voice | second route's settings, `/etc/lampOS/voice/second.json` | JSON | none | read past and named, as `provider.json` (`second_config.rs:79`, `319`; line `lamp-voice second: …`), a key's field refused by name | `tts_provider` is a string checked against a list (`second_config.rs:338-347`), still refused | as `provider.json`: unreadable or a bad known value refuses to start, naming the file (`second_config.rs:310-336`) |
| voice | conversation thread, `/run/lampos-voice/thread.json` (kept across a restart) | JSON | none | refused (`thread.rs:153`) | n/a | set aside as `.unreadable-<ms>` with a line (`thread.rs:368-372`, `917-940`) |
| voice | news told, tasks typed, `/var/lib/lampOS/voice/tasks-told`, `tasks-typed` | text lines | none | n/a | n/a | read as empty, no line (`task_news.rs:419-423`): news may be told again |
| voice | room sounds yes, `/var/lib/lampOS/consent/room-sounds.json` (`LAMP_ONBOARDING`, no-slide lamps only) | JSON | `version` 1, ignored | ignored | only `"yes"` is yes | lamp-ears reads anything else, or no file, as no; the voice removes it at start when its progress holds no yes, never writes it back (`onboarding/storage.rs`) |
| voice | onboarding progress, `/var/lib/lampOS/voice/onboarding/progress.json` | JSON | none | ignored | a step name this build doesn't know is read past and kept, the walk goes on with the steps it knows (#2110); an outcome it doesn't know is refused | an unreadable file stops the walk with `onboarding_storage_failed` (`onboarding.rs`, `Progress::check`) |
| voice | relay key `/etc/lampOS/voice/credential`; person's name `/etc/lampOS/person/name` | text | n/a | n/a | n/a | key read raw (`config.rs:400`); an unfit name is no name, with a line (`person.rs`) |
| voice | sessions and first output, `/var/lib/lampOS/voice/sessions/…`, `first-output/*.jsonl` | JSONL | first output `schema` 1 | — | — | written only, never read back at start |
| cloud | setup's account, `/etc/lampOS/setup/backend.json` | JSON | none | read (`account.rs:14`, all `serde(default)`) | n/a | an extra field reads. A changed type or bad JSON refuses to start, exit 2 (`main.rs:120-123`) |
| cloud | the person's own reports choice, `/var/lib/lampos-cloud/reports.json` (#2052) | JSON | none; the fields `share`, `at`, `by`, `held` | **read as off**, whatever `share` says, and kept for a roll forward, with a line (`cloud/src/reports.rs:774-814`). A no given over it is not taken for one already given: written, the queue deleted, `stopped` sent (`reports.rs:1163`) | an unknown `by`, or a `held` that isn't a boolean: read as off the same way | unreadable (garbage, cut short, no boolean `share`, can't be opened) **reads as off, never falls through to setup's yes**, and at start is set aside whole as `reports.json.unread-<s>`, an off written in its place, with a line (`reports.rs:1094-1126`). A newer record with a `share` that isn't a boolean is one of these: set aside and replaced by an off, not kept in place, which fails safe |
| cloud | setup's reports answer, `/etc/lampOS/setup/reports.json` | JSON | `asked`; above 1 reads as "no", said by nothing (a re-ask, as designed) | **read as off** with a line, left there (#2052, `reports.rs:815-854`) | n/a | unreadable reads as off with a line; left where it is, since it is root's and `lamp-cloud` can't write `/etc` |
| setup | its reports answer, `/etc/lampOS/setup/reports.json` | JSON | `asked` 1 (`setup/src/serve.rs:592`) | n/a | n/a | written whole by `reports_record` (`serve.rs:595-615`, `694-700`), never read back by setup |
| cloud | queue state, `/var/lib/lampos-cloud/reports/state.json` | JSON | none | read, dropped at the next save; a missing field is its default (`serde(default)`, `reports.rs:911`) | n/a | unreadable is set aside whole as `state.json.unread-<s>` with a line (`reports.rs:1130-1156`); the next `seq` goes past every report still on flash (`reports.rs:1052`). Lost with the queue empty, `seq` starts again at 1: harmless, since the backend drops a duplicate by `(device, seq, sent_at)` |
| cloud | queued reports, `/var/lib/lampos-cloud/reports/<seq>.json` | JSON | `schema` `lamp-report/1`; a higher one is **kept where it is and not sent**, with a line (`reports.rs:945-993`) | read, and sent as is | a `kind` other than `crash` or `stopped`: kept and not sent, as a newer schema | unreadable (garbage, cut short, over 4 KiB, no `sent_at`) is set aside whole as `<seq>.json.unread-<s>`, never sent, with a line; the rest are read. A no deletes the kept newer ones too (`reports.rs:1280`) |
| cloud | exit notes, `/run/lampos-exits/<unit>@<at_us>.json` (root's, 0644) | JSON | none | read | a new `result` sends no crash | unreadable is skipped with one line and left whole: `lamp-cloud` can't write the folder, and `/run` goes at the next boot; `write_whole`'s `.tmp` is no longer read (`reports.rs:1457-1508`) |
| setup | serve config (only `lamp-update install --setup on` writes it, `update/src/install.rs:375-384`), `/etc/lampOS/setup/serve.json` | JSON | none | read, dropped with a line (`setup/src/serve.rs`, `read`) | n/a | an extra field is dropped and named in the journal; a bad known value or bad JSON still refuses to start, exit 2. Not in update's settings list, so a put-back never restores it |
| setup | reset plan, `/etc/lampOS/setup/reset-plan.json` | JSON | none | **refused**, except `keep_back`, whose every entry is dropped with a line and its path wiped (`setup/src/reset.rs`, `read`) | a `wipe` entry that isn't a plain path's text **refuses the reset**; a newer plan's extra plain path is wiped (still checked against `PROTECTED`) | all or nothing: a refused plan journals nothing, keeps the marker and wipes nothing; `watch-reset` keeps watching and tells the person who holds the button "Nothing was wiped. Update me, then reset again." (say service, network only), the other commands exit 2. A bad known value or bad JSON is refused as before. Written over by the build's at each install and each round that carries `reset_plan` (#806), the old one kept in the records first; a put-back restores it |
| setup | done marker `/var/lib/lampos-setup/done`; reset journal `reset.json` | file / JSON | `version` written, never read | n/a | n/a | existence only (`serve.rs:702-703`, `reset.rs:333`) |
| update | lamp-update's own switch, `/var/lib/lampos-update/self.json` | JSON | `version` 1, never checked | read, dropped (a guard test) | n/a | fixed for #1680: one it can't parse is a switch that **failed**, never one confirmed. When the link moved after the file was written and `update/` holds exactly one other build, the new build is refused, then the link goes back, then the file is set aside (`update/src/own.rs`, `known_builds`, `settle`); a cut between steps never turns it around, since a start that finds the other build refused takes the put-back as done and only sets the file aside. This is done by the updater that runs, which then execs the old one, or by `lamp-update-back` on its usual gate, the deadline counted from the file's mtime. When the builds aren't known it's renamed `self.json.corrupt-<ms>` with a `log.jsonl` line and the link left, by `serve` only after the dry round a confirm needs. A round refuses while it waits. A file the disk won't give (not bad JSON) is never settled: that round is refused with a line (`self-unread`) and the next one reads it again. Only a `lamp-update-back` installed from this build on does its part |
| update | `/var/lib/lampos-update/state.json` | JSON | none | **kept**: fields a later build wrote survive a save (`State::later`, `records.rs`) | n/a | read when a round acts. Unparseable is set aside as `state.json.corrupt-<ms>` with a line. `peek_state` (dry rounds) still falls back to empty silently |
| update | `/var/lib/lampos-update/journal.json`, a switch under way | JSON | none | read, dropped (`records.rs`) | n/a | fixed for #1680: one it can't parse is set aside as `journal.json.corrupt-<ms>` with a line (`round.rs`, `acting`), and the round goes on from the units as they are. What it drops is a half-done switch's put-back: units, links, models, settings and skill files stay as the cut switch left them, the next round finishes that switch, and if that fails its put-back returns to the mixed state, not to the build before. Still better than every round failing for good. A newer field about what to put back is still not put back |
| update | `self-refused`, `previous/`, `log.jsonl`, `harness-at-boot` in `/var/lib/lampos-update/` | text / JSONL | none | n/a | n/a | bad lines skipped (`own.rs:109`, `records.rs:375`); `log.jsonl` never read back |
| update | apply's `request.json`, `back.json`, `done.json` in `/var/lib/lampos-update/apply/` | JSON | `version`, never checked (`apply.rs:79`, `112`) | read, dropped | `Op` refused (`apply.rs:68`) | refused when used: that apply or back fails and writes nothing (`apply.rs:370`, `623`, `297`) |
| policy | decision and judge logs, `/var/lib/lampos-policy/<kind>-<date>.jsonl` | JSONL | none | never parsed by the daemon (`record.rs:136-145`) | n/a | only the last byte is read, to end a cut line |
| policy | judge key, `/etc/lampOS/policy/jev.key` | one token | n/a | n/a | n/a | refuses to start on anything but one token (`decide.rs:997-1014`); a token can't gain a field |
| selftest | reports, `/var/lib/lamp-selftest/` | JSON + text | none | — | — | written only; pruned by name (`selftest/src/report.rs:40-55`) |
| records | retention config (install writes it), `/etc/lampOS/records.json` | JSON | `schema_version` 1; higher is told apart first (`records/src/config.rs`, `read`, #1680): `check` prunes nothing, writes no result, says so in one line and succeeds; the reporter says "records: a newer schema, cleanup paused" at warn in lamp-health | **read**, named once a run by path (`records/src/forward.rs`); a key-like field refused by name | refused (`Kind`): an unknown kind never becomes a path | runs as before, with one line naming the fields. A file that doesn't parse or a bad known value still fails the run, told by serde's category, line and column only |
| records | last cleanup report, `/run/lamp-records/last.json` | JSON | `schema_version`; higher is **set aside** as `.newer-N-T` (`json_file.rs`) | read, dropped (`lib.rs`, `RunReport`) | at schema 1 a plain new `mode` (lowercase and `_`, every other field this build's) is **set aside** as `.newer-mode-T` and the cleanup runs (#1680); any other mode is refused | a malformed one, or a new mode beside a field that doesn't read, is still "not a recognized cleanup result" and left in place. tmpfs, so a boot clears it |
| near-call | the near call's bar, `/var/lib/lampOS/voice/near-call.json` | JSON | none | read (`near-call/src/lib.rs:189`) | n/a | unreadable is the default bar, no line, file kept (`lib.rs:246`) |
| keeper, bus | `keeper.json`; kept logs `logs.*.jsonl` | JSON / JSONL | none | read | n/a | not ready by default (`keeper/src/package.rs:259`); logs only listed by name |
| settings | snapshot, `--dir/settings.json` | JSON | `version` must be 1 | refused (`settings/src/engine.rs:47,57`) | refused | refuses to start (`main.rs:156`; `#[should_panic]` test `engine.rs:331`). No unit runs lamp-settings |
| station | a run's record, `--out/<run>.json` | JSON | `format` 1 | refused | refused | only as `--retest-of`, which refuses that run (`station/src/main.rs:117`). A bench tool, not on the lamp |
| expression | `/etc/lampOS/expression/expression.json` (update's settings, `install.rs:335`) | JSON | `version` must be 1; another is refused | **read**, named once in the journal by path (`catalogue.rs`, `Catalogue::from_json_naming`; `main.rs`, #1680) | refused (`Trigger`, `catalogue.rs:13`) | a newer catalogue's unknown fields, at any depth, are read past and named ("fields this build does not read, ignored: …"); a bad known value, a new trigger or another `version` still refuses to start. Tests: `crates/expression/tests/it/forward.rs` |

No store: health, mood, air, name (it reads only its models), actions,
turn-end, observer, ears, camera, audio, ring, environment, lean,
turn-owner, voice-processes, gemini, msgs, same-voice, local-time, ipc.
surround has stores but no lamp runs it (`deploy/product/README.md:41`);
its forward-read tests are written and expected to fail. sound-fixtures
and voice-eval are for the Mac only.

### Fix order

One PR per crate onto `lamp-voice`, each with a test that loads the next
release's record and fails without the change, as lamp-tasks' fixture
test does (#1512):

1. **voice**: `provider.json` and `second.json` read past an unknown field
   and value; a file it can't read at all is the only refusal. Done for
   fields (#1680); a new value of a known field is still refused, since
   these are settings and running on a broken one is worse. Then the
   delivery consumer's child (with schedule, below).
2. **setup**: `serve.json` and `reset-plan.json` read one release
   forward (`el/stores-setup`, the rule agreed on #2060). A next build
   names anything to wipe as a plain path in `wipe`, which this build
   wipes, and anything to keep back only in `keep_back`, whose entries
   this build drops (wiping more). A new `wipe` kind or any other new
   field refuses the reset whole, with nothing wiped.
5. **memory**: a higher `user_version` sets the database aside and starts
   a new one, with a line and a health warning; an unknown category is
   kept, not read as `Thread` (both done, #1680). Old journal lines that
   don't read stay dropped: they are an older build's (the row above).
6. **update** (done): `self.json` it can't read is a failed switch, put
   back when both builds are known, else set aside, and the rounds go on;
   `journal.json` is set aside with a line; `state.json` keeps a later
   build's fields.
7. **controls**, **expression** (#2065), **records**: read their files
   one release forward (one PR each). **body** stays strict (#2071): its
   poses are a safety exception.
8. The medium rows after: **cloud** (reports deleted and `state.json`
   reset silently; done in #2077, with the choice read as off
   when it can't be read), **eyes** (`forgets-pending`: done, set aside
   with a line), **skills** (`off`). The eyes' `.from-photos` stays as it
   is: a file it can't read, a later version's included, is deleted on
   purpose, since unconfirmed prints out of the 30 days' reach are no
   one's to keep (the reviewer on #1968).

## Most likely wrong on a lamp

- **That a minute of health says enough to put back a bad voice.** A voice
  that crashes, is restarted by systemd, or says it is in error on
  `diagnostics` is caught. A voice that runs, says it is well and hears
  nothing, mishears, or answers badly is not: the voice's own report says
  only whether it can reach the speech service. A part whose unit is
  `Type=exec` (all of lamp-4ace's) is "running" as soon as it has started;
  the body's start checks and the voice's first listening are not waited
  for. Until parts say ready (keeper.md), a first conversation after an
  update is the real check.
- **Unit files that change between builds.** Ordinary linked product services now use the signed `product` carrier above; pinned and exact lamp-specific services are carried from
  `deploy/<lamp>/` of the build, which the first builder keeps by hand, and
  LAMP.md and the units there already disagree (LAMP.md says voice
  `d628ed1d`; `lamp-voice.service` names `c8ccd3dd...`). The build is
  written into a carried unit, so a stale build there does no harm; a stale
  option or `Environment=` line does. A drop-in (`lamp-voice.service.d/`)
  is not read and still overrides. Restart ordering reads these managed
  base service files, not systemd's effective graph: unit aliases and
  symlinked unit files are not resolved, and `.socket` units and external
  services are not graph nodes or directly restarted. The parser recognizes
  `Sockets=` as an ordering declaration, but the current managed inventory
  contains only services. The product Eyes and Skills units declare `After=` Voice;
  their order follows those files too. A unit the build carries for a part
  not installed is not added, and a part's new unit is never enabled:
  adding a part to a lamp stays the first builder's.
- **The disk holding two copies.** lamp-4ace's board said 14% free on
  2026-10-14 (health.md); the disk's size is not in the repository. The
  check counts every new file whole, and `/opt/lampOS/releases/` already
  holds every voice installed by hand, which lamp-update never removes.
- **The quiet moment.** It needs the voice on the bus (`LAMP_BUS=on`, as on
  lamp-4ace). In an office the 45 s may come only at night. The check is
  made again right after the units are written and, after reading queued
  speech, at the voice's own restart boundary. Someone can still start a
  turn after the last check and before systemd stops the voice: it has no
  pause that closes it to new turns yet.
- **Switching parts from the updater's sandbox.** The 19 October
  parts trial ran `once` directly; the 20 October service trial switched
  only the updater. Neither shows a round that switches parts under the product
  updater unit's `ProtectSystem=strict` sandbox.
- **The body's restart folds the lamp** (body.md); at a quiet moment it
  may still be standing from the last conversation.

## Not shown

- A published release fetched over the internet, verified with the real
  signing key, switched quietly and put back on failure, including an
  attended run with the body. These remain
  [#1812](https://github.com/autonomous-ai/lampOS/issues/1812)'s acceptance
  checks. Publication is now active, but independent verification with the
  trusted public key and the attended acceptance run remain outstanding;
  the local trials do not complete them.
- Independent trust and device acceptance of the published release. Real
  CI publication itself has run: release `lampos-lamp-voice-6449`, source
  `be16c3327`, came from successful ARM64 build and publisher run
  [39973722029](https://github.com/autonomous-ai/lampOS/actions/runs/39973722029).
  Matching downloaded bytes to its asset digest and manifest entry does not
  substitute for independent signature verification or an attended install.
- A power cut during a switch, on the lamp's storage: the journal is
  tested with the files a cut would leave, on the Mac.
- Models in the manifest (`LAMP_UPDATE_MODELS`): tested on the Mac only
  (`tests/round.rs`). A download cut short starts again from nothing; it
  isn't resumed. No published build carries models yet.
- The image's own files (`LAMP_UPDATE_IMAGE`): tested on the Mac only
  (`image.rs`, `apply.rs`, `tests/round.rs`), with lamp-update-apply called
  in the round's place of systemd and every command recorded, not run.
  From the review of #1945: an image path outside `IMAGE_FILES` (manifest
  check), listed twice, or whose text isn't its SHA-256 is refused; a
  signed build naming `/etc/sudoers.d/lamp` writes nothing; a hardware
  profile that can't be read fails no round, on or off; an older
  lamp-update-back doesn't get the voice restarted.
  `systemd-analyze verify`, `daemon-reload`, `udevadm control --reload`
  and the speaker unit's start haven't run on a lamp, nor the wider
  sandbox. No published build carries the image's files yet.
- Settings and tmpfiles (`LAMP_UPDATE_APPLY`): tested on the Mac only
  (`apply.rs`, `tests/round.rs`), with lamp-update-apply called in the
  round's place of systemd. The unit's sandbox, `systemd-tmpfiles` and
  `systemctl restart` of a one-shot haven't run on a lamp. The product publisher now includes settings; a published carrier and an attended settings switch still need device acceptance.
- Setup's reset plan (#806): tested on the Mac only (`tests/round.rs`,
  `tests/install.rs`): a lamp with the plan of 19 Oct or of 20 Oct 10:36
  (lamp-0c89's), updated by a round, ends with the build's and a backup.
  No round carrying the plan has run on a lamp. The product publisher now includes it with settings; this does not claim an attended published-plan switch.
- The keeper,
  `lampos.target`, timers of no part, models and libraries a build does not carry,
  configuration under `/etc/lampOS` (but for `second.json`, above), and a
  part new to a lamp.
- Two parts of one build that must change together across a failed
  switch: everything changed is put back together, but a part that stores
  data in a new form (memory, schedule) before being put back is not
  undone.

### Released Cloud readers of installed package provenance

The [Cloud provenance release-pair record](verification/cloud-provenance-release-pair-20261024/README.md)
covers the `skills.list` consumer separately from package installation.
Actual released Skills writers and Cloud readers meet through installed
package metadata and a private MQTT peer, both directions. Fixed install
paths require a fresh network-disabled native container with private tmpfs;
no host lamp state or real account is used. The two explicit native cases
passed in official ARM64
[run 40070585559](https://github.com/autonomous-ai/lampOS/actions/runs/40070585559)
on `2ad786f91`, with 14 matching child exits and verified container removal.
The linked qualification record retains the exact head, proof and limits.
Different Cloud payloads contain the same provenance parser; this is not
proof of distinct parser versions,
Store signature trust or physical update durability.

### Projection read inputs for a future frozen plan

Head-touch projection now has an internal text/lazy-directory input boundary.
Ordinary wrappers retain their existing filesystem behavior and evaluation
order; a test-only closed reader proves replay without filesystem fallback.
The [read-boundary contract](head-touch-inputs.md) records why drop-in absence,
membership, custom pins and conditional calibration remain dependencies. It
adds no actual collector, authentication, snapshot freshness or writer guard.

### Read-only bounded capture for head-pair policy (#3689)

A separate, unwired collector now supplies that policy with descriptor-anchored,
bounded text and directory observations. It retains regular bytes/metadata,
explicit missing-leaf/ancestor facts, bounded link chains, actual null-mask
metadata and complete directory membership. Evaluator output is returned only
after a fresh comparison; provider errors poison collection even if caught.
Closed replay has no filesystem fallback and offers explicit read-only
revalidation for a future immediately-before-write boundary. Ordinary wrappers,
policy and call order remain unchanged.

Stable trusted root ancestry/ownership, excluded writers, complete evaluation
and candidate/output binding remain caller responsibilities. No file writes,
authentication, admission, helper execution, bootstrap or restoration are added;
this is neither an atomic snapshot nor a whole-round/systemd configuration proof.
The [capture contract](head-touch-capture.md) records compatibility, conservative
collector-only bounds and unverified deployment coverage. The original
`5d866fd9c/992303fc8` full Mac attempt remains failed at **520/8/6 ignored**;
six fixture permission errors and two unchanged World failures are retained,
with the latter causes unestablished. The subsequent explicit-permission source
on `3c9838fea6` remains **530/1/6 ignored**: this Mac filesystem returned `EILSEQ`
while creating a raw-name fixture before the collector ran. The current
`1d97a2c6d0` source keeps portable exact-byte closed replay and real invalid-UTF8
content refusal everywhere, with physical raw-name capture/revalidation in an
explicit Linux subsection that native ARM64 CI must execute before Ready.
No whole test is ignored and production behavior is unchanged. The first
fixture-corrected full Mac gate on `0d1d1f2b94` passed **531/0/6 ignored**, all
52 required controls once, with formatting, strict Update Clippy, source/config
guards and actual/preserved executable hashes closed. The validation prose was
finalized afterward without changing Rust/test bytes; it adds no runtime credit.
Neither failed result is waived or diagnosed by the pass. Mac physical raw-name
capture remains unverified, native ARM64 must run the Linux subsection, and no
device/bootstrap/restore acceptance is claimed here.

### Bounded saved head inputs for restart replay (#3689)

The private [head-input codec](head-touch-codec.md) encodes captured facts and
decodes externally hash/size-bound bytes into a separate closed `HeadInputs`
provider. Decode and replay read no live files; explicit fresh revalidation
remains separate. Typed negative facts, null metadata, raw names/link targets,
witnesses and enumeration order are retained, as are exact accepted future
metadata bytes. Semantic versions, operations, kinds and duplicate known fields
refuse under streaming and whole-document bounds.

This remains unwired data preparation. It adds no storage or durability receipt,
authentication, path authority, executable plan, writer grant, helper refresh or
restore. Ordinary reader policy and native capture equality are unchanged;
current source/native evidence and physical acceptance remain separate.

### Cap projected pins output for the future saved plan (#3689)

A separate private [plan-output path](head-touch-plan-output.md) reuses the
ordinary head-pair evaluator with capped streaming pretty serialization.
New derived pins text is at most 1 MiB including its newline. An owned staged
step vector is returned only on success; serialization or late provider errors
return a fixed refusal and no partial vector. This is a bound on newly derived
output, not on parsing, preexisting text or a complete authenticated plan.

Ordinary callers retain their formatter, read order, diagnostics and bytes.
The new path is unwired and introduces no storage, path authority, writer grant,
helper refresh or restoration. Native/device acceptance remains separate.

### Raw ownership capture for a future writer guard

The unwired `writer_settings` adapter now associates each ordinary setting
with bounded descriptor-observed bytes and the shared existing ownership
decision, including omitted no-ops. It adds no live apply path, helper,
allowlist entry or grant. Its read-only revalidation checks the same bytes,
selected metadata, ancestor identities and explicit absences without
re-planning or enlarging the original byte budget. No mutation boundary is
wired. The [capture contract](writer-settings-capture.md)
separates this raw input from authentication, head-touch projection, effect
admission and independent recovery.

### Read-only agreement of saved Head and ordinary file observations (#3689)

The private [coherence check](head-preimage-coherence.md) compares exact logical
root spelling, each ordinary root identity, represented lexical directory
prefixes, overlapping regular bytes and typed absences. Captured directory
membership is checked where it represents the next ordinary component. Link
resolution and null/device overlap remain conservatively unsupported; unrelated
closed Head facts retain their semantics. No encoding, runtime caller, live
read, admission, writer grant or restoration effect is added. This agreement
is not authentication, freshness, exclusion or an atomic filesystem snapshot.

### Pure ordinary plan preparation (#3689)

The private [preparation API](ordinary-plan-preparation.md) now checks full
signed input/key/context and the independently selected product records against
saved preimages and closed Head facts. It retains every signed row and the
projector's separate order, including an appended PINS no-op becoming a write.
Only the unchanged writer identity and a narrow ordinary-settings shape are
accepted; units, models, image/reset/tmpfiles/skills effects remain refused.
The borrowed-context in-memory result is neither durable nor an apply grant.
There is no State/live-file reader, runtime caller, capability bump, privileged
writer, publication or restoration path.

### When the writer may be refreshed (#3689)

The refresh copies the running lamp-update to the writer's fixed path. That
copy, `lamp-update-back`, is also what puts an unconfirmed lamp-update back.
So it may be replaced only once lamp-update's own switch is settled.
`writer_ready::ready` says yes only when no `self.json` is in the records (one
that can't be read counts as not settled), `update/current` names a build (a
bare 64-hex name, not a path), and that build's `lamp-update` is a regular
file holding exactly the bytes of the lamp-update that runs. A lamp-update
put back while still running, a build changed under its name, or a link to
anything else is refused, with why. It reads the records from the lock held
on them (`records::Lock`, which knows its folder), so a caller can't ask
without holding that lock, and no switch of lamp-update's own begins while
it is held. The refresh (next sections, "The settings writer refreshed")
asks it under the round's lock, held until the copy is placed.

### The writer's one-shot unit (#3689)

A round's sandbox may write `/etc/systemd/system` but not
`/usr/local/lib/lampos`, where the fixed settings writer is. So the refresh
runs as a one-shot unit of its own, `lamp-update-writer.service`, whose text
is compiled into lamp-update (`writer_unit::TEXT`). The round writes it and
starts it, and systemd runs `/opt/lampOS/update/current/lamp-update
writer-refresh` as root under that unit's sandbox: `ProtectSystem=strict`,
writable only `/usr/local/lib/lampos` and `/var/lib/lampos-update`, the update
key read-only, no network, 120 s. It has no `[Install]` and is never enabled.

`writer_unit::run` writes the text at `/etc/systemd/system/lamp-update-writer.service`
(mode 0644), only when it differs, then reloads systemd. A link at the name
(a masked unit is one) or anything but a regular file is refused and left as
it is. Before the start it reads systemd's own view of the unit (`Id`,
`LoadState`, `FragmentPath`, `DropInPaths`, `UnitFileState`). That view must
be the file written: loaded, from `/etc/systemd/system`, no drop-in, and
static or disabled. A drop-in, a fragment elsewhere, a masked, enabled,
runtime or generated unit, a missing or repeated property, or an unknown one
starts nothing. systemd is asked only for this one unit name, through two
methods of the `Systemd` trait. A systemd that lacks them refuses. A start
that runs past its bound ends only the waiting: systemd's job goes on, so
its result is read from what the command leaves, not from the start.

A round runs it to refresh the settings writer ("The settings writer
refreshed", below). The unit has been checked by its tests' reading of the
text and a fake systemd, not by a real systemd: neither `systemd-analyze
verify` nor a start on Bookworm has run.

### A fixed copy, checked (#3689)

`fixed_copy::place` is the step that puts a copy of lamp-update at a fixed
root path, such as `/usr/local/lib/lampos/lamp-update-back`, which no round
can write. The settings writer's refresh needs it, whichever way that is
finished. It writes the new copy beside the fixed path (`.<name>.new`, mode
0755, synced) and runs it once with `writer-probe`, within 20 s, its process
group owned. Only when the copy answers with the SHA-256 of the bytes
written is it renamed over the fixed path and the folder synced. With a kept
path (`lamp-update-back.prev`, say), the copy it replaces is written there
first, with its mode. A kept file already there is used only when it holds
the same bytes as the fixed path: a different one may be the only good copy
left, so nothing is replaced.

A copy that doesn't answer, answers another SHA-256 or another protocol, or
fails, is removed, and the fixed path and kept path are left as they were.
The same bytes already at the fixed path, executable, are left and nothing
is run. A link or a folder at either path is refused before anything is
written, and so is a copy over 64 MiB. A cut at any step leaves the fixed
path whole, old or new. If writing the kept copy or the rename fails, the new
copy is removed too. A folder that can't be synced after the rename is
said in what `place` returns (`Replaced` with `synced: false`): the new
copy is in place, though a power cut could still bring the old one back,
and it is never read as "nothing replaced". The caller keeps other writers out of the folder while it
runs: the copy there is read once and renamed over without being read again.
On Linux `writer-probe` hashes `/proc/self/exe`, the running file itself, so
a file put at its path since it started can't answer for it.

The probe shows that the copy starts on this lamp and that its bytes are the
ones written. It does not show that its `back` or `apply` work: those are
proved by their own first use. `lamp-update writer-refresh` calls it ("The
settings writer refreshed", below). Its tests run real programs as copies:
one that answers as lamp-update does, one that answers another SHA-256, and
one that fails.

### Bounded saved ordinary plans (#3689)

The private [plan container](ordinary-plan-container.md) explicitly encodes a
successfully prepared result and reopens it against an externally protected
plan identity and the original expected context. Exact envelope/preimage/Head
bytes, all signed decisions and projected order remain separate and immutable.
Bounded framing and deterministic replay comparison refuse semantic changes;
accepted nonsemantic metadata stays in the original container bytes. This is
checked data without a State/live-file reader, durable publication, journal
admission, apply grant, executor or restoration.

### Original product facts for closed ordinary replay (#3689)

The private [present-only product projection](ordinary-product-context.md)
retains exact externally hash/size-bound original State bytes and streams only
installed/refused/selected recorded-writing facts for preparation. It preserves
unknown metadata and unselected paths for a later signed release. Missing or
corrupt input never invokes the ordinary loader's fallback. This deliberately
limited projection is not full State validation, authentication, live capture,
immutable publication, admission, execution or recovery acknowledgement.

### The settings writer refreshed (#3689)

A round that applies refreshes the fixed settings writer
(`/usr/local/lib/lampos/lamp-update-back`) from the lamp-update that runs,
before it looks at a build. So the build's settings are written by the
writer of the same build (`writer_swap`). It does so only when the writer is
another copy. A lamp with no writer at all, installed before #1680, is left
as it is: that copy is `install`'s to put there.

- **The round's side** (`writer_swap::ask`), under its records' lock: it asks
  `writer_ready::ready` (no switch of lamp-update's own waiting, and
  `update/current` names the build that runs). Then it writes
  `writer-refresh.request` in the records, naming that build's SHA-256, and
  starts the writer's one-shot unit (above) and waits for it.
- **The unit's side**, `lamp-update writer-refresh`, as root:
  - The round holds the lock, so the program can't take it. It acts only on
    a request naming its own bytes, written within the last 5 minutes.
  - Run by hand with no round at work, it takes the lock and asks `ready`
    itself.
  - Either way it places its own bytes with `fixed_copy::place`, the copy
    there kept as `lamp-update-back.prev`. It removes the request, and keeps
    what it did as `writer-refresh.done` (the build now there, the one
    before, whether the folder was synced).
- **What the round says:** the writer is now this build, or why it is not.
  A unit that can't run takes its request back and leaves the writer as it
  was. A dry round (`check`) and a round given none of its own bytes (every
  test but these) refresh nothing.

**Proved, or put back.** A new writer is proved by its own `back`. The
fallback's timer runs it every 2 minutes, and a `back` that ends well notes
its own bytes' SHA-256 and the time in the records (`writer-back.ok`). A
`back` that found the records' lock held (a round at work) ran no further
than its start, and notes nothing. The writer's unit clears any failed state
`lamp-update-back.service` still shows from the copy before, so a failure
read later is the new writer's. A build put back stays put back through a
later refresh. The unit's start is waited for 150 s, past its own 120 s
bound. Until the refreshed writer is proved:

- **No refresh over it.** A later round looks, and does nothing else with
  the writer.
- **No switch of lamp-update's own begins**, since that `back` is a switch's
  only way back: the round says it waits.
- **Proved:** once a `back` by this writer has ended well since the refresh,
  the round asks the unit to remove `lamp-update-back.prev`
  (`writer_swap::Op::Proven`). A later build of lamp-update may then refresh
  it again.
- **Put back:** when `lamp-update-back.service` has failed, or this round's
  first `apply` by the new writer failed, the round asks the unit to rename
  `.prev` back over it (`Op::PutBack`). The apply's own undo runs first,
  with the writer that made it, when it wrote anything; a writer that
  failed before its first write is put back all the same. The build put
  back is kept in `writer-refresh.done` and never asked to be the writer
  again.
- **A failed `back` that began before the refresh** is not the new
  writer's. The copy before's `back` may be mid-run when the refresh lands,
  and fail after the unit's `reset-failed`. So a `failed` counts only when
  the back unit's `ExecMainStartTimestamp` is after the refresh's
  `at_unix_ms`. It is read with `systemctl show --timestamp=unix`, in whole
  seconds, and asked only of a `failed` unit. As root, `systemctl` asks
  systemd's own socket, so it answers on a lamp with no D-Bus daemon,
  where `busctl` doesn't (Reviewer-three's P2 on #4199, shown on systemd
  252).
- **The refresh's `at_unix_ms`** is taken once the copy is in place, after
  its writes, its probe (up to 20 s) and the rename. So every `back` that
  began of the copy before began by it (Reviewer-three's P3 on #4190).
- **A failure in the refresh's own second, or before,** is waited past to
  the next `back`, about 2 minutes on, and said (`… back failed in a run
  that began before it was refreshed: the next back judges it`). When
  systemd can't say, it counts (El's P3 on #4178).

`installed_units` leaves out the writer's unit, as it leaves out
lamp-update's own. It starts `update/current` too, and taken for a part it
would move that link as a part's switch, outside the self-switch and its way
back. A test pins that. Not shown on a real systemd or a 7071 lamp: the tests
run the unit's program in-process, and a writer's failed `back` is a fake
systemd's failed state.
