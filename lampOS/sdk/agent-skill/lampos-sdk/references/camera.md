# The camera

The camera comes through lamp-control with the `camera` grant. On a lamp
without it, the calls are refused, or the `camera` grant can't be given.

- **A picture:** Python `await lamp.camera_snapshot()` returns the JPEG
  bytes, about 320 across, of the eyes' newest frame. JS
  `await lamp.cameraSnapshot()` returns a Buffer.
- **From lamp-mcp:** `lamp_camera_snapshot()` returns the picture as an
  image. The stream has no tool: its frames are for a program, not an
  agent's turn.
- **A stream:** Python `await lamp.camera_stream(True, fps=2)`, or with a
  size `camera_stream(True, 24, "640x480")`; JS takes them in order,
  `lamp.cameraStream(true, 2)` or `lamp.cameraStream(true, 24, '640x480')`
  (positional, not an options object). It sends `camera.frame` events, `fps`
  1 to 30 a second at most, each new frame once as it comes, until
  `camera_stream(False)` / `cameraStream(false)`, `stop`, or a
  `camera.ended` event (with its `why`). You don't subscribe to these: they
  come with the stream you asked for. Listen with
  `lamp.on("camera.frame", handler)`.
- **Two paths, and the lamp picks:** the answer says which.
  - **`fast: true`** (where the eyes serve every frame, Ten's fast stream):
    each frame as the camera made it, `size` `640x480` (the default when you
    give none) or `1280x720`, the JPEG as bytes in `data`.
  - **Otherwise:** the eyes' newest frame, about 1 to 2 a second, the JPEG
    as base64 `jpeg`.

  The answer and each frame carry `fps_now`, the rate the lamp is actually
  sending. Measured on lamp-75c9: asked 24 fps at 640×480, the lamp said
  `fps_now` 24–25, and a laptop on Wi-Fi received 14.4 fps at about
  730 KB/s. Pick the size and rate for what your program does with the
  picture, not the largest.
- **Never while private:** a stream ends (`camera.ended`, "the lamp is
  private") and a snapshot is refused `private` while the person has set the
  lamp private.
- **Only on the lamp's own network:** a computer admitted from beyond it (the
  lamp's remote opt-in) never gets the camera. An ssh tunnel is local to the
  lamp and carries it: that is the owner's choice when they give ssh, never a
  way around it.
- **The microphone** has its grant in the same schema change; its calls come
  with it.
