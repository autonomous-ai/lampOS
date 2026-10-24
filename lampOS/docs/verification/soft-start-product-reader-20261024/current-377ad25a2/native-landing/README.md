# Corrected product source and train 106: existing native results

Second independently collected the original completed source and train runs; no CI rerun or local execution was started. Source `377ad25a2`, its actual preview `d16a4defe`, train `e79ddfcf7`, its actual preview `ab5dc8fb9`, and landed `da57072b2` have the same complete tree, `90783fa6e059c59367a1a10f246ca83fc9f13824`.

Each ARM run reports 11,262 PASS and one LEAK: all 11,263 selected cases terminal, 106 skipped, zero failed or unrun. All 122 selected regression/unit/census cases are literal PASS. The parser collapsed only 35 identical echoed terminal lines; the LEAK remains explicitly recorded. Both original Linux formatting/strict Clippy runs succeeded.

Source ARM completed at 05:54:55 +07; train ARM completed at 06:01:22 +07; merge was at 06:01:32 +07. Both native gates therefore completed before this merge. Metadata uses original UTC fields; this paragraph uses team time.

Primary log hashes identify `gh run view --log` renderings, not raw Actions ZIP bytes. Selected lines, full-run hashes, original final-job times and complete-tree bindings are retained. Compiler/profile limitations of the preceding 18-case Mac gate remain in the parent packet; this collection adds no local execution. No acoustic or physical-device measurement, deployment approval, or resolution of train 104's separate readiness incident follows.
