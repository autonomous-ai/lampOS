# Sentinel 0c1d on 90ca: Mac Reader module qualification

Current [#3543](https://github.com/autonomous-ai/lampOS/pull/3543) head `0c1d8bc145c5e6c117f5b2666cea9c1133bf8b29`, based on `90ca725c514bbb1e4b36318d7b3ccbb14dfd05f1`, passed all **four actual Mac Reader module cases, 4/0/0 once**, serially. Runtime was 1.175 s; the controller/wrapper exited zero in 568.222 s. These are command boundaries, without a performance claim.

The freshly compiled Voice library's all/ignored inventories contain **3,826** cases. The complete actual `session_reader::tests::` module contains exactly four, all ordinary; **3,822 were filtered out** by the explicit module scope. This is not a full Voice runtime. Every selected identity and the final summary match its inventory. The changed Linux case `session_reader::tests::linux::replaced_record_never_changes_an_outside_sentinel` was not compiled or run on Mac; no Linux or isolated-root DAC success is inferred.

Formatting, strict all-targets Clippy over the actual twelve-package Voice reverse closure and workspace build passed. Supported clean invalidated only Voice, and only invalidated Voice strict artifacts were required `fresh:false`. Workspace build preceded library test compilation. The actual module artifact, source, profile/features and independent frozen copy are bound; all actual module hash, bytes, mode, mtime, device and inode guards remain strict.

All **10,674** tracked source files, index, HEAD and clean status remained exact. The canonical alias matches the preserved official 0c1d source except for its branch label. A separate postgate full-owner/tree comparison also passed before publication. The base transition from completed 5ac/a3b consists only of Main's two Expression fixture/documentation paths; this gate establishes a fresh result for the new 0c1d/90ca source.

All **236 inherited plus one new = 237** independent frozen witnesses retained strict hash/bytes/mode/mtime/device/inode equality. All eleven owned stages exited zero and closed their owned groups, with no unresolved process, retry, runtime reuse, deadline stop or disk-reserve stop. The lease was released. No alias-inode exception was applied to the actual module or any frozen witness.

Fresh exact-head ARM64/Linux CI is a separate audit. Earlier head checks keep their original scopes. This host packet qualifies neither full Voice runtime nor any future Restart change, device behavior or physical latency.

[audit.json](audit.json) exports all four identities, inventory/filter counts, module binding, source/official-alias equality, 237 strict witness guards, stage exits and retained-receipt hashes. It was prepared from completed evidence without compiler, current-source or target access.
