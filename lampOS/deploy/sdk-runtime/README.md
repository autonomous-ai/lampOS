# sdk-runtime

The pure-Python `websockets` that the lamp's one copy of the SDK carries (#6507), so a skill's
script has it whichever Python runs it (`/usr/bin/python3` has none).

- **Version:** websockets 15.0.1 (BSD-3-Clause, `websockets/LICENSE`), the `.py` files of the
  wheel as published on PyPI, with no C speedups (they are optional; the module falls back to
  Python). `websockets.SHA256SUMS` lists every file's SHA-256.
- **Where it goes:** `lamp-update` writes it, with the build's own `sdk/python/src/lampos`, to
  `/usr/local/lib/lampos/sdk-python/` (root's, read-only to a run); a skill's run has that folder
  on `PYTHONPATH`. See `lampOS/docs/tasks.md`.
- **Changing it:** copy the new wheel's `.py` files here, make `websockets.SHA256SUMS` again and
  `crates/update/src/sdk_tree_files.rs` (the test says how).
