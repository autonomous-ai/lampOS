# Name current-Common host result

The five-path Name preparation for #3014 was checked on tree
`49a8abb5835432fb18eaa4f5e921d7defc891d3e`, based on Common
`00c79072298c583c88ac92a0cad42d2f9155520c`. No runtime Rust changed.
This is a source-specific host receipt, not a published-head or device gate.

Mac and Linux each pass formatting, strict workspace/all-target Clippy,
**112 tests / 0 failed / 0 ignored**, and workspace build. The
[receipt](receipt.json) names the commands and selected counts. All 10,044
tracked files, modes, mtimes and semantic index entries stayed unchanged;
process cleanup is verified, all 17 Linux containers removed, and both
build targets released.

The existing synthetic legacy-source name helper measured first/repeated
entry-to-return **411/90 microseconds on Mac** and **5547/105 microseconds
on Linux**. Linux's first sample misses the under-5-ms host target. No cause
is proven, no threshold changed, and no rerun was made for a better number.
This fixture does not measure v2 Checking continuation, cold storage,
provider, acoustic or device latency.

Train 50 `09f8e7412` arrived during these gates and is not qualified here.
First's no-stacked-PR direction means this source is held for Common-first
landing or a routing decision before another dependent publication. Formal
review, compatible Voice/Eyes pins, actual published-head native CI and
First's known-person/other-person/room/off-camera/privacy lamp checks remain
required. The old 38/40 model result remains historical. The memory row stays
0.45. No lamp action or score increase was made.
