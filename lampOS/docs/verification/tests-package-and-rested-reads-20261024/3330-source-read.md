# #3330 independent source read

Exact head **22e5f48736fe9b0e796b3783edee76522b0185c2** is one six-file continuation commit on #3323 **6c8c52e371353b20fc729ffcdbcccaf839a46321**, itself directly on **9ed44f88c573837c50451c94f4d88f010e8a6721**. This read inspected the new package fixture, inherited process helpers, dedicated workflow and documentation. No Cargo/build/test, local binary execution, worktree mutation or public post was performed. Root owns current native artifacts and execution claims.

**No concrete P1/P2 found in the reviewed source.** This is source clearance only; it does not substitute for exact-head native evidence or parent/current-base integration gates.

## Actual released readers and writers

`package.rs:136–212` executes the direct private pinned executable with exact argv, empty environment, null stdin and private logs. It checks executable digest/size before spawning, owns the returned Child immediately before any transcript assertion (168–173), reaps via the bounded inherited wait, compares bytes afterward, and records actual exit code/signal. It correctly avoids a racy live `/proc` check for a CLI that may already have exited. Failed commands must specifically exit 1, have no signal and emit no success stdout. Install inputs preserve exact bytes and mode (180–191).

Inherited artifact pins match previously downloaded GitHub API metadata: 6408 asset 668607909, 8,807,104 bytes, SHA `2c6bc8f...`; 6498 asset 670759909, 8,786,048 bytes, SHA `87e50012...`. The binaries have distinct digests. Published `skills/src/package.rs` and the entire `main.rs` are byte-identical between ee53765c8 and 846b3d203; the lib.rs delta is presence-duration Watch bookkeeping. Main's manage branch returns before daemon initialization and invokes the released pack/install/list functions. Positive archives and installations are made by those CLI commands, never by today's package implementation or fixture JSON.

Versioned input folders are synthetic, separately created and frozen before execution (215–244, 599–610). Actual A packs/installs version 1.0.0 plus a disabled companion; B lists them and reinstalls A's package with a complete identical-tree check (611–652). B packs/installs frozen version 1.1.0; A lists and reinstalls B's package and must retain B's exact tree (654–678). Both permutations are separate ignored tests. The source schedules 30 fast commands per case: four packs, 20 installs (six positive and 14 refused), six inventories. Native counts must be independently observed, not inferred from this source enumeration.

## Assertion strength and all seven refusals

`tree` (22–60) includes the root, every descendant directory/file, modes and bytes, rejects symlinks/other file types and bounds membership and file sizes. Snapshot copies preserve actual bytes while the receipt records original modes. `installed` (439–465) compares every installed payload byte with the frozen source, asserts exact file/directory modes, and compares the newly written `.package.json` to the archive digest and decoded manifest. It explicitly records `provenance_reader_executed:false`; this is writer-byte verification, not Cloud consumer qualification.

`listed` (348–382) compares the complete inventory map, exact kinds/versions/permissions, disabled companion row and the full enabled-function vector. Duplicate or extra inventory rows fail. The parser checks summary function count/names and permits only incidental trailing summary prose; it does not weaken inventory membership. Every listing also proves the complete installed tree unchanged.

`negatives` (467–541) supplies **seven controls to each released reader in each case**:

1. unsupported package format 2;
2. changed payload with its old digest retained;
3. hidden `.package.json` archive member;
4. unknown TOML field;
5. unsupported skill kind;
6. a speech step without its speak permission;
7. an enabled tool conflicting with the installed disabled companion's reserved tool.

All but the intentional bad-digest control regenerate archive file hashes, so field/kind/permission errors are tested beyond digest validation. Each released command must exit 1 with its specific reason substring and empty stdout; every file/directory member, byte and mode of the live installation must remain identical to the pre-refusal tree (511–530), catching residual staging files. Each group ends with a fresh complete positive inventory. Generic tar/JSON constructs only negative archives and inspects bytes.

`not_loaded` (543–595) deliberately adds a synthetic invalid folder alongside an actual released installation in a separate inventory. It requires the exact not-loaded row, exact surviving positive row and function set, and unchanged files, even though list itself returns zero. This negative state is explicitly disclosed and never presented as a positive released writer.

## Ownership, limits and workflow

`new_case` creates unique retained 0700 case/subdirectories (91–133). Source trees are frozen 0555/0444 and byte/mode checked at the end; positive input archives are retained 0444 and installation inputs checked after each command. Installed 0755/0644 modes are expected within the private case. The fixture rejects links and special files rather than following them. There is one owned child at a time, 15-second command/90-second case bounds, and inherited five-second kill/reap failure cleanup. The offline management path does not launch skill steps/providers/actuators; no blanket process killing or borrowed-child cleanup exists.

The inherited dedicated workflow is byte-identical to parent #3323 and executes the entire ignored Skills release_pair target serially: four unchanged parent routine cases plus two package cases. Shared fixture.rs differs only by the four-line module/export addition; existing routine assertions and process cleanup are unchanged. Ordinary-suite ignored counts cannot qualify these commands. Source/workflow hashes, ancestry, release-source equality and API pin crosschecks are in `source-receipt.json` and `supporting-checks.json`; `continuation.diff` preserves the exact final commit.

Limits are accurately stated: local unsigned-file install only; no released Cloud provenance reader, signed Store/catalog/account/MQTT trust, complete product folder set, updater installation, live reload, actual skill output, device behavior or power-loss durability. Current README native status is intentionally pending; root/author must bind observed command/refusal/tree/cleanup counts to the exact native artifact before a full READ-OK.
