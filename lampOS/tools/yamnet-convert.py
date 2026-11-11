"""Converts Google's YAMNet weights (yamnet.h5) to the ONNX pair lamp-ears
loads (#6232, sound-events.md "Where the weights come from"), and checks the
result against Google's own Keras model. Run by tools/yamnet-convert.sh, which
fetches the pinned inputs and makes the venv; not meant to be run alone.

  python -I yamnet-convert.py --h5 yamnet.h5 --yamnet-dir TFM/research/audioset/yamnet \
      --torch-audioset TA --out OUT

Writes OUT/yamnet.onnx and OUT/yamnet.data (graph [1, 1, 96, 64] log-mel patch
-> [1, 521] logits, its weights as external data named yamnet.data), the
synthesized test WAVs in OUT/fixtures, OUT/fixtures/patches.f32 with
OUT/fixtures/keras_logits.f32 (Google's patches and its logits on them) and
OUT/parity.txt. Exits non-zero when the ONNX model's logits differ from the
Keras model's by more than TOLERANCE on any patch of any fixture.
"""

import argparse
import math
import os
import sys
import wave

# Largest absolute logit difference allowed between our ONNX graph and Google's
# Keras model on the same patches: float32 rounding over 28 layers, far below
# anything a 0.6 sigmoid bar could notice (a logit step of 1e-3 moves a score
# near 0.6 by about 2.4e-4).
TOLERANCE = 1e-3
RATE = 16000


def parse():
    p = argparse.ArgumentParser()
    p.add_argument("--h5", required=True)
    p.add_argument("--yamnet-dir", required=True)
    p.add_argument("--torch-audioset", required=True)
    p.add_argument("--out", required=True)
    return p.parse_args()


def fixtures():
    """Synthesized 4 s clips, no recording of anyone: a name and int16 samples."""
    import numpy as np

    n = 4 * RATE
    t = np.arange(n) / RATE
    rng = np.random.default_rng(6232)

    def hz(m):
        return 440 * 2 ** ((m - 69) / 12)

    # Music: a C-Am-F-G progression at 96 bpm, piano-like decaying partials,
    # a bass and a soft kick on each beat.
    beat = 60 / 96
    music = np.zeros(n)
    for i, chord in enumerate([[60, 64, 67], [57, 60, 64], [53, 57, 60], [55, 59, 62]] * 2):
        start = int(i * 2 * beat * RATE)
        if start >= n:
            break
        seg = t[: n - start]
        env = np.exp(-seg * 2.5)
        for m in chord + [chord[0] - 24]:
            for k in range(1, 5):
                music[start:] += env * np.sin(2 * np.pi * hz(m) * k * seg) / k
    for b in range(int(4 / beat) + 1):
        start = int(b * beat * RATE)
        seg = t[: min(n - start, 2400)]
        music[start : start + len(seg)] += 2 * np.exp(-seg * 40) * np.sin(2 * np.pi * 60 * seg)

    # Speech-like: a glottal pulse train with a falling-rising pitch through
    # changing vowel formants, opened and closed at a syllable rate.
    f0 = 120 + 30 * np.sin(2 * np.pi * 0.7 * t)
    phase = 2 * np.pi * np.cumsum(f0) / RATE
    source = sum(np.sin(k * phase) / k for k in range(1, 30))
    vowels = [(730, 1090), (270, 2290), (300, 870), (530, 1840)]
    speech = np.zeros(n)
    syll = int(0.25 * RATE)
    for s in range(n // syll):
        a, b = s * syll, (s + 1) * syll
        f1, f2 = vowels[s % len(vowels)]
        seg = source[a:b]
        spec = np.fft.rfft(seg)
        freqs = np.fft.rfftfreq(len(seg), 1 / RATE)
        gain = sum(1 / (1 + ((freqs - f) / 90) ** 2) for f in (f1, f2))
        speech[a:b] = np.fft.irfft(spec * gain, len(seg)) * np.hanning(len(seg))

    clips = {
        "silence": np.zeros(n),
        "tone-440": np.sin(2 * np.pi * 440 * t),
        "noise": rng.standard_normal(n),
        "music": music,
        "speech-like": speech,
    }
    out = {}
    for name, x in clips.items():
        peak = np.max(np.abs(x))
        x = x / peak * 0.5 if peak > 0 else x
        out[name] = np.round(x * 32767).astype(np.int16)
    return out


def main():
    args = parse()
    # Isolated mode (-I) puts no script or current directory on the path:
    # the two pinned source trees are added by name, nothing else.
    sys.path[:0] = [
        args.yamnet_dir,
        args.torch_audioset,
        os.path.join(args.torch_audioset, "tf_2_torch"),
    ]
    import numpy as np
    import onnx
    import onnxruntime
    import tensorflow as tf
    import tf_keras
    import torch

    import convert_yamnet  # torch_audioset's converter, its weight mapping
    import features as features_lib  # Google's YAMNet frontend
    import params as yamnet_params
    import yamnet as yamnet_lib  # Google's YAMNet model

    os.makedirs(args.out)
    graph = os.path.join(args.out, "yamnet.onnx")

    # 1. yamnet.h5 -> PyTorch, by torch_audioset's own mapping functions.
    model = convert_yamnet.torch_yamnet(pretrained=False)
    convert_yamnet.load_tf_weights(
        model.state_dict(), convert_yamnet.load_keras_weight_in_pt_format(args.h5)
    )
    model.eval()

    # 2. PyTorch -> ONNX, weights in one external file named yamnet.data.
    torch.onnx.export(
        model,
        (torch.zeros(1, 1, 96, 64),),
        graph,
        input_names=["patch"],
        output_names=["logits"],
        opset_version=17,
        do_constant_folding=True,
        dynamo=False,
    )
    proto = onnx.load(graph)
    onnx.save_model(
        proto,
        graph,
        save_as_external_data=True,
        all_tensors_to_one_file=True,
        location="yamnet.data",
        size_threshold=0,
    )
    onnx.checker.check_model(graph)

    # 3. Google's reference: its Keras model with yamnet.h5, and its frontend.
    params = yamnet_params.Params()
    keras = yamnet_lib.yamnet_frames_model(params)
    keras.load_weights(args.h5)
    dense = [layer for layer in keras.layers if isinstance(layer, tf_keras.layers.Dense)]
    assert len(dense) == 1
    logits_model = tf_keras.Model(keras.input, dense[0].output)

    session = onnxruntime.InferenceSession(graph, providers=["CPUExecutionProvider"])
    fixture_dir = os.path.join(args.out, "fixtures")
    os.makedirs(fixture_dir)
    lines = [f"tolerance (max |logit difference|): {TOLERANCE}"]
    worst = 0.0
    all_patches, all_logits = [], []
    for name, pcm in fixtures().items():
        with wave.open(os.path.join(fixture_dir, f"{name}.wav"), "wb") as w:
            w.setnchannels(1)
            w.setsampwidth(2)
            w.setframerate(RATE)
            w.writeframes(pcm.tobytes())
        waveform = pcm.astype(np.float32) / 32768.0
        padded = features_lib.pad_waveform(tf.constant(waveform), params)
        _, patches = features_lib.waveform_to_log_mel_spectrogram_patches(padded, params)
        patches = patches.numpy()
        keras_scores = keras(tf.constant(waveform))[0].numpy()
        keras_logits = logits_model(tf.constant(waveform)).numpy()
        ours = np.concatenate(
            [session.run(None, {"patch": p[None, None]})[0] for p in patches]
        )
        all_patches.append(patches)
        all_logits.append(keras_logits)
        logit_err = float(np.max(np.abs(ours - keras_logits)))
        score_err = float(np.max(np.abs(1 / (1 + np.exp(-ours)) - keras_scores)))
        worst = max(worst, logit_err)
        top = int(np.argmax(keras_scores.mean(axis=0)))
        lines.append(
            f"{name}: {len(patches)} patches, max |logit diff| {logit_err:.3g}, "
            f"max |score diff| {score_err:.3g}, Keras top class {top}, "
            f"music(132) {keras_scores[:, 132].max():.4f}, speech(0) {keras_scores[:, 0].max():.4f}"
        )
    np.concatenate(all_patches).astype("<f4").tofile(os.path.join(fixture_dir, "patches.f32"))
    np.concatenate(all_logits).astype("<f4").tofile(os.path.join(fixture_dir, "keras_logits.f32"))
    lines.append(f"worst: {worst:.3g} ({'within' if worst <= TOLERANCE else 'OVER'} tolerance)")
    text = "\n".join(lines) + "\n"
    with open(os.path.join(args.out, "parity.txt"), "w") as f:
        f.write(text)
    print(text, end="")
    if not math.isfinite(worst) or worst > TOLERANCE:
        sys.exit("the ONNX model does not match Google's Keras model")


if __name__ == "__main__":
    main()
