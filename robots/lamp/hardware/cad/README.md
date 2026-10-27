# CAD

Mechanical source files for Lamp.

Large CAD binaries (`*.stp`, `*.step`, `*.stl`, `*.f3d`, `*.f3z`) are tracked
via **Git LFS**. See the repo-root `.gitattributes` for the filter rules.

## Files

| Folder | Contents |
|--------|----------|
| `step/` | 17 STEP parts — `base`, `base-cap`, `neck`, `arm-1-part1/2`, `arm-2-part1/2`, `swivel-part-part1/2/3`, `head-part1/2/3`, `light-cover`, `cap-servo`, `button`, and `lamp` (the full assembly, ~93 MB) |
| `stl/` | the same 17 parts as STL |
| `f3d/` | `Lamp_09102026.f3d`, the Fusion 360 design of 9 Oct 2026 (~37 MB) |

The servo carriers are CNC aluminium; the wood trim is decorative CNC; everything else prints.

## Uploading a new revision

1. Install Git LFS once per machine: `brew install git-lfs && git lfs install`.
2. Drop the file in `hardware/cad/`.
3. Commit and push as normal:

   ```bash
   git add hardware/cad/step/<part>.stp hardware/cad/stl/<part>.stl
   git commit -m "cad: bump <part>"
   git push
   ```

   Git LFS handles the upload to GitHub's LFS storage automatically.
4. Update the table above (file + date) and commit `hardware/cad/README.md`.

## Cloning

A fresh clone needs LFS too. After `git clone`, run:

```bash
git lfs install
git lfs pull
```

to fetch the actual binaries (otherwise the working tree gets LFS pointer
stubs instead of the real files).

## Changelog

- **2026-10-27** — added the Fusion 360 design `f3d/Lamp_09102026.f3d` (9 Oct 2026). The STEP, STL and GLB files are unchanged.

- **v3** (2026-05-20) — initial STEP export (`lamp-v3.stp`, since replaced by the per-part files above; see `../cad-archive-v0/`).
