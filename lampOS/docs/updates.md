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
  checked against CI's `SHA256SUMS`) and every unit of every lamp's own
  folder (`deploy/<lamp>/`, a folder with a `LAMP.md`), signs it, and
  publishes the binaries, the units (as `<lamp>.<unit>`), the manifest and
  the signed envelope as release `lampos-lamp-voice-<run number>`: a
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
          sparse-checkout: lampOS/deploy
      - uses: actions/download-artifact@v4
        with:
          name: lampos-arm64-${{ github.sha }}
          path: build
      - name: manifest
        run: |
          chmod +x build/lamp-*
          build/lamp-update manifest --binaries build --deploy lampOS/deploy \
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

`build/lamp-*` takes the binaries and the units together (each unit asset
begins with its lamp's name). Artifacts lose the files' modes, hence the
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
3. **The build.** The envelope's key fingerprint must be the lamp's, its
   signature must match the manifest's bytes, the manifest must be schema 1
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
   Then each changed unit that was running is restarted: the voice first,
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
    whole, as at the switch), and those restarted are restarted again in the reverse
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
    switch, are removed: the lamp keeps its build and the one before. A folder it did
    not make (every one installed by hand) is never removed.

### The commands

```text
lamp-update check|once|serve --channel-url URL [--channel NAME] [--key-file PATH] [--lamp NAME]
                             [--apply on|off] [--every-s N] [--quiet-wait-s N] [--watch-s N]
                             [--keep-free-mb N] [--root DIR] [--bus-root DIR]
lamp-update install --channel-url URL [--channel NAME] [--key-file PATH] [--setup on|off]
lamp-update harness-off
lamp-update check-units --cat FILE [--product DIR]
lamp-update manifest --binaries DIR --deploy DIR --channel NAME --sequence N --commit SHA --files-url URL
lamp-update sign --key-file PRIVATE --manifest FILE --out FILE
lamp-update public-key --key-file PRIVATE
```

`harness-off` undoes install's Harness at boot, and only that, and
`check-units` says whether a lamp runs exactly the product's units: both
in `docs/install.md` (step 9, and "Checking a lamp runs the product's
units").

`install` puts lampOS on a blank lamp from the newest build: every product
part through its `current` link, the product units, users and zone, the
product settings and every product skill (`/var/lib/lampOS/skills/`),
enabled and not started ([install.md](install.md)). A round writes only
the units a build carries for this lamp by name, never the product's
settings or skills: a product lamp gets a new product skill or switch when
`lamp-update install` is run again. `check` never writes, whatever `--apply` says. `once` and `check` end with
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
a switch cut short is put back by the link too), `previous/` (the units
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

### lamp-update's own build on a product lamp

A round never switches lamp-update itself (`lamp-update.service` is never
a part), on either kind of lamp. On a product lamp its unit also runs
through `/opt/lampOS/update/current/`. It moves to its own newer build only
when `lamp-update install` is run again, which installs every product
part's newest build beside the old and moves each link, its own among them,
without restarting anything; the new updater runs from its next start.
Replacing the link under a running updater is safe on Linux: the program
already running stays mapped. A round that switches the updater itself, at
the end of a switch and watched like a part, is not built.

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
   cargo run -q -p lamp-update -- manifest --binaries ~/lampos-channel --deploy deploy \
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
- lamp-update updating itself (its own unit is never a part), the keeper,
  `lampos.target`, timers, models and libraries a build does not carry,
  configuration under `/etc/lampOS`, and a part new to a lamp.
- Two parts of one build that must change together across a failed
  switch: everything changed is put back together, but a part that stores
  data in a new form (memory, schedule) before being put back is not
  undone.
- The Linux build: built and tested natively on Linux ARM64 by CI on this
  branch, not run there by hand.
