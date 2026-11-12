# Getting kit5754 onto a bench Mac

This branch holds the kit's text: RUN.md, the scripts, `kit5754.py`, `src/` and SHA256SUMS. The two aarch64 binaries, `lamp-bus` and `kit5754-skip`, are not committed. SHA256SUMS lists every file, the binaries included, and setup checks it on the lamp.

- **The packed kit:** `kit5754.tgz`, sha256 `d66a12deb092d75587d9863819d2d135abca39f3875be038d97c256c4c8a2bc5`. It holds this folder plus both binaries. Ask the kit's builder for it on the issue; check the sha256 before use.
- **`kit5754-skip`** builds from `src/` (std only). `src/Cargo.toml` has no `[[bin]]` and the source isn't at `src/main.rs`, so lay it out first:

  ```sh
  mkdir -p b/src && cp src/Cargo.toml b/ && cp src/kit5754-skip.rs b/src/main.rs
  cd b && cargo zigbuild --release --target aarch64-unknown-linux-gnu
  # target/aarch64-unknown-linux-gnu/release/kit5754-skip
  ```

  A rebuild need not be byte-equal to the sha in SHA256SUMS; the tgz's copy is the one the kit was cleared on.
- **`lamp-bus`** is kit5526's aarch64 cross-build of the bus crate (RUN.md, "The helper binaries"), sha256 `99357d29ddadf97062435246255cf23cb21d24cdb1abf43043fd753dd566bf0c`. The kit records no lampOS commit for it; take it from the tgz.

```sh
shasum -a 256 -c SHA256SUMS   # with both binaries in this folder: every line OK
```

## Plan note: the run moved to c0a4

The first builder moved the run to lamp **c0a4**, Fri 13 Nov, 11:30–13:20 (the meal window), not 52e6. The scripts are unchanged here: editing them would break SHA256SUMS and the tgz's sha.

### To check for c0a4

| file:line | 52e6-specific value |
|---|---|
| RUN.md:1 | title "on lamp-52e6" |
| RUN.md:39 | "The 52e6 holder's GO", and 52e6's shared kit lock |
| RUN.md:40 | 52e6's `/var/lib/lampos-skills-state/off` holds `comfort-break` (a person's choice); lifting it needs that holder's yes. Read c0a4's own off file: if `comfort-break` isn't in it, `LIFT_OFF` isn't needed; if `meal-check` is, setup stops (setup.sh:49) |
| RUN.md:44 | HOLD line "lamp-52e6" |
| RUN.md:49, 53, 54, 173, 175 | the address `<52e6>` for scp/ssh |
| RUN.md:63 | "LIFT_OFF=yes only with the 52e6 holder's yes" |
| RUN.md:100 | "the desk distance the other 52e6 kits use" |
| RUN.md:104, 125, 183 | PLAYING / QUIET lines "lamp-52e6" |
| setup.sh:5 | comment "only with the 52e6 holder's yes" |
| setup.sh:52 | stop message "with the 52e6 holder's yes" |

Not tied to 52e6 but worth checking on c0a4: setup.sh:46 needs lamp-skills run with `LAMP_NUDGES_PRESENT=on`; setup.sh:47–48 need `comfort-break` installed with a `[present]` section; row3.sh:41 runs fast mode only 09:00–19:48, which the 11:30–13:20 slot meets. RUN.md:5 sets `K` to a local scratch folder on the builder's Mac; use the folder you fetched into.
