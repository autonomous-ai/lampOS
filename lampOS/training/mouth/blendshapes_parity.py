"""Checks the blendshapes conversion against Google's TFLite model it came
from (#1671).

Five inputs of 146 points each: four of seeded noise around a face-sized
spread, one a grid. Each runs through the TFLite interpreter and through
ONNX Runtime; prints the largest difference in the 52 scores, and fails
above 1e-4.

    python blendshapes_parity.py face_blendshapes.tflite face_blendshapes.onnx
"""

import sys

import numpy as np
import onnxruntime as ort
import tensorflow as tf

LIMIT = 1e-4


def main(tflite: str, onnx_path: str) -> None:
    interpreter = tf.lite.Interpreter(model_path=tflite)
    interpreter.allocate_tensors()
    given = interpreter.get_input_details()[0]
    session = ort.InferenceSession(onnx_path, providers=["CPUExecutionProvider"])
    name = session.get_inputs()[0].name
    rng = np.random.default_rng(1)
    inputs = [(rng.random((1, 146, 2)) * 200 + 100).astype(np.float32) for _ in range(4)]
    y, x = np.divmod(np.arange(146), 13)
    inputs.append(np.stack([x * 15.0, y * 18.0], -1)[None].astype(np.float32))
    worst = 0.0
    for points in inputs:
        interpreter.set_tensor(given["index"], points)
        interpreter.invoke()
        expected = interpreter.get_tensor(interpreter.get_output_details()[0]["index"]).reshape(-1)
        got = session.run(None, {name: points})[0].reshape(-1)
        worst = max(worst, float(np.abs(got - expected).max()))
    print("largest difference from TFLite over %d inputs: %.2e" % (len(inputs), worst))
    if worst > LIMIT:
        sys.exit("more than %g" % LIMIT)


if __name__ == "__main__":
    main(*sys.argv[1:3])
