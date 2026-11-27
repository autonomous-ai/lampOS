# Light-ASD benchmark artifacts

These are local, independently verified exports for the off-by-default
sensor 5 component bench. No model is downloaded or loaded by a product unit.
The [input and inference contract](../../docs/speaking-face-light-asd.md)
describes the fixed trailing windows and their limitations.

- Upstream: <https://github.com/Junhua-Liao/Light-ASD>
- Source commit: `ed38c232de5efe0261dbd68627c0ade7cdfe14eb`
- Source licence: MIT; preserve the upstream notice with any model copy.
- Checkpoint: `weight/pretrain_AVA_CVPR.model` (AVA, without TalkSet tuning).
- Checkpoint SHA-256:
  `d44bc3ea7baa8e0946fa3921311714a630ed8b90a1928fab0dbe30d918909317`
- Export: ONNX opset 17, IR 8, batch 1, trained classifier included.
- Inputs: signed-PCM16-derived MFCC `[1,4*T,13]` and gray `[1,T,112,112]`.
- Outputs, in order: logits, speaking logit, speaking softmax probability.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `light_asd_ava_t25.onnx` | 4103414 | `a2386f3d087af52809901a568df504ebdbdbe6161d6d958b3fccae31ec50352d` |
| `light_asd_ava_t50.onnx` | 4103415 | `9c56974f3933cda59e5524e26aa9c5ef79e854ff78e43b09c0c1eb3adc5b1ef9` |

The source's unbatched rank-four `MaxPool3d(1,1,3)` is lowered to the equivalent
`MaxPool2d(1,3)` for export. This operation is bit-exact; complete exported
predictions were also checked against the untouched PyTorch model. Both pinned
exports run with tract-onnx 0.23.8. Numerical parity is not sensor accuracy.

Original source, checkpoint, exact conversion commands and environment,
licence, failed exports, synthetic and real parity inputs, output arrays,
timing observations and SHA-256 inventories are retained in the sensor 5
evaluation archive. The EVAL on #8848 identifies that archive by hashes.
The exports are not an upstream-supported ONNX release or an automatically
available lamp asset. They remain local for this offline review; the runner
requires the explicitly supplied, matching artifact. Model distribution and
live deployment need a later reviewed step. Media rights are separate from
the code licence; no real person's image or audio belongs in this directory.
