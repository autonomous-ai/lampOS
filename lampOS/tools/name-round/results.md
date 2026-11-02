# Name model round, 1 Nov (#3923)

Everything here was made on the iMac from data already on it. Nothing was downloaded, installed or played, no lamp was reached, and nothing was committed or pushed.

## 1. Reproduction: 17 of 59, matched line for line

The Mac has no onnxruntime, so `onnxmini.py` decodes the ONNX files by hand and runs them in NumPy. `frontend.py` then works out the front end at every 10 ms position at once. It matches the per-chunk interpreter to about 1e-6.

`scorer.py` replays `mic2_audio.jsonl`: 59,186 blocks with no gaps in their sequence numbers. It applies the crate's hearing rule (`said.rs`: the bar, a hearing closes when the score falls or after 0.8 s, then 1 s held off).

- **With one 80 ms grid at phase 2** (two 10 ms frames into the recording), the replay gives all 21 `heard its name` lines and all 44 `under its bars` lines in `name.log`. Every score is within 0.0005 and every time within about 40 ms. That makes **17 of 59** calls, the live figure.
- **So lamp-4ace ran the single 80 ms grid on 31 Oct**, from its own unit. With the product unit's `--step-ms 10`, the shipped head would have heard **29 of 59** on the same audio, plus 14 hearings the journal doesn't have.
- **a01 can't be heard in this recording.** Its play began 150 ms before the recording did, and a score needs 2.03 s of sound first. So 58 calls are really in reach.

## 2. Data (all on this Mac)

| Use | Source |
|---|---|
| calls, clean | ElevenLabs kit, 70 calls in 8 voices (`prompts/eleven/wav`), 16 room copies each. macOS `say` prompts (`prompts/generated`), 436 calls. New `say` renders in the Mac's 27 English voices (`render_sayx.sh`), 16 calls and 20 sound-alike lines per voice. |
| calls, real room | V1 captures of the lamp's microphone (`names/*`, 13–18 Oct): 95 played `say` calls |
| negatives | Kit lines without the name (d09, which mentions the lamp, left out). The 318 `say` lines without the name. 1,395 new `say` sentences in 27 voices (`render_sayy.sh`). V1 talk and near names ("Hey Liam"). The lamp's own TTS. **lamp-4ace mic2:** the stretches between calls in four rP runs (24–25 Oct, its own answers and the room). **Raw mic2 while the lamp tells stories:** half the cut-in runs' snippets (`scenarios/cutin-*-mic2.wav`). |
| validation | The other four rP runs: 40 kit calls on lamp-4ace's mic2, plus 23 min between them. The epoch is picked here. |
| test | The 59 live calls and the 12.4 min between them. **Nothing of the live recording was trained on.** The other half of the cut-in snippets (12.7 min of the lamp's own voice). The kit lines without the name, through simulated rooms. |

**Room copies** (`augment.py`) use mic2's response, measured from the four rP training runs against the clean clips (`channel_mag.py`):

- the response itself: a high-pass near 120 Hz, a notch near 500 Hz, −13 dB at 1 kHz, −18 dB at 3 kHz, −27 dB at 4 kHz and −32 dB above 5 kHz. It is applied with a random ±4 dB ripple and tilt, or a generic band-limit in a quarter of copies;
- reverb with RT60 from 0.15 to 0.6 s;
- the onset lost in 30% of copies;
- a call level from −28 to −13 dBFS;
- room sound from the rP training gaps under it;
- the lamp's own voice under it in 30% of copies.

**Training** (`train.py`) uses the shipped head's shape (16×96 → 96 → 48 → 1) and multiple-instance learning: each call copy is a bag of windows, and the best one is its positive. Hard negatives are up-weighted.

## 3. Results

**Test setups.** Live: the 59 calls, and false calls per hour over the 12.4 min between calls, where 1 false call is 4.8/h. "lamp-4ace setup" means one 80 ms grid at the phase it ran; "product" means `--step-ms 10`. Own voice: false calls per hour in the 12.7 min of raw mic2 while the lamp talks.

| head | live, lamp-4ace setup, bar 0.8 | 0.9 | 0.95 | live, product, 0.9 | own voice, 0.9 | validation, 0.9 |
|---|---|---|---|---|---|---|
| shipped | 25, 34/h | **17, 19/h** | 16, 5/h | 29, 29/h | 11/h | 20.6/40, 8.4/h |
| **G** (all data, positives weighted 0.5) | 55, 5/h | **54, 0/h** | 52, 0/h | **56, 0/h** | 14/h | 40/40, 0/h |
| F (no new `say` calls) | 53, 0/h | 53, 0/h | 50, 0/h | 54, 0/h | 12/h | 39.8/40, 0/h |
| C (no story negatives) | 55, 0/h | 55, 0/h | 52, 0/h | 56, 0/h | **84/h** | 40/40, 1.0/h |
| A (kit, `say`, V1, rP only) | 55, 5/h | 54, 0/h | 54, 0/h | 56, 5/h | 36/h | 40/40, 0.6/h |

**Voices never trained on.** These two-fold results are the honest figure for a new voice. Each fold leaves out four kit voices (calls and lines); the hits count only the left-out voices' live calls.

- G's recipe, lamp-4ace setup: **46 of 59 at 0.9, 50 at 0.8**, at 2.4/h live; the product setup gives 50 at 0.9.
- Own voice for those fold heads: 16–19/h.
- Kit lines without the name, through simulated rooms: **2.6% of copies fire at 0.9, against 4.7% for the shipped head.**
- d05 ("The white one."): 6% of copies, against 25% for the shipped head.

**Per voice, G in the lamp-4ace setup at 0.9:**

| | adam | alice | antoni | charlotte | chris | george | jessica | matilda |
|---|---|---|---|---|---|---|---|---|
| G | 13/15 | 15/15 | 4/4 | 4/4 | 1/1 | 7/8 | 5/5 | 5/7 |
| shipped | 4/15 | 2/15 | 3/4 | 0/4 | 1/1 | 2/8 | 2/5 | 3/7 |

**What G misses:** a01, p01, p11, o05 and o06. In the product setup it misses only a01, p01 and p11.

**A second seed of G's recipe** (`G_s1`) gives 54 of 59 at 0.9 with 1 false call (4.8/h), and 57 in the product setup at 4.8/h. Its own-voice rate is 15/h.

**Lessons from the other runs:**

- Training for room calls alone makes a head fire on the lamp's own voice: A at 36/h, C at 84/h.
- The raw mic2 story snippets and more varied `say` negatives bring it back to the shipped head's level (F and G).
- Training on the clean kit clips of the test lines adds about 8 calls over voices that were left out.

## Files

- **Best candidate:** `candidates/name-G.onnx`, SHA-256 `4b036bff92bb0ed9509c1a96ce4dfdd5b0455e947bf8f0f2dc5b94de11265327`. It is the shipped `name.onnx` with its six weight tensors replaced in place (same names, shapes, graph and opset), so `lamp-name` loads it the same way.
- The other candidates are in `work/cand_*.onnx` and their scores in `work/res_*.json`.
- Scripts are at the top level. `report.py [live80|avg80|step10]` prints the full tables, including the eight-phase average.
- The `work/` sets, about 450 MB, can be deleted. The `say` renders can be made again with `render_say*.sh`.

## What an on-lamp check needs

1. Put `name-G.onnx` in as `name.onnx` on lamp-4ace, keeping the shipped file for rollback.
2. Run `namerec.sh` again: the 59 calls through mic2. The offline replay predicts about 54.
3. **False calls, measured live:**
   - the busy sets' desk lines, including d05;
   - long stories with cut-ins (its own voice);
   - an hour of the room with nobody calling;
   - all at the product's `--step-ms 10` as well as 80 ms.
4. **A real person calling across the room**, ideally a few. Every call here is TTS.
5. The owner's yes before a model file is committed, as Thirteen noted on #3923.
