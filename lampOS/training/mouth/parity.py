"""Checks the ONNX conversion against Google's TFLite model it came from.

Five inputs (four of seeded noise, one gradient), each run through the
TFLite interpreter and through ONNX Runtime; prints the largest difference
in the 478 x 3 landmarks, and fails above 1e-3.

    python parity.py face_landmarks_detector.tflite face_mesh_v2.onnx
"""

import sys

import numpy as np
import onnxruntime as ort
import tensorflow as tf

LIMIT = 1e-3


def landmarks(outputs) -> np.ndarray:
    # The largest output is the 1434 landmark coordinates.
    return max(outputs, key=lambda a: a.size).reshape(-1)


def main(tflite: str, onnx_path: str) -> None:
    interpreter = tf.lite.Interpreter(model_path=tflite)
    interpreter.allocate_tensors()
    given = interpreter.get_input_details()[0]
    session = ort.InferenceSession(onnx_path, providers=["CPUExecutionProvider"])
    name = session.get_inputs()[0].name
    rng = np.random.default_rng(1)
    inputs = [rng.random((1, 256, 256, 3), dtype=np.float32) for _ in range(4)]
    y, x = np.mgrid[0:256, 0:256]
    inputs.append(np.stack([x / 255, y / 255, (x + y) / 510], -1)[None].astype(np.float32))
    worst = 0.0
    for image in inputs:
        interpreter.set_tensor(given["index"], image)
        interpreter.invoke()
        expected = landmarks([interpreter.get_tensor(o["index"]) for o in interpreter.get_output_details()])
        got = landmarks(session.run(None, {name: image}))
        worst = max(worst, float(np.abs(got - expected).max()))
    print("largest difference from TFLite over %d inputs: %.2e" % (len(inputs), worst))
    if worst > LIMIT:
        sys.exit("more than %g" % LIMIT)


if __name__ == "__main__":
    main(*sys.argv[1:3])
