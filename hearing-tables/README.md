# hearing_replay tables for #828 (numbers only)

For Sixth's `LAMP_VAD_INPUT=linear-gated` rule (#1504) and Seventh's cut-in grid. Made on the office Mac on 18 Oct 2026 by the first builder.

- **Tools:** `lamp-audio replay-aec RUN OUT --linear-output`, then `hearing_replay OUT` (aec_only) and `hearing_replay OUT --signal aec_linear`, built from lamp-voice `0b0ee5b28` (debug).
- **Files:** `<set>-aec_only.tsv` and `<set>-aec_linear.tsv` are each replay's `segment-001…-hearing.tsv`, one row per 10 ms block. `<set>-plays.tsv` gives the played lines: `start_us`, `duration_us`, voice, kind or text, on the lamp's CLOCK_MONOTONIC.
- **Sets:**
  - `1018-mic2-071031`, `1018-mic1-072710`: this morning's stop rehearsals. The story fixture at 75 %, the voice stopped, four lines from `say -v Daniel`. `start_us` is read over SSH just before `say`, so up to ~0.3 s early.
  - **mic1's replay has no render** (`render_quiet` 64 on every block).
  - `1014-*`: 14 Oct's played duplex sessions (`say` voices over the lamp's stories): cut-q, cut-a, cut-b, cut-d, both-a. cut-c and both-noagc were refused by `replay-aec` (cut-c's recording never finished).
- **What's here:** no audio. Only played synthetic voices were recorded, and no person.
