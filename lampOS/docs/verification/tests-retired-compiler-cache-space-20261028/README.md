# Retired compiler-cache capacity

Only 1,902 reconstructible, non-executable Rust compiler archives/metadata files in two inactive Oct24 qualification caches were retired. File kind/header, link count 1 and every recorded stat field were checked; each was SHA256-recorded before deletion, with no open cache files. All 732 current protected/actual paths and inodes, both current caches and the historical debug2 cache were excluded.

Allocated bytes retired: 3,895,992,320. Observed free space: 2,620,268,544 to 6,232,231,936 bytes. All 10,008 retained old-cache files passed exact before/after stat checks. Source, Git, raw evidence, test executables, dynamic libraries and witnesses remain. The private path/hash retirement manifest is retained. Earlier intact-cache claims remain historical; retired compiler intermediates are not a future environment/replay guarantee. No Cargo clean, current-cache change, source timestamp touch, test pass or limit reduction is inferred.
