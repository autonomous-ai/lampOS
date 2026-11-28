# Hearing an interruption over the lamp’s own voice

Sensor 7 compares recorded product events with a local speech recognizer that listens throughout playback. The candidate uses no wake word or upstream speech decision. The offline tool emits possible stop cues and cannot control a lamp. Its eventual product switch is proposed as default-off. [Assignment](https://github.com/autonomous-ai/lampOS/issues/8923) · [Frozen candidate](https://github.com/autonomous-ai/lampOS/issues/8923#issuecomment-7237422960) · [Tiếng Việt](../../vi/overlap-stop_vi.md)

## Reproduce a continuous replay

Download the publisher’s `vosk-model-small-en-us-0.15.zip` (Apache-2.0, SHA-256 `30f26242c4eb449f948e42cb302dd7a686cb29a3423a8367f99ff41780942498`) and the Vosk 0.3.44 macOS universal2 wheel (SHA-256 `029d0b3d6a5cff874c575b8a5814a4cccfb37608cff52e846c02a8c67a882801`). Unpack each locally. The wheel contains the native library `vosk/libvosk.dyld` (SHA-256 `82fdfba0dde392a7dbba70f9c1a17f6e3da27a50444a9f53939508525a1e6fdf`). The replay checks the inventory of the 14 model files before scoring. The original open decoding graph runs without a restricted phrase grammar. [Publisher’s model and license](https://alphacephei.com/vosk/models) · [Versioned runtime files](https://pypi.org/project/vosk/0.3.44/#files)

Run from `lampOS/` with a fresh output path per uninterrupted stretch. The input is little-endian signed 16-bit mono PCM at 16 kHz, with no WAV header. Use only the sent microphone samples. Keep real frame sequence numbers and timing alongside the trace. At an unrecorded interval start a fresh decoder and mark the gap. Keep participant audio and all per-line transcripts on the collecting Mac.

```sh
ruby tools/overlap-stop/replay.rb "$VOSK_LIBRARY" "$VOSK_MODEL_DIR" \
  < continuous.s16le > new-trace.jsonl
ruby tools/overlap-stop/replay-test.rb
```

The decoder gets 320 samples at a time and writes a row immediately after each block. It emits the first exact stop phrase in a decoder utterance, with one second between votes. The command list and optional fillers are frozen in the registration. A revised partial can never retract a vote already issued. Endpoints are decided by the recognizer. End-of-file does not force a final hypothesis. The final row confirms how many samples were processed.

`end_sample` gives the newest available input. `compute_us` measures decoding through classification. `ready_us` adds that work to input availability, including any accumulated single-worker backlog. It excludes recording I/O, writing the evidence trace, output dispatch and acoustic silence. To score a vote, join it to independent same-clock evidence of the user’s actual acoustic onset and active speaker output. Report the 400 ms row separately from any later recognition. A kit playback marker, a final transcript, and an observed speaker stop represent different times.
