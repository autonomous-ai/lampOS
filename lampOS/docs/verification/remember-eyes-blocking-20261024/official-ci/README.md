# Original native gate and ready disposition

Exact PR #3412 head fa82c279e7b056ff45dd1af3fd77d3a3ea7adaa3 on current
lamp-voice bcb540a08. Original ARM64 40130968164 and Linux 40130968163
complete successfully. Actual preview a69ec96fd3ed7cae06cbed165d1e898f94570caa
has the same whole tree as authored fa82: 8d8c615acf16f1dd865f64e957012220b0bb13c8.

ARM: 7,409 PASS plus one passing LEAK, 53 skipped; all 7,410 selected cases
terminal, zero failed/unrun. All sixteen remember-speaker module cases and
the retained Mac idle-pool failure case bind literal native PASS rows.
The primary rendering has 34 exactly identical echoed rows, collapsed only
by identical status/index/target. The LEAK is the existing same_voice
journal case and is not silently represented as a normal PASS.
Actual Linux pinned workspace formatting and every-target strict Clippy
steps pass. ARM tests, doc tests and own-harness steps pass. No rerun.
Final jobs metadata is captured separately; primary hashes describe gh
renderings, not raw Actions ZIPs. Full primary output remains available
through the linked immutable public run IDs; selected lines and all terminal
rows are retained here.

Reviewer-three READ-OK 6389355728 and Reviewer-two source/local read and
accepted three-alone disposition 6389410236 complete. Their own extra
observations keep their own scope. Original Mac full library remains
FAILED 3,742/1/15, distinct-target total 4,031/1/21, with three isolated
passes accepted under the rule. It is never relabelled a full pass.
Executed pre-rebase source is now retrievable on second/remember-eyes-mac-20261024.
No physical behavior, provider, runtime latency, product activation, local
Linux ARM build/test or device action is claimed. Train owns any merge.
