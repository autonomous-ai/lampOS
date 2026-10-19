# Updates: a lamp keeps its lampOS current by itself

Task 45 ([#737](https://github.com/autonomous-ai/lampOS/issues/737), the
second builder, proposal `second-P5`). Until now a lamp took new lampOS
software only when the first builder copied binaries to it over SSH from
the office Mac (`~/lamp-4ace-v2-deploy-20261012/*_deploy.sh`). This is the
simplest version that works end to end ("wide first, deep later"):

- CI signs and publishes every tested build of `lamp-voice` as a GitHub
  release (a proposal below: the workflow is the first builder's, and the
  key is the owner's to make).
- `lamp-update` (`crates/update`), its own process with its own unit and
  records, learns of a newer build, fetches what changed, checks the
  signature and every SHA-256, installs each part beside the old one,
  switches at a quiet moment, watches each part for a minute, and puts the
  previous build back by itself if a part is not well.

**Kind: off unless set.** Nothing changes on a lamp until `lamp-update` is
installed there, and then nothing is written until its switch
`--apply on` is given: with `--apply off` (the default) it reads the
channel and says what it would do, and writes nothing, not even its own
records. A test shows it
(`without_apply_nothing_is_written_and_what_would_change_is_said`: every
file under the lamp's root byte for byte the same, and systemd not asked).
It publishes nothing on the bus at any setting, and with `--apply off` it
does not even subscribe. After
its minute on lamp-4ace the default becomes `on` and `off` stays to turn it
off ([joining.md](joining.md), "The switch is for that first minute").

## What exists and what we take

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

## Publishing (a proposal)

**A GitHub release for every tested build of `lamp-voice`, and one release
that is the channel's head.**

- After the ARM64 job has built and tested a push of `lamp-voice`, a second
  job makes a manifest of every `lamp-*` binary (with its SHA-256 and size,
  checked against CI's `SHA256SUMS`), every unit of every lamp's own
  folder (`deploy/<lamp>/`, a folder with a `LAMP.md`), and the product
  skills (`--skills lampOS/skills`, below: "The product's skills"), signs it,
  and publishes the binaries, the units (as `<lamp>.<unit>`), the product
  skills (`lamp-product-skills`), the manifest and the signed envelope as
  release `lampos-lamp-voice-<run number>`: a
  prerelease, never "Latest", which this repository's other releases use.
- It then replaces the one asset of release `lampos-lamp-voice`, the
  channel's head: `lampos-manifest.json`, the signed envelope of that
  newest build. A lamp polls only that file, every 15 minutes:
  `https://github.com/autonomous-ai/lampOS/releases/download/lampos-lamp-voice/lampos-manifest.json`.
  It answers with a redirect, as every release download does; neither the
  API, its hourly limit nor a token is involved. One file, so the manifest and its
  signature are never read from two builds; while it is being replaced a
  lamp may find nothing, and nothing changes.
- The sequence is the workflow's run number, which only goes up. A lamp
  takes a build only if its sequence is greater than the one it runs.
- `min_updater` (#1680), when set, is the oldest updater that can apply the
  build. It's a level, `manifest::UPDATER` in `lamp-update` (2 now: 2 reads `models`), raised
  whenever the updater learns to apply something an older one can't. It
  isn't the sequence, because a binary doesn't know its CI run number. It
  sits in the manifest's signed bytes like every other field. Left out
  (`lamp-update manifest` without `--min-updater`), it isn't written, and
  the manifest is byte for byte what an updater before the field reads.
  `lamp-update manifest --min-updater N` refuses a build that carries no
  `lamp-update`, and an `N` above its own `UPDATER`, which is the carried
  updater's level because the build's own `lamp-update` makes the manifest.
  So a channel never names an updater no lamp could reach.
- The signature: Ed25519 over the manifest's exact bytes, with `ring`
  0.17.14, which the workspace already has through rustls (no new crate
  is fetched; `base64` 0.22.1 is already there through lamp-gemini and
  lamp-expression).
- The key: made once by the owner, off the repository, never by a builder:
  `openssl genpkey -algorithm ed25519 -outform DER | base64`. That one line
  is the repository secret **`LAMPOS_UPDATE_KEY`**. Its public half,
  `lamp-update public-key --key-file FILE`, goes to each lamp at setup as
  `/etc/lampOS/update/key.pub`. Nothing in the repository holds either.
- Why not the alternatives: CI's artifacts need a token, are zipped and
  last a week. A moving release with the binaries in it would let a lamp
  read a manifest of one build and a binary of the next. TUF (above) is
  the deeper design. Whole-image A/B updates (RAUC) wait for the board's
  boot and partitions to be qualified.
- What it costs: about 147 MB of assets for each push of `lamp-voice`
  (31 binaries; run 144 of 2026-10-14). The prune step below keeps the
  newest 30 builds.

The proposed job, for `.github/workflows/lampos-arm64.yml` (not changed by
this task):

```yaml
  publish:
    name: lampOS — sign and publish lamp-voice's build for the lamps
    needs: arm64
    if: github.event_name == 'push' && github.ref == 'refs/heads/lamp-voice'
    runs-on: ubuntu-22.04-arm
    timeout-minutes: 20
    permissions:
      contents: write
    concurrency:
      group: lampos-publish-lamp-voice
      cancel-in-progress: false
    steps:
      - uses: actions/checkout@v4
        with:
          sparse-checkout: |
            lampOS/deploy
            lampOS/skills
      - uses: actions/download-artifact@v4
        with:
          name: lampos-arm64-${{ github.sha }}
          path: build
      - name: manifest
        run: |
          chmod +x build/lamp-*
          build/lamp-update manifest --binaries build --deploy lampOS/deploy --skills lampOS/skills \
            --channel lamp-voice --sequence ${{ github.run_number }} --commit ${{ github.sha }} \
            --files-url "https://github.com/${{ github.repository }}/releases/download/lampos-lamp-voice-${{ github.run_number }}/"
      - name: sign
        env:
          LAMPOS_UPDATE_KEY: ${{ secrets.LAMPOS_UPDATE_KEY }}
        run: |
          test -n "$LAMPOS_UPDATE_KEY" || { echo "LAMPOS_UPDATE_KEY is not set: nothing is published"; exit 1; }
          umask 077
          printf '%s\n' "$LAMPOS_UPDATE_KEY" > "$RUNNER_TEMP/update.key"
          build/lamp-update sign --key-file "$RUNNER_TEMP/update.key" \
            --manifest build/manifest.json --out build/lampos-manifest.json
          rm -f "$RUNNER_TEMP/update.key"
      - name: publish the build, then make it the channel's head
        env:
          GH_TOKEN: ${{ github.token }}
          REPO: ${{ github.repository }}
        run: |
          tag="lampos-lamp-voice-${{ github.run_number }}"
          gh release create "$tag" --repo "$REPO" --target "${{ github.sha }}" \
            --title "lampOS lamp-voice build ${{ github.run_number }}" \
            --notes "Built and tested from ${{ github.sha }}. Lamps take it through lampos-lamp-voice (lampOS/docs/updates.md)." \
            --prerelease --latest=false \
            build/lamp-* build/manifest.json build/lampos-manifest.json build/SHA256SUMS
          gh release view lampos-lamp-voice --repo "$REPO" >/dev/null 2>&1 ||
            gh release create lampos-lamp-voice --repo "$REPO" --target "${{ github.sha }}" \
              --title "lampOS lamp-voice: the newest build" \
              --notes "The signed manifest of the newest build of lamp-voice (lampOS/docs/updates.md)." \
              --prerelease --latest=false
          gh release upload lampos-lamp-voice build/lampos-manifest.json --repo "$REPO" --clobber
      - name: keep the newest 30 builds
        env:
          GH_TOKEN: ${{ github.token }}
          REPO: ${{ github.repository }}
        run: |
          gh release list --repo "$REPO" --limit 300 --json tagName -q '.[].tagName' |
            grep -E '^lampos-lamp-voice-[0-9]+$' | sort -t- -k4 -n | head -n -30 |
            xargs -r -n1 gh release delete --repo "$REPO" --cleanup-tag --yes
```

`build/lamp-*` takes the binaries, the units and the product skills together
(each unit asset begins with its lamp's name). Artifacts lose the files' modes, hence the
`chmod`. A re-run of the same run has the same run number, its tag exists
and the job fails without moving the head.

## On the lamp

### One round

`lamp-update once` is one round; `lamp-update serve` one every 15 minutes
(`--every-s`), reading the bus in between. A round
(`crates/update/src/round.rs`):

1. **A switch cut short is put back.** If the journal of a switch is there
   (the program stopped, or the lamp lost power, after the units were
   written), every unit it names that differs from how it was is written
   back and restarted, and that build is refused.
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
   - **its unit**: the build's copy from this lamp's own folder
     (`deploy/<lamp>/<unit>`, the lamp being `--lamp`, or its host name) if
     the build carries one, else the lamp's own. For a part that names its
     build, the build's folder is written in everywhere the unit names its
     folder (so the controls' `--config` follows too). A product part's unit
     names no build and is left as it is, unless the build carries another;
     what moves is its link.
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
   voice that says nothing on the bus cannot be judged and is not switched;
   a voice whose unit is not running hears nobody and is no reason to wait.
   No quiet moment within `--quiet-wait-s` (3600): the new folders stay
   installed beside, nothing is switched, status 4, and the next round tries
   again.
8. **Switch.** The units as they are, each moved link with the build it
   names, and the journal are written; then the new units (a product part's
   only if the build changed it), `systemctl daemon-reload`, and each
   product part's `current` replaced whole: a new link made beside as
   `.current.lamp-update`, renamed over it, the folder synced. Quiet is asked once more: if
   someone spoke meanwhile, the old units go back and nothing is restarted.
   Then the build's product skills are written (planned again first, as the
   lamp may have changed a file while the round waited; each file's bytes
   before are in the journal, written with it). Then each changed unit that was running is restarted: the voice first,
   at the quiet moment itself; the body; then the rest by name. A unit that
   was not running is switched without being started.
9. **Watch.** Every second for `--watch-s` (60): each restarted unit must
   not fail, stop, be in `auto-restart`, be started again by systemd
   (`NRestarts`) or change process (`MainPID`). At the end it must be
   running, and if it was in `lamp/health` before the switch it must be
   there again, not stale, and not in error unless it was before. A part
   that never reported is judged by systemd alone.
10. **Back.** A part not well: every changed unit is written back as it
    was, reloaded, every moved link named its old build again (replaced
    whole, as at the switch), each product skill's file the switch wrote put
    back as it was (or removed, with any folder made for it if still empty;
    a file the lamp changed since it was written is left, and said), and
    those restarted are restarted again in the reverse
    order: the voice last, and, unless the voice is what failed, only
    between two turns (resting, nothing heard or said for 5 s; at once if
    its state cannot be seen). They are watched again, the outcome says how
    the previous build is, and the build is refused from then on.
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
    characters, and within 5 s (`JOURNAL_WITHIN`: killed after, and then no
    line, so a put-back never waits on the journal). "build 812 put back:
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
  hidden files, each text, at most 1 MiB. It is listed in the manifest's
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
- **File by file, as #1913 decides**, from three: the build's before (its
  SHA-256 in `state.json`'s `settings`, the record install keeps, or one
  install was about to write), the build's now, and the lamp's now. Written:
  one still the build's before, and one never there and never recorded.
  Recorded only: one already the build's now. Kept, and said (`kept PATH:
  this lamp's own; the build's is not written`): one the lamp changed, and
  one removed on the lamp after it was recorded. A folder the build's
  bundle doesn't name (a skill the person installed) is never read or
  written but by the guard's copy. A skill the build no longer carries is
  left as it is.
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
`a_round_places_the_builds_product_skills_and_keeps_the_lamps_own`,
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
```

`harness-on` puts Harness on the lamp with Harness's own installer and sets
it at boot (#1676), `harness-off` undoes install's Harness at boot, and only that, and
`check-units` says whether a lamp runs exactly the product's units: both
in `docs/install.md` (step 9, and "Checking a lamp runs the product's
units").

`install` puts lampOS on a blank lamp from the newest build: every product
part through its `current` link, the product units, users and zone, the
product settings and every product skill (`/var/lib/lampOS/skills/`),
enabled and not started ([install.md](install.md)). A round writes only
the units a build carries for this lamp by name, and with
`LAMP_UPDATE_APPLY=on` the product's settings and tmpfiles the build
carries (below, "Settings and tmpfiles"), and its product skills from
`lamp-product-skills` (above); otherwise a product lamp
gets a new product skill or switch when `lamp-update install` is run again. `check` never writes, whatever `--apply` says. `once` and `check` end with
0 (up to date, switched, or only saying), 1 (refused, put back, failed), 3
(the channel cannot be reached), 4 (no quiet moment); a wrong option with 2.
`serve` prints a round's outcome only when it differs from the last. The
defaults: channel `lamp-voice`, key `/etc/lampOS/update/key.pub`, lamp the
host name, bus `LAMP_BUS_ROOT` or `/run/lampos-bus`.

The unit is [`deploy/units/lamp-update.service`](../deploy/units/lamp-update.service):
root (it writes `/opt/lampOS` and `/etc/systemd/system` and restarts
units), `ProtectSystem=strict` with only those two, the bus and its
records writable, `Nice=10` and idle I/O, not part of `lampos.target`.

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

A round never switches lamp-update itself (`lamp-update.service` is never
a part), on either kind of lamp. On a product lamp its unit also runs
through `/opt/lampOS/update/current/`. It moves to its own newer build only
when `lamp-update install` is run again, which installs every product
part's newest build beside the old and moves each link, its own among them,
without restarting anything; the new updater runs from its next start.
Replacing the link under a running updater is safe on Linux: the program
already running stays mapped.

### lamp-update switching itself (`LAMP_UPDATE_SELF`, off)

With `LAMP_UPDATE_SELF=on` in `lamp-update.service`'s environment, a round
also moves lamp-update to its build's own binary (`own.rs`, #1680). Off, or
absent, a round is exactly as above: no `self.json` is ever written, so the
confirmation and lamp-update-back find nothing to do. The product unit does
not set it yet.

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
   `lamp-update` at `/usr/local/lib/lampos/lamp-update-back`. Only `install`
   writes it, and it is outside `lamp-update.service`'s `ReadWritePaths`, so
   no round can change it. `lamp-update-back.service` runs it as
   `lamp-update-back back`. It is started by `lamp-update.service`'s
   `OnFailure=` and every 2 min by `lamp-update-back.timer`. When
   `self.json` is there and lamp-update is not `active`/`running`, or 10 min
   (`own::DEADLINE`) have passed since the link moved, it points
   `update/current` at the old build again, adds the new binary's SHA-256 to
   `self-refused` (never switched to again), removes `self.json`, and runs
   `systemctl reset-failed` and `restart --no-block lamp-update`. Otherwise it
   ends, having changed nothing. The unit has
   `ConditionPathExists=/usr/local/lib/lampos/lamp-update-back`: on a lamp
   not installed since #1680 (a round never adds a unit: it changes only
   units already on the lamp, and never `lamp-update.service`), the timer
   and `OnFailure=` find the condition unmet and systemd skips the start,
   which is not a failure. lamp-health reads parts' reports on the bus, not
   systemd's unit states, so a skipped or failed lamp-update-back is no part
   in error either way.

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

### Models in the manifest (`LAMP_UPDATE_MODELS`, off)

A build can carry the models its parts read (`models.rs`, #1680): a
`models` list in the signed manifest, each entry in
`deploy/product/models.json`'s form (`path`, `url`, `bytes`, `sha256`, and
an optional `archive` for a file inside a pinned `.tgz`).
`lamp-update manifest --models deploy/product/models.json` writes it and
raises `min_updater` to 2 (`manifest::MODELS_UPDATER`), so an updater from
before the field climbs first. `UPDATER` is 2. A manifest without models
doesn't write the field and is byte for byte what it was. A model's path
must be under `/opt/lampOS/`, neither the store nor in it, nor in a
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

### Settings and tmpfiles (`LAMP_UPDATE_APPLY`, off)

A build can carry the product's settings and tmpfiles (`apply.rs`,
#1680): a `settings` list in the signed manifest, each entry `path`,
`text` (inline), `sha256` and `mode` (`0644` or `0600`), or `path` and
`removed: true`. `lamp-update manifest --settings on` writes install's own
(`install::SETTINGS` and `lampos.conf`, `install::product_settings`), but
for the product skills' files under `/var/lib/lampOS/skills/`: those are
`lamp-product-skills`', placed by the round and judged by the skills
guard, so one file never has two writers at the switch. It also raises `min_updater` to 3 (`manifest::SETTINGS_UPDATER`). `UPDATER` is 3.
A manifest without settings doesn't write the field. A path must be
`/etc/lampOS/records.json`, `/etc/tmpfiles.d/lampos.conf`, or a file in
plain parts under `/etc/lampOS/expression/`, `/etc/lampOS/controls/` or
`/var/lib/lampOS/skills/`; any other refuses the build. So a build never
writes a key, the voice's service, the zone, setup's answers,
`body/lamp.env`, `person/name` or the update key: those are setup's, the
person's or the lamp's. At most 128 settings and 160 KB of text.

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
- **Root, small.** At the switch, after the journal and before any unit,
  link or model moves, the round stages `apply/request.json` and the
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
  round takes those into `state.json` and removes the request. Its unit:
  `ProtectSystem=strict`, writable only `/etc` (for the image's
  `/etc/asound.conf`, replaced by a rename beside it; what is written
  there is only what the path rules allow), `/var/lib/lampOS`, the records,
  `/run` (for the tmpfiles) and `/usr/local/libexec`, the update key
  read-only, no network.
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

Off, or absent: a build's settings aren't written, and the round says
which (`build N's settings are not written (...): LAMP_UPDATE_APPLY=on is
not set`). The rest of the build is taken as today. Users and groups
aren't carried: a later step, when a build needs a new one.

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
  `lamp-cmedia-speaker.service`, `respeaker-lite-softvol.service`,
  logrotate's `zz-lamp-ramlog.conf` and its `lamp-ramlog-postrotate`,
  journald's `zz-lamp-journal.conf`, and the udev rules `90-respeaker-lite`,
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
  nothing. `min_updater` is then at least 4 (`manifest::IMAGE_UPDATER`);
  `UPDATER` is 4.
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
  moment within `--quiet-wait-s`, nothing is written or restarted, the
  round ends `not-quiet`, and the next round tries again. A lamp's own
  `asound.conf`, kept, restarts nothing.
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
  whole however much it prints.
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
| tasks | the store's container, `/var/lib/lampos-tasks/store/tasks.sqlite3` | SQLite | `user_version`, must be 1 | an extra table, index or trigger is refused (`tasks/src/store.rs:180-189`) | n/a | **refuses to start** on any other `user_version` (`store.rs:164-168`, through `runtime.rs:1776` to exit 1). Only the JSON snapshot inside is set aside (below) |
| tasks | task state in the same file (`state.snapshot`) | JSON | `schema` 2 (oldest 1); higher is **set aside** as `tasks.sqlite3.newer-<schema>-<s>`, with a journal and health line (`engine.rs:1008-1025`, `1948-2000`) | read on `State`, `Task`, `Dispatch` (#1512); **refused** on `Target`, `Launch`, `Observation`, `LaunchOutcome` (`harness.rs:108,119,127,183`) | **refused**: e.g. `dispatch.state: "later"` at the same schema | a newer `schema` is set aside (low). An unknown value, or a field on a strict type, **without** a schema bump **refuses to start** (`engine.rs:390`). The bump is a rule in a comment (`engine.rs:58-66`), not a check |
| schedule | the store's container, `/var/lib/lampos-schedule/store/schedule.sqlite3` | SQLite | `user_version`, must be 1 | n/a | n/a | **refuses to start** (`schedule/src/store.rs:165-169`, `runtime.rs:477`, exit 1) |
| schedule | reminders and timers in the same file | JSON | `schema` 1; higher is **set aside** as `schedule.sqlite3.newer-…`, its identity kept (`engine.rs:349-361`, `416-419`) | read | **refused**: `SavedRepeat`, `Weekday`, `Item` (`engine/stored.rs:69`); e.g. a repeat `kind: "monthly"` | an unknown value at schema 1 **refuses to start** (`stored.rs:36` → `engine.rs:345` → exit). The receipt alone is dropped with a line (`stored.rs:18-22`) |
| schedule | the zone setup and install write, `/etc/lampOS/zone.env` (`LAMP_ZONE`) | env | none | n/a | a zone this build's tzdb doesn't know | **refuses to start** (`engine.rs:301-306`, `320-324`; `validate_zone`, `time.rs:225`). Skew between tzdb releases, not a store format |
| update | lamp-update's own switch, `/var/lib/lampos-update/self.json` | JSON | `version` 1, never checked | read, dropped | n/a | `serve` confirms it before any round (`update/src/main.rs:668-705`). One it can't parse (`own.rs:71`) is "not confirmed" every 30 s for ever (`main.rs:698`): **no round ever runs**, the service stays active, `OnFailure=` never fires. `lamp-update-back back` exits 1 on the same file every timer run. Only with `LAMP_UPDATE_SELF=on` |
| controls | `/etc/lampOS/controls/controls.json`, `controls-pins.json` (update's settings write them, `install.rs:339`) | JSON | none | refused (`controls/src/engine.rs:24`, `touch_pins.rs:74`, `pins.rs:45`) | refused | **refuses to start** (`main.rs:269-276`, `597`, `822-826`). Softened: apply restores the files with the build on going back (`update/src/apply.rs:14-20`); a person's own edit is kept and not restored |
| body | poses `lamp-body-measure` writes once, `/etc/lampOS/body/lamp.env` (`BODY_POSES`, passed as arguments) | env | none | a flag it doesn't know is refused (`body/src/bin/lamp-body.rs:391`) | an unknown on/off value refused (`lamp-body.rs:270-359`) | **refuses to start** (`lamp-body.rs:781-795`, `StartLimitBurst=3`), if a newer measure's flag meets an older body |

### Medium

| Part | Store, path | Format | Version | Field | Variant | At start, on a record it can't read |
|---|---|---|---|---|---|---|
| tasks, schedule | the newer build's optional fields | JSON | — | read and dropped | — | tasks rewrites its state as it opens, so they go at once; schedule at its next save (`schedule/src/engine.rs:409`). As designed in #1505, but the data is lost on a roll forward |
| schedule (run by the voice) | reminder delivery ledger, `/var/lib/lampOS/voice/schedule-delivery/schedule.sqlite3` | SQLite + JSON | `user_version` 1; `schema` 1, higher **set aside** (`schedule/src/delivery.rs:562-605`) | top level read (`delivery.rs:397`); nested types refused (`delivery.rs:45-185`) | refused (`delivery.rs:128,164`) | a nested field or a new value without a bump, or a higher `user_version`: the reminder child exits, the voice logs "Schedule consumer failed" and runs on (`voice/src/reminders.rs:319-326`). Reminders stay unsaid on every restart; the data is kept |
| memory | old journal and kept files, imported once, `/var/lib/lampos-memory/<person>/journal-*.jsonl`, `kept.jsonl` | JSONL | none | read, dropped (`Exchange`, `Kept` take no `deny_unknown_fields`) | — | a line that doesn't read is only counted, then the file is deleted (`days.rs:228-295`, `308-324`): **dropped**. Not changed by #1680: an older build's files, not a newer one's, and a line that doesn't read is a cut or broken one; keeping it would keep a journal past `keep_days` and past a forget |
| memory | a tidy job's input and result (`jobs` table) | JSON in TEXT | none | refused (`tidy.rs:52-93`; `#[should_panic]` test `tidy.rs:395`) | refused (`Category`, `tidy.rs:44`) | not at start: that tidy fails "stored tidy input is invalid" (`days.rs:976`, `1127`, `1147`). The job clears a day after it was prepared, as every job does: `maintain` runs at start and before every request (`days.rs:344-347`, `days_protocol.rs:245`); until then it counts toward the 8 pending (`MAX_PENDING`) |
| update | `/var/lib/lampos-update/state.json` | JSON | none | read, dropped (`records.rs:50`) | n/a | read when a round acts (`round.rs:503` → `records.rs:270`). Unparseable is set aside as `state.json.corrupt-<ms>` with a line (low); a readable newer one loses its fields on the next save (`records.rs:309`). `peek_state` falls back to empty silently (`records.rs:302`) |
| update | `/var/lib/lampos-update/journal.json`, a switch under way | JSON | none | read, dropped (`records.rs:104-149`) | n/a | one it can't parse: every round is `Failed("the journal cannot be read")` (`round.rs:498`), never set aside, so updates stop until someone removes it. A newer field about what to put back is not put back |
| eyes | forgets waiting, `/var/lib/lampos-eyes/set-aside/forgets-pending` | JSON | none | read | n/a | set aside as `forgets-pending.unreadable-<unix s>` with a line, never counted as records, and forgetting goes on; merged back at a start once it can be read (`who.rs` `pending`, #1680) |
| skills | skills the person turned off, `/var/lib/lampos-skills-state/off` (cloud reads it too, `cloud/src/skills.rs:81`) | text lines | none | n/a | n/a | a line that isn't a plain name is skipped with no line (`skills/src/lib.rs:3158-3170`) and gone at the next rewrite (`lib.rs:3178-3190`) |
| cloud | login a ping rotated, `/var/lib/lampos-cloud/rotated.json` | JSON | none | read (`cloud/src/account.rs:14`) | n/a | unreadable falls back to setup's account silently (`main.rs:124-128`); the next rotation overwrites it whole |
| cloud | queue state, `/var/lib/lampos-cloud/reports/state.json` | JSON | none | read; no `serde(default)`, so a retyped or missing field fails | n/a | **starts empty silently** (`reports.rs:791-793`): `seq` back to 0, the next report reuses seq 1 (tested) |
| cloud | queued reports, `/var/lib/lampos-cloud/reports/<seq>.json` | JSON | `schema` `lamp-report/1`, written, never checked | read as `Value` | a `kind` other than `crash` or `stopped` | **deleted with no line** (`reports.rs:821-840`, tested with `"kind": "panic"`) |
| records | voice sessions, `/var/lib/lampOS/voice/sessions` | folders | n/a | an extra file in a session is refused (`records/src/store.rs:22-28`, `394-396`) | n/a | that session is kept and reported and the run exits 1 (`run.rs:253-261`); a newer voice's sessions are never pruned |
| eyes | faces from photos, `<people>.from-photos` (only with `--photo-dir`, not in the product unit) | JSON | `version` 1; any other **deletes the file** (`eyes/src/photo.rs:195-209`) | read | n/a | dropped with a line. Off in the product |
| voice | person's light, `/var/lib/lampOS/light/person.json` (`LAMP_LIGHT_KEEP`, not set in the product) | JSON | none | read (`msgs/src/lib.rs:597`) | an unknown colour filtered (`ring_light.rs:170-175`) | dropped silently, overwritten at the next change. Off |
| voice | made skills index, `/var/lib/lampOS/voice/made-skills/made.json` (`LAMP_MAKE_SKILL`, off) | JSON | none | `Value` | n/a | unreadable reads as `[]` (`made_skill.rs:510-515`) and is overwritten (`547-575`). Off |
| tasks | mark of machine-written targets, `targets.json.from-machines` (only `--targets-from-machines`, off) | text | none | n/a | n/a | may overwrite a newer build's targets file whose hash is marked (`runtime.rs:1186-1220`). Off |

### Low

| Part | Store, path | Format | Version | Field | Variant | At start, on a record it can't read |
|---|---|---|---|---|---|---|
| memory | memory across days, `/var/lib/lampos-memory/memory-days.sqlite3` | SQLite | `user_version` 1; higher is **set aside** whole as `memory-days.sqlite3.newer-db<version>-<s>` (its sidecars too, if any outlived the close), never changed, with a journal line naming only the file, and the part starts with an empty store (`memory/src/days.rs:173-180`, `1266-1289`); health warns with `set_aside` on every start while the file is there (`days.rs:1331-1352`, `application.rs:199`, `health.rs`; #1680). Read before any setting is applied, so `journal_mode` never rewrites it. No code reads it back yet: a roll forward opens `memory-days.sqlite3`; whichever build brings `user_version` 2 reads `newer-db2-*` back | columns read by name; `CREATE … IF NOT EXISTS` | n/a | the part starts and serves. The kept file's records are not read or searched here. Every forget (`forget`, `forget_last`, `forget_all`) removes every set-aside file whole, sidecars too, and health's warning clears; so does the usual pass once the file's `<s>` stamp is `keep_days` old (`days.rs:349-353`) (`days.rs:605-609`, `646`, `686`, `690-697`, `1294-1328`, `server.rs:262`; #2067); the 5 s reset wipes the whole folder (`setup/fixtures/reset-plan-product.json:5`) |
| memory | `category` of an entry | SQLite text | — | — | an unknown one reads as no category (`category_from`, `days.rs:1421-1431`): kept as stored, never read as another, given to a tidy as a previous observation, chosen as one, or superseded (`days.rs:1025`, `1067`; #1680) | the entry is kept and read; in a session note it is a plain fact |
| tasks | targets, `/var/lib/lampos-tasks/targets.json` | JSON | `version` 1 | refused, read as "newer" | refused, read as "newer" (`runtime.rs:255-279`) | runs with hand-overs off, journal and health say so (`main.rs:132-150`) |
| tasks | computer names, `targets.names.json` (only `--set-up-later`) | JSON | `version` 1 | refused (`runtime.rs:992`) | n/a | no names, one line (`runtime.rs:1275-1283`) |
| eyes | known faces, `/var/lib/lampos-eyes/people.json` | JSON | `version` 1; higher sets the whole file aside in `set-aside/` (`eyes/src/who.rs:426-437`, `1153-1168`) | read, dropped on save (`who.rs:295`, `309`) | n/a | a person it can't read is set aside alone, one count-only line (`who.rs:441-478`). Nothing reads `set-aside/` back, so a version 2 leaves the lamp knowing nobody |
| eyes | its own set-aside records, `set-aside/people-*.json` | JSON | — | `Value` | n/a | not loaded; counted on health (`who.rs:1222-1228`) |
| skills | installed skills, `/var/lib/lampOS/skills/<name>/` (`skill.toml`, `SKILL.md`) | TOML + Markdown | the skill's own semver only | refused (`skills/src/lib.rs:125`, ~20 nested types) | refused (`lib.rs:663-664`) | that skill is "not loaded: …" and the rest load (`main.rs:687-690`, `lib.rs:2828-2836`). Its functions are gone, core skills included |
| voice | Live provider settings (install writes them), `/etc/lampOS/voice/provider.json` | JSON | none | read past, named once in the journal: `lamp-voice: <file>: fields this build does not read, ignored: …` (`voice/src/config.rs:70`, `90`, #1680); a field named as a key's (`*_key`, `token`, `*_secret`, `*_password`) is refused by name, since the file never holds a key (`config.rs:80`) | refused: a known field's bad value is a broken config, not a newer one (`Authentication`, `ReconnectContext`, `SilentResponse`, `config.rs:120-153`) | a file that doesn't read, or a known field with a bad value, still **refuses to start**: the voice can't run without it. The line names the file and serde's reason (`config.rs:226`) |
| voice | second route's settings, `/etc/lampOS/voice/second.json` | JSON | none | read past and named, as `provider.json` (`second_config.rs:79`, `319`; line `lamp-voice second: …`), a key's field refused by name | `tts_provider` is a string checked against a list (`second_config.rs:338-347`), still refused | as `provider.json`: unreadable or a bad known value refuses to start, naming the file (`second_config.rs:310-336`) |
| voice | conversation thread, `/run/lampos-voice/thread.json` (kept across a restart) | JSON | none | refused (`thread.rs:153`) | n/a | set aside as `.unreadable-<ms>` with a line (`thread.rs:368-372`, `917-940`) |
| voice | news told, tasks typed, `/var/lib/lampOS/voice/tasks-told`, `tasks-typed` | text lines | none | n/a | n/a | read as empty, no line (`task_news.rs:419-423`): news may be told again |
| voice | relay key `/etc/lampOS/voice/credential`; person's name `/etc/lampOS/person/name` | text | n/a | n/a | n/a | key read raw (`config.rs:400`); an unfit name is no name, with a line (`person.rs`) |
| voice | sessions and first output, `/var/lib/lampOS/voice/sessions/…`, `first-output/*.jsonl` | JSONL | first output `schema` 1 | — | — | written only, never read back at start |
| cloud | setup's account, `/etc/lampOS/setup/backend.json` | JSON | none | read (`account.rs:14`, all `serde(default)`) | n/a | an extra field reads. A changed type or bad JSON refuses to start, exit 2 (`main.rs:120-123`) |
| cloud | report choices, `/var/lib/lampos-cloud/reports.json`, `/etc/lampOS/setup/reports.json` | JSON | `asked`; above 1 reads as "no" (`reports.rs:712-713`) | `Value` | an unknown `by` is none (`reports.rs:669`) | read per send; unreadable is "no" |
| setup | serve config (only `lamp-update install --setup on` writes it, `update/src/install.rs:375-384`), `/etc/lampOS/setup/serve.json` | JSON | none | read, dropped with a line (`setup/src/serve.rs`, `read`) | n/a | an extra field is dropped and named in the journal; a bad known value or bad JSON still refuses to start, exit 2. Not in update's settings list, so a put-back never restores it |
| setup | reset plan, `/etc/lampOS/setup/reset-plan.json` | JSON | none | **refused**, except `keep_back`, whose every entry is dropped with a line and its path wiped (`setup/src/reset.rs`, `read`) | a `wipe` entry that isn't a plain path's text **refuses the reset**; a newer plan's extra plain path is wiped (still checked against `PROTECTED`) | all or nothing: a refused plan journals nothing, keeps the marker and wipes nothing; `watch-reset` keeps watching and tells the person who holds the button "Nothing was wiped. Update me, then reset again." (say service, network only), the other commands exit 2. A bad known value or bad JSON is refused as before. Not restored by a put-back |
| setup | done marker `/var/lib/lampos-setup/done`; reset journal `reset.json` | file / JSON | `version` written, never read | n/a | n/a | existence only (`serve.rs:702-703`, `reset.rs:333`) |
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
| expression | `/etc/lampOS/expression/expression.json` (update's settings, `install.rs:335`) | JSON | `version` must be 1; another is refused | **read**, named once in the journal by path (`catalogue.rs`, `Catalogue::from_json_naming`; `main.rs`, #1680) | refused (`Trigger`, `catalogue.rs:13`) | a newer catalogue's unknown fields, at any depth, are read past and named ("fields this build does not read, ignored: …"); a bad known value, a new trigger or another `version` still refuses to start. Tests: `crates/expression/tests/forward.rs` |

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
3. **tasks**: a higher SQLite `user_version` sets the file aside, as a
   higher `schema` does; an unknown stored value without a bump is set
   aside, not refused.
4. **schedule**: the same for its store and the delivery ledger, and an
   unknown zone falls back to the system's with a line.
5. **memory**: a higher `user_version` sets the database aside and starts
   a new one, with a line and a health warning; an unknown category is
   kept, not read as `Thread` (both done, #1680). Old journal lines that
   don't read stay dropped: they are an older build's (the row above).
6. **update**: `self.json` it can't read is set aside and the rounds go
   on; so is `journal.json`, with a line.
7. **controls**, **expression** (#2065), **records**, **body**: read their files
   one release forward (one PR each).
8. The medium rows after: **cloud** (reports deleted and `state.json`
   reset silently), **eyes** (`forgets-pending`: done, set aside with a
   line), **skills** (`off`). The eyes' `.from-photos` stays as it is: a
   file it can't read, a later version's included, is deleted on purpose,
   since unconfirmed prints out of the 30 days' reach are no one's to keep
   (the reviewer on #1968).

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
- **Unit files that change between builds.** They are carried from
  `deploy/<lamp>/` of the build, which the first builder keeps by hand, and
  LAMP.md and the units there already disagree (LAMP.md says voice
  `d628ed1d`; `lamp-voice.service` names `c8ccd3dd...`). The build is
  written into a carried unit, so a stale build there does no harm; a stale
  option or `Environment=` line does. A drop-in (`lamp-voice.service.d/`)
  is not read and still overrides. A unit the build carries for a part not
  installed is not added, and a part's new unit is never enabled: adding
  a part to a lamp stays the first builder's.
- **The disk holding two copies.** lamp-4ace's board said 14% free on
  2026-10-14 (health.md); the disk's size is not in the repository. The
  check counts every new file whole, and `/opt/lampOS/releases/` already
  holds every voice installed by hand, which lamp-update never removes.
- **The quiet moment.** It needs the voice on the bus (`LAMP_BUS=on`, as on
  lamp-4ace). In an office the 45 s may come only at night. The check is
  made again right after the units are written, but someone who starts
  speaking in the second before the restart is cut off: the voice has no
  pause that closes it to new turns yet.
- **`ProtectSystem=strict` and systemd's own calls.** `daemon-reload` and
  `restart` from inside the unit's sandbox were not tried on a lamp.
- **The body's restart folds the lamp** (body.md); at a quiet moment it
  may still be standing from the last conversation.

## Not shown

- Anything on a lamp: no lamp-update has run on any lamp, and no number
  above is a lamp's.
- CI publishing: the job above has not run; no key exists.
- A power cut during a switch, on the lamp's storage: the journal is
  tested with the files a cut would leave, on the Mac.
- lamp-update switching itself on a lamp (`LAMP_UPDATE_SELF`): tested on
  the Mac only (`tests/round.rs`, #1680), with a new binary that exits at
  once run for real and lamp-update-back called as its unit would call it.
  The `exec`, `OnFailure=`, the timer and the `systemctl` calls haven't run
  under systemd. The rehearsal on lamp-4ace is the first builder's.
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
  `systemctl restart` of a one-shot haven't run on a lamp. No published
  build carries settings yet.
- The keeper,
  `lampos.target`, timers, models and libraries a build does not carry,
  configuration under `/etc/lampOS` (but for `second.json`, above), and a
  part new to a lamp.
- Two parts of one build that must change together across a failed
  switch: everything changed is put back together, but a part that stores
  data in a new form (memory, schedule) before being put back is not
  undone.
- The Linux build: built and tested natively on Linux ARM64 by CI on this
  branch, not run there by hand.
