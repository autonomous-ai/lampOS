# The NPU service (`lamp-npu`)

The lamp's NPU has one owner: `lamp-npu` (`crates/npu`), the only process that
opens `/dev/vipcore`. Its first op is `pose`; the eyes' face models (YuNet,
SFace) and later app models are meant to move onto the same service, so that
two processes never fight over the NPU. Off unless `LAMP_NPU=on`
(switches.md). Built for #7415 on the first builder's ruling of 16:48 on
19 Nov 2026 (`pose` first, App Only builds it, one owner of the NPU, `pose` on
the camera grant, the model kept out of the repository) and the owner's
decision on the model's delivery, with the first builder's four rules of 16:55.

## What it does

- **`pose`.** One JPEG in, one person's 17 COCO keypoints out, each `x`, `y`
  in the picture's own pixels and a `score`, with the person's score, the
  model's id and the time each step took. Asked through lamp-control by a
  computer or an app that holds the `camera` grant (control.md, "Poses").
- **The model is not in the image.** The service fetches the board's model
  from the Store, checks it and keeps it ("When the model is fetched").
- **The NPU is opened when asked.** The model is loaded at the first `pose`
  after the service starts, or after it was let go, and let go after 3
  minutes with no ask (`--idle-s`). A load is about the model's size read
  from the disk and checked again.
- **It eases off with the board's heat.** It reads `board/heat`
  (`lamp_msgs::BoardHeat`, health.md): one client may ask 15 times a second
  at `ok`, 5 at `warm` and once at `hot` (`rate`, with `retry_ms`), and all
  clients together 20, 6 and 2 times (`busy`). At about 20 ms a run the NPU
  stays more than half free, which is the eyes' share when they move here.
- **Never while the lamp is private.** lamp-control refuses `pose` while its
  watch says the lamp is private, and again if it turned private before the
  answer came. The service itself also refuses while the bus's
  `voice/privacy` lease says the microphone is not permitted or the switch is
  muted, or the bus can't be read, and drops an answer if that changed during
  the run.
- **Nothing is kept of a picture.** The JPEG and the keypoints live only for
  the ask; the journal says the model's loads, fetches and failures, never a
  picture or a pose.

## Ops and answers

The socket is `/run/lampos-npu/npu.sock`, in the unit's own runtime folder
(0700, `lampos`'s): only lamp-control and the lamp's own parts reach it, and an
app only through lamp-control's grant check. One JSON line asked, one
answered (`crates/npu/src/serve.rs`).

- `{"op":"pose","client":"c_…","jpeg":"<base64>"}` →
  `{"ok":true,"pose":{"score","keypoints":[{"x","y","score"}…17]}|null,
  "width","height","model","ms":{"decode","npu","post","total"}}`, or
  `{"ok":false,"code","why","retry_ms"?}`, `code` one of `private`, `rate`,
  `busy`, `preparing`, `unavailable`, `bad_request`.
- `{"op":"status"}` → the ops it answers, its board, and for `pose` the
  model's state (`absent`, `preparing`, `ready`, `unavailable` with why),
  whether it is loaded, and the kept model's id, head and licence.

`lamp-npu pose FILE.jpg` and `lamp-npu status` ask a running service from the
lamp's own shell, through the same socket.

## Models and heads

A model is a network binary graph (`.nb`) converted for the NPU with its
normalisation baked in, so it takes one RGB frame of bytes, `n, h, w, 3` or
`n, 3, h, w`, and gives its raw outputs. A region of the picture is scaled
into the model's frame, bilinear and black past the picture's edges, as
OpenCV's `warpAffine` makes it, and the keypoints are mapped back
(`crates/npu/src/image.rs`). A client's first frame is the whole picture,
centred on black to the frame's shape (a 640x480 picture in MoveNet's square
has black above and below). After it, the head may name the region to look
at next for that client (`PoseHead::next`): kept 1.5 s (`TRACK_FOR`), for a
picture of the same size, and forgotten when the lamp turns private or the
model is let go. A head reads the outputs (`crates/npu/src/head.rs`, the
`PoseHead` trait):

- `movenet`: MoveNet SinglePose (Lightning, 192x192; Thunder, 256x256),
  Apache-2.0, the model the lamp ships (the owner, 19 Nov). Converted for the
  NPU (`movenet_lightning_pcq.nb`, int8 per channel, 192x192 RGB bytes fed as
  they are), it stops before MoveNet's own decode and gives four maps on a
  48x48 grid, channels last: the person's centre (1), the keypoints' heat
  (17), their offsets in the cell (34) and their distance from the centre
  (34). The decode is MoveNet's own, on the CPU: the centre is the centre
  map's highest cell weighed by its distance from the grid's middle; each
  keypoint is the most heat near where the centre's regression puts it,
  weighed down by distance, plus its offset; its score is its heat. The
  person's score is the keypoints' mean. The next frame is cropped around
  the person, as the MoveNet tutorial does: a square on the hips' middle,
  1.9 times the torso or 1.2 times the body seen, whichever is wider; the
  whole picture when the shoulders or hips are not seen (score 0.2 or less)
  or the square would be wider than the picture. Checked against the
  reference decode the conversion was measured with (`mn.py`, within 0.0002
  of the float model): its keypoints from the NPU's int8 maps of two frames,
  its next crops of two frames and its crops of a made picture
  (`crates/npu/tests/fixtures/movenet/`, made by `make.py` there; no model,
  no picture of a person).
- `yolov8-pose`: YOLOv8-pose cut before its decode, as the reference
  conversion cuts it (box 64 / class 1 / keypoints 51 at strides 8, 16 and
  32): the most confident anchor is the person. Kept for comparison only: its
  weights are AGPL-3.0, so no lamp is sent them and none is in the
  repository.

Which head a model has is its manifest's `head`, so the Store swaps a model
without a lampOS release; a new kind of model is one more head. A score under
0.2 is no person (`pose: null`).

The NPU is reached through VeriSilicon's VIPLite (`/usr/lib/libNBGlinker.so`,
2.0.3.2-AW on the A733, target `VIP9000NANODI_PID0X1000003B`), opened at run
time (`crates/npu/src/vip.rs`, the crate's only unsafe code), so the build
needs no vendor file. A lamp without the library or the device answers
`unavailable` ("this lamp has no NPU"), never a crash.

## When the model is fetched

The image carries no model. The service fetches one:

- at the first `pose` (the ask is answered `preparing`, with `retry_ms`
  2000, until the model is kept: never an error loop), and
- when an installed app names `pose` in its manifest's `npu` (apps.md),
  looked at once a minute, so the owner's first session doesn't wait on the
  download.

A lamp with no such app and no ask fetches nothing.

In the Store, a model is three files under `<base>/<op>/<board>/`:
`lamp-model.json` (format 1, kind `npu-model`: `id`, `op`, `head`, `board`,
`file`, `size`, `sha256`, `licence`), `lamp-model.sig` and the model file.
In order, and nothing is written before the last step
(`crates/npu/src/model.rs`):

1. The base is https on the pinned Store host (`--models-url` on
   `--models-host` exactly: no other host, no user, no port of its own);
   `lamp_update::fetch`'s client follows redirects only to https.
2. The manifest (at most 4 KiB) and its signature are fetched into memory.
   The signature is checked before the manifest is read, with the Store's
   keys as the image ships them (`lamp_skills::package::store_keys`: the
   build's `store-key.pub` in the skills folder, else the factory's), the
   same check and format as an app's `lamp-skill.sig`, never a key the
   manifest names.
3. The manifest is checked: this op, a head this build reads, this board, a
   plain file name, at most 20 MB.
4. The file is fetched into memory, at most its manifest's size, and its
   size and SHA-256 checked.
5. Only then are the three written into a staging folder beside the kept
   one, synced, and swapped in by renames.

A failed step leaves nothing behind, and the ask is answered `unavailable`
with when it is tried again: 30 s, doubled each time up to 30 minutes. What
is kept is checked again, signature and SHA-256, at every load; a file
changed on the disk is removed, never run, and fetched again after the wait.

`lamp-npu pack --key-file KEY --model FILE --id ID --head HEAD --licence SPDX
--out DIR` makes the three files on the Store's side, signed as
`lamp-skills pack --key-file` signs a package.

**Open: no model is hosted yet.** No source is set in the product unit: no
AGPL model is served to lamps at all (the first builder's third rule), and the
Apache-2.0 model is still to be converted and measured. Where the Store hosts
and signs models is the Store's side to settle; until then a `pose` is
answered `unavailable` ("no model source is set on this lamp").

## Removal and reset

When the last pose-using app is removed, the model stays: it is a few MB,
and the next app or ask uses it without a download. A reset wipes it with
the service's state, `/var/lib/lampos-npu` (setup.md).

## The product unit

`deploy/product/lamp-npu.service`: installed and enabled on every lamp, as
`lampos` in `lampos-parts.slice`, under the keeper, its state in
`/var/lib/lampos-npu` (0700) and its socket in `/run/lampos-npu` (0700),
`/dev/vipcore` its only device (`DevicePolicy=closed`). With `LAMP_NPU`
unset, `serve` idles and touches nothing: no socket, no file, no NPU, no
network. A drop-in setting `LAMP_NPU=on` here and in `lamp-control.service`,
and the Store's `--models-url` and `--models-host`, turns it on.

## Measured

See the pull request for #7415's first run on lamp-0c89. Unit tests cover the
fetch's checks with a fake Store, the service's rules with a fake NPU and its
own clock, both heads, the picture's fit and the VIPLite struct's layout.
