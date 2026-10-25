Body Hand baseline binary audit

All 99 candidate/baseline retained executable hashes were verified. For 98 pairs, every byte outside the LC_SYMTAB string table is identical. All load commands (including UUID), symbol records, file-backed sections and layout are identical. Every differing string is an object path whose final opaque .rcgu.o suffix changed, and each is referenced only by N_OSO (0x66) debug-map records. No embedded __DWARF section difference explains these pairs; the concrete difference is debug-map object naming.

This includes all four failed integration targets and all actual normal CLI witnesses. The sole runtime-section/layout difference is lamp-body lib-test lamp_body, where the authored test changed. It must remain a distinct executable; the audit does not assert its semantic equivalence.

The source snapshots differ in the two owned Body test/docs files. All 99 paired actual compiler records have equal full package/target/profile/features and fresh:false. Source mtimes and execution conditions are not equated by binary matching. The first strict suffix-only pass retained a conservative 11/99 classification because it did not allow archive-member closing parentheses; a separate bounded string-table review establishes the other 87 using exact N_OSO references. Original intermediate evidence is retained.

Candidate full run 7,316 passed / 20 failed / 52 ignored; baseline 7,321 passed / 15 failed / 52 ignored. One overlap is a case-name match, not a reviewed phase waiver. Neither result is green. No causal attribution, complete semantic equivalence, or standalone qualification follows. No witnesses were executed, Cargo/tests run, source modified, or cache cleaned. Reports contain hashes/identities, not executable bytes or personal metadata.
