# Settings writer refresh: the GOOD/BAD rehearsal on a 7071 lamp

The procedure for #3689 item 1 (plan A, #4178), as #3689's "Done when" asks:
each piece rehearsed GOOD and BAD on a lamp imaged with build 7071. Nothing
here has been run. Second owns no lamp. Whoever runs it books the slot
(Fifth), takes the lamp's lease and posts PLAYING/QUIET as usual, and posts
the results on #3689.

What it shows: a round refreshes the fixed settings writer
(`/usr/local/lib/lampos/lamp-update-back`) from the lamp-update that runs,
keeps the old copy as `.prev`, and then either proves the new writer and
removes `.prev`, or puts `.prev` back. And no self-switch begins while the
writer is unproved.

## The lamp and its channel

- A lamp imaged with 7071, set up, product-installed (its own unit starts
  `update/current/lamp-update`), with `lamp-update-back.timer` armed. Not
  lamp-4ace, whose units are its own.
- Before anything, record and keep privately:
  - the SHA-256 of `/usr/local/lib/lampos/lamp-update-back`, which should be
    7071's lamp-update;
  - the `update/current` target;
  - the installed sequence in `/var/lib/lampos-update/state.json`;
  - every file in `/var/lib/lampos-update/` whose name starts `self-` or
    `writer-`.

  There should be no `lamp-update-back.prev` and no
  `/etc/systemd/system/lamp-update-writer.service`.
- Follow a test channel signed with a throwaway key, never the production
  head or key. The product carrier's acceptance
  ([product-carrier-20261025](../product-carrier-20261025/README.md),
  "Inputs and scope" to "Signed builds") already sets one up; take its
  steps for the key, the publisher CLI and the upload, and its cleanup.

## Builds

All from the native ARM64 artifact of one lamp-voice commit that carries
#4178, never a Mac binary:

- **G (GOOD):** that commit's build, as the publisher makes it (`min_updater`
  8). The lamp climbs from updater 5 to it, confirms, and then refreshes its
  writer from it.
- **B1 (a writer whose `back` fails):** the same commit with one throwaway
  change on an unmerged branch: `lamp-update back` exits 1 at its start.
  Its `writer-probe` still answers, so it is placed. Never merged, never on
  the production channel.
- **S (a settings change):** G's binaries with one setting in the
  allowlist changed, for example `/etc/lampOS/records.json`, at a sequence
  above G.

## GOOD: refresh, proof, `.prev` removed

1. **The climb.** The lamp follows the channel with G. Its journal shows the
   climb: the build refused for updater 8, lamp-update switched alone, then
   confirmed. `self.json` is gone afterwards.
2. **The refresh.** The next round says `the settings writer is now build
   <G's lamp-update, 12 hex>`. Then check:
   - `lamp-update-back`'s SHA-256 is G's lamp-update.
   - `lamp-update-back.prev`'s is 7071's, with mode 0755.
   - `/etc/systemd/system/lamp-update-writer.service` equals `writer_unit::TEXT`.
   - `writer-refresh.done` names G's build with `before` 7071's and `proven:
     false`.
   - No `writer-refresh.request` is left.
3. **No self-switch while unproved.** Optional: publish a G′ with a changed
   lamp-update before step 4. A round says `lamp-update's own switch waits:
   the settings writer was refreshed and has not proved itself yet`, and
   writes no `self.json`.
4. **The proof.** Within about 2 minutes, `lamp-update-back.timer` runs the
   new writer's `back`, which ends well and writes `writer-back.ok` naming
   G's lamp-update. The next round says `… has proved itself (its back ended
   well): the copy before is removed`. Then `.prev` is gone and `done` says
   `proven: true`.
5. **The new writer writes.** Publish S. Its round applies the setting
   through `lamp-update-apply.service`, now the new writer, and the file has
   S's bytes.

## BAD 1: the new writer's `back` fails

Start again from a 7071 lamp, or put the writer back by hand (below), and
follow the channel with B1.

- After the climb and the refresh, `lamp-update-back.service` fails, since B1's
  `back` exits 1.
- The next round says `the settings writer refreshed is put back (its back
  failed): the copy before is the writer again`. Then check:
  - `lamp-update-back` is 7071's again.
  - `.prev` is gone.
  - `writer-refresh.done` has `put_back` naming B1's lamp-update.
- A later round asks nothing of the writer for that build: no
  `lamp-update-writer.service` start in the journal.

## BAD 3: a refresh after a put-back is unproved too

From the BAD 1 lamp, with B1 put back and 7071's writer again. This is the
case of Reviewer-two's P2 on #4178: a refresh carries the last put-back
forward in `writer-refresh.done`, and that must not make it count as
proved.

- Stop `lamp-update-back.timer` for the step, which a person notes and
  restores, so the new writer's `back` doesn't prove it before the checks.
- Follow the channel with G. lamp-update switches to G and confirms, and
  the next round says `the settings writer is now build <G's lamp-update, 12
  hex>`. Then check:
  - `lamp-update-back` is G's lamp-update, and `.prev` is 7071's.
  - `writer-refresh.done` names G's build with `before` 7071's,
    `proven: false`, and `put_back` still naming B1's lamp-update.
- Publish G′ (as in GOOD step 3). A round says `lamp-update's own switch
  waits: the settings writer was refreshed and has not proved itself yet`,
  and writes no `self.json`.
- Start the timer again. Its `back` ends well, the next round says `… has
  proved itself …`, `.prev` is gone and `done` says `proven: true`. Then
  G′'s switch may begin.

## BAD 2: the new writer's first `apply` fails

From the GOOD lamp after step 2, before the proof (or with the timer
stopped for the step, which a person notes and restores):

- Make the setting S changes unwritable for the writer. `chattr +i` on that
  file is enough, and is undone afterwards.
- Publish S. Its `apply` fails, and the build is put back: the settings'
  undo runs first, by the writer that made it.
- Then the round says `… is put back (its first apply failed) …`, and
  `lamp-update-back` is 7071's again.
- `chattr -i` the file. A later build is not asked to make that lamp-update
  the writer again.

## A HOLD, not a retry

Any step whose result differs is a HOLD. Keep the first evidence: the
journal from the cursor, and the records' files above. Then stop. Don't
run it again to get a pass.

## Putting the lamp back

- Write 7071's lamp-update back over `/usr/local/lib/lampos/lamp-update-back`,
  as root, by hand.
- Remove `lamp-update-back.prev` and `lamp-update-writer.service`, and run
  `systemctl daemon-reload`.
- Remove the `writer-*` records.
- Restore the channel, the key and the installed records from the private
  backup, as the carrier's acceptance does.
- Verify each against the hashes recorded at the start, then release the
  lease.
