# Read-only capture of head-pair inputs

Part of #3689. This prerequisite remains unwired: it performs no update, helper
execution, authentication, admission, path authorization, restore or device
action. Existing ordinary head-pair wrappers, policy and call order are unchanged.

`head_touch::capture::collect(root, evaluator)` supplies the existing `HeadInputs`
interface to an evaluator and returns its output plus `FrozenInputs` only after
every captured fact has been freshly compared. Any provider error poisons
collection, including one the evaluator catches. Policy and I/O errors at this
boundary use fixed diagnostics. Directory enumeration is collected eagerly and
bounded before returning its owned iterator; iterator errors therefore cannot
leave an apparently usable partial membership. The ordinary adapter remains lazy.

The callback must run the complete intended policy and bound/authenticate its
carried in-memory inputs. This API cannot know that an arbitrary callback ran a
whole plan. The caller must bind candidate units, signed settings, raw ownership
observations and projected outputs to the same transaction. `project_with` and
`verify_with` use the frozen provider without filesystem fallback. Standalone
`active_pins`, install preflight and other ordinary callers remain ordinary;
this piece does not freeze a complete round or systemd's effective configuration.

`FrozenInputs::revalidate(root)` provides a separate read-only comparison for a
future immediately-before-write boundary. It compares root identity, every
traversed directory and link, regular bytes/metadata, negative facts, complete
directory membership and actual null-device witness. A failed comparison grants
nothing. Stable root ancestry/ownership and exclusion of concurrent writers are
caller preconditions throughout collection, revalidation and use. These checks
are observations rather than an atomic snapshot or a kernel/storage deadline.

## Compatibility and descriptor handling

The root must be an absolute path whose final component opens as a no-follow
directory. Traversal uses held directory descriptors and no-follow metadata.
Links are resolved explicitly with bounded `readlinkat_raw`, never by an
unbounded ordinary reader or `canonicalize` followed by an unanchored open.
Regular files compare the no-follow pre-stat, opened descriptor, named leaf and
post-read descriptor identity; a cap-plus-one read detects growth. Selected
metadata includes device/inode, type/mode, owner/group, link count, length and
mtime/ctime with nanoseconds; atime is excluded because reads can advance it.
ACLs/xattrs are not captured. Pre-stat and open remain separate system calls:
writer exclusion is required to prevent replacement between them.

The regular-read primitive checks metadata and length around its read; it does
not prove that an equal-length concurrent rewrite advances a timestamp. The
collector's final fresh observation also compares the complete file contents.
Linux can give multiple changes the same coarse timestamp, with finer updates
depending on filesystem support and observation; see the
[kernel's multigrain timestamp documentation](https://docs.kernel.org/filesystems/multigrain-ts.html).
Writer exclusion remains required even when both comparisons agree.

Relative regular-file and directory aliases are supported, including merged-/usr
layouts. Absolute link targets follow actual host/root containment: they must
lie beneath the authorized absolute read-root spelling and are then traversed
from its held descriptor. With deployment root `/`, absolute host paths remain
in that root. A fixture's absolute target inside its authorized root is supported;
an absolute `/usr/lib` link is not silently reinterpreted as fixture-root `/usr/lib`.
Root escape, loops, unsupported paths and bound overflow refuse collection.
Requested keys must use absolute lexical paths without repeated separators,
`.`/`..` or a trailing separator (except `/`). This conservative collector-only
refusal prevents `Path` equality from collapsing directory-requiring spellings
into an already-cached regular file. Link targets are parsed separately and
preserve the kernel's directory requirement for trailing `/` or `/.`.

The sole outside-root exception is a terminal link whose exact target is
`/dev/null`. Its actual no-follow metadata must identify a character device;
the witness is retained and rechecked. An in-root character leaf must match that
actual null device's rdev. No device contents are read; known FIFOs, sockets and
other devices are refused before any attempted content read. A null spelling
alone is insufficient. A null mask and an empty regular file both give the policy
empty text but remain different typed capture facts.

`MissingLeaf` and `MissingAncestor` retain negative facts and the existing prefix;
an unrequested key always errors. Missing directories and present-empty directories
are distinct. Every directory name is retained in original enumeration order,
including ignored non-`.conf` names; revalidation compares the complete membership
as an unordered set, while replay retains the original captured order. This is
not a claim that an arbitrary order-sensitive callback would produce the same
output from the current filesystem enumeration order. Raw non-UTF8 names are
supported without lossy conversion. Regular file contents must be UTF-8. Regular
hard links are supported, with observed link count and metadata bound to the
capture; this does not prove exclusive control of aliases outside the read root.

Repeated lookups replay the same fact. Body calibration remains conditional on
the policy's actual requests. Custom active pins, the omitted no-op product-pins
read, local overrides and carried base units preserve their existing semantics.
No hostname, binary, current-link, bus-file or controls-mute-file read is added.
Debug exposes counts only, never stored paths, link targets or calibration text.

## Collector-only limits

These are conservative admission limits, with no deployment-coverage claim;
they do not change the ordinary reader's accepted sizes or behavior:

| Resource | Limit |
| --- | --- |
| One regular dependency | 1 MiB |
| Aggregate unique requested regular contents | 4 MiB |
| Unique text/directory keys combined | 256 |
| Inspected names in one directory | 128 |
| Aggregate raw directory-name bytes | 64 KiB |
| Requested path and one link target | 4,096 raw bytes |
| Requested absolute components and cumulative expanded relative components | 64 each |
| Followed links per lookup | 16 |
| Cumulative lookup/resolution/enumeration work units | 16,384 |

Ignored names consume the name budgets. Link expansions consume cumulative
components/work; cached requests still consume work. The fresh revalidation has
the same bounded budget. Bound failure or a partial enumeration releases no
successful frozen result or evaluator output. Eager collector admission can fail
before the ordinary lazy evaluator would reject an earlier entry; its ordinary
error ordering is unchanged.

The 4,096-byte path bound is per requested path and per raw link target, not a
4,096-byte expanded-path claim. At most sixteen followed targets plus the
original input bound cumulative raw resolution text by `17 * 4,096` bytes;
the cumulative expanded-component and work bounds still apply. Descriptors
retain traversal anchoring rather than assembling an unbounded resolved path.

## Validation boundary

Private-filesystem controls are authored for real evaluator replay without a
filesystem, links/absolute containment/loops, null versus other devices/FIFO/socket,
negative and directory membership changes, root/ancestor substitution, same-
descriptor races, hard links, UTF-8, poisoned errors, calibration and budgets.
The original uncommitted source on `5d866fd9c/992303fc8` passed formatting,
strict Update all-target Clippy, no-run compilation and discovery: 534 cases,
528 eligible and six ignored, including all 21 new controls. Its first full
preferred Nextest run finished **520 passed / 8 failed / 6 ignored**, no selected
unrun cases; the new controls were **19 passed / 2 failed**. Six failure sites
reported `EPERM` before the intended operation: four existing local TCP fixtures,
the new Unix-socket fixture and the new raw-name creation. Two unchanged World
bus controls returned empty results; their cause remains unestablished. The run
used the default sandbox; no assertion, timeout or skip was weakened and no test
was retried. Its complete failed source and executables remain retained.

The intermediate `e6be1d779/f9926922d` carry was source-only. The later source
on Main `3c9838fea6`, with explicit fixture permissions, passed preparation and
ran its first full gate: **530 passed / 1 failed / 6 ignored**, no selected
unrun cases; the new controls were **20 passed / 1 failed**. All source,
configuration and actual/preserved executable guards closed. The sole failure
was `EILSEQ` (92) while creating the raw-byte filename, before the collector
was invoked. This observed Mac filesystem rejected the fixture; the result
does not demonstrate a collector error. That failed source and its original
permission request, result and executables remain retained. The two World
controls passed in this different source/environment, which does not establish
the cause of their old failures or waive them.

The next source on Main `1d97a2c6d0` changes only that control and its
documentation. On every platform the control checks exact raw `OsString`
closed-replay/entries bytes with no filesystem fallback, and refuses actual
invalid-UTF8 file contents at an ordinary filename, including caught-error
poisoning. Its synthetic closed-name fixture is not an admitted physical
snapshot. The physical bad-byte filename create/capture/revalidate subsection
is explicitly `cfg(target_os = "linux")`, with its assertions retained and no
catch or ignore of `EILSEQ`. The whole test remains enabled on Mac. Physical
non-UTF8 filename capture on the observed Mac filesystem is **unverified**;
portable closed replay is not substituted for that filesystem proof. Native
Linux ARM64 CI must execute the physical subsection before the PR is ready.

This approved fixture-only change leaves production APIs, raw-name handling,
ordinary-reader policy and all other controls unchanged. After carrying its
exact source onto Main `0d1d1f2b94`, the first corrected full Mac gate passed:
**531 unique PASS / 0 bad or selected unrun / 6 ignored**, including all 21
collector, four seam, 18 foundation, one null-mask and eight receipt controls
once (52 deduplicated controls). Formatting, strict Update all-target Clippy,
no-run compilation and discovery passed. The preferred Nextest 0.9.150 run used
profile `pr`, `all()`, four processes, zero retries and no fail-fast, with explicit
fixture permissions. Doctests had zero cases; both custom Mac skip checks each
reported zero passes/one ignored and overlap the six existing ignores. All
7,499 source/configuration/index/mode guards and ten actual/preserved executable
hashes closed, and the native controller exited 0. Neither earlier failed
result is transferred, waived or treated as diagnosed by this pass.

This validation paragraph was finalized after that frozen-source run. Its
documentation-only delta changes no Rust/test bytes and grants no new runtime
credit. Physical non-UTF8 filename capture on Mac remains unverified; the Linux
subsection still requires its native ARM64 execution before Ready.
No ARM64, installed-lamp, performance, power-cut, bootstrap or restoration
acceptance is claimed by this prerequisite.

Original ARM64 run 40543995512 on `6523fa886b/0d1d1f2b94` failed: **2,508
passed / 1 failed / 53 skipped**, with 5,294 selected cases unrun after fail-fast.
The physical Linux raw-name control passed, which does not make the run green.
The same-descriptor control failed its shared refusal assertion; the two loop
branches were unlabeled, and its Result and metadata were not recorded. The
exact failed subcase and cause therefore remain unestablished. The timestamp limitation above is a general contract limit,
not a diagnosis of that instance. The complete failed source, run and log remain
retained; no original CI rerun is used to replace that result.

The subsequent fixture-only correction makes the primitive's post-read mutation
change length, preserving its pre-open no-follow substitution case. The full
collector control now checks an equal-length content mutation and a private
modeled-equal-metadata case, requiring the complete-byte comparison to refuse it.
Only these two control bodies and this validation contract change; production
Rust and the other 19 collector control bodies remain unchanged. No assertion,
timeout, retry or skip is weakened. The corrected source requires its own
current scoped gate and new native CI before Ready; no new runtime success is
claimed here. Physical raw-name capture on Mac remains unverified.
