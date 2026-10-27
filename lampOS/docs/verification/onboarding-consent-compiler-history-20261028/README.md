# Consent admission: component checkpoint and compiler history

Ready: no.

The Eyes-only checkpoint on staged tree `b95ddc557662b48e5c5d75b53ce444e168e0593b`, based on `f903659bba298cdee290ee4285038a8f46af930b`, passed formatting, strict Eyes all-target Clippy, and the entire `remember_speaker` integration target: **11 unique passes, zero failures or ignored cases**. Four controls are new and seven are retained. Actual native completion was 99292/0. Commands, environment, source and closure facts are in [eyes-checkpoint.json](eyes-checkpoint.json); identities are in [eyes-11-ledger.json](eyes-11-ledger.json).

This qualifies that Eyes protocol/collector/store component only. It does not qualify the full caller-bound Voice consent implementation, activation, any later source tree, a physical lamp, or installed matching Voice/Eyes binaries.

Three subsequent full-source compiler attempts stopped at strict Clippy, each with **zero test bodies executed**. Formatting and metadata passed; workspace build, runtime discovery, runtime tests and docs were unrun:

| Epoch | Staged tree | Actual native result | Primary diagnostics |
| --- | --- | --- | --- |
| R1 | `9ad3276e27dfe3a5658e16eaee54eb8fc828d692` | 76111 / 101 | E0308 queue pattern type mismatch |
| R2 | `5da1fbca532e5594cfdf608c9f3a295f1873e064` | 82193 / 101 | `clippy::nonminimal_bool` |
| R3 | `0c7f5e858710631bc6e7b15f7513099b746dddcc` | 9586 / 101 | Unresolved playback import and non-exhaustive test matches; all emitted strict diagnostics retained in the JSON |

[compiler-failures.json](compiler-failures.json) records exact commands, actual results and diagnostic locations. Each epoch closed its owned process groups and verified source stability plus the retained 7,893 files and 276 archives. These failures remain failed. Corrections and the next source require their own gate; no earlier component or fixture result transfers automatically. The full Voice consent runtime is unrun at this checkpoint. The policy, independent review, current-base, privacy/composition and physical conditions remain separate.
