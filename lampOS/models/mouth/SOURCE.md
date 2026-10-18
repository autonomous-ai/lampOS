# face_mesh_v2.onnx

Google's MediaPipe Face Mesh V2 landmarks model (the `face_landmarks_detector`
of FaceLandmarker), converted to ONNX for #720: inner-lip points to tell a
face speaking from a still one at desk distance, where YuNet's mouth corners
are not enough (#1385, docs/eyes.md). Not ours: converted, not retrained.
`lamp-eyes --mouth-model` reads it (docs/eyes.md): to the journal, and with
`LAMP_EYES_SPEAKING=on` to the face's `speaking` (`from: mesh`).

| | |
|---|---|
| File | `face_mesh_v2.onnx`, 4,915,552 bytes. Not in git, since a part's models come from a release (`deploy/product/models.json`). Until one carries it, `training/mouth/make.sh` makes it, and it is placed by hand |
| SHA-256 | `09d24504cdc51240b6ccd8c457040c7cb61ab3485e0e08696dd44ce52ed8131a` |
| Input | `input_12`, float32 `[batch, 256, 256, 3]`: a face crop in RGB from 0 to 1, with a 25% margin and the eyes level |
| Outputs | `Identity`, float32 `[batch, 1, 1, 1434]`: 478 landmarks, x, y and z each. The lips are 40 of them, 20 outer and 20 inner; the inner gap is points 13 and 14, and the face's height is 10 to 152. `Identity_1`, `[batch, 1, 1, 1]`, and `Identity_2`, `[batch, 1]`: the face score and a second score, not used here |
| Graph | ONNX opset 13, 223 nodes, 257 initializers, from tf2onnx 1.16.1 with TensorFlow 2.15.1 |
| Matches the original | to within 3.8e-4 in every landmark coordinate against the TFLite model under TensorFlow Lite, over five inputs (`parity.py`) |
| On lamp-52e6 | One pass of the Hugging Face conversion below (the same landmarks), with lamp-eyes running (#720, 2026-10-18 10:3x; median of 100): ONNX Runtime 1.30, one thread, 33 ms on a big core (A76) and 154 ms on a small one (A55); tract-onnx 0.23.8, 85 ms and 290 ms |

## Where it comes from, and under which licence

| Source | | Licence |
|---|---|---|
| `face_landmarker.task`, float16, version 1, from `storage.googleapis.com/mediapipe-models/face_landmarker/face_landmarker/float16/1/`, 3,758,596 bytes, SHA-256 `64184e229b263107bc2b804c6625db1341ff2bb731874b0bcc2fe6544e0bc9ff` | A zip; its `face_landmarks_detector.tflite` (2,553,590 bytes, SHA-256 `c7d54204ce0448474c7f3fa9af494787c0965cbdd6f20fc72867e43046bd43d5`) is what is converted | Apache-2.0, per Google's model card for MediaPipe Face Mesh V2 |

The ONNX file another party publishes on Hugging Face
(`fernandotonon/QtMeshEditor-facemesh-onnx`) gives the same landmarks to the
same 3.8e-4, but it is not used: this one is made from Google's file by a
command anyone can run.

Not used: the 68-point models (dlib's, OpenCV's LBF, 2D-FAN) and InsightFace's,
all trained on iBUG 300-W or with weights for non-commercial use only (#720).

## The command that made it

From `lampOS/training/mouth/`, on an Apple M1 Max laptop (macOS 26), Python
3.11 from `uv sync` (`uv.lock`):

    sh make.sh

which downloads the `.task`, checks it and its `.tflite`, converts with
`tf2onnx --opset 13`, renames tf2onnx's counter-named constants by first use
and the batch dimension `batch`, and empties the doc string that holds the
source's path (`canon.py`: tf2onnx's own names and that path change from run
to run, nothing else does), checks the landmarks against the TFLite model
(`parity.py`), and checks the SHA-256 above. Three conversions, one of
them by `make.sh` from an empty cache, gave this same file.
