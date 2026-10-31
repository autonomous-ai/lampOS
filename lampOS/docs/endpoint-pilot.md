# End-of-turn pilot package (MAIN-057)

This is a conservative, opt-in endpoint package for Main to compose into the
supervised pilot ([supervised-pilot.md](supervised-pilot.md)).

- **Base.** The current composition is on `lamp-v2-chat` `675c71c62`, branch
  `lamp-a2-endpoint-pilot`. The first package (`lamp-a2-pilot`, base
  `892ec0a1`) and its limit fix (`lamp-a2-pilot-limits`) are unchanged.
- **Where it ran.** Nothing here ran on a Lamp, the shared office VM or a
  cloud service. Every measurement comes from an M2 Max and an ARM64 Linux VM
  on that host. Neither is an A733.
- **Default unchanged.** Without `--endpoint smart-turn`, no endpoint worker
  starts and the detector, admission, playback and provider paths are the
  base's. The `endpoint_policy` and `local_endpoint` trace records keep
  exactly their existing fields.

## The fast model (2026-10-14)

The worker can run the same pinned model fast: `lamp-turn-end --fast`. Its
weight multiplications are done in 8 bits by ONNX Runtime, on two threads,
on the fastest cores. Without `--fast` nothing changes. The second builder
built it for task 1 of [work-split.md](work-split.md). Nothing here ran on
a lamp.

**Why.** On lamp-4ace one verdict took 383 ms on the two fast cores and
1,292 ms on the six slow ones, so the model could never end a turn before
the 600 ms window ([endpoint-detection.md](endpoint-detection.md), "On the
lamp, 2026-10-14"). Wanted: a verdict within 150 ms of being asked, on the
fast cores, that agrees with the present model on labelled speech.

### What exists and what we take

Each way was timed on the Linux ARM64 VM (Colima on the M2 Max, 6 virtual
cores, Debian bookworm, Rust 1.96.0) with one full 8 s window: the model
alone, warm median. The agreement in this table was measured on the M2 Max
itself (tract, or ONNX Runtime 1.31 in Python), on the 1,167 held-out human
clips described below; the chosen way's is measured again on the VM, with
the worker's own code, in "Agreement with today's model".

| Way | Time | Agrees with today's model? | Taken |
| --- | --- | --- | --- |
| tract, floats, 1 thread (today) | 85 ms | it is today's | Kept as the default |
| tract, floats, 2 threads (`multithread-mm`) | 54 ms | yes | No: not enough |
| tract, half floats (the A76 and the M2 both have them) | 69 ms; 58 ms with softmax and normalisation kept in floats | 0.9678 against 0.9685 on the check signal | No: tract's half-float softmax took 21.5 ms, and the conversions eat what is saved |
| tract, 8 bits | as slow as floats, see below | | No |
| A shorter window, 6 or 4 s in place of 8 | not timed | No: on the 1,167 held-out human clips, 15–25 verdicts change side of 0.5 at 6 s and 52–59 at 4 s, and 4 s loses 2.7–3.3 points of accuracy | No |
| ONNX Runtime 1.31, floats | 85 ms; 44 ms on 2 threads | yes | No: not enough |
| ONNX Runtime 1.31, Pipecat's own 8-bit file (`smart-turn-v3.2-cpu.onnx`) | 40 ms; 22 ms on 2 threads | 24 of 1,167 change side as recorded and 44 asked 200 ms into a pause; 0.5 and 1.4 points of accuracy lost | No |
| ONNX Runtime 1.30, the pinned float file made 8-bit as the worker loads it, one multiplication kept in floats | 52 ms; 26.6–27.4 ms on 2 threads | 2 and 3 of 1,167 change side; accuracy within 0.2 point | **Yes** |

**Why tract's 8 bits are slow.** tract 0.23.8 has a fast 8-bit kernel for
ARM (SDOT), but its build script assembles a probe with the directive
`.cpu generic+fp+simd+dotprod`. Debian bookworm's assembler (binutils 2.40)
and clang 14, 15 and 16 refuse it; binutils 2.42 (Ubuntu 24.04) and 2.44
(Debian trixie) accept it, and `.arch armv8.2-a+dotprod` works everywhere.
So every lamp build made in a bookworm container has no SDOT kernel, and
tract's 8-bit multiplication runs at float speed without a word. With the
directive patched the 8-bit multiplications became 3.2 times faster, but the
whole model only reached 53 ms, 45 ms on 2 threads: tract quantizes in a
scalar loop (8.9 ms of the 53) and runs everything but multiplications on
one thread. Pipecat's own 8-bit file still gives a wrong answer under tract
(0.5588 against ONNX Runtime's 0.9809), from a quantize-and-dequantize
pair outside the multiplications; with those pairs bypassed it is right
(0.9646 against 0.9641). Neither is used.

**Also checked.** The VM's processor has an 8-bit matrix instruction
(i8mm) that the A76 lacks. Hiding it from ONNX Runtime (a test library that
masks the bit in `getauxval`; ONNX Runtime reads it) changed nothing:
26.0 against 27.0 ms. So the VM's ONNX Runtime times do not lean on an
instruction the lamp does not have. ONNX Runtime's threads busy-wait
between steps by default; that cost no more processor time per verdict than
sleeping (54 against 53 ms) and kept the worst case steadier (28–30 against
33–36 ms), so it is kept.

**The 8-bit form.** The model file stays the one the lamp has,
`smart-turn-v3.2-gpu.onnx`, pinned by size and SHA-256. Each time the worker
starts it makes the 8-bit form in memory (`crates/turn-end/src/eight_bit.rs`),
in the form ONNX Runtime's own `quantize_dynamic` writes, which ONNX Runtime
runs as one 8-bit multiplication:

- each weight matrix in int8, one scale per output column, symmetric;
- each activation quantized as it arrives, with its own scale (once when
  several multiplications read it);
- 24 of the model's 26 weight multiplications; the pooling's last (256 × 1)
  is too small to bother, and one is kept in floats on purpose.

That one is the third layer's projection after the GELU (`node_MatMul_223`).
Its input has outliers that one 8-bit scale per request cannot hold. It was
chosen on the development clips (shards 0 and 1), where keeping it in floats
alone took the verdicts that change side of 0.5 from 13 to 4, and only then
checked on the held-out clips (shards 4 and 5), where it took them from 21 to
0 under ONNX Runtime 1.31 in Python. It costs about 2 ms.

### How it runs

- `lamp-turn-end --model PATH [--parent PID] --fast`, otherwise as before:
  - keeps itself on the cores with the largest `cpu_capacity` (cpu6 and cpu7
    on the A733; where all cores are alike, as in the VM, it chooses none),
    so ONNX Runtime's second thread starts there too;
  - checks the ONNX Runtime library beside the worker
    (`libonnxruntime.so.1.30.0`) by size and SHA-256 before loading it;
  - checks the model as before, makes its 8-bit form and opens it on two
    threads;
  - answers exactly as before. `Ready` names the same model.
- Its address space is capped at 544 MiB in place of 256 MiB: 2.2 times the
  243.5 MiB peak measured with two threads. Core files stay off and the
  niceness +10, which counts only at the ordinary policy: the product's voice
  runs at real-time round-robin priority 20 (`CPUSchedulingPolicy=rr`), and
  the worker it starts inherits that, where niceness does nothing (#1608).
- By default ONNX Runtime's threads spin for a short spell when they run out
  of work, before they block ("for a short period before blocking", `ort`
  rc.14). So the default worker is near idle between verdicts: 30 ms of
  processor time or less over 3 s of quiet, from 500 ms after a verdict
  (`fast_worker_answers_is_bounded_and_idles_without_spinning`). The spell
  after each verdict, and between the parallel steps inside one, is at the
  voice's real-time priority, where a spinning thread holds its core from
  everything at ordinary priority there, and `lamp-name` runs on the same two
  cores (`CPUAffinity=6 7`, nice 5). With `--no-spin` (the voice passes it
  with `LAMP_TURN_END_NO_SPIN=on`, off unless set) both its intra-op and
  inter-op threads block at once instead
  (`session.intra_op.allow_spinning` and `session.inter_op.allow_spinning`
  at 0). The gain is that short spell; the cost may be a wake-up at each
  parallel step inside a verdict (the reviewer on #1614). `bench … --fast
  --no-spin` and `--check-model … --fast --no-spin` measure it, and say
  `"spin":false`. Not measured on a lamp yet: the check is `bench` with and
  without it, at `chrt -r 20` as the voice runs it, cold and warm verdict
  times compared.
- **Below the voice (`--realtime-priority N`, #1608).** A verdict holds
  both big cores at the voice's round-robin 20 for about 148 ms. The
  voice's capture thread runs there too at times (`serve-runtime.md`, "The
  capture worker's cores"), and at the same priority it waits out the
  verdict; inside a turn, 100 ms without a microphone frame lapses every
  lease (#1608, #1459). With `--realtime-priority N` the worker lowers its
  first thread to round-robin N before ONNX Runtime starts its pool, so its
  threads start at N too (`thread-priority`, no `unsafe`). The voice's
  threads at 20 then take a core back from it at once, and a verdict still
  runs ahead of every ordinary process. It only lowers a real-time
  priority: from an ordinary policy (the simulator) or one already at N or
  below it changes nothing, and it never raises one. The journal says
  `lamp-turn-end: real-time priority 20 lowered to 10`, or `… 20 kept, not
  raised to 30` for an N at or above the voice's. The voice passes it
  with `LAMP_TURN_END_PRIORITY=N` (unset, the worker keeps the voice's; a
  value that isn't 1 to 99 is said once and ignored), for either model.
  `bench … --realtime-priority N` measures it and reports `scheduling`. Not
  measured on a lamp yet: the check is the verdict times first and repeated
  with it on and off (`bench` at `chrt -r 20`), and, from a played battery,
  how many turns end `silence_fallback` (`timeout`), a verdict that came
  after the window had ended the turn, with it on and off.
- A missing or different library ends the worker at startup with the
  reason. The core then keeps the silence window on every pause, as for any
  other failed worker, and never restarts it.
- `WorkerCommand::smart_turn_fast` (`crates/turn-end-wire`) is the command
  the voice starts it with for `--endpoint smart-turn-fast`, the product's.
- `lamp-turn-end --check-model PATH --fast` checks the installation: the
  check signal must give the fast model's recorded 0.9682266 within 0.001.
- `lamp-turn-end bench PATH [RUNS] [--fast] [--threads N] [--cores fast|all|LIST] [--runtime LIBRARY] [--no-spin]`
  times the features and the model separately for each run on the full
  check signal (an 8 s window, the longest request), under the worker's own
  bounds, then prints one JSON line: the median and worst of each, processor
  time, memory, the cores it was kept on and each core's part number.
- `lamp-turn-end agree PATH DIRECTORY... [--csv FILE]` runs both models on
  labelled clips (a `manifest.json` and 16 kHz WAV files, the layout of
  [endpoint-detection.md](endpoint-detection.md), "Recorded human clips")
  as recorded and 200 ms into a pause, and reports how far they agree.

### Measured, not on the lamp

On the Linux ARM64 VM, the release worker, `bench` with 25 runs, under the
worker's bounds (niceness +10). The fast runs were repeated three times.

| | Exact (today) | Fast | |
| --- | --- | --- | --- |
| Verdict: features and model, median | 87.0 ms | 29.5–30.3 ms | 2.9 times faster |
| Worst of 24 warm runs | 92.8 ms | 31.3–33.2 ms | |
| Features | 2.8 ms | 2.9 ms | |
| Model | 84.2 ms | 26.6–27.4 ms | 3.1 times |
| One thread in place of two, median of 24 | | 54.7–55.9 ms | |
| Processor time per verdict | 88 ms | 59–61 ms over two threads | |
| Load: check, rewrite, session | 151 ms | 263–272 ms | |
| Peak address space / resident | 114 / 101 MiB | 243.5 / 100 MiB | |
| Processor time over 3 s idle after a verdict | | 0 ms | |

On the M2 Max itself (macOS): 34.2 ms exact (tract uses Apple's matrix
unit there) and 25.5 ms fast (before the kept multiplication).

**An estimate for the lamp, not a measurement.** The exact model took 383 ms
on lamp-4ace's fast cores against 87 ms here, 4.4 times as long. At the same
ratio the fast model would take about 130 ms, inside the 150 ms wanted. The
ratio may not hold; see below.

### Agreement with today's model

`lamp-turn-end agree` on the VM, release build, every clip asked as
recorded and 200 ms into a pause (with a −63 dBFS floor, as the voice asks).
The clips are the human clips of `pipecat-ai/smart-turn-data-v3.2-test`
(revision `dfc179241fee`, CC-BY-4.0; English and Spanish) and its English
generated speech. "Change side" is a verdict on the other side of 0.5. The
bands are the voice's (`ConfirmPolicy`: finished at 0.9 or more, clearly
unfinished under 0.01).

| Clips | Asked | Clips | Change side | Change band | Of which into "finished" | Correct at 0.5, today / fast | Largest difference |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Human, development (shards 0, 1) | as recorded | 1,180 | 6 | 12 | 5 | 95.42% / 95.42% | 0.177 |
| | 200 ms into a pause | 1,180 | 3 | 14 | 5 | 94.41% / 94.32% | 0.197 |
| Human, held out (shards 4, 5) | as recorded | 1,167 | 2 | 10 | 5 | 96.32% / 96.14% | 0.208 |
| | 200 ms into a pause | 1,167 | 3 | 18 | 5 | 95.63% / 95.54% | 0.228 |
| English, generated (shards 4, 5) | as recorded | 503 | 2 | 2 | 2 | 94.43% / 94.43% | 0.262 |
| | 200 ms into a pause | 503 | 2 | 4 | 3 | 93.44% / 93.44% | 0.133 |

The median difference is 0.0001 everywhere and the 99th percentile at most
0.121. Held out, 200 ms into a pause, the 18 band changes are: 5 into
"finished" (the turn would end at 400 ms where today it waits), 7 out of it,
3 into "clearly unfinished" and 3 out of it. Pipecat's own 8-bit file under
ONNX Runtime 1.31 (Python, M2 Max), on the same clips asked the same way:
44 change side and 117 change band, 1.4 points lost.

**Asking earlier.** The voice asks 200 ms into a pause. The same held-out
clips asked by today's model (ONNX Runtime 1.31 floats, Python, M2 Max) at
100 ms into the pause: finished turns judged finished 91.5% (90.6% at
200 ms), unfinished ones judged finished 3.3% (4.0%), correct at 0.5 95.2%
(95.6%). So asking at 100 ms is no worse at what ends a turn early. With the
fast model's 130 ms (if the estimate holds) a verdict would be in hand at
about 230 ms, and a finished sentence could end at 300 ms. That is the
voice's rule (`ConfirmPolicy` in `crates/voice/src/activity.rs`) and the first
builder's to change; nothing here changes it. These are recordings replayed
digitally, not pauses as the lamp's own detector finds them.

### What it costs

- **ONNX Runtime 1.30.0**, released by Microsoft on 2026-09-10 (a month
  older than 1.31.0, released 2026-10-09). MIT licence. The file
  `lib/libonnxruntime.so.1.30.0` of
  `https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-linux-aarch64-1.30.0.tgz`
  (archive SHA-256 `e16a27a8…3e3da`): 25,135,496 bytes, SHA-256
  `64e903a43a041240fd6bcffe0ac6d4fea47ef87bf24b9d097801bd00a9612a4b`. Built
  by Microsoft; nothing is built here. It needs glibc 2.28 and libstdc++
  `GLIBCXX_3.4.22` or later (Debian bookworm has 2.36 and 3.4.30), and only
  the system's C and C++ libraries besides. Only `lamp-turn-end --fast`
  loads it; `lamp-voice` links nothing new.
- **Rust crates:** `ort` 2.0.0-rc.14 and `ort-sys` 2.0.0-rc.14 (MIT or
  Apache-2.0; a release candidate, used with its default features off and
  `load-dynamic`, so its build script links and downloads nothing), and
  `libloading` 0.9.0 (ISC). Three lock entries; none removed or changed.
- **The worker:** 24,854,552 bytes in release on Linux ARM64 (18,235,200
  stripped), 0.6 MB more than before.

### On the lamp

The one thing to learn first is the time on the A76. From the release
directory, as the service user, with the library copied beside the worker:

```sh
curl -fsSLO https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-linux-aarch64-1.30.0.tgz
tar -xzf onnxruntime-linux-aarch64-1.30.0.tgz
cp onnxruntime-linux-aarch64-1.30.0/lib/libonnxruntime.so.1.30.0 .
./lamp-turn-end bench smart-turn-v3.2-gpu.onnx 30                # today's worker, as it is
./lamp-turn-end bench smart-turn-v3.2-gpu.onnx 30 --cores fast   # held to the fast cores
./lamp-turn-end bench smart-turn-v3.2-gpu.onnx 30 --fast         # the fast model
./lamp-turn-end --check-model smart-turn-v3.2-gpu.onnx --fast
```

What it should show: the fast runs list `"cores":[6,7]` and `"cpus"` with
`0xd0b` (Cortex-A76) for cpu6 and cpu7; the second line's `total_ms`
median near 383 ms (the check-model figure); the third's under 150 ms; the
check exits 0. Then, to use it, the voice starts the worker with
`WorkerCommand::smart_turn_fast` in place of `WorkerCommand::smart_turn`
(`crates/voice/src/coordinator/endpoint.rs`, `EndpointRig::start`), for
example behind an `--endpoint smart-turn-fast` value beside `smart-turn`.

### Measured on lamp-4ace (first builder, 2026-10-14, 10:19)

The worker cross-built in the lamp's build container (`bf3c0221…`, from
`a98e18a3f`), Microsoft's library fetched from the address above and
found to be the pinned one (the archive `e16a27a8…3e3da`; the library
25,135,496 bytes, `64e903a4…612a4b`), both in a directory of their own on
the lamp with a copy of the model the lamp already has. The voice, the
name part, the eyes, the body and the rest were running as usual. Thirty
runs each, milliseconds for one verdict, features and model:

| | Median | Worst | Processor time a verdict | Cores | Peak address space |
|---|---|---|---|---|---|
| As it is (exact) | 381.6 | 391.9 | 383 | all eight | 114 MiB |
| Exact, held to the fast cores | 386.2 | 391.0 | 386 | 6, 7 | 114 MiB |
| Fast | 148.3 | 160.5 | 280 | 6, 7 | 243 MiB of the 544 allowed |
| Fast, one thread | 213.1 | 221.6 | 215 | 6, 7 | 152 MiB |

The first verdict after loading took 147 ms fast (453 ms exact); loading
took 0.59 s fast and 0.40 s exact. On the test clip the fast model said
0.96844 and the exact one 0.96853; `--check-model … --fast` exited 0, its
own warm median 146.7 ms. The kernel gives the cores' kinds (`0xd0b` for
cpu6 and cpu7) and the worker chose those two.

So of the five things below: the time is 148 ms where 130 was reckoned
and under 150 was hoped; the fast cores were found; the library loads on
the lamp's system; the memory cap holds; loading takes 0.6 s. With the
name part on the same two cores, two threads still beat one by 65 ms.

What it makes possible: asked 200 ms into a pause, a verdict about 350 ms
after someone stops, where the silence rule alone needs 600 ms.

The voice starts the fast worker with `--endpoint smart-turn-fast`
(`Endpoint::SmartTurnFast` in `crates/voice/src/options.rs`,
`EndpointRig::start`), beside `smart-turn`, which stays as it was. With
lamp-4ace's 600 ms window the voice then uses the policy it already has
for a short window (`ConfirmPolicy::responsive`): a verdict of 0.9 or
more ends the turn once the pause is 400 ms long, a verdict under 0.01
holds it open until 1.5 s, and anything between, or no verdict in time,
ends on the window as today. So a sentence the model is sure is finished
is answered 200 ms sooner, and one it is sure is not finished is waited
for longer.

Not shown: any of it in a session of the voice. The lamp's unit does not
have the flag. The risk of ending early is cutting someone off in the
middle of what they are saying, so the scenes with pauses in them are to
be played to it with nobody in the office before it is on for anyone.

### Where this is most likely to be wrong on the lamp

1. **The time.** The 130 ms is the VM's time scaled by the exact model's
   ratio. The A76 has half the M2's vector units, and an 8-bit
   multiplication may lose more or less than a float one on it. `bench
   --fast` answers it in one line. If it is over 150 ms, try `--threads 1`
   too: with lamp-name on the same two cores every 80 ms, two threads may
   wait on each other.
2. **The fast cores.** If the lamp's kernel gives no `cpu_capacity`, the
   worker falls back to each core's highest frequency (2.0 GHz against
   1.79 GHz); if neither is there it keeps no core and says `"cores"` with
   all eight. Then `--cores 6,7` on `bench` shows what keeping would give.
3. **The library on the lamp's system.** If the lamp's libstdc++ or glibc is
   older than the library needs, the check fails at load with the loader's
   message. The worker then fails at startup and the voice keeps its window.
4. **The memory cap.** 544 MiB was measured in the VM. Another glibc may
   reserve more address space per thread; then loading aborts. The check
   prints `vm_peak_kib`.
5. **Load time.** About 270 ms in the VM; perhaps 1.2 s on the lamp, inside
   the 10 s the core allows a worker to become ready.

### Not shown

(Written before the measurement above, which settles the first sentence.)
Anything on the lamp: its time, its memory, its cores, its loader. Speech
through the lamp's microphone and its processing; the clips are recordings
replayed digitally. Whether a 300 or 400 ms end for a finished sentence
suits the people at the desk (the review above found that earlier ends
interrupt more). The fast model in a full session of the voice (its tests
use the worker through the core's client, not the voice).

## What the pilot runs

| | Command line | Endpoint |
| --- | --- | --- |
| Baseline (unchanged default) | `lamp-voice directed … --turn-taking v1-baseline`, or `lamp-voice serve … --turn-taking duplex` | 1,200 ms of quiet; no endpoint worker starts |
| Candidate | The same command plus `--endpoint smart-turn` | Parity profile: a confident complete verdict ends at 1,000 ms of quiet, a clearly unfinished one at 1,500 ms, anything else and every failure at 1,200 ms |

Duplex gates the scores that the same baseline detector sees, so the
candidate confirms duplex pauses exactly as V1-baseline pauses. `vad-only`
has no silence fallback and refuses `--endpoint smart-turn`.

**With a shorter silence window.** `--end-silence-ms` at or under 1,000 ms
cannot hold the parity profile, whose confident verdict waits a full second.
With `--endpoint smart-turn` such a window selects the quicker profile instead
(`ConfirmPolicy::responsive`): a confident complete verdict ends the turn at
400 ms of quiet, a clearly unfinished one holds it to 1,500 ms, and anything
else, with every failure, ends on the window that was chosen. A window of
400 ms or less leaves nothing to confirm and is refused at startup. This
profile adds cutoffs at pauses between sentences on labelled speech (see the
review below), so it is for a path on which a person who goes on speaking is
continued: `--provider-continuation kept-start`.

**First run on lamp-4ace (A733), 2026-10-12, and why it is off there.** One
`serve` session with `--turn-taking duplex --end-silence-ms 700 --endpoint
smart-turn`, four utterances played from a loudspeaker to `mic2`. The worker
was ready 862 ms after its spawn (model load 403 ms) and held 59 MB resident.

| Utterance | Inference | Verdict heard at | Probability | Turn ended |
| --- | --- | --- | --- | --- |
| a complete question | 405 ms | 630 ms into the pause | 0.887 | at 700 ms, `model_uncertain` |
| a complete question | no verdict within 500 ms | none | none | at 700 ms, `silence_fallback` (`timeout`) |
| a clause left unfinished | 427 ms | 660 ms | 0.441 | at 700 ms, `model_uncertain` |
| a statement and a complete question | 364 ms | 570 ms | 0.154 | at 700 ms, `model_uncertain` |

So on this board a classification costs about four times what the bounds
table gives for the VM, a verdict is heard 570 to 660 ms into a pause, and no
turn ended sooner than the 700 ms window: the quicker profile has nothing to
give there until inference is several times faster. The probabilities are for
loudspeaker speech through a microphone with its own processing and say
little about a person. The option is off on lamp-4ace again; the worker and
the model stay installed beside the release.

- **Rollback.** Restart without `--endpoint smart-turn`. Nothing else changes:
  - no rebuild is needed;
  - the worker and model may stay installed, unused;
  - no default, threshold or provider setting differs.
- **Excluded from the pilot**, and absent from this branch:
  - `--endpoint smart-turn-early` and the continuation hold
    (`lamp-a2-continuation`). They were parked after Main's strict
    continuation probe failed with `BarrierTimeout`
    ([result](verification/strict-continuation-20261011/README.md)). No
    speculative early endpoint is proposed until Main decides on current
    evidence.
  - The evaluation crate `lamp-endpoint-eval`.
  - Any change to the barrier policy, provider, camera or motor code.
- **Expected effect.** At most 200 ms less endpoint wait on confident finished
  turns: 77% of finished held-out turns. It does not shorten the 1.1–1.6 s from
  the endpoint to the first ALSA write.

### Recommendation

The package is ready to compose. **I recommend that the first attended
sessions run the baseline**, with the candidate enabled only in a separately
labelled session under the rule below. The reason is the continue-before-output
path:

- A person who resumes after the endpoint but before the reply is audible
  takes the interruption barrier. Every case parity adds is on this path. In
  the held-out turn model, parity takes it in 16 of 1,167 clips against the
  baseline's 9.
- **History, retained.** On 11 October, with an immediate successor Start,
  one real Lamp run and Main's strict probe both ended in `BarrierTimeout` on
  this path ([real-device-audio-pilot.md](real-device-audio-pilot.md),
  "Repeated trial"; [strict probe](verification/strict-continuation-20261011/README.md)).
- **Current base `675c71c62`.**
  - Agent 1's retained successor Start is composed behind
    `--provider-continuation kept-start`. It is off by default; the installed
    pilot configuration and the trial configuration select it.
  - Bounded kept-input accounting, with abandonment separated from
    continuation (`7a0684b2a`), and the capture-clock correction are also
    composed.
  - Three service-only probes each ran once, with cached input sent
    directly to Gemini, no microphone or speaker
    ([evidence](verification/continuation-service-20261011/README.md)):
    - immediate Start again timed out;
    - retained Start settled and answered the before-output case (Start held
      717 ms) and the during-output case.
  - These probes do not exercise the retained-cancellation defect and do not
    qualify the integration.
- **Physical.** The final native serve run used `kept-start`. It superseded
  one pending turn before output with its correction, and the correction was
  answered ([result](verification/final-serve-pilot-20261012/README.md)).
  That is one case, not a rate: physical continuation remains limited.
- **So** this path is no longer known to fail when `kept-start` is selected,
  but it is not qualified.
  - The candidate should run only with `kept-start`, as the trial
    configuration does.
  - Without it, the immediate-Start failure applies.
- The saving is at most 200 ms, while a barrier failure ends the provider
  connection.
- The [review of the six added splits](#review-of-the-six-added-held-out-splits)
  shows the cases behind this.

This is Main's decision; nothing in this package enables the candidate by
default.

## Opt-in latency trial on cached speech

The trial compares the candidate with the existing cached-speech cohorts and
changes nothing else.

- **Configuration.**
  - Use `fixtures/voice-eval-serve-endpoint-trial.example.json`. It is the
    installed pilot configuration plus `"endpoint": "smart-turn"`.
  - A dry-run test proves the two differ in nothing else and that the runner
    forwards exactly the pilot flags followed by `--endpoint smart-turn`.
  - Keep the same plan (`fixtures/voice-eval-serve-v1.json`), cached WAVs and
    manifests (no synthesis), Lamp, room, microphone selection and output
    volume as the baseline cohort it is compared with.
  - Loudspeaker and direct-human cohorts stay separate.
- **Pairing.** Alternate baseline and candidate attempts in one sitting,
  labelled per attempt. Earlier sittings are context, not a control.
- **Per turn, from the runtime's own records:**
  - `local_endpoint.includes_silence_wait_ms`: the wait actually used, 1,000,
    1,200 or 1,500 ms.
  - `local_endpoint.endpoint`: the basis (`model_complete`,
    `model_incomplete`, `model_uncertain`, `silence_fallback`) and the
    fallback reason.
  - `endpoint_worker` availability changes, and `endpoint_worker_final`
    (`reaped` must be true).
- **Measures.**
  - The distribution of waits.
  - Endpoint to first ALSA-accepted write, the boundary already reported.
  - Their sum, the same estimate Main reports (for example 1.2 s + 1.275 s in
    [pilot readiness](verification/pilot-readiness-20261012/README.md)).
  - Acoustic timing comes only from verified, listened room recordings.
- **Expected effect, and its limits.**
  - At most 200 ms less on confident completed turns.
  - The reply path after the endpoint is unchanged.
  - Different generated replies prevent a causal reply-latency claim.
- **Fallbacks kept:**
  - A missing, loading, failed, hung or late worker ends on the 1,200 ms
    window, with its reason.
  - Interruption admission, the duplex yield, playback and the provider are
    unchanged code.
  - Their suites pass on this branch: `duplex`, `pilot_faults`, and the
    endpoint full-session tests under V1-baseline and duplex.
- **Stop.** The [rollback rule](#pilot-acceptance-and-rollback-rule) applies
  unchanged. An unreaped worker fails runtime cleanup, so no later service
  cycle starts beside it.

## Source closure

Base `675c71c62`. Each commit applies alone in order. The first seven are
the reviewed `lamp-a2-pilot-limits` commits replayed onto this base; only the
fourth needed conflict resolution, against the newer duplex, notice and
motor code.

| Commit | Files | Content | Shared? |
| --- | --- | --- | --- |
| `42a07701` | `crates/turn-end-wire/**`, `crates/turn-end/**` | Worker, IPC, supervisor, features, pinned model loading, process and parity tests, fixture provenance | New crates only |
| `1d94414d` | `Cargo.toml`, `Cargo.lock`, `crates/voice/Cargo.toml` | Two workspace members; `lamp-voice` → `lamp-turn-end-wire`; 65 added lock entries, none removed or changed | Shared |
| `ae5fea33` | `crates/voice/src/activity.rs`, `crates/voice/tests/it/endpoint_confirmation.rs` | Detector seam, baseline-exact pauses, parity `ConfirmPolicy`; no early end | Shared (`activity.rs`) |
| `3de3b636` | `coordinator.rs`, `coordinator/endpoint.rs`, `options.rs`, `coordinator/tests/endpoint_session.rs`, and one `endpoint: None` field in the test `Rig` literals of `camera.rs`, `optional_ring_tests.rs`, `provider_terminal.rs`, `service_mute.rs`, `termination.rs`, `motor/tests.rs`, `tests/pilot_faults.rs` | `--endpoint smart-turn`, worker start in `spawn_rig`, traces, confirmed-reap teardown, full-session stress tests; the baseline `endpoint_policy` record keeps its fields | Shared |
| `fc55a090` | `crates/turn-end/{Cargo.toml,src/main.rs,src/lib.rs,src/reference.rs,tests/parity.rs}`, `crates/turn-end-wire/tests/process.rs` | `--check-model` self-check, Linux resource bounds, damaged-model and idle tests | New crates only |
| `afd2c52b` | `crates/voice/src/coordinator/tests/endpoint_session.rs` | Emulated endpoint sessions run one at a time | Agent 2 test file |
| `eea0ac0c` | `crates/turn-end/src/main.rs` and tests | Worker bounds never raise an inherited soft or hard limit (MAIN-059) | New crates only |
| `e053b100` | `crates/voice/src/coordinator.rs` | Baseline `local_endpoint` records keep their exact fields | Shared |
| `98075207` | `crates/voice-eval/src/{main.rs,physical/session.rs}`, `crates/voice-eval/tests/it/serve_plan.rs`, `fixtures/voice-eval-serve-endpoint-trial.example.json` | `--endpoint` as a typed runtime flag beside `--microphone`, validated by lamp-voice's parser; the trial configuration | Agent 2 crate |
| `549daeb7` | `crates/voice/src/options.rs`, `coordinator/tests/endpoint_session.rs` (and formatting in `coordinator.rs`, `coordinator/endpoint.rs`) | Smart-turn on duplex, with duplex full-session tests | Shared (`options.rs`) |
| docs commits | `docs/endpoint-detection.md`, `docs/endpoint-pilot.md`, `coordination/agent-2.md` | This package | Agent 2 files |

The hash of the docs and status commit is in
[coordination/agent-2.md](../coordination/agent-2.md).

## Asset and dependency manifest

### Model

| Field | Value |
| --- | --- |
| Source | `https://huggingface.co/pipecat-ai/smart-turn-v3/resolve/f766f81d3cfdf7737ac64aad813d91bbfd56bf93/smart-turn-v3.2-gpu.onnx` |
| Revision | `f766f81d3cfdf7737ac64aad813d91bbfd56bf93` |
| Installed name | `smart-turn-v3.2-gpu.onnx`, beside `lamp-voice` and `lamp-turn-end` |
| Size | 32,411,198 bytes |
| SHA256 | `ab8dc64b88713f90b571c15b714bd1330e6c883cad8763dacf65c9376dc539be` |
| License | BSD-2-Clause |
| Pinned in | `lamp_turn_end_wire::SMART_TURN_MODEL_SHA256` (core) and `lamp_turn_end::SMART_TURN_V3_2` (worker) |
| Not used | The int8 `smart-turn-v3.2-cpu.onnx`: tract 0.23.8 mis-executes its uint8 quantize/dequantize pairs |

The core refuses a worker whose `Ready` reports any other SHA256. The worker
refuses any other bytes before the ONNX parser reads them.

### Executables

| Binary | Links | Linux ARM64 VM size, release / stripped |
| --- | --- | --- |
| `lamp-voice` | `lamp-turn-end-wire` (no third-party crate added) | 9,665,032 bytes at `b1f43e50` (first package: 8,406,016 at `892ec0a1` → 8,506,584 bytes / 6,165,560 → 6,231,096 bytes) |
| `lamp-turn-end` | tract 0.23.8 and the crates below | 24,233,112 / 17,776,360 bytes |

### Rust dependencies

- **Toolchain.** Rust 1.96.0 (`rust-toolchain.toml`); build with `--locked`.
- **Direct dependencies of `lamp-turn-end`:**
  - `tract-onnx =0.23.8` with default features off;
  - `rustfft =6.4.1`;
  - `sha2` (workspace);
  - `rustix =1.1.5`, with features `process`, `thread` (keeping the fast
    model on the fast cores) and `time`;
  - since 2026-10-14, for the fast model: `ort =2.0.0-rc.14` (default
    features off; `std`, `load-dynamic`, `api-30`), `prost =0.14.4`
    (already in tract's tree), and `hound` and `serde_json` (workspace) for
    `lamp-turn-end agree`.
- **`lamp-turn-end-wire`** depends only on `lamp-ipc`. It adds nothing to
  `lamp-voice`'s third-party tree:
  `cargo tree --locked -p lamp-voice -e normal --target all` lists no new
  third-party package.
- **Lockfile.** The 63 third-party lock entries added against `892ec0a1` are
  listed in the [appendix](#appendix-added-lock-entries), with versions,
  licenses and checksums.
  - 55 of them are compiled into the Linux ARM64 worker.
  - The other 8 are lock entries for other targets, build scripts on other
    hosts, or optional features that are not activated.
  - All are MIT, Apache-2.0, Zlib, Unlicense, BSD-2-Clause or BlueOak-1.0.0
    licensed, alone or as alternatives.

## A733 check bundle (ready to copy)

The final ARM gate built the worker from `b1f43e50` on M2's Linux ARM64 VM
(Rust 1.96.0, Debian bookworm, glibc 2.36). The worker needs glibc 2.34 or
later. The bundle has no credentials and no audio. It is stored on M2 at
`lampOS/.cache/endpoint/artifacts/b1f43e50a/lamp-endpoint-check-b1f43e50a.tar`
(Git-ignored):

| File | Bytes | SHA256 |
| --- | --- | --- |
| `lamp-endpoint-check-b1f43e50a.tar` | 56,650,752 | `71d5506e371ae9415a5eba7ef928c3f3bd2febe9c0c059c54d197d0ab317162d` |
| `lamp-endpoint-worker` | 24,233,112 | `58852eb0697628d293af0d118614bd5fcf04c9d24a940044adcce1471b73d7ec` |
| `smart-turn-v3.2-gpu.onnx` | 32,411,198 | `ab8dc64b88713f90b571c15b714bd1330e6c883cad8763dacf65c9376dc539be` |
| `README.txt` | 1,352 | `0fed97e9fa1b71c146a68f95098a60c3aa6f12d1cd4e45baced5e85ea1c22196` |

The same gate's `lamp-voice` is 9,665,032 bytes, SHA256
`e0c306772c4ff6cd175894e8face40b6d7f3c9f7ecc74412e31d987abe61dc62`. It is not
in the bundle: root builds and composes the shared runtime. Embedded build
paths may change the hash of another build of the same commit.

On the idle Lamp, as the service user and under the service's limits:

```sh
tar -xf lamp-endpoint-check-b1f43e50a.tar && cd lamp-endpoint-check-b1f43e50a
sha256sum -c SHA256SUMS
cat /proc/loadavg; grep -E "address space|core file" /proc/self/limits
for i in 1 2 3; do ./lamp-endpoint-worker --check-model ./smart-turn-v3.2-gpu.onnx; echo "exit $?"; done
```

Before handing over, the bundle was extracted and verified in the VM and
checked once more there; it exited 0. On the VM the three gate checks gave:
- load 158–160 ms and cold 93–96 ms;
- warm median 93–96 ms and warm max 97–101 ms;
- 92–96 ms CPU per check;
- VmPeak 116,216 KiB under the 262,144 KiB address-space limit, at niceness 10.

These are not A733 numbers.

## Preparation and check (Rust only)

```sh
cargo build --locked --release -p lamp-voice -p lamp-turn-end
curl -fsSL --max-filesize 33000000 -o target/release/smart-turn-v3.2-gpu.onnx \
  https://huggingface.co/pipecat-ai/smart-turn-v3/resolve/f766f81d3cfdf7737ac64aad813d91bbfd56bf93/smart-turn-v3.2-gpu.onnx
target/release/lamp-turn-end --check-model target/release/smart-turn-v3.2-gpu.onnx
```

Install `lamp-turn-end` and the model in the same directory as
`lamp-voice`. Then run `--check-model` there, as the service user, under the
service's own limits. It runs these steps:

1. Applies the worker's Linux bounds.
2. Verifies the size and SHA256 before parsing.
3. Loads the model with tract.
4. Classifies the `voiced_9s` reference signal ten times. Every probability
   must match ONNX Runtime 1.31.0's 0.9685323 within 1e-4.

Exit 0 prints one JSON line. Exit 2 names the problem on stderr: a missing,
truncated, corrupt or different file, a load failure, or a parity mismatch. An
allocation beyond the address-space cap aborts the check (exit 134), as it
would abort the worker. Delete the file after any failure.

Example JSON line, from the Linux VM:

```json
{"model":"smart-turn-v3.2-gpu.onnx","bytes":32411198,"sha256":"ab8dc64b88713f90b571c15b714bd1330e6c883cad8763dacf65c9376dc539be","probability":0.96853244,"reference":0.9685323,"load_ms":157.3,"load_cpu_ms":157.1,"cold_ms":96.3,"warm_median_ms":96.4,"warm_max_ms":101.4,"cpu_per_check_ms":96.5,"vm_peak_kib":116176,"vm_hwm_kib":102520,"vm_rss_kib":59076,"address_space_limit_kib":262144,"nice":10}
```

Record the device's line with the pilot evidence: it is the first A733
measurement of this worker.

## Measured bounds

All rows are from the M2 Max (macOS) or its ARM64 Colima VM (6 vCPUs, Docker
`rust:1.96.0-bookworm`, NEON only). The A733 is unmeasured.

| Bound | How it is enforced | macOS | Linux ARM64 VM |
| --- | --- | --- | --- |
| Cold start: spawn to `Ready`, as the core sees it | Startup limit 10 s, then `Failed` (never restarted) and the 1,200 ms window | 164–165 ms (first launch of a new binary 1,022 ms) | 241–257 ms in 5 profiled starts; 298 ms in the gate while another test compiled |
| Model load with hash check | Inside the startup limit | 130–131 ms | 157–175 ms |
| First classification of a full 8 s window | Inside the startup limit | 36–39 ms | 96–98 ms |
| Warm classification, 8 s window, median / max of 9 | Result timeout 500 ms after submission (fallback); hang limit 2 s (kill, reap, replace at most twice) | 34–37 / 35–38 ms | 94–96 / 96–101 ms |
| Worker CPU per classification | One thread; one request at a time; Linux niceness +10 | 34–37 ms | 95–97 ms |
| Worker memory | Linux address-space cap 256 MiB; an allocation beyond it aborts the worker | 190–198 MB peak resident | 113.5 MiB peak virtual, 100 MiB peak resident, 58 MiB resident when warm |
| Repeated checks | Buffers reused | n/a | peak resident unchanged after 50 more full-window checks (`worker_process_stays_ready_and_bounded_across_idle_and_repeated_checks`) |
| Idle | Only a request in flight is timed | 5 s (default test) | Real worker idle 600 s: same process, no kill; resident 59,772 KiB and peak 102,648 KiB both when warm, after the idle and after 50 more checks. Scripted worker idle 600 s, then answers |
| Core | Three client threads and at most one 256 KB request frame; `submit` never waits | submit p50 / max 67 / 346 µs | 72 / 148 µs |
| Core files | Linux core file limit 0 | n/a | `/proc/PID/limits` checked by the idle test |

The memory cap was checked on the VM: under a 96 MiB service cap the self-check
aborted with "memory allocation of 2359296 bytes failed" (exit 134) and wrote no
core file. Under the worker's own 256 MiB cap every check passed.

Contention raises these costs. One self-check ran on the VM right after a
release build: load 334 ms, first classification 204 ms, warm median 95 ms,
warm max 130 ms and 111 ms of CPU per check. The peak memory was unchanged.

**CPU budget.**

- The detector submits at most one request per pause, 200 ms into the pause.
  Each request costs about 95 ms of one VM core.
- Ten pauses a minute would cost about 1.6% of one VM core on average, with
  bursts of one core for about 0.1 s.
- On the A733, measure the device's `cpu_per_check_ms` before judging
  contention. Under Linux's fair scheduler, niceness +10 gives each capture,
  playback and provider thread about nine times the worker's CPU share when
  they compete for a core; under the product voice's real-time policy, which
  the worker inherits, it shares their priority instead (#1608). A late
  verdict falls back to 1,200 ms.

## Stress evidence

Each row is a real process, a real pipe or the real `conversation()` loop.
Session tests run with emulated capture, privacy, provider and speaker peers.
They keep the production freshness bounds, so they run one at a time: twelve
concurrent sessions on the 6-vCPU VM starved each other's 50 ms privacy
lease.

| MAIN-057 path | Result | Tests |
| --- | --- | --- |
| Missing file | `Failed` once, never restarted; every pause 1,200 ms; session ends normally | `worker_process_refuses_missing_or_wrong_models_without_blocking` (real binary), `missing_model_is_final_and_never_restarted`, `missing_model_session_ends_on_the_unchanged_silence_fallback` (session) |
| Wrong model | A different file is refused by size or hash; a worker reporting another model is refused before any request | same real-binary test, `a_worker_reporting_another_model_is_refused_before_any_request` |
| Malformed or truncated model | A half-length copy fails "unexpected size"; a same-size copy with one flipped byte fails "pinned SHA256"; neither reaches the parser, both reaped, no restart | `worker_process_refuses_truncated_and_corrupt_models_once`, `check_model_verifies_the_installed_model_and_refuses_damaged_copies` (real binary, real model), `model_load_refuses_wrong_bytes_before_parsing` |
| Loading | A worker still loading for the whole session leaves `silence_fallback` (`loading`) at 1,200 ms | `a_worker_still_loading_leaves_the_baseline_window` (session) |
| Repeated timeout | Every verdict 700 ms late: every turn ends `silence_fallback` (`timeout`) at 1,200 ms | `verdicts_after_the_result_timeout_fall_back_on_every_turn` (session) |
| Hang | Capture, endpoint and session end are never delayed; the worker is killed, reaped and replaced | `hung_worker_never_delays_capture_endpoint_or_session_end` (session) |
| Inference crash | The failed pause falls back (`failed`), the next falls back while recovering (`recovering`), and the replacement's verdict is used again (`model_complete`) | `a_worker_failure_falls_back_through_recovery_and_then_resumes` (session), `serving_failures_are_replaced_within_budget`, `protocol_violations_are_replaced` |
| Rapid resume | Five bursts separated by 300 ms pauses, each judged complete: one request carries every burst, and the turn ends once at 1,000 ms after the last | `rapid_resumes_never_end_the_turn_early`, `complete_verdict_ends_early_but_resumed_speech_keeps_the_turn` (session) |
| Privacy shutdown and reopen | Two mute cycles: each worker is confirmed reaped and gone before the next cycle's worker exists | `mute_then_reopen_never_overlaps_endpoint_workers`, `confirmed_endpoint_exit_keeps_the_restartable_mute` |
| Unreaped worker | Cleanup fails; the mute is not restartable; no service cycle reopens | `unconfirmed_endpoint_exit_fails_mute_cleanup_and_never_reopens_service` |
| Idle longevity | The scripted worker idles 600 s (300 hang limits) and the real worker 600 s, on the VM: same process, no kill, then correct answers | `an_idle_worker_stays_ready_across_a_long_quiet`, `worker_process_stays_ready_and_bounded_across_idle_and_repeated_checks` with `LAMP_ENDPOINT_IDLE_SECS=600` |
| Normal path | A complete verdict ends input at 1.0 s of quiet and its answer plays after it | `a_complete_verdict_ends_at_one_second_and_its_answer_plays`, `real_worker_session_records_verdict_provenance` (real worker and model) |

A fallback test shows that the missing, loading and failed paths end every
turn on exactly the baseline's block (`tests/endpoint_confirmation.rs`, 40
seeds). Baseline defaults are untouched: `--endpoint` defaults to `silence`,
and `smart-turn` is refused without `--turn-taking v1-baseline`.

## Review of the six added held-out splits

The parity profile was frozen on development shards 0 and 1. These cases come
from the fresh held-out shards 4 and 5 (1,167 clips). They were reviewed after
the fact and **nothing was re-tuned on them**. Matched pause by pause against
the baseline, parity adds 6 splits and removes none.

| Clip | Source | Clip label | Speech before the pause | Pause the person left | Verdict | Parity ended at | Person resumed after the endpoint | Speech still to come |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `6d46ae83` | `mundo_1` (Spanish) | finished | 4.5 s | 1,020 ms | 0.976 | 1,000 ms | 30 ms | 1.1 s |
| `93d2e60a` | `liva_1` | finished | 4.2 s | 1,000 ms | 0.964 | 1,000 ms | 10 ms | 11.4 s |
| `7a81fec2` | `liva_1` | finished | 10.4 s | 1,110 ms | 0.984 | 1,000 ms | 120 ms | 5.0 s |
| `74a01472` | `liva_1` | unfinished | 9.9 s | 1,100 ms | 0.971 | 1,000 ms | 110 ms | 5.4 s |
| `89e462c6` | `liva_1` | unfinished | 6.5 s | 1,090 ms | 0.990 | 1,000 ms | 100 ms | 6.7 s |
| `9027a54c` | `liva_1` | unfinished | 2.0 s | 1,130 ms | 0.990 | 1,000 ms | 140 ms | 9.3 s |

All six cases share one signature:

- **The pause.** The person paused for 1.00–1.13 s after audio the model
  heard as finished (0.96–0.99), then went on with 1–11 s more speech.
- **Why the baseline kept the turn.** It waits 1,200 ms, and every one of
  these pauses was shorter than that.
- **Where they occur.**
  - Five are in `liva_1`, long-form English recordings of 12–16 s with
    several sentences.
  - One is Spanish.
  - None is in the 300 short-form English clips.

There are no transcripts, so whether the first part was a complete sentence
cannot be checked.

What the pilot would do in each case:

1. Input ends at 1.0 s. The person resumes 10–140 ms later, long before a reply
   is audible.
2. The existing path cancels the pending reply (`user_interrupted`). It then
   admits the continuation as an interruption, behind the provider barrier.
3. History: with an immediate successor Start, Main's probe and one real Lamp
   run showed `BarrierTimeout` on this before-output path (11 October). The
   2 s settlement bound expired while the successor was still speaking.
4. Current evidence covers `kept-start` on `675c71c62`:
   - one service-only probe settled and answered a before-output
     continuation;
   - one native serve run superseded a pending turn before output with its
     correction.

   Neither is a rate. Every one of these six continuations lasts over 1 s,
   and five last 5 s or more, longer than the tested cases.
5. So in the pilot each case like these still takes the barrier. With
   `kept-start` it is expected to settle, but physical evidence is limited.
   A failure would cost the provider connection, not only the person's whole
   thought.

### Rationale for keeping the profile conservative, without re-tuning

- **The development set.** It showed the same balance: +4 and −3 splits on
  1,180 clips. The held-out rate (0.5% of clips) confirms that estimate; it
  is not a new finding to optimise against.
- **Other frozen profiles.**
  - The `strict` profile (≥ 0.99, ending at 800 ms) would have avoided four
    of these six. On development data it ended only 16% of finished turns
    early, which is why parity was frozen instead.
  - Choosing `strict` now because of these six cases would be selection on
    the held-out set, so it is not proposed.
  - If Main wants fewer barrier exposures, the baseline is the honest
    choice.
- **Why not a lower commit.** Every step below 1,000 ms adds splits
  quickly: the 400 ms profile adds 338 on the same held-out set.
- **Parity's other choices protect the baseline.**
  - Clearly unfinished verdicts wait 1,500 ms.
  - Every missing or late verdict keeps 1,200 ms.
  - Any speech-scored block keeps the turn.
- **Unfinished clip ends.** Parity ended 19 of 569 unfinished clip ends
  before 1,200 ms; the baseline ended none. A clip ends with no continuation,
  so these are not counted as splits. In a live conversation the person
  would continue after more than 2.5 s, past either endpoint.

## Played on lamp-4ace with the fast worker, 2026-10-14 afternoon

The five pause scenes (`pauses.sh`: two plain questions, and thoughts with
a 450, 650 and 900 ms silence inside), synthetic voices from the office
iMac, the voice's other settings as on the lamp that day. One run each.

| Run (local time) | The voice | Answered | From the end of speech to the first word, answered ones |
|---|---|---|---|
| 13:03 | 600 ms of quiet, no model | 2 of 5 (the first held by the busy-room rule) | 2.1, 1.9 and 2.9 s |
| 13:30 | `--endpoint smart-turn-fast` | 4 of 5 | 0.4 s of quiet, then 1.1 to 1.7 s |
| 13:32 | the same | 0 of 5 | (the name cut off; below) |
| 13:42 | the same, with the floor below | 4 of 5 | Tokyo 1.3 s, spider 1.4 s |

The model took "Hey Lamp," said alone for a finished turn (p 0.99, 0.98)
and ended it after 400 ms. Kept-start continuation then healed it where
the room was quiet, but in a busy room the question that followed had no
name in it, and the voice's rule held its answer (13:32, four times). So
since 13:41 the policy's early commit counts only for an utterance with at
least 80 speech blocks (`ConfirmPolicy::least_speech_blocks`, 0.8 s,
`bea2dd1c9`); a shorter one waits the full 600 ms as before. The beach
scene (450 ms inside a sentence) is cut and goes unanswered in both fast
runs that answered; the synthetic voice ends each piece with a falling
tone, which is what this model listens for, so a person's pause is likely
kinder to it. Not shown: people's own voices and pauses.

The fast worker is on, on lamp-4ace, since 13:21 (`--endpoint
smart-turn-fast`; with the floor since 13:41).

### Short lines asked (`LAMP_ENDPOINT_SHORT_VERDICT`, #2135, 21 Oct)

Under that floor a line is never sent to the model: it ends on the 600 ms
window as `silence_fallback` with `fallback: short`. On lamp-4ace in the 9
hours to 21 Oct 04:56 that was 71 of 635 Ends (11%), every fallback of the
night: speech blocks p10/p50/p90 24/77/119, 610/1,171/1,601 ms from
admission to End, 51 of them under 1.5 s (the first builder on #1722).
Right for "Yes."; wrong for a line short only because the person paused
early.

With `LAMP_ENDPOINT_SHORT_VERDICT=on` (off unless set), a line of at least
20 speech blocks (0.2 s, `SHORT_VERDICT_BLOCKS`) and under the floor is sent
too (`ConfirmPolicy::short_verdict_blocks`). Its verdict ends it no sooner:

- a complete verdict still waits for the 600 ms window, as the floor says,
  so "Yes." ends as before;
- an uncertain verdict, or none in time, ends it there too;
- only a clearly unfinished one (under 0.01) holds it, to the 1.5 s
  `incomplete_blocks`, so the rest of "Hey Lamp, my…" stays in the line.

Each End of such a line says how many times it was sent under the floor
(`short_asks`, `local_endpoint.endpoint`): the inferences the switch adds,
one a pause (134 ms, 145 ms round trip on lamp-4ace on 21 Oct), so a line
that pauses twice before the floor counts two (the reviewer's P3 on
#2380). The floor was set on 14 Oct when, in a talkative office, the model
took a whole core and the board reached 76 °C. The endpoint runs before
the room's rules, so in a busy room every short line of other people's
talk is asked too: the cost is measured in a talkative room, not only in
the played test's empty one (below). If that shows heat, short lines are
skipped while the room is busy, as a next step.

Not covered: a line under 20 speech blocks. Round 1 of `open-name.sh` at
03:11 (turn 68) had 12, its name missed by `lamp-name` (#694); and "Hey
Lamp," said alone, which the model takes for finished (p 0.99 above), is
the call wait's (`--call-wait-ms`).

Tests: `with_short_verdicts_an_unfinished_short_line_waits_and_a_finished_one_ends_as_before`
(`tests/it/endpoint_confirmation.rs`) and
`coordinator::endpoint::tests::short_lines_are_asked_only_with_the_switch_and_a_floor`.

For a lamp, both ways: the played test's `silence_fallback` count and
`short_asks` sum; `open-name.sh`; and the day's short replies ("Yes. …
Please start it.") ending no later. Then, with the switch on, a stretch of
two people talking (#829's `twopeople829.sh`, or an hour at the desk), with
the `short_asks` sum, the worker's CPU and the board's temperature beside
the same stretch with it off (the reviewer's P3 on #2380).

### A lower floor, the start counted (`LAMP_ENDPOINT_LEAST_SPEECH_MS`, #2602, 22 Oct)

On lamp-4ace a third of whole played day lines ended as `silence_fallback`
with `fallback: short`: never asked, ended at the 600 ms window, not the
400 ms commit (the first builder on #2602, 22 Oct 04:28). Two things keep
them under the floor:

- the floor itself, 0.8 s, set for "Hey Lamp," said alone (about 0.6 s,
  14 Oct above), which the call's wait (`--call-wait-ms`, #782) now holds
  for 2 s when a line is only the name;
- the count: the confirmation counts speech blocks (scored 0.35 or more)
  only after the line starts, so the six start blocks (`START_BLOCKS`) never
  count, and a line needs 0.86 s of speech to be asked.

With `LAMP_ENDPOINT_LEAST_SPEECH_MS=MS` (off unless set), a whole number of
ms from 500 to the profile's own floor (800 with `--end-silence-ms 600`):

- the floor is `MS` (`ConfirmPolicy::least_speech_blocks`, MS / 10 blocks),
  and the short lines' floor (`LAMP_ENDPOINT_SHORT_VERDICT`) is kept under it;
- the line's start counts toward it (`ConfirmPolicy::count_start`): the
  count begins at the speech blocks the detector heard to start the line.

Not under 500 (the first builder's P2 on #2751, 22 Oct 06:10): at 400
the replay committed "Hey Lamp," said alone, the line the floor was set
for. At 500 a 0.6 s line is over the floor and a complete verdict commits
it at 400 ms; what holds "Hey Lamp," alone is the call's wait, which the
lamp runs (`--call-wait-ms 2000`), so the switch is for a lamp with it.

Anything else (not a number, under 500, over the profile's floor, or the
parity profile, which has none) leaves the floor as it is, and the voice
says so once on stderr; with it on, it says the floor once. Off, the policy
and its record are as before (`count_start` is absent from
`endpoint_policy` when off).

Every End now says the speech the confirmation counted (`speech_ms`,
`local_endpoint.endpoint`, with the switch on or off), so a day's
`silence_fallback: short` lines show how far under the floor each was, and
the replay (`start` below) can try floors on the same lines.

Tests: `a_lower_floor_ends_a_short_finished_line_at_the_commit` and
`the_start_counts_toward_the_floor_only_with_the_switch`
(`tests/it/endpoint_confirmation.rs`), and
`coordinator::endpoint::tests::the_floor_is_lowered_only_by_ms_from_500_to_the_profiles`;
`the_name_alone_still_waits_for_the_rest_with_a_lower_floor` shows the
call's wait holding "Hey Lamp," and a plain 0.6 s line committed.

For a lamp: the played day both ways, the `silence_fallback: short` count
and the `speech_ms` of each; "Hey Lamp," alone with `--call-wait-ms` (it
must still wait for the rest); the cut-ins of #782's comma scenes; and the
worker's CPU and the board's temperature in a talkative stretch, since
more lines are asked.

## Cut at a comma (#782, 2026-10-15)

Measured by the fourth builder on its Intel Mac, from the first builder's
records of the 44 scenes played to lamp-4ace on 15 October; nothing was run
on a lamp.

**How often.** Of 158 ends of speech in those scenes, 84 were the fast
model's (`model_complete`, 400 ms at 0.9 or more). Ten of them came while a
played line was still going on, nine of those at a comma inside it (p 0.91
to 0.99), and in every one the rest of the line came in within 2 s as a new
turn. The 600 ms window cut two more lines, just after "Hey Lamp," (shorter
than the 0.8 s floor). A line counts as cut when the quiet the end waited
for began more than 0.25 s before the line's end, both taken from each line
rendered again by `say` to a file; the place of each cut is found the same
way, rendering the line up to each comma or full stop. The six cuts at a
full stop are the two-breath scenes, said that way on purpose.

**Why there.** The same lines rendered and replayed digitally through the
voice's own detector and the pinned model (below) get the same verdicts at
those commas, 0.92 to 0.99, but the person is speaking again before 400 ms
of quiet, and only one of the nine is cut. As lamp-4ace heard them, the
comma pauses were 400 to 560 ms (the rest's admission less 60 ms), 50 to
150 ms longer than `say` makes them. Adding the room's −28 dB floor and its
400 ms hold to the replay does not make up the difference: what lengthens
the pause is the way from the loudspeaker to the voice microphone. People
pause longer still: inside one speaker's talk the median pause is 580 to
640 ms (Heldner and Edlund 2010), so no early end under about 600 ms leaves
them whole, as the held-out clips above show for 400 ms (+338 splits).
Voice agents that use Smart Turn or models like it hold an unfinished-sounding
turn for 2.5 to 3 s (LiveKit, Pipecat) and end a finished one after 0.2 to
0.7 s; LiveKit's own bench puts Smart Turn v3.2 at 35% cut off at 300 ms
and 15% at 600 ms.

**What a cut costs here.** Kept-start joins the two parts, and the answer is
right. The reply comes as soon as for an uncut turn whose last verdict was
the same. What is lost is that only the second part is heard as text, and
that in a busy room the second part has no name in it, so the busy-room rule
holds it. That second loss is what `LAMP_CUT_KEEPS_NAME` mends
([attention.md](attention.md), "A cut keeps its name").

**The commit, as a switch.** `--endpoint-commit-ms MS` sets how long a pause
must last before a confident verdict ends the turn (`ConfirmPolicy::commit_blocks`,
a multiple of 10 ms). Without it the profile keeps its own: 400 ms under a
window of a second or less, as lamp-4ace runs, one second above it. It needs
an `--endpoint` model and must be shorter than the end-of-speech wait in
force; the voice refuses to start otherwise. The record of the policy
(`endpoint_policy`) says the commit in force. Measured on the 1,167
held-out human clips (digital replay, the real detector, lamp-4ace's 600 ms
window, verdicts 130 ms late), every 50 ms the commit waits:

| Commit | Splits (the person went on) | Clips with a split | Wait at a finished end, median (mean) |
|---|---|---|---|
| 400 ms, as now | 527 | 384 | 400 ms (468) |
| 450 ms | 474 | 350 | 450 ms (504), +50 |
| 500 ms | 424 | 320 | 500 ms (539), +100 |
| 550 ms | 369 | 285 | 550 ms (571), +150 |
| none, the 600 ms window only | 330 | 258 | 600 ms (606), +200 |

So the 400 ms commit adds 197 splits to what the window alone makes, and
550 ms takes 158 of them away (80%) for 150 ms more at every confident end.
Most of the rest are the window's own: at 600 ms it ends pauses that the
1,200 ms baseline above keeps (13 splits there), nearly all of them in the
long recordings (`liva_1`, 311 of the 330). Kept-start continues those.

On the scenes played to lamp-4ace, the comma pauses it heard (400 to 560 ms)
say what a commit would have kept whole: 450 ms 1 of the 9, 500 ms 6, 550 ms
7, and only the window 9. The window itself still ends the two "Hey Lamp,"
pauses of 600 and 630 ms.

**Holding on a rise.** Whether the last 300 ms before the quiet rose in
pitch or level does not tell a pause from an end. In the rendered lines,
`say` falls before a comma (pitch −3.6 semitones per 100 ms at the median,
rising in 12%) and rises before the end of a question (47%). In the
held-out human clips, pitch rose before 55% of the pauses the person went
on after and before 49% of finished ends, and the level fell before both.
A rule built on it would hold about as many finished turns as it keeps
pauses whole, so it was not built.

**The replay.** `cargo run --release -p lamp-voice-eval --example
turn_end_replay -- MODEL CLIPS.json --profile NAME:COMMIT_MS:P:LEAST_MS[:recheck][:short][:start] …`
feeds 16 kHz mono WAV files through `TurnDetector` with the model in its own
process, each verdict handed over a fixed 130 ms later on the replay's own
clock, and prints each turn's start, end and decision, and every verdict.
`--speech-level-db` and `--faint-floor-db`/`--faint-hold-ms` add the
voice's own level and floor. Nothing is played. A profile's three flags set
the policy as the lamp would:
- `recheck` asks an uncertain verdict again at once, with the pause so far, and a complete second ends the line at the commit (`ConfirmPolicy::recheck_uncertain`, off on every lamp, with no switch). A turn it ended says `first_p`, the first verdict's probability.
- `short` asks lines under the commit's floor from 0.2 s, as `LAMP_ENDPOINT_SHORT_VERDICT` does. Each turn says its `short_asks`.
- `start` counts the line's start toward `LEAST_MS` (`ConfirmPolicy::count_start`), as `LAMP_ENDPOINT_LEAST_SPEECH_MS` does with its floor in `LEAST_MS`; so `lower:400:0.9:500:start` is the switch at 500 beside `lamp:400:0.9:800`. Each turn says its `speech_ms`, as counted.

`--unfinished-below P` and `--unsure-ms MS` are the night dials, `LAMP_ENDPOINT_UNFINISHED_BELOW` and `LAMP_ENDPOINT_UNSURE_MS`, read as the voice reads them (`lamp_voice::activity::unfinished_below` and `unsure_ms`: 0.01 to 0.6, and 600 to 1500 ms in steps of 10). A value out of range is refused, not ignored. With either, each clip and profile is replayed twice, `"dials": "before"` as the profile is and `"dials": "after"` with them, and each line says the `unfinished_below` and `unsure_ms` it ran with (0 is the silence window). Each turn's `basis`, `p` and `silence_ms` are its end's.

On 21 Oct (#2135, #1722 07:45) neither paid. On 53 real-speech lines the second ask ended 1 line 140 ms sooner. On 428 played lines it ended 12 sooner and cut 2 in two. The short verdict changed no End on either, for 340 more inferences. Both are kept to measure the next real bundles. Its evidence for #782 is in
[verification/comma-pause-20261015/](verification/comma-pause-20261015/).

## A bare call ends nothing (#782, 2026-10-15)

Off unless the voice is started with `--call-wait-ms MS` (built by the
fourth builder, as the first builder asked on #782 at 21:53). An utterance
that is only the lamp's name so far ("Hey Lamp," or "Lamp,", with or
without a greeting) waits that long for the rest instead of ending at the
window or at a confident verdict. When the rest comes, it is the same
utterance, and its own pause ends it as any other. When nothing comes, it
ends when the wait runs out, and its `local_endpoint` record says
`call_wait_ms`.

Why: on lamp-4ace at 21:47 and 21:50 (`cutname.sh`, two people trading
lines, then "Hey Lamp, [600 ms] what is the capital of Japan?"), each of
three rounds' first piece was "Hey Lamp," ended by `silence_fallback` at
600 ms. The question was then dropped by the busy room's floor hold, or the
name alone was answered.

How it knows it is a call:
- the first sign of the lamp's name from `lamp-name` (`name/rising` on the
  bus, so it needs `LAMP_BUS=on`), beginning no more than 0.5 s before the
  utterance began (`CALL_BEFORE_US`). Not `name/heard`: on lamp-4ace on 15
  October, 23:02 to 23:05, with this option, `name/heard` came 0.08 to 0.19
  s after the 600 ms window had ended "Hey Lamp," in all four calls, and no
  wait began ([name.md](name.md), "Said twice");
- the utterance so far holds at most 1.2 s of speech
  (`CALL_SPEECH_BLOCKS`). "Hey Lamp," at the office's rate is about 0.7 s;
- it waits once per utterance;
- the first sign comes while the name is still being said, so the call's
  own words go on for a moment after it: a gap under 0.2 s
  (`CALL_PAUSE_BLOCKS`) keeps the wait. Speech after a quiet of 0.2 s or
  more is the rest, and ends as any other utterance does, and so does an
  utterance that grows past 1.2 s of speech, which is more than a call
  (the reviewer's P2 on #891: a sign at 0.35 s into a 0.7 s call had let
  the call's next word end the wait).
- a hand Stop ends the wait, and a sign of the name that comes after the
  Stop cannot arm one for the same utterance. The abandoned utterance
  drains to the ordinary end-of-speech wait, and only a new utterance can
  wait again (the reviewer's P2 on #891 at 04:30: Stop had left the input
  draining for the whole 2 s, and a fresh command right after it was
  dropped).

The wait must be longer than the end-of-speech wait in force and 3 s at
most, or the voice refuses to start. The `call_wait_policy` record says
the wait in force, and a `call_wait` record says each time one began.

While it waits the turn is still being listened to, so the eye shows
Listening as it does for any utterance, and the body's look at a heard
name (`--look-at-name`, on by default) glances up. The cost is that a
false hearing of the name in a short utterance ("Yeah.") delays its end by
up to the wait.

**When nothing more comes** (the first builder, #782, 00:49): "Hey Lamp."
alone waited its 2 s on lamp-4ace at 00:31 and then got no answer. A person
who hears their name and nothing more says "Yes?" and waits. So when the
wait runs out with only the call heard, the coordinator sends the worker
`ProviderInput::Called` just before the request's end, and the worker tells
the model beside that request, with whatever else the request was told
(`CALLED_NOTE`: "The person said your name, Lamp, and nothing more: they
are calling you. Answer as someone who hears their name does, with one
short word such as "Yes?", and wait for them to go on."). It is sent only
then; an end sent early (`--early-end-ms`) is past telling. It isn't sent
for a local-only turn (#830), whose request never went out, or when the
queue to the worker is already at its bound (64 commands with the standard
hold): the end keeps the one slot past the bound that it always had, and
the note is left out. The worker keeps it for the request's end, and
sends the two together, the note first, only when the link's input queue
to Gemini has room for both; with room for one, the end goes alone, so it
never meets a queue the note filled (the reviewer on #992). The second
voice and the fixture provider take it and do nothing. Tests:
`a_bare_call_is_told_to_the_model_before_its_end` (the note reaches the
service before the end),
`only_a_call_whose_wait_ran_out_is_told_to_the_model` and
`a_bare_calls_note_goes_with_its_end_only_when_both_fit`. Not yet measured on
the lamp: "Hey Lamp." alone five times, and the busy-room scene again.

Tests (`crates/voice/src/coordinator/tests/call_wait.rs`, `crates/voice/src/options.rs`):
- only the name waits, and its record says so;
- the rest is the same utterance, and ends at its own pause;
- the wait's bounds, and a name heard long before;
- the option is refused at or under the window.

Through the coordinator's handling of `name/rising` on a real bus
(`coordinator/tests/call_wait.rs`): a first sign 0.23 s into the quiet, as
`lamp-name` would send it, makes the call wait its two seconds; without the
option it ends at the window; a sign after the window is too late. Through
the real hand Stop (`coordinator/tests/hand_controls.rs`,
`a_hand_stop_ends_a_calls_wait_in_either_order_and_a_fresh_command_is_heard`):
Stop after the sign, and a sign after Stop, both drain at 600 ms, and a
fresh command after it is a new candidate. Not tested on the host: the voice opening that subscriber from its environment
(`LAMP_BUS=on`), which a run on the lamp does.

## Pilot acceptance and rollback rule

Applies to the candidate (`--endpoint smart-turn`) only.

### Before the first candidate session

1. **Device self-check.** `lamp-turn-end --check-model` on the device,
   run as the service user and under its limits, exits 0.
2. **Bounds.** Its `warm_max_ms` is below 400. A slower device falls back on
   most pauses and cannot show a benefit, so keep the baseline.
3. **Paired baseline.** A baseline session (no `--endpoint`) on the same
   build, device and placement has been recorded.

### Roll back at once

Restart without `--endpoint smart-turn` on any one of these:

1. **Barrier failure after a model endpoint.** A `local_endpoint` with
   `endpoint.basis` `model_complete` is followed, for the same turn, by
   `turn_finished` with outcome `user_interrupted` before any
   `speaker_first_write` for that turn. That turn, or the next, then ends in a
   provider failure (`BarrierTimeout`, `provider_turn_failed`, or a
   disconnection).
2. **Wrong build or configuration.** Any of:
   - `endpoint_policy` is not the parity profile;
   - a `local_endpoint` has `includes_silence_wait_ms` below 1,000;
   - any `model_*` decision has no verdict provenance;
   - the model hash is not `ab8dc64b…539be`.
3. **Unreaped worker.** Any of:
   - `endpoint_worker_final` with `reaped` false;
   - a cleanup failure naming the endpoint worker;
   - a `lamp-turn-end` process alive after `lamp-voice` has exited.
4. **Audio disturbed.** More `playback_gap` events or capture faults per turn
   than in the paired baseline session.
5. **Continuations after model endpoints.** Three or more interrupted
   `model_complete` turns (as in trigger 1, even without a provider failure)
   in the first 100 candidate turns. In the held-out turn model, parity
   interrupts at most about 1.4 turns per 100 clips on any basis, 0.6 of them
   added by parity. Three or more `model_complete` cases point to a higher
   rate than the evidence.
6. **Cut-off reports.** Any cutoff the person or the facilitator reports on a
   turn whose `endpoint.basis` is `model_complete`.

### Accept for further supervised use

After at least 100 candidate turns over at least three sessions, all of these
must hold:

- no rollback trigger fired;
- in every session the worker reached `ready` before the person's first
  pause, with no `failed` and at most one `recovering`;
- at least 50% of finished turns ended `model_complete` at 1,000 ms
  (77% held-out);
- `silence_fallback` with `timeout` on fewer than 5% of pauses;
- in the facilitator's feedback, no more cut-off reports than in the
  baseline sessions.

**No go.** If the benefit criteria fail without a rollback trigger (for
example, frequent `timeout` fallbacks on the A733), keep the baseline. That is
a missing benefit, not a safety finding.

Keep every attempt, as [supervised-pilot.md](supervised-pilot.md) requires.
Record for each session:

- `endpoint_policy`;
- every `endpoint_worker` change;
- each `local_endpoint.endpoint`;
- `endpoint_worker_final`;
- the device's `--check-model` line.

## Appendix: added lock entries

Against `892ec0a1`, `Cargo.lock` gains these 63 third-party entries and the two
workspace crates `lamp-turn-end` and `lamp-turn-end-wire`. No entry is removed
or changed. "Compiled" means the package is in
`cargo tree --locked -p lamp-turn-end -e normal --target aarch64-unknown-linux-gnu`.

| Crate | Version | License | Compiled | Cargo.lock checksum (SHA256) |
| --- | --- | --- | --- | --- |
| `anymap3` | 1.1.0 | BlueOak-1.0.0 OR MIT OR Apache-2.0 | yes | `fb5dfbc6d8d2675589ccbe4d0fd61df2419075625f8c1a62325e718e2b0049f9` |
| `bit-set` | 0.11.1 | Apache-2.0 OR MIT | yes | `56d87354e4229f54a44f7bf2435906a4656dba36026ab6eaca629a2c436a691c` |
| `bit-vec` | 0.10.1 | Apache-2.0 OR MIT | yes | `5727b15fa97d4f4fee0a3b7c3d550ed0269f54329207b86388de918604e31269` |
| `borsh` | 1.8.1 | MIT OR Apache-2.0 | no | `553c5d846a6ba5150c65e3b1b8ec073bcf1abc20f9b7220de384a4443ea4e20a` |
| `borsh-derive` | 1.8.1 | Apache-2.0 | no | `12cdfe656708a01f89b451a7d36466e6fe6c414de0aa18fc54f864f6f9ca9f56` |
| `byteorder` | 1.5.0 | Unlicense OR MIT | yes | `1fd0f2584146f6f2ef48085050886acf353beff7305ebd1ae69500e27c67f64b` |
| `crunchy` | 0.2.4 | MIT | no | `460fbee9c2c2f33933d720630a6a0bac33ba7053db5344fac858d4b8952d77d5` |
| `derive-new` | 0.7.0 | MIT | yes | `2cdc8d50f426189eef89dac62fabfa0abb27d5cc008f25bf4156a0203325becc` |
| `downcast-rs` | 2.0.2 | MIT OR Apache-2.0 | yes | `117240f60069e65410b3ae1bb213295bd828f707b5bec6596a1afc8793ce0cbc` |
| `dyn-clone` | 1.0.20 | MIT OR Apache-2.0 | yes | `d0881ea181b1df73ff77ffaaf9c7544ecc11e82fba9b5f27b262a3c73a332555` |
| `dyn-hash` | 1.0.0 | MIT OR Apache-2.0 | yes | `5fdab65db9274e0168143841eb8f864a0a21f8b1b8d2ba6812bbe6024346e99e` |
| `erased-serde` | 0.4.10 | MIT OR Apache-2.0 | yes | `d2add8a07dd6a8d93ff627029c51de145e12686fbc36ecb298ac22e74cf02dec` |
| `filetime` | 0.2.29 | MIT/Apache-2.0 | yes | `5c287a33c7f0a620c38e641e7f60827713987b3c0f26e8ddc9462cc69cf75759` |
| `foldhash` | 0.2.0 | Zlib | yes | `77ce24cb58228fbb8aa041425bb1050850ac19177686ea6e0f41a70416f56fdb` |
| `half` | 2.7.1 | MIT OR Apache-2.0 | yes | `6ea2d84b969582b4b1864a92dc5d27cd2b77b622a8d79306834f1be5ba20d84b` |
| `hashbrown` | 0.16.1 | MIT OR Apache-2.0 | yes | `841d1cc9bed7f9236f321df977030373f4a4163ae1a7dbfe1a51a2c1a51d9100` |
| `inventory` | 0.3.25 | MIT OR Apache-2.0 | yes | `6928282826c822ad91bf1c9a1cb90a30ba1c26770749929b4656cd6be829cd7c` |
| `itertools` | 0.15.0 | MIT OR Apache-2.0 | yes | `8b4baf93f58d4425749ca49a51c50ebab072c5df6994d08fed93541c331481dc` |
| `lazy_static` | 1.5.1 | MIT OR Apache-2.0 | yes | `20870f649af7073d53e38067b2a84312175d56ea15217e1b15bc83506ec50afb` |
| `libm` | 0.2.16 | MIT | yes | `b6d2cec3eae94f9f509c767b45932f1ada8350c4bdb85af2fcab4a3c14807981` |
| `lock_api` | 0.4.14 | MIT OR Apache-2.0 | yes | `224399e74b87b5f3557511d98dff8b14089b3dadafcab6bb93eab67d3aace965` |
| `maplit` | 1.0.2 | MIT/Apache-2.0 | yes | `3e2e65a1a2e43cfcb47a895c4c8b10d1f4a61097f9f254f183aee60cad9c651d` |
| `matrixmultiply` | 0.3.11 | MIT/Apache-2.0 | yes | `3f607c237553f086e7043417a51df26b2eb899d3caff94e6a67592ff992fedc7` |
| `memmap2` | 0.9.11 | MIT OR Apache-2.0 | yes | `d1219ed1b7f229ee7104d281dd01d6802fe28bb6e95d292942c4daacdeb798c0` |
| `memo-map` | 0.3.4 | Apache-2.0 | no | `5449c8c750f1a07ea702bbd212bd999fceece9b3d1508b17023b3e174583124b` |
| `minijinja` | 2.24.0 | Apache-2.0 | no | `86886cf6dbf4e614b19c9a1eec9775f021869d7eadde0fc73921a81b90c9b4c9` |
| `ndarray` | 0.17.2 | MIT OR Apache-2.0 | yes | `520080814a7a6b4a6e9070823bb24b4531daac8c4627e08ba5de8c5ef2f2752d` |
| `nom` | 8.0.0 | MIT | yes | `df9761775871bdef83bee530e60050f7e54b1105350d6884eb0fb4f46c2f9405` |
| `nom-language` | 0.1.0 | MIT | yes | `2de2bc5b451bfedaef92c90b8939a8fff5770bdcc1fafd6239d086aab8fa6b29` |
| `num-bigint` | 0.4.8 | MIT OR Apache-2.0 | yes | `c89e69e7e0f03bea5ef08013795c25018e101932225a656383bd384495ecc367` |
| `num-complex` | 0.4.6 | MIT OR Apache-2.0 | yes | `73f88a1307638156682bada9d7604135552957b7818057dcef22705b4d509495` |
| `num-integer` | 0.1.47 | MIT OR Apache-2.0 | yes | `7ce2d95d4b3734dc35aa2f45e1aa22cd416814592a4f9d9205e11affd5b8e10b` |
| `num-rational` | 0.4.2 | MIT OR Apache-2.0 | yes | `f83d14da390562dca69fc84082e73e548e1ad308d24accdedd2720017cb37824` |
| `parking_lot` | 0.12.5 | MIT OR Apache-2.0 | yes | `93857453250e3077bd71ff98b6a65ea6621a19bb0f559a85248955ac12c45a1a` |
| `parking_lot_core` | 0.9.12 | MIT OR Apache-2.0 | yes | `2621685985a2ebf1c516881c026032ac7deafcda1a2c9b7850dc81e3dfcb64c1` |
| `portable-atomic` | 1.15.0 | Apache-2.0 OR MIT | no | `05c8b63e8d9609db387f0324918f81d68fe27748f084ef092fb35954d0539a85` |
| `portable-atomic-util` | 0.2.8 | Apache-2.0 OR MIT | no | `10ab3eb7f3becc3a1cbc4f2c6f20267996cfc1a6467a873763411b136a122715` |
| `primal-check` | 0.3.4 | MIT OR Apache-2.0 | yes | `dc0d895b311e3af9902528fbb8f928688abbd95872819320517cc24ca6b2bd08` |
| `prost` | 0.14.4 | Apache-2.0 | yes | `528ac67416ff8646872a3c02cad9cc4ee5dc9f9540c9b10771855c95cb2e5ae1` |
| `prost-derive` | 0.14.4 | Apache-2.0 | yes | `b570b25f7617e43d59005d0990ccb79e950a423952cea19671b7a876da390adf` |
| `rand_distr` | 0.6.0 | MIT OR Apache-2.0 | yes | `4d431c2703ccf129de4d45253c03f49ebb22b97d6ad79ee3ecfc7e3f4862c1d8` |
| `rawpointer` | 0.2.1 | MIT/Apache-2.0 | yes | `60a357793950651c4ed0f3f52338f53b2f809f32d83a07f72909fa13e4c6c1e3` |
| `redox_syscall` | 0.5.18 | MIT | no | `ed2bf2547551a7053d6fdfafda3f938979645c44812fbfcda098faae3f1a362d` |
| `rustfft` | 6.4.1 | MIT OR Apache-2.0 | yes | `21db5f9893e91f41798c88680037dba611ca6674703c1a18601b01a72c8adb89` |
| `scopeguard` | 1.2.0 | MIT OR Apache-2.0 | yes | `94143f37725109f92c262ed2cf5e59bce7498c01bcc1502d7b9afe439a4e9f49` |
| `simd-adler32` | 0.3.10 | MIT | yes | `3a219298ac11a56ea9a6d2120044824d6f01aeb034955e7af7bc16858527deea` |
| `smallvec` | 1.16.3 | MIT OR Apache-2.0 | yes | `5b3dc8af474f516a851ff4bd12db780f948b9250ad37211e4eec0bccea54e01b` |
| `strength_reduce` | 0.2.4 | MIT OR Apache-2.0 | yes | `fe895eb47f22e2ddd4dabc02bce419d2e643c8e3b585c78158b349195bc24d82` |
| `string-interner` | 0.20.0 | MIT/Apache-2.0 | yes | `ad3df9b59e2eded8d825c7c4363ad339a20fb6bc0b9a4778560f518f59910b15` |
| `tar` | 0.4.46 | MIT OR Apache-2.0 | yes | `3f6221d9a6003c78398e3b239969f352578258df48c8eb051caadae0015bc840` |
| `tract-core` | 0.23.8 | MIT OR Apache-2.0 | yes | `0a5a9e07e863fc28355436c5a48fe859784832094ba1ff410357396ae87ff1e3` |
| `tract-data` | 0.23.8 | MIT OR Apache-2.0 | yes | `a33bafcf1d3dfa404c3271dd08304b162deb8fcfe9536290fb97e578c32d5dd5` |
| `tract-extra` | 0.23.8 | MIT OR Apache-2.0 | yes | `2a05c86ece0c47880c7726e08dabb72d68e9cba0cd36d44bc0aa90e2291a05db` |
| `tract-hir` | 0.23.8 | MIT OR Apache-2.0 | yes | `a52e3ec1f3beb16d552806386e123cf5632d4206154695a80aaecdb6ee2c5128` |
| `tract-linalg` | 0.23.8 | MIT OR Apache-2.0 | yes | `1cc37ab328b95cef8eef98324122e9f956fd3ca197f0892e09ff90fe495fa222` |
| `tract-nnef` | 0.23.8 | MIT OR Apache-2.0 | yes | `5d95e30d7e6c2f73516212367a5b49354c105b52285e8919ec439c84556f54b3` |
| `tract-onnx` | 0.23.8 | MIT OR Apache-2.0 | yes | `5a316674683d04c6070146badf57a7b8b9d99d4e38f586cbe3214e553d70bc2d` |
| `tract-onnx-opl` | 0.23.8 | MIT OR Apache-2.0 | yes | `091e126c52d608214f8784f468c7d0be44459e7a369fb9a26f92aa593a78f4a5` |
| `transpose` | 0.2.3 | MIT OR Apache-2.0 | yes | `1ad61aed86bc3faea4300c7aee358b4c6d0c8d6ccc36524c96e4c92ccf26e77e` |
| `typeid` | 1.0.3 | MIT OR Apache-2.0 | yes | `bc7d623258602320d5c55d1bc22793b57daff0ec7efc270ea7d55ce1d5f5471c` |
| `xattr` | 1.6.1 | MIT OR Apache-2.0 | yes | `32e45ad4206f6d2479085147f02bc2ef834ac85886624a23575ae137c8aa8156` |
| `zerocopy` | 0.8.62 | BSD-2-Clause OR Apache-2.0 OR MIT | yes | `86502bf56ac7c77571a32e2647bb2a15894565e981fb2a48d7bde2d91c965a9d` |
| `zerocopy-derive` | 0.8.62 | BSD-2-Clause OR Apache-2.0 OR MIT | yes | `5457206954b06561e2608c7e19cf58b1926586d999c246eebe4502f7e2039d1a` |
