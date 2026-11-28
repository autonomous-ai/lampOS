# The lamp-channel retrain (#8907, 28-29 Nov 2026): what is here

Backend's "Hey Lamp" clips were played from Mac 2's speaker through the room into lamp-dbda's mic2, the
bus topic lamp-name hears. These folders cut those recordings into clips, score the name models on
them as lamp-name does, and train on them in rounds. Nothing here was taken from the lamp by this
work (the recordings were copied in by the first builder), and nothing is deleted.

| Folder | What |
|---|---|
| `tools/` | `lampcut.py` (the cutter), `lampeval.py` (the lamp-channel and October scoring, and its tables), `mergeeval.py` (a model scored alone beside heylamp-1's three in one heylamp.py report), `livecheck.py` (the offline scoring against lamp-name's own hearings, and the A/B replay), `octcut.py` (October's recordings), `run_py.py` (runs a training script under `python -I`), `steps.sh` (every step, by hand) |
| `full/` | Session 1's cut: `track.wav`, `decode.json`, `blocks.tsv`, `lamp-manifest.tsv`, `lamp-clips/`, `floors/` |
| `s2-*/` | Session 2's cuts, one folder a round |
| `eval/` | The scores (JSON, one file a part) and the tables (`report-lamp-channel*.md`) |
| `october-2/` | October's lamp recordings cut and labelled (test only: macOS voices, see below) |
| `livecheck/` | The offline scoring of this morning's A/B recording against lamp-name's own `name/heard` |
| `piper/` | Piper's LibriTTS voices made for training (its README has the licence and the split) |
| `oww-features/` | Why openWakeWord's precomputed features are not used (non-commercial licence) |
| `dev-partial-*/`, `logs/` | The cutter's development runs on the 07:36 snapshot, and every step's log |

The training code is the local branch `first/heylamp-lamp-channel` (worktree
`/Users/autonomous/code/autonomous-os/.claude/worktrees/first-heylamp-lamp-channel`, whose
`lampOS/.cache` is the first builder's checkout's, linked). The sets and models are in that cache:
`.cache/heylamp/sets/<SET>/`.

## Test only, never trained on

- The 800 lamp-recorded `use=test` clips (14 held-out ElevenLabs and Gemini voices) and the 200
  `test-extra` (OpenAI's) from session 1: the fixed test of every round.
- October's recordings and this morning's A/B `mac-*.aiff`: macOS system voices. The macOS licence
  (Tahoe 26, §2.F) allows them for personal, non-commercial use only.
- The 113 held-out LibriTTS speakers, Cori and LJ Speech (the Piper test slice).

## Rounds

Every step is `zsh tools/steps.sh STEP` with its variables (the script's head lists them). Round 1
(session 1, 28 Nov ~13:00):

    zsh tools/steps.sh cut                                  # session 1 -> full/
    zsh tools/steps.sh eval-lamp                            # the three baselines on the lamp test
    SET=heylamp-3 zsh tools/steps.sh lamp-set               # heylamp-1 + session 1's 2,400 train cuts
    SET=heylamp-3 zsh tools/steps.sh lamp-inputs
    SET=heylamp-3 zsh tools/steps.sh train
    SET=heylamp-3 zsh tools/steps.sh eval-model             # lamp test, lamp and clean
    SET=heylamp-3 zsh tools/steps.sh eval-clean-set; SET=heylamp-3 zsh tools/steps.sh merge-clean

Round 2 (~20:00) and round 3 (Sunday afternoon) cut session 2, which runs in segments (a, b, ...: it
pauses for lamp tests), each segment into a folder of its own (`plays-2<SEG>.tsv`, sealed to
`recording-2<SEG>/`, snapshots in `recording-2<SEG>-partial/`; `index-2.tsv` and `blocks-2/` serve every
segment, a block's number the same in all), and train a new set on every cut so far. Session 1's test
clips stay the test:

    L=$HOME/lamp-voice-data/heylamp-8907/lamp-channel-1128
    CUTS=$L/work/s2a REC=$L/recording-2a CUT_ARGS="--blocks-dir blocks-2 --index index-2.tsv --plays plays-2a.tsv --no-track-wav" \
      zsh tools/steps.sh cut                     # one per segment (a snapshot: REC=$L/recording-2a-partial)
    SET=heylamp-4 MANIFESTS="$L/work/full/lamp-manifest.tsv $L/work/s2a/lamp-manifest.tsv" zsh tools/steps.sh lamp-set
    SET=heylamp-4 zsh tools/steps.sh lamp-inputs; SET=heylamp-4 zsh tools/steps.sh add-piper   # with the Piper voices
    SET=heylamp-4 zsh tools/steps.sh train
    SET=heylamp-4 zsh tools/steps.sh eval-model; SET=heylamp-4 zsh tools/steps.sh eval-october
    SET=heylamp-4 zsh tools/steps.sh eval-clean-set; SET=heylamp-4 zsh tools/steps.sh merge-clean
    SUMMARY_SETS="heylamp-3 heylamp-3p heylamp-4" zsh tools/steps.sh summary

A segment's manifest lists every clip of index-2.tsv; those its plays do not hold are marked "not
played yet" and lamp-set passes them by, so the manifests of all segments can be given together.

A larger classifier on the same inputs: `SET=... SHAPE=flat HIDDEN=96 MODEL=heylamp-flat96 zsh
tools/steps.sh train` (615,699 bytes, Flatten-Gemm-Relu-Gemm-Relu-Gemm-Sigmoid: the installed
model's kind of graph).
