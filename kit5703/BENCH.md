# Getting kit5703 onto a bench Mac

The kit's scripts and RUN.md are on this branch. The lamp-body binary is not committed: it comes from #5703's own CI artifact.

```sh
git clone --depth 1 --branch jarvis/kit-5703 https://github.com/autonomous-ai/lampOS kit5703-src
cd kit5703-src/kit5703
gh run download 42590674657 -R autonomous-ai/lampOS -n lampos-arm64-5caac04a46496e245a723b1fbddc8859d1f18925 -D _artifact
cp _artifact/lamp-body .   # or wherever lamp-body sits in the artifact
shasum -a 256 -c SHA256SUMS   # every line OK; lamp-body must be c2f6b952…
```

The artifact expires on 16 Nov 2026. In RUN.md, `$K` is the folder you copy to the lamp (this folder). Then follow RUN.md's Steps.
