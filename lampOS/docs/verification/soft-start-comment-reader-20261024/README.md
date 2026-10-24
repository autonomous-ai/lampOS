# Soft-start comment precision read

Exact a6e780f86 on fc31e0123; READ-OK, no P1/P2/P3 demonstrated. Only six
added/deleted lines change, all doc comments in coordinator.rs. The only
changed path is verified; all non-doc-comment lines match the base, and
pinned Rust 1.96 file rustfmt plus workspace cargo fmt pass without changes.
No implementation/test, Clippy, Cargo build, behavioral or physical experiment
is claimed or required for this comment-only read.

The comment now limits the exclusion to additional soft-start lead as judged
by software evidence. It explicitly preserves the ordinary 300 ms prefix,
which may still hold the lamp sound. This matches soft_begin's last-unsilent
floor and min(usual), and the already-qualified duplex documentation. It
closes the source-comment P3 from Second's 3411 read.

First's newer exact-build played report on 3411 compares both switches off
against both on. Its reported first-word outcome is combined-arm evidence,
not a new independent single-switch result or raw-audio measurement by this
reader. This source-comment correction is not product activation, acoustic
tail certification or resolution of train 104's separate readiness incident.
