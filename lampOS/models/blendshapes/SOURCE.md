# face_blendshapes.onnx

Google's MediaPipe Blendshape V2 model (the `face_blendshapes` of
FaceLandmarker), converted to ONNX for #1671: how the person at the lamp
seems, from 52 movements of their face, read from Face Mesh V2's points
(`models/mouth/SOURCE.md`). Not ours: converted, not retrained.
`lamp-eyes --seems-blendshapes` reads it (docs/eyes.md), with `--mouth-model`.

| | |
|---|---|
| File | `face_blendshapes.onnx`, 1,882,583 bytes. Not in git, since a part's models come from a release (`deploy/product/models.json`). Until one carries it, `training/mouth/make.sh` makes it, and it is placed by hand |
| SHA-256 | `68056086da2409d47891e88b7f44d331d60e3d0bc053f5ff0fd51b0915f11ddd` |
| Input | `serving_default_input_points:0`, float32 `[1, 146, 2]`: 146 of the mesh's 478 points, x and y, as MediaPipe picks them (`face_blendshapes_graph.cc`, `kLandmarksSubsetIdxs`; `blendshapes::POINTS`) |
| Output | `StatefulPartitionedCall:0`, float32 `[52]`: the 52 movements from 0 to 1, `_neutral` first, in MediaPipe's order (`kBlendshapeNames`) |
| Graph | ONNX opset 13, 189 nodes, 67 initializers, from tf2onnx 1.16.1 with TensorFlow 2.15.1 |
| Matches the original | to within 1.4e-6 in every score against the TFLite model under TensorFlow Lite, over five inputs (`blendshapes_parity.py`) |

## Where it comes from, and under which licence

| Source | | Licence |
|---|---|---|
| `face_landmarker.task`, float16, version 1, the same download as the mesh's (SHA-256 `64184e229b263107bc2b804c6625db1341ff2bb731874b0bcc2fe6544e0bc9ff`) | A zip; its `face_blendshapes.tflite` (955,312 bytes, SHA-256 `4f36dded049db18d76048567439b2a7f58f1daabc00d78bfe8f3ad396a2d2082`) is what is converted | Apache-2.0, per Google's [model card for Blendshape V2](https://storage.googleapis.com/mediapipe-assets/Model%20Card%20Blendshape%20V2.pdf): trained on samples generated from Google's GHUM 3D model and samples captured in a lab, with "detection of human facial expressions from monocular video" among its intended uses |

## The command that made it

From `lampOS/training/mouth/`, on a Linux x86 laptop, Python 3.11 from
`uv sync` (`uv.lock`):

    sh make.sh

which makes the mesh first (`models/mouth/SOURCE.md`), then checks the
`.tflite` above, converts it with `tf2onnx --opset 13` under
`PYTHONHASHSEED=0` (tf2onnx orders this graph by Python's string hashing,
so without it each run's file differs), applies the mesh's `canon.py`,
checks the scores against the TFLite model (`blendshapes_parity.py`), and
checks the SHA-256 above. Three conversions gave this same file.
