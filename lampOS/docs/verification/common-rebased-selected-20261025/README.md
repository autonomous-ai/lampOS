# Common rebased host results: selected receipt

Ready for lamp-voice: no

Tested source f5d92567ef89f7a7aeebee1d591038bb586c7b20, tree d454da643e6b9809a36c1cfc87ea5c8b03df110f, parent 4270083a2e32af70397be5c705ec42af046f7a33. This is the local unpushed restack of #3064. Both hosts ran formatting, strict workspace/all-target Clippy, workspace build, and full Eyes, Voice, Update, Skills and Memory tests with offline locked dependencies and serial pretty test output. The separately reviewed JSON summaries contain selected counts, hashes and Booleans only; raw packets stay private.

| Host | Eyes | Voice | Update | Skills | Memory | Gate |
| --- | --- | --- | --- | --- | --- | --- |
| Mac | 357/0/5 | 4127/12/21 | 386/0/6 | 516/0/9 | 324/0/8 | FAILED 101 |
| Linux | 360/0/5 | 4200/0/24 | 388/0/4 | 519/0/9 | 324/0/8 | 0 |

Counts are passed/failed/ignored. All eight stages were observed on each host; Mac failed only full Voice. Five Mac and six Linux required controls passed once. Both hosts freshly compiled all 46 local workspace members after scoped invalidation, with source guards and bounded cleanup/release verified. Those controls and Linux success do not waive the twelve Mac failures.

Mac terminal SHA-256: 18e58b7649dac311a2aa0df2cf4cc6f841684317944f1bfd94865bfd850e943e. Linux terminal: 9da4968cd057b56ac5e66d359388a7a838dda289778ab190a7da7ce1c0f30908. Private packet manifests: Mac 6ab17d222da99b38843bef95d5613a2ca8a131258a6a46e7ee6eefe5445860ac; Linux 01fe6285d81a1f8342329b4b3b1ebd81a8d2db8712d4ea7665427b470b467659.

Next is one matched clean-parent full Voice comparison and the first three prescribed isolated observations for each extra failure. Every original failure remains; no cause, retry-pass, timing waiver, formal READ-OK, current native acceptance, activation or physical qualification is claimed. The current executable was privately preserved after termination and before cleaning; its initial failed-execution hash was not observed. Parent composition, current-head native checks and full privacy review remain open. No lamp action occurred.
